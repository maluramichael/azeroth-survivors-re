# Kontinent-Lauf („Undying Run“): Implementierungsplan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Ein neuer Lauf, der nicht in einer Zone steht, sondern über die Eastern Kingdoms zieht: Westfall → Elwynn Forest → Redridge Mountains → Duskwood. Ein Pfeil über dem Helden zeigt zur nächsten Station, jede Zone schickt ihre eigene Horde und ihre Bosse, die Horde kommt von vorn und bleibt dran, Heilung liegt auf dem Weg, die Welt lädt ohne Ladebildschirm nach, am Ende wartet der Duskwood-Finalboss in einem geschlossenen Ring. Kanboard-Ticket #1142.

**Architecture:** Der Lauf ist **ein zusätzlicher Eintrag in `data::MAPS`** („Undying Run“, ganz hinten, damit gespeicherte Indizes bleiben). `Selection.stage` bleibt den ganzen Lauf über dieser Index; dadurch funktionieren Rekorde, Erfolge, Profil, `coop::StartRun { map }` und `enemies::def_ref` ohne Drahtänderung. Sein Roster und sein Boss-Plan sind die **aneinandergehängten** Roster und Pläne seiner Zonen, mit Segmenten je Zone. Eine neue Ressource **`route::Bounds`** ersetzt überall die Annahme „Arena = Zentrum + Leine“: im normalen Lauf liefert sie genau die alten Werte, im Kontinent-Lauf ist sie unterwegs offen und am Ende ein Ring. Die Uhr bleibt der Takt (Bosse alle 2:15 aus der Zone, in der der Held gerade ist, Wellen, Schübe), die Schwierigkeit folgt `Run::pace()` = dem Weiteren von Uhr und Wegstrecke. Das Nav-Raster wird für den Lauf ein mitwanderndes Fenster um den Helden. Das Streaming bekommt in `benilla-world` einen additiven Boden für das äußere Fenster (aus der Referenz).

**Tech Stack:** Rust, Bevy 0.18.1, avian3d (Kollision, `SpatialQuery`), `bevy_replicon` 0.40.4 (nur Koop-Paket).

**Referenz-Code:** `~/.cache/coop-reference/marekvi95-undying-run/` (Branch `continent-run`, Commit `84cc7c0`, MIT OR Apache-2.0, Quelle in `SOURCE.txt`). Wichtig: `crates/benilla-app/src/survivors/continent.rs` (Route, `route_frac`, Pfeil, Abschneiden hinten), `git show a22be67` (Streaming-Boden `StreamLookahead`, `StreamWindow::at_with_floor`, Test), `git show 8434e0b e911b1e` (Heilung auf dem Weg). **Nicht** übernehmen: `RUN_SPEED = 2.0`, `ROUTE_CHASE = 1.5`, die globale Senkung von `damage_scale`, die Bosse „einer je betretener Zone“, `ROUTE_ZONES` mit leeren Rostern, die Zonenliste quer durch den Kontinent bis Naxxramas. Unser Code hat seitdem Koop, Quests, Questlog, Finale, Barriere, Schwierigkeit, Bedrohungen und den Simulator; die Referenz kennt nichts davon.

## Was der Code heute annimmt (geprüft)

- **Eine Arena je Lauf.** `hero::drive_hero` hält den Helden in `stage_def().leash` um `hero::arena(map)` und setzt ihn bei mehr als `2 * leash` zurück in die Mitte (`hero.rs` ab Zeile 403). Dieselbe Annahme steckt in `mobility.rs` (Sprünge, Zeile 211), `auto.rs::plan` (Heimweg, Zeile 180), `balance.rs::fly` (Zeile 358), `debug.rs` (F9 und Ringe), `coop/host.rs::apply_moves` (`valid_move(.., centre, leash)`, Zeile 609), `barrier.rs::build_barrier` (Ring mit `leash`, neu gebaut nur bei Wechsel von `selection.stage`) und `nav.rs::scan_world` (Raster `leash + MARGIN` um die Arena, einmal je `stage`).
- **Nav-Raster:** fest um die Arena, gescannt erst nach `SETTLE_FRAMES` = 30 Frames ohne neue Collider (`ColliderEpoch`), höchstens `MAX_REFRESHES` = 4 Nachscans. Außerhalb des Rasters gilt alles als frei (`NavGrid::fits`, `flow` → `None`, Gegner laufen dann gerade auf den Helden zu).
- **Streaming:** `drive_hero` schreibt `player.pos` = Held, das ist der Fokus. `StreamWindow::at(farclip, ..)` (`benilla-world/src/terrain_stream/window.rs`) gibt bei `farclip` 350 ein äußeres Fenster von 13 Chunks (≈ 433 yd), Kacheln sind 16 Chunks (533 yd). Ist die Kachel unter dem Fokus nicht `furnished`, hebt `loading_screen.rs` den Ladebildschirm („Backstop: focus not resident“), und die Simulationskette pausiert (`run_if(!covering)`). Genau das ist der „Ruckler“, den die Referenz mit dem breiteren Fenster bekämpft. Der Art-Sweep rechnet mit `max_resident_reach_yd(777)` (`art_scope.rs:26`), ein Boden bis 26 Chunks ist also sicher.
- **Kollision:** `collide_step` hält den Helden an Bäumen, Gebäuden **und Klippen** auf; die Höhe folgt dem Terrain. Gerade Linien zwischen Zonenmitten laufen gegen Bergketten.
- **Spawns:** `enemies::spawn_enemies` nimmt `data::roster(selection.map)` und `finale::plan(selection.map).timed`, Ring 34 bis 42 yd um einen stehenden Helden (reihum), Bosse nach `BOSS_EVERY_SECS`. Gems: `MAX_GEMS` = 350, darüber fallen keine neuen (`enemies.rs:887`); hinten liegengelassene Gems würden im Lauf die Erfahrung abklemmen.
- **Koop:** Gegner gehen als `def_ref(selection.stage, def)` = `(Liste, Index)` über den Draht, der Gast löst sie mit `def_at(selection.stage, ..)` auf. Ein Gast, der mitten im Lauf kommt, startet bei `arena(map) + aside` (`coop/host.rs::spot`, `coop/client.rs::step_aside`).
- **Questlog:** die Zonenkette ist `zone_order()` über **alle** `MAPS` nach `zone_min_level` (`questlog.rs:213`). Ein neuer Eintrag mit Mindeststufe 1 würde sich vorn einsortieren und die Kette bestehender Profile verschieben.
- **Tests über alle `MAPS`:** `threats.rs` (1 bis 6 Schützen je Roster), `enemies.rs` (`def_ref` findet alles wieder), `quests.rs`, `questlog.rs` (Kette, `unlock_all`), `profile.rs`, `achievements.rs`.

