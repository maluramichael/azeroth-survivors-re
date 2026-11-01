//! The zone's finale: its dungeon boss near the end of a run, and an unbeatable world boss after
//! thirty minutes.
//!
//! The finale takes the place of the roster's last timed boss: the clock spawns the other five
//! ([`Plan::timed`], read by [`super::enemies::spawn_enemies`]) and the finale comes at the sixth
//! slot, 13:30. Killing it wins the run, ahead of the 15:00 bell that wins it anyway, and opens a
//! portal for [`PORTAL_SECS`] seconds ([`Gate`]): a hero who steps through meets the zone's bonus
//! boss ([`super::rares::bonus_for`], ticket #1135), a far stronger world boss with a signature
//! prize; the run stays won either way. Past [`REAPER_AT`] Lord Kazzak hunts the hero and cannot
//! die.

use std::ops::Range;
use std::sync::{LazyLock, OnceLock};

use benilla_world::schedule::WorldStage;
use benilla_world::world_point::WorldPoint;
use bevy::prelude::*;

use super::chatlog::{ChatKind, ChatLine};
use super::data::{self, EnemyDef};
use super::enemies::{self, Dying, Enemy, Loot};
use super::hero::{Hero, Peer};
use super::items::BossDrop;
use super::minimap::MapMarker;
use super::nav::{NavGrid, SpawnProbe};
use super::rares;
use super::route::{self, Bounds, RouteRun};
use super::units::{self, flat, Fx};
use super::{HeroState, Mail, Phase, Rng, Run, Selection};

/// The finale comes in the slot of the sixth timed boss.
const FINALE_AT: f32 = data::BOSS_EVERY_SECS * 6.0;
/// Thirty minutes in: the reaper. Past the 15:00 bell that wins the run, so no run reaches it
/// until the win rule lets one go on.
const REAPER_AT: f32 = 1800.0;
/// The finale has the zone's strongest roster boss's numbers, this much tougher, and is drawn
/// larger.
const FINALE_HP: f32 = 1.5;
const FINALE_DAMAGE: f32 = 1.25;
const FINALE_XP: u32 = 3;
const FINALE_SIZE: f32 = 1.2;
/// How far from the hero either appears, in yards.
const SUMMON_RANGE: f32 = 30.0;
/// How far from the end ring's centre the finale of a route appears, in yards.
const ROUTE_SUMMON_RANGE: f32 = 25.0;
/// The flourish of a win: Divine Shield, as the clock's win plays it.
const SPELL_WIN: u32 = 642;
/// The bonus boss's portal ([`Gate`]): `InstancePortal_Red.mdx`, no collision hull, a first guess
/// at its scale (it is about 9 yd wide at 1.0).
const PORTAL_DISPLAY: u32 = 4711;
const PORTAL_SCALE: f32 = 0.5;
/// How far from the hero the portal stands, in yards, in the way the hero faces.
const PORTAL_RANGE: f32 = 8.0;
/// How close a standing hero must come to step through.
const PORTAL_ENTER: f32 = 2.5;
/// How long the portal stays open once the finale has fallen.
const PORTAL_SECS: f32 = 20.0;
/// How long a place for the portal is looked for before the run just ends.
const PORTAL_TRIES: f32 = 2.0;
/// How far from the hero who stepped through the bonus boss arrives.
const BONUS_RANGE: f32 = 25.0;
/// The gold the bonus boss's fall pays on top of its loot, and the seconds before the results.
const BONUS_GOLD: u32 = 200;
const BONUS_END_SECS: f32 = 3.0;

/// Lord Kazzak, hunting: health beyond reach, damage beyond armor, faster than the hero.
static REAPER: EnemyDef = EnemyDef {
    name: "Lord Kazzak",
    display: 12449,
    hp: 1.0e9,
    speed: 9.0,
    damage: 600.0,
    scale: 1.1,
    xp: 0,
    level: 63,
    big: true,
    ranged: None,
};

/// The dungeon boss of the zone; its death wins the run.
#[derive(Component)]
pub(super) struct Finale;

/// The unbeatable hunter of the thirtieth minute.
#[derive(Component)]
pub(super) struct Reaper;

