//! WoW unit frames: the hero's `PlayerFrame` at the top left and a `TargetFrame` beside it, both
//! drawn from `UI-TargetingFrame` with the stock 1.12 offsets. A click on an enemy targets it,
//! and the abilities favour the target ([`Target`], read by [`super::abilities`]).
//!
//! A click is a press and a release that stay put: the left button also drags the camera, so a
//! drag targets nothing. A click on an enemy (the cursor's ray through its body, else the ground
//! point under the cursor within a yard or two) targets the nearest one; a click on bare ground
//! clears the target. **Esc** is left to the pause. A target that dies is dropped.

use benilla_world::view::WorldCamera;
use bevy::prelude::*;
use bevy::window::PrimaryWindow;

use super::enemies::Enemy;
use super::focus::Target;
use super::hero::{AimPoint, UiCover};
use super::hud::{self, HudFont, UiArt, GOLD_TONE};
use super::profile::Profile;
use super::{Phase, Run, Selection};
use benilla_app::survivors_host::vplates::con_color;

/// `PlayerFrame` and `TargetFrame` are 232x100 (`PlayerFrame.xml`, `TargetFrame.xml`).
const FRAME_W: f32 = 232.0;
const FRAME_H: f32 = 100.0;
const LEFT: f32 = 4.0;
const TOP: f32 = 2.0;
/// The player frame sits at x 0, the target frame at x 250 in the stock layout.
const GAP: f32 = 18.0;
const PORTRAIT: f32 = 64.0;
const PORTRAIT_Y: f32 = 12.0;
/// The health and mana bars, 119x12 (`PlayerFrameHealthBar`, `PlayerFrameManaBar`).
const BAR_W: f32 = 119.0;
const BAR_H: f32 = 12.0;
const HEALTH_Y: f32 = 41.0;
const MANA_Y: f32 = 52.0;
/// The plate behind the name and both bars, 119x41 at y 22 (`PlayerFrameBackground`).
const PLATE_Y: f32 = 22.0;
const PLATE_H: f32 = 41.0;
const NAME_Y: f32 = 31.0;
const LEVEL_Y: f32 = 66.0;
/// Level with the HUD (400), over it, under the character window and the level-up cards.
const Z: i32 = 401;
const PLATE: Color = Color::srgba(0.0, 0.0, 0.0, 0.55);
const HOSTILE: Color = Color::srgba(0.62, 0.08, 0.08, 0.8);
const HERO_BAR: Color = Color::srgb(0.0, 0.9, 0.0);
const XP_BAR: Color = Color::srgb(0.58, 0.0, 0.55);
const CLASSES: &str = "Interface\\Glues\\CharacterCreate\\UI-CharacterCreate-Classes";
/// The frame art, 256x128: the 232x100 frame is its columns from `0.09375` on, rows to `0.78125`.
/// The player frame draws it mirrored (`PlayerFrameTexture` texcoords `1.0, 0.09375, 0, 0.78125`),
/// the target frame as it is (`0.09375, 1.0, 0, 0.78125`).
const FRAME_ART: &str = "Interface\\TargetingFrame\\UI-TargetingFrame";
/// The gold dragon of an elite or a boss (`TargetFrame_CheckClassification`).
const ELITE_ART: &str = "Interface\\TargetingFrame\\UI-TargetingFrame-Elite";
const FRAME_TC: [f32; 4] = [0.09375, 1.0, 0.0, 0.78125];
const STATUS_BAR: &str = "Interface\\TargetingFrame\\UI-StatusBar";
/// The target's portraits: a grunt, an elite, a boss.
const FOE_ICONS: [&str; 3] = [
    "Interface\\Icons\\Ability_Creature_Cursed_02",
    "Interface\\Icons\\INV_Misc_Bone_HumanSkull_01",
    "Interface\\Icons\\INV_Misc_Head_Dragon_01",
];
const WHOLE: [f32; 4] = [0.0, 1.0, 0.0, 1.0];
/// A press and release this close (screen px) are a click, not a camera drag.
const CLICK_PX: f32 = 5.0;
/// How far from the cursor's ground point an enemy still counts as clicked (yd, besides its radius).
const AIM_YD: f32 = 1.5;

/// Where a frame has its parts, from the frame's own top left: the player frame has the portrait
/// on the left, the target frame on the right.
#[derive(Clone, Copy)]
struct Side {
    portrait_x: f32,
    bars_x: f32,
    /// The centres of the name (over the bars) and the level (under the portrait).
    name_x: f32,
    level_x: f32,
}

