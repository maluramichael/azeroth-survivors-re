//! The options window: sound volumes, the camera and the display mode (windowed or fullscreen),
//! opened from the pick screen and the pause screen, and kept across restarts.
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
//! The performance part (native only) is two stepped sliders and two toggles
//! ([`super::framerate`]): the frame-rate cap and the dynamic scale and the readout are [`Prefs`],
//! the render scale is the engine's `renderScale` CVar, which the registry saves itself.

use bevy::prelude::*;
use bevy::ui::{FocusPolicy, RelativeCursorPosition};

use super::hud::{HudFont, MenuRoot, PauseRoot};
use super::kit::{self, KitButton, KitCaption, KitStyle};
use super::framerate::{self, CAPS, SCALES};
use super::menu::MenuBuilt;
use super::prefs::Prefs;
use super::Phase;
use benilla_app::survivors_host::cvars::Cvars;
use benilla_app::survivors_host::glue::GlueVisuals;

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

/// Whether the window is open.
#[derive(Resource, Default)]
pub(super) struct OptionsUi {
    pub(super) open: bool,
    root: Option<Entity>,
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

/// One of the window's sliders: a volume (a CVar), the mouse sensitivity ([`Prefs`]), or one of
/// the two stepped performance sliders. A stepped slider's value is the index of its stop, in
/// [`CAPS`] or [`SCALES`].
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
#[cfg_attr(target_arch = "wasm32", allow(dead_code))]
enum Slider {
    Volume(Volume),
    Sensitivity,
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
            Self::FrameCap => "Frame rate cap",
            Self::RenderScale => "World resolution",
        }
    }

    /// The number of stops of a stepped slider.
    fn stops(self) -> Option<usize> {
        match self {
            Self::FrameCap => Some(CAPS.len()),
            Self::RenderScale => Some(SCALES.len()),
            Self::Volume(_) | Self::Sensitivity => None,
        }
    }

    /// Width of the name column; the longer name needs more room than the volumes'.
    fn label_width(self) -> f32 {
        match self {
            Self::Volume(_) => 76.0,
            Self::Sensitivity | Self::FrameCap | Self::RenderScale => 128.0,
        }
    }

    /// The value in its own unit: `[0, 1]` for a volume, a multiplier for the sensitivity.
    fn current(self, cvars: &Cvars, prefs: &Prefs) -> f32 {
        match self {
            Self::Volume(v) => volume(cvars, v),
            Self::Sensitivity => prefs.camera_sensitivity.clamp(SENS_MIN, SENS_MAX),
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
            Self::FrameCap => framerate::cap_label(CAPS[(now as usize).min(CAPS.len() - 1)]),
            Self::RenderScale => format!(
                "{}%",
                (SCALES[(now as usize).min(SCALES.len() - 1)] * 100.0).round()
            ),
        }
    }
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
}

