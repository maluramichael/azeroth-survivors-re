# Engine surface of `survivors/` (crate split, step 1: what survivors uses of benilla-app)

Read-only analysis, nothing built (no cargo run). Basis: `crates/benilla-app/src/survivors/` (42 files
incl. `mod.rs`), commit `2c6dd94`. Paths below are relative to `crates/benilla-app/src/` unless noted.

## Summary

- **75** engine items are named by `crate::...` (128 code occurrences, plus 13 in doc comments) in
  **28 of the survivors files**. 2 more items (`BackdropEdges`, `SetOutcome`) must become `pub` because they
  appear in the signature or a `pub` field of the items above (otherwise `private_interfaces` warns, and
  `scripts/gates.sh:109` runs clippy with `-D warnings`).
- **29 upstream files** need visibility changes (one token each, `pub(crate)` -> `pub`, or `mod x;` ->
  `pub(crate) mod x;`), plus `lib.rs` (entry point, see "Entry points") and optionally `shaders.rs`.
  50 fields and 10 methods of engine types must become `pub`.
- **No blockers**: no orphan impls, no engine macros, no `include_str!`/`env!` in survivors. The two non-trivial
  points are (a) four items are only reachable through private submodules or `pub(crate) use` re-exports
  (`BoneAttach`, `Player`, `GlueSound`, `ScriptedFacing`), fixed with a one-token `mod` visibility change each,
  and (b) the `survivors::ACTIVE` flag / `active()` is read in `lib.rs` (`launch`); it can disappear completely
  if `launch` receives the plugin as an argument (see "Entry points").
- Everything in `benilla-app` is a private `mod` at the crate root. Items private at the crate root are visible
  everywhere inside the crate, so a facade module at the crate root may name `crate::glue::...`,
  `crate::net::...` etc. For second level modules (`glue::art`, `entities::mount`, `creature_anim::select`,
  `minimap::blips`) the parent `mod` is `pub(crate)` (checked, column "parent module vis"), so the facade can
  name them too. Where the parent `mod` is plain `mod x;` (second level), the table says what to change.

### Rules that apply to every row

1. `pub use` of an item needs the item itself to be `pub` (nominal). `pub(crate)` + `pub use` fails with E0364.
   A `pub(crate) use inner::X;` re-export chain cannot be re-exported either: the facade must name the defining
   path, which has to be reachable (a `pub(crate) mod` hop) or the `pub(crate) use` must become `pub use`.
2. A struct made `pub` keeps its `pub(crate)` fields private to the crate; only fields/methods survivors touches
   are listed ("Fields and methods") and have to become `pub`. Tuple-struct constructors (`Guid(x)`) need the
   field `pub`.
3. A type that appears in the signature of a `pub` fn or in a `pub` field of a newly exposed item must be at least
   as visible (else `private_interfaces` warning). These are covered in the tables (marked "transitive").
4. Types of external crates (`benilla_protocol::EntityKind`, `benilla_formats::SpellCatalog`,
   `benilla_assets::WorldAssets`, ...) are already `pub` in their crates; nothing to do.

## Engine items by module

Legend: "root" = declared in `lib.rs` as private `mod x;` (visible crate-wide, fine for a facade at the crate
root). "change" lists the exact edit; `vis` -> `pub` means replace `pub(crate)` by `pub`.

### net (`mod net;` lib.rs:73, root)

| item | kind | defined at | current vis | parent module vis | change needed |
|---|---|---|---|---|---|
| `FieldChanged` | struct (Message), fields already `pub` | net.rs:343 | `pub(crate)` | root | struct -> `pub` |
| `Guid` | tuple struct (Component) | net.rs:273 | `pub(crate)`, field `pub(crate) u64` | root | struct -> `pub`, field `.0` -> `pub` |
| `ObjectStore` | tuple struct (Component) | net.rs:322 | `pub(crate)`, field `pub(crate) ObjectFields` | root | struct -> `pub`, field `.0` -> `pub` |
| `NetEntity` | struct (Component), built by struct literal | net.rs:277 | `pub(crate)`, fields `kind`, `display_id`, `scale` `pub(crate)` | root | struct -> `pub`, the 3 fields (net.rs:278, 279, 285) -> `pub` |
| `ScriptedFacing` | unit struct (Component), used as a value | net/motion/facing.rs:54 | `pub(crate)` | `mod motion;` net.rs:26 private, `mod facing;` net/motion.rs:10 private; re-exported `pub(crate)` at net/motion.rs:21 and net.rs:43-46 | struct -> `pub`; net.rs:26 `mod motion;` -> `pub(crate) mod motion;`; net/motion.rs:10 `mod facing;` -> `pub(crate) mod facing;`. Facade path: `crate::net::motion::facing::ScriptedFacing`. (Alternative: change both `pub(crate) use` lines to `pub use` after splitting `ScriptedFacing` out of the grouped lists; more lines touched.) |

Survivors also uses `benilla_protocol::messages::{ObjectFields, ObjectType}` and `benilla_protocol::EntityKind` (external crate, fine).

### ui_action (`mod ui_action;` lib.rs:121, root)

| item | kind | defined at | current vis | parent module vis | change needed |
|---|---|---|---|---|---|
| `Spells` | struct (Resource), read as `Res<Spells>`; field `.catalog` read | ui_action/mod.rs:87 | `pub(crate)`, `catalog` `pub(crate)` (ui_action/mod.rs:88) | root | struct -> `pub`, field `catalog` -> `pub` (type `benilla_formats::SpellCatalog` is external) |

