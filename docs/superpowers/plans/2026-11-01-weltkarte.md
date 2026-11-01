# Weltkarten-Modus: Implementierungsplan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Im Lauf öffnet **M** die WoW-Weltkarte der Zone, in der das Schlachtfeld liegt (die echten Kacheln aus `Interface\WorldMap\<Zone>\`, platziert über `WorldMapArea.dbc`), mit dem Helden als Pfeil, der Barriere, Bossen, Elites, Truhen, Beute, Quest-Zielen und der Gruppe. Im Hub gibt es zusätzlich ein Fenster „World Map“: die Kontinentkarte, auf der man die Zone anklickt; gesperrte Zonen grau, Bestzeit und Sperrgrund im Infostreifen.

**Ticket:** Kanboard #1149 „Weltkarte auf M“ (Projekt „Azeroth Survivors“, #18).

**Architecture:** Ein neues Modul `crates/survivors-app/src/worldmap/` mit vier Dateien. `atlas.rs` (Grundlage) lädt `WorldMapArea.dbc`, `WorldMapOverlay.dbc` und die `.zmp`-Raster einmal, findet die Kartenseite zu einem Schlachtfeld und baut eine Seite (zwölf 256-px-Kacheln plus alle Overlays, als wäre alles erkundet) in einen beliebigen UI-Knoten. `run.rs` ist das Kartenfenster im Lauf mit zwei Ebenen: **Zone** (die WoW-Zonenkarte, das Schlachtfeld als Kreis darauf) und **Schlachtfeld** (`field.rs`: der Boden aus den Minimap-Kacheln, auf das Arenaquadrat zugeschnitten, mit Barrierering und allen Markern). `hub.rs` ist das Hub-Fenster mit der Kontinentkarte. Marker, die in privaten Komponenten anderer Module leben (Truhen, Quest-Items, Beute, Gruppe), bekommen bei ihrem Spawn eine gemeinsame Komponente `MapPin`; Bosse und Elites liest die Karte direkt aus `Enemy::boss` und `Elite`.

**Warum zwei Ebenen:** Die Arena ist ein Kreis mit `leash` 30 bis 150 yd ([`data::MAPS`]), eine Zonenseite deckt 3000 bis 6000 yd auf 1002 px. Auf der Zonenseite schrumpft das ganze Schlachtfeld auf 4 bis 40 px; Truhen und Elites darin sind dort nicht unterscheidbar (so sieht es bei hearthside aus). Die Zonenseite liefert das WoW-Gefühl und die Orientierung, die Schlachtfeld-Ebene die taktische Information. Die Minimap-Kacheln haben 0,48 px/yd, die Zonenseiten 0,17 bis 0,3 px/yd: für die Nahansicht sind die Minimap-Kacheln die schärfere Quelle.

**Tech Stack:** Rust, Bevy 0.18.1 (UI: `ImageNode { rect }`, `UiTransform::from_rotation(Rot2)`, `RelativeCursorPosition`, `Overflow::clip()`), `benilla-formats` (`load_world_map_area_catalog`, `load_world_map_overlay_catalog`, `load_zone_map`, `load_minimap_translate`, `world_to_tile`, `tile_to_world`), `benilla-assets` (`WorldAssets::sprite_texture`, `decode_rgba`, `chain.lock_recover()`, `MapCatalogRes`, `coords::bevy_to_wow`).

**Referenz-Code:**
- `~/.cache/coop-reference/hearthside-worldmap/worldmap.rs` (matguitarist/hearthside, Commit `7b689f96e84b6bc7ee1778b895de1d27c86735e8`, `crates/benilla-app/src/survivors/worldmap.rs`, Apache-2.0, Quelle in `SOURCE.txt`). Daraus übernehmen: `project`, `zone_for`, das Zerschneiden der Overlays in 256-px-Stücke (`overlay`), die Kachelschleife und die Pfeildrehung. Herkunft im Modulkopf nennen (Muster: `coop/host.rs:1-4`).
- Diese Codebasis: `benilla-world/src/map_proj.rs` (`zone_uv`, `ZoneRect`, gleiche Achsen), `benilla-ui/src/script/worldmap.rs:84` (`area_grid_cell`, die `.zmp`-Zelle unter einer Weltposition) und `:239` (`continent_hover`, Lage und Zuschnitt der `<Zone>Highlight`-Textur auf der Kontinentseite), `benilla-app/src/ui_world_map.rs:188` (`.zmp`-Werte über `AreaTable.zone_id` eine Stufe hochrollen), `survivors-app/src/minimap.rs` (Minimap-Kacheln über `md5translate.trs`, CPU-Komposit in ein `Image`, 12-Hz-Takt).

---

## Bestandsaufnahme (was schon da ist)

| Was | Wo | Bedeutung für die Karte |
|---|---|---|
| `WorldMapArea.dbc` geladen, Felder `pub` (`map_id`, `area_id`, `name`, `loc_left/right/top/bottom`), Katalog mit `get`, `continent(map_id)`, `iter()` in Dateireihenfolge | `benilla-formats/src/world_map_area.rs` | Seite und Zuschnitt jeder Zone und jedes Kontinents. Der Katalog hat private Felder: Tests bauen `WorldMapArea`-Zeilen direkt, nicht den Katalog. |
| `WorldMapOverlay.dbc` mit `for_area(id)` | `benilla-formats/src/world_map_overlay.rs` | Die erkundeten Teilgebiete; ohne sie ist die Zonenkarte nur die leere Pergament-Grundfläche. |
| `.zmp` (128 × 128 `AreaTable`-Ids je Kontinent) | `benilla-formats/src/zone_map.rs` (`load_zone_map(chain, "Azeroth" / "Kalimdor")`) | Hover und Klick auf eine ganze Zonenfläche im Hub. |
| Projektionen wie im Original | `benilla-world/src/map_proj.rs` (`pub mod map_proj`) | Bestätigt die Achsen: u wächst nach Osten (−wy), v nach Süden (−wx). |
| FrameXML-`WorldMapFrame` mit Datenfeed | `benilla-app/src/ui_world_map.rs`, `benilla-ui/src/script/worldmap.rs` | **Nicht nutzbar:** Survivors läuft mit `CaptureMode`, FrameXML wird nicht geladen (`lib.rs`, `SurvivorsPlugin::build`). Die Karte wird in Bevy-UI gebaut, die Formeln von dort abgeschrieben. |
| Minimap: `md5translate.trs`, Kachel-Cache, CPU-Komposit, `ObjectIcons`-Zellen, `MinimapArrow` | `survivors-app/src/minimap.rs` | Vorlage für die Schlachtfeld-Ebene. Ihre Typen (`Rgba`, Kachel-Cache) sind privat; `field.rs` hält einen eigenen kleinen Cache. |
| Barriere: Kreis `leash` um `hero::arena(def)`, violett-blau | `survivors-app/src/barrier.rs` (`FAR_TINT`, `NEAR_TINT`, privat) | Ring auf beiden Ebenen; Farbe als eigene Konstante nachbauen. |
| Schlachtfelder: `MapDef { name, zone, map, center, leash }`, 14 Stück | `survivors-app/src/data.rs:1702` | `zone` ist teils der Kontinent („Kalimdor“): die Kartenseite kommt aus `WorldMapArea`, nicht aus diesem Feld. |
| Sperren, Bestzeiten | `menu.rs:923` `zone_locked`/`zone_lock` (privat), `questlog::zone_lock`, `records::key`, `records::note` | Hub-Pins grau, Infostreifen. |
| Fensterstil | `survivors-app/src/kit.rs` (`panel`, `heading`, `button`, `KitStyle::Tab`, Farben `TITLE`, `CREAM`, `MUTED`, `DISABLED`) | Beide Fenster im Stil des Hubs. |
| Ebenen im Lauf | `hud.rs`: `HudRoot` 400, `charpanel` 405, `LevelUpRoot` 420, `PauseRoot` 430, `OverRoot` 440 | Kartenfenster auf `GlobalZIndex(425)`. |
| Tasten | `hud::keys` (`hud.rs:1285`): Esc pausiert, Enter startet; belegt sind C, F, G, R, V, X, Tab, F5 bis F9, F12 | **M** und **Z** sind frei. `textfield::TextFocus` muss beachtet werden. |
| Skript und Tour | `shots.rs` (Tasten `A`..`Z`, Zeit ab Lauf-Start), `auto.rs` (`TOUR`, `Windows`, Pausen-Zweig `auto.rs:294`) | Screenshots ohne Tastatur. |

