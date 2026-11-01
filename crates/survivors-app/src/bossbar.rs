//! The boss bar: a wide health bar at the top centre, under the clock and the surge line, for every
//! boss on the field. Up to [`MAX_BARS`] stack, the newest bosses; a fallen boss's bar fades out
//! over [`FADE_SECS`]. Each bar wears the casting bar's border (`UI-CastingBar-Border`) around a
//! red `UI-StatusBar` fill, with the boss's name on the left and "health / max (percent)" on the
//! right. A boss far above the hero's class level shows the skull beside it; a line over the bar
//! says "Final boss", "Enraged", "Rare" or "Unbeatable". A [`super::enemies::Rare`] has a silver
//! bar. Hidden under the level-up cards.
//!
//! On arrival the raid-warning horn sounds once and every boss gets its
//! [`super::minimap::MapMarker`] (drawn by the minimap). The bar only draws: it reads the
//! [`Enemy`] components, which a co-op guest has too, so it runs on a guest as well.
//!
//! Shape after Dean's `bossbar.rs` (`deanmathieson/azeroth-survivors-re`, branch `progression`,
//! `10c6a77`, MIT OR Apache-2.0): the newest-two rule, the fade and the whole-percent rounding.
//! The art, the kinds, the skull rule and the slide-in are this fork's.

use std::collections::HashMap;

use benilla_app::survivors_host::sound::GlueSound;
use bevy::prelude::*;
use bevy::ui::{FocusPolicy, Val2};

use super::enemies::{Enemy, Rare};
use super::enrage::Enraged;
use super::finale::{self, Finale};
use super::hud::{self, ArtDone, HudFont, UiArt};
use super::kit;
use super::minimap::MapMarker;
use super::profile::Profile;
use super::{Phase, Selection};

/// Bars shown at once; more bosses alive show the newest.
pub(super) const MAX_BARS: usize = 2;
/// Seconds a fallen boss's bar takes to fade.
pub(super) const FADE_SECS: f32 = 1.5;
/// A new bar fades in over this many seconds and slides down from [`SLIDE_PX`] above its seat
/// over [`SLIDE_SECS`].
const FADE_IN_SECS: f32 = 0.3;
const SLIDE_SECS: f32 = 0.5;
const SLIDE_PX: f32 = 34.0;
/// A boss this many levels above the hero's class level wears the skull.
const SKULL_LEVELS: u32 = 10;
/// A boss with this much health cannot be killed (the reaper's 1e9): its numbers are not shown.
const UNBEATABLE_HP: f32 = 1.0e8;

/// Under the clock (about 50), its line of counts and the surge line (88, one line tall), on the
/// 768-unit canvas.
const TOP: f32 = 108.0;
/// Level with the HUD, over the unit frames (all 401), under the pause (410) and the cards (420).
const Z: i32 = 401;
/// The casting bar is 195x13 inside a 256x64 border texture; the bar here is that scaled up.
const BAR_W: f32 = 320.0;
const SCALE: f32 = BAR_W / 195.0;
const BAR_H: f32 = 13.0 * SCALE;
const FRAME_W: f32 = 256.0 * SCALE;
const FRAME_H: f32 = 64.0 * SCALE;
/// The bar sits at (30.5, 25.5) in the border texture.
const FRAME_X: f32 = 30.5 * SCALE;
const FRAME_Y: f32 = 25.5 * SCALE;
/// The border's rim reaches about this far over and under the bar.
const RIM: f32 = 12.0;
/// The line over the bar, and the space between two bars (the lower rim and a little air).
const TAG_H: f32 = 12.0;
const GAP: f32 = RIM + 4.0;
const SKULL: f32 = 26.0;
const BACKDROP: Color = Color::srgba(0.0, 0.0, 0.0, 0.7);

const BORDER_ART: &str = "Interface\\CastingBar\\UI-CastingBar-Border";
const FILL_ART: &str = "Interface\\TargetingFrame\\UI-StatusBar";
const SKULL_ART: &str = "Interface\\TargetingFrame\\UI-TargetingFrame-Skull";
const WHOLE: [f32; 4] = [0.0, 1.0, 0.0, 1.0];

