# Bosse, Wächter und Drop-Modelle: Implementierungsplan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Drei Wünsche von Michael in einer Runde: (1) Bosse mit eigenen Mechaniken, Ankündigung, Boss-Leiste oben und eigener Beute, dazu Rares und ein freiwilliger Bonus-Boss; (2) Wächter-Elites an festen Posten, die eine Truhe oder einen Schrein bewachen und auf der Minimap stehen; (3) echte WoW-1.12-Modelle für alles, was auf dem Boden liegt (Erfahrung, Gold, Ausrüstung, Truhen, Pickups), schwebend und sich drehend, statt der Bevy-Zylinder mit Kugel.

**Architecture:** Alles bleibt im Crate `survivors-app`, ein Modul je Feature. Die Bosse bekommen eine datengetriebene Skript-Bibliothek (`bosses/`), deren Datenmodell aus Deans `raid/` übernommen ist (When/Pick/Does/Shape), aber ohne Deans Debuff-, Dispel-, Tank- und Zwangsbewegungs-Teil. Die Bodenmarkierungen in `threats.rs` lernen Kegel, Linie und Ring. Wächter sind normale `Enemy`-Elites mit dem Marker `Steered`, den `move_enemies` überspringt, solange ein anderes Modul sie bewegt. Drops werden zu GameObjects (`units::object_bundle`) mit Displays aus `GameObjectDisplayInfo.dbc`; damit sieht ein Koop-Gast sie automatisch (der Host repliziert jedes `ObjLook`). Schweben und Drehen ist ein reiner Bild-Versatz zwischen `First` und `PostUpdate`, damit weder `collect_gems` noch die Koop-Replikation (`NetPos`) ihn sehen.

**Tech Stack:** Rust, Bevy 0.18.1, die vorhandenen Engine-Wege (`units::spawn_object`/`object_bundle`, `spawn_area_object`, `Fx`, `GlueSound`), `benilla_formats` für datengestützte Tests (`wow_data_or_skip!`, `load_gameobject_catalog`, `load_creature_catalog`, `load_sound_kit_catalog`).

**Quellen:** Kanboard #1134 (Raid-Stages Molten Core), #1135 (optionaler Bonus-Boss), #1121 (Elite-Lager an festen Posten), #1118 (geschlossen; offen daraus: Koop-Gäste sehen Geschosse und Kreise nicht). Referenz-Code: `~/.cache/coop-reference/dean-progression-full/` (Branch `progression`, Commit `10c6a77`, MIT OR Apache-2.0): `crates/benilla-app/src/survivors/raid/{mod,bosses,draw,tests}.rs`, `bossbar.rs`, `treasure.rs`, `docs/RAIDS.md`, `docs/MC-SPIKE.md`.

---

## Befund: was heute da ist

