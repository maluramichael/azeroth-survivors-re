//! The transport: renet2's netcode over UDP. A host binds a port on every interface; a client
//! binds an ephemeral one and connects to the host's address. Friends-only, so the netcode
//! handshake is unsecured: anyone who can reach the port can join.
//!
//! Taken from deanmathieson/azeroth-survivors-re (branch `progression`, `coop/net.rs`,
//! MIT OR Apache-2.0), with this fork's port, protocol id and party size.

use std::net::{IpAddr, Ipv4Addr, Ipv6Addr, SocketAddr, ToSocketAddrs, UdpSocket};

use bevy_replicon::prelude::RepliconChannels;
use bevy_replicon_renet2::netcode::{
    ClientAuthentication, NativeSocket, NetcodeClientTransport, NetcodeServerTransport,
    ServerAuthentication, ServerSetupConfig,
};
use bevy_replicon_renet2::renet2::{ConnectionConfig, RenetClient, RenetServer};
use bevy_replicon_renet2::RenetChannelsExt;

/// The port a host listens on unless told otherwise. Not 5000: macOS's AirPlay receiver sits on
/// TCP 5000, which only confuses a search for what is listening.
pub(super) const DEFAULT_PORT: u16 = 37373;
/// "WSURV1" and the wire version: a build speaking another protocol is turned away at the
/// handshake. Version 2: telegraphs, missiles, chest lids and map markers replicate, and
/// `ObjLook` and `NetEnemy` carry more. Version 3: `NetRun` carries the run's place on its route.
const PROTOCOL_ID: u64 = 0x5753_5552_5631_0003;
/// Clients one host takes: the party's other three.
pub(super) const MAX_CLIENTS: usize = 3;

fn now() -> std::time::Duration {
    web_time::SystemTime::now()
        .duration_since(web_time::UNIX_EPOCH)
        .unwrap_or_default()
}

fn other(e: impl std::fmt::Display) -> std::io::Error {
    std::io::Error::other(e.to_string())
}

fn config(channels: &RepliconChannels) -> ConnectionConfig {
    ConnectionConfig::from_channels(channels.server_configs(), channels.client_configs())
}

/// A server on `port` (0 picks a free one) on every IPv4 interface, and the port it got.
pub(super) fn host(
    channels: &RepliconChannels,
    port: u16,
) -> std::io::Result<(RenetServer, NetcodeServerTransport, u16)> {
    let socket = UdpSocket::bind(SocketAddr::new(Ipv4Addr::UNSPECIFIED.into(), port))?;
    let bound = socket.local_addr()?;
    let setup = ServerSetupConfig {
        current_time: now(),
        // One more than the seats: a guest over the limit is let in long enough to be told the
        // party is full, instead of never hearing back.
        max_clients: MAX_CLIENTS + 1,
        protocol_id: PROTOCOL_ID,
        // Unsecured: the address list is not checked, so the bound one serves.
        socket_addresses: vec![vec![bound]],
        authentication: ServerAuthentication::Unsecure,
    };
    let socket = NativeSocket::new(socket).map_err(other)?;
    let transport = NetcodeServerTransport::new(setup, socket)?;
    Ok((RenetServer::new(config(channels)), transport, bound.port()))
}

/// A client connecting to `server`, under a random client id.
pub(super) fn join(
    channels: &RepliconChannels,
    server: SocketAddr,
) -> std::io::Result<(RenetClient, NetcodeClientTransport)> {
    let any: IpAddr = match server {
        SocketAddr::V4(_) => Ipv4Addr::UNSPECIFIED.into(),
        SocketAddr::V6(_) => Ipv6Addr::UNSPECIFIED.into(),
    };
    let socket = UdpSocket::bind(SocketAddr::new(any, 0))?;
    let time = now();
    let auth = ClientAuthentication::Unsecure {
        client_id: (time.as_nanos() as u64) ^ (u64::from(std::process::id()) << 32),
        protocol_id: PROTOCOL_ID,
        socket_id: 0,
        server_addr: server,
        user_data: None,
    };
    let socket = NativeSocket::new(socket).map_err(other)?;
    let transport = NetcodeClientTransport::new(time, auth, socket).map_err(other)?;
    Ok((RenetClient::new(config(channels), false), transport))
}

