//! The options window: sound volumes, the display mode (windowed or fullscreen), the frame budget,
//! the graphics (with presets, [`super::gfxpresets`]) and the camera, in four tabs. Opened from the
//! pick screen and the pause screen, and kept across restarts.
//!
//! Nothing here keeps its own settings. The sliders write the engine's CVars (`MasterVolume`,
//! `MusicVolume`, `SoundVolume`, `AmbienceVolume`, `MasterSoundEffects`), whose observers move
//! [`crate::sound::SoundConfig`] live, and the display buttons write `gxWindow` and `gxMaximize`
//! and commit the latch, which [`crate::video`] applies to the window. The registry saves
//! `config.toml` on its own, so a launcher's `--windowed` (`WOW_WIN`) and the file keep working;
//! while such a launch flag holds, the display row is shown but off, and nothing is saved over it.
//!
//! "Fullscreen" is the borderless window over the monitor, the only fullscreen benilla has
//! ([`crate::video`]: macOS has no exclusive mode and `WindowMode::Fullscreen` panics on a live
//! change). The browser build cannot change the window, so it has no display row.
//!
//! The camera part (mouse sensitivity, inverted pitch, shake) is not a CVar: it writes
//! [`Prefs`], which [`super::hero`] reads, and saves `survivors-prefs.toml` when a value is
//! final (a toggle pressed, a slider let go).
//!
//! The graphics tab writes the engine's CVars (`farclip`, `worldDetail`, `spellEffectLevel`,
//! `weatherDensity`, `ffxGlow`, `ffxDeath`, `gxMultisample`, `anisotropic`) and the picture
//! settings in [`Prefs`] (anti-aliasing, the grade and its sharpness, hit flashes, death fades).
//! `gxMultisample` is latched like `gxWindow`, so its button commits the latch; `anisotropic`
//! takes effect at the next start only, and the tab says so. A preset row sets all of them at once
//! and shows "Custom" while no preset matches.
//!
//! The performance part (native only, on the video tab) is two stepped sliders and two toggles
//! ([`super::framerate`]): the frame-rate cap and the dynamic scale and the readout are [`Prefs`],
//! the render scale is the engine's `renderScale` CVar, which the registry saves itself.

use bevy::prelude::*;
use bevy::ui::{FocusPolicy, RelativeCursorPosition};

use super::Phase;
use super::hud::{HudFont, MenuRoot, PauseRoot};
use super::kit::{self, KitButton, KitCaption, KitStyle};
use super::framerate::{self, CAPS, SCALES};
use super::gfxpresets::{self, Preset};
use super::menu::MenuBuilt;
use super::prefs::{AntiAlias, Prefs};
use benilla_app::survivors_host::cvars::Cvars;
use benilla_app::survivors_host::glue::GlueVisuals;
use benilla_world::view::FARCLIP_RANGE;

const PANEL_W: f32 = 460.0;
/// Over the pause screen (430) and the results (440).
const Z: i32 = 450;
const TRACK_H: f32 = 18.0;
/// The mouse sensitivity slider's range and step.
const SENS_MIN: f32 = 0.25;
const SENS_MAX: f32 = 2.0;
const SENS_STEP: f32 = 0.05;

const GROOVE_EDGE: Color = Color::srgb(0.47, 0.37, 0.19);
const FILL: Color = kit::TITLE;
/// The pause screen's own button look, so the Options button there matches its neighbours.
const PAUSE_BUTTON: Color = Color::srgba(0.18, 0.06, 0.04, 0.95);
const PAUSE_BUTTON_HOT: Color = Color::srgba(0.36, 0.12, 0.06, 0.98);
const PAUSE_EDGE: Color = Color::srgb(0.55, 0.45, 0.25);
/// Narrower than the logical size a window is ever remembered at.
#[cfg(not(target_arch = "wasm32"))]
const MIN_WINDOWED: u32 = 640;
/// How long a dragged window edge must hold still before the size is remembered.
#[cfg(not(target_arch = "wasm32"))]
const SIZE_SETTLE: f32 = 1.0;

pub(super) fn plugin(app: &mut App) {
    app.init_resource::<OptionsUi>().add_systems(
        Update,
        (build, pause_button, option_clicks, drag, refresh)
            .chain()
            // After the keys, which read Esc and Enter off [`OptionsUi::open`] first.
            .after(super::hud::keys)
            .before(GlueVisuals),
    );
    #[cfg(not(target_arch = "wasm32"))]
    app.add_systems(Update, remember_windowed_size);
}

/// Whether the window is open, and which tab it shows.
#[derive(Resource, Default)]
pub(super) struct OptionsUi {
    pub(super) open: bool,
    pub(super) tab: OptionsTab,
    root: Option<Entity>,
}

/// The window's tabs. Video (the display mode and the frame budget) is native only: the browser
/// cannot change its window.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub(super) enum OptionsTab {
    #[default]
    Sound,
    Video,
    Graphics,
    Controls,
}

impl OptionsTab {
    const ALL: [Self; 4] = [Self::Sound, Self::Video, Self::Graphics, Self::Controls];

    fn label(self) -> &'static str {
        match self {
            Self::Sound => "Sound",
            Self::Video => "Video",
            Self::Graphics => "Graphics",
            Self::Controls => "Controls",
        }
    }

    /// Whether this build has the tab.
    fn shown(self) -> bool {
        self != Self::Video || cfg!(not(target_arch = "wasm32"))
    }
}

/// One of the volume sliders.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Volume {
    Master,
    Music,
    Effects,
    Ambience,
}

impl Volume {
    const ALL: [Self; 4] = [Self::Master, Self::Music, Self::Effects, Self::Ambience];