/// The zone's bonus boss, behind the portal ([`super::rares::bonus_for`]).
#[derive(Component)]
struct Bonus;

/// The portal that leads to the bonus boss.
#[derive(Component)]
struct Portal;

/// Where the way to the bonus boss stands, from the finale's fall to the results.
#[derive(Resource, Default, Clone, Copy, Debug, PartialEq)]
enum Gate {
    /// The finale still stands, or the zone has no bonus boss.
    #[default]
    Closed,
    /// The finale fell; a place for the portal is looked for, for this many seconds.
    Opening { left: f32 },
    /// The portal stands at `at` for `left` more seconds.
    Open {
        left: f32,
        portal: Entity,
        at: Vec3,
    },
    /// A hero stepped through at `at`; the bonus boss comes once a place for it is free.
    Arriving { at: Vec3 },
    /// The bonus boss fights.
    Fight,
    /// The bonus boss fell; the results come in this many seconds.
    Done { left: f32 },
}

/// What a second of the open portal brings.
#[derive(Debug, PartialEq)]
enum Step {
    /// Still open, with this long left.
    Wait(f32),
    /// A hero stepped through.
    Enter,
    /// The time ran out.
    Close,
}

/// The open portal after `dt` seconds with `left` to go, and whether a standing hero is at it.
/// Stepping through in the last moment still counts.
fn gate_step(left: f32, dt: f32, entered: bool) -> Step {
    if entered {
        Step::Enter
    } else if left - dt <= 0.0 {
        Step::Close
    } else {
        Step::Wait(left - dt)
    }
}

/// Who the finale is, by battleground name.
enum Pick {
    /// A boss already on the zone's roster, taken out of the clock's schedule.
    Promote(&'static str),
    /// A boss the roster lacks: its name, display, scale and level, with the strongest roster
    /// boss's numbers.
    Extra(&'static str, u32, f32, u32),
}

/// A battleground without an entry takes the last of its roster's bosses.
///
/// Display ids of `Extra`: unverified (VanCleef 2029, Ragnaros 11121, from memory); the promoted
/// ones are the roster's own.
const FINALES: &[(&str, Pick)] = &[
    ("The Barrens", Pick::Promote("Agathelos the Raging")),
    ("The Dark Portal", Pick::Promote("Lord Kazzak")),
    (
        "Gates of Ahn'Qiraj",
        Pick::Promote("Ossirian the Unscarred"),
    ),
    ("Fire Plume Ridge", Pick::Extra("Ragnaros", 11121, 0.5, 63)),
    ("Gurubashi Arena", Pick::Promote("Hakkar")),
    ("Elwynn Forest", Pick::Promote("Hogger")),
    ("Durotar", Pick::Promote("Death Flayer")),
    ("Teldrassil", Pick::Promote("Duskstalker")),
    ("Dun Morogh", Pick::Promote("Great Father Arctikus")),
    ("Mulgore", Pick::Promote("Ghost Howl")),
    ("Tirisfal Glades", Pick::Promote("Bayne")),
    ("Westfall", Pick::Extra("Edwin VanCleef", 2029, 1.9, 21)),
    ("Duskwood", Pick::Promote("Nefaru")),
    ("Redridge Mountains", Pick::Promote("Rohh the Silent")),
    ("Mount Hyjal", Pick::Promote("Ysondre")),
    ("Naxxramas", Pick::Promote("Kel'Thuzad")),
];

/// A battleground's boss schedule.
pub(super) struct Plan {
    /// The roster's bosses the clock spawns, in order.
    pub timed: Vec<EnemyDef>,
    pub finale: EnemyDef,
    /// On a route ([`super::route`]): each zone's [`data::MAPS`] index and its range in `timed`.
    /// Empty for a zone.
    pub segments: Vec<(usize, Range<usize>)>,
}

static PLANS: LazyLock<Vec<Plan>> = LazyLock::new(|| {
    // The zones first; a route waits for them.
    let mut plans: Vec<Option<Plan>> = data::MAPS
        .iter()
        .enumerate()
        .map(|(i, m)| {
            (super::route::route_of(i).is_none()).then(|| build(m.name, &data::roster(i).bosses))
        })
        .collect();
    // A route's clock spawns its zones' timed bosses in turn, and its finale is its last zone's.
    // Read from `plans`: calling `plan` here would initialise this lock from inside itself.
    for i in 0..plans.len() {
        let Some(route) = super::route::route_of(i) else {
            continue;
        };
        let zones = route.zones();
        let mut timed = Vec::new();
        let mut segments = Vec::new();
        for &zone in &zones {
            let part = plans[zone].as_ref().expect("a route's zone is no route");
            segments.push((zone, timed.len()..timed.len() + part.timed.len()));
            timed.extend_from_slice(&part.timed);
        }
        let last = zones.last().copied().expect("a route has zones");
        let finale = plans[last].as_ref().expect("a route's zone is no route").finale;
        plans[i] = Some(Plan {
            timed,
            finale,
            segments,
        });
    }
    plans.into_iter().map(|p| p.expect("every plan built")).collect()
});

/// The plan for the battleground at `map`.
/// The creature that ends a run that lasts too long.
pub(super) fn reaper() -> &'static EnemyDef {
    &REAPER
}

pub(super) fn plan(map: usize) -> &'static Plan {
    &PLANS[map.min(PLANS.len() - 1)]
}

