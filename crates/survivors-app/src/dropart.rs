//! How a drop on the ground looks: real WoW 1.12 models from `GameObjectDisplayInfo.dbc` that
//! hover and turn, instead of a Bevy cylinder with a ball.
//!
//! Experience is cave crystals (a purple gem for the big ones), gold a heap of coins, health a
//! turkey leg (a battleground rune from a boss), gear a sword, a breastplate, a jewel or a bag,
//! and the power-up shards a rune each. Gear from green up, and every chest, stands in a column
//! of light ([`Column`]) whose colour and height follow the quality. A drop is a GameObject
//! ([`ObjLook`]), so the host replicates it and a co-op guest sees it as well.
//!
//! Hovering and turning is a picture offset and nothing else ([`Hover`]): [`apply_hover`] adds it
//! between `Update` and the transform propagation, [`restore_hover`] takes it back in `First`.
//! Pick-up distances, the nav scan and the co-op replication (`NetPos` is written from the
//! transform) only ever see the drop where it lies.
//!
//! The models are the hull-free ones of the drop-model table in the boss, guard and drop plan
//! (annex 1), except the chests, which keep theirs. A hull a drop's model carries all the same
//! ([`strip_hulls`]) is taken off again.

use avian3d::prelude::{Collider, RigidBody};
use benilla_world::schedule::WorldStage;
use bevy::prelude::*;
use bevy::transform::TransformSystems;

use super::chests::ChestTier;
use super::enemies::{Gem, GemKind};
use super::items::{Quality, Slot};
use super::units::{self, ObjLook, Still};
use super::Run;

/// Radians a hovering drop turns per second.
const SPIN: f32 = 1.1;
/// Yards a hovering drop bobs up and down around its height.
const BOB: f32 = 0.1;
/// Radians per second of the bob.
const BOB_RATE: f32 = 2.0;
/// Yards `enemies::spawn_gem` lifts a gem off the ground already; a gem hovers that much less.
const GEM_LIFT: f32 = 0.5;

/// A model a drop wears: its `GameObjectDisplayInfo` row, the scale to draw it at, how high it
/// floats over the ground (yards) and how fast it turns (radians per second, 0 lies still).
#[derive(Clone, Copy, Debug, PartialEq)]
struct Model {
    display: u32,
    scale: f32,
    lift: f32,
    spin: f32,
}

const fn model(display: u32, scale: f32, lift: f32, spin: f32) -> Model {
    Model {
        display,
        scale,
        lift,
        spin,
    }
}

// Experience. `CaveMineCrystalFormation01.mdx`, `..03.mdx` and `MuseumGem01.mdx`.
const CRYSTAL_SMALL: Model = model(5091, 0.6, 0.5, SPIN);
const CRYSTAL_MID: Model = model(5092, 0.7, 0.5, SPIN);
const GEM_BIG: Model = model(3511, 1.1, 0.7, SPIN);
// `TurkeyLeg.mdx`, and the battleground rune `PVP_Rune_Restoration.mdx` for a boss's heal.
const TURKEY: Model = model(114, 2.5, 0.5, SPIN);
const RUNE_HEAL: Model = model(5991, 0.6, 0.4, SPIN);
// The shards: `G_RuneBlue01.mdx`, `G_HolyLightWell.mdx` (stands) and `PVP_Rune_Invis.mdx`.
const RUNE_HEARTH: Model = model(342, 1.2, 0.8, SPIN);
const WELL: Model = model(6671, 0.6, 0.0, 0.0);
const RUNE_NOVA: Model = model(5932, 0.6, 0.4, SPIN);
// Gear: `HumanSword02.mdx`, `ArmorBreastplateBlue.mdx`, `MuseumGem01.mdx` (smaller than the
// experience gem) and `TugofWar_RedDustBag01.mdx` (a bag lies on the ground).
const SWORD: Model = model(670, 0.9, 0.6, SPIN);
const ARMOR: Model = model(4074, 2.2, 0.6, SPIN);
const JEWEL: Model = model(3511, 0.7, 0.7, SPIN);
const BAG: Model = model(6895, 0.8, 0.0, 0.0);
/// `GoldPilesmall01.mdx`, a flat heap about two yards across at scale 1; it lies.
const GOLD_DISPLAY: u32 = 2614;