---

## Entscheidungen

### Pause oder weiterlaufen (Variante A empfohlen)

- **Variante A, empfohlen: Allein pausiert die Karte, im Koop läuft die Welt weiter.** Allein setzt M `Phase::Paused` (die Uhr steht über das vorhandene `pause()` in `lib.rs`), das Pausenmenü bleibt verborgen, solange die Karte offen ist; M oder Esc schließt und setzt `Phase::Playing`. Im Koop (`NetRole != Solo`) bleibt die Phase `Playing`: der Held läuft weiter mit WASD, die Marker bewegen sich live, wie die WoW-Karte 2006. Begründung: Im Koop kann ohnehin niemand pausieren (`run_on`, `lib.rs`), und allein ist ein Blick auf die Karte so harmlos wie das Pausenmenü, das es schon gibt.
- **Variante B: Läuft immer weiter.** Wie WoW, aber allein mitten in der Horde gefährlich, und die Karte deckt den Helden in der Bildmitte zu.
- In beiden Varianten aktualisieren sich die Marker mit 10 Hz (Wanduhr), damit das Fenster im Koop stimmt. Der Code für A enthält B: nur der Schalter `pauses()` entscheidet.

### Ebenen und Tasten

- **M**: öffnen und schließen. **Esc**: schließen. **Z**, Mausrad oder die zwei Reiter oben im Fenster: zwischen **Zone** und **Schlachtfeld** wechseln. Rechtsklick auf die Karte: eine Ebene hinaus (Schlachtfeld nach Zone), wie in WoW. Linksklick auf den Arenakreis der Zonenseite: hinein.
- Beim ersten Öffnen eines Laufs: Ebene **Zone** (Offene Frage 2); danach die zuletzt benutzte, solange das Spiel läuft (`WorldMapUi.level` bleibt über Läufe stehen).
- Im Hub: **M** oder der neue Reiter „World Map“ öffnet die Kontinentkarte; Esc oder „Close“ schließt; Enter startet nicht, solange das Fenster offen ist (wie bei den anderen Hub-Fenstern).

### Was auf welcher Ebene erscheint

| Marker | Quelle | Zone | Schlachtfeld | Bild (Fallback: farbiger Punkt) |
|---|---|---|---|---|
| Held | `With<Hero>` | Pfeil | Pfeil, gedreht | `minimap::blips::PLAYER_ARROW_TEXTURE` |
| Gruppe | `MapPin::Ally` | Punkt | Symbol | `Interface\WorldMap\WorldMapPartyIcon` |
| Barriere | `def.leash`, `hero::arena(def)` | Kreis, mind. 10 px | Ring im Bodenbild, außen abgedunkelt | gezeichnet |
| Boss | `Enemy::boss && !dead` | Totenkopf | Totenkopf 22 px, Tooltip mit Namen | `Interface\TargetingFrame\UI-TargetingFrame-Skull` |
| Elite | `With<Elite>`, `!dead` | nein | Stern 16 px | `Interface\TargetingFrame\UI-RaidTargetingIcons`, Zelle 1 (Stern, `[0, 0.25, 0, 0.25]`) |
| Quest-Ziel: Kreatur | `QuestLog::hunted()` enthält `enemy.def.name` | nein | gelber Punkt im Bodenbild | gezeichnet |
| Quest-Ziel: Item am Boden | `MapPin::QuestItem` | nein | Symbol 14 px | `Interface\Minimap\ObjectIcons`, Zelle 3 (`[0.75, 1.0, 0, 0.25]`), gelb getönt |
| Truhe | `MapPin::Chest` | nein | Symbol 16 px | `Interface\Minimap\ObjectIcons`, Zelle 0 (`[0, 0.25, 0, 0.25]`) mit goldenem Rand |
| Beute (Gegenstände, Splitter) | `MapPin::Loot(color)` | nein | Punkt 6 px in der Beacon-Farbe | gezeichnet |
| Gewöhnliche Gegner | `Enemy`, `!dead`, nicht Boss/Elite | nein | roter 2-px-Punkt im Bodenbild | gezeichnet (Offene Frage 4) |

Die Texturen, die diese Codebasis schon kennt: `MinimapArrow` und `ObjectIcons` (`minimap.rs`), `UI-TargetingFrame-Skull` und `UI-RaidTargetingIcons` (`benilla-ui/src/script/nameplate.rs`, `SKULL_TEXTURE`, `RAID_ICON_TEXTURE`), `WorldMapPartyIcon` (nur in einem Test, `benilla-app/src/ui_script/battlefield_minimap_tests.rs:226`, aus FrameXML). Die Zellen von `ObjectIcons` (4 Spalten zu 0,25) folgen `minimap.rs` (`TRACKED_UNIT_CELL`); Zelle 0 und 3 sind beim Bauen am Bild zu prüfen. Liefert `sprite_texture` trotzdem `None`, fällt der Marker auf einen farbigen runden Knoten zurück und es gibt **ein** `warn!` pro Pfad.

