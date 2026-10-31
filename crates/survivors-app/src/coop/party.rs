//! The party frames: on the left, under this player's own frame and spells, one small frame per
//! other hero in the party, as WoW's `PartyMemberFrame` stacks them: the class in a round
//! portrait with the party's level under it, the name, a health bar with its numbers (grey and
//! "Dead" while the hero lies), and the hero's spells with their ranks.
//!
//! Drawn from what replicates for every hero ([`NetHero`], [`NetVitals`], [`NetBuild`]): on the
//! host from its guests' puppets, on a guest from the host's hero and the other guests'.

use bevy::prelude::*;

use super::client::OwnPuppet;
use super::wire::{NetBuild, NetHero, NetVitals};
use crate::hero::Hero;
use crate::hud::{HudFont, SpellIcon, UiArt, GOLD_TONE};
use crate::{data, NetRole, Phase, Run};

/// Under the player frame (100 tall) and its two rows of spell and passive icons.
const TOP: f32 = 196.0;
const LEFT: f32 = 6.0;
const W: f32 = 206.0;
const H: f32 = 58.0;
const GAP: f32 = 6.0;
const PORTRAIT: f32 = 42.0;
const BARS_X: f32 = 52.0;
const BAR_W: f32 = 148.0;
const BAR_H: f32 = 12.0;
const ICON: f32 = 18.0;
/// Spell icons a frame has room for; a hero has six at most outside the test modes.
const ICONS: usize = 7;
const Z: i32 = 401;
const PLATE: Color = Color::srgba(0.0, 0.0, 0.0, 0.55);
const HEALTH: Color = Color::srgb(0.0, 0.85, 0.0);
const FALLEN: Color = Color::srgb(0.45, 0.45, 0.45);
const CLASSES: &str = "Interface\\Glues\\CharacterCreate\\UI-CharacterCreate-Classes";
const STATUS_BAR: &str = "Interface\\TargetingFrame\\UI-StatusBar";
const WHOLE: [f32; 4] = [0.0, 1.0, 0.0, 1.0];

pub(super) fn plugin(app: &mut App) {
    app.init_resource::<PartyUi>()
        .add_systems(
            Update,
            show.run_if(in_run).run_if(|role: Res<NetRole>| *role != NetRole::Solo),
        )
        .init_resource::<ResultsUi>()
        .add_systems(OnEnter(Phase::Menu), (teardown, drop_results))
        .add_systems(OnEnter(Phase::GameOver), teardown)
        .add_systems(
            Update,
            results
                .run_if(in_state(Phase::GameOver))
                .run_if(|role: Res<NetRole>| *role != NetRole::Solo),
        );
}

/// The party's lines on the results screen, once built.
#[derive(Resource, Default)]
struct ResultsUi(Option<Entity>);

fn drop_results(mut commands: Commands, mut ui: ResMut<ResultsUi>) {
    if let Some(root) = ui.0.take() {
        commands.entity(root).try_despawn();
    }
}

/// One hero's line on the results: name, class, damage dealt, and whether it lay at the end.
fn result_line(hero: &NetHero, vitals: &NetVitals) -> String {
    let class = data::CLASSES
        .iter()
        .find(|c| c.id == hero.class_id)
        .map_or("", |c| c.name);
    let fell = if vitals.dead { "   (fell)" } else { "" };
    format!(
        "{}   {}   {} damage{}",
        hero.name,
        class,
        crate::hud::compact(vitals.damage),
        fell
    )
}

/// The results screen of a party run lists every hero, by player number, on the left.
fn results(
    mut commands: Commands,
    mut ui: ResMut<ResultsUi>,
    font: Option<Res<HudFont>>,
    heroes: Query<(&NetHero, &NetVitals)>,
) {
    if ui.0.is_some() {
        return;
    }
    let Some(font) = font else { return };
    let mut all: Vec<(&NetHero, &NetVitals)> = heroes.iter().collect();
    if all.len() < 2 {
        return;
    }
    all.sort_by_key(|(h, _)| h.player);
    let root = commands
        .spawn((
            Node {
                position_type: PositionType::Absolute,
                left: Val::Px(40.0),
                top: Val::Percent(38.0),
                flex_direction: FlexDirection::Column,
                row_gap: Val::Px(6.0),
                padding: UiRect::all(Val::Px(12.0)),
                border_radius: BorderRadius::all(Val::Px(6.0)),
                ..default()
            },
            BackgroundColor(Color::srgba(0.0, 0.0, 0.0, 0.6)),
            GlobalZIndex(Z + 100),
        ))
        .with_children(|col| {
            col.spawn((
                Text::new("The party"),
                TextFont {
                    font: font.0.clone(),
                    font_size: 18.0,
                    ..default()
                },
                TextColor(GOLD_TONE),
            ));
            for (hero, vitals) in &all {
                col.spawn((
                    Text::new(result_line(hero, vitals)),
                    TextFont {
                        font: font.0.clone(),
                        font_size: 14.0,
                        ..default()
                    },
                    TextColor(if vitals.dead {
                        Color::srgb(0.7, 0.7, 0.7)
                    } else {
                        Color::WHITE
                    }),
                ));
            }
        })
        .id();
    ui.0 = Some(root);
}

