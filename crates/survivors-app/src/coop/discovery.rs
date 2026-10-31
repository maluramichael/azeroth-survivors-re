//! LAN search: a guest asks the local network who hosts, hosts answer. Question and answer over
//! UDP broadcast on [`SEARCH_PORT`], only while someone searches; no background thread.
//!
//! Wire format (little endian), version [`VERSION`]:
//!
//! - query: `"WSRV?"`, version byte.
//! - answer: `"WSRV!"`, version byte, game port `u16`, class id `u8`, zone `u16`, players `u8`,
//!   max players `u8`, locked `u8`, build tag `u32`, lobby name and host name each as a length
//!   byte plus that many UTF-8 bytes (at most [`NAME_MAX`]).
//!
//! A host answers by unicast to whoever asked, so only a search puts traffic on the network.
//! The searcher asks `255.255.255.255`, the broadcast address of every IPv4 interface (macOS
//! sends the global broadcast out of the primary interface only) and `127.0.0.1` (two games on
//! one machine find each other). Every socket is non-blocking and read once a frame.

use std::collections::HashSet;
use std::io::{self, ErrorKind};
use std::net::{IpAddr, Ipv4Addr, SocketAddr, UdpSocket};

use bevy::prelude::*;

use super::{Link, Session};
use crate::{NetRole, Selection};

/// The port hosts listen on for the search (the game itself is on `net::DEFAULT_PORT`).
pub(super) const SEARCH_PORT: u16 = 37374;
/// Wire version; a packet of another version is ignored on both sides.
const VERSION: u8 = 1;
const QUERY_MAGIC: &[u8; 5] = b"WSRV?";
const ANSWER_MAGIC: &[u8; 5] = b"WSRV!";
/// Longest lobby or host name on the wire, in bytes.
const NAME_MAX: usize = 48;
/// Seconds between two questions while searching.
const ASK_EVERY: f64 = 1.0;
/// A host silent this long leaves the list.
const LIST_TTL: f64 = 3.0;
/// Seconds before a socket that failed to bind is tried again.
const REBIND_EVERY: f64 = 2.0;
/// Packets one socket reads per frame, so a flood cannot stall a frame.
const READ_PER_FRAME: usize = 64;

/// Tells two builds apart: the crate version, hashed. A host of another build is not listed,
/// because its game port would speak another protocol.
pub(super) const BUILD: u32 = fnv1a(env!("CARGO_PKG_VERSION").as_bytes());

/// 32-bit FNV-1a.
const fn fnv1a(bytes: &[u8]) -> u32 {
    let mut hash = 0x811c_9dc5_u32;
    let mut i = 0;
    while i < bytes.len() {
        hash ^= bytes[i] as u32;
        hash = hash.wrapping_mul(0x0100_0193);
        i += 1;
    }
    hash
}

pub(super) fn plugin(app: &mut App) {
    app.init_resource::<LanSearch>()
        .init_resource::<LanGames>()
        // Both run every frame and drop their socket when their condition ends; idle they cost
        // two early returns.
        .add_systems(Update, (answer_search, search_lan));
}

/// Set by the window while its LAN tab is open: this machine asks every second.
#[derive(Resource, Default)]
pub(super) struct LanSearch {
    pub active: bool,
}

/// The parties that answered lately, newest answer kept per host address. Sorted by lobby name,
/// then address, so the list does not jump between answers.
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

/// What a host tells the network about its party.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct Answer {
    /// The UDP port the game itself listens on.
    pub game_port: u16,
    pub lobby: String,
    pub host: String,
    pub class_id: u8,
    pub zone: u16,
    pub players: u8,
    pub max_players: u8,
    pub locked: bool,
    /// The host's [`BUILD`].
    pub build: u32,
}

/// The question a searching guest sends.
pub(super) fn encode_query() -> Vec<u8> {
    let mut out = QUERY_MAGIC.to_vec();
    out.push(VERSION);
    out
}

/// Whether `packet` is a question of this wire version.
pub(super) fn is_query(packet: &[u8]) -> bool {
    packet.len() == QUERY_MAGIC.len() + 1
        && packet.starts_with(QUERY_MAGIC)
        && packet[QUERY_MAGIC.len()] == VERSION
}