/// `PlayerFrame.xml`: portrait at 42,-12, bars at 106, name at the frame's centre +50,+19, level
/// at -63,-16.
const PLAYER: Side = Side {
    portrait_x: 42.0,
    bars_x: 106.0,
    name_x: 166.0,
    level_x: 53.0,
};
/// `TargetFrame.xml`: portrait at the right edge -42, bars at 7, name at -50,+19, level at 63,-16.
const TARGET: Side = Side {
    portrait_x: 126.0,
    bars_x: 7.0,
    name_x: 66.0,
    level_x: 179.0,
};

pub(super) fn plugin(app: &mut App) {
    app.init_resource::<FramesUi>()
        .add_systems(
            Update,
            (click, show, mark)
                .chain()
                .after(super::hero::place_camera)
                .after(super::enemies::move_enemies)
                .run_if(in_run),
        )
        .add_systems(OnEnter(Phase::Menu), (teardown, forget))
        .add_systems(OnEnter(Phase::GameOver), teardown);
}

/// The phases that put the HUD on screen.
fn in_run(phase: Res<State<Phase>>) -> bool {
    matches!(phase.get(), Phase::Playing | Phase::LevelUp | Phase::Paused)
}

/// The enemy under a click: the one whose body the ray passes nearest the camera, else the one
/// nearest the ground point `aim`. `foes` are `(feet, radius)`; the answer indexes it.
fn pick_index(origin: Vec3, dir: Vec3, aim: Option<Vec3>, foes: &[(Vec3, f32)]) -> Option<usize> {
    let mut best: Option<(f32, usize)> = None;
    for (i, &(feet, radius)) in foes.iter().enumerate() {
        let reach = radius.max(0.6) + 0.4;
        let height = (radius * 3.0).max(2.5);
        // The body as four points up its height; the ray's miss distance to each.
        for k in [0.15f32, 0.4, 0.65, 0.9] {
            let v = feet + Vec3::Y * (height * k) - origin;
            let t = v.dot(dir);
            if t > 0.0 && (v - dir * t).length() <= reach && best.map_or(true, |(bt, _)| t < bt) {
                best = Some((t, i));
            }
        }
    }
    if let Some((_, i)) = best {
        return Some(i);
    }
    let aim = aim?;
    foes.iter()
        .enumerate()
        .map(|(i, &(feet, radius))| (i, super::units::flat(feet - aim).length() - radius))
        .filter(|&(_, d)| d <= AIM_YD)
        .min_by(|a, b| a.1.total_cmp(&b.1))
        .map(|(i, _)| i)
}

/// A left click, press to release, targets the enemy under the cursor or clears the target.
#[allow(clippy::too_many_arguments)]
fn click(
    phase: Res<State<Phase>>,
    mouse: Res<ButtonInput<MouseButton>>,
    windows: Query<&Window, With<PrimaryWindow>>,
    cams: Query<(&Camera, &Transform), With<WorldCamera>>,
    aim: Res<AimPoint>,
    ui: UiCover,
    enemies: Query<(Entity, &Transform, &Enemy)>,
    mut target: ResMut<Target>,
    mut press: Local<Option<Vec2>>,
) {
    if *phase.get() != Phase::Playing {
        *press = None;
        return;
    }
    let cursor = windows.single().ok().and_then(Window::cursor_position);
    if mouse.just_pressed(MouseButton::Left) {
        // A press on a panel is the panel's.
        *press = if ui.cursor_over_ui() { None } else { cursor };
    }
    if !mouse.just_released(MouseButton::Left) {
        return;
    }
    let (Some(down), Some(up)) = (press.take(), cursor) else {
        return;
    };
    if down.distance(up) > CLICK_PX {
        return;
    }
    let ray = cams
        .single()
        .ok()
        .and_then(|(cam, tf)| cam.viewport_to_world(&GlobalTransform::from(*tf), up).ok());
    let Some(ray) = ray else { return };
    let live: Vec<(Entity, Vec3, f32)> = enemies
        .iter()
        .filter(|(_, _, e)| !e.dead)
        .map(|(e, tf, en)| (e, tf.translation, en.radius))
        .collect();
    let foes: Vec<(Vec3, f32)> = live.iter().map(|&(_, p, r)| (p, r)).collect();
    let hit = pick_index(ray.origin, *ray.direction, aim.0, &foes).map(|i| live[i].0);
    if target.0 != hit {
        target.0 = hit;
    }
}

