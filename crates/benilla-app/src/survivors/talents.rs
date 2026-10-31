//! Talent trees: three specs per class, points from the character's level, free respec at the
//! pick screen. The run's talent bonus lands in [`super::Run::talents`].
//!
//! A character earns one point per level from [`FIRST_LEVEL`] on, 51 at the cap, as in 1.12. A
//! spec is a short column of talents; the talent at position `i` opens once the spec holds
//! [`needs`]`(i)` points, so the column fills from the top, as a WoW tree's tiers do. The ranks are
//! kept in [`Profile::talents`] under [`key`] (class id and talent name) and folded into a
//! [`StatMods`] by [`mods_for`]. Renaming a talent drops its saved ranks; never rename one.
//!
//! The panel is a window in the glue frame art on the pick screen, opened from the Talents button
//! under the zone list. Clicking a talent spends a point; Respec hands every point of the selected
//! class back for free.

use std::collections::BTreeMap;

use bevy::prelude::*;
use bevy::ui::FocusPolicy;

use super::hud::{self, HudFont, MenuRoot};
use super::menu::MenuBuilt;
use super::mods::StatMods;
use super::profile::Profile;
use super::{data, Phase, Selection};
use crate::glue::art::{GlueArt, DIM, GOLD, NAME_EDGE};
use crate::glue::backdrop::{backdrop_border, tiled_bg_node};
use crate::glue::widgets::{
    abs, glue_button, outlined_text, GlueBtnKind, GlueCaption, GlueDisabled, GlueText,
};
use crate::glue::{GlueClicks, GlueVisuals};

/// The first character level that earns a point.
pub(super) const FIRST_LEVEL: u32 = 10;
/// Points a spec must hold per position before the talent at that position opens.
pub(super) const TIER_STEP: u32 = 2;
/// Talents in the longest spec; the panel lays out this many rows.
const MAX_ROWS: usize = 6;

/// What a rank of a talent (or a world buff) adds, one field of [`StatMods`].
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Fx {
    Might,
    MaxHp,
    Speed,
    Regen,
    Haste,
    Area,
    Magnet,
    Growth,
    Armor,
    Crit,
    Greed,
}

impl Fx {
    /// Add `v` of this effect to `m`.
    pub fn apply(self, m: &mut StatMods, v: f32) {
        match self {
            Self::Might => m.might += v,
            Self::MaxHp => m.max_hp += v,
            Self::Speed => m.speed += v,
            Self::Regen => m.regen += v,
            Self::Haste => m.haste += v,
            Self::Area => m.area += v,
            Self::Magnet => m.magnet += v,
            Self::Growth => m.growth += v,
            Self::Armor => m.armor += v,
            Self::Crit => m.crit += v,
            Self::Greed => m.greed += v,
        }
    }

    /// "+3% damage": `v` of this effect, as the panels print it.
    pub fn describe(self, v: f32) -> String {
        match self {
            Self::Might => format!("+{} damage", hud::pct(v)),
            Self::MaxHp => format!("+{} health", hud::pct(v)),
            Self::Speed => format!("+{} speed", hud::pct(v)),
            Self::Regen => format!("+{} regen/s", hud::num(v)),
            Self::Haste => format!("+{} haste", hud::pct(v)),
            Self::Area => format!("+{} area", hud::pct(v)),
            Self::Magnet => format!("+{} pickup range", hud::pct(v)),
            Self::Growth => format!("+{} experience", hud::pct(v)),
            Self::Armor => format!("+{} armor", hud::num(v)),
            Self::Crit => format!("+{} crit", hud::pct(v)),
            Self::Greed => format!("+{} gold", hud::pct(v)),
        }
    }
}

/// One talent: up to `max` ranks, each adding `per_rank` of `fx`.
pub(super) struct Talent {
    pub name: &'static str,
    /// A file in `Interface\Icons`, without the folder and the extension.
    pub icon: &'static str,
    pub fx: Fx,
    pub per_rank: f32,
    pub max: u8,
}

const fn t(name: &'static str, icon: &'static str, fx: Fx, per_rank: f32, max: u8) -> Talent {
    Talent {
        name,
        icon,
        fx,
        per_rank,
        max,
    }
}

/// One spec: a column of talents, top first.
pub(super) struct Spec {
    pub name: &'static str,
    pub icon: &'static str,
    pub talents: &'static [Talent],
}

struct ClassTrees {
    /// [`data::ClassDef::id`].
    id: u8,
    specs: [Spec; 3],
}

use Fx::*;

