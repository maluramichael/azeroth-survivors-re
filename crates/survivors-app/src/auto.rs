//! `WOW_SURVIVORS_AUTO=<class name>`: a hands-off run for testing. It picks the class and its first
//! look, begins once the world is up, takes a random card at every level, walks to the nearest
//! gem away from the crowd and logs the run every ten seconds; `WOW_SURVIVORS_SHOTS=<dir>` also
//! saves a screenshot every few seconds, which needs no window focus. A `:idle`, `:all` or `:late`
//! suffix on the class name changes the run, as the fields below say. `WOW_SURVIVORS_MAP=<index or
//! name>` picks the battleground. `WOW_SURVIVORS_WINDOWS=1` first opens the hub's windows one by
//! one and photographs each ([`tour`]).

use bevy::prelude::*;
use bevy::render::view::screenshot::{save_to_disk, Screenshot};

use super::enemies::{Enemy, Gem};
use super::hero::Hero;
use super::hud::{self, Offer};
use super::route::{self, RouteRun};
use super::units::flat;
use super::{data, Phase, Rng, Run, Selection};

#[derive(Resource)]
pub(super) struct Auto {
    class: usize,
    /// An index into [`data::MAPS`].
    map: usize,
    /// `<class>:idle`: stand still, to reach the results screen quickly.
    idle: bool,
    /// `<class>:all`: the class's whole ability pool from the first second.
    all: bool,
    /// `<class>:late`: the whole pool at its top level, two thirds of the way in, for the crowd at
    /// its thickest.
    late: bool,
    frames: u32,
    wait: f32,
    shots: Option<std::path::PathBuf>,
    shot_t: f32,
    shot_n: u32,
    log_t: f32,
    dir: Vec3,
    /// The screens already captured once: the menu, the first level-ups, the results.
    seen: Vec<String>,
    /// `WOW_SURVIVORS_WINDOWS`: the next step of [`TOUR`], until it is done.
    tour: Option<usize>,
    /// The run's one look at the pause menu, [`PAUSE_AT`] seconds in, is behind it.
    paused_once: bool,
    /// On a route, how the walk to the next point is going ([`Watch`]).
    watch: Watch,
}

/// Seconds of pushing toward a road point after which the pilot checks it got somewhere.
const STUCK_SECS: f32 = 4.0;
/// The yards of progress toward the point that [`STUCK_SECS`] of pushing must bring.
const STUCK_YARDS: f32 = 3.0;
/// On a route the pilot only goes for a gem this near; the road comes first.
const ROUTE_GEM_REACH: f32 = 12.0;

/// The pilot's watch on its walk down a route's road: where the current window began and how many
/// seconds it has pushed toward the point since, so a cliff or a wall in the straight line shows
/// up as a logged position ([`Watch::step`]) for a road point to be set there.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
struct Watch {
    /// The point the window is on; a new one starts a new window.
    point: usize,
    /// How far the point was when the window began.
    from: f32,
    /// Seconds spent steering toward the point in the window.
    pushed: f32,
}

impl Watch {
    /// One frame: `point` is `dist` yards off, and `pushing` says the pilot steers toward it
    /// (not away from a crowd or toward a gem). True when a window of [`STUCK_SECS`] pushing ended
    /// with less than [`STUCK_YARDS`] gained; the next window begins then.
    fn step(&mut self, point: usize, dist: f32, pushing: bool, dt: f32) -> bool {
        if self.point != point {
            *self = Watch {
                point,
                from: dist,
                pushed: 0.0,
            };
            return false;
        }
        if pushing {
            self.pushed += dt;
        }
        if self.pushed < STUCK_SECS {
            return false;
        }
        let stuck = self.from - dist < STUCK_YARDS;
        self.from = dist;
        self.pushed = 0.0;
        stuck
    }
}

/// How far into a run the autopilot opens the pause menu once and photographs it (`pause.png`),
/// with the crowd, its health bars and its damage numbers under the dimmer.
const PAUSE_AT: f32 = 20.0;

/// The hub's windows the tour opens, in order, and the screenshot each gives.
const TOUR: [&str; 8] = [
    "achievements",
    "collection",
    "loot",
    "trainer",
    "options",
    "coop",
    "questlog",
    "worldmap",
];

