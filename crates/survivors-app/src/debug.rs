//! A debug mode for quick iteration, and the class and zone picked from the command line.
//!
//! - `WOW_SURVIVORS_CLASS=<name>` and `WOW_SURVIVORS_MAP=<name or index>` (`--class`, `--zone`)
//!   preselect the class and the battleground at the menu, over the last pick ([`preselect`]).
//! - `WOW_SURVIVORS_DEBUG=1` (`--debug`) skips the menu into a run with that pick, saves nothing
//!   (as an automated run does), starts the hero unhurtable and lays the overlay over the run:
//!   the leash and the spawn rings on the ground, the hero's position and the keys.
//!
//! The keys, debug mode only: F5 holds the waves and the clock, F6 clears the field, F7 toggles
//! god mode, F8 the overlay, F9 puts the hero at the edge the camera looks at, F10 steps MSAA and
//! F11 the anti-aliasing (FXAA, SMAA) live; F3 drops the finale ([`super::finale`]) and F4 jumps
//! to a guard post ([`super::guards`]). On a route ([`super::route`]) F9 puts the hero a little
//! short of the next station instead (the road points between are skipped), F1 short of the next
//! road point, and F2 says where the hero stands in WoW coordinates, to measure the road with.

use std::sync::atomic::{AtomicBool, Ordering};

use benilla_world::world_point::WorldPoint;
use bevy::asset::RenderAssetUsages;
use bevy::camera::visibility::NoFrustumCulling;
use bevy::mesh::{Indices, PrimitiveTopology, VertexAttributeValues};
use bevy::prelude::*;

use super::enemies::{self, Enemy};
use super::hero::{CamRig, Hero};
use super::kit::{self, KitFonts};
use super::route::{self, Point, RouteRun};
use super::units::flat;
use super::{data, NetRole, Phase, Run, Selection};

/// God mode: the hero takes no damage ([`super::hero::hurt_hero`]).
static GOD: AtomicBool = AtomicBool::new(false);
/// The waves and the run clock stand still ([`super::enemies::spawn_enemies`],
/// [`super::hero::hero_vitals`]).
static HOLD: AtomicBool = AtomicBool::new(false);

/// Whether god mode is on.
pub(super) fn god() -> bool {
    GOD.load(Ordering::Relaxed)
}

/// Whether the waves and the run clock are held.
pub(super) fn waves_held() -> bool {
    HOLD.load(Ordering::Relaxed)
}

/// Whether `WOW_SURVIVORS_DEBUG` asks for the debug mode.
pub(super) fn active() -> bool {
    benilla_app::survivors_host::webenv::var("WOW_SURVIVORS_DEBUG").is_some_and(|v| v != "0")
}

/// The class named `name` (any case), as an index into [`data::CLASSES`].
pub(super) fn class_index(name: &str) -> Option<usize> {
    let name = name.trim();
    data::CLASSES
        .iter()
        .position(|c| c.name.eq_ignore_ascii_case(name))
}

/// The battleground `m` names, by index or by a piece of its name (`barrens`, `3`), as an index
/// into [`data::MAPS`].
pub(super) fn map_index(m: &str) -> Option<usize> {
    let m = m.trim().to_ascii_lowercase();
    m.parse::<usize>()
        .ok()
        .filter(|&i| i < data::MAPS.len())
        .or_else(|| {
            data::MAPS
                .iter()
                .position(|d| d.name.to_ascii_lowercase().contains(&m))
        })
}

/// The debug mode's state.
#[derive(Resource)]
struct DebugMode {
    /// The run the mode starts at launch has begun.
    started: bool,
    /// Seconds the menu has stood ready, so the start waits for the pick to settle.
    ready: f32,
    overlay: bool,
}

/// The overlay's text panel.
#[derive(Component)]
struct DebugText;

/// The ground rings: one mesh, rebuilt on the terrain each frame.
#[derive(Component)]
struct DebugRings;

pub(super) fn plugin(app: &mut App) {
    app.add_systems(Update, preselect.after(super::menu::restore_pick));
    if !active() {
        return;
    }
    GOD.store(true, Ordering::Relaxed);
    info!("survivors debug: on (F5 waves, F6 clear, F7 god mode, F8 overlay, F9 to the edge or the next station, F1 to the next road point, F2 mark the spot, F3 drop the finale, F4 to a post, F10 MSAA, F11 AA)");
    app.insert_resource(DebugMode {
        started: false,
        ready: 0.0,
        overlay: true,
    })
    .add_systems(
        Update,
        (start_run, keys, show_text, show_rings).after(benilla_world::schedule::WorldStage::Input),
    );
}