/// The phases that put the HUD on screen.
fn in_run(phase: Res<State<Phase>>) -> bool {
    matches!(phase.get(), Phase::Playing | Phase::LevelUp | Phase::Paused)
}

/// One member's frame: the nodes kept current every frame.
struct Member {
    name: Entity,
    level: Entity,
    hp_fill: Entity,
    hp_text: Entity,
    /// The rank labels over the spell icons, in the build's order.
    ranks: Vec<Entity>,
}

/// What the frames were built for: the members in order, with their class and spells. A change
/// builds them again.
type Shape = Vec<(Entity, u8, Vec<String>)>;

#[derive(Resource, Default)]
struct PartyUi {
    root: Option<Entity>,
    shape: Shape,
    members: Vec<Member>,
}

fn teardown(mut commands: Commands, mut ui: ResMut<PartyUi>) {
    if let Some(root) = ui.root.take() {
        commands.entity(root).try_despawn();
    }
    ui.shape.clear();
    ui.members.clear();
}

/// The other heroes, by player number: every replicated hero but this machine's own (its hero on
/// the host, its puppet on a guest).
#[allow(clippy::type_complexity)]
fn show(
    mut commands: Commands,
    mut ui: ResMut<PartyUi>,
    run: Res<Run>,
    font: Option<Res<HudFont>>,
    others: Query<
        (Entity, &NetHero, &NetVitals, &NetBuild),
        (Without<Hero>, Without<OwnPuppet>),
    >,
    mut nodes: Query<&mut Node>,
    mut texts: Query<(&mut Text, &mut TextColor)>,
    mut fills: Query<&mut ImageNode>,
) {
    let Some(font) = font else { return };
    let mut members: Vec<(Entity, &NetHero, &NetVitals, &NetBuild)> = others.iter().collect();
    members.sort_by_key(|(_, h, ..)| h.player);
    let shape: Shape = members
        .iter()
        .map(|(e, h, _, b)| (*e, h.class_id, b.abilities.iter().map(|a| a.0.clone()).collect()))
        .collect();
    let ui = &mut *ui;
    if ui.root.is_none() || shape != ui.shape {
        if let Some(root) = ui.root.take() {
            commands.entity(root).try_despawn();
        }
        let (root, built) = build(&mut commands, &font.0, &members);
        ui.root = Some(root);
        ui.members = built;
        ui.shape = shape;
        // Filled from the next frame on, once the nodes exist.
        return;
    }
    for (m, (_, hero, vitals, build)) in ui.members.iter().zip(&members) {
        let share = if vitals.max_hp > 0.0 {
            (vitals.hp / vitals.max_hp).clamp(0.0, 1.0)
        } else {
            0.0
        };
        if let Ok(mut node) = nodes.get_mut(m.hp_fill) {
            set(&mut node.width, Val::Percent(if vitals.dead { 100.0 } else { share * 100.0 }));
        }
        if let Ok(mut img) = fills.get_mut(m.hp_fill) {
            // The bar's art loads tinted; only a fall changes its colour.
            if img.color != Color::NONE {
                set(&mut img.color, if vitals.dead { FALLEN } else { HEALTH });
            }
        }
        let hp_line = if vitals.dead {
            "Dead".to_string()
        } else {
            format!("{} / {}", vitals.hp.max(0.0).ceil() as u64, vitals.max_hp.round() as u64)
        };
        let lines = [
            (m.name, hero.name.clone(), GOLD_TONE),
            (m.level, run.level.to_string(), GOLD_TONE),
            (m.hp_text, hp_line, Color::WHITE),
        ];
        for (entity, value, color) in lines {
            if let Ok((mut text, mut tc)) = texts.get_mut(entity) {
                set(&mut text.0, value);
                set(&mut tc.0, color);
            }
        }
        for (&rank, ability) in m.ranks.iter().zip(&build.abilities) {
            if let Ok((mut text, _)) = texts.get_mut(rank) {
                set(&mut text.0, ability.1.to_string());
            }
        }
    }
}