### webenv (`pub mod webenv;` lib.rs:195)

| item | kind | defined at | current vis | parent module vis | change needed |
|---|---|---|---|---|---|
| `var` | fn | webenv.rs:17 (native), webenv.rs:38 (wasm32) | `pub` | `pub mod` | none. Callable as `benilla_app::webenv::var`; the facade mirrors it for a uniform rewrite. |

### loading_screen (`mod loading_screen;` lib.rs:67, root)

| item | kind | defined at | current vis | parent module vis | change needed |
|---|---|---|---|---|---|
| `LoadingScreen` | struct (Resource); only `.covering()` is called | loading_screen.rs:133 | `pub(crate)` | root | struct -> `pub`; method `covering` loading_screen.rs:171 -> `pub` (careful: loading_screen.rs:86 is a different `covering`, of another type) |

### glue (`mod glue;` lib.rs:60, root; submodules in glue/mod.rs)

| item | kind | defined at | current vis | parent module vis | change needed |
|---|---|---|---|---|---|
| `GlueClicks` | struct (Resource), `.hit(entity)` | glue/mod.rs:63 | `pub(crate)` | root | struct -> `pub`; method `hit` glue/mod.rs:66 -> `pub` |
| `GlueVisuals` | unit struct (SystemSet), `.before(GlueVisuals)` | glue/mod.rs:23 | `pub(crate)` | root | struct -> `pub` |

#### glue::art (`pub(crate) mod art;` glue/mod.rs:6)

| item | kind | defined at | current vis | parent module vis | change needed |
|---|---|---|---|---|---|
| `GlueArt` | struct (Resource) | glue/art.rs:80 | `pub(crate)` | `pub(crate)` | struct -> `pub`; 10 fields and 1 method, see "Fields and methods" |
| `class_tc` | fn | glue/art.rs:557 | `pub(crate)` | `pub(crate)` | -> `pub` |
| `race_tc` | fn | glue/art.rs:550 | `pub(crate)` | `pub(crate)` | -> `pub` |
| `tc_rect` | fn | glue/art.rs:579 | `pub(crate)` | `pub(crate)` | -> `pub` |
| `DIM` | const | glue/art.rs:17 | `pub(crate)` | `pub(crate)` | -> `pub` |
| `GOLD` | const | glue/art.rs:15 | `pub(crate)` | `pub(crate)` | -> `pub` |
| `NAME_EDGE` | const | glue/art.rs:39 | `pub(crate)` | `pub(crate)` | -> `pub` |

#### glue::widgets (`pub(crate) mod widgets;` glue/mod.rs:9)

| item | kind | defined at | current vis | parent module vis | change needed |
|---|---|---|---|---|---|
| `glue_button` | generic fn `<A: Component>`; takes `&GlueArt`, `GlueBtnKind` | glue/widgets.rs:693 | `pub(crate)` | `pub(crate)` | -> `pub` |
| `outlined_text` | generic fn; takes `GlueText` | glue/widgets.rs:98 | `pub(crate)` | `pub(crate)` | -> `pub` |
| `icon_button` | generic fn; takes `&GlueArt` | glue/widgets.rs:287 | `pub(crate)` | `pub(crate)` | -> `pub` |
| `abs` | fn | glue/widgets.rs:63 | `pub(crate)` | `pub(crate)` | -> `pub` |
| `overlay` | fn | glue/widgets.rs:75 | `pub(crate)` | `pub(crate)` | -> `pub` |
| `GlueBtnKind` | enum (`Normal`, `Small` used) | glue/widgets.rs:437 | `pub(crate)` | `pub(crate)` | -> `pub` (variants follow the enum) |
| `GlueDisabled` | tuple struct (Component), `.0` read and written | glue/widgets.rs:37 | `pub(crate)`, field `pub(crate)` | `pub(crate)` | struct -> `pub`, field `.0` -> `pub` |
| `GlueText<'a>` | struct literal, fields `text`, `size`, `color`, `wrap` already `pub` | glue/widgets.rs:88 | `pub(crate)` | `pub(crate)` | struct -> `pub` |
| `Hilight` | unit struct (Component) | glue/widgets.rs:19 | `pub(crate)` | `pub(crate)` | -> `pub` |
| `HoverLabel` | unit struct (Component) | glue/widgets.rs:30 | `pub(crate)` | `pub(crate)` | -> `pub` |
| `LockHighlight` | tuple struct (Component, Default), `.0` read and written | glue/widgets.rs:23 | `pub(crate)`, field `pub(crate)` | `pub(crate)` | struct -> `pub`, field `.0` -> `pub` |
| `GlueCaption` | unit struct (Component) | glue/widgets.rs:43 | `pub(crate)` | `pub(crate)` | -> `pub` |

#### glue::backdrop (`pub(crate) mod backdrop;` glue/mod.rs:7)