fn build(map: &str, bosses: &[EnemyDef]) -> Plan {
    let pick = FINALES.iter().find(|(m, _)| *m == map).map(|(_, p)| p);
    let last = bosses.len() - 1;
    let promoted = match pick {
        Some(Pick::Promote(name)) => bosses.iter().position(|b| b.name == *name),
        _ => None,
    };
    let mut timed = bosses.to_vec();
    timed.remove(promoted.unwrap_or(last));
    let strongest = bosses[last];
    let mut look = promoted.map_or(strongest, |i| bosses[i]);
    if let Some(&Pick::Extra(name, display, scale, level)) = pick {
        look = EnemyDef {
            name,
            display,
            scale,
            level,
            ..look
        };
    }
    let finale = EnemyDef {
        hp: strongest.hp * FINALE_HP,
        damage: strongest.damage * FINALE_DAMAGE,
        xp: strongest.xp * FINALE_XP,
        scale: look.scale * FINALE_SIZE,
        ..look
    };
    Plan {
        timed,
        finale,
        segments: Vec::new(),
    }
}

/// When the finale comes; `WOW_SURVIVORS_FINALE=<seconds>` moves it, to photograph it.
pub(super) fn finale_at() -> f32 {
    static AT: OnceLock<f32> = OnceLock::new();
    *AT.get_or_init(|| {
        benilla_app::survivors_host::webenv::var("WOW_SURVIVORS_FINALE")
            .and_then(|v| v.parse().ok())
            .unwrap_or(FINALE_AT)
    })
}

/// What has come this run.
#[derive(Resource, Default)]
pub(super) struct Stage {
    finale: bool,
    reaper: bool,
}

pub(super) fn plugin(app: &mut App) {
    app.init_resource::<Stage>()
        .init_resource::<Gate>()
        .add_systems(
            Update,
            (
                summon,
                hunt,
                drop_finale,
                finale_fallen,
                open_portal,
                tick_gate,
                bonus_slain,
            )
                .chain()
                .after(WorldStage::Input)
                .run_if(super::run_on)
                .run_if(super::simulates)
                .run_if(|l: Res<benilla_app::survivors_host::loading_screen::LoadingScreen>| !l.covering()),
        )
        .add_systems(OnEnter(Phase::Menu), |mut stage: ResMut<Stage>, mut gate: ResMut<Gate>| {
            *stage = Stage::default();
            *gate = Gate::default();
        });
}

/// Whether the finale is due: on the clock, or on a route once its last station is reached
/// (`arrived`, the end ring stands).
fn finale_due(on_route: bool, arrived: bool, elapsed: f32, at: f32) -> bool {
    if on_route {
        arrived
    } else {
        elapsed >= at
    }
}

