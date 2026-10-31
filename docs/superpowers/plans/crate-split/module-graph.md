# Survivors module graph and crate split proposal

Read-only analysis of `crates/benilla-app/src/survivors/` at `features` (2c6dd94 plus the two untracked
files): 34 files (33 modules plus `mod.rs`), 21,540 lines, 17 test modules with 82 tests. It feeds
Task 1 and Task 3 of `docs/superpowers/plans/2026-10-31-survivors-crate-split.md`.

## Summary

- The module graph is one big cycle. Without the registration edges of `mod.rs` and without `auto.rs`,
  23 of 34 modules form one strongly connected component (SCC); with them, 31 of 34 do (only `data`,
  `fill`, `grade` are outside). The cycle is held together by 28 edges, almost all of which point
  from "model" code into UI or system code (`hud::HudFont`, `enemies::Enemy`, `abilities::Hit`, ...).
- Every one of those edges is fixed by moving a well-defined set of items into
  `survivors-core` (135 items outside the modules that are core wholesale, about 2,560 lines of
  which 830 are the talent trees and rules, 540 the `NavGrid`, 260 the `mod.rs` types; listed
  under "Items to move into core"), plus three small design changes: four `SystemSet`s for ordering
  across crates, a `BossPlans` resource, and an `Autopilot` resource. After that the graph of the
  leaf crates has no leaf-to-leaf edge at all (checked item by item with a script, see "Method").
- Proposed crates (lines include tests): **core ~6,300**, hud ~3,500, meta ~3,100, windows ~1,900,
  overlays ~1,850, loot ~1,470, combat ~1,440, world ~1,180, bosses ~290, app (aggregator + `auto`) ~490.
  This differs from the starting point in three places: `ui` is split in three (hud, windows,
  overlays) because it would be 7,100 lines on its own; the talent and trainer rules and tables, the
  whole `NavGrid`, the `Mobility` table and the boss-plan type go to core, not to a leaf; `evolutions`
  is a window, not combat.
- First leaf to extract after core: **bosses** (`finale.rs`). After the prep it has no incoming edge
  from another leaf, 290 lines, and it is the example the plan was written for.

## Method

- Resolved with a script (`/tmp/sv-analysis`, not committed): parse every `use super::...`,
  `use crate::survivors::...` (none exist), inline `super::x::Item`, `crate::survivors::x::Item`,
  module aliases (`hud::keys`) and root items (`super::Run`), after stripping comments and string
  and char literals. A reference is an identifier occurrence, so counts are references, not
  distinct call sites. Test code is counted separately (`+Nt`).
- The second pass builds an item-level graph: 953 top-level items (struct, enum, fn, const,
  static, impl blocks attached to their type), 2,060 references between them. A proposed
  assignment of items to crates is then checked: everything a core item references must itself be
  core (closure), and no item outside core may reference an item of another leaf.
- Limits: references through trait methods or method calls on a type are not names, so a
  moved type's private methods and fields are checked separately ("Visibility breaks"). Name
  collisions inflate a few counts (`Fx` exists twice, see below). I did not run cargo; the compiler
  will list any miss precisely, which is why the visibility section is a prediction.

## Edges

Module-level graph, one row per ordered pair. `mod (root)` is `mod.rs`. "refs" counts non-test
references; `+Nt` are references in the file's `#[cfg(test)]` module. Items from `mod.rs` that it
re-exports from `data` (`Stat`, `AbilityDef`, ...) are shown as `data`. Rows with `mod (root)` as the
source and a `plugin` or a system as the item are the registration edges; the others are type uses
(`Run` naming `MetaBonus`, `MobilityState`, ...).

