//! Screenshots from inside the game, no desktop capture needed.
//!
//! - F12 saves one into the shots folder: `WOW_SURVIVORS_SHOTS` (`--shots`), else `Screenshots`
//!   in the state folder.
//! - `WOW_SURVIVORS_SCRIPT` (`--script`) plays a timeline against the run, for a test without
//!   anyone at the keyboard: `;`-separated steps, each `<seconds> <action>`, the seconds counted
//!   from the run's start. The actions: `shot <name>` saves `<name>.png` into the shots folder,
//!   `key <KEY>` taps a key for one frame, `hold <KEY> <seconds>` keeps it down, `quit` ends the
//!   game. Keys are `F1`..`F12`, `A`..`Z`, `0`..`9`, `Space`, `Escape`, `Enter`, `Tab` and the
//!   arrows (`Up`, `Down`, `Left`, `Right`). Example, with `--debug`:
//!   `6 shot overlay; 7 key F9; 8.5 shot edge; 9 hold W 2; 12 shot walked; 13 quit`.

use std::path::PathBuf;

use bevy::input::InputSystems;
use bevy::prelude::*;
use bevy::render::view::screenshot::{save_to_disk, Screenshot};

use super::Phase;

/// One step of the script.
#[derive(Clone, Debug, PartialEq)]
enum Action {
    Shot(String),
    Key(KeyCode),
    Hold(KeyCode, f32),
    Quit,
}

/// The script's steps, in time order, and how far it has played.
#[derive(Resource, Default)]
struct Script {
    steps: Vec<(f32, Action)>,
    next: usize,
    /// Seconds since the run began; `None` until it has.
    t: Option<f32>,
    /// Keys down: tapped ones until the next frame, held ones until their time is up.
    down: Vec<(KeyCode, f32)>,
}

pub(super) fn plugin(app: &mut App) {
    let script = benilla_app::survivors_host::webenv::var("WOW_SURVIVORS_SCRIPT")
        .map(|s| parse(&s))
        .unwrap_or_default();
    if !script.is_empty() {
        info!("survivors script: {} steps", script.len());
    }
    app.insert_resource(Script {
        steps: script,
        ..default()
    })
    .add_systems(Update, hotkey)
    // After the keyboard is read and before anything reads it, so a scripted key looks typed.
    .add_systems(PreUpdate, play.after(InputSystems));
}

/// The folder screenshots go to.
fn folder() -> Option<PathBuf> {
    std::env::var_os("WOW_SURVIVORS_SHOTS")
        .map(PathBuf::from)
        .or_else(|| benilla_app::survivors_host::local_state::home().map(|h| h.join("Screenshots")))
}

/// Save the window to `name` (`.png` added) in [`folder`]; the path, or `None` without one.
fn shoot(commands: &mut Commands, name: &str) -> Option<PathBuf> {
    let dir = folder()?;
    if let Err(e) = std::fs::create_dir_all(&dir) {
        warn!("survivors shots: cannot make {}: {e}", dir.display());
        return None;
    }
    let path = dir.join(format!("{name}.png"));
    commands
        .spawn(Screenshot::primary_window())
        .observe(save_to_disk(path.clone()));
    Some(path)
}

/// F12: a screenshot, named for the moment.
fn hotkey(
    keys: Res<ButtonInput<KeyCode>>,
    mut commands: Commands,
    mut chat: MessageWriter<super::chatlog::ChatLine>,
    mut n: Local<u32>,
) {
    if !keys.just_pressed(KeyCode::F12) {
        return;
    }
    let secs = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_secs());
    *n += 1;
    if let Some(path) = shoot(&mut commands, &format!("survivors-{secs}-{}", *n)) {
        info!("survivors shots: {}", path.display());
        chat.write(super::chatlog::ChatLine::system(format!(
            "Screenshot saved: {}",
            path.display()
        )));
    }
}

/// Play the script from the run's start: tap, hold and release keys, take the shots, quit.
fn play(
    time: Res<Time<Real>>,
    phase: Res<State<Phase>>,
    mut script: ResMut<Script>,
    mut input: ResMut<ButtonInput<KeyCode>>,
    mut commands: Commands,
    mut exit: MessageWriter<AppExit>,
) {
    if script.steps.is_empty() {
        return;
    }
    let s = &mut *script;
    let t = match s.t {
        Some(t) => t + time.delta_secs(),
        None if *phase.get() == Phase::Playing => 0.0,
        None => return,
    };
    s.t = Some(t);
    // Release what is due before pressing again, so a tap of the same key lands anew.
    s.down.retain(|&(k, until)| {
        let keep = t < until;
        if !keep {
            input.release(k);
        }
        keep
    });
    for &(k, _) in &s.down {
        input.press(k);
    }
    while let Some((at, action)) = s.steps.get(s.next).cloned() {
        if t < at {
            break;
        }
        s.next += 1;
        match action {
            Action::Shot(name) => {
                if let Some(path) = shoot(&mut commands, &name) {
                    info!("survivors script: {at}s shot {}", path.display());
                }
            }
            // Down for this frame only: released on the next.
            Action::Key(k) => {
                input.press(k);
                s.down.push((k, t));
            }
            Action::Hold(k, secs) => {
                input.press(k);
                s.down.push((k, t + secs));
            }
            Action::Quit => {
                info!("survivors script: {at}s quit");
                exit.write(AppExit::Success);
            }
        }
    }
}