/// The parts of one unit frame the systems update.
struct Frame {
    root: Entity,
    name: Entity,
    level: Entity,
    hp_fill: Entity,
    hp_text: Entity,
    mp_fill: Entity,
    mp_text: Entity,
    /// The gold dragon over the plain frame art.
    elite: Entity,
    /// The portrait layers, one shown at a time.
    icons: Vec<Entity>,
}

/// The nodes the frames update, once built.
struct Parts {
    root: Entity,
    hero: Frame,
    foe: Frame,
    /// The marker over the target's head.
    marker: Entity,
}

#[derive(Resource, Default)]
struct FramesUi {
    parts: Option<Parts>,
}

/// Back at the menu or the results: the frames go, and the next run builds its own.
fn teardown(mut commands: Commands, mut ui: ResMut<FramesUi>) {
    if let Some(parts) = ui.parts.take() {
        commands.entity(parts.root).try_despawn();
    }
}

/// A new run starts with no target.
fn forget(mut target: ResMut<Target>) {
    target.0 = None;
}

fn text(font: &Handle<Font>, size: f32, color: Color) -> impl Bundle {
    (
        Text::new(""),
        TextFont {
            font: font.clone(),
            font_size: size,
            ..default()
        },
        TextColor(color),
        TextShadow {
            offset: Vec2::splat(1.0),
            color: Color::BLACK,
        },
        TextLayout::new(Justify::Center, LineBreak::NoWrap),
    )
}

/// A box at `x`,`y` from its parent's top left.
fn put(x: f32, y: f32, w: f32, h: f32) -> Node {
    Node {
        position_type: PositionType::Absolute,
        left: Val::Px(x),
        top: Val::Px(y),
        width: Val::Px(w),
        height: Val::Px(h),
        ..default()
    }
}

/// A line of text centred on `cx`,`cy`, clipped to `w` wide; the text's entity.
fn label(
    parent: &mut ChildSpawnerCommands,
    font: &Handle<Font>,
    size: f32,
    color: Color,
    cx: f32,
    cy: f32,
    w: f32,
) -> Entity {
    const H: f32 = 16.0;
    let mut id = Entity::PLACEHOLDER;
    parent
        .spawn(Node {
            justify_content: JustifyContent::Center,
            align_items: AlignItems::Center,
            overflow: Overflow::clip(),
            ..put(cx - w / 2.0, cy - H / 2.0, w, H)
        })
        .with_children(|slot| {
            id = slot.spawn(text(font, size, color)).id();
        });
    id
}

/// A bar's fill, `UI-StatusBar` tinted, in a 119x12 seat at `x`,`y`; the fill's entity.
fn bar(parent: &mut ChildSpawnerCommands, x: f32, y: f32, tint: Color) -> Entity {
    let mut id = Entity::PLACEHOLDER;
    parent.spawn(put(x, y, BAR_W, BAR_H)).with_children(|seat| {
        id = seat
            .spawn((
                Node {
                    width: Val::Percent(100.0),
                    height: Val::Percent(100.0),
                    ..default()
                },
                ImageNode {
                    color: Color::NONE,
                    ..default()
                },
                UiArt::tinted(STATUS_BAR, tint),
            ))
            .id();
    });
    id
}

/// A portrait layer: the art clipped to a circle over a dark disc.
struct Icon {
    path: &'static str,
    coords: [f32; 4],
    shown: bool,
}

/// A frame art layer filling the frame.
fn art(path: &'static str, flip: bool, shown: bool) -> impl Bundle {
    (
        put(0.0, 0.0, FRAME_W, FRAME_H),
        ImageNode {
            color: Color::NONE,
            flip_x: flip,
            ..default()
        },
        UiArt::crop(path, FRAME_TC),
        if shown {
            Visibility::Inherited
        } else {
            Visibility::Hidden
        },
    )
}