| from | to | items | refs |
|---|---|---|---|
| abilities | chatlog | `ChatKind`, `ChatLine` x2 | 3 |
| abilities | data | `AbilityDef` x2, `Effect` x15, `Kind` x23 | 40 |
| abilities | enemies | `Dot`, `Enemy` x4, `Loot` x2, `mark_dying` | 8 |
| abilities | focus | `Focus` x10 (+10 in tests), `Target` | 11 (+10t) |
| abilities | hero | `Hero`, `reach` | 2 |
| abilities | mod (root) | `Rng` x3, `Run` x4, `RunEntity` | 8 |
| abilities | units | `COLOR_HEAL`, `COLOR_MELEE` x2, `COLOR_SPELL` x5, `FACTION_FRIENDLY`, `Fx` x3, `Marks`, `UnitSpec`, `flat` x9, `kill_unit` x2, `spawn_area_object`, `spawn_unit` | 27 |
| auto | data | `CLASSES`, `MAPS` x2, `MAX_ABILITY_LEVEL`, `VICTORY_SECS`, `ability` | 6 |
| auto | enemies | `Enemy` x2, `Gem` | 3 |
| auto | hero | `Hero` x4, `arena` | 5 |
| auto | hud | `Offer`, `pick`, `start_run` | 3 |
| auto | mod (root) | `Phase` x8, `Rng`, `Run`, `Selection` x2 | 12 |
| auto | profile | `Profile` | 1 |
| auto | talents | `TalentsUi` | 1 |
| auto | units | `flat` x3 | 3 |
| barrier | hero | `Hero` x2, `arena` | 3 |
| barrier | mod (root) | `Phase` x3, `RunEntity`, `Selection` | 5 |
| cards | data | `AbilityDef` x2, `Kind` x19, `MAX_ABILITY_LEVEL`, `MAX_PASSIVE_LEVEL` x2, `ability` | 25 |
| cards | fill | `FillMaterial` x2 | 2 |
| cards | hud | `Choice` x8, `DIM_GREY`, `GOLD_TONE` x2, `HudFont`, `Offer` x3, `OfferRow`, `SpellIcon`, `centered_bundle` x3, `describe_card`, `pick`, `text_bundle` x2 | 24 |
| cards | mod (root) | `Phase` x5, `Rng`, `Run` x4 | 10 |
| charpanel | hud | `GOLD_TONE`, `HudFont`, `grouped`, `num` x5, `pct` x7, `spell_name` | 16 |
| charpanel | items | `gear_lines` | 1 |
| charpanel | mod (root) | `Phase` x6, `Run` x3, `Selection` | 10 |
| charpanel | profile | `Profile` | 1 |
| chatlog | hud | `HudFont`, `grouped` x2, `spell_name` | 4 |
| chatlog | mod (root) | `Phase` x6, `Run` | 7 |
| chests | chatlog | `ChatKind`, `ChatLine` x4 | 5 |
| chests | data | `MAX_ABILITY_LEVEL` (+1 in tests), `MAX_PASSIVE_LEVEL` (+1 in tests) | 2 (+2t) |
| chests | enemies | `Dying`, `Elite`, `Enemy` | 3 |
| chests | finale | `Finale` | 1 |
| chests | hero | `Hero` | 1 |
| chests | hud | `spell_name` | 1 |
| chests | items | `drop_boost` | 1 |
| chests | mod (root) | `Phase`, `Rng` x2, `Run` x2 | 5 |
| chests | pickups | `Beacons` | 1 |
| chests | units | `Fx`, `flat`, `spawn_object` | 3 |
| difficulty | chatlog | `ChatLine` x2 | 2 |
| difficulty | hud | `HudFont`, `MenuRoot`, `OverPayout`, `keys`, `num`, `refresh_results` | 6 |
| difficulty | menu | `MenuBuilt` | 1 |
| difficulty | mod (root) | `Phase` x5, `Run` x2 | 7 |
| difficulty | profile | `Profile` x2 | 2 |
| enemies | abilities | `Hit` x2 | 2 |
| enemies | chatlog | `ChatKind`, `ChatLine` x7 | 8 |
| enemies | data | `BOSS_EVERY_SECS` x3, `EnemyDef` x4, `XP_RATE`, `damage_scale`, `hp_scale`, `progress` x2, `roster`, `target_alive`, `wave_pool`, `xp_to_next` x2 | 17 |
| enemies | finale | `plan` | 1 |
| enemies | hero | `HERO_RADIUS`, `Hero` x4, `hurt_hero` | 6 |
| enemies | items | `drop_boost` | 1 |
| enemies | mobility | `LEARN_LEVEL`, `for_class` | 2 |
| enemies | mod (root) | `Phase` x2, `Rng` x4, `Run` x5, `Selection` | 12 |
| enemies | nav | `NavGrid` x3, `SpawnProbe` x3 | 6 |
| enemies | units | `COLOR_HURT`, `FACTION_MONSTER`, `Fx`, `Marks` x2, `UnitSpec`, `flat` x3, `spawn_object`, `spawn_unit`, `yaw_toward` x2 | 13 |
| evolutions | cards | `LEGENDARY` | 1 |
| evolutions | chatlog | `ChatKind`, `ChatLine` x2 | 3 |
| evolutions | data | `AbilityDef` (+1 in tests), `CLASSES` (+3 in tests), `EVOLUTIONS` x4 (+3 in tests), `EVOLVED_LEVEL` (+1 in tests), `Evolution` x3 (+1 in tests), `MAX_ABILITY_LEVEL` x3 (+1 in tests), `MAX_PASSIVE_LEVEL` (+1 in tests), `PASSIVES` (+1 in tests), `ability` (+4 in tests), `evolved_name` (+1 in tests) | 12 (+17t) |
| evolutions | hud | `GOLD_TONE`, `HudFont`, `spell_name` | 3 |
| evolutions | mod (root) | `Owned` (+1 in tests), `Phase` x8, `Run` x5 | 13 (+1t) |
| finale | chatlog | `ChatKind` x2, `ChatLine` x3 | 5 |
| finale | data | `BOSS_EVERY_SECS`, `EnemyDef` x7 (+2 in tests), `MAPS`, `roster` | 10 (+2t) |
| finale | enemies | `Dying`, `Enemy`, `spawn_enemy` x2, `spawn_spot` | 5 |
| finale | hero | `Hero` x2 | 2 |
| finale | mod (root) | `Phase` x4, `Rng`, `Run` x2, `Selection` | 8 |
| finale | nav | `NavGrid`, `SpawnProbe` | 2 |
| finale | units | `Fx` | 1 |
| focus | hud | `GOLD_TONE` x3, `HudFont` | 4 |
| focus | mod (root) | `Phase` x8 | 8 |
| frames | data | `xp_to_next` | 1 |
| frames | enemies | `Enemy` x3, `move_enemies` | 4 |
| frames | focus | `Target` x4 | 4 |
| frames | hero | `AimPoint`, `UiCover`, `place_camera` | 3 |
| frames | hud | `GOLD_TONE` x8, `HudFont`, `UiArt`, `grouped` x4 | 14 |
| frames | mod (root) | `Phase` x8, `Run` x2, `Selection` | 11 |
| frames | profile | `Profile` | 1 |
| frames | units | `flat` | 1 |
| hero | auto | `Auto`, `steer` | 2 |
| hero | data | `ClassDef`, `MapDef`, `VICTORY_SECS` | 3 |
| hero | mod (root) | `Phase` x10, `Run` x3, `Selection` x2 | 15 |
| hero | units | `FACTION_FRIENDLY`, `Fx`, `Marks` x2, `UnitSpec`, `flat` x2, `kill_unit`, `spawn_unit`, `wow_to_bevy`, `yaw_toward` | 11 |
| hud | cards | `LEGENDARY`, `Rays` | 2 |
| hud | data | `AbilityDef` x4, `CLASSES` x2, `ClassDef`, `EVOLVED_LEVEL` x3, `Effect` x9, `Evolution`, `Kind` x44, `MAX_ABILITIES`, `MAX_ABILITY_LEVEL` x6, `MAX_PASSIVES`, `MAX_PASSIVE_LEVEL` x4, `PASSIVES` x2, `PassiveDef` x2, `Stat` x35, `ability` x5, `evolved_name`, `xp_to_next` | 122 |
| hud | evolutions | `evolved_from`, `ready` | 2 |
| hud | fill | `FillMaterial` x3 | 3 |
| hud | hero | `Hero` x2, `arena`, `spawn_hero` | 4 |
| hud | minimap | `MiniMap`, `spawn_cluster` | 2 |
| hud | mod (root) | `Owned` x5, `Phase` x35, `Rng` x3, `Run` x15, `Selection` x6, `Stats` x4 | 68 |
| hud | options | `OptionsUi` | 1 |
| hud | profile | `Award`, `MAX_LEVEL`, `Profile` x3, `xp_to_next` | 6 |
| hud | talents | `TalentsUi` | 1 |
| hud | trainer | `TrainerUi` | 1 |
| items | chatlog | `ChatKind` x2, `ChatLine` x8 | 10 |
| items | data | `progress` | 1 |
| items | enemies | `Dying`, `Elite`, `Enemy` | 3 |
| items | finale | `Finale` | 1 |
| items | hero | `Hero` x3 | 3 |
| items | hud | `UiArt` | 1 |
| items | mod (root) | `Phase` x7, `Rng` x2 (+2 in tests), `Run` x4, `RunEntity` | 14 (+2t) |
| items | mods | `StatMods` x5 | 5 |
| items | pickups | `Beacons` | 1 |
| items | units | `flat` | 1 |
| menu | data | `CLASSES` x3, `MAPS` x5, `roster` | 9 |
| menu | hud | `HudFont` x2, `MenuRoot`, `looks_for`, `rgb`, `start_run` | 6 |
| menu | mod (root) | `Phase` x5, `Rng`, `Run`, `Selection` x9 | 16 |
| menu | profile | `MAX_LEVEL`, `Profile` x5, `xp_to_next`, `zone_min_level` | 8 |
| minimap | enemies | `Enemy` | 1 |
| minimap | hero | `Hero`, `MINUTE_OF_DAY` | 2 |
| minimap | hud | `UiArt` | 1 |
| minimap | mod (root) | `Run` | 1 |
| mobility | enemies | `Enemy` | 1 |
| mobility | hero | `AimPoint`, `Hero` x2, `arena`, `reach` x3 | 7 |
| mobility | mod (root) | `Phase` x2, `Run` x5 | 7 |
| mobility | units | `Fx`, `flat` x6, `yaw_toward` x3 | 10 |
| mod (root) | abilities | `Hit`, `PendingHits` x2, `apply_hits`, `cast_abilities`, `tick_areas` | 6 |
| mod (root) | auto | `drive`, `from_env`, `plan` | 3 |
| mod (root) | barrier | `plugin` | 1 |
| mod (root) | cards | `PendingPick`, `animate_cards`, `choose_card`, `deal_cards` | 4 |
| mod (root) | charpanel | `plugin` | 1 |
| mod (root) | chatlog | `plugin` | 1 |
| mod (root) | chests | `plugin` | 1 |
| mod (root) | data | `AbilityDef`, `CLASSES` x2, `ClassDef` x2, `MAPS` x4, `MapDef` x2, `PassiveDef`, `Stat` x10 | 22 |
| mod (root) | difficulty | `Difficulty` x2, `plugin` | 3 |
| mod (root) | enemies | `Loot`, `Spawner` x3, `collect_gems`, `drop_loot`, `move_enemies`, `reap_dead`, `spawn_enemies`, `still_gems`, `tick_marks` | 11 |
| mod (root) | evolutions | `plugin` | 1 |
| mod (root) | fill | `FillMaterial` | 1 |
| mod (root) | finale | `plugin` | 1 |
| mod (root) | focus | `plugin` | 1 |
| mod (root) | frames | `plugin` | 1 |
| mod (root) | grade | `plugin` | 1 |
| mod (root) | hero | `AimPoint`, `CamRig`, `aim_cursor`, `daylight`, `drive_hero`, `drop_stray_buttons`, `follow_map`, `hero_vitals`, `place_camera` | 9 |
| mod (root) | hud | `Offer`, `SweepUiMaterial`, `apply_ui_scale`, `buttons`, `fill_art`, `fill_icons`, `hero_plate`, `icon_tooltip`, `keys` x2, `menu_preview`, `refresh_hud`, `refresh_menu` x2, `refresh_mobility`, `refresh_results`, `roll_offer`, `setup_hud`, `show_panels` x2, `update_cooldown_sweeps`, `xp_hover` | 22 |
| mod (root) | items | `plugin` | 1 |
| mod (root) | menu | `build_menu`, `menu_clicks`, `plugin`, `refresh_menu_ui`, `restore_pick` | 5 |
| mod (root) | minimap | `compose`, `map_buttons`, `zone_text` | 3 |
| mod (root) | mobility | `MobilityState` x2, `cast_mobility` | 3 |
| mod (root) | mods | `StatMods` x6 | 6 |
| mod (root) | nav | `plugin` | 1 |
| mod (root) | options | `plugin` | 1 |
| mod (root) | pickups | `plugin` | 1 |
| mod (root) | plates | `plugin` | 1 |
| mod (root) | profile | `MetaBonus` x2, `award_run`, `load_profile` | 4 |
| mod (root) | quests | `plugin` | 1 |
| mod (root) | talents | `plugin` | 1 |
| mod (root) | trainer | `plugin` | 1 |
| mod (root) | worldbuffs | `plugin` | 1 |
| mods | mod (root) | `Stats` | 1 |
| nav | enemies | `move_enemies` | 1 |
| nav | hero | `Hero`, `arena` | 2 |
| nav | mod (root) | `Phase`, `RunEntity` x3, `Selection` | 5 |
| nav | units | `flat` x6 (+1 in tests) | 6 (+1t) |
| options | hud | `HudFont`, `MenuRoot`, `PauseRoot`, `keys` | 4 |
| options | menu | `MenuBuilt` | 1 |
| options | mod (root) | `Phase` x6 | 6 |
| pickups | abilities | `Hit` x2 | 2 |
| pickups | chatlog | `ChatLine` x2 | 2 |
| pickups | enemies | `Dying`, `Elite`, `Enemy` x2, `Gem` | 5 |
| pickups | finale | `Reaper` | 1 |
| pickups | hero | `Hero` | 1 |
| pickups | items | `drop_boost` | 1 |
| pickups | mod (root) | `Phase`, `Rng`, `Run`, `RunEntity` | 4 |
| pickups | units | `Fx`, `Marks`, `flat` x3 | 5 |
| plates | enemies | `Enemy`, `move_enemies` | 2 |
| plates | hero | `Hero`, `place_camera` | 2 |
| plates | hud | `GOLD_TONE`, `HudFont` | 2 |
| plates | mod (root) | `Phase` x6, `Selection` | 7 |
| plates | profile | `Profile` | 1 |
| profile | chatlog | `ChatKind` x3, `ChatLine` x6 | 9 |
| profile | data | `CLASSES` x4 (+3 in tests), `roster` | 5 (+3t) |
| profile | difficulty | `Difficulty` x3 | 3 |
| profile | hud | `grouped` x2 | 2 |
| profile | mod (root) | `Run` x2, `Selection` | 3 |
| profile | mods | `StatMods` | 1 |
| profile | talents | `mods_for` | 1 |
| profile | trainer | `apply_ranks` | 1 |
| quests | chatlog | `ChatKind`, `ChatLine` x6 | 7 |
| quests | data | `EnemyDef` x4, `MAPS` (+1 in tests), `roster` (+1 in tests), `xp_to_next` | 6 (+2t) |
| quests | enemies | `Loot` | 1 |
| quests | hero | `Hero` | 1 |
| quests | hud | `GOLD_TONE` x2, `HudFont` | 3 |
| quests | mod (root) | `Phase` x8, `Rng` x4 (+2 in tests), `Run` x2, `Selection` | 15 (+2t) |
| quests | units | `flat`, `spawn_object` | 2 |
| talents | data | `CLASSES` x3 (+3 in tests) | 3 (+3t) |
| talents | hud | `HudFont`, `MenuRoot`, `keys`, `num` x2, `pct` x9 | 14 |
| talents | menu | `MenuBuilt` | 1 |
| talents | mod (root) | `Phase` x5, `Selection` x3 | 8 |
| talents | mods | `StatMods` x3 (+1 in tests) | 3 (+1t) |
| talents | profile | `Profile` x3 | 3 |
| trainer | data | `CLASSES` x2 | 2 |
| trainer | hud | `HudFont`, `MenuRoot`, `grouped` x6 (+3 in tests) | 8 (+3t) |
| trainer | menu | `MenuAction` x6, `MenuBuilt`, `refresh_menu_ui` | 8 |
| trainer | mod (root) | `Phase` x4, `Selection` x2 | 6 |
| trainer | profile | `MetaBonus` x2 (+1 in tests), `Profile` x5 (+1 in tests) | 7 (+2t) |
| units | data | `Weapon` | 1 |
| units | mod (root) | `RunEntity` x3 | 3 |
| worldbuffs | chatlog | `ChatLine` x2 | 2 |
| worldbuffs | hud | `GOLD_TONE` x2, `HudFont` | 3 |
| worldbuffs | mod (root) | `Phase` x3, `Rng` x2 (+2 in tests), `Run` | 6 (+2t) |
| worldbuffs | mods | `StatMods` x2 (+1 in tests) | 2 (+1t) |
| worldbuffs | talents | `Fx` x2 | 2 |

### Hubs, by number of distinct dependants

- `mod (root)`: `Phase` (25 modules), `Run` (21), `Selection` (15), `Rng` (12), `RunEntity` (6).
- `hud`: `HudFont` (14), `GOLD_TONE` (8), `MenuRoot` (5), `grouped`/`num`/`pct`/`spell_name`/`UiArt`.
  Everything that draws anything reaches into `hud.rs` for the same ten helpers.
- `hero`: `Hero` (14), `arena` (5). `enemies`: `Enemy` (10), `Dying`/`Elite`/`Loot`/`Gem`.
- `chatlog`: `ChatLine` (11 writers). `units`: `flat` (11), `Fx` (7). `data`: 17 consuming
  modules. `profile`: `Profile` (9).

## Cycles

### The strongly connected components

1. All edges (including registration and `auto`): one SCC of 31 modules: `abilities auto barrier
   cards charpanel chatlog chests difficulty enemies evolutions finale focus frames hero hud items
   menu minimap mobility mod mods nav options pickups plates profile quests talents trainer units
   worldbuffs`. Outside: `data`, `fill`, `grade`.
