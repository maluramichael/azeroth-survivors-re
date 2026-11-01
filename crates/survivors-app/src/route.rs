//! The Undying Run: a run that does not stand in one zone but crosses the Eastern Kingdoms, from
//! Westfall through Elwynn Forest and Redridge to Duskwood (Kanboard #1142).
//!
//! A route is one more [`data::MAPS`] entry ("Undying Run", last, so saved indices hold), so
//! [`Selection::stage`] stays its index for the whole run and records, achievements, the profile,
//! co-op's run start and the creature refs work unchanged. Its roster and its boss plan are its
//! zones' rosters and plans one after the other, in segments per zone ([`data::segment`],
//! [`super::finale::Plan::segments`]). [`Bounds`] stands in for "the arena is its centre and its
//! leash" everywhere: on a zone it is exactly that, on a route it is open on the road and a ring
//! round the last station once it is reached ([`RouteRun::arrived`]). The clock keeps the beat;
//! the difficulty follows [`super::Run::pace`], the further of the clock and the road walked.
//!
//! Parts follow marekvi95/undying-run (branch continent-run, 84cc7c0, MIT OR Apache-2.0).

// The route's packages (its run, horde, nav, streaming, co-op, pilot and display) read the rest
// of this as they land; drop this then.

use std::sync::LazyLock;

use benilla_world::schedule::WorldStage;
use bevy::prelude::*;

use super::units::{flat, wow_to_bevy};
use super::{data, Phase, Selection};

/// The hero's pace on a route, over the class's own.
pub(crate) const TRAVEL_SPEED: f32 = 1.25;
/// A hero this near a station has reached it.
pub(crate) const STOP_RADIUS: f32 = 20.0;
/// A hero this near a road point has passed it.
pub(crate) const PASS_RADIUS: f32 = 25.0;
/// The ring round the last station, once reached.
pub(crate) const END_RING: f32 = 40.0;
/// The horde and the gems farther than this from every standing hero are cut away behind.
pub(crate) const CULL_YD: f32 = 110.0;
/// The share of the maximum health every standing hero gets back at a station.
pub(crate) const REST_HEAL: f32 = 0.5;
/// The share of the horde that comes from ahead, within [`AHEAD_CONE`] radians of the heading.
pub(crate) const AHEAD_SHARE: f32 = 0.7;
pub(crate) const AHEAD_CONE: f32 = 1.0;
/// A heal on the road every this many seconds, at most this many lying, this far ahead.
pub(crate) const ROAD_HEAL_EVERY: f32 = 10.0;
pub(crate) const ROAD_HEAL_CAP: usize = 3;
pub(crate) const ROAD_HEAL_AHEAD: f32 = 14.0;
/// The floor on the streamed world's outer window on a route, in chunks.
pub(crate) const ROUTE_STREAM_CHUNKS: i32 = 20;

/// One stop on a route.
pub(crate) struct Station {
    /// The data::MAPS zone whose horde and bosses come on the way into this station.
    pub zone: &'static str,
    /// What the banner and the HUD call it.
    pub label: &'static str,
    /// WoW x and y of the stop; the height comes from the terrain.
    pub at: [f32; 2],
    /// Road points on the way here from the station before, in order (WoW x, y).
    pub via: &'static [[f32; 2]],
}

/// A run across several zones: the [`data::MAPS`] entry named `map`, its stations in order, the
/// first the start and the last the end.
pub(crate) struct RouteDef {
    pub map: &'static str,
    pub stations: &'static [Station],
}

/// A point to walk past: a road point (`station: None`) or a station (its index).
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct Point {
    /// Flat Bevy coordinates, `y = 0`.
    pub at: Vec3,
    pub station: Option<usize>,
}

