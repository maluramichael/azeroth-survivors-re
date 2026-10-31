//! The loot tables: per zone, what drops there and how likely, from grunts, elites, the timed
//! bosses and the finale ([`super::items`] weights, [`super::finale::plan`]). Opened from the hub's
//! Loot Tables tab.

use bevy::prelude::*;

/// Whether the loot tables window is open; the hub's tab sets it.
#[derive(Resource, Default)]
pub(crate) struct LootUi {
    pub(crate) open: bool,
}

pub(crate) fn plugin(app: &mut App) {
    app.init_resource::<LootUi>();
}