2. Without the edges leaving `mod.rs` for plugins and systems, and without `auto.rs`: one SCC of
   23 modules: `abilities cards chatlog difficulty enemies evolutions finale focus hero hud items
   menu minimap mobility mod mods nav options pickups profile talents trainer units`. This is the
   real type-level cycle, the one the crate split has to break.
3. Without `mod` as a node at all (only module-to-module edges): 21 modules, the same set minus
   `mod`, `mods`, `units` and plus `auto`. `auto -> hero -> auto` is the one cycle the registration
   edges hide.

### Edges that close it

A feedback arc set (Eades-Lin-Smyth order, weighted by reference count) removes 28 edges, weight 78,
and leaves a DAG. These are the edges that close the cycles, and how the proposal resolves each:

| edge | items | resolution |
|---|---|---|
| chatlog -> hud | `HudFont`, `grouped`, `spell_name` | UI kit goes to core (`hud.rs`: `HudFont`, `GOLD_TONE`, `grouped`, `num`, `pct`, `spell_name`, ...) |
| difficulty -> hud | `HudFont`, `MenuRoot`, `OverPayout`, `num`, `keys`, `refresh_results` | kit items to core; `keys`/`refresh_results` become `SurvivorsSet::Keys`/`RefreshResults` |
| difficulty -> menu | `MenuBuilt` | marker to core |
| difficulty -> profile | `Profile` | `profile.rs` is core |
| enemies -> abilities | `Hit` | the message type to core |
| enemies -> items, pickups -> items | `drop_boost` | 10-line env-var fn to core |
| pickups -> abilities | `Hit` | as above |
| evolutions -> cards, hud -> cards | `LEGENDARY` (and `Rays`, which stays: both are in the hud crate) | colour const to core |
| evolutions -> hud | `GOLD_TONE`, `HudFont`, `spell_name` | UI kit |
| finale -> enemies | `Dying`, `Enemy`, `spawn_enemy`, `spawn_spot` | enemy model and spawn API to core |
| menu -> hud | `HudFont`, `MenuRoot`, `rgb`, `looks_for`, `start_run` | kit, plus `start_run`/`looks_for` to core (they read only `Run`, `Selection`, `Profile`, `data`) |
| minimap -> enemies, mobility -> enemies | `Enemy` | enemy model |
| minimap -> hud | `UiArt` | to core |
| mod -> difficulty, mobility, profile | `Difficulty`, `MobilityState`, `MetaBonus` | those types to core (they are fields of `Run`) |
| mods -> mod | `Stats` | `Stats` is core |
| nav -> enemies | `move_enemies` (ordering only) | `SurvivorsSet::MoveEnemies` |
| options -> hud | `HudFont`, `MenuRoot`, `PauseRoot`, `keys` | kit + set |
| profile -> hud | `grouped` | kit |
| profile -> talents | `mods_for` | talent tables and rules to core |
| profile -> trainer | `apply_ranks` | trainer upgrades and rules to core |
| talents -> hud, talents -> menu, trainer -> hud | `HudFont`, `MenuRoot`, `keys`, `num`, `pct`, `grouped`, `MenuBuilt` | kit + `MenuBuilt` + set |

### Two-cycles (mutual dependencies)

`mobility <-> mod`, `difficulty <-> profile`, `difficulty <-> mod`, `enemies <-> finale`,
`enemies <-> mobility`, `enemies <-> items`, `enemies <-> nav`, `hud <-> talents`, `hud <-> options`,
`hud <-> trainer`, `hud <-> minimap`, `hud <-> profile`, `items <-> pickups`, `abilities <-> enemies`,
`profile <-> talents`, `profile <-> trainer`, `evolutions <-> hud`, `mod <-> profile`, `mod <-> mods`,
`cards <-> hud`.

`items <-> pickups` and `cards <-> hud` stay as they are: each pair ends up inside one crate (loot,
hud). `enemies <-> abilities`, `enemies <-> finale`, `hud <-> {talents,options,trainer,minimap}` need
the moves above.

### Cycle through the aggregator

`hero::drive_hero` reads `auto::Auto` and `auto::steer` (`hero.rs:296,311`), while `auto.rs` reads
`Hero`, `arena`, `Enemy`, `Gem`, `hud::Offer`, ... (`auto.rs:11-15`) and `mod.rs` registers `auto`'s
systems. If `auto` lives in the aggregator, `world -> app` closes a cycle. Fix: the autopilot's
only output the hero needs is `Auto.dir` (`auto.rs:35`, set at `:120`, `:151`; read through `steer`
at `:105`). Move that one `Vec3` to a core resource `Autopilot { dir }`; `auto::plan` writes it,
`drive_hero` reads `Option<Res<Autopilot>>`. `Auto` stays in the aggregator.

### Result after the proposal

The item-level check of the proposed assignment finds exactly 12 leaf-to-leaf references left, all
of three kinds, all fixed by the prep commits (see "Extraction order"):

| from -> to | items | fix |
|---|---|---|
| meta -> hud (3), windows -> hud (1) | `difficulty.plugin`, `talents.plugin`, `options.plugin` order `.after(hud::keys)`, `.after(hud::refresh_results)` | `SurvivorsSet::Keys`, `RefreshResults` |
| overlays -> combat (2), world -> combat (1) | `frames.plugin`, `plates.plugin` `.after(enemies::move_enemies)`; `nav.plugin` `.before(...)` | `SurvivorsSet::MoveEnemies` |
| overlays -> world (2) | `frames.plugin`, `plates.plugin` `.after(hero::place_camera)` | `SurvivorsSet::PlaceCamera` |
| combat -> bosses (1) | `enemies::spawn_enemies` calls `finale::plan(selection.map).timed` (`enemies.rs:246`) | `BossPlans` resource (below) |
| world -> app (2) | `hero::drive_hero` -> `auto::Auto`, `auto::steer` | `Autopilot` resource |

## Visibility breaks

Rust 2021: `pub(super)` in a child of `survivors` means "visible in all of survivors"; across
crates it must be `pub`. `pub(crate)` the same. A private item is visible to its own module only;
a private item of `mod.rs` is visible to all its children, which is a third way to break.

Today's code has 289 `pub(super)` occurrences (items, fields, methods) and 42 `pub(crate)` (36 of
them in `data.rs`; the others are `Difficulty`, `StatMods`, `MetaBonus`, `SurvivorsPlugin`, `ACTIVE`,
`active`). The last three are the ones `benilla-app/src/lib.rs:231,305,444` uses (Task 2 of the plan
covers those).

### Items that cross a crate boundary and are not `pub` (217)

Of 217: 178 `pub(super)`, 27 `pub(crate)`, 12 private (helpers dragged into core with their
parent). 71 of them cross only into the aggregator (plugins, systems and resources `mod.rs`
registers). Format: `:line` `item` (kind, current visibility, crates that use it from outside).
Every cross-used item has an explicit visibility today, none is private, except the dragged helpers.

