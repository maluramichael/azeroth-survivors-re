# Rebuild timings

`CC=gcc cargo build --profile play -p warcraft-survivors`, warm `target/`, MacBook Pro, 2026-10-31.

## Before the split (survivors inside benilla-app)

| edit | wall time |
| --- | --- |
| comment line in `survivors/finale.rs` | 11 s |
| `FINALE_HP` 1.5 to 1.51 in `survivors/finale.rs` | 6 s (benilla-app 4.9 s, link 1.25 s) |
| `BASE_HP` 120 to 121 in `survivors/mod.rs` | 7 s |
| `DIM` colour in `survivors/hud.rs` | 12 s |
| file touched back to its old content | 6 s |
| the round of ten features (13 files changed, 11 new) | 2 min 44 s |

## After Task 2 (survivors in its own crate `survivors-app`)

| edit | wall time |
| --- | --- |
| `FINALE_HP` 1.5 to 1.51 in `finale.rs` | 2 s (survivors-app, then the link) |
| `BASE_HP` 120 to 121 in `lib.rs` | 2 s |
| `DIM` colour in `hud.rs` | 3.5 s |
| the whole of `survivors-app` from scratch (`cargo clean -p survivors-app`) | 11 s |

The link alone is about 1.3 s, so a leaf crate could save at most about 1 s per edit on top of this.