    /// The registered CVar the slider writes.
    fn cvar(self) -> &'static str {
        match self {
            Self::Master => "MasterVolume",
            Self::Music => "MusicVolume",
            Self::Effects => "SoundVolume",
            Self::Ambience => "AmbienceVolume",
        }
    }

    fn label(self) -> &'static str {
        match self {
            Self::Master => "Master",
            Self::Music => "Music",
            Self::Effects => "Effects",
            Self::Ambience => "Ambience",
        }
    }
}

/// One of the window's sliders: a volume (a CVar), the mouse sensitivity ([`Prefs`]), the view
/// distance (the `farclip` CVar), the grade's sharpness ([`Prefs`]), or one of the two stepped
/// performance sliders. A stepped slider's value is the index of its stop, in [`CAPS`] or
/// [`SCALES`].
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
#[cfg_attr(target_arch = "wasm32", allow(dead_code))]
enum Slider {
    Volume(Volume),
    Sensitivity,
    /// The view distance in yards (the `farclip` CVar).
    ViewDistance,
    /// The grade's sharpening, `[0, 1]` ([`Prefs::sharpness`]).
    Sharpness,
    /// The frame-rate cap ([`Prefs::frame_cap`]).
    FrameCap,
    /// The world's render scale (the `renderScale` CVar).
    RenderScale,
}

impl Slider {
    fn label(self) -> &'static str {
        match self {
            Self::Volume(v) => v.label(),
            Self::Sensitivity => "Mouse sensitivity",
            Self::ViewDistance => "View distance",
            Self::Sharpness => "Sharpness",
            Self::FrameCap => "Frame rate cap",
            Self::RenderScale => "World resolution",
        }
    }

    /// The number of stops of a stepped slider.
    fn stops(self) -> Option<usize> {
        match self {
            Self::FrameCap => Some(CAPS.len()),
            Self::RenderScale => Some(SCALES.len()),
            Self::Volume(_) | Self::Sensitivity | Self::ViewDistance | Self::Sharpness => None,
        }
    }

    /// Width of the name column; the longer name needs more room than the volumes'.
    fn label_width(self) -> f32 {
        match self {
            Self::Volume(_) => 76.0,
            Self::Sensitivity
            | Self::ViewDistance
            | Self::Sharpness
            | Self::FrameCap
            | Self::RenderScale => 128.0,
        }
    }

    /// The value in its own unit: `[0, 1]` for a volume, a multiplier for the sensitivity.
    fn current(self, cvars: &Cvars, prefs: &Prefs) -> f32 {
        match self {
            Self::Volume(v) => volume(cvars, v),
            Self::Sensitivity => prefs.camera_sensitivity.clamp(SENS_MIN, SENS_MAX),
            Self::ViewDistance => gfxpresets::view_distance(cvars),
            Self::Sharpness => prefs.sharpness.clamp(0.0, 1.0),
            Self::FrameCap => framerate::cap_index(prefs.frame_cap) as f32,
            Self::RenderScale => framerate::scale_index(chosen_scale(cvars)) as f32,
        }
    }

    /// Where the thumb sits, `[0, 1]` along the track.
    fn fraction(self, cvars: &Cvars, prefs: &Prefs) -> f32 {
        let now = self.current(cvars, prefs);
        match self {
            Self::Volume(_) => now,
            Self::Sensitivity => (now - SENS_MIN) / (SENS_MAX - SENS_MIN),
            Self::ViewDistance => (now - view_min()) / (view_max() - view_min()),
            Self::Sharpness => now,
            Self::FrameCap | Self::RenderScale => {
                now / (self.stops().unwrap_or(2) - 1).max(1) as f32
            }
        }
    }

    /// The value a cursor at `t` along the track asks for, in steps (a volume of one percent).
    fn at(self, t: f32) -> f32 {
        let t = t.clamp(0.0, 1.0);
        match self {
            Self::Volume(_) => (t * 100.0).round() / 100.0,
            Self::Sensitivity => {
                let raw = SENS_MIN + t * (SENS_MAX - SENS_MIN);
                // Rounded to two places again, so the saved number has no float dust.
                ((raw / SENS_STEP).round() * SENS_STEP * 100.0).round() / 100.0
            }
            Self::ViewDistance => (view_min() + t * (view_max() - view_min())).round(),
            // Five percent a step, rounded again so the saved number has no float dust.
            Self::Sharpness => ((t * 20.0).round() * 5.0).round() / 100.0,
            Self::FrameCap | Self::RenderScale => {
                let last = self.stops().unwrap_or(2) - 1;
                (t * last as f32).round()
            }
        }
    }

    /// The readout beside the track.
    fn readout(self, cvars: &Cvars, prefs: &Prefs) -> String {
        let now = self.current(cvars, prefs);
        match self {
            Self::Volume(_) => format!("{}%", (now * 100.0).round()),
            Self::Sensitivity => format!("{now:.2}x"),
            Self::ViewDistance => format!("{}", now.round()),
            Self::Sharpness => format!("{}%", (now * 100.0).round()),
            Self::FrameCap => framerate::cap_label(CAPS[(now as usize).min(CAPS.len() - 1)]),
            Self::RenderScale => format!(
                "{}%",
                (SCALES[(now as usize).min(SCALES.len() - 1)] * 100.0).round()
            ),
        }
    }
}

fn view_min() -> f32 {
    *FARCLIP_RANGE.start()
}

fn view_max() -> f32 {
    *FARCLIP_RANGE.end()
}

/// The render scale the player chose: the `renderScale` CVar, 1.0 when it is missing.
fn chosen_scale(cvars: &Cvars) -> f32 {
    cvars
        .num(framerate::SCALE_CVAR)
        .filter(|v| v.is_finite() && *v > 0.0)
        .unwrap_or(1.0)
}