/// `--class` and `--zone`: once the last pick is restored, put the named class and battleground
/// in its place. An automated run picks its own.
fn preselect(
    creatures: Option<Res<benilla_app::survivors_host::entities::Creatures>>,
    auto: Option<Res<super::auto::Auto>>,
    mut selection: ResMut<Selection>,
    mut done: Local<bool>,
) {
    if *done || creatures.is_none() {
        return;
    }
    *done = true;
    if auto.is_some() {
        return;
    }
    let var = benilla_app::survivors_host::webenv::var;
    if let Some(m) = var("WOW_SURVIVORS_MAP") {
        match map_index(&m) {
            Some(map) => {
                selection.map = map;
                selection.stage = map;
                selection.version += 1;
            }
            None => warn!("survivors: no battleground `{m}`"),
        }
    }
    if let Some(c) = var("WOW_SURVIVORS_CLASS") {
        match class_index(&c) {
            // The menu fills the looks for the class and picks the first.
            Some(class) => {
                selection.class = Some(class);
                selection.looks.clear();
                selection.look = None;
                selection.version += 1;
            }
            None => warn!("survivors: no class `{c}`"),
        }
    }
}

/// How long the menu stands ready before the debug run begins, in seconds.
const START_AFTER: f32 = 0.5;

/// Skip the menu once: begin the run as soon as the world is in and the pick has its look.
#[allow(clippy::too_many_arguments)]
fn start_run(
    time: Res<Time<Real>>,
    mut mode: ResMut<DebugMode>,
    phase: Res<State<Phase>>,
    mut next: ResMut<NextState<Phase>>,
    mut run: ResMut<Run>,
    mut selection: ResMut<Selection>,
    profile: Res<super::profile::Profile>,
    loading: Res<benilla_app::survivors_host::loading_screen::LoadingScreen>,
    heroes: Query<(), With<Hero>>,
    role: Res<NetRole>,
) {
    if mode.started || *phase.get() != Phase::Menu || *role == NetRole::Client {
        return;
    }
    if selection.class.is_none() {
        // No pick yet and none named: the first class; the menu fills its looks.
        selection.class = Some(0);
        selection.looks.clear();
        selection.look = None;
        selection.version += 1;
    }
    if heroes.is_empty() || loading.covering() || selection.look.is_none() {
        mode.ready = 0.0;
        return;
    }
    mode.ready += time.delta_secs();
    if mode.ready < START_AFTER {
        return;
    }
    if super::hud::start_run(&mut run, &mut selection, &profile) {
        mode.started = true;
        info!(
            "survivors debug: run begins as {} at {}",
            run.class.name,
            selection.map_def().name
        );
        next.set(Phase::Playing);
    }
}

/// Inside the leash by this many yards when F9 puts the hero at the edge.
const EDGE_MARGIN: f32 = 3.0;

/// Short of a road point or a station by this many yards when F9 and F1 put the hero there: near
/// enough to count as reached in the next frame ([`route::STOP_RADIUS`] is 20, the pass radius 25).
const JUMP_SHORT: f32 = 15.0;

/// Where a jump to `points[idx]` sets the hero: [`JUMP_SHORT`] yards short of it on the leg from
/// the point before (from `from` when it is the first), at `y = 0`.
fn short_of(points: &[Point], idx: usize, from: Vec3) -> Vec3 {
    let to = points[idx].at;
    let before = idx.checked_sub(1).map_or(from, |i| points[i].at);
    let dir = flat(to - before).normalize_or_zero();
    flat(to - dir * JUMP_SHORT)
}

/// The next station's index in `points` at or after `next`, if one is left.
fn next_station(points: &[Point], next: usize) -> Option<usize> {
    (next..points.len()).find(|&i| points[i].station.is_some())
}

