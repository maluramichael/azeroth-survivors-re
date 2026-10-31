//! Best runs: per zone and class, the longest survival, most kills and most damage, kept in the
//! profile and shown on the hub.

use bevy::prelude::*;
use serde::{Deserialize, Serialize};

use super::chatlog::ChatLine;
use super::profile::Profile;
use super::{data, Phase, Run, Selection};

pub(super) fn plugin(app: &mut App) {
    app.add_systems(
        OnEnter(Phase::GameOver),
        record_run.after(super::profile::award_run),
    );
}

/// One zone and class's bests. Each field is its own best, not one run's line.
#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub(crate) struct Best {
    /// Longest survival, in seconds.
    pub secs: f32,
    pub kills: u32,
    pub damage: f64,
    pub wins: u32,
}

/// The profile key of a zone and class: the zone's index in [`data::MAPS`] and the class's id,
/// the same identifiers the achievement counters use, so a rename changes no key.
pub(crate) fn key(map: usize, class_id: u8) -> String {
    format!("{map}:{class_id}")
}

/// Fold one run into `best`: each field only if it beats the stored one, a win counts once.
/// True when the survival time or the kill count was beaten (the "New best!" cases).
fn merge(best: &mut Best, secs: f32, kills: u32, damage: f64, won: bool) -> bool {
    let mut beaten = false;
    if secs > best.secs {
        best.secs = secs;
        beaten = true;
    }
    if kills > best.kills {
        best.kills = kills;
        beaten = true;
    }
    if damage > best.damage {
        best.damage = damage;
    }
    if won {
        best.wins += 1;
    }
    beaten
}

/// "12:34".
fn clock(secs: f32) -> String {
    let s = secs.max(0.0).round() as u32;
    format!("{}:{:02}", s / 60, s % 60)
}

/// The hub's note under a zone row: "Best 12:34, 812 kills", empty before the first run.
pub(crate) fn note(best: Option<&Best>) -> String {
    match best {
        Some(b) if b.secs > 0.0 || b.kills > 0 => {
            format!("Best {}, {} kills", clock(b.secs), b.kills)
        }
        _ => String::new(),
    }
}

/// `OnEnter(GameOver)`, after the payout: fold the run into its zone and class's bests, say so in
/// chat when the time or the kills beat an earlier run there, and save.
fn record_run(
    run: Res<Run>,
    selection: Res<Selection>,
    mut profile: ResMut<Profile>,
    mut chat: MessageWriter<ChatLine>,
) {
    let key = key(selection.stage, run.class.id);
    let first = !profile.records.contains_key(&key);
    let best = profile.records.entry(key).or_default();
    let (old_secs, old_kills) = (best.secs, best.kills);
    let beaten = merge(best, run.elapsed, run.kills, run.damage_done, run.won);
    if beaten && !first {
        let what = match (best.secs > old_secs, best.kills > old_kills) {
            (true, true) => format!("{}, {} kills", clock(best.secs), best.kills),
            (true, false) => format!("{} survived", clock(best.secs)),
            _ => format!("{} kills", best.kills),
        };
        chat.write(ChatLine::system(format!(
            "New best! {} as {}: {what}",
            data::MAPS[selection.stage.min(data::MAPS.len() - 1)].name,
            run.class.name
        )));
    }
    // Saves nothing for an automated run.
    profile.save();
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn first_run_sets_every_field() {
        let mut b = Best::default();
        assert!(merge(&mut b, 120.0, 40, 5000.0, false));
        assert_eq!(
            b,
            Best {
                secs: 120.0,
                kills: 40,
                damage: 5000.0,
                wins: 0
            }
        );
    }

    #[test]
    fn each_field_keeps_its_own_best() {
        let mut b = Best {
            secs: 300.0,
            kills: 100,
            damage: 9000.0,
            wins: 0,
        };
        // Shorter, more kills, less damage: only the kills move.
        assert!(merge(&mut b, 200.0, 150, 4000.0, false));
        assert_eq!((b.secs, b.kills, b.damage), (300.0, 150, 9000.0));
    }

    #[test]
    fn a_worse_run_beats_nothing() {
        let mut b = Best {
            secs: 300.0,
            kills: 100,
            damage: 9000.0,
            wins: 1,
        };
        let before = b;
        assert!(!merge(&mut b, 300.0, 100, 9000.0, false));
        assert_eq!(b, before);
    }

    #[test]
    fn damage_alone_is_no_new_best() {
        let mut b = Best {
            secs: 300.0,
            kills: 100,
            damage: 9000.0,
            wins: 0,
        };
        assert!(!merge(&mut b, 100.0, 10, 20000.0, false));
        assert_eq!(b.damage, 20000.0);
    }

    #[test]
    fn wins_count_every_win() {
        let mut b = Best::default();
        merge(&mut b, 600.0, 10, 1.0, true);
        merge(&mut b, 100.0, 1, 1.0, true);
        merge(&mut b, 100.0, 1, 1.0, false);
        assert_eq!(b.wins, 2);
    }

    #[test]
    fn keys_tell_zone_and_class_apart() {
        assert_ne!(key(1, 2), key(2, 1));
        assert_eq!(key(1, 2), key(1, 2));
    }

    #[test]
    fn note_is_empty_without_a_record() {
        assert_eq!(note(None), "");
        assert_eq!(note(Some(&Best::default())), "");
        let b = Best {
            secs: 754.0,
            kills: 812,
            ..Best::default()
        };
        assert_eq!(note(Some(&b)), "Best 12:34, 812 kills");
    }
}
