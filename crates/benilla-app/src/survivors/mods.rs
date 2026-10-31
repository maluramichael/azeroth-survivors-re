//! Stat modifiers from the run's own sources (gear, world buffs, talents), folded into
//! [`super::Run::stats`] after the passives and the profile's bonus. Every field adds; the
//! multiplied stats take a fraction (0.1 = +10 %).

use super::Stats;

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub(crate) struct StatMods {
    /// Added to might (1.0 is base damage).
    pub might: f32,
    /// Max health, as a fraction added.
    pub max_hp: f32,
    /// Movement speed, as a fraction added.
    pub speed: f32,
    /// Health per second.
    pub regen: f32,
    /// Cooldown reduction (0.1 = 10 %).
    pub haste: f32,
    /// Added to area (1.0 is base size).
    pub area: f32,
    /// Pickup range, as a fraction added.
    pub magnet: f32,
    /// Added to experience gained (1.0 is base).
    pub growth: f32,
    /// Flat damage taken off every blow.
    pub armor: f32,
    /// Added to critical strike chance (0.05 = 5 %).
    pub crit: f32,
    /// Gold picked up, as a fraction added; read where gold is looted, not in [`Stats`].
    pub greed: f32,
}

impl StatMods {
    pub fn add(&mut self, o: &StatMods) {
        self.might += o.might;
        self.max_hp += o.max_hp;
        self.speed += o.speed;
        self.regen += o.regen;
        self.haste += o.haste;
        self.area += o.area;
        self.magnet += o.magnet;
        self.growth += o.growth;
        self.armor += o.armor;
        self.crit += o.crit;
        self.greed += o.greed;
    }

    pub fn apply(&self, s: &mut Stats) {
        s.might += self.might;
        s.max_hp *= 1.0 + self.max_hp;
        s.speed *= 1.0 + self.speed;
        s.regen += self.regen;
        s.haste += self.haste;
        s.area += self.area;
        s.magnet *= 1.0 + self.magnet;
        s.growth += self.growth;
        s.armor += self.armor;
        s.crit += self.crit;
    }
}