---

## Global Constraints

- Kein Gedankenstrich U+2014 in Code, Kommentaren, Texten. Kommentare englisch im Stil der Nachbardateien (ganze Sätze, Doc-Kommentare mit `//!`/`///`); Herkunft übernommenen Codes im Modulkopf.
- Größen im 768-Einheiten-Canvas (`UiScale`, `hud::apply_ui_scale`). Die Fenster müssen in 1024 × 768 passen (4:3); 16:9 ist 1365 breit.
- wasm-tauglich: `survivors-app` baut auch für den Browser (`wenilla`). Keine `std::time`, keine Threads; Takt über `Res<Time<Real>>` wie `minimap::compose`. Nichts davon hinter `cfg(not(wasm32))`, außer was ohnehin schon gegated ist (`coop`, `shots`).
- Kosten: Seitenaufbau nur beim Öffnen, Abbau beim Schließen (`despawn_related::<Children>()`). Das Schlachtfeld-Bodenbild nur einmal je `Selection::stage` komponiert, danach pro 10-Hz-Takt: Grundbild kopieren, Punkte setzen, Upload. Kein System der Karte arbeitet, solange kein Fenster offen ist (frühes `return`).
- Karte zu ist der Normalzustand: Ohne M verhält sich das Spiel exakt wie heute.
- Agenten (CLAUDE.md, „Feature rounds“): nur eigene Dateien und Funktionen, immer `Edit`, nie eine geteilte Datei ganz neu schreiben, nie `lib.rs`, kein git-Befehl, der etwas ändert, kein Spielstart. Prüfen mit `CC=gcc cargo check -p survivors-app --profile play`, Fehler in fremden Dateien ignorieren. Tests: `CC=gcc cargo test -p survivors-app --profile play --lib worldmap`.
- Datenabhängige Tests mit `benilla_formats::wow_data_or_skip!()` und `benilla_formats::open_chain(&data)` (Muster: `benilla-formats/src/world_map_area.rs`, Test unten): ohne Spieldaten übersprungen.

## Review Focus

1. Allein: M öffnet und pausiert (Uhr steht, Gegner stehen), M und Esc schließen und setzen fort, das Pausenmenü erscheint dabei nie. Esc ohne Karte pausiert wie bisher. Paket A, Test `the_map_pauses_only_alone`, Integration Schritt 3.
2. Koop: M öffnet ohne Phasenwechsel, der Held läuft weiter, Marker bewegen sich; ein Level-up oder das Lauf-Ende schließt die Karte. Paket A Schritt 4.
3. Jedes der 14 Schlachtfelder findet eine Kartenseite (Zone, sonst Kontinent), der Arenamittelpunkt liegt auf ihr. Grundlage, Test `every_battleground_has_a_page`.
4. Der Pfeil zeigt in die Laufrichtung (Screenshot nach `hold D 2` zeigt nach Osten). Integration Schritt 2.
5. Hub: ein Klick auf eine gesperrte Zone wählt sie wie in der Liste (die Liste zeigt sie auch), Enter World bleibt dann aus, wie heute. Kein zweiter Weg an `zone_locked` vorbei. Paket C.
6. Tippen in einem Textfeld (Koop-Name) mit M öffnet keine Karte (`TextFocus`). Paket A und C.

---

### Task 0: Grundlage (Orchestrator, vor den Paketen, ein Commit)

**Files:**
- Create: `crates/survivors-app/src/worldmap/mod.rs`, `worldmap/atlas.rs`, `worldmap/run.rs`, `worldmap/field.rs`, `worldmap/hub.rs`
- Modify: `crates/survivors-app/src/lib.rs` (`mod worldmap;`, `worldmap::plugin` in eine der `add_plugins`-Gruppen)
- Modify (Haken, je wenige Zeilen): `hud.rs` (`keys`, `show_panels`), `menu.rs` (Reiter, `pick_map`, Sichtbarkeit), `pickups.rs` (`Beacons::attach`), `chests.rs:136`, `quests.rs:283`, `hero.rs:244`, `coop/client.rs:446`, `auto.rs` (`Windows`, `TOUR`, Pausen-Zweig)

**Interfaces (Produces):**

`worldmap/mod.rs`:

```rust
//! The world map: M in a run opens the zone the battleground lies in, as WoW's world map shows
//! it, or the battlefield itself up close ([`run`], [`field`]); the hub's World Map window picks
//! the zone on the continent ([`hub`]). The pages, their placement and the DBCs are [`atlas`].

pub(crate) mod atlas;
mod field;
mod hub;
mod run;

use bevy::prelude::*;

/// The run's map window. `level` outlives the run, so the next opening shows the last one used.
#[derive(Resource, Default)]
pub(crate) struct WorldMapUi {
    pub open: bool,
    pub level: MapLevel,
    /// The map put the run into `Phase::Paused` (alone) and must resume it on closing.
    pub paused: bool,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
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
    pub continent: Option<u32>,
    /// The battleground under the cursor, an index into `data::MAPS`.
    pub hover: Option<usize>,
}

/// Something on the battlefield the map shows, put on it where it is spawned: the owners keep
/// their own components private.
#[derive(Component, Clone, Copy, Debug, PartialEq)]
pub(crate) enum MapPin {
    /// A ground drop lit by a beacon (an item, a shard), in its beacon colour.
    Loot(Color),
    Chest,
    QuestItem,
    /// Another hero of the party (a host's puppet, a guest's remote body).
    Ally,
}

/// The run window's two page slots: [`run`] fills `ZoneSlot`, [`field`] fills `FieldSlot`; the
/// window shows the one of `WorldMapUi::level`.
#[derive(Component)]
pub(crate) struct ZoneSlot;
#[derive(Component)]
pub(crate) struct FieldSlot;

/// The barrier's violet-blue, as `barrier.rs` tints the wall (between `FAR_TINT` and `NEAR_TINT`).
pub(crate) const BARRIER_TINT: Color = Color::srgb(0.55, 0.50, 1.0);

pub(super) fn plugin(app: &mut App) {
    app.init_resource::<WorldMapUi>()
        .init_resource::<HubMapUi>()
        .init_resource::<atlas::Atlas>();
    run::plugin(app);
    field::plugin(app);
    hub::plugin(app);
}
```

`run.rs`, `field.rs`, `hub.rs`: je nur Modulkopf (ein Satz) und `pub(super) fn plugin(_app: &mut App) {}`.

