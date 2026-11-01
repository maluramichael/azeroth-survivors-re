@AGENTS.md

# This fork (maluramichael/azeroth-survivors-re)

`AGENTS.md` is upstream's file and stays untouched; the fork's own rules live here.

## Where the work happens

- This repository is a fork of OutThisLife/azeroth-survivors-re (`upstream`), not the original.
- All work happens on `features`, the fork's default branch, in the main tree
  `~/development/projects/azeroth-survivors-re`. It is what people see on the fork and what gets
  played locally. Commit straight on `features`; there are no feature branches and no pull
  requests to upstream. Do not ask about them.
- New upstream commits may be merged into `features` when Michael asks for it.

## Feature rounds with several agents in one tree

Bigger game features are built by several agents at once in the main tree. One tree means one
`target/` and one incremental build. A round runs like this:

1. The orchestrator (the main session) lays the shared groundwork itself first: data types,
   resources and the hooks every feature needs, plus an empty module per feature already
   registered in `crates/survivors-app/src/lib.rs` (`pub(super) fn plugin(app: &mut App)`). Commit it.
2. Cheap implementer agents (Sonnet) run in parallel, one feature each, in the background. Each
   brief lists exactly which files or functions the agent owns; agents edit only those (Edit,
   never a whole-file rewrite of a shared file), never touch `lib.rs`, run no git command that
   changes state, never launch the game, and check with
   `CC=gcc cargo check -p survivors-app --profile play`, ignoring errors in files they do not own.
3. The orchestrator then builds once (`cargo survivors`), fixes what breaks between features,
   runs the game (auto mode with `WOW_SURVIVORS_SHOTS`) and looks at the screenshots, reads the
   diffs, sends targeted follow-ups to the agents, and commits one commit per feature with its
   README row.

## Every new feature gets a screenshot and a README row

- Take a screenshot that shows the feature. `WOW_SURVIVORS_AUTO=<class>[:idle|:all|:late]`,
  `WOW_SURVIVORS_MAP=<zone>` and `WOW_SURVIVORS_SHOTS=<dir>` play a run by themselves and save
  screenshots without anyone at the keyboard (`crates/survivors-app/src/auto.rs`).
- Save it as JPEG, 1600 px wide, as `docs/screenshots/fork-<feature>.jpg`, and keep a copy in
  `~/.claude/screenshots/azeroth-survivors-re/`.
- Add a row at the top of the "What this fork adds" table in `README.md`: feature, what it does in
  one or two sentences, and the screenshot at `width="480"`. A change with nothing to show (a
  fix, tooling) gets a row without a screenshot.
