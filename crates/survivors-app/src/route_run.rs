//! The Undying Run's course ([`super::route`]): the hero set down at the start, the road points
//! and stations passed in turn with a rest at every station between the ends, the end ring
//! standing once the last station is reached ([`super::route::Bounds`], [`super::barrier`]), a
//! gold column of light over the next station, and the horde and the gems cut away far behind
//! every hero so the hunt stays in front.
//!
//! Nothing here acts off a route. The finale of a route comes from [`super::finale::summon`] once
//! [`RouteRun::arrived`] is set.
//!
//! Parts follow marekvi95/undying-run (branch continent-run, 84cc7c0, MIT OR Apache-2.0).

use benilla_world::world_point::WorldPoint;
use bevy::asset::RenderAssetUsages;
use bevy::mesh::{Indices, PrimitiveTopology};
use bevy::prelude::*;

use super::enemies::{self, Dying, Enemy, Gem};
use super::finale::{self, Finale, Reaper};
use super::hero::{self, Hero, Peer};
use super::route::{
    route_frac, route_of, Point, RouteRun, CULL_YD, PASS_RADIUS, REST_HEAL, STOP_RADIUS,
};
use super::units::{flat, Fx};
use super::{HeroState, NetRole, Phase, Run, RunEntity, Selection};

/// A road point also counts as passed when a hero within this many yards is nearer the point
/// after it: a corner cut across the grass, not a hairpin turn.
const CUT_RADIUS: f32 = PASS_RADIUS * 4.0;
/// Seconds between the sweeps that cut the horde and the gems behind.
const CULL_EVERY: f32 = 0.5;
/// Renew's sparkle, on each hero that rests.
const SPELL_REST: u32 = 139;

/// Height of the column over the next station, and how far it sinks below the ground, in yards.
const BEACON_HEIGHT: f32 = 40.0;
const BEACON_SINK: f32 = 15.0;
/// The column's two tubes: a wide faint one and a narrow bright core, with the opacity each has
/// at its foot (it fades to nothing at the top).
const BEACON_TUBES: [(f32, f32); 2] = [(2.2, 0.35), (0.6, 0.8)];
const BEACON_SIDES: usize = 16;
/// Linear gold.
const BEACON_GOLD: [f32; 3] = [1.0, 0.72, 0.18];
/// Seconds between terrain re-samples of the column's foot.
const BEACON_RESAMPLE: f32 = 0.5;

pub(super) fn plugin(app: &mut App) {
    app.add_systems(
        Update,
        // The hero is set down even behind the loading screen: the jump raises it itself.
        place_start
            .after(hero::drive_hero)
            .before(track)
            .before(enemies::spawn_enemies)
            .run_if(in_state(Phase::Playing))
            .run_if(super::simulates),
    )
    .add_systems(
        Update,
        (track, cull_behind)
            .chain()
            .after(hero::drive_hero)
            .before(enemies::spawn_enemies)
            .before(finale::summon)
            .run_if(super::run_on)
            .run_if(super::simulates)
            .run_if(|l: Res<benilla_app::survivors_host::loading_screen::LoadingScreen>| !l.covering()),
    )
    .add_systems(Update, station_beacon.run_if(not(in_state(Phase::Menu))));
}

/// The resources the balance simulator ([`super::balance`]) needs for [`sim_systems`].
#[cfg(test)]
#[allow(dead_code)]
pub(super) fn sim_init(app: &mut App) {
    app.init_resource::<RouteRun>();
}

/// The run's systems for the balance simulator ([`super::balance`]): the start, the stations and
/// the cut behind. The beacon is only to look at.
#[cfg(test)]
#[allow(dead_code)]
pub(super) fn sim_systems() -> bevy::ecs::schedule::ScheduleConfigs<bevy::ecs::system::ScheduleSystem> {
    (place_start, track, cull_behind).chain().into_configs()
}

