//! Smoothing for replicated bodies: they ease toward the last position the host sent.

use bevy::prelude::*;

/// The ease toward the host's place (s): about two replication rounds.
const TAU: f32 = 0.1;
/// A gap past which a body cuts to the host's place instead of easing there, in yards.
const SNAP: f32 = 8.0;

/// `current` moved toward `target` by one frame of `dt` seconds: exponential, so the gap closes by
/// the same share each frame whatever the frame rate. A gap over [`SNAP`] (a teleport, a new zone)
/// is jumped.
pub(super) fn follow(current: Vec3, target: Vec3, dt: f32) -> Vec3 {
    if current.distance(target) > SNAP {
        return target;
    }
    current.lerp(target, 1.0 - (-dt / TAU).exp())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn follow_closes_most_of_the_gap_in_a_third_of_a_second() {
        let mut p = Vec3::ZERO;
        for _ in 0..20 {
            p = follow(p, Vec3::X, 1.0 / 60.0);
        }
        assert!(p.x > 0.95 && p.x <= 1.0);
    }

    #[test]
    fn a_teleport_snaps() {
        assert_eq!(
            follow(Vec3::ZERO, Vec3::new(20.0, 0.0, 0.0), 0.016),
            Vec3::new(20.0, 0.0, 0.0)
        );
    }
}
