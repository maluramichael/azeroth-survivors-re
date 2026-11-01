//! A kit per boss: its abilities, its arrival line and its signature loot, after the plan's
//! table "Boss-Kits" (`docs/superpowers/plans/2026-11-01-bosse-waechter-drops.md`), and the adds
//! the kits call. A boss without a kit keeps the shared circle and volley
//! ([`super::super::threats`]).
//!
//! Spell ids are the ones the plan checked against `Spell.dbc` (Cleave 15284, Flame Breath 16390,
//! Charge 22120, War Stomp 45, Frenzy 19451, Wrath of Ragnaros 20566, Lava Bomb 20474, Fire Nova
//! 11969, Shadow Bolt Volley 15245, Frostbolt Volley 22643, Poison Bolt Volley 24099, Thrash
//! 21919) and the creature Shadow Bolt 9613 the shared turns already cast. A spell here only
//! plays its cast; the blow is the ability's own, sized by the boss's damage. The sound kits are
//! the ones checked in `SoundEntries.dbc`; the yell texts are from memory (assumptions).
//!
//! The timings keep a boss about as busy as the shared turns do (one blow every seven to ten
//! seconds): two or three rows, each every ten to twenty seconds.

use super::super::data::School;
use super::super::items::{Quality, Slot, Stat, Tpl};
use super::shapes::Shape;
use super::{Aim, BossAbility as A, Does, Kit, When};

const CLEAVE: u32 = 15284;
const FLAME_BREATH: u32 = 16390;
const CHARGE: u32 = 22120;
const WAR_STOMP: u32 = 45;
const FRENZY: u32 = 19451;
const WRATH_OF_RAGNAROS: u32 = 20566;
const LAVA_BOMB: u32 = 20474;
const FIRE_NOVA: u32 = 11969;
const SHADOW_VOLLEY: u32 = 15245;
const FROST_VOLLEY: u32 = 22643;
const POISON_VOLLEY: u32 = 24099;
const THRASH: u32 = 21919;
/// The creature Shadow Bolt the shared turns cast.
const SHADOW_BOLT: u32 = 9613;

// --- Rows ---

const fn every(first: f32, every: f32) -> When {
    When::Every { first, every }
}

/// A shape that strikes once, `damage` times the boss's own, after `windup` seconds.
const fn ground(shape: Shape, windup: f32, damage: f32) -> Does {
    Does::Ground {
        shape,
        windup,
        damage,
        pool: None,
    }
}

/// As [`ground`], and the blow leaves a pool burning for `secs` that does `share` of the boss's
/// damage every second.
const fn burning(shape: Shape, windup: f32, damage: f32, share: f32, secs: f32) -> Does {
    Does::Ground {
        shape,
        windup,
        damage,
        pool: Some((share, secs)),
    }
}

/// A ring of missiles round the boss.
const fn ring(count: u8, school: School) -> Does {
    Does::Volley {
        count,
        school,
        spread: None,
    }
}

/// A fan of missiles at the hero, `spread` radians across.
const fn fan(count: u8, school: School, spread: f32) -> Does {
    Does::Volley {
        count,
        school,
        spread: Some(spread),
    }
}

const fn adds(name: &'static str, count: u8, radius: f32) -> Does {
    Does::Adds {
        name,
        count,
        radius,
    }
}

const fn charge(windup: f32, speed: f32) -> Does {
    Does::Charge { windup, speed }
}

const fn frenzy(secs: f32, speed: f32) -> Does {
    Does::Frenzy { secs, speed }
}

// The shapes the rows share.

/// The red circle under the hero the shared turns drop.
const UNDER_HERO: Shape = Shape::Circle { radius: 5.5 };
/// A stomp round the boss.
const STOMP: Shape = Shape::Circle { radius: 9.0 };
/// A short cone, about 75 degrees across.
const CLEAVE_CONE: Shape = Shape::Cone {
    radius: 9.0,
    half_angle: 0.65,
};
/// A long cone, a breath.
const BREATH: Shape = Shape::Cone {
    radius: 12.0,
    half_angle: 0.5,
};
/// Safe close to the boss and well away from it.
const NOVA_RING: Shape = Shape::Donut {
    inner: 4.0,
    outer: 14.0,
};
/// One of a few bombs round the hero.
const BOMB: Shape = Shape::Circle { radius: 4.0 };

