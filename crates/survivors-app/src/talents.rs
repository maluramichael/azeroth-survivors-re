//! Talent trees: three specs per class, points from the character's level, free respec at the
//! pick screen. The run's talent bonus lands in [`super::Run::talents`].
//!
//! A character earns one point per level from [`FIRST_LEVEL`] on, 51 at the cap, as in 1.12. A
//! spec is a short column of talents; the talent at position `i` opens once the spec holds
//! [`needs`]`(i)` points, so the column fills from the top, as a WoW tree's tiers do. The ranks are
//! kept in [`Profile::talents`] under [`key`] (class id and talent name) and folded into a
//! [`StatMods`] by [`mods_for`]. Renaming a talent drops its saved ranks; never rename one.
//!
//! The panel is a kit window ([`super::kit`]) on the pick screen, opened through [`TalentsUi`]:
//! three trees over their 1.12 background art, each talent an icon button with a rank badge.
//! Hovering a talent reads it at the bottom, clicking spends a point; Respec hands every point of
//! the selected class back for free.

use std::collections::BTreeMap;

use bevy::prelude::*;
use bevy::ui::FocusPolicy;

use super::hud::{self, MenuRoot};
use super::kit::{self, KitButton, KitFonts, KitStyle};
use super::menu::MenuBuilt;
use super::mods::StatMods;
use super::profile::Profile;
use super::{data, Phase, Selection};
use benilla_app::survivors_host::glue::GlueVisuals;

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

/// A button of the window.
#[derive(Component, Clone, Copy, PartialEq, Eq, Debug)]
enum TalentAction {
    Close,
    Respec,
}

/// A talent's icon button.
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
    /// The "x/y" badge on a talent's icon.
    Rank(usize, usize),
    /// The hovered talent's name and rank, or the idle hint.
    TipName,
    /// Its effect per rank.
    TipEffect,
    /// Whether it can be raised.
    TipHint,
}

/// An icon the refresh fills for the selected class.
#[derive(Component, Clone, Copy)]
enum Icon {
    Spec(usize),
    Row(usize, usize),
}

/// One quarter of a tree's background art: `quad` 0 top left, 1 top right, 2 bottom left,
/// 3 bottom right.
#[derive(Component, Clone, Copy)]
struct Backdrop {
    spec: usize,
    quad: usize,
}

/// A line of the tree between talent `idx - 1` and `idx` of `spec`.
#[derive(Component, Clone, Copy)]
struct Connector {
    spec: usize,
    idx: usize,
}

/// The dimmed full-screen layer holding the window, hidden until opened.
#[derive(Component)]
struct TalentPanel;
/// On [`MenuRoot`] once the window is built into it.
#[derive(Component)]
struct TalentsBuilt;

/// Whether the window is open.
#[derive(Resource, Default)]
pub(super) struct TalentsUi {
    pub(super) open: bool,
}

const PANEL_W: f32 = 980.0;
const PANEL_H: f32 = 640.0;
/// A tree's body: the background art at its own size, 320 x 384, clipped to this.
const TREE_W: f32 = 300.0;
const TREE_H: f32 = 384.0;
/// The art is painted over the talent frame's 332-unit-tall scroll area, not the quarters' full
/// 384: stretched by `TREE_H / 332`, so the painted part fills the tree body.
const ART_Y: f32 = TREE_H / 332.0;
/// The art's four quarters: top left 256 x 256, top right 64 x 256, bottom left 256 x 128,
/// bottom right 64 x 128, stretched by [`ART_Y`]; `(left, top, width, height)`.
const QUARTERS: [(f32, f32, f32, f32); 4] = [
    (0.0, 0.0, 256.0, 256.0 * ART_Y),
    (256.0, 0.0, 64.0, 256.0 * ART_Y),
    (0.0, 256.0 * ART_Y, 256.0, 128.0 * ART_Y),
    (256.0, 256.0 * ART_Y, 64.0, 128.0 * ART_Y),
];
const ICON: f32 = 40.0;
const TIER_TOP: f32 = 20.0;
const TIER_PITCH: f32 = 60.0;
const COL_X0: f32 = 36.0;
const COL_PITCH: f32 = 64.0;
/// The column (of four) each position of a spec sits in, so a tree zigzags and some neighbours
/// stack. One row of this per spec.
const COLUMNS: [[usize; MAX_ROWS]; 3] = [[1, 2, 2, 1, 1, 2], [2, 1, 1, 2, 2, 1], [1, 1, 2, 2, 1, 1]];