static TREES: &[ClassTrees] = &[
    ClassTrees {
        id: 1,
        specs: [
            Spec {
                name: "Arms",
                icon: "Ability_Rogue_Eviscerate",
                talents: &[
                    t(
                        "Improved Heroic Strike",
                        "Ability_MeleeDamage",
                        Might,
                        0.03,
                        3,
                    ),
                    t("Deflection", "Ability_Parry", Armor, 0.5, 5),
                    t("Mace Specialization", "INV_Mace_01", Crit, 0.01, 5),
                    t("Deep Wounds", "Ability_BackStab", Might, 0.04, 3),
                    t("Impale", "Ability_Warrior_Impale", Crit, 0.02, 2),
                ],
            },
            Spec {
                name: "Fury",
                icon: "Ability_Warrior_InnerRage",
                talents: &[
                    t("Booming Voice", "Spell_Nature_Purge", Area, 0.04, 5),
                    t("Cruelty", "Ability_Rogue_Eviscerate", Crit, 0.01, 5),
                    t(
                        "Improved Battle Shout",
                        "Ability_Warrior_BattleShout",
                        Might,
                        0.03,
                        5,
                    ),
                    t("Piercing Howl", "Spell_Shadow_DeathScream", Speed, 0.02, 3),
                    t("Enrage", "Spell_Shadow_UnholyFrenzy", Haste, 0.02, 5),
                    t("Flurry", "Ability_GhoulFrenzy", Haste, 0.03, 5),
                ],
            },
            Spec {
                name: "Protection",
                icon: "Ability_Defend",
                talents: &[
                    t("Shield Specialization", "INV_Shield_06", Armor, 0.5, 5),
                    t("Anticipation", "Spell_Nature_MirrorImage", Speed, 0.02, 5),
                    t("Toughness", "Spell_Holy_DevotionAura", MaxHp, 0.04, 5),
                    t(
                        "Improved Bloodrage",
                        "Ability_Racial_BloodRage",
                        Regen,
                        0.15,
                        2,
                    ),
                    t("Defiance", "Ability_Warrior_InnerRage", Armor, 0.5, 3),
                ],
            },
        ],
    },
    ClassTrees {
        id: 2,
        specs: [
            Spec {
                name: "Holy",
                icon: "Spell_Holy_HolyBolt",
                talents: &[
                    t(
                        "Divine Strength",
                        "Spell_Holy_RighteousFury",
                        Might,
                        0.03,
                        5,
                    ),
                    t("Divine Intellect", "Spell_Nature_Sleep", Haste, 0.01, 5),
                    t("Spiritual Focus", "Spell_Arcane_Blink", Area, 0.03, 5),
                    t("Illumination", "Spell_Holy_GuardianSpirit", Regen, 0.2, 5),
                    t("Holy Power", "Spell_Holy_Power", Crit, 0.01, 5),
                ],
            },
            Spec {
                name: "Protection",
                icon: "Spell_Holy_DevotionAura",
                talents: &[
                    t("Redoubt", "Ability_Defend", Armor, 0.5, 5),
                    t(
                        "Improved Righteous Fury",
                        "Spell_Holy_SealOfFury",
                        Armor,
                        0.5,
                        3,
                    ),
                    t("Toughness", "Spell_Holy_DevotionAura", MaxHp, 0.04, 5),
                    t(
                        "Guardian's Favor",
                        "Spell_Holy_SealOfProtection",
                        Speed,
                        0.02,
                        2,
                    ),
                    t(
                        "Anticipation",
                        "Spell_Magic_LesserInvisibility",
                        Regen,
                        0.2,
                        5,
                    ),
                ],
            },
            Spec {
                name: "Retribution",
                icon: "Spell_Holy_AuraOfLight",
                talents: &[
                    t("Benediction", "Spell_Frost_WindWalkOn", Greed, 0.05, 5),
                    t(
                        "Improved Blessing of Might",
                        "Spell_Holy_FistOfJustice",
                        Might,
                        0.03,
                        5,
                    ),
                    t("Conviction", "Spell_Holy_RetributionAura", Crit, 0.01, 5),
                    t("Vindication", "Spell_Holy_Vindication", Magnet, 0.08, 3),
                    t(
                        "Two-Handed Weapon Specialization",
                        "INV_Hammer_04",
                        Might,
                        0.02,
                        5,
                    ),
                ],
            },
        ],
    },
    ClassTrees {
        id: 3,
        specs: [
            Spec {
                name: "Beast Mastery",
                icon: "Ability_Hunter_BeastTaming",
                talents: &[
                    t(
                        "Endurance Training",
                        "Spell_Nature_Reincarnation",
                        MaxHp,
                        0.04,
                        5,
                    ),
                    t(
                        "Improved Aspect of the Hawk",
                        "Spell_Nature_RavenForm",
                        Haste,
                        0.02,
                        5,
                    ),
                    t("Bestial Swiftness", "Ability_Druid_Dash", Speed, 0.04, 1),
                    t("Thick Hide", "INV_Misc_Pelt_Bear_03", Armor, 0.5, 3),
                    t("Ferocity", "INV_Misc_MonsterClaw_04", Crit, 0.01, 5),
                ],
            },
            Spec {
                name: "Marksmanship",
                icon: "Ability_Marksmanship",
                talents: &[
                    t("Lethal Shots", "Ability_SearingArrow", Crit, 0.01, 5),
                    t("Efficiency", "Spell_Frost_WizardMark", Haste, 0.01, 5),
                    t("Mortal Shots", "Ability_PierceDamage", Might, 0.03, 5),
                    t("Hawk Eye", "Ability_TownWatch", Area, 0.04, 3),
                    t("Barrage", "Ability_UpgradeMoonGlaive", Might, 0.02, 3),
                ],
            },
            Spec {
                name: "Survival",
                icon: "Spell_Nature_StrangleVines",
                talents: &[
                    t("Savage Strikes", "Ability_Rogue_Ambush", Crit, 0.02, 2),
                    t("Deflection", "Ability_Parry", Armor, 0.5, 5),
                    t("Survivalist", "Spell_Shadow_Twilight", MaxHp, 0.04, 5),
                    t("Surefooted", "Ability_Kick", Speed, 0.02, 3),
                    t(
                        "Killer Instinct",
                        "Spell_Holy_BlessingOfStamina",
                        Crit,
                        0.01,
                        3,
                    ),
                ],
            },
        ],
    },
    ClassTrees {
        id: 4,
        specs: [
            Spec {
                name: "Assassination",
                icon: "Ability_Rogue_Eviscerate",
                talents: &[
                    t("Malice", "Ability_Racial_BloodRage", Crit, 0.01, 5),
                    t(
                        "Improved Eviscerate",
                        "Ability_Rogue_Eviscerate",
                        Might,
                        0.02,
                        3,
                    ),
                    t("Lethality", "Ability_CriticalStrike", Crit, 0.02, 5),
                    t("Vile Poisons", "Ability_Rogue_FeignDeath", Might, 0.02, 5),
                    t("Seal Fate", "Spell_Shadow_ChillTouch", Haste, 0.01, 5),
                ],
            },
            Spec {
                name: "Combat",
                icon: "Ability_BackStab",
                talents: &[
                    t(
                        "Improved Sinister Strike",
                        "Spell_Shadow_RitualOfSacrifice",
                        Haste,
                        0.01,
                        2,
                    ),
                    t(
                        "Dual Wield Specialization",
                        "Ability_DualWield",
                        Might,
                        0.03,
                        5,
                    ),
                    t("Precision", "Ability_Marksmanship", Crit, 0.01, 5),
                    t("Endurance", "Spell_Shadow_ShadowWard", Haste, 0.02, 2),
                    t(
                        "Lightning Reflexes",
                        "Spell_Nature_Invisibilty",
                        Speed,
                        0.02,
                        5,
                    ),
                    t("Aggression", "Ability_Racial_Avatar", Might, 0.02, 3),
                ],
            },
            Spec {
                name: "Subtlety",
                icon: "Ability_Stealth",
                talents: &[
                    t("Camouflage", "Ability_Stealth", Speed, 0.02, 5),
                    t("Opportunity", "Ability_Warrior_Riposte", Might, 0.04, 5),
                    t("Initiative", "Spell_Shadow_Fumble", Magnet, 0.08, 3),
                    t(
                        "Elusiveness",
                        "Spell_Magic_LesserInvisibility",
                        Armor,
                        0.5,
                        2,
                    ),
                    t("Ghostly Strike", "Spell_Shadow_Curse", Regen, 0.2, 3),
                ],
            },
        ],
    },
    ClassTrees {
        id: 5,
        specs: [
            Spec {
                name: "Discipline",
                icon: "Spell_Holy_PowerWordShield",
                talents: &[
                    t("Unbreakable Will", "Spell_Magic_MageArmor", Armor, 0.5, 5),
                    t("Wand Specialization", "INV_Wand_01", Might, 0.02, 5),
                    t(
                        "Silent Resolve",
                        "Spell_Nature_ManaRegenTotem",
                        Regen,
                        0.2,
                        5,
                    ),
                    t(
                        "Improved Power Word: Fortitude",
                        "Spell_Holy_WordFortitude",
                        MaxHp,
                        0.04,
                        5,
                    ),
                    t("Mental Agility", "Spell_Nature_Brilliance", Haste, 0.01, 5),
                ],
            },
            Spec {
                name: "Holy",
                icon: "Spell_Holy_GuardianSpirit",
                talents: &[
                    t("Healing Focus", "Spell_Holy_HealingFocus", Regen, 0.2, 2),
                    t("Improved Renew", "Spell_Holy_Renew", Regen, 0.2, 3),
                    t(
                        "Holy Specialization",
                        "Spell_Holy_SearingLight",
                        Crit,
                        0.01,
                        5,
                    ),
                    t(
                        "Spiritual Guidance",
                        "Spell_Holy_SpiritualGuidence",
                        Might,
                        0.03,
                        5,
                    ),
                    t("Spell Warding", "Spell_Holy_SpellWarding", Armor, 0.5, 5),
                ],
            },
            Spec {
                name: "Shadow",
                icon: "Spell_Shadow_ShadowWordPain",
                talents: &[
                    t("Spirit Tap", "Spell_Shadow_Requiem", Growth, 0.03, 5),
                    t("Blackout", "Spell_Shadow_GatherShadows", Crit, 0.01, 5),
                    t("Shadow Focus", "Spell_Shadow_ShadowWord", Area, 0.03, 5),
                    t("Shadow Weaving", "Spell_Shadow_BlackPlague", Might, 0.03, 5),
                    t("Darkness", "Spell_Shadow_Twilight", Might, 0.02, 5),
                    t(
                        "Improved Shadow Word: Pain",
                        "Spell_Shadow_ShadowWordPain",
                        Haste,
                        0.01,
                        2,
                    ),
                ],
            },
        ],
    },
    ClassTrees {
        id: 7,
        specs: [
            Spec {
                name: "Elemental",
                icon: "Spell_Nature_Lightning",
                talents: &[
                    t("Convection", "Spell_Nature_WispSplode", Haste, 0.01, 5),
                    t("Concussion", "Spell_Fire_Fireball", Might, 0.01, 5),
                    t("Call of Flame", "Spell_Fire_Immolation", Area, 0.03, 3),
                    t("Call of Thunder", "Spell_Nature_CallStorm", Crit, 0.01, 5),
                    t("Reverberation", "Spell_Frost_FrostWard", Haste, 0.01, 5),
                ],
            },
            Spec {
                name: "Enhancement",
                icon: "Spell_Nature_LightningShield",
                talents: &[
                    t(
                        "Ancestral Knowledge",
                        "Spell_Shadow_GrimWard",
                        MaxHp,
                        0.02,
                        5,
                    ),
                    t("Thundering Strikes", "Ability_ThunderBolt", Crit, 0.01, 5),
                    t(
                        "Improved Ghost Wolf",
                        "Spell_Nature_SpiritWolf",
                        Speed,
                        0.02,
                        2,
                    ),
                    t(
                        "Weapon Mastery",
                        "Ability_Hunter_SwiftStrike",
                        Might,
                        0.02,
                        5,
                    ),
                    t(
                        "Elemental Weapons",
                        "Spell_FireResistanceTotem_01",
                        Might,
                        0.03,
                        3,
                    ),
                ],
            },
            Spec {
                name: "Restoration",
                icon: "Spell_Nature_HealingWaveGreater",
                talents: &[
                    t(
                        "Improved Healing Wave",
                        "Spell_Nature_MagicImmunity",
                        Regen,
                        0.2,
                        5,
                    ),
                    t("Tidal Focus", "Spell_Frost_ManaRecharge", Haste, 0.01, 5),
                    t("Totemic Focus", "Spell_Nature_MoonGlow", Area, 0.04, 5),
                    t("Purification", "Spell_Frost_WizardMark", MaxHp, 0.04, 5),
                    t(
                        "Nature's Swiftness",
                        "Spell_Nature_RavenForm",
                        Speed,
                        0.04,
                        1,
                    ),
                ],
            },
        ],
    },
    ClassTrees {
        id: 8,
        specs: [
            Spec {
                name: "Arcane",
                icon: "Spell_Holy_MagicalSentry",
                talents: &[
                    t(
                        "Arcane Concentration",
                        "Spell_Shadow_ManaBurn",
                        Haste,
                        0.01,
                        5,
                    ),
                    t(
                        "Improved Arcane Missiles",
                        "Spell_Nature_StarFall",
                        Might,
                        0.03,
                        5,
                    ),
                    t(
                        "Arcane Meditation",
                        "Spell_Shadow_SiphonMana",
                        Regen,
                        0.2,
                        3,
                    ),
                    t("Arcane Mind", "Spell_Shadow_Charm", Growth, 0.03, 5),
                    t("Arcane Instability", "Spell_Shadow_Teleport", Crit, 0.01, 5),
                ],
            },
            Spec {
                name: "Fire",
                icon: "Spell_Fire_FireBolt02",
                talents: &[
                    t("Improved Fireball", "Spell_Fire_FlameBolt", Might, 0.03, 5),
                    t("Ignite", "Spell_Fire_Incinerate", Might, 0.04, 5),
                    t("Flame Throwing", "Spell_Fire_Flare", Area, 0.04, 2),
                    t("Critical Mass", "Spell_Nature_WispHeal", Crit, 0.02, 3),
                    t("Fire Power", "Spell_Fire_Immolation", Might, 0.02, 5),
                    t("Burning Soul", "Spell_Fire_Fire", Armor, 0.5, 2),
                ],
            },
            Spec {
                name: "Frost",
                icon: "Spell_Frost_FrostBolt02",
                talents: &[
                    t("Frost Warding", "Spell_Frost_FrostArmor", Armor, 0.5, 2),
                    t(
                        "Improved Frostbolt",
                        "Spell_Frost_FrostBolt02",
                        Might,
                        0.03,
                        5,
                    ),
                    t(
                        "Elemental Precision",
                        "Spell_Ice_MagicDamage",
                        Area,
                        0.03,
                        3,
                    ),
                    t("Ice Shards", "Spell_Frost_IceShard", Crit, 0.01, 5),
                    t("Piercing Ice", "Spell_Frost_Frostbolt", Might, 0.02, 3),
                    t("Shatter", "Spell_Frost_FrostShock", Crit, 0.02, 5),
                ],
            },
        ],
    },
    ClassTrees {
        id: 9,
        specs: [
            Spec {
                name: "Affliction",
                icon: "Spell_Shadow_DeathCoil",
                talents: &[
                    t(
                        "Improved Corruption",
                        "Spell_Shadow_AbominationExplosion",
                        Might,
                        0.02,
                        5,
                    ),
                    t(
                        "Suppression",
                        "Spell_Shadow_UnsummonBuilding",
                        Armor,
                        0.5,
                        5,
                    ),
                    t(
                        "Improved Life Tap",
                        "Spell_Shadow_BurningSpirit",
                        Regen,
                        0.2,
                        2,
                    ),
                    t("Soul Siphon", "Spell_Shadow_LifeDrain02", Growth, 0.03, 2),
                    t(
                        "Fel Concentration",
                        "Spell_Shadow_FingerOfDeath",
                        Haste,
                        0.01,
                        5,
                    ),
                    t(
                        "Shadow Mastery",
                        "Spell_Shadow_ShadeTrueSight",
                        Might,
                        0.02,
                        5,
                    ),
                ],
            },
            Spec {
                name: "Demonology",
                icon: "Spell_Shadow_Metamorphosis",
                talents: &[
                    t("Improved Healthstone", "INV_Stone_04", MaxHp, 0.04, 2),
                    t(
                        "Demonic Embrace",
                        "Spell_Shadow_Metamorphosis",
                        MaxHp,
                        0.03,
                        5,
                    ),
                    t("Fel Intellect", "Spell_Holy_MagicalSentry", Might, 0.02, 3),
                    t("Fel Stamina", "Spell_Shadow_AntiShadow", MaxHp, 0.03, 5),
                    t(
                        "Master Summoner",
                        "Spell_Shadow_ImpPhaseShift",
                        Haste,
                        0.01,
                        2,
                    ),
                    t("Unholy Power", "Spell_Shadow_ShadowWard", Might, 0.03, 5),
                ],
            },
            Spec {
                name: "Destruction",
                icon: "Spell_Shadow_RainOfFire",
                talents: &[
                    t(
                        "Improved Shadow Bolt",
                        "Spell_Shadow_ShadowBolt",
                        Might,
                        0.04,
                        5,
                    ),
                    t("Cataclysm", "Spell_Fire_WindsofWoe", Area, 0.03, 5),
                    t("Bane", "Spell_Shadow_DeathPact", Haste, 0.01, 5),
                    t("Devastation", "Spell_Fire_FlameShock", Crit, 0.01, 5),
                    t("Improved Immolate", "Spell_Fire_Immolation", Might, 0.02, 5),
                ],
            },
        ],
    },
    ClassTrees {
        id: 11,
        specs: [
            Spec {
                name: "Balance",
                icon: "Spell_Nature_StarFall",
                talents: &[
                    t(
                        "Starlight Wrath",
                        "Spell_Nature_AbolishMagic",
                        Haste,
                        0.01,
                        5,
                    ),
                    t("Nature's Grasp", "Spell_Nature_NaturesWrath", Armor, 0.5, 3),
                    t("Improved Moonfire", "Spell_Nature_StarFall", Might, 0.02, 5),
                    t("Vengeance", "Spell_Nature_Purge", Crit, 0.02, 5),
                    t("Moonglow", "Spell_Nature_Sentinal", Regen, 0.2, 3),
                    t("Moonfury", "Spell_Nature_MoonGlow", Might, 0.03, 5),
                ],
            },
            Spec {
                name: "Feral Combat",
                icon: "Ability_Racial_BearForm",
                talents: &[
                    t("Feral Instinct", "Ability_Ambush", Speed, 0.03, 3),
                    t("Thick Hide", "INV_Misc_Pelt_Bear_03", Armor, 0.5, 5),
                    t("Sharpened Claws", "INV_Misc_MonsterClaw_04", Crit, 0.02, 3),
                    t(
                        "Heart of the Wild",
                        "Spell_Holy_BlessingOfAgility",
                        MaxHp,
                        0.04,
                        5,
                    ),
                    t("Primal Fury", "Ability_Racial_Cannibalize", Might, 0.03, 2),
                ],
            },
            Spec {
                name: "Restoration",
                icon: "Spell_Nature_HealingTouch",
                talents: &[
                    t(
                        "Improved Mark of the Wild",
                        "Spell_Nature_Regeneration",
                        MaxHp,
                        0.02,
                        5,
                    ),
                    t("Furor", "Spell_Holy_BlessingOfStamina", Speed, 0.02, 5),
                    t(
                        "Improved Regrowth",
                        "Spell_Nature_ResistNature",
                        Regen,
                        0.2,
                        5,
                    ),
                    t("Tranquil Spirit", "Spell_Holy_ElunesGrace", Haste, 0.01, 5),
                    t(
                        "Natural Shapeshifter",
                        "Spell_Nature_WispSplode",
                        Magnet,
                        0.08,
                        3,
                    ),
                ],
            },
        ],
    },
];