/// The debug keys.
#[allow(clippy::too_many_arguments)]
fn keys(
    keys: Res<ButtonInput<KeyCode>>,
    mut mode: ResMut<DebugMode>,
    phase: Res<State<Phase>>,
    bounds: Res<super::route::Bounds>,
    (selection, mut route_run, role): (Res<Selection>, ResMut<RouteRun>, Res<NetRole>),
    rig: Res<CamRig>,
    mut commands: Commands,
    mut hero: Query<&mut Transform, With<Hero>>,
    mut foes: Query<(Entity, &mut Enemy)>,
    mut chat: MessageWriter<super::chatlog::ChatLine>,
    mut picture: (
        ResMut<benilla_app::survivors_host::cvars::Cvars>,
        ResMut<super::prefs::Prefs>,
    ),
) {
    let in_run = matches!(phase.get(), Phase::Playing | Phase::LevelUp | Phase::Paused);
    let mut say = |s: String| chat.write(super::chatlog::ChatLine::system(s));
    if keys.just_pressed(KeyCode::F5) {
        let held = !HOLD.fetch_xor(true, Ordering::Relaxed);
        say(format!(
            "Debug: waves {}.",
            if held { "held" } else { "running" }
        ));
    }
    if keys.just_pressed(KeyCode::F7) {
        let on = !GOD.fetch_xor(true, Ordering::Relaxed);
        say(format!(
            "Debug: god mode {}.",
            if on { "on" } else { "off" }
        ));
    }
    if keys.just_pressed(KeyCode::F8) {
        mode.overlay = !mode.overlay;
    }
    // F10 steps MSAA live through 1, 2, 4, 8 samples; F11 the post-process anti-aliasing. Not
    // saved: the prefs only reach the file from the Options window.
    if keys.just_pressed(KeyCode::F10) {
        let (cvars, _) = &mut picture;
        let now = cvars.num("gxMultisample").unwrap_or(1.0) as u32;
        let next = match now {
            0 | 1 => 2,
            2 | 3 => 4,
            4..=7 => 8,
            _ => 1,
        };
        cvars.set("gxMultisample", &next.to_string());
        cvars.commit_latched();
        say(format!("Debug: multisampling {next}x."));
    }
    if keys.just_pressed(KeyCode::F11) {
        use super::prefs::AntiAlias;
        let (_, prefs) = &mut picture;
        prefs.anti_alias = match prefs.anti_alias {
            AntiAlias::Off => AntiAlias::Fxaa,
            AntiAlias::Fxaa => AntiAlias::Smaa,
            AntiAlias::Smaa => AntiAlias::Off,
        };
        say(format!("Debug: anti-aliasing {:?}.", prefs.anti_alias));
    }
    if !in_run {
        return;
    }
    if keys.just_pressed(KeyCode::F6) {
        let mut n = 0;
        for (e, mut en) in &mut foes {
            if en.dead {
                continue;
            }
            // A kill without its loot or its count.
            en.dead = true;
            en.hp = 0.0;
            en.dots.clear();
            enemies::mark_dying(&mut commands, e);
            n += 1;
        }
        say(format!("Debug: {n} enemies cleared."));
    }
    // On a route under way the jumps follow the road; a co-op guest takes the host's run, so it
    // only moves itself.
    let road = route::route_of(selection.stage).filter(|_| route_run.placed && !route_run.arrived);
    let pos = hero.single().map(|tf| tf.translation).ok();
    if keys.just_pressed(KeyCode::F1) || keys.just_pressed(KeyCode::F9) {
        if let (Some(def), Some(pos)) = (road, pos) {
            let points = def.points();
            let station = keys.just_pressed(KeyCode::F9);
            let idx = if station {
                next_station(points, route_run.next)
            } else {
                Some(route_run.next).filter(|&i| i < points.len())
            };
            if let Some(idx) = idx {
                let to = short_of(points, idx, pos);
                if station && *role != NetRole::Client {
                    // The road points on the way count as passed.
                    route_run.next = route_run.next.max(idx);
                }
                if let Ok(mut tf) = hero.single_mut() {
                    // `drive_hero` sets the height from the ground under it.
                    tf.translation.x = to.x;
                    tf.translation.z = to.z;
                }
                let wow = benilla_assets::coords::bevy_to_wow(to);
                say(format!(
                    "Debug: to point {idx}{} at wow {:.0}, {:.0}.",
                    points[idx]
                        .station
                        .map_or(String::new(), |s| format!(" ({})", def.stations[s].label)),
                    wow[0],
                    wow[1]
                ));
            } else {
                say("Debug: the route has no point left.".to_string());
            }
        } else if keys.just_pressed(KeyCode::F9) {
            if let Ok(mut tf) = hero.single_mut() {
                let (sy, cy) = rig.yaw.sin_cos();
                let ahead = Vec3::new(-sy, 0.0, -cy);
                let to = bounds.centre + ahead * (bounds.leash - EDGE_MARGIN).max(0.0);
                // `drive_hero` sets the height from the ground under it.
                tf.translation.x = to.x;
                tf.translation.z = to.z;
            }
        }
    }
    // F2: where the hero stands, for placing a road point (the pilot logs where it sticks).
    if keys.just_pressed(KeyCode::F2) {
        if let Some(pos) = pos {
            let wow = benilla_assets::coords::bevy_to_wow(pos);
            let near = route::route_of(selection.stage).and_then(|def| {
                def.points()
                    .get(route_run.next)
                    .map(|p| (route_run.next, flat(p.at - pos).length()))
            });
            let there = near.map_or(String::new(), |(k, d)| format!(", {d:.0} yd to point {k}"));
            info!(
                "survivors debug: mark wow ({:.0}, {:.0}){there}",
                wow[0], wow[1]
            );
            say(format!("Debug: wow ({:.0}, {:.0}){there}.", wow[0], wow[1]));
        }
    }
}