/// The next point after `next` that the heroes at `at` have not passed: every point they stand
/// near ([`STOP_RADIUS`] for a station, [`PASS_RADIUS`] for a road point) or, for a road point,
/// cut the corner of ([`CUT_RADIUS`]) is walked past, in order. A point far ahead is not.
pub(super) fn advance(points: &[Point], next: usize, at: &[Vec3]) -> usize {
    let mut next = next;
    while let Some(p) = points.get(next) {
        let radius = if p.station.is_some() {
            STOP_RADIUS
        } else {
            PASS_RADIUS
        };
        let reached = at.iter().any(|&h| flat(h - p.at).length() <= radius);
        // Only a road point can be cut: a station is reached, never skipped.
        let cut = p.station.is_none()
            && points.get(next + 1).is_some_and(|after| {
                at.iter().any(|&h| {
                    let d = flat(h - p.at).length();
                    d <= CUT_RADIUS && flat(h - after.at).length() < d
                })
            });
        if !(reached || cut) {
            break;
        }
        next += 1;
    }
    next
}

/// Whether the horde or the gem at `at` is cut away: farther than [`CULL_YD`] from every standing
/// hero in `heroes`. Bosses stay, to be fought on the way; with no one standing nothing goes.
pub(super) fn culled(at: Vec3, heroes: &[Vec3], boss: bool) -> bool {
    !boss
        && !heroes.is_empty()
        && heroes
            .iter()
            .all(|&h| flat(h - at).length_squared() > CULL_YD * CULL_YD)
}

/// The health `hp` of a hero with `max` health after a rest.
fn rested(hp: f32, max: f32) -> f32 {
    (hp + REST_HEAL * max).min(max).max(hp)
}

/// Set the hero down at the first station and start the run's count. Runs on the machine that
/// simulates; a guest takes its place from the host.
fn place_start(
    mut route: ResMut<RouteRun>,
    mut run: ResMut<Run>,
    selection: Res<Selection>,
    role: Res<NetRole>,
    world: WorldPoint,
    mut hero: Query<&mut Transform, With<Hero>>,
) {
    let Some(def) = route_of(selection.stage) else {
        return;
    };
    if route.placed || *role == NetRole::Client {
        return;
    }
    let Ok(mut tf) = hero.single_mut() else {
        return;
    };
    let mut at = def.points()[def.point_of(0)].at;
    at.y = tf.translation.y;
    at.y = world.terrain_height_under(at).unwrap_or(at.y);
    tf.translation = at;
    route.placed = true;
    route.stop = 1;
    route.next = def.point_of(0) + 1;
    let last = def.stations[def.stations.len() - 1].label;
    run.announce(
        format!("Cross the {} to {last}", selection.stage_def().zone),
        4.0,
    );
}

/// Walk the road: pass its points and stations in turn, rest at each station between the ends,
/// close the end ring at the last, and keep the share of the route walked ([`Run::route_frac`]).
fn track(
    mut run: ResMut<Run>,
    mut route: ResMut<RouteRun>,
    selection: Res<Selection>,
    hero: Query<(Entity, &Transform), With<Hero>>,
    mut peers: Query<(Entity, &Transform, &mut HeroState), (With<Peer>, Without<Hero>)>,
    mut fx: Fx,
) {
    let Some(def) = route_of(selection.stage) else {
        return;
    };
    if !route.placed {
        return;
    }
    if route.arrived {
        if run.route_frac < 1.0 {
            run.route_frac = 1.0;
        }
        return;
    }
    // Every standing hero: this machine's and each co-op puppet's.
    let mut standing: Vec<(Entity, Vec3)> = Vec::new();
    if run.dead_t.is_none() {
        if let Ok((e, tf)) = hero.single() {
            standing.push((e, tf.translation));
        }
    }
    standing.extend(
        peers
            .iter()
            .filter(|(_, _, s)| s.dead_t.is_none())
            .map(|(e, tf, _)| (e, tf.translation)),
    );
    if standing.is_empty() {
        return;
    }
    let at: Vec<Vec3> = standing.iter().map(|&(_, p)| p).collect();
    let points = def.points();
    let next = advance(points, route.next, &at);
    let mut want = route.clone();
    want.next = next;
    let last = def.stations.len() - 1;
    let passed = route.next.min(points.len())..next.min(points.len());
    for station in points[passed].iter().filter_map(|p| p.station) {
        want.stop = want.stop.max(station + 1);
        let label = def.stations[station].label;
        if station == last {
            want.arrived = true;
            run.announce(format!("{label} reached"), 4.0);
        } else if station > 0 {
            run.announce(
                format!("{label} reached. Next: {}", def.stations[station + 1].label),
                4.0,
            );
            // A rest: every standing hero gets half its health back (not on Hardcore, which
            // has no health to pick up).
            if run.difficulty.health_gems() {
                if run.dead_t.is_none() {
                    let max = run.stats().max_hp;
                    run.hp = rested(run.hp, max);
                }
                for (e, _, mut state) in &mut peers {
                    if state.dead_t.is_none() {
                        let max = state.stats().max_hp;
                        state.hp = rested(state.hp, max);
                        fx.impact(e, SPELL_REST);
                    }
                }
                if let Ok((e, _)) = hero.single() {
                    if run.dead_t.is_none() {
                        fx.impact(e, SPELL_REST);
                    }
                }
            }
        }
    }
    if *route != want {
        *route = want.clone();
    }
    let flat_points: Vec<Vec3> = points.iter().map(|p| p.at).collect();
    let frac = if want.arrived {
        1.0
    } else {
        at.iter()
            .map(|&h| route_frac(&flat_points, want.next, h))
            .fold(0.0, f32::max)
    };
    if frac > run.route_frac {
        run.route_frac = frac;
    }
}