const FILL_BOSS: Color = Color::srgb(0.78, 0.07, 0.05);
const FILL_ENRAGED: Color = Color::srgb(1.0, 0.32, 0.0);
const FILL_RARE: Color = Color::srgb(0.70, 0.74, 0.82);
const FILL_UNBEATABLE: Color = Color::srgb(0.50, 0.15, 0.75);
const TAG_FINAL: Color = Color::srgb(1.0, 0.5, 0.1);
const TAG_ENRAGED: Color = Color::srgb(1.0, 0.25, 0.15);
const TAG_RARE: Color = Color::srgb(0.78, 0.82, 0.9);

/// What a bar says about its boss.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum BossKind {
    /// A clock boss, or any other boss.
    Boss,
    /// The zone's finale: its death wins the run.
    Final,
    /// A named rare ([`Rare`]).
    Rare,
    /// The reaper: health beyond reach.
    Unbeatable,
}

/// A boss on the field, as the bar reads it.
#[derive(Clone, Debug, PartialEq)]
pub(super) struct BossInfo {
    /// The order the bosses were first seen in: the higher, the newer.
    pub seq: u64,
    pub name: &'static str,
    pub hp: f32,
    pub max_hp: f32,
    pub kind: BossKind,
    pub enraged: bool,
    /// Far above the hero's class level.
    pub skull: bool,
    /// Seconds since the bar first showed it.
    pub age: f32,
    /// Seconds since it fell; `None` while it stands.
    pub dead_for: Option<f32>,
}

/// One bar's contents.
#[derive(Clone, Debug, PartialEq)]
pub(super) struct Bar {
    pub name: &'static str,
    pub skull: bool,
    /// The fill, 0 to 1.
    pub fill: f32,
    /// "current / max (percent%)".
    pub health: String,
    /// The line over the bar: "Final boss", "Enraged", ... or empty.
    pub tag: String,
    /// Fade in times fade out, 0 to 1.
    pub alpha: f32,
    /// How far above its seat the bar still is while it slides in, in UI units (0 or negative).
    pub lift: f32,
    pub fill_tint: Color,
    pub tag_tone: Color,
    pub name_tone: Color,
}

/// Health as a whole percentage: never 0 while standing, nor 100 once scratched.
pub(super) fn percent(hp: f32, max_hp: f32) -> u32 {
    if max_hp <= 0.0 || hp <= 0.0 {
        return 0;
    }
    let p = (hp / max_hp * 100.0).clamp(0.0, 100.0);
    if p >= 100.0 {
        100
    } else {
        (p.floor() as u32).clamp(1, 99)
    }
}

/// A health figure: grouped by thousands, short ("1.2M") from a million up.
fn count(v: f32) -> String {
    if v >= 1.0e6 {
        hud::compact(f64::from(v))
    } else {
        hud::grouped(v.max(0.0).ceil() as u64)
    }
}

/// The right-hand text of a bar. The reaper's health is no number worth showing.
fn health_text(hp: f32, max_hp: f32, kind: BossKind) -> String {
    if kind == BossKind::Unbeatable {
        return "???".to_string();
    }
    format!(
        "{} / {} ({}%)",
        count(hp),
        count(max_hp.max(1.0)),
        percent(hp, max_hp)
    )
}

/// The line over the bar.
fn tag_text(kind: BossKind, enraged: bool) -> String {
    let mut parts = Vec::new();
    match kind {
        BossKind::Final => parts.push("Final boss"),
        BossKind::Unbeatable => parts.push("Unbeatable"),
        BossKind::Rare => parts.push("Rare"),
        BossKind::Boss => {}
    }
    if enraged {
        parts.push("Enraged");
    }
    parts.join(", ")
}

/// The fill's colour: red, hotter once enraged, silver for a rare.
fn fill_tint(kind: BossKind, enraged: bool) -> Color {
    match kind {
        BossKind::Rare => FILL_RARE,
        BossKind::Unbeatable => FILL_UNBEATABLE,
        _ if enraged => FILL_ENRAGED,
        _ => FILL_BOSS,
    }
}

fn tag_tone(kind: BossKind, enraged: bool) -> Color {
    if enraged {
        TAG_ENRAGED
    } else if kind == BossKind::Rare {
        TAG_RARE
    } else {
        TAG_FINAL
    }
}