/// The overlay's text: the toggles, the hero's place and the run's state.
#[allow(clippy::too_many_arguments)]
fn show_text(
    mut commands: Commands,
    mode: Res<DebugMode>,
    phase: Res<State<Phase>>,
    fonts: Option<Res<KitFonts>>,
    run: Res<Run>,
    selection: Res<Selection>,
    bounds: Res<super::route::Bounds>,
    route_run: Res<RouteRun>,
    hero: Query<&Transform, With<Hero>>,
    foes: Query<&Enemy>,
    mut panel: Query<(&mut Text, &mut Node), With<DebugText>>,
    mut spawned: Local<bool>,
) {
    let Ok((mut text, mut node)) = panel.single_mut() else {
        if let (false, Some(fonts)) = (*spawned, fonts) {
            *spawned = true;
            commands.spawn((
                DebugText,
                kit::body(&fonts, "", 13.0, kit::CREAM),
                Node {
                    position_type: PositionType::Absolute,
                    left: Val::Px(12.0),
                    top: Val::Px(200.0),
                    padding: UiRect::axes(Val::Px(8.0), Val::Px(6.0)),
                    display: Display::None,
                    ..default()
                },
                BackgroundColor(Color::srgba(0.0, 0.0, 0.0, 0.6)),
                GlobalZIndex(401),
                bevy::ui::FocusPolicy::Pass,
            ));
        }
        return;
    };
    let in_run = matches!(phase.get(), Phase::Playing | Phase::LevelUp | Phase::Paused);
    let display = if in_run { Display::Flex } else { Display::None };
    if node.display != display {
        node.display = display;
    }
    if !in_run {
        return;
    }
    let on_off = |b: bool| if b { "on" } else { "off" };
    let s = if !mode.overlay {
        "DEBUG  F8 overlay".to_string()
    } else {
        let map = selection.stage_def();
        let (pos, from_centre) = hero.single().map_or(([0.0; 3], 0.0), |tf| {
            (
                benilla_assets::coords::bevy_to_wow(tf.translation),
                flat(tf.translation - bounds.centre).length(),
            )
        });
        let alive = foes.iter().filter(|e| !e.dead).count();
        let t = run.elapsed as u32;
        let pace = run.pace() as u32;
        // A route: the stop, the point under way, how far off it is, how much is walked and the
        // pace the difficulty follows; the circle only while it is fenced.
        let road = route::route_of(selection.stage).map_or(String::new(), |def| {
            let left = def
                .points()
                .get(route_run.next)
                .zip(hero.single().ok())
                .map_or("-".to_string(), |(p, tf)| {
                    format!("{:.0} yd", flat(p.at - tf.translation).length())
                });
            format!(
                "\nroute stop {}/{}  next {}/{}  {left}\nfrac {:.2}  pace {}:{:02}{}",
                route_run.stop,
                def.stations.len(),
                route_run.next,
                def.points().len(),
                run.route_frac,
                pace / 60,
                pace % 60,
                if route_run.arrived { "  ARRIVED" } else { "" },
            )
        });
        let circle = if bounds.fenced {
            format!("centre {from_centre:.0} / {:.0} yd", bounds.leash)
        } else {
            "open road".to_string()
        };
        format!(
            "DEBUG\n\
             F5  waves {}\n\
             F6  clear the field\n\
             F7  god mode {}\n\
             F8  overlay\n\
             F9  to the edge ahead / next station\n\
             F1  next road point  F2 mark the spot\n\
             F3  drop the finale  F4 to a post\n\
             F10 MSAA  F11 AA\n\
             \n\
             {} at {}\n\
             pos {:.1} {:.1} {:.1}\n\
             {circle}{road}\n\
             alive {alive}  t {}:{:02}  level {}\n\
             \n\
             rings: white {:.0} clear, magenta {:.0} encircle,\n\
             red {:.0} boss, yellow {:.0} to {:.0} spawn, blue leash",
            if waves_held() { "HELD" } else { "running" },
            on_off(god()),
            run.class.name,
            map.name,
            pos[0],
            pos[1],
            pos[2],
            t / 60,
            t % 60,
            run.level,
            enemies::SPAWN_CLEAR,
            enemies::RING_RADIUS,
            enemies::BOSS_RADIUS,
            enemies::SPAWN_NEAR,
            enemies::SPAWN_FAR,
        )
    };
    if text.0 != s {
        text.0 = s;
    }
}

