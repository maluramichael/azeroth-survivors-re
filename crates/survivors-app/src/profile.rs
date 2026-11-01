//! The persistent profile: one character per class with a level that carries across runs, the
//! gold banked from runs, which classes are bought, and the trainer's ranks. Saved as
//! `benilla-config/survivors-profile.toml` (`localStorage` on the page) through
//! [`crate::local_state`], after every change.
//!
//! A run pays out when it ends ([`award_run`]): character experience from kills, time, bosses and
//! a win, and the gold picked up. The character's level gates the zones ([`zone_min_level`]) and
//! adds a little health and damage ([`Profile::bonus`]), so a higher zone is reachable and
//! survivable. `WOW_SURVIVORS_UNLOCK_ALL=1` opens every class and zone for testing, without saving
//! that; an automated run (`WOW_SURVIVORS_AUTO`) opens everything and saves nothing.

use std::collections::{BTreeMap, BTreeSet};

use bevy::prelude::*;
use serde::{Deserialize, Serialize};

use super::chatlog::{ChatKind, ChatLine};
use super::difficulty::Difficulty;
use super::mods::StatMods;
use super::{data, hud, talents, Run, Selection};

/// The level cap, as in 1.12.
pub(super) const MAX_LEVEL: u32 = 60;

/// Bought from the start; the rest cost gold ([`class_price`]). Indices into [`data::CLASSES`]:
/// Warrior, Hunter, Mage, one each of melee, ranged and caster.
const STARTER_CLASSES: [usize; 3] = [0, 2, 6];

/// One class's character.
#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub(super) struct Character {
    pub level: u32,
    /// Experience into the current level.
    pub xp: u64,
}

impl Default for Character {
    fn default() -> Self {
        Self { level: 1, xp: 0 }
    }
}

/// What the last run paid out, for the results screen. Not saved.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(super) struct Award {
    pub xp: u64,
    pub levels: u32,
    pub gold: u64,
}

/// The meta progression's effect on a run, copied into [`Run::meta`] when the run starts so
/// [`Run::stats`] stays a pure function of the run.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub(crate) struct MetaBonus {
    /// Multiplies max health.
    pub max_hp: f32,
    /// Added to might (1.0 is base damage).
    pub might: f32,
    /// Multiplies movement speed.
    pub speed: f32,
    /// Added to haste (cooldown reduction, 0.1 = 10 %).
    pub haste: f32,
    /// Multiplies the pickup radius.
    pub magnet: f32,
    /// Added to armor (flat damage taken off every blow).
    pub armor: f32,
    /// Added to health regenerated per second.
    pub regen: f32,
    /// Multiplies gold picked up.
    pub greed: f32,
}

impl Default for MetaBonus {
    fn default() -> Self {
        Self {
            max_hp: 1.0,
            might: 0.0,
            speed: 1.0,
            haste: 0.0,
            magnet: 1.0,
            armor: 0.0,
            regen: 0.0,
            greed: 1.0,
        }
    }
}

#[derive(Resource, Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub(super) struct Profile {
    pub gold: u64,
    /// One per [`data::CLASSES`] entry, by index.
    pub characters: Vec<Character>,
    /// Bought classes, by index into [`data::CLASSES`].
    pub classes: Vec<bool>,
    /// The trainer's ranks, by upgrade key.
    pub trainer: BTreeMap<String, u8>,
    /// The talent ranks of every class, by [`talents::key`].
    pub talents: BTreeMap<String, u8>,
    /// The pick screen's difficulty, kept across runs.
    pub difficulty: Difficulty,
    /// Every item template ever picked up, by name ([`super::collection`]).
    pub collection: BTreeSet<String>,
    /// Earned achievements, by key, with the day each was earned (days since 1970)
    /// ([`super::achievements`]).
    pub achievements: BTreeMap<String, u32>,
    /// Lifetime counters the achievements read (kills, gold, bosses, runs won per zone...).
    pub counters: BTreeMap<String, u64>,
    /// Reward points spent on perks: rank by perk key ([`super::achievements`]).
    pub perks: BTreeMap<String, u8>,
    /// Best run per zone and class, by [`super::records::key`] ([`super::records`]).
    pub records: BTreeMap<String, super::records::Best>,
    /// What the last run paid out.
    #[serde(skip)]
    pub last_award: Option<Award>,
    /// `WOW_SURVIVORS_UNLOCK_ALL`: every class and zone open; never saved.
    #[serde(skip)]
    pub unlock_all: bool,
    /// An automated run (`WOW_SURVIVORS_AUTO`) or the debug mode ([`super::debug`]): everything
    /// open and nothing saved, so a test run never touches the player's progress.
    #[serde(skip)]
    pub ephemeral: bool,
}

