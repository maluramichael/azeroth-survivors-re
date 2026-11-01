//! The shapes a boss warns on the ground: circle, cone, line and ring.
//!
//! Taken from `raid/mod.rs` of deanmathieson/azeroth-survivors-re (branch `progression`, commit
//! `10c6a77`, MIT OR Apache-2.0): the shape enum, its `contains` and `forward`. Added here: the
//! wind-up floor of the plan, `reach`, `grown` and `escape`, which the telegraph meshes
//! ([`super::super::threats`]) and the simulator's pilot read (`min_windup`, `grown` and `escape`
//! are for the tests and the pilot).

use bevy::prelude::*;

use super::super::units::flat;

/// A ground telegraph's shape. Circles and rings sit on their origin; cones and lines start at the
/// origin and point along the telegraph's facing.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) enum Shape {
    Circle {
        radius: f32,
    },
    /// `half_angle` in radians either side of the facing.
    Cone {
        radius: f32,
        half_angle: f32,
    },
    Line {
        length: f32,
        width: f32,
    },
    /// A ring: safe near the middle and outside it.
    Donut {
        inner: f32,
        outer: f32,
    },
}

/// The ground-plane direction a yaw faces (the yaw [`super::super::units::yaw_toward`] gives).
pub(crate) fn forward(yaw: f32) -> Vec3 {
    Quat::from_rotation_y(yaw) * Vec3::NEG_Z
}

/// The shortest warning each shape gets, in seconds: a circle is read at a glance, the others
/// need longer to find the way out.
#[cfg(test)]
const WINDUP_CIRCLE: f32 = 1.5;
#[cfg(test)]
const WINDUP_OTHER: f32 = 2.0;

impl Shape {
    /// Whether `p` stands inside the shape laid at `origin`, facing `yaw`. Only the ground plane
    /// counts.
    pub(crate) fn contains(self, origin: Vec3, yaw: f32, p: Vec3) -> bool {
        let off = flat(p - origin);
        let d = off.length();
        let ahead = forward(yaw);
        match self {
            Shape::Circle { radius } => d <= radius,
            Shape::Donut { inner, outer } => d >= inner && d <= outer,
            Shape::Cone { radius, half_angle } => {
                if d > radius {
                    return false;
                }
                // At the apex a hero is in, whichever way it faces.
                d < 0.5 || ahead.angle_between(off) <= half_angle
            }
            Shape::Line { length, width } => {
                let along = off.dot(ahead);
                let side = off - ahead * along;
                (0.0..=length).contains(&along) && side.length() <= width / 2.0
            }
        }
    }

    /// The wind-up this shape needs at least before it strikes.
    #[cfg(test)]
    pub(crate) fn min_windup(self) -> f32 {
        match self {
            Shape::Circle { .. } => WINDUP_CIRCLE,
            Shape::Cone { .. } | Shape::Line { .. } | Shape::Donut { .. } => WINDUP_OTHER,
        }
    }

    /// The farthest the shape reaches from its origin, in yards.
    pub(crate) fn reach(self) -> f32 {
        match self {
            Shape::Circle { radius } | Shape::Cone { radius, .. } => radius,
            Shape::Donut { outer, .. } => outer,
            Shape::Line { length, width } => length.hypot(width / 2.0),
        }
    }

    /// The shape `by` yards wider all round, for a margin of safety.
    #[cfg(test)]
    pub(crate) fn grown(self, by: f32) -> Shape {
        match self {
            Shape::Circle { radius } => Shape::Circle { radius: radius + by },
            Shape::Cone { radius, half_angle } => Shape::Cone {
                radius: radius + by,
                half_angle,
            },
            Shape::Line { length, width } => Shape::Line {
                length: length + by,
                width: width + 2.0 * by,
            },
            Shape::Donut { inner, outer } => Shape::Donut {
                inner: (inner - by).max(0.0),
                outer: outer + by,
            },
        }
    }