/// What kind of boss this is. The reaper is known by its health, so a co-op guest (who has no
/// marker components) tells it too.
fn kind_of(rare: bool, finale: bool, max_hp: f32) -> BossKind {
    if max_hp >= UNBEATABLE_HP {
        BossKind::Unbeatable
    } else if rare {
        BossKind::Rare
    } else if finale {
        BossKind::Final
    } else {
        BossKind::Boss
    }
}

/// Whether a boss of `boss_level` wears the skull against a hero of class level `my_level`.
fn skull(my_level: u32, boss_level: u32) -> bool {
    boss_level >= my_level.saturating_add(SKULL_LEVELS)
}

/// Ease out: fast at first, settling.
fn ease_out(t: f32) -> f32 {
    let t = t.clamp(0.0, 1.0);
    1.0 - (1.0 - t) * (1.0 - t)
}

fn bar(b: &BossInfo) -> Bar {
    let hp = if b.dead_for.is_some() {
        0.0
    } else {
        b.hp.max(0.0)
    };
    let max = b.max_hp.max(1.0);
    let fade_out = b
        .dead_for
        .map_or(1.0, |t| (1.0 - t / FADE_SECS).clamp(0.0, 1.0));
    let fade_in = (b.age / FADE_IN_SECS).clamp(0.0, 1.0);
    Bar {
        name: b.name,
        skull: b.skull,
        fill: (hp / max).clamp(0.0, 1.0),
        health: health_text(hp, max, b.kind),
        tag: tag_text(b.kind, b.enraged),
        alpha: fade_in * fade_out,
        lift: -(1.0 - ease_out(b.age / SLIDE_SECS)) * SLIDE_PX,
        fill_tint: fill_tint(b.kind, b.enraged),
        tag_tone: tag_tone(b.kind, b.enraged),
        name_tone: if b.kind == BossKind::Rare {
            TAG_RARE
        } else {
            hud::GOLD_TONE
        },
    }
}

/// The bars for `bosses`: those standing and those still fading, the [`MAX_BARS`] newest, the
/// older one on top.
pub(super) fn bars(bosses: &[BossInfo]) -> Vec<Bar> {
    let mut shown: Vec<&BossInfo> = bosses
        .iter()
        .filter(|b| b.dead_for.is_none_or(|t| t < FADE_SECS))
        .collect();
    shown.sort_by_key(|b| b.seq);
    let skip = shown.len().saturating_sub(MAX_BARS);
    shown.into_iter().skip(skip).map(bar).collect()
}

#[derive(Component)]
struct BossBarRoot;
/// Bar `i`'s own root.
#[derive(Component)]
struct BarSlot(usize);
#[derive(Component, Clone, Copy, PartialEq, Eq)]
enum Part {
    /// The line over the bar.
    Tag,
    Name,
    Health,
    /// The dark box the fill sits in.
    Back,
    Fill,
    /// The casting bar's border art.
    Frame,
    Skull,
}
#[derive(Component)]
struct BarPart(usize, Part);

/// The boss bar's systems, registered by [`super::SurvivorsPlugin`]. Nothing here simulates, so
/// all of it runs on a co-op guest.
pub(super) fn plugin(app: &mut App) {
    app.add_systems(Startup, setup.after(hud::setup_hud))
        .add_systems(
            Update,
            (arrive.run_if(in_run), refresh.after(hud::show_panels), make_room),
        );
}

/// The phases that put the HUD on screen.
fn in_run(phase: Res<State<Phase>>) -> bool {
    matches!(phase.get(), Phase::Playing | Phase::LevelUp | Phase::Paused)
}

