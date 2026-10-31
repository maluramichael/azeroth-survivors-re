//! Loot in WoW quality colours: gear dropped by enemies, worn in slots for the run
//! ([`super::Run::gear`]), and one piece kept in the bank after the run.
//!
//! The dead sometimes leave an item, lit on the ground by a beam in its quality colour (grey,
//! white, green, blue, purple); elites and bosses drop better ones. Walking over it picks it up and
//! the best item per slot is worn on its own (the newer wins a tie). When the run ends the single
//! best piece worn goes to the bank, `survivors-items.toml`, and is worn from the start of the
//! next run: one item carries over, as in the genre's "keep one" rule.

use std::sync::{Mutex, OnceLock};

use benilla_world::schedule::WorldStage;
use bevy::prelude::*;
use serde::{Deserialize, Serialize};

use super::chatlog::{ChatKind, ChatLine};
use super::enemies::{Dying, Elite, Enemy};
use super::finale::Finale;
use super::hero::Hero;
use super::hud::UiArt;
use super::mods::StatMods;
use super::pickups::Beacons;
use super::units::flat;
use super::{data, Phase, Rng, Run, RunEntity};

/// Chance a normal kill drops an item, an elite's, and how many items lie on the ground at once.
pub(super) const GRUNT_CHANCE: f32 = 0.012;
pub(super) const ELITE_CHANCE: f32 = 0.6;
const MAX_DROPS: usize = 30;
/// Seconds an item waits on the ground, and how near the hero must walk to take it.
const DROP_LIFE: f32 = 90.0;
const PICKUP_RADIUS: f32 = 1.8;
/// Quality weights, Poor to Epic.
pub(super) const WEIGHTS_GRUNT: [f32; 5] = [45.0, 35.0, 15.0, 4.5, 0.5];
pub(super) const WEIGHTS_ELITE: [f32; 5] = [0.0, 25.0, 45.0, 25.0, 5.0];
pub(super) const WEIGHTS_BOSS: [f32; 5] = [0.0, 0.0, 25.0, 55.0, 20.0];
pub(super) const WEIGHTS_FINALE: [f32; 5] = [0.0, 0.0, 0.0, 40.0, 60.0];
/// How much of an item's points its first stat takes; the second gets the rest.
const FIRST_SHARE: f32 = 0.65;
/// Items found late in the run are up to this much stronger.
const LATE_BONUS: f32 = 0.5;

const SLOT_PX: f32 = 26.0;
const SLOT_GAP: f32 = 3.0;
/// With the HUD (400), under the chat window (402) and the character window (405).
const Z: i32 = 401;
const SLOT_FILL: Color = super::kit::INSET_FILL;

pub(super) fn plugin(app: &mut App) {
    app.init_resource::<Gear>()
        .init_resource::<Bank>()
        .init_resource::<GearUi>()
        .add_systems(Startup, load_bank)
        .add_systems(
            Update,
            (arm, drop_items, collect_items)
                .chain()
                .after(WorldStage::Input)
                .run_if(in_state(Phase::Playing))
                .run_if(|l: Res<benilla_app::survivors_host::loading_screen::LoadingScreen>| !l.covering()),
        )
        .add_systems(Update, show_strip.run_if(in_run))
        .add_systems(OnEnter(Phase::GameOver), (bank_best, teardown))
        .add_systems(OnEnter(Phase::Menu), (reset, teardown));
}

/// The phases that put the HUD on screen.
fn in_run(phase: Res<State<Phase>>) -> bool {
    matches!(phase.get(), Phase::Playing | Phase::LevelUp | Phase::Paused)
}

/// Multiplies every drop chance (items, chests, shards, elites): `WOW_SURVIVORS_DROPS=20` fills a
/// run with loot, to photograph it.
pub(super) fn drop_boost() -> f32 {
    static BOOST: OnceLock<f32> = OnceLock::new();
    *BOOST.get_or_init(|| {
        benilla_app::survivors_host::webenv::var("WOW_SURVIVORS_DROPS")
            .and_then(|v| v.parse::<f32>().ok())
            .map_or(1.0, |v| v.clamp(0.0, 1000.0))
    })
}

