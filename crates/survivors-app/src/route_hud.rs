//! What the Undying Run shows ([`super::route`]): the gold arrow over the hero toward the next
//! road point and the line under the banner with the next station, the yards left and the count.
//! The minimap's gold dot for the station is drawn by [`super::minimap`], the menu row and the
//! "Final boss: Duskwood" line by [`super::menu`] and [`super::dmgmeter`]; all read the same
//! [`next_station`].
//!
//! Both hide off a route, while the hero is down, before the start and once the last station is
//! reached (the end ring stands and the finale comes). A co-op guest draws them too, from the run
//! the host sent.
//!
//! Parts follow marekvi95/undying-run (branch continent-run, 84cc7c0, MIT OR Apache-2.0).

use std::f32::consts::FRAC_PI_2;

use bevy::light::NotShadowCaster;
use bevy::mesh::ConeMeshBuilder;
use bevy::prelude::*;

use super::hero::Hero;
use super::hud::GOLD_TONE;
use super::kit::{self, KitFonts};
use super::route::{route_of, Point, RouteDef, RouteRun};
use super::units::{flat, yaw_toward};
use super::{Phase, Run, Selection};

/// How far above the hero's feet the arrow hovers, in yards.
const ARROW_HOVER: f32 = 4.0;
/// Above the HUD and level with [`super::dmgmeter`]'s window.
const Z: i32 = 401;
/// Where the line sits: the banner's top (22 %), then the banner's own height below it.
const LINE_TOP_PERCENT: f32 = 22.0;
const LINE_BELOW_BANNER_PX: f32 = 42.0;

/// The arrow over the hero, turned by [`show_route`] to the next road point.
#[derive(Component)]
struct RouteArrow;

/// The line's holder, shown and hidden as a whole.
#[derive(Component)]
struct RouteLineRoot;

/// The line under the banner: the next station, how far, and which of how many.
#[derive(Component)]
struct RouteLine;

/// The arrow's and the line's systems.
pub(super) fn plugin(app: &mut App) {
    app.add_systems(Startup, spawn_arrow).add_systems(
        Update,
        (build_line, show_route)
            .chain()
            .after(super::hero::drive_hero),
    );
}

/// The next station and the way to it: what the arrow, the line and the minimap's dot show.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) struct NextStation {
    /// The station's index in [`RouteDef::stations`].
    pub station: usize,
    /// Where it stands, flat in Bevy coordinates.
    pub at: Vec3,
}

/// The station the run walks to, `None` before the hero is set down and once the last is
/// reached.
pub(super) fn next_station(def: &RouteDef, run: &RouteRun) -> Option<NextStation> {
    if !run.placed || run.arrived || run.stop >= def.stations.len() {
        return None;
    }
    let point = def.points().get(def.point_of(run.stop))?;
    Some(NextStation {
        station: run.stop,
        at: point.at,
    })
}

/// The arrow's turn and the line's words for a hero at `at`.
#[derive(Clone, Debug, PartialEq)]
struct Aim {
    /// From the hero to the next road point (not the station: the road bends).
    to: Vec3,
    line: String,
}

/// Where the arrow points and what the line says, `None` where neither shows.
fn aim(def: &RouteDef, run: &RouteRun, at: Vec3) -> Option<Aim> {
    let station = next_station(def, run)?;
    let points = def.points();
    let next = points.get(run.next)?;
    let target = def.point_of(station.station);
    Some(Aim {
        to: flat(next.at - at),
        line: route_line(
            def.stations[station.station].label,
            yards_left(points, run.next, target, at),
            station.station + 1,
            def.stations.len(),
        ),
    })
}

/// The yards from `at` to the point `target` along the road points from `next` on: the way to
/// the next point, then the legs between it and the target.
fn yards_left(points: &[Point], next: usize, target: usize, at: Vec3) -> f32 {
    let Some(last) = points.len().checked_sub(1) else {
        return 0.0;
    };
    let target = target.min(last);
    let first = next.min(target);
    let legs: f32 = points[first..=target]
        .windows(2)
        .map(|w| flat(w[1].at - w[0].at).length())
        .sum();
    flat(points[first].at - at).length() + legs
}