**Bosse.** Jede der 16 Zonen hat in `data.rs` (`MapDef.bosses`, The Barrens in `BOSSES`) sechs benannte Classic-Bosse und Rares mit echten Displays. Ich habe alle 96 Display-Ids gegen `CreatureDisplayInfo.dbc` → `CreatureModelData.dbc` aufgelöst: jede hat eine Zeile und ein passendes Modell (Hogger 384 `GnollMelee.mdx`, Kazzak 12449 `LordKezzak.mdx`, Hakkar 15295 `Hakkar.mdx`, Kel'Thuzad 15945 `KelThuzad.mdx`, Mother Fang 2541 `MineSpider.mdx` ...). Die Daten-Tabelle "welche Zone welche Bosse" gibt es also schon; was fehlt, ist alles, was einen Boss von einem großen Grunt unterscheidet:
- `enemies::spawn_enemies` ruft die fünf Uhr-Bosse alle 135 s (`data::BOSS_EVERY_SECS`), `finale.rs` den sechsten als Finale (13:30, sein Tod gewinnt den Lauf; Westfall und Fire Plume Ridge holen VanCleef 2029 `HumanMale.mdx`+Extra und Ragnaros 11121 `Ragnaros.mdx` dazu, beide geprüft), Kazzak als unbesiegbarer Reaper ab 30:00.
- Mechanik heute für alle gleich (`threats.rs::boss_moves`): alle 7 bis 10 s abwechselnd ein roter Kreis unter dem Helden oder ein Ring aus 8 Geschossen (`plan_for`), Spell 9613 für alle. `enrage.rs` macht sie unter 40 % schneller und härter.
- Ankündigung: Banner "X approaches!" und eine Chatzeile. Keine Boss-Leiste; die Plate ist 1,4-mal größer (`plates.rs`), der Zielrahmen trägt den goldenen Drachen (`frames.rs`).
- Beute: Truhe mit 3 oder 5 Rängen (`chests.rs`), Gegenstand mit Boss-Gewichten (`items.rs`), immer ein Splitter (`pickups.rs`), Gold und Heilung (`enemies::drop_loot`). Nichts Boss-Eigenes.
- Koop: Kreise und Geschosse sind reine Mesh-Entities ohne Replikation; Gäste sehen sie nicht (#1118, Kommentar).

**Elites.** `spawn_enemies` würfelt Elites aus dem Strom (`elite_chance`, höchstens 8), `threats::arm` gibt ihnen ein Affix (Swift, Shielded, Volatile). Sie jagen mit der Horde. Es gibt keine festen Posten. `move_enemies` holt jeden Gegner, der weiter als 75 yd vom Helden steht, nach vorn ("stray"): ein stehender Wächter würde sofort weggezaubert. Die Minimap (`minimap.rs::compose`) malt nur rote Punkte für Gegner im Sichtkreis (Zoom 5: etwa 67 yd Radius).

**Drops.**
- Erfahrung: GameObjects, die Irrlichter aus Blackfathom eingefroren (`enemies::look_for`, `still_gems`): 426 `BFD_WispSmall.mdx`, 1267 `..Green`, 1307 `..Purple`. Ihr Partikel-Umfang ist 7,7 yd groß.
- Gold: 2614 `GoldPilesmall01.mdx`. Heilung: 114 `TurkeyLeg.mdx`.
- Truhe: 259 `TreasureChest01.mdx` plus der Bevy-Strahl.
- **Ausrüstung und Splitter haben gar kein Modell**: nur `pickups::Beacons`, ein durchsichtiger Bevy-Zylinder mit einer Kugel (`items.rs::drop_items`, `pickups.rs::drop_shards`). Das sind Michaels "hässliche Platzhalter". Weil sie kein `ObjLook` tragen, sieht ein Koop-Gast sie heute überhaupt nicht.
- Quest-Gegenstände: das grüne Irrlicht 1267 (`quests.rs::ITEM_DISPLAY`).
- **Kollision:** Jedes GameObject mit Kollisionshülle bekommt beim Anhängen `RigidBody::Static` + `Collider` auf die Entity und erhöht `ColliderEpoch` (`benilla-app/src/entities/attach/mod.rs:833-845`). `GoldPilesmall01` hat eine Hülle (35 Dreiecke): heute wird jeder Goldhaufen ein statischer Kollider, der sich beim Einsammeln bewegt, und jeder Drop stößt den Nav-Rescan an (`nav.rs`, `REFRESH_QUIET`, `MAX_REFRESHES = 4`). Für Drops nehmen wir deshalb hüllenfreie Modelle und entfernen eine trotzdem angehängte Hülle.

**GameObjects laden geht schon:** `units::spawn_object` / `object_bundle` (Typ 5 GENERIC, Zustand READY) baut ein GameObject, das die Engine wie ein gestreamtes anhängt; Typ 3 (CHEST) läuft durch die Zustandsmaschine (`benilla-app/src/go_anim.rs::go_animates`), Zustand 0 hebt den Deckel (so macht es Dean in `treasure.rs:585-603`).

**Spieldaten:** `wow-data/Data/` im Repo (von `make fetch-data`; `crates/warcraft-survivors/src/main.rs:175` nimmt es, wenn `WOW_DATA` leer ist). Gelesen habe ich sie ohne cargo mit dem vorhandenen `target/release/benilla-extract` (`extract`, `blp`) und einem kleinen MPQ/DBC-Leser unter `/tmp/bwd-tools/` (nur lesend, außerhalb des Repos). Was in den Tabellen unten "geprüft" heißt, steht so in den DBCs; Größen sind die Header-Bounds der M2 (bei Partikelmodellen inklusive Partikel), "Hülle" die Zahl der Kollisionsdreiecke.

## Bewertung: Deans `raid/` (Branch `progression`)

`raid/mod.rs` (1573 Zeilen), `bosses.rs` (Lucifron mit Flamewaker Protectors, Magmadar), `draw.rs` (Gizmo-Linien), `tests.rs` (1158 Zeilen), dazu `bossbar.rs` und `treasure.rs` (Wächter, Truhen-Stufen).

**Gut und übernehmbar:**
- Das Datenmodell: ein Boss ist ein `Script` aus `BossAbility`-Zeilen mit `When` (Every/At/Health), `Pick` (Boss, Tank, Random, Within), `Does` (Ground, Adds, Frenzy, Berserk, Phase, Yell) und `yell(text, sound)`. Das passt genau auf "je Boss ein, zwei eigene Mechaniken".
- `Shape { Circle, Cone, Line, Donut }` mit `contains(origin, yaw, p)` und `forward(yaw)`: reine Geometrie mit Tests, eins zu eins übernehmbar.
- Ideen aus `bossbar.rs` (bis zu zwei Leisten, Ausblenden nach dem Tod, Schädel bei hohem Level) und `treasure.rs` (Wächter auf einem Ring bei 0,75 Leash, Weckradius `min(25, leash*0.4)`, Truhen-Stufen mit echten Displays, Deckel per `GO_STATE_ACTIVE`).

**Nicht übernehmen (jetzt):**
- Debuffs mit Klassen-Dispel, Tank-Swap-Stapel, Gedankenkontrolle, Furcht und Rückstoß als Zwangsbewegung (`ForcedMove`, `Walk`): setzt Tank-Rolle, Heilerklassen und eine Bewegungs-Übernahme im Helden voraus, die `hero.rs` nicht hat. Lucifron lebt von seinen Flüchen; ohne Dispel bleibt von ihm wenig.
- Schaden als Anteil der maximalen Heldengesundheit: bei uns skaliert Boss-Schaden mit `e.damage` (Zonen-Leiter, Uhr, Schwierigkeit), und `balance.rs` ist darauf getunt. Wir behalten unser Modell.
- Gizmo-Linien zum Zeichnen: unsere gefüllten Mesh-Kreise lesen sich besser; Deans Weg, das Telegraph-Datum zu replizieren und auf jeder Maschine zu zeichnen, übernehmen wir in Paket F.
- Die Abhängigkeiten auf seine Module (`party`, `strip::world_runs`, `run_options`, `bonus::School`).

**Urteil:** Datenmodell und Geometrie übernehmen (Herkunft im Modulkopf nennen, wie bei `coop/`), den Rest nicht. Die Raid-Stufe Molten Core (#1134) ist ein eigener Plan: Deans `MC-SPIKE.md` sagt "go" für Map 409, verlangt aber eine deckenbewusste Bodensonde, Lava-Schaden und einen Kamera-Check. Diese Runde legt die Skript-Bibliothek, auf der Lucifron und Magmadar danach sitzen.

## Global Constraints

- Michael spielt aus diesem Tree. **Kein cargo** (kein build, check, test, clippy) und kein Spielstart, bis Michael den Bau freigibt (Memory "agents-parallel-no-cargo"). Agenten prüfen Typen per Auge gegen den gelesenen Code. Erst nach Freigabe: `CC=gcc cargo check -p survivors-app --profile play`, Fehler in fremden Dateien ignorieren.
- Agenten editieren nur ihre Dateien (Edit, nie eine geteilte Datei neu schreiben), nie `lib.rs`, kein zustandsändernder git-Befehl.
- Kein Gedankenstrich U+2014 in Code, Kommentaren, Texten. Kommentare englisch im Stil der Datei. Übernommener Code von Dean: Herkunft im Modulkopf (`deanmathieson/azeroth-survivors-re`, Branch `progression`, `10c6a77`, MIT OR Apache-2.0).
- Alles, was simuliert (spawnt, schadet, belohnt), läuft mit `.run_if(super::run_on).run_if(super::simulates)` und hinter dem Ladebildschirm-Check wie die übrigen Systeme. Alles, was nur zeichnet (Boss-Leiste, Minimap-Marker, Schweben), läuft auch beim Koop-Gast.
- Jeder neue Gegner muss über `enemies::def_ref`/`def_at` auffindbar sein (Koop schickt Gegner als `(Liste, Index)`). Neue Gegner-Definitionen kommen deshalb nur aus `data::roster(map)` oder der neuen Liste `data::extras(map)` (Kategorie 5).
- Drops nur mit hüllenfreien Modellen (Spalte "Hülle" = 0 in der Modell-Tabelle), Ausnahme die Truhen wie heute. Eine trotzdem angehängte Hülle an einem Drop entfernt `dropart::strip_hulls`.
- Boss-Schaden bleibt ein Anteil von `e.damage * e.weaken` wie in `threats.rs`; neue Mechaniken haben eine Vorwarnung von mindestens 1,5 s (Kreis) und 2,0 s (Kegel, Linie, Ring).
- `run.bosses` zählt weiter nur die fünf Uhr-Bosse und das Finale. Rares, Adds, Wächter und der Bonus-Boss erhöhen es nicht (der Rhythmus-Test liest es).
- Alle Spell-Ids in den Tabellen unten habe ich gegen `Spell.dbc` (Name in Spalte 120, Visual in 115) geprüft; neue Ids nur mit derselben Prüfung, sonst als Annahme kommentieren (Stil von `threats.rs`).

## Review Focus

1. Ein Wächter steht, bis man kommt: kein Stray-Teleport, kein Nav-Rescan-Sturm, kein Mitzählen in der Horde-Obergrenze. Paket C Test `posts_stay_inside_every_leash`, Integration Schritt 4.
2. Der Rhythmus des Laufs bleibt: `the_run_keeps_its_rhythm` grün, `cargo survivors-sim` Überlebenszeit im Median höchstens 10 % unter dem Stand vor der Runde (Integration Schritt 3).
3. Koop bricht nicht: `every_creature_of_a_battleground_is_found_again_by_its_ref` deckt die Extras ab; Drops und Kolonnen kommen beim Gast an (ObjLook); Gäste-Systeme rufen keine Simulation.
4. Die Bodenmarkierungen sind fair: jede neue Form hat ihre Vorwarnung, der Simulator-Pilot (`threats::Dodger`) weicht allen Formen aus.
5. Keine Kollider an Drops: nach 5 Minuten Lauf mit `WOW_SURVIVORS_DROPS=20` hängt der Held an keinem Goldhaufen, `nav` hat höchstens seine 4 Rescans.

---

## Task 0: Grundlage (Orchestrator, vor den Agenten, ein Commit)

**Files (alle vom Orchestrator):** `lib.rs`, `data.rs`, `enemies.rs`, `threats.rs` (nur `arm`), `chests.rs`, `items.rs`, `pickups.rs`, `hero.rs` (nur die Glocke), `minimap.rs` (nur der Typ), `units.rs`, neu `bosses/mod.rs`, `bosses/kits.rs`, `bossbar.rs`, `guards.rs`, `dropart.rs`, `rares.rs`.

**Produces:**
- `lib.rs`: Module `bosses`, `bossbar`, `guards`, `dropart`, `rares` mit je `pub(super) fn plugin(app: &mut App)` registriert. `Run.encore: bool` (Standard `false`): ein Bonus-Kampf läuft, die 15:00-Glocke wartet.
- `hero.rs::hero_vitals`: `if run.elapsed >= data::VICTORY_SECS && !run.encore`.
- `enemies.rs`:
  - `#[derive(Component)] pub(super) struct Steered;` "Another module moves this enemy (a guard at its post, a charging boss); the chase leaves it be." `move_enemies` fragt `Without<Steered>` ab (Bewegung, Stray-Teleport, Trennung, Kontaktschlag). `spawn_enemies` zählt `alive`, `elites` und `shooters` nur `Without<Steered>`.
  - `#[derive(Component)] pub(super) struct Rare;` (ein benannter Rare, `boss: true`, zählt nicht in `run.bosses`).
  - `def_ref`/`def_at`: Kategorie `(5, i)` = `data::extras(map)[i]`; der Test `every_creature_of_a_battleground_is_found_again_by_its_ref` kettet `data::extras(map)` an.
  - `Gem::kind(&self) -> GemKind`; `look_for` zieht nach `dropart::gem_look(kind) -> (u32, f32)` um, `spawn_gem` ruft es auf (Verhalten gleich).
- `data.rs`: `pub(crate) fn extras(map: usize) -> &'static [EnemyDef]`, ein `LazyLock<Vec<Vec<EnemyDef>>>` aus in dieser festen Reihenfolge `super::bosses::adds_for(map)`, `super::rares::rares_for(map)`, `super::rares::bonus_for(map)` (alle drei `-> Vec<EnemyDef>`, Stubs leer). Dazu `pub(crate) fn rung(map: usize, at: f32) -> EnemyDef`: die Zahlen (hp, damage, xp) der Zonen-Leiter an der Stelle `at` in 0..1 der Horde, damit Adds und Rares zonengerecht skalieren.
- `threats.rs::arm`: `if e.boss && !reaper && !super::bosses::scripted(e.def.name)`; `bosses::scripted` ist ein Stub `false`.
- `chests.rs`: `#[derive(Clone, Copy, PartialEq, Eq, Debug)] pub(super) enum ChestTier { Elite, Boss, Finale, Post }`, `#[derive(Component)] pub(super) struct Locked;` (`open_chests` überspringt `With<Locked>`), `pub(super) fn spawn_chest(commands: &mut Commands, run: &mut Run, at: Vec3, ranks: usize, tier: ChestTier) -> Entity`. `drop_chests` nutzt es; ein `Rare` würfelt wie ein Elite (1, selten 3 Ränge), droppt aber immer. Statt `beacons.attach` setzt `spawn_chest` `dropart::DropArt::Chest(tier)`.
- `items.rs`: statt `beacons.attach` setzt `drop_items` `DropArt::Item { slot, quality }`. Neu `pub(super) fn roll_from(tpl: &Tpl, rng: &mut Rng, progress: f32) -> Item` (der Rumpf von `roll_item`), `#[derive(Message, Clone)] pub(crate) struct BossDrop { pub pos: Vec3, pub tpl: &'static Tpl, pub wear_now: bool }` und ein System, das daraus ein `ItemDrop` legt (oder bei `wear_now` sofort trägt wie das Finale heute). `Item::slot()`/`quality()` als Lesezugriffe.
- `pickups.rs`: `Beacons` zieht nach `dropart.rs` um; `drop_shards` setzt `DropArt::Shard(ShardArt::{Hearthstone, Intervention, Nova})`.
- `minimap.rs`: `#[derive(Component, Clone, Copy, PartialEq, Eq, Debug)] pub(super) enum MapMarker { Post, Chest, Shrine, Boss, Rare, Portal }`, noch ohne Zeichnung.
- `units.rs`: `const GO_TYPE_CHEST: u32 = 3`, `pub(super) fn chest_bundle(serial, display, scale, pos) -> impl Bundle` (wie `object_bundle`, Typ 3), `pub(super) fn open_object(entity, guid, store, fields)` setzt `GAMEOBJECT_STATE` 0 und schreibt `FieldChanged` (Vorbild Dean `treasure.rs:585-603`).
- `dropart.rs`: `#[derive(Component, Clone, Copy, Debug, PartialEq)] pub(super) enum DropArt { Item { slot: items::Slot, quality: items::Quality }, Shard(ShardArt), Chest(ChestTier) }`, `gem_look`, `Beacons` und ein System `dress` auf `Added<DropArt>`, das genau den heutigen Strahl mit Kugel anhängt. **Verhalten nach Task 0 unverändert.**
- `bosses/mod.rs`: `scripted(name) -> bool` (false), `adds_for(map) -> Vec<EnemyDef>` (leer), `plugin`, `#[cfg(test)] sim_systems()` (leeres System). `bosses/kits.rs` leer. `bossbar.rs`, `guards.rs` (`sim_systems`), `rares.rs` (`rares_for`, `bonus_for` leer, `sim_systems`) als leere Plugins.

- [ ] **Step 1:** Obiges umsetzen, jeder Stub mit Doc-Kommentar, was sein Paket liefert.
- [ ] **Step 2 (erst nach Michaels Freigabe):** `CC=gcc cargo test --profile play -p survivors-app --lib` grün, `cargo survivors -- --debug --zone elwynn -w --shots /tmp/t0 --script "20 shot t0; 21 quit"`: Drops sehen aus wie vorher.
- [ ] **Step 3:** Commit `Survivors: groundwork for boss scripts, guard posts, rares and drop models`.

---

## Welle 1: fünf Pakete parallel

### Paket A: Boss-Mechaniken (`bosses/`, `threats.rs`)

**Files (besitzt):** `bosses/mod.rs`, `bosses/kits.rs`, neu `bosses/shapes.rs`, `threats.rs` (ganz; Task 0 hat nur `arm` berührt).

**Consumes:** `enemies::{spawn_enemy, Enemy, Steered, Dying, def_ref}`, `data::{roster, extras, rung}`, `items::{BossDrop, Tpl, Slot, Quality, Stat}`, `units::{Fx, spawn_area_object}`, `benilla_app::survivors_host::sound::GlueSound`, `chatlog::{ChatLine, ChatKind}`, `Run::announce`.

**Produces:**
- `bosses/shapes.rs` (von Dean): `Shape { Circle { radius }, Cone { radius, half_angle }, Line { length, width }, Donut { inner, outer } }`, `Shape::contains(origin, yaw, p) -> bool`, `forward(yaw) -> Vec3`, `min_windup(shape) -> f32`.
- `bosses/mod.rs`: `When { Every { first, every }, At(f32), Health(f32) }`, `Aim { Boss, Hero, AroundHero(u8) }`, `Does`:
  - `Ground { shape, windup, damage, pool: Option<(f32 /*dps share*/, f32 /*secs*/)> }` (Kreis unter dem Helden, Kegel/Linie vom Boss zum Helden, Ring um den Boss; `pool` hinterlässt `spawn_area_object(2120 Flamestrike)` und schadet jede 0,5 s),
  - `Volley { count, school, spread: Option<f32> }` (Ring wie heute, mit `spread` ein Fächer zum Helden),
  - `Adds { name: &'static str, count: u8, radius: f32 }` (aus `data::extras(map)`, gespawnt mit `spawn_enemy(.., boss: false, elite: false)`),
  - `Charge { windup, speed }` (Linie als Vorwarnung, dann rennt der Boss mit `Steered` die Linie entlang und schlägt, wen er trifft),
  - `Frenzy { secs, speed }` (Tempo mal `speed` für `secs`, Spell 19451 als Aura),
  - `Phase(u8)`.
  `BossAbility { name, spell, when, aim, does, yell: Option<(&str text, &str kit)>, phases }` mit `const fn new(..)`, `.yell(..)`, `.in_phases(..)`; `Kit { boss: &'static str, abilities: &'static [BossAbility], arrive: Option<(&str, &str)>, loot: &'static [Tpl] }`; `scripted(name)`, `kit_for(name) -> Option<&'static Kit>`, `adds_for(map)`.
- `threats.rs`: `Telegraph` trägt `shape: Shape, origin: Vec3, yaw: f32` statt Mitte und Radius; Meshes für Kegel, Linie, Ring im Stil von `fill_mesh`/`edge_mesh` (Bodenhöhe je Stützpunkt); `tick_telegraphs` prüft `shape.contains`; `pub(super) fn spawn_telegraph(.., shape, origin, yaw, secs, damage, pool)` für `bosses`. `Dodger::steer` weicht jeder Form aus (weg vom Ursprung, bei Linie und Kegel seitlich).

- [ ] **Step 1: Tests zuerst** (`bosses/shapes.rs`, `bosses/mod.rs`):

```rust
#[test]
fn shapes_hold_what_they_should() {
    // Deans Fälle aus raid/tests.rs: Kreis am Rand drin, knapp drüber draußen; Kegel vorn drin,
    // hinten draußen, an der Spitze immer drin; Linie im Streifen drin, seitlich und dahinter
    // draußen; Ring: innen und außen draußen, dazwischen drin.
}

#[test]
fn every_kit_names_a_boss_that_exists() {
    // Jeder Kit.boss ist in roster(map).bosses, finale::plan(map).finale oder extras(map) einer Zone.
}

#[test]
fn every_add_is_found_by_def_ref() {
    // Für jede Zone und jedes Adds { name } eines dort spielenden Kits: extras(map) enthält den
    // Namen und def_ref(map, def) == Some((5, i)).
}

#[test]
fn every_ground_ability_warns_long_enough() {
    // windup >= shape.min_windup() für jede Ground- und Charge-Zeile.
}

#[test]
fn a_health_ability_fires_once_as_health_drops() {}
```

Datengestützt (überspringt ohne Daten, `benilla_formats::wow_data_or_skip!()`): `every_voice_kit_is_in_sound_entries` mit `load_sound_kit_catalog`.

- [ ] **Step 2:** `threats.rs` auf Formen umbauen, Kreis-Verhalten der heutigen Bosse unverändert (deren Tests bleiben grün).
- [ ] **Step 3:** Hirn in `bosses/mod.rs`: `attach_kits` (`Added<Enemy>` mit `boss` und Kit → Komponente `BossBrain { clock, next, done, phase, frenzy_t }`, Ankunftsruf `GlueSound(kit)` + Chatzeile `ChatKind::Boss`), `run_kits` (fällige Fähigkeiten wie Deans `RaidBoss::due`, `Fx::cast(boss, spell, ..)`), `charge` (bewegt `Steered`-Bosse, nimmt `Steered` am Ende wieder ab), `burn_pools`, `boss_loot` (`Added<Dying>` mit Kit: mit 60 % ein `BossDrop` aus `kit.loot`, Finale immer, `wear_now` beim Finale).
- [ ] **Step 4:** `bosses/kits.rs` nach der Tabelle **"Boss-Kits"** im Anhang. Zonen ohne Zeile behalten die heutige Kreis-/Salven-Folge (`threats::boss_moves`), denn `scripted` ist dort `false`.
- [ ] **Step 5:** Debug-Hilfe: `WOW_SURVIVORS_BOSS=<Name>` ruft diesen Boss der Zone 5 s nach dem Start 25 yd vor den Helden (nur `super::debug::active()`), zählt nicht in `run.bosses`.
- [ ] **Step 6:** `#[cfg(test)] pub(super) fn sim_systems()` mit `run_kits`, `charge`, `burn_pools`, `boss_loot`.

**Screenshot** (nach Freigabe):
```
WOW_SURVIVORS_BOSS=Hogger cargo survivors -- --debug --class mage --zone elwynn -w --shots /tmp/shots-bosses \
  --script "5.5 shot arrive; 9 shot adds; 13 shot charge; 17 shot fight; 22 quit"
WOW_SURVIVORS_BOSS="Edwin VanCleef" cargo survivors -- --debug --class warrior --zone westfall -w --shots /tmp/shots-bosses \
  --script "8 shot daggers; 12 shot blackguards; 16 quit"
```
**README-Zeile:** "Boss mechanics | Every zone's bosses fight with their own moves from a script table: Hogger calls Riverpaw gnolls and charges, VanCleef throws fans of daggers and calls his Blackguards, Ragnaros drops lava bombs that leave burning pools. Cones, lines and rings join the red circles, each warned on the ground first. Bosses call out as they arrive and drop their own signature loot." Bild `fork-boss-mechanics.jpg`.

### Paket B: Boss-Leiste, Ankündigung, Rare-Rahmen (`bossbar.rs`, `frames.rs`)

**Files (besitzt):** `bossbar.rs`, `frames.rs`.

**Consumes:** `enemies::{Enemy, Rare, Dying}`, `enrage::Enraged`, `minimap::MapMarker`, `hud::{HudFont, UiArt}`, `kit`, `GlueSound`.

**Produces:**
- Die Leiste oben Mitte unter Uhr und Surge-Zeile (`TOP` etwa 108 auf dem 768er-Raster, Z 401): Rahmen `Interface\CastingBar\UI-CastingBar-Border.blp`, Füllung `Interface\TargetingFrame\UI-StatusBar.blp` rot, Name links, `hp / max` und Prozent rechts, Schädel `Interface\TargetingFrame\UI-TargetingFrame-Skull.blp` wenn der Boss 10+ Level über dem Klassenlevel steht, Zusatz "Final boss" beim `Finale`, "Enraged" sobald `Enraged`, silbern beim `Rare`. Höchstens zwei Leisten (die jüngsten Bosse), eine gefallene blendet in 1,5 s aus (Deans `bossbar.rs` als Vorbild). Unter den Level-Up-Karten verborgen.
- Ankunft: beim `Added<Enemy>` mit `boss` einmal `GlueSound("RaidWarning")` (geprüft in `SoundEntries.dbc`) und die Leiste gleitet ein. Den Text liefert der vorhandene Banner.
- `MapMarker::Boss` (bzw. `Rare`) auf jeden Boss beim Erscheinen; gezeichnet von Paket C.
- `frames.rs`: Zielrahmen eines `Rare` mit `Interface\TargetingFrame\UI-TargetingFrame-Rare.blp` (geprüft, im Archiv).
- Läuft auch beim Koop-Gast: `Enemy` mit `boss`, `hp`, `max_hp` kommt dort über `enemies::shown_enemy` an.

- [ ] **Step 1: Tests:** `two_bars_show_the_newest_bosses`, `a_fallen_boss_fades_and_goes`, `percent_text_rounds_down` (reine Funktionen über eine Liste `BossInfo { name, hp, max_hp, dead_for, kind }`).
- [ ] **Step 2:** UI bauen, Daten pro Frame aus den Bossen, Nodes nur bei Änderung anfassen (Muster `plates.rs`).
- [ ] **Step 3:** Rare-Rahmen in `frames.rs`.

**Screenshot:** mit Paket A zusammen (`shot arrive`, `shot fight`), dazu `WOW_SURVIVORS_FINALE=12 cargo survivors -- --debug --zone westfall -w --shots /tmp/shots-bar --script "13 shot finale_bar; 14 quit"`.
**README-Zeile:** "Boss bar | A wide health bar at the top for every boss on the field (two at most), with its name, health and a skull for one far above you; it marks the finale and an enraged boss, and the raid-warning horn sounds as a boss arrives." Bild `fork-boss-bar.jpg`.

### Paket C: Wächter-Posten und Minimap (`guards.rs`, `minimap.rs`)

**Files (besitzt):** `guards.rs`, `minimap.rs`.

**Consumes:** `enemies::{spawn_enemy, spawn_spot, Enemy, Elite, Steered, Dying}`, `chests::{spawn_chest, ChestTier, Locked}`, `nav::{NavGrid, SpawnProbe}`, `hero::{arena, Hero, Peer}`, `data::{roster, wave_pool}`, `units::{spawn_object, flat, Fx}`, `minimap::MapMarker`, `debug::active`.

**Produces:**
- `pub(super) fn post_offsets(leash: f32, phase: f32) -> Vec<Vec3>`: 2 Posten bei Leash unter 60, 3 unter 120, sonst 4; Radius `(leash * 0.7).min(100.0)`, gleichmäßig verteilt, gedreht um `phase` (aus dem Run-`Rng`). `pub(super) fn wake_radius(leash) -> f32 = (leash * 0.35).min(14.0)`.
- Ein Posten ist eine Entity `Post { at, members: Vec<Entity>, reward: Entity, cleared_at: Option<f32>, tier }` mit `MapMarker::Post`, `RunEntity`. Er wird gesetzt, sobald die Welt dort da ist (`probe.clear(p)` und `world.terrain_height_under(p).is_some()`, sonst nächster Frame), nicht am ersten Frame.
- Lager: ein Elite (Stufe `roster` bei 40 bis 70 % der Leiter, `elite: true`, bekommt sein Affix von `threats::arm`) und zwei Grunts derselben Stufe, alle mit `Steered` und `Guard { post }`. Der Elite patrouilliert langsam (40 % Tempo) zwischen drei Punkten im Abstand 5 yd um den Posten (`nav.step`, Boden, `MovementState` für die Laufanimation), die Grunts stehen.
- Wecken: ein stehender Held (lokal oder Puppe) im Weckradius, oder ein Lagermitglied hat Schaden genommen → `Steered` weg bei allen, Chatzeile "<Elite> and its guards attack!". Ab da jagen sie mit der Horde.
- Belohnung: zwei von drei Posten eine Truhe `chests::spawn_chest(.., ranks: 3, ChestTier::Post)` mit `Locked`, einer von drei ein Schrein (`spawn_object` 218 `UtherShrinePedestal.mdx` + Säule 1309 `AuraBlue.mdx`, `MapMarker::Shrine`): Berühren heilt voll, gibt `shield = 30 % max_hp` für 20 s und 15 Gold je 10 Level. Fällt das ganze Lager, wird `Locked` entfernt, Chatzeile "The cache at <Ort> is unguarded." Nahe an einer verschlossenen Truhe: Banner "Guarded by <Elite>".
- Wiederkehr (#1121): 180 s nach dem Leeren (und nachdem die Belohnung genommen ist) kommt das Lager wieder, Stufe aus `wave_pool(elapsed)` plus zwei Ränge, neue Belohnung.
- Minimap: `compose` zeichnet nach den roten Punkten jede `MapMarker`-Entity: `Post` als Kreuz/Schädel aus `Interface\TargetingFrame\UI-RaidTargetingIcons.blp` (geprüft, im Archiv; 4x2 Zellen, Schädel unten rechts, Annahme zur Zellenordnung, per Bild prüfen), `Chest` als gelbe Zelle 0 aus `Interface\Minimap\ObjectIcons.blp` (Zelle 0 gelb, 1 rot, 2 grün, 4 blau: gesehen), `Shrine` grün, `Boss` mit `UI-TargetingFrame-Skull.blp`, `Rare` silberblau, `Portal` blau. `Post`, `Boss` und `Portal` außerhalb des Sichtkreises werden an den Rand geklemmt, halb transparent.
- Debug: F10 setzt den Helden 18 yd vor den nächsten Posten (nur `debug::active()`).
- `#[cfg(test)] pub(super) fn sim_systems()`.

- [ ] **Step 1: Tests:**

```rust
#[test]
fn posts_stay_inside_every_leash() {
    for map in data::MAPS {
        let posts = post_offsets(map.leash, 0.3);
        assert!(!posts.is_empty());
        for p in &posts {
            assert!(p.length() <= map.leash - 4.0, "{}", map.name);
            assert!(p.length() > wake_radius(map.leash) + 6.0, "{}: a post wakes at the start", map.name);
        }
    }
}

#[test]
fn posts_keep_apart() { /* jeder Abstand >= 2 * (wake_radius + 3) */ }

#[test]
fn a_camp_wakes_together() { /* reine Funktion: ein getroffenes Mitglied weckt alle */ }

#[test]
fn edge_markers_clamp_to_the_ring() { /* Punkt außerhalb -> auf Radius - Rand, Richtung gleich */ }
```

- [ ] **Step 2:** Posten, Lager, Patrouille, Wecken, Belohnung, Wiederkehr.
- [ ] **Step 3:** Minimap-Marker inklusive Randklemme.

**Screenshot:**
```
cargo survivors -- --debug --class hunter --zone elwynn -w --shots /tmp/shots-posts \
  --script "8 shot minimap_posts; 9 key F10; 11 shot post_idle; 12 hold W 1.2; 14.5 shot post_awake; 30 shot post_cleared; 31 quit"
```
**README-Zeile:** "Guard posts | Elite camps hold fixed posts around the zone: an elite patrols with two guards beside a locked chest or a shrine and stays put until you come near or hit one of them. Clear the camp to open the cache; it is manned again three minutes later. Posts, chests, shrines and bosses show on the minimap, the far ones at its rim." Bild `fork-guard-posts.jpg`.

### Paket D: Drop-Modelle (`dropart.rs`, `chests.rs`)

**Files (besitzt):** `dropart.rs`, `chests.rs` (nach Task 0: nur Aussehen und Öffnen; die Rang-Logik bleibt), `quests.rs` (nur `ITEM_DISPLAY`/`ITEM_SCALE`, optional).

**Consumes:** `units::{object_bundle, chest_bundle, spawn_object, open_object, ObjLook, Still}`, `enemies::{Gem, GemKind}`, `items::{Slot, Quality}`, `chests::ChestTier`, `Run::guid`.

**Produces:**
- `gem_look`, `DropArt`-Aussehen und Säulen nach der **Modell-Tabelle** im Anhang. Der Drop wird selbst das GameObject (`object_bundle` auf die vorhandene Entity, Position und Komponenten bleiben); die farbige Säule ist eine eigene GameObject-Entity `Column { of }` mit `ObjLook`, die mit ihrem Drop verschwindet (`follow_columns`). Beides bekommt `ObjLook` und geht damit von selbst an Koop-Gäste.
- `Hover { spin: f32, bob: f32, lift: f32, phase: f32, applied: Vec3, base_rot: Quat }`, gesetzt auf `Added<ObjLook>` anhand des Displays (so auch beim Gast): `PostUpdate` vor `TransformSystems::Propagate` addiert Hub `lift + bob * sin(t)` und Drehung um Y, `First` zieht genau das wieder ab. Gezogene Gems (`Gem::pulled`) schweben nicht.
- `strip_hulls`: `Added<Collider>` an einer Entity, deren `ObjLook.display` in der Drop-Liste steht → `RigidBody` und `Collider` entfernen (Host und Gast).
- Truhen öffnen sich sichtbar: `spawn_chest` baut mit `chest_bundle` (Typ 3); `open_chests` despawnt nicht mehr sofort, sondern ruft `units::open_object`, spielt `GlueSound("ChestOpen")` (geprüft) und blendet nach 2,5 s mit `DespawnFade` aus. Fehlt der Deckel im Bild, fällt es auf das heutige Sofort-Verschwinden zurück (ein Schalter).
- Die Bevy-`Beacons` fliegen raus.

- [ ] **Step 1: Tests:**

```rust
#[test]
fn every_quality_and_slot_has_a_look() {}

#[test]
fn better_items_stand_in_taller_columns() {
    // Poor/Common: keine Säule; Uncommon < Rare < Epic in Höhe (aus der Tabelle).
}

#[test]
fn hover_offsets_cancel_out() {
    // apply dann restore ergibt die Ausgangs-Transform, auch wenn dazwischen bewegt wurde.
}

#[test]
fn every_drop_display_is_in_the_client_data() {
    let data = benilla_formats::wow_data_or_skip!();
    // load_gameobject_catalog: jede Display-Id der Tabelle hat einen Modellpfad, und die .m2 liegt
    // in der Kette (Deans every_chest_tier_display_is_in_the_client_data als Vorbild).
}
```

- [ ] **Step 2:** Aussehen, Säulen, Schweben, Hüllen entfernen.
- [ ] **Step 3:** Truhen-Deckel.
- [ ] **Step 4 (optional):** Quest-Gegenstand auf 6895 `TugofWar_RedDustBag01` (Beutel, Hülle wird entfernt) statt Irrlicht, wenn er im Bild besser liest.

**Screenshot:**
```
WOW_SURVIVORS_DROPS=30 cargo survivors -- --debug --class mage --zone barrens -w --shots /tmp/shots-drops \
  --script "20 shot drops_a; 28 key F5; 29 shot drops_b; 32 hold D 1.5; 35 shot drops_close; 36 quit"
```
(F5 hält die Wellen an, damit die Drops liegen bleiben; Gott-Modus ist im Debug an.)
**README-Zeile:** "Real loot models | Everything on the ground is a WoW model now: experience as glowing cave crystals (a purple gem for the big ones), gear as a sword, a breastplate or a bag in a column of light in its quality's colour, the power-ups as the battleground runes, and chests whose lids lift as you open them. They hover and turn, and a co-op guest sees them too." Bild `fork-loot-models.jpg`.

### Paket E: Rares und Bonus-Boss (`rares.rs`, `finale.rs`)

**Files (besitzt):** `rares.rs`, `finale.rs`.

**Consumes:** `data::{roster, rung, extras}`, `enemies::{spawn_enemy, spawn_spot, Rare, Enemy, Dying}`, `items::BossDrop`, `units::spawn_object`, `minimap::MapMarker`, `Run::{announce, encore}`.

**Produces:**
- `rares_for(map) -> Vec<EnemyDef>` und `bonus_for(map) -> Vec<EnemyDef>` nach der Tabelle **"Rares und Bonus-Bosse"**: Zahlen aus `data::rung(map, 0.85)` mal 6 (Rare) bzw. aus dem Finale mal 2 (Bonus), Name, Display und Größe aus der Tabelle.
- Rares: ab 3:00 alle 150 bis 240 s mit 60 % ein Rare der Zone (höchstens einer lebt), `spawn_enemy(.., boss: true, elite: false)` + `Rare` + `MapMarker::Rare`, auf `BOSS_RADIUS` um einen stehenden Helden. Banner "A rare creature appears: <Name>!", Chat `ChatKind::Boss`. Zählt nicht in `run.bosses`. `WOW_SURVIVORS_RARE=<s>` legt den ersten fest (nur Debug).
- Bonus-Boss (#1135): `finale_slain` setzt `run.won = true`, spielt den Sieg-Effekt und stellt 8 yd neben den Helden ein Portal (4711 `InstancePortal_Red.mdx`, hüllenfrei, `MapMarker::Portal`). Banner "The way to <Bonus> lies open. Step through within 20 s." Betritt ein stehender Held das Portal (2,5 yd) → `run.encore = true`, Portal weg, Bonus-Boss 25 yd entfernt, Ankunfts-Banner. Läuft die Zeit ab → `GameOver` wie heute. Fällt der Bonus-Boss: Signatur-Beute mit `wear_now`, 200 Gold, Banner, nach 3 s `GameOver`. Fallen alle Helden im Bonus-Kampf, bleibt der Lauf gewonnen.
- Debug: F11 lässt das Finale fallen (nur `debug::active()`), um den Bonus zu fotografieren.

- [ ] **Step 1: Tests:** `every_zone_has_a_bonus_boss`, `rares_are_found_by_def_ref`, `a_rare_is_stronger_than_its_rung`, `the_bonus_outweighs_the_finale`, `the_portal_waits_then_closes` (reine Zeitlogik).
- [ ] **Step 2:** Rares, dann Bonus.
- [ ] **Step 3:** `#[cfg(test)] pub(super) fn sim_systems()` (nur Rares; der Bonus liegt jenseits der Simulator-Zeit).

**Screenshot:**
```
WOW_SURVIVORS_RARE=6 WOW_SURVIVORS_FINALE=14 cargo survivors -- --debug --class paladin --zone elwynn -w --shots /tmp/shots-rare \
  --script "8 shot rare; 15 key F11; 17 shot portal; 18 hold W 1; 21 shot bonus; 26 quit"
```
**README-Zeile:** "Rares and a bonus boss | Named rares of the zone walk out now and then between the bosses, with a silver frame and their own chest. Slay the finale and a portal opens for twenty seconds: step through to face a far stronger world boss (Onyxia, Azuregos, Nefarian ...) for its signature loot; the run stays won either way." Bild `fork-bonus-boss.jpg`.

---

## Welle 2: Paket F, Koop-Sicht (nach A und C committed)

**Files:** `coop/wire.rs`, `coop/host.rs`, `coop/client.rs`, dazu je ein schmaler Hook in `threats.rs` (`pub(super) fn telegraph_visual(..)`, `missile_visual(..)`) und `units.rs` (`ObjLook.go_type`).

- Neue replizierte Komponenten: `NetTelegraph { shape, origin, yaw, secs, t }` und `NetMissile { dir, speed }` auf den Host-Entities (in `tag_world` mit `Replicated`), `MapMarker` repliziert. Registrierung in `wire::protocol` hinten anhängen (Reihenfolge ist Protokoll).
- Der Gast baut daraus die Meshes über `threats::telegraph_visual`/`missile_visual`, ohne Schaden (der Host rechnet).
- `ObjLook` bekommt `go_type: u32` (Standard 5), `build_objects` nutzt `chest_bundle` für Typ 3; der Deckel öffnet beim Gast, wenn der Host den Zustand schickt (`NetGoState` oder Despawn-Fade, je nach Aufwand).
- Test in `wire.rs`: `a_telegraph_round_trips`. Zwei Instanzen wie in Koop-Stufe 1, Task 6: auf den Gast-Screenshots sind Kreise, Kegel und Geschosse zu sehen.
- Schließt den offenen Punkt aus #1118. **README:** bestehende Koop-Zeile um einen Satz ergänzen, kein eigenes Bild.

---

## Integration (Orchestrator, nach Welle 1)

- [ ] **Step 1:** Diffs der fünf Pakete lesen, Konflikte an den Schnittstellen beheben.
- [ ] **Step 2 (nach Freigabe):** `cargo survivors`, Warnungen weg; `CC=gcc cargo test --profile play -p survivors-app --lib`.
- [ ] **Step 3:** `balance.rs::sim_app` nimmt `bosses::sim_systems()`, `guards::sim_systems()`, `rares::sim_systems()` in die zweite Kette auf (nach `threats::sim_systems`), `guards` braucht `NavGrid` und `ColliderTrees` (sind da). `the_run_keeps_its_rhythm` muss unverändert grün sein (die Bosse kommen weiter um 2:15, Rares erst ab 3:00, `run.bosses` zählt nur Uhr und Finale). Zusätzliche Annahme in den Test: bei 2:30 hat der erste Boss mindestens eine Kit-Fähigkeit gewirkt, wenn die Zone (The Barrens, `map: 0`) ein Kit hat. Dann `cargo survivors-sim` vor und nach der Runde vergleichen: Median-Überlebenszeit je Klasse höchstens 10 % schlechter, sonst Schaden-Anteile der Kits senken (nicht die Vorwarnung).
- [ ] **Step 4:** Spiel laufen lassen mit den Screenshot-Skripten der Pakete, Bilder ansehen: Wächter stehen, Minimap-Marker am Rand, Drops schweben und haben Modelle, Säulen in Qualitätsfarbe, Truhendeckel. Ein Lauf `WOW_SURVIVORS_AUTO=warlock WOW_SURVIVORS_DROPS=20` 5 Minuten: Held bleibt nirgends hängen, im Log höchstens 4 Nav-Rescans.
- [ ] **Step 5:** Ein Commit je Paket mit README-Zeile oben in "What this fork adds", Bild als JPEG 1600 px breit unter `docs/screenshots/fork-<feature>.jpg`, Kopie in `~/.claude/screenshots/azeroth-survivors-re/`. Reihenfolge: D (`Survivors loot: real WoW models for every drop, hovering and turning`), C (`Survivors guard posts: elite camps at fixed posts, and markers on the minimap`), A (`Survivors bosses: a script per boss with cones, lines, rings, adds and charges`), B (`Survivors boss bar: health at the top, the raid warning on arrival, the rare frame`), E (`Survivors rares and a bonus boss behind a portal after the finale`).
- [ ] **Step 6:** Kanboard: #1121 und #1135 nach Done mit Commit-Hashes; #1134 kommentieren (Skript-Bibliothek liegt, Raid-Stufe als eigener Plan).

---

## Anhang 1: Modell-Tabelle für Drops (GameObjectDisplayInfo, geprüft)

Größe = Header-Bounds der M2 in Yards (bei Partikelmodellen mit Partikeln), Hülle = Kollisionsdreiecke. Skalen sind Startwerte für den Screenshot-Abgleich.

| Verwendung | Display | Modell | Größe | Hülle | Start-Skala, Schweben |
|---|---|---|---|---|---|
| Erfahrung klein (unter 5) | 5091 | `World\Dungeon\Cave\PassiveDoodads\Crystals\CaveMineCrystalFormation01.mdx` (Textur `CAVECRYSTAL01_64.BLP`, cyan leuchtend, gesehen) | 0,7 x 0,7 x 1,0 | 0 | 0,6, Hub 0,5, dreht |
| Erfahrung mittel (5 bis 19) | 5092 | `..\CaveMineCrystalFormation03.mdx` (dieselbe Textur) | 1,1 x 1,1 x 1,4 | 0 | 0,7, Hub 0,5, dreht |
| Erfahrung groß (20+) | 3511 | `World\KhazModan\Blackrock\PassiveDoodads\MuseumGem\MuseumGem01.mdx` (lila Edelstein in Goldfassung, gesehen) | 0,9 x 0,9 x 0,8 | 8 (wird entfernt) | 1,1, Hub 0,7, dreht |
| Gold | 2614 | `World\Generic\PassiveDoodads\TreasurePiles\GoldPilesmall01.mdx` (wie heute) | 1,9 x 1,9 x 0,3 | 35 (wird entfernt) | 0,45 bis 1,4 wie heute, liegt |
| Boss-Gold zusätzlich | 1268 | `World\Generic\PassiveDoodads\ParticleEmitters\AuraYellowShort.mdx` | 2,0 x 2,0 x 1,6 | 0 | 1,0 |
| Heilung (Kill) | 114 | `..\Food&Utensils\TurkeyLeg.mdx` (wie heute) | 0,9 x 0,4 x 0,4 | 0 | 2,5, Hub 0,5, dreht |
| Heilung (Boss) | 5991 | `World\Generic\PVP\Runes\PVP_Rune_Restoration.mdx` (Schlachtfeld-Rune) | 9,0 (Partikel) | 0 | 0,6, Hub 0,4, dreht |
| Splitter Hearthstone | 342 | `World\Goober\G_RuneBlue01.mdx` (schwebende blaue Rune) | 0,8 x 1,7 x 1,7 | 0 | 1,2, Hub 0,8, dreht |
| Splitter Divine Intervention | 6671 | `World\Goober\G_HolyLightWell.mdx` (Lichtbrunnen) | 1,1 x 1,6 x 4,8 | 0 | 0,6, steht |
| Splitter Frost Nova | 5932 | `World\Generic\PVP\Runes\PVP_Rune_Invis.mdx` (schwarzes Juwel, Schneeflocken) | 2,9 x 4,6 x 4,6 | 0 | 0,6, Hub 0,4, dreht |
| Ausrüstung Waffe | 670 | `World\Generic\Human\Passive Doodads\Weapons&Armor\HumanSword02.mdx` | 2,2 x 0,1 x 0,5 | 0 | 0,9, Hub 0,6, dreht |
| Ausrüstung Kopf, Schulter, Brust, Beine | 4074 | `WORLD\GENERIC\HUMAN\PASSIVE DOODADS\ARMOR\ARMORBREASTPLATEBLUE.MDX` | 0,5 x 0,6 x 0,6 | 0 | 2,2, Hub 0,6, dreht |
| Ausrüstung Ring, Hals, Schmuck | 3511 | `MuseumGem01.mdx` (wie oben, kleiner) | 0,9 | 8 (wird entfernt) | 0,7, Hub 0,7, dreht |
| Ausrüstung Rücken, Taille, Hände, Füße | 6895 | `World\Generic\PassiveDoodads\TugofWar\TugofWar_RedDustBag01.mdx` (Beutel) | 1,2 x 1,0 x 1,0 | 52 (wird entfernt) | 0,8, liegt |
| Säule Uncommon | 3612 / 3013 | `..\ParticleEmitters\AuraGreen.mdx` / `AuraGreenTall.mdx` | 2,0 x 2,0 x 3,1 / Tall | 0 | 1,0 |
| Säule Rare | 298 | `..\ParticleEmitters\AuraBlueTall.mdx` | 2,0 x 2,0 x 6,8 | 0 | 1,0 |
| Säule Epic | 2234 | `..\ParticleEmitters\AuraPurpleVeryTall.mdx` | 2,0 x 2,0 x 12,0 | 0 | 1,0 |
| Säule Truhe | 1310 | `..\ParticleEmitters\AuraYellow.mdx` | 2,0 x 2,0 x 3,1 | 0 | 1,0 |
| Truhe Elite | 259 | `World\SkillActivated\Containers\TreasureChest01.mdx` (wie heute, 6 Animationen) | 2,5 x 1,9 x 1,7 | 66 (bleibt) | 1,0 |
| Truhe Boss, Rare | 1447 | `World\Generic\ActiveDoodads\Chests\ChestActive01.mdx` (Funkeln, `SPARKLE.BLP`) | 3,2 x 3,2 x 3,1 | 12 (bleibt) | 0,8 |
| Truhe Finale, Bonus | 6502 | `World\Goober\G_ChestAhnQiraj.mdx` | 1,3 x 2,9 x 1,8 | 12 (bleibt) | 1,6 |
| Truhe Wächter-Posten | 41 | `World\Generic\ActiveDoodads\Chest03\Chest03.mdx` (eisenbeschlagen) | 1,4 x 1,2 x 1,2 | 12 (bleibt) | 1,8 |
| Schrein Wächter-Posten | 218 | `World\Lordaeron\TirisfalGlade\PassiveDoodads\UtherShrine\UtherShrinePedestal.mdx` + Säule 1309 `AuraBlue.mdx` | 0,6 x 1,1 x 1,2 | 12 (bleibt) | 2,0 |
| Portal Bonus-Boss | 4711 | `World\Generic\ActiveDoodads\InstancePortal\InstancePortal_Red.mdx` | 9,3 x 9,3 x 11,9 | 0 | 0,5 |

**Annahmen, im Bild zu prüfen:** dass die `Aura*`-Partikel in der Farbe ihres Namens leuchten (alle vier nutzen dieselbe Textur `SMOKEWISPY01.BLP`, die Farbe kommt aus den Partikelfarben); dass `HumanSword02` flach liegt (Bounds 0,5 hoch sprechen dafür); dass Typ 3 bei `TreasureChest01`, `ChestActive01`, `G_ChestAhnQiraj` und `Chest03` den Deckel hebt. Ausrüstungs-Weltmodelle aus `ItemDisplayInfo.dbc` (die echte Waffe des gedroppten Gegenstands) bräuchten einen neuen Engine-Weg (ein Item-Display ohne Träger); Rüstung, Schmuck und Beutel haben dort ohnehin nur Icons (`INV_Misc_Bag_*`, `INV_Chest_*`, Modellspalten leer, geprüft). Deshalb die GameObject-Doodads oben.

## Anhang 2: Boss-Kits (Paket A)

Vokabular: **Kreis** (unter dem Helden), **Kegel** (vom Boss zum Helden), **Linie/Ansturm** (`Charge`), **Ring** (Donut um den Boss, sicher ist nah dran), **Stampfer** (Kreis um den Boss), **Salve** (Ring) und **Fächer** (zum Helden), **Bomben** (mehrere Kreise um den Helden mit brennender Fläche), **Adds**, **Raserei**. Spell-Ids geprüft gegen `Spell.dbc`: Cleave 15284, Flame Breath 16390, Frost Breath 16099, Charge 22120, War Stomp 45, Frenzy 19451, Wrath of Ragnaros 20566, Lava Bomb 20474, Flamestrike 2120 (Fläche), Fire Nova 11969, Shadow Bolt Volley 15245, Frostbolt Volley 22643, Fireball Volley 15243, Poison Bolt Volley 24099, Thrash 21919, Enrage 8599 (heute schon in `enrage.rs`, damit bestätigt), Shadow Bolt 9613 (heute in `threats.rs`, bestätigt). Sound-Kits geprüft in `SoundEntries.dbc`: `RaidWarning`, `ChestOpen`, `A_Edwin VanCleef Aggro01`, `A_Edwin VanCleef Health01`, `A_RagnarosArrival01`, `A_RagnarosSpecialAttack01`, `A_HakkarAggro`, `A_BloodlordMandokirAggro`, `Ossirian - Agro`, `A_NefarianAggro`, `A_Mr Smite Alarm01`. Die Ruf-Texte sind aus dem Gedächtnis (Annahme).

| Zone | Boss | Mechaniken | Adds (in `extras`) | Signatur-Beute (Annahme: Namen echter Drops) |
|---|---|---|---|---|
| Elwynn | Mother Fang | Adds; Kreis | 4 x "Mine Spider" (Display 382) | |
| Elwynn | Narg the Taskmaster | Adds; Stampfer 45 | 3 x "Kobold Laborer" (373) | |
| Elwynn (Finale) | Hogger | Adds bei 70 % und 35 %; Ansturm 22120 | 3 x "Riverpaw Runt" (383) | "Huge Gnoll Claw" (Trinket, Uncommon) |
| Elwynn | Thuros Lightfingers | Fächer 5 Dolche (Arrow) | | |
| Elwynn | Fedfennel | Kegel 15284 | | |
| Elwynn | Gruff Swiftbite | Ansturm; Raserei 19451 | | |
| Westfall | Brainwashed Noble | Salve 15245; Kreis | | |
| Westfall | Marisa du'Paige | Salve 22643; Ring 11969 | | |
| Westfall | Sergeant Brashclaw | Kegel 15284; Adds | 2 x "Riverpaw Brute" (384) | |
| Westfall | Klaven Mortwake | Fächer 5 Dolche | | |
| Westfall | Lupos | Ansturm; Raserei | | |
| Westfall | Vultros | Ansturm (Sturzflug) | | |
| Westfall (Finale) | Edwin VanCleef | Fächer 7 Dolche; Adds bei 50 % mit Ruf `A_Edwin VanCleef Health01`; Ankunft `A_Edwin VanCleef Aggro01` "None may challenge the Brotherhood!" | 3 x "Defias Blackguard" (2331) | "Cruel Barb" (Weapon, Rare), "Cape of the Brotherhood" (Back, Rare) |
| The Barrens | Echeyakee | Ansturm; Raserei | | |
| The Barrens | Takk the Leaper | Ansturm (Sprung) | | |
| The Barrens | Lakota'mani | Stampfer 45 | | |
| The Barrens | Humar the Pridelord | Kegel; Raserei | | |
| The Barrens (Finale) | Agathelos the Raging | Ansturm zweimal; Stampfer | | |
| The Barrens | Hezrul Bloodmark | Fächer (Arrow); Stampfer | | |
| Durotar | Geolord Mottle | Salve 24099; Bomben | | |
| Durotar | Burning Blade Cultist | Adds | 3 x "Vile Familiar" (1185) | |
| Durotar (Finale) | Death Flayer | Kegel (Gift); Ansturm | | |
| Fire Plume Ridge (Finale) | Ragnaros | Bomben 20474 mit Fläche 2120; Stampfer 20566 "Wrath of Ragnaros" mit Ruf `A_RagnarosSpecialAttack01`; Adds bei 50 %; Ankunft `A_RagnarosArrival01` | 4 x "Son of Flame" (1204) | "Perdition's Blade" (Weapon, Epic) |
| Fire Plume Ridge | Baron Charr | Bomben | | |
| Fire Plume Ridge | King Mosh | Stampfer; Ansturm | | |
| Gurubashi Arena | Bloodlord Mandokir | Ansturm; Kegel; Ankunft `A_BloodlordMandokirAggro` | | |
| Gurubashi Arena | Jin'do the Hexxer | Salve 15245; Adds | 3 x "Shade of Jin'do" (Display aus dem Roster) | |
| Gurubashi Arena (Finale) | Hakkar | Salve 15245; Bomben; Ankunft `A_HakkarAggro` | | |
| Gates of Ahn'Qiraj | Kurinnaxx | Bomben (Sandfallen) | | |
| Gates of Ahn'Qiraj | Moam | Ring | | |
| Gates of Ahn'Qiraj | Princess Huhuran | Raserei; Salve 24099 | | |
| Gates of Ahn'Qiraj (Finale) | Ossirian the Unscarred | Stampfer; Salve (Nature); Ankunft `Ossirian - Agro` | | |
| Naxxramas | Patchwerk | Kegel 21919; Raserei | | |
| Naxxramas | Gluth | Adds; Raserei | 4 x "Zombie Chow" (Display aus dem Roster) | |
| Naxxramas (Finale) | Kel'Thuzad | Salve 22643; Bomben (Schattenrisse); Ring | | "Might of Menethil" (Weapon, Epic) |
| Mount Hyjal | Taerar, Lethon, Emeriss | Kegel 16390 (Atem); Bomben | | |
| Mount Hyjal (Finale) | Ysondre | Kegel; Salve (Nature) | | |

Zonen und Bosse ohne Zeile behalten die heutige Kreis-/Salven-Folge. Die Agenten ergänzen nach demselben Muster, Add-Displays immer aus einem Zonen-Roster (dann sind sie geprüft, Anhang 4).

## Anhang 3: Rares und Bonus-Bosse (Paket E)

Displays der Bonus-Bosse geprüft (`CreatureDisplayInfo` → Modell): Onyxia 8570 `DragonOnyxia.mdx`, Azuregos 11460 `DragonAzurgoz.mdx`, Nefarian 11380 `DragonNefarian.mdx`, Chromaggus 14367 `Drakeadon.mdx`, Gahz'ranka 15288 `Hydra.mdx`, Viscidus 15686 `GiantSlime.mdx`, Baron Rivendare 10729 `MountedDeathKnight.mdx`, Taragaman 7970 `FelGuard.mdx`, Rhahk'Zor 14403 `Ogre.mdx`, Sneed's Shredder 1269 `GoblinShredder.mdx`, Gilnid 7125 `GoblinMale.mdx`+Extra, Mr. Smite 2026 `TaurenMale.mdx`+Extra, Captain Greenskin 7113 `GoblinMale.mdx`+Extra, Cookie 1305 `Murloc.mdx`. Rare-Namen sind Classic-Rares der Zone aus dem Gedächtnis (Annahme), ihre Displays geliehen aus dem Zonen-Roster (damit geprüft) oder oben geprüft.

| Zone | Rares | Bonus-Boss |
|---|---|---|
| Elwynn Forest | Princess (Eber, 503, x1,5) | Onyxia (8570) |
| Westfall | Rhahk'Zor (14403), Sneed's Shredder (1269), Mr. Smite (2026), Captain Greenskin (7113), Cookie (1305) als Deadmines-Minibosse | Onyxia |
| Duskwood, Redridge, Tirisfal, Dun Morogh | je zwei aus der Zone, Displays aus dem eigenen Roster | Onyxia |
| The Barrens | Owatanka (Donnerechse, 2764), Brokespear (Stacheleber, 1253), Sister Rathtalon (Harpyie, 2163) | Azuregos (11460) |
| Durotar | Taragaman the Hungerer (7970), Gazz'uz (Brennende Klinge, 4194) | Azuregos |
| Mulgore, Teldrassil | je zwei aus dem Roster | Azuregos |
| Fire Plume Ridge | aus dem Roster | Nefarian (11380) |
| The Dark Portal | aus dem Roster | Chromaggus (14367) |
| Gurubashi Arena | aus dem Roster | Gahz'ranka (15288) |
| Gates of Ahn'Qiraj | aus dem Roster | Viscidus (15686) |
| Mount Hyjal | aus dem Roster | Nefarian |
| Naxxramas | aus dem Roster | Baron Rivendare (10729) |

## Anhang 4: Alle 96 Roster-Bosse aufgelöst (Stand `data.rs`)

Jede Display-Id der heutigen Boss-Listen hat eine `CreatureDisplayInfo`-Zeile und ein passendes Modell, z. B. Mother Fang 2541 `MineSpider.mdx`, Narg 774 `Kobold.mdx`, Hogger 384 `GnollMelee.mdx`, Thuros 3341 `HumanMale.mdx`+Extra, Lupos 11412 `DireWolf.mdx`, Vultros 507 `CarrionBird.mdx`, Akubar 10920 `LostOne.mdx`, Hel'nurath 14556 `DreadLord.mdx`, Moam 15392 `ObsidianDestroyer.mdx`, Huhuran 15739 `SilithidWaspBoss.mdx`, King Mosh 5305 `trex.mdx`, Mandokir 11288 `TrollMale.mdx`+Extra, Patchwerk 16174 `FleshGolem.mdx`, Thaddius 16137 `FleshTitan.mdx`, Emeriss 15366 `Dragon.mdx`, Agathelos 2450 `Boar.mdx`. Auffällig, aber gewollt: Fedfennel und Gruff Swiftbite teilen 175, Old Icebeard und Vagash teilen 1050, Lupos steht in Westfall und Duskwood. Nachrechnen mit dem Test `every_kit_names_a_boss_that_exists` und einem datengestützten `every_boss_display_has_a_model` (`load_creature_catalog`, `display_models`) in Paket A.

---

## Aufwand

| Teil | Wer | Schätzung |
|---|---|---|
| Task 0 Grundlage | Orchestrator | 2 bis 3 h |
| A Boss-Mechaniken | Agent | 1 Agententag (größtes Paket) |
| B Boss-Leiste | Agent | 3 h |
| C Wächter-Posten + Minimap | Agent | 5 bis 6 h |
| D Drop-Modelle | Agent | 4 bis 5 h |
| E Rares + Bonus-Boss | Agent | 4 h |
| Integration, Simulator, Bilder, Commits | Orchestrator | 3 bis 4 h |
| F Koop-Sicht (Welle 2) | Agent | 4 h |

Welle 1 läuft parallel: etwa ein Arbeitstag Wanduhr plus Integration; mit Welle 2 anderthalb Tage.

## Offene Fragen an Michael

1. **Molten Core (#1134) in dieser Runde oder als eigener Plan?** Empfehlung: eigener Plan danach. Map 409 lädt laut Deans Spike, braucht aber eine deckenbewusste Bodensonde, Lava-Schaden und einen Kamera-Check; diese Runde legt die Skript-Bibliothek, auf der Lucifron und Magmadar dann sitzen.
2. **Bonus-Boss: freiwillig per Portal nach dem Finale (20 s) oder automatisch?** Empfehlung: Portal, der Lauf ist schon gewonnen, wer will, holt sich die Signatur-Beute. Die Zuordnung Zone → Bonus-Boss (Onyxia, Azuregos, Nefarian ...) in Anhang 3 ist ein Vorschlag.
3. **Wächter-Belohnung: Truhe, Schrein oder beides?** Empfehlung: beides, zwei von drei Posten eine verschlossene Truhe (3 Ränge), einer ein Schrein (volle Heilung, Schild, Gold); das Lager kehrt nach 3 Minuten zurück.
4. **Erfahrung als Kristalle statt Irrlichter?** Empfehlung: ja (cyan Höhlenkristall, groß ein lila Edelstein); sie haben keine Partikel und sind damit billiger als die Irrlichter. Die Lichtsäule nur ab Grün, damit graue und weiße Ware das Feld nicht zustellt.
