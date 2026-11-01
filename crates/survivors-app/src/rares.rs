//! Rares and the bonus boss: named rares of the zone that walk out now and then between the
//! bosses, and a far stronger world boss behind a portal after the finale (ticket #1135).
//!
//! A rare is a boss-flagged enemy ([`super::enemies::Rare`]) with its own name, look and silver
//! frame; it never counts in `run.bosses`, so the clock's bosses and the run's rhythm stay as they
//! were. The bonus boss is only defined here ([`bonus_for`]); the portal that starts the fight is
//! [`super::finale`]'s. Both lists join the zone's extras ([`super::data::extras`]), after the
//! boss adds, in this order: rares, then the bonus boss.
//!
//! The names are Classic rares from memory, the looks are the zone roster's own (or checked
//! against `CreatureDisplayInfo.dbc`, see the plan's appendix 3).

use std::sync::{LazyLock, OnceLock};

use benilla_world::world_point::WorldPoint;
use bevy::prelude::*;

use super::chatlog::{ChatKind, ChatLine};
use super::data::{self, EnemyDef};
use super::enemies::{self, Enemy, Rare, BOSS_RADIUS};
use super::finale;
use super::hero::{Hero, Peer};
use super::items::{Quality, Slot, Stat, Tpl};
use super::minimap::MapMarker;
use super::nav::{NavGrid, SpawnProbe};
use super::{HeroState, Rng, Run, Selection};

/// The first roll for a rare falls at 3:00, so the clock's first boss (2:15) walks out alone.
const RARE_FROM: f32 = 180.0;
/// Seconds between one roll and the next, from and to.
const RARE_EVERY: (f32, f32) = (150.0, 240.0);
/// Chance a roll brings a rare out.
const RARE_CHANCE: f32 = 0.6;
/// A roll finding a boss on the field, or no free spot, tries again after this many seconds.
const RETRY_SECS: f32 = 5.0;
/// A rare has the numbers of the horde at this place on its zone's ladder ([`data::rung`])...
const RARE_RUNG: f32 = 0.85;
/// ...this many times the health, experience, and this many times the damage.
const RARE_HP: f32 = 6.0;
const RARE_XP: u32 = 6;
const RARE_DAMAGE: f32 = 1.5;
/// A rare keeps its source's pace, held between these so it can be caught and dodged.
const RARE_SPEED: (f32, f32) = (3.6, 5.0);

/// The bonus boss has the finale's numbers, this many times the health and this many the damage
/// and experience.
const BONUS_HP: f32 = 2.0;
const BONUS_DAMAGE: f32 = 1.5;
const BONUS_XP: u32 = 2;

pub(super) fn plugin(app: &mut App) {
    app.add_systems(
        Update,
        rare_comes
            .after(benilla_world::schedule::WorldStage::Input)
            .run_if(super::run_on)
            .run_if(super::simulates)
            .run_if(|l: Res<benilla_app::survivors_host::loading_screen::LoadingScreen>| !l.covering()),
    );
}

/// What a rare is, before its zone's numbers: name, look, size, level and whether it takes a
/// great beast's room.
struct RareSpec {
    name: &'static str,
    display: u32,
    scale: f32,
    level: u32,
    big: bool,
}

const fn rare(name: &'static str, display: u32, scale: f32, level: u32) -> RareSpec {
    RareSpec {
        name,
        display,
        scale,
        level,
        big: false,
    }
}

const fn huge(name: &'static str, display: u32, scale: f32, level: u32) -> RareSpec {
    RareSpec {
        big: true,
        ..rare(name, display, scale, level)
    }
}

