//! Direct-IP co-op: one player hosts, up to three join by address; no server in between.
//! The host simulates the run and replicates the heroes; a guest plays its own hero and sends
//! where it walks. Design: `docs/superpowers/specs/2026-10-31-coop-direct-ip-design.md`.

mod client;
mod discovery;
mod host;
mod lobby;
mod net;
mod party;
mod smooth;
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
    host::plugin(app);
    client::plugin(app);
    discovery::plugin(app);
    lobby::plugin(app);
    party::plugin(app);
    app.init_resource::<Session>()
        .init_resource::<super::CoopUi>()
        .add_message::<CoopCommand>()
        .add_systems(Startup, connect_from_env)
        .add_systems(Update, (run_commands, post_notice).chain());
}

/// What the co-op window asks for; carried out by [`run_commands`], which needs the whole world.
#[derive(Message, Clone, Debug, PartialEq, Eq)]
pub(super) enum CoopCommand {
    /// Host a party with [`Session::lobby`]'s settings.
    Host,
    /// Join the party at `addr` (`host:port`, a bare address, or a name).
    Join { addr: String, password: String },
    /// Leave the party, or stop hosting it.
    Leave,
}

/// Where this machine's link stands, for the window.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(super) enum Link {
    #[default]
    Off,
    Hosting { port: u16 },
    /// Connecting, until the host's welcome.
    Joining,
    Joined,
}

/// The host's party settings, from the window's Host tab.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct Lobby {
    /// Shown in the LAN list; empty means "<name>'s party".
    pub name: String,
    /// Players including the host, 2 to 4.
    pub max_players: u8,
    /// Empty: anyone who reaches the port may join.
    pub password: String,
    /// Answers the LAN search; a private party is joined by address only.
    pub public: bool,
    pub port: u16,
}

impl Default for Lobby {
    fn default() -> Self {
        Self {
            name: String::new(),
            max_players: 4,
            password: String::new(),
            public: true,
            port: net::DEFAULT_PORT,
        }
    }
}

impl Lobby {
    /// Guest seats a party of `max_players` has, at most what the transport takes.
    pub fn seats(max_players: u8) -> u8 {
        max_players.saturating_sub(1).min(net::MAX_CLIENTS as u8)
    }
}

/// How far apart the party's heroes start, per player number, from the battleground's centre:
/// every hero is set down there, and two heroes on one spot look like one.
pub(super) const ASIDE_YARDS: f32 = 3.0;

/// Whether a guest's `given` password opens a party locked with `password` (none: always).
pub(super) fn admits(password: &str, given: &str) -> bool {
    password.is_empty() || password == given
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
    pub link: Link,
    /// The party settings this machine hosts with.
    pub lobby: Lobby,
    /// The password this machine joins with.
    pub join_password: String,
    /// The window's status line: what happened last, in words.
    pub status: String,
    /// A guest's return to the menu was the host's doing ([`wire::EndRun`]) or comes from the
    /// results of a run the host ended ([`wire::RunOver`]); any other return leaves the party.
    pub host_sent_menu: bool,
}

/// A guest the host has seated.
pub(super) struct Guest {
    /// The connected client's entity (replicon's `ClientId::Client`); it despawns when the guest
    /// leaves.
    pub client: Entity,
    pub player: u8,
    pub hello: wire::Hello,
    /// The guest's last word on what its hero brings ([`wire::Loadout`]).
    pub loadout: Option<wire::Loadout>,
}

/// The lowest free seat for a new guest out of `seats` ([`Lobby::seats`]); `None` when full.
pub(super) fn free_slot(guests: &[Guest], seats: u8) -> Option<u8> {
    (1..=seats).find(|p| guests.iter().all(|g| g.player != *p))
}

/// Host with the session's lobby settings: bind, insert the server, and say where friends join.
pub(super) fn host_now(world: &mut World) -> Result<String, String> {
    if *world.resource::<NetRole>() != NetRole::Solo {
        return Err("Leave the current party first".into());
    }
    let port = world.resource::<Session>().lobby.port;
    let (server, transport, port) =
        net::host(world.resource::<RepliconChannels>(), port).map_err(|e| e.to_string())?;
    world.insert_resource(server);
    world.insert_resource(transport);
    // From now on every effect played is kept for the guests ([`host::send_fx`]).
    world.init_resource::<crate::units::FxOut>();
    *world.resource_mut::<NetRole>() = NetRole::Host;
    let mut session = world.resource_mut::<Session>();
    session.player = Some(0);
    session.link = Link::Hosting { port };
    let at = net::lan_ip().map_or_else(|| "this machine".to_string(), |ip| ip.to_string());
    info!("survivors: co-op: hosting on {at}:{port} (UDP)");
    Ok(format!("Hosting on {at}:{port}. Friends join with that address."))
}