/// A button of the window, on the pick screen or inside it.
#[derive(Component, Clone, Copy, PartialEq, Eq, Debug)]
#[cfg_attr(target_arch = "wasm32", allow(dead_code))]
enum OptionsAction {
    Close,
    /// The master enable, `MasterSoundEffects`.
    Mute,
    /// [`Prefs::invert_pitch`].
    InvertPitch,
    /// [`Prefs::camera_shake`].
    CameraShake,
    /// [`Prefs::dynamic_scale`].
    DynamicScale,
    /// [`Prefs::show_fps`].
    ShowFps,
    Windowed,
    Fullscreen,
    /// A tab of the bar.
    Tab(OptionsTab),
    /// A graphics preset ([`gfxpresets`]).
    Preset(Preset),
    /// A cycling or toggling graphics button.
    Choice(Choice),
}

/// A graphics button that steps through a CVar's values or flips a toggle: the engine's detail
/// levels and switches (CVars) and the survivors' own picture settings ([`Prefs`]).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Choice {
    /// `worldDetail`: the ground clutter, 0..2.
    GroundClutter,
    /// `spellEffectLevel`: 0..2.
    SpellDetail,
    /// `weatherDensity`: 0..3.
    Weather,
    /// `anisotropic`: 1, 2, 4, 8, 16; the engine takes it at the next start.
    Anisotropic,
    /// `gxMultisample`: 1 (off), 2, 4, 8; latched.
    Multisample,
    /// [`Prefs::anti_alias`]: Off, FXAA, SMAA.
    AntiAlias,
    /// `ffxGlow`.
    Glow,
    /// `ffxDeath`.
    DeathEffect,
    /// [`Prefs::grade`].
    Grade,
    /// [`Prefs::hit_flash`].
    HitFlash,
    /// [`Prefs::death_fade`].
    CorpseFade,
}

/// The stops of the detail buttons, by CVar value.
const DETAIL: [&str; 3] = ["Low", "Medium", "High"];
const WEATHER: [&str; 4] = ["Minimal", "Light", "Medium", "Full"];

impl Choice {
    fn label(self) -> &'static str {
        match self {
            Self::GroundClutter => "Ground clutter",
            Self::SpellDetail => "Spell detail",
            Self::Weather => "Weather",
            Self::Anisotropic => "Anisotropic",
            Self::Multisample => "Multisampling",
            Self::AntiAlias => "Anti-aliasing",
            Self::Glow => "Screen glow",
            Self::DeathEffect => "Death effect",
            Self::Grade => "Sharpen & color",
            Self::HitFlash => "Hit flash",
            Self::CorpseFade => "Corpse fade",
        }
    }

    /// The button's text: its name and the current value.
    fn caption(self, cvars: &Cvars, prefs: &Prefs) -> String {
        let on = |v: bool| if v { "On" } else { "Off" };
        let steps = |steps: &[u32], name: &str| {
            let v = steps[gfxpresets::nearest(steps, cvars.num(name).unwrap_or(1.0))];
            if v <= 1 {
                "Off".to_string()
            } else {
                format!("{v}x")
            }
        };
        let value = match self {
            Self::GroundClutter => {
                DETAIL[gfxpresets::level(cvars, gfxpresets::WORLD_DETAIL, 1, 2)].to_string()
            }
            Self::SpellDetail => {
                DETAIL[gfxpresets::level(cvars, gfxpresets::SPELL_DETAIL, 2, 2)].to_string()
            }
            Self::Weather => {
                WEATHER[gfxpresets::level(cvars, gfxpresets::WEATHER, 3, 3)].to_string()
            }
            Self::Anisotropic => steps(&gfxpresets::ANISO_STEPS[..], gfxpresets::ANISO),
            Self::Multisample => steps(&gfxpresets::MSAA_STEPS[..], gfxpresets::MSAA),
            Self::AntiAlias => aa_label(prefs.anti_alias).to_string(),
            Self::Glow | Self::DeathEffect | Self::Grade | Self::HitFlash | Self::CorpseFade => {
                on(self.on(cvars, prefs)).to_string()
            }
        };
        format!("{}: {value}", self.label())
    }

    /// Whether a toggle is on, shown as the picked look; the cycling buttons are never picked.
    fn on(self, cvars: &Cvars, prefs: &Prefs) -> bool {
        match self {
            Self::Glow => cvars.flag(gfxpresets::GLOW).unwrap_or(true),
            Self::DeathEffect => cvars.flag(gfxpresets::DEATH_FX).unwrap_or(true),
            Self::Grade => prefs.grade,
            Self::HitFlash => prefs.hit_flash,
            Self::CorpseFade => prefs.death_fade,
            Self::GroundClutter
            | Self::SpellDetail
            | Self::Weather
            | Self::Anisotropic
            | Self::Multisample
            | Self::AntiAlias => false,
        }
    }

    /// One press: the next value, wrapping around, or the toggle flipped. Returns whether a pref
    /// changed, which the caller saves.
    fn step(self, cvars: &mut Cvars, prefs: &mut Prefs) -> bool {
        match self {
            Self::GroundClutter => {
                cycle(cvars, gfxpresets::WORLD_DETAIL, 1, 2);
                false
            }
            Self::SpellDetail => {
                cycle(cvars, gfxpresets::SPELL_DETAIL, 2, 2);
                false
            }
            Self::Weather => {
                cycle(cvars, gfxpresets::WEATHER, 3, 3);
                false
            }
            Self::Anisotropic => {
                cycle_steps(cvars, gfxpresets::ANISO, &gfxpresets::ANISO_STEPS);
                false
            }
            Self::Multisample => {
                cycle_steps(cvars, gfxpresets::MSAA, &gfxpresets::MSAA_STEPS);
                // Latched, like `gxWindow`: committing is what `RestartGx` does in the stock panel.
                cvars.commit_latched();
                false
            }
            Self::AntiAlias => {
                prefs.anti_alias = match prefs.anti_alias {
                    AntiAlias::Off => AntiAlias::Fxaa,
                    AntiAlias::Fxaa => AntiAlias::Smaa,
                    AntiAlias::Smaa => AntiAlias::Off,
                };
                true
            }
            Self::Glow => {
                flip(cvars, gfxpresets::GLOW);
                false
            }
            Self::DeathEffect => {
                flip(cvars, gfxpresets::DEATH_FX);
                false
            }
            Self::Grade => {
                prefs.grade = !prefs.grade;
                true
            }
            Self::HitFlash => {
                prefs.hit_flash = !prefs.hit_flash;
                true
            }
            Self::CorpseFade => {
                prefs.death_fade = !prefs.death_fade;
                true
            }
        }
    }
}

