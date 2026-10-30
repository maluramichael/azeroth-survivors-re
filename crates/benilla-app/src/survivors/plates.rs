//! Enemy health bars: the WoW nameplate (V toggles, as in 1.12) over the horde during a run. A
//! fixed pool of UI nodes is seated on the nearest enemies every frame and the bosses always get
//! one, so a few hundred enemies cost a bounded number of nodes. The level sits at the plate's
//! right end as in 1.12, coloured by the con against the class's character level.

use benilla_ui::script::nameplate;
use benilla_world::view::WorldCamera;
use bevy::prelude::*;

use super::enemies::{self, Enemy};
use super::hero::Hero;
use super::hud::{HudFont, GOLD_TONE};
use super::profile::Profile;
use super::{Phase, Selection};
use crate::vplates::{
    con_color, gx_px, plate_basis, text_px, BAR_H, BAR_OFF_X, BAR_OFF_Y, BAR_W, LEVEL_H,
    LEVEL_OFF_X, LEVEL_OFF_Y, PLATE_H, PLATE_W,
};

/// Bars at once on the horde, the nearest to the hero; bosses come on top of this.
const CAP: usize = 60;
/// A boss's bar and name are this much larger than a grunt's.
const BOSS_SCALE: f32 = 1.4;
/// The name's height in gx units, as the main client's plates size it.
const NAME_H: f32 = 0.01;
/// Above the HUD's root (400) so the hero's own plate and panels draw over the bars.
const Z: i32 = 395;
const HOSTILE: Color = Color::srgb(1.0, 0.0, 0.0);

pub(super) fn plugin(app: &mut App) {
    app.init_resource::<Plates>()
        .add_systems(
            Update,
            (toggle, drive)
                .chain()
                .after(crate::survivors::hero::place_camera)
                .after(enemies::move_enemies)
                .run_if(in_run),
        )
        .add_systems(OnEnter(Phase::Menu), teardown)
        .add_systems(OnEnter(Phase::GameOver), teardown);
}

/// The phases that put the HUD on screen.
fn in_run(phase: Res<State<Phase>>) -> bool {
    matches!(phase.get(), Phase::Playing | Phase::LevelUp | Phase::Paused)
}

/// The nodes of one bar.
struct Slot {
    frame: Entity,
    bar: Entity,
    fill: Entity,
    name_box: Entity,
    name: Entity,
    level_box: Entity,
    level: Entity,
}

#[derive(Resource)]
struct Plates {
    /// V: whether the bars show.
    shown: bool,
    /// The fill and border textures, once the archive can give them.
    images: Option<(Handle<Image>, Handle<Image>)>,
    root: Option<Entity>,
    slots: Vec<Slot>,
}

impl Default for Plates {
    fn default() -> Self {
        Self {
            shown: true,
            images: None,
            root: None,
            slots: Vec::new(),
        }
    }
}

/// One enemy that may get a bar this frame.
struct Cand {
    boss: bool,
    dist: f32,
    tf: Transform,
    entity: Entity,
    hp: f32,
    name: &'static str,
    level: u32,
}

/// One bar seated this frame.
struct Seat {
    screen: Vec2,
    boss: bool,
    hp: f32,
    name: &'static str,
    level: u32,
}

fn toggle(keys: Res<ButtonInput<KeyCode>>, mut plates: ResMut<Plates>) {
    if keys.just_pressed(KeyCode::KeyV) {
        plates.shown = !plates.shown;
    }
}

/// Back at the menu or the results: the pool goes, and the next run builds its own.
fn teardown(mut commands: Commands, mut plates: ResMut<Plates>) {
    if let Some(root) = plates.root.take() {
        commands.entity(root).try_despawn();
    }
    plates.slots.clear();
}