/// Cut away the horde and the gems left far behind every standing hero, so the hunt stays in
/// front of the hero and the gems do not fill [`enemies`]'s cap. Bosses, the finale and the
/// reaper stay; so does a gem that is flying to the hero.
fn cull_behind(
    mut commands: Commands,
    time: Res<Time>,
    run: Res<Run>,
    selection: Res<Selection>,
    route: Res<RouteRun>,
    hero: Query<&Transform, With<Hero>>,
    peers: Query<(&Transform, &HeroState), (With<Peer>, Without<Hero>)>,
    enemies: Query<
        (Entity, &Transform, &Enemy),
        (Without<Dying>, Without<Finale>, Without<Reaper>),
    >,
    gems: Query<(Entity, &Transform, &Gem)>,
    mut since: Local<f32>,
) {
    if !route.placed || route_of(selection.stage).is_none() {
        return;
    }
    *since += time.delta_secs();
    if *since < CULL_EVERY {
        return;
    }
    *since = 0.0;
    let heroes: Vec<Vec3> = super::rares::standing(&run, &hero, &peers)
        .iter()
        .map(|t| t.translation)
        .collect();
    if heroes.is_empty() {
        return;
    }
    for (e, tf, en) in &enemies {
        if !en.dead && culled(tf.translation, &heroes, en.boss) {
            commands.entity(e).despawn();
        }
    }
    for (e, tf, gem) in &gems {
        if !gem.pulled && culled(tf.translation, &heroes, false) {
            commands.entity(e).despawn();
        }
    }
}

/// The column of light over the next station.
#[derive(Component)]
struct Beacon {
    /// The station it stands on (an index into [`super::route::RouteDef::stations`]).
    stop: usize,
    material: Handle<StandardMaterial>,
    resample: f32,
}

