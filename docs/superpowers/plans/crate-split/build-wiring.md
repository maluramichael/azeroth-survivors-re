# Crate split: Cargo wiring, features, wasm, CI

Read-only analysis, 2026-10-31, tree at `2c6dd94`. Nothing was built. All facts below come from
reading `Cargo.toml` files, `.cargo/config.toml`, the build scripts, the workflows and the
survivors sources. Things I could not measure without cargo are marked **unmeasured**.

## Summary

- **No blockers.** Every external dependency survivors uses is already in `Cargo.lock`, no
  `build.rs` or `env!` is involved, and the only `cfg` inside survivors is `target_arch = "wasm32"`
  in `options.rs` and `#[cfg(test)]`.
- **Hard rule for the entry crates:** `survivors-app` must be taken with `default-features = false`
  in `warcraft-survivors` and in `wenilla`. If `survivors-app` has `default = ["dev"]` (it should,
  see section 2) and `wenilla` omits `default-features = false`, the web page gets
  `benilla-app/dev` (debug panel, instruments, a baked source path).
- Feature chain: `warcraft-survivors/dev` -> `survivors-app/dev` -> `survivors-core/dev` and each
  leaf's `dev` -> `benilla-app/dev` -> `benilla-assets/dev`, `benilla-formats/dev`,
  `benilla-world/dev`, `dep:bevy_egui`. Render backend: `wenilla/webgpu|webgl2` ->
  `survivors-app/webgpu|webgl2` -> `benilla-app/webgpu|webgl2` -> `bevy/*`, `benilla-world/*`.
- The workspace `members = ["crates/*"]` glob picks the new crates up; no `[workspace]` edit and no
  profile edit is needed. `Cargo.lock` gains the new packages (commit it).
- Link speed: nothing exotic slows the link (macOS, Apple `ld`, no LTO, no debuginfo). The
  binary is 235 MB because it statically links `libbenilla_app` (175 MB rlib). There is no big
  cheap link win; one experiment is listed in section 6.

## 1. Workspace `Cargo.toml`

- `members = ["crates/*"]`, no `default-members`, resolver 2. New crates under `crates/` are
  members automatically. Add nothing.
- `[workspace.dependencies]` today: `anyhow`, `bevy` (default features off, see below), `wgpu`,
  `clap`, `sha1`. Survivors needs only `bevy` from it. Do not add entries: the rule in the file
  header is "a version two crates share is pinned in `[workspace.dependencies]`", but
  `serde`, `toml`, `avian3d`, `web-time` are per-crate in every existing crate (benilla-app,
  benilla-world, benilla-assets), and moving them would touch upstream manifests. Declare them
  per crate with the same spec.
- `[workspace.lints.clippy]` `too_many_arguments = "allow"`: new crates need
  `[lints] workspace = true` (benilla-app and the launchers have it).
- `[patch.crates-io]`: `lua-src`, `mlua-sys`, `kira`. Workspace-wide, nothing to add.
- Profiles, all of them:
  - `[profile.dev.package."*"] opt-level = 3`. `"*"` matches non-workspace-member packages only,
    so it does not apply to our crates and does not need an entry for the new ones.
  - `[profile.dev] opt-level = 1`: applies to every member including the new crates.
  - `[profile.ship]` (inherits release, fat LTO, 1 CGU): all members.
  - `[profile.play]` (inherits release, `incremental = true`, `lto = "off"`): all members. Release
    itself is not customised (opt-level 3, `debug = false`, `codegen-units` default 16, which
    incremental raises to 256).
  - There are **no per-package overrides** for benilla-app or any other member. So the new crates
    get exactly the same treatment as benilla-app with zero edits.
- **Do not touch `[profile.dev]`, `[profile.dev.package."*"]`, `[patch.crates-io]` or the `bevy`
  entry:** `crates/benilla-app/tests/launcher_shape.rs` asserts the documented launcher in
  `examples/extended_launcher.rs` repeats them.