/// "Next: Elwynn Forest · 830 yd · 2 of 4": the station the run walks to, the yards left, and
/// that station's number (counting the start) of how many stations the route has.
fn route_line(label: &str, yards: f32, stop: usize, stops: usize) -> String {
    format!("Next: {label} \u{b7} {yards:.0} yd \u{b7} {stop} of {stops}")
}

/// The arrow: a gold shaft and a four-sided head, three yards from tail to tip. Built once at
/// startup and hidden until a route is walked; it is not a run entity, so it outlives the runs.
fn spawn_arrow(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
) {
    let gold = materials.add(StandardMaterial {
        base_color: GOLD_TONE,
        unlit: true,
        ..default()
    });
    let shaft = meshes.add(Cuboid::new(0.3, 0.3, 2.0));
    // Four sides rather than a smooth cone, so the head reads as a point.
    let head = meshes.add(
        ConeMeshBuilder {
            cone: Cone {
                radius: 0.5,
                height: 1.0,
            },
            resolution: 4,
            ..default()
        }
        .build(),
    );
    commands
        .spawn((RouteArrow, Transform::default(), Visibility::Hidden))
        .with_children(|arrow| {
            arrow.spawn((
                Mesh3d(shaft),
                MeshMaterial3d(gold.clone()),
                Transform::from_xyz(0.0, 0.0, 0.5),
                NotShadowCaster,
            ));
            // A cone points up its own Y; this turns the tip to the arrow's forward, -Z.
            arrow.spawn((
                Mesh3d(head),
                MeshMaterial3d(gold),
                Transform::from_xyz(0.0, 0.0, -1.0)
                    .with_rotation(Quat::from_rotation_x(-FRAC_PI_2)),
                NotShadowCaster,
            ));
        });
}

/// The turn that points the arrow's tip along the ground toward `to`.
fn arrow_turn(to: Vec3) -> Quat {
    Quat::from_rotation_y(yaw_toward(flat(to)))
}

/// Build the line once the fonts are up: a full-width row under the banner, hidden until a
/// route is walked.
fn build_line(mut commands: Commands, fonts: Option<Res<KitFonts>>, built: Query<(), With<RouteLine>>) {
    let Some(fonts) = fonts else { return };
    if !built.is_empty() {
        return;
    }
    commands
        .spawn((
            RouteLineRoot,
            Node {
                position_type: PositionType::Absolute,
                top: Val::Percent(LINE_TOP_PERCENT),
                margin: UiRect::top(Val::Px(LINE_BELOW_BANNER_PX)),
                width: Val::Percent(100.0),
                justify_content: JustifyContent::Center,
                display: Display::None,
                ..default()
            },
            GlobalZIndex(Z),
        ))
        .with_children(|row| {
            row.spawn((
                RouteLine,
                kit::text(&fonts.body, "", 16.0, GOLD_TONE),
                TextLayout::new_with_justify(Justify::Center),
            ));
        });
}