/// The models that hover, by display.
const HOVERING: [Model; 9] = [
    CRYSTAL_SMALL,
    CRYSTAL_MID,
    GEM_BIG,
    TURKEY,
    RUNE_HEAL,
    RUNE_HEARTH,
    RUNE_NOVA,
    SWORD,
    ARMOR,
];

/// Every display a drop wears but a chest: the ones whose hull [`strip_hulls`] takes off.
const DROP_DISPLAYS: [u32; 15] = [
    5091,
    5092,
    3511,
    114,
    5991,
    342,
    6671,
    5932,
    670,
    4074,
    6895,
    GOLD_DISPLAY,
    3612,
    298,
    2234,
];

/// A column of light over a drop: an `Aura*.mdx` particle model of `height` yards.
#[derive(Clone, Copy, Debug, PartialEq)]
struct ColumnLook {
    display: u32,
    height: f32,
}

/// `AuraYellow.mdx`: every chest.
const CHEST_COLUMN: ColumnLook = ColumnLook {
    display: 1310,
    height: 3.1,
};

pub(super) fn plugin(app: &mut App) {
    let apply = apply_hover.before(TransformSystems::Propagate);
    // The co-op host reads the transforms into the replicated positions before it sends; the
    // offset must not be in them.
    #[cfg(all(feature = "coop", not(target_arch = "wasm32")))]
    let apply = apply.after(bevy_replicon::prelude::ServerSystems::Send);
    app.add_systems(First, restore_hover)
        .add_systems(
            Update,
            (dress, hover_new, strip_hulls, follow_columns)
                .chain()
                .after(WorldStage::Input),
        )
        .add_systems(PostUpdate, apply);
}

/// What a drop on the ground is, for its look: a piece of gear, a power-up shard or a chest.
/// Set by [`super::items`], [`super::pickups`] and [`super::chests`] on the drop's entity.
#[derive(Component, Clone, Copy, Debug, PartialEq)]
pub(super) enum DropArt {
    Item { slot: Slot, quality: Quality },
    Shard(ShardArt),
    Chest(ChestTier),
}

/// The power-up a shard holds ([`super::pickups`]).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum ShardArt {
    Hearthstone,
    Intervention,
    Nova,
}

impl ShardArt {
    fn model(self) -> Model {
        match self {
            ShardArt::Hearthstone => RUNE_HEARTH,
            ShardArt::Intervention => WELL,
            ShardArt::Nova => RUNE_NOVA,
        }
    }
}

/// The model a piece of gear for `slot` lies as.
fn item_model(slot: Slot) -> Model {
    match slot {
        Slot::Weapon => SWORD,
        Slot::Head | Slot::Shoulder | Slot::Chest | Slot::Legs => ARMOR,
        Slot::Neck | Slot::Ring | Slot::Trinket => JEWEL,
        Slot::Back | Slot::Waist | Slot::Hands | Slot::Feet => BAG,
    }
}

/// The column over gear of `quality`: none for grey and white, then green, blue and purple,
/// each taller than the one before.
fn column_for(quality: Quality) -> Option<ColumnLook> {
    // `AuraGreen.mdx`, `AuraBlueTall.mdx` and `AuraPurpleVeryTall.mdx`.
    match quality {
        Quality::Poor | Quality::Common => None,
        Quality::Uncommon => Some(ColumnLook {
            display: 3612,
            height: 3.1,
        }),
        Quality::Rare => Some(ColumnLook {
            display: 298,
            height: 6.8,
        }),
        Quality::Epic => Some(ColumnLook {
            display: 2234,
            height: 12.0,
        }),
    }
}

/// The `GameObjectDisplayInfo` row and scale a chest of `tier` wears.
pub(super) fn chest_look(tier: ChestTier) -> (u32, f32) {
    match tier {
        // `TreasureChest01.mdx`, as before.
        ChestTier::Elite => (259, 1.0),
        // `ChestActive01.mdx`, a sparkling chest.
        ChestTier::Boss => (1447, 0.8),
        // `G_ChestAhnQiraj.mdx`.
        ChestTier::Finale => (6502, 1.6),
        // `Chest03.mdx`, an iron-bound one.
        ChestTier::Post => (41, 1.8),
    }
}

