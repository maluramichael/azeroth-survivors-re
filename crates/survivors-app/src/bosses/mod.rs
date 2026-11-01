//! Boss scripts: a boss with a [`kits`] row fights with its own moves instead of the shared turns
//! of circle and volley ([`super::threats`]).
//!
//! Groundwork only so far; the boss-mechanics package brings:
//! - `shapes.rs`: `Shape { Circle, Cone, Line, Donut }` with `contains(origin, yaw, p)`,
//!   `forward(yaw)` and `min_windup()`, after deanmathieson/azeroth-survivors-re, branch
//!   `progression`, `10c6a77` (`raid/`), MIT OR Apache-2.0;
//! - here the script model (`When`, `Aim`, `Does`, `BossAbility`, `Kit`), `kit_for(name)`, and
//!   the brain that runs it: `attach_kits`, `run_kits`, `charge` (a boss under
//!   [`super::enemies::Steered`]), `burn_pools` and `boss_loot` (a [`super::items::BossDrop`]);
//! - [`kits`]: a kit per boss after the plan's table, and the adds they call ([`adds_for`]).

mod kits;

use bevy::prelude::*;

use super::data::EnemyDef;

/// The boss scripts' systems. Empty until the boss-mechanics package.
pub(super) fn plugin(_app: &mut App) {}

/// Whether the boss called `name` has a script of its own, so [`super::threats`] gives it none of
/// the shared turns. No boss has one yet.
pub(super) fn scripted(_name: &str) -> bool {
    false
}

/// The adds battleground `map`'s boss scripts call, the first part of [`super::data::extras`]
/// (found again by co-op as `(5, i)`). Their numbers come from [`super::data::rung`]. None yet.
pub(super) fn adds_for(_map: usize) -> Vec<EnemyDef> {
    Vec::new()
}

/// The boss scripts' systems, for the balance simulator ([`super::balance`]): `run_kits`,
/// `charge`, `burn_pools` and `boss_loot` once they exist.
#[cfg(test)]
#[allow(dead_code)]
pub(super) fn sim_systems() -> bevy::ecs::schedule::ScheduleConfigs<bevy::ecs::system::ScheduleSystem> {
    idle.into_configs()
}

#[cfg(test)]
fn idle() {}