/// `text` cut to [`NAME_MAX`] bytes at a character boundary.
fn capped(text: &str) -> &str {
    if text.len() <= NAME_MAX {
        return text;
    }
    let mut end = NAME_MAX;
    while !text.is_char_boundary(end) {
        end -= 1;
    }
    &text[..end]
}

/// The answer as bytes; names beyond [`NAME_MAX`] bytes are cut.
pub(super) fn encode_answer(answer: &Answer) -> Vec<u8> {
    let mut out = ANSWER_MAGIC.to_vec();
    out.push(VERSION);
    out.extend_from_slice(&answer.game_port.to_le_bytes());
    out.push(answer.class_id);
    out.extend_from_slice(&answer.zone.to_le_bytes());
    out.push(answer.players);
    out.push(answer.max_players);
    out.push(u8::from(answer.locked));
    out.extend_from_slice(&answer.build.to_le_bytes());
    for text in [&answer.lobby, &answer.host] {
        let text = capped(text);
        out.push(text.len() as u8);
        out.extend_from_slice(text.as_bytes());
    }
    out
}

/// Reads bytes off the front of a packet; every read is `None` past the end.
struct Reader<'a>(&'a [u8]);

impl<'a> Reader<'a> {
    fn take(&mut self, n: usize) -> Option<&'a [u8]> {
        let (head, rest) = self.0.split_at_checked(n)?;
        self.0 = rest;
        Some(head)
    }

    fn u8(&mut self) -> Option<u8> {
        self.take(1).map(|b| b[0])
    }

    fn u16(&mut self) -> Option<u16> {
        self.take(2).map(|b| u16::from_le_bytes([b[0], b[1]]))
    }

    fn u32(&mut self) -> Option<u32> {
        self.take(4)
            .map(|b| u32::from_le_bytes([b[0], b[1], b[2], b[3]]))
    }

    /// A length-prefixed UTF-8 string of at most [`NAME_MAX`] bytes.
    fn text(&mut self) -> Option<String> {
        let len = usize::from(self.u8()?);
        if len > NAME_MAX {
            return None;
        }
        std::str::from_utf8(self.take(len)?).ok().map(str::to_owned)
    }
}

/// The answer in `packet`, or `None` for garbage, a truncated packet, trailing bytes or another
/// wire version.
pub(super) fn decode_answer(packet: &[u8]) -> Option<Answer> {
    let mut r = Reader(packet);
    if r.take(ANSWER_MAGIC.len())? != ANSWER_MAGIC || r.u8()? != VERSION {
        return None;
    }
    let answer = Answer {
        game_port: r.u16()?,
        class_id: r.u8()?,
        zone: r.u16()?,
        players: r.u8()?,
        max_players: r.u8()?,
        locked: r.u8()? != 0,
        build: r.u32()?,
        lobby: r.text()?,
        host: r.text()?,
    };
    r.0.is_empty().then_some(answer)
}

// ---- sockets -------------------------------------------------------------------------------

/// The host's listening socket on `port`, non-blocking. std cannot set `SO_REUSEADDR`, so a
/// second game on the same machine fails here with `AddrInUse`.
fn bind_host(port: u16) -> io::Result<UdpSocket> {
    let socket = UdpSocket::bind((Ipv4Addr::UNSPECIFIED, port))?;
    socket.set_nonblocking(true)?;
    Ok(socket)
}

/// Answers every pending question on `socket` with `answer`, by unicast to the asker; returns
/// how many it answered.
fn answer_queries(socket: &UdpSocket, answer: &Answer) -> io::Result<usize> {
    let reply = encode_answer(answer);
    let mut buf = [0u8; 64];
    let mut answered = 0;
    for _ in 0..READ_PER_FRAME {
        match socket.recv_from(&mut buf) {
            Ok((n, from)) => {
                if is_query(&buf[..n]) {
                    match socket.send_to(&reply, from) {
                        Ok(_) => answered += 1,
                        // One unreachable asker is not worth giving up the others.
                        Err(e) if e.kind() == ErrorKind::WouldBlock => {}
                        Err(e) if is_peer_gone(&e) => {}
                        Err(e) => return Err(e),
                    }
                }
            }
            Err(e) if e.kind() == ErrorKind::WouldBlock => break,
            Err(e) if is_peer_gone(&e) || e.kind() == ErrorKind::Interrupted => {}
            Err(e) => return Err(e),
        }
    }
    Ok(answered)
}