`worldmap/atlas.rs` (fertig, mit Tests; Kern aus hearthside):

```rust
/// The page the WorldMap art covers in its own pixels: four 256-px tiles across, three down, the
/// last column and row cut short (`WorldMapFrame.xml`'s `WorldMapDetailTile1` to `12`).
pub(crate) const PAGE: Vec2 = Vec2::new(1002.0, 668.0);
pub(crate) const TILE: f32 = 256.0;

/// The world-map DBCs, loaded at the first opening of either window.
#[derive(Resource, Default)]
pub(crate) struct Atlas {
    loaded: bool,
    areas: Vec<(u32, WorldMapArea)>,          // file order, cloned out of the catalog
    overlays: Option<WorldMapOverlayCatalog>,
    /// `.zmp` per continent map id, cells rolled up one step to their zone (`ui_world_map.rs:188`).
    grids: HashMap<u32, Vec<u32>>,
}

impl Atlas {
    /// Load the DBCs once; a missing table logs one `warn!` and leaves it empty.
    pub(crate) fn ensure(&mut self, assets: &mut WorldAssets, areas: Option<&AreaTableRes>);
    /// The zone page of a battleground: the zone `standing` (an `AreaTable` zone id) if it has a
    /// page, else the smallest zone page of `def.map` holding `def.center`, else the continent.
    pub(crate) fn page_for(&self, def: &MapDef, standing: Option<u32>) -> Option<(u32, &WorldMapArea)>;
    /// The continent page of a `Map.dbc` id (`area_id == 0`).
    pub(crate) fn continent(&self, map_id: u32) -> Option<(u32, &WorldMapArea)>;
    pub(crate) fn overlays(&self, id: u32) -> &[WorldMapOverlay];
    /// The zone (`AreaTable` id) under WoW `(wx, wy)` on continent `map_id`, from its `.zmp`.
    pub(crate) fn zone_at(&self, map_id: u32, wx: f32, wy: f32) -> Option<u32>;
}

/// Where WoW `(wx, wy)` falls on a page, 0..1 across (east) and down (south).
pub(crate) fn project(page: &WorldMapArea, wx: f32, wy: f32) -> Vec2;
/// The inverse: WoW `(wx, wy)` at page fraction `uv`.
pub(crate) fn unproject(page: &WorldMapArea, uv: Vec2) -> (f32, f32);
/// The `.zmp` cell under WoW `(wx, wy)`: `area_grid_cell` (`benilla-ui/src/script/worldmap.rs:84`)
/// with the page lerp folded out, `col + 128 * row` of `0.5 - w * 2.929_687_6e-5`.
pub(crate) fn grid_cell(wx: f32, wy: f32) -> Option<usize>;
/// The page choice behind [`Atlas::page_for`], over plain rows so a test can feed it.
pub(crate) fn zone_for<'a>(
    pages: impl Iterator<Item = (u32, &'a WorldMapArea)> + Clone,
    map: u32,
    standing: Option<u32>,
    center: [f32; 3],
) -> Option<(u32, &'a WorldMapArea)>;
/// The page's twelve tiles and every overlay (as if all of it were explored), at `scale`
/// canvas units per art pixel, as children of `parent`. Missing art is skipped.
pub(crate) fn spawn_page(
    parent: &mut ChildSpawnerCommands,
    assets: &mut WorldAssets,
    images: &mut Assets<Image>,
    page: &WorldMapArea,
    overlays: &[WorldMapOverlay],
    scale: f32,
);
/// An absolutely placed box, and the same as an ellipse (`BorderRadius::MAX`).
pub(crate) fn at(p: Vec2, w: f32, h: f32) -> Node;
pub(crate) fn round(p: Vec2, w: f32, h: f32) -> Node;
/// A marker image from a sheet cell `[left, right, top, bottom]` (0..1), or `None` while the
/// sheet is missing; the caller falls back to a dot.
pub(crate) fn cell_image(assets: &mut WorldAssets, images: &mut Assets<Image>, path: &str, cell: [f32; 4]) -> Option<ImageNode>;
```

Pfade: Kachel `Interface\WorldMap\{name}\{name}{1..12}.blp`, Overlay-Stück `Interface\WorldMap\{name}\{texture_name}{n}.blp` (n über die Stücke quer, dann runter), wie hearthside `overlay()`. `cell_image` braucht die Bildgröße für `ImageNode::rect` in Pixeln: `images.get(&handle).map(|i| i.size_f32())` direkt nach `sprite_texture` (die Textur wird dort synchron angelegt).

- [ ] **Step 1: Tests in `atlas.rs`**

```rust
#[test]
fn north_is_up_and_west_is_left() {
    let z = row(100.0, -100.0, 200.0, 0.0);
    assert_eq!(project(&z, 200.0, 100.0), Vec2::ZERO);
    assert_eq!(project(&z, 0.0, -100.0), Vec2::ONE);
    assert_eq!(project(&z, 100.0, 0.0), Vec2::splat(0.5));
    let (wx, wy) = unproject(&z, Vec2::new(0.25, 0.75));
    assert!((project(&z, wx, wy) - Vec2::new(0.25, 0.75)).length() < 1e-5);
}

#[test]
fn the_smallest_page_holding_the_centre_wins() {
    // A big page and a small one inside it, both map 1; centre inside both -> the small one.
    // A `standing` that names the big one's area -> the big one. Centre outside both -> None.
}

#[test]
fn grid_cell_matches_the_engine_cell() {
    // Goldshire (-9450, 60) is cell 12735 (benilla-formats zone_map.rs test).
    assert_eq!(grid_cell(-9450.0, 60.0), Some(12735));
}

#[test]
fn every_battleground_has_a_page() {
    let data = benilla_formats::wow_data_or_skip!();
    let mut chain = benilla_formats::open_chain(&data).expect("open chain");
    // Load WorldMapArea; for each data::MAPS entry: page_for(def, None) is Some, the centre
    // projects into [0,1]^2, and `Interface\WorldMap\{name}\{name}1.blp` is in the chain
    // (`chain.contains`). Print the page name per battleground (`--nocapture` shows the table).
}
```