/// One bar's nodes under `root`: the fill under the border, a name seated on the plate's centre
/// line for a boss, and the level at the right end.
fn spawn_slot(
    commands: &mut Commands,
    root: Entity,
    font: &Handle<Font>,
    (fill_img, border_img): &(Handle<Image>, Handle<Image>),
) -> Slot {
    let absolute = || Node {
        position_type: PositionType::Absolute,
        ..default()
    };
    let full = || Node {
        width: Val::Percent(100.0),
        height: Val::Percent(100.0),
        ..absolute()
    };
    let text = |font_size: f32| {
        (
            Text::new(""),
            TextFont {
                font: font.clone(),
                font_size,
                ..default()
            },
            TextColor(GOLD_TONE),
            TextShadow {
                offset: Vec2::new(1.0, 1.0),
                color: Color::BLACK,
            },
            TextLayout::new(Justify::Center, LineBreak::NoWrap),
        )
    };
    let mut ids = [Entity::PLACEHOLDER; 7];
    commands.entity(root).with_children(|root| {
        let mut frame = root.spawn((absolute(), Visibility::Hidden));
        ids[0] = frame.id();
        frame.with_children(|plate| {
            let mut bar = plate.spawn(absolute());
            ids[1] = bar.id();
            bar.with_children(|bar| {
                ids[2] = bar
                    .spawn((
                        Node {
                            width: Val::Percent(100.0),
                            height: Val::Percent(100.0),
                            ..default()
                        },
                        ImageNode::new(fill_img.clone()).with_color(HOSTILE),
                    ))
                    .id();
            });
            plate.spawn((full(), ImageNode::new(border_img.clone())));
            let mut name_box = plate.spawn((
                Node {
                    justify_content: JustifyContent::Center,
                    align_items: AlignItems::FlexEnd,
                    overflow: Overflow::visible(),
                    ..absolute()
                },
                Visibility::Hidden,
            ));
            ids[3] = name_box.id();
            name_box.with_children(|b| {
                ids[4] = b.spawn(text(11.0)).id();
            });
            let mut level_box = plate.spawn(Node {
                justify_content: JustifyContent::Center,
                align_items: AlignItems::Center,
                overflow: Overflow::visible(),
                ..absolute()
            });
            ids[5] = level_box.id();
            level_box.with_children(|b| {
                ids[6] = b.spawn(text(10.0)).id();
            });
        });
    });
    let [frame, bar, fill, name_box, name, level_box, level] = ids;
    Slot {
        frame,
        bar,
        fill,
        name_box,
        name,
        level_box,
        level,
    }
}

fn set<T: PartialEq>(slot: &mut T, v: T) {
    if *slot != v {
        *slot = v;
    }
}

