# Survivors crate split Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** An edit to one survivors feature (for example the world boss) rebuilds only a small crate and the link, not the 407,000-line `benilla-app`.

**Architecture:** The survivors mode (`crates/benilla-app/src/survivors/`, about 15,000 lines) leaves `benilla-app`. `benilla-app` keeps the engine and exposes what survivors needs through one fork-owned facade module plus `pub` on the items it re-exports. `run_survivors` takes the survivors plugin as an argument, so the launcher and the wasm entry inject it. First the mode moves whole into one crate (`survivors-app`, the big win), then it splits into a shared `survivors-core` and feature crates that depend only on core, so a leaf edit rebuilds one leaf crate, the aggregator and the link.

**Tech Stack:** Rust 2021 workspace, Bevy 0.18.1, cargo profile `play` (release + incremental), wasm32 via `wenilla`.

**Spec:** Michael, 2026-10-31: "Restrukturierung, dass Parts von dem Code in Module ausgelagert werden, dass wir nicht alles bauen müssen. Wenn ich an WorldBoss arbeite, dass das einfach funktioniert, weil das nur dort geändert wird." Measured today: a one-line survivors edit rebuilds `benilla-app` in about 13 s under `play`, a feature round (several files, `Run` in `mod.rs`) about 3 min.

## Global Constraints