/// `class`'s three specs, `class` an index into [`data::CLASSES`].
pub(super) fn specs(class: usize) -> Option<&'static [Spec; 3]> {
    let id = data::CLASSES.get(class)?.id;
    TREES.iter().find(|c| c.id == id).map(|c| &c.specs)
}

/// The key a talent's ranks are saved under: the class id and the name, lowercase with
/// underscores.
pub(super) fn key(class: usize, talent: &Talent) -> String {
    let id = data::CLASSES.get(class).map_or(0, |c| c.id);
    let slug: String = talent
        .name
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() {
                c.to_ascii_lowercase()
            } else {
                '_'
            }
        })
        .collect();
    format!("{id}_{slug}")
}

/// Points a character of `level` has earned in all.
pub(super) fn points_total(level: u32) -> u32 {
    level.saturating_sub(FIRST_LEVEL - 1)
}

/// Points a talent at position `idx` needs in its spec before it opens.
pub(super) fn needs(idx: usize) -> u32 {
    TIER_STEP * idx as u32
}

/// The ranks in `talent`, clamped to its maximum so an edited save cannot overshoot.
pub(super) fn rank(ranks: &BTreeMap<String, u8>, class: usize, talent: &Talent) -> u8 {
    ranks
        .get(&key(class, talent))
        .copied()
        .unwrap_or(0)
        .min(talent.max)
}