## Global Constraints

- **Ohne den neuen Eintrag verhält sich das Spiel exakt wie heute.** `Bounds` liefert im normalen Lauf bitgenau die alte Leine; `the_run_keeps_its_rhythm` und alle bestehenden Tests bleiben grün, ohne geänderte Schwellen.
- **Ein Kontinent, keine Weltreise im Lauf:** alle Stationen auf Map 0. `follow_map` sieht nur `stage_def().map` = 0.
- **Gegner-Definitionen kommen im Lauf immer aus `data::roster(stage)` und `finale::plan(stage)`** (dem Routen-Roster), nie aus `roster(zone)`. Sonst findet `def_ref` sie nicht, und der Koop-Gast sieht sie nicht.
- **Konstanten** (in `route.rs`, Startwerte, der Simulator stimmt sie ab): `TRAVEL_SPEED = 1.25` (Tempo über dem der Klasse, nur im Lauf), `STOP_RADIUS = 20.0`, `PASS_RADIUS = 25.0`, `END_RING = 40.0`, `CULL_YD = 110.0`, `REST_HEAL = 0.5` (Anteil der max. Gesundheit an jeder Station), `AHEAD_SHARE = 0.7`, `AHEAD_CONE = 1.0` (rad), `ROAD_HEAL_EVERY = 10.0`, `ROAD_HEAL_CAP = 3`, `ROAD_HEAL_AHEAD = 14.0`, `ROUTE_STREAM_CHUNKS = 20`.
- **Texte im Spiel englisch**, Kommentare englisch im Stil der Datei. Kein Gedankenstrich U+2014 in Code, Kommentaren, Texten. Code nach Vorbild der Referenz bekommt im Modulkopf: `Parts follow marekvi95/undying-run (branch continent-run, 84cc7c0, MIT OR Apache-2.0).`
- **Agenten-Regeln aus `CLAUDE.md`** („Feature rounds with several agents in one tree“): nur die eigenen Dateien und Funktionen, nur `Edit`, nie `lib.rs`, kein zustandsändernder git-Befehl, kein Spielstart. **cargo erst auf Ansage des Orchestrators** (Michael spielt aus diesem Tree); dann `CC=gcc cargo check -p survivors-app --profile play` mit Michaels Umgebung (`CARGO_HOME`, `RUSTUP_HOME`, `CC=gcc`, `CXX=g++`, steht in `.claude/settings.local.json`), Fehler in fremden Dateien ignorieren.
- **Koop:** `PROTOCOL_ID` wird `0x5753_5552_5631_0002`, weil `NetRun` Felder bekommt.

## Risiken und Gegenmaßnahmen

1. **Streaming über Zonengrenzen (Ladebildschirm mitten im Lauf).** Läuft der Held schneller in eine Kachel, als ihre Doodads und WMOs (gedrosselt, wenige ms je Frame) fertig werden, hebt der Backstop den Ladebildschirm, und der Lauf steht. Große WMOs (Stormwind südlich der Strecke Westfall → Elwynn) kosten am meisten.
   Gegenmaßnahme: Paket P4 übernimmt den additiven `StreamLookahead` der Referenz (Default 0 = heutiges Verhalten, Boden 20 Chunks nur im Lauf, Kacheln werden ≈ 230 yd früher angefordert); Reisetempo nur 1,25 statt 2,0; Wegpunkte mit Abstand zu Stormwind. Messung in der Integration: `grep -c "loading screen: up (focus not resident" ` und `grep "frame hitch"` im Log eines ganzen Auto-Laufs; Ziel 0 nach dem Start. Rückfall ohne Engine-Änderung: `ViewDistance.farclip` im Lauf auf 500 (äußeres Fenster 18 Chunks), kostet Bildrate.
2. **Wege sind nicht begehbar.** `collide_step` stoppt an Klippen; die Luftlinien Westfall → Elwynn, Elwynn → Redridge, Redridge → Duskwood kreuzen Hügel und Bergketten.
   Gegenmaßnahme: Stationen sind die geprüften `MAPS`-Zentren, dazwischen `via`-Punkte auf Straßen (Westbrook-Brücke, Lakeridge Highway / Three Corners, Straße nach Darkshire). Die Punkte werden **im Spiel vermessen**: der Pilot läuft die Strecke ab, P6 loggt `survivors route: stuck at wow (x, y) toward point k`, die Debug-Anzeige zeigt die WoW-Position; der Orchestrator trägt Punkte nach, bis ein Auto-Lauf ohne Hänger durchkommt. Der Pfeil zeigt immer auf den nächsten Punkt, nicht auf die Station.
3. **Nav-Raster an eine Zone gebunden.** Fest um die Arena und mit Ruhe-Bedingung gescannt: unterwegs gäbe es gar kein Raster, und weil ständig Collider nachströmen, käme der Scan nie zur Ruhe.
   Gegenmaßnahme: P3 macht das Raster im Lauf zu einem Fenster (192 × 192 Zellen) um den Helden, das in Schritten von 8 Zellen nachrückt und die überlappenden Zellen behält; gescannt wird ohne Ruhe-Bedingung, zuerst die nie geprüften Zellen nahe dem Helden, dann zyklisch alles. Bis dahin gilt „nicht geprüft = frei“, `SpawnProbe` prüft jeden Spawn ohnehin direkt an der Stelle.