/// The address a friend types, `host:port` or a bare host on [`DEFAULT_PORT`]: an IPv4 or IPv6
/// address (`[::1]:37373`) or a name the resolver knows.
pub(super) fn parse_addr(text: &str) -> Result<SocketAddr, String> {
    let text = text.trim();
    if text.is_empty() {
        return Err("Type the host's address first".into());
    }
    if let Ok(addr) = text.parse::<SocketAddr>() {
        return Ok(addr);
    }
    if let Ok(ip) = text.trim_matches(['[', ']']).parse::<IpAddr>() {
        return Ok(SocketAddr::new(ip, DEFAULT_PORT));
    }
    let with_port = if text
        .rsplit_once(':')
        .is_some_and(|(_, p)| p.parse::<u16>().is_ok())
    {
        text.to_string()
    } else {
        format!("{text}:{DEFAULT_PORT}")
    };
    with_port
        .to_socket_addrs()
        .ok()
        .and_then(|mut addrs| {
            let all: Vec<SocketAddr> = addrs.by_ref().collect();
            all.iter().find(|a| a.is_ipv4()).or(all.first()).copied()
        })
        .ok_or_else(|| format!("Cannot find the host {text:?}"))
}

/// A port as typed; empty is [`DEFAULT_PORT`].
pub(super) fn parse_port(text: &str) -> Result<u16, String> {
    let text = text.trim();
    if text.is_empty() {
        return Ok(DEFAULT_PORT);
    }
    text.parse::<u16>()
        .ok()
        .filter(|&p| p > 0)
        .ok_or_else(|| format!("{text:?} is not a port (1 to 65535)"))
}

/// This machine's address on its local network: the interface the route to the internet leaves
/// by. Connecting a UDP socket sends nothing.
pub(super) fn lan_ip() -> Option<IpAddr> {
    let socket = UdpSocket::bind((Ipv4Addr::UNSPECIFIED, 0)).ok()?;
    socket.connect((Ipv4Addr::new(8, 8, 8, 8), 80)).ok()?;
    let ip = socket.local_addr().ok()?.ip();
    (!ip.is_unspecified()).then_some(ip)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn addresses_parse_with_or_without_a_port() {
        assert_eq!(
            parse_addr("192.168.1.20:6000"),
            Ok("192.168.1.20:6000".parse().unwrap())
        );
        assert_eq!(
            parse_addr(" 100.64.0.7 "),
            Ok(SocketAddr::new("100.64.0.7".parse().unwrap(), DEFAULT_PORT))
        );
        assert_eq!(parse_addr("[::1]:5001"), Ok("[::1]:5001".parse().unwrap()));
        assert_eq!(
            parse_addr("::1"),
            Ok(SocketAddr::new("::1".parse().unwrap(), DEFAULT_PORT))
        );
        assert_eq!(
            parse_addr("localhost:5002").map(|a| a.port()),
            Ok(5002),
            "names resolve"
        );
        assert!(parse_addr("").is_err());
        assert_eq!(parse_port(""), Ok(DEFAULT_PORT));
        assert_eq!(parse_port("7777"), Ok(7777));
        assert!(parse_port("0").is_err() && parse_port("70000").is_err());
    }

    #[test]
    fn bad_addresses_are_refused() {
        assert!(parse_addr("").is_err());
        assert!(parse_addr("not a host name !!").is_err());
        assert_eq!(parse_addr("10.0.0.5").unwrap().port(), DEFAULT_PORT);
    }

    #[test]
    fn a_taken_port_is_an_error() {
        let channels = RepliconChannels::default();
        let (_server, _transport, port) = host(&channels, 0).unwrap();
        assert!(host(&channels, port).is_err());
    }
}