#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Debug, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub(super) enum Quality {
    Poor,
    Common,
    Uncommon,
    Rare,
    Epic,
}

impl Quality {
    pub(super) const ALL: [Quality; 5] = [
        Quality::Poor,
        Quality::Common,
        Quality::Uncommon,
        Quality::Rare,
        Quality::Epic,
    ];

    /// The item-quality colours of the 1.12 client.
    pub(super) fn color(self) -> Color {
        let (r, g, b) = match self {
            Quality::Poor => (157, 157, 157),
            Quality::Common => (255, 255, 255),
            Quality::Uncommon => (30, 255, 0),
            Quality::Rare => (0, 112, 221),
            Quality::Epic => (163, 53, 238),
        };
        Color::srgb_u8(r, g, b)
    }

    /// Stat points an item of this quality carries, before its roll.
    fn points(self) -> f32 {
        match self {
            Quality::Poor => 1.0,
            Quality::Common => 1.5,
            Quality::Uncommon => 2.5,
            Quality::Rare => 4.0,
            Quality::Epic => 6.0,
        }
    }

    /// Height of the beam over the drop, in yards: the better, the taller.
    fn beam(self) -> f32 {
        match self {
            Quality::Poor => 1.2,
            Quality::Common => 2.2,
            Quality::Uncommon => 3.5,
            Quality::Rare => 5.0,
            Quality::Epic => 7.0,
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub(super) enum Slot {
    Head,
    Neck,
    Shoulder,
    Back,
    Chest,
    Waist,
    Hands,
    Legs,
    Feet,
    Ring,
    Trinket,
    Weapon,
}

impl Slot {
    pub(super) const ALL: [Slot; 12] = [
        Slot::Head,
        Slot::Neck,
        Slot::Shoulder,
        Slot::Back,
        Slot::Chest,
        Slot::Waist,
        Slot::Hands,
        Slot::Legs,
        Slot::Feet,
        Slot::Ring,
        Slot::Trinket,
        Slot::Weapon,
    ];

    pub(super) fn label(self) -> &'static str {
        match self {
            Slot::Head => "Head",
            Slot::Neck => "Neck",
            Slot::Shoulder => "Shoulder",
            Slot::Back => "Back",
            Slot::Chest => "Chest",
            Slot::Waist => "Waist",
            Slot::Hands => "Hands",
            Slot::Legs => "Legs",
            Slot::Feet => "Feet",
            Slot::Ring => "Ring",
            Slot::Trinket => "Trinket",
            Slot::Weapon => "Weapon",
        }
    }

    /// The slot's `Interface\Icons` texture. Names from memory of the 1.12 art, not checked
    /// against the archive; a missing one leaves the square blank.
    pub(super) fn icon(self) -> &'static str {
        match self {
            Slot::Head => "Interface\\Icons\\INV_Helmet_01.blp",
            Slot::Neck => "Interface\\Icons\\INV_Jewelry_Necklace_01.blp",
            Slot::Shoulder => "Interface\\Icons\\INV_Shoulder_01.blp",
            Slot::Back => "Interface\\Icons\\INV_Misc_Cape_01.blp",
            Slot::Chest => "Interface\\Icons\\INV_Chest_Cloth_01.blp",
            Slot::Waist => "Interface\\Icons\\INV_Belt_01.blp",
            Slot::Hands => "Interface\\Icons\\INV_Gauntlets_01.blp",
            Slot::Legs => "Interface\\Icons\\INV_Pants_01.blp",
            Slot::Feet => "Interface\\Icons\\INV_Boots_01.blp",
            Slot::Ring => "Interface\\Icons\\INV_Jewelry_Ring_01.blp",
            Slot::Trinket => "Interface\\Icons\\INV_Jewelry_Talisman_01.blp",
            Slot::Weapon => "Interface\\Icons\\INV_Sword_01.blp",
        }
    }
}

/// A stat an item rolls, one per [`StatMods`] field.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
enum Stat {
    Might,
    Health,
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

impl Stat {
    /// What one point of it is worth in the stat's own units.
    fn unit(self) -> f32 {
        match self {
            Stat::Might => 0.02,
            Stat::Health => 0.03,
            Stat::Speed => 0.01,
            Stat::Regen => 0.1,
            Stat::Haste => 0.01,
            Stat::Area => 0.02,
            Stat::Magnet => 0.04,
            Stat::Growth => 0.02,
            Stat::Armor => 0.3,
            Stat::Crit => 0.01,
            Stat::Greed => 0.03,
        }
    }

