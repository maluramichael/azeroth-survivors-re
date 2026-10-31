//! LAN search: a guest asks the local network who hosts, hosts answer. Question and answer over
//! UDP broadcast on [`SEARCH_PORT`], only while someone searches; no background thread.

use std::net::SocketAddr;

use bevy::prelude::*;

/// The port hosts listen on for the search (the game itself is on `net::DEFAULT_PORT`).
pub(super) const SEARCH_PORT: u16 = 37374;

pub(super) fn plugin(app: &mut App) {
    app.init_resource::<LanSearch>().init_resource::<LanGames>();
}

/// Set by the window while its LAN tab is open: this machine asks every second.
#[derive(Resource, Default)]
pub(super) struct LanSearch {
    pub active: bool,
}

/// The parties that answered lately, newest answer kept per host address.
#[derive(Resource, Default)]
pub(super) struct LanGames {
    pub games: Vec<LanGame>,
}

/// One host's answer.
#[derive(Clone, Debug, PartialEq)]
pub(super) struct LanGame {
    /// Where to join: the host's address and game port.
    pub addr: SocketAddr,
    /// The party's name ([`super::Lobby::name`], or "<host>'s party").
    pub lobby: String,
    /// The host's player name.
    pub host: String,
    pub class_id: u8,
    /// Index into `data::MAPS`.
    pub zone: usize,
    /// Players in the party, the host included, and the most it takes.
    pub players: u8,
    pub max_players: u8,
    /// A password is needed.
    pub locked: bool,
    /// `Time<Real>` seconds of the last answer; the list drops a host silent for 3 s.
    pub seen: f64,
}