fn aa_label(mode: AntiAlias) -> &'static str {
    match mode {
        AntiAlias::Off => "Off",
        AntiAlias::Fxaa => "FXAA",
        AntiAlias::Smaa => "SMAA",
    }
}

/// A stepped CVar (0..=`max`) one stop on, back to 0 after the last.
fn cycle(cvars: &mut Cvars, name: &str, default: usize, max: usize) {
    let next = (gfxpresets::level(cvars, name, default, max) + 1) % (max + 1);
    cvars.set(name, &next.to_string());
}

/// A CVar over a list of values one value on, back to the first after the last.
fn cycle_steps(cvars: &mut Cvars, name: &str, steps: &[u32]) {
    let at = gfxpresets::nearest(steps, cvars.num(name).unwrap_or(1.0));
    cvars.set(name, &steps[(at + 1) % steps.len()].to_string());
}

/// A flag CVar flipped.
fn flip(cvars: &mut Cvars, name: &str) {
    let on = cvars.flag(name).unwrap_or(true);
    cvars.set(name, if on { "0" } else { "1" });
}

/// A text the refresh keeps current.
#[derive(Component, Clone, Copy)]
#[cfg_attr(target_arch = "wasm32", allow(dead_code))]
enum Label {
    /// A slider's readout.
    Value(Slider),
    /// The display mode line.
    Display,
    /// The preset line: the matching preset or "Custom".
    Preset,
}

/// The dimmed full-screen layer holding the window, hidden until opened.
#[derive(Component)]
struct OptionsPanel;
/// A slider's hit area; the press and the drag read the cursor off it.
#[derive(Component)]
struct Track(Slider);
#[derive(Component)]
struct Fill(Slider);
#[derive(Component)]
struct Thumb(Slider);
/// One tab's content; the refresh shows the page of the open tab.
#[derive(Component)]
struct Page(OptionsTab);
/// The Options button inside the pause screen.
#[derive(Component)]
struct PauseButton;
/// On the pick screen and the pause screen once their entry point is built in.
#[derive(Component)]
struct OptionsBuilt;

/// A slider's value, `[0, 1]`; an unregistered or unparsed row reads full.
fn volume(cvars: &Cvars, v: Volume) -> f32 {
    cvars.num(v.cvar()).unwrap_or(1.0).clamp(0.0, 1.0)
}

fn sound_on(cvars: &Cvars) -> bool {
    cvars.flag("MasterSoundEffects").unwrap_or(true)
}

/// Whether the window covers the monitor: `gxWindow` 0, or a maximized windowed window
/// ([`crate::video::window_mode`]).
fn fullscreen_now(cvars: &Cvars) -> bool {
    cvars.flag("gxWindow") == Some(false) || cvars.flag("gxMaximize") == Some(true)
}

/// Stage the mode and commit the latch, which `RestartGx` does in the stock panel; the events
/// reach [`crate::video::on_cvar`] on the next sync.
fn set_display(cvars: &mut Cvars, fullscreen: bool) {
    cvars.set("gxWindow", if fullscreen { "0" } else { "1" });
    cvars.set("gxMaximize", "0");
    cvars.commit_latched();
}

fn display_note(cvars: &Cvars) -> String {
    if benilla_app::survivors_host::video::windowed_env() {
        "Set by a launch option for this session".to_string()
    } else if fullscreen_now(cvars) {
        "Now: fullscreen".to_string()
    } else {
        format!(
            "Now: windowed, {}",
            cvars.get("gxResolution").unwrap_or("1600x900")
        )
    }
}

/// Build the Options button into the pick screen and the pause screen, and the window itself
/// once the glue art is in (the pick screen being built says so).
fn build(
    mut commands: Commands,
    menus: Query<Entity, (With<MenuRoot>, With<MenuBuilt>, Without<OptionsBuilt>)>,
    pauses: Query<Entity, (With<PauseRoot>, Without<OptionsBuilt>)>,
    fonts: Res<kit::KitFonts>,
    font: Res<HudFont>,
    mut ui: ResMut<OptionsUi>,
) {
    let f = &font.0;
    if let Ok(root) = pauses.single() {
        // Between Resume and Abandon run.
        let button = commands
            .spawn((
                PauseButton,
                Button,
                BackgroundColor(PAUSE_BUTTON),
                BorderColor::all(PAUSE_EDGE),
                Node {
                    width: Val::Px(240.0),
                    justify_content: JustifyContent::Center,
                    padding: UiRect::axes(Val::Px(20.0), Val::Px(8.0)),
                    border: UiRect::all(Val::Px(2.0)),
                    border_radius: BorderRadius::all(Val::Px(4.0)),
                    ..default()
                },
            ))
            .with_children(|b| {
                b.spawn((
                    Text::new("Options"),
                    TextFont {
                        font: f.clone(),
                        font_size: 20.0,
                        ..default()
                    },
                    TextColor(Color::WHITE),
                    TextShadow {
                        offset: Vec2::splat(1.5),
                        color: Color::BLACK,
                    },
                ));
            })
            .id();
        commands
            .entity(root)
            .insert(OptionsBuilt)
            .insert_children(2, &[button]);
    }
    let Ok(root) = menus.single() else { return };
    // The pick screen's Options tab lives in the top bar ([`super::menu`]); no button here.
    commands.entity(root).insert(OptionsBuilt);
    if ui.root.is_none() {
        ui.root = Some(panel(&mut commands, &fonts));
    }
}