- `abilities.rs` (5): `:18` `Hit` (struct, pub(super), to app/combat/loot), `:51` `PendingHits` (struct, pub(super), to app), `:225` `cast_abilities` (fn, pub(super), to app), `:585` `tick_areas` (fn, pub(super), to app), `:716` `apply_hits` (fn, pub(super), to app)
- `auto.rs` (2): `:18` `Auto` (struct, pub(super), to world), `:105` `steer` (fn, pub(super), to world)
- `barrier.rs` (1): `:43` `plugin` (fn, pub(super), to app)
- `cards.rs` (5): `:19` `LEGENDARY` (const, pub(super), to hud/windows), `:162` `PendingPick` (struct, pub(super), to app), `:167` `deal_cards` (fn, pub(super), to app), `:401` `animate_cards` (fn, pub(super), to app), `:516` `choose_card` (fn, pub(super), to app)
- `charpanel.rs` (1): `:28` `plugin` (fn, pub(super), to app)
- `chatlog.rs` (3): `:53` `ChatKind` (enum, pub(super), to bosses/combat/loot/overlays/windows), `:84` `ChatLine` (enum, pub(super), to bosses/combat/loot/meta/overlays/windows), `:252` `plugin` (fn, pub(super), to app)
- `chests.rs` (1): `:36` `plugin` (fn, pub(super), to app)
- `data.rs` (25): `:25` `Kind` (enum, pub(crate), to combat/hud), `:61` `Effect` (enum, pub(crate), to combat/hud), `:76` `AbilityDef` (struct, pub(crate), to combat/hud), `:995` `Evolution` (struct, pub(crate), to hud/windows), `:1005` `EVOLVED_LEVEL` (const, pub(crate), to hud), `:1022` `EVOLUTIONS` (static, pub(crate), to windows), `:1040` `ability` (fn, pub(crate), to app/hud/windows), `:1047` `MAX_ABILITY_LEVEL` (const, pub(crate), to app/hud/loot/windows), `:1048` `MAX_PASSIVE_LEVEL` (const, pub(crate), to hud/loot), `:1049` `MAX_ABILITIES` (const, pub(crate), to hud), `:1050` `MAX_PASSIVES` (const, pub(crate), to hud), `:1053` `Stat` (enum, pub(crate), to hud), `:1066` `PassiveDef` (struct, pub(crate), to hud), `:1075` `PASSIVES` (static, pub(crate), to hud/windows), `:1148` `CLASSES` (static, pub(crate), to app/hud/meta), `:1362` `EnemyDef` (struct, pub(crate), to bosses/overlays), `:1468` `VICTORY_SECS` (const, pub(crate), to app/world), `:1470` `BOSS_EVERY_SECS` (const, pub(crate), to bosses/combat), `:1472` `XP_RATE` (const, pub(crate), to combat), `:1475` `progress` (fn, pub(crate), to combat/loot), `:1480` `wave_pool` (fn, pub(crate), to combat), `:1490` `target_alive` (fn, pub(crate), to combat), `:1505` `xp_to_next` (fn, pub(crate), to combat/hud/overlays), `:1566` `MAPS` (static, pub(crate), to app/bosses/meta), `:2135` `roster` (fn, pub(crate), to bosses/combat/meta/overlays)
- `difficulty.rs` (2): `:21` `Difficulty` (enum, pub(crate), to meta), `:115` `plugin` (fn, pub(super), to app)
- `enemies.rs` (19): `:21` `Enemy` (struct, pub(super), to app/bosses/combat/hud/loot/overlays/world), `:44` `Elite` (struct, pub(super), to combat/loot), `:61` `Dot` (struct, pub(super), to combat), `:69` `Dying` (struct, pub(super), to bosses/combat/loot), `:72` `Spawner` (struct, pub(super), to app), `:78` `Loot` (struct, pub(super), to app/combat/overlays), `:89` `GemKind` (enum, pub(super), to combat), `:97` `Gem` (struct, pub(super), to app/combat/loot), `:129` `spawn_enemy` (fn, pub(super), to bosses/combat), `:191` `ground` (fn, private, to combat), `:204` `spawn_spot` (fn, pub(super), to bosses/combat), `:222` `spawn_enemies` (fn, pub(super), to app), `:310` `move_enemies` (fn, pub(super), to app/overlays/world), `:471` `tick_marks` (fn, pub(super), to app), `:484` `reap_dead` (fn, pub(super), to app), `:500` `mark_dying` (fn, pub(super), to combat), `:526` `drop_loot` (fn, pub(super), to app), `:595` `collect_gems` (fn, pub(super), to app), `:670` `still_gems` (fn, pub(super), to app)
- `evolutions.rs` (3): `:24` `plugin` (fn, pub(super), to app), `:51` `ready` (fn, pub(super), to hud), `:61` `evolved_from` (fn, pub(super), to hud)
- `fill.rs` (1): `:11` `FillMaterial` (struct, pub(super), to app)
- `finale.rs` (5): `:54` `Finale` (struct, pub(super), to bosses/loot), `:58` `Reaper` (struct, pub(super), to bosses/loot), `:96` `Plan` (struct, pub(super), to bosses), `:111` `plan` (fn, pub(super), to combat), `:162` `plugin` (fn, pub(super), to app)
- `focus.rs` (3): `:17` `Focus` (enum, pub(super), to combat/overlays), `:74` `Target` (struct, pub(super), to combat/overlays), `:76` `plugin` (fn, pub(super), to app)
- `frames.rs` (1): `:47` `plugin` (fn, pub(super), to app)
- `grade.rs` (1): `:68` `plugin` (fn, pub(super), to app)
- `hero.rs` (17): `:15` `Hero` (struct, pub(super), to app/bosses/combat/hud/loot/overlays/world), `:25` `HERO_RADIUS` (const, pub(super), to combat), `:64` `CamRig` (struct, pub(super), to app), `:105` `UiCover` (struct, pub(super), to overlays/world), `:143` `arena` (fn, pub(super), to app/hud/world), `:149` `follow_map` (fn, pub(super), to app), `:163` `spawn_hero` (fn, pub(super), to hud), `:198` `MINUTE_OF_DAY` (const, pub(super), to hud/world), `:200` `daylight` (fn, pub(super), to app), `:211` `AimPoint` (struct, pub(super), to app/overlays/world), `:215` `aim_cursor` (fn, pub(super), to app), `:286` `drive_hero` (fn, pub(super), to app), `:413` `drop_stray_buttons` (fn, pub(super), to app), `:427` `place_camera` (fn, pub(super), to app/overlays), `:522` `hurt_hero` (fn, pub(super), to combat), `:537` `hero_vitals` (fn, pub(super), to app), `:627` `reach` (fn, pub(super), to combat/world)
- `hud.rs` (36): `:20` `GOLD` (const, private, to hud), `:26` `DIM` (const, private, to hud), `:27` `DIM_GREY` (const, pub(super), to hud), `:28` `GOLD_TONE` (const, pub(super), to hud/meta/overlays/windows), `:31` `MenuRoot` (struct, pub(super), to hud/meta/windows), `:37` `PauseRoot` (struct, pub(super), to hud/windows), `:81` `SweepUiMaterial` (struct, pub(super), to app), `:117` `OverPayout` (struct, pub(super), to hud/meta), `:128` `UiArt` (struct, pub(super), to hud/loot/overlays), `:193` `Offer` (struct, pub(super), to app), `:208` `HudFont` (struct, pub(super), to hud/meta/overlays/windows), `:210` `rgb` (fn, pub(super), to meta), `:304` `spell_name` (fn, pub(super), to hud/loot/windows), `:319` `setup_hud` (fn, pub(super), to app), `:946` `menu_preview` (fn, pub(super), to app), `:988` `start_run` (fn, pub(super), to app/hud/meta), `:1076` `roll_offer` (fn, pub(super), to app), `:1080` `pick` (fn, pub(super), to app), `:1121` `looks_for` (fn, pub(super), to hud/meta), `:1138` `buttons` (fn, pub(super), to app), `:1166` `keys` (fn, pub(super), to app/meta/windows), `:1211` `refresh_menu` (fn, pub(super), to app), `:1381` `refresh_hud` (fn, pub(super), to app), `:1546` `fill_icons` (fn, pub(super), to app), `:1583` `refresh_mobility` (fn, pub(super), to app), `:1623` `update_cooldown_sweeps` (fn, pub(super), to app), `:1642` `fill_art` (fn, pub(super), to app), `:1665` `apply_ui_scale` (fn, pub(super), to app), `:1685` `hero_plate` (fn, pub(super), to app), `:1765` `xp_hover` (fn, pub(super), to app), `:1820` `refresh_results` (fn, pub(super), to app/meta), `:1893` `grouped` (fn, pub(super), to hud/meta/overlays/windows), `:1917` `num` (fn, pub(super), to hud/meta/windows), `:1925` `pct` (fn, pub(super), to hud/windows), `:2289` `icon_tooltip` (fn, pub(super), to app), `:2410` `show_panels` (fn, pub(super), to app)
- `items.rs` (4): `:49` `plugin` (fn, pub(super), to app), `:74` `drop_boost` (fn, pub(super), to combat/loot), `:530` `WORN` (static, private, to loot), `:532` `gear_lines` (fn, pub(super), to windows)
- `menu.rs` (7): `:42` `MenuAction` (enum, pub(super), to meta), `:53` `MenuBuilt` (struct, pub(super), to meta/windows), `:81` `restore_pick` (fn, pub(super), to app), `:138` `build_menu` (fn, pub(super), to app), `:531` `menu_clicks` (fn, pub(super), to app), `:620` `refresh_menu_ui` (fn, pub(super), to app), `:722` `plugin` (fn, pub(super), to app)
- `minimap.rs` (3): `:266` `map_buttons` (fn, pub(super), to app), `:318` `zone_text` (fn, pub(super), to app), `:357` `compose` (fn, pub(super), to app)
- `mobility.rs` (4): `:12` `LEARN_LEVEL` (const, pub(super), to combat), `:16` `Move` (enum, pub(super), to world), `:39` `for_class` (fn, pub(super), to combat), `:174` `cast_mobility` (fn, pub(super), to app)
- `mod.rs` (7): `:195` `Phase` (enum, pub(super), to app/bosses/combat/hud/loot/meta/overlays/windows/world), `:206` `RunEntity` (struct, pub(super), to app/combat/loot/world), `:210` `Selection` (struct, pub(super), to app/bosses/combat/hud/meta/overlays/windows/world), `:240` `Owned` (struct, pub(super), to app/hud), `:268` `Stats` (struct, pub(super), to hud), `:287` `Run` (struct, pub(super), to app/bosses/combat/hud/loot/meta/overlays/windows/world), `:430` `Rng` (struct, pub(super), to app/bosses/combat/hud/loot/meta/overlays)
- `mods.rs` (1): `:8` `StatMods` (struct, pub(crate), to loot/meta)
- `nav.rs` (9): `:33` `CELL` (const, private, to world), `:43` `BODY_MID` (const, private, to world), `:90` `NavGrid` (struct, pub(super), to bosses/combat/world), `:525` `ring_len` (fn, private, to world), `:534` `ring_cell` (fn, private, to world), `:564` `body_probe` (fn, private, to world), `:571` `SpawnProbe` (struct, pub(super), to bosses/combat), `:590` `solid_at` (fn, private, to world), `:712` `plugin` (fn, pub(super), to app)
- `options.rs` (2): `:62` `plugin` (fn, pub(super), to app), `:77` `OptionsUi` (struct, pub(super), to hud/windows)
- `pickups.rs` (1): `:51` `plugin` (fn, pub(super), to app)
- `plates.rs` (1): `:30` `plugin` (fn, pub(super), to app)
- `profile.rs` (7): `:23` `MAX_LEVEL` (const, pub(super), to hud/meta), `:45` `Award` (struct, pub(super), to hud), `:90` `Profile` (struct, pub(super), to app/hud/meta/overlays/windows), `:271` `xp_to_next` (fn, pub(super), to hud/meta), `:276` `zone_min_level` (fn, pub(super), to meta), `:299` `load_profile` (fn, pub(super), to app), `:305` `award_run` (fn, pub(super), to app)
- `quests.rs` (1): `:45` `plugin` (fn, pub(super), to app)
- `talents.rs` (16): `:36` `MAX_ROWS` (const, private, to meta), `:40` `Fx` (enum, pub(super), to meta), `:100` `t` (fn, private, to meta), `:111` `Spec` (struct, pub(super), to meta), `:755` `specs` (fn, pub(super), to meta), `:779` `points_total` (fn, pub(super), to meta), `:784` `needs` (fn, pub(super), to meta), `:789` `rank` (fn, pub(super), to meta), `:798` `spec_spent` (fn, pub(super), to meta), `:806` `spent` (fn, pub(super), to meta), `:811` `available` (fn, pub(super), to meta), `:817` `can_spend` (fn, pub(super), to meta), `:836` `spend` (fn, pub(super), to meta), `:855` `respec` (fn, pub(super), to meta), `:917` `TalentsUi` (struct, pub(super), to app/hud/meta), `:948` `plugin` (fn, pub(super), to app)
- `trainer.rs` (8): `:33` `Upgrade` (struct, pub(super), to meta), `:46` `UPGRADES` (const, pub(super), to meta), `:114` `rank` (fn, pub(super), to meta), `:119` `next_price` (fn, pub(super), to meta), `:125` `spent` (fn, pub(super), to meta), `:144` `class_price` (fn, pub(super), to meta), `:191` `TrainerUi` (struct, pub(super), to hud/meta), `:220` `plugin` (fn, pub(super), to app)
- `units.rs` (14): `:26` `FACTION_FRIENDLY` (const, pub(super), to combat), `:50` `yaw_toward` (fn, pub(super), to combat/world), `:54` `flat` (fn, pub(super), to app/combat/loot/overlays/world), `:58` `UnitSpec` (struct, pub(super), to combat), `:72` `spawn_unit` (fn, pub(super), to combat), `:116` `spawn_object` (fn, pub(super), to combat/loot/overlays), `:150` `kill_unit` (fn, pub(super), to combat/world), `:158` `spawn_area_object` (fn, pub(super), to combat), `:199` `Marks` (struct, pub(super), to combat/loot/world), `:301` `Fx` (struct, pub(super), to bosses/combat/loot/world), `:372` `COLOR_SPELL` (const, pub(super), to combat), `:373` `COLOR_MELEE` (const, pub(super), to combat), `:374` `COLOR_HURT` (const, pub(super), to combat), `:375` `COLOR_HEAL` (const, pub(super), to combat)
- `worldbuffs.rs` (1): `:169` `plugin` (fn, pub(super), to app)

Rule of thumb: in core, `sed 's/pub(super)/pub/; s/pub(crate)/pub/'` over the moved code is
correct; in a leaf crate `pub(super)` stays (it means "the crate" there, since `lib.rs` is the parent)
except for the plugin function, resources and systems the aggregator uses.

### Fields and methods reached across the new boundary

Private or `pub(super)` members used by code that ends up in another crate (all verified by
grep in the original file):