const ART_TINT: Color = Color::srgb(0.75, 0.75, 0.75);
const EDGE_GREY: Color = Color::srgb(0.55, 0.55, 0.55);
const EDGE_DARK: Color = Color::srgb(0.22, 0.2, 0.18);
const LINE_LIT: Color = Color::srgb(0.75, 0.6, 0.28);
const LINE_DIM: Color = Color::srgba(0.45, 0.4, 0.3, 0.5);
const BADGE_FILL: Color = Color::srgba(0.0, 0.0, 0.0, 0.85);
const QUESTION: &str = "Interface\\Icons\\INV_Misc_QuestionMark.blp";
const IDLE_TIP: &str = "Hover a talent to read it.";
const IDLE_HINT: &str = "Click spends a point. Respec hands every point back for free.";

fn icon_path(icon: &str) -> String {
    format!("Interface\\Icons\\{icon}.blp")
}

/// `Interface\TalentFrame\<this>-TopLeft.blp` and its three siblings: the tree's background art,
/// by class id ([`data::ClassDef::id`]) and spec position, as `TalentTab.dbc` names it.
fn backdrop_name(class_id: u8, spec: usize) -> Option<&'static str> {
    let names: [&str; 3] = match class_id {
        1 => ["WarriorArms", "WarriorFury", "WarriorProtection"],
        2 => ["PaladinHoly", "PaladinProtection", "PaladinCombat"],
        3 => ["HunterBeastMastery", "HunterMarksmanship", "HunterSurvival"],
        4 => ["RogueAssassination", "RogueCombat", "RogueSubtlety"],
        5 => ["PriestDiscipline", "PriestHoly", "PriestShadow"],
        7 => ["ShamanElementalCombat", "ShamanEnhancement", "ShamanRestoration"],
        8 => ["MageArcane", "MageFire", "MageFrost"],
        9 => ["WarlockCurses", "WarlockSummoning", "WarlockDestruction"],
        11 => ["DruidBalance", "DruidFeralCombat", "DruidRestoration"],
        _ => return None,
    };
    names.get(spec).copied()
}

const QUARTER_NAMES: [&str; 4] = ["TopLeft", "TopRight", "BottomLeft", "BottomRight"];