impl Auto {
    /// One screenshot named for this screen, the first time it shows.
    fn capture_once(&mut self, commands: &mut Commands, name: String) {
        if self.seen.contains(&name) {
            return;
        }
        // A browser has no folder to write to: the moment goes to the console instead, for a
        // DevTools driver to screenshot.
        let Some(dir) = &self.shots else {
            info!("survivors auto: capture {name}");
            self.seen.push(name);
            return;
        };
        let path = dir.join(format!("{name}.png"));
        commands
            .spawn(Screenshot::primary_window())
            .observe(save_to_disk(path));
        self.seen.push(name);
    }
}

pub(super) fn from_env() -> Option<Auto> {
    let want = benilla_app::survivors_host::webenv::var("WOW_SURVIVORS_AUTO")?;
    let (name, mode) = want.trim().split_once(':').unwrap_or((want.trim(), ""));
    let idle = mode.eq_ignore_ascii_case("idle");
    let late = mode.eq_ignore_ascii_case("late");
    let all = late || mode.eq_ignore_ascii_case("all");
    let class = super::debug::class_index(name).unwrap_or(0);
    let map = benilla_app::survivors_host::webenv::var("WOW_SURVIVORS_MAP")
        .and_then(|m| super::debug::map_index(&m))
        .unwrap_or(0);
    let shots = std::env::var_os("WOW_SURVIVORS_SHOTS").map(std::path::PathBuf::from);
    if let Some(dir) = &shots {
        let _ = std::fs::create_dir_all(dir);
    }
    Some(Auto {
        class,
        map,
        idle,
        all,
        late,
        frames: 0,
        wait: 0.0,
        shots,
        shot_t: 3.0,
        shot_n: 0,
        log_t: 0.0,
        dir: Vec3::ZERO,
        seen: Vec::new(),
        tour: benilla_app::survivors_host::webenv::var("WOW_SURVIVORS_WINDOWS").map(|_| 0),
        paused_once: false,
        watch: Watch::default(),
    })
}

/// The pilot alone, for the balance simulator ([`super::balance`]): no screenshots, no tour.
#[cfg(test)]
pub(super) fn pilot(class: usize, map: usize) -> Auto {
    Auto {
        class,
        map,
        idle: false,
        all: false,
        late: false,
        frames: 0,
        wait: 0.0,
        shots: None,
        shot_t: 0.0,
        shot_n: 0,
        log_t: 0.0,
        dir: Vec3::ZERO,
        seen: Vec::new(),
        tour: None,
        paused_once: true,
        watch: Watch::default(),
    }
}

/// The walk the autopilot steers, for [`super::hero::drive_hero`].
pub(super) fn steer(auto: &Auto) -> Vec3 {
    auto.dir
}