/// The routes. The stations are the zones' checked [`data::MAPS`] centres; the road points
/// between them are measured in the game (the pilot logs where it sticks).
pub(crate) static ROUTES: &[RouteDef] = &[RouteDef {
    map: "Undying Run",
    stations: &[
        Station {
            zone: "Westfall",
            label: "Westfall",
            at: [-10252.2, 1483.5],
            via: &[],
        },
        Station {
            zone: "Elwynn Forest",
            label: "Elwynn Forest",
            at: [-9548.5, 115.0],
            via: &[],
        },
        Station {
            zone: "Redridge Mountains",
            label: "Redridge Mountains",
            at: [-9199.8, -2767.5],
            via: &[],
        },
        Station {
            zone: "Duskwood",
            label: "Duskwood",
            at: [-10051.8, -792.5],
            via: &[],
        },
    ],
}];

/// The route that battleground `map` is, if it is one (by its [`data::MAPS`] name).
pub(crate) fn route_of(map: usize) -> Option<&'static RouteDef> {
    let name = data::MAPS.get(map)?.name;
    ROUTES.iter().find(|r| r.map == name)
}

/// Every road point and station of `stations` in order, flat in Bevy coordinates.
fn walk(stations: &[Station]) -> Vec<Point> {
    let flat_at = |p: [f32; 2]| wow_to_bevy([p[0], p[1], 0.0]);
    let mut points = Vec::new();
    for (i, s) in stations.iter().enumerate() {
        points.extend(s.via.iter().map(|&v| Point {
            at: flat_at(v),
            station: None,
        }));
        points.push(Point {
            at: flat_at(s.at),
            station: Some(i),
        });
    }
    points
}

/// [`RouteDef::points`] of every route, in the order of [`ROUTES`].
static POINTS: LazyLock<Vec<Vec<Point>>> =
    LazyLock::new(|| ROUTES.iter().map(|r| walk(r.stations)).collect());

impl RouteDef {
    /// Every road point and station in order, the start first.
    pub fn points(&self) -> &'static [Point] {
        let i = ROUTES.iter().position(|r| r.map == self.map).unwrap_or(0);
        &POINTS[i]
    }

    /// The [`data::MAPS`] indices of the zones on the way, each once, in order: the segments of
    /// the route's roster and plan.
    pub fn zones(&self) -> Vec<usize> {
        let mut zones = Vec::new();
        for i in 0..self.stations.len() {
            let z = self.zone_of(i);
            if !zones.contains(&z) {
                zones.push(z);
            }
        }
        zones
    }

    /// The [`data::MAPS`] index of station `station`'s zone (the last station past the end).
    pub fn zone_of(&self, station: usize) -> usize {
        let s = &self.stations[station.min(self.stations.len() - 1)];
        data::MAPS
            .iter()
            .position(|m| m.name == s.zone)
            .unwrap_or_else(|| panic!("route {}: no zone called {}", self.map, s.zone))
    }

    /// Where station `station` stands in [`Self::points`].
    pub fn point_of(&self, station: usize) -> usize {
        self.points()
            .iter()
            .position(|p| p.station == Some(station))
            .unwrap_or(self.points().len() - 1)
    }
}

/// The run's place on its route. Reset at the menu; the host (or a hero alone) writes it, a
/// co-op guest takes it from the host's run.
#[derive(Resource, Default, Clone, Debug, PartialEq)]
pub(crate) struct RouteRun {
    /// The index into [`RouteDef::points`] of the next point; `points().len()` once all are
    /// walked.
    pub next: usize,
    /// Stations reached, the start counted (1 once the run starts).
    pub stop: usize,
    /// At the last station: the end ring stands.
    pub arrived: bool,
    /// The hero was set down at the start.
    pub placed: bool,
}

/// The share of the route walked to `at`: the legs behind the point before `next`, plus the way
/// along the leg toward `next`.
pub(crate) fn route_frac(points: &[Vec3], next: usize, at: Vec3) -> f32 {
    let leg = |w: &[Vec3]| flat(w[1] - w[0]).length();
    let total: f32 = points.windows(2).map(leg).sum();
    if total <= 0.0 {
        return 0.0;
    }
    let next = next.min(points.len());
    let behind: f32 = points[..next].windows(2).map(leg).sum();
    let along = match (
        next.checked_sub(1).and_then(|i| points.get(i)),
        points.get(next),
    ) {
        (Some(&a), Some(&b)) => {
            let ab = flat(b - a);
            let t = (flat(at - a).dot(ab) / ab.length_squared().max(f32::EPSILON)).clamp(0.0, 1.0);
            t * ab.length()
        }
        _ => 0.0,
    };
    ((behind + along) / total).clamp(0.0, 1.0)
}