| item | kind | defined at | current vis | parent module vis | change needed |
|---|---|---|---|---|---|
| `backdrop_border` | fn; takes `&BackdropEdges` | glue/backdrop.rs:93 | `pub(crate)` | `pub(crate)` | -> `pub` |
| `backdrop_edges` | fn; returns `Option<BackdropEdges>` | glue/backdrop.rs:58 | `pub(crate)` | `pub(crate)` | -> `pub` |
| `tiled_bg_node` | fn | glue/backdrop.rs:225 | `pub(crate)` | `pub(crate)` | -> `pub` |
| `BackdropEdges` (transitive: type of `GlueArt.name_border` and in the two fns) | struct (Clone), not named by survivors | glue/backdrop.rs:16 | `pub(crate)` | `pub(crate)` | struct -> `pub` (fields stay) |

#### glue::add_material (`pub(crate) mod add_material;` glue/mod.rs:5)

| item | kind | defined at | current vis | parent module vis | change needed |
|---|---|---|---|---|---|
| `AddUiMaterial` | struct (Asset, UiMaterial), only as `Assets<AddUiMaterial>` (menu.rs:144); type of `GlueArt.select_highlight` | glue/add_material.rs:14 | `pub(crate)` | `pub(crate)` | struct -> `pub` (fields stay) |

### creature_anim (`mod creature_anim;` lib.rs:45, root)

| item | kind | defined at | current vis | parent module vis | change needed |
|---|---|---|---|---|---|
| `CastEvent` | struct (Message), built by literal | creature_anim.rs:348 | `pub(crate)`, fields `pub(crate)` | root | struct -> `pub`; fields `entity`, `spell_id`, `kind`, `seq` (349-353) -> `pub` |
| `CastEventKind` | enum (`Go`, `Impact { weapon_visual }`) | creature_anim.rs:358 | `pub(crate)` | root | -> `pub` |
| `PlaySeq` | tuple struct (Resource), `ResMut<PlaySeq>`, `.next()` | creature_anim.rs:337 | `pub(crate)`, field private | root | struct -> `pub`; method `next` creature_anim.rs:340 -> `pub` (the private `u64` field stays private) |
| `SpellGoTargets` | struct (Message), built by literal | creature_anim.rs:488 | `pub(crate)`, fields `pub(crate)` | root | struct -> `pub`; fields `caster`, `spell_id`, `hits`, `misses`, `dest`, `ammo_display_id`, `seq` (489-503) -> `pub` |
| `SwingMessage` | struct (Message), built by literal | creature_anim.rs:403 | `pub(crate)`, fields `pub(crate)` | root | struct -> `pub`; fields `attacker`, `victim`, `hit_info`, `victim_state`, `damage`, `displayed`, `seq` (404-416) -> `pub` |
| `MovementState` | struct (Component, Default); fields `speed`, `vertical_speed`, `flags` read/written | creature_anim/select.rs:177 | `pub(crate)`, fields `pub(crate)` | `pub(crate) mod select;` creature_anim.rs:18; re-exported `pub(crate) use select::{..}` creature_anim.rs:19-21 | struct -> `pub`; fields `speed` (179), `vertical_speed` (181), `flags` (183) -> `pub`. Facade names `crate::creature_anim::select::MovementState`, so the `pub(crate) use` line stays untouched. |
| `move_flags` | module of `pub const` (`FORWARD`, `BACKWARD`, `STRAFE_LEFT`, `STRAFE_RIGHT`, `FALLING`) | creature_anim/select.rs:65 | `pub(crate) mod` | `pub(crate) mod select;` | `pub(crate) mod move_flags` -> `pub mod move_flags` (consts inside are already `pub`) |

### local_state (`mod local_state;` lib.rs:68, root)

| item | kind | defined at | current vis | parent module vis | change needed |
|---|---|---|---|---|---|
| `home` | fn -> `Option<PathBuf>` | local_state.rs:26 | `pub(crate)` | root | -> `pub` |
| `read_to_string` | fn | local_state.rs:272 | `pub(crate)` | root | -> `pub` |
| `write_atomic` | fn | local_state.rs:242 | `pub(crate)` | root | -> `pub` |

### vplates (`mod vplates;` lib.rs:188, root)

| item | kind | defined at | current vis | parent module vis | change needed |
|---|---|---|---|---|---|
| `con_color` | fn | vplates.rs:140 | `pub(crate)` | root | -> `pub` |
| `gx_px` | fn | vplates.rs:181 | `pub(crate)` | root | -> `pub` |
| `plate_basis` | fn | vplates.rs:171 | `pub(crate)` | root | -> `pub` |
| `text_px` | fn | vplates.rs:188 | `pub(crate)` | root | -> `pub` |
| `PLATE_FRIENDLY` | const | vplates.rs:114 | `pub(crate)` | root | -> `pub` |
| `CON_YELLOW` | const | vplates.rs:134 | `pub(crate)` | root | -> `pub` |
| `PLATE_W` / `PLATE_H` | const | vplates.rs:85 / 86 | `pub(crate)` | root | -> `pub` |
| `BAR_OFF_X` / `BAR_OFF_Y` / `BAR_W` / `BAR_H` | const | vplates.rs:88 / 89 / 90 / 91 | `pub(crate)` | root | -> `pub` |
| `LEVEL_OFF_X` / `LEVEL_OFF_Y` / `LEVEL_H` | const | vplates.rs:97 / 98 / 96 | `pub(crate)` | root | -> `pub` |

### entities (`mod entities;` lib.rs:56, root; entities.rs + entities/ directory)