- [ ] **Step 2:** FAIL prüfen, dann `atlas.rs` umsetzen, Tests PASS. Der Ausdruck aus `every_battleground_has_a_page` (welche Seite jedes Schlachtfeld bekommt) kommt in den Commit-Text; landet „Mount Hyjal“ auf dem Kontinent, ist das so gewollt.
- [ ] **Step 3: Haken in geteilten Dateien.**
  - `lib.rs`: `mod worldmap;` alphabetisch, `worldmap::plugin` in die erste `add_plugins`-Gruppe.
  - `hud.rs` `keys` (`hud.rs:1285`): neuer Parameter `maps: (Res<super::worldmap::WorldMapUi>, Res<super::worldmap::HubMapUi>)`. `Phase::Menu`: Enter nur, wenn `!maps.1.open`. `Phase::Playing`: Esc nur, wenn `!maps.0.open`. `Phase::Paused`: Esc nur, wenn `!maps.0.open` (die Karte schließt selbst, `run.rs`).
  - `hud.rs` `show_panels` (`hud.rs:2532`): `PauseRoot` nur bei `p == Phase::Paused && !map.open` (neuer Parameter `map: Res<super::worldmap::WorldMapUi>`).
  - `menu.rs`: `MenuTab::WorldMap` mit Beschriftung „World Map“ vor „Quest Log“; `tab_clicks` setzt `HubMapUi.open = true`. `zone_locked` und `zone_lock` werden `pub(super)`. Neue `pub(super) fn pick_map(selection: &mut Selection, i: usize)` (setzt `map`, erhöht `version`, ruft `save_pick`); `menu_clicks` nutzt sie für `MenuAction::Map`.
  - `pickups.rs` `Beacons::attach`: am Ende `commands.entity(parent).insert(MapPin::Loot(color))`.
  - `chests.rs:136`: nach `beacons.attach(..)` `commands.entity(e).insert(MapPin::Chest)` (ersetzt das `Loot` aus `attach`; die Befehle laufen in Reihenfolge).
  - `quests.rs:283`: `MapPin::QuestItem` in dasselbe `insert`.
  - `hero.rs:244` (`spawn_peer`) und `coop/client.rs:446` (`RemoteBody`): `MapPin::Ally` dazu.
  - `auto.rs`: `Windows` bekommt `ResMut<'w, super::worldmap::HubMapUi>` als achtes Feld, `TOUR` den Eintrag `"worldmap"` am Ende, `open` setzt `w.7.open = on == Some(7)`. Der `Phase::Paused`-Zweig (`auto.rs:294`) setzt nicht fort, solange `WorldMapUi.open` (neuer Parameter), damit ein Skript-Screenshot der Karte stehen bleibt.
- [ ] **Step 4:** `CC=gcc cargo check -p survivors-app --profile play` ohne Fehler und Warnungen (leere Plugins dürfen `#[allow(dead_code)]` auf den noch ungenutzten Typen tragen, bis die Pakete sie nutzen; der Orchestrator nimmt das in Task 4 wieder heraus). `CC=gcc cargo test -p survivors-app --profile play --lib worldmap` PASS.
- [ ] **Step 5:** Commit `Survivors world map: the atlas, the map pins and the hooks, before the windows`.

---

### Paket A: Das Kartenfenster im Lauf und die Zonen-Ebene (Sonnet)

**Owns:** `crates/survivors-app/src/worldmap/run.rs` (ganz). Sonst nichts.

**Consumes:** `WorldMapUi`, `MapLevel`, `ZoneSlot`, `FieldSlot`, `BARRIER_TINT`, `atlas::{Atlas, PAGE, TILE, project, spawn_page, at, round, cell_image}`; `kit::{panel, heading, button, body, KitFonts, KitStyle, KitButton}`; `Selection::stage_def`, `hero::{Hero, arena}`, `enemies::Enemy`, `NetRole`, `Phase`, `textfield::TextFocus`, `WorldPoint::area()`, `AreaTableRes`, `benilla_app::survivors_host::sound::GlueSound` (`"igQuestLogOpen"`, `"igQuestLogClose"`, wie hearthside).

**Produces:** das Fenster; `pub(super) fn pauses(role: NetRole) -> bool` (`role == Solo`); `pub(super) const PAGE_SCALE: f32 = 0.85` (die Seite 852 × 568).

**Aufbau des Fensters** (`GlobalZIndex(425)`, `FocusPolicy::Block`, `RunEntity`, mittig, `Display::None` solange zu): `kit::panel` mit Überschrift (Schlachtfeldname, darunter klein der Name der Seite, z. B. „The Barrens · Kalimdor“ ohne Gedankenstrich: Mittelpunkt U+00B7 ist erlaubt), zwei `KitStyle::Tab`-Reiter „Zone“ und „Battlefield“, darunter ein Rahmen 852 × 568 mit `Overflow::clip()` und zwei Kindern `ZoneSlot` und `FieldSlot` (beide absolut, volle Größe, je nach Ebene `Display::Flex`/`None`), darunter eine Hinweiszeile: „M or Esc to close · Z, the wheel or right-click to change the view“ und die Legende (nur auf der Schlachtfeld-Ebene sichtbar: Hero, Party, Boss, Elite, Chest, Quest, Loot mit ihren Symbolen, Größe 12).

- [ ] **Step 1: Test** in `run.rs`, der Zustandsübergang als reine Funktion:

```rust
/// What a key does to the map: `Some(open)` for a new state, with whether to pause or resume.
fn on_key(open: bool, paused: bool, key: MapKey, role: NetRole) -> Transition { .. }

#[test]
fn the_map_pauses_only_alone() {
    // closed + M, Solo -> open, pause; closed + M, Host or Client -> open, no pause;
    // open + Esc with paused -> closed, resume; open + Esc without -> closed, no resume;
    // open + Z -> level flips, nothing else.
}
```

- [ ] **Step 2:** FAIL, dann `on_key` und die Systeme, registriert in `run::plugin` mit `.after(crate::hud::keys)`:
  - `build_window`: einmal je Lauf, sobald `HudRoot` steht (Muster `charpanel::build`), Abbau in `OnEnter(Phase::Menu)` und `OnEnter(Phase::GameOver)`.
  - `map_keys`: liest M, Esc, Z, Mausrad (`MessageReader<MouseWheel>`), Rechtsklick (`ButtonInput<MouseButton>`), Reiter-Klicks. Nur in `Playing` (öffnen) und, solange offen, in `Playing`/`Paused`; nie bei `TextFocus.0.is_some()`. Öffnen: `pauses(role)` entscheidet über `next.set(Phase::Paused)` und `ui.paused`. Schließen: `ui.paused` setzt `Phase::Playing`. Sound wie hearthside.
  - `close_on_phase`: wechselt die Phase zu `LevelUp`, `GameOver` oder `Menu`, geht die Karte zu (`paused = false`). Wird im Koop aus `Paused` heraus (Pausenmenü) nichts geändert.
  - `fill_zone`: bei `ui.is_changed()` und `open && level == Zone`: `ZoneSlot` leeren, `atlas.ensure`, `page_for(stage_def, standing)` (standing wie `minimap::zone_text`: `world.area()`, über `AreaTableRes` auf die Zone hochrollen), `spawn_page(.., PAGE_SCALE)`, Arenakreis (`round`, Rand 2 px `BARRIER_TINT`, Füllung `BARRIER_TINT` mit Alpha 0.10, Durchmesser mindestens 10), als `Button` mit Marker `ArenaCircle`: Klick setzt `level = Field`. Ohne Seite: Text „No map for this zone“ in der Mitte.
  - `refresh_zone`: 10 Hz, solange `open && level == Zone`: Heldenpfeil (Größe 24, `UiTransform::from_rotation(Rot2::radians(-yaw))`, `yaw` wie `minimap.rs`: `rotation.to_euler(EulerRot::YXZ).0`) und Totenköpfe der lebenden Bosse; Knoten wiederverwenden (ein Pool mit `Display::None` für ungenutzte), nicht jedes Mal neu spawnen.
  - `show_level`: `ZoneSlot`/`FieldSlot`-Anzeige und Reiter `selected` nach `ui.level`.