/// The window: title, the tab bar, one page per tab (sound, video, graphics, controls) and Close.
/// The pages are all built; the refresh shows the one of the open tab. The panel hangs from the top
/// edge, so the tab bar stays put while the pages differ in height.
fn panel(commands: &mut Commands, fonts: &kit::KitFonts) -> Entity {
    let px = Val::Px;
    commands
        .spawn((
            OptionsPanel,
            // Swallows the clicks on whatever is under it.
            FocusPolicy::Block,
            BackgroundColor(Color::srgba(0.0, 0.0, 0.0, 0.6)),
            GlobalZIndex(Z),
            Node {
                position_type: PositionType::Absolute,
                width: Val::Percent(100.0),
                height: Val::Percent(100.0),
                justify_content: JustifyContent::Center,
                align_items: AlignItems::FlexStart,
                display: Display::None,
                ..default()
            },
        ))
        .with_children(|dim| {
            dim.spawn(kit::panel(Node {
                width: px(PANEL_W),
                flex_direction: FlexDirection::Column,
                padding: UiRect::axes(px(26.0), px(16.0)),
                row_gap: px(8.0),
                margin: UiRect::top(px(24.0)),
                ..default()
            }))
            .with_children(|frame| {
                kit::heading(frame, fonts, "Options", 26.0);
                tab_bar(frame, fonts);

                page(frame, OptionsTab::Sound, |p| sound_page(p, fonts));
                #[cfg(not(target_arch = "wasm32"))]
                page(frame, OptionsTab::Video, |p| video_page(p, fonts));
                page(frame, OptionsTab::Graphics, |p| graphics_page(p, fonts));
                page(frame, OptionsTab::Controls, |p| controls_page(p, fonts));

                frame
                    .spawn(Node {
                        width: Val::Percent(100.0),
                        justify_content: JustifyContent::Center,
                        margin: UiRect::top(px(8.0)),
                        ..default()
                    })
                    .with_children(|b| {
                        action_button(b, fonts, OptionsAction::Close, "Close", 146.0);
                    });
            });
        })
        .id()
}

/// The row of tab buttons under the title.
fn tab_bar(frame: &mut ChildSpawnerCommands, fonts: &kit::KitFonts) {
    frame
        .spawn(Node {
            width: Val::Percent(100.0),
            justify_content: JustifyContent::Center,
            column_gap: Val::Px(8.0),
            ..default()
        })
        .with_children(|bar| {
            for tab in OptionsTab::ALL.into_iter().filter(|t| t.shown()) {
                kit::button(
                    bar,
                    fonts,
                    KitStyle::Tab,
                    tab.label(),
                    None,
                    Node {
                        min_width: Val::Px(96.0),
                        height: Val::Px(28.0),
                        ..default()
                    },
                    OptionsAction::Tab(tab),
                );
            }
        });
}

/// One tab's page: a column that `fill` builds, shown by the refresh while its tab is open.
fn page(
    frame: &mut ChildSpawnerCommands,
    tab: OptionsTab,
    fill: impl FnOnce(&mut ChildSpawnerCommands),
) {
    frame
        .spawn((
            Page(tab),
            Node {
                width: Val::Percent(100.0),
                flex_direction: FlexDirection::Column,
                row_gap: Val::Px(6.0),
                display: Display::None,
                ..default()
            },
        ))
        .with_children(fill);
}

/// A small-caps section label, a little apart from what is above it.
fn section(parent: &mut ChildSpawnerCommands, fonts: &kit::KitFonts, name: &str) {
    parent.spawn((
        kit::label(fonts, name),
        Node {
            margin: UiRect::top(Val::Px(4.0)),
            ..default()
        },
    ));
}

/// An inset box holding slider rows.
fn slider_box(parent: &mut ChildSpawnerCommands, fonts: &kit::KitFonts, sliders: &[Slider]) {
    parent
        .spawn(kit::inset(Node {
            width: Val::Percent(100.0),
            flex_direction: FlexDirection::Column,
            row_gap: Val::Px(10.0),
            padding: UiRect::axes(Val::Px(14.0), Val::Px(12.0)),
            ..default()
        }))
        .with_children(|rows| {
            for slider in sliders {
                slider_row(rows, fonts, *slider);
            }
        });
}

/// A row of buttons spread over the window's width: `width` wide each, captions as given (the
/// refresh rewrites the choice buttons' own).
fn button_row(
    parent: &mut ChildSpawnerCommands,
    fonts: &kit::KitFonts,
    width: f32,
    buttons: &[(OptionsAction, &str)],
) {
    parent
        .spawn(Node {
            width: Val::Percent(100.0),
            justify_content: if buttons.len() == 1 {
                JustifyContent::Center
            } else {
                JustifyContent::SpaceBetween
            },
            ..default()
        })
        .with_children(|row| {
            for (action, caption) in buttons {
                action_button(row, fonts, *action, caption, width);
            }
        });
}

/// A centred line of small muted text; `marker` goes on the text, so the refresh can find it.
fn note(parent: &mut ChildSpawnerCommands, fonts: &kit::KitFonts, text: &str, marker: impl Bundle) {
    parent
        .spawn(Node {
            width: Val::Percent(100.0),
            justify_content: JustifyContent::Center,
            ..default()
        })
        .with_children(|row| {
            row.spawn((marker, kit::body(fonts, text, 12.0, kit::MUTED)));
        });
}