/// The rares of the battleground called `zone`. The names are from memory; the displays are the
/// zone roster's own, except the ones the plan's appendix 3 names (14403, 1269, 2026, 7113, 1305,
/// 7970, 4194), which it checked against the client's tables.
fn specs(zone: &str) -> &'static [RareSpec] {
    match zone {
        "The Barrens" => const { &[
            huge("Owatanka", 2764, 1.1, 30),
            rare("Brokespear", 1253, 1.7, 20),
            rare("Sister Rathtalon", 2163, 1.7, 23),
        ] },
        "The Dark Portal" => const { &[
            rare("Hematos", 798, 2.0, 54),
            rare("Mojo the Twisted", 14401, 1.7, 53),
        ] },
        "Gates of Ahn'Qiraj" => const { &[
            huge("Anubisath Warder", 15347, 0.9, 63),
            huge("Obsidian Eradicator", 15343, 1.7, 63),
        ] },
        "Fire Plume Ridge" => const { &[
            rare("Mushgog", 11634, 1.6, 54),
            rare("Zerillis", 8411, 1.0, 55),
        ] },
        "Gurubashi Arena" => const { &[
            rare("Kurmokk", 11031, 2.4, 44),
            rare("Bhag'thera", 11452, 2.0, 43),
        ] },
        "Elwynn Forest" => const { &[rare("Princess", 503, 1.5, 8)] },
        "Durotar" => const { &[
            rare("Taragaman the Hungerer", 7970, 1.7, 11),
            rare("Gazz'uz", 4194, 1.6, 10),
        ] },
        "Teldrassil" => const { &[
            rare("Githyiss the Vile", 760, 1.7, 8),
            rare("Lady Sathrah", 760, 2.0, 9),
        ] },
        "Dun Morogh" => const { &[
            huge("Mangeclaw", 8840, 1.5, 10),
            rare("Grawmug", 5607, 1.8, 10),
        ] },
        "Mulgore" => const { &[
            rare("Matriarch Swiftwing", 1229, 1.3, 10),
            rare("Old Greyhoof", 1221, 1.7, 10),
        ] },
        "Tirisfal Glades" => const { &[
            rare("Captain Vachon", 2483, 1.7, 10),
            rare("Gnawbone", 847, 1.7, 9),
        ] },
        // The Deadmines' minibosses, out for a walk.
        "Westfall" => const { &[
            rare("Rhahk'Zor", 14403, 1.6, 19),
            rare("Sneed's Shredder", 1269, 1.2, 20),
            rare("Mr. Smite", 2026, 1.6, 20),
            rare("Captain Greenskin", 7113, 1.8, 20),
            rare("Cookie", 1305, 1.8, 20),
        ] },
        "Duskwood" => const { &[
            rare("Mor'Ladim", 9786, 1.7, 35),
            rare("Skeletal Terror", 200, 1.8, 32),
        ] },
        "Redridge Mountains" => const { &[
            rare("Big Samras", 500, 1.7, 20),
            rare("Squiddic", 1305, 1.7, 19),
        ] },
        "Mount Hyjal" => const { &[
            rare("Deathmaw", 7949, 1.5, 52),
            rare("Jadefire Overlord", 11334, 1.7, 56),
        ] },
        "Naxxramas" => const { &[
            rare("Deathknight Captain", 16508, 2.3, 61),
            rare("Dread Plaguehound", 9022, 1.9, 60),
        ] },
        _ => const { &[] },
    }
}

/// A zone's bonus boss: who it is and the piece of loot it leaves.
struct BonusSpec {
    name: &'static str,
    display: u32,
    scale: f32,
    level: u32,
    loot: &'static Tpl,
}

/// The signature pieces of loot, named after the real drops (an assumption, like the rares'
/// names). Statics, so a [`super::items::BossDrop`] can carry them.
static ONYXIA_CLOAK: Tpl = Tpl {
    slot: Slot::Back,
    quality: Quality::Epic,
    name: "Onyxia Scale Cloak",
    stats: [Stat::Armor, Stat::Health],
};
static AZURESONG: Tpl = Tpl {
    slot: Slot::Weapon,
    quality: Quality::Epic,
    name: "Azuresong Mageblade",
    stats: [Stat::Might, Stat::Haste],
};
static ASHKANDI: Tpl = Tpl {
    slot: Slot::Weapon,
    quality: Quality::Epic,
    name: "Ashkandi, Greatsword of the Brotherhood",
    stats: [Stat::Might, Stat::Crit],
};
static TEMPERED_SWORD: Tpl = Tpl {
    slot: Slot::Weapon,
    quality: Quality::Epic,
    name: "Chromatically Tempered Sword",
    stats: [Stat::Might, Stat::Area],
};
static GAHZRILLA_FANG: Tpl = Tpl {
    slot: Slot::Weapon,
    quality: Quality::Epic,
    name: "Gahz'rilla Fang",
    stats: [Stat::Crit, Stat::Haste],
};
static ANNIHILATION: Tpl = Tpl {
    slot: Slot::Hands,
    quality: Quality::Epic,
    name: "Gauntlets of Annihilation",
    stats: [Stat::Might, Stat::Crit],
};
static DEATHCHARGER: Tpl = Tpl {
    slot: Slot::Trinket,
    quality: Quality::Epic,
    name: "Deathcharger's Reins",
    stats: [Stat::Speed, Stat::Magnet],
};