| item | kind | defined at | current vis | parent module vis | change needed |
|---|---|---|---|---|---|
| `Creatures` | struct (Resource); only `.display_race_sex(id)` is called | entities.rs:234 | `pub(crate)` | root | struct -> `pub`; method `display_race_sex` entities.rs:246 -> `pub` |
| `OverheadFallback` | tuple struct (Component), only in `Query<&OverheadFallback>` | entities.rs:132 | `pub(crate)` | root | -> `pub` |
| `overhead_anchor` | generic fn; takes `&Query<&BoneAttach>`, `&Query<&OverheadFallback>`, `&Query<(), With<MountChild>>` | entities.rs:176 | `pub(crate)` | root | -> `pub` |
| `BoneAttach` | struct (Component), only in `Query<&BoneAttach>` | entities/equipment/mod.rs:187 | `pub(crate)` | `mod equipment;` entities.rs:40 is **private**; `pub(crate) use equipment::{BoneAttach, Equipment}` entities.rs:91 | struct -> `pub`; entities.rs:40 `mod equipment;` -> `pub(crate) mod equipment;`. Facade path `crate::entities::equipment::BoneAttach` (a `pub use` from the entities.rs:91 re-export would fail: `Equipment` is `pub(crate)` too). |
| `MountChild` | tuple struct (Component), only in `With<MountChild>` | entities/mount.rs:29 | `pub(crate)` | `pub(crate) mod mount;` entities.rs:49 | struct -> `pub` |

### Single-item modules

| item | kind | defined at | current vis | parent module vis | change needed |
|---|---|---|---|---|---|
| `ui_pass::project_overlay` | fn | ui_pass.rs:336 | `pub(crate)` | `mod ui_pass;` lib.rs:158 root | -> `pub` |
| `player::Player` | struct (Resource); fields `active`, `detached`, `settling`, `world_stale`, `pos` written/read (hero.rs:401-405, 442) | player/state.rs:287 | `pub(crate)`, those 5 fields `pub(crate)` (state.rs:288, 305, 344, 353, 320) | `mod player;` lib.rs:79 root; `mod state;` player.rs:62 **private**; `pub(crate) use state::{Player, ..}` player.rs:93 | struct -> `pub`; the 5 fields -> `pub`; player.rs:62 `mod state;` -> `pub(crate) mod state;`. Facade path `crate::player::state::Player`. Other fields (`pub(super) modes` ...) stay. |
| `char_select::wow_font` | fn | char_select/mod.rs:853 | `pub(crate)` | `mod char_select;` lib.rs:36 root | -> `pub` |
| `char_select::race_name` | fn | char_select/mod.rs:859 | `pub(crate)` | root | -> `pub` |
| `area::AreaTableRes` | tuple struct (Resource), `.0` used (`.get`, `.name`) | area.rs:45 | `pub(crate)`, field `pub(crate)` | `mod area;` lib.rs:23 root | struct -> `pub`, field `.0` -> `pub` (type `benilla_formats::AreaTableCatalog` is external) |
| `minimap::blips::PLAYER_ARROW_TEXTURE` | const &str | minimap/blips/mod.rs:63 | `pub(crate)` | `mod minimap;` lib.rs:70 root; `pub(crate) mod blips;` minimap/mod.rs:12 | -> `pub` |
| `run_mode::CaptureMode` | unit struct (Resource), inserted as a value | run_mode.rs:14 | `pub(crate)` | `mod run_mode;` lib.rs:92 root | -> `pub` |
| `cvars::Cvars` | struct (Resource), methods `get`, `num`, `flag`, `set`, `commit_latched` called | cvars/mod.rs:121 | `pub(crate)` | `mod cvars;` lib.rs:48 root | struct -> `pub`; 5 methods, see below |
| `cvars::SetOutcome` (transitive: return type of `Cvars::set`) | enum | cvars/mod.rs:106 | `pub(crate)` | root | -> `pub` |
| `video::windowed_env` | fn | video.rs:28 | `pub(crate)` | `mod video;` lib.rs:187 root | -> `pub` |
| `video::parse_resolution` | fn | video.rs:83 | `pub(crate)` | root | -> `pub` |
| `sound::GlueSound` | tuple struct (Message), built as `GlueSound("QUESTCOMPLETED")`, `add_message::<GlueSound>()` | sound/glue.rs:30 | `pub(crate)`, field `pub(crate) &'static str` | `mod sound;` lib.rs:105 root; `mod glue;` sound/mod.rs:20 **private**; `pub(crate) use glue::GlueSound` sound/mod.rs:58 | struct -> `pub`, field `.0` -> `pub`; sound/mod.rs:20 `mod glue;` -> `pub(crate) mod glue;`. Facade path `crate::sound::glue::GlueSound`. |
| `combat_text::CombatTextSpawn` | struct (Message), built by literal | combat_text/mod.rs:68 | `pub(crate)`, 4 fields `pub(crate)` | `mod combat_text;` lib.rs:41 root | struct -> `pub`; fields `anchor`, `text`, `category`, `color` (69-74) -> `pub` |