/// A text the refresh keeps current.
#[derive(Component, Clone, Copy)]
#[cfg_attr(target_arch = "wasm32", allow(dead_code))]
enum Label {
    /// A slider's readout.
    Value(Slider),
    /// The display mode line.
    Display,
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

/// The window: title, the four sliders and the mute button, the camera part, the display row,
/// and Close.
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
                align_items: AlignItems::Center,
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
                ..default()
            }))
            .with_children(|frame| {
                kit::heading(frame, fonts, "Options", 26.0);

                frame.spawn(kit::label(fonts, "Sound"));
                frame
                    .spawn(kit::inset(Node {
                        width: Val::Percent(100.0),
                        flex_direction: FlexDirection::Column,
                        row_gap: px(10.0),
                        padding: UiRect::axes(px(14.0), px(12.0)),
                        ..default()
                    }))
                    .with_children(|rows| {
                        for v in Volume::ALL {
                            slider_row(rows, fonts, Slider::Volume(v));
                        }
                    });
                frame
                    .spawn(Node {
                        width: Val::Percent(100.0),
                        justify_content: JustifyContent::Center,
                        ..default()
                    })
                    .with_children(|b| {
                        action_button(b, fonts, OptionsAction::Mute, "Sound: On", 190.0);
                    });

                frame.spawn((
                    kit::label(fonts, "Camera"),
                    Node {
                        margin: UiRect::top(px(6.0)),
                        ..default()
                    },
                ));
                frame
                    .spawn(kit::inset(Node {
                        width: Val::Percent(100.0),
                        flex_direction: FlexDirection::Column,
                        padding: UiRect::axes(px(14.0), px(12.0)),
                        ..default()
                    }))
                    .with_children(|rows| {
                        slider_row(rows, fonts, Slider::Sensitivity);
                    });
                frame
                    .spawn(Node {
                        width: Val::Percent(100.0),
                        justify_content: JustifyContent::SpaceBetween,
                        ..default()
                    })
                    .with_children(|row| {
                        for (action, caption) in [
                            (OptionsAction::InvertPitch, "Invert pitch: Off"),
                            (OptionsAction::CameraShake, "Camera shake: On"),
                        ] {
                            action_button(row, fonts, action, caption, 190.0);
                        }
                    });

                #[cfg(not(target_arch = "wasm32"))]
                {
                    frame.spawn((
                        kit::label(fonts, "Display"),
                        Node {
                            margin: UiRect::top(px(6.0)),
                            ..default()
                        },
                    ));
                    frame
                        .spawn(Node {
                            width: Val::Percent(100.0),
                            justify_content: JustifyContent::SpaceBetween,
                            ..default()
                        })
                        .with_children(|row| {
                            for (action, caption) in [
                                (OptionsAction::Windowed, "Windowed"),
                                (OptionsAction::Fullscreen, "Fullscreen"),
                            ] {
                                action_button(row, fonts, action, caption, 190.0);
                            }
                        });
                    frame
                        .spawn(Node {
                            width: Val::Percent(100.0),
                            justify_content: JustifyContent::Center,
                            ..default()
                        })
                        .with_children(|row| {
                            row.spawn((Label::Display, kit::body(fonts, "", 12.0, kit::MUTED)));
                        });

                    frame.spawn((
                        kit::label(fonts, "Performance"),
                        Node {
                            margin: UiRect::top(px(6.0)),
                            ..default()
                        },
                    ));
                    frame
                        .spawn(kit::inset(Node {
                            width: Val::Percent(100.0),
                            flex_direction: FlexDirection::Column,
                            row_gap: px(10.0),
                            padding: UiRect::axes(px(14.0), px(12.0)),
                            ..default()
                        }))
                        .with_children(|rows| {
                            slider_row(rows, fonts, Slider::FrameCap);
                            slider_row(rows, fonts, Slider::RenderScale);
                        });
                    frame
                        .spawn(Node {
                            width: Val::Percent(100.0),
                            justify_content: JustifyContent::SpaceBetween,
                            ..default()
                        })
                        .with_children(|row| {
                            for (action, caption) in [
                                (OptionsAction::DynamicScale, "Auto resolution: Off"),
                                (OptionsAction::ShowFps, "Show FPS: Off"),
                            ] {
                                action_button(row, fonts, action, caption, 190.0);
                            }
                        });
                }

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
        OptionsAction::Windowed | OptionsAction::Fullscreen if !off => {
            set_display(&mut cvars, action == OptionsAction::Fullscreen);
        }
        OptionsAction::Windowed | OptionsAction::Fullscreen => {}
    }
}

/// A press or a drag on a track sets its value to where the cursor is, in steps (a volume of one
/// percent). A pressed node stays pressed until the release, so the drag follows outside the
/// track too. The prefs (the sensitivity, the cap) are saved once the drag has ended, not on every
/// step of it; a render scale is a CVar, which the registry saves when it has been quiet.
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
            held |= matches!(*slider, Slider::Sensitivity | Slider::FrameCap);
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
        };
        if text.0 != want {
            text.0 = want;
        }
    }
    let fullscreen = fullscreen_now(&cvars);
    let launch_owned = benilla_app::survivors_host::video::windowed_env();
    for (button, action, mut state) in &mut buttons {
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