/// Where talent `idx` of `spec` sits in the tree body: `(left, top)`.
fn slot(spec: usize, idx: usize) -> (f32, f32) {
    let col = COLUMNS[spec % 3][idx % MAX_ROWS];
    (
        COL_X0 + col as f32 * COL_PITCH,
        TIER_TOP + idx as f32 * TIER_PITCH,
    )
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

/// Build the window into the pick screen once it is built.
fn build(
    mut commands: Commands,
    roots: Query<Entity, (With<MenuRoot>, With<MenuBuilt>, Without<TalentsBuilt>)>,
    fonts: Res<KitFonts>,
) {
    let Ok(root) = roots.single() else { return };
    commands
        .entity(root)
        .insert(TalentsBuilt)
        .with_children(|ui| {
            panel(ui, &fonts);
        });
}

/// The window: title, the class line, three trees, the hovered talent's text, Respec and Close.
fn panel(ui: &mut ChildSpawnerCommands, fonts: &KitFonts) {
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
        dim.spawn(kit::panel(Node {
            width: px(PANEL_W),
            height: px(PANEL_H),
            flex_direction: FlexDirection::Column,
            padding: UiRect::axes(px(24.0), px(12.0)),
            row_gap: px(6.0),
            ..default()
        }))
        .with_children(|win| {
            kit::heading(win, fonts, "Talents", 26.0);
            win.spawn(Node {
                width: Val::Percent(100.0),
                justify_content: JustifyContent::Center,
                ..default()
            })
            .with_children(|row| {
                row.spawn((kit::body(fonts, "", 14.0, kit::CREAM), Label::Sub));
            });

            win.spawn(Node {
                width: Val::Percent(100.0),
                justify_content: JustifyContent::Center,
                column_gap: px(25.0),
                margin: UiRect::top(px(4.0)),
                ..default()
            })
            .with_children(|cols| {
                for spec in 0..3 {
                    tree(cols, fonts, spec);
                }
            });

            // The hovered talent, or the idle hint.
            win.spawn(kit::inset(Node {
                width: Val::Percent(100.0),
                height: px(56.0),
                flex_direction: FlexDirection::Column,
                justify_content: JustifyContent::Center,
                padding: UiRect::axes(px(12.0), px(4.0)),
                row_gap: px(2.0),
                ..default()
            }))
            .with_children(|tip| {
                tip.spawn((
                    kit::body(fonts, IDLE_TIP, 15.0, kit::MUTED),
                    Label::TipName,
                ));
                tip.spawn((
                    kit::body(fonts, IDLE_HINT, 12.0, kit::MUTED),
                    Label::TipEffect,
                ));
                tip.spawn((kit::body(fonts, "", 12.0, kit::MUTED), Label::TipHint));
            });

            win.spawn(Node {
                width: Val::Percent(100.0),
                justify_content: JustifyContent::SpaceBetween,
                ..default()
            })
            .with_children(|foot| {
                for (action, caption) in [
                    (TalentAction::Respec, "Respec"),
                    (TalentAction::Close, "Close"),
                ] {
                    kit::button(
                        foot,
                        fonts,
                        KitStyle::Normal,
                        caption,
                        None,
                        Node {
                            width: px(150.0),
                            height: px(30.0),
                            ..default()
                        },
                        action,
                    );
                }
            });
        });
    });
}

/// One tree: its header line (icon, name, points) over the body with the art and the talents.
fn tree(cols: &mut ChildSpawnerCommands, fonts: &KitFonts, spec: usize) {
    let px = Val::Px;
    cols.spawn(Node {
        width: px(TREE_W),
        flex_direction: FlexDirection::Column,
        row_gap: px(4.0),
        ..default()
    })
    .with_children(|col| {
        col.spawn(Node {
            width: Val::Percent(100.0),
            height: px(30.0),
            align_items: AlignItems::Center,
            justify_content: JustifyContent::SpaceBetween,
            ..default()
        })
        .with_children(|head| {
            head.spawn(Node {
                align_items: AlignItems::Center,
                column_gap: px(8.0),
                ..default()
            })
            .with_children(|left| {
                left.spawn((
                    Icon::Spec(spec),
                    ImageNode {
                        color: Color::NONE,
                        ..default()
                    },
                    Node {
                        width: px(26.0),
                        height: px(26.0),
                        ..default()
                    },
                ));
                left.spawn((
                    kit::text(&fonts.title, "", 20.0, kit::TITLE),
                    Label::SpecName(spec),
                ));
            });
            head.spawn((
                kit::body(fonts, "", 12.0, kit::MUTED),
                Label::SpecPoints(spec),
            ));
        });

        col.spawn((
            BackgroundColor(Color::srgb(0.03, 0.025, 0.02)),
            BorderColor::all(kit::INSET_EDGE),
            Node {
                width: px(TREE_W),
                height: px(TREE_H),
                border: UiRect::all(px(1.0)),
                overflow: Overflow::clip(),
                ..default()
            },
        ))
        .with_children(|body| {
            for (quad, (left, top, w, h)) in QUARTERS.into_iter().enumerate() {
                body.spawn((
                    Backdrop { spec, quad },
                    ImageNode {
                        color: Color::NONE,
                        ..default()
                    },
                    Node {
                        position_type: PositionType::Absolute,
                        left: px(left),
                        top: px(top),
                        width: px(w),
                        height: px(h),
                        ..default()
                    },
                ));
            }
            for idx in 1..MAX_ROWS {
                connectors(body, spec, idx);
            }
            for idx in 0..MAX_ROWS {
                row(body, fonts, spec, idx);
            }
        });
    });
}

