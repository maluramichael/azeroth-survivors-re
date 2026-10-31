//! What crosses the wire: the components every hero replicates with, and the messages between a
//! guest and its host. Registered in one order on both ends ([`protocol`]); replicon checks the
//! two match at the handshake.

use bevy::ecs::entity::{EntityMapper, MapEntities};
use bevy::prelude::*;
use bevy_replicon::prelude::*;
use serde::{Deserialize, Serialize};

use crate::units::{AreaLook, FxRecord, ObjLook, Still};


// Replicated components ------------------------------------------------------------------------

pub(crate) use crate::units::Look;

/// Where a unit stands, quantised: centimetres, a 256-step facing, and the walk the animation
/// shows (pace in cm/s and the movement flags). From deanmathieson's fork (`coop/wire.rs`).
#[derive(Component, Serialize, Deserialize, Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct NetPos {
    pub at: [i32; 3],
    pub yaw: u8,
    pub pace: u16,
    pub flags: u32,
}

impl NetPos {
    pub fn new(pos: Vec3, yaw: f32, pace: f32, flags: u32) -> Self {
        let cm = |v: f32| (v * 100.0).round() as i32;
        let turn = yaw.rem_euclid(std::f32::consts::TAU) / std::f32::consts::TAU;
        Self {
            at: [cm(pos.x), cm(pos.y), cm(pos.z)],
            yaw: ((turn * 256.0).round() as u32 % 256) as u8,
            pace: (pace.max(0.0) * 100.0).round().min(f32::from(u16::MAX)) as u16,
            flags,
        }
    }

    pub fn pos(&self) -> Vec3 {
        Vec3::new(
            self.at[0] as f32 / 100.0,
            self.at[1] as f32 / 100.0,
            self.at[2] as f32 / 100.0,
        )
    }

    /// The facing in radians, in `[0, τ)`.
    pub fn yaw(&self) -> f32 {
        f32::from(self.yaw) / 256.0 * std::f32::consts::TAU
    }

    pub fn pace(&self) -> f32 {
        f32::from(self.pace) / 100.0
    }
}

/// Who a hero is: its player (0 is the host), name and class.
#[derive(Component, Serialize, Deserialize, Clone, Debug, PartialEq)]
pub(crate) struct NetHero {
    pub player: u8,
    pub name: String,
    pub class_id: u8,
}

/// An enemy as a guest needs it: which creature (by [`crate::enemies::def_ref`]), its health,
/// and what kind it is.
#[derive(Component, Serialize, Deserialize, Clone, Copy, Debug, PartialEq)]
pub(crate) struct NetEnemy {
    pub def: (u8, u16),
    pub hp: f32,
    pub max_hp: f32,
    pub boss: bool,
    pub elite: bool,
    pub dead: bool,
}

/// A hero's health, shield, fall and gold this run.
#[derive(Component, Serialize, Deserialize, Clone, Copy, Debug, Default, PartialEq)]
pub(crate) struct NetVitals {
    pub hp: f32,
    pub max_hp: f32,
    pub shield: f32,
    pub dead: bool,
    pub gold: u32,
    /// Damage dealt this run, for the results.
    pub damage: f64,
}

/// A hero's spells (key, rank, cooldown left in centiseconds) and passives (key, rank).
#[derive(Component, Serialize, Deserialize, Clone, Debug, Default, PartialEq)]
pub(crate) struct NetBuild {
    pub abilities: Vec<(String, u8, u16)>,
    pub passives: Vec<(String, u8)>,
}

/// The auras a unit shows, slot by slot: the buff slots on a hero, the debuffs on an enemy.
#[derive(Component, Serialize, Deserialize, Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct NetAuras {
    pub buffs: bool,
    pub spells: [u32; 8],
}

/// The party's quests as the tracker shows them, on the [`NetRun`] entity.
#[derive(Component, Serialize, Deserialize, Clone, Debug, Default, PartialEq)]
pub(crate) struct NetQuests(pub Vec<crate::quests::QuestLine>);

/// What the party shares, on one entity of its own: the clock, the kills, the level and its
/// experience, the banner.
#[derive(Component, Serialize, Deserialize, Clone, Debug, Default, PartialEq)]
pub(crate) struct NetRun {
    pub elapsed: f32,
    pub kills: u32,
    pub level: u32,
    pub xp: f32,
    /// The last party-wide banner and its number ([`crate::Run::announce`]).
    pub banner: (u32, String),
}

// Guest to host --------------------------------------------------------------------------------