Doc-comment only (no code dependency, so no facade entry; the intra-doc links will simply not resolve in
survivors-app and should be reworded to plain code spans): `crate::local_state` (evolutions.rs:3, profile.rs:4),
`crate::vplates` (hud.rs:1681), `crate::glue` (menu.rs:2), `crate::minimap` (minimap.rs:8),
`crate::run_survivors` (mod.rs:2, 46), `crate::run_mode::CaptureMode` (mod.rs:5), `crate::sound::SoundConfig`
(options.rs:6), `crate::video` (options.rs:7, 12), `crate::video::window_mode` (options.rs:163),
`crate::video::on_cvar` (options.rs:169).

## Fields and methods

Everything survivors reads, writes or constructs on an engine type, and what has to become `pub`.

| type | access in survivors | needs `pub` (def line) |
|---|---|---|
| `Guid` | `Guid(guid)` (units.rs:99, 135, 169), `guid.0` (hero.rs:592, 599; abilities.rs:848, 874; enemies.rs:479) | field `.0` (net.rs:273) |
| `ObjectStore` | `ObjectStore(ObjectFields::from_pairs(..))` (units.rs:105, 141, 175), `store.0.merge(..)` (units.rs:151-153, 287) | field `.0` (net.rs:322) |
| `NetEntity` | literal `{ kind, display_id, scale }` (units.rs:100, 136, 170) | `kind` (net.rs:278), `display_id` (279), `scale` (285) |
| `FieldChanged` | literal `{ entity, guid, kind, index, old, new }` (units.rs:288) | none, fields already `pub` |
| `Spells` | `spells.catalog.get(id)` then `.name`, `.speed`, `.icon` (abilities.rs:264, 694; hud.rs:313, 1561) | `catalog` (ui_action/mod.rs:88) |
| `LoadingScreen` | `.covering()` (9 `run_if`s, auto.rs, barrier.rs, nav.rs ...) | method `covering` (loading_screen.rs:171) |
| `GlueArt` | `.ensure_loaded(&mut WorldAssets, &mut Assets<Image>, &mut Assets<AddUiMaterial>)` (menu.rs:149); fields read: `races`, `classes`, `gender`, `banners`, `logo`, `tower_bg`, `tower_border`, `tooltip_bg`, `name_border`, `select_highlight` (menu.rs 155-476, 709; difficulty/options/talents/trainer read `name_border`, `tooltip_bg`) | method `ensure_loaded` (glue/art.rs:192); fields at glue/art.rs:82, 83, 84, 86, 88, 97, 98, 111, 110, 115 |
| `GlueClicks` | `.hit(entity)` (7 files) | method `hit` (glue/mod.rs:66) |
| `GlueDisabled` | `.0` read and written (menu.rs:666, 985; difficulty.rs, options.rs, talents.rs, trainer.rs queries) | field (glue/widgets.rs:37) |
| `LockHighlight` | `LockHighlight::default()` (menu.rs:465), `.0` read and written (menu.rs:660) | field (glue/widgets.rs:23) |
| `GlueText` | literal `{ text, size, color, wrap }` | none, fields already `pub` |
| `MovementState` | `::default()` (units.rs:106), `.speed`, `.flags`, `.vertical_speed` (hero.rs:343-385, enemies.rs:374-446) | fields at creature_anim/select.rs:179, 181, 183 |
| `CastEvent` | literal (units.rs:316, 336) | fields at creature_anim.rs:349-353 |
| `SpellGoTargets` | literal (units.rs:322) | fields at creature_anim.rs:489-503 |
| `SwingMessage` | literal (units.rs:349) | fields at creature_anim.rs:404-416 |
| `PlaySeq` | `seq.next()` (units.rs:315, 335, 348) | method `next` (creature_anim.rs:340) |
| `CombatTextSpawn` | literal (units.rs:363) | fields at combat_text/mod.rs:69-74 |
| `Creatures` | `.display_race_sex(d)` (hud.rs:1127) | method (entities.rs:246) |
| `Player` | `.active`, `.detached`, `.settling`, `.world_stale`, `.pos` (hero.rs:401-405, 442) | fields at player/state.rs:288, 305, 344, 353, 320 |
| `AreaTableRes` | `areas.0.get(..)`, `areas.0.name(..)` (minimap.rs:326-337) | field `.0` (area.rs:45) |
| `Cvars` | `.get`, `.num`, `.flag`, `.set`, `.commit_latched` (options.rs:154-184, 600-604) | methods at cvars/mod.rs:192, 196, 201, 291, 329 |
| `GlueSound` | `GlueSound("QUESTCOMPLETED")` (quests.rs:317) | field `.0` (sound/glue.rs:30) |

Counts: 50 fields (Guid 1, ObjectStore 1, NetEntity 3, Spells 1, GlueArt 10, GlueDisabled 1, LockHighlight 1,
MovementState 3, CastEvent 4, SpellGoTargets 7, SwingMessage 7, CombatTextSpawn 4, Player 5, AreaTableRes 1,
GlueSound 1) and 10 methods (`covering`, `hit`, `ensure_loaded`, `next`, `display_race_sex`, and the 5 of `Cvars`).

Types reached only through fields of the above that survivors uses as values, never names (`ArrowArt`,
`ScrollArt`, ... in the unused `GlueArt` fields) need no change: those fields stay `pub(crate)`.

## Orphan impls