/// Toward the nearest gem, pushed off by every enemy close enough to bite, drifting home when
/// there is nothing to pick up. A co-op host first goes to a fallen guest and stays beside it
/// until it is back up. On a route's road it walks to the next point instead of circling, leaving
/// it only for a gem close by, and logs where it sticks.
pub(super) fn plan(
    time: Res<Time<Real>>,
    mut auto: ResMut<Auto>,
    bounds: Res<super::route::Bounds>,
    selection: Res<Selection>,
    route_run: Res<RouteRun>,
    phase: Res<State<Phase>>,
    loading: Option<Res<benilla_app::survivors_host::loading_screen::LoadingScreen>>,
    hero: Query<&Transform, With<Hero>>,
    gems: Query<&Transform, (With<Gem>, Without<Hero>)>,
    foes: Query<(&Transform, &Enemy), Without<Hero>>,
    fallen: Query<(&Transform, &super::HeroState), (With<super::hero::Peer>, Without<Hero>)>,
) {
    let Some(hero) = hero.single().ok().filter(|_| !auto.idle) else {
        auto.dir = Vec3::ZERO;
        return;
    };
    let at = hero.translation;
    if let Some(to) = fallen
        .iter()
        .filter(|(_, s)| s.dead_t.is_some())
        .map(|(t, _)| flat(t.translation - at))
        .min_by(|a, b| a.length().total_cmp(&b.length()))
    {
        auto.dir = if to.length() > 1.5 {
            to.normalize_or_zero()
        } else {
            Vec3::ZERO
        };
        return;
    }
    // The road: the heading to the next point while the route is under way.
    let road = route::route_of(selection.stage)
        .filter(|_| route_run.placed && !route_run.arrived)
        .and_then(|def| route::heading(def, &route_run, at));
    let reach = if road.is_some() {
        ROUTE_GEM_REACH
    } else {
        30.0
    };
    let mut want = gems
        .iter()
        .map(|t| flat(t.translation - at))
        .filter(|d| d.length() < reach)
        .min_by(|a, b| a.length().total_cmp(&b.length()))
        .map_or(Vec3::ZERO, |d| d.normalize_or_zero());
    let mut road_dir = Vec3::ZERO;
    if let Some(a) = road {
        road_dir = Vec3::new(a.cos(), 0.0, a.sin());
    }
    if want == Vec3::ZERO {
        if road.is_some() {
            want = road_dir;
        } else {
            let a = time.elapsed_secs() * 0.35;
            want = Vec3::new(a.cos(), 0.0, a.sin()) * 0.5;
            // Circle near the centre; on a route's open road there is none.
            let home = flat(bounds.centre - at);
            if bounds.fenced && home.length() > (bounds.leash * 0.5).min(40.0) {
                want += home.normalize();
            }
        }
    }
    let mut away = Vec3::ZERO;
    for (t, e) in &foes {
        if e.dead {
            continue;
        }
        let d = flat(at - t.translation);
        let len = d.length();
        let reach = 7.0 + e.radius;
        if len < reach && len > 0.01 {
            away += d / len * (reach - len) / reach * 3.0;
        }
    }
    // On the road the horde mostly comes from ahead: step aside from it, never back, or the
    // pilot is pushed home and never reaches a station.
    if road.is_some() {
        let back = away.dot(road_dir);
        if back < 0.0 {
            away -= road_dir * back;
        }
    }
    auto.dir = (want + away).normalize_or_zero();

    // Watch the walk: standing still against a cliff shows as pushing without getting nearer.
    // Only a running game counts (a card pick, the pause or a loading screen stand still too).
    let covered = loading.is_some_and(|l| l.covering());
    let next = route::route_of(selection.stage)
        .filter(|_| road.is_some() && *phase.get() == Phase::Playing && !covered)
        .and_then(|def| def.points().get(route_run.next));
    let Some(next) = next else {
        auto.watch = Watch::default();
        return;
    };
    let dist = flat(next.at - at).length();
    let pushing = auto.dir.dot(road_dir) > 0.5;
    if auto
        .watch
        .step(route_run.next, dist, pushing, time.delta_secs())
    {
        let wow = benilla_assets::coords::bevy_to_wow(at);
        info!(
            "survivors route: stuck at wow ({:.0}, {:.0}) toward point {}",
            wow[0], wow[1], route_run.next
        );
    }
}