/// The lines from talent `idx - 1` down to `idx`: one straight line in the same column, else a
/// stub, a bar across and a stub.
fn connectors(body: &mut ChildSpawnerCommands, spec: usize, idx: usize) {
    let px = Val::Px;
    let (px0, py0) = slot(spec, idx - 1);
    let (px1, _) = slot(spec, idx);
    let (cx0, cx1) = (px0 + ICON / 2.0, px1 + ICON / 2.0);
    let y0 = py0 + ICON;
    let gap = TIER_PITCH - ICON;
    let line = |left: f32, top: f32, w: f32, h: f32| {
        (
            Connector { spec, idx },
            BackgroundColor(LINE_DIM),
            Node {
                position_type: PositionType::Absolute,
                left: px(left),
                top: px(top),
                width: px(w),
                height: px(h),
                ..default()
            },
        )
    };
    if (cx0 - cx1).abs() < 1.0 {
        body.spawn(line(cx0 - 1.0, y0, 2.0, gap));
    } else {
        let half = gap / 2.0;
        body.spawn(line(cx0 - 1.0, y0, 2.0, half));
        body.spawn(line(cx0.min(cx1) - 1.0, y0 + half - 1.0, (cx0 - cx1).abs() + 2.0, 2.0));
        body.spawn(line(cx1 - 1.0, y0 + half, 2.0, half));
    }
}

