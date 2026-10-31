//! WoW unit frames: the hero's portrait frame at the top left and a target frame beside it; a
//! click on an enemy targets it, and the abilities favour the target ([`Target`], read by
//! [`super::abilities`]).
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
use crate::vplates::con_color;

const FRAME_W: f32 = 190.0;
const FRAME_H: f32 = 54.0;
const LEFT: f32 = 10.0;
const TOP: f32 = 10.0;
const GAP: f32 = 8.0;
const PAD: f32 = 5.0;
const PORTRAIT: f32 = 42.0;
const COLUMN_GAP: f32 = 6.0;
/// What the column beside the portrait has, so its bars run to the frame's edge.
const COLUMN_W: f32 = FRAME_W - 2.0 * PAD - 2.0 - PORTRAIT - COLUMN_GAP;
/// Level with the HUD (400), over it, under the character window and the level-up cards.
const Z: i32 = 401;
const FILL: Color = Color::srgba(0.02, 0.02, 0.03, 0.85);
const EDGE: Color = Color::srgba(0.8, 0.8, 0.8, 0.9);
const BAR_BACK: Color = Color::srgba(0.0, 0.0, 0.0, 0.6);
const HERO_BAR: Color = Color::srgb(0.1, 0.75, 0.1);
const FOE_BAR: Color = Color::srgb(0.8, 0.12, 0.12);
const XP_BAR: Color = Color::srgb(0.58, 0.0, 0.55);
const TAG: Color = Color::srgb(0.9, 0.9, 0.9);
const CLASSES: &str = "Interface\\Glues\\CharacterCreate\\UI-CharacterCreate-Classes";
/// A press and release this close (screen px) are a click, not a camera drag.
const CLICK_PX: f32 = 5.0;
/// How far from the cursor's ground point an enemy still counts as clicked (yd, besides its radius).
const AIM_YD: f32 = 1.5;

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

/// The nodes the frames update, once built.
struct Parts {
    root: Entity,
    name: Entity,
    level: Entity,
    hp_fill: Entity,
    hp_text: Entity,
    xp_fill: Entity,
    /// The target frame, its parts and the marker over the target's head.
    foe: Entity,
    foe_box: Entity,
    foe_name: Entity,
    foe_level: Entity,
    foe_tag: Entity,
    foe_fill: Entity,
    foe_text: Entity,
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
    )
}

/// A frame's outer panel at `left`.
fn panel(left: f32, shown: bool) -> impl Bundle {
    (
        Node {
            display: if shown { Display::Flex } else { Display::None },
            position_type: PositionType::Absolute,
            left: Val::Px(left),
            top: Val::Px(TOP),
            width: Val::Px(FRAME_W),
            height: Val::Px(FRAME_H),
            padding: UiRect::all(Val::Px(PAD)),
            column_gap: Val::Px(COLUMN_GAP),
            border: UiRect::all(Val::Px(1.0)),
            border_radius: BorderRadius::all(Val::Px(5.0)),
            ..default()
        },
        BackgroundColor(FILL),
        BorderColor::all(EDGE),
    )
}

/// The column beside the portrait.
fn column() -> impl Bundle {
    Node {
        width: Val::Px(COLUMN_W),
        flex_direction: FlexDirection::Column,
        justify_content: JustifyContent::Center,
        row_gap: Val::Px(2.0),
        ..default()
    }
}

/// A line with a name on the left and something small on the right.
fn row() -> impl Bundle {
    Node {
        width: Val::Percent(100.0),
        height: Val::Px(14.0),
        justify_content: JustifyContent::SpaceBetween,
        align_items: AlignItems::Center,
        ..default()
    }
}

/// A health bar with its numbers over the fill, or a thin one with none.
fn bar(height: f32) -> impl Bundle {
    (
        Node {
            width: Val::Percent(100.0),
            height: Val::Px(height),
            justify_content: JustifyContent::Center,
            align_items: AlignItems::Center,
            overflow: Overflow::clip(),
            ..default()
        },
        BackgroundColor(BAR_BACK),
    )
}

fn fill(color: Color) -> impl Bundle {
    (
        Node {
            position_type: PositionType::Absolute,
            left: Val::Px(0.0),
            top: Val::Px(0.0),
            height: Val::Percent(100.0),
            width: Val::Percent(100.0),
            ..default()
        },
        BackgroundColor(color),
    )
}

fn rgb(hex: u32) -> Color {
    Color::srgb_u8((hex >> 16) as u8, (hex >> 8) as u8, hex as u8)
}