/// Two nested open tubes, bright at the foot and fading to nothing at the top: the column.
fn column_mesh() -> Mesh {
    let mut positions: Vec<[f32; 3]> = Vec::new();
    let mut normals: Vec<[f32; 3]> = Vec::new();
    let mut colors: Vec<[f32; 4]> = Vec::new();
    let mut indices: Vec<u32> = Vec::new();
    let [r, g, b] = BEACON_GOLD;
    for (radius, alpha) in BEACON_TUBES {
        let first = positions.len() as u32;
        let n = BEACON_SIDES as u32;
        for i in 0..n {
            let a = i as f32 / n as f32 * std::f32::consts::TAU;
            let (s, c) = a.sin_cos();
            let (x, z) = (c * radius, s * radius);
            // Foot, then top.
            positions.push([x, -BEACON_SINK, z]);
            positions.push([x, BEACON_HEIGHT, z]);
            normals.push([c, 0.0, s]);
            normals.push([c, 0.0, s]);
            colors.push([r, g, b, alpha]);
            colors.push([r, g, b, 0.0]);
        }
        for i in 0..n {
            let j = (i + 1) % n;
            let (foot_i, top_i) = (first + 2 * i, first + 2 * i + 1);
            let (foot_j, top_j) = (first + 2 * j, first + 2 * j + 1);
            indices.extend_from_slice(&[foot_i, foot_j, top_j, foot_i, top_j, top_i]);
        }
    }
    let mut mesh = Mesh::new(
        PrimitiveTopology::TriangleList,
        RenderAssetUsages::default(),
    );
    mesh.insert_attribute(Mesh::ATTRIBUTE_POSITION, positions);
    mesh.insert_attribute(Mesh::ATTRIBUTE_NORMAL, normals);
    mesh.insert_attribute(Mesh::ATTRIBUTE_COLOR, colors);
    mesh.insert_indices(Indices::U32(indices));
    mesh
}

/// Keep one column over the station the run walks to next: raised when the run starts and moved
/// on at every station, gone at the end ring. It sits on the terrain as it streams in and
/// pulses a little.
#[allow(clippy::too_many_arguments)]
fn station_beacon(
    mut commands: Commands,
    time: Res<Time>,
    selection: Res<Selection>,
    route: Res<RouteRun>,
    hero: Query<&Transform, With<Hero>>,
    mut beacons: Query<(Entity, &mut Beacon, &mut Transform), Without<Hero>>,
    world: WorldPoint,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
) {
    let want = route_of(selection.stage)
        .filter(|_| route.placed && !route.arrived)
        .filter(|def| route.stop < def.stations.len())
        .map(|def| (def, route.stop));
    let Some((def, stop)) = want else {
        for (e, ..) in &beacons {
            commands.entity(e).despawn();
        }
        return;
    };
    let dt = time.delta_secs();
    let pulse = 0.75 + 0.25 * (time.elapsed_secs() * 2.0).sin();
    let mut have = false;
    for (e, mut b, mut tf) in &mut beacons {
        if b.stop != stop || have {
            commands.entity(e).despawn();
            continue;
        }
        have = true;
        b.resample -= dt;
        if b.resample <= 0.0 {
            b.resample = BEACON_RESAMPLE;
            if let Some(h) = world.terrain_height_under(tf.translation) {
                if (tf.translation.y - h).abs() > 0.05 {
                    tf.translation.y = h;
                }
            }
        }
        if let Some(m) = materials.get_mut(&b.material) {
            m.base_color = Color::WHITE.with_alpha(pulse);
        }
    }
    if have {
        return;
    }
    let mut at = def.points()[def.point_of(stop)].at;
    let fallback = hero.single().map_or(0.0, |t| t.translation.y);
    at.y = world.terrain_height_under(Vec3::new(at.x, fallback, at.z)).unwrap_or(fallback);
    let material = materials.add(StandardMaterial {
        base_color: Color::WHITE.with_alpha(pulse),
        unlit: true,
        alpha_mode: AlphaMode::Add,
        cull_mode: None,
        double_sided: true,
        fog_enabled: false,
        ..default()
    });
    commands.spawn((
        Beacon {
            stop,
            material: material.clone(),
            resample: BEACON_RESAMPLE,
        },
        Mesh3d(meshes.add(column_mesh())),
        MeshMaterial3d(material),
        Transform::from_translation(at),
        Visibility::default(),
        RunEntity,
    ));
}

#[cfg(test)]
mod tests {
    use super::*;
    use bevy::mesh::VertexAttributeValues;

    /// A road point (`None`) or a station (`Some(index)`) at `(x, z)`, flat.
    fn point(x: f32, z: f32, station: Option<usize>) -> Point {
        Point {
            at: Vec3::new(x, 0.0, z),
            station,
        }
    }

    /// Station 0, road points A and B, station 1, a road point after it.
    fn road() -> Vec<Point> {
        vec![
            point(0.0, 0.0, Some(0)),
            point(100.0, 0.0, None),
            point(100.0, 100.0, None),
            point(300.0, 100.0, Some(1)),
            point(400.0, 100.0, None),
        ]
    }