/// One unit frame at `left`: from the back the plate (with a `strip` colour behind the name), the
/// portraits, the bars, the frame art (mirrored for the player; with the elite dragon over it for
/// a target), then the texts, as the stock frames layer them.
#[allow(clippy::too_many_arguments)]
fn unit_frame(
    root: &mut ChildSpawnerCommands,
    font: &Handle<Font>,
    left: f32,
    side: Side,
    flip: bool,
    shown: bool,
    strip: Option<Color>,
    icons: &[Icon],
    bars: [Color; 2],
) -> Frame {
    let mut f = Frame {
        root: Entity::PLACEHOLDER,
        name: Entity::PLACEHOLDER,
        level: Entity::PLACEHOLDER,
        hp_fill: Entity::PLACEHOLDER,
        hp_text: Entity::PLACEHOLDER,
        mp_fill: Entity::PLACEHOLDER,
        mp_text: Entity::PLACEHOLDER,
        elite: Entity::PLACEHOLDER,
        icons: Vec::new(),
    };
    let mut frame = root.spawn(Node {
        display: if shown { Display::Flex } else { Display::None },
        ..put(left, TOP, FRAME_W, FRAME_H)
    });
    f.root = frame.id();
    frame.with_children(|p| {
        p.spawn((put(side.bars_x, PLATE_Y, BAR_W, PLATE_H), BackgroundColor(PLATE)));
        if let Some(color) = strip {
            p.spawn((put(side.bars_x, PLATE_Y, BAR_W, 19.0), BackgroundColor(color)));
        }
        for icon in icons {
            f.icons.push(
                p.spawn((
                    Node {
                        border_radius: BorderRadius::MAX,
                        ..put(side.portrait_x, PORTRAIT_Y, PORTRAIT, PORTRAIT)
                    },
                    BackgroundColor(if icon.shown {
                        Color::srgba(0.0, 0.0, 0.0, 0.6)
                    } else {
                        Color::NONE
                    }),
                    ImageNode {
                        color: Color::NONE,
                        ..default()
                    },
                    UiArt::crop(icon.path, icon.coords),
                    if icon.shown {
                        Visibility::Inherited
                    } else {
                        Visibility::Hidden
                    },
                ))
                .id(),
            );
        }
        f.hp_fill = bar(p, side.bars_x, HEALTH_Y, bars[0]);
        f.mp_fill = bar(p, side.bars_x, MANA_Y, bars[1]);
        p.spawn(art(FRAME_ART, flip, true));
        f.elite = p.spawn(art(ELITE_ART, flip, false)).id();
        f.name = label(p, font, 12.0, GOLD_TONE, side.name_x, NAME_Y, BAR_W - 6.0);
        f.level = label(p, font, 12.0, GOLD_TONE, side.level_x, LEVEL_Y, 26.0);
        let bars_cx = side.bars_x + BAR_W / 2.0;
        f.hp_text = label(p, font, 10.0, Color::WHITE, bars_cx, HEALTH_Y + BAR_H / 2.0, BAR_W);
        f.mp_text = label(p, font, 10.0, Color::WHITE, bars_cx, MANA_Y + BAR_H / 2.0, BAR_W);
    });
    f
}

fn build(commands: &mut Commands, ui: &mut FramesUi, font: &Handle<Font>, run: &Run) {
    let mut hero = None;
    let mut foe = None;
    let mut marker = Entity::PLACEHOLDER;
    let root = commands
        .spawn((
            Node {
                position_type: PositionType::Absolute,
                width: Val::Percent(100.0),
                height: Val::Percent(100.0),
                ..default()
            },
            GlobalZIndex(Z),
        ))
        .with_children(|root| {
            // The hero: the class icon in the round portrait, name and level, health, and the run's
            // experience where the mana would be.
            let coords =
                benilla_app::survivors_host::glue::art::class_tc(run.class.id).unwrap_or(WHOLE);
            let class = [Icon {
                path: CLASSES,
                coords,
                shown: true,
            }];
            hero = Some(unit_frame(
                root,
                font,
                LEFT,
                PLAYER,
                true,
                true,
                None,
                &class,
                [HERO_BAR, XP_BAR],
            ));
            // The target: hidden until there is one; its portrait follows the kind of enemy.
            let icons = FOE_ICONS.map(|path| Icon {
                path,
                coords: WHOLE,
                shown: false,
            });
            foe = Some(unit_frame(
                root,
                font,
                LEFT + FRAME_W + GAP,
                TARGET,
                false,
                false,
                Some(HOSTILE),
                &icons,
                [HERO_BAR, Color::NONE],
            ));
            // A gold diamond over the target's head.
            marker = root
                .spawn((
                    Node {
                        position_type: PositionType::Absolute,
                        width: Val::Px(11.0),
                        height: Val::Px(11.0),
                        ..default()
                    },
                    BackgroundColor(GOLD_TONE),
                    UiTransform::from_rotation(Rot2::degrees(45.0)),
                    Visibility::Hidden,
                ))
                .id();
        })
        .id();
    let (Some(hero), Some(foe)) = (hero, foe) else {
        return;
    };
    ui.parts = Some(Parts {
        root,
        hero,
        foe,
        marker,
    });
}