/// The `GameObjectDisplayInfo` row and scale a pickup wears: a crystal per experience tier, the
/// bigger tiers rarer, a turkey leg for health (a rune for a boss's) and a pile of gold that
/// grows with its worth.
pub(super) fn gem_look(kind: GemKind) -> (u32, f32) {
    let m = match kind {
        GemKind::Heal(hp) if hp >= 100.0 => RUNE_HEAL,
        GemKind::Heal(_) => TURKEY,
        GemKind::Gold(g) => {
            let scale = if g >= 100 {
                1.4
            } else if g >= 25 {
                0.9
            } else if g >= 6 {
                0.6
            } else {
                0.45
            };
            model(GOLD_DISPLAY, scale, 0.0, 0.0)
        }
        GemKind::Xp(xp) if xp >= 20 => GEM_BIG,
        GemKind::Xp(xp) if xp >= 5 => CRYSTAL_MID,
        GemKind::Xp(_) => CRYSTAL_SMALL,
    };
    (m.display, m.scale)
}

/// The column of light of the drop `of`: its own GameObject, so a co-op guest sees it too. It
/// goes when its drop does ([`follow_columns`]).
#[derive(Component)]
struct Column {
    of: Entity,
}

/// A new drop becomes its model, and gear and chests get their column.
fn dress(
    mut commands: Commands,
    mut run: ResMut<Run>,
    fresh: Query<(Entity, &DropArt, &Transform), Added<DropArt>>,
) {
    for (e, art, tf) in &fresh {
        let at = tf.translation;
        let column = match *art {
            DropArt::Item { slot, quality } => {
                let m = item_model(slot);
                commands
                    .entity(e)
                    .insert(units::object_bundle(run.guid(), m.display, m.scale, at));
                column_for(quality)
            }
            DropArt::Shard(shard) => {
                let m = shard.model();
                commands
                    .entity(e)
                    .insert(units::object_bundle(run.guid(), m.display, m.scale, at));
                None
            }
            // A chest is spawned as its model already ([`super::chests::spawn_chest`]).
            DropArt::Chest(_) => Some(CHEST_COLUMN),
        };
        if let Some(c) = column {
            commands.spawn((
                units::object_bundle(run.guid(), c.display, 1.0, at),
                Column { of: e },
            ));
        }
    }
}

/// A column stands where its drop lies and goes when the drop is picked up, opened or gone.
fn follow_columns(
    mut commands: Commands,
    mut columns: Query<(Entity, &Column, &mut Transform), Without<DropArt>>,
    drops: Query<&Transform, With<DropArt>>,
) {
    for (e, column, mut tf) in &mut columns {
        match drops.get(column.of) {
            Ok(drop) => {
                if tf.translation != drop.translation {
                    tf.translation = drop.translation;
                }
            }
            Err(_) => commands.entity(e).despawn(),
        }
    }
}

/// A drop that carries a hull gets rid of it: a gem, a heap of gold or a bag is a model to look
/// at, not a wall to walk into. Runs on the host and on the guest alike.
fn strip_hulls(mut commands: Commands, fresh: Query<(Entity, &ObjLook), Added<Collider>>) {
    for (e, look) in &fresh {
        if DROP_DISPLAYS.contains(&look.display) {
            commands.entity(e).remove::<(RigidBody, Collider)>();
        }
    }
}

/// A drop that floats and turns: the picture offset [`apply_hover`] adds to its transform each
/// frame and [`restore_hover`] takes off again. Set from the model's display when the drop's
/// [`ObjLook`] appears, so a co-op guest's copy hovers as well.
#[derive(Component, Clone, Copy, Debug)]
pub(super) struct Hover {
    /// Radians per second.
    spin: f32,
    /// Yards the bob swings either way.
    bob: f32,
    /// Yards above where the drop lies.
    lift: f32,
    /// Radians, so the drops do not rise and fall together.
    phase: f32,
    /// What the last [`Hover::apply`] did, until [`Hover::restore`] undoes it.
    shown: Option<Shown>,
}

/// The transform before an offset was added, and the one written.
#[derive(Clone, Copy, Debug)]
struct Shown {
    pos: Vec3,
    rot: Quat,
    wrote_pos: Vec3,
    wrote_rot: Quat,
}