/// The four volumes and the mute button.
fn sound_page(p: &mut ChildSpawnerCommands, fonts: &kit::KitFonts) {
    p.spawn(kit::label(fonts, "Sound"));
    slider_box(p, fonts, &Volume::ALL.map(Slider::Volume));
    button_row(p, fonts, 190.0, &[(OptionsAction::Mute, "Sound: On")]);
}

/// The camera: mouse sensitivity, inverted pitch, shake.
fn controls_page(p: &mut ChildSpawnerCommands, fonts: &kit::KitFonts) {
    p.spawn(kit::label(fonts, "Camera"));
    slider_box(p, fonts, &[Slider::Sensitivity]);
    button_row(
        p,
        fonts,
        190.0,
        &[
            (OptionsAction::InvertPitch, "Invert pitch: Off"),
            (OptionsAction::CameraShake, "Camera shake: On"),
        ],
    );
}

/// The display mode and the frame budget (native only).
#[cfg(not(target_arch = "wasm32"))]
fn video_page(p: &mut ChildSpawnerCommands, fonts: &kit::KitFonts) {
    p.spawn(kit::label(fonts, "Display"));
    button_row(
        p,
        fonts,
        190.0,
        &[
            (OptionsAction::Windowed, "Windowed"),
            (OptionsAction::Fullscreen, "Fullscreen"),
        ],
    );
    note(p, fonts, "", Label::Display);

    section(p, fonts, "Performance");
    slider_box(p, fonts, &[Slider::FrameCap, Slider::RenderScale]);
    button_row(
        p,
        fonts,
        190.0,
        &[
            (OptionsAction::DynamicScale, "Auto resolution: Off"),
            (OptionsAction::ShowFps, "Show FPS: Off"),
        ],
    );
}

/// The preset row, the engine's detail levels and the survivors' picture settings.
fn graphics_page(p: &mut ChildSpawnerCommands, fonts: &kit::KitFonts) {
    let choice = |c: Choice| (OptionsAction::Choice(c), c.label());

    p.spawn(kit::label(fonts, "Preset"));
    let presets: Vec<(OptionsAction, &str)> = Preset::ALL
        .into_iter()
        .map(|preset| (OptionsAction::Preset(preset), preset.label()))
        .collect();
    button_row(p, fonts, 96.0, &presets);
    note(p, fonts, "", Label::Preset);

    section(p, fonts, "World");
    slider_box(p, fonts, &[Slider::ViewDistance]);
    button_row(
        p,
        fonts,
        200.0,
        &[choice(Choice::GroundClutter), choice(Choice::Weather)],
    );
    button_row(
        p,
        fonts,
        200.0,
        &[choice(Choice::SpellDetail), choice(Choice::Anisotropic)],
    );
    note(p, fonts, "Anisotropic filtering applies after restart.", ());

    section(p, fonts, "Picture");
    button_row(
        p,
        fonts,
        200.0,
        &[choice(Choice::Multisample), choice(Choice::AntiAlias)],
    );
    button_row(
        p,
        fonts,
        200.0,
        &[choice(Choice::Glow), choice(Choice::DeathEffect)],
    );
    button_row(
        p,
        fonts,
        200.0,
        &[choice(Choice::HitFlash), choice(Choice::CorpseFade)],
    );
    button_row(p, fonts, 200.0, &[choice(Choice::Grade)]);
    slider_box(p, fonts, &[Slider::Sharpness]);
}

/// A kit button of the window, `width` wide.
fn action_button(
    parent: &mut ChildSpawnerCommands,
    fonts: &kit::KitFonts,
    action: OptionsAction,
    caption: &str,
    width: f32,
) {
    kit::button(
        parent,
        fonts,
        KitStyle::Normal,
        caption,
        None,
        Node {
            width: Val::Px(width),
            height: Val::Px(30.0),
            ..default()
        },
        action,
    );
}

/// One slider's row: its name, a track with a fill and a thumb, and the percentage.
fn slider_row(parent: &mut ChildSpawnerCommands, fonts: &kit::KitFonts, v: Slider) {
    let px = Val::Px;
    parent
        .spawn(Node {
            width: Val::Percent(100.0),
            align_items: AlignItems::Center,
            column_gap: px(14.0),
            ..default()
        })
        .with_children(|row| {
            row.spawn((
                kit::body(fonts, v.label(), 14.0, kit::CREAM),
                Node {
                    width: px(v.label_width()),
                    flex_shrink: 0.0,
                    ..default()
                },
            ));
            row.spawn((
                Track(v),
                Button,
                RelativeCursorPosition::default(),
                Node {
                    flex_grow: 1.0,
                    flex_basis: px(0.0),
                    height: px(TRACK_H),
                    ..default()
                },
            ))
            .with_children(|t| {
                t.spawn((
                    BackgroundColor(kit::INSET_FILL),
                    BorderColor::all(kit::INSET_EDGE),
                    Node {
                        position_type: PositionType::Absolute,
                        left: px(0.0),
                        right: px(0.0),
                        top: px(5.0),
                        height: px(8.0),
                        border: UiRect::all(px(1.0)),
                        border_radius: BorderRadius::all(px(3.0)),
                        ..default()
                    },
                ));
                t.spawn((
                    Fill(v),
                    BackgroundColor(FILL),
                    Node {
                        position_type: PositionType::Absolute,
                        left: px(1.0),
                        top: px(6.0),
                        height: px(6.0),
                        width: Val::Percent(0.0),
                        border_radius: BorderRadius::all(px(2.0)),
                        ..default()
                    },
                ));
                t.spawn((
                    Thumb(v),
                    BackgroundColor(kit::CREAM),
                    BorderColor::all(GROOVE_EDGE),
                    Node {
                        position_type: PositionType::Absolute,
                        left: Val::Percent(0.0),
                        top: px(1.0),
                        width: px(8.0),
                        height: px(16.0),
                        margin: UiRect::left(px(-4.0)),
                        border: UiRect::all(px(1.0)),
                        border_radius: BorderRadius::all(px(2.0)),
                        ..default()
                    },
                ));
            });
            row.spawn((
                Label::Value(v),
                kit::body(fonts, "", 14.0, kit::CREAM),
                Node {
                    width: px(48.0),
                    flex_shrink: 0.0,
                    ..default()
                },
            ));
        });
}