/// The finale and the reaper arrive on the clock; the finale of a route, in its end ring.
#[allow(clippy::too_many_arguments)]
pub(super) fn summon(
    mut commands: Commands,
    mut run: ResMut<Run>,
    mut stage: ResMut<Stage>,
    mut rng: ResMut<Rng>,
    selection: Res<Selection>,
    route: Res<RouteRun>,
    bounds: Res<Bounds>,
    nav: Res<NavGrid>,
    probe: SpawnProbe,
    world: WorldPoint,
    hero: Query<&Transform, With<Hero>>,
    mut chat: MessageWriter<ChatLine>,
) {
    let Ok(h) = hero.single() else { return };
    if run.party_down {
        return;
    }
    let hpos = h.translation;
    let on_route = route::route_of(selection.stage).is_some();
    let wants_finale =
        !stage.finale && finale_due(on_route, route.arrived, run.elapsed, finale_at());
    // The reaper waits out a bonus fight.
    let wants_reaper = !stage.reaper && run.elapsed >= REAPER_AT && !run.encore;
    if !wants_finale && !wants_reaper {
        return;
    }
    let a = rng.range(0.0, std::f32::consts::TAU);
    // The finale of a route stands in the end ring, round its centre; the rest by the hero.
    let (anchor, range) = if wants_finale && on_route {
        (
            Vec3::new(bounds.centre.x, hpos.y, bounds.centre.z),
            ROUTE_SUMMON_RANGE,
        )
    } else {
        (hpos, SUMMON_RANGE)
    };
    // No free spot yet: the arrival waits for the next frame.
    let Some(pos) = enemies::spawn_spot(&nav, &probe, &world, anchor, a, range) else {
        return;
    };
    if wants_finale {
        let def = &plan(selection.map).finale;
        let e = enemies::spawn_enemy(&mut commands, &mut run, def, pos, hpos, true, false);
        commands.entity(e).insert(Finale);
        stage.finale = true;
        // It counts with the bosses the payout pays for.
        run.bosses += 1;
        run.announce(format!("{} awaits!", def.name), 4.0);
        chat.write(ChatLine::text(
            ChatKind::Boss,
            format!("{} awaits! Slay it and the zone is yours.", def.name),
        ));
    } else {
        let e = enemies::spawn_enemy(&mut commands, &mut run, &REAPER, pos, hpos, true, false);
        commands.entity(e).insert(Reaper);
        stage.reaper = true;
        run.announce(format!("{} hunts you!", REAPER.name), 5.0);
        chat.write(ChatLine::text(
            ChatKind::Boss,
            format!("{} has come for you. Nothing can stop him.", REAPER.name),
        ));
    }
}

/// The reaper shrugs off every blow and every spell.
pub(super) fn hunt(mut reapers: Query<&mut Enemy, With<Reaper>>) {
    for mut e in &mut reapers {
        e.hp = e.max_hp;
        e.slow = 1.0;
        e.slow_t = 0.0;
        e.root_t = 0.0;
        e.stun_t = 0.0;
        e.fear_t = 0.0;
    }
}

/// The finale's death wins the run, on the spot. This is how the balance simulator plays it (the
/// bonus fight lies past its clock); the game plays [`finale_fallen`].
#[cfg(test)]
pub(super) fn finale_slain(
    mut run: ResMut<Run>,
    mut next: ResMut<NextState<Phase>>,
    slain: Query<(), (With<Finale>, Added<Dying>)>,
    hero: Query<Entity, With<Hero>>,
    mut fx: Fx,
) {
    if slain.is_empty() || run.party_down {
        return;
    }
    run.won = true;
    if let Ok(hero) = hero.single() {
        fx.cast(hero, SPELL_WIN, &[hero], None);
    }
    next.set(Phase::GameOver);
}

/// Debug mode's F3 drops the finale where it stands, as a kill without its count, to photograph
/// the portal and the bonus boss (F4 jumps to a guard post, F10 and F11 step the anti-aliasing,
/// F12 is the screenshot).
fn drop_finale(
    keys: Res<ButtonInput<KeyCode>>,
    mut commands: Commands,
    mut run: ResMut<Run>,
    mut finale: Query<(Entity, &mut Enemy, &Transform), (With<Finale>, Without<Dying>)>,
    mut loot: MessageWriter<Loot>,
) {
    if !keys.just_pressed(KeyCode::F3) || !super::debug::active() {
        return;
    }
    for (e, mut en, tf) in &mut finale {
        en.dead = true;
        en.hp = 0.0;
        en.dots.clear();
        enemies::mark_dying(&mut commands, e);
        loot.write(Loot {
            pos: tf.translation,
            xp: en.xp,
            boss: true,
            level: en.def.level,
            def: en.def,
        });
        run.kills += 1;
    }
}