fn set<T: PartialEq>(slot: &mut T, v: T) {
    if *slot != v {
        *slot = v;
    }
}

/// Show the layer `shown` of `layers` and hide the rest.
fn pick_layer(vis: &mut Query<&mut Visibility>, layers: &[Entity], shown: usize) {
    for (i, &e) in layers.iter().enumerate() {
        if let Ok(mut v) = vis.get_mut(e) {
            set(
                &mut *v,
                if i == shown {
                    Visibility::Inherited
                } else {
                    Visibility::Hidden
                },
            );
        }
    }
}

/// Build the frames on the first frame of a run, then keep them current: the hero's health and
/// experience, and the target's name, level, health and portrait. A target gone or dead is dropped.
#[allow(clippy::too_many_arguments)]
fn show(
    mut commands: Commands,
    mut ui: ResMut<FramesUi>,
    mut target: ResMut<Target>,
    run: Res<Run>,
    selection: Res<Selection>,
    profile: Option<Res<Profile>>,
    font: Option<Res<HudFont>>,
    enemies: Query<&Enemy>,
    mut nodes: Query<&mut Node>,
    mut texts: Query<(&mut Text, &mut TextColor)>,
    mut vis: Query<&mut Visibility>,
) {
    let ui = &mut *ui;
    let Some(parts) = &ui.parts else {
        if let Some(font) = font {
            // Seated from the next frame on, once its nodes exist.
            build(&mut commands, ui, &font.0, &run);
        }
        return;
    };
    let stats = run.stats();
    let hp = (run.hp / stats.max_hp.max(1.0)).clamp(0.0, 1.0);
    let need = super::data::xp_to_next(run.level);
    let xp = (run.xp / need).clamp(0.0, 1.0);
    for (entity, share) in [(parts.hero.hp_fill, hp), (parts.hero.mp_fill, xp)] {
        if let Ok(mut node) = nodes.get_mut(entity) {
            set(&mut node.width, Val::Percent(share * 100.0));
        }
    }
    let hp_line = format!(
        "{} / {}",
        hud::grouped(run.hp.max(0.0).ceil() as u64),
        hud::grouped(stats.max_hp.round() as u64)
    );
    let xp_line = format!(
        "{} / {}",
        hud::grouped(run.xp.max(0.0) as u64),
        hud::grouped(need.max(0.0) as u64)
    );
    for (entity, value, color) in [
        (parts.hero.name, run.class.name.to_string(), GOLD_TONE),
        (parts.hero.level, run.level.to_string(), GOLD_TONE),
        (parts.hero.hp_text, hp_line, Color::WHITE),
        (parts.hero.mp_text, xp_line, Color::WHITE),
    ] {
        if let Ok((mut text, mut tc)) = texts.get_mut(entity) {
            set(&mut text.0, value);
            set(&mut tc.0, color);
        }
    }

    // The target, if it is still there and alive.
    let foe = target
        .0
        .and_then(|e| enemies.get(e).ok())
        .filter(|e| !e.dead);
    if foe.is_none() && target.0.is_some() {
        target.0 = None;
    }
    let f = &parts.foe;
    if let Ok(mut node) = nodes.get_mut(f.root) {
        set(
            &mut node.display,
            if foe.is_some() {
                Display::Flex
            } else {
                Display::None
            },
        );
    }
    let Some(e) = foe else { return };
    // The con colour against the class's character, as the nameplates colour it.
    let my_level = selection
        .class
        .zip(profile.as_deref())
        .map_or(1, |(class, p)| p.character(class).level);
    let [r, g, b, a] = con_color(my_level, e.def.level);
    let con = Color::srgba(r, g, b, a);
    // The portrait and the border follow the kind: a boss and an elite wear the gold dragon.
    let kind = if e.boss {
        2
    } else if e.def.big {
        1
    } else {
        0
    };
    pick_layer(&mut vis, &f.icons, kind);
    pick_layer(&mut vis, &[f.elite], if kind > 0 { 0 } else { 1 });
    if let Ok(mut node) = nodes.get_mut(f.hp_fill) {
        set(
            &mut node.width,
            Val::Percent((e.hp / e.max_hp.max(1.0)).clamp(0.0, 1.0) * 100.0),
        );
    }
    let foe_hp = format!(
        "{} / {}",
        hud::grouped(e.hp.max(0.0).ceil() as u64),
        hud::grouped(e.max_hp.ceil() as u64)
    );
    for (entity, value, color) in [
        (f.name, e.def.name.to_string(), GOLD_TONE),
        (f.level, e.def.level.to_string(), con),
        (f.hp_text, foe_hp, Color::WHITE),
    ] {
        if let Ok((mut text, mut tc)) = texts.get_mut(entity) {
            set(&mut text.0, value);
            set(&mut tc.0, color);
        }
    }
}