/// Onyxia, Azuregos, Nefarian, Chromaggus, Gahz'ranka, Viscidus and Baron Rivendare, with
/// displays checked against `CreatureDisplayInfo.dbc` (the plan's appendix 3). The scales are
/// first guesses against the model, to settle by screenshot.
const ONYXIA: BonusSpec = BonusSpec {
    name: "Onyxia",
    display: 8570,
    scale: 0.7,
    level: 63,
    loot: &ONYXIA_CLOAK,
};
const AZUREGOS: BonusSpec = BonusSpec {
    name: "Azuregos",
    display: 11460,
    scale: 0.7,
    level: 63,
    loot: &AZURESONG,
};
const NEFARIAN: BonusSpec = BonusSpec {
    name: "Nefarian",
    display: 11380,
    scale: 0.7,
    level: 63,
    loot: &ASHKANDI,
};
const CHROMAGGUS: BonusSpec = BonusSpec {
    name: "Chromaggus",
    display: 14367,
    scale: 0.8,
    level: 63,
    loot: &TEMPERED_SWORD,
};
const GAHZRANKA: BonusSpec = BonusSpec {
    name: "Gahz'ranka",
    display: 15288,
    scale: 1.0,
    level: 63,
    loot: &GAHZRILLA_FANG,
};
const VISCIDUS: BonusSpec = BonusSpec {
    name: "Viscidus",
    display: 15686,
    scale: 1.0,
    level: 63,
    loot: &ANNIHILATION,
};
const RIVENDARE: BonusSpec = BonusSpec {
    name: "Baron Rivendare",
    display: 10729,
    scale: 1.5,
    level: 63,
    loot: &DEATHCHARGER,
};

/// The bonus boss of the battleground called `zone`.
fn bonus_spec(zone: &str) -> Option<&'static BonusSpec> {
    Some(match zone {
        "Elwynn Forest" | "Westfall" | "Duskwood" | "Redridge Mountains" | "Tirisfal Glades"
        | "Dun Morogh" => &ONYXIA,
        "The Barrens" | "Durotar" | "Mulgore" | "Teldrassil" => &AZUREGOS,
        "Fire Plume Ridge" | "Mount Hyjal" => &NEFARIAN,
        "The Dark Portal" => &CHROMAGGUS,
        "Gurubashi Arena" => &GAHZRANKA,
        "Gates of Ahn'Qiraj" => &VISCIDUS,
        "Naxxramas" => &RIVENDARE,
        _ => return None,
    })
}

fn zone(map: usize) -> &'static str {
    data::MAPS[map.min(data::MAPS.len() - 1)].name
}

/// Battleground `map`'s rares, the second part of [`super::data::extras`]: the zone's horde at
/// [`RARE_RUNG`] along its ladder, [`RARE_HP`] times tougher, under the rare's own name and look.
pub(super) fn rares_for(map: usize) -> Vec<EnemyDef> {
    let base = data::rung(map, RARE_RUNG);
    specs(zone(map))
        .iter()
        .map(|s| EnemyDef {
            name: s.name,
            display: s.display,
            hp: base.hp * RARE_HP,
            speed: base.speed.clamp(RARE_SPEED.0, RARE_SPEED.1),
            damage: base.damage * RARE_DAMAGE,
            scale: s.scale,
            xp: base.xp * RARE_XP,
            level: s.level,
            big: s.big,
            ranged: None,
        })
        .collect()
}

/// Battleground `map`'s bonus boss, the last part of [`super::data::extras`]: the zone's finale,
/// [`BONUS_HP`] times as tough, wearing a world boss's look.
pub(super) fn bonus_for(map: usize) -> Vec<EnemyDef> {
    let Some(s) = bonus_spec(zone(map)) else {
        return Vec::new();
    };
    let finale = &finale::plan(map).finale;
    vec![EnemyDef {
        name: s.name,
        display: s.display,
        hp: finale.hp * BONUS_HP,
        speed: finale.speed,
        damage: finale.damage * BONUS_DAMAGE,
        scale: s.scale,
        xp: finale.xp * BONUS_XP,
        level: s.level,
        big: true,
        ranged: None,
    }]
}

