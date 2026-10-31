# Survivors crate split Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** An edit to one survivors feature (for example the world boss) rebuilds only a small crate and the link, not the 407,000-line `benilla-app`.

**Architecture:** First the mode leaves `benilla-app` whole, into `crates/survivors-app` (the big win: the engine no longer rebuilds). Then, inside `survivors-app`, four small prep changes cut the leaf-to-leaf edges, the shared items gather in a `core/` sub-tree, `core/` becomes the crate `survivors-core`, and the feature crates come out one by one. Every step builds, passes the tests and is committed before the next.

**Tech Stack:** Rust 2021 workspace, Bevy 0.18.1, cargo profile `play` (release + incremental), wasm32 via `wenilla`.

**Spec:** Michael, 2026-10-31: "Wenn ich an WorldBoss arbeite, dass das einfach funktioniert, weil das nur dort geändert wird." Then: "Zieh Code raus in ein Modul, mach alles sauber und bau. Dann mach mit dem nächsten Modul weiter."

**Analysis (read before each task, it has the line numbers):**
- `docs/superpowers/plans/crate-split/engine-surface.md`: the 75 engine items survivors names, the visibility edits (29 upstream files, 50 fields, 10 methods, 5 `mod` lines), the facade draft.
- `docs/superpowers/plans/crate-split/module-graph.md`: the module graph (one big cycle), the items that go to core, the crate table, the extraction order, where each test module goes.
- `docs/superpowers/plans/crate-split/build-wiring.md`: the Cargo.toml contents, the feature chain, CI and doc paths.

## Global Constraints

- Every cargo command runs with `CC=gcc`. No cargo while Michael plays (check `/bin/ps -axo pid=,command= | grep warcraft-survivors` first).
- One step = move, clean up, build, test, commit. Never two crates in one step.
- Gate per step: `CC=gcc cargo build --profile play -p warcraft-survivors` with no warnings from our crates, then the tests of the touched crates (`CC=gcc cargo test --profile play -p <crate> --lib`, total stays 82), then commit. After Task 2, Task 4 and the last leaf also: `cargo check --target wasm32-unknown-unknown -p wenilla --no-default-features --features webgpu` and an auto run (`WOW_SURVIVORS_AUTO=mage:all WOW_SURVIVORS_MAP=elwynn WOW_SURVIVORS_SHOTS=/tmp/split-<step>`, 60 s, no panic, screenshots look like before).
- Moves use `git mv`, so history follows the files. Code is moved, not rewritten: only paths and visibility change, plus the four prep changes in Task 3.
- Upstream files (everything in `benilla-app` outside `survivors/`) change only in visibility, plus `lib.rs` (entry point) and the new fork file `survivors_host.rs`.
- Save files keep their names and paths (`survivors-profile.toml`, `survivors-items.toml`, `survivors-spellbook.toml` under `benilla-config/`).
- The shaders (`embedded://benilla_app/shaders/survivors_*.wgsl`) stay in `benilla-app` for now.
- No em dashes. Commit only named paths, never `git add -A`.

## Review Focus

- The browser build: `wenilla` must get `survivors-app` with `default-features = false` and forward `webgpu`/`webgl2`, or the dev tools land in the web build.
- Saves: a profile written before the split loads after it (paths unchanged, serde field names unchanged by `pub` edits).
- System order: the cross-crate `.after/.before` become `SurvivorsSet` entries; a missing `in_set` compiles fine but reorders silently. The auto run's screenshots (plates following units, frames, menu) catch it.
- The `minimap` name clash: survivors' `minimap.rs:404` names the engine's `crate::minimap::blips`; it must become the facade path, not the survivors module.
- Tests move with their code; the count stays 82 (`module-graph.md`, "Tests").

---

### Task 1: Baseline

- [ ] **Step 1:** Append a comment line to `crates/benilla-app/src/survivors/finale.rs`, `time CC=gcc cargo build --profile play -p warcraft-survivors`, revert the line, build again to restore. Record the wall time at the top of `docs/superpowers/plans/crate-split/timings.md`.