**None.** Every `impl` in survivors (`grep -n "^\s*impl"`: 54 hits) has a survivors-local self type or trait:
`impl Plugin for SurvivorsPlugin` (mod.rs:58), `impl UiMaterial for FillMaterial` (fill.rs:26) and
`for SweepUiMaterial` (hud.rs:86), `impl FullscreenMaterial for Grade` (grade.rs:45), `impl Default for ...`
on local types, and inherent impls on local types (`Run`, `Profile`, `NavGrid`, `UiCover<'_, '_>` hero.rs:120,
`SpawnProbe` nav.rs:576, `Fx<'_>` units.rs:312, `Marks`, ...). Bevy traits on local types are fine in another
crate. No `impl EngineType { }`, no extension traits.

## Macros and paths

- Engine macros: **none**. The only `macro_rules!` is `ability!` (data.rs:119), local to survivors and used
  there (83 call sites); `format!`/`assert!`/`matches!`/`warn!`/`info!` are std/bevy. `#[macro_use]`: none.
- `include_str!`, `include_bytes!`, `env!`, `option_env!`, `CARGO_MANIFEST_DIR`, `BENILLA_PROJECT_DIR` in
  survivors: **none** (the only `cfg!` is `cfg!(target_arch = "wasm32")` at options.rs:31, target-based, same
  in any crate). `#[cfg(feature = ...)]` in survivors: none (so no `dev` feature needed in survivors-app).
  The launcher shims keep `env!("BENILLA_*")`.
- **Embedded shaders**, the one path that is crate-relative: fill.rs:28 loads
  `embedded://benilla_app/shaders/survivors_fill.wgsl`, grade.rs:47 `.../survivors_grade.wgsl`, hud.rs:88
  `.../ui_sweep.wgsl`. They are registered by `embedded_asset!` in `shaders.rs:16-17` (survivors) and
  `shaders.rs:15` (sweep), files in `src/shaders/`. Two options:
  - **A (recommended for the split, zero upstream edits):** leave the shaders and `shaders.rs` where they are.
    The strings keep working because the registry is `benilla_app/...`. Cost: editing a survivors shader
    still recompiles benilla-app (rare).
  - **B (later):** `git mv` the two wgsl files to `crates/survivors-app/src/shaders/`, register them with
    `embedded_asset!(app, "shaders/survivors_grade.wgsl")` from a file directly in `survivors-app/src/`
    (not from a submodule, see the comment at the top of `shaders.rs`), change the two strings to
    `embedded://survivors_app/shaders/survivors_*.wgsl`, and in upstream `shaders.rs` delete lines 16-17 and
    change `assert_eq!(found, 8, ...)` to `6` (shaders.rs test). `ui_sweep.wgsl` stays in benilla-app either way.