fn setup(mut commands: Commands, font: Res<HudFont>) {
    let f = &font.0;
    commands
        .spawn((
            BossBarRoot,
            Node {
                position_type: PositionType::Absolute,
                top: Val::Px(TOP),
                width: Val::Percent(100.0),
                flex_direction: FlexDirection::Column,
                align_items: AlignItems::Center,
                row_gap: Val::Px(GAP),
                ..default()
            },
            GlobalZIndex(Z),
            Visibility::Hidden,
            FocusPolicy::Pass,
        ))
        .with_children(|root| {
            for i in 0..MAX_BARS {
                root.spawn((
                    BarSlot(i),
                    Node {
                        flex_direction: FlexDirection::Column,
                        align_items: AlignItems::Center,
                        display: Display::None,
                        ..default()
                    },
                    UiTransform::IDENTITY,
                ))
                .with_children(|slot| {
                    slot.spawn((
                        BarPart(i, Part::Tag),
                        Node {
                            height: Val::Px(TAG_H),
                            ..default()
                        },
                        kit::text(f, "", 11.0, TAG_FINAL),
                    ));
                    slot.spawn((
                        BarPart(i, Part::Back),
                        Node {
                            width: Val::Px(BAR_W),
                            height: Val::Px(BAR_H),
                            margin: UiRect::top(Val::Px(RIM - 2.0)),
                            ..default()
                        },
                        BackgroundColor(BACKDROP),
                    ))
                    .with_children(|bar| {
                        bar.spawn((
                            BarPart(i, Part::Fill),
                            Node {
                                position_type: PositionType::Absolute,
                                left: Val::Px(0.0),
                                top: Val::Px(0.0),
                                width: Val::Percent(100.0),
                                height: Val::Percent(100.0),
                                ..default()
                            },
                            ImageNode {
                                color: Color::NONE,
                                ..default()
                            },
                            UiArt::tinted(FILL_ART, FILL_BOSS),
                        ));
                        // The border over the fill, its window on the bar.
                        bar.spawn((
                            BarPart(i, Part::Frame),
                            Node {
                                position_type: PositionType::Absolute,
                                left: Val::Px(-FRAME_X),
                                top: Val::Px(-FRAME_Y),
                                width: Val::Px(FRAME_W),
                                height: Val::Px(FRAME_H),
                                ..default()
                            },
                            ImageNode {
                                color: Color::NONE,
                                ..default()
                            },
                            UiArt::crop(BORDER_ART, WHOLE),
                        ));
                        // The skull stands outside the left end of the border.
                        bar.spawn((
                            BarPart(i, Part::Skull),
                            Node {
                                position_type: PositionType::Absolute,
                                left: Val::Px(-(SKULL + 14.0)),
                                top: Val::Px((BAR_H - SKULL) / 2.0),
                                width: Val::Px(SKULL),
                                height: Val::Px(SKULL),
                                display: Display::None,
                                ..default()
                            },
                            ImageNode {
                                color: Color::NONE,
                                ..default()
                            },
                            UiArt::crop(SKULL_ART, WHOLE),
                        ));
                        // The texts, over the border.
                        bar.spawn(Node {
                            position_type: PositionType::Absolute,
                            left: Val::Px(0.0),
                            top: Val::Px(0.0),
                            width: Val::Percent(100.0),
                            height: Val::Percent(100.0),
                            padding: UiRect::horizontal(Val::Px(12.0)),
                            justify_content: JustifyContent::SpaceBetween,
                            align_items: AlignItems::Center,
                            ..default()
                        })
                        .with_children(|row| {
                            row.spawn((
                                BarPart(i, Part::Name),
                                kit::text(f, "", 13.0, hud::GOLD_TONE),
                            ));
                            row.spawn((
                                BarPart(i, Part::Health),
                                kit::text(f, "", 12.0, Color::WHITE),
                            ));
                        });
                    });
                });
            }
        });
}

/// A boss arrives: the raid-warning horn once, and the boss's mark on the minimap (a rare's
/// own). A marker that is already there, put on by the module that spawned the boss, stays.
fn arrive(
    mut commands: Commands,
    fresh: Query<(Entity, &Enemy, Has<Rare>), (Added<Enemy>, Without<MapMarker>)>,
    mut sound: MessageWriter<GlueSound>,
) {
    let mut horn = false;
    for (e, enemy, rare) in &fresh {
        if !enemy.boss {
            continue;
        }
        commands.entity(e).try_insert(if rare {
            MapMarker::Rare
        } else {
            MapMarker::Boss
        });
        horn = true;
    }
    if horn {
        sound.write(GlueSound("RaidWarning"));
    }
}

