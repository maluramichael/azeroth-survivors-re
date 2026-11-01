//! The balance simulator: whole runs played headless, with no game data, no window and no GPU,
//! through the mode's own systems in their order, on an empty, flat world, with a fixed step and
//! as fast as the CPU goes.
//!
//! The hero is flown by the same pilot as an automated run ([`super::auto::plan`]: toward the
//! nearest gem, away from anything close enough to bite) and takes a random card at each level.
//! The world has no trees or walls, so nothing blocks the horde or the hero; a number tuned here
//! is checked again in the game.
//!
//! `cargo survivors-sim` (an alias for this module's ignored test) plays every class, three
//! seeds each, and prints one line per run and one per class. Environment knobs:
//!
//! - `SIM_CLASSES=mage,warrior` (default: all)
//! - `SIM_ZONES=barrens,3` (default: The Barrens)
//! - `SIM_SEEDS=5` runs per class and zone (default 3)
//! - `SIM_SECS=300` the most seconds a run plays (default: the win, plus a minute)
//! - `SIM_LEVEL=20` the characters' level, for the meta bonus (default 1)
//! - `SIM_DIFFICULTY=heroic` (default normal)
//! - `SIM_TRACE=1` also prints each run's timeline: every 30 seconds the level, the spells and
//!   talents held, the enemies spawned and alive, the encircling waves, surges and bosses so far,
//!   the kills and the hero's health.
//!
//! In the suite, [`the_run_keeps_its_rhythm`] plays a few classes for five minutes and holds the
//! timeline to the game's rhythm (enemies come, waves and bosses arrive on time, the hero levels
//! and learns), so a change that breaks a run's flow fails a test.

#![cfg(test)]

use std::time::Duration;

use bevy::prelude::*;
use bevy::state::app::StatesPlugin;
use bevy::time::TimeUpdateStrategy;

use super::enemies::{Dying, Enemy};
use super::hero::Hero;
use super::units::flat;
use super::{abilities, auto, data, enemies, enrage, finale, hero, hud, surge};
use super::{HeroState, NetRole, Phase, Rng, Run, Selection};

/// The simulation's step: a 20 fps game.
const DT: f32 = 0.05;

