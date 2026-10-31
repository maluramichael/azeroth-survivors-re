# Koop per Direct IP und LAN: Entwurf

Stand: 2026-10-31. Branch `features`, Bevy 0.18.1, Modus in `crates/survivors-app`.
Recherche und Entwurf, noch kein Code.

## Ziel

Ein Spieler hostet einen Lauf, bis zu drei weitere treten bei (höchstens 4 Spieler). Beitreten
geht über eine eingetippte Adresse (`192.168.1.20`, `100.64.0.7:37373`, `[::1]`) oder über eine
Liste der Hosts, die im LAN gefunden wurden. Der Host-Rechner ist zugleich Server und Spieler
(Listen-Server). Die Desktop-Version (macOS, Windows, Linux) hat Vorrang.

## Nicht-Ziele

- Kein zentraler Server: keine Lobby-Server, kein Relay, kein Signalling-Server, kein STUN/TURN.
- Kein Matchmaking, keine öffentliche Lobby-Liste, keine Accounts.
- Kein Anti-Cheat. Es ist ein Spiel unter Freunden; der Host vertraut den Gästen innerhalb
  von Grenzen (Positionen in der Arena, Werte gekappt).
- Kein Koop im Browser in dieser Runde. Was das später bedeutet, steht unter "Browser".
- Kein NAT-Traversal und kein UPnP in dieser Runde; nur dokumentiert.
- Kein Host-Wechsel, wenn der Host geht.
- Kein Couch-Koop (mehrere Spieler an einem Rechner).

## Was die Forks gebaut haben

Gesichtet mit `search.py fork-issues OutThisLife/azeroth-survivors-re --json --readme --commits 30`
(249 Forks, 30 aktiv), `search.py issues <fork>`, `gh api` und flachen Klonen nach `/tmp/coop-*`.
Alle fünf Forks stehen wie wir unter `MIT OR Apache-2.0` (Workspace-`license`, `LICENSE-MIT` und
`LICENSE-APACHE` im Repo). Code daraus darf also übernommen werden; die Herkunft gehört in die
Commit-Nachricht und in einen Kopfkommentar der übernommenen Datei.

Wichtig für die Übernahme: Alle Forks haben den Modus noch unter
`crates/benilla-app/src/survivors/`, wir seit heute unter `crates/survivors-app/src/`, und alle sind
weit auseinandergelaufen (FlorianFortier 211, deanmathieson 131 Commits vor upstream). Mergen geht
nicht; übernehmbar sind einzelne Dateien und Muster.

