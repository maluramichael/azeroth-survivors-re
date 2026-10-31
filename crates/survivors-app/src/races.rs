//! Racial traits: each race lends the hero a small bonus for the whole run, folded into
//! [`super::Run::stats`] beside gear, world buffs and talents.

use bevy::prelude::*;

use super::mods::StatMods;
use super::talents::Fx;

pub(super) fn plugin(_app: &mut App) {}

/// One race's trait: the 1.12 racial it is named after and the edge it gives.
struct Racial {
    /// The pick screen's race id (the WoW race id, see `char_select::race_name`).
    race: u8,
    name: &'static str,
    fx: Fx,
    value: f32,
}

/// Every race the pick screen offers. Each is worth about one talent's three to five ranks,
/// so none outweighs a class talent or a trainer upgrade.
const RACIALS: [Racial; 8] = [
    Racial { race: 1, name: "The Human Spirit", fx: Fx::Growth, value: 0.10 },
    Racial { race: 2, name: "Blood Fury", fx: Fx::Might, value: 0.08 },
    Racial { race: 3, name: "Stoneform", fx: Fx::Armor, value: 1.0 },
    Racial { race: 4, name: "Quickness", fx: Fx::Speed, value: 0.05 },
    Racial { race: 5, name: "Cannibalize", fx: Fx::Regen, value: 0.5 },
    Racial { race: 6, name: "Endurance", fx: Fx::MaxHp, value: 0.08 },
    Racial { race: 7, name: "Expansive Mind", fx: Fx::Area, value: 0.06 },
    Racial { race: 8, name: "Berserking", fx: Fx::Haste, value: 0.05 },
];

fn racial(race: u8) -> Option<&'static Racial> {
    RACIALS.iter().find(|r| r.race == race)
}

/// The bonus `race` (the pick screen's race id, [`super::Run::race`]) gives.
pub(crate) fn mods(race: u8) -> StatMods {
    let mut m = StatMods::default();
    if let Some(r) = racial(race) {
        r.fx.apply(&mut m, r.value);
    }
    m
}

/// The name of `race`'s racial, or `None` for an id the pick screen does not offer.
#[cfg(test)]
fn trait_name(race: u8) -> Option<&'static str> {
    racial(race).map(|r| r.name)
}

/// "Blood Fury: +8% damage": the racial and what it gives, as the hub prints it.
pub(crate) fn trait_text(race: u8) -> Option<String> {
    racial(race).map(|r| format!("{}: {}", r.name, r.fx.describe(r.value)))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_race_has_a_trait() {
        for race in 1..=8u8 {
            assert!(trait_name(race).is_some(), "race {race}");
            assert!(trait_text(race).is_some(), "race {race}");
        }
    }

    #[test]
    fn mods_are_not_empty() {
        for race in 1..=8u8 {
            let m = mods(race);
            let sum = m.might
                + m.max_hp
                + m.speed
                + m.regen
                + m.haste
                + m.area
                + m.magnet
                + m.growth
                + m.armor
                + m.crit;
            assert!(sum > 0.0, "race {race}");
            // Gold is read outside `Stats`, so a racial must not live there.
            assert_eq!(m.greed, 0.0, "race {race}");
        }
    }

    #[test]
    fn unknown_ids_give_nothing() {
        for race in [0u8, 9, 255] {
            assert_eq!(mods(race), StatMods::default());
            assert!(trait_name(race).is_none());
            assert!(trait_text(race).is_none());
        }
    }

    #[test]
    fn text_names_the_racial_and_the_edge() {
        assert_eq!(trait_text(2).as_deref(), Some("Blood Fury: +8% damage"));
        assert_eq!(trait_text(5).as_deref(), Some("Cannibalize: +0.5 regen/s"));
    }
}