/// The finale's death wins the run and, where the zone has a bonus boss, opens the way to it
/// ([`open_portal`]); the results wait for the portal to close or the bonus fight to end.
fn finale_fallen(
    mut run: ResMut<Run>,
    mut gate: ResMut<Gate>,
    mut next: ResMut<NextState<Phase>>,
    selection: Res<Selection>,
    slain: Query<(), (With<Finale>, Added<Dying>)>,
    hero: Query<Entity, With<Hero>>,
    mut fx: Fx,
) {
    if slain.is_empty() || run.party_down {
        return;
    }
    run.won = true;
    if let Ok(hero) = hero.single() {
        fx.cast(hero, SPELL_WIN, &[hero], None);
    }
    if rares::bonus_of(selection.map).is_some() {
        *gate = Gate::Opening { left: PORTAL_TRIES };
    } else {
        next.set(Phase::GameOver);
    }
}

/// Stands the portal [`PORTAL_RANGE`] yards ahead of a standing hero (round a tree if need be) and
/// holds the 15:00 bell for it ([`Run::encore`]). With no place for it in a moment, or no one
/// standing, the run just ends.
#[allow(clippy::too_many_arguments)]
fn open_portal(
    mut commands: Commands,
    time: Res<Time>,
    mut run: ResMut<Run>,
    mut gate: ResMut<Gate>,
    mut next: ResMut<NextState<Phase>>,
    selection: Res<Selection>,
    nav: Res<NavGrid>,
    probe: SpawnProbe,
    world: WorldPoint,
    hero: Query<&Transform, With<Hero>>,
    peers: Query<(&Transform, &HeroState), (With<Peer>, Without<Hero>)>,
    mut chat: MessageWriter<ChatLine>,
) {
    let Gate::Opening { left } = *gate else { return };
    let left = left - time.delta_secs();
    let spot = rares::standing(&run, &hero, &peers).first().and_then(|t| {
        let ahead = flat(t.rotation * Vec3::NEG_Z);
        let a = ahead.z.atan2(ahead.x);
        enemies::spawn_spot(&nav, &probe, &world, t.translation, a, PORTAL_RANGE)
    });
    let (Some(at), Some(bonus)) = (spot, rares::bonus_of(selection.map)) else {
        if left <= 0.0 {
            *gate = Gate::Closed;
            next.set(Phase::GameOver);
        } else {
            *gate = Gate::Opening { left };
        }
        return;
    };
    let serial = run.guid();
    let portal = units::spawn_object(&mut commands, serial, PORTAL_DISPLAY, PORTAL_SCALE, at);
    commands.entity(portal).insert((Portal, MapMarker::Portal));
    run.encore = true;
    *gate = Gate::Open {
        left: PORTAL_SECS,
        portal,
        at,
    };
    let text = format!(
        "The way to {} lies open. Step through within {} s.",
        bonus.name, PORTAL_SECS as u32
    );
    run.announce(text.clone(), 5.0);
    chat.write(ChatLine::text(ChatKind::Boss, text));
}