- Starts only after the running 3-agent feature round is integrated and committed on `features`.
- Every cargo command runs with `CC=gcc` (Michael's terminal sets it; a mismatch rebuilds Bevy).
- No cargo while Michael plays; builds happen when he says so.
- Upstream files change as little as possible: visibility (`pub(crate)` to `pub`) on re-exported items, the `run_survivors` signature, and removing `mod survivors;`. Everything else goes into new, fork-owned files.
- The wasm build must keep compiling: `cargo check --target wasm32-unknown-unknown -p wenilla --no-default-features --features webgpu`.
- Save paths stay the same (`benilla-config/survivors-profile.toml`, `survivors-items.toml`, `survivors-spellbook.toml` via `local_state`), so no progress is lost.
- No em dashes in code, docs or commits.
- Work on `features` directly; no upstream PR.

## Review Focus

- Saves after the move: a profile written before the split loads unchanged after it (same path, same serde field names).
- `WOW_SURVIVORS=1` without the launcher (the env path `survivors::active()` reads via `webenv`) still boots survivors.
- The `dev` feature: `warcraft-survivors` default `dev` must still reach `benilla-app/dev` through the new crates (forward `dev` in every survivors crate that names it).
- Private-field access across crates: `Run.mobile` and other `pub(super)`/private fields used by sibling modules become `pub` in core; nothing outside survivors may start depending on them.
- Auto mode (`WOW_SURVIVORS_AUTO`, `WOW_SURVIVORS_SHOTS`) still drives a full run and writes screenshots.

---

### Task 1: Baseline and dependency map

**Files:**
- Create: `docs/superpowers/plans/2026-10-31-survivors-crate-split.baseline.md`

**Interfaces:**
- Produces: the module-to-crate table and the list of `crate::` items survivors uses, consumed by Tasks 2 and 3.

- [ ] **Step 1: Measure the baseline.** Append one comment line to `crates/benilla-app/src/survivors/finale.rs`, run `time CC=gcc cargo build --profile play -p warcraft-survivors`, revert the line, run it again. Record both wall times.
- [ ] **Step 2: Map the edges.** For each survivors module, list which sibling modules it names (`super::<m>`) and which `crate::<module>::<item>` paths it uses (`grep -rhoE "crate::[a-z_]+(::[A-Za-z_]+)*" crates/benilla-app/src/survivors`). Today: 21 engine modules, top users `glue` (22), `ui_action` (11), `entities` (10), `video` (8), `local_state` (8).
- [ ] **Step 3: Fix the crate table** (adjust only where Step 2 shows a leaf module is named by a sibling; then the named type moves to core):

| crate | modules |
|---|---|
| `survivors-core` | the types of `mod.rs` (`Phase`, `Run`, `Stats`, `Selection`, `Rng`, `Owned`, `RunEntity`), `data`, `mods`, `difficulty`, `units`, `profile` (data and persistence), the messages (`Loot`, `Hit`, `ChatLine`), `Enemy`/`GemKind`, `Hero`/`AimPoint`/`CamRig`/`arena`, `NavGrid`, `Focus` |
| `survivors-world` | hero movement and camera systems, `nav` systems, `barrier`, `mobility` |
| `survivors-combat` | `abilities`, `enemies` systems, `evolutions` |
| `survivors-loot` | `items`, `chests`, `pickups` |
| `survivors-bosses` | `finale` |
| `survivors-ui` | `hud`, `cards`, `charpanel`, `chatlog` window, `frames`, focus icon, `plates`, `minimap`, `grade`, `fill`, `options`, `quests` |
| `survivors-meta` | `menu`, `trainer`, `talents`, `worldbuffs` |
| `survivors-app` | `SurvivorsPlugin` (registers every crate's plugin), `auto` |

- [ ] **Step 4: Commit** the baseline file: `git add docs/superpowers/plans/2026-10-31-survivors-crate-split.baseline.md && git commit -m "Plan: survivors crate split baseline"`.

### Task 2: One crate out of benilla-app

**Files:**
- Create: `crates/benilla-app/src/survivors_host.rs` (fork-owned facade), `crates/survivors-app/Cargo.toml`, `crates/survivors-app/src/lib.rs`
- Move: `crates/benilla-app/src/survivors/*` to `crates/survivors-app/src/` (`git mv`, keeps history)
- Modify: `crates/benilla-app/src/lib.rs` (`mod survivors;` out, `pub mod survivors_host;` in, `run_survivors` signature), the engine modules from Task 1 Step 2 (visibility only), `crates/warcraft-survivors/{Cargo.toml,src/main.rs}`, `crates/wenilla/{Cargo.toml,src/lib.rs}`

**Interfaces:**
- Consumes: the item list from Task 1.
- Produces: `pub fn run_survivors(build: BuildId, survivors: impl bevy::app::Plugin) -> AppExit` in benilla-app; `pub fn survivors_active() -> bool` (the moved `ACTIVE` flag and env check stay in benilla-app, since `lib.rs` reads them before plugins exist); `benilla_app::survivors_host::*` re-exporting every engine item survivors uses; `survivors_app::SurvivorsPlugin`.

- [ ] **Step 1: Facade.** In `survivors_host.rs`, `pub use crate::<module>::<item>;` for each item from Task 1, grouped by engine module with a one-line comment each. Raise only those items (and their fields survivors touches) to `pub`; `#[doc(hidden)]` on the facade module.
- [ ] **Step 2: Inject the plugin.** `run_survivors` sets the active flag and adds the passed plugin where `lib.rs:444` added `survivors::SurvivorsPlugin`; every `survivors::active()` in `lib.rs` becomes `survivors_active()`.
- [ ] **Step 3: Move the mode.** `git mv` the directory, `mod.rs` becomes `lib.rs`, `crate::X` becomes `benilla_app::survivors_host::X`, `crate::survivors::` becomes `crate::`. `Cargo.toml`: depends on `benilla-app` (`default-features = false`), `benilla-world`, `benilla-assets`, `benilla-ui`, `benilla-formats`, `avian3d`, `bevy`, `serde`, `toml` as survivors uses them; `[features] dev = ["benilla-app/dev"]`, `webgl2 = ["benilla-app/webgl2"]`.
- [ ] **Step 4: Entry points.** `warcraft-survivors` and `wenilla` depend on `survivors-app` (forwarding `dev` and the render features) and call `benilla_app::run_survivors(build, survivors_app::SurvivorsPlugin)`.
- [ ] **Step 5: Verify.** `CC=gcc cargo build --profile play -p warcraft-survivors`: builds. `CC=gcc cargo test -p survivors-app --lib`: all survivors tests pass (34 or more). The wasm check from Global Constraints: passes. An auto run (`WOW_SURVIVORS_AUTO=mage WOW_SURVIVORS_MAP=elwynn WOW_SURVIVORS_SHOTS=/tmp/split-shots`, 60 s): no panic, screenshots written, the pick screen shows the existing profile's level.
- [ ] **Step 6: Measure** the Task 1 edit again (now in `crates/survivors-app/src/finale.rs`). Expected: well under the baseline; record the number.
- [ ] **Step 7: Commit:** `Survivors: move the mode out of benilla-app into survivors-app`.

### Task 3: Core and feature crates

**Files:**
- Create: `crates/survivors-{core,world,combat,loot,bosses,ui,meta}/` with `Cargo.toml` and `src/lib.rs` each
- Move: modules per the Task 1 table (`git mv`)
- Modify: `crates/survivors-app/src/lib.rs` (keeps `SurvivorsPlugin` and `auto` only)

**Interfaces:**
- Consumes: `survivors_app` from Task 2.
- Produces: `survivors_core::{Phase, Run, Stats, Selection, ...}`; each feature crate exports `pub fn plugin(app: &mut App)`; no feature crate depends on another feature crate (only on `survivors-core` and the engine), checked by `cargo tree -p survivors-bosses -e normal | grep survivors-` listing only `survivors-core`.

- [ ] **Step 1: Core first.** Move the core modules, make the items siblings use `pub`, re-export them at the crate root. Build, test, commit: `Survivors: survivors-core with the shared types`.
- [ ] **Step 2: One leaf at a time**, in this order: `bosses`, `loot`, `meta`, `ui`, `combat`, `world`. Per leaf: move, replace `super::` paths with `survivors_core::`, register its plugin in `survivors-app`, build, run its tests, commit (`Survivors: survivors-<leaf> crate`). A leaf that needs another leaf's item: move that item to core instead of adding the edge.
- [ ] **Step 3: Verify** as Task 2 Step 5, plus the dependency check above for every leaf.
- [ ] **Step 4: Measure** the edit in `survivors-bosses/src/finale.rs` and one in `survivors-core` (expected slower: every crate rebuilds). Record both in the baseline file.

### Task 4: Working rules for the new layout

**Files:**
- Modify: `CLAUDE.md` (fork section), `README.md` ("What this fork adds": a row without screenshot, "Faster rebuilds: survivors in its own crates", PR column "none, built on `features`"), memory `worktree-workflow.md`

- [ ] **Step 1:** In CLAUDE.md's feature-round section: one agent per crate where possible; agents check only their crate (`CC=gcc cargo check -p survivors-<leaf> --profile play`) when the orchestrator allows cargo; shared types change only in `survivors-core`, and only by the orchestrator.
- [ ] **Step 2:** README row with the measured before and after numbers from the baseline file.
- [ ] **Step 3: Commit:** `Docs: survivors crate layout and rebuild times`.