const fn stomp(first: f32, every_secs: f32, spell: u32) -> A {
    A::new("Stomp", spell, every(first, every_secs), Aim::Boss, ground(STOMP, 1.8, 1.6))
}

const fn circle(first: f32, every_secs: f32) -> A {
    A::new("Shadow Blast", SHADOW_BOLT, every(first, every_secs), Aim::Hero, ground(UNDER_HERO, 1.5, 2.0))
}

const fn cleave(first: f32, every_secs: f32, spell: u32) -> A {
    A::new("Cleave", spell, every(first, every_secs), Aim::Hero, ground(CLEAVE_CONE, 2.0, 1.8))
}

const fn bombs(first: f32, every_secs: f32, spell: u32) -> A {
    A::new("Bombs", spell, every(first, every_secs), Aim::AroundHero(3), ground(BOMB, 2.0, 1.4))
}

const fn rush(first: f32, every_secs: f32) -> A {
    A::new("Charge", CHARGE, every(first, every_secs), Aim::Hero, charge(2.0, 16.0))
}

const fn rage(first: f32, every_secs: f32) -> A {
    A::new("Frenzy", FRENZY, every(first, every_secs), Aim::Boss, frenzy(6.0, 1.4))
}

const fn volley(first: f32, every_secs: f32, spell: u32, school: School, count: u8) -> A {
    A::new("Volley", spell, every(first, every_secs), Aim::Boss, ring(count, school))
}

const fn daggers(first: f32, every_secs: f32, count: u8, spread: f32) -> A {
    A::new("Daggers", 0, every(first, every_secs), Aim::Hero, fan(count, School::Arrow, spread))
}

// --- Signature loot ---

static HUGE_GNOLL_CLAW: [Tpl; 1] = [Tpl {
    slot: Slot::Trinket,
    quality: Quality::Uncommon,
    name: "Huge Gnoll Claw",
    stats: [Stat::Might, Stat::Health],
}];

static DEFIAS_LOOT: [Tpl; 2] = [
    Tpl {
        slot: Slot::Weapon,
        quality: Quality::Rare,
        name: "Cruel Barb",
        stats: [Stat::Might, Stat::Crit],
    },
    Tpl {
        slot: Slot::Back,
        quality: Quality::Rare,
        name: "Cape of the Brotherhood",
        stats: [Stat::Armor, Stat::Speed],
    },
];

static PERDITIONS_BLADE: [Tpl; 1] = [Tpl {
    slot: Slot::Weapon,
    quality: Quality::Epic,
    name: "Perdition's Blade",
    stats: [Stat::Might, Stat::Area],
}];

static MIGHT_OF_MENETHIL: [Tpl; 1] = [Tpl {
    slot: Slot::Weapon,
    quality: Quality::Epic,
    name: "Might of Menethil",
    stats: [Stat::Might, Stat::Haste],
}];

// --- Shared scripts ---

/// The green dragons: a breath down a cone and bombs of nightmare round the hero.
static DRAGON: [A; 2] = [
    A::new("Breath", FLAME_BREATH, every(4.0, 10.0), Aim::Hero, ground(BREATH, 2.0, 1.8)),
    bombs(8.0, 16.0, 0),
];

/// A script's rows as a `&'static` slice. Every kit's rows are their own constant, so each is
/// certain to live for the whole run, whatever the rows' const function calls.
macro_rules! rows {
    ($($row:expr),* $(,)?) => {{
        const ROWS: &[A] = &[$($row),*];
        ROWS
    }};
}

const fn kit(boss: &'static str, abilities: &'static [A]) -> Kit {
    Kit {
        boss,
        abilities,
        arrive: None,
        loot: &[],
    }
}