| Fork | Transport | Crates | Autorität | Repliziert | Level-ups | Entdeckung | Umfang | Übernehmbar |
|---|---|---|---|---|---|---|---|---|
| **deanmathieson** (Branch `progression`) | UDP direkt, renet2-Netcode, unverschlüsselt, Port 5000 | `bevy_replicon` 0.40.4, `bevy_replicon_renet2` 0.17.0 (Feature `coop`, nicht für wasm) | Host simuliert alles; jeder Gast besitzt die Bewegung seines Helden (30 Hz, unreliable, vom Host geglaubt) | `Look` (Display, Skala, Waffen) einmal, `NetPos` quantisiert (cm, 1/256 Umdrehung), `NetHero`, `NetVitals`, `NetBuild`, `NetEnemy`, `NetLife`, `NetRun`; Effekte als Nachricht `NetFx` (Cast, Treffer, Schwung, Zahl) | Gemeinsame XP-Leiste; im Koop keine Pause: jede Stufe legt jedem Spieler eine Hand zurück, Tab öffnet sie als Leiste, Spiel läuft weiter; 50 % weniger Schaden während man wählt; Zeitlupe 40 %, wenn alle wählen | keine; Host-Panel zeigt die eigene LAN-IP (Trick: UDP-Socket nach 8.8.8.8 verbinden) | Groundwork +1900, Party-Regeln +1076, Online +4217 Zeilen (14. und 15. Oktober); `coop/` heute ca. 5200 Zeilen inkl. 1100 Test | **Am meisten.** `coop/net.rs` (160 Zeilen: Host, Join, Adress-Parser, LAN-IP) fast wörtlich; `mod.rs`-Aufbau (Session, Link, NetRole, Protokoll-Registrierung, Build-Tag-Abgleich); Wire-Typen als Vorlage |
| **FlorianFortier** | WebSocket über einen eigenen Lobby-Hub (`survivors-server`, axum), Desktop über `tungstenite`-Thread, Browser über die `WebSocket` der Seite | `survivors-net` (Protokoll, Codec ohne Abhängigkeiten, Hub-Zustandsautomat), `survivors-server`; Spielseite `survivors/net/` | Host simuliert; der Hub ist ein Relay mit Lobby-Codes. Gäste steuern ihren Helden lokal | Einheiten wie ein WoW-Server: Spawn mit Update-Feldern, Feldänderungen, Bewegung, Despawn, jeder Cast und jede Zahl, 20 Hz in einem Frame pro Tick | Spiel pausiert nie; Host teilt jedem Helden Karten zu und prüft Wahl, Reroll, Banish, Skip (PR #29) | Lobby-Code oder öffentliche Liste auf dem Hub | ca. 9600 Zeilen (Netz-Crates und `net/`) | Ideen und Tests, nicht der Transport (braucht den Hub, also einen Server). Gut: `smooth.rs` (Snapshot-Interpolation mit Absenderuhr, 185 Zeilen, PR #36), die Desync-Tests über eine schlechte Leitung (PR #50), Rejoin per Token (PR #51), Loadout eines Gasts (PR #20) |
| **GladstoneOG** | Eigenes UDP+TCP auf Port 25565, `std::thread`, JSON (`serde_json`) | keine neuen; `survivors/net.rs` (850) und `sync.rs` (1405) | Listen-Server oder Headless-Server (`--server`); Host simuliert, Gäste schicken Eingaben (60 Hz) | Kompletter `WorldSnapshot` aller Spieler, Gegner, Gems, Flächen, Totems und Effekte als ein JSON-Datagramm, 30 Hz | Gemeinsame XP; Level-up pausiert alle, jeder wählt aus eigenen Karten | keine; Direct IP für LAN, ZeroTier, Tailscale | ca. 2300 Zeilen | Wenig. Ein JSON-Snapshot mit 200 Gegnern ist weit über 1500 Byte, also IP-Fragmente; geht eines verloren, fehlt der ganze Snapshot. Effekte im unzuverlässigen Snapshot gehen mit verloren |
| **2boxtrickshot-droid** (PR #1, offen) | WebSocket zu einem Node-Router (`ws` 8.22) auf dem Host-PC, Port 8787 | `azeroth-coop`: eigene, vereinfachte Simulation mit serde-Snapshots, als wasm | Host-Browser simuliert in einem Worker; Gäste rendern nur | Snapshots der eigenen Simulation; die Bevy-Welt zeichnet sie nur | Spiel läuft weiter, Karten per Klick | keine; Einladungslink mit Capability im URL-Fragment, Portweiterleitung | ca. 1200 Zeilen Rust, ca. 380 Zeilen Adapter, JS | Nichts für uns: eigene Regeln statt des Solo-Modus, nur Browser. Lesenswert ist `docs/COOP.md` zu NAT, CGNAT und TLS ohne laufende Kosten. Nicht bevy_replicon/renet2, wie im Auftrag vermutet: das war deanmathieson |
| **N3rdmade** (Branch `multiplayer`) | TCP 45454 | `survivors-coop`: Konsolen-Lobby (191 Zeilen) | keine | nichts; nur Plätze, Ready, Start | keine | Direct IP | 191 Zeilen | Nichts; es startet kein Spiel |

Bekannte Probleme aus den Forks:

- **Desync durch Spawn und erste Bewegung im selben Frame** (FlorianFortier #50): Der Gast legte die
  Kopie über einen deferred Command an und suchte sie im selben Frame in einer Query; die ersten
  Bewegungen gingen verloren, eine danach stehende Einheit blieb für immer falsch. bevy_replicon
  wendet Spawn, Insert und Mutation in der richtigen Reihenfolge an; diese Fehlerklasse fällt weg.
- **Ruckelnde Mitspieler** (FlorianFortier #36): 20-Hz-Pakete ohne Glättung und zwei
  unsynchronisierte Uhren. Lösung: Position mit Absenderzeit, 100 ms hinter dem Absender
  zwischen zwei echten Positionen interpolieren, Sprünge (Blink, Charge) sofort zeigen.
- **Bandbreite**: FlorianFortier misst 100 bis 200 KB/s pro Gast spät im Lauf (volle Horde, 20 Hz).
  Volle Inbox: zuerst Effekte und Bewegungen verwerfen, nie Spawn, Felder oder Despawn.
- **Verbindungsabbruch**: deanmathieson schickt bei Host-Abbruch alle ins Menü und hat kein
  Rejoin; FlorianFortier hält den Platz als "Away" mit Token und schickt dem Rückkehrer einen
  vollen Snapshot (bei replicon macht das ein neuer Client von selbst).
- **Gleicher Build**: deanmathieson vergleicht beim Handshake Protokoll, Version und Commit und
  lehnt mit Klartext ab. Ohne das sieht man nur Kaputtes.
- **Zwei Instanzen auf einem Rechner** teilen sich das Profil (deanmathieson). Bei uns trennt
  `BENILLA_HOME` das (`benilla-app/src/local_state.rs`).

## Netzwerk-Optionen für Bevy 0.18

Geprüft am 2026-10-31 über die crates.io-API (`/api/v1/crates/<name>/<version>/dependencies`).
Die neuesten Versionen verlangen inzwischen Bevy 0.20; für uns zählt die neueste Version, die
Bevy 0.18 verlangt.

| Crate | Neueste für Bevy 0.18 (veröffentlicht) | Hängt ab von | Neueste insgesamt (verlangt) |
|---|---|---|---|
| `bevy_replicon` | 0.40.4 (2026-06-16) | `bevy ^0.18` | 0.45.5 (Bevy 0.20) |
| `bevy_replicon_renet2` | 0.17.0 (2026-05-24) | `bevy_replicon ^0.40`, `bevy_renet2 ^0.15.0` | 1.0.0 (Bevy 0.20) |
| `renet2` | 0.15.0 (2026-05-24) | `bevy_ecs ^0.18` (optional) | 1.0.0 |
| `bevy_replicon_renet` | 0.16.0 (2026-05-17) | `bevy_replicon ^0.40`, `bevy_renet ^4.0` | 0.21.0 (Bevy 0.20) |
| `bevy_renet` | 4.0.1 (2026-03-04) | `renet ^2.0.0` | 6.0.0 (Bevy 0.20) |
| `bevy_replicon_quinnet` | 0.19.0 (2026-05-23) | `bevy_replicon ^0.40` | 0.21.0 (Bevy 0.20) |
| `bevy_quinnet` | 0.20.0 (2026-01-17) | `quinn ^0.11.5` | 0.22.0 (Bevy 0.20) |
| `lightyear` | 0.27.0 (2026-06-22) | `bevy_ecs ^0.18` | 0.31.0 (Bevy 0.20) |
| `aeronet` / `aeronet_replicon` | 0.20.0 (2026-04-02) | `bevy_replicon ^0.39.2` (nicht 0.40) | 0.22.0 (Bevy 0.20) |
| `bevy_matchbox` | 0.14.0 (2026-02-13) | `bevy ^0.18.0` | 0.15.0 (Bevy 0.19) |
| `naia-bevy-client` | 0.25.0 (2026-05-12) | `bevy_ecs ^0.18` | 0.25.0 |
| `bevy_ggrs` | 0.21.0 (2026-04-09) | `bevy ^0.18.0` | 0.22.0 (Bevy 0.19) |
| `mdns-sd` | 0.21.6 (2026-10-19) | ohne Bevy | 0.21.6 |
| `if-addrs` | 0.15.0 (2026-02-08) | ohne Bevy | 0.15.0 |
| `igd-next` | 0.18.0 (2026-10-04) | ohne Bevy | 0.18.0 |

Quelle für "Neueste für Bevy 0.18": der Sparse-Index `index.crates.io`, jeweils die höchste nicht
zurückgezogene Version, deren `bevy`-, `bevy_ecs`- oder `bevy_app`-Anforderung 0.18 nennt. Das
Paar `bevy_replicon` 0.40.4 und `bevy_replicon_renet2` 0.17.0 ist genau das, was deanmathieson
benutzt. Ein Bevy-Upgrade von upstream auf 0.19 oder 0.20 heißt: replicon auf 0.41 bis 0.45 und
renet2 auf 1.0 mitziehen.

Bewertung:

- **bevy_replicon + bevy_replicon_renet2** (oder `bevy_replicon_renet`): Server-autoritative
  Replikation von Komponenten, Listen-Server (Host ist Server und Spieler zugleich),
  Nachrichten Client zu Server und Server zu Client, Entity-Mapping, Tick-Reihenfolge. renet2 bringt
  natives UDP und für später WebSocket- und WebTransport-Client-Transporte (wasm-Gäste an einem
  nativen Host). Genau dieses Paar hat deanmathieson auf Bevy 0.18 mit diesem Spiel zum Laufen
  gebracht.
- **lightyear**: Replikation plus Vorhersage mit Rollback, Interpolation, Bandbreitenbudget mit
  Prioritäten, Host-Client-Modus; UDP direkt, WebSocket/WebTransport/Steam über aeronet. Für
  Bevy 0.18 sind es die Versionen 0.26 bis 0.27 (README-Tabelle). Stärker, aber größer, schneller im
  Wandel, und Vorhersage brauchen wir nicht: der eigene Held wird ohnehin lokal gesteuert, die
  Gegner nur gezeigt.
- **bevy_quinnet / bevy_replicon_quinnet**: QUIC über `quinn`, verschlüsselt, zuverlässige und
  unzuverlässige Kanäle. Gut, aber QUIC über das Internet bringt nichts gegenüber renet, und im
  Browser gibt es kein QUIC außer WebTransport.
- **aeronet (+ aeronet_replicon)**: Transport-Sammlung (WebTransport, WebSocket, Steam, Channel),
  gut für Browser-Gäste, weniger erprobt für natives UDP.
- **renet allein**: ginge, dann schreiben wir Replikation selbst (das, was FlorianFortier und
  GladstoneOG von Hand gebaut haben).
- **matchbox (bevy_matchbox, matchbox_socket)**: WebRTC-Peer-to-Peer, auch im Browser. Braucht
  einen **Signalling-Server** (`matchbox_server`) und für viele NATs STUN/TURN. Damit fällt es unter
  die Nicht-Ziele. Für Bevy 0.18 gäbe es `bevy_matchbox` 0.14.0, aber die replicon-Brücke
  `bevy_replicon_matchbox` steht seit 2025-07 still (Bevy 0.16).
- **naia**: eigenes Replikationsmodell, 0.25.0 für Bevy 0.18, aber seit 2026-05 ohne Release und
  kleineres Umfeld.
- **bevy_ggrs (Rollback-Lockstep)**: nicht für uns, siehe Variante C.

## Architektur-Varianten

Die Bandbreitenrechnung gilt für alle: Gegnerziel `data::target_alive` liegt bei höchstens 220,
mit Surge 330; mit vier Spielern und +35 % Menge je Mitspieler deckeln wir bei 400. Pro bewegtem
Gegner und Tick fallen etwa 8 bis 14 Byte an (Entity-Kennung, Komponenten-Kopf, x und z als
Zentimeter-Varint, Gierwinkel 1 Byte, Tempo und Flags; die Höhe setzt der Client selbst, weil
`ground_clamp_creatures` jede Kreatur ohnehin auf den Boden setzt). Bei 20 Hz und 400 Gegnern sind
das 64 bis 112 KB/s pro Gast, dazu Effekte und Zahlen (geschätzt 5 bis 20 KB/s). Für drei Gäste
lädt der Host also 0,2 bis 0,4 MB/s hoch, rund 2 bis 3,5 Mbit/s. Das passt im LAN immer und bei
üblichem Upload ab 10 Mbit/s auch übers Internet. Spätere Sparmaßnahmen: ruhende Gegner gar nicht
senden (replicon sendet nur geänderte Komponenten), Position nur bei mehr als 5 cm Änderung,
Gierwinkel aus der Bewegungsrichtung ableiten, Gegner weit weg vom jeweiligen Gast seltener oder gar
nicht schicken (Sichtbarkeit pro Client, in der replicon-Version zu prüfen).

### Variante A: bevy_replicon + renet2 über UDP, Host-autoritativ (Empfehlung)

- Crates: `bevy_replicon` und `bevy_replicon_renet2` mit `native_transport`, hinter einem Feature
  `coop` in `survivors-app`, nur `cfg(not(target_arch = "wasm32"))`.
- Autorität: Der Host simuliert alles, was den Lauf entscheidet (Spawner, Gegner-KI, Fähigkeiten,
  Treffer, Drops, XP, Gold, Bosse, Truhen). Jeder Gast steuert seinen Helden lokal mit Kollision
  und Navigation und schickt Position, Blickrichtung und Bewegungsflags 20 bis 30 Mal pro Sekunde
  (unreliable). Der Host glaubt sie innerhalb der Arena.
- Gäste simulieren nichts: Sie bauen jeden Körper aus der replizierten `Look`-Komponente mit
  `units::spawn_unit`, interpolieren 100 ms hinter dem Host und spielen Effekte aus einer
  Effekt-Nachricht über dieselben Writer (`units::Fx`) wie solo.
- Pro: Die Reihenfolge Spawn, Mutation, Despawn und das Entity-Mapping macht die Bibliothek; ein
  neuer oder zurückkehrender Gast bekommt automatisch die ganze Welt; der Weg ist auf genau diesem
  Spiel und dieser Bevy-Version erprobt (deanmathieson); `net.rs` ist fast fertig übernehmbar.
  renet2 hält den Weg zu Browser-Gästen offen.
- Contra: zwei Abhängigkeiten, deren Major-Version an Bevy hängt (bei jedem Bevy-Upgrade von
  upstream mitziehen); die Netcode-Verbindung ist im Modus `Unsecure` unverschlüsselt und ohne
  Anmeldung, wer den Port erreicht, kann beitreten (Lobby-Bestätigung durch den Host als Schutz).
- Aufwand: 13 bis 19 Tage über alle Stufen (siehe Stufenplan).

### Variante B: Eigenes Protokoll auf `std::net::UdpSocket`, Host-autoritativ

- Ohne neue Crates: ein eigener Binär-Codec (wie `survivors-net` von FlorianFortier) über einen
  nicht blockierenden UDP-Socket, eigene Zuverlässigkeit für Spawn, Despawn und Level-up, eigene
  Paketaufteilung unter 1200 Byte, eigene Snapshots oder Deltas.
- Pro: keine Abhängigkeit an Bevy-Versionen, volle Kontrolle über jedes Byte, der Codec liefe
  auch im Browser.
- Contra: Wir bauen nach, was renet und replicon schon können, und erben deren Fehlerklassen
  (FlorianFortier #50, GladstoneOGs fragmentierte JSON-Snapshots). Tests für schlechte
  Verbindungen müssen wir selbst schreiben.
- Aufwand: 20 bis 28 Tage, davon etwa eine Woche nur für Transport, Zuverlässigkeit und Tests.

### Variante C: Lockstep (alle simulieren, nur Eingaben werden getauscht)

- Jeder Rechner simuliert den ganzen Lauf mit demselben Seed; getauscht werden nur die Eingaben
  pro Tick (selbst gebaut oder `bevy_ggrs`).
- Pro: minimale Bandbreite, unabhängig von der Gegnerzahl.
- Contra: verlangt eine bitgenau deterministische Simulation. Unsere ist es nicht: `Time::delta`
  mit variabler Bildrate in jedem System, `f32` über verschiedene CPUs, avian3d-Kollision, Navigation,
  Abfragen über Entity-Reihenfolge, `Rng` an vielen Stellen, `HashMap`-Iteration. `lib.rs` sagt es
  selbst: mit `WOW_SURVIVORS_SEED` laufen zwei Läufe auseinander, sobald die Uhr zählt. Umbau auf
  feste Ticks und Determinismus wäre größer als das ganze Koop; ein Desync zeigt sich erst Minuten
  später. `bevy_ggrs` 0.21.0 gäbe es für Bevy 0.18, löst aber das Determinismus-Problem nicht.
- Aufwand: unseriös schätzbar, sicher über 30 Tage. Verworfen.

### Empfehlung

Variante A. Sie ist der einzige Weg, der auf unserem Spiel und Bevy 0.18 schon einmal lief, sie
nimmt uns die Fehlerklassen ab, an denen die selbstgebauten Netze der Forks gescheitert sind, und
sie braucht keinen Server: Der Host bindet einen UDP-Port, die Gäste verbinden sich direkt. Die
Bandbreite reicht für vier Spieler mit 400 Gegnern. Wir übernehmen `net.rs` und den Aufbau von
deanmathieson, die Interpolations- und Testideen von FlorianFortier, und schreiben die
Spielseite für unsere Modulstruktur neu.

## Entdeckung im LAN und Direct IP

### Ports

- **UDP 37373**: das Spiel (renet2-Netcode). Einstellbar im Host-Panel und über
  `WOW_SURVIVORS_HOST=<port>`.
- **UDP 37374**: die LAN-Suche.
- Nicht Port 5000 wie bei deanmathieson: auf macOS belegt der AirPlay-Empfänger (ControlCenter)
  TCP 5000 und 7000; das stört UDP nicht, verwirrt aber bei jeder Fehlersuche.

### Direct IP

- Join-Feld nimmt `host:port`, eine nackte IPv4- oder IPv6-Adresse (dann Port 37373) oder einen
  Namen, den der Resolver kennt (`deanmathieson` `coop/net.rs::parse_addr`, mit Tests).
- Das Host-Panel zeigt die eigene LAN-Adresse (UDP-Socket nach außen verbinden und `local_addr`
  lesen; es geht dabei kein Paket raus).
- **Über das Internet** braucht der Host eine UDP-Portweiterleitung 37373 auf seinen Rechner und
  gibt seine öffentliche IP weiter. Hinter CGNAT oder Dual-Stack-Lite geht das nicht; dann bleibt
  eine öffentliche IPv6 oder ein VPN.
- **Einfachster Weg übers Internet**: Tailscale oder ZeroTier auf allen Rechnern, dann Direct IP
  auf die `100.x.y.z`-Adresse. Keine Weiterleitung, kein zentraler Spieldienst (Tailscale selbst ist
  ein Koordinationsdienst, aber nicht unserer und optional).
- **UPnP/NAT-PMP** (`igd-next` 0.18.0, 2026-10-04) könnte die Weiterleitung automatisch öffnen.
  Nicht in dieser Runde: viele Router haben UPnP aus, und ein Fehlschlag muss trotzdem erklärt
  werden.

### LAN-Suche

Vorschlag: eigener UDP-Broadcast mit Frage und Antwort, kein mDNS.

- Der Host lauscht auf UDP 37374 (`0.0.0.0`). Ein suchender Gast schickt jede Sekunde, solange die
  Liste offen ist, `WSRV?` mit Protokollversion an `255.255.255.255:37374` und an die
  Broadcast-Adresse jeder Schnittstelle (`if-addrs` 0.15.0). Der Host antwortet per Unicast mit Name,
  Zone, belegten Plätzen, Spielport und Build-Tag. Die Liste zeigt jeden Host, der in den letzten 3
  Sekunden geantwortet hat; ein Klick füllt das Join-Feld.
- Frage und Antwort statt Dauerbeacon: Es geht nur Verkehr übers Netz, während jemand sucht.
- Beide Sockets sind nicht blockierend und werden in einem System pro Frame gelesen; kein Thread.
- Jede Schnittstelle einzeln, nicht nur `255.255.255.255`: macOS schickt den globalen Broadcast nur
  über die Hauptschnittstelle, und Apple rät ausdrücklich, auf allen broadcastfähigen Schnittstellen
  zu suchen (TN3179).
- Über Tailscale und ZeroTier gibt es keinen Broadcast; dort bleibt Direct IP.
- **mDNS** (`mdns-sd` 0.21.6, reines Rust, eigener Thread) wäre die Alternative: `_wsurvivors._udp`
  registrieren und durchsuchen. Mehr Abhängigkeit, ein Hintergrund-Thread, Port 5353 neben
  `mDNSResponder`, und auf macOS gelten dieselben Regeln wie für Broadcast. Bringt für 2 bis 4
  Rechner im selben Netz nichts, was der Broadcast nicht kann.

### macOS

Aus Apples TN3179 "Understanding local network privacy" (Revision 2026-10-06):

- Seit macOS 15 fragt das System, bevor ein Programm das lokale Netz nutzt. Betroffen sind
  ausgehende Verbindungen und UDP-Unicast an LAN-Adressen, UDP-Broadcast und -Multicast in beide
  Richtungen und Bonjour. Nicht betroffen: eingehendes UDP-Unicast und eingehende TCP-Verbindungen
  annehmen. Ein Host, der nur auf Pakete wartet, braucht es also erst beim Antworten; ein Gast
  braucht es sofort.
- **Kommandozeilenprogramme, die aus dem Terminal oder per SSH laufen, sind ausgenommen**,
  samt Kindprozessen. `cargo survivors` aus dem Terminal fragt also nicht.
- Ein `.app`-Bundle (falls wir das Spiel später als App ausliefern) wird gefragt. Dann gehört
  `NSLocalNetworkUsageDescription` in die `Info.plist`, für mDNS zusätzlich `NSBonjourServices`.
  Das Multicast-Entitlement braucht macOS nicht (nur iOS).
- macOS merkt sich die Erlaubnis über die Code-Signatur und die UUID der Binary. Unsignierte oder
  ad hoc signierte Builds können nach jedem Neubau wieder fragen oder sich falsch verhalten; für eine
  ausgelieferte App braucht es eine Apple-Signatur.
- Wird gerade gefragt, kann das System die erste Operation sofort ablehnen. Die Suche wiederholt
  ihre Frage ohnehin jede Sekunde; der Verbindungsaufbau von renet wiederholt sich auch.
- VPN-Schnittstellen (Tailscale, ZeroTier) zählen nicht als lokales Netz; dort fragt macOS nicht.
- Unabhängig davon die Anwendungs-Firewall (Systemeinstellungen, Netzwerk, Firewall), falls
  eingeschaltet: Sie fragt beim ersten Lauschen "eingehende Verbindungen erlauben?", bei
  unsignierten Builds unter Umständen nach jedem Neubau.
- Windows fragt beim ersten Binden über die Windows-Firewall (privates Netz erlauben).

### Browser

- Im Browser gibt es weder UDP noch lauschende Sockets. Ein Browser kann nie hosten, ohne
  WebRTC, und WebRTC braucht einen Signalling-Weg (matchbox: Signalling-Server), also einen
  zentralen Dienst.
- Was später ohne Server ginge: Ein **Browser-Gast an einem nativen Host**. renet2 hat Client-
  Transporte für WebSocket und WebTransport; der native Host lauscht zusätzlich auf einem
  WebSocket- oder WebTransport-Port. Haken: Eine `https`-Seite (der Realm) darf nur `wss://`, also
  braucht der Host ein Zertifikat, dem der Browser traut; WebTransport erlaubt selbstsignierte
  Zertifikate über `serverCertificateHashes`, aber nur mit kurzer Laufzeit. Über die Dev-Seite
  auf `http://127.0.0.1:8090` ginge `ws://` im LAN.
- Für diese Runde: das Feature `coop` ist auf wasm aus, der Browser bleibt solo.

## Spielregeln für 2 bis 4 Spieler (Vorschlag)

Alles hier ist ein Vorschlag zur Entscheidung, keine beschlossene Regel.

- **XP und Stufe gemeinsam.** Eine XP-Leiste, eine Stufe für die Gruppe; jeder Gem zählt für alle,
  egal wer ihn einsammelt. Gems fliegen zum nächsten Helden, dessen Magnet sie erreicht. So machen
  es alle drei Forks mit echtem Koop.
- **Level-up**:
  - Stufe 3a (billig, zuerst): Die Gruppe steigt auf, das Spiel pausiert für alle, jeder sieht
    seine eigenen drei Karten, der Host wartet, bis alle gewählt haben. Nach 20 Sekunden wählt das
    Spiel für Säumige zufällig. Oben steht "Warte auf: Name, Name". Das passt zum heutigen
    `Phase::LevelUp`, das `Time<Virtual>` pausiert.
  - Stufe 3b (wenn 3a nervt): keine Pause mehr. Jede Stufe legt jedem Spieler eine Hand zurück, er
    öffnet sie mit Tab und wählt im Laufen; während die Karten offen sind, nimmt er weniger
    Schaden (deanmathieson). Früh im Lauf steigt man alle paar Sekunden auf; mit vier Spielern
    hieße 3a dann Dauerpause.
  - Reroll, Banish, Skip gehören dem Spieler; der Host teilt aus und prüft jede Wahl gegen die Hand,
    die er ausgeteilt hat.
- **Niedergeschlagen statt tot.** Bei 0 Leben fällt ein Held, die Gegner lassen ihn in Ruhe. Ein
  Mitspieler, der 3 Sekunden innerhalb von 3 Yards steht, holt ihn mit 50 % Leben zurück. Der Lauf
  ist verloren, wenn alle gleichzeitig liegen. Wer liegt, sieht die Kamera weiter über seinem Helden
  (später: Zuschauen bei einem Mitspieler).
- **Gegner skalieren mit der Gruppe**: Leben ×(1 + 0,5 × (n − 1)), Menge ×(1 + 0,35 × (n − 1)),
  höchstens 400 Gegner. Spawn-Ringe abwechselnd um einen stehenden Helden, nie näher als 25 Yards an
  irgendeinem Helden. Bosse zielen auf den nächsten Helden.
- **Kamera pro Spieler**: Jeder Rechner folgt seinem eigenen Helden wie solo. Mitspieler als
  Namensschild, Gruppenrahmen links (auf Basis von `frames.rs`) und Punkt auf der Minimap.
- **Gold**: Wer eine Münze aufhebt, bekommt sie; gezählt mit dessen Multiplikator. Truhen gehören dem,
  der sie öffnet, mit Beute aus dessen Klassen-Beutetabelle.
- **Profil**: Jeder bringt sein Profil mit (Rasse, Talente, Ausrüstung, Sammlungsboni, Perks,
  gekappte Werte). Am Ende schreibt jeder Rechner nur sein eigenes Profil fort (Gold, Erfolge,
  Rekorde, Beute).
- **Schwierigkeit, Zone, Laufoptionen**: die des Hosts.
- **Pause (Esc)**: im Koop nur ein Menü über dem eigenen Bild, die Welt läuft weiter. Nur die
  Level-up-Pause aus 3a hält alle an.
- **Gast geht**: Sein Held verschwindet, die Skalierung sinkt ab der nächsten Spawnwelle. Später:
  Platz als "Away" halten und Rejoin (FlorianFortier #51).
- **Host geht**: Der Lauf endet für alle. Die Gäste sehen die Ergebnisse mit dem Stand bis dahin und
  bekommen ihr Gold und ihre Erfolge gutgeschrieben, als hätten sie verloren.

## Was sich in `crates/survivors-app` ändert

Der große Umbau ist nicht das Netz, sondern dass der Modus heute genau einen Helden kennt: `Run`
(`lib.rs`) hält Leben, Fähigkeiten, Passive, Schild, Gold, Meta-Boni und Ausrüstung des einen
Helden, und rund 25 Systeme holen den Helden mit `Query<.., With<Hero>>` und `single()`. `Res<Run>`
oder `ResMut<Run>` steht in 28 Dateien.

### Neue Dateien

- `coop/mod.rs`: Plugin, `Session` (Link: Off, Hosting, Joining, Joined; eigene Spielernummer;
  Lobby-Liste; Status), `NetRole` (Solo, Host, Client) als Ressource, Run-Conditions
  `simulates()` (Solo oder Host) und `is_client()`, Registrierung aller replizierten Komponenten
  und Nachrichten in fester Reihenfolge, Build-Tag-Abgleich.
- `coop/net.rs`: Host binden, Join, Adress- und Port-Parser, LAN-IP (aus deanmathieson).
- `coop/wire.rs`: `Look` (Display, Skala, Level, Fraktion, `bytes0`, Waffen, Scheide), `NetPos`
  (x und z in cm, Gier 1 Byte, Tempo, Bewegungsflags, Absenderzeit), `NetHero` (Spieler, Name,
  Klasse, Rasse), `NetVitals` (Leben, Max, Schild, liegt, Revive-Fortschritt), `NetBuild` (Fähigkeiten
  und Passive mit Rang), `NetEnemy` (Roster-Index, Boss, Elite), `NetLife` (Lebensanteil, tot),
  `NetRun` (Uhr, Kills, Stufe, XP, Bosse, Banner), `NetGem`. Nachrichten: `Hello`
  (Name, Klasse, Look, Loadout), `MoveInput`, `CardAction`, `Welcome`, `Refused`, `Lobby`,
  `StartRun` (Zone, Seed, Schwierigkeit), `RunPhase`, `Hand`, `NetFx`.
- `coop/host.rs`: Gäste begrüßen und Plätze vergeben, Gast-Helden spawnen, `MoveInput` anwenden,
  die Simulation in Wire-Komponenten spiegeln (`PostUpdate`), Effekte weiterreichen, Hände
  austeilen und Wahlen prüfen.
- `coop/client.rs`: aus `Look` Körper bauen, `NetPos` interpolieren, `NetFx` abspielen, eigene
  Bewegung senden, Phasenwechsel des Hosts übernehmen.
- `coop/smooth.rs`: Snapshot-Interpolation (Idee aus FlorianFortier `net/smooth.rs`).
- `coop/discovery.rs`: LAN-Suche (Frage und Antwort über UDP 37374).
- `coop/lobby.rs`: Panel im Menü: Host mit Port, Join mit Adresse, LAN-Liste, Gruppenliste, Start.

### Geänderte Dateien

- `Cargo.toml` (Workspace und `survivors-app`): `bevy_replicon` und `bevy_replicon_renet2` als
  optionale Abhängigkeiten, Feature `coop` in `survivors-app`, durchgereicht von
  `warcraft-survivors`; `wenilla` lässt es aus.
- `lib.rs`: `Run` teilen in `Run` (gemeinsam: `elapsed`, `kills`, `level`, `xp`, `pending_levels`,
  `bosses`, `rings`, `won`, `banner`, `next_guid`, `difficulty`) und eine Komponente
  `HeroState` am Helden (Klasse, Rasse, Leben, Fähigkeiten, Passive, Schild, Dornen, Heilung,
  Gold, `meta`, `mobile`, `gear`, `buffs`, `talents`, `dead_t`, `damage_done`, `version`).
  `Run::stats()` wird `HeroState::stats()`. `Phase` bleibt; im Client setzt der Host sie. Die
  Simulations-Kette bekommt `.run_if(simulates)`. `reset_run` räumt Session-Reste auf.
- `hero.rs`: Marker `LocalHero` für den eigenen Helden. `aim_cursor`, `drive_hero`, `place_camera`,
  `follow_map` nur für `LocalHero`. `spawn_hero` bekommt Spielernummer und `HeroState`. `hero_vitals`
  über alle Helden; Liegen und Revive.
- `enemies.rs`: `spawn_enemies` mit Gruppenskalierung und Ring um einen Helden reihum;
  `move_enemies` verfolgt den nächsten stehenden Helden und schadet dessen `HeroState`;
  `collect_gems` zum nächsten Magneten, XP in den gemeinsamen `Run`; `drop_loot` merkt sich den
  Töter.
- `abilities.rs`: `cast_abilities` über alle Helden statt `hero.single()`; `Hit` bekommt
  `by: Entity` (wer traf, für Schadensmesser und Zuordnung); `apply_hits` und `tick_areas` nur auf
  dem Host.
- `units.rs`: `spawn_unit` hängt `Look` an jeden Körper; `Fx` schreibt auf dem Host zusätzlich in
  einen Ausgang, den `coop/host.rs` als `NetFx` verschickt; der Client spielt `NetFx` über dieselben
  `Fx`-Methoden ab.
- `hud.rs` und `cards.rs`: HUD liest den `HeroState` des `LocalHero`; `Offer` wird pro Held
  (Komponente oder Map nach Spielernummer); `roll_offer` teilt auf dem Host für jeden aus;
  `choose_card` wendet solo und auf dem Host direkt an, im Client schickt es `CardAction`.
  Für 3a: Overlay "Warte auf ...".
- `pickups.rs`, `chests.rs`, `items.rs`, `evolutions.rs`, `finale.rs`, `quests.rs`, `surge.rs`,
  `enrage.rs`, `worldbuffs.rs`, `barrier.rs`, `mobility.rs`: auf dem Host über alle Helden bzw. für
  den auslösenden Helden; im Client aus.
- `frames.rs`: Gruppenrahmen aus den replizierten Helden. `plates.rs`: Namen über Mitspielern.
  `minimap.rs`: Punkte für Mitspieler. `dmgmeter.rs`: pro Held.
- `menu.rs`: Knopf "Mehrspieler" neben "Enter World"; im Client startet Enter World nichts.
- `profile.rs`: Loadout für `Hello` bauen; `award_run` schreibt nur das eigene Profil.
- `auto.rs`: Testhilfen `WOW_SURVIVORS_HOST=<port>`, `WOW_SURVIVORS_JOIN=<adresse>`,
  `WOW_SURVIVORS_NAME=<name>`, damit zwei Instanzen auf einem Mac (mit eigenem `BENILLA_HOME`)
  ohne Tastatur zusammen spielen und Screenshots machen.
- `benilla-app`: voraussichtlich nichts. Ein Client baut Körper genau wie solo über `Guid`,
  `NetEntity` und `ObjectStore`, die `survivors_host` schon freigibt.

## Stufenplan

Aufwand in Arbeitstagen: Michael als Orchestrator mit Implementer-Agents, inklusive Bauen,
Testen mit zwei Instanzen und einem zweiten Rechner, Screenshots und README-Zeile.

### Stufe 1: Zwei Spieler laufen sichtbar zusammen in derselben Zone (2 bis 3 Tage)

- Abhängigkeiten, Feature `coop`, `coop/mod.rs`, `net.rs`, minimale `wire.rs` (`Look`, `NetPos`,
  `NetHero`, `MoveInput`, `Hello`, `Welcome`, `StartRun`).
- Host und Join über Umgebungsvariablen und ein schlichtes Panel mit Adressfeld.
- `LocalHero`-Marker; der Gast-Held ist auf dem Host eine Puppe, die `MoveInput` folgt, und wird
  noch nicht angegriffen. Der Host repliziert beide Helden; der Gast sieht den Host-Helden
  interpoliert.
- Der Gast simuliert nichts (`simulates()` aus), sieht in Stufe 1 also noch keine Gegner.
- Fertig, wenn: zwei Instanzen auf einem Mac und zwei Rechner im LAN laufen sichtbar zusammen,
  mit Screenshot `docs/screenshots/fork-coop.jpg`.

### Stufe 2: Gegner und Treffer host-autoritativ (5 bis 7 Tage)

- Der Umbau `Run` zu `Run` plus `HeroState`, allein, solo ohne Verhaltensänderung (ein Commit,
  vorher und nachher gleich im Auto-Modus).
- Gegner, Gems, Flächen, Totems, Truhen repliziert; `NetFx` für Casts, Treffer, Schwünge, Zahlen.
- Gegner verfolgen den nächsten Helden, Fähigkeiten aller Helden laufen auf dem Host, Schaden
  wird dem Werfer gutgeschrieben, Gruppenskalierung.
- Messung der Bandbreite bei voller Horde (renet-Statistik ins Log).

### Stufe 3: Level-ups, Loot, Revive (4 bis 6 Tage)

- Gemeinsame XP, Level-up nach Regel 3a, Hände pro Spieler, `CardAction` vom Gast, Prüfung auf
  dem Host.
- Gold und Truhen pro Spieler, Profil-Loadout im `Hello`, `award_run` lokal.
- Liegen und Revive, Niederlage erst bei allen, Ergebnisbildschirm für die Gruppe.
- Host geht: sauberes Ende für die Gäste; Gast geht: Held weg.

### Stufe 4: LAN-Suche, 4 Spieler (2 bis 3 Tage)

- `coop/discovery.rs` und die Liste im Panel.
- Vier Plätze, Gruppenrahmen, Minimap-Punkte, Namen.
- Build-Abgleich mit Klartext-Ablehnung, Lobby mit Bereit-Status, Kick durch den Host.
- Test mit vier Instanzen und mindestens zwei echten Rechnern, macOS und Windows.

Summe: 13 bis 19 Tage. Danach, nicht geplant: Level-up 3b ohne Pause, Rejoin, Browser-Gast über
WebSocket, UPnP.

## Erste konkrete Schritte

1. Im Work-Worktree (oder als Feature-Runde auf `features`, siehe `CLAUDE.md`) die Abhängigkeiten
   in einer Wegwerf-Probe prüfen: `bevy_replicon = "=0.40.4"`, `bevy_replicon_renet2 = "=0.17.0"`
   (Features `renet_serde`, `client`, `server`, `native_transport`) gegen unser `Cargo.lock`
   bauen und schauen, ob sich Abhängigkeiten von Bevy doppeln.
2. `coop/net.rs` aus deanmathieson übernehmen (Herkunft im Kopfkommentar), Ports auf 37373 ändern,
   seine Tests mitnehmen.
3. `LocalHero` einführen und die vier Input-Systeme in `hero.rs` darauf umstellen: kleiner, solo
   folgenloser Commit, der Stufe 1 vorbereitet.
4. `Look` aus `UnitSpec` ableiten und in `units::spawn_unit` anhängen.
5. Danach Stufe 1 wie oben.

## Entscheidungen (2026-10-31)

- **Netz**: Variante A, `bevy_replicon` + `bevy_replicon_renet2`. Michael nimmt in Kauf, dass
  replicon bei jedem Bevy-Upgrade mitgezogen wird.
- **Level-up**: keine Pause. Das Spiel läuft weiter; wer gerade seine Karten wählt, ist dabei
  unverwundbar und wird von den Gegnern ignoriert (sie verfolgen ihn nicht und schlagen nicht zu).
  Das ersetzt 3a und 3b in Stufe 3. Solo bleibt der Level-up wie heute (Pause), bis Stufe 3 die
  Regel für beide Fälle festlegt.
- **Zeitpunkt**: jetzt, als nächste Runde auf `features`, ohne parallele Feature-Runde.
- Noch offen, mit Vorschlag als Standard bis Michael anders entscheidet: gemeinsame XP-Leiste (2),
  LAN plus Tailscale und Portweiterleitung ohne UPnP (3), Profil-Mitbringsel gekappt (5), Host geht
  beendet den Lauf (6), Koop auf `features` ohne Upstream-PR (8).

## Offene Fragen an Michael

1. **Level-up**: erst die Pause für alle mit 20-Sekunden-Zeitlimit (3a, billig), oder gleich ohne
   Pause mit zurückgelegten Händen (3b, wie deanmathieson)?
2. **XP**: gemeinsame Leiste und Stufe für die Gruppe (Vorschlag), oder jeder für sich?
3. **Internet**: Reicht "LAN plus Tailscale/ZeroTier plus Portweiterleitung von Hand", oder soll
   UPnP (`igd-next`) früh mit rein?
4. **Bevy-Version**: Die replicon-Versionen für Bevy 0.18 sind eingefroren. Wenn upstream auf 0.19
   oder 0.20 geht, ziehen wir replicon mit. Einverstanden, oder soll Koop eine Bevy-unabhängige
   Lösung sein (Variante B)?
5. **Profil-Mitbringsel**: Darf ein Gast seine Ausrüstung, Talente und Perks mitbringen (gekappt,
   ohne Prüfung), oder spielen im Koop alle mit Basiswerten?
6. **Host geht**: Reicht "Lauf endet für alle", oder soll ein Gast übernehmen können (Host-Wechsel
   ist teuer, mehrere Tage)?
7. **Umbau-Zeitpunkt**: Der `Run`/`HeroState`-Umbau berührt 28 Dateien. Er sollte allein laufen,
   ohne andere Feature-Runde parallel. Wann passt das?
8. **Upstream**: Koop als eigene Feature-Runde auf `features` (kein Upstream-PR, wie die letzten
   Runden), oder als Branch von `upstream/main` mit PR an OutThisLife?