/// What one run came to.
#[derive(Clone, Debug)]
struct Report {
    class: &'static str,
    zone: &'static str,
    seed: u64,
    won: bool,
    /// When the hero fell, if it did.
    died: Option<f32>,
    /// Seconds of play.
    secs: f32,
    level: u32,
    kills: u32,
    dealt: f32,
    taken: f32,
    /// The lowest health share the hero fell to.
    low: f32,
    /// The spells that did the most damage, most first.
    top: Vec<(&'static str, f32)>,
    /// Every [`CHECK_EVERY`] seconds, where the run stood.
    timeline: Vec<Check>,
    /// The last [`LAST_SECS`] seconds, one point a second: how the run ended.
    last: Vec<Moment>,
}

/// Seconds of a run's end [`Report::last`] keeps.
const LAST_SECS: usize = 20;

/// One second of a run's end.
#[derive(Clone, Debug, Default)]
struct Moment {
    t: f32,
    hp: f32,
    /// Enemies within [`CROWD_NEAR`] yards, alive in all, and a boss's health share while one
    /// is within [`BOSS_NEAR`] yards.
    crowd: usize,
    alive: usize,
    boss: Option<f32>,
    level: u32,
    /// Encircling waves so far, and enemies spawned in all.
    rings: u32,
    spawned: u32,
}

/// Seconds between two points of a run's timeline.
const CHECK_EVERY: f32 = 30.0;

/// Where a run stood at one moment.
#[derive(Clone, Debug, Default)]
struct Check {
    t: f32,
    level: u32,
    /// Spells and talents held.
    spells: usize,
    talents: usize,
    /// Enemies spawned since the start (elites among them), and alive now.
    spawned: u32,
    elites: u32,
    alive: usize,
    /// Encircling waves, surges begun and bosses so far.
    rings: u32,
    surges: u32,
    bosses: u32,
    kills: u32,
    /// The hero's health share.
    hp: f32,
    /// Health lost since the last point, the part of it with a boss within [`BOSS_NEAR`] yards,
    /// and the most enemies within [`CROWD_NEAR`] yards at once.
    taken: f32,
    by_boss: f32,
    crowd: usize,
}

/// A hit taken with a boss this near counts as the boss's, roughly.
const BOSS_NEAR: f32 = 6.0;
/// The enemies this near the hero are the ones biting.
const CROWD_NEAR: f32 = 4.0;

/// Where the hero stands against the horde this step: the enemies within [`CROWD_NEAR`] yards,
/// and the health share of a boss within [`BOSS_NEAR`], if one is.
fn pressure(world: &mut World) -> (usize, Option<f32>) {
    let Some(at) = world
        .query_filtered::<&Transform, With<Hero>>()
        .iter(world)
        .next()
        .map(|t| t.translation)
    else {
        return (0, None);
    };
    let mut crowd = 0;
    let mut boss: Option<f32> = None;
    for (t, e) in world.query::<(&Transform, &Enemy)>().iter(world) {
        if e.dead {
            continue;
        }
        let d = flat(t.translation - at).length();
        crowd += usize::from(d <= CROWD_NEAR + e.radius);
        if e.boss && d <= BOSS_NEAR + e.radius {
            boss = Some(e.hp / e.max_hp.max(1.0));
        }
    }
    (crowd, boss)
}

/// What the run's own state does not count.
#[derive(Resource, Default)]
struct Tally {
    spawned: u32,
    elites: u32,
    surges: u32,
    surging: bool,
}

/// Count the enemies as they come, and the surges as they begin.
fn tally(
    mut tally: ResMut<Tally>,
    surge: Res<surge::Surge>,
    new: Query<Has<enemies::Elite>, Added<Enemy>>,
) {
    for elite in &new {
        tally.spawned += 1;
        tally.elites += u32::from(elite);
    }
    if surge.active && !tally.surging {
        tally.surges += 1;
    }
    tally.surging = surge.active;
}

/// The run's settings.
#[derive(Clone, Copy)]
struct Setup {
    class: usize,
    map: usize,
    seed: u64,
    secs: f32,
    level: u32,
    difficulty: super::difficulty::Difficulty,
}

/// A seed spread over all 64 bits (splitmix64's finaliser), so neighbouring seeds play apart.
fn mix(seed: u64) -> u64 {
    let mut z = seed.wrapping_add(0x9E37_79B9_7F4A_7C15);
    z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
    z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
    (z ^ (z >> 31)) | 1
}

/// A race the class can be, for its racial: the first of the class's usual ones.
fn race_for(class: &data::ClassDef) -> u8 {
    match class.name {
        // Orc, Night Elf, Dwarf.
        "Shaman" => 2,
        "Druid" => 4,
        "Hunter" => 3,
        // Human.
        _ => 1,
    }
}

/// A headless app that plays one run of `setup`, its hero standing at the arena's centre.
fn sim_app(setup: Setup) -> App {
    let mut app = App::new();
    app.add_plugins((MinimalPlugins, StatesPlugin))
        .insert_resource(TimeUpdateStrategy::ManualDuration(Duration::from_secs_f32(
            DT,
        )))
        .init_state::<Phase>()
        .insert_resource(Rng(mix(setup.seed)))
        .insert_resource(auto::pilot(setup.class, setup.map))
        .insert_resource(Selection {
            class: Some(setup.class),
            map: setup.map,
            stage: setup.map,
            ..Selection::default()
        })
        .init_resource::<NetRole>()
        .init_resource::<Tally>()
        .init_resource::<super::GuestMail>()
        .init_resource::<enemies::Spawner>()
        .init_resource::<abilities::PendingHits>()
        .init_resource::<hud::Offer>()
        .init_resource::<super::achievements::RunPerks>()
        .init_resource::<super::focus::Focus>()
        .init_resource::<super::focus::Target>()
        .init_resource::<super::dmgmeter::DamageDone>()
        .init_resource::<super::prefs::Shake>()
        .init_resource::<surge::Surge>()
        .init_resource::<finale::Stage>()
        .init_resource::<super::nav::NavGrid>()
        // An empty world: no colliders for the spawn probe to find.
        .init_resource::<avian3d::collider_tree::ColliderTrees>()
        .init_resource::<benilla_app::survivors_host::creature_anim::PlaySeq>()
        .add_message::<abilities::Hit>()
        .add_message::<enemies::Loot>()
        .add_message::<super::chatlog::ChatLine>()
        .add_message::<benilla_app::survivors_host::creature_anim::CastEvent>()
        .add_message::<benilla_app::survivors_host::creature_anim::SpellGoTargets>()
        .add_message::<benilla_app::survivors_host::creature_anim::SwingMessage>()
        .add_message::<benilla_app::survivors_host::combat_text::CombatTextSpawn>()
        .add_message::<benilla_app::survivors_host::net::FieldChanged>()
        .add_systems(
            Update,
            (
                // The fight, as the game chains it.
                (
                    clear_fallen,
                    auto::plan,
                    fly,
                    hero::mark_picking,
                    enemies::spawn_enemies,
                    enemies::move_enemies,
                    abilities::cast_abilities,
                    abilities::tick_areas,
                    abilities::apply_hits,
                    enemies::tick_marks,
                    enemies::drop_loot,
                    enemies::collect_gems,
                )
                    .chain(),
                // Loot, the hero's health and the clock's events.
                (
                    super::items::sim_systems(),
                    super::chests::sim_systems(),
                    super::pickups::sim_systems(),
                    super::worldbuffs::sim_systems(),
                    hero::hero_vitals,
                    surge::tick,
                    enrage::enrage_bosses,
                    (finale::summon, finale::hunt, finale::finale_slain).chain(),
                    tally,
                )
                    .chain(),
            )
                .chain()
                .run_if(in_state(Phase::Playing)),
        )
        .add_systems(OnEnter(Phase::LevelUp), hud::roll_offer)
        .add_systems(Update, choose.run_if(in_state(Phase::LevelUp)));
    super::items::sim_init(&mut app);
    super::pickups::sim_init(&mut app);
    super::worldbuffs::sim_init(&mut app);
    app.init_resource::<Assets<Mesh>>()
        .init_resource::<Assets<StandardMaterial>>();
    benilla_world::world_point::init_world_point_resources(app.world_mut());

    let mut profile = super::profile::Profile {
        unlock_all: true,
        difficulty: setup.difficulty,
        ..Default::default()
    };
    for c in &mut profile.characters {
        c.level = setup.level;
    }
    let class = &data::CLASSES[setup.class];
    let race = race_for(class);
    let mut run = Run {
        difficulty: profile.difficulty,
        me: HeroState::new(
            class,
            race,
            profile.bonus(setup.class),
            profile.talent_bonus(setup.class),
        ),
        ..Run::default()
    };
    let serial = run.guid();
    let mut at = hero::arena(&data::MAPS[setup.map]);
    at.y = 0.0;
    let world = app.world_mut();
    let mut commands = world.commands();
    hero::spawn_hero(&mut commands, class, 0, race, 0, serial, at);
    world.flush();
    world.insert_resource(run);
    world.insert_resource(profile);
    world.resource_mut::<NextState<Phase>>().set(Phase::Playing);
    app
}

/// A fallen enemy goes a frame after it fell (the game fades it out with the engine), once
/// [`enemies::collect_gems`] has seen it fall.
fn clear_fallen(mut commands: Commands, fallen: Query<(Entity, Ref<Dying>)>) {
    for (e, d) in &fallen {
        if !d.is_added() {
            commands.entity(e).despawn();
        }
    }
}

/// Walk the hero where the pilot steers, held inside the leash; the flat world has nothing to
/// collide with.
fn fly(
    time: Res<Time>,
    run: Res<Run>,
    pilot: Res<auto::Auto>,
    selection: Res<Selection>,
    mut hero: Query<&mut Transform, With<Hero>>,
) {
    let Ok(mut tf) = hero.single_mut() else {
        return;
    };
    if run.dead_t.is_some() {
        return;
    }
    let dir = auto::steer(&pilot);
    let mut p = tf.translation + dir * run.stats().speed * run.haste_mult() * time.delta_secs();
    let map = selection.stage_def();
    let centre = hero::arena(map);
    let off = flat(p - centre);
    if off.length() > map.leash {
        let held = centre + off.normalize() * map.leash;
        p.x = held.x;
        p.z = held.z;
    }
    tf.translation = p;
}

/// Take a random card from the hand, as an automated run does.
fn choose(
    mut run: ResMut<Run>,
    mut offer: ResMut<hud::Offer>,
    mut rng: ResMut<Rng>,
    mut next: ResMut<NextState<Phase>>,
) {
    let n = offer.len().max(1);
    let i = rng.index(n);
    hud::pick(i, &mut run, &mut offer, &mut rng, &mut next);
    // An empty hand picks nothing: back to the fight.
    if offer.len() == 0 {
        next.set(Phase::Playing);
    }
}

/// Play one run of `setup` to its end or its time limit.
fn play(setup: Setup) -> Report {
    let mut app = sim_app(setup);
    let mut taken = 0.0f32;
    let mut low = 1.0f32;
    let mut last_hp: Option<f32> = None;
    let mut timeline = Vec::new();
    let mut last: Vec<Moment> = Vec::new();
    let mut next_moment = 0.0f32;
    let mut next_check = CHECK_EVERY;
    let steps = (setup.secs / DT).ceil() as u32;
    // Over the current stretch of the timeline.
    let (mut win_taken, mut win_boss, mut win_crowd) = (0.0f32, 0.0f32, 0usize);
    for _ in 0..steps {
        app.update();
        let (crowd, boss) = pressure(app.world_mut());
        let boss_near = boss.is_some();
        win_crowd = win_crowd.max(crowd);
        let now = app.world().resource::<Run>().elapsed;
        if now >= next_moment {
            next_moment += 1.0;
            let alive = app
                .world_mut()
                .query::<&Enemy>()
                .iter(app.world())
                .filter(|e| !e.dead)
                .count();
            let run = app.world().resource::<Run>();
            last.push(Moment {
                t: now,
                hp: run.hp.max(0.0) / run.stats().max_hp.max(1.0),
                crowd,
                alive,
                boss,
                level: run.level,
                rings: run.rings,
                spawned: app.world().resource::<Tally>().spawned,
            });
            if last.len() > LAST_SECS {
                last.remove(0);
            }
        }
        let hp = app.world().resource::<Run>().hp.max(0.0);
        if let Some(prev) = last_hp {
            if hp < prev {
                taken += prev - hp;
                win_taken += prev - hp;
                if boss_near {
                    win_boss += prev - hp;
                }
            }
        }
        last_hp = Some(hp);
        if app.world().resource::<Run>().elapsed >= next_check {
            next_check += CHECK_EVERY;
            timeline.push(Check {
                taken: win_taken,
                by_boss: win_boss,
                crowd: win_crowd,
                ..check(app.world_mut())
            });
            (win_taken, win_boss, win_crowd) = (0.0, 0.0, 0);
        }
        let world = app.world();
        let run = world.resource::<Run>();
        low = low.min(hp / run.stats().max_hp.max(1.0));
        if *world.resource::<State<Phase>>().get() == Phase::GameOver || run.party_down {
            break;
        }
    }
    let world = app.world();
    let run = world.resource::<Run>();
    let meter = world.resource::<super::dmgmeter::DamageDone>();
    let mut top: Vec<(&'static str, f32)> = meter.0.values().map(|(d, v)| (d.key, *v)).collect();
    top.sort_by(|a, b| b.1.total_cmp(&a.1));
    let dealt = top.iter().map(|t| t.1).sum();
    top.truncate(3);
    Report {
        class: data::CLASSES[setup.class].name,
        zone: data::MAPS[setup.map].name,
        seed: setup.seed,
        won: run.won,
        died: run.dead_t.map(|_| run.elapsed),
        secs: run.elapsed,
        level: run.level,
        kills: run.kills,
        dealt,
        taken,
        low,
        top,
        timeline,
        last,
    }
}

/// The run as it stands.
fn check(world: &mut World) -> Check {
    let alive = world
        .query::<&Enemy>()
        .iter(world)
        .filter(|e| !e.dead)
        .count();
    let tally = world.resource::<Tally>();
    let run = world.resource::<Run>();
    Check {
        t: run.elapsed,
        level: run.level,
        spells: run.abilities.len(),
        talents: run.passives.len(),
        spawned: tally.spawned,
        elites: tally.elites,
        alive,
        rings: run.rings,
        surges: tally.surges,
        bosses: run.bosses,
        kills: run.kills,
        hp: run.hp.max(0.0) / run.stats().max_hp.max(1.0),
        ..Check::default()
    }
}

fn print_timeline(r: &Report) {
    println!("\n{} at {}, seed {}", r.class, r.zone, r.seed);
    let head = [
        "t", "level", "spells", "talents", "spawned", "elites", "alive", "rings", "surges",
        "bosses", "kills", "hp", "taken", "by boss", "crowd",
    ];
    let widths = [6, 5, 6, 7, 7, 6, 5, 5, 6, 6, 6, 5, 6, 7, 5];
    let row = |cells: Vec<String>| {
        let line: Vec<String> = cells
            .iter()
            .zip(widths)
            .map(|(c, w)| format!("{c:>w$}"))
            .collect();
        println!("{}", line.join(" "));
    };
    row(head.iter().map(|h| h.to_string()).collect());
    for c in &r.timeline {
        row(vec![
            mm_ss(c.t),
            c.level.to_string(),
            c.spells.to_string(),
            c.talents.to_string(),
            c.spawned.to_string(),
            c.elites.to_string(),
            c.alive.to_string(),
            c.rings.to_string(),
            c.surges.to_string(),
            c.bosses.to_string(),
            c.kills.to_string(),
            format!("{:.0}%", c.hp * 100.0),
            format!("{:.0}", c.taken),
            format!("{:.0}", c.by_boss),
            c.crowd.to_string(),
        ]);
    }
}

fn print_end(r: &Report) {
    println!("  the last {LAST_SECS} seconds:");
    println!(
        "  {:>6} {:>5} {:>5} {:>5} {:>5} {:>5} {:>5} {:>7}",
        "t", "hp", "crowd", "alive", "boss", "level", "rings", "spawned"
    );
    for m in &r.last {
        let boss = m
            .boss
            .map_or("-".to_string(), |b| format!("{:.0}%", b * 100.0));
        println!(
            "  {:>6} {:>4.0}% {:>5} {:>5} {:>5} {:>5} {:>5} {:>7}",
            mm_ss(m.t),
            m.hp * 100.0,
            m.crowd,
            m.alive,
            boss,
            m.level,
            m.rings,
            m.spawned
        );
    }
}

fn mm_ss(secs: f32) -> String {
    let s = secs.max(0.0) as u32;
    format!("{}:{:02}", s / 60, s % 60)
}

/// `var` split at commas, each piece read by `read`; `all` when unset.
fn list<T>(var: &str, read: impl Fn(&str) -> Option<T>, all: impl Fn() -> Vec<T>) -> Vec<T> {
    match std::env::var(var) {
        Ok(v) if !v.trim().is_empty() => v
            .split(',')
            .map(|p| read(p).unwrap_or_else(|| panic!("{var}: cannot read `{}`", p.trim())))
            .collect(),
        _ => all(),
    }
}

fn number<T: std::str::FromStr>(var: &str, default: T) -> T {
    std::env::var(var)
        .ok()
        .and_then(|v| v.trim().parse().ok())
        .unwrap_or(default)
}

/// Every run the environment asks for, played on all cores; the reports in the asked order.
fn play_all(setups: Vec<Setup>) -> Vec<Report> {
    let threads = std::thread::available_parallelism().map_or(4, |n| n.get());
    let next = std::sync::atomic::AtomicUsize::new(0);
    let reports = std::sync::Mutex::new(vec![None; setups.len()]);
    std::thread::scope(|s| {
        for _ in 0..threads.min(setups.len()) {
            s.spawn(|| loop {
                let i = next.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                let Some(&setup) = setups.get(i) else { break };
                let report = play(setup);
                reports.lock().unwrap()[i] = Some(report);
            });
        }
    });
    reports
        .into_inner()
        .unwrap()
        .into_iter()
        .map(|r| r.expect("every run reports"))
        .collect()
}

fn print_table(reports: &[Report]) {
    println!(
        "\n{:<8} {:<22} {:>4} {:>8} {:>5} {:>6} {:>9} {:>6} {:>8} {:>5}  top spells",
        "class", "zone", "seed", "result", "level", "kills", "damage", "dps", "taken", "low"
    );
    for r in reports {
        let result = if r.won {
            "won".to_string()
        } else if let Some(t) = r.died {
            format!("died {}", mm_ss(t))
        } else {
            format!("at {}", mm_ss(r.secs))
        };
        let top: Vec<String> = r
            .top
            .iter()
            .map(|(n, v)| format!("{n} {:.0}%", v / r.dealt.max(1.0) * 100.0))
            .collect();
        println!(
            "{:<8} {:<22} {:>4} {:>8} {:>5} {:>6} {:>9.0} {:>6.0} {:>8.0} {:>4.0}%  {}",
            r.class,
            r.zone,
            r.seed,
            result,
            r.level,
            r.kills,
            r.dealt,
            r.dealt / r.secs.max(1.0),
            r.taken,
            r.low * 100.0,
            top.join(", ")
        );
    }
    println!(
        "\n{:<8} {:>5} {:>8} {:>7} {:>6} {:>7}",
        "class", "runs", "won", "level", "dps", "survived"
    );
    let mut classes: Vec<&str> = reports.iter().map(|r| r.class).collect();
    classes.dedup();
    for class in classes {
        let mine: Vec<&Report> = reports.iter().filter(|r| r.class == class).collect();
        let n = mine.len() as f32;
        let won = mine.iter().filter(|r| r.won).count();
        let level = mine.iter().map(|r| r.level as f32).sum::<f32>() / n;
        let dps = mine.iter().map(|r| r.dealt / r.secs.max(1.0)).sum::<f32>() / n;
        let secs = mine.iter().map(|r| r.secs).sum::<f32>() / n;
        println!(
            "{:<8} {:>5} {:>8} {:>7.1} {:>6.0} {:>7}",
            class,
            mine.len(),
            format!("{won}/{}", mine.len()),
            level,
            dps,
            mm_ss(secs)
        );
    }
}

/// `cargo survivors-sim`: every class through whole runs, the table printed.
#[test]
#[ignore = "whole runs for every class; `cargo survivors-sim` runs it"]
fn balance_table() {
    let classes = list("SIM_CLASSES", super::debug::class_index, || {
        (0..data::CLASSES.len()).collect()
    });
    let zones = list("SIM_ZONES", super::debug::map_index, || vec![0]);
    let seeds: u64 = number("SIM_SEEDS", 3);
    let secs: f32 = number("SIM_SECS", data::VICTORY_SECS + 60.0);
    let level: u32 = number("SIM_LEVEL", 1);
    let difficulty = std::env::var("SIM_DIFFICULTY").map_or(Default::default(), |d| {
        let d = d.trim().to_ascii_lowercase();
        super::difficulty::Difficulty::ALL
            .into_iter()
            .find(|x| x.label().to_ascii_lowercase() == d)
            .unwrap_or_else(|| panic!("SIM_DIFFICULTY: no difficulty `{d}`"))
    });
    let mut setups = Vec::new();
    for &map in &zones {
        for &class in &classes {
            for seed in 1..=seeds {
                setups.push(Setup {
                    class,
                    map,
                    seed,
                    secs,
                    level,
                    difficulty,
                });
            }
        }
    }
    let started = std::time::Instant::now();
    let reports = play_all(setups);
    if std::env::var_os("SIM_TRACE").is_some() {
        for r in &reports {
            print_timeline(r);
            if r.died.is_some() {
                print_end(r);
            }
        }
    }
    print_table(&reports);
    let played: f32 = reports.iter().map(|r| r.secs).sum();
    println!(
        "\n{} runs, {} of play in {:.1} s",
        reports.len(),
        mm_ss(played),
        started.elapsed().as_secs_f32()
    );
}

/// The run's rhythm, held for five minutes by a few classes and seeds: enemies keep coming, the
/// encircling waves, surges and bosses arrive on the game's clock, and the hero levels and learns.
/// A change that breaks how a run flows fails here; the bands are wide enough for a balance tweak.
#[test]
fn the_run_keeps_its_rhythm() {
    let mut setups = Vec::new();
    // Melee, ranged and caster: Warrior, Hunter and Mage.
    for class in [0, 2, 6] {
        for seed in 1..=2 {
            setups.push(Setup {
                class,
                map: 0,
                seed,
                secs: 300.0,
                level: 1,
                difficulty: Default::default(),
            });
        }
    }
    let at = |r: &Report, secs: f32| -> Check {
        r.timeline
            .iter()
            .find(|c| c.t >= secs - 0.5)
            .cloned()
            .unwrap_or_else(|| {
                panic!(
                    "{} seed {} has no point at {secs} s: {r:?}",
                    r.class, r.seed
                )
            })
    };
    for r in play_all(setups) {
        let who = format!("{} seed {}", r.class, r.seed);
        assert!(
            r.died.is_none_or(|t| t > 120.0),
            "{who} fell in the first two minutes: {r:?}"
        );
        // The first half minute: the horde comes and the hero levels.
        let c = at(&r, 30.0);
        assert!(c.spawned >= 15, "{who} at 0:30: {c:?}");
        assert!(c.level >= 2, "{who} at 0:30: {c:?}");
        // A minute in: kills, a second spell or talent, the first surge (45 s) has begun.
        let c = at(&r, 60.0);
        assert!(c.kills >= 5, "{who} at 1:00: {c:?}");
        assert!(c.spells + c.talents >= 2, "{who} at 1:00: {c:?}");
        assert_eq!(c.surges, 1, "{who} at 1:00: {c:?}");
        assert_eq!(
            c.rings, 0,
            "{who} at 1:00: the first ring is due at 1:07: {c:?}"
        );
        // The first ring (67.5 s) and the first boss (135 s) arrive on time.
        let c = at(&r, 90.0);
        assert_eq!(c.rings, 1, "{who} at 1:30: {c:?}");
        assert_eq!(c.bosses, 0, "{who} at 1:30: {c:?}");
        if r.died.is_none_or(|t| t > 150.0) {
            let c = at(&r, 150.0);
            assert_eq!(c.bosses, 1, "{who} at 2:30: {c:?}");
            assert!(c.level >= 5, "{who} at 2:30: {c:?}");
            assert!(c.alive <= 400, "{who} at 2:30: {c:?}");
        }
    }
}