| member | declared | used by | note |
|---|---|---|---|
| `Run.mobile` (private) | `mod.rs:310` | `enemies.rs:416`, `hud.rs:1613`, `mobility.rs:164-277` | private in `mod.rs`, visible to all children today; `pub` in core |
| `Dying.0` (private tuple field) | `enemies.rs:69` | `reap_dead` `enemies.rs:487-495` | `Dying` is core, `reap_dead` is combat; make the field `pub` |
| `Gem.kind`, `Gem.speed` (private) | `enemies.rs:98,101` | `spawn_gem` (`enemies.rs:578`), `drop_loot` (`:526`), `collect_gems` (`:595`), `still_gems` (`:670`) | model in core, systems in combat |
| `Hero.vy`, `Hero.air` (`pub(super)`) | `hero.rs:17,21` | `drive_hero` and friends | `pub` |
| `UiCover::cursor_over_ui` (`pub(super)`) | `hero.rs:121` | `frames.rs`, `hero.rs` | `pub` (the fields `windows`, `nodes` stay private, the method is the API) |
| `OverPayout.0` (private) | `hud.rs:117` | `hud.rs:899,904,1827`, `difficulty.rs:297` | tuple field `pub` |
| `UiArt.path/.coords/.tint` (private), `UiArt::tinted` (private), `UiArt::crop` (`pub(super)`) | `hud.rs:128-153` | `hud.rs:1651-1660` | `UiArt` goes to core, the system that fills it stays in hud |
| `NavGrid.stage/.center/.radius/.half/.solid` (private), `NavGrid::new/.center_of/.inside/.set/.track` (private) | `nav.rs:92-105,152-520` | `scan_world`, `track_hero` (`nav.rs:613-710`) | `NavGrid` goes to core, the scan systems stay in world: `pub` for these |
| `OptionsUi.open` (`pub(super)`), `OptionsUi.root` (private) | `options.rs:78,79` | `options.rs`, `hud.rs:1173` | resource to core |
| `TalentsUi.open`, `TrainerUi.open` (`pub(super)`) | `talents.rs:918`, `trainer.rs:192` | `hud.rs:1172-1174`, `auto.rs:167`, their own UI | resources to core |
| `Focus::index/.label/.hint/.color/.icon` (private), `Focus::next` (`pub(super)`) | `focus.rs:27-69` | the focus window in `focus.rs` (`:35,231,238,247`) | `Focus` is core (abilities reads it), the focus window stays in overlays |
| `ChatKind::color` (private) | `chatlog.rs:68` | `chatlog.rs:459` | type in core, window in windows |
| `Difficulty::color` (private) | `difficulty.rs:28` | `difficulty.rs:264-265` | type in core, UI in meta |
| `SweepUiMaterial.sweep` (`pub(super)`) | `hud.rs:83` | hud only | stays in the hud crate, no change |
| `Rng.0` (private tuple field) | `mod.rs:428` | `mod.rs` only | no change; the fields are used only through methods |

Name collisions that matter when the files become modules of one crate: `xp_to_next` (`data.rs` and
`profile.rs`), `Fx` (`units.rs:301` SystemParam and `talents.rs:40` effect enum), `rank` and `spent`
(`talents.rs` and `trainer.rs`). Core must keep the file layout as modules (`core::talents::rank`,
`core::trainer::rank`, `core::units::Fx`, `core::talents::Fx`), not a flat namespace. Private
look-alikes that are not collisions: `items::Stat` (`items.rs:210`) vs `data::Stat`, `pickups::Kind`
(`pickups.rs:136`) vs `data::Kind`; both are file-local and stay in loot. `Label` exists privately in
`options.rs`, `talents.rs`, `trainer.rs` and is never shared.

## Inherent and trait impls across modules

Only one inherent impl sits on a type from another module:

- `mobility.rs:154-170`: `impl Run { fn mobility(&self); fn haste_mult(&self) }`. `Run` is defined in
  `mod.rs`. If `Run` moves to core and this impl stays in a leaf, it is an orphan error. The two methods
  are used by `hud.rs:1591,2359`, `charpanel.rs:203`, `hero.rs:335` and `mobility.rs:194`, so a leaf
  home would create leaf-to-leaf edges. Resolution: the impl moves to core with the `Mobility` table
  (`mobility.rs:15-143`: `Move`, `Mobility`, `MOBILITY`, `for_class`, `LEARN_LEVEL`,
  `MobilityState`), 150 lines. `cast_mobility` (`mobility.rs:172-278`) stays in world.

All other `impl` blocks are on a type of their own file or on a foreign trait with a local type
(`UiMaterial for FillMaterial` `fill.rs:26`, `UiMaterial for SweepUiMaterial` `hud.rs:86`,
`FullscreenMaterial for Grade` `grade.rs:45`, `Plugin for SurvivorsPlugin`, `Default for Run/Rng/CamRig/
Character/MetaBonus/Profile`). They are fine as long as the impl stays in the crate of the type: `FillMaterial`
and `SweepUiMaterial` are both used only by hud code and the aggregator's `UiMaterialPlugin`
registrations, so they stay together in the hud crate. The program has no local traits and no
`#[macro_export]` (`data.rs:119 macro_rules! ability` is file-local).

Impl blocks that move with their type inside one file (no problem, but they must move together):
`Hit` (`abilities.rs`), `ChatKind`/`ChatLine`, `Difficulty`, `Focus`, `UiCover`, `UiArt`, `NavGrid`,
`Field`, `SpawnProbe`, `Selection`, `Owned`, `Run`, `Rng`, `Fx` (units).

## Registration and ordering

### What `mod.rs` registers directly (not in a module's own `plugin`)

| registration | systems / items | home crate after the split |
|---|---|---|
| `init_state::<Phase>()`, `init_resource::<Rng/Selection/Run>()` | | core plugin |
| `init_resource::<hero::CamRig>()`, `hero::AimPoint` | | world / core plugin |
| `init_resource::<abilities::PendingHits>()` | | combat plugin (`pub`) |
| `init_resource::<enemies::Spawner>()` | | combat plugin (`pub`) |
| `init_resource::<hud::Offer>()`, `cards::PendingPick` | | hud plugin |
| `add_message::<abilities::Hit>()`, `add_message::<enemies::Loot>()` | message types in core | core plugin |
| `UiMaterialPlugin<hud::SweepUiMaterial>`, `<fill::FillMaterial>` | | hud plugin |
| `Startup`: `hud::setup_hud`, `profile::load_profile` | | hud plugin; core plugin |
| `Update`: `hero::daylight` | | world plugin |
| Input chain (`WorldStage::Input`): `hero::drop_stray_buttons`, `aim_cursor`, `mobility::cast_mobility`, `hero::drive_hero`, `place_camera` | one chain | world plugin (all world) |
| Run chain (`after(Input)`, `Phase::Playing`, loading screen not covering): `enemies::spawn_enemies`, `move_enemies`, `abilities::cast_abilities`, `tick_areas`, `apply_hits`, `enemies::tick_marks`, `drop_loot`, `collect_gems`, `hero::hero_vitals` | one chain across combat and world (`hero_vitals` is last) | **aggregator** |
| `(enemies::reap_dead, still_gems).after(Input)` | | combat plugin |
| UI chain (`after(Input)`): `hero::follow_map`, `hud::menu_preview`, `buttons`, `keys`, `refresh_menu`, `menu::restore_pick`, `build_menu`, `menu_clicks`, `menu::refresh_menu_ui.before(GlueVisuals)`, `cards::deal_cards`, `choose_card`, `animate_cards`, `hud::refresh_hud`, `refresh_results`, `fill_icons`, `update_cooldown_sweeps`, `refresh_mobility`, `show_panels` | one chain across world, hud, meta | **aggregator**: the only place that may see hud and meta together |
| HUD chain (`after(hud::show_panels)`): `hud::apply_ui_scale`, `fill_art`, `xp_hover`, `icon_tooltip`, `hero_plate`, `minimap::map_buttons`, `zone_text`, `compose` | all hud crate | hud plugin |
| `OnEnter(Playing)`: `unpause`; `OnEnter(LevelUp)`: `pause`, `hud::roll_offer`; `OnEnter(Paused)`: `pause`; `OnEnter(GameOver)`: `unpause`, `profile::award_run`; `OnEnter(Menu)`: `unpause`, `reset_run` | | aggregator (`pause`, `unpause`, `reset_run`); hud (`roll_offer`); core (`award_run`) |
| `auto::drive.after(hud::keys).before(hud::refresh_menu)`, `auto::plan.before(WorldStage::Input)` | only with `WOW_SURVIVORS_AUTO` | aggregator (direct paths, fine) |
| Plugins: `grade`, `trainer`, `plates`, `barrier`, `menu`, `charpanel`, `nav`, `focus`, `chatlog`, `options`, `talents`, `worldbuffs`, `difficulty`, `items`, `chests`, `pickups`, `finale`, `evolutions`, `quests`, `frames` | | each leaf crate exports one `pub fn plugin(app: &mut App)` that calls its modules' plugins; the aggregator adds them |

The two chains that cross crates (Run chain, UI chain) stay in `survivors-app`; that is why every
system in them becomes `pub`. The UI chain interleaves `hud` and `menu` tightly (hud, menu, cards, hud
again), so it cannot be handed to the crates.

`reset_run` (`mod.rs:472-493`) mutates `abilities::PendingHits` and `enemies::Spawner`; it stays in the
aggregator and those two types become `pub` in combat. Moving them to core instead would add two
tiny types to core and a combat edit to the shared crate for no gain.

### Ordering constraints that name a system of another module

| where | constraint | becomes |
|---|---|---|
| `difficulty.rs:122-129` | `.after(hud::keys)`, `.before(GlueVisuals)`, second chain `.after(hud::refresh_results)` | `.after(SurvivorsSet::Keys)`, `.after(SurvivorsSet::RefreshResults)` |
| `options.rs:68` | `.after(super::hud::keys)` | `.after(SurvivorsSet::Keys)` |
| `talents.rs:954` | `.after(hud::keys)` | `.after(SurvivorsSet::Keys)` |
| `frames.rs:53-54` | `.after(super::hero::place_camera)`, `.after(super::enemies::move_enemies)` | `PlaceCamera`, `MoveEnemies` |
| `plates.rs:36-37` | `.after(crate::survivors::hero::place_camera)`, `.after(enemies::move_enemies)` | same |
| `nav.rs:719` | `.before(super::enemies::move_enemies)` | `.before(SurvivorsSet::MoveEnemies)` |
| `trainer.rs:235` | `.after(menu::refresh_menu_ui)` | stays (both in meta) or `SurvivorsSet::RefreshMenuUi` if meta is split |
| `menu.rs:726-729` | `build_pick_extras.after(build_menu)`, `.after(refresh_menu_ui)` | stays (same file) |

`SurvivorsSet` in core:

```rust
#[derive(SystemSet, Debug, Clone, PartialEq, Eq, Hash)]
pub enum SurvivorsSet { Keys, RefreshResults, PlaceCamera, MoveEnemies }
```

The aggregator (which adds `hud::keys`, `hud::refresh_results`, `hero::place_camera`,
`enemies::move_enemies` inside its chains) puts them in the set: `hud::keys.in_set(SurvivorsSet::Keys)`
inside the chain tuple. A set is the only cross-crate ordering handle that needs no import of
the other crate. All engine orderings (`WorldStage::Input`, `WorldStage::Stream`, `GlueVisuals`) are
engine types and stay as they are.

### Boss schedule (`enemies` -> `finale::plan`)