- [ ] **Step 3:** Tests PASS, `cargo check` ohne Fehler in `run.rs`.
- [ ] **Step 4 (Review Focus 2, nur lesen, nicht starten):** Im Code nachweisen, dass im Koop kein `next.set` passiert und dass `hero::drive_hero` (`hero.rs:360`) in `Playing` den Helden weiter steuert. In der Rückmeldung an den Orchestrator nennen.

**Hinweis:** Die Zonenseite wird nur beim Öffnen und beim Ebenenwechsel gebaut. Die Overlays sind das Teuerste (bis zu ~40 Stücke je Zone), die Texturen cacht `WorldAssets`.

---

### Paket B: Die Schlachtfeld-Ebene (Sonnet)

**Owns:** `crates/survivors-app/src/worldmap/field.rs` (ganz); in `crates/survivors-app/src/quests.rs` nur die neue Methode `QuestLog::hunted` (ein neuer `impl`-Block unter dem bestehenden, nichts anderes anfassen).

**Consumes:** `WorldMapUi`, `MapLevel::Field`, `FieldSlot`, `MapPin`, `BARRIER_TINT`, `atlas::{at, round, cell_image}`; `Selection::stage_def`, `hero::{Hero, arena}`, `enemies::{Enemy, Elite}`, `quests::QuestLog`, `MapCatalogRes`, `benilla_formats::{load_minimap_translate, world_to_tile, tile_to_world, MinimapTranslate}`, `WorldAssets::decode_rgba`, `benilla_assets::coords::bevy_to_wow`.

**Produces:**
- in `quests.rs`: `pub(crate) fn hunted(&self) -> Vec<&'static str>`: die Kreaturnamen der unerledigten `Goal::Kill` und `Goal::Collect`.
- in `field.rs`: `pub(super) const FIELD_PX: usize = 512` (Bodenbild), Anzeige 568 × 568 mittig im Slot; `fn view_half(leash: f32) -> f32 { (leash * 1.15).max(60.0) }` (halbe Seitenlänge in yd); `fn field_uv(center: (f32, f32), half: f32, wx: f32, wy: f32) -> Vec2`; `fn shade_outside(d: f32, radius: f32) -> (f32, f32)` (Abdunklung außen, Ringstärke).

- [ ] **Step 1: Tests** in `field.rs`:

```rust
#[test]
fn the_view_frames_the_barrier_with_room() {
    assert_eq!(view_half(150.0), 172.5);
    assert_eq!(view_half(30.0), 60.0); // Gurubashi: not a postage stamp
}

#[test]
fn north_up_east_right_on_the_field() {
    let c = (100.0, 200.0);
    assert_eq!(field_uv(c, 50.0, 100.0, 200.0), Vec2::splat(0.5));
    // North (wx + 50) is the top edge, east (wy - 50) the right edge.
    assert_eq!(field_uv(c, 50.0, 150.0, 150.0), Vec2::new(1.0, 0.0));
}

#[test]
fn the_ring_sits_on_the_leash() {
    // shade_outside(radius, radius).1 is the ring's full strength; well inside: (1.0, 0.0);
    // well outside: darkening about 0.45, no ring.
}
```

- [ ] **Step 2:** FAIL, dann:
  - `Field`-Ressource (privat, `init_resource` in `field::plugin`): `stage: Option<usize>`, `translate: Option<MinimapTranslate>`, `tiles: HashMap<(u32, u32), Option<Vec<u8>>>`, `base: Vec<u8>` (das komponierte Grundbild), `buf: Vec<u8>`, `image: Handle<Image>` (anlegen wie `MiniMap::new`, `Rgba8UnormSrgb`, `FIELD_PX²`).
  - `compose_ground`: wenn `open && level == Field` und `stage != Some(selection.stage)`: Kacheln laden wie `minimap::compose` (`catalog.0.directory(def.map)`, `translate.tile(dir, tx, ty)`, `decode_rgba("textures\\Minimap\\{hash}")`, nur 256²), für jeden Bildpunkt die Weltposition (`field_uv` umgekehrt), Kachel `world_to_tile`, Texel aus `tile_to_world` wie `minimap.rs` (`origin`, `tex_per_yd`), bilinear innerhalb der Kachel; fehlende Kachel: dunkles Pergament `srgb(0.16, 0.13, 0.09)`. Danach `shade_outside` gegen `leash`: außen abdunkeln, auf dem Kreis 3 px Ring in `BARRIER_TINT` mit weichem Rand. Ergebnis in `base`, `stage` merken. Einmal je Schlachtfeld, nicht je Öffnen.
  - `fill_field`: bei `ui.is_changed()` und `open && level == Field`: `FieldSlot` leeren, `ImageNode::new(field.image)` 568 × 568 mittig, dazu ein leerer Container `FieldMarks` darüber für die Symbole.
  - `refresh_field`: 10 Hz (`Time<Real>`, wie `minimap::compose`), solange offen und Ebene `Field`: `buf = base`, gewöhnliche Gegner als 2-px-Rot `srgb(0.85, 0.12, 0.10)`, gejagte Kreaturen (`hunted()`) als 3-px-Gelb `srgb(1.0, 0.85, 0.1)`, `MapPin::Loot(color)` als 4-px-Punkt; Upload wie `minimap.rs` Ende. Symbole als UI-Knoten in `FieldMarks` aus einem Pool je Art (Boss 22, Elite 16, Truhe 16, Quest-Item 14, Gruppe 16, Held 26 mit Drehung), Mittelpunkt auf `field_uv(..) * 568`; außerhalb des Bildes nicht zeigen.
  - Tooltip (klein, optional als letzter Schritt): ein Boss-Symbol mit `Interaction::Hovered` zeigt den Namen (`enemy.def.name`) in einer `kit::body`-Zeile über dem Symbol.
