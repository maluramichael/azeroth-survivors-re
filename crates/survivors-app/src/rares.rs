//! Rares and the bonus boss: named rares of the zone that walk out now and then between the
//! bosses, and a far stronger world boss behind a portal after the finale (ticket #1135).
//!
//! Groundwork only so far; the rares package brings the rare and bonus definitions
//! ([`rares_for`], [`bonus_for`]), the rare's spawn (a [`super::enemies::Rare`] with
//! `boss: true`, never counted in `run.bosses`), and the portal that starts the bonus fight with
//! [`super::Run::encore`].

use bevy::prelude::*;

use super::data::EnemyDef;

/// The rares' and the bonus boss's systems. Empty until the rares package.
pub(super) fn plugin(_app: &mut App) {}

/// Battleground `map`'s rares, the second part of [`super::data::extras`]. None yet.
pub(super) fn rares_for(_map: usize) -> Vec<EnemyDef> {
    Vec::new()
}

/// Battleground `map`'s bonus boss, the last part of [`super::data::extras`]. None yet.
pub(super) fn bonus_for(_map: usize) -> Vec<EnemyDef> {
    Vec::new()
}

/// The rares' systems, for the balance simulator ([`super::balance`]); the bonus fight lies past
/// the simulator's clock.
#[cfg(test)]
#[allow(dead_code)]
pub(super) fn sim_systems() -> bevy::ecs::schedule::ScheduleConfigs<bevy::ecs::system::ScheduleSystem> {
    idle.into_configs()
}

#[cfg(test)]
fn idle() {}