- bevy as the workspace declares it: `default-features = false`, features `default_app`,
  `default_platform`, `2d_bevy_render`, `3d_bevy_render`, `ui_api`, `ui_bevy_render`, `scene`,
  `picking`. benilla-app, benilla-world and benilla-assets all take it as `bevy.workspace = true`
  with no extra features, and survivors uses only `bevy::{prelude, ui, window, render, ecs, asset,
  input, image, mesh, math, shader, ui_render, core_pipeline, camera, platform}`, all present in
  that set (benilla-app compiles survivors against exactly it). So survivors crates use
  **`bevy.workspace = true`** and nothing else. Never `bevy = "0.18"` or `features = ["ui"]`
  (brings `bevy_audio` back, see the comment in the root file).

Optional per-package profile block (not required, **unmeasured** trade-off). Incremental codegen
of a survivors crate at opt-level 3 is the main cost of an edit; dropping the game-logic crates to
opt-level 1 shortens it, but the file header notes that Bevy's generic hot paths monomorphize into
our crates and opt-level 0 cost Stormwind 15 fps, so check the frame time with an auto run before
keeping it:

```toml
# [profile.play.package.survivors-app]   # repeat per survivors-* crate; cargo has no glob for names
# opt-level = 1
```

## 2. benilla-app features and what survivors uses

`crates/benilla-app/Cargo.toml` features:

| feature | forwards |
|---|---|
| `default` | `["dev"]` |
| `dev` | `benilla-assets/dev`, `benilla-formats/dev`, `benilla-world/dev`, `dep:bevy_egui` |
| `tracy` | `bevy/trace_tracy` |
| `trace_chrome` | `bevy/trace_chrome` |
| `webgl2` | `bevy/webgl2`, `benilla-world/webgl2` |
| `webgpu` | `bevy/webgpu`, `benilla-world/webgpu` |

Sub-crate features: `benilla-world`: `default=["dev"]`, `dev = [benilla-formats/dev, benilla-assets/dev,
bevy/debug]`, `webgl2`, `webgpu`. `benilla-assets`: `default=["dev"]`, `dev=[benilla-formats/dev]`.
`benilla-formats`: `default=["dev"]`, `dev=[]`. `benilla-ui`, `benilla-protocol`: no features.

Every `use` root inside `crates/benilla-app/src/survivors/` (counts from grep):

| path root | uses | declare in survivors crate as (same spec as benilla-app) |
|---|---|---|
| `bevy::` | 67 `use`, plus inline paths | `bevy.workspace = true` |
| `benilla_world::` | 23 + inline (`schedule::WorldStage`, `world_point::WorldPoint`, `view::WorldCamera`, `rig_anim`, `model_fade`, `collision`, `world_map`, `dev_state::DebugState`) | `benilla-world = { path = "../benilla-world", default-features = false }` |
| `benilla_assets::` | `WorldAssets`, `MapCatalogRes`, `LockRecover` (7 files) | `benilla-assets = { path = "../benilla-assets", default-features = false }` |
| `benilla_formats::` | `minimap.rs` (`tile_to_world`, `world_to_tile`, `MinimapTranslate`, `load_minimap_translate`) | `benilla-formats = { path = "../benilla-formats", default-features = false }` |
| `benilla_ui::` | `hud.rs` (`script::nameplate`) | `benilla-ui = { path = "../benilla-ui" }` |
| `benilla_protocol::` | `units.rs` (`messages::{ObjectFields, ObjectType}`, `EntityKind`) | `benilla-protocol = { path = "../benilla-protocol" }` |
| `avian3d::` | `hero.rs`, `nav.rs` (`Collider`, `SpatialQuery`, `SpatialQueryFilter`, `move_and_slide`) | `avian3d = "0.6"` (benilla-app and benilla-world both: default features) |
| `serde::` | derives in `difficulty`, `evolutions`, `profile`, `items` | `serde = { version = "1", features = ["derive"] }` |
| `toml::` | `difficulty`, `evolutions`, `profile`, `items` (save files and tests) | `toml = "0.8"` |
| `web_time::` | `mod.rs:434` (`SystemTime`, `UNIX_EPOCH`) | `web-time = "1"` |
| `std` | 12 | - |