/// The zone whose horde comes at `at`: the zone of the station left behind until half the leg is
/// walked, the next station's after. Before the start the first zone, past the end the last.
pub(crate) fn leg_zone(def: &RouteDef, run: &RouteRun, at: Vec3) -> usize {
    let n = def.stations.len();
    if run.stop == 0 || run.stop >= n {
        return def.zone_of(run.stop.saturating_sub(1));
    }
    let (from, to) = (def.point_of(run.stop - 1), def.point_of(run.stop));
    let leg: Vec<Vec3> = def.points()[from..=to].iter().map(|p| p.at).collect();
    let next = run.next.clamp(from + 1, to) - from;
    if route_frac(&leg, next, at) < 0.5 {
        def.zone_of(run.stop - 1)
    } else {
        def.zone_of(run.stop)
    }
}

/// The heading from `at` to the next point, as `atan2(z, x)` (the angle the spawn ring takes);
/// `None` once the route is walked.
pub(crate) fn heading(def: &RouteDef, run: &RouteRun, at: Vec3) -> Option<f32> {
    if run.arrived {
        return None;
    }
    let to = flat(def.points().get(run.next)?.at - at);
    (to.length_squared() > f32::EPSILON).then(|| to.z.atan2(to.x))
}

/// Where the run may go: a circle (`fenced`) round `centre` of radius `leash`, or open. On a
/// zone it is the zone's centre and leash, as the code read them before routes. `key` changes
/// whenever the shape does, for what is built once per shape (the barrier).
#[derive(Resource, Clone, Copy, Debug, Default, PartialEq)]
pub(crate) struct Bounds {
    /// The centre; while open, the middle of the leg under way.
    pub centre: Vec3,
    /// The radius; while open, half the leg under way (what frames the run: the world map).
    pub leash: f32,
    /// The hero is held inside; open on a route's road.
    pub fenced: bool,
    /// `(stage, 0)` on a zone and before a route starts, `(stage, stop)` on the road and
    /// `(stage, 255)` in the end ring.
    pub key: (usize, u8),
}

impl Bounds {
    /// [`super::hero::drive_hero`]'s rule: past twice the leash (just come from another
    /// battleground) back to the centre, past the leash to its edge; open, nowhere.
    pub fn hold(&self, pos: Vec3) -> Vec3 {
        if !self.fenced {
            return pos;
        }
        let off = flat(pos - self.centre);
        if off.length() > self.leash * 2.0 {
            self.centre
        } else if off.length() > self.leash {
            let held = self.centre + off.normalize() * self.leash;
            Vec3::new(held.x, pos.y, held.z)
        } else {
            pos
        }
    }

    /// A jump's rule ([`super::mobility`]): to the edge only; open, nowhere.
    pub fn clamp(&self, pos: Vec3) -> Vec3 {
        if !self.fenced {
            return pos;
        }
        let off = flat(pos - self.centre);
        if off.length() > self.leash {
            let held = self.centre + off.normalize() * self.leash;
            Vec3::new(held.x, pos.y, held.z)
        } else {
            pos
        }
    }
}

/// The [`Bounds`] of battleground `stage` with the run at `route`.
pub(crate) fn bounds_for(stage: usize, route: &RouteRun) -> Bounds {
    let def = &data::MAPS[stage.min(data::MAPS.len() - 1)];
    let zone = Bounds {
        centre: super::hero::arena(def),
        leash: def.leash,
        fenced: true,
        key: (stage, 0),
    };
    let Some(r) = route_of(stage) else {
        return zone;
    };
    let points = r.points();
    let last = r.stations.len() - 1;
    if route.arrived {
        return Bounds {
            centre: points[r.point_of(last)].at,
            leash: END_RING,
            fenced: true,
            key: (stage, u8::MAX),
        };
    }
    if !route.placed || route.stop == 0 {
        // Before the start the hero is held at the first station as on a zone.
        return zone;
    }
    let (a, b) = (
        points[r.point_of(route.stop - 1)].at,
        points[r.point_of(route.stop.min(last))].at,
    );
    Bounds {
        centre: (a + b) * 0.5,
        leash: flat(b - a).length() * 0.5,
        fenced: false,
        key: (stage, route.stop.min(usize::from(u8::MAX - 1)) as u8),
    }
}