/// Points put into one spec.
pub(super) fn spec_spent(ranks: &BTreeMap<String, u8>, class: usize, spec: &Spec) -> u32 {
    spec.talents
        .iter()
        .map(|t| u32::from(rank(ranks, class, t)))
        .sum()
}

/// Points put into all of `class`'s specs.
pub(super) fn spent(ranks: &BTreeMap<String, u8>, class: usize) -> u32 {
    specs(class).map_or(0, |s| s.iter().map(|s| spec_spent(ranks, class, s)).sum())
}

/// Points `class` still has to spend at `level`.
pub(super) fn available(ranks: &BTreeMap<String, u8>, class: usize, level: u32) -> u32 {
    points_total(level).saturating_sub(spent(ranks, class))
}

/// Whether the talent at `spec`, `idx` can take another rank: it has room, a point is free and its
/// spec holds enough points for its position.
pub(super) fn can_spend(
    ranks: &BTreeMap<String, u8>,
    class: usize,
    level: u32,
    spec: usize,
    idx: usize,
) -> bool {
    let Some(s) = specs(class).and_then(|s| s.get(spec)) else {
        return false;
    };
    let Some(talent) = s.talents.get(idx) else {
        return false;
    };
    rank(ranks, class, talent) < talent.max
        && available(ranks, class, level) > 0
        && spec_spent(ranks, class, s) >= needs(idx)
}

