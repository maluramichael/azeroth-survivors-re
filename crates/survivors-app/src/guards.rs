//! Guard posts: elite camps at fixed posts around the zone, each guarding a locked chest
//! ([`super::chests::Locked`]) or a shrine (ticket #1121).
//!
//! Groundwork only so far; the guard-posts package brings `post_offsets(leash, phase)` and
//! `wake_radius(leash)`, a `Post` per camp with its [`super::minimap::MapMarker`], an elite that
//! patrols and two guards that stand (all [`super::enemies::Steered`] until woken), the reward
//! that opens once the camp falls, and the camp's return three minutes after.

use bevy::prelude::*;

/// The guard posts' systems. Empty until the guard-posts package.
pub(super) fn plugin(_app: &mut App) {}

/// The guard posts' systems, for the balance simulator ([`super::balance`]).
#[cfg(test)]
#[allow(dead_code)]
pub(super) fn sim_systems() -> bevy::ecs::schedule::ScheduleConfigs<bevy::ecs::system::ScheduleSystem> {
    idle.into_configs()
}

#[cfg(test)]
fn idle() {}