/// The pace over the class's own: [`TRAVEL_SPEED`] on a route, else 1.
pub(crate) fn travel_speed(selection: &Selection) -> f32 {
    if route_of(selection.stage).is_some() {
        TRAVEL_SPEED
    } else {
        1.0
    }
}

/// Keep [`Bounds`] on the battleground stood in and the run's place on its route. Every frame,
/// before the hero moves; written only when it changes.
pub(crate) fn update_bounds(
    selection: Res<Selection>,
    route: Res<RouteRun>,
    mut bounds: ResMut<Bounds>,
) {
    let want = bounds_for(selection.stage, &route);
    if *bounds != want {
        *bounds = want;
    }
}

pub(super) fn plugin(app: &mut App) {
    app.init_resource::<RouteRun>()
        .init_resource::<Bounds>()
        .add_systems(Update, update_bounds.before(WorldStage::Input))
        .add_systems(OnEnter(Phase::Menu), |mut route: ResMut<RouteRun>| {
            *route = RouteRun::default();
        });
}

#[cfg(test)]
mod tests {
    use super::*;

    fn undying() -> (usize, &'static RouteDef) {
        let map = data::MAPS
            .iter()
            .position(|m| m.name == "Undying Run")
            .expect("the Undying Run is a battleground");
        (map, route_of(map).expect("a route"))
    }

    #[test]
    fn every_station_names_a_zone_with_a_horde_and_bosses() {
        for r in ROUTES {
            let map = data::MAPS
                .iter()
                .position(|m| m.name == r.map)
                .unwrap_or_else(|| panic!("{}: no battleground", r.map));
            assert!(r.stations.len() >= 3, "{}: stops between its ends", r.map);
            let def = &data::MAPS[map];
            for (i, s) in r.stations.iter().enumerate() {
                let zone = r.zone_of(i);
                assert!(route_of(zone).is_none(), "{}: a route on a route", s.zone);
                assert!(data::roster(zone).enemies.len() >= 3, "{}: a horde", s.zone);
                assert!(!super::super::finale::plan(zone).timed.is_empty(), "{}: bosses", s.zone);
                assert_eq!(data::MAPS[zone].map, def.map, "{}: one continent", s.zone);
            }
            // The run starts where the hero is set down.
            assert_eq!([def.center[0], def.center[1]], r.stations[0].at);
            assert_eq!(def.leash, END_RING);
            let points = r.points();
            assert_eq!(points.iter().filter(|p| p.station.is_some()).count(), r.stations.len());
            assert_eq!(points.last().unwrap().station, Some(r.stations.len() - 1));
            assert!(points.iter().all(|p| p.at.y == 0.0));
        }
    }

    #[test]
    fn route_frac_counts_legs_behind_and_along() {
        let p = [
            Vec3::ZERO,
            Vec3::new(100.0, 0.0, 0.0),
            Vec3::new(100.0, 0.0, 100.0),
        ];
        assert!((route_frac(&p, 1, Vec3::new(50.0, 0.0, 0.0)) - 0.25).abs() < 1e-4);
        assert!((route_frac(&p, 2, Vec3::new(100.0, 0.0, 50.0)) - 0.75).abs() < 1e-4);
        // Height plays no part, nor a hero off to the side of the leg.
        assert!((route_frac(&p, 1, Vec3::new(50.0, 30.0, 20.0)) - 0.25).abs() < 1e-4);
        assert_eq!(route_frac(&p, 3, Vec3::ZERO), 1.0);
        assert_eq!(route_frac(&p, 0, Vec3::ZERO), 0.0);
    }