Not used by survivors, so not needed: `rand` (survivors has its own `Rng`), `anyhow`, `wgpu`,
`regex`, `bytemuck`, `image`, `libc`, `crossbeam-channel`, `async-channel`, `winit`, `kira`,
`wasm-bindgen`, `js-sys`, `web-sys`, `cpal`, `bevy_egui`. The render derives (`AsBindGroup`,
`ShaderType`, `ExtractComponent` in `fill.rs`, `grade.rs`, `hud.rs`) resolve through the `bevy`
dependency, so `bevy` must be a direct dependency of every crate that holds such a derive
(`survivors-ui` in the split). `encase` and `wgpu` are not named.

Everything else survivors touches is `crate::...` of benilla-app (`glue`, `local_state`, `ui_action`,
`entities`, `webenv`, `loading_screen`, `video`, `vplates`, `net`, `sound`, `creature_anim`,
`char_select`, `ui_pass`, `player`, `minimap`, `run_mode`, `cvars`, `combat_text`, `area`) and
needs the facade; that is the other agents' part, not wiring. None of these modules is
`#[cfg(feature = "dev")]` in `lib.rs` (the gated ones are `asset_churn`, `capture`, `debug_panel`,
`hover_log`, `preflight`, `probe_shield`), so a player build (no `dev`) of the facade works.