/// Seat a bar on each enemy that deserves one and hide the rest of the pool.
#[allow(clippy::too_many_arguments, clippy::type_complexity)]
fn drive(
    mut commands: Commands,
    mut plates: ResMut<Plates>,
    ui_scale: Res<UiScale>,
    font: Option<Res<HudFont>>,
    assets: Option<ResMut<benilla_assets::WorldAssets>>,
    mut images: ResMut<Assets<Image>>,
    hero: Query<&Transform, With<Hero>>,
    (selection, profile): (Res<Selection>, Option<Res<Profile>>),
    cams: Query<(&Camera, &Transform), With<WorldCamera>>,
    enemies: Query<(Entity, &Enemy, &Transform)>,
    anchor_q: (
        Query<&crate::entities::BoneAttach>,
        Query<&benilla_world::rig_anim::RigPose>,
        Query<&crate::entities::OverheadFallback>,
        Query<&GlobalTransform>,
        Query<(), With<crate::entities::mount::MountChild>>,
    ),
    mut nodes: Query<(&mut Node, &mut Visibility)>,
    mut texts: Query<(&mut Text, &mut TextFont, &mut TextColor)>,
    mut cands: Local<Vec<Cand>>,
) {
    let plates = &mut *plates;
    let mut seats: Vec<Seat> = Vec::new();
    // The con compares against the class's character, the level that also opens the zones.
    let my_level = selection
        .class
        .zip(profile.as_deref())
        .map_or(1, |(class, p)| p.character(class).level);
    let view = cams.single().ok().and_then(|(cam, tf)| {
        Some((
            cam,
            GlobalTransform::from(*tf),
            cam.logical_viewport_size()?,
        ))
    });
    if plates.shown {
        if let (Ok(h), Some((cam, cam_tf, viewport))) = (hero.single(), view.as_ref()) {
            cands.clear();
            for (entity, e, tf) in &enemies {
                if e.dead {
                    continue;
                }
                cands.push(Cand {
                    boss: e.boss,
                    dist: tf.translation.distance_squared(h.translation),
                    tf: *tf,
                    entity,
                    hp: (e.hp / e.max_hp.max(1.0)).clamp(0.0, 1.0),
                    name: e.def.name,
                    level: e.def.level,
                });
            }
            // Bosses first, then the nearest; the horde stops at the cap.
            cands.sort_unstable_by(|a, b| b.boss.cmp(&a.boss).then(a.dist.total_cmp(&b.dist)));
            let mut horde = 0;
            for c in cands.iter() {
                if !c.boss {
                    if horde >= CAP {
                        break;
                    }
                    horde += 1;
                }
                let anchor = crate::entities::overhead_anchor(
                    c.entity,
                    &c.tf,
                    &anchor_q.0,
                    &anchor_q.1,
                    &anchor_q.2,
                    &anchor_q.3,
                    &anchor_q.4,
                );
                // Off screen or behind the camera: no bar.
                let Some(screen) = crate::ui_pass::project_overlay(
                    cam,
                    cam_tf,
                    anchor + Vec3::Y * 0.67,
                    *viewport,
                ) else {
                    continue;
                };
                seats.push(Seat {
                    screen,
                    boss: c.boss,
                    hp: c.hp,
                    name: c.name,
                    level: c.level,
                });
            }
        }
    }

    if !seats.is_empty() {
        // The pool is built on first need, and grown to this frame's count; a new slot is
        // seated from the next frame on, once its nodes exist.
        if plates.images.is_none() {
            if let Some(mut assets) = assets {
                let fill = assets.sprite_texture(nameplate::BAR_FILL_TEXTURE, &mut images);
                let border = assets.sprite_texture(nameplate::BORDER_TEXTURE, &mut images);
                plates.images = fill.zip(border);
            }
        }
        if let (Some(art), Some(font)) = (plates.images.clone(), font) {
            let root = *plates.root.get_or_insert_with(|| {
                commands
                    .spawn((
                        Node {
                            position_type: PositionType::Absolute,
                            width: Val::Percent(100.0),
                            height: Val::Percent(100.0),
                            ..default()
                        },
                        GlobalZIndex(Z),
                    ))
                    .id()
            });
            while plates.slots.len() < seats.len() {
                let slot = spawn_slot(&mut commands, root, &font.0, &art);
                plates.slots.push(slot);
            }
        }
    }

    let (basis, s) = match view {
        Some((_, _, viewport)) => (plate_basis(viewport), ui_scale.0),
        None => (0.0, 1.0),
    };
    for (i, slot) in plates.slots.iter().enumerate() {
        let Ok((mut frame, mut vis)) = nodes.get_mut(slot.frame) else {
            continue;
        };
        let Some(&Seat {
            screen,
            boss,
            hp,
            name,
            level,
        }) = seats.get(i)
        else {
            set(&mut *vis, Visibility::Hidden);
            continue;
        };
        set(&mut *vis, Visibility::Inherited);
        let k = if boss { BOSS_SCALE } else { 1.0 };
        let gx = |v: f32| gx_px(v * k, basis) / s;
        let (pw, ph) = (gx(PLATE_W), gx(PLATE_H));
        let bar_top = ph - gx(BAR_OFF_Y) - gx(BAR_H);
        // The bar's top edge sits on the projected point, so the name rises above it.
        set(&mut frame.width, Val::Px(pw));
        set(&mut frame.height, Val::Px(ph));
        frame.left = Val::Px(screen.x / s - pw / 2.0);
        frame.top = Val::Px(screen.y / s - bar_top);
        if let Ok((mut bar, _)) = nodes.get_mut(slot.bar) {
            set(&mut bar.left, Val::Px(gx(BAR_OFF_X)));
            set(&mut bar.top, Val::Px(bar_top));
            set(&mut bar.width, Val::Px(gx(BAR_W)));
            set(&mut bar.height, Val::Px(gx(BAR_H)));
        }
        if let Ok((mut fill, _)) = nodes.get_mut(slot.fill) {
            set(&mut fill.width, Val::Percent(hp * 100.0));
        }
        if let Ok((mut b, mut bv)) = nodes.get_mut(slot.name_box) {
            set(&mut b.width, Val::Px(pw));
            set(&mut b.height, Val::Px(ph / 2.0));
            set(
                &mut *bv,
                if boss {
                    Visibility::Inherited
                } else {
                    Visibility::Hidden
                },
            );
        }
        // The level's centre at the plate's bottom right, pulled in and up by the client's offsets.
        let level_h = text_px(LEVEL_H * k, basis) / s;
        if let Ok((mut b, _)) = nodes.get_mut(slot.level_box) {
            let w = level_h * 3.0;
            set(&mut b.width, Val::Px(w));
            set(&mut b.height, Val::Px(level_h));
            set(&mut b.left, Val::Px(pw - gx(LEVEL_OFF_X) - w / 2.0));
            set(&mut b.top, Val::Px(ph - gx(LEVEL_OFF_Y) - level_h / 2.0));
        }
        if let Ok((mut text, mut tf, mut color)) = texts.get_mut(slot.level) {
            let label = level.to_string();
            if text.0 != label {
                text.0 = label;
            }
            set(&mut tf.font_size, level_h);
            let [r, g, b, a] = con_color(my_level, level);
            set(&mut color.0, Color::srgba(r, g, b, a));
        }
        if boss {
            if let Ok((mut text, mut tf, _)) = texts.get_mut(slot.name) {
                if text.0 != name {
                    text.0 = name.to_string();
                }
                set(&mut tf.font_size, text_px(NAME_H * k, basis) / s);
            }
        }
    }
}