/// The bosses the bar knows, by entity. A fallen boss's entry outlives its corpse until its bar
/// has faded.
#[derive(Default)]
struct Seen {
    next: u64,
    by: HashMap<Entity, BossInfo>,
}

/// Read the bosses off the field, then keep the bars current. Nodes and texts are only touched
/// when they change.
#[allow(clippy::too_many_arguments, clippy::type_complexity)] // one Bevy system's full input set
/// The world-buff offer sits where the bar does: while it is up, the bar moves under it.
fn make_room(
    panels: Query<(), With<super::worldbuffs::BuffPanel>>,
    mut root: Query<&mut Node, With<BossBarRoot>>,
) {
    let want = if panels.is_empty() {
        TOP
    } else {
        super::worldbuffs::PANEL_BOTTOM + GAP
    };
    if let Ok(mut node) = root.single_mut() {
        if node.top != Val::Px(want) {
            node.top = Val::Px(want);
        }
    }
}

fn refresh(
    phase: Res<State<Phase>>,
    selection: Res<Selection>,
    profile: Option<Res<Profile>>,
    time: Res<Time>,
    enemies: Query<(Entity, &Enemy, Has<Rare>, Has<Finale>, Has<Enraged>)>,
    mut seen: Local<Seen>,
    mut root: Query<&mut Visibility, With<BossBarRoot>>,
    mut slots: Query<(&BarSlot, &mut Node, &mut UiTransform), Without<BarPart>>,
    mut nodes: Query<(&BarPart, &mut Node), Without<BarSlot>>,
    mut texts: Query<(&BarPart, &mut Text, &mut TextColor)>,
    mut images: Query<(&BarPart, &mut ImageNode, Has<ArtDone>)>,
    mut backs: Query<(&BarPart, &mut BackgroundColor)>,
) {
    let p = *phase.get();
    // In a run, and never over the level-up cards.
    let on = matches!(p, Phase::Playing | Phase::Paused);
    // Compare before writing: a write marks the node changed, which re-lays the UI out.
    if let Ok(mut v) = root.single_mut() {
        let want = if on {
            Visibility::Inherited
        } else {
            Visibility::Hidden
        };
        if *v != want {
            *v = want;
        }
    }
    if !on {
        if matches!(p, Phase::Menu | Phase::GameOver) {
            *seen = Seen::default();
        }
        return;
    }

    let seen = &mut *seen;
    let dt = time.delta_secs();
    let my_level = selection
        .class
        .zip(profile.as_deref())
        .map_or(1, |(class, p)| p.character(class).level);
    // A guest has no `Finale` component: the finale is told by name as well.
    let final_name = finale::plan(selection.map).finale.name;
    let mut alive = Vec::new();
    for (e, enemy, rare, is_finale, enraged) in &enemies {
        if !enemy.boss {
            continue;
        }
        alive.push(e);
        let next = &mut seen.next;
        let info = seen.by.entry(e).or_insert_with(|| {
            *next += 1;
            BossInfo {
                seq: *next,
                name: enemy.def.name,
                hp: 0.0,
                max_hp: 1.0,
                kind: BossKind::Boss,
                enraged: false,
                skull: false,
                age: 0.0,
                dead_for: None,
            }
        });
        info.name = enemy.def.name;
        info.hp = enemy.hp;
        info.max_hp = enemy.max_hp;
        info.kind = kind_of(
            rare,
            is_finale || enemy.def.name == final_name,
            enemy.max_hp,
        );
        info.enraged = enraged;
        info.skull = skull(my_level, enemy.def.level);
        if enemy.dead && info.dead_for.is_none() {
            info.dead_for = Some(0.0);
        }
    }
    for info in seen.by.values_mut() {
        info.age += dt;
        if let Some(t) = &mut info.dead_for {
            *t += dt;
        }
    }
    // A boss that left the field alive is gone; a fallen one keeps its bar until it has faded.
    seen.by
        .retain(|e, b| alive.contains(e) || b.dead_for.is_some_and(|t| t < FADE_SECS));
    let infos: Vec<BossInfo> = seen.by.values().cloned().collect();
    let shown = bars(&infos);

    for (slot, mut node, mut tf) in &mut slots {
        let bar = shown.get(slot.0);
        let display = if bar.is_some() {
            Display::Flex
        } else {
            Display::None
        };
        if node.display != display {
            node.display = display;
        }
        if let Some(bar) = bar {
            let lift = Val2::px(0.0, bar.lift);
            if tf.translation != lift {
                tf.translation = lift;
            }
        }
    }
    for (part, mut node) in &mut nodes {
        let Some(bar) = shown.get(part.0) else {
            continue;
        };
        match part.1 {
            Part::Fill => {
                let width = Val::Percent(bar.fill * 100.0);
                if node.width != width {
                    node.width = width;
                }
            }
            Part::Skull => {
                let display = if bar.skull {
                    Display::Flex
                } else {
                    Display::None
                };
                if node.display != display {
                    node.display = display;
                }
            }
            _ => {}
        }
    }
    for (part, mut text, mut color) in &mut texts {
        let Some(bar) = shown.get(part.0) else {
            continue;
        };
        let (want, tone) = match part.1 {
            Part::Tag => (bar.tag.clone(), bar.tag_tone),
            Part::Name => (bar.name.to_string(), bar.name_tone),
            Part::Health => (bar.health.clone(), Color::WHITE),
            _ => continue,
        };
        if text.0 != want {
            text.0 = want;
        }
        let tone = tone.with_alpha(bar.alpha);
        if color.0 != tone {
            color.0 = tone;
        }
    }
    for (part, mut img, ready) in &mut images {
        let Some(bar) = shown.get(part.0) else {
            continue;
        };
        // Until the art is read its colour stays clear.
        if !ready {
            continue;
        }
        let tint = if part.1 == Part::Fill {
            bar.fill_tint
        } else {
            Color::WHITE
        };
        let want = tint.with_alpha(bar.alpha);
        if img.color != want {
            img.color = want;
        }
    }
    for (part, mut bg) in &mut backs {
        if part.1 != Part::Back {
            continue;
        }
        let Some(bar) = shown.get(part.0) else {
            continue;
        };
        let want = BACKDROP.with_alpha(BACKDROP.alpha() * bar.alpha);
        if bg.0 != want {
            bg.0 = want;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const NAMES: [&str; 4] = ["Hogger", "Gath'Ilzogg", "Morgaine", "Van Cleef"];

    fn boss(seq: u64, hp: f32, dead_for: Option<f32>) -> BossInfo {
        BossInfo {
            seq,
            name: NAMES[seq as usize % NAMES.len()],
            hp,
            max_hp: 1000.0,
            kind: BossKind::Boss,
            enraged: false,
            skull: false,
            age: 10.0,
            dead_for,
        }
    }

    #[test]
    fn percent_text_rounds_down() {
        assert_eq!(percent(1000.0, 1000.0), 100);
        assert_eq!(percent(999.0, 1000.0), 99, "scratched is not full");
        assert_eq!(percent(505.0, 1000.0), 50, "rounds down, not to nearest");
        assert_eq!(percent(1.0, 1000.0), 1, "standing is never 0%");
        assert_eq!(percent(0.0, 1000.0), 0);
        assert_eq!(percent(-5.0, 1000.0), 0);
        assert_eq!(percent(10.0, 0.0), 0);
        assert_eq!(
            health_text(999.0, 1000.0, BossKind::Boss),
            "999 / 1,000 (99%)"
        );
        assert_eq!(
            health_text(2.5e6, 5.0e6, BossKind::Boss),
            "2.5M / 5.0M (50%)"
        );
    }

    #[test]
    fn two_bars_show_the_newest_bosses() {
        let shown = bars(&[boss(3, 400.0, None), boss(1, 900.0, None), boss(2, 1000.0, None)]);
        let names: Vec<_> = shown.iter().map(|b| b.name).collect();
        assert_eq!(names, [NAMES[2], NAMES[3]], "the two newest, oldest first");
        assert_eq!(shown[0].health, "1,000 / 1,000 (100%)");
        assert_eq!(shown[1].health, "400 / 1,000 (40%)");
        assert!((shown[1].fill - 0.4).abs() < 1e-6);
        assert!(shown.iter().all(|b| b.alpha == 1.0 && b.lift == 0.0));
        assert_eq!(bars(&[boss(1, 900.0, None)]).len(), 1);
    }

    #[test]
    fn a_fallen_boss_fades_and_goes() {
        let shown = bars(&[boss(1, 120.0, Some(FADE_SECS / 2.0))]);
        assert_eq!(shown.len(), 1);
        assert!((shown[0].alpha - 0.5).abs() < 1e-6);
        assert_eq!(shown[0].fill, 0.0);
        assert_eq!(shown[0].health, "0 / 1,000 (0%)");
        assert!(bars(&[boss(1, 0.0, Some(FADE_SECS))]).is_empty());
        assert!(bars(&[]).is_empty());
        // A fading corpse still takes its seat among the newest two.
        let shown = bars(&[
            boss(1, 10.0, None),
            boss(2, 0.0, Some(0.2)),
            boss(3, 500.0, None),
        ]);
        assert_eq!(shown.len(), MAX_BARS);
        assert_eq!(shown[0].name, NAMES[2]);
    }

    #[test]
    fn a_new_bar_slides_in_and_settles() {
        let mut b = boss(1, 1000.0, None);
        b.age = 0.0;
        let first = bar(&b);
        assert_eq!(first.alpha, 0.0);
        assert!((first.lift + SLIDE_PX).abs() < 1e-4, "starts SLIDE_PX above");
        b.age = SLIDE_SECS / 2.0;
        let mid = bar(&b);
        assert!(mid.lift < 0.0 && mid.lift > first.lift);
        assert!(mid.alpha > 0.0 && mid.alpha <= 1.0);
        b.age = SLIDE_SECS;
        let done = bar(&b);
        assert_eq!(done.lift, 0.0);
        assert_eq!(done.alpha, 1.0);
    }

    #[test]
    fn the_tag_names_the_kind_and_the_rage() {
        assert_eq!(tag_text(BossKind::Boss, false), "");
        assert_eq!(tag_text(BossKind::Final, false), "Final boss");
        assert_eq!(tag_text(BossKind::Boss, true), "Enraged");
        assert_eq!(tag_text(BossKind::Final, true), "Final boss, Enraged");
        assert_eq!(tag_text(BossKind::Rare, false), "Rare");
        assert_eq!(tag_text(BossKind::Unbeatable, false), "Unbeatable");
    }

    #[test]
    fn a_rare_is_silver_and_rage_heats_the_red() {
        assert_eq!(fill_tint(BossKind::Rare, false), FILL_RARE);
        assert_eq!(fill_tint(BossKind::Rare, true), FILL_RARE);
        assert_eq!(fill_tint(BossKind::Boss, false), FILL_BOSS);
        assert_eq!(fill_tint(BossKind::Final, true), FILL_ENRAGED);
        assert_ne!(FILL_BOSS, FILL_ENRAGED);
    }

    #[test]
    fn kinds_come_from_the_marks_and_the_health() {
        assert_eq!(kind_of(false, false, 5.0e4), BossKind::Boss);
        assert_eq!(kind_of(false, true, 5.0e4), BossKind::Final);
        assert_eq!(kind_of(true, false, 5.0e4), BossKind::Rare);
        assert_eq!(kind_of(false, false, 1.0e9), BossKind::Unbeatable);
        assert_eq!(health_text(1.0e9, 1.0e9, BossKind::Unbeatable), "???");
    }

    #[test]
    fn the_skull_is_for_ten_levels_above() {
        assert!(!skull(20, 29));
        assert!(skull(20, 30));
        assert!(skull(1, 60));
        assert!(!skull(60, 60));
        assert!(!skull(u32::MAX, 60), "no overflow");
    }

    #[test]
    fn the_border_art_scales_with_the_bar() {
        // The casting bar's 195x13 window in its 256x64 art, scaled to BAR_W.
        assert!((FRAME_W - 256.0 * BAR_W / 195.0).abs() < 1e-3);
        assert!((BAR_H - 13.0 * BAR_W / 195.0).abs() < 1e-3);
        const { assert!(FRAME_X > 0.0 && FRAME_Y > 0.0) };
    }
}