**Default features of the new crates.** Give `survivors-app`, `survivors-core` and every leaf
`default = ["dev"]`, like benilla-app. Reason: `cargo check -p survivors-<x> --profile play` and
`cargo test -p survivors-app --lib` (the plan's verification commands) otherwise build benilla-app
with a different feature set (no `dev`) than the launcher does, which compiles the 407k-line
crate a second time. With `default = ["dev"]` the standalone `-p` build and the launcher build
share one `benilla-app` artifact. Entry crates and sibling survivors crates always take their
survivors dependencies with `default-features = false` and forward `dev` explicitly.

## 3. `cfg(...)` inside survivors

- `#[cfg(test)]`: 17 files (`chatlog`, `difficulty`, `abilities`, `pickups`, `evolutions`,
  `profile`, `quests`, `chests`, `finale`, `talents`, `hero`, `items`, `focus`, `trainer`,
  `worldbuffs`, `nav`, `frames`). All test modules use only
  `use super::*;` plus bevy paths; one reaches a sibling with `super::super::Owned`
  (`evolutions.rs:400`), which must become `survivors_core::Owned` (or `crate::Owned`) after the
  split. No test module names `crate::` engine items, so no `#[cfg(test)]` facade is needed.
  No dev-dependencies are required (`toml`/`serde` are normal dependencies).
- `target_arch = "wasm32"`: `options.rs` only (lines 31 `cfg!`, 56, 59, 71, 116, 128, 349, 713; the
  `cfg_attr(..., allow(dead_code))` ones at 116 and 128). Needs no feature, travels with the file.
- `feature = "..."`: **none**. `target_os`, `unix`, `windows`, `debug_assertions`: **none**.
- Survivors reads no `env!` / `option_env!` / `include_str!` / `include_bytes!` /
  `include_dir!`. It reads runtime env through `crate::webenv::var(...)` (works on wasm).
- Embedded shaders: `grade.rs`, `fill.rs`, `hud.rs` name `embedded://benilla_app/shaders/...`
  and the WGSL files and `embedded_asset!` calls live in `crates/benilla-app/src/shaders.rs` (and
  `src/shaders/survivors_{grade,fill}.wgsl`). They keep working unchanged after the move because
  the files stay registered in benilla-app (the path is the registering crate's name). Leave them
  there for the first move (no upstream file touched). Later option: move the two survivors
  `.wgsl` files into the shader-owning crate and register them with
  `embedded_asset!(app, "shaders/survivors_grade.wgsl")` from that crate's `src/lib.rs` root (the
  macro bakes the call-site directory in, so it must be called from the crate root), then the path
  becomes `embedded://survivors_<crate>/shaders/...` and a shader edit no longer rebuilds
  benilla-app. That removes two lines from the upstream `shaders.rs` and its read-dir test still
  passes (it enumerates what is left).

So no `cfg(feature)` has to be created on the new crates for survivors source. The `dev`,
`webgl2` and `webgpu` features exist only to forward to benilla-app.

## 4. Entry crates

`crates/warcraft-survivors/Cargo.toml` now:

```toml
[features]
default = ["dev"]
dev = ["benilla-app/dev", "benilla-formats/dev"]
[dependencies]
benilla-app = { path = "../benilla-app", default-features = false }
benilla-formats = { path = "../benilla-formats", default-features = false }
clap.workspace = true
[build-dependencies]
benilla-buildstamp = { path = "../benilla-buildstamp" }
```

`main.rs` calls `benilla_app::run_survivors(BuildId { ... })` and `benilla_formats::set_project_folder`.
No `tracy` / `trace_chrome` features here (only the `benilla` launcher forwards them); add
`tracy = ["benilla-app/tracy"]` only if wanted.

`crates/wenilla/Cargo.toml` now: `webgl2 = ["benilla-app/webgl2"]`, `webgpu = ["benilla-app/webgpu"]`,
no `default`, no `dev` feature; `benilla-app = { path, default-features = false }`;
`[lib] crate-type = ["cdylib"]`; `wasm-bindgen = "=0.2.122"` pinned; `console_error_panic_hook`;
`benilla-buildstamp` as build-dependency. Its `lib.rs` calls `benilla_app::run_survivors(BuildId {..})`
exactly like the native shim, so both entry points change the same way (add the plugin argument).

Required new lines are in section 9. Note `wenilla` has no `dev` feature, so
`survivors-app = { ..., default-features = false }` is what keeps `dev` out of the page.

## 5. `build.rs`

- `benilla-app` has **no** `build.rs` (deliberately: a commit must not rebuild it).
- `warcraft-survivors/build.rs`, `wenilla/build.rs`, `benilla/build.rs` each call
  `benilla_buildstamp::emit()`, which sets six `cargo::rustc-env` vars (`BENILLA_GIT_SHA`,
  `_SHORT`, `_DATE`, `_DESCRIBE`, `BENILLA_PROFILE`, `BENILLA_PROJECT_DIR`) for **that package**
  only. They are read with `env!` in the launchers' `main.rs` / `lib.rs` and handed to benilla-app
  as data (`BuildId`, `project_dir`).
- Survivors code reads none of them (`grep env!` over `survivors/` is empty). **No `build.rs`
  is needed for any new crate**, and none must be added: a build script in `survivors-app` that
  reran on git changes would rebuild it on every commit.
- `BENILLA_PROJECT_DIR` is non-empty only when the launcher's own feature is literally named
  `dev` (`CARGO_FEATURE_DEV`). Keep `warcraft-survivors`' feature named `dev`.
- Invariant to respect in the new crates: no `env!("CARGO_MANIFEST_DIR")` in non-test code. The test
  `run_mode::tests::the_dev_plane_has_exactly_one_door` in benilla-app exists because that literal
  would bake the build machine's path into a player binary. Survivors has none today.

## 6. Incremental speed facts

- `[profile.play]`: inherits release (`opt-level = 3`, `debug = false`, no strip, `panic =
  unwind`), `incremental = true`, `lto = "off"`, `codegen-units` unset (cargo uses 256 when
  incremental). `debug` is off, so there is no dSYM / debuginfo to link or strip.
- `.cargo/config.toml` is only the two aliases (`play`, `survivors`). There are **no** `rustflags`,
  `[target.*]` entries, `linker`, or `-fuse-ld`. No `~/.cargo/config.toml`. No `sccache`, `lld`,
  `mold` or `zld` on `PATH` in this shell. `rust-toolchain.toml` pins stable 1.98.1 with clippy and
  rustfmt, so nightly-only flags (`-Zshare-generics`, `-Zthreads`) are out.
- Dependencies of the workspace (bevy, avian, kira, ...) are not incremental and not rebuilt on an
  edit. Workspace members are incremental.
- After the split, an edit in `survivors-app` (single crate stage) rebuilds, in order:
  1. `survivors-app` (incremental: parse, typeck and macro expansion of the whole crate again,
     optimized codegen only for the CGUs the edit touched). `benilla-app` is **not** rebuilt
     because it does not depend on `survivors-app`.
  2. `warcraft-survivors` (a one-file bin: recompiled because its dependency changed, tiny).
  3. The link of `target/play/warcraft-survivors` (currently 235 MB; it pulls the 175 MB
     `libbenilla_app` rlib plus bevy).
  An edit in a feature crate after the second stage: that leaf, then `survivors-app` (the
  aggregator re-typechecks against the new leaf `rmeta` and re-links its plugin list, small), then
  `warcraft-survivors`, then the link. An edit in `survivors-core`: core, every leaf, the
  aggregator, the launcher, the link.
- A commit changes `BENILLA_GIT_*`: that re-runs only the launcher build scripts
  (`warcraft-survivors`, `wenilla`), as today. Not the new crates.
- Things to watch that keep the rebuild larger than the edit:
  - **Generics instantiated in the downstream crate.** Bevy's system plumbing (`add_systems(f)`,
    `Query`, `Commands`) is monomorphized in the crate that names the system function. Under
    `opt-level 3` sharing of generics across crates is off (`share-generics` defaults on only for
    opt-level 0/1/s/z), so each survivors crate re-instantiates what it uses. This is where the
    time of `survivors-app` itself goes; smaller leaves shrink it.
  - **`run_survivors` must not be generic over the plugin if it can avoid it.** `launch` is
    deliberately not generic "so a launcher compiles none of it". Take the plugin as
    `Box<dyn FnOnce(&mut App)>` / `Box<dyn Plugin>` or `impl Plugin` only in a thin wrapper that
    boxes it, so no engine code is monomorphized in the launcher. (Also: `run_with` exists but marks
    the build `extended` in `BuildId` and runs `extend` after the client is built, while survivors
    must add its plugin at `lib.rs:443`; a separate entry is right.)
