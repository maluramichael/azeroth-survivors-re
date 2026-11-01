//! How a drop on the ground looks: the experience, gold and health a kill leaves ([`gem_look`]),
//! and the gear, shards and chests marked with a [`DropArt`] by the module that spawns them.
//!
//! For now every [`DropArt`] is lit as before, by [`Beacons`]: a tall, faint beam and a glowing
//! orb in one colour ([`dress`]). The drop-models package replaces that with WoW 1.12 models from
//! `GameObjectDisplayInfo.dbc` that hover and turn, a column of light in the quality's colour,
//! chests whose lids lift, and `strip_hulls` to take a collision hull off a drop.

use benilla_world::schedule::WorldStage;
use bevy::prelude::*;

use super::chests::ChestTier;
use super::enemies::GemKind;
use super::items::{Quality, Slot};

/// A chest's beam: gold, six yards tall.
const CHEST_COLOR: Color = Color::srgb(1.0, 0.8, 0.2);
const CHEST_BEAM: f32 = 6.0;
/// A shard's beam height.
const SHARD_BEAM: f32 = 6.0;

const BEAM_RADIUS: f32 = 0.14;
const ORB_RADIUS: f32 = 0.3;
const BEAM_ALPHA: f32 = 0.28;
const ORB_ALPHA: f32 = 0.9;
/// How high the orb floats over the ground.
const ORB_LIFT: f32 = 0.7;

pub(super) fn plugin(app: &mut App) {
    app.init_resource::<Beacons>()
        .add_systems(Update, dress.after(WorldStage::Input));
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
    fn color(self) -> Color {
        match self {
            ShardArt::Hearthstone => Color::srgb(0.35, 0.6, 1.0),
            ShardArt::Intervention => Color::srgb(1.0, 1.0, 0.8),
            ShardArt::Nova => Color::srgb(0.55, 0.9, 1.0),
        }
    }
}

impl DropArt {
    /// The beacon's colour and height in yards.
    fn beacon(self) -> (Color, f32) {
        match self {
            DropArt::Item { quality, .. } => (quality.color(), quality.beam()),
            DropArt::Shard(s) => (s.color(), SHARD_BEAM),
            DropArt::Chest(_) => (CHEST_COLOR, CHEST_BEAM),
        }
    }
}

/// The `GameObjectDisplayInfo` row and scale a pickup wears: a wisp per experience tier, the
/// bigger tiers rarer, a turkey leg for health and a pile of gold that grows with its worth.
pub(super) fn gem_look(kind: GemKind) -> (u32, f32) {
    match kind {
        // `TurkeyLeg.mdx`.
        GemKind::Heal(_) => (114, 2.5),
        // `GoldPilesmall01.mdx`, a flat heap about two yards across at scale 1.
        GemKind::Gold(g) if g >= 100 => (2614, 1.4),
        GemKind::Gold(g) if g >= 25 => (2614, 0.9),
        GemKind::Gold(g) if g >= 6 => (2614, 0.6),
        GemKind::Gold(_) => (2614, 0.45),
        // `BFD_WispSmallPurple.mdx`.
        GemKind::Xp(xp) if xp >= 20 => (1307, 1.0),
        // `BFD_WispSmallGreen.mdx`.
        GemKind::Xp(xp) if xp >= 5 => (1267, 1.0),
        // `BFD_WispSmall.mdx`.
        GemKind::Xp(_) => (426, 1.0),
    }
}

/// A new drop is lit with its beacon.
fn dress(
    mut commands: Commands,
    mut beacons: ResMut<Beacons>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    fresh: Query<(Entity, &DropArt), Added<DropArt>>,
) {
    for (e, art) in &fresh {
        let (color, height) = art.beacon();
        beacons.attach(&mut commands, &mut meshes, &mut materials, e, color, height);
    }
}

/// The meshes and materials the beacons share: one beam and one orb mesh, a material per colour.
#[derive(Resource, Default)]
pub(super) struct Beacons {
    meshes: Option<(Handle<Mesh>, Handle<Mesh>)>,
    /// Colour as bytes, whether it is the beam's (faint) material, and the material.
    materials: Vec<([u8; 3], bool, Handle<StandardMaterial>)>,
}

impl Beacons {
    /// Light `parent` with a beam `height` yards tall and an orb, both `color`.
    pub fn attach(
        &mut self,
        commands: &mut Commands,
        meshes: &mut Assets<Mesh>,
        materials: &mut Assets<StandardMaterial>,
        parent: Entity,
        color: Color,
        height: f32,
    ) {
        let (beam, orb) = self
            .meshes
            .get_or_insert_with(|| {
                (
                    meshes.add(Cylinder::new(BEAM_RADIUS, 1.0)),
                    meshes.add(Sphere::new(ORB_RADIUS)),
                )
            })
            .clone();
        let glass = self.material(materials, color, true);
        let solid = self.material(materials, color, false);
        commands.entity(parent).with_children(|p| {
            p.spawn((
                Mesh3d(beam),
                MeshMaterial3d(glass),
                Transform::from_xyz(0.0, height * 0.5, 0.0).with_scale(Vec3::new(1.0, height, 1.0)),
            ));
            p.spawn((
                Mesh3d(orb),
                MeshMaterial3d(solid),
                Transform::from_xyz(0.0, ORB_LIFT, 0.0),
            ));
        });
    }

    fn material(
        &mut self,
        materials: &mut Assets<StandardMaterial>,
        color: Color,
        glass: bool,
    ) -> Handle<StandardMaterial> {
        let rgba = color.to_srgba();
        let key = [rgba.red, rgba.green, rgba.blue].map(|c| (c * 255.0).round() as u8);
        if let Some((_, _, h)) = self
            .materials
            .iter()
            .find(|(k, g, _)| *k == key && *g == glass)
        {
            return h.clone();
        }
        let alpha = if glass { BEAM_ALPHA } else { ORB_ALPHA };
        let h = materials.add(StandardMaterial {
            base_color: Color::srgba(rgba.red, rgba.green, rgba.blue, alpha),
            unlit: true,
            alpha_mode: AlphaMode::Blend,
            cull_mode: None,
            fog_enabled: false,
            ..default()
        });
        self.materials.push((key, glass, h.clone()));
        h
    }
}