    fn hero(x: f32, z: f32) -> Vec3 {
        // Height plays no part.
        Vec3::new(x, 7.0, z)
    }

    #[test]
    fn points_are_passed_in_order_and_a_corner_cut_counts() {
        let p = road();
        // At the start nothing is passed yet.
        assert_eq!(advance(&p, 1, &[hero(0.0, 0.0)]), 1);
        // Within the radius of the next point: passed; the one after is still 90 yd off.
        assert_eq!(advance(&p, 1, &[hero(100.0, 10.0)]), 2);
        // A corner cut: 60 yd from A, but nearer B.
        assert_eq!(advance(&p, 1, &[hero(110.0, 60.0)]), 2);
        // A point far ahead is not passed, nor the ones before it.
        assert_eq!(advance(&p, 1, &[hero(300.0, 100.0)]), 1);
        // Two points in one go, in order.
        assert_eq!(advance(&p, 1, &[hero(100.0, 0.0), hero(100.0, 100.0)]), 3);
        // Past the end there is nothing to pass.
        assert_eq!(advance(&p, p.len(), &[hero(400.0, 100.0)]), p.len());
    }

    #[test]
    fn a_station_needs_a_hero_within_its_radius() {
        let p = road();
        // 22 yd off: past the station's radius, within a road point's.
        const { assert!(STOP_RADIUS < 22.0 && 22.0 <= PASS_RADIUS) };
        assert_eq!(advance(&p, 3, &[hero(300.0, 78.0)]), 3);
        assert_eq!(advance(&p, 1, &[hero(100.0, 22.0)]), 2);
        // Within it: reached, and the road point after is not.
        assert_eq!(advance(&p, 3, &[hero(300.0, 85.0)]), 4);
        // Any of several heroes counts; a hero nearer the point after does not skip a station.
        assert_eq!(advance(&p, 3, &[hero(0.0, 0.0), hero(300.0, 90.0)]), 4);
        assert_eq!(advance(&p, 3, &[hero(360.0, 100.0)]), 3);
    }

    #[test]
    fn the_horde_far_behind_every_hero_goes_but_bosses_stay() {
        let at = Vec3::ZERO;
        let far = CULL_YD + 1.0;
        let near = CULL_YD - 1.0;
        assert!(culled(at, &[hero(far, 0.0)], false));
        assert!(!culled(at, &[hero(far, 0.0)], true));
        assert!(!culled(at, &[hero(near, 0.0)], false));
        // Near any one standing hero keeps it.
        assert!(!culled(at, &[hero(far, 0.0), hero(0.0, near)], false));
        assert!(culled(at, &[hero(far, 0.0), hero(0.0, -far)], false));
        // No one standing: nothing is cut.
        assert!(!culled(at, &[], false));
    }

    #[test]
    fn a_rest_gives_back_half_and_never_past_the_maximum() {
        assert!((rested(20.0, 100.0) - 20.0 - REST_HEAL * 100.0).abs() < 1e-4);
        assert_eq!(rested(80.0, 100.0), 100.0);
        // A hero above its maximum (a shield's worth) is left as it is.
        assert_eq!(rested(130.0, 100.0), 130.0);
    }

    #[test]
    fn the_column_fades_from_its_foot_to_nothing() {
        let mesh = column_mesh();
        let Some(VertexAttributeValues::Float32x4(colors)) = mesh.attribute(Mesh::ATTRIBUTE_COLOR)
        else {
            panic!("the column has vertex colours");
        };
        assert_eq!(colors.len(), BEACON_TUBES.len() * BEACON_SIDES * 2);
        // Foot and top alternate; the top is clear.
        assert!(colors.iter().step_by(2).all(|c| c[3] > 0.0));
        assert!(colors.iter().skip(1).step_by(2).all(|c| c[3] == 0.0));
        let n = mesh.indices().expect("indices").len();
        assert_eq!(n, BEACON_TUBES.len() * BEACON_SIDES * 6);
        // Every index lands on a vertex.
        assert!(mesh.indices().expect("indices").iter().all(|i| i < colors.len()));
    }
}