- Link: ld64/ld-prime on arm64 macOS is already the fast linker; `lld` is not an improvement there
  and is not installed. Options, **all unmeasured, try one at a time and compare with
  `CC=gcc cargo build --profile play -p warcraft-survivors --timings`**:
  1. `cargo::rustc-link-arg-bins=-Wl,-no_deduplicate` from `warcraft-survivors/build.rs`, on macOS
     only (`CARGO_CFG_TARGET_OS == "macos"`). Skips code/data dedup, which Apple documents as a link
     speed-up for iteration builds. A build-script link arg changes only the bin's link, so none of
     the 48 GB `target/` is invalidated (a `[target.*] rustflags` entry would rebuild every crate
     once). Risk: ld-prime may warn or ignore the flag; the binary gets slightly larger.
  2. `bevy/dynamic_linking` behind an opt-in feature (`fast-link`) on `warcraft-survivors`. Links
     bevy as a dylib. Poor fit here: the bulk of the link is `benilla-app`, which stays static,
     changing bevy's features rebuilds all bevy crates once, and the dylib must be found at run
     time. Not recommended.
  3. Do nothing: with no LTO and no debuginfo the link is a few seconds (**unmeasured**) next to
     the compile of survivors-app. Measure with `--timings` before spending effort here.

## 7. CI and scripts that must change

Workflows:

| file:line | content | action |
|---|---|---|
| `.github/workflows/ci.yml:52` | `cargo test -p benilla-formats -p benilla-app --no-default-features --lib` (job `player-tests` step) | add `-p survivors-app` (and `-p survivors-core` plus leaves that have tests, once they exist; a `-p survivors-app` alone does not run the tests of its dependencies). Today the 17 survivors test modules run here as part of `benilla-app`; without this change they silently stop running in the player build. |
| `scripts/gates.sh:150` | same command in the `player-tests` gate (comment block at 138-149 says "`benilla-app` without `dev`") | same edit; mention survivors in the comment. |
| `.github/workflows/ci.yml:46,49` | `cargo clippy --workspace --all-targets`, `cargo test --workspace` | nothing: new crates are members, covered. |
| `.github/workflows/check.yml` `wasm` job | `cargo check --target wasm32-unknown-unknown -p wenilla --no-default-features --features webgpu` | nothing: `wenilla` depends on `survivors-app`, so the wasm check covers every survivors crate. |
| `.github/workflows/check.yml` | no survivors test job | optional new step in `client-carries`: `cargo test -p survivors-app --lib` (the job already installs the native Bevy libs). |
| `.github/workflows/bump-realm-pin.yml` | - | nothing (it is a pin bot; the prod image builds from `main`, not from this fork's `features` branch). |

Scripts and Makefile: `Makefile:29` (`cargo run --release -p warcraft-survivors`),
`scripts/web-build.sh` (`-p wenilla`), `.cargo/config.toml` aliases: no change. `scripts/genmap.sh`
(lines 33-44) greps `crates/benilla-app/src/lib.rs` for `add_plugins(...)` to render the plugin
list in `docs/MAP.md`; the `add_plugins(survivors::SurvivorsPlugin)` line goes away, so the
generated list changes by that one entry (harmless). `scripts/doc-links.py` runs
`cargo doc --workspace`, so it covers the new crates; its doc links of the form
``[`crate::sound::SoundConfig`]`` in survivors files become unresolved `crate::` paths whose leaf
still exists in the workspace, which the script treats as benign. `scripts/pass-span-lint.py`
scans `crates/**/*.rs`: covers new crates.

Cargo.lock: commit the lock change in the same commit as the new manifests (CI uses the lock; no new
external packages appear).

## 8. Docs naming `crates/benilla-app/src/survivors`

Tracked files:

- `CLAUDE.md:29` - "`survivors/mod.rs` (`pub(super) fn plugin(app: &mut App)`)" -> the new
  aggregator file (`crates/survivors-app/src/lib.rs`), and after the second stage the per-crate
  `lib.rs` files.
- `CLAUDE.md:34` - agents check with `cargo check -p benilla-app --profile play` -> `CC=gcc cargo
  check -p survivors-app --profile play` (or `-p survivors-<leaf>`); also add `CC=gcc`.
- `CLAUDE.md:44` - `crates/benilla-app/src/survivors/auto.rs` -> `crates/survivors-app/src/auto.rs`.
- `README.md:155` - "The survivors mode is `crates/benilla-app/src/survivors/`" -> `crates/survivors-*`.
- `docs/superpowers/plans/2026-10-31-survivors-crate-split.md:7, 42, 43, 63, 64` - the plan itself
  (describes the old location on purpose; keep as history or update Task 1/2 paths when executing).
- `AGENTS.md`: no mention of the survivors path (upstream's file, report only: nothing to do).
- `CONTROLS.md`, `docs/MAP.md`, `docs/METHOD.md`, `docs/CONTRIBUTING.md`, `docs/UPSTREAM.md`,
  `docs/PERFORMANCE.md`: no mention.

Untracked files (not in git, update by hand):

- `TODO.md:13, 21, 28, 33, 55, 60, 70, 75, 88, 100, 112, 117, 123` - paths `crates/benilla-app/src/survivors/{hero,data,auto,enemies,cards}.rs`
  and short forms `survivors/{hud,menu,abilities}.rs`. Short forms stay roughly right (file names
  survive the move), the long ones need the new crate directory.
- `RESEARCH.md:12` - "meist in `survivors/`" describes FlorianFortier's fork, not this tree: leave.

Source comments that name the old location: `crates/benilla-app/src/lib.rs:109-110, 229` (doc
link ``[`survivors`]``, removed with the module); `survivors/mod.rs:2, 5, 46` (`crate::run_survivors`,
`crate::run_mode::CaptureMode`) become `benilla_app::...` paths.

Auto-memory (outside the repo): `crate-split-plan.md` and `worktree-workflow.md` mention the layout.

## 9. Exact file contents to create

### `crates/survivors-app/Cargo.toml` (stage 1: the whole mode in one crate)

```toml
[package]
name = "survivors-app"
version.workspace = true
edition.workspace = true
license.workspace = true
description = "Warcraft Survivors: the top-down survival mode as a plugin on top of benilla-app"

[lints]
workspace = true

[lib]
# No doc examples here; a doctest binary would link the whole engine for nothing.
doctest = false

[features]
# Same seam as benilla-app: `dev` on by default so `cargo check -p survivors-app` shares the
# `benilla-app` artifact with the launcher; the entry crates take this crate without defaults and
# forward `dev`.
default = ["dev"]
dev = [
    "benilla-app/dev",
    "benilla-assets/dev",
    "benilla-formats/dev",
    "benilla-world/dev",
]
# Browser render backend, forwarded from `wenilla`.
webgl2 = ["benilla-app/webgl2"]
webgpu = ["benilla-app/webgpu"]

[dependencies]
# `default-features = false` on the benilla crates: a path dependency resolves its own defaults,
# and `dev` must arrive only through the feature above.
benilla-app = { path = "../benilla-app", default-features = false }
benilla-assets = { path = "../benilla-assets", default-features = false }
benilla-formats = { path = "../benilla-formats", default-features = false }
benilla-protocol = { path = "../benilla-protocol" }
benilla-ui = { path = "../benilla-ui" }
benilla-world = { path = "../benilla-world", default-features = false }
# bevy's feature set is the workspace's; never name features here (the audio stack would return).
bevy.workspace = true
# Collision for the hero and the nav grid; the version and features benilla-app and benilla-world take.
avian3d = "0.6"
# The profile, items, difficulty and spellbook saves.
serde = { version = "1", features = ["derive"] }
toml = "0.8"
# The run seed's wall-clock read; std's `SystemTime` panics in a browser.
web-time = "1"
```

### `crates/survivors-core/Cargo.toml` (stage 2: shared types)

Dependencies are what core's modules name today (`mod.rs` types, `data`, `mods`, `difficulty`,
`units`, `profile`): `units` needs `benilla_protocol` and `crate::net` (so benilla-app),
`profile`/`difficulty` need serde, toml, `benilla_assets::WorldAssets`, `local_state` and `webenv`
(benilla-app). `benilla-ui`, `avian3d` and `benilla-formats` are not core's: leave them out and add
only if a module moved into core names them.

```toml
[package]
name = "survivors-core"
version.workspace = true
edition.workspace = true
license.workspace = true
description = "Warcraft Survivors: the types, data tables and saves every survivors crate shares"

[lints]
workspace = true

[lib]
doctest = false

[features]
default = ["dev"]
dev = ["benilla-app/dev", "benilla-assets/dev", "benilla-world/dev"]
webgl2 = ["benilla-app/webgl2"]
webgpu = ["benilla-app/webgpu"]

[dependencies]
benilla-app = { path = "../benilla-app", default-features = false }
benilla-assets = { path = "../benilla-assets", default-features = false }
benilla-protocol = { path = "../benilla-protocol" }
benilla-world = { path = "../benilla-world", default-features = false }
bevy.workspace = true
serde = { version = "1", features = ["derive"] }
toml = "0.8"
web-time = "1"
```

### Leaf crate template (`crates/survivors-<leaf>/Cargo.toml`)

Rule: a leaf depends on `survivors-core` and the engine, **never on another leaf**. Delete the
lines the leaf does not name (an unused dependency still compiles, but a needless edge is how a
rebuild spreads). Check with `cargo tree -p survivors-<leaf> -e normal | grep survivors-`: only
`survivors-core` may appear.

```toml
[package]
name = "survivors-LEAF"
version.workspace = true
edition.workspace = true
license.workspace = true
description = "Warcraft Survivors: LEAF DESCRIPTION"

[lints]
workspace = true

[lib]
doctest = false

[features]
default = ["dev"]
# survivors-core forwards the benilla `dev` features; a leaf that names a benilla crate directly
# lists that crate's `dev` here too.
dev = ["survivors-core/dev"]

[dependencies]
survivors-core = { path = "../survivors-core", default-features = false }
benilla-app = { path = "../benilla-app", default-features = false }
bevy.workspace = true
# Only what the leaf names (see the table in section 2):
# benilla-assets = { path = "../benilla-assets", default-features = false }
# benilla-formats = { path = "../benilla-formats", default-features = false }
# benilla-protocol = { path = "../benilla-protocol" }
# benilla-ui = { path = "../benilla-ui" }
# benilla-world = { path = "../benilla-world", default-features = false }
# avian3d = "0.6"                                  # survivors-world only (hero, nav, barrier)
# serde = { version = "1", features = ["derive"] } # survivors-loot, survivors-meta (saves)
# toml = "0.8"                                     # same
```

A leaf that has `AsBindGroup` / `ShaderType` / `ExtractComponent` derives (`fill`, `grade`, `hud`
in `survivors-ui`) needs `bevy.workspace = true` as a direct dependency; every leaf has it.

### `survivors-app` after stage 2 (replace the `[dependencies]` additions, keep the rest)

```toml
[features]
default = ["dev"]
dev = [
    "survivors-core/dev",
    "survivors-world/dev",
    "survivors-combat/dev",
    "survivors-loot/dev",
    "survivors-bosses/dev",
    "survivors-ui/dev",
    "survivors-meta/dev",
    "benilla-app/dev",
]
webgl2 = ["benilla-app/webgl2"]
webgpu = ["benilla-app/webgpu"]

[dependencies]
survivors-core = { path = "../survivors-core", default-features = false }
survivors-world = { path = "../survivors-world", default-features = false }
survivors-combat = { path = "../survivors-combat", default-features = false }
survivors-loot = { path = "../survivors-loot", default-features = false }
survivors-bosses = { path = "../survivors-bosses", default-features = false }
survivors-ui = { path = "../survivors-ui", default-features = false }
survivors-meta = { path = "../survivors-meta", default-features = false }
benilla-app = { path = "../benilla-app", default-features = false }
bevy.workspace = true
```

The aggregator keeps `auto.rs`; list the benilla crates there that `auto.rs` and the
`SurvivorsPlugin` body name (check with the grep in section 2 restricted to those two files).

### Root `Cargo.toml`

No change (members glob, no per-package profile entries). Optional only, the commented block in
section 1.

### `crates/warcraft-survivors/Cargo.toml`

```diff
 [features]
 default = ["dev"]
-dev = ["benilla-app/dev", "benilla-formats/dev"]
+dev = ["benilla-app/dev", "benilla-formats/dev", "survivors-app/dev"]

 [dependencies]
 benilla-app = { path = "../benilla-app", default-features = false }
 benilla-formats = { path = "../benilla-formats", default-features = false }
+# `default-features = false`, or `--no-default-features` here still builds the mode with `dev`.
+survivors-app = { path = "../survivors-app", default-features = false }
 clap.workspace = true
```

### `crates/wenilla/Cargo.toml`

```diff
 [features]
-webgl2 = ["benilla-app/webgl2"]
-webgpu = ["benilla-app/webgpu"]
+webgl2 = ["benilla-app/webgl2", "survivors-app/webgl2"]
+webgpu = ["benilla-app/webgpu", "survivors-app/webgpu"]

 [dependencies]
 benilla-app = { path = "../benilla-app", default-features = false }
+# Same load-bearing `default-features = false`: survivors-app's default is `dev`, and this page
+# must not get benilla-app's instruments through it.
+survivors-app = { path = "../survivors-app", default-features = false }
```

Verify the web build keeps `dev` out: `cargo tree --target wasm32-unknown-unknown -p wenilla
--no-default-features --features webgpu -e features -i benilla-app` must not list `dev`.

### Edits in the existing manifests/scripts outside the crates above

```diff
# .github/workflows/ci.yml:52 and scripts/gates.sh:150
-cargo test -p benilla-formats -p benilla-app --no-default-features --lib
+cargo test -p benilla-formats -p benilla-app -p survivors-app --no-default-features --lib
```

(Stage 2: add `-p survivors-core` and each leaf that has `#[cfg(test)]` code.)

### Commands to verify after the move (always `CC=gcc`)

```bash
CC=gcc cargo check -p survivors-app --profile play
CC=gcc cargo build --profile play -p warcraft-survivors
CC=gcc cargo test -p survivors-app --lib
CC=gcc cargo test -p benilla-formats -p benilla-app -p survivors-app --no-default-features --lib
cargo check --target wasm32-unknown-unknown -p wenilla --no-default-features --features webgpu
CC=gcc cargo tree -p survivors-app -e features -i benilla-app   # dev present via warcraft-survivors only
```