    fn add(self, m: &mut StatMods, v: f32) {
        match self {
            Stat::Might => m.might += v,
            Stat::Health => m.max_hp += v,
            Stat::Speed => m.speed += v,
            Stat::Regen => m.regen += v,
            Stat::Haste => m.haste += v,
            Stat::Area => m.area += v,
            Stat::Magnet => m.magnet += v,
            Stat::Growth => m.growth += v,
            Stat::Armor => m.armor += v,
            Stat::Crit => m.crit += v,
            Stat::Greed => m.greed += v,
        }
    }
}

#[derive(Clone, Copy, PartialEq, Debug, Serialize, Deserialize)]
struct Roll {
    stat: Stat,
    value: f32,
}

/// One piece of gear.
#[derive(Clone, PartialEq, Debug, Serialize, Deserialize)]
pub(super) struct Item {
    name: String,
    slot: Slot,
    quality: Quality,
    stats: Vec<Roll>,
}

impl Item {
    fn mods(&self) -> StatMods {
        let mut m = StatMods::default();
        for r in &self.stats {
            r.stat.add(&mut m, r.value);
        }
        m
    }

    /// Stat points in all, the measure the best item per slot is picked by.
    fn power(&self) -> f32 {
        self.stats.iter().map(|r| r.value / r.stat.unit()).sum()
    }

    /// Whether `self` ranks above `other` for the bank: quality first, then points.
    fn outranks(&self, other: &Item) -> bool {
        (self.quality, self.power()) > (other.quality, other.power())
    }
}

/// An item the dice can roll: its name and slot, its quality and the two stats it favours.
pub(super) struct Tpl {
    pub(super) slot: Slot,
    pub(super) quality: Quality,
    pub(super) name: &'static str,
    stats: [Stat; 2],
}

pub(super) mod table {
    use super::Quality::*;
    use super::Slot::*;
    use super::Stat::*;
    use super::Tpl;

    const fn t(
        slot: super::Slot,
        quality: super::Quality,
        name: &'static str,
        a: super::Stat,
        b: super::Stat,
    ) -> Tpl {
        Tpl {
            slot,
            quality,
            name,
            stats: [a, b],
        }
    }