/// How many rares and bonus bosses each battleground has, to cut them out of its extras.
static COUNTS: LazyLock<Vec<(usize, usize)>> = LazyLock::new(|| {
    (0..data::MAPS.len())
        .map(|m| (rares_for(m).len(), bonus_for(m).len()))
        .collect()
});

/// Battleground `map`'s rares, as they stand in its extras (so co-op finds them again by
/// [`enemies::def_ref`]).
pub(super) fn rares_of(map: usize) -> &'static [EnemyDef] {
    let all = data::extras(map);
    let (rares, bonus) = COUNTS[map.min(COUNTS.len() - 1)];
    &all[all.len() - bonus - rares..all.len() - bonus]
}

/// Battleground `map`'s bonus boss, as it stands in its extras.
pub(super) fn bonus_of(map: usize) -> Option<&'static EnemyDef> {
    let all = data::extras(map);
    (COUNTS[map.min(COUNTS.len() - 1)].1 > 0).then(|| &all[all.len() - 1])
}

/// The piece of loot battleground `map`'s bonus boss leaves.
pub(super) fn bonus_loot(map: usize) -> Option<&'static Tpl> {
    bonus_spec(zone(map)).map(|s| s.loot)
}

/// Where every standing hero is: this machine's (if it stands) and each standing co-op puppet's.
pub(super) fn standing(
    run: &Run,
    hero: &Query<&Transform, With<Hero>>,
    peers: &Query<(&Transform, &HeroState), (With<Peer>, Without<Hero>)>,
) -> Vec<Transform> {
    let mut at = Vec::new();
    if let Ok(h) = hero.single() {
        if run.dead_t.is_none() {
            at.push(*h);
        }
    }
    at.extend(
        peers
            .iter()
            .filter(|(_, s)| s.dead_t.is_none())
            .map(|(tf, _)| *tf),
    );
    at
}

/// The run second the first rare is rolled for; `WOW_SURVIVORS_RARE=<seconds>` moves it and makes
/// that first roll sure, to photograph it (debug mode only).
fn first_roll() -> (f32, bool) {
    static AT: OnceLock<Option<f32>> = OnceLock::new();
    let debug = *AT.get_or_init(|| {
        if !super::debug::active() {
            return None;
        }
        benilla_app::survivors_host::webenv::var("WOW_SURVIVORS_RARE").and_then(|v| v.parse().ok())
    });
    (debug.unwrap_or(RARE_FROM), debug.is_some())
}

/// When the next rare is rolled for.
#[derive(Default)]
struct Clock {
    /// The run second of the next roll; `None` until the run's first look.
    next: Option<f32>,
    /// Rolls made so far this run.
    rolls: u32,
    /// The run clock when last seen, to notice a new run.
    seen: f32,
}

/// From 3:00 on, every few minutes a roll may bring out a rare of the zone, one at a time, at the
/// boss ring around a standing hero. Not while a boss walks (a clock boss, the finale), not once
/// the finale looms, and never counted in `run.bosses`.
#[allow(clippy::too_many_arguments)]
fn rare_comes(
    mut commands: Commands,
    mut run: ResMut<Run>,
    mut rng: ResMut<Rng>,
    selection: Res<Selection>,
    nav: Res<NavGrid>,
    probe: SpawnProbe,
    world: WorldPoint,
    hero: Query<&Transform, With<Hero>>,
    peers: Query<(&Transform, &HeroState), (With<Peer>, Without<Hero>)>,
    foes: Query<&Enemy>,
    mut chat: MessageWriter<ChatLine>,
    mut clock: Local<Clock>,
) {
    // A new run starts the clock over.
    if run.elapsed < clock.seen {
        *clock = Clock::default();
    }
    clock.seen = run.elapsed;
    let (first, sure) = first_roll();
    let next = *clock.next.get_or_insert(first);
    if run.elapsed < next || run.party_down || run.encore || run.elapsed >= finale::finale_at() {
        return;
    }
    let list = rares_of(selection.map);
    let heroes = standing(&run, &hero, &peers);
    if list.is_empty() || heroes.is_empty() {
        return;
    }
    // One at a time, and not into a boss fight.
    if foes.iter().any(|e| e.boss && !e.dead) {
        clock.next = Some(run.elapsed + RETRY_SECS);
        return;
    }
    if (sure && clock.rolls == 0) || rng.f32() < RARE_CHANCE {
        let hpos = heroes[rng.index(heroes.len())].translation;
        let a = rng.range(0.0, std::f32::consts::TAU);
        let Some(pos) = enemies::spawn_spot(&nav, &probe, &world, hpos, a, BOSS_RADIUS) else {
            clock.next = Some(run.elapsed + 1.0);
            return;
        };
        let def = &list[rng.index(list.len())];
        let e = enemies::spawn_enemy(&mut commands, &mut run, def, pos, hpos, true, false);
        commands.entity(e).insert((Rare, MapMarker::Rare));
        run.announce(format!("A rare creature appears: {}!", def.name), 4.0);
        chat.write(ChatLine::text(
            ChatKind::Boss,
            format!("A rare creature appears: {}!", def.name),
        ));
    }
    clock.rolls += 1;
    clock.next = Some(run.elapsed + rng.range(RARE_EVERY.0, RARE_EVERY.1));
}