/// Columns per ring: enough for a round ring at the leash's size.
const RING_COLUMNS: usize = 96;
/// How tall a ring stands off the ground, and how far above it it starts, in yards.
const RING_HEIGHT: f32 = 0.5;
const RING_LIFT: f32 = 0.1;

/// The rings, in their drawing order: around the hero `true`, else around the arena centre;
/// their radius; their colour (linear RGBA).
fn rings(bounds: &super::route::Bounds) -> [(bool, f32, [f32; 4]); 6] {
    [
        (true, enemies::SPAWN_CLEAR, [1.0, 1.0, 1.0, 0.6]),
        (true, enemies::RING_RADIUS, [1.0, 0.1, 0.9, 0.75]),
        (true, enemies::BOSS_RADIUS, [1.0, 0.08, 0.05, 0.7]),
        (true, enemies::SPAWN_NEAR, [1.0, 0.85, 0.1, 0.7]),
        (true, enemies::SPAWN_FAR, [1.0, 0.85, 0.1, 0.7]),
        // Open bounds have no edge: a ring of no size.
        (
            false,
            if bounds.fenced { bounds.leash } else { 0.0 },
            [0.15, 0.6, 1.0, 0.8],
        ),
    ]
}

/// One ribbon per ring: two vertices per column, the ring closed by repeating the first column.
fn ring_mesh(colors: &[[f32; 4]]) -> Mesh {
    let per = (RING_COLUMNS + 1) * 2;
    let n = colors.len() * per;
    let mut mesh = Mesh::new(
        PrimitiveTopology::TriangleList,
        RenderAssetUsages::default(),
    );
    mesh.insert_attribute(Mesh::ATTRIBUTE_POSITION, vec![[0.0f32; 3]; n]);
    mesh.insert_attribute(Mesh::ATTRIBUTE_NORMAL, vec![[0.0f32, 1.0, 0.0]; n]);
    mesh.insert_attribute(Mesh::ATTRIBUTE_UV_0, vec![[0.0f32; 2]; n]);
    let color: Vec<[f32; 4]> = colors
        .iter()
        .flat_map(|c| std::iter::repeat_n(*c, per))
        .collect();
    mesh.insert_attribute(Mesh::ATTRIBUTE_COLOR, color);
    let mut idx = Vec::with_capacity(colors.len() * RING_COLUMNS * 6);
    for r in 0..colors.len() {
        let base = (r * per) as u32;
        for i in 0..RING_COLUMNS as u32 {
            let (b0, t0, b1, t1) = (
                base + 2 * i,
                base + 2 * i + 1,
                base + 2 * i + 2,
                base + 2 * i + 3,
            );
            idx.extend_from_slice(&[b0, b1, t1, b0, t1, t0]);
        }
    }
    mesh.insert_indices(Indices::U32(idx));
    mesh
}