4. **Koop.** Ein Gast, der mitten im Lauf kommt, stünde am Start, Kilometer vom Host; `valid_move` würde jede Bewegung außerhalb der Leine verwerfen; der Gast kennt die Station nicht.
   Gegenmaßnahme: P5 schickt Routenstand in `NetRun` (`route_next`, `route_stop`, `route_arrived`, `route_frac`), der Gast rechnet daraus dieselben `Bounds` und denselben Pfeil. Ein Gast im Lauf landet neben dem Host-Helden, nicht am Start. Offene `Bounds` prüfen gegen den Host-Helden (1500 yd). Stationen zählt, wer zuerst ankommt; schließt sich der Endring, holt `Bounds::hold` Nachzügler hinein (mehr als `2 * END_RING` entfernt → in die Mitte, wie heute beim Zonenwechsel).
5. **Horde und Beute bleiben hinten liegen.** Ohne Abschneiden altert die Horde hinter dem Helden, `target_alive` ist voll, vorn kommt nichts; Gems füllen `MAX_GEMS`.
   Gegenmaßnahme: P1 entfernt Nicht-Bosse und Gems, die weiter als `CULL_YD` von **allen** stehenden Helden sind; P2 spawnt `AHEAD_SHARE` der Horde im Kegel zur nächsten Wegmarke.
6. **Fremde Tests über alle `MAPS`** brechen am neuen Eintrag (Schützen-Zahl im Sammel-Roster, Questlog-Kette). Gegenmaßnahme: die Grundlage passt sie im selben Commit an.

---

### Task 0: Grundlage (Orchestrator, ein Commit)

**Files:**
- Create: `crates/survivors-app/src/route.rs` (Typen, Tabelle, reine Funktionen, `Bounds`, `RouteRun`, `update_bounds`)
- Create (leer, registriert): `crates/survivors-app/src/route_run.rs` (P1), `crates/survivors-app/src/route_hud.rs` (P7)
- Modify: `crates/survivors-app/src/lib.rs` (`mod route; mod route_run; mod route_hud;`, Plugins, `Run.route_frac`, `Run::pace()`, `enemies::road_heals` in der Kette nach `drop_loot`)
- Modify: `crates/survivors-app/src/data.rs` (MAPS-Eintrag, Roster in zwei Durchgängen, Segmente)
- Modify: `crates/survivors-app/src/finale.rs` (nur `PLANS`/`Plan`: zwei Durchgänge, Routen-Plan, Segmente)
- Modify: `crates/survivors-app/src/hero.rs` (`drive_hero`: `Bounds::hold` und `travel_speed`; `hero_vitals`: keine 15:00-Glocke im Lauf)
- Modify: `crates/survivors-app/src/mobility.rs`, `auto.rs` (Heimweg), `balance.rs` (`fly`, Ressourcen `Bounds`/`RouteRun` im `sim_app`), `debug.rs` (F9 und Ringe lesen `Bounds`), `coop/host.rs` (`apply_moves` liest `Bounds`): mechanischer Tausch, Verhalten gleich
- Modify: `crates/survivors-app/src/enemies.rs` (nur: `spawn_gem` wird `pub(super)`, leere `pub(super) fn road_heals() {}`)
- Modify: `crates/survivors-app/src/questlog.rs` (`zone_order` ohne Läufe, `zone_lock` für einen Lauf), Tests in `threats.rs`, `questlog.rs`

**Interfaces (Produces):**

```rust
// route.rs
pub(crate) struct Station {
    /// The data::MAPS zone whose horde and bosses come on the way into this station.
    pub zone: &'static str,
    /// What the banner and the HUD call it.
    pub label: &'static str,
    /// WoW x and y of the stop; the height comes from the terrain.
    pub at: [f32; 2],
    /// Road points on the way here from the station before, in order (WoW x, y).
    pub via: &'static [[f32; 2]],
}
pub(crate) struct RouteDef { pub map: &'static str, pub stations: &'static [Station] }
pub(crate) struct Point { pub at: Vec3, pub station: Option<usize> } // flat Bevy coords, y = 0
pub(crate) static ROUTES: &[RouteDef];
pub(crate) fn route_of(map: usize) -> Option<&'static RouteDef>;   // by data::MAPS[map].name
impl RouteDef {
    pub fn points(&self) -> &'static [Point];   // every via and station in order, cached (OnceLock)
    pub fn zones(&self) -> Vec<usize>;          // MAPS indices, distinct, in order: the roster segments
    pub fn zone_of(&self, station: usize) -> usize;
}
/// The run's place on its route. Reset at the menu; the host (or alone) writes it, a guest takes it from NetRun.
#[derive(Resource, Default, Clone, Debug, PartialEq)]
pub(crate) struct RouteRun {
    pub next: usize,     // index into points() of the next point; points().len() once walked
    pub stop: usize,     // stations reached, the start counted (1 once the run starts)
    pub arrived: bool,   // at the last station: the end ring stands
    pub placed: bool,    // the hero was set down at the start
}
pub(crate) fn route_frac(points: &[Vec3], next: usize, at: Vec3) -> f32;     // from the reference
pub(crate) fn leg_zone(def: &RouteDef, run: &RouteRun, at: Vec3) -> usize;  // origin zone before half the leg, destination after
pub(crate) fn heading(def: &RouteDef, run: &RouteRun, at: Vec3) -> Option<f32>; // atan2(z, x) to the next point
#[derive(Resource, Clone, Copy, Debug, PartialEq)]
pub(crate) struct Bounds { pub centre: Vec3, pub leash: f32, pub fenced: bool, pub key: (usize, u8) }
impl Bounds {
    pub fn hold(&self, pos: Vec3) -> Vec3;  // drive_hero's rule: past 2 * leash to the centre, past leash to the edge; open: pos
    pub fn clamp(&self, pos: Vec3) -> Vec3; // mobility's rule: to the edge only; open: pos
}
pub(crate) fn travel_speed(selection: &Selection) -> f32; // TRAVEL_SPEED on a route, else 1.0
fn update_bounds(selection: Res<Selection>, route: Res<RouteRun>, mut bounds: ResMut<Bounds>); // every frame, before WorldStage::Input, no run conditions
pub(super) fn plugin(app: &mut App); // init Bounds, RouteRun; update_bounds; OnEnter(Menu) resets RouteRun

// lib.rs, Run
pub route_frac: f32,                                  // 0 off a route
impl Run { pub fn pace(&self) -> f32 { self.elapsed.max(self.route_frac * data::VICTORY_SECS) } }

// data.rs
pub(crate) struct Segment { pub map: usize, pub enemies: Range<usize>, pub bosses: Range<usize> }
pub(crate) struct Roster { pub enemies: Vec<EnemyDef>, pub bosses: Vec<EnemyDef>, pub segments: Vec<Segment> } // segments empty for a zone
pub(crate) fn segment(map: usize, zone: usize) -> Option<&'static Segment>;

// finale.rs
pub(super) struct Plan { pub timed: Vec<EnemyDef>, pub finale: EnemyDef, pub segments: Vec<(usize, Range<usize>)> } // (zone, timed range); empty for a zone
```

