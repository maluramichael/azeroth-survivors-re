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
}

/// The hub's windows the tour opens, in order, and the screenshot each gives.
const TOUR: [&str; 6] = ["achievements", "collection", "loot", "trainer", "options", "coop"];

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
    let class = data::CLASSES
        .iter()
        .position(|c| c.name.eq_ignore_ascii_case(name))
        .unwrap_or(0);
    let map = benilla_app::survivors_host::webenv::var("WOW_SURVIVORS_MAP").map_or(0, |m| {
        let m = m.trim().to_ascii_lowercase();
        m.parse::<usize>()
            .ok()
            .filter(|&i| i < data::MAPS.len())
            .unwrap_or_else(|| {
                data::MAPS
                    .iter()
                    .position(|d| d.name.to_ascii_lowercase().contains(&m))
                    .unwrap_or(0)
            })
    });
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
    })
}

/// The walk the autopilot steers, for [`super::hero::drive_hero`].
pub(super) fn steer(auto: &Auto) -> Vec3 {
    auto.dir
}

/// Toward the nearest gem, pushed off by every enemy close enough to bite, drifting home when
/// there is nothing to pick up.
pub(super) fn plan(
    time: Res<Time<Real>>,
    mut auto: ResMut<Auto>,
    selection: Res<Selection>,
    hero: Query<&Transform, With<Hero>>,
    gems: Query<&Transform, (With<Gem>, Without<Hero>)>,
    foes: Query<(&Transform, &Enemy), Without<Hero>>,
) {
    let Some(hero) = hero.single().ok().filter(|_| !auto.idle) else {
        auto.dir = Vec3::ZERO;
        return;
    };
    let at = hero.translation;
    let mut want = gems
        .iter()
        .map(|t| flat(t.translation - at))
        .filter(|d| d.length() < 30.0)
        .min_by(|a, b| a.length().total_cmp(&b.length()))
        .map_or(Vec3::ZERO, |d| d.normalize_or_zero());
    if want == Vec3::ZERO {
        let a = time.elapsed_secs() * 0.35;
        want = Vec3::new(a.cos(), 0.0, a.sin()) * 0.5;
        let map = selection.map_def();
        let home = flat(super::hero::arena(map) - at);
        if home.length() > (map.leash * 0.5).min(40.0) {
            want += home.normalize();
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
    auto.dir = (want + away).normalize_or_zero();
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
    heroes: Query<(), With<Hero>>,
    enemies: Query<&Enemy>,
    role: Res<super::NetRole>,
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
        _ => auto.wait = 0.0,
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
        info!(
            "survivors auto: {:?} t={:.0}s fps={fps:.0} alive={alive} level={} hp={:.0} kills={} damage={:.0} build=[{}]",
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