impl Hover {
    fn new(m: &Model, gem: bool, seed: u64) -> Self {
        Hover {
            spin: m.spin,
            bob: BOB,
            // A gem is lifted off the ground already.
            lift: if gem {
                (m.lift - GEM_LIFT).max(0.0)
            } else {
                m.lift
            },
            phase: (seed % 628) as f32 / 100.0,
            shown: None,
        }
    }

    /// The lift and the turn at `t` seconds.
    fn offset(&self, t: f32) -> (Vec3, Quat) {
        let up = self.lift + self.bob * (t * BOB_RATE + self.phase).sin();
        (
            Vec3::Y * up,
            Quat::from_rotation_y(self.spin * t + self.phase),
        )
    }

    /// Add the offset at `t` to `tf`, remembering what was there.
    fn apply(&mut self, tf: &mut Transform, t: f32) {
        let (up, turn) = self.offset(t);
        let (pos, rot) = (tf.translation, tf.rotation);
        tf.translation = pos + up;
        tf.rotation = rot * turn;
        self.shown = Some(Shown {
            pos,
            rot,
            wrote_pos: tf.translation,
            wrote_rot: tf.rotation,
        });
    }

    /// Take the offset off `tf` again. A part somebody else wrote since (the co-op guest's
    /// smoothing, a drop that was moved) is theirs now and stays.
    fn restore(&mut self, tf: &mut Transform) {
        if let Some(s) = self.shown.take() {
            if tf.translation == s.wrote_pos {
                tf.translation = s.pos;
            }
            if tf.rotation == s.wrote_rot {
                tf.rotation = s.rot;
            }
        }
    }
}

/// A drop whose model hovers gets its [`Hover`] (on the host, alone and on the guest).
fn hover_new(
    mut commands: Commands,
    fresh: Query<(Entity, &ObjLook, Has<Still>), (Added<ObjLook>, Without<Hover>)>,
) {
    for (e, look, gem) in &fresh {
        let Some(m) = HOVERING.iter().find(|m| m.display == look.display) else {
            continue;
        };
        let mut entity = commands.entity(e);
        entity.insert(Hover::new(m, gem, e.to_bits()));
        // The boss's rune is all particles: held still, it would not shine.
        if m.display == RUNE_HEAL.display {
            entity.remove::<Still>();
        }
    }
}

/// `First`: the offset of the last frame comes off, so every system of this frame sees the drop
/// where it lies.
fn restore_hover(mut drops: Query<(&mut Transform, &mut Hover)>) {
    for (mut tf, mut hover) in &mut drops {
        if hover.shown.is_some() {
            hover.restore(&mut tf);
        }
    }
}