`enemies.rs:246` reads `finale::plan(selection.map).timed`: the roster's bosses the clock spawns,
minus the one `finale.rs` promotes to the finale. The data behind it (`FINALES`, `Pick`, `build`,
`PLANS`, `FINALE_HP/DAMAGE/XP/SIZE`, `finale.rs:24-146`) is boss tuning, exactly what a person
working on the world boss edits. Proposal: core gets only the types
(`pub struct Plan { timed, finale }` `finale.rs:96`, and `pub struct BossPlans(pub Vec<Plan>)` with
`fn plan(&self, map) -> &Plan`); `survivors-bosses`' plugin builds the plans and inserts
`BossPlans`; `spawn_enemies` takes `Res<BossPlans>`. The tuning stays in the small crate.
`Finale` (`finale.rs:54`) and `Reaper` (`:58`) are unit markers read by `chests.rs`, `items.rs`,
`pickups.rs`; they go to core.

## Shared messages/resources/components

Writers are `MessageWriter`/`ResMut`/spawn/insert sites, readers are `MessageReader`/`Res`/query.

### Messages

| message | defined | writers | readers | registered |
|---|---|---|---|---|
| `Hit` | `abilities.rs:18` | `abilities` (`:137,237,597`), `enemies` (`:319`), `pickups` (`:249`) | `abilities::apply_hits` (`:721`) | `mod.rs` (`add_message`) |
| `Loot` | `enemies.rs:78` | `abilities::apply_hits` (`:731`) | `enemies::drop_loot` (`:528`), `quests` (`:229`) | `mod.rs` |
| `ChatLine` | `chatlog.rs:84` | `abilities` (`:733`), `chests`, `difficulty`, `enemies` (x2), `evolutions`, `finale`, `items` (x5), `pickups`, `profile::award_run`, `quests` (x2), `worldbuffs` | `chatlog::collect` (`:270`) | `chatlog::plugin` |

All three types go to core; their registration moves to the core plugin so a build without the chat
window still works.

### Resources

| resource | defined | mutated by | read by | home |
|---|---|---|---|---|
| `Run` | `mod.rs:287` | abilities, auto, cards, chests, enemies, finale, hero, hud, items, menu, mobility, pickups, quests, worldbuffs, `reset_run` | abilities, cards, charpanel, chatlog, difficulty, evolutions, frames, hero, hud, minimap, profile | core |
| `Selection` | `mod.rs:210` | auto, hud, menu, `reset_run` | barrier, charpanel, enemies, finale, frames, hero, nav, plates, profile, quests, talents, trainer | core |
| `Rng` | `mod.rs:430` | abilities, auto, cards, chests, enemies, finale, hud, items, menu, pickups, quests, worldbuffs | (all mutate; no reader) | core |
| `Profile` | `profile.rs:90` | difficulty, profile (`award_run`), talents, trainer | charpanel, frames, hud, menu, plates | core |
| `Focus` / `Target` | `focus.rs:17,74` | focus (cycle), frames (`Target`) | abilities | core |
| `NavGrid` | `nav.rs:90` | nav (scan, track) | enemies, finale | core (written by world) |
| `Offer`, `PendingPick` | `hud.rs:193`, `cards.rs:162` | hud, cards, auto | cards, hud | hud crate (`Offer` `pub` for the aggregator and auto) |
| `HudFont` | `hud.rs:208` | hud (`setup_hud`) | 14 modules | core (the kit) |
| `OptionsUi`, `TalentsUi`, `TrainerUi` | `options.rs:77`, `talents.rs:917`, `trainer.rs:191` | their windows, `auto` (`TalentsUi`) | `hud::keys` | core |
| `Beacons` | `pickups.rs:64` | pickups, chests, items | same | loot |
| `Spawner`, `PendingHits` | `enemies.rs:72`, `abilities.rs:51` | combat, `reset_run` | combat | combat |
| `AimPoint` | `hero.rs:211` | `aim_cursor` | frames, mobility | core |
| `CamRig` | `hero.rs:64` | hero | hero | world |
| `MiniMap`, `Gear`, `Bank`, `Tracker`, `ChatLog`, `Spellbook`, `Stage`, `Plates`, ... | own file | own file | own file | the crate of the file |