- [ ] **Step 3:** Tests PASS, `cargo check` ohne Fehler in `field.rs` und `quests.rs`.
- [ ] **Step 4:** In der Rückmeldung die gemessene Zeit von `compose_ground` nennen, falls der Orchestrator sie loggen lassen will (`info!` mit `Instant` aus `bevy::platform::time::Instant`, nicht `std`), sonst weglassen.

**Hinweis:** Auf dem Koop-Gast gibt es `Enemy` und `Elite` auf den gespiegelten Gegnern (`coop/client.rs:481`), aber keine Truhen und Beute (nicht repliziert): Die Karte zeigt dort, was da ist, ohne Sonderfall.

---

### Paket C: Zonenwahl auf der Kontinentkarte im Hub (Sonnet)

**Owns:** `crates/survivors-app/src/worldmap/hub.rs` (ganz). Sonst nichts.

**Consumes:** `HubMapUi`, `atlas::{Atlas, PAGE, project, unproject, spawn_page, at, round}`; `menu::{MenuBuilt, pick_map, zone_locked, zone_lock}`, `hud::MenuRoot`, `kit::*`, `Selection`, `Profile`, `records::{key, note}`, `questlog::{zone_lock, zone_quest}`, `data::MAPS`, `textfield::TextFocus`, `AreaTableRes`, `RelativeCursorPosition` (Bevy: `normalized` ist **mittig** null, `(0.5, 0.5)` unten rechts, also `uv = normalized + 0.5`; `bevy_ui-0.18.1/src/focus.rs:72-91`).

**Produces:** das Fenster; `pub(super) const HUB_SCALE: f32 = 0.72` (Seite 721 × 481); `fn pins(atlas: &Atlas, continent: u32) -> Vec<(usize, Vec2)>` (MAPS-Index, Seitenbruch), `fn highlight_rect(continent: &WorldMapArea, zone: &WorldMapArea) -> (Vec2, Vec2, f32)` (Ecke und Größe als Seitenbruch, `tex_pct_y` nach `continent_hover`, `benilla-ui/src/script/worldmap.rs:239-269`).

**Aufbau** (Kind von `MenuRoot`, Muster `questlog::window`: abgedunkelter Vollbild-Knoten mit `FocusPolicy::Block`, darin `kit::panel`): Überschrift „World Map“, Reiter „Eastern Kingdoms“ und „Kalimdor“, die Seite in einem Rahmen mit `RelativeCursorPosition` und `Interaction`, darunter ein Infostreifen (Höhe ~90): Name der Zone (Morpheus 20) und Zone/Kontinent, Stufenbereich, Bestzeit `records::note`, Sperrgrund `zone_lock(..).long()` in `kit::WARN`, Zonenquest `zone_quest(i)`; rechts unten „Close“. Breite ≤ 1000, Höhe ≤ 740.

- [ ] **Step 1: Tests** in `hub.rs`:

```rust
#[test]
fn the_highlight_covers_the_zone_rect() {
    // A continent row 0..1000 x 0..1000 and a zone row covering its north-west quarter:
    // highlight_rect gives corner (0, 0), size (0.5, 0.5); tex_pct_y = dim / next_pow2(dim).
}

#[test]
fn every_battleground_is_a_pin_on_its_continent() {
    let data = benilla_formats::wow_data_or_skip!();
    // Atlas over the chain; for continent 0 and 1, pins() lists every MAPS entry with that map,
    // each inside (0,1)^2, and zone_at(map, centre) of each resolves to the same zone its page has
    // (print the collisions, e.g. two battlegrounds in one zone, and assert there are none).
}
```

- [ ] **Step 2:** FAIL, dann die Systeme in `hub::plugin` (`.after(crate::hud::keys).before(GlueVisuals)`, nur `in_state(Phase::Menu)`):
  - `build`: einmal in `MenuRoot` mit `MenuBuilt` (Muster `questlog::build`).
  - `keys`: M öffnet/schließt (nicht bei `TextFocus`, nicht wenn ein anderes Hub-Fenster offen ist: dieselben Flags wie `hud::keys` `Phase::Menu`), Esc schließt.
  - `fill`: bei Wechsel von `open` oder `continent`: Seite leeren, `atlas.ensure`, Kontinent = `ui.continent` oder der von `selection.map_def().map`, `spawn_page(.., HUB_SCALE)` (Kontinente haben keine Overlays), dann je Schlachtfeld dieses Kontinents ein Pin: runder `Button` 16 × 16 mit Marker `HubPin(i)`, Gold `kit::TITLE` mit dunklem Rand; gesperrt (`zone_locked`) grau `kit::DISABLED`; gewählt (`selection.map == i`) größer (22) mit hellem Rand. Der Name steht nur am gewählten und am überfahrenen Pin (`kit::body` 11, mit Schatten), damit die dichten Zonen um Elwynn nicht überlappen.
  - `hover`: aus `RelativeCursorPosition` die Weltposition (`unproject`), `atlas.zone_at(continent, wx, wy)`, Zone auf MAPS-Index abbilden (über `page_for` je Schlachtfeld vorberechnet); ein überfahrener Pin hat Vorrang. Ergebnis in `ui.hover`.
  - `highlight`: für `ui.hover`, sonst die gewählte Zone: `Interface\WorldMap\{zone}\{zone}Highlight` an `highlight_rect`, `ImageNode { rect: Some(Rect::new(0, 0, w, h * tex_pct_y)), color: weiß mit Alpha 0.55 }` (normales Alpha-Blending; das Original blendet additiv, `AddUiMaterial` hat private Felder). Fehlt die Textur: der Rand des Rechtecks in `kit::TITLE`.
  - `clicks`: Linksklick auf Pin oder Zonenfläche mit Schlachtfeld: `menu::pick_map(&mut selection, i)` (dieselbe Wahl wie die Liste, gesperrte Zonen eingeschlossen; Enter World bleibt dann aus, wie heute). Klick ins Leere: nichts. Reiter: `ui.continent`. Close: zu.
  - `info`: der Infostreifen für `ui.hover`, sonst die gewählte Zone.
- [ ] **Step 3:** Tests PASS, `cargo check` ohne Fehler in `hub.rs`.

---

### Task 4: Integration, Screenshots, README, Commits (Orchestrator)