/// A guest's first word: who it is and what its hero looks like.
#[derive(Message, Serialize, Deserialize, Clone, Debug, PartialEq)]
pub(crate) struct Hello {
    pub name: String,
    pub class_id: u8,
    pub display: u32,
    pub race: u8,
    pub sex: u8,
    /// What the guest typed for a locked party; empty otherwise.
    pub password: String,
}

/// A level-up pick or skip by keys ([`crate::hud::Picked`]).
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
pub(crate) enum NetPicked {
    NewAbility(String),
    Upgrade(String),
    NewPassive(String),
    UpPassive(String),
    Evolve(String),
    Heal,
    Skip(u32),
}

impl NetPicked {
    pub fn from_picked(p: crate::hud::Picked) -> Self {
        use crate::hud::Picked as P;
        match p {
            P::NewAbility(k) => Self::NewAbility(k.into()),
            P::Upgrade(k) => Self::Upgrade(k.into()),
            P::NewPassive(k) => Self::NewPassive(k.into()),
            P::UpPassive(k) => Self::UpPassive(k.into()),
            P::Evolve(k) => Self::Evolve(k.into()),
            P::Heal => Self::Heal,
            P::Skip(g) => Self::Skip(g),
        }
    }

    /// The pick with this machine's own keys; `None` for a key it does not know.
    pub fn to_picked(&self) -> Option<crate::hud::Picked> {
        use crate::data;
        use crate::hud::Picked as P;
        let ability = |k: &str| data::ABILITIES.iter().find(|a| a.key == k).map(|a| a.key);
        let passive = |k: &str| data::PASSIVES.iter().find(|p| p.key == k).map(|p| p.key);
        Some(match self {
            Self::NewAbility(k) => P::NewAbility(ability(k)?),
            Self::Upgrade(k) => P::Upgrade(ability(k)?),
            Self::NewPassive(k) => P::NewPassive(passive(k)?),
            Self::UpPassive(k) => P::UpPassive(passive(k)?),
            Self::Evolve(k) => P::Evolve(ability(k)?),
            Self::Heal => P::Heal,
            Self::Skip(g) => P::Skip(*g),
        })
    }
}

/// What the guest's hero brings beyond its class: the profile's bonus and talents, the gear worn
/// and the world buffs taken. Sent when the run starts and whenever one changes; taken as it is.
#[derive(Message, Serialize, Deserialize, Clone, Copy, Debug, Default, PartialEq)]
pub(crate) struct Loadout {
    pub meta: crate::profile::MetaBonus,
    pub talents: crate::mods::StatMods,
    pub gear: crate::mods::StatMods,
    pub buffs: crate::mods::StatMods,
}

impl Loadout {
    pub fn of(hero: &crate::HeroState) -> Self {
        Self {
            meta: hero.meta,
            talents: hero.talents,
            gear: hero.gear,
            buffs: hero.buffs,
        }
    }

    /// Every number is a number: no NaN or infinity reaches the host's sums.
    pub fn is_finite(&self) -> bool {
        let m = &self.meta;
        let meta = [m.max_hp, m.might, m.speed, m.haste, m.magnet, m.armor, m.regen, m.greed];
        let mods = |s: &crate::mods::StatMods| {
            [
                s.might, s.max_hp, s.speed, s.regen, s.haste, s.area, s.magnet, s.growth, s.armor,
                s.crit, s.greed,
            ]
        };
        meta.iter()
            .chain(&mods(&self.talents))
            .chain(&mods(&self.gear))
            .chain(&mods(&self.buffs))
            .all(|v| v.is_finite())
    }

    /// Onto `hero`, keeping its share of health.
    pub fn apply(&self, hero: &mut crate::HeroState) {
        let before = hero.stats().max_hp.max(1.0);
        let share = (hero.hp / before).clamp(0.0, 1.0);
        hero.meta = self.meta;
        hero.talents = self.talents;
        hero.gear = self.gear;
        hero.buffs = self.buffs;
        if hero.dead_t.is_none() {
            hero.hp = share * hero.stats().max_hp;
        }
    }
}

/// The guest's focus ([`crate::focus::Focus::to_byte`]) and the enemy it clicked, if any.
#[derive(Message, Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Aim {
    pub focus: u8,
    pub target: Option<Entity>,
}

impl MapEntities for Aim {
    fn map_entities<E: EntityMapper>(&mut self, mapper: &mut E) {
        if let Some(t) = &mut self.target {
            *t = mapper.get_mapped(*t);
        }
    }
}

/// The guest took a level-up card or skipped a hand.
#[derive(Message, Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
pub(crate) struct CardAction(pub NetPicked);