/// Lay the rings on the terrain around the hero and the arena centre while the overlay is on.
#[allow(clippy::too_many_arguments)]
fn show_rings(
    mut commands: Commands,
    mode: Res<DebugMode>,
    phase: Res<State<Phase>>,
    bounds: Res<super::route::Bounds>,
    hero: Query<&Transform, (With<Hero>, Without<DebugRings>)>,
    mut shown: Query<(&Mesh3d, &mut Visibility), With<DebugRings>>,
    world: WorldPoint,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
) {
    let rings = rings(&bounds);
    let Ok((mesh, mut vis)) = shown.single_mut() else {
        let colors: Vec<[f32; 4]> = rings.iter().map(|r| r.2).collect();
        commands.spawn((
            DebugRings,
            Mesh3d(meshes.add(ring_mesh(&colors))),
            MeshMaterial3d(materials.add(StandardMaterial {
                base_color: Color::WHITE,
                unlit: true,
                alpha_mode: AlphaMode::Blend,
                cull_mode: None,
                double_sided: true,
                fog_enabled: false,
                ..default()
            })),
            Transform::default(),
            Visibility::Hidden,
            // The vertices follow the hero and the terrain; the stored bounds would not.
            NoFrustumCulling,
        ));
        return;
    };
    let in_run = matches!(phase.get(), Phase::Playing | Phase::LevelUp | Phase::Paused);
    let hero = hero.single().ok().map(|t| t.translation);
    let want = match (in_run && mode.overlay, hero) {
        (true, Some(_)) => Visibility::Inherited,
        _ => Visibility::Hidden,
    };
    if *vis != want {
        *vis = want;
    }
    let (Visibility::Inherited, Some(hero)) = (want, hero) else {
        return;
    };
    let Some(mesh) = meshes.get_mut(&mesh.0) else {
        return;
    };
    let Some(VertexAttributeValues::Float32x3(pos)) = mesh.attribute_mut(Mesh::ATTRIBUTE_POSITION)
    else {
        return;
    };
    let centre = bounds.centre;
    let per = (RING_COLUMNS + 1) * 2;
    for (r, (on_hero, radius, _)) in rings.iter().enumerate() {
        let at = if *on_hero { hero } else { centre };
        for i in 0..=RING_COLUMNS {
            let a = i as f32 / RING_COLUMNS as f32 * std::f32::consts::TAU;
            let mut p = at + Vec3::new(a.cos(), 0.0, a.sin()) * *radius;
            p.y = world
                .terrain_height_under(Vec3::new(p.x, hero.y, p.z))
                .unwrap_or(hero.y)
                + RING_LIFT;
            let v = r * per + 2 * i;
            pos[v] = p.to_array();
            pos[v + 1] = (p + Vec3::Y * RING_HEIGHT).to_array();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn road() -> Vec<Point> {
        let p = |x: f32, z: f32, station| Point {
            at: Vec3::new(x, 0.0, z),
            station,
        };
        vec![
            p(0.0, 0.0, Some(0)),
            p(100.0, 0.0, None),
            p(100.0, 200.0, Some(1)),
            p(300.0, 200.0, Some(2)),
        ]
    }

    #[test]
    fn a_jump_lands_short_of_the_point_on_the_leg_that_leads_to_it() {
        let points = road();
        let at = short_of(&points, 1, Vec3::ZERO);
        assert!(
            (at - Vec3::new(100.0 - JUMP_SHORT, 0.0, 0.0)).length() < 1e-3,
            "{at}"
        );
        let at = short_of(&points, 2, Vec3::ZERO);
        assert!(
            (at - Vec3::new(100.0, 0.0, 200.0 - JUMP_SHORT)).length() < 1e-3,
            "{at}"
        );
        // The first point has no leg: short of it as seen from the hero.
        let at = short_of(&points, 0, Vec3::new(-50.0, 0.0, 0.0));
        assert!(
            (at - Vec3::new(-JUMP_SHORT, 0.0, 0.0)).length() < 1e-3,
            "{at}"
        );
        // Near enough to count as reached.
        const { assert!(JUMP_SHORT < route::STOP_RADIUS && JUMP_SHORT < route::PASS_RADIUS) };
    }

    #[test]
    fn a_station_jump_skips_the_road_points() {
        let points = road();
        assert_eq!(next_station(&points, 1), Some(2));
        assert_eq!(next_station(&points, 2), Some(2));
        assert_eq!(next_station(&points, 3), Some(3));
        assert_eq!(next_station(&points, 4), None);
    }
}
