//! The Undying Run's course ([`super::route`]): the start, the stations reached in turn with a
//! rest at each, the end ring round the last, and the horde and the gems cut away far behind
//! every hero. A stub until the route's run package fills it in.
//!
//! Parts follow marekvi95/undying-run (branch continent-run, 84cc7c0, MIT OR Apache-2.0).

use bevy::prelude::*;

/// The run's systems: placing the hero at the start, tracking the stations, cutting the horde
/// behind, the beacon on the next station. None yet.
pub(super) fn plugin(_app: &mut App) {}

/// The resources the balance simulator ([`super::balance`]) needs for [`sim_systems`]. None yet.
#[cfg(test)]
#[allow(dead_code)]
pub(super) fn sim_init(_app: &mut App) {}

/// The run's systems for the balance simulator ([`super::balance`]); an idle one for now.
#[cfg(test)]
#[allow(dead_code)]
pub(super) fn sim_systems() -> bevy::ecs::schedule::ScheduleConfigs<bevy::ecs::system::ScheduleSystem> {
    idle.into_configs()
}

#[cfg(test)]
#[allow(dead_code)]
fn idle() {}