/// Every boss with a script of its own. [`super::kit_for`] looks a boss up by name.
pub(super) static KITS: &[Kit] = &[
    // --- Elwynn Forest ---
    kit(
        "Mother Fang",
        rows![
            A::new("Brood", 0, every(6.0, 22.0), Aim::Boss, adds("Mine Spider", 4, 5.0)),
            circle(4.0, 11.0),
        ],
    ),
    kit(
        "Narg the Taskmaster",
        rows![
            A::new("Taskmaster's Call", 0, every(7.0, 24.0), Aim::Boss, adds("Kobold Laborer", 3, 5.0)),
            stomp(5.0, 13.0, WAR_STOMP),
        ],
    ),
    // The finale: Hogger calls the Riverpaw twice as he weakens, and charges.
    Kit {
        boss: "Hogger",
        abilities: rows![
            A::new("Riverpaw Call", 0, When::Health(0.7), Aim::Boss, adds("Riverpaw Runt", 3, 5.0))
                .yell("Yipe! Hogger needs help!", ""),
            A::new("Riverpaw Call", 0, When::Health(0.35), Aim::Boss, adds("Riverpaw Runt", 3, 5.0)),
            rush(6.0, 14.0),
        ],
        arrive: None,
        loot: &HUGE_GNOLL_CLAW,
    },
    kit("Thuros Lightfingers", rows![daggers(4.0, 9.0, 5, 0.9), circle(8.0, 14.0)]),
    kit("Fedfennel", rows![cleave(4.0, 10.0, CLEAVE), circle(9.0, 16.0)]),
    kit("Gruff Swiftbite", rows![rush(6.0, 14.0), rage(12.0, 22.0)]),
    // --- Westfall ---
    kit(
        "Brainwashed Noble",
        rows![
            volley(5.0, 14.0, SHADOW_VOLLEY, School::Shadow, 8),
            circle(3.0, 10.0),
        ],
    ),
    kit(
        "Marisa du'Paige",
        rows![
            volley(4.0, 15.0, FROST_VOLLEY, School::Frost, 8),
            A::new("Fire Nova", FIRE_NOVA, every(8.0, 16.0), Aim::Boss, ground(NOVA_RING, 2.5, 1.8)),
        ],
    ),
    kit(
        "Sergeant Brashclaw",
        rows![
            cleave(4.0, 11.0, CLEAVE),
            A::new("Riverpaw Reinforcements", 0, every(8.0, 26.0), Aim::Boss, adds("Riverpaw Brute", 2, 5.0)),
        ],
    ),
    kit("Klaven Mortwake", rows![daggers(4.0, 9.0, 5, 0.9), circle(9.0, 15.0)]),
    kit("Lupos", rows![rush(5.0, 13.0), rage(11.0, 20.0)]),
    // A dive: a charge that comes faster.
    kit(
        "Vultros",
        rows![A::new("Dive", CHARGE, every(4.0, 11.0), Aim::Hero, charge(2.0, 18.0))],
    ),
    // The finale: a fan of daggers, a circle, and the Blackguards at half health.
    Kit {
        boss: "Edwin VanCleef",
        abilities: rows![
            daggers(4.0, 10.0, 7, 1.2),
            circle(7.0, 13.0),
            A::new("Blackguards", 0, When::Health(0.5), Aim::Boss, adds("Defias Blackguard", 3, 5.0))
                .yell("Lackeys, take over!", "A_Edwin VanCleef Health01"),
        ],
        arrive: Some(("None may challenge the Brotherhood!", "A_Edwin VanCleef Aggro01")),
        loot: &DEFIAS_LOOT,
    },
    // --- The Barrens ---
    kit("Echeyakee", rows![rush(5.0, 13.0), rage(12.0, 22.0)]),
    kit("Takk the Leaper", rows![A::new("Leap", CHARGE, every(4.0, 11.0), Aim::Hero, charge(2.0, 18.0))]),
    kit("Lakota'mani", rows![stomp(4.0, 11.0, WAR_STOMP), circle(8.0, 15.0)]),
    kit("Humar the Pridelord", rows![cleave(4.0, 10.0, CLEAVE), rage(12.0, 22.0)]),
    // The finale charges twice running, then stomps.
    kit(
        "Agathelos the Raging",
        rows![rush(5.0, 16.0), rush(9.0, 16.0), stomp(7.0, 15.0, WAR_STOMP)],
    ),
    kit("Hezrul Bloodmark", rows![daggers(4.0, 9.0, 5, 0.9), stomp(8.0, 14.0, WAR_STOMP)]),
    // --- Durotar ---
    kit(
        "Geolord Mottle",
        rows![
            volley(5.0, 14.0, POISON_VOLLEY, School::Nature, 8),
            bombs(8.0, 15.0, 0),
        ],
    ),
    kit(
        "Burning Blade Cultist",
        rows![
            A::new("Summon Familiars", 0, every(6.0, 24.0), Aim::Boss, adds("Vile Familiar", 3, 5.0)),
            circle(4.0, 11.0),
        ],
    ),
    kit(
        "Death Flayer",
        rows![cleave(4.0, 10.0, POISON_VOLLEY), rush(8.0, 16.0)],
    ),
    // --- Fire Plume Ridge ---
    Kit {
        boss: "Ragnaros",
        abilities: rows![
            A::new(
                "Lava Bombs",
                LAVA_BOMB,
                every(4.0, 12.0),
                Aim::AroundHero(3),
                burning(Shape::Circle { radius: 4.5 }, 2.0, 1.4, 0.3, 8.0),
            ),
            A::new("Wrath of Ragnaros", WRATH_OF_RAGNAROS, every(8.0, 18.0), Aim::Boss, ground(Shape::Circle { radius: 10.0 }, 2.0, 1.8))
                .yell("BY FIRE BE PURGED!", "A_RagnarosSpecialAttack01"),
            A::new("Sons of Flame", 0, When::Health(0.5), Aim::Boss, adds("Son of Flame", 4, 6.0)),
        ],
        arrive: Some(("Impudent whelps! You've rushed headlong to your own demise!", "A_RagnarosArrival01")),
        loot: &PERDITIONS_BLADE,
    },
    kit("Baron Charr", rows![bombs(4.0, 12.0, LAVA_BOMB), circle(8.0, 15.0)]),
    kit("King Mosh", rows![stomp(4.0, 12.0, WAR_STOMP), rush(8.0, 16.0)]),
    // --- Gurubashi Arena ---
    Kit {
        boss: "Bloodlord Mandokir",
        abilities: rows![rush(5.0, 14.0), cleave(8.0, 12.0, CLEAVE)],
        arrive: Some(("I'll feed your souls to Hakkar himself!", "A_BloodlordMandokirAggro")),
        loot: &[],
    },
    kit(
        "Jin'do the Hexxer",
        rows![
            volley(5.0, 14.0, SHADOW_VOLLEY, School::Shadow, 8),
            A::new("Shades of Jin'do", 0, every(8.0, 26.0), Aim::Boss, adds("Shade of Jin'do", 3, 6.0)),
        ],
    ),
    Kit {
        boss: "Hakkar",
        abilities: rows![
            volley(4.0, 13.0, SHADOW_VOLLEY, School::Shadow, 10),
            A::new(
                "Blood Pools",
                0,
                every(8.0, 16.0),
                Aim::AroundHero(3),
                burning(BOMB, 2.0, 1.4, 0.25, 6.0),
            ),
        ],
        arrive: Some(("FACE THE WRATH OF THE SOULFLAYER!", "A_HakkarAggro")),
        loot: &[],
    },
    // --- Gates of Ahn'Qiraj ---
    kit(
        "Kurinnaxx",
        rows![A::new(
            "Sand Traps",
            0,
            every(4.0, 12.0),
            Aim::AroundHero(3),
            burning(BOMB, 2.0, 1.4, 0.25, 7.0),
        )],
    ),
    kit(
        "Moam",
        rows![
            A::new("Mana Nova", FIRE_NOVA, every(5.0, 14.0), Aim::Boss, ground(NOVA_RING, 2.5, 1.8)),
            circle(9.0, 16.0),
        ],
    ),
    kit(
        "Princess Huhuran",
        rows![rage(8.0, 20.0), volley(5.0, 14.0, POISON_VOLLEY, School::Nature, 8)],
    ),
    Kit {
        boss: "Ossirian the Unscarred",
        abilities: rows![
            stomp(4.0, 13.0, WAR_STOMP),
            volley(7.0, 14.0, POISON_VOLLEY, School::Nature, 10),
        ],
        arrive: Some(("The sands will swallow you!", "Ossirian - Agro")),
        loot: &[],
    },
    // --- Naxxramas ---
    kit(
        "Patchwerk",
        rows![
            A::new("Hateful Strike", THRASH, every(4.0, 9.0), Aim::Hero, ground(Shape::Cone { radius: 8.0, half_angle: 0.9 }, 2.0, 2.0)),
            rage(14.0, 24.0),
        ],
    ),
    kit(
        "Gluth",
        rows![
            A::new("Zombie Chow", 0, every(7.0, 24.0), Aim::Boss, adds("Zombie Chow", 4, 6.0)),
            rage(12.0, 20.0),
        ],
    ),
    Kit {
        boss: "Kel'Thuzad",
        abilities: rows![
            volley(5.0, 13.0, FROST_VOLLEY, School::Frost, 10),
            A::new(
                "Shadow Fissures",
                0,
                every(8.0, 16.0),
                Aim::AroundHero(3),
                burning(BOMB, 2.0, 1.4, 0.25, 7.0),
            ),
            A::new("Frost Blast", FROST_VOLLEY, every(12.0, 24.0), Aim::Boss, ground(NOVA_RING, 2.5, 1.8)),
        ],
        arrive: None,
        loot: &MIGHT_OF_MENETHIL,
    },
    // --- Mount Hyjal ---
    kit("Taerar", &DRAGON),
    kit("Lethon", &DRAGON),
    kit("Emeriss", &DRAGON),
    kit(
        "Ysondre",
        rows![
            A::new("Breath", FLAME_BREATH, every(4.0, 10.0), Aim::Hero, ground(BREATH, 2.0, 1.8)),
            volley(7.0, 14.0, POISON_VOLLEY, School::Nature, 8),
        ],
    ),
];