    /// Twelve slots by five qualities, named after 1.12 items.
    pub static TEMPLATES: &[Tpl] = &[
        t(Head, Poor, "Frayed Cloth Cap", Armor, Magnet),
        t(Head, Common, "Rugged Leather Helm", Armor, Health),
        t(Head, Uncommon, "Defias Renegade Cowl", Crit, Haste),
        t(Head, Rare, "Helm of Valor", Area, Crit),
        t(Head, Epic, "Lionheart Helm", Might, Area),
        t(Neck, Poor, "Cracked Pendant", Regen, Magnet),
        t(Neck, Common, "Copper Necklace", Magnet, Growth),
        t(Neck, Uncommon, "Amulet of the Moon", Growth, Regen),
        t(Neck, Rare, "Mark of Fordring", Growth, Greed),
        t(Neck, Epic, "Choker of the Shifting Sands", Growth, Might),
        t(Shoulder, Poor, "Patched Pauldrons", Armor, Health),
        t(Shoulder, Common, "Rawhide Spaulders", Health, Armor),
        t(Shoulder, Uncommon, "Stonefist Shoulders", Health, Regen),
        t(Shoulder, Rare, "Truestrike Shoulders", Armor, Health),
        t(Shoulder, Epic, "Pauldrons of Might", Health, Might),
        t(Back, Poor, "Tattered Cloak", Armor, Speed),
        t(Back, Common, "Reinforced Cloak", Speed, Armor),
        t(Back, Uncommon, "Hillman's Cloak", Speed, Haste),
        t(Back, Rare, "Cloak of Warding", Speed, Crit),
        t(Back, Epic, "Cloak of Draconic Might", Speed, Might),
        t(Chest, Poor, "Torn Tunic", Health, Armor),
        t(Chest, Common, "Layered Tunic", Health, Regen),
        t(Chest, Uncommon, "Defias Leather Vest", Health, Armor),
        t(Chest, Rare, "Breastplate of Valor", Health, Regen),
        t(Chest, Epic, "Robe of the Archmage", Health, Might),
        t(Waist, Poor, "Frayed Belt", Armor, Greed),
        t(Waist, Common, "Heavy Leather Belt", Greed, Armor),
        t(Waist, Uncommon, "Blackened Defias Belt", Greed, Health),
        t(Waist, Rare, "Girdle of the Mentor", Haste, Greed),
        t(Waist, Epic, "Onslaught Girdle", Haste, Might),
        t(Hands, Poor, "Worn Gloves", Haste, Armor),
        t(Hands, Common, "Rough Leather Gloves", Haste, Crit),
        t(Hands, Uncommon, "Stonefist Gauntlets", Haste, Might),
        t(Hands, Rare, "Gloves of the Hidden Temple", Crit, Haste),
        t(Hands, Epic, "Gauntlets of Annihilation", Haste, Crit),
        t(Legs, Poor, "Tattered Leggings", Armor, Health),
        t(Legs, Common, "Dark Leather Pants", Health, Speed),
        t(Legs, Uncommon, "Defias Leather Pants", Armor, Speed),
        t(Legs, Rare, "Legplates of Valor", Health, Armor),
        t(Legs, Epic, "Eldritch Legguards", Might, Health),
        t(Feet, Poor, "Worn Sandals", Speed, Armor),
        t(Feet, Common, "Heavy Boots", Speed, Magnet),
        t(Feet, Uncommon, "Boots of the Fallen Hero", Speed, Health),
        t(Feet, Rare, "Warbear Boots", Speed, Haste),
        t(Feet, Epic, "Sabatons of the Flamewalker", Speed, Crit),
        t(Ring, Poor, "Rusty Band", Crit, Greed),
        t(Ring, Common, "Copper Ring", Crit, Magnet),
        t(Ring, Uncommon, "Ring of Calm", Crit, Might),
        t(Ring, Rare, "Blackstone Ring", Crit, Haste),
        t(Ring, Epic, "Band of Accuria", Crit, Might),
        t(Trinket, Poor, "Chipped Idol", Magnet, Greed),
        t(Trinket, Common, "Carved Totem", Regen, Magnet),
        t(
            Trinket,
            Uncommon,
            "Talisman of Ephemeral Power",
            Area,
            Growth,
        ),
        t(Trinket, Rare, "Hand of Justice", Haste, Area),
        t(Trinket, Epic, "Zandalarian Hero Charm", Might, Haste),
        t(Weapon, Poor, "Worn Shortsword", Might, Crit),
        t(Weapon, Common, "Hardened Cleaver", Might, Area),
        t(Weapon, Uncommon, "Dragonmaw Cutlass", Might, Crit),
        t(Weapon, Rare, "Dark Iron Pulverizer", Might, Haste),
        t(Weapon, Epic, "Thunderfury, Blessed Blade", Might, Area),
    ];
}

/// The quality `r` (0 to 1) falls on among `weights`, Poor to Epic.
fn pick_quality(weights: &[f32; 5], r: f32) -> Quality {
    let total: f32 = weights.iter().sum();
    let mut at = r * total;
    for (q, w) in Quality::ALL.iter().zip(weights) {
        if at < *w {
            return *q;
        }
        at -= w;
    }
    // Rounding at the top edge, or all-zero weights: the best that has any weight.
    Quality::ALL
        .iter()
        .zip(weights)
        .rev()
        .find(|(_, w)| **w > 0.0)
        .map_or(Quality::Common, |(q, _)| *q)
}

/// A random item of `quality`, stronger the later in the run (`progress` 0 to 1).
fn roll_item(rng: &mut Rng, quality: Quality, progress: f32) -> Item {
    let pool: Vec<&Tpl> = table::TEMPLATES
        .iter()
        .filter(|t| t.quality == quality)
        .collect();
    let tpl = pool[rng.index(pool.len())];
    let points =
        quality.points() * rng.range(0.85, 1.15) * (1.0 + LATE_BONUS * progress.clamp(0.0, 1.0));
    let [a, b] = tpl.stats;
    Item {
        name: tpl.name.to_string(),
        slot: tpl.slot,
        quality,
        stats: vec![
            Roll {
                stat: a,
                value: points * FIRST_SHARE * a.unit(),
            },
            Roll {
                stat: b,
                value: points * (1.0 - FIRST_SHARE) * b.unit(),
            },
        ],
    }
}

/// What the hero wears this run.
#[derive(Resource, Default)]
struct Gear {
    worn: [Option<Item>; 12],
    /// The banked item has been put on for this run.
    armed: bool,
}

impl Gear {
    /// Wear `item` unless the slot holds a stronger one (a tie goes to the newer). Whether it was
    /// worn.
    fn wear(&mut self, item: Item) -> bool {
        let slot = &mut self.worn[item.slot as usize];
        if slot.as_ref().is_some_and(|cur| cur.power() > item.power()) {
            return false;
        }
        *slot = Some(item);
        true
    }