- **Paths inside survivors that change on the move:**
  - `crate::survivors::hero::place_camera` (plates.rs:36) -> `crate::hero::place_camera` (if `survivors/mod.rs`
    becomes `survivors-app/src/lib.rs`).
  - `super::super::Owned` (evolutions.rs:400, inside a `tests` module): still resolves to the crate root, which
    holds `Owned` after the move; no change.
  - **Name collision to watch:** survivors has its own `minimap` module; the engine path
    `crate::minimap::blips::PLAYER_ARROW_TEXTURE` (minimap.rs:404) must be rewritten to the facade, not
    left as `crate::minimap::...` (it would silently point at survivors' minimap and fail to compile). All 128
    `crate::<engine module>::` occurrences have to be rewritten; counts by module: glue 30, local_state 15,
    entities 14, ui_action 14, webenv 13, loading_screen 9, net 5, vplates 5, creature_anim 4, video 4,
    char_select 3, sound 3, player 2, ui_pass 2, area 1, combat_text 1, cvars 1, minimap 1, run_mode 1.
  - A mechanical rewrite works with the mirrored facade below:
    `crate::(area|char_select|combat_text|creature_anim|cvars|entities|glue|loading_screen|local_state|minimap|net|player|run_mode|sound|ui_action|ui_pass|video|vplates|webenv)::`
    -> `benilla_app::survivors_host::$1::` (or `use benilla_app::survivors_host as host;` and `host::$1::`).
    Apply it only to lines that are not doc comments.
- External crates survivors uses directly (for `survivors-app/Cargo.toml`): `bevy` (workspace), `benilla-app`,
  `benilla-world` (16 files), `benilla-assets` (9 files), `benilla-ui` (hud.rs, plates.rs: `script::nameplate`),
  `benilla-formats` (minimap.rs), `benilla-protocol` (units.rs), `serde` (derive) and `toml` (4 files),
  `web-time` (mod.rs:434). `image` hits in the grep are `bevy::image`. All with `default-features = false` where
  the workspace does (benilla-app's own manifest shows the pattern); `serde = { version = "1", features = ["derive"] }`,
  `toml = "0.8"` as in benilla-app.

## benilla-app references to survivors

Outside `survivors/` there is exactly one file: `lib.rs`. (Also `shaders.rs:16-17` and a comment in
`target/hover.rs:286`, neither a code dependency.)

| lib.rs line | text | what it needs for the split |
|---|---|---|
| 109-110 | `/// Warcraft Survivors: ...` `mod survivors;` | delete; add `pub mod survivors_host;` (near `pub mod webenv;`, line 195) |
| 229-232 | `pub fn run_survivors(build: BuildId) -> AppExit { survivors::ACTIVE.store(true, ...); launch(build, None) }` | new signature, see "Entry points" |
| 305-308 | `let survivors = survivors::active(); if survivors { benilla_world::world_map::set_start_map(1); }` | `let survivors = survivors_plugin.is_some();` (engine keeps the Kalimdor start map) |
| 344-348 | window title `"Warcraft Survivors"` vs `"benilla"` | unchanged (uses the bool) |
| 436-442 | `connect: !capturing && !survivors`, `start: if survivors { char_select::ClientState::InWorld } else { run_mode::start_state() }` | unchanged (uses the bool) |
| 443-445 | `if survivors { app.add_plugins(survivors::SurvivorsPlugin); }` | `if let Some(add) = survivors_plugin { add(&mut app); }` |

So the active flag does **not** have to stay in benilla-app as a static: nothing outside `lib.rs` reads
`survivors::ACTIVE` / `survivors::active()` (grep over `crates/`, `web/`, `scripts/`, `.github/`: no other hit),
and `launch` can derive the bool from "was a survivors plugin passed". Consequence: the
`WOW_SURVIVORS` env / `?survivors=1` fallback in `survivors/mod.rs:51-54` is dropped (it could only turn the mode
on in a binary that has no plugin linked). `survivors::SurvivorsPlugin` also does `insert_resource(CaptureMode)`
first thing, which still happens inside the plugin, before `OnEnter(InWorld)`.

Nothing else in benilla-app reads `CaptureMode` conditionally on survivors; engine systems keep seeing the same
resource as today.

## Entry points

Current:

- `crates/benilla-app/src/lib.rs:230`: `pub fn run_survivors(build: BuildId) -> AppExit`
  (sets `survivors::ACTIVE`, then `launch(build, None)`); `launch(build, extend: Option<Extension<'_>>)` is
  private and non-generic on purpose ("a launcher compiles none of it"), `type Extension<'a> = Box<dyn FnOnce(&mut App) + 'a>`.
- `crates/warcraft-survivors/src/main.rs:75` and `crates/wenilla/src/lib.rs:30`:
  `benilla_app::run_survivors(BuildId { version, describe, sha, short, date, profile, project_dir, ..Default::default() })`
  (`wenilla` discards the `AppExit`).
- There is already the seam `run_with(build, extend)` (lib.rs:222) for "a crate on top", but it runs `extend`
  late (after `WebBridgePlugin`, the asset loaders, `webprogress`) and marks the build `extended` in the banner.
  Survivors needs the plugin at today's position and the banner unchanged, hence a dedicated parameter.

Proposed (all in `lib.rs`; the diff is about 15 lines):

```rust
pub fn run(build: BuildId) -> AppExit {
    launch(build, None, None)
}

pub fn run_with(build: BuildId, extend: impl FnOnce(&mut App)) -> AppExit {
    launch(build, Some(Box::new(extend)), None)
}

/// Warcraft Survivors: the client booted server-less into a top-down survival mode ... `survivors` is the
/// mode's plugin (the `survivors-app` crate's `SurvivorsPlugin`), added right after the game plugins.
pub fn run_survivors(build: BuildId, survivors: impl Plugin) -> AppExit {
    launch(
        build,
        None,
        Some(Box::new(move |app: &mut App| {
            app.add_plugins(survivors);
        })),
    )
}

fn launch(build: BuildId, extend: Option<Extension<'_>>, survivors_plugin: Option<Extension<'_>>) -> AppExit {
    ...
    let survivors = survivors_plugin.is_some();          // was: survivors::active()
    ...
    if let Some(add) = survivors_plugin {                // was: if survivors { app.add_plugins(survivors::SurvivorsPlugin); }
        add(&mut app);
    }
```

Everything else in `launch` stays (the `survivors` bool keeps its name, so lines 306, 344, 436-437 are
untouched). `build.extended` is still `extend.is_some()`, so the banner is unchanged.

Callers (the plugin type is `survivors_app::SurvivorsPlugin`, `pub struct` at the survivors-app crate root,
today `pub(crate) struct SurvivorsPlugin` at survivors/mod.rs:56):

```rust
benilla_app::run_survivors(BuildId { ... }, survivors_app::SurvivorsPlugin)
```

in `crates/warcraft-survivors/src/main.rs:75` and `crates/wenilla/src/lib.rs:30`; each Cargo.toml gets
`survivors-app = { path = "../survivors-app", default-features = false }` (no bevy dependency needed in the
shims). `survivors/mod.rs` loses `ACTIVE`, `active()` (lines 46-54) and the unused `crate::webenv` call in them.
Cargo.lock gains the new package entry.

## Facade draft

Literal content for `crates/benilla-app/src/survivors_host.rs`, plus `pub mod survivors_host;` in `lib.rs`
(next to `pub mod webenv;`). It mirrors the engine module paths, so the survivors-app rewrite is a pure prefix
replacement (`crate::glue::art::GlueArt` -> `benilla_app::survivors_host::glue::art::GlueArt`) and the
`crate::minimap` collision cannot occur. Paths use the defining module where a `pub(crate) use` chain would
block a re-export (`equipment`, `state`, `glue`, `motion::facing`), which is why those five `mod` lines become
`pub(crate) mod`.

```rust
//! The engine surface the Warcraft Survivors crate (`survivors-app`) builds on. Fork-owned: every line
//! here is a `pub use` of an item that is `pub` in its home module, so upstream files change by a
//! visibility keyword only. The module layout mirrors this crate's, so a `crate::glue::art::GlueArt` in
//! the survivors tree is `benilla_app::survivors_host::glue::art::GlueArt` there.

pub mod area {
    pub use crate::area::AreaTableRes;
}

pub mod char_select {
    pub use crate::char_select::{race_name, wow_font};
}

pub mod combat_text {
    pub use crate::combat_text::CombatTextSpawn;
}

pub mod creature_anim {
    pub use crate::creature_anim::select::{move_flags, MovementState};
    pub use crate::creature_anim::{
        CastEvent, CastEventKind, PlaySeq, SpellGoTargets, SwingMessage,
    };
}

pub mod cvars {
    pub use crate::cvars::{Cvars, SetOutcome};
}

pub mod entities {
    pub use crate::entities::equipment::BoneAttach;
    pub use crate::entities::{overhead_anchor, Creatures, OverheadFallback};

    pub mod mount {
        pub use crate::entities::mount::MountChild;
    }
}

pub mod glue {
    pub use crate::glue::{GlueClicks, GlueVisuals};

    pub mod add_material {
        pub use crate::glue::add_material::AddUiMaterial;
    }

    pub mod art {
        pub use crate::glue::art::{
            class_tc, race_tc, tc_rect, GlueArt, DIM, GOLD, NAME_EDGE,
        };
    }

    pub mod backdrop {
        pub use crate::glue::backdrop::{
            backdrop_border, backdrop_edges, tiled_bg_node, BackdropEdges,
        };
    }

    pub mod widgets {
        pub use crate::glue::widgets::{
            abs, glue_button, icon_button, outlined_text, overlay, GlueBtnKind, GlueCaption,
            GlueDisabled, GlueText, Hilight, HoverLabel, LockHighlight,
        };
    }
}

pub mod loading_screen {
    pub use crate::loading_screen::LoadingScreen;
}

pub mod local_state {
    pub use crate::local_state::{home, read_to_string, write_atomic};
}

pub mod minimap {
    pub mod blips {
        pub use crate::minimap::blips::PLAYER_ARROW_TEXTURE;
    }
}

pub mod net {
    pub use crate::net::motion::facing::ScriptedFacing;
    pub use crate::net::{FieldChanged, Guid, NetEntity, ObjectStore};
}

pub mod player {
    pub use crate::player::state::Player;
}

pub mod run_mode {
    pub use crate::run_mode::CaptureMode;
}

pub mod sound {
    pub use crate::sound::glue::GlueSound;
}

pub mod ui_action {
    pub use crate::ui_action::Spells;
}

pub mod ui_pass {
    pub use crate::ui_pass::project_overlay;
}

pub mod video {
    pub use crate::video::{parse_resolution, windowed_env};
}

pub mod vplates {
    pub use crate::vplates::{
        con_color, gx_px, plate_basis, text_px, BAR_H, BAR_OFF_X, BAR_OFF_Y, BAR_W, CON_YELLOW,
        LEVEL_H, LEVEL_OFF_X, LEVEL_OFF_Y, PLATE_FRIENDLY, PLATE_H, PLATE_W,
    };
}

pub mod webenv {
    pub use crate::webenv::var;
}
```

### The `mod` visibility edits the facade relies on (5 lines)

| file:line | now | after |
|---|---|---|
| net.rs:26 | `mod motion;` | `pub(crate) mod motion;` |
| net/motion.rs:10 | `mod facing;` | `pub(crate) mod facing;` |
| entities.rs:40 | `mod equipment;` | `pub(crate) mod equipment;` |
| player.rs:62 | `mod state;` | `pub(crate) mod state;` |
| sound/mod.rs:20 | `mod glue;` | `pub(crate) mod glue;` |

(`net` needs two hops.) Plus `creature_anim/select.rs:65`: `pub(crate) mod move_flags` -> `pub mod move_flags`.

### Upstream files touched (29, visibility only) and `lib.rs`

`net.rs`, `net/motion.rs`, `net/motion/facing.rs`, `ui_action/mod.rs`, `loading_screen.rs`, `glue/mod.rs`,
`glue/art.rs`, `glue/widgets.rs`, `glue/backdrop.rs`, `glue/add_material.rs`, `creature_anim.rs`,
`creature_anim/select.rs`, `local_state.rs`, `vplates.rs`, `entities.rs`, `entities/equipment/mod.rs`,
`entities/mount.rs`, `ui_pass.rs`, `player.rs`, `player/state.rs`, `char_select/mod.rs`, `area.rs`,
`minimap/blips/mod.rs`, `run_mode.rs`, `cvars/mod.rs`, `video.rs`, `sound/mod.rs`, `sound/glue.rs`,
`combat_text/mod.rs`. Not visibility: `lib.rs` (entry point), optional `shaders.rs` (option B).

### Caveats (not verified by a build; this analysis ran no cargo)

- E0364/`private_interfaces` expectations above come from the language rules, not from a compile. After the
  edits run `cargo check -p benilla-app` first (facade + visibilities), then `cargo check --workspace
  --all-targets` and the wasm check from `AGENTS.md`. If `private_interfaces` fires for a type this list missed,
  the fix is `pub` on that type (it is only a nominal change).
- `cargo clippy -D warnings` (gates.sh:109) makes any such warning an error; there should be none by
  construction (rule 3), but it is the first thing to look at when the gate fails.