- [ ] **Step 1: Tabelle.** Ein Lauf, Name `"Undying Run"`. Stationen sind die geprüften `MAPS`-Zentren, `via` zunächst leer (P6 und die Integration füllen sie):
  1. `Westfall`, „Westfall“, `[-10252.2, 1483.5]` (Start)
  2. `Elwynn Forest`, „Elwynn Forest“, `[-9548.5, 115.0]` (Luftlinie 1538 yd)
  3. `Redridge Mountains`, „Redridge Mountains“, `[-9199.8, -2767.5]` (2903 yd)
  4. `Duskwood`, „Duskwood“, `[-10051.8, -792.5]` (2151 yd, Ziel; Finale = Duskwoods Finalboss)
- [ ] **Step 2: MAPS-Eintrag** ganz hinten: `name: "Undying Run"`, `zone: "Eastern Kingdoms"`, `map: 0`, `center` = Westfall-Zentrum `[-10252.2, 1483.5, 41.6]`, `leash: 40.0` (= `END_RING`, Kommentar: „the radius of the end ring“), `victory: "You crossed the Eastern Kingdoms."`, `foes: &[]`, `bosses: &[]`.
- [ ] **Step 3: Roster in zwei Durchgängen.** `ROSTERS` baut erst alle Zonen wie heute, dann jeden Lauf als Verkettung der Zonen-Roster (Kopien der schon geleiterten `EnemyDef`, **nicht** neu durch `ladder`) mit `segments`. Achtung: innerhalb der `LazyLock`-Initialisierung nicht `roster()` aufrufen (rekursive Initialisierung); aus dem gerade gebauten `Vec` lesen. Ebenso `finale::PLANS`: erst die Zonen, dann der Lauf mit `timed` = Verkettung der Zonen-`timed` und `finale` = Kopie von `plan(Duskwood).finale`.
- [ ] **Step 4: `Bounds`-Tausch.** `drive_hero` ersetzt den Leinenblock durch `pos = bounds.hold(pos)` und multipliziert das Tempo mit `route::travel_speed(&selection)`; `hero_vitals` (bekommt `Res<Selection>`) gewinnt bei `VICTORY_SECS` nur, wenn `route_of(selection.stage).is_none()`. `mobility.rs` nimmt `bounds.clamp`, `auto.rs::plan` den Heimweg zu `bounds.centre` nur wenn `fenced`, `balance.rs::fly` `bounds.hold` (im `sim_app` `Bounds` und `RouteRun` einsetzen, `route::update_bounds` vor `fly`), `debug.rs` Kante und Ring aus `Bounds`, `coop/host.rs::apply_moves` `valid_move(.., bounds.centre, bounds.leash)` (offen: gegen den Host-Helden mit 1500 yd, Verfeinerung in P5).
- [ ] **Step 5: Questlog.** `zone_order` lässt Läufe aus; `zone_lock(profile, class, route)` = die erste gesperrte Zone des Laufs (Stufe oder Quest), sonst offen. Kein eigener Quest für den Lauf.
- [ ] **Step 6: Tests** (in `route.rs`, `data.rs`, `finale.rs`, `questlog.rs`):

```rust
#[test]
fn every_station_names_a_zone_with_a_horde_and_bosses() { /* zone in MAPS, roster >= 3 enemies, plan.timed non-empty, all on map 0 */ }

#[test]
fn route_frac_counts_legs_behind_and_along() { /* the reference's test, on points() style input */ }

#[test]
fn fenced_bounds_hold_as_the_old_leash_did() {
    let b = Bounds { centre: Vec3::ZERO, leash: 50.0, fenced: true, key: (0, 0) };
    assert_eq!(b.hold(Vec3::new(30.0, 2.0, 0.0)), Vec3::new(30.0, 2.0, 0.0));
    assert_eq!(b.hold(Vec3::new(60.0, 2.0, 0.0)), Vec3::new(50.0, 2.0, 0.0));
    assert_eq!(b.hold(Vec3::new(120.0, 2.0, 0.0)), Vec3::ZERO);
}

#[test]
fn open_bounds_hold_nothing() { /* fenced: false, any pos comes back unchanged, clamp too */ }

#[test]
fn a_route_roster_is_its_zones_in_order() { /* segments cover enemies and bosses without gaps; names equal roster(zone) segment by segment */ }

#[test]
fn a_route_plan_ends_on_its_last_zones_finale() { /* plan(route).finale.name == plan(duskwood).finale.name; segments cover timed */ }

#[test]
fn routes_stay_out_of_the_zone_chain_and_open_with_their_zones() { /* zone_order has no route; a profile with Duskwood locked has the route locked */ }
```

  `threats.rs`: der Schützen-Test prüft bei einem Lauf jedes Segment einzeln. `questlog.rs`: `order.len() == MAPS.len() - Läufe`. `enemies.rs`: `every_creature_of_a_battleground_is_found_again_by_its_ref` läuft unverändert über den Lauf und beweist die Zeigergleichheit.