/// Errors about the other end of a datagram, not about this socket (Windows reports an earlier
/// send to a closed port on the next read).
fn is_peer_gone(e: &io::Error) -> bool {
    matches!(
        e.kind(),
        ErrorKind::ConnectionReset | ErrorKind::ConnectionRefused | ErrorKind::HostUnreachable
    )
}

/// The searcher's socket on an ephemeral port: non-blocking, allowed to broadcast.
fn bind_search() -> io::Result<UdpSocket> {
    let socket = UdpSocket::bind((Ipv4Addr::UNSPECIFIED, 0))?;
    socket.set_nonblocking(true)?;
    socket.set_broadcast(true)?;
    Ok(socket)
}

/// Sends the question to every target. A target that fails does not stop the others; the first
/// error, if any, is returned for logging.
fn ask(socket: &UdpSocket, targets: &[SocketAddr]) -> Option<io::Error> {
    let query = encode_query();
    let mut first = None;
    for target in targets {
        if let Err(e) = socket.send_to(&query, target) {
            first.get_or_insert(e);
        }
    }
    first
}

/// Where a question goes: the global broadcast, each interface's broadcast address, loopback.
fn ask_targets(broadcasts: &[Ipv4Addr], port: u16) -> Vec<SocketAddr> {
    let mut ips = vec![Ipv4Addr::BROADCAST, Ipv4Addr::LOCALHOST];
    for ip in broadcasts {
        if !ips.contains(ip) {
            ips.push(*ip);
        }
    }
    ips.into_iter()
        .map(|ip| SocketAddr::new(ip.into(), port))
        .collect()
}

/// This machine's own addresses and the broadcast addresses of its up IPv4 interfaces.
fn interfaces() -> io::Result<(Vec<IpAddr>, Vec<Ipv4Addr>)> {
    let mut locals = Vec::new();
    let mut broadcasts = Vec::new();
    for iface in if_addrs::get_if_addrs()? {
        locals.push(iface.ip());
        if let if_addrs::IfAddr::V4(v4) = &iface.addr {
            if let Some(broadcast) = v4
                .broadcast
                .filter(|_| !v4.is_loopback() && iface.is_oper_up())
            {
                broadcasts.push(broadcast);
            }
        }
    }
    Ok((locals, broadcasts))
}

/// Reads every pending answer on `socket` into `games`. An answer from one of this machine's own
/// addresses (`locals`) is listed under loopback, so a host on this machine appears once however
/// the question reached it. Other builds and garbage are skipped.
fn collect_answers(
    socket: &UdpSocket,
    now: f64,
    locals: &[IpAddr],
    games: &mut Vec<LanGame>,
) -> io::Result<()> {
    let mut buf = [0u8; 512];
    for _ in 0..READ_PER_FRAME {
        let (n, from) = match socket.recv_from(&mut buf) {
            Ok(got) => got,
            Err(e) if e.kind() == ErrorKind::WouldBlock => break,
            Err(e) if is_peer_gone(&e) || e.kind() == ErrorKind::Interrupted => continue,
            Err(e) => return Err(e),
        };
        let Some(answer) = decode_answer(&buf[..n]) else {
            continue;
        };
        if answer.build != BUILD || answer.game_port == 0 {
            continue;
        }
        let ip = if locals.contains(&from.ip()) {
            IpAddr::V4(Ipv4Addr::LOCALHOST)
        } else {
            from.ip()
        };
        let game = LanGame {
            addr: SocketAddr::new(ip, answer.game_port),
            lobby: answer.lobby,
            host: answer.host,
            class_id: answer.class_id,
            zone: usize::from(answer.zone),
            players: answer.players,
            max_players: answer.max_players,
            locked: answer.locked,
            seen: now,
        };
        match games.iter_mut().find(|g| g.addr == game.addr) {
            Some(known) => *known = game,
            None => games.push(game),
        }
    }
    Ok(())
}