/// Spend one point on the talent at `spec`, `idx`; whether it went through.
pub(super) fn spend(
    ranks: &mut BTreeMap<String, u8>,
    class: usize,
    level: u32,
    spec: usize,
    idx: usize,
) -> bool {
    if !can_spend(ranks, class, level, spec, idx) {
        return false;
    }
    let Some(talent) = specs(class).and_then(|s| s.get(spec)?.talents.get(idx)) else {
        return false;
    };
    let next = rank(ranks, class, talent) + 1;
    ranks.insert(key(class, talent), next);
    true
}

/// Take back every point of `class`.
pub(super) fn respec(ranks: &mut BTreeMap<String, u8>, class: usize) {
    let id = data::CLASSES.get(class).map_or(0, |c| c.id);
    let prefix = format!("{id}_");
    ranks.retain(|k, _| !k.starts_with(&prefix));
}

/// The bonus `class`'s ranks add to a run.
pub(super) fn mods_for(ranks: &BTreeMap<String, u8>, class: usize) -> StatMods {
    let mut m = StatMods::default();
    for spec in specs(class).into_iter().flatten() {
        for t in spec.talents {
            t.fx.apply(&mut m, t.per_rank * f32::from(rank(ranks, class, t)));
        }
    }
    m
}

// ── The panel ────────────────────────────────────────────────────────────────────────────────────

/// A button of the panel or the pick screen.
#[derive(Component, Clone, Copy, PartialEq, Eq, Debug)]
enum TalentAction {
    Open,
    Close,
    Respec,
}

/// A talent's row, a button of its own.
#[derive(Component, Clone, Copy)]
struct TalentRow {
    spec: usize,
    idx: usize,
}

/// A text the refresh keeps current.
#[derive(Component, Clone, Copy)]
enum Label {
    /// The class, its level and the points left.
    Sub,
    SpecName(usize),
    SpecPoints(usize),
    Name(usize, usize),
    Effect(usize, usize),
    Rank(usize, usize),
}

/// An icon the refresh fills for the selected class.
#[derive(Component, Clone, Copy)]
enum Icon {
    Spec(usize),
    Row(usize, usize),
}

/// The dimmed full-screen layer holding the window, hidden until opened.
#[derive(Component)]
struct TalentPanel;
/// On [`MenuRoot`] once the button and the window are built into it.
#[derive(Component)]
struct TalentsBuilt;

/// Whether the window is open.
#[derive(Resource, Default)]
pub(super) struct TalentsUi {
    pub(super) open: bool,
}

const PANEL_W: f32 = 790.0;
const PANEL_H: f32 = 540.0;
const COL_W: f32 = 236.0;
const COL_GAP: f32 = 12.0;
const COL_X0: f32 = (PANEL_W - (3.0 * COL_W + 2.0 * COL_GAP)) / 2.0;
const HEAD_TOP: f32 = 86.0;
const ROWS_TOP: f32 = 126.0;
const ROW_PITCH: f32 = 54.0;
const ROW_H: f32 = 50.0;
/// The Talents button: centred under the difficulty selector, below its three buttons.
const BUTTON_TOP: f32 = 722.0;
const BUTTON_W: f32 = 136.0;

const FRAME_BORDER: Color = Color::srgb(0.8, 0.8, 0.8);
const FRAME_FILL: Color = Color::srgba(0.09, 0.09, 0.09, 0.95);
const ROW_BG: Color = Color::srgba(0.0, 0.0, 0.0, 0.35);
const ROW_HOT: Color = Color::srgba(0.35, 0.28, 0.1, 0.6);
const ROW_EDGE: Color = Color::srgb(0.25, 0.2, 0.1);
const ROW_EDGE_OPEN: Color = Color::srgb(1.0, 0.78, 0.0);
const MAXED: Color = Color::srgb(0.4, 1.0, 0.4);
const SHORT: Color = Color::srgb(1.0, 0.3, 0.3);
const QUESTION: &str = "Interface\\Icons\\INV_Misc_QuestionMark.blp";

fn icon_path(icon: &str) -> String {
    format!("Interface\\Icons\\{icon}.blp")
}

pub(super) fn plugin(app: &mut App) {
    app.init_resource::<TalentsUi>().add_systems(
        Update,
        (build, show_panel, talent_clicks, refresh)
            .chain()
            // After the keys, which read Enter off [`TalentsUi::open`] first.
            .after(hud::keys)
            .before(GlueVisuals),
    );
}

/// Build the Talents button and the window into the pick screen once it is built.
fn build(
    mut commands: Commands,
    roots: Query<Entity, (With<MenuRoot>, With<MenuBuilt>, Without<TalentsBuilt>)>,
    glue: Res<GlueArt>,
    font: Res<HudFont>,
) {
    let Ok(root) = roots.single() else { return };
    let f = &font.0;
    let px = Val::Px;
    commands
        .entity(root)
        .insert(TalentsBuilt)
        .with_children(|ui| {
            ui.spawn(Node {
                position_type: PositionType::Absolute,
                right: px(5.0 + (260.0 - BUTTON_W) / 2.0),
                top: px(BUTTON_TOP),
                ..default()
            })
            .with_children(|b| {
                glue_button(
                    b,
                    &glue,
                    f,
                    TalentAction::Open,
                    "Talents",
                    BUTTON_W,
                    30.0,
                    GlueBtnKind::Small,
                    1.0,
                );
            });
            panel(ui, &glue, f);
        });
}

/// A text at `(left, top)` in a `w` × `h` box, tagged `label`.
fn label(
    frame: &mut ChildSpawnerCommands,
    f: &Handle<Font>,
    label: Label,
    (left, top, w, h): (f32, f32, f32, f32),
    size: f32,
    color: Color,
) {
    outlined_text(
        frame,
        abs(1.0, left, top, w, h),
        (),
        label,
        GlueText {
            text: "",
            size,
            color,
            wrap: false,
        },
        f,
        1.0,
    );
}