/// The steps of a script, sorted by time; a step that does not read is skipped with a warning.
fn parse(script: &str) -> Vec<(f32, Action)> {
    let mut steps: Vec<(f32, Action)> = script
        .split(';')
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .filter_map(|step| {
            let parsed = parse_step(step);
            if parsed.is_none() {
                warn!("survivors script: cannot read `{step}`");
            }
            parsed
        })
        .collect();
    steps.sort_by(|a, b| a.0.total_cmp(&b.0));
    steps
}

fn parse_step(step: &str) -> Option<(f32, Action)> {
    let mut words = step.split_whitespace();
    let at: f32 = words
        .next()?
        .parse()
        .ok()
        .filter(|t: &f32| t.is_finite() && *t >= 0.0)?;
    let action = match words.next()?.to_ascii_lowercase().as_str() {
        "shot" => {
            let name = words.next()?;
            // A bare file name: no folder of its own and no way out of the shots folder.
            if name.contains(['/', '\\']) || name.starts_with('.') {
                return None;
            }
            Action::Shot(name.to_string())
        }
        "key" => Action::Key(key(words.next()?)?),
        "hold" => {
            let k = key(words.next()?)?;
            let secs: f32 = words
                .next()?
                .parse()
                .ok()
                .filter(|s: &f32| s.is_finite() && *s > 0.0)?;
            Action::Hold(k, secs)
        }
        "quit" => Action::Quit,
        _ => return None,
    };
    words.next().is_none().then_some((at, action))
}

/// A key by its name: `F1`..`F12`, a letter, a digit, or one of the named keys.
fn key(name: &str) -> Option<KeyCode> {
    const F: [KeyCode; 12] = [
        KeyCode::F1,
        KeyCode::F2,
        KeyCode::F3,
        KeyCode::F4,
        KeyCode::F5,
        KeyCode::F6,
        KeyCode::F7,
        KeyCode::F8,
        KeyCode::F9,
        KeyCode::F10,
        KeyCode::F11,
        KeyCode::F12,
    ];
    const LETTERS: [KeyCode; 26] = [
        KeyCode::KeyA,
        KeyCode::KeyB,
        KeyCode::KeyC,
        KeyCode::KeyD,
        KeyCode::KeyE,
        KeyCode::KeyF,
        KeyCode::KeyG,
        KeyCode::KeyH,
        KeyCode::KeyI,
        KeyCode::KeyJ,
        KeyCode::KeyK,
        KeyCode::KeyL,
        KeyCode::KeyM,
        KeyCode::KeyN,
        KeyCode::KeyO,
        KeyCode::KeyP,
        KeyCode::KeyQ,
        KeyCode::KeyR,
        KeyCode::KeyS,
        KeyCode::KeyT,
        KeyCode::KeyU,
        KeyCode::KeyV,
        KeyCode::KeyW,
        KeyCode::KeyX,
        KeyCode::KeyY,
        KeyCode::KeyZ,
    ];
    const DIGITS: [KeyCode; 10] = [
        KeyCode::Digit0,
        KeyCode::Digit1,
        KeyCode::Digit2,
        KeyCode::Digit3,
        KeyCode::Digit4,
        KeyCode::Digit5,
        KeyCode::Digit6,
        KeyCode::Digit7,
        KeyCode::Digit8,
        KeyCode::Digit9,
    ];
    let upper = name.to_ascii_uppercase();
    if let Some(n) = upper
        .strip_prefix('F')
        .and_then(|n| n.parse::<usize>().ok())
    {
        return (1..=12).contains(&n).then(|| F[n - 1]);
    }
    let mut chars = upper.chars();
    if let (Some(c), None) = (chars.next(), chars.next()) {
        if c.is_ascii_uppercase() {
            return Some(LETTERS[(c as u8 - b'A') as usize]);
        }
        if c.is_ascii_digit() {
            return Some(DIGITS[(c as u8 - b'0') as usize]);
        }
    }
    Some(match upper.as_str() {
        "SPACE" => KeyCode::Space,
        "ESCAPE" | "ESC" => KeyCode::Escape,
        "ENTER" | "RETURN" => KeyCode::Enter,
        "TAB" => KeyCode::Tab,
        "UP" => KeyCode::ArrowUp,
        "DOWN" => KeyCode::ArrowDown,
        "LEFT" => KeyCode::ArrowLeft,
        "RIGHT" => KeyCode::ArrowRight,
        _ => return None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_script_reads_in_time_order() {
        let steps = parse("8.5 shot edge; 6 shot overlay ;7 key f9; 9 hold W 2; 13 quit;");
        assert_eq!(
            steps,
            vec![
                (6.0, Action::Shot("overlay".into())),
                (7.0, Action::Key(KeyCode::F9)),
                (8.5, Action::Shot("edge".into())),
                (9.0, Action::Hold(KeyCode::KeyW, 2.0)),
                (13.0, Action::Quit),
            ]
        );
    }

    #[test]
    fn a_step_that_does_not_read_is_skipped() {
        let steps = parse(
            "x shot a; 1 shot; 2 key F13; 3 hold W; 4 shot ../up; 5 shot a b; 6 jump; 7 key Esc",
        );
        assert_eq!(steps, vec![(7.0, Action::Key(KeyCode::Escape))]);
    }

    #[test]
    fn keys_read_by_name() {
        assert_eq!(key("F1"), Some(KeyCode::F1));
        assert_eq!(key("f12"), Some(KeyCode::F12));
        assert_eq!(key("F0"), None);
        assert_eq!(key("a"), Some(KeyCode::KeyA));
        assert_eq!(key("7"), Some(KeyCode::Digit7));
        assert_eq!(key("space"), Some(KeyCode::Space));
        assert_eq!(key("Left"), Some(KeyCode::ArrowLeft));
        assert_eq!(key("??"), None);
    }
}