- [ ] **Step 7:** `CC=gcc cargo test --profile play -p survivors-app --lib` → alle PASS (auf Ansage, wenn Michael nicht spielt); `CC=gcc cargo check -p survivors-app --profile play` ohne Warnungen.
- [ ] **Step 8:** Commit `Survivors undying run: the route, its roster and the bounds every arena reads`.

---

Die Pakete P1 bis P7 laufen parallel, je ein Sonnet-Agent im Hintergrund. Jedes besitzt genau die genannten Dateien oder Funktionen.

### P1: Lauf-Logik (Stationen, Rast, Endring, Abschneiden)

**Files (besitzt):** `crates/survivors-app/src/route_run.rs` (ganz), `crates/survivors-app/src/finale.rs` (nur `summon`), `crates/survivors-app/src/barrier.rs` (ganz)

**Interfaces:**
- Consumes: `route::{route_of, RouteRun, Bounds, Point, route_frac, STOP_RADIUS, PASS_RADIUS, END_RING, CULL_YD, REST_HEAL}`, `Run::announce`, `enemies::{Enemy, Gem, Dying}`, `finale::{Finale, Reaper}`, `hero::{Hero, Peer}`, `pickups::Beacons`, `HeroState`.
- Produces in `route_run.rs`: `pub(super) fn plugin(app)`, `pub(super) fn sim_init(app)`, `pub(super) fn sim_systems() -> ScheduleConfigs<ScheduleSystem>` (Muster `items::sim_systems`), reine Funktionen `pub(super) fn advance(points: &[Point], next: usize, at: &[Vec3]) -> usize` und `pub(super) fn culled(at: Vec3, heroes: &[Vec3], boss: bool) -> bool`.

- [ ] **Step 1: Tests** in `route_run.rs`:

```rust
#[test]
fn points_are_passed_in_order_and_a_corner_cut_counts() { /* within PASS_RADIUS, or nearer the point after: next advances; a point far ahead does not */ }
#[test]
fn a_station_needs_a_hero_within_its_radius() { /* STOP_RADIUS for stations, PASS_RADIUS for via points; any of several heroes counts */ }
#[test]
fn the_horde_far_behind_every_hero_goes_but_bosses_stay() { /* culled(..) */ }
```

- [ ] **Step 2:** FAIL, dann `advance` und `culled`.
- [ ] **Step 3: Systeme** (in `plugin`, `.after(hero::drive_hero).before(enemies::spawn_enemies)`):
  - `place_start`: im Lauf, `Phase::Playing`, `!placed`, `NetRole != Client` → Held an den ersten Punkt (Höhe aus `WorldPoint`), `placed = true`, `stop = 1`, `next = 1`, Banner `"Cross the Eastern Kingdoms to Duskwood"`. **Nicht** hinter `!covering`: der Sprung löst selbst das Laden aus.
  - `track` (`run_on`, `simulates`, `!covering`): `advance` mit Held und stehenden Puppen; jede neue Station: `stop += 1`, `run.announce(format!("{label} reached. Next: {next}"), 4.0)`, Rast: jeder stehende Held `hp += REST_HEAL * max_hp` (gedeckelt), Renew-Funken (`Fx`, Zauber 139); letzte Station: `arrived = true` (der Endring folgt über `update_bounds`). `run.route_frac = max(alt, route_frac(..))`, bei `arrived` 1,0.
  - `cull_behind` (`simulates`, `!covering`): Gegner ohne `boss`, ohne `Finale`/`Reaper`, und `Gem`, wenn `culled(..)`; `commands.entity(e).despawn()`.
  - `station_beacon`: ein `RunEntity` mit `Beacons::attach(.., gold, 40.0)` an der nächsten Station, umgesetzt bei jedem `stop`.
- [ ] **Step 4: `finale.rs::summon`** im Lauf: das Finale kommt bei `route.arrived` (nicht bei `finale_at()`), an `bounds.centre` + 25 yd; `plan(selection.map).finale` wie bisher. Reaper unverändert ab 30:00.
- [ ] **Step 5: `barrier.rs`** baut den Ring aus `Bounds` (Mitte, Radius) und schlüsselt auf `bounds.key` statt `selection.stage`; ist `fenced == false`, steht kein Ring. Im normalen Lauf identisch zu heute.
- [ ] **Step 6:** Tests PASS (auf Ansage), `cargo check` ohne Fehler in eigenen Dateien.

### P2: Horde und Heilung unterwegs

**Files (besitzt):** `crates/survivors-app/src/enemies.rs` (`spawn_enemies`, `spawn_enemy`, `road_heals`, neue reine Funktionen und Tests)

**Interfaces:**
- Consumes: `route::{route_of, RouteRun, leg_zone, heading, AHEAD_SHARE, AHEAD_CONE, ROAD_HEAL_EVERY, ROAD_HEAL_CAP, ROAD_HEAL_AHEAD}`, `data::segment`, `finale::plan(..).segments`, `Run::pace`.
- Produces: `pub(super) fn spawn_angle(rng: &mut Rng, ahead: Option<f32>) -> f32`, `pub(super) fn zone_pool(pace: f32, seg: &Segment) -> [usize; 3]` (absolute Indizes in `roster(stage).enemies`).

- [ ] **Step 1: Tests:**

```rust
#[test]
fn the_route_horde_comes_from_the_zone_of_the_leg() { /* zone_pool stays inside seg.enemies; early pace takes the segment's first tiers */ }
#[test]
fn most_spawns_lean_toward_the_next_stop() { /* 1000 draws of spawn_angle with ahead = Some(0.0): >= 65 % within AHEAD_CONE; ahead = None: uniform */ }
#[test]
fn a_road_heal_lands_ahead_of_the_hero() { /* pure helper: position within ROAD_HEAL_AHEAD +- 4 yd along the heading */ }
```