/// The window: title, the class line, three specs of six rows, Respec and Close.
fn panel(ui: &mut ChildSpawnerCommands, glue: &GlueArt, f: &Handle<Font>) {
    let px = Val::Px;
    ui.spawn((
        TalentPanel,
        // Swallows the clicks on the pick screen under it.
        FocusPolicy::Block,
        BackgroundColor(Color::srgba(0.0, 0.0, 0.0, 0.6)),
        Node {
            position_type: PositionType::Absolute,
            width: Val::Percent(100.0),
            height: Val::Percent(100.0),
            justify_content: JustifyContent::Center,
            align_items: AlignItems::Center,
            display: Display::None,
            ..default()
        },
    ))
    .with_children(|dim| {
        dim.spawn(Node {
            width: px(PANEL_W),
            height: px(PANEL_H),
            ..default()
        })
        .with_children(|frame| {
            if let (Some(bg), Some(border)) = (&glue.tooltip_bg, &glue.name_border) {
                frame.spawn((
                    tiled_bg_node(bg.clone(), NAME_EDGE, 1.0, FRAME_FILL),
                    BackgroundColor(Color::srgb(0.03, 0.03, 0.04)),
                    Node {
                        position_type: PositionType::Absolute,
                        left: px(10.0),
                        right: px(5.0),
                        top: px(4.0),
                        bottom: px(9.0),
                        ..default()
                    },
                ));
                backdrop_border(frame, border, NAME_EDGE, FRAME_BORDER);
            } else {
                frame.spawn((BackgroundColor(FRAME_FILL), crate::glue::widgets::overlay()));
            }
            outlined_text(
                frame,
                Node {
                    position_type: PositionType::Absolute,
                    top: px(16.0),
                    width: Val::Percent(100.0),
                    justify_content: JustifyContent::Center,
                    ..default()
                },
                (),
                (),
                GlueText {
                    text: "Talents",
                    size: 22.0,
                    color: GOLD,
                    wrap: false,
                },
                f,
                1.0,
            );
            // Full width, centred by the node; the text itself is left-aligned inside it.
            frame
                .spawn(Node {
                    position_type: PositionType::Absolute,
                    top: px(52.0),
                    width: Val::Percent(100.0),
                    justify_content: JustifyContent::Center,
                    ..default()
                })
                .with_children(|row| {
                    outlined_text(
                        row,
                        Node::default(),
                        (),
                        Label::Sub,
                        GlueText {
                            text: "",
                            size: 14.0,
                            color: Color::WHITE,
                            wrap: false,
                        },
                        f,
                        1.0,
                    );
                });

            for spec in 0..3 {
                let x = COL_X0 + spec as f32 * (COL_W + COL_GAP);
                frame.spawn((
                    Icon::Spec(spec),
                    ImageNode {
                        color: Color::NONE,
                        ..default()
                    },
                    abs(1.0, x, HEAD_TOP, 30.0, 30.0),
                ));
                label(
                    frame,
                    f,
                    Label::SpecName(spec),
                    (x + 38.0, HEAD_TOP, COL_W - 38.0, 17.0),
                    16.0,
                    GOLD,
                );
                label(
                    frame,
                    f,
                    Label::SpecPoints(spec),
                    (x + 38.0, HEAD_TOP + 18.0, COL_W - 38.0, 14.0),
                    11.0,
                    DIM,
                );
                for idx in 0..MAX_ROWS {
                    row(frame, f, x, spec, idx);
                }
            }

            let foot = PANEL_H - 56.0;
            for (left, action, caption) in [
                (34.0, TalentAction::Respec, "Respec"),
                (PANEL_W - 34.0 - 146.0, TalentAction::Close, "Close"),
            ] {
                frame
                    .spawn(Node {
                        position_type: PositionType::Absolute,
                        left: px(left),
                        top: px(foot),
                        ..default()
                    })
                    .with_children(|b| {
                        glue_button(
                            b,
                            glue,
                            f,
                            action,
                            caption,
                            146.0,
                            30.0,
                            GlueBtnKind::Small,
                            1.0,
                        );
                    });
            }
            outlined_text(
                frame,
                Node {
                    position_type: PositionType::Absolute,
                    top: px(foot + 9.0),
                    width: Val::Percent(100.0),
                    justify_content: JustifyContent::Center,
                    ..default()
                },
                (),
                (),
                GlueText {
                    text: "Click a talent to spend a point. Respec is free.",
                    size: 12.0,
                    color: DIM,
                    wrap: false,
                },
                f,
                1.0,
            );
        });
    });
}

/// One talent's row: icon, name, the effect (or what is missing to open it) and the rank.
fn row(frame: &mut ChildSpawnerCommands, f: &Handle<Font>, x: f32, spec: usize, idx: usize) {
    frame
        .spawn((
            TalentRow { spec, idx },
            Button,
            BackgroundColor(ROW_BG),
            BorderColor::all(ROW_EDGE),
            Node {
                position_type: PositionType::Absolute,
                left: Val::Px(x),
                top: Val::Px(ROWS_TOP + idx as f32 * ROW_PITCH),
                width: Val::Px(COL_W),
                height: Val::Px(ROW_H),
                border: UiRect::all(Val::Px(1.0)),
                ..default()
            },
        ))
        .with_children(|r| {
            r.spawn((
                Icon::Row(spec, idx),
                ImageNode {
                    color: Color::NONE,
                    ..default()
                },
                abs(1.0, 6.0, 6.0, 36.0, 36.0),
            ));
            label(
                r,
                f,
                Label::Name(spec, idx),
                (50.0, 6.0, COL_W - 56.0, 15.0),
                12.0,
                Color::WHITE,
            );
            label(
                r,
                f,
                Label::Effect(spec, idx),
                (50.0, 25.0, COL_W - 90.0, 14.0),
                11.0,
                DIM,
            );
            label(
                r,
                f,
                Label::Rank(spec, idx),
                (COL_W - 40.0, 24.0, 34.0, 16.0),
                14.0,
                Color::WHITE,
            );
        });
}

