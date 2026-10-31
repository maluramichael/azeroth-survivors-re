//! Achievements and the rewards they buy. Every achievement is worth points; earned points are
//! spent on perks that change the level-up screen ([`RunPerks`]): more rerolls, banishes, the
//! skip, a bigger first hand. Opened from the hub's Achievements tab. Earned achievements, the
//! lifetime counters and the perk ranks live in [`super::profile::Profile`].

use bevy::prelude::*;

/// Whether the achievements window is open; the hub's tab sets it.
#[derive(Resource, Default)]
pub(crate) struct AchievementsUi {
    pub(crate) open: bool,
}

/// What the level-up screen allows this run, from the perks bought. Set when a run starts; the
/// level-up screen ([`super::cards`]) spends `rerolls` and `banishes` and reads the rest.
#[derive(Resource, Clone, Debug, PartialEq)]
pub(crate) struct RunPerks {
    /// Hands that can still be dealt again this run.
    pub(crate) rerolls: u32,
    /// Cards that can still be banished this run: a banished choice never comes back this run.
    pub(crate) banishes: u32,
    /// Whether a hand can be skipped for gold.
    pub(crate) skip: bool,
    /// Cards in the first hand of the run (the others hold the usual three).
    pub(crate) first_hand: usize,
}

impl Default for RunPerks {
    /// A run with no perks bought.
    fn default() -> Self {
        Self {
            rerolls: 1,
            banishes: 0,
            skip: true,
            first_hand: 3,
        }
    }
}

pub(crate) fn plugin(app: &mut App) {
    app.init_resource::<AchievementsUi>()
        .init_resource::<RunPerks>();
}
