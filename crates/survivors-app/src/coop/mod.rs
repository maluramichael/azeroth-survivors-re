//! Direct-IP co-op: one player hosts, up to three join by address; no server in between.
//! The host simulates the run and replicates the heroes; a guest plays its own hero and sends
//! where it walks. Design: `docs/superpowers/specs/2026-10-31-coop-direct-ip-design.md`.

mod net;

use bevy::prelude::*;
use bevy_replicon::prelude::*;
use bevy_replicon_renet2::RepliconRenetPlugins;

pub(super) fn plugin(app: &mut App) {
    app.add_plugins((
        RepliconPlugins.set(ServerPlugin {
            // Ticked on the wall clock by the host, so a paused run still syncs.
            tick_schedule: None,
            ..default()
        }),
        RepliconRenetPlugins,
    ));
}