- [ ] **Step 2:** FAIL, dann die Funktionen.
- [ ] **Step 3: `spawn_enemies` im Lauf:** Horde aus `zone_pool(run.pace(), segment(stage, leg_zone(..)))`, Schützen-Ersatz über `threats::melee_of(&roster.enemies, &pool, pick)` mit denselben absoluten Indizes; Winkel `spawn_angle(rng, heading(..))` vom jeweiligen Anker-Helden aus; Uhr-Bosse wie heute im Takt, aber aus dem `timed`-Segment der Zone, in der der Anker steht (`timed[seg.start + run.bosses.min(len - 1)]`), höchstens so viele wie das Segment lang ist; Wellen unverändert. `target_alive`, `elite_chance`, `threats::max_shooters` nehmen `run.pace()`.
- [ ] **Step 4: `spawn_enemy`:** `hp_scale` und `damage_scale` aus `run.pace()` (im normalen Lauf = `elapsed`, also gleich).
- [ ] **Step 5: `road_heals`** (Referenz `spawn_route_heals`, Stand e911b1e): nur im Lauf und solange nicht `arrived`; alle `ROAD_HEAL_EVERY` s ein `GemKind::Heal(30.0)` `ROAD_HEAL_AHEAD` yd voraus, seitlich ± 4 yd, höchstens `ROAD_HEAL_CAP` liegend; ältere hinten räumt P1 ab.
- [ ] **Step 6:** Tests PASS, `the_run_keeps_its_rhythm` PASS (auf Ansage).

### P3: Nav-Raster als wanderndes Fenster

**Files (besitzt):** `crates/survivors-app/src/nav.rs` (ganz)

**Interfaces:**
- Consumes: `route::{route_of, Bounds}`, `Selection`.
- Produces: `NavGrid` schlüsselt auf `Option<GridKey>` (`Zone(stage)` oder `Route(stage)`), `fn shift(&mut self, origin: Vec2)` (behält überlappende Zellen), `probed: Vec<bool>`.

- [ ] **Step 1: Tests:**

```rust
#[test]
fn a_moved_grid_keeps_the_cells_it_still_covers() { /* wall at x = 10; shift by 8 cells; wall still solid at its world position */ }
#[test]
fn a_moved_grid_scans_only_what_it_has_not_seen() { /* after shift, probed is false exactly on the new band */ }
#[test]
fn the_route_grid_follows_the_hero() { /* hero 30 yd from the grid centre -> origin moves by whole 8-cell steps toward it */ }
```

- [ ] **Step 2:** FAIL, dann umsetzen. Fenster 192 × 192 Zellen (`CELL` 1 yd), nachrücken wenn der Held mehr als 24 yd von der Rastermitte steht, in ganzen 8-Zellen-Schritten.
- [ ] **Step 3: Scan im Lauf:** keine `SETTLE_FRAMES`-/`REFRESH_QUIET`-Bedingung; erst ungeprüfte Zellen in Ringen um den Helden, dann zyklisch alle (Budget `SCAN_BUDGET` wie heute). Ohne Lauf bleibt `scan_world` Zeile für Zeile, wie es ist.
- [ ] **Step 4:** Alle bestehenden Nav-Tests PASS, neue PASS.

### P4: Streaming ohne Ladebildschirm

**Files (besitzt):** `crates/benilla-world/src/terrain_stream.rs` (Ressource, Init, die eine Zeile `StreamWindow::at`), `crates/benilla-world/src/terrain_stream/window.rs`, `crates/survivors-app/src/hero.rs` (nur `follow_map`)

**Interfaces:**
- Produces in `benilla-world`: `#[derive(Resource, Default, Clone, Copy)] pub struct StreamLookahead { pub outer_floor_chunks: i32 }`, `StreamWindow::at_with_floor(farclip, wow_x, wow_y, outer_floor) -> Self`; `at` ruft `at_with_floor(.., 0)`.
- Consumes in `follow_map`: `route::{route_of, ROUTE_STREAM_CHUNKS}`.

- [ ] **Step 1:** Referenz-Diff `git show a22be67 -- crates/benilla-world` übernehmen (unser `terrain_stream.rs` hat an der Stelle denselben Stand, Zeile ≈ 556), mit ihrem Test `a_route_floor_widens_the_window_and_never_narrows_it`, Floor-Wert im Test 20 statt 26, plus `far_corner_yd(&route) < max_resident_reach_yd(777.0)`. Kommentar mit Herkunft am Typ.
- [ ] **Step 2:** `follow_map` setzt `outer_floor_chunks = ROUTE_STREAM_CHUNKS` solange `route_of(selection.stage)` ein Lauf ist, sonst 0 (nur bei Änderung schreiben).
- [ ] **Step 3:** `CC=gcc cargo test --profile play -p benilla-world terrain_stream::window` → PASS (auf Ansage). `cargo check -p survivors-app`.

### P5: Koop

**Files (besitzt):** `crates/survivors-app/src/coop/wire.rs` (`NetRun`), `coop/net.rs` (`PROTOCOL_ID`), `coop/host.rs` (`mirror`-Teil für `NetRun`, `spot`, `apply_moves`), `coop/client.rs` (`sync_run`, `step_aside`, neue Platzierung)

**Interfaces:**
- `NetRun` bekommt `route_next: u16, route_stop: u8, route_arrived: bool, route_frac: f32` (Default 0).
- Produces in `host.rs`: `pub(super) fn valid_open_move(m: &MoveInput, host: Vec3) -> bool` (endlich, Tempo 0..=50, höchstens 1500 yd vom Host-Helden).
- Produces in `client.rs`: `fn landing(own: Vec3, host: Vec3, player: u8) -> Option<Vec3>` (`Some(host + aside(player))`, wenn `own` mehr als 60 yd vom Host).

- [ ] **Step 1: Tests:**

