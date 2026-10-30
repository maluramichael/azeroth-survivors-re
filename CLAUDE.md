@AGENTS.md

# This fork (maluramichael/azeroth-survivors-re)

`AGENTS.md` is upstream's file and stays untouched; the fork's own rules live here.

## Branches and worktrees

- `features` is the fork's default branch: `upstream/main` (OutThisLife/azeroth-survivors-re)
  plus every feature branch merged in with `--no-ff`. It is what people see on the fork and what
  gets played locally, from the main tree `~/development/projects/azeroth-survivors-re`.
- `main` mirrors `upstream/main` and gets no commits of its own.
- New work happens in the work worktree `~/development/projects/azeroth-survivors-re-work`:
  `git switch -c <feature> upstream/main` there, never a branch off `features`. Upstream's
  `docs/CONTRIBUTING.md` wants one change per pull request, so a feature branch holds that one
  change and nothing else. PRs go to `-R OutThisLife/azeroth-survivors-re` with
  `.github/pull_request_template.md`.
- When a feature is done, merge its branch into `features`, then update the README table there.
  New upstream commits are merged into `features` as well.

## Every new feature gets a screenshot and a README row

- Take a screenshot that shows the feature. `WOW_SURVIVORS_AUTO=<class>[:idle|:all|:late]`,
  `WOW_SURVIVORS_MAP=<zone>` and `WOW_SURVIVORS_SHOTS=<dir>` play a run by themselves and save
  screenshots without anyone at the keyboard (`crates/benilla-app/src/survivors/auto.rs`).
- Save it as JPEG, 1600 px wide, as `docs/screenshots/fork-<feature>.jpg`, and keep a copy in
  `~/.claude/screenshots/azeroth-survivors-re/`.
- Add a row at the top of the "What this fork adds" table in `README.md`: feature, what it does in
  one or two sentences, the screenshot at `width="320"`, and the upstream PR link. A change with
  nothing to show (a fix, tooling) gets a row without a screenshot.
- README rows and screenshots are committed on `features` only, never on a feature branch: they
  would break upstream's one-change-per-PR rule.