### Task 2: `survivors-app`, the whole mode out of benilla-app

**Files:** create `crates/benilla-app/src/survivors_host.rs`, `crates/survivors-app/Cargo.toml`; `git mv crates/benilla-app/src/survivors crates/survivors-app/src` with `mod.rs` renamed to `lib.rs`; modify `crates/benilla-app/src/lib.rs`, the 29 upstream files from `engine-surface.md`, `crates/warcraft-survivors/{Cargo.toml,src/main.rs}`, `crates/wenilla/{Cargo.toml,src/lib.rs}`, `.github/workflows/ci.yml:52`, `scripts/gates.sh:150`.

**Interfaces:**
- `benilla_app::run_survivors(build: BuildId, survivors: fn(&mut App)) -> AppExit`: not generic, so the launcher compiles no engine code. `launch` gets a third parameter `survivors: Option<Extension<'_>>`; its `let survivors = survivors::active()` becomes `survivors.is_some()` (rename the parameter to avoid the clash), and the plugin is added where `lib.rs:444` adds `SurvivorsPlugin` today. `ACTIVE`, `active()` and the `WOW_SURVIVORS` env fallback go away (nothing else uses them, `wenilla` calls `run_survivors` directly).
- `benilla_app::survivors_host::<engine module>::<item>`: the facade mirrors the engine module paths, so the rewrite is a prefix swap `crate::glue::` to `benilla_app::survivors_host::glue::`.
- `survivors_app::plugin(app: &mut App)`: the crate's entry, wrapping today's `SurvivorsPlugin`.

- [ ] **Step 1:** Visibility edits per `engine-surface.md` (items, fields, methods, the 5 `mod` lines, `BackdropEdges`, `SetOutcome`), then `survivors_host.rs` from its "Facade draft", `pub mod survivors_host;` in `lib.rs`.
- [ ] **Step 2:** Entry point in `lib.rs` as above; remove `mod survivors;`.
- [ ] **Step 3:** `git mv`, `Cargo.toml` from `build-wiring.md` section 9 (stage 1), then rewrite paths: `crate::survivors::` to `crate::`, `crate::<engine>::` to `benilla_app::survivors_host::<engine>::` (watch `minimap`), `pub(crate)` stays `pub(crate)` inside the new crate.
- [ ] **Step 4:** Entry crates: `survivors-app = { path = "../survivors-app", default-features = false }` with `dev` and `webgpu`/`webgl2` forwarding, calls become `run_survivors(build, survivors_app::plugin)`. CI and `gates.sh` gain `-p survivors-app`.
- [ ] **Step 5:** Gate (build, `cargo test -p survivors-app --lib` = 82, wasm check, auto run). Record the time of the Task 1 edit, now in `crates/survivors-app/src/finale.rs`, in `timings.md`.
- [ ] **Step 6:** Commit `Survivors: move the mode out of benilla-app into survivors-app`. Update the paths in `CLAUDE.md`, `README.md:155` and `TODO.md` in the same commit.

### Task 3: Prep inside survivors-app (four commits)

Each step: change, gate, commit. Details and line numbers in `module-graph.md`, "Registration and ordering" and "Items to move into core".

- [ ] **Step 1: `SurvivorsSet`** in `lib.rs`: `#[derive(SystemSet, Debug, Clone, PartialEq, Eq, Hash)] pub enum SurvivorsSet { Keys, RefreshResults, PlaceCamera, MoveEnemies }`. `lib.rs` puts `hud::keys`, `hud::refresh_results`, `hero::place_camera`, `enemies::move_enemies` in them; the six sites (`difficulty.rs:122-129`, `options.rs:68`, `talents.rs:954`, `frames.rs:53-54`, `plates.rs:36-37`, `nav.rs:719`) order against the set. Commit `Survivors: order across modules by SurvivorsSet`.
- [ ] **Step 2: `Autopilot { dir: Vec3 }`** resource replaces `Auto.dir` for `drive_hero`. Commit.
- [ ] **Step 3: `BossPlans(Vec<Plan>)`** resource with `fn plan(&self, map: usize) -> &Plan`; `finale::plugin` inserts it, `spawn_enemies` takes `Res<BossPlans>`. Commit.
- [ ] **Step 4: gear mirror setter** `items::set_worn(lines)` replaces the direct writes to `WORN`. Commit.