/// Close on Esc or when the pick screen goes; for an automated run, `WOW_SURVIVORS_TALENTS=1`
/// opens the window (`=spend` with every point spent) until the auto run has photographed and
/// closed it; and the display follows [`TalentsUi::open`].
fn show_panel(
    mut ui: ResMut<TalentsUi>,
    phase: Res<State<Phase>>,
    keys: Res<ButtonInput<KeyCode>>,
    selection: Res<Selection>,
    mut profile: ResMut<Profile>,
    mut panel: Query<&mut Node, With<TalentPanel>>,
    mut auto: Local<bool>,
) {
    if ui.open && (*phase.get() != Phase::Menu || keys.just_pressed(KeyCode::Escape)) {
        ui.open = false;
    }
    if profile.ephemeral && *phase.get() == Phase::Menu && panel.iter().next().is_some() {
        if !*auto {
            if let Some(mode) = crate::webenv::var("WOW_SURVIVORS_TALENTS") {
                *auto = true;
                ui.open = true;
                if mode == "spend" {
                    if let Some(class) = selection.class {
                        let level = profile.character(class).level;
                        let mut guard = 0;
                        // Down the columns in turn, so every spec shows some ranks.
                        while guard < 200 && available(&profile.talents, class, level) > 0 {
                            let spec = guard % 3;
                            let idx = (guard / 3) % MAX_ROWS;
                            spend(&mut profile.talents, class, level, spec, idx);
                            guard += 1;
                        }
                    }
                }
            }
        }
    }
    let display = if ui.open {
        Display::Flex
    } else {
        Display::None
    };
    for mut node in &mut panel {
        if node.display != display {
            node.display = display;
        }
    }
}

/// The button's and the rows' clicks: open, close, respec, spend a point.
fn talent_clicks(
    clicks: Res<GlueClicks>,
    actions: Query<(Entity, &TalentAction)>,
    rows: Query<(Entity, &TalentRow)>,
    phase: Res<State<Phase>>,
    selection: Res<Selection>,
    mut profile: ResMut<Profile>,
    mut ui: ResMut<TalentsUi>,
) {
    if *phase.get() != Phase::Menu {
        return;
    }
    if let Some(action) = actions
        .iter()
        .find(|(e, _)| clicks.hit(*e))
        .map(|(_, a)| *a)
    {
        match action {
            TalentAction::Open => ui.open = true,
            TalentAction::Close => ui.open = false,
            TalentAction::Respec => {
                if let Some(class) = selection.class {
                    if spent(&profile.talents, class) > 0 {
                        respec(&mut profile.talents, class);
                        profile.save();
                    }
                }
            }
        }
        return;
    }
    if !ui.open {
        return;
    }
    let Some(class) = selection.class else { return };
    let Some(&TalentRow { spec, idx }) = rows.iter().find(|(e, _)| clicks.hit(*e)).map(|(_, r)| r)
    else {
        return;
    };
    let level = profile.character(class).level;
    if spend(&mut profile.talents, class, level, spec, idx) {
        profile.save();
    }
}

/// Set a glue button's caption, which sits in a text under it.
fn set_caption(
    button: Entity,
    want: &str,
    children: &Query<&Children>,
    captions: &mut Query<&mut Text, With<GlueCaption>>,
) {
    for d in children.iter_descendants(button) {
        if let Ok(mut text) = captions.get_mut(d) {
            if text.0 != want {
                text.0 = want.to_string();
            }
            return;
        }
    }
}