```rust
#[test]
fn an_open_bound_takes_a_guest_near_the_host_and_drops_one_far_off() { /* valid_open_move */ }
#[test]
fn a_late_guest_on_a_route_lands_beside_the_host() { /* landing(..) */ }
#[test]
fn net_run_carries_the_route() { /* NetRun with route fields round-trips serde as wire.rs's other tests do */ }
```

- [ ] **Step 2:** FAIL, dann umsetzen; `PROTOCOL_ID` → `0x5753_5552_5631_0002`.
- [ ] **Step 3: Host:** `NetRun` aus `RouteRun` und `run.route_frac`; `spot()` im Lauf = Host-Held + `aside`; `apply_moves` bei offenen `Bounds` mit `valid_open_move`, bei geschlossenen wie heute.
- [ ] **Step 4: Gast:** `sync_run` schreibt `RouteRun` (`next`, `stop`, `arrived`, `placed = true`) und `run.route_frac`; einmal je Lauf, sobald der Host-Held (`NetHero.player == 0` mit `NetPos`) bekannt ist, `landing(..)` auf den eigenen Helden anwenden; `step_aside` bleibt für den gemeinsamen Start.
- [ ] **Step 5:** Tests PASS.

### P6: Pilot, Simulator, Debug

**Files (besitzt):** `crates/survivors-app/src/auto.rs` (`plan`, Log), `crates/survivors-app/src/balance.rs` (ganz), `crates/survivors-app/src/debug.rs` (ganz)

**Interfaces:**
- Consumes: `route::*`, `route_run::{sim_init, sim_systems}`, `enemies::road_heals`.

- [ ] **Step 1: Pilot:** im Lauf ersetzt die Fahrt zum nächsten Punkt (`heading`) das Kreisen; Gems unter 12 yd und gefallene Verbündete gehen vor, Ausweichen wie heute. Hänger-Log: weniger als 3 yd Fortschritt zum Punkt in 4 s → `info!("survivors route: stuck at wow ({x:.0}, {y:.0}) toward point {k}")`; alle 10 s im Lauf-Log zusätzlich `stop`, `next`, Entfernung.
- [ ] **Step 2: Simulator:** `SIM_ZONES=undying` findet den Lauf (`debug::map_index`); `sim_app` registriert `route::update_bounds`, `route_run::sim_systems()`, `enemies::road_heals`; `Report` bekommt `stops: usize` und die Zeit je Station; `SIM_TRACE` zeigt sie. Test:

```rust
#[test]
fn the_route_is_walked() {
    // warrior and mage, seed 1, map = the route, 420 s: no fall in the first two minutes,
    // station 2 reached, the horde spawned ahead (>= 60 % of spawns within the cone),
    // at most ROAD_HEAL_CAP road heals lying at once, gems on the ground < MAX_GEMS.
}
```

  Schwellen erst im Simulator messen, dann mit 30 % Reserve festschreiben.
- [ ] **Step 3: Debug:** F9 im Lauf setzt den Helden 15 yd vor die nächste Station (sie zählt im nächsten Frame als erreicht); die Anzeige zeigt `stop/next/frac/pace`, Ringe nur bei `fenced`.
- [ ] **Step 4:** Tests PASS (auf Ansage), `cargo survivors-sim` mit `SIM_ZONES=undying SIM_SEEDS=2` liefert für jede Klasse eine Zeile.

### P7: Anzeige (Pfeil, Zeile, Minimap, Menü)

**Files (besitzt):** `crates/survivors-app/src/route_hud.rs` (ganz), `minimap.rs` (nur `compose`: Stationspunkt), `menu.rs` (Zonenliste, `refresh_enter`), `loottables.rs` (Liste ohne Läufe), `dmgmeter.rs` (nur `refresh_next_boss`), `achievements.rs` (nur `zone_points`, `zone_icon`)

**Interfaces:**
- Consumes: `route::*`, `hud::GOLD_TONE`, `kit::KitFonts`.
- Produces: `pub(super) fn plugin(app)`, `fn arrow_turn(to: Vec3) -> Quat` (Referenz), `fn route_line(label: &str, yards: f32, stop: usize, stops: usize) -> String`.

- [ ] **Step 1: Tests:** `arrow_points_at_the_next_point` (Referenz), `the_route_line_reads_stop_distance_and_count` (`"Next: Elwynn Forest · 830 yd · 2 of 4"`).
- [ ] **Step 2: Pfeil** nach `continent.rs::spawn_arrow`/`show_route` (Gold, Schaft und vierseitige Spitze, 4 yd über dem Helden, auf den nächsten **Punkt** gedreht), eigene Wurzel, versteckt außerhalb eines Laufs, wenn der Held liegt, und bei `arrived`. Zeile unter dem Banner mit eigener UI-Wurzel (`GlobalZIndex` wie `dmgmeter.rs`), Entfernung = Rest der Wegpunkte bis zur nächsten Station.
- [ ] **Step 3: Minimap:** goldener Punkt für die nächste Station, am Rand festgeklemmt, nach den Gegnerpunkten gezeichnet.
- [ ] **Step 4: Menü:** Läufe stehen in eigener Zeile über den Zonen, Untertitel `"4 zones, Westfall to Duskwood"`; Enter-Knopf `"Enter Undying Run"`, Untertitel ebenso; Sperre über `zone_lock` (die erste gesperrte Zone). `loottables` ohne Läufe. `dmgmeter`: im Lauf `"Next boss in mm:ss"` wie gehabt, statt `"Final boss in"` → `"Final boss: Duskwood"` bis `arrived`. Erfolg: `"Undying Run"` → 50 Punkte, Symbol `INV_Misc_Map_01`.
- [ ] **Step 5:** Tests PASS.

---

### Task 8: Integration (Orchestrator)