/// The open portal waits for a standing hero and closes on time; the bonus boss arrives once a
/// hero has stepped through; the results come a moment after it falls.
#[allow(clippy::too_many_arguments)]
fn tick_gate(
    mut commands: Commands,
    time: Res<Time>,
    mut run: ResMut<Run>,
    mut gate: ResMut<Gate>,
    mut next: ResMut<NextState<Phase>>,
    mut rng: ResMut<Rng>,
    selection: Res<Selection>,
    nav: Res<NavGrid>,
    probe: SpawnProbe,
    world: WorldPoint,
    hero: Query<&Transform, With<Hero>>,
    peers: Query<(&Transform, &HeroState), (With<Peer>, Without<Hero>)>,
    mut chat: MessageWriter<ChatLine>,
) {
    let dt = time.delta_secs();
    match *gate {
        Gate::Open { left, portal, at } => {
            let heroes = rares::standing(&run, &hero, &peers);
            let through = heroes
                .iter()
                .find(|t| flat(t.translation - at).length() <= PORTAL_ENTER);
            match gate_step(left, dt, through.is_some()) {
                Step::Wait(left) => *gate = Gate::Open { left, portal, at },
                Step::Enter => {
                    commands.entity(portal).despawn();
                    *gate = Gate::Arriving {
                        at: through.map_or(at, |t| t.translation),
                    };
                }
                Step::Close => {
                    commands.entity(portal).despawn();
                    run.encore = false;
                    run.announce("The portal closes.".into(), 3.0);
                    *gate = Gate::Closed;
                    next.set(Phase::GameOver);
                }
            }
        }
        Gate::Arriving { at } => {
            let Some(def) = rares::bonus_of(selection.map) else {
                *gate = Gate::Closed;
                run.encore = false;
                next.set(Phase::GameOver);
                return;
            };
            let a = rng.range(0.0, std::f32::consts::TAU);
            // No free spot yet: the arrival waits for the next frame.
            let Some(pos) = enemies::spawn_spot(&nav, &probe, &world, at, a, BONUS_RANGE) else {
                return;
            };
            let e = enemies::spawn_enemy(&mut commands, &mut run, def, pos, at, true, false);
            commands.entity(e).insert((Bonus, MapMarker::Boss));
            *gate = Gate::Fight;
            run.announce(format!("{} is upon you!", def.name), 4.0);
            chat.write(ChatLine::text(
                ChatKind::Boss,
                format!("{} is upon you! Slay it and its treasure is yours.", def.name),
            ));
        }
        Gate::Done { left } => {
            if left - dt <= 0.0 {
                *gate = Gate::Closed;
                run.encore = false;
                next.set(Phase::GameOver);
            } else {
                *gate = Gate::Done { left: left - dt };
            }
        }
        Gate::Closed | Gate::Opening { .. } | Gate::Fight => {}
    }
}

/// The bonus boss's fall: its signature piece of loot goes on at once, and a purse of gold; the
/// run, won already, ends in a moment.
#[allow(clippy::too_many_arguments)]
fn bonus_slain(
    mut run: ResMut<Run>,
    mut gate: ResMut<Gate>,
    selection: Res<Selection>,
    slain: Query<&Transform, (With<Bonus>, Added<Dying>)>,
    peers: Query<(Entity, &HeroState), (With<Peer>, Without<Hero>)>,
    mut drops: MessageWriter<BossDrop>,
    mut chat: MessageWriter<ChatLine>,
    mut mail: ResMut<super::GuestMail>,
) {
    let Some(tf) = slain.iter().next() else { return };
    if let Some(tpl) = rares::bonus_loot(selection.map) {
        drops.write(BossDrop {
            pos: tf.translation,
            tpl,
            wear_now: true,
        });
    }
    // The purse is every standing hero's: this machine's and each co-op guest's.
    if run.dead_t.is_none() {
        run.me.gold += BONUS_GOLD;
        chat.write(ChatLine::Gold(BONUS_GOLD));
    }
    for (p, state) in &peers {
        if state.dead_t.is_none() {
            mail.0.push((p, Mail::Gold(BONUS_GOLD)));
        }
    }
    run.announce("Its treasure is yours!".into(), 4.0);
    *gate = Gate::Done {
        left: BONUS_END_SECS,
    };
}

#[cfg(test)]
mod tests {
    use super::*;

    fn boss(name: &'static str, hp: f32) -> EnemyDef {
        EnemyDef {
            name,
            display: 1,
            hp,
            speed: 4.0,
            damage: 10.0,
            scale: 1.0,
            xp: 10,
            level: 5,
            big: false,
            ranged: None,
        }
    }

    #[test]
    fn promoted_boss_leaves_the_schedule_and_takes_the_strongest_numbers() {
        let bosses = [boss("A", 10.0), boss("Hogger", 20.0), boss("C", 100.0)];
        let p = build("Elwynn Forest", &bosses);
        assert_eq!(p.timed.len(), 2);
        assert!(p.timed.iter().all(|b| b.name != "Hogger"));
        assert_eq!(p.finale.name, "Hogger");
        assert_eq!(p.finale.hp, 100.0 * FINALE_HP);
    }