/// Join the party at `addr`, with `password`.
pub(super) fn join_now(world: &mut World, addr: &str, password: &str) -> Result<String, String> {
    if *world.resource::<NetRole>() != NetRole::Solo {
        return Err("Leave the current party first".into());
    }
    let to = net::parse_addr(addr)?;
    let (client, transport) =
        net::join(world.resource::<RepliconChannels>(), to).map_err(|e| e.to_string())?;
    world.insert_resource(client);
    world.insert_resource(transport);
    *world.resource_mut::<NetRole>() = NetRole::Client;
    let mut session = world.resource_mut::<Session>();
    session.link = Link::Joining;
    session.join_password = password.to_string();
    info!("survivors: co-op: joining {to}");
    Ok(format!("Joining the party at {to}..."))
}

/// Leave the party or stop hosting it; back to playing alone.
pub(super) fn leave_now(world: &mut World) -> String {
    match *world.resource::<NetRole>() {
        NetRole::Solo => return "Not in a party".into(),
        NetRole::Client => client::leave(world),
        NetRole::Host => host::stop(world),
    }
    *world.resource_mut::<NetRole>() = NetRole::Solo;
    let mut session = world.resource_mut::<Session>();
    session.player = None;
    session.guests.clear();
    session.link = Link::Off;
    "You left the party.".into()
}

/// Carry out what the window asked for; the outcome goes to the status line and the chat.
fn run_commands(world: &mut World) {
    let commands: Vec<CoopCommand> = world.resource_mut::<Messages<CoopCommand>>().drain().collect();
    for command in commands {
        let said = match command {
            CoopCommand::Host => host_now(world).unwrap_or_else(|e| format!("Cannot host: {e}")),
            CoopCommand::Join { addr, password } => join_now(world, &addr, &password)
                .unwrap_or_else(|e| format!("Cannot join {addr}: {e}")),
            CoopCommand::Leave => leave_now(world),
        };
        let mut session = world.resource_mut::<Session>();
        session.status = said.clone();
        session.notice = Some(said);
    }
}

/// `WOW_SURVIVORS_HOST=<port>` (empty or `1` for the default port) hosts at start,
/// `WOW_SURVIVORS_JOIN=<address>` joins; the launcher's `--host` and `--join` set them. A failure
/// is said in the log and the chat, and the game goes on alone.
fn connect_from_env(world: &mut World) {
    let var = benilla_app::survivors_host::webenv::var;
    let name = var("WOW_SURVIVORS_NAME").filter(|n| !n.trim().is_empty());
    world.resource_mut::<Session>().name = name.unwrap_or_default();
    if let Some(port) = var("WOW_SURVIVORS_HOST") {
        let said = match port.trim() {
            "" | "1" => Ok(net::DEFAULT_PORT),
            p => net::parse_port(p),
        }
        .and_then(|p| {
            world.resource_mut::<Session>().lobby.port = p;
            host_now(world)
        })
        .unwrap_or_else(|e| {
            warn!("survivors: co-op: cannot host ({e})");
            format!("Cannot host a party: {e}")
        });
        world.resource_mut::<Session>().notice = Some(said);
    } else if let Some(addr) = var("WOW_SURVIVORS_JOIN") {
        let password = var("WOW_SURVIVORS_PASSWORD").unwrap_or_default();
        let said = join_now(world, &addr, &password).unwrap_or_else(|e| {
            warn!("survivors: co-op: cannot join {addr:?} ({e})");
            format!("Cannot join {addr}: {e}")
        });
        world.resource_mut::<Session>().notice = Some(said);
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
            client: Entity::PLACEHOLDER,
            player,
            loadout: None,
            hello: wire::Hello {
                name: format!("P{player}"),
                class_id: 8,
                display: 0,
                race: 1,
                sex: 0,
                password: String::new(),
            },
        }
    }

    #[test]
    fn freed_slot_is_reused() {
        assert_eq!(free_slot(&[], 3), Some(1));
        assert_eq!(free_slot(&[guest(1), guest(3)], 3), Some(2));
        assert_eq!(free_slot(&[guest(1), guest(2), guest(3)], 3), None);
    }

    #[test]
    fn a_smaller_lobby_fills_sooner() {
        // A two-player lobby seats one guest.
        assert_eq!(free_slot(&[], Lobby::seats(2)), Some(1));
        assert_eq!(free_slot(&[guest(1)], Lobby::seats(2)), None);
        assert_eq!(Lobby::seats(9), 3, "never more than the transport takes");
    }

    #[test]
    fn the_password_must_match_when_there_is_one() {
        assert!(admits("", ""));
        assert!(admits("", "anything"));
        assert!(admits("murloc", "murloc"));
        assert!(!admits("murloc", ""));
        assert!(!admits("murloc", "Murloc"));
    }
}