/// Where the guest's own hero is, 20 times a second.
#[derive(Message, Serialize, Deserialize, Clone, Copy, Debug, PartialEq)]
pub(crate) struct MoveInput {
    pub at: [f32; 3],
    pub yaw: f32,
    pub pace: f32,
    pub flags: u32,
    /// The level-up cards are open ([`crate::HeroState::picking`]).
    pub picking: bool,
}

// Host to guest --------------------------------------------------------------------------------

/// The guest has a seat: its player number.
#[derive(Message, Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Welcome {
    pub player: u8,
}

/// The guest is turned away, and why.
#[derive(Message, Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
pub(crate) struct Refused {
    pub reason: String,
}

/// The host's run began (or is under way): the guest enters the same battleground.
#[derive(Message, Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct StartRun {
    pub map: usize,
}

/// The host is back at the menu: so is the guest.
#[derive(Message, Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct EndRun;

/// A word for one guest about its own hero ([`crate::Mail`]).
#[derive(Message, Serialize, Deserialize, Clone, Debug, PartialEq)]
pub(crate) enum NetMail {
    Banner(String),
    Line(String),
    Gold(u32),
    Item(crate::items::Item),
}

impl NetMail {
    pub fn from_mail(m: crate::Mail) -> Self {
        match m {
            crate::Mail::Banner(t) => Self::Banner(t),
            crate::Mail::Line(t) => Self::Line(t),
            crate::Mail::Gold(g) => Self::Gold(g),
            crate::Mail::Item(i) => Self::Item(i),
        }
    }
}

/// The host's run is over: won, or its hero fell.
#[derive(Message, Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct RunOver {
    pub won: bool,
}

/// One effect the host played, for the guest to play the same ([`FxRecord`]).
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub(crate) enum NetFx {
    Cast {
        caster: Entity,
        spell: u32,
        hits: Vec<Entity>,
        dest: Option<[f32; 3]>,
    },
    Impact {
        target: Entity,
        spell: u32,
    },
    Swing {
        attacker: Entity,
        victim: Entity,
        damage: f32,
    },
    Number {
        anchor: Entity,
        amount: f32,
        crit: bool,
        color: u32,
    },
}

impl NetFx {
    pub fn from_record(r: &FxRecord) -> Self {
        match r.clone() {
            FxRecord::Cast {
                caster,
                spell,
                hits,
                dest,
            } => Self::Cast {
                caster,
                spell,
                hits,
                dest: dest.map(|d| d.to_array()),
            },
            FxRecord::Impact { target, spell } => Self::Impact { target, spell },
            FxRecord::Swing {
                attacker,
                victim,
                damage,
            } => Self::Swing {
                attacker,
                victim,
                damage,
            },
            FxRecord::Number {
                anchor,
                amount,
                crit,
                color,
            } => Self::Number {
                anchor,
                amount,
                crit,
                color,
            },
        }
    }

    /// Every entity the effect names.
    pub fn entities(&self) -> Vec<Entity> {
        match self {
            Self::Cast { caster, hits, .. } => std::iter::once(*caster).chain(hits.iter().copied()).collect(),
            Self::Impact { target, .. } => vec![*target],
            Self::Swing {
                attacker, victim, ..
            } => vec![*attacker, *victim],
            Self::Number { anchor, .. } => vec![*anchor],
        }
    }

    /// The same effect with every entity passed through `f`.
    pub fn map(&mut self, mut f: impl FnMut(Entity) -> Entity) {
        match self {
            Self::Cast { caster, hits, .. } => {
                *caster = f(*caster);
                for h in hits {
                    *h = f(*h);
                }
            }
            Self::Impact { target, .. } => *target = f(*target),
            Self::Swing {
                attacker, victim, ..
            } => {
                *attacker = f(*attacker);
                *victim = f(*victim);
            }
            Self::Number { anchor, .. } => *anchor = f(*anchor),
        }
    }
}

/// A frame's effects, in the order the host played them. The entities are the host's and are
/// mapped to the guest's on arrival.
#[derive(Message, Serialize, Deserialize, Clone, Debug, Default, PartialEq)]
pub(crate) struct FxBatch(pub Vec<NetFx>);

impl MapEntities for FxBatch {
    fn map_entities<E: EntityMapper>(&mut self, mapper: &mut E) {
        for fx in &mut self.0 {
            fx.map(|e| mapper.get_mapped(e));
        }
    }
}

