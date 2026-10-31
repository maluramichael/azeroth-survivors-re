//! Wave surges: every so often the horde presses in harder for a few seconds, announced ahead.
//!
//! The clock is [`super::Run::elapsed`], so a surge pauses with the game. [`tick`] sets
//! [`Surge::active`], which [`super::enemies::spawn_enemies`] reads; [`refresh_text`] keeps the
//! line under the clock ("Surge in 5", then "The horde surges!").

use bevy::prelude::*;

use super::kit::{self, KitFonts};
use super::{finale, Phase, Run};

/// Seconds into the run before the first surge.
const FIRST: f32 = 60.0;
/// Seconds from the start of one surge to the start of the next.
const PERIOD: f32 = 45.0;
/// Seconds a surge lasts.
const LENGTH: f32 = 12.0;
/// Seconds ahead of a surge that the countdown shows.
const WARN_SECS: f32 = 5.0;
/// Distance from the top of the 768-unit canvas: under the clock's level line.
const TOP: f32 = 88.0;
/// Over the HUD (400), level with the damage meter.
const Z: i32 = 401;

pub(super) fn plugin(app: &mut App) {
    app.init_resource::<Surge>()
        .add_systems(Update, tick.run_if(super::run_on))
        .add_systems(Update, refresh_text)
        .add_systems(OnEnter(Phase::Menu), reset);
}

/// Read by [`super::enemies::spawn_enemies`]: while `active`, more enemies come.
#[derive(Resource, Default)]
pub(crate) struct Surge {
    pub active: bool,
    /// Seconds until the next surge starts, or until the running one ends.
    secs: f32,
}

/// The line under the clock.
#[derive(Component)]
struct SurgeText;

/// Where a run's clock stands in the surge cycle: whether a surge is on, and the seconds until
/// the next change (the surge's end while on, the next surge's start while off).
fn phase_at(elapsed: f32) -> (bool, f32) {
    if elapsed < FIRST {
        return (false, FIRST - elapsed);
    }
    let t = (elapsed - FIRST) % PERIOD;
    if t < LENGTH {
        (true, LENGTH - t)
    } else {
        (false, PERIOD - t)
    }
}

/// Follow the run's clock. The finale has the arena to itself, so there is no surge from there on.
fn tick(run: Res<Run>, mut surge: ResMut<Surge>) {
    let (active, secs) = if run.elapsed >= finale::finale_at() {
        (false, f32::INFINITY)
    } else {
        phase_at(run.elapsed)
    };
    surge.active = active;
    surge.secs = secs;
}

/// A new run starts from the menu: no surge carries over.
fn reset(mut surge: ResMut<Surge>) {
    *surge = Surge::default();
}

/// Spawn the line once the fonts are up, show it while a run is on, and word it.
fn refresh_text(
    mut commands: Commands,
    phase: Res<State<Phase>>,
    fonts: Option<Res<KitFonts>>,
    surge: Res<Surge>,
    mut line: Query<(&mut Text, &mut Node), With<SurgeText>>,
    mut spawned: Local<bool>,
) {
    let Ok((mut text, mut node)) = line.single_mut() else {
        if !*spawned {
            if let Some(fonts) = fonts {
                *spawned = true;
                commands.spawn((
                    SurgeText,
                    kit::body(&fonts, "", 16.0, kit::WARN),
                    TextLayout::new_with_justify(Justify::Center),
                    Node {
                        position_type: PositionType::Absolute,
                        top: Val::Px(TOP),
                        width: Val::Percent(100.0),
                        display: Display::None,
                        ..default()
                    },
                    GlobalZIndex(Z),
                ));
            }
        }
        return;
    };
    let in_run = matches!(
        phase.get(),
        Phase::Playing | Phase::LevelUp | Phase::Paused
    );
    let s = if !in_run {
        String::new()
    } else if surge.active {
        "The horde surges!".to_string()
    } else if surge.secs <= WARN_SECS {
        format!("Surge in {}", surge.secs.ceil() as u32)
    } else {
        String::new()
    };
    let display = if s.is_empty() { Display::None } else { Display::Flex };
    if node.display != display {
        node.display = display;
    }
    if text.0 != s {
        text.0 = s;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn quiet_before_the_first_surge() {
        assert_eq!(phase_at(0.0), (false, 60.0));
        assert_eq!(phase_at(55.0), (false, 5.0));
    }

    #[test]
    fn a_surge_runs_twelve_seconds() {
        assert_eq!(phase_at(60.0), (true, 12.0));
        assert_eq!(phase_at(66.0), (true, 6.0));
        assert_eq!(phase_at(72.0), (false, 33.0));
    }

    #[test]
    fn the_cycle_repeats_every_forty_five_seconds() {
        assert_eq!(phase_at(100.0), (false, 5.0));
        assert_eq!(phase_at(105.0), (true, 12.0));
        assert_eq!(phase_at(105.0 + 3.0 * PERIOD), (true, 12.0));
    }
}