/// The rares' systems, for the balance simulator ([`super::balance`]); the bonus fight lies past
/// the simulator's clock.
#[cfg(test)]
#[allow(dead_code)]
pub(super) fn sim_systems() -> bevy::ecs::schedule::ScheduleConfigs<bevy::ecs::system::ScheduleSystem> {
    rare_comes.into_configs()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_zone_has_a_bonus_boss() {
        for (map, m) in data::MAPS.iter().enumerate() {
            // A route has no rares and no portal of its own.
            if super::super::route::route_of(map).is_some() {
                assert!(rares_of(map).is_empty() && bonus_of(map).is_none(), "{}", m.name);
                continue;
            }
            let bonus = bonus_for(map);
            assert_eq!(bonus.len(), 1, "{}", m.name);
            assert!(bonus_loot(map).is_some(), "{}", m.name);
            assert!(bonus_of(map).is_some(), "{}", m.name);
            assert!(!rares_for(map).is_empty(), "{} has no rares", m.name);
        }
    }

    #[test]
    fn rares_are_found_by_def_ref() {
        for (map, m) in data::MAPS.iter().enumerate() {
            let rares = rares_of(map);
            assert_eq!(rares.len(), rares_for(map).len(), "{}", m.name);
            for def in rares.iter().chain(bonus_of(map)) {
                let r = enemies::def_ref(map, def)
                    .unwrap_or_else(|| panic!("{} in {} has no ref", def.name, m.name));
                assert_eq!(r.0, 5, "{}", def.name);
                assert!(
                    std::ptr::eq(enemies::def_at(map, r).unwrap(), def),
                    "{} in {}",
                    def.name,
                    m.name
                );
            }
        }
    }

    #[test]
    fn a_rare_is_stronger_than_its_rung() {
        for (map, m) in data::MAPS.iter().enumerate() {
            let rung = data::rung(map, RARE_RUNG);
            for r in rares_of(map) {
                assert!(r.hp >= rung.hp * RARE_HP, "{} in {}", r.name, m.name);
                assert!(r.damage > rung.damage, "{} in {}", r.name, m.name);
                assert!(r.xp > rung.xp, "{} in {}", r.name, m.name);
                assert!(r.ranged.is_none(), "{}", r.name);
            }
        }
    }

    #[test]
    fn a_zone_names_each_rare_once() {
        for (map, m) in data::MAPS.iter().enumerate() {
            let names: Vec<_> = rares_of(map).iter().map(|r| r.name).collect();
            for (i, n) in names.iter().enumerate() {
                assert!(!names[..i].contains(n), "{n} twice in {}", m.name);
            }
        }
    }

    #[test]
    fn the_bonus_outweighs_the_finale() {
        for (map, m) in data::MAPS.iter().enumerate() {
            let finale = &finale::plan(map).finale;
            let Some(bonus) = bonus_of(map) else {
                assert!(super::super::route::route_of(map).is_some(), "{}", m.name);
                continue;
            };
            assert!(bonus.hp >= finale.hp * BONUS_HP, "{}", m.name);
            assert!(bonus.damage > finale.damage, "{}", m.name);
            assert!(bonus.xp > finale.xp, "{}", m.name);
            assert!(bonus.big && bonus.ranged.is_none(), "{}", m.name);
            assert_ne!(bonus.name, finale.name, "{}", m.name);
        }
    }

    #[test]
    fn the_bonus_loot_is_epic() {
        for map in (0..data::MAPS.len()).filter(|&m| super::super::route::route_of(m).is_none()) {
            assert_eq!(bonus_loot(map).unwrap().quality, Quality::Epic);
        }
    }
}