/// Drops the hosts silent for [`LIST_TTL`] and sorts the rest by lobby name, then address.
fn tidy(games: &mut Vec<LanGame>, now: f64) {
    games.retain(|g| now - g.seen <= LIST_TTL);
    games.sort_by(|a, b| {
        a.lobby
            .to_lowercase()
            .cmp(&b.lobby.to_lowercase())
            .then_with(|| a.addr.cmp(&b.addr))
    });
}

/// Logs each kind of socket error once, so a broken network does not fill the log at 60 Hz.
#[derive(Default)]
struct Quiet(HashSet<(&'static str, ErrorKind)>);

impl Quiet {
    fn warn(&mut self, what: &'static str, error: &io::Error) {
        if self.0.insert((what, error.kind())) {
            warn!("survivors: co-op: LAN search: {what}: {error}");
        }
    }
}

// ---- systems -------------------------------------------------------------------------------

/// The host's state between frames.
#[derive(Default)]
struct HostSide {
    socket: Option<UdpSocket>,
    /// `Time<Real>` seconds before which a failed bind is not tried again.
    retry_at: f64,
    quiet: Quiet,
}

/// Host side: while hosting a public party, answer every question that reached
/// [`SEARCH_PORT`] with the party's current data. Drops the socket when hosting stops or the
/// party turns private.
fn answer_search(
    role: Res<NetRole>,
    session: Res<Session>,
    selection: Res<Selection>,
    time: Res<Time<Real>>,
    mut side: Local<HostSide>,
) {
    let side = &mut *side;
    let game_port = match session.link {
        Link::Hosting { port } if *role == NetRole::Host && session.lobby.public => port,
        _ => {
            if side.socket.take().is_some() {
                info!("survivors: co-op: LAN search: no longer answering");
            }
            return;
        }
    };
    let now = time.elapsed_secs_f64();
    if side.socket.is_none() {
        if now < side.retry_at {
            return;
        }
        match bind_host(SEARCH_PORT) {
            Ok(socket) => side.socket = Some(socket),
            Err(e) => {
                // Usually another game on this machine already answers; this one then stays
                // out of the list, and joining by address works as before.
                side.quiet
                    .warn("cannot listen, this party stays out of the LAN list", &e);
                side.retry_at = now + REBIND_EVERY;
                return;
            }
        }
    }
    let Some(socket) = &side.socket else { return };
    let class = selection.class_def();
    let name = session.name.trim();
    let host = if !name.is_empty() {
        name.to_string()
    } else {
        class.map_or_else(|| "Host".to_string(), |c| c.name.to_string())
    };
    let lobby = session.lobby.name.trim();
    let lobby = if lobby.is_empty() {
        format!("{host}'s party")
    } else {
        lobby.to_string()
    };
    let answer = Answer {
        game_port,
        lobby,
        host,
        class_id: class.map_or(0, |c| c.id),
        zone: u16::try_from(selection.stage).unwrap_or(u16::MAX),
        players: u8::try_from(1 + session.guests.len()).unwrap_or(u8::MAX),
        max_players: session.lobby.max_players,
        locked: !session.lobby.password.is_empty(),
        build: BUILD,
    };
    if let Err(e) = answer_queries(socket, &answer) {
        side.quiet.warn("cannot answer", &e);
    }
}

/// The searcher's state between frames.
#[derive(Default)]
struct SearchSide {
    socket: Option<UdpSocket>,
    retry_at: f64,
    /// `Time<Real>` seconds of the next question.
    next_ask: f64,
    /// This machine's own addresses, refreshed with every question.
    locals: Vec<IpAddr>,
    quiet: Quiet,
}

/// Search side: while [`LanSearch::active`], ask every second and keep [`LanGames`] up to date
/// from the answers. Drops the socket and the list when the search ends.
fn search_lan(
    search: Res<LanSearch>,
    time: Res<Time<Real>>,
    mut games: ResMut<LanGames>,
    mut side: Local<SearchSide>,
) {
    let side = &mut *side;
    if !search.active {
        side.socket = None;
        side.next_ask = 0.0;
        side.retry_at = 0.0;
        if !games.games.is_empty() {
            games.games.clear();
        }
        return;
    }
    let now = time.elapsed_secs_f64();
    if side.socket.is_none() {
        if now < side.retry_at {
            return;
        }
        match bind_search() {
            Ok(socket) => side.socket = Some(socket),
            Err(e) => {
                side.quiet.warn("cannot open the search socket", &e);
                side.retry_at = now + REBIND_EVERY;
                return;
            }
        }
    }
    let Some(socket) = &side.socket else { return };
    if now >= side.next_ask {
        side.next_ask = now + ASK_EVERY;
        let broadcasts = match interfaces() {
            Ok((locals, broadcasts)) => {
                side.locals = locals;
                broadcasts
            }
            Err(e) => {
                side.quiet.warn("cannot list the network interfaces", &e);
                Vec::new()
            }
        };
        if let Some(e) = ask(socket, &ask_targets(&broadcasts, SEARCH_PORT)) {
            side.quiet.warn("cannot send the question", &e);
        }
    }
    let mut found = games.games.clone();
    if let Err(e) = collect_answers(socket, now, &side.locals, &mut found) {
        side.quiet.warn("cannot read answers", &e);
    }
    tidy(&mut found, now);
    // Assign only a changed list, so the window's change detection is not tripped every frame.
    if found != games.games {
        games.games = found;
    }
}

#[cfg(test)]
mod tests {
    use std::thread::sleep;
    use std::time::Duration;

    use super::*;

    fn sample() -> Answer {
        Answer {
            game_port: 37373,
            lobby: "Friday raid".into(),
            host: "Thrall".into(),
            class_id: 7,
            zone: 3,
            players: 2,
            max_players: 4,
            locked: true,
            build: 0xDEAD_BEEF,
        }
    }

    #[test]
    fn the_query_is_recognised_and_nothing_else() {
        let query = encode_query();
        assert_eq!(query, b"WSRV?\x01");
        assert!(is_query(&query));
        assert!(!is_query(b"WSRV?\x02"), "another version");
        assert!(!is_query(b"WSRV?"), "no version byte");
        assert!(!is_query(b"WSRV?\x01\x00"), "trailing byte");
        assert!(!is_query(&encode_answer(&sample())));
        assert!(!is_query(&[]));
    }

    #[test]
    fn an_answer_round_trips_with_exact_bytes() {
        let bytes = encode_answer(&sample());
        let mut want = b"WSRV!\x01".to_vec();
        want.extend_from_slice(&37373u16.to_le_bytes());
        want.extend_from_slice(&[7]);
        want.extend_from_slice(&3u16.to_le_bytes());
        want.extend_from_slice(&[2, 4, 1]);
        want.extend_from_slice(&0xDEAD_BEEFu32.to_le_bytes());
        want.push(11);
        want.extend_from_slice(b"Friday raid");
        want.push(6);
        want.extend_from_slice(b"Thrall");
        assert_eq!(bytes, want);
        assert_eq!(decode_answer(&bytes), Some(sample()));
    }

    #[test]
    fn unlocked_and_empty_names_round_trip() {
        let answer = Answer {
            locked: false,
            lobby: String::new(),
            host: String::new(),
            ..sample()
        };
        assert_eq!(decode_answer(&encode_answer(&answer)), Some(answer));
    }

    #[test]
    fn a_truncated_packet_decodes_to_none() {
        let bytes = encode_answer(&sample());
        for cut in 0..bytes.len() {
            assert_eq!(decode_answer(&bytes[..cut]), None, "cut at {cut}");
        }
    }

    #[test]
    fn another_version_or_garbage_decodes_to_none() {
        let mut bytes = encode_answer(&sample());
        bytes[5] = VERSION + 1;
        assert_eq!(decode_answer(&bytes), None);
        let mut bytes = encode_answer(&sample());
        bytes[0] = b'X';
        assert_eq!(decode_answer(&bytes), None);
        assert_eq!(decode_answer(b"hello world, this is not an answer"), None);
        assert_eq!(decode_answer(&encode_query()), None);
        let mut bytes = encode_answer(&sample());
        bytes.push(0);
        assert_eq!(decode_answer(&bytes), None, "trailing byte");
    }

    #[test]
    fn invalid_utf8_decodes_to_none() {
        let mut bytes = encode_answer(&Answer {
            lobby: "abc".into(),
            host: String::new(),
            ..sample()
        });
        let at = bytes.len() - 5;
        assert_eq!(bytes[at], 3);
        bytes[at + 1] = 0xFF;
        assert_eq!(decode_answer(&bytes), None);
    }

    #[test]
    fn a_name_longer_than_the_cap_on_the_wire_is_refused() {
        let mut bytes = encode_answer(&Answer {
            lobby: String::new(),
            host: String::new(),
            ..sample()
        });
        let at = bytes.len() - 2;
        bytes[at] = (NAME_MAX + 1) as u8;
        bytes.splice(at + 1..at + 1, vec![b'a'; NAME_MAX + 1]);
        assert_eq!(decode_answer(&bytes), None);
    }

    #[test]
    fn oversize_names_are_capped_on_a_character_boundary() {
        let answer = Answer {
            lobby: "x".repeat(200),
            // 47 ASCII bytes, then a two-byte character that straddles the cap.
            host: format!("{}é", "h".repeat(NAME_MAX - 1)),
            ..sample()
        };
        let back = decode_answer(&encode_answer(&answer)).expect("a capped answer decodes");
        assert_eq!(back.lobby, "x".repeat(NAME_MAX));
        assert_eq!(back.host, "h".repeat(NAME_MAX - 1));
        assert_eq!(back.game_port, answer.game_port);
    }

    #[test]
    fn an_answer_stays_small() {
        let answer = Answer {
            lobby: "l".repeat(100),
            host: "h".repeat(100),
            ..sample()
        };
        assert!(encode_answer(&answer).len() <= 128);
    }

    fn game(addr: &str, lobby: &str, seen: f64) -> LanGame {
        LanGame {
            addr: addr.parse().unwrap(),
            lobby: lobby.into(),
            host: "h".into(),
            class_id: 1,
            zone: 0,
            players: 1,
            max_players: 4,
            locked: false,
            seen,
        }
    }

    #[test]
    fn the_list_drops_the_silent_and_sorts_stably() {
        let mut games = vec![
            game("10.0.0.9:37373", "beta", 9.0),
            game("10.0.0.2:37373", "Alpha", 9.0),
            game("10.0.0.1:37373", "beta", 9.5),
            game("10.0.0.7:37373", "gone", 6.0),
        ];
        tidy(&mut games, 10.0);
        let order: Vec<_> = games.iter().map(|g| g.addr.to_string()).collect();
        assert_eq!(
            order,
            ["10.0.0.2:37373", "10.0.0.1:37373", "10.0.0.9:37373"]
        );
    }

    #[test]
    fn the_targets_hold_the_global_broadcast_loopback_and_each_interface_once() {
        let nets = [
            Ipv4Addr::new(192, 168, 1, 255),
            Ipv4Addr::new(192, 168, 1, 255),
            Ipv4Addr::BROADCAST,
        ];
        let targets = ask_targets(&nets, 5);
        let want: Vec<SocketAddr> = ["255.255.255.255:5", "127.0.0.1:5", "192.168.1.255:5"]
            .iter()
            .map(|s| s.parse().unwrap())
            .collect();
        assert_eq!(targets, want);
    }

    /// Reads until `games` has an entry or about a second has passed.
    fn wait_for_answer(searcher: &UdpSocket, locals: &[IpAddr], games: &mut Vec<LanGame>) {
        for _ in 0..100 {
            collect_answers(searcher, 1.0, locals, games).unwrap();
            if !games.is_empty() {
                return;
            }
            sleep(Duration::from_millis(10));
        }
    }

    #[test]
    fn a_searcher_gets_an_answer_from_a_host_over_loopback() {
        let host = bind_host(0).unwrap();
        let port = host.local_addr().unwrap().port();
        let searcher = bind_search().unwrap();
        let target = SocketAddr::from((Ipv4Addr::LOCALHOST, port));
        assert!(ask(&searcher, &[target]).is_none());

        let mut answered = 0;
        let mut games = Vec::new();
        for _ in 0..100 {
            answered += answer_queries(&host, &sample_with_build()).unwrap();
            collect_answers(&searcher, 1.0, &[], &mut games).unwrap();
            if !games.is_empty() {
                break;
            }
            sleep(Duration::from_millis(10));
        }
        assert_eq!(answered, 1);
        assert_eq!(games.len(), 1);
        let found = &games[0];
        assert_eq!(found.addr, SocketAddr::from((Ipv4Addr::LOCALHOST, 37373)));
        assert_eq!(found.lobby, "Friday raid");
        assert_eq!(found.host, "Thrall");
        assert_eq!((found.class_id, found.zone), (7, 3));
        assert_eq!(
            (found.players, found.max_players, found.locked),
            (2, 4, true)
        );
        assert_eq!(found.seen, 1.0);

        // The same host again updates its entry instead of adding one.
        assert!(ask(&searcher, &[target]).is_none());
        let mut again = games.clone();
        for _ in 0..100 {
            answer_queries(&host, &sample_with_build()).unwrap();
            collect_answers(&searcher, 2.0, &[], &mut again).unwrap();
            if again[0].seen == 2.0 {
                break;
            }
            sleep(Duration::from_millis(10));
        }
        assert_eq!(again.len(), 1);
        assert_eq!(again[0].seen, 2.0);
    }

    fn sample_with_build() -> Answer {
        Answer {
            build: BUILD,
            ..sample()
        }
    }

    #[test]
    fn an_answer_from_this_machine_is_listed_under_loopback() {
        let host = bind_host(0).unwrap();
        let port = host.local_addr().unwrap().port();
        let searcher = bind_search().unwrap();
        // Pretend the loopback address is one of this machine's interface addresses.
        let locals = [IpAddr::V4(Ipv4Addr::LOCALHOST)];
        ask(&searcher, &[SocketAddr::from((Ipv4Addr::LOCALHOST, port))]);
        sleep(Duration::from_millis(20));
        answer_queries(&host, &sample_with_build()).unwrap();
        let mut games = Vec::new();
        wait_for_answer(&searcher, &locals, &mut games);
        assert_eq!(games.len(), 1);
        assert!(games[0].addr.ip().is_loopback());
    }

    #[test]
    fn answers_of_another_build_and_noise_are_skipped() {
        let searcher = bind_search().unwrap();
        let to = SocketAddr::from((Ipv4Addr::LOCALHOST, searcher.local_addr().unwrap().port()));
        let sender = UdpSocket::bind((Ipv4Addr::LOCALHOST, 0)).unwrap();
        let other = Answer {
            build: BUILD ^ 1,
            ..sample()
        };
        sender.send_to(&encode_answer(&other), to).unwrap();
        sender.send_to(b"noise", to).unwrap();
        sender
            .send_to(&encode_answer(&sample_with_build()), to)
            .unwrap();
        let mut games = Vec::new();
        wait_for_answer(&searcher, &[], &mut games);
        assert_eq!(games.len(), 1);
        assert_eq!(games[0].lobby, "Friday raid");
    }

    #[test]
    fn a_host_ignores_packets_that_are_not_questions() {
        let host = bind_host(0).unwrap();
        let port = host.local_addr().unwrap().port();
        let other = UdpSocket::bind((Ipv4Addr::LOCALHOST, 0)).unwrap();
        other.set_nonblocking(true).unwrap();
        other
            .send_to(b"WSRV?\x09", (Ipv4Addr::LOCALHOST, port))
            .unwrap();
        sleep(Duration::from_millis(30));
        assert_eq!(answer_queries(&host, &sample()).unwrap(), 0);
        sleep(Duration::from_millis(30));
        assert!(other.recv_from(&mut [0u8; 8]).is_err());
    }

    #[test]
    fn the_build_tag_follows_the_crate_version() {
        assert_eq!(BUILD, fnv1a(env!("CARGO_PKG_VERSION").as_bytes()));
        assert_ne!(fnv1a(b"0.1.0"), fnv1a(b"0.1.1"));
    }
}