    #[test]
    fn a_route_plan_ends_on_its_last_zones_finale() {
        let mut routes = 0;
        for map in 0..data::MAPS.len() {
            let Some(route) = super::super::route::route_of(map) else {
                assert!(plan(map).segments.is_empty());
                continue;
            };
            routes += 1;
            let p = plan(map);
            let zones = route.zones();
            let last = *zones.last().unwrap();
            assert_eq!(p.finale.name, plan(last).finale.name);
            assert_eq!(p.finale.hp, plan(last).finale.hp);
            let mut at = 0;
            for ((zone, range), &want) in p.segments.iter().zip(&zones) {
                assert_eq!(*zone, want);
                assert_eq!(range.start, at, "no gaps");
                at = range.end;
                let names: Vec<_> = p.timed[range.clone()].iter().map(|b| b.name).collect();
                let own: Vec<_> = plan(want).timed.iter().map(|b| b.name).collect();
                assert_eq!(names, own);
            }
            assert_eq!(at, p.timed.len());
        }
        assert!(routes > 0);
    }

    #[test]
    fn extra_boss_takes_the_last_slot() {
        let bosses = [boss("A", 10.0), boss("B", 50.0)];
        let p = build("Westfall", &bosses);
        assert_eq!(p.timed.len(), 1);
        assert_eq!(p.timed[0].name, "A");
        assert_eq!(p.finale.name, "Edwin VanCleef");
        assert_eq!(p.finale.hp, 50.0 * FINALE_HP);
    }

    #[test]
    fn unlisted_zone_promotes_its_last_boss() {
        let bosses = [boss("A", 10.0), boss("B", 50.0)];
        let p = build("Nowhere", &bosses);
        assert_eq!(p.timed.len(), 1);
        assert_eq!(p.finale.name, "B");
    }

    #[test]
    fn the_portal_waits_then_closes() {
        assert_eq!(gate_step(PORTAL_SECS, 1.0, false), Step::Wait(PORTAL_SECS - 1.0));
        assert_eq!(gate_step(1.0, 0.5, false), Step::Wait(0.5));
        assert_eq!(gate_step(0.5, 0.5, false), Step::Close);
        assert_eq!(gate_step(0.1, 1.0, false), Step::Close);
        // Stepping through in the last moment still counts.
        assert_eq!(gate_step(0.1, 1.0, true), Step::Enter);
        // The window in frames: open for its twenty seconds, then shut.
        let (mut left, mut frames) = (PORTAL_SECS, 0u32);
        loop {
            match gate_step(left, 1.0 / 60.0, false) {
                Step::Wait(l) => {
                    left = l;
                    frames += 1;
                }
                Step::Close => break,
                Step::Enter => unreachable!("no one is at the portal"),
            }
        }
        assert!((frames as f32 / 60.0 - PORTAL_SECS).abs() < 0.1, "{frames} frames");
    }

    #[test]
    fn the_portal_stands_apart_from_the_hero_and_the_bonus_boss_farther() {
        const { assert!(PORTAL_ENTER < PORTAL_RANGE) };
        const { assert!(PORTAL_RANGE < BONUS_RANGE) };
        // The window ends before the 15:00 bell even for the latest finale.
        const { assert!(FINALE_AT + PORTAL_SECS < data::VICTORY_SECS) };
    }

    #[test]
    fn a_route_finale_waits_for_the_end_and_a_zone_finale_for_the_clock() {
        let at = finale_at();
        // A zone: the clock, whatever the route says.
        assert!(!finale_due(false, false, at - 1.0, at));
        assert!(finale_due(false, false, at, at));
        assert!(!finale_due(false, true, at - 1.0, at));
        // A route: the end ring, however late or early the clock is.
        assert!(!finale_due(true, false, at * 3.0, at));
        assert!(finale_due(true, true, 0.0, at));
        // It stands inside the ring it is summoned for.
        const { assert!(ROUTE_SUMMON_RANGE < route::END_RING) };
    }
}
