//! The boss bar: a wide health bar at the top centre for every boss on the field.
//!
//! Groundwork only so far; the boss-bar package brings at most two bars (the newest bosses) under
//! the clock and the surge line with name, health and percent, a skull for a boss far above the
//! hero, "Final boss" and "Enraged" tags and a silver bar for a [`super::enemies::Rare`]; a fallen
//! boss's bar fades. On arrival the raid-warning horn sounds and the boss gets its
//! [`super::minimap::MapMarker`]. It only draws, so it runs on a co-op guest too.

use bevy::prelude::*;

/// The boss bar's systems. Empty until the boss-bar package.
pub(super) fn plugin(_app: &mut App) {}
