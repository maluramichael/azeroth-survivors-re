//! The world map: M in a run opens the zone the battleground lies in, as WoW's world map shows
//! it, or the battlefield itself up close ([`run`], [`field`]); the hub's World Map window picks
//! the zone on the continent ([`hub`]). The pages, their placement and the DBCs are [`atlas`].
//!
//! What the map marks comes from three places. The minimap's [`MapMarker`] already sits on guard
//! posts, their chests and shrines, bosses, rares and the bonus portal, and reaches a co-op guest
//! over the wire. Every drop on the ground carries its [`DropArt`] (gear, shards, chests, read
//! with [`loot_tint`]). [`MapPin`] marks only what neither covers: quest items and the party.
//!
//! [`MapMarker`]: crate::minimap::MapMarker
//! [`DropArt`]: crate::dropart::DropArt

// Task 0 of the world-map round: the windows that read these types are still to come.
#[allow(dead_code)]
pub(crate) mod atlas;
mod field;
mod hub;
mod run;

use bevy::prelude::*;

use crate::dropart::DropArt;

/// The run's map window. `level` outlives the run, so the next opening shows the last one used.
#[derive(Resource, Default)]
pub(crate) struct WorldMapUi {
    pub open: bool,
    #[allow(dead_code)]
    pub level: MapLevel,
    /// The map put the run into `Phase::Paused` (alone) and must resume it on closing.
    #[allow(dead_code)]
    pub paused: bool,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
#[allow(dead_code)]
pub(crate) enum MapLevel {
    /// The zone's WorldMap page, the battleground a circle on it.
    #[default]
    Zone,
    /// The battleground up close, on the minimap's ground art.
    Field,
}

/// The hub's World Map window.
#[derive(Resource, Default)]
pub(crate) struct HubMapUi {
    pub open: bool,
    /// The continent shown, a `Map.dbc` id (0 Eastern Kingdoms, 1 Kalimdor); `None` follows the
    /// zone picked.
    #[allow(dead_code)]
    pub continent: Option<u32>,
    /// The battleground under the cursor, an index into `data::MAPS`.
    #[allow(dead_code)]
    pub hover: Option<usize>,
}

/// Something on the battlefield the map shows that neither the minimap's
/// [`MapMarker`](crate::minimap::MapMarker) nor a drop's [`DropArt`] names, put on it where it is
/// spawned: the owners keep their own components private.
#[derive(Component, Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum MapPin {
    /// An item a collect quest wants, on the ground ([`crate::quests`]).
    QuestItem,
    /// Another hero of the party: a guest's puppet on the host ([`crate::hero::spawn_peer`]), the
    /// host's or another guest's body on a guest (`coop::client`).
    Ally,
}

/// The colour a drop on the ground is marked in: gear in its quality's, a shard in a pale blue.
/// `None` for a chest, which the map draws as an icon of its own.
#[allow(dead_code)]
pub(crate) fn loot_tint(art: &DropArt) -> Option<Color> {
    match art {
        DropArt::Item { quality, .. } => Some(quality.color()),
        DropArt::Shard(_) => Some(Color::srgb(0.45, 0.85, 1.0)),
        DropArt::Chest(_) => None,
    }
}

/// The run window's two page slots: [`run`] fills `ZoneSlot`, [`field`] fills `FieldSlot`; the
/// window shows the one of `WorldMapUi::level`.
#[derive(Component)]
#[allow(dead_code)]
pub(crate) struct ZoneSlot;
#[derive(Component)]
#[allow(dead_code)]
pub(crate) struct FieldSlot;

/// The barrier's violet-blue, as `barrier.rs` tints the wall (between `FAR_TINT` and `NEAR_TINT`).
#[allow(dead_code)]
pub(crate) const BARRIER_TINT: Color = Color::srgb(0.55, 0.50, 1.0);

pub(super) fn plugin(app: &mut App) {
    app.init_resource::<WorldMapUi>()
        .init_resource::<HubMapUi>()
        .init_resource::<atlas::Atlas>();
    run::plugin(app);
    field::plugin(app);
    hub::plugin(app);
}