    /// The way out of the shape from `p` (a unit vector on the ground plane): away from the
    /// origin of a circle, sideways out of a cone or a line, the nearer way out of a ring.
    #[cfg(test)]
    pub(crate) fn escape(self, origin: Vec3, yaw: f32, p: Vec3) -> Vec3 {
        let off = flat(p - origin);
        let ahead = forward(yaw);
        let radial = off.normalize_or(ahead);
        match self {
            Shape::Circle { .. } => radial,
            Shape::Donut { inner, outer } => {
                let d = off.length();
                if d - inner < outer - d {
                    -radial
                } else {
                    radial
                }
            }
            Shape::Cone { .. } | Shape::Line { .. } => {
                let side = off - ahead * off.dot(ahead);
                side.normalize_or(Vec3::new(-ahead.z, 0.0, ahead.x))
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const ORIGIN: Vec3 = Vec3::new(10.0, 3.0, -4.0);

    fn at(local: Vec3, yaw: f32) -> Vec3 {
        ORIGIN + Quat::from_rotation_y(yaw) * local
    }

    #[test]
    fn shapes_hold_what_they_should() {
        let yaw = 0.8;
        // A circle: on the rim in, a hair past it out, height ignored.
        let circle = Shape::Circle { radius: 5.0 };
        assert!(circle.contains(ORIGIN, 0.0, ORIGIN + Vec3::new(5.0, 40.0, 0.0)));
        assert!(!circle.contains(ORIGIN, 0.0, ORIGIN + Vec3::new(5.01, 0.0, 0.0)));
        // A cone: ahead in, behind out, the apex always in, past its reach out.
        let cone = Shape::Cone {
            radius: 10.0,
            half_angle: 0.5,
        };
        assert!(cone.contains(ORIGIN, yaw, at(Vec3::new(0.0, 0.0, -6.0), yaw)));
        assert!(cone.contains(ORIGIN, yaw, at(Vec3::new(2.0, 0.0, -6.0), yaw)));
        assert!(!cone.contains(ORIGIN, yaw, at(Vec3::new(0.0, 0.0, 6.0), yaw)));
        assert!(!cone.contains(ORIGIN, yaw, at(Vec3::new(6.0, 0.0, -2.0), yaw)));
        assert!(!cone.contains(ORIGIN, yaw, at(Vec3::new(0.0, 0.0, -10.5), yaw)));
        assert!(cone.contains(ORIGIN, yaw, ORIGIN + Vec3::new(0.2, 0.0, 0.1)));
        // A line: in its strip, out beside it, behind it and past its end.
        let line = Shape::Line {
            length: 20.0,
            width: 4.0,
        };
        assert!(line.contains(ORIGIN, yaw, at(Vec3::new(1.9, 0.0, -10.0), yaw)));
        assert!(line.contains(ORIGIN, yaw, at(Vec3::new(0.0, 0.0, -19.9), yaw)));
        assert!(!line.contains(ORIGIN, yaw, at(Vec3::new(2.1, 0.0, -10.0), yaw)));
        assert!(!line.contains(ORIGIN, yaw, at(Vec3::new(0.0, 0.0, 1.0), yaw)));
        assert!(!line.contains(ORIGIN, yaw, at(Vec3::new(0.0, 0.0, -20.5), yaw)));
        // A ring: inside its hole and outside it are safe, between is not.
        let ring = Shape::Donut {
            inner: 4.0,
            outer: 12.0,
        };
        assert!(!ring.contains(ORIGIN, 0.0, ORIGIN + Vec3::new(3.9, 0.0, 0.0)));
        assert!(ring.contains(ORIGIN, 0.0, ORIGIN + Vec3::new(4.0, 0.0, 0.0)));
        assert!(ring.contains(ORIGIN, 0.0, ORIGIN + Vec3::new(0.0, 0.0, -12.0)));
        assert!(!ring.contains(ORIGIN, 0.0, ORIGIN + Vec3::new(0.0, 0.0, -12.1)));
    }

    #[test]
    fn forward_is_the_way_a_yaw_faces() {
        use super::super::super::units::yaw_toward;
        for dir in [Vec3::X, Vec3::Z, Vec3::NEG_X, Vec3::NEG_Z, Vec3::new(1.0, 0.0, 2.0)] {
            let dir = dir.normalize();
            assert!((forward(yaw_toward(dir)) - dir).length() < 1e-5, "{dir}");
        }
    }

    #[test]
    fn grown_shapes_hold_what_the_plain_ones_do_and_a_margin_more() {
        let shapes = [
            Shape::Circle { radius: 5.0 },
            Shape::Cone {
                radius: 9.0,
                half_angle: 0.6,
            },
            Shape::Line {
                length: 18.0,
                width: 4.0,
            },
            Shape::Donut {
                inner: 4.0,
                outer: 12.0,
            },
        ];
        for shape in shapes {
            let big = shape.grown(1.0);
            for x in -30..=30 {
                for z in -30..=30 {
                    let p = Vec3::new(x as f32 * 0.5, 0.0, z as f32 * 0.5);
                    if shape.contains(Vec3::ZERO, 0.4, p) {
                        assert!(big.contains(Vec3::ZERO, 0.4, p), "{shape:?} at {p}");
                    }
                }
            }
        }
        // A hero a yard outside a circle is inside its margin.
        let circle = Shape::Circle { radius: 5.0 }.grown(1.0);
        assert!(circle.contains(Vec3::ZERO, 0.0, Vec3::new(5.9, 0.0, 0.0)));
        assert!(!circle.contains(Vec3::ZERO, 0.0, Vec3::new(6.1, 0.0, 0.0)));
    }

    #[test]
    fn walking_the_escape_leaves_every_shape() {
        let shapes = [
            Shape::Circle { radius: 6.0 },
            Shape::Cone {
                radius: 12.0,
                half_angle: 0.7,
            },
            Shape::Line {
                length: 24.0,
                width: 4.0,
            },
            Shape::Donut {
                inner: 4.0,
                outer: 14.0,
            },
        ];
        let yaw = -1.1;
        for shape in shapes {
            for x in -14..=14 {
                for z in -14..=14 {
                    let mut p = ORIGIN + Quat::from_rotation_y(yaw) * Vec3::new(x as f32, 0.0, z as f32 - 6.0);
                    if !shape.contains(ORIGIN, yaw, p) {
                        continue;
                    }
                    let mut steps = 0;
                    while shape.contains(ORIGIN, yaw, p) && steps < 60 {
                        let way = shape.escape(ORIGIN, yaw, p);
                        assert!((way.length() - 1.0).abs() < 1e-4, "{shape:?}: {way}");
                        p += way * 0.5;
                        steps += 1;
                    }
                    assert!(steps < 60, "{shape:?} from {x},{z} never leaves");
                }
            }
        }
    }

    #[test]
    fn every_shape_has_a_wind_up_and_a_reach() {
        assert_eq!(Shape::Circle { radius: 3.0 }.min_windup(), 1.5);
        for shape in [
            Shape::Cone {
                radius: 9.0,
                half_angle: 0.5,
            },
            Shape::Line {
                length: 20.0,
                width: 4.0,
            },
            Shape::Donut {
                inner: 4.0,
                outer: 12.0,
            },
        ] {
            assert_eq!(shape.min_windup(), 2.0);
        }
        // Everything inside a shape lies within its reach.
        let line = Shape::Line {
            length: 20.0,
            width: 4.0,
        };
        assert!(line.reach() >= 20.0);
        assert_eq!(Shape::Donut { inner: 4.0, outer: 12.0 }.reach(), 12.0);
    }
}
