//! Direct-IP co-op: one player hosts, up to three join by address; no server in between.
//! The host simulates the run and replicates the heroes; a guest plays its own hero and sends
//! where it walks. Design: `docs/superpowers/specs/2026-10-31-coop-direct-ip-design.md`.

mod net;
mod wire;

use bevy::prelude::*;
use bevy_replicon::prelude::*;
use bevy_replicon_renet2::RepliconRenetPlugins;

use super::chatlog::ChatLine;
use super::NetRole;

pub(super) fn plugin(app: &mut App) {
    app.add_plugins((
        RepliconPlugins.set(ServerPlugin {
            // Ticked on the wall clock by the host, so a paused run still syncs.
            tick_schedule: None,
            ..default()
        }),
        RepliconRenetPlugins,
    ));
    wire::protocol(app);
    app.init_resource::<Session>()
        .add_systems(Startup, connect_from_env)
        .add_systems(Update, post_notice);
}

/// The session as this machine sees it.
#[derive(Resource, Default)]
pub(super) struct Session {
    /// This machine's player number once seated: 0 for the host, 1 to 3 for a guest.
    pub player: Option<u8>,
    /// The name this player goes by (`WOW_SURVIVORS_NAME`, else the class's).
    pub name: String,
    /// On the host: the guests seated, in the order they came.
    pub guests: Vec<Guest>,
    /// A line for the chat window, posted once and cleared ([`post_notice`]).
    pub notice: Option<String>,
}

/// A guest the host has seated.
pub(super) struct Guest {
    /// renet's client id.
    pub client: u64,
    pub player: u8,
    pub hello: wire::Hello,
}

/// The lowest free seat for a new guest, 1 to 3; `None` when the party is full.
pub(super) fn free_slot(guests: &[Guest]) -> Option<u8> {
    (1..=net::MAX_CLIENTS as u8).find(|p| guests.iter().all(|g| g.player != *p))
}

/// `WOW_SURVIVORS_HOST=<port>` (empty or `1` for the default port) hosts at start,
/// `WOW_SURVIVORS_JOIN=<address>` joins; the launcher's `--host` and `--join` set them. A failure
/// is said in the log and the chat, and the game goes on alone.
fn connect_from_env(world: &mut World) {
    let var = benilla_app::survivors_host::webenv::var;
    let name = var("WOW_SURVIVORS_NAME").filter(|n| !n.trim().is_empty());
    world.resource_mut::<Session>().name = name.unwrap_or_default();
    if let Some(port) = var("WOW_SURVIVORS_HOST") {
        let port = match port.trim() {
            "" | "1" => Ok(net::DEFAULT_PORT),
            p => net::parse_port(p),
        };
        let made = port.and_then(|p| {
            net::host(world.resource::<RepliconChannels>(), p).map_err(|e| e.to_string())
        });
        let notice = match made {
            Ok((server, transport, port)) => {
                world.insert_resource(server);
                world.insert_resource(transport);
                *world.resource_mut::<NetRole>() = NetRole::Host;
                world.resource_mut::<Session>().player = Some(0);
                let at = net::lan_ip().map_or_else(|| "this machine".to_string(), |ip| ip.to_string());
                info!("survivors: co-op: hosting on UDP port {port}");
                format!("Hosting a party on {at}:{port}. Friends join with --join {at}:{port}")
            }
            Err(e) => {
                warn!("survivors: co-op: cannot host ({e})");
                format!("Cannot host a party: {e}")
            }
        };
        world.resource_mut::<Session>().notice = Some(notice);
    } else if let Some(addr) = var("WOW_SURVIVORS_JOIN") {
        let made = net::parse_addr(&addr).and_then(|to| {
            net::join(world.resource::<RepliconChannels>(), to)
                .map(|link| (link, to))
                .map_err(|e| e.to_string())
        });
        let notice = match made {
            Ok(((client, transport), to)) => {
                world.insert_resource(client);
                world.insert_resource(transport);
                *world.resource_mut::<NetRole>() = NetRole::Client;
                info!("survivors: co-op: joining {to}");
                format!("Joining the party at {to}...")
            }
            Err(e) => {
                warn!("survivors: co-op: cannot join {addr:?} ({e})");
                format!("Cannot join {addr}: {e}")
            }
        };
        world.resource_mut::<Session>().notice = Some(notice);
    }
}

/// The session's last word, into the chat window.
fn post_notice(mut session: ResMut<Session>, mut chat: MessageWriter<ChatLine>) {
    if let Some(text) = session.notice.take() {
        chat.write(ChatLine::system(text));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn guest(player: u8) -> Guest {
        Guest {
            client: u64::from(player) * 100,
            player,
            hello: wire::Hello {
                name: format!("P{player}"),
                class_id: 8,
                display: 0,
                race: 1,
                sex: 0,
            },
        }
    }

    #[test]
    fn freed_slot_is_reused() {
        assert_eq!(free_slot(&[]), Some(1));
        assert_eq!(free_slot(&[guest(1), guest(3)]), Some(2));
        assert_eq!(free_slot(&[guest(1), guest(2), guest(3)]), None);
    }
}