    #[test]
    fn fenced_bounds_hold_as_the_old_leash_did() {
        let b = Bounds {
            centre: Vec3::ZERO,
            leash: 50.0,
            fenced: true,
            key: (0, 0),
        };
        assert_eq!(b.hold(Vec3::new(30.0, 2.0, 0.0)), Vec3::new(30.0, 2.0, 0.0));
        assert_eq!(b.hold(Vec3::new(60.0, 2.0, 0.0)), Vec3::new(50.0, 2.0, 0.0));
        assert_eq!(b.hold(Vec3::new(120.0, 2.0, 0.0)), Vec3::ZERO);
        assert_eq!(b.clamp(Vec3::new(120.0, 2.0, 0.0)), Vec3::new(50.0, 2.0, 0.0));
        assert_eq!(b.clamp(Vec3::new(0.0, 2.0, -30.0)), Vec3::new(0.0, 2.0, -30.0));
    }

    #[test]
    fn open_bounds_hold_nothing() {
        let b = Bounds {
            centre: Vec3::ZERO,
            leash: 50.0,
            fenced: false,
            key: (0, 1),
        };
        for p in [Vec3::new(30.0, 2.0, 0.0), Vec3::new(600.0, 2.0, 0.0), Vec3::splat(-5000.0)] {
            assert_eq!(b.hold(p), p);
            assert_eq!(b.clamp(p), p);
        }
    }

    #[test]
    fn a_zone_keeps_its_centre_and_leash() {
        for (stage, def) in data::MAPS.iter().enumerate() {
            if route_of(stage).is_some() {
                continue;
            }
            let b = bounds_for(stage, &RouteRun::default());
            assert_eq!(b.centre, super::super::hero::arena(def), "{}", def.name);
            assert_eq!(b.leash, def.leash, "{}", def.name);
            assert!(b.fenced);
            assert_eq!(b.key, (stage, 0));
        }
    }

    #[test]
    fn a_route_is_open_on_the_road_and_rings_its_end() {
        let (map, r) = undying();
        let start = bounds_for(map, &RouteRun::default());
        assert!(start.fenced && start.key == (map, 0));
        let mut run = RouteRun {
            next: 1,
            stop: 1,
            arrived: false,
            placed: true,
        };
        let road = bounds_for(map, &run);
        assert!(!road.fenced);
        assert_eq!(road.key, (map, 1));
        run.stop = 2;
        assert_ne!(bounds_for(map, &run).key, road.key);
        run.stop = r.stations.len();
        run.next = r.points().len();
        run.arrived = true;
        let ring = bounds_for(map, &run);
        assert!(ring.fenced);
        assert_eq!(ring.leash, END_RING);
        assert_eq!(ring.centre, r.points().last().unwrap().at);
    }

    #[test]
    fn the_horde_changes_zone_halfway_down_the_leg() {
        let (_, r) = undying();
        let (from, to) = (r.points()[r.point_of(0)].at, r.points()[r.point_of(1)].at);
        let run = RouteRun {
            next: r.point_of(0) + 1,
            stop: 1,
            arrived: false,
            placed: true,
        };
        assert_eq!(leg_zone(r, &run, from.lerp(to, 0.3)), r.zone_of(0));
        assert_eq!(leg_zone(r, &run, from.lerp(to, 0.7)), r.zone_of(1));
        assert_eq!(leg_zone(r, &RouteRun::default(), from), r.zone_of(0));
        let end = RouteRun {
            stop: r.stations.len(),
            arrived: true,
            ..run
        };
        assert_eq!(leg_zone(r, &end, to), r.zone_of(r.stations.len() - 1));
    }

    #[test]
    fn the_heading_points_down_the_road() {
        let (_, r) = undying();
        let from = r.points()[0].at;
        let run = RouteRun {
            next: 1,
            stop: 1,
            arrived: false,
            placed: true,
        };
        let a = heading(r, &run, from).expect("a heading");
        let to = flat(r.points()[1].at - from).normalize();
        assert!((Vec3::new(a.cos(), 0.0, a.sin()) - to).length() < 1e-4);
        assert_eq!(heading(r, &RouteRun { arrived: true, ..run }, from), None);
    }
}