#[allow(clippy::too_many_arguments)]
pub(super) fn drive(
    mut commands: Commands,
    time: Res<Time<Real>>,
    mut auto: ResMut<Auto>,
    phase: Res<State<Phase>>,
    mut next: ResMut<NextState<Phase>>,
    mut selection: ResMut<Selection>,
    mut run: ResMut<Run>,
    profile: Res<super::profile::Profile>,
    mut offer: ResMut<Offer>,
    mut rng: ResMut<Rng>,
    loading: Res<benilla_app::survivors_host::loading_screen::LoadingScreen>,
    mut talents: ResMut<super::talents::TalentsUi>,
    heroes: Query<&Transform, With<Hero>>,
    enemies: Query<&Enemy>,
    (role, map, route_run): (
        Res<super::NetRole>,
        Res<super::worldmap::WorldMapUi>,
        Res<RouteRun>,
    ),
    mut options: ResMut<super::options::OptionsUi>,
) {
    let dt = time.delta_secs();
    auto.wait += dt;
    auto.frames += 1;
    match phase.get() {
        Phase::Menu => {
            if selection.map != auto.map {
                selection.map = auto.map;
                selection.version += 1;
                auto.wait = 0.0;
            } else if selection.class != Some(auto.class) {
                selection.class = Some(auto.class);
                selection.looks.clear();
                selection.look = None;
                selection.version += 1;
                auto.wait = 0.0;
            } else if heroes.is_empty() || loading.covering() {
                auto.wait = 0.0;
            } else if auto.tour.is_some_and(|t| t < TOUR.len()) {
                // [`tour`] is photographing the hub's windows.
                auto.wait = 0.0;
            } else if talents.open {
                // `WOW_SURVIVORS_TALENTS` opened the talent window: photograph it, then close it.
                if auto.wait > 2.0 {
                    auto.capture_once(&mut commands, "talents".into());
                }
                if auto.wait > 3.0 {
                    talents.open = false;
                    auto.wait = 0.0;
                }
            } else if auto.wait > 2.0 && auto.wait < 3.0 {
                auto.capture_once(&mut commands, "menu".into());
            } else if *role == super::NetRole::Client {
                // A guest never starts a run: the host does, and `coop::client` follows it in.
                auto.wait = 3.0;
            } else if auto.wait > 3.0 && hud::start_run(&mut run, &mut selection, &profile) {
                info!(
                    "survivors auto: run begins as {} at {}",
                    run.class.name,
                    selection.map_def().name
                );
                if auto.all {
                    for key in run.class.pool {
                        if !run.abilities.iter().any(|o| o.def.key == *key) {
                            run.abilities.push(super::Owned {
                                def: data::ability(key),
                                level: 1,
                                cd: 0.5,
                            });
                        }
                    }
                }
                if auto.late {
                    for o in &mut run.abilities {
                        o.level = data::MAX_ABILITY_LEVEL;
                    }
                    run.elapsed = data::VICTORY_SECS * 2.0 / 3.0;
                    run.bosses = 3;
                    run.rings = 4;
                }
                next.set(Phase::Playing);
                auto.paused_once = false;
                auto.wait = 0.0;
            }
        }
        Phase::Playing => {
            if !auto.paused_once
                && !auto.idle
                && *role == super::NetRole::Solo
                && run.elapsed > PAUSE_AT
            {
                auto.paused_once = true;
                next.set(Phase::Paused);
            }
            auto.wait = 0.0;
        }
        Phase::Paused => {
            // The pause screen, then the Options window's Graphics tab over the live run. A map
            // opened by a script holds the pause until the script closes it, for its screenshot.
            if map.open {
                auto.wait = 0.0;
            } else if auto.wait > 1.0 && auto.wait < 2.0 {
                auto.capture_once(&mut commands, "pause".into());
            } else if auto.wait >= 2.0 && auto.wait < 3.5 {
                options.open = true;
                options.tab = super::options::OptionsTab::Graphics;
                if auto.wait > 3.0 {
                    auto.capture_once(&mut commands, "options-graphics".into());
                }
            } else if auto.wait >= 3.5 {
                options.open = false;
                options.tab = super::options::OptionsTab::default();
                next.set(Phase::Playing);
                auto.wait = 0.0;
            }
        }
        Phase::LevelUp => {
            // After the deal-in, so the shot shows the cards settled.
            if auto.wait > 1.2 && (run.level <= 3 || run.level == 10) {
                auto.capture_once(&mut commands, format!("levelup-{}", run.level));
            }
            if auto.wait > 1.6 {
                let n = offer.len().max(1);
                let i = (rng.range(0.0, n as f32) as usize).min(n - 1);
                hud::pick(i, &mut run, &mut offer, &mut rng, &mut next);
                auto.wait = 0.0;
            }
        }
        Phase::GameOver => {
            if auto.wait > 1.5 {
                auto.capture_once(&mut commands, "over".into());
            }
            if auto.wait > 6.0 {
                next.set(Phase::Menu);
                auto.wait = 0.0;
            }
        }
    }

    auto.log_t -= dt;
    if auto.log_t <= 0.0 && *phase.get() != Phase::Menu {
        auto.log_t = 10.0;
        let fps = auto.frames as f32 / 10.0;
        auto.frames = 0;
        let alive = enemies.iter().filter(|e| !e.dead).count();
        let build: Vec<String> = run
            .abilities
            .iter()
            .map(|o| format!("{}:{}", o.def.key, o.level))
            .chain(run.passives.iter().map(|(p, l)| format!("{}:{}", p.key, l)))
            .collect();
        // On a route: the stations reached, the point under way and how far off it is.
        let road = route::route_of(selection.stage).map_or(String::new(), |def| {
            let left = heroes.single().ok().and_then(|h| {
                def.points()
                    .get(route_run.next)
                    .map(|p| flat(p.at - h.translation).length())
            });
            format!(
                " route=[stop {}/{} next {} left {} frac {:.2}]",
                route_run.stop,
                def.stations.len(),
                route_run.next,
                left.map_or("-".to_string(), |d| format!("{d:.0}yd")),
                run.route_frac
            )
        });
        info!(
            "survivors auto: {:?} t={:.0}s fps={fps:.0} alive={alive} level={} hp={:.0} kills={} damage={:.0} build=[{}]{road}",
            phase.get(),
            run.elapsed,
            run.level,
            run.hp,
            run.kills,
            run.damage_done,
            build.join(" ")
        );
    }

    if let Some(dir) = auto.shots.clone() {
        auto.shot_t -= dt;
        if auto.shot_t <= 0.0 {
            auto.shot_t = 6.0;
            auto.shot_n += 1;
            let path = dir.join(format!("shot-{:03}.png", auto.shot_n));
            commands
                .spawn(Screenshot::primary_window())
                .observe(save_to_disk(path));
        }
    }
}