impl Default for Profile {
    fn default() -> Self {
        Self {
            gold: 0,
            characters: vec![Character::default(); data::CLASSES.len()],
            classes: (0..data::CLASSES.len())
                .map(|i| STARTER_CLASSES.contains(&i))
                .collect(),
            trainer: BTreeMap::new(),
            talents: BTreeMap::new(),
            difficulty: Difficulty::default(),
            collection: BTreeSet::new(),
            achievements: BTreeMap::new(),
            counters: BTreeMap::new(),
            perks: BTreeMap::new(),
            records: BTreeMap::new(),
            last_award: None,
            unlock_all: false,
            ephemeral: false,
        }
    }
}

impl Profile {
    fn path() -> Option<std::path::PathBuf> {
        benilla_app::survivors_host::local_state::home().map(|h| h.join("survivors-profile.toml"))
    }

    /// The saved profile, or a fresh one; a file that does not parse is kept aside as `.bad` so a
    /// format change never silently wipes progress.
    pub fn load() -> Self {
        let mut p = Self::path()
            .and_then(|path| {
                let text = benilla_app::survivors_host::local_state::read_to_string(&path).ok()?;
                match toml::from_str::<Self>(&text) {
                    Ok(p) => Some(p),
                    Err(e) => {
                        warn!("survivors: profile unreadable, starting fresh ({e})");
                        let _ =
                            benilla_app::survivors_host::local_state::write_atomic(&path.with_extension("bad"), &text);
                        None
                    }
                }
            })
            .unwrap_or_default();
        p.normalize();
        p.ephemeral = super::throwaway();
        if p.ephemeral {
            p.apply_test_env();
        }
        // An automated run opens everything unless `WOW_SURVIVORS_UNLOCK_ALL=0` keeps the locks, to
        // photograph them.
        p.unlock_all = match benilla_app::survivors_host::webenv::var("WOW_SURVIVORS_UNLOCK_ALL") {
            Some(v) => v != "0",
            None => p.ephemeral,
        };
        p
    }

    /// `WOW_SURVIVORS_LEVEL=<n>` sets every character's level and `WOW_SURVIVORS_DIFFICULTY=
    /// normal|heroic|hardcore` the pick, for an automated run; it saves nothing.
    fn apply_test_env(&mut self) {
        if let Some(level) = benilla_app::survivors_host::webenv::var("WOW_SURVIVORS_LEVEL").and_then(|v| v.parse().ok())
        {
            for c in &mut self.characters {
                c.level = u32::clamp(level, 1, MAX_LEVEL);
            }
        }
        if let Some(d) = benilla_app::survivors_host::webenv::var("WOW_SURVIVORS_DIFFICULTY") {
            let d = d.trim().to_ascii_lowercase();
            if let Some(d) = Difficulty::ALL
                .into_iter()
                .find(|x| x.label().to_ascii_lowercase() == d)
            {
                self.difficulty = d;
            }
        }
    }