/// Hang the arrow over the hero, turned to the next road point, and word the line. Both hide off
/// a route, outside a run, while the hero is down and once the route is walked.
#[allow(clippy::too_many_arguments)]
fn show_route(
    phase: Res<State<Phase>>,
    selection: Res<Selection>,
    route: Res<RouteRun>,
    run: Res<Run>,
    hero: Query<&Transform, With<Hero>>,
    mut arrows: Query<(&mut Transform, &mut Visibility), (With<RouteArrow>, Without<Hero>)>,
    mut roots: Query<&mut Node, With<RouteLineRoot>>,
    mut lines: Query<&mut Text, With<RouteLine>>,
) {
    let playing = matches!(
        phase.get(),
        Phase::Playing | Phase::LevelUp | Phase::Paused
    );
    let shown = match (route_of(selection.stage), hero.single()) {
        (Some(def), Ok(h)) if playing && run.dead_t.is_none() => {
            aim(def, &route, h.translation).map(|aim| (aim, h.translation))
        }
        _ => None,
    };
    let want = if shown.is_some() {
        Visibility::Visible
    } else {
        Visibility::Hidden
    };
    for (mut tf, mut vis) in &mut arrows {
        if let Some((aim, at)) = &shown {
            tf.translation = *at + Vec3::Y * ARROW_HOVER;
            tf.rotation = arrow_turn(aim.to);
        }
        if *vis != want {
            *vis = want;
        }
    }
    let display = if shown.is_some() {
        Display::Flex
    } else {
        Display::None
    };
    for mut node in &mut roots {
        if node.display != display {
            node.display = display;
        }
    }
    if let Some((aim, _)) = &shown {
        for mut text in &mut lines {
            if text.0 != aim.line {
                text.0.clone_from(&aim.line);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn undying() -> &'static RouteDef {
        let map = super::super::data::MAPS
            .iter()
            .position(|m| m.name == "Undying Run")
            .expect("the Undying Run is a battleground");
        route_of(map).expect("a route")
    }

    #[test]
    fn arrow_points_at_the_next_point() {
        for to in [
            Vec3::X,
            -Vec3::Z,
            Vec3::new(1.0, 9.0, -2.0),
            Vec3::new(-3.0, 0.0, 4.0),
        ] {
            let tip = arrow_turn(to) * Vec3::NEG_Z;
            assert!(tip.distance(flat(to).normalize()) < 1e-5, "{to:?}");
        }
    }

    #[test]
    fn the_route_line_reads_stop_distance_and_count() {
        assert_eq!(
            route_line("Elwynn Forest", 830.2, 2, 4),
            "Next: Elwynn Forest \u{b7} 830 yd \u{b7} 2 of 4"
        );
        assert_eq!(
            route_line("Duskwood", 12.6, 4, 4),
            "Next: Duskwood \u{b7} 13 yd \u{b7} 4 of 4"
        );
    }

    #[test]
    fn the_distance_runs_along_the_road_points_to_the_station() {
        let at = |x: f32, z: f32| Point {
            at: Vec3::new(x, 0.0, z),
            station: None,
        };
        let points = [at(0.0, 0.0), at(100.0, 0.0), at(100.0, 100.0), at(100.0, 300.0)];
        // From the start: 100 to the first, then the legs to the station at index 2.
        assert!((yards_left(&points, 1, 2, Vec3::ZERO) - 200.0).abs() < 1e-3);
        // Part way along the first leg, height and side play no part.
        assert!((yards_left(&points, 1, 2, Vec3::new(40.0, 7.0, 0.0)) - 160.0).abs() < 1e-3);
        // A point past the station (a cut corner) counts the straight way to it.
        assert!((yards_left(&points, 3, 2, Vec3::new(100.0, 0.0, 40.0)) - 60.0).abs() < 1e-3);
        assert_eq!(yards_left(&[], 0, 0, Vec3::ZERO), 0.0);
    }

    #[test]
    fn nothing_shows_before_the_start_and_after_the_end() {
        let def = undying();
        let at = def.points()[0].at;
        let walking = RouteRun {
            next: 1,
            stop: 1,
            arrived: false,
            placed: true,
        };
        let aimed = aim(def, &walking, at).expect("an aim on the road");
        let to = flat(def.points()[1].at - at);
        assert!((aimed.to - to).length() < 1e-3);
        assert!(aimed.line.starts_with(&format!("Next: {}", def.stations[1].label)));
        assert!(aimed.line.ends_with(&format!("2 of {}", def.stations.len())));
        assert_eq!(next_station(def, &walking).map(|n| n.station), Some(1));
        let unplaced = RouteRun {
            placed: false,
            ..walking.clone()
        };
        assert_eq!(aim(def, &unplaced, at), None);
        assert_eq!(next_station(def, &unplaced), None);
        let arrived = RouteRun {
            arrived: true,
            stop: def.stations.len(),
            next: def.points().len(),
            ..walking
        };
        assert_eq!(aim(def, &arrived, at), None);
        assert_eq!(next_station(def, &arrived), None);
    }
}