fn build(commands: &mut Commands, ui: &mut FramesUi, font: &Handle<Font>, run: &Run) {
    let mut hero = [Entity::PLACEHOLDER; 5];
    let mut foe = [Entity::PLACEHOLDER; 8];
    let class = rgb(run.class.color);
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
            // The hero: the class icon, name and level, health, and the run's experience.
            root.spawn(panel(LEFT, true)).with_children(|p| {
                let coords =
                    crate::glue::art::class_tc(run.class.id).unwrap_or([0.0, 1.0, 0.0, 1.0]);
                p.spawn((
                    Node {
                        width: Val::Px(PORTRAIT),
                        height: Val::Px(PORTRAIT),
                        border: UiRect::all(Val::Px(1.0)),
                        flex_shrink: 0.0,
                        ..default()
                    },
                    BorderColor::all(class),
                    ImageNode {
                        color: Color::NONE,
                        ..default()
                    },
                    UiArt::crop(CLASSES, coords),
                ));
                p.spawn(column()).with_children(|c| {
                    c.spawn(row()).with_children(|r| {
                        hero[0] = r.spawn(text(font, 12.0, class)).id();
                        hero[1] = r.spawn(text(font, 12.0, GOLD_TONE)).id();
                    });
                    c.spawn(bar(15.0)).with_children(|b| {
                        hero[2] = b.spawn(fill(HERO_BAR)).id();
                        hero[3] = b.spawn(text(font, 11.0, Color::WHITE)).id();
                    });
                    c.spawn(bar(5.0)).with_children(|b| {
                        hero[4] = b.spawn(fill(XP_BAR)).id();
                    });
                });
            });
            // The target: its level in the portrait's seat, name and tag, health.
            let mut frame = root.spawn(panel(LEFT + FRAME_W + GAP, false));
            foe[0] = frame.id();
            frame.with_children(|p| {
                let mut seat = p.spawn((
                    Node {
                        width: Val::Px(PORTRAIT),
                        height: Val::Px(PORTRAIT),
                        border: UiRect::all(Val::Px(1.0)),
                        flex_shrink: 0.0,
                        justify_content: JustifyContent::Center,
                        align_items: AlignItems::Center,
                        ..default()
                    },
                    BackgroundColor(BAR_BACK),
                    BorderColor::all(EDGE),
                ));
                foe[1] = seat.id();
                seat.with_children(|s| {
                    foe[3] = s.spawn(text(font, 20.0, GOLD_TONE)).id();
                });
                p.spawn(column()).with_children(|c| {
                    c.spawn(row()).with_children(|r| {
                        foe[2] = r.spawn(text(font, 12.0, GOLD_TONE)).id();
                        foe[4] = r.spawn(text(font, 10.0, TAG)).id();
                    });
                    c.spawn(bar(15.0)).with_children(|b| {
                        foe[5] = b.spawn(fill(FOE_BAR)).id();
                        foe[6] = b.spawn(text(font, 11.0, Color::WHITE)).id();
                    });
                });
            });
            // A gold diamond over the target's head.
            foe[7] = root
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
    ui.parts = Some(Parts {
        root,
        name: hero[0],
        level: hero[1],
        hp_fill: hero[2],
        hp_text: hero[3],
        xp_fill: hero[4],
        foe: foe[0],
        foe_box: foe[1],
        foe_name: foe[2],
        foe_level: foe[3],
        foe_tag: foe[4],
        foe_fill: foe[5],
        foe_text: foe[6],
        marker: foe[7],
    });
}

fn set<T: PartialEq>(slot: &mut T, v: T) {
    if *slot != v {
        *slot = v;
    }
}

/// Build the frames on the first frame of a run, then keep them current: the hero's health and
/// experience, and the target's name, level, health and tag. A target gone or dead is dropped.
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
    mut borders: Query<&mut BorderColor>,
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
    for (entity, share) in [(parts.hp_fill, hp), (parts.xp_fill, xp)] {
        if let Ok(mut node) = nodes.get_mut(entity) {
            set(&mut node.width, Val::Percent(share * 100.0));
        }
    }
    let class = rgb(run.class.color);
    let hp_line = format!(
        "{} / {}",
        hud::grouped(run.hp.max(0.0).ceil() as u64),
        hud::grouped(stats.max_hp.round() as u64)
    );
    for (entity, value, color) in [
        (parts.name, run.class.name.to_string(), class),
        (parts.level, run.level.to_string(), GOLD_TONE),
        (parts.hp_text, hp_line, Color::WHITE),
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
    if let Ok(mut node) = nodes.get_mut(parts.foe) {
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
    let tag = if e.boss {
        "Boss"
    } else if e.def.big {
        "Elite"
    } else {
        ""
    };
    let edge = if e.boss { GOLD_TONE } else { EDGE };
    if let Ok(mut border) = borders.get_mut(parts.foe_box) {
        *border = BorderColor::all(edge);
    }
    if let Ok(mut node) = nodes.get_mut(parts.foe_fill) {
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
        (parts.foe_name, e.def.name.to_string(), GOLD_TONE),
        (parts.foe_level, e.def.level.to_string(), con),
        (
            parts.foe_tag,
            tag.to_string(),
            if e.boss { GOLD_TONE } else { TAG },
        ),
        (parts.foe_text, foe_hp, Color::WHITE),
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
        Query<&crate::entities::BoneAttach>,
        Query<&benilla_world::rig_anim::RigPose>,
        Query<&crate::entities::OverheadFallback>,
        Query<&GlobalTransform>,
        Query<(), With<crate::entities::mount::MountChild>>,
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
        let anchor = crate::entities::overhead_anchor(
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
        crate::ui_pass::project_overlay(
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