- [ ] **Step 1:** Ein Build `cargo survivors`, Brüche zwischen Paketen beheben, Diffs lesen, gezielte Nachfragen an die Agenten.
- [ ] **Step 2: Strecke vermessen.** `WOW_SURVIVORS_AUTO=warrior WOW_SURVIVORS_MAP=undying WOW_SURVIVORS_SHOTS=/tmp/undying-shots perl -e 'alarm 1200; exec @ARGV' ./target/play/warcraft-survivors > /tmp/undying.log 2>&1`. Jede `stuck at`-Zeile: an der Stelle mit `--debug --zone undying` (Anzeige zeigt WoW-Position) einen `via`-Punkt auf der Straße wählen (Westfall → Elwynn über die Westbrook-Brücke südlich an Stormwind vorbei, Elwynn → Redridge über den Lakeridge Highway / Three Corners, Redridge → Duskwood über Three Corners und die Straße nach Darkshire), in `route.rs` eintragen, wiederholen bis zum Sieg ohne Hänger.
- [ ] **Step 3: Streaming messen** im selben Log: `grep -c "loading screen: up (focus not resident" /tmp/undying.log` (Ziel: nur der Start), `grep "frame hitch" /tmp/undying.log`. Bleiben Ladebildschirme: `ROUTE_STREAM_CHUNKS` auf 24 oder `TRAVEL_SPEED` senken, dann Rückfall `farclip` 500 prüfen.
- [ ] **Step 4: Balance:** `SIM_ZONES=undying SIM_SEEDS=3 cargo survivors-sim`; Ziel: Ankunft an der Endstation bei 14 bis 17 Minuten für eine mittlere Klasse, Siegquote auf Normal ähnlich der von Westfall. Stellschrauben: `TRAVEL_SPEED`, `AHEAD_SHARE`, `ROAD_HEAL_EVERY`, `REST_HEAL`. `cargo test -p survivors-app --profile play` grün.
- [ ] **Step 5: Koop:** Host und Gast wie Task 6 im Koop-Plan (`BENILLA_HOME=/tmp/coop-a` / `-b`, `WOW_SURVIVORS_MAP=undying` beim Host); Gast 60 s später starten → landet neben dem Host, sieht Pfeil und Zeile; beide Logs ohne Panic.
- [ ] **Step 6: Screenshot-Skript:**

```bash
cargo survivors -- --debug --class warrior --zone undying -w --shots /tmp/undying-script \
  --script "8 shot start; 9 hold W 4; 14 shot leg; 15 key F9; 35 shot elwynn; 36 key F9; 56 key F9; 76 shot ring; 78 quit"
```

  Bild für die README eher aus dem Auto-Lauf (Pfeil, Horde voraus, Zeile, Minimap-Punkt). Als `docs/screenshots/fork-undying-run.jpg` (JPEG, 1600 px breit), Kopie in `~/.claude/screenshots/azeroth-survivors-re/`.
- [ ] **Step 7: README-Zeile** oben in „What this fork adds“:

  `| Undying Run | A run across the Eastern Kingdoms instead of one zone: from Westfall through Elwynn Forest and Redridge to Duskwood. A gold arrow over the hero points down the road to the next stop, each zone sends its own horde and bosses, the horde keeps coming from ahead, heals wait on the road and every stop gives a rest; Duskwood's boss waits in a closed ring at the end. Builds on marekvi95's undying-run. | <img src="docs/screenshots/fork-undying-run.jpg" width="480"> |`
- [ ] **Step 8: Commits**, einer je Paket nach der Grundlage: `Survivors undying run: stops, rests, the end ring and the horde cut behind` (P1), `...: the horde from the zone of the road, leaning ahead, and heals on the way` (P2), `...: a nav grid that walks with the hero` (P3), `World streaming: a floor on the outer window for a viewer that crosses the map` (P4), `...: co-op parties walk the route together` (P5), `...: the pilot, the simulator and the debug keys walk the route` (P6), `...: the arrow, the route line, the minimap stop and the menu row` (P7, mit README-Zeile und Bild). Push `features`, Ticket #1142 kommentieren.

## Aufwand

- Grundlage (Task 0): 2 bis 3 Stunden Orchestrator.
- P1 bis P7 parallel: je 1 bis 3 Stunden Agentenzeit, zusammen etwa 3 Stunden Wanduhr; P1 und P3 sind die größten.
- Integration: 4 bis 6 Stunden, davon das meiste Wegpunkte vermessen und Streaming/Balance messen. Die Strecke ist die größte Unbekannte.
- Gesamt etwa anderthalb Arbeitstage.

## Offene Fragen an Michael

1. **Strecke.** Empfehlung: Westfall → Elwynn Forest → Redridge Mountains → Duskwood (vier Zonen mit fertigen Rostern, ≈ 6600 yd Luftlinie, 14 bis 17 Minuten, Finale Nefaru in Duskwood, passt zu „Undying“). Stranglethorn später als zweiter Lauf: sein einziger Eintrag ist die Gurubashi-Arena (Leine 30, Gegner Stufe 37 bis 62) und läge ≈ 3300 yd hinter Duskwood. Soll es diese Strecke sein, oder Elwynn als Start?
2. **Stationen.** Empfehlung: Zwischenstationen sind Rast (halbe Heilung, Banner, Leuchtsäule), die Zonenbosse kommen weiter im 2:15-Takt aus der Zone, in der man gerade ist; nur die Endstation schließt einen Ring mit dem Finale. Hält Rhythmus und Balance. Alternative: an jeder Station ein Ringkampf gegen den Zonenboss (mehr Stopps, längerer Lauf, mehr Abstimmung).
3. **Engine-Änderung.** Empfehlung: `StreamLookahead` in `benilla-world` übernehmen (erste Änderung des Forks an den Engine-Crates, rein additiv, Default ändert nichts, ≈ 40 Zeilen mit Test). Alternative ohne Engine-Änderung: Sichtweite im Lauf auf 500 heben, kostet Bildrate.
4. **Menü und Freischaltung.** Empfehlung: eigene Zeile „Undying Run“ über der Zonenliste, offen sobald alle vier Zonen für die Klasse offen sind, ohne eigene Quest. Alternative: ein Quest „Win in Duskwood“ im Questlog.