/// The open flags of the hub's windows, for [`tour`].
type Windows<'w> = (
    ResMut<'w, super::achievements::AchievementsUi>,
    ResMut<'w, super::collection::CollectionUi>,
    ResMut<'w, super::loottables::LootUi>,
    ResMut<'w, super::trainer::TrainerUi>,
    ResMut<'w, super::options::OptionsUi>,
    // Present only in a build with co-op.
    Option<ResMut<'w, super::CoopUi>>,
    ResMut<'w, super::questlog::QuestLogUi>,
    ResMut<'w, super::worldmap::HubMapUi>,
);

/// `WOW_SURVIVORS_WINDOWS`: at the menu, open each of [`TOUR`]'s windows alone, photograph it
/// after it settles and go on. The book gets some pieces first, so the shot is not all question
/// marks; an automated run's profile is never saved.
#[allow(clippy::too_many_arguments)]
pub(super) fn tour(
    mut commands: Commands,
    time: Res<Time<Real>>,
    mut auto: ResMut<Auto>,
    phase: Res<State<Phase>>,
    loading: Res<benilla_app::survivors_host::loading_screen::LoadingScreen>,
    heroes: Query<(), With<Hero>>,
    mut profile: ResMut<super::profile::Profile>,
    mut windows: Windows,
    mut wait: Local<f32>,
) {
    let Some(step) = auto.tour.filter(|&t| t < TOUR.len()) else {
        return;
    };
    if *phase.get() != Phase::Menu || heroes.is_empty() || loading.covering() {
        *wait = 0.0;
        return;
    }
    if step == 0 && *wait == 0.0 {
        for tpl in super::items::table::TEMPLATES.iter().step_by(3) {
            profile.collection.insert(tpl.name.to_string());
        }
    }
    *wait += time.delta_secs();
    let open = |w: &mut Windows, on: Option<usize>| {
        w.0.open = on == Some(0);
        w.1.open = on == Some(1);
        w.2.open = on == Some(2);
        w.3.open = on == Some(3);
        w.4.open = on == Some(4);
        if let Some(coop) = w.5.as_mut() {
            coop.open = on == Some(5);
        }
        w.6.open = on == Some(6);
        w.7.open = on == Some(7);
    };
    open(&mut windows, Some(step));
    if *wait > 2.5 {
        auto.capture_once(&mut commands, format!("win-{}", TOUR[step]));
    }
    if *wait > 3.0 {
        *wait = 0.0;
        auto.tour = Some(step + 1);
        if step + 1 == TOUR.len() {
            open(&mut windows, None);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Push toward point 1 for `secs`, getting `gain` yards nearer each second from `start`.
    fn push(w: &mut Watch, start: f32, gain: f32, secs: u32) -> Vec<bool> {
        (1..=secs)
            .map(|t| w.step(1, start - gain * t as f32, true, 1.0))
            .collect()
    }

    #[test]
    fn a_pilot_that_gets_nowhere_is_stuck_after_four_seconds_of_pushing() {
        let mut w = Watch::default();
        assert!(
            !w.step(1, 300.0, true, 1.0),
            "the first frame only opens the window"
        );
        let calls = push(&mut w, 300.0, 0.2, 8);
        // The next window begins at once: stuck again four seconds later.
        let at: Vec<usize> = (0..calls.len()).filter(|&i| calls[i]).collect();
        assert_eq!(at, [3, 7], "{calls:?}");
    }

    #[test]
    fn a_pilot_that_closes_in_is_never_stuck() {
        let mut w = Watch::default();
        w.step(1, 300.0, true, 1.0);
        assert!(push(&mut w, 300.0, 5.0, 20).iter().all(|&s| !s));
    }

    #[test]
    fn dodging_and_gems_do_not_count_as_pushing() {
        let mut w = Watch::default();
        w.step(1, 300.0, true, 1.0);
        for _ in 0..30 {
            assert!(!w.step(1, 300.0, false, 1.0));
        }
    }

    #[test]
    fn a_new_point_begins_a_new_window() {
        let mut w = Watch::default();
        w.step(1, 300.0, true, 1.0);
        for _ in 0..3 {
            w.step(1, 300.0, true, 1.0);
        }
        assert!(!w.step(2, 500.0, true, 1.0));
        assert_eq!(w.pushed, 0.0);
        assert_eq!(w.from, 500.0);
    }
}