/// The diamond over the target's head, seated as the nameplates are, outside the UI scale.
#[allow(clippy::type_complexity)]
fn mark(
    ui: Res<FramesUi>,
    target: Res<Target>,
    ui_scale: Res<UiScale>,
    enemies: Query<(&Transform, &Enemy)>,
    cams: Query<(&Camera, &Transform), With<WorldCamera>>,
    anchor_q: (
        Query<&benilla_app::survivors_host::entities::BoneAttach>,
        Query<&benilla_world::rig_anim::RigPose>,
        Query<&benilla_app::survivors_host::entities::OverheadFallback>,
        Query<&GlobalTransform>,
        Query<(), With<benilla_app::survivors_host::entities::mount::MountChild>>,
    ),
    mut marker: Query<(&mut Node, &mut Visibility)>,
) {
    let Some(parts) = &ui.parts else { return };
    let Ok((mut node, mut vis)) = marker.get_mut(parts.marker) else {
        return;
    };
    let seat = (|| {
        let e = target.0?;
        let (tf, enemy) = enemies.get(e).ok()?;
        let (cam, cam_tf) = cams.single().ok()?;
        let viewport = cam.logical_viewport_size()?;
        let anchor = benilla_app::survivors_host::entities::overhead_anchor(
            e,
            tf,
            &anchor_q.0,
            &anchor_q.1,
            &anchor_q.2,
            &anchor_q.3,
            &anchor_q.4,
        );
        // Over the nameplate (a bar 0.67 above the anchor), and over a boss's name.
        let lift = if enemy.boss { 2.0 } else { 1.3 };
        benilla_app::survivors_host::ui_pass::project_overlay(
            cam,
            &GlobalTransform::from(*cam_tf),
            anchor + Vec3::Y * lift,
            viewport,
        )
    })();
    match seat {
        Some(screen) => {
            let s = ui_scale.0.max(0.01);
            node.left = Val::Px(screen.x / s - 5.5);
            node.top = Val::Px(screen.y / s - 11.0);
            set(&mut *vis, Visibility::Inherited);
        }
        None => set(&mut *vis, Visibility::Hidden),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A camera at the origin looking down +z.
    const EYE: Vec3 = Vec3::ZERO;
    const FWD: Vec3 = Vec3::Z;

    #[test]
    fn the_ray_picks_the_body_it_passes_through() {
        let foes = [
            (Vec3::new(5.0, -1.0, 10.0), 0.6),
            (Vec3::new(0.0, -1.0, 10.0), 0.6),
        ];
        assert_eq!(pick_index(EYE, FWD, None, &foes), Some(1));
    }

    #[test]
    fn of_two_bodies_on_the_ray_the_nearer_wins() {
        let foes = [
            (Vec3::new(0.0, -1.0, 20.0), 0.6),
            (Vec3::new(0.0, -1.0, 8.0), 0.6),
        ];
        assert_eq!(pick_index(EYE, FWD, None, &foes), Some(1));
    }

    #[test]
    fn a_miss_falls_back_to_the_ground_point() {
        let foes = [
            (Vec3::new(30.0, 0.0, 30.0), 0.6),
            (Vec3::new(10.0, 0.0, 50.0), 0.6),
        ];
        let aim = Some(Vec3::new(10.5, 0.0, 49.0));
        assert_eq!(pick_index(EYE, FWD, aim, &foes), Some(1));
        // Too far from the ground point: bare ground, nothing targeted.
        let far = Some(Vec3::new(10.0, 0.0, 40.0));
        assert_eq!(pick_index(EYE, FWD, far, &foes), None);
        assert_eq!(pick_index(EYE, FWD, None, &foes), None);
    }

    #[test]
    fn nothing_behind_the_camera_is_hit() {
        let foes = [(Vec3::new(0.0, -1.0, -10.0), 0.6)];
        assert_eq!(pick_index(EYE, FWD, None, &foes), None);
    }
}