/// One talent: an icon button in the tree with its rank badge at the bottom right corner.
fn row(body: &mut ChildSpawnerCommands, fonts: &KitFonts, spec: usize, idx: usize) {
    let px = Val::Px;
    let (left, top) = slot(spec, idx);
    body.spawn((
        TalentRow { spec, idx },
        Button,
        BackgroundColor(Color::BLACK),
        BorderColor::all(EDGE_DARK),
        Node {
            position_type: PositionType::Absolute,
            left: px(left),
            top: px(top),
            width: px(ICON),
            height: px(ICON),
            border: UiRect::all(px(2.0)),
            border_radius: BorderRadius::all(px(3.0)),
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
            Node {
                width: Val::Percent(100.0),
                height: Val::Percent(100.0),
                ..default()
            },
        ));
        r.spawn((
            BackgroundColor(BADGE_FILL),
            Node {
                position_type: PositionType::Absolute,
                right: px(-8.0),
                bottom: px(-8.0),
                padding: UiRect::axes(px(3.0), px(0.0)),
                border_radius: BorderRadius::all(px(3.0)),
                ..default()
            },
        ))
        .with_children(|badge| {
            badge.spawn((
                kit::body(fonts, "", 11.0, kit::MUTED),
                Label::Rank(spec, idx),
            ));
        });
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
    if profile.ephemeral
        && *phase.get() == Phase::Menu
        && panel.iter().next().is_some()
        && !*auto
    {
        if let Some(mode) = benilla_app::survivors_host::webenv::var("WOW_SURVIVORS_TALENTS") {
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

/// The window's buttons and the talents' clicks: close, respec, spend a point.
fn talent_clicks(
    actions: Query<(&Interaction, &TalentAction, &KitButton), Changed<Interaction>>,
    rows: Query<(&Interaction, &TalentRow), Changed<Interaction>>,
    phase: Res<State<Phase>>,
    selection: Res<Selection>,
    mut profile: ResMut<Profile>,
    mut ui: ResMut<TalentsUi>,
) {
    if *phase.get() != Phase::Menu || !ui.open {
        return;
    }
    for (interaction, action, button) in &actions {
        if *interaction != Interaction::Pressed || button.disabled {
            continue;
        }
        match action {
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
    let Some(class) = selection.class else { return };
    for (interaction, &TalentRow { spec, idx }) in &rows {
        if *interaction != Interaction::Pressed {
            continue;
        }
        let level = profile.character(class).level;
        if spend(&mut profile.talents, class, level, spec, idx) {
            profile.save();
        }
        return;
    }
}

/// While open, the whole window for the selected class.
fn refresh(
    ui: Res<TalentsUi>,
    profile: Res<Profile>,
    selection: Res<Selection>,
    assets: Option<ResMut<benilla_assets::WorldAssets>>,
    mut images: ResMut<Assets<Image>>,
    mut loaded: Local<Option<usize>>,
    mut rows: Query<
        (&TalentRow, &Interaction, &mut Node, &mut BorderColor),
        (Without<Icon>, Without<Connector>),
    >,
    mut lines: Query<(&Connector, &mut Node, &mut BackgroundColor), Without<TalentRow>>,
    mut icons: Query<(&Icon, &mut ImageNode), Without<Backdrop>>,
    mut backs: Query<(&Backdrop, &mut ImageNode), Without<Icon>>,
    mut labels: Query<(&Label, &mut Text, &mut TextColor)>,
    mut buttons: Query<(&TalentAction, &mut KitButton)>,
) {
    let class = selection.class;
    for (action, mut button) in &mut buttons {
        if *action == TalentAction::Respec {
            let off = class.is_none_or(|c| spent(&profile.talents, c) == 0);
            if button.disabled != off {
                button.disabled = off;
            }
        }
    }
    if !ui.open {
        return;
    }
    let level = class.map_or(1, |c| profile.character(c).level);
    let (Some(class), Some(def)) = (class, selection.class_def()) else {
        return;
    };
    let left = available(&profile.talents, class, level);
    let trees = specs(class);
    let ranks = &profile.talents;
    let talent_at = |spec: usize, idx: usize| {
        trees
            .and_then(|t| t.get(spec))
            .and_then(|s| s.talents.get(idx))
    };

    if *loaded != Some(class) {
        if let Some(mut assets) = assets {
            for (icon, mut image) in &mut icons {
                let name = match *icon {
                    Icon::Spec(s) => trees.and_then(|t| t.get(s)).map(|s| s.icon),
                    Icon::Row(s, i) => talent_at(s, i).map(|t| t.icon),
                };
                let Some(name) = name else { continue };
                let handle = assets
                    .sprite_texture(&icon_path(name), &mut images)
                    .or_else(|| assets.sprite_texture(QUESTION, &mut images));
                if let Some(handle) = handle {
                    image.image = handle;
                }
            }
            let class_id = def.id;
            for (back, mut image) in &mut backs {
                let handle = backdrop_name(class_id, back.spec).and_then(|name| {
                    let path = format!(
                        "Interface\\TalentFrame\\{name}-{}.blp",
                        QUARTER_NAMES[back.quad]
                    );
                    assets.sprite_texture(&path, &mut images)
                });
                // A missing file leaves the quarter invisible rather than a white block.
                let tint = if handle.is_some() { ART_TINT } else { Color::NONE };
                if let Some(handle) = handle {
                    image.image = handle;
                }
                image.color = tint;
            }
            *loaded = Some(class);
        }
    }

    let mut hover: Option<(usize, usize)> = None;
    for (row, interaction, mut node, mut edge) in &mut rows {
        let t = talent_at(row.spec, row.idx);
        let display = if t.is_some() {
            Display::Flex
        } else {
            Display::None
        };
        if node.display != display {
            node.display = display;
        }
        let Some(t) = t else { continue };
        let r = rank(ranks, class, t);
        let can = can_spend(ranks, class, level, row.spec, row.idx);
        let hot = *interaction != Interaction::None;
        if hot {
            hover = Some((row.spec, row.idx));
        }
        let unlocked = trees
            .and_then(|t| t.get(row.spec))
            .is_some_and(|s| spec_spent(ranks, class, s) >= needs(row.idx));
        let want = if r >= t.max {
            kit::TITLE
        } else if r > 0 {
            kit::BONUS
        } else if hot && can {
            kit::CREAM
        } else if unlocked {
            EDGE_GREY
        } else {
            EDGE_DARK
        };
        let want = BorderColor::all(want);
        if *edge != want {
            *edge = want;
        }
    }
    for (line, mut node, mut bg) in &mut lines {
        let display = if talent_at(line.spec, line.idx).is_some() {
            Display::Flex
        } else {
            Display::None
        };
        if node.display != display {
            node.display = display;
        }
        let lit = trees
            .and_then(|t| t.get(line.spec))
            .is_some_and(|s| spec_spent(ranks, class, s) >= needs(line.idx));
        let want = if lit { LINE_LIT } else { LINE_DIM };
        if bg.0 != want {
            bg.0 = want;
        }
    }
    // Which icons exist for this class: a spec with fewer than six talents leaves the rest empty.
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
        // A reachable talent or one with ranks reads in full colour, one with points to spend
        // but not yet open duller, one out of reach dark and desaturated.
        let want = match idx.and_then(|i| s.talents.get(i).map(|t| (i, t))) {
            Some((_, t)) if rank(ranks, class, t) > 0 => Color::WHITE,
            Some((i, _)) if can_spend(ranks, class, level, spec, i) => Color::WHITE,
            Some((i, _)) if spec_spent(ranks, class, s) >= needs(i) => Color::srgb(0.6, 0.6, 0.6),
            Some(_) => Color::srgb(0.35, 0.35, 0.35),
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
                        "{}, level {level}. Points to spend: {left} of {total}",
                        def.name
                    )
                };
                (line, if left > 0 { kit::TITLE } else { kit::CREAM })
            }
            Label::SpecName(s) => (
                trees
                    .and_then(|t| t.get(s))
                    .map_or(String::new(), |s| s.name.to_string()),
                kit::TITLE,
            ),
            Label::SpecPoints(s) => (
                trees.and_then(|t| t.get(s)).map_or(String::new(), |sp| {
                    format!("{} points", spec_spent(ranks, class, sp))
                }),
                kit::MUTED,
            ),
            Label::Rank(s, i) => match talent_at(s, i) {
                // Not a talent of this spec: the badge stays empty.
                None => (String::new(), kit::MUTED),
                Some(t) => {
                    let r = rank(ranks, class, t);
                    (
                        format!("{r}/{}", t.max),
                        if r >= t.max {
                            kit::TITLE
                        } else if r > 0 {
                            kit::BONUS
                        } else {
                            kit::MUTED
                        },
                    )
                }
            },
            Label::TipName | Label::TipEffect | Label::TipHint => {
                let shown = hover.and_then(|(s, i)| Some((i, talent_at(s, i)?, trees?.get(s)?)));
                match shown {
                    None => match *label {
                        Label::TipName => (IDLE_TIP.to_string(), kit::MUTED),
                        Label::TipEffect => (IDLE_HINT.to_string(), kit::MUTED),
                        _ => (String::new(), kit::MUTED),
                    },
                    Some((i, t, sp)) => {
                        let r = rank(ranks, class, t);
                        match *label {
                            Label::TipName => (
                                format!("{}   Rank {r}/{}", t.name, t.max),
                                if r >= t.max { kit::TITLE } else { kit::CREAM },
                            ),
                            Label::TipEffect => (
                                if r == 0 {
                                    format!("{} per rank", t.fx.describe(t.per_rank))
                                } else {
                                    format!(
                                        "{} per rank, {} now",
                                        t.fx.describe(t.per_rank),
                                        t.fx.describe(t.per_rank * f32::from(r))
                                    )
                                },
                                kit::BONUS,
                            ),
                            _ => {
                                if spec_spent(ranks, class, sp) < needs(i) {
                                    (
                                        format!("Needs {} points in {}", needs(i), sp.name),
                                        kit::WARN,
                                    )
                                } else if r >= t.max {
                                    ("Maxed".to_string(), kit::TITLE)
                                } else if left == 0 {
                                    ("No points left to spend".to_string(), kit::MUTED)
                                } else {
                                    ("Click to spend a point".to_string(), kit::CREAM)
                                }
                            }
                        }
                    }
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