/// `PostUpdate`, before the transforms propagate: float and turn. A gem on its way to the hero
/// has stopped hovering.
fn apply_hover(
    time: Res<Time<Real>>,
    mut drops: Query<(&mut Transform, &mut Hover, Option<&Gem>)>,
) {
    let t = time.elapsed_secs();
    for (mut tf, mut hover, gem) in &mut drops {
        if gem.is_some_and(|g| g.pulled) {
            continue;
        }
        hover.apply(&mut tf, t);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_quality_and_slot_has_a_look() {
        for slot in Slot::ALL {
            let m = item_model(slot);
            assert!(m.scale > 0.0, "{slot:?}");
            assert!(DROP_DISPLAYS.contains(&m.display), "{slot:?}");
        }
        for shard in [
            ShardArt::Hearthstone,
            ShardArt::Intervention,
            ShardArt::Nova,
        ] {
            let m = shard.model();
            assert!(m.scale > 0.0, "{shard:?}");
            assert!(DROP_DISPLAYS.contains(&m.display), "{shard:?}");
        }
        for quality in Quality::ALL {
            // Grey and white lie without a column, green and up stand in one.
            assert_eq!(column_for(quality).is_some(), quality >= Quality::Uncommon);
            if let Some(c) = column_for(quality) {
                assert!(DROP_DISPLAYS.contains(&c.display), "{quality:?}");
            }
        }
        for kind in [
            GemKind::Xp(1),
            GemKind::Xp(5),
            GemKind::Xp(20),
            GemKind::Heal(30.0),
            GemKind::Heal(1000.0),
            GemKind::Gold(1),
            GemKind::Gold(6),
            GemKind::Gold(25),
            GemKind::Gold(100),
        ] {
            let (display, scale) = gem_look(kind);
            assert!(scale > 0.0, "{kind:?}");
            assert!(DROP_DISPLAYS.contains(&display), "{kind:?}");
        }
        for tier in [
            ChestTier::Elite,
            ChestTier::Boss,
            ChestTier::Finale,
            ChestTier::Post,
        ] {
            let (display, scale) = chest_look(tier);
            assert!(scale > 0.0, "{tier:?}");
            // A chest keeps its hull: it is not one of the drops whose hull comes off.
            assert!(!DROP_DISPLAYS.contains(&display), "{tier:?}");
        }
    }

    #[test]
    fn better_items_stand_in_taller_columns() {
        let height = |q| column_for(q).map_or(0.0, |c| c.height);
        assert_eq!(height(Quality::Poor), 0.0);
        assert_eq!(height(Quality::Common), 0.0);
        assert!(height(Quality::Uncommon) > 0.0);
        assert!(height(Quality::Uncommon) < height(Quality::Rare));
        assert!(height(Quality::Rare) < height(Quality::Epic));
    }

    #[test]
    fn hover_offsets_cancel_out() {
        let m = SWORD;
        let mut hover = Hover::new(&m, false, 7);
        let start = Transform::from_xyz(1.0, 2.0, 3.0).with_rotation(Quat::from_rotation_y(0.3));
        let mut tf = start;
        hover.apply(&mut tf, 1.7);
        assert_ne!(tf.translation, start.translation);
        assert_ne!(tf.rotation, start.rotation);
        hover.restore(&mut tf);
        assert_eq!(tf, start);

        // Restoring twice, or with nothing applied, changes nothing.
        hover.restore(&mut tf);
        assert_eq!(tf, start);

        // Somebody moved the drop in between: the move stays, only what is still ours comes off.
        hover.apply(&mut tf, 2.9);
        let moved = Vec3::new(8.0, 2.0, -4.0);
        tf.translation = moved;
        hover.restore(&mut tf);
        assert_eq!(tf.translation, moved);
        assert_eq!(tf.rotation, start.rotation);

        // Many frames in a row do not drift.
        let mut tf = start;
        for frame in 0..500 {
            hover.restore(&mut tf);
            hover.apply(&mut tf, frame as f32 * 0.016);
        }
        hover.restore(&mut tf);
        assert_eq!(tf, start);
    }

    #[test]
    fn a_gem_hovers_less_and_a_lying_model_not_at_all() {
        let ground = Hover::new(&JEWEL, false, 0);
        let gem = Hover::new(&JEWEL, true, 0);
        assert!((ground.lift - gem.lift - GEM_LIFT).abs() < 1e-6);
        // A crystal lifted by the gem already floats only by its bob.
        assert_eq!(Hover::new(&CRYSTAL_SMALL, true, 0).lift, 0.0);
        // The bag, the well and the gold lie or stand: no hover for them.
        for m in [BAG, WELL, model(GOLD_DISPLAY, 1.0, 0.0, 0.0)] {
            assert!(!HOVERING.iter().any(|h| h.display == m.display));
        }
    }

    #[test]
    fn every_drop_display_is_in_the_client_data() {
        let data = benilla_formats::wow_data_or_skip!();
        let mut chain = benilla_formats::Chain::open(&data).unwrap();
        let catalog = benilla_formats::load_gameobject_catalog(&mut chain).unwrap();
        let mut displays: Vec<u32> = DROP_DISPLAYS.to_vec();
        displays.push(CHEST_COLUMN.display);
        for tier in [
            ChestTier::Elite,
            ChestTier::Boss,
            ChestTier::Finale,
            ChestTier::Post,
        ] {
            displays.push(chest_look(tier).0);
        }
        for display in displays {
            let model = catalog
                .model_path(display)
                .unwrap_or_else(|| panic!("no GameObjectDisplayInfo row {display}"));
            let stem = model.rsplit_once('.').map_or(model, |(stem, _)| stem);
            let m2 = format!("{stem}.m2");
            assert!(
                chain.read(&m2).is_ok() || chain.read(model).is_ok(),
                "display {display}: {model} is not in the data"
            );
        }
    }
}