    pub fn save(&self) {
        if self.ephemeral {
            return;
        }
        let Some(path) = Self::path() else { return };
        match toml::to_string(self) {
            Ok(text) => {
                if let Err(e) = benilla_app::survivors_host::local_state::write_atomic(&path, &text) {
                    warn!("survivors: profile not saved ({e})");
                }
            }
            Err(e) => warn!("survivors: profile not serialized ({e})"),
        }
    }

    /// Pad or trim the per-class lists to the class table, and clamp levels.
    fn normalize(&mut self) {
        let n = data::CLASSES.len();
        self.characters.resize(n, Character::default());
        let fresh = Self::default().classes;
        self.classes
            .extend_from_slice(&fresh[self.classes.len().min(n)..]);
        self.classes.truncate(n);
        for &i in &STARTER_CLASSES {
            self.classes[i] = true;
        }
        for c in &mut self.characters {
            c.level = c.level.clamp(1, MAX_LEVEL);
        }
    }

    pub fn character(&self, class: usize) -> Character {
        self.characters.get(class).copied().unwrap_or_default()
    }

    pub fn class_unlocked(&self, class: usize) -> bool {
        self.unlock_all || self.classes.get(class).copied().unwrap_or(false)
    }

    /// Whether `class`'s character may enter `map`: its level is at least the zone's lowest.
    pub fn zone_unlocked(&self, class: usize, map: usize) -> bool {
        self.unlock_all || self.character(class).level >= zone_min_level(map)
    }

    /// The bonus a run with `class` starts with: the character's level, then the trainer.
    pub fn bonus(&self, class: usize) -> MetaBonus {
        let level = self.character(class).level.saturating_sub(1) as f32;
        let mut bonus = MetaBonus {
            // Level 60: +118 % health, +89 % might.
            max_hp: 1.0 + 0.02 * level,
            might: 0.015 * level,
            ..MetaBonus::default()
        };
        super::trainer::apply_ranks(&mut bonus, &self.trainer);
        bonus
    }

    /// The talent bonus a run with `class` starts with ([`Run::talents`]).
    pub fn talent_bonus(&self, class: usize) -> StatMods {
        talents::mods_for(&self.talents, class)
    }

    /// Add character experience to `class`, levelling up as far as it reaches; returns the levels
    /// gained. Experience past the cap is dropped.
    pub fn add_xp(&mut self, class: usize, xp: u64) -> u32 {
        let Some(c) = self.characters.get_mut(class) else {
            return 0;
        };
        let before = c.level;
        c.xp += xp;
        while c.level < MAX_LEVEL && c.xp >= xp_to_next(c.level) {
            c.xp -= xp_to_next(c.level);
            c.level += 1;
        }
        if c.level == MAX_LEVEL {
            c.xp = 0;
        }
        c.level - before
    }
}

/// Character experience from `level` to the next: a 10-minute run in a zone of its level is
/// about one level early on, a few runs per level near the cap.
pub(super) fn xp_to_next(level: u32) -> u64 {
    150 + 60 * u64::from(level.saturating_sub(1))
}

/// The lowest creature level in `map`'s roster, the "Level 10" of its "Level 10-32" at the menu.
pub(super) fn zone_min_level(map: usize) -> u32 {
    data::roster(map)
        .enemies
        .iter()
        .map(|e| e.level)
        .min()
        .unwrap_or(1)
}

/// The character experience a finished run earns: kills, time survived, bosses and a win, more in
/// a zone above the character's level and less below it.
pub(super) fn run_xp(run: &Run, char_level: u32, zone_level: u32) -> u64 {
    let base = run.kills as f32
        + run.elapsed * 0.5
        + run.bosses as f32 * 50.0
        + if run.won { 200.0 } else { 0.0 };
    // Ten levels below the character is half, ten above is double.
    let gap = zone_level as f32 - char_level as f32;
    let scale = 2f32.powf((gap / 10.0).clamp(-1.0, 1.0));
    (base * scale).round() as u64
}