fn set<T: PartialEq>(slot: &mut T, v: T) {
    if *slot != v {
        *slot = v;
    }
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

/// A line of text centred in a `w` by `h` box at `x`,`y`; the text's entity.
fn label(
    parent: &mut ChildSpawnerCommands,
    font: &Handle<Font>,
    size: f32,
    color: Color,
    (x, y, w, h): (f32, f32, f32, f32),
) -> Entity {
    let mut id = Entity::PLACEHOLDER;
    parent
        .spawn(Node {
            justify_content: JustifyContent::Center,
            align_items: AlignItems::Center,
            overflow: Overflow::clip(),
            ..put(x, y, w, h)
        })
        .with_children(|slot| {
            id = slot.spawn(text(font, size, color)).id();
        });
    id
}

/// The frames, one per member from the top down.
fn build(
    commands: &mut Commands,
    font: &Handle<Font>,
    members: &[(Entity, &NetHero, &NetVitals, &NetBuild)],
) -> (Entity, Vec<Member>) {
    let mut built = Vec::with_capacity(members.len());
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
            for (i, (_, hero, _, build)) in members.iter().enumerate() {
                let top = TOP + i as f32 * (H + GAP);
                let mut m = Member {
                    name: Entity::PLACEHOLDER,
                    level: Entity::PLACEHOLDER,
                    hp_fill: Entity::PLACEHOLDER,
                    hp_text: Entity::PLACEHOLDER,
                    ranks: Vec::new(),
                };
                root.spawn(put(LEFT, top, W, H)).with_children(|f| {
                    f.spawn((
                        Node {
                            border_radius: BorderRadius::all(Val::Px(6.0)),
                            ..put(0.0, 0.0, W, H)
                        },
                        BackgroundColor(PLATE),
                    ));
                    // The class in a round portrait, the party's level in a disc under it.
                    let coords =
                        benilla_app::survivors_host::glue::art::class_tc(hero.class_id).unwrap_or(WHOLE);
                    f.spawn((
                        Node {
                            border_radius: BorderRadius::MAX,
                            ..put(4.0, 4.0, PORTRAIT, PORTRAIT)
                        },
                        BackgroundColor(Color::srgba(0.0, 0.0, 0.0, 0.6)),
                        ImageNode {
                            color: Color::NONE,
                            ..default()
                        },
                        UiArt::crop(CLASSES, coords),
                    ));
                    f.spawn((
                        Node {
                            border_radius: BorderRadius::MAX,
                            ..put(2.0, 36.0, 20.0, 20.0)
                        },
                        BackgroundColor(Color::srgba(0.0, 0.0, 0.0, 0.85)),
                    ));
                    m.level = label(f, font, 10.0, GOLD_TONE, (2.0, 36.0, 20.0, 20.0));
                    m.name = label(f, font, 12.0, GOLD_TONE, (BARS_X, 2.0, BAR_W, 14.0));
                    f.spawn((put(BARS_X, 18.0, BAR_W, BAR_H), BackgroundColor(Color::srgba(0.0, 0.0, 0.0, 0.7))))
                        .with_children(|seat| {
                            m.hp_fill = seat
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
                                    UiArt::tinted(STATUS_BAR, HEALTH),
                                ))
                                .id();
                        });
                    m.hp_text = label(f, font, 9.0, Color::WHITE, (BARS_X, 18.0, BAR_W, BAR_H));
                    // The spells, with their ranks in the corner.
                    for (k, (key, _, _)) in build.abilities.iter().take(ICONS).enumerate() {
                        let Some(def) = data::ABILITIES.iter().find(|a| a.key == *key) else {
                            continue;
                        };
                        let x = BARS_X + k as f32 * (ICON + 3.0);
                        f.spawn((
                            put(x, 35.0, ICON, ICON),
                            ImageNode {
                                color: Color::NONE,
                                ..default()
                            },
                            SpellIcon(def.spell),
                        ));
                        m.ranks.push(label(f, font, 8.0, Color::WHITE, (x + 8.0, 45.0, 10.0, 10.0)));
                    }
                });
                built.push(m);
            }
        })
        .id();
    (root, built)
}
