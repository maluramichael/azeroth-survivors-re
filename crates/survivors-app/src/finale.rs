//! The zone's finale: its dungeon boss near the end of a run, and an unbeatable world boss after
//! thirty minutes.
//!
//! The finale takes the place of the roster's last timed boss: the clock spawns the other five
//! ([`Plan::timed`], read by [`super::enemies::spawn_enemies`]) and the finale comes at the sixth
//! slot, 13:30. Killing it wins the run at once, ahead of the 15:00 bell that wins it anyway.
//! Past [`REAPER_AT`] Lord Kazzak hunts the hero and cannot die.

use std::sync::{LazyLock, OnceLock};

use benilla_world::schedule::WorldStage;
use benilla_world::world_point::WorldPoint;
use bevy::prelude::*;

use super::chatlog::{ChatKind, ChatLine};
use super::data::{self, EnemyDef};
use super::enemies::{self, Dying, Enemy};
use super::hero::Hero;
use super::nav::{NavGrid, SpawnProbe};
use super::units::Fx;
use super::{Phase, Rng, Run, Selection};

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
/// The flourish of a win: Divine Shield, as the clock's win plays it.
const SPELL_WIN: u32 = 642;

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
};

/// The dungeon boss of the zone; its death wins the run.
#[derive(Component)]
pub(super) struct Finale;

/// The unbeatable hunter of the thirtieth minute.
#[derive(Component)]
pub(super) struct Reaper;

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
}

static PLANS: LazyLock<Vec<Plan>> = LazyLock::new(|| {
    data::MAPS
        .iter()
        .enumerate()
        .map(|(i, m)| build(m.name, &data::roster(i).bosses))
        .collect()
});

/// The plan for the battleground at `map`.
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
    Plan { timed, finale }
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
struct Stage {
    finale: bool,
    reaper: bool,
}

pub(super) fn plugin(app: &mut App) {
    app.init_resource::<Stage>()
        .add_systems(
            Update,
            (summon, hunt, finale_slain)
                .chain()
                .after(WorldStage::Input)
                .run_if(in_state(Phase::Playing))
                .run_if(super::simulates)
                .run_if(|l: Res<benilla_app::survivors_host::loading_screen::LoadingScreen>| !l.covering()),
        )
        .add_systems(OnEnter(Phase::Menu), |mut stage: ResMut<Stage>| {
            *stage = Stage::default();
        });
}

/// The finale and the reaper arrive on the clock.
#[allow(clippy::too_many_arguments)]
fn summon(
    mut commands: Commands,
    mut run: ResMut<Run>,
    mut stage: ResMut<Stage>,
    mut rng: ResMut<Rng>,
    selection: Res<Selection>,
    nav: Res<NavGrid>,
    probe: SpawnProbe,
    world: WorldPoint,
    hero: Query<&Transform, With<Hero>>,
    mut chat: MessageWriter<ChatLine>,
) {
    let Ok(h) = hero.single() else { return };
    if run.dead_t.is_some() {
        return;
    }
    let hpos = h.translation;
    let wants_finale = !stage.finale && run.elapsed >= finale_at();
    let wants_reaper = !stage.reaper && run.elapsed >= REAPER_AT;
    if !wants_finale && !wants_reaper {
        return;
    }
    let a = rng.range(0.0, std::f32::consts::TAU);
    // No free spot yet: the arrival waits for the next frame.
    let Some(pos) = enemies::spawn_spot(&nav, &probe, &world, hpos, a, SUMMON_RANGE) else {
        return;
    };
    if wants_finale {
        let def = &plan(selection.map).finale;
        let e = enemies::spawn_enemy(&mut commands, &mut run, def, pos, hpos, true, false);
        commands.entity(e).insert(Finale);
        stage.finale = true;
        // It counts with the bosses the payout pays for.
        run.bosses += 1;
        run.banner = Some((format!("{} awaits!", def.name), 4.0));
        chat.write(ChatLine::text(
            ChatKind::Boss,
            format!("{} awaits! Slay it and the zone is yours.", def.name),
        ));
    } else {
        let e = enemies::spawn_enemy(&mut commands, &mut run, &REAPER, pos, hpos, true, false);
        commands.entity(e).insert(Reaper);
        stage.reaper = true;
        run.banner = Some((format!("{} hunts you!", REAPER.name), 5.0));
        chat.write(ChatLine::text(
            ChatKind::Boss,
            format!("{} has come for you. Nothing can stop him.", REAPER.name),
        ));
    }
}

/// The reaper shrugs off every blow and every spell.
fn hunt(mut reapers: Query<&mut Enemy, With<Reaper>>) {
    for mut e in &mut reapers {
        e.hp = e.max_hp;
        e.slow = 1.0;
        e.slow_t = 0.0;
        e.root_t = 0.0;
        e.stun_t = 0.0;
        e.fear_t = 0.0;
    }
}

/// The finale's death wins the run.
fn finale_slain(
    mut run: ResMut<Run>,
    mut next: ResMut<NextState<Phase>>,
    slain: Query<(), (With<Finale>, Added<Dying>)>,
    hero: Query<Entity, With<Hero>>,
    mut fx: Fx,
) {
    if slain.is_empty() || run.dead_t.is_some() {
        return;
    }
    run.won = true;
    if let Ok(hero) = hero.single() {
        fx.cast(hero, SPELL_WIN, &[hero], None);
    }
    next.set(Phase::GameOver);
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
}