/// The pause screen's Options button: hover colour, and open on the press as its neighbours act.
fn pause_button(
    mut buttons: Query<
        (&Interaction, &mut BackgroundColor),
        (With<PauseButton>, Changed<Interaction>),
    >,
    phase: Res<State<Phase>>,
    mut ui: ResMut<OptionsUi>,
) {
    for (interaction, mut bg) in &mut buttons {
        match interaction {
            Interaction::None => bg.0 = PAUSE_BUTTON,
            Interaction::Hovered => bg.0 = PAUSE_BUTTON_HOT,
            Interaction::Pressed => {
                bg.0 = PAUSE_BUTTON_HOT;
                if *phase.get() == Phase::Paused {
                    ui.open = true;
                }
            }
        }
    }
}

/// The window's buttons, and Esc and the phase closing it.
fn option_clicks(
    keys: Res<ButtonInput<KeyCode>>,
    actions: Query<(&Interaction, &OptionsAction, &KitButton), Changed<Interaction>>,
    phase: Res<State<Phase>>,
    mut cvars: ResMut<Cvars>,
    mut prefs: ResMut<Prefs>,
    mut ui: ResMut<OptionsUi>,
) {
    let leaving = !matches!(phase.get(), Phase::Menu | Phase::Paused);
    if ui.open && (leaving || keys.just_pressed(KeyCode::Escape)) {
        ui.open = false;
    }
    let Some((action, off)) = actions
        .iter()
        .find(|(i, _, _)| **i == Interaction::Pressed)
        .map(|(_, a, b)| (*a, b.disabled || b.selected))
    else {
        return;
    };
    match action {
        OptionsAction::Close => ui.open = false,
        OptionsAction::Mute => {
            let on = sound_on(&cvars);
            cvars.set("MasterSoundEffects", if on { "0" } else { "1" });
        }
        OptionsAction::InvertPitch => {
            prefs.invert_pitch = !prefs.invert_pitch;
            prefs.save();
        }
        OptionsAction::CameraShake => {
            prefs.camera_shake = !prefs.camera_shake;
            prefs.save();
        }
        OptionsAction::DynamicScale => {
            prefs.dynamic_scale = !prefs.dynamic_scale;
            prefs.save();
        }
        OptionsAction::ShowFps => {
            prefs.show_fps = !prefs.show_fps;
            prefs.save();
        }
        OptionsAction::Tab(tab) => ui.tab = tab,
        OptionsAction::Preset(preset) => {
            gfxpresets::apply(preset, &mut cvars, &mut prefs);
            prefs.save();
        }
        OptionsAction::Choice(choice) => {
            if choice.step(&mut cvars, &mut prefs) {
                prefs.save();
            }
        }
        OptionsAction::Windowed | OptionsAction::Fullscreen if !off => {
            set_display(&mut cvars, action == OptionsAction::Fullscreen);
        }
        OptionsAction::Windowed | OptionsAction::Fullscreen => {}
    }
}

/// A press or a drag on a track sets its value to where the cursor is, in steps (a volume of one
/// percent). A pressed node stays pressed until the release, so the drag follows outside the
/// track too. The prefs (the sensitivity, the cap, the sharpness) are saved once the drag has
/// ended, not on every step of it; a render scale or a view distance is a CVar, which the registry
/// saves when it has been quiet.
fn drag(
    tracks: Query<(&Track, &Interaction, &RelativeCursorPosition)>,
    mut cvars: ResMut<Cvars>,
    mut prefs: ResMut<Prefs>,
    ui: Res<OptionsUi>,
    mut unsaved: Local<bool>,
) {
    let mut held = false;
    if ui.open {
        for (Track(slider), interaction, cursor) in &tracks {
            let (Interaction::Pressed, Some(at)) = (interaction, cursor.normalized) else {
                continue;
            };
            held |= matches!(
                *slider,
                Slider::Sensitivity | Slider::FrameCap | Slider::Sharpness
            );
            let want = slider.at(at.x + 0.5);
            if (want - slider.current(&cvars, &prefs)).abs() <= 0.004 {
                continue;
            }
            match *slider {
                Slider::Volume(v) => {
                    cvars.set(v.cvar(), &want.to_string());
                }
                Slider::Sensitivity => {
                    prefs.camera_sensitivity = want;
                    *unsaved = true;
                }
                Slider::Sharpness => {
                    prefs.sharpness = want;
                    *unsaved = true;
                }
                Slider::ViewDistance => {
                    cvars.set(gfxpresets::FARCLIP, &want.to_string());
                }
                Slider::FrameCap => {
                    prefs.frame_cap = CAPS[(want as usize).min(CAPS.len() - 1)];
                    *unsaved = true;
                }
                Slider::RenderScale => {
                    let scale = SCALES[(want as usize).min(SCALES.len() - 1)];
                    cvars.set(framerate::SCALE_CVAR, &scale.to_string());
                }
            }
        }
    }
    // Let go (or the window closed under the cursor): write the file once.
    if *unsaved && !held {
        *unsaved = false;
        prefs.save();
    }
}