/// The button's caption with the points free; while open, the whole window for the selected class.
#[allow(clippy::too_many_arguments, clippy::type_complexity)]
fn refresh(
    ui: Res<TalentsUi>,
    profile: Res<Profile>,
    selection: Res<Selection>,
    assets: Option<ResMut<benilla_assets::WorldAssets>>,
    mut images: ResMut<Assets<Image>>,
    mut loaded: Local<Option<usize>>,
    mut rows: Query<
        (
            &TalentRow,
            &Interaction,
            &mut Node,
            &mut BackgroundColor,
            &mut BorderColor,
        ),
        Without<Icon>,
    >,
    mut icons: Query<(&Icon, &mut ImageNode)>,
    mut labels: Query<(&Label, &mut Text, &mut TextColor), Without<GlueCaption>>,
    mut buttons: Query<(Entity, &TalentAction, &mut GlueDisabled)>,
    children: Query<&Children>,
    mut captions: Query<&mut Text, With<GlueCaption>>,
) {
    let class = selection.class;
    let level = class.map_or(1, |c| profile.character(c).level);
    let left = class.map_or(0, |c| available(&profile.talents, c, level));
    for (button, action, mut disabled) in &mut buttons {
        match *action {
            TalentAction::Open => {
                let want = if left > 0 {
                    format!("Talents ({left})")
                } else {
                    "Talents".to_string()
                };
                set_caption(button, &want, &children, &mut captions);
            }
            TalentAction::Respec => {
                let off = class.is_none_or(|c| spent(&profile.talents, c) == 0);
                if disabled.0 != off {
                    disabled.0 = off;
                }
            }
            TalentAction::Close => {}
        }
    }
    if !ui.open {
        return;
    }
    let (Some(class), Some(def)) = (class, selection.class_def()) else {
        return;
    };
    let trees = specs(class);
    let ranks = &profile.talents;

    if *loaded != Some(class) {
        if let Some(mut assets) = assets {
            for (icon, mut image) in &mut icons {
                let name = match *icon {
                    Icon::Spec(s) => trees.and_then(|t| t.get(s)).map(|s| s.icon),
                    Icon::Row(s, i) => trees
                        .and_then(|t| t.get(s))
                        .and_then(|s| s.talents.get(i))
                        .map(|t| t.icon),
                };
                let Some(name) = name else { continue };
                let handle = assets
                    .sprite_texture(&icon_path(name), &mut images)
                    .or_else(|| assets.sprite_texture(QUESTION, &mut images));
                if let Some(handle) = handle {
                    image.image = handle;
                }
            }
            *loaded = Some(class);
        }
    }

    for (row, interaction, mut node, mut bg, mut edge) in &mut rows {
        let talent = trees
            .and_then(|t| t.get(row.spec))
            .and_then(|s| s.talents.get(row.idx));
        let display = if talent.is_some() {
            Display::Flex
        } else {
            Display::None
        };
        if node.display != display {
            node.display = display;
        }
        let open = can_spend(ranks, class, level, row.spec, row.idx);
        let want_bg = if open && *interaction != Interaction::None {
            ROW_HOT
        } else {
            ROW_BG
        };
        if bg.0 != want_bg {
            bg.0 = want_bg;
        }
        let want_edge = BorderColor::all(if open { ROW_EDGE_OPEN } else { ROW_EDGE });
        if *edge != want_edge {
            *edge = want_edge;
        }
    }
    for (icon, mut image) in &mut icons {
        if loaded.is_none() {
            break;
        }
        let (spec, idx) = match *icon {
            Icon::Spec(s) => (s, None),
            Icon::Row(s, i) => (s, Some(i)),
        };
        let Some(s) = trees.and_then(|t| t.get(spec)) else {
            continue;
        };
        // A talent with no rank and none to open reads grey, one with ranks full colour.
        let want = match idx.and_then(|i| s.talents.get(i)) {
            Some(t) if rank(ranks, class, t) > 0 => Color::WHITE,
            Some(_) if can_spend(ranks, class, level, spec, idx.unwrap_or(0)) => Color::WHITE,
            Some(_) => Color::srgb(0.4, 0.4, 0.4),
            None => Color::WHITE,
        };
        if image.color != want {
            image.color = want;
        }
    }
    for (label, mut text, mut color) in &mut labels {
        let (want, tint) = match *label {
            Label::Sub => {
                let total = points_total(level);
                let line = if total == 0 {
                    format!(
                        "{}, level {level}. Talent points start at level {FIRST_LEVEL}.",
                        def.name
                    )
                } else {
                    format!(
                        "{}, level {level}.   Points to spend: {left} of {total}",
                        def.name
                    )
                };
                (line, if left > 0 { GOLD } else { Color::WHITE })
            }
            Label::SpecName(s) => (
                trees
                    .and_then(|t| t.get(s))
                    .map_or(String::new(), |s| s.name.to_string()),
                GOLD,
            ),
            Label::SpecPoints(s) => (
                trees.and_then(|t| t.get(s)).map_or(String::new(), |sp| {
                    format!("{} points", spec_spent(ranks, class, sp))
                }),
                DIM,
            ),
            Label::Name(s, i) | Label::Effect(s, i) | Label::Rank(s, i) => {
                let Some(sp) = trees.and_then(|t| t.get(s)) else {
                    continue;
                };
                let Some(t) = sp.talents.get(i) else { continue };
                let r = rank(ranks, class, t);
                match *label {
                    Label::Name(..) => (
                        t.name.to_string(),
                        if r >= t.max { MAXED } else { Color::WHITE },
                    ),
                    Label::Effect(..) => {
                        let have = spec_spent(ranks, class, sp);
                        if have < needs(i) {
                            (format!("Needs {} points in {}", needs(i), sp.name), SHORT)
                        } else {
                            (format!("{} a rank", t.fx.describe(t.per_rank)), DIM)
                        }
                    }
                    _ => (
                        format!("{r}/{}", t.max),
                        if r >= t.max {
                            MAXED
                        } else if r > 0 {
                            GOLD
                        } else {
                            Color::WHITE
                        },
                    ),
                }
            }
        };
        if text.0 != want {
            text.0 = want;
        }
        if color.0 != tint {
            color.0 = tint;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The class index of Mage in [`data::CLASSES`].
    fn mage() -> usize {
        data::CLASSES.iter().position(|c| c.id == 8).unwrap()
    }

    #[test]
    fn every_class_has_three_specs_of_four_to_six_talents() {
        for (i, c) in data::CLASSES.iter().enumerate() {
            let s = specs(i).unwrap_or_else(|| panic!("{} has no trees", c.name));
            for spec in s {
                assert!(
                    (4..=MAX_ROWS).contains(&spec.talents.len()),
                    "{} {}",
                    c.name,
                    spec.name
                );
                for t in spec.talents {
                    assert!((1..=5).contains(&t.max), "{}", t.name);
                    assert!(t.per_rank > 0.0, "{}", t.name);
                }
            }
        }
    }

    #[test]
    fn talent_keys_are_unique_within_a_class() {
        for i in 0..data::CLASSES.len() {
            let mut keys: Vec<String> = specs(i)
                .unwrap()
                .iter()
                .flat_map(|s| s.talents.iter().map(|t| key(i, t)))
                .collect();
            let n = keys.len();
            keys.sort();
            keys.dedup();
            assert_eq!(keys.len(), n, "class {i}");
        }
    }

    #[test]
    fn a_point_a_level_from_ten() {
        assert_eq!(points_total(1), 0);
        assert_eq!(points_total(9), 0);
        assert_eq!(points_total(10), 1);
        assert_eq!(points_total(60), 51);
    }

    #[test]
    fn points_gate_spending_and_tiers_gate_positions() {
        let c = mage();
        let mut ranks = BTreeMap::new();
        // No points at level 9.
        assert!(!spend(&mut ranks, c, 9, 1, 0));
        // Level 14: five points. The third position of Fire needs four points in the spec.
        assert!(spend(&mut ranks, c, 14, 1, 0));
        assert!(
            !can_spend(&ranks, c, 14, 1, 2),
            "needs 4 points in the spec"
        );
        assert!(spend(&mut ranks, c, 14, 1, 0));
        assert!(spend(&mut ranks, c, 14, 1, 1));
        assert!(spend(&mut ranks, c, 14, 1, 1));
        assert_eq!(available(&ranks, c, 14), 1);
        assert!(spend(&mut ranks, c, 14, 1, 2));
        assert_eq!(spent(&ranks, c), 5);
        assert!(!spend(&mut ranks, c, 14, 1, 0), "out of points");
    }

    #[test]
    fn a_talent_stops_at_its_maximum() {
        let c = mage();
        let mut ranks = BTreeMap::new();
        let top = &specs(c).unwrap()[1].talents[0];
        for _ in 0..top.max {
            assert!(spend(&mut ranks, c, 60, 1, 0));
        }
        assert!(!spend(&mut ranks, c, 60, 1, 0));
        assert_eq!(rank(&ranks, c, top), top.max);
    }

    #[test]
    fn respec_hands_back_one_class_only() {
        let (c, other) = (mage(), 0);
        let mut ranks = BTreeMap::new();
        assert!(spend(&mut ranks, c, 60, 1, 0));
        assert!(spend(&mut ranks, other, 60, 1, 0));
        respec(&mut ranks, c);
        assert_eq!(spent(&ranks, c), 0);
        assert_eq!(spent(&ranks, other), 1);
        assert_eq!(available(&ranks, c, 10), 1);
    }

    #[test]
    fn ranks_fold_into_the_mods() {
        let c = mage();
        let mut ranks = BTreeMap::new();
        assert_eq!(mods_for(&ranks, c), StatMods::default());
        // Improved Fireball: 3% damage a rank.
        for _ in 0..2 {
            assert!(spend(&mut ranks, c, 60, 1, 0));
        }
        let m = mods_for(&ranks, c);
        assert!((m.might - 0.06).abs() < 1e-6);
        assert_eq!(m.crit, 0.0);
    }

    #[test]
    fn an_edited_save_cannot_overshoot_a_maximum() {
        let c = mage();
        let top = &specs(c).unwrap()[1].talents[0];
        let mut ranks = BTreeMap::new();
        ranks.insert(key(c, top), 200);
        assert_eq!(rank(&ranks, c, top), top.max);
    }
}