/// The replication rules and messages, in the one order both ends register them.
pub(super) fn protocol(app: &mut App) {
    app.replicate::<Look>()
        .replicate::<NetPos>()
        .replicate::<NetHero>()
        .replicate::<ObjLook>()
        .replicate::<AreaLook>()
        .replicate::<Still>()
        .replicate::<NetEnemy>()
        .replicate::<NetVitals>()
        .replicate::<NetBuild>()
        .replicate::<NetAuras>()
        .replicate::<NetRun>()
        .replicate::<NetQuests>()
        .add_client_message::<Hello>(Channel::Ordered)
        .add_client_message::<MoveInput>(Channel::Unreliable)
        .add_client_message::<CardAction>(Channel::Ordered)
        .add_client_message::<Loadout>(Channel::Ordered)
        .add_mapped_client_message::<Aim>(Channel::Ordered)
        .add_server_message::<Welcome>(Channel::Ordered)
        .add_server_message::<Refused>(Channel::Ordered)
        .make_message_independent::<Refused>()
        .add_server_message::<StartRun>(Channel::Ordered)
        .add_server_message::<EndRun>(Channel::Ordered)
        .add_server_message::<RunOver>(Channel::Ordered)
        .add_server_message::<NetMail>(Channel::Ordered)
        .add_mapped_server_message::<FxBatch>(Channel::Ordered);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::units::UnitSpec;

    #[test]
    fn a_loadout_keeps_the_share_of_health_and_refuses_nan() {
        let class = &crate::data::CLASSES[0];
        let mut hero = crate::HeroState::new(class, 1, Default::default(), Default::default());
        hero.hp = hero.stats().max_hp / 2.0;
        let mut loadout = Loadout::of(&hero);
        loadout.meta.max_hp = 2.0;
        loadout.gear.might = 0.5;
        assert!(loadout.is_finite());
        loadout.apply(&mut hero);
        assert!((hero.hp / hero.stats().max_hp - 0.5).abs() < 1e-4);
        assert_eq!(hero.gear.might, 0.5);
        loadout.buffs.crit = f32::NAN;
        assert!(!loadout.is_finite());
    }

    #[test]
    fn an_effect_maps_every_entity_it_names() {
        let a = Entity::from_raw_u32(1).unwrap();
        let b = Entity::from_raw_u32(2).unwrap();
        let c = Entity::from_raw_u32(3).unwrap();
        let mut fx = NetFx::from_record(&FxRecord::Cast {
            caster: a,
            spell: 116,
            hits: vec![b, c],
            dest: Some(Vec3::new(1.0, 2.0, 3.0)),
        });
        assert_eq!(fx.entities(), vec![a, b, c]);
        fx.map(|e| if e == b { a } else { e });
        assert_eq!(fx.entities(), vec![a, a, c]);
        let NetFx::Cast { dest, .. } = fx else { panic!("a cast") };
        assert_eq!(dest, Some([1.0, 2.0, 3.0]));
        let mut swing = NetFx::from_record(&FxRecord::Swing {
            attacker: a,
            victim: c,
            damage: 12.0,
        });
        swing.map(|e| if e == c { b } else { e });
        assert_eq!(swing.entities(), vec![a, b]);
    }

    #[test]
    fn net_pos_round_trips_to_a_centimetre() {
        let p = NetPos::new(Vec3::new(12.345, -3.21, 700.009), 1.0, 7.0, 3);
        assert!((p.pos() - Vec3::new(12.345, -3.21, 700.009)).abs().max_element() <= 0.006);
        assert!((p.yaw() - 1.0).abs() < std::f32::consts::TAU / 256.0);
        assert!((p.pace() - 7.0).abs() < 0.01);
        assert_eq!(p.flags, 3);
    }

    #[test]
    fn a_look_rebuilds_the_spec_it_came_from() {
        let weapons = [(0u16, 1927u32, 2u8, 10u8, 2u8, 17u8, 2u8), (1, 2001, 4, 6, 1, 14, 3)];
        let spec = UnitSpec {
            serial: 7,
            display: 1234,
            scale: 1.0,
            level: 60,
            faction: 35,
            bytes0: 0x0109_0105,
            weapons: &weapons,
            sheath: 1,
        };
        let look = Look::from_spec(&spec);
        let back = look.spec(7);
        assert_eq!(back.serial, 7);
        assert_eq!(back.display, spec.display);
        assert_eq!(back.scale, spec.scale);
        assert_eq!(back.level, spec.level);
        assert_eq!(back.faction, spec.faction);
        assert_eq!(back.bytes0, spec.bytes0);
        assert_eq!(back.weapons, spec.weapons);
        assert_eq!(back.sheath, spec.sheath);
    }
}