New in the proposal: `BossPlans` (written by bosses' plugin, read by `spawn_enemies`), `Autopilot`
(written by `auto::plan`, read by `drive_hero`).

### Components and system parameters

| item | defined | inserted/spawned by | read by | home |
|---|---|---|---|---|
| `Hero` | `hero.rs:15` | `hero::spawn_hero` (called from `hud::menu_preview`) | abilities, auto, barrier, chests, enemies, finale, hud, items, minimap, mobility, nav, pickups, plates, quests | core |
| `Enemy` | `enemies.rs:21` | `spawn_enemy` (enemies, finale) | abilities, auto, chests, finale, frames, items, minimap, mobility, pickups, plates | core |
| `Elite`, `Dying`, `Dot` | `enemies.rs:44,69,61` | `spawn_enemy`, `mark_dying` (abilities, enemies) | chests, items, pickups, frames, finale | core |
| `Gem`, `GemKind` | `enemies.rs:97,89` | `drop_loot` (enemies) | pickups, auto, enemies | core |
| `Finale`, `Reaper` | `finale.rs:54,58` | `finale::summon` | chests, items, pickups | core |
| `Marks` | `units.rs:199` | `spawn_enemy`, `spawn_hero` | abilities, enemies, hero, pickups | core (`units`) |
| `RunEntity` | `mod.rs:206` | everything that spawns a run entity | `reset_run` | core |
| `MenuRoot`, `PauseRoot`, `OverPayout`, `MenuBuilt`, `MenuAction` | `hud.rs:31,37,117`, `menu.rs:53,42` | hud, menu | difficulty, options, talents, trainer | core (the kit) |
| `UiArt` | `hud.rs:128` | hud, frames, items, minimap | hud (`fill_art`) | core |
| `Fx` (SystemParam) | `units.rs:301` | | abilities, chests, enemies, finale, hero, mobility, pickups | core |
| `UiCover`, `SpawnProbe` (SystemParams) | `hero.rs:105`, `nav.rs:571` | | frames, hero; enemies, finale | core |

Engine-owned types shared by survivors files (not our problem, listed for completeness):
`ObjectStore`, `FieldChanged`, `Guid` (`crate::net`), `MovementState` (`crate::creature_anim`),
`WorldPoint`, `WorldCollision`, `LoadingScreen`, `GlueVisuals`, `WorldStage`.

## Proposed crates

Line counts are code plus tests of the files, allocated by item (tests by what they test, see
"Tests"). They add up to about 21,550, the current 21,540 within rounding, plus about 60 lines of new glue.

| crate | modules / items | approx lines | depends on |
|---|---|---|---|
| `survivors-core` | `mod.rs` types (`Phase`, `RunEntity`, `Selection`, `Owned`, `Stats`, `Run`, `Rng`), `data`, `mods`, `units`, `profile` (all of it, with `load_profile`/`award_run`), `difficulty::Difficulty`, `chatlog::{ChatKind, ChatLine}`, `hero` model (`Hero`, `AimPoint`, `UiCover`, `arena`, `spawn_hero`, `hurt_hero`, `reach`, `MINUTE_OF_DAY`, `HERO_RADIUS`), enemy model and spawn API (`Enemy`, `Elite`, `Dying`, `Dot`, `Gem`, `GemKind`, `Loot`, `spawn_enemy`, `spawn_spot`, `mark_dying`), `abilities::Hit`, `nav::{NavGrid, Field, SpawnProbe}`, mobility data and `impl Run`, **talent data and rules** (`talents.rs:32-875`: trees, `Fx`, `mods_for`, `spend`, ...), **trainer data and rules** (`trainer.rs:28-150`), `focus::{Focus, Target}`, the UI kit from `hud.rs` (`HudFont`, colours, `grouped/num/pct/rgb/spell_name`, `MenuRoot`, `PauseRoot`, `OverPayout`, `UiArt`, `start_run`, `looks_for`), `menu::{MenuAction, MenuBuilt}`, `OptionsUi`/`TalentsUi`/`TrainerUi`, `cards::LEGENDARY`, evolution rules (`ready`, `evolved_from`, `holds`), gear mirror (`WORN`, `gear_lines`, a new setter), `drop_boost`, `Finale`/`Reaper`/`Plan`/`BossPlans`, `SurvivorsSet`, `Autopilot`, core plugin | ~6,300 (of which data 2,180, talents 860, nav 580, tests 590) | `benilla-app` facade (`webenv`, `local_state`, `net`, `creature_anim`, `ui_action`, `entities`, `combat_text`), `benilla-protocol`, bevy |
| `survivors-world` | `hero` systems (`drive_hero`, `place_camera`, `aim_cursor`, `follow_map`, `hero_vitals`, `daylight`, `drop_stray_buttons`, `CamRig`), `nav` scan (`scan_world`, `track_hero`, `Scan`, `plugin`), `barrier`, `mobility::cast_mobility` | ~1,180 | core, `benilla-app` facade (`loading_screen`, `player`, `creature_anim`, `net`), `benilla_world` |
| `survivors-combat` | `abilities` systems (`cast_abilities`, `tick_areas`, `apply_hits`, `PendingHits`, `Totem`, target picking), `enemies` systems (`spawn_enemies`, `move_enemies`, `tick_marks`, `reap_dead`, `drop_loot`, `collect_gems`, `still_gems`, `Spawner`, `spawn_gem`) | ~1,440 | core, facade (`net`, `ui_action`, `creature_anim`) |
| `survivors-loot` | `items` (item model, tables, `Gear`, `Bank`, systems), `chests`, `pickups` (`Beacons`) | ~1,470 | core, facade (`local_state`, `loading_screen`, `ui_action`) |
| `survivors-bosses` | `finale` (`summon`, `hunt`, `finale_slain`, plans, `FINALES`, `REAPER`, `Stage`) | ~290 | core, facade (`webenv`, `loading_screen`) |
| `survivors-hud` | `hud` systems (`setup_hud`, `refresh_*`, tooltips, `show_panels`, `roll_offer`, `pick`, `buttons`, `keys`), `cards`, `minimap`, `fill` | ~3,500 | core, facade (`ui_action`, `vplates`, `char_select`, `glue`, `area`), `benilla_assets/_ui/_formats` |
| `survivors-windows` | `charpanel`, `chatlog` window (`ChatLog`, build/show), `options`, `evolutions` window (`Spellbook`, `BookUi`, `recipes`) | ~1,900 | core, facade (`glue`, `ui_action`, `video`, `local_state`, `cvars`) |
| `survivors-overlays` | `frames`, `plates`, `focus` window, `quests`, `grade` | ~1,850 | core, facade (`entities`, `vplates`, `sound`, `ui_pass`, `glue`), `benilla_world/_assets/_ui` |
| `survivors-meta` | `menu` (minus `MenuAction`/`MenuBuilt`), `trainer` UI, `talents` UI, `difficulty` UI, `worldbuffs` | ~3,100 | core, facade (`glue` 21 uses, `entities`, `local_state`, `char_select`), `benilla_assets` |
| `survivors-app` | `SurvivorsPlugin` (calls every `plugin`, owns the Run chain and UI chain, `pause`, `unpause`, `reset_run`), `auto` (minus `Autopilot`) | ~490 | all of the above |

Dependency shape: `app -> {world, combat, loot, bosses, hud, windows, overlays, meta} -> core ->
benilla-app facade`. No arrow between leaves (checked).

Notes on the choices:

- **`ui` is three crates**: `hud.rs` (2,450 lines) is bound tightly to `cards` (`Offer`, `Choice`,
  `describe_card`, `pick`) and `minimap` (`MiniMap`, `spawn_cluster`) and to `fill`; everything else
  UI-ish depends on `hud.rs` only through the kit that goes to core. Splitting these off costs nothing
  more than the kit move and makes each part 1,850 to 3,500 lines.
- **`bosses` is kept at 290 lines** although it is small: it is the stated example, and with
  `BossPlans` the schedule tuning stays inside it. If compile fixed overhead per crate matters, merge it
  into `survivors-combat`; the graph allows it (bosses reads `Enemy`, `spawn_enemy`, `NavGrid` from core
  and nothing from combat).
- **`evolutions` is a window**, not combat: nearly all of the file is `Spellbook`/`BookUi`/`show`/`build`.
  Only the rules (`holds`, `ready`, `evolved_from`, 26 lines) go to core because `hud::choices_for`
  needs them.
- **`meta` can be split four ways** (menu + difficulty UI, talents UI, trainer UI, worldbuffs) with
  one more `SystemSet` (`RefreshMenuUi`, for `trainer.rs:235`). The graph has no other edge between
  them. Not proposed now: they total 3,100 lines and are rarely edited together with core.
- **Cost of putting rules and tables in core**: an edit to `data.rs`, the talent trees (628-line `TREES`),
  `UPGRADES`, `MOBILITY` or `NavGrid` rebuilds core and all nine dependants in parallel. Two options
  to avoid this later, neither needed now: (a) a mid-layer `survivors-nav` crate for `NavGrid`
  (consumers: combat, bosses, world), useful while the pathfinding work continues; (b) the talents
  and trainer rules reached through a `Resource` of function pointers set by meta (`Profile::bonus`
  and `talent_bonus` are called only from `start_run`, which has three callers). Splitting `data` into its own
  crate gains nothing: every dependant needs it, so every dependant rebuilds on an edit either way.
- **The leaf crates never see each other.** Anything a leaf needs from another leaf is a core item or a
  `SystemSet`. The one place where hud and meta must see each other is the aggregator.

## Items to move into core

Beyond the modules that are core as a whole (`data.rs` 2,179 lines, `mods.rs` 60, `units.rs` 375,
`profile.rs` 408). `mod.rs` keeps `SurvivorsPlugin`, `plugin` wiring, `pause`, `unpause`, `reset_run`
(about 200 lines, to the aggregator) and `ACTIVE`/`active` (Task 2 of the plan). `lines` counts the item with its doc comments and
attributes. "closure" rows are private helpers a moved item cannot do without.

| from | lines | item | why it must be in core |
|---|---|---|---|
| `abilities.rs:18` | 31 | `Hit` (impl/struct) | used by combat, loot (+ aggregator) |
| `cards.rs:19` | 1 | `LEGENDARY` (const) | used by hud, windows |
| `chatlog.rs:53` | 29 | `ChatKind` (enum/impl) | used by bosses, combat, loot, overlays, windows |
| `chatlog.rs:84` | 18 | `ChatLine` (enum/impl) | used by bosses, combat, loot, meta, overlays, windows |
| `difficulty.rs:21` | 71 | `Difficulty` (enum/impl) | a field of `Run`; the pick-screen UI in meta reads it |
| `enemies.rs:21` | 21 | `Enemy` (struct) | used by bosses, combat, hud, loot, overlays, world (+ aggregator) |
| `enemies.rs:44` | 3 | `Elite` (struct) | used by combat, loot |
| `enemies.rs:61` | 5 | `Dot` (struct) | used by combat |
| `enemies.rs:69` | 3 | `Dying` (struct) | used by bosses, combat, loot |
| `enemies.rs:78` | 11 | `Loot` (struct) | used by combat, overlays (+ aggregator) |
| `enemies.rs:89` | 7 | `GemKind` (enum) | used by combat |
| `enemies.rs:97` | 7 | `Gem` (struct) | used by combat, loot (+ aggregator) |
| `enemies.rs:129` | 61 | `spawn_enemy` (fn) | used by bosses, combat |
| `enemies.rs:204` | 18 | `spawn_spot` (fn) | used by bosses, combat |
| `enemies.rs:500` | 3 | `mark_dying` (fn) | used by combat |
| `enemies.rs` | 11 | `:47` `ELITE_SCALE`, `:48` `ELITE_HP`, `:49` `ELITE_DAMAGE`, `:50` `ELITE_XP`, `:52` `ELITE_AURA`, `:125` `radius_for` | private helpers of `enemies.spawn_enemy` |
| `enemies.rs` | 8 | `:191` `ground`, `:198` `SPAWN_TRIES`, `:199` `SPAWN_FAN` | private helpers of `enemies.spawn_spot`; also used directly by combat |
| `evolutions.rs:51` | 9 | `ready` (fn) | used by hud |
| `evolutions.rs:61` | 6 | `evolved_from` (fn) | used by hud |
| `evolutions.rs` | 11 | `:40` `holds` | private helpers of `evolutions.ready` |
| `finale.rs:54` | 3 | `Finale` (struct) | used by bosses, loot |
| `finale.rs:58` | 3 | `Reaper` (struct) | used by bosses, loot |
| `finale.rs:96` | 6 | `Plan` (struct) | type of the new `BossPlans` resource that `enemies` reads (see "Boss schedule") |
| `focus.rs:17` | 54 | `Focus` (enum/impl) | used by combat, overlays |
| `focus.rs:74` | 4 | `Target` (struct) | used by combat, overlays |
| `hero.rs:15` | 9 | `Hero` (struct) | used by bosses, combat, hud, loot, overlays, world (+ aggregator) |
| `hero.rs:25` | 2 | `HERO_RADIUS` (const) | used by combat |
| `hero.rs:105` | 30 | `UiCover` (impl/struct) | used by overlays, world |
| `hero.rs:143` | 4 | `arena` (fn) | used by hud, world (+ aggregator) |
| `hero.rs:163` | 34 | `spawn_hero` (fn) | used by hud |
| `hero.rs:198` | 2 | `MINUTE_OF_DAY` (const) | used by hud, world |
| `hero.rs:211` | 4 | `AimPoint` (struct) | used by overlays, world (+ aggregator) |
| `hero.rs:522` | 15 | `hurt_hero` (fn) | used by combat |
| `hero.rs:627` | 4 | `reach` (fn) | used by combat, world |
| `hero.rs` | 9 | `:111` `UiNode` | private helpers of `hero.UiCover` |
| `hud.rs:27` | 1 | `DIM_GREY` (const) | used by hud |
| `hud.rs:28` | 1 | `GOLD_TONE` (const) | used by hud, meta, overlays, windows |
| `hud.rs:31` | 2 | `MenuRoot` (struct) | used by hud, meta, windows |
| `hud.rs:37` | 2 | `PauseRoot` (struct) | used by hud, windows |
| `hud.rs:117` | 4 | `OverPayout` (struct) | used by hud, meta |
| `hud.rs:128` | 25 | `UiArt` (impl/struct) | used by hud, loot, overlays |
| `hud.rs:208` | 2 | `HudFont` (struct) | used by hud, meta, overlays, windows |
| `hud.rs:210` | 3 | `rgb` (fn) | used by meta |
| `hud.rs:304` | 14 | `spell_name` (fn) | used by hud, loot, windows |
| `hud.rs:988` | 33 | `start_run` (fn) | used by hud, meta (+ aggregator) |
| `hud.rs:1121` | 15 | `looks_for` (fn) | used by hud, meta |
| `hud.rs:1893` | 12 | `grouped` (fn) | used by hud, meta, overlays, windows |
| `hud.rs:1917` | 8 | `num` (fn) | used by hud, meta, windows |
| `hud.rs:1925` | 3 | `pct` (fn) | used by hud, windows |
| `hud.rs` | 1 | `:26` `DIM` | private helpers of `hud.DIM_GREY`; also used directly by hud |
| `hud.rs` | 1 | `:20` `GOLD` | private helpers of `hud.GOLD_TONE`; also used directly by hud |
| `items.rs:74` | 10 | `drop_boost` (fn) | used by combat, loot |
| `items.rs:532` | 3 | `gear_lines` (fn) | used by windows |
| `items.rs` | 3 | `:530` `WORN` | private helpers of `items.gear_lines`; also used directly by loot |
| `menu.rs:42` | 9 | `MenuAction` (enum) | used by meta |
| `menu.rs:53` | 3 | `MenuBuilt` (struct) | used by meta, windows |
| `mobility.rs:12` | 1 | `LEARN_LEVEL` (const) | used by combat |
| `mobility.rs:39` | 4 | `for_class` (fn) | used by combat |
| `mobility.rs:147` | 8 | `MobilityState` (struct) | a field of `Run` (`mod.rs:310`) |
| `mobility.rs:154` | 17 | `Run` (impl) | `impl Run` must live with `Run`; `hud`, `charpanel`, `hero` call it (see "Inherent and trait impls") |
| `mobility.rs` | 13 | `:16` `Move` | private helpers of `mobility.Mobility`; also used directly by world |
| `mobility.rs` | 109 | `:29` `Mobility`, `:43` `MOBILITY` | private helpers of `mobility.for_class` |
| `mod.rs:195` | 10 | `Phase` (enum) | used by bosses, combat, hud, loot, meta, overlays, windows, world (+ aggregator) |
| `mod.rs:206` | 3 | `RunEntity` (struct) | used by combat, loot, world (+ aggregator) |
| `mod.rs:210` | 29 | `Selection` (impl/struct) | used by bosses, combat, hud, meta, overlays, windows, world (+ aggregator) |
| `mod.rs:240` | 25 | `Owned` (impl/struct) | used by hud (+ aggregator) |
| `mod.rs:268` | 14 | `Stats` (struct) | used by hud |
| `mod.rs:281` | 1 | `BASE_HP` (const) | only referenced by other core items |
| `mod.rs:282` | 1 | `BASE_SPEED` (const) | only referenced by other core items |
| `mod.rs:283` | 1 | `BASE_MAGNET` (const) | only referenced by other core items |
| `mod.rs:287` | 140 | `Run` (impl/struct) | used by bosses, combat, hud, loot, meta, overlays, windows, world (+ aggregator) |
| `mod.rs:430` | 32 | `Rng` (impl/struct) | used by bosses, combat, hud, loot, meta, overlays (+ aggregator) |
| `nav.rs:90` | 392 | `NavGrid` (impl/struct) | used by bosses, combat, world |
| `nav.rs:571` | 19 | `SpawnProbe` (impl/struct) | used by bosses, combat |
| `nav.rs` | 3 | `:65` `MIN_REACH` | private helpers of `nav.Field` |
| `nav.rs` | 3 | `:68` `STRAIGHT`, `:69` `DIAGONAL` | private helpers of `nav.NEIGHBOURS` |
| `nav.rs` | 88 | `:33` `CELL`, `:56` `FIELD_RADIUS`, `:58` `LOOKAHEAD`, `:60` `FLOW_EVERY`, `:62` `FREE_SEARCH`, `:71` `BUCKETS`, `:73` `UNREACHED`, `:75` `NEIGHBOURS`, `:110` `Field`, `:525` `ring_len`, `:534` `ring_cell` | private helpers of `nav.NavGrid`; also used directly by world |
| `nav.rs` | 25 | `:43` `BODY_MID`, `:564` `body_probe`, `:590` `solid_at` | private helpers of `nav.SpawnProbe`; also used directly by world |
| `nav.rs` | 5 | `:39` `BODY_RADIUS`, `:40` `BODY_BOTTOM`, `:41` `BODY_TOP` | private helpers of `nav.body_probe` |
| `options.rs:77` | 6 | `OptionsUi` (struct) | used by hud, windows |
| `talents.rs:32` | 2 | `FIRST_LEVEL` (const) | only referenced by other core items |
| `talents.rs:34` | 2 | `TIER_STEP` (const) | only referenced by other core items |
| `talents.rs:36` | 2 | `MAX_ROWS` (const) | used by meta |
| `talents.rs:40` | 50 | `Fx` (enum/impl) | `worldbuffs.rs:13` and `units::Fx` clash by name; keep as `core::talents::Fx` |
| `talents.rs:91` | 9 | `Talent` (struct) | only referenced by other core items |
| `talents.rs:100` | 9 | `t` (fn) | used by meta |
| `talents.rs:111` | 6 | `Spec` (struct) | used by meta |
| `talents.rs:117` | 5 | `ClassTrees` (struct) | only referenced by other core items |
| `talents.rs:125` | 628 | `TREES` (static) | only referenced by other core items |
| `talents.rs:755` | 5 | `specs` (fn) | used by meta |
| `talents.rs:762` | 17 | `key` (fn) | only referenced by other core items |
| `talents.rs:779` | 4 | `points_total` (fn) | used by meta |
| `talents.rs:784` | 4 | `needs` (fn) | used by meta |
| `talents.rs:789` | 8 | `rank` (fn) | used by meta |
| `talents.rs:798` | 7 | `spec_spent` (fn) | used by meta |
| `talents.rs:806` | 4 | `spent` (fn) | used by meta |
| `talents.rs:811` | 4 | `available` (fn) | used by meta |
| `talents.rs:817` | 19 | `can_spend` (fn) | used by meta |
| `talents.rs:836` | 18 | `spend` (fn) | used by meta |
| `talents.rs:855` | 6 | `respec` (fn) | used by meta |
| `talents.rs:862` | 10 | `mods_for` (fn) | called by `Profile::talent_bonus` (`profile.rs:247`) |
| `talents.rs:917` | 5 | `TalentsUi` (struct) | used by hud, meta (+ aggregator) |
| `trainer.rs:28` | 2 | `STARTERS` (const) | only referenced by other core items |
| `trainer.rs:30` | 2 | `CLASS_BASE_PRICE` (const) | only referenced by other core items |
| `trainer.rs:33` | 13 | `Upgrade` (struct) | used by meta |
| `trainer.rs:46` | 66 | `UPGRADES` (const) | used by meta |
| `trainer.rs:114` | 4 | `rank` (fn) | used by meta |
| `trainer.rs:119` | 5 | `next_price` (fn) | used by meta |
| `trainer.rs:125` | 11 | `spent` (fn) | used by meta |
| `trainer.rs:137` | 6 | `apply_ranks` (fn) | called by `Profile::bonus` (`profile.rs:241`) |
| `trainer.rs:144` | 5 | `class_price` (fn) | used by meta |
| `trainer.rs:191` | 5 | `TrainerUi` (struct) | used by hud, meta |

New items that do not exist yet (about 60 lines in core):

| item | purpose |
|---|---|
| `SurvivorsSet` | `Keys`, `RefreshResults`, `PlaceCamera`, `MoveEnemies` |
| `Autopilot { dir: Vec3 }` (Resource) | replaces `Auto.dir` (`auto.rs:35`) for `drive_hero` (`hero.rs:296,311`) |
| `BossPlans(Vec<Plan>)` (Resource) | replaces `finale::plan` for `enemies.rs:246` |
| `set_gear_lines(Vec<...>)` | the writer for `WORN` (`items.rs:539,701` write the static today) |
| core `plugin` | `init_state::<Phase>`, `Run`, `Rng`, `Selection`, `AimPoint`, the messages `Hit`/`Loot`/`ChatLine`, `profile::load_profile`, `award_run` |

The `WORN` static is a mirror the character window reads because "the window's line builder only gets
the run" (`items.rs:528`). The cleaner fix is a field on `Run`; the setter is the smaller edit.

## Extraction order

Core first, as one step: every edge in the SCC goes through it, and nothing else can move before.
Prep commits first, inside the monolith (or inside `survivors-app` after Task 2), each testable with
the existing 82 tests and the auto run:

1. `SurvivorsSet` and the six `.after/.before` sites (`difficulty.rs:122-129`, `options.rs:68`,
   `talents.rs:954`, `frames.rs:53-54`, `plates.rs:36-37`, `nav.rs:719`); the aggregator puts the four
   systems in the sets.
2. `Autopilot` split out of `Auto`.
3. `BossPlans`: `Plan` to core, `finale::plugin` inserts it, `spawn_enemies` reads it.
4. `set_gear_lines` for `WORN`.
5. The item moves above, still in one crate, as a `core/` sub-tree with the file layout kept as modules
   (`core::talents`, `core::trainer`, `core::units`, ...). Then the `git mv` into `survivors-core` is
   mechanical, and `pub(super)` to `pub` is a sed.

Then the leaves. After core every leaf depends on core only, so the order is about risk, not about
dependency, and the leaves can be done in parallel by one agent each. Edges that still point at
a leaf before its extraction (the reason for the order):

| leaf | incoming edges from other leaves (before prep) | outgoing edits | order |
|---|---|---|---|
| bosses | 1 (`enemies` -> `finale::plan`, fixed by `BossPlans`) | 3 markers + `Plan` to core, `plugin` inserts `BossPlans` | **1** |
| loot | 0 | `drop_boost`, `WORN` to core (setter); writes `Hit`, `ChatLine` (core) | 2 |
| windows | 0 | `gear_lines` to core, `.after(Keys)` | 3 |
| overlays | 0 | `.after(PlaceCamera/MoveEnemies)` | 4 |
| meta | 0 | 3 `.after(Keys/RefreshResults)`, 1.1k lines of talents/trainer to core | 5 |
| hud | 4 (difficulty, talents, options order after `hud::keys`/`refresh_results`) | 132 lines of kit, `start_run`, `looks_for` to core; owns two chains with meta | 6 |
| combat | 3 (frames, plates, nav order after `move_enemies`) | `Enemy`, spawn API, `Hit`, `Loot` to core; `Spawner`, `PendingHits` `pub` | 7 |
| world | 2 (frames, plates after `place_camera`) and the `Autopilot` edge | `NavGrid`, `Hero`, `UiCover`, mobility data to core; most `pub` fixes | 8 |

Reasons for the tail: hud, combat and world are named by the aggregator's two chains and have the most
`pub` promotions. `nav.rs`, `enemies.rs` and `hero.rs` are the files agents named `enemy-pathfinding`
and `hero-collision` in this session are probably editing; extract those leaves after that work is
committed.

**First leaf to extract: `survivors-bosses`.** 291 lines, one incoming edge, no private fields crossing,
no engine facade beyond `webenv` and `loading_screen`, and the `SurvivorsSet`/`BossPlans` prep it needs
is also needed by the rest.

## Tests

17 test modules, 82 `#[test]`s (the plan counted "34 or more"). They use `use super::*`, so they
see the imports of their file. After the split a test module moves with the code it tests;
evolutions is the only module that splits.

| file:lines | tests | uses | moves to |
|---|---|---|---|
| `abilities.rs:880-960` | 6 (`pick` target selection) | `pick`, `Focus` (core) | combat |
| `chatlog.rs:464-554` | 6 (aggregation, history, fade) | `ChatLog`, `AGG_SECS`, `MAX_LINES`, `VISIBLE`, `FADE_*`, `ChatKind` (core) | windows |
| `chests.rs:214-252` | 3 | `candidates`, `draw`, `banner` | loot |
| `difficulty.rs:313-354` | 3 | `Difficulty` | core |
| `evolutions.rs:324-425` | 6 | 4 on `holds` and `data::EVOLUTIONS` (`:400` uses `super::super::Owned`, becomes `crate::Owned`); 2 on `recipes`/`ROWS` and `Spellbook` | 4 to core, 2 to windows |
| `finale.rs:260-305` | 3 | `build`, `FINALE_HP` | bosses (stays because `build` stays) |
| `focus.rs:261-284` | 1 | `Focus` | core |
| `frames.rs:560-605` | 4 | `pick_index` | overlays |
| `hero.rs:631-671` | 2 | `drop_stray_buttons`, `RunSystemOnce`, window events | world |
| `items.rs:801-899` | 7 | `Quality`, `Slot`, `Gear`, `Bank`, `roll_item`, `pick_quality`, `Rng` (core) | loot |
| `nav.rs:726-975` | 17 | `NavGrid`, `Field`, `ring_cell`, `ring_len`, `FREE_SEARCH`, `FLOW_EVERY`, `FIELD_RADIUS`, `units::flat` | core |
| `pickups.rs:324-342` | 1 | `roll`, `CHANCE_*`, local `Kind` | loot |
| `profile.rs:354-408` | 4 | `Profile`, `Character`, `MetaBonus`, `xp_to_next` | core |
| `quests.rs:460-531` | 3 | `roll`, `QUESTS`, `Goal`, `creature`, `Rng` | overlays |
| `talents.rs:1551-1669` | 8 | `specs`, `spend`, `respec`, `mods_for`, `StatMods` | core (all talent rules move) |
| `trainer.rs:910-951` | 4 | `UPGRADES`, `next_price`, `apply_ranks`, `class_price`, `grouped` | core |
| `worldbuffs.rs:369-414` | 4 | `BUFFS`, `OFFERED`, `roll`, `Rng` (core) | meta |

Per crate: core 41 (difficulty 3, evolutions 4, focus 1, nav 17, profile 4, talents 8, trainer 4),
windows 8 (chatlog 6, evolutions 2), loot 11 (chests 3, items 7, pickups 1), overlays 7 (frames 4,
quests 3), combat 6, world 2, bosses 3, meta 4. Sum 82.

`data.rs`, `mods.rs`, `units.rs`, `hud.rs`, `menu.rs`, `cards.rs`, `minimap.rs`, `options.rs`, `plates.rs`, `grade.rs`,
`fill.rs`, `charpanel.rs`, `barrier.rs`, `mobility.rs`, `auto.rs`, `mod.rs` have no tests. The `hero.rs` tests need
`bevy::ecs::system::RunSystemOnce` and window events, so `survivors-world` needs `bevy` with the same
features as today; no other test needs a dev-dependency beyond what the file already imports.