// --- Adds ---

/// A creature a script calls: a new definition (name, look and pace of its own) whose numbers
/// come from the zone's horde at its place `at` along the ladder (0 first, 1 last,
/// [`super::super::data::rung`]). Displays are the zone's roster ones, so the client has them.
pub(super) struct Add {
    pub zone: &'static str,
    pub name: &'static str,
    pub display: u32,
    pub scale: f32,
    pub speed: f32,
    pub at: f32,
}

/// The adds of every zone, in the order the zone's extras list them ([`super::adds_for`]).
pub(super) static ADDS: &[Add] = &[
    Add { zone: "Elwynn Forest", name: "Mine Spider", display: 382, scale: 1.0, speed: 4.6, at: 0.35 },
    Add { zone: "Elwynn Forest", name: "Kobold Laborer", display: 373, scale: 1.0, speed: 4.2, at: 0.35 },
    Add { zone: "Elwynn Forest", name: "Riverpaw Runt", display: 383, scale: 1.0, speed: 4.4, at: 0.4 },
    Add { zone: "Westfall", name: "Riverpaw Brute", display: 384, scale: 1.0, speed: 4.4, at: 0.45 },
    Add { zone: "Westfall", name: "Defias Blackguard", display: 2331, scale: 1.1, speed: 4.4, at: 0.5 },
    Add { zone: "Durotar", name: "Vile Familiar", display: 1185, scale: 1.0, speed: 4.6, at: 0.3 },
    Add { zone: "Fire Plume Ridge", name: "Son of Flame", display: 1204, scale: 1.0, speed: 4.8, at: 0.7 },
    Add { zone: "Gurubashi Arena", name: "Shade of Jin'do", display: 11229, scale: 1.2, speed: 4.6, at: 0.7 },
    Add { zone: "Naxxramas", name: "Zombie Chow", display: 10626, scale: 1.3, speed: 4.2, at: 0.5 },
];