/// Startup: read the profile.
pub(super) fn load_profile(mut commands: Commands) {
    commands.insert_resource(Profile::load());
}

/// `OnEnter(GameOver)`: pay out the run (experience to its class's character, its gold to the
/// bank) and save.
pub(super) fn award_run(
    run: Res<Run>,
    role: Res<super::NetRole>,
    selection: Res<Selection>,
    mut profile: ResMut<Profile>,
    mut chat: MessageWriter<ChatLine>,
) {
    let Some(class) = data::CLASSES.iter().position(|c| c.id == run.class.id) else {
        return;
    };
    let char_level = profile.character(class).level;
    let reward = run.difficulty.reward_mult();
    let xp = (run_xp(&run, char_level, zone_min_level(selection.stage)) as f32 * reward).round()
        as u64;
    let levels = profile.add_xp(class, xp);
    let gold = (u64::from(run.gold) as f32 * reward).round() as u64;
    profile.gold += gold;
    profile.last_award = Some(Award { xp, levels, gold });
    profile.save();
    if run.won {
        chat.write(ChatLine::system("Victory!"));
    } else if run.host_gone {
        chat.write(ChatLine::system("The host left; the run is over."));
    } else if *role != super::NetRole::Solo {
        chat.write(ChatLine::text(ChatKind::Combat, "Your party has fallen."));
    } else {
        chat.write(ChatLine::text(ChatKind::Combat, "You have died."));
    }
    chat.write(ChatLine::text(
        ChatKind::Xp,
        format!(
            "Your {} gains {} experience.",
            run.class.name,
            hud::grouped(xp)
        ),
    ));
    if levels > 0 {
        chat.write(ChatLine::system(format!(
            "Your {} has reached level {}.",
            run.class.name,
            profile.character(class).level
        )));
    }
    chat.write(ChatLine::text(
        ChatKind::Money,
        format!("{} gold added to your bank.", hud::grouped(gold)),
    ));
    info!(
        "survivors: run paid {xp} xp ({levels} levels, now {}) and {gold} gold (bank {})",
        profile.character(class).level,
        profile.gold
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn levels_carry_over_and_cap() {
        let mut p = Profile::default();
        assert_eq!(p.add_xp(0, xp_to_next(1) - 1), 0);
        assert_eq!(p.add_xp(0, 1), 1);
        assert_eq!(p.character(0), Character { level: 2, xp: 0 });
        assert!(p.add_xp(0, u64::MAX / 2) > 0);
        assert_eq!(
            p.character(0),
            Character {
                level: MAX_LEVEL,
                xp: 0
            }
        );
    }

    #[test]
    fn starters_are_unlocked_and_survive_a_short_file() {
        let mut p: Profile = toml::from_str("gold = 5\nclasses = [false]").unwrap();
        p.normalize();
        assert_eq!(p.gold, 5);
        assert_eq!(p.classes.len(), data::CLASSES.len());
        assert_eq!(p.characters.len(), data::CLASSES.len());
        for i in 0..data::CLASSES.len() {
            assert_eq!(
                p.class_unlocked(i),
                STARTER_CLASSES.contains(&i),
                "class {i}"
            );
        }
    }

    #[test]
    fn round_trips_through_toml() {
        let mut p = Profile::default();
        p.gold = 1234;
        p.add_xp(2, 1000);
        p.trainer.insert("might".into(), 3);
        let back: Profile = toml::from_str(&toml::to_string(&p).unwrap()).unwrap();
        assert_eq!(back, p);
    }

    #[test]
    fn level_bonus_grows_with_level() {
        let mut p = Profile::default();
        assert_eq!(p.bonus(0), MetaBonus::default());
        p.characters[0].level = 11;
        let b = p.bonus(0);
        assert!((b.max_hp - 1.2).abs() < 1e-5 && (b.might - 0.15).abs() < 1e-5);
    }
}