- [ ] **Step 1:** Diffs der drei Pakete lesen; `cargo survivors` bauen; Brüche zwischen den Paketen beheben; die `#[allow(dead_code)]` aus Task 0 entfernen; `CC=gcc cargo test -p survivors-app --profile play` (ganze Suite, `the_run_keeps_its_rhythm` muss grün bleiben).
- [ ] **Step 2: Lauf, allein (Review Focus 1, 3, 4).** Barrens, `:late` für Bosse, Elites und Truhen:

```bash
cargo survivors -- --auto mage:late --zone barrens -w --size 1600x900 --shots /tmp/wm \
  --script "30 key M; 31.5 shot wm-zone; 32 key Z; 33.5 shot wm-field; 34 key Escape; 35 shot wm-closed; 36 hold D 2; 38.5 key M; 40 shot wm-east; 41 key M; 42 quit"
```

  Erwartet: `wm-zone` zeigt die Barrens-Seite mit Overlays und dem Kreis, `wm-field` den Boden mit Ring, Pfeil, Totenköpfen, Sternen, Truhen; `wm-closed` das Spiel ohne Pausenmenü; `wm-east` den Pfeil nach rechts. Zwischen `wm-zone` und `wm-field` steht die Uhr (Run-Zeit im HUD gleich). Zweimal mit `--zone gurubashi` (kleinste Arena) und `--zone naxxramas` wiederholen.
- [ ] **Step 3: Esc ohne Karte** pausiert wie bisher: `--script "10 key Escape; 11.5 shot paused; 12 key Escape; 13 quit"` zeigt das Pausenmenü.
- [ ] **Step 4: Koop (Review Focus 2).** Zwei Instanzen wie im Koop-Plan (`docs/superpowers/plans/2026-10-31-coop-stage1.md`, Task 6), der Gast mit `--script "20 key M; 21 hold W 2; 23.5 shot coop-map; 24 key M; 25 quit"`: auf dem Bild hat sich der Held bewegt, der Host-Held erscheint als Gruppensymbol.
- [ ] **Step 5: Hub (Review Focus 5).** `WOW_SURVIVORS_WINDOWS=1 cargo survivors -- --auto mage --zone elwynn -w --size 1600x900 --shots /tmp/wm-hub` speichert `win-worldmap.png` (letzter Tour-Schritt): Östliche Königreiche, Elwynn hervorgehoben, Infostreifen gefüllt. Von Hand (Michael, falls gewünscht): Pin anklicken, die Liste rechts folgt.
- [ ] **Step 6: Screenshots.** Bestes Bild je Fenster als JPEG, 1600 px breit: `sips -s format jpeg --resampleWidth 1600 /tmp/wm/wm-field.png --out docs/screenshots/fork-worldmap.jpg` und `.../win-worldmap.png` nach `docs/screenshots/fork-worldmap-hub.jpg`; Kopien nach `~/.claude/screenshots/azeroth-survivors-re/`.
- [ ] **Step 7: README-Zeile** oben in „What this fork adds“:

```markdown
| World map | M opens the world map of the zone the battleground lies in, WoW's own map art with every area explored, the battlefield a circle on it. Z (or the wheel) zooms to the battlefield: the ground from the minimap art, the barrier, your arrow, the party, bosses, elites, chests, loot and quest targets. Alone the run pauses while it is open; in co-op the world goes on. The hub's World Map window picks the zone on the continent: locked zones grey, the best run and what is missing below. Map code after matguitarist/hearthside (Apache-2.0). | <img src="docs/screenshots/fork-worldmap.jpg" width="480" alt="The world map on the battlefield level: the barrier ring, the hero's arrow, boss skulls, elite stars and chests"> |
```

- [ ] **Step 8: Commits**, je Paket einer, in dieser Reihenfolge: `Survivors world map: M opens the zone's page, alone the run pauses` (A), `Survivors world map: the battlefield up close, with the barrier and everything on it` (B), `Survivors world map: pick the zone on the continent at the hub` (C), `README: row for the world map`. Push `features`. Kanboard #1149 kommentieren (Commits, Screenshots) und auf erledigt setzen.

---

## Nicht in dieser Runde

- Truhen, Beute und Quest-Items auf dem Koop-Gast (sie werden nicht repliziert; eine eigene Komponente im Wire-Protokoll wäre Koop-Stufe-Arbeit).
- Ein Nebel über unerkundeten Gebieten (alle Overlays gelten als erkundet).
- Die Weltebene „Azeroth“ mit beiden Kontinenten auf einer Seite; der Hub hat zwei Kontinent-Reiter.
- Klick auf die Minimap öffnet die Karte (eine Zeile in `minimap::map_buttons`, wenn gewünscht).
- Bedrohungen (Geschosse, Boden-Telegraphen) auf der Karte.

## Aufwand

| Teil | Wer | Umfang | Zeit |
|---|---|---|---|
| Task 0 Grundlage | Orchestrator | ~250 Zeilen `atlas.rs`, ~40 Zeilen Haken in 9 Dateien | 1 bis 1,5 h |
| Paket A | Sonnet | ~350 Zeilen | 1,5 h |
| Paket B | Sonnet | ~400 Zeilen | 2 h |
| Paket C | Sonnet | ~450 Zeilen | 2 h |
| Task 4 Integration | Orchestrator | Bauen, Läufe, Screenshots, README | 1,5 h |

A, B und C laufen parallel: insgesamt etwa 4,5 bis 5 h Wanduhrzeit.

## Offene Fragen an Michael

1. **Pause oder weiterlaufen?** Empfehlung: allein pausiert die Karte (wie das Pausenmenü), im Koop läuft die Welt weiter (dort pausiert ohnehin nichts). Ein Schalter in den Optionen kostet eine Zeile mehr, ist aber erst nötig, wenn dir die Pause beim Spielen zu bequem wird.
2. **Mit welcher Ebene öffnet M?** Empfehlung: beim ersten Mal die Zonenkarte (das WoW-Bild, wie im Ticket), danach die zuletzt benutzte. Wer im Kampf die Truhen sucht, drückt einmal Z und bekommt ab dann das Schlachtfeld.
3. **Hub: Kartenfenster neben der Liste oder statt der Liste?** Empfehlung: ein eigenes Fenster „World Map“ (Reiter und M), die Zonenliste bleibt. Die Liste ist auf kleinen Fenstern und im Browser schneller, und das Heldenmodell in der Mitte bleibt frei.
4. **Gewöhnliche Gegner auf der Karte?** Empfehlung: ja, als kleine rote Punkte auf der Schlachtfeld-Ebene, wie auf der Minimap; die Dichte der Horde zu sehen hilft beim Weg zur Truhe. Auf der Zonenebene nur der Kreis, der Pfeil und die Bosse.