/// Show or hide the window and bring sliders, readouts, the toggle captions and the display row up
/// to the CVars and the prefs.
fn refresh(
    ui: Res<OptionsUi>,
    cvars: Res<Cvars>,
    prefs: Res<Prefs>,
    mut panel: Query<&mut Node, (With<OptionsPanel>, Without<Fill>, Without<Thumb>)>,
    mut pages: Query<(&Page, &mut Node), (Without<OptionsPanel>, Without<Fill>, Without<Thumb>)>,
    mut fills: Query<(&Fill, &mut Node), Without<Thumb>>,
    mut thumbs: Query<(&Thumb, &mut Node), Without<Fill>>,
    mut labels: Query<(&Label, &mut Text), Without<KitCaption>>,
    mut buttons: Query<(Entity, &OptionsAction, &mut KitButton)>,
    children: Query<&Children>,
    mut captions: Query<&mut Text, With<KitCaption>>,
) {
    let display = if ui.open {
        Display::Flex
    } else {
        Display::None
    };
    for mut node in &mut panel {
        if node.display != display {
            node.display = display;
        }
    }
    if !ui.open {
        return;
    }
    for (Page(tab), mut node) in &mut pages {
        let want = if *tab == ui.tab {
            Display::Flex
        } else {
            Display::None
        };
        if node.display != want {
            node.display = want;
        }
    }
    let preset = gfxpresets::matching(&cvars, &prefs);
    for (Fill(v), mut node) in &mut fills {
        let want = Val::Percent(v.fraction(&cvars, &prefs) * 100.0);
        if node.width != want {
            node.width = want;
        }
    }
    for (Thumb(v), mut node) in &mut thumbs {
        let want = Val::Percent(v.fraction(&cvars, &prefs) * 100.0);
        if node.left != want {
            node.left = want;
        }
    }
    for (label, mut text) in &mut labels {
        let want = match *label {
            Label::Value(v) => v.readout(&cvars, &prefs),
            Label::Display => display_note(&cvars),
            Label::Preset => match preset {
                Some(p) => format!("Preset: {}", p.label()),
                None => "Preset: Custom".to_string(),
            },
        };
        if text.0 != want {
            text.0 = want;
        }
    }
    let fullscreen = fullscreen_now(&cvars);
    let launch_owned = benilla_app::survivors_host::video::windowed_env();
    for (button, action, mut state) in &mut buttons {
        // The text of a graphics button, built here so the match below can borrow it.
        let choice_caption: String;
        // (disabled, selected, caption): the display button of the current mode shows as picked.
        let (off, picked, caption) = match *action {
            OptionsAction::Mute => (
                false,
                false,
                Some(if sound_on(&cvars) {
                    "Sound: On"
                } else {
                    "Sound: Off"
                }),
            ),
            OptionsAction::InvertPitch => (
                false,
                prefs.invert_pitch,
                Some(if prefs.invert_pitch {
                    "Invert pitch: On"
                } else {
                    "Invert pitch: Off"
                }),
            ),
            OptionsAction::CameraShake => (
                false,
                prefs.camera_shake,
                Some(if prefs.camera_shake {
                    "Camera shake: On"
                } else {
                    "Camera shake: Off"
                }),
            ),
            OptionsAction::DynamicScale => (
                false,
                prefs.dynamic_scale,
                Some(if prefs.dynamic_scale {
                    "Auto resolution: On"
                } else {
                    "Auto resolution: Off"
                }),
            ),
            OptionsAction::ShowFps => (
                false,
                prefs.show_fps,
                Some(if prefs.show_fps {
                    "Show FPS: On"
                } else {
                    "Show FPS: Off"
                }),
            ),
            OptionsAction::Windowed => (launch_owned, !fullscreen, None),
            OptionsAction::Fullscreen => (launch_owned, fullscreen, None),
            OptionsAction::Tab(tab) => (false, ui.tab == tab, None),
            OptionsAction::Preset(p) => (false, preset == Some(p), None),
            OptionsAction::Choice(c) => {
                choice_caption = c.caption(&cvars, &prefs);
                (false, c.on(&cvars, &prefs), Some(choice_caption.as_str()))
            }
            OptionsAction::Close => continue,
        };
        if state.disabled != off {
            state.disabled = off;
        }
        if state.selected != picked {
            state.selected = picked;
        }
        let Some(want) = caption else { continue };
        for d in children.iter_descendants(button) {
            if let Ok(mut text) = captions.get_mut(d) {
                if text.0 != want {
                    text.0 = want.to_string();
                }
                break;
            }
        }
    }
}

/// A window the player resized by dragging an edge is remembered as the windowed size
/// (`gxResolution`), once it has held still, so leaving fullscreen brings it back. Not while the
/// launch flags own the window, nor while it is anything but a plain window.
#[cfg(not(target_arch = "wasm32"))]
fn remember_windowed_size(
    time: Res<Time<Real>>,
    windows: Query<&Window, With<bevy::window::PrimaryWindow>>,
    mut cvars: ResMut<Cvars>,
    mut seen: Local<(UVec2, f32)>,
) {
    let Ok(window) = windows.single() else { return };
    if benilla_app::survivors_host::video::windowed_env()
        || window.mode != bevy::window::WindowMode::Windowed
        || fullscreen_now(&cvars)
    {
        *seen = (UVec2::ZERO, 0.0);
        return;
    }
    let size = UVec2::new(
        window.resolution.width() as u32,
        window.resolution.height() as u32,
    );
    let now = time.elapsed_secs();
    if size != seen.0 {
        *seen = (size, now);
        return;
    }
    if now - seen.1 < SIZE_SETTLE || size.x < MIN_WINDOWED || size.y < MIN_WINDOWED / 2 {
        return;
    }
    let stored = cvars
        .get("gxResolution")
        .and_then(benilla_app::survivors_host::video::parse_resolution);
    if stored != Some(size) {
        cvars.set("gxResolution", &format!("{}x{}", size.x, size.y));
        cvars.commit_latched();
    }
}