### Task 4: `survivors-core`

- [ ] **Step 1:** Inside `survivors-app`, create `src/core/` and move into it, module by module with the file layout kept (`core::talents`, `core::trainer`, `core::units`, ...; core cannot be flat because `Fx`, `rank`, `spent`, `xp_to_next` clash): the whole files `data`, `mods`, `units`, `profile`, `difficulty`; and the items listed in `module-graph.md` "Items to move into core" (the `mod.rs` types, enemy and hero models, `Hit`, `ChatKind`/`ChatLine`, `NavGrid`/`SpawnProbe`, mobility data with `impl Run`, talent and trainer rules, `Focus`/`Target`, the hud UI kit, `MenuAction`/`MenuBuilt`, the `*Ui` open flags, `LEGENDARY`, evolution rules, `drop_boost`, `WORN`, `Finale`/`Reaper`/`Plan`/`BossPlans`, `SurvivorsSet`, `Autopilot`) plus a `core::plugin` with the shared `init_state`, resources and messages. Old paths keep working through `pub use` in the old modules only until Step 2. Gate, commit `Survivors: gather the shared types in core`.
- [ ] **Step 2:** `git mv src/core crates/survivors-core/src`, `Cargo.toml` from `build-wiring.md`, `pub(super)`/`pub(crate)` on moved items and fields to `pub`, `survivors-app` depends on `survivors-core`, paths `crate::core::` to `survivors_core::`. Gate (core tests 41, app tests 41), wasm check, auto run. Commit `Survivors: survivors-core crate`.

### Task 5: Leaf crates, one per step

Order (risk, not dependency; after core no leaf needs another): **bosses** (`finale`), **loot** (`items`, `chests`, `pickups`), **windows** (`charpanel`, chat window, `options`, `evolutions` window), **overlays** (`frames`, `plates`, focus window, `quests`, `grade`), **meta** (`menu`, trainer UI, talents UI, difficulty UI, `worldbuffs`), **hud** (`hud`, `cards`, `minimap`, `fill`), **combat** (`abilities`, `enemies` systems), **world** (`hero` systems, `nav` scan, `barrier`, `mobility::cast_mobility`).

Per leaf:
- [ ] `git mv` its files to `crates/survivors-<leaf>/src/`, Cargo.toml from the leaf template, `lib.rs` with `pub fn plugin(app: &mut App)`.
- [ ] Paths: `super::<core item>` to `survivors_core::...`; items the aggregator's two chains name (Run chain, UI chain, `reset_run`'s `PendingHits`/`Spawner`) become `pub`.
- [ ] Check `cargo tree -p survivors-<leaf> -e normal --prefix none | grep survivors-` lists only `survivors-core`.
- [ ] Gate, commit `Survivors: survivors-<leaf> crate`.

After the last leaf `survivors-app` holds only the aggregator plugin (the Run chain, the UI chain, `pause`, `unpause`, `reset_run`) and `auto`.

### Task 6: Measure and write it down

- [ ] Time a one-line edit in `survivors-bosses/src/finale.rs`, in `survivors-hud/src/hud.rs` and in `survivors-core/src/data.rs`; add them to `timings.md`.
- [ ] `CLAUDE.md` feature rounds: one agent per crate; shared types change only in `survivors-core`, by the orchestrator.
- [ ] README row without screenshot: "Faster rebuilds", with the before and after numbers.
- [ ] Commit `Docs: survivors crate layout and rebuild times`, push `features`.