    fn total(&self) -> StatMods {
        let mut m = StatMods::default();
        for item in self.worn.iter().flatten() {
            m.add(&item.mods());
        }
        m
    }

    /// The piece the bank would keep.
    fn best(&self) -> Option<&Item> {
        self.worn
            .iter()
            .flatten()
            .fold(None, |best: Option<&Item>, i| match best {
                Some(b) if !i.outranks(b) => Some(b),
                _ => Some(i),
            })
    }
}

/// The one item kept between runs.
#[derive(Resource, Default, Serialize, Deserialize)]
#[serde(default)]
struct Bank {
    item: Option<Item>,
}

/// An automated run (`WOW_SURVIVORS_AUTO`) reads and writes no bank, as it saves no profile.
fn ephemeral() -> bool {
    benilla_app::survivors_host::webenv::var("WOW_SURVIVORS_AUTO").is_some()
}

fn bank_path() -> Option<std::path::PathBuf> {
    benilla_app::survivors_host::local_state::home().map(|h| h.join("survivors-items.toml"))
}

fn load_bank(mut bank: ResMut<Bank>) {
    if ephemeral() {
        return;
    }
    let Some(path) = bank_path() else { return };
    let Ok(text) = benilla_app::survivors_host::local_state::read_to_string(&path) else {
        return;
    };
    match toml::from_str::<Bank>(&text) {
        Ok(b) => *bank = b,
        Err(e) => {
            warn!("survivors: item bank unreadable, starting empty ({e})");
            let _ = benilla_app::survivors_host::local_state::write_atomic(&path.with_extension("bad"), &text);
        }
    }
}

fn save_bank(bank: &Bank) {
    if ephemeral() {
        return;
    }
    let Some(path) = bank_path() else { return };
    match toml::to_string(bank) {
        Ok(text) => {
            if let Err(e) = benilla_app::survivors_host::local_state::write_atomic(&path, &text) {
                warn!("survivors: item bank not saved ({e})");
            }
        }
        Err(e) => warn!("survivors: item bank not serialized ({e})"),
    }
}

/// What the character window lists: slot, item name and quality colour per piece worn, in slot
/// order. Mirrored here because the window's line builder only gets the run.
static WORN: Mutex<Vec<(&'static str, String, Color)>> = Mutex::new(Vec::new());

pub(super) fn gear_lines() -> Vec<(&'static str, String, Color)> {
    WORN.lock().map(|w| w.clone()).unwrap_or_default()
}

/// Fold the worn pieces into the run's stats and the window's list.
fn sync(run: &mut Run, gear: &Gear) {
    run.gear = gear.total();
    if let Ok(mut w) = WORN.lock() {
        *w = Slot::ALL
            .iter()
            .filter_map(|s| {
                let item = gear.worn[*s as usize].as_ref()?;
                Some((s.label(), item.name.clone(), item.quality.color()))
            })
            .collect();
    }
}

/// The chat window has one colour per kind and none per quality; this is the one place to change
/// when it learns them.
fn chat_kind(_quality: Quality) -> ChatKind {
    ChatKind::System
}

fn loot_line(chat: &mut MessageWriter<ChatLine>, item: &Item) {
    chat.write(ChatLine::text(
        chat_kind(item.quality),
        format!("You receive loot: [{}].", item.name),
    ));
}

/// A new run puts the banked item on.
fn arm(
    mut gear: ResMut<Gear>,
    bank: Res<Bank>,
    mut run: ResMut<Run>,
    mut chat: MessageWriter<ChatLine>,
) {
    if gear.armed {
        return;
    }
    gear.armed = true;
    if let Some(item) = bank.item.clone() {
        chat.write(ChatLine::text(
            chat_kind(item.quality),
            format!("You equip your banked [{}].", item.name),
        ));
        gear.wear(item);
        sync(&mut run, &gear);
    }
}

/// An item on the ground.
#[derive(Component)]
struct ItemDrop {
    item: Item,
    life: f32,
}

/// The dead drop items: now and then a normal kill, often an elite, always a boss. The finale's
/// gear goes straight on, the run being over before anyone could walk to it.
#[allow(clippy::too_many_arguments)]
fn drop_items(
    mut commands: Commands,
    mut rng: ResMut<Rng>,
    mut run: ResMut<Run>,
    mut gear: ResMut<Gear>,
    mut beacons: ResMut<Beacons>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    dead: Query<(&Transform, &Enemy, Has<Elite>, Has<Finale>), Added<Dying>>,
    drops: Query<(), With<ItemDrop>>,
    mut chat: MessageWriter<ChatLine>,
) {
    let boost = drop_boost();
    let progress = data::progress(run.elapsed);
    let mut on_ground = drops.iter().count();
    for (tf, enemy, elite, finale) in &dead {
        if finale {
            let quality = pick_quality(&WEIGHTS_FINALE, rng.f32());
            let item = roll_item(&mut rng, quality, progress);
            loot_line(&mut chat, &item);
            gear.wear(item);
            sync(&mut run, &gear);
            continue;
        }
        let (chance, weights) = if enemy.boss {
            (1.0, &WEIGHTS_BOSS)
        } else if elite {
            (ELITE_CHANCE * boost, &WEIGHTS_ELITE)
        } else {
            (GRUNT_CHANCE * boost, &WEIGHTS_GRUNT)
        };
        if (on_ground >= MAX_DROPS && !enemy.boss) || rng.f32() >= chance {
            continue;
        }
        let quality = pick_quality(weights, rng.f32());
        let item = roll_item(&mut rng, quality, progress);
        let a = rng.range(0.0, std::f32::consts::TAU);
        let at = tf.translation + Vec3::new(a.cos(), 0.0, a.sin()) * 0.9;
        let (color, height) = (item.quality.color(), item.quality.beam());
        let e = commands
            .spawn((
                ItemDrop {
                    item,
                    life: DROP_LIFE,
                },
                Transform::from_translation(at),
                Visibility::default(),
                RunEntity,
            ))
            .id();
        beacons.attach(&mut commands, &mut meshes, &mut materials, e, color, height);
        on_ground += 1;
    }
}

/// Walking over an item picks it up, and wears it if it is the best for its slot.
fn collect_items(
    mut commands: Commands,
    time: Res<Time>,
    mut run: ResMut<Run>,
    mut gear: ResMut<Gear>,
    hero: Query<&Transform, With<Hero>>,
    mut drops: Query<(Entity, &Transform, &mut ItemDrop)>,
    mut chat: MessageWriter<ChatLine>,
) {
    let Ok(h) = hero.single() else { return };
    if run.dead_t.is_some() {
        return;
    }
    let dt = time.delta_secs();
    for (e, tf, mut lying) in &mut drops {
        lying.life -= dt;
        if lying.life <= 0.0 {
            commands.entity(e).despawn();
            continue;
        }
        if flat(tf.translation - h.translation).length() > PICKUP_RADIUS {
            continue;
        }
        commands.entity(e).despawn();
        loot_line(&mut chat, &lying.item);
        if gear.wear(lying.item.clone()) {
            sync(&mut run, &gear);
        }
    }
}

/// The run is over: the best piece worn goes to the bank, unless the bank already holds better.
fn bank_best(gear: Res<Gear>, mut bank: ResMut<Bank>, mut chat: MessageWriter<ChatLine>) {
    let Some(best) = gear.best() else { return };
    if bank.item.as_ref().is_some_and(|kept| kept.outranks(best)) {
        return;
    }
    if bank.item.as_ref() == Some(best) {
        return;
    }
    chat.write(ChatLine::text(
        chat_kind(best.quality),
        format!("[{}] is kept in your bank.", best.name),
    ));
    bank.item = Some(best.clone());
    save_bank(&bank);
}

/// Back at the menu: the gear comes off.
fn reset(mut gear: ResMut<Gear>) {
    *gear = Gear::default();
    if let Ok(mut w) = WORN.lock() {
        w.clear();
    }
}

/// One square of the strip, for a slot.
#[derive(Component)]
struct StripSlot {
    slot: Slot,
    /// The quality its border shows now; `None` while empty and hidden.
    shown: Option<Quality>,
}

#[derive(Resource, Default)]
struct GearUi {
    root: Option<Entity>,
}

fn teardown(mut commands: Commands, mut ui: ResMut<GearUi>) {
    if let Some(root) = ui.root.take() {
        commands.entity(root).try_despawn();
    }
}

/// The strip of worn pieces at the right edge, an icon in a quality-coloured frame each, shown
/// only for slots that hold something.
fn show_strip(
    mut commands: Commands,
    mut ui: ResMut<GearUi>,
    gear: Res<Gear>,
    mut slots: Query<(&mut StripSlot, &mut Node, &mut BorderColor)>,
) {
    if ui.root.is_none() {
        let root = commands
            .spawn((
                Node {
                    position_type: PositionType::Absolute,
                    right: Val::Px(16.0),
                    bottom: Val::Px(150.0),
                    width: Val::Px(SLOT_PX * 2.0 + SLOT_GAP),
                    flex_direction: FlexDirection::Row,
                    flex_wrap: FlexWrap::Wrap,
                    column_gap: Val::Px(SLOT_GAP),
                    row_gap: Val::Px(SLOT_GAP),
                    ..default()
                },
                GlobalZIndex(Z),
            ))
            .with_children(|strip| {
                for slot in Slot::ALL {
                    strip
                        .spawn((
                            StripSlot { slot, shown: None },
                            Node {
                                width: Val::Px(SLOT_PX),
                                height: Val::Px(SLOT_PX),
                                border: UiRect::all(Val::Px(2.0)),
                                border_radius: BorderRadius::all(Val::Px(3.0)),
                                display: Display::None,
                                ..default()
                            },
                            BackgroundColor(SLOT_FILL),
                            BorderColor::all(Color::WHITE),
                        ))
                        .with_children(|square| {
                            square.spawn((
                                Node {
                                    width: Val::Percent(100.0),
                                    height: Val::Percent(100.0),
                                    ..default()
                                },
                                ImageNode {
                                    color: Color::NONE,
                                    ..default()
                                },
                                UiArt::crop(slot.icon(), [0.0, 1.0, 0.0, 1.0]),
                            ));
                        });
                }
            })
            .id();
        ui.root = Some(root);
        return;
    }
    for (mut square, mut node, mut border) in &mut slots {
        let want = gear.worn[square.slot as usize].as_ref().map(|i| i.quality);
        if square.shown == want {
            continue;
        }
        square.shown = want;
        node.display = if want.is_some() {
            Display::Flex
        } else {
            Display::None
        };
        if let Some(q) = want {
            *border = BorderColor::all(q.color());
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn item(slot: Slot, quality: Quality, points: f32, stat: Stat) -> Item {
        Item {
            name: "x".into(),
            slot,
            quality,
            stats: vec![Roll {
                stat,
                value: points * stat.unit(),
            }],
        }
    }

    #[test]
    fn every_slot_has_every_quality() {
        for s in Slot::ALL {
            for q in Quality::ALL {
                let n = table::TEMPLATES
                    .iter()
                    .filter(|t| t.slot == s && t.quality == q)
                    .count();
                assert_eq!(n, 1, "{s:?} {q:?}");
            }
        }
    }

    #[test]
    fn quality_follows_the_weights() {
        let w = [1.0, 1.0, 1.0, 1.0, 1.0];
        assert_eq!(pick_quality(&w, 0.0), Quality::Poor);
        assert_eq!(pick_quality(&w, 0.39), Quality::Common);
        assert_eq!(pick_quality(&w, 0.99), Quality::Epic);
        // A zero weight is never picked, even at the edge.
        let only_green = [0.0, 0.0, 1.0, 0.0, 0.0];
        assert_eq!(pick_quality(&only_green, 0.0), Quality::Uncommon);
        assert_eq!(pick_quality(&only_green, 0.999_999), Quality::Uncommon);
    }

    #[test]
    fn the_best_item_per_slot_is_worn() {
        let mut gear = Gear::default();
        assert!(gear.wear(item(Slot::Head, Quality::Common, 3.0, Stat::Might)));
        assert!(!gear.wear(item(Slot::Head, Quality::Poor, 1.0, Stat::Might)));
        assert!(gear.wear(item(Slot::Head, Quality::Rare, 5.0, Stat::Might)));
        assert!((gear.total().might - 0.1).abs() < 1e-6);
    }

    #[test]
    fn a_tie_goes_to_the_newer() {
        let mut gear = Gear::default();
        let mut old = item(Slot::Ring, Quality::Common, 2.0, Stat::Crit);
        old.name = "old".into();
        let mut new = item(Slot::Ring, Quality::Common, 2.0, Stat::Crit);
        new.name = "new".into();
        gear.wear(old);
        assert!(gear.wear(new));
        let worn = gear.worn[Slot::Ring as usize].as_ref();
        assert_eq!(worn.map(|i| i.name.as_str()), Some("new"));
    }

    #[test]
    fn the_bank_keeps_the_highest_quality_then_points() {
        let mut gear = Gear::default();
        gear.wear(item(Slot::Head, Quality::Rare, 3.0, Stat::Might));
        gear.wear(item(Slot::Feet, Quality::Rare, 5.0, Stat::Speed));
        gear.wear(item(Slot::Ring, Quality::Uncommon, 9.0, Stat::Crit));
        let best = gear.best().unwrap();
        assert_eq!(best.slot, Slot::Feet);
        assert!(Gear::default().best().is_none());
    }

    #[test]
    fn rolled_items_scale_with_quality_and_the_run() {
        let mut rng = Rng::default();
        for q in Quality::ALL {
            let early = roll_item(&mut rng, q, 0.0);
            assert_eq!(early.quality, q);
            assert!(early.power() >= q.points() * 0.85 - 1e-4);
            assert!(early.power() <= q.points() * 1.15 + 1e-4);
            let late = roll_item(&mut rng, q, 1.0);
            assert!(late.power() >= q.points() * 0.85 * (1.0 + LATE_BONUS) - 1e-4);
        }
    }

    #[test]
    fn the_bank_survives_a_round_trip_through_toml() {
        let bank = Bank {
            item: Some(roll_item(&mut Rng::default(), Quality::Epic, 0.5)),
        };
        let text = toml::to_string(&bank).unwrap();
        let back: Bank = toml::from_str(&text).unwrap();
        assert_eq!(back.item, bank.item);
        let empty: Bank = toml::from_str("").unwrap();
        assert!(empty.item.is_none());
    }
}
