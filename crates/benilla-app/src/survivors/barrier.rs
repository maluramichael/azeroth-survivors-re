//! The edge of the battleground as a magical barrier. The hero cannot leave the circle of radius
//! `leash` around the arena centre ([`super::hero::drive_hero`]); this stands a violet-blue energy
//! wall on that circle, on the terrain, faint from afar and bright where the hero closes in.
//!
//! One unlit, alpha-blended, double-sided material is shared by a ring of wall segments. A
//! segment is a four-vertex strip whose vertex colours carry the glow, so the brightness follows
//! the hero smoothly without a material per segment; the heights come from the terrain as it
//! streams in. The pattern is generated here (a rune-like lattice that fades up from the ground)
//! and drifts along the wall.

use benilla_world::schedule::WorldStage;
use benilla_world::world_point::WorldPoint;
use bevy::asset::RenderAssetUsages;
use bevy::camera::visibility::NoFrustumCulling;
use bevy::image::{ImageAddressMode, ImageFilterMode, ImageSampler, ImageSamplerDescriptor};
use bevy::math::Affine2;
use bevy::mesh::{Indices, PrimitiveTopology, VertexAttributeValues};
use bevy::prelude::*;
use bevy::render::render_resource::{Extent3d, TextureDimension, TextureFormat};

use super::hero::{arena, Hero};
use super::{Phase, RunEntity, Selection};

/// How far above the ground the wall stands, and how far it sinks below, in yards.
const HEIGHT: f32 = 14.0;
const SINK: f32 = 2.5;
/// Yards of wall one copy of the pattern covers.
const TILE: f32 = 8.0;
/// Within this many yards of the hero the wall brightens.
const NEAR: f32 = 24.0;
/// The opacity the far wall keeps, before the pattern's own transparency.
const FAINT: f32 = 0.35;
/// Drift of the pattern along the wall, tiles per second.
const DRIFT: f32 = 0.03;
/// Seconds between terrain re-samples of the ring.
const RESAMPLE: f32 = 0.5;
/// Seconds between flashes while the hero keeps pressing on the wall.
const NUDGE: f32 = 0.8;
/// Colour of the far wall and of the wall at the hero's side, linear.
const FAR_TINT: [f32; 3] = [0.30, 0.18, 1.0];
const NEAR_TINT: [f32; 3] = [0.65, 0.85, 1.0];

pub(super) fn plugin(app: &mut App) {
    app.add_systems(
        Update,
        (build_barrier, tend_barrier)
            .chain()
            .after(WorldStage::Input)
            .run_if(not(in_state(Phase::Menu))),
    );
}

/// The ring of wall segments and what it shows right now. Lives on the root entity of the wall,
/// which carries [`RunEntity`]; the segments are its children.
#[derive(Component)]
struct Barrier {
    /// The battleground it was built for ([`Selection::stage`]).
    stage: usize,
    center: Vec3,
    radius: f32,
    /// Terrain height at each ring point, `None` until its tiles have streamed in.
    ground: Vec<Option<f32>>,
    /// The height an unsampled point stands at: where the hero was when the wall went up.
    base: f32,
    /// One mesh per segment; segment `i` runs from ring point `i` to point `i + 1`.
    segments: Vec<Handle<Mesh>>,
    /// The `(opacity, glow)` last written to each ring point's vertices.
    shown: Vec<(f32, f32)>,
    material: Handle<StandardMaterial>,
    /// 1 right after the hero hits the wall, fading out.
    flash: f32,
    touching: bool,
    nudge: f32,
    resample: f32,
}

impl Barrier {
    fn count(&self) -> usize {
        self.segments.len()
    }

    /// Ring point `i` on the ground plane.
    fn point(&self, i: usize) -> Vec3 {
        let a = (i % self.count()) as f32 / self.count() as f32 * std::f32::consts::TAU;
        self.center + Vec3::new(a.cos(), 0.0, a.sin()) * self.radius
    }

    fn ground_at(&self, i: usize) -> f32 {
        self.ground[i % self.count()].unwrap_or(self.base)
    }

    /// Write segment `i`'s positions and glow into `mesh`, in place.
    fn write_segment(&self, mesh: &mut Mesh, i: usize) {
        let n = self.count();
        let j = (i + 1) % n;
        let (h0, h1) = (self.ground_at(i), self.ground_at(j));
        let (p0, p1) = (self.point(i), self.point(j));
        if let Some(VertexAttributeValues::Float32x3(v)) =
            mesh.attribute_mut(Mesh::ATTRIBUTE_POSITION)
        {
            v[0] = [p0.x, h0 - SINK, p0.z];
            v[1] = [p1.x, h1 - SINK, p1.z];
            v[2] = [p1.x, h1 + HEIGHT, p1.z];
            v[3] = [p0.x, h0 + HEIGHT, p0.z];
        }
        if let Some(VertexAttributeValues::Float32x4(c)) =
            mesh.attribute_mut(Mesh::ATTRIBUTE_COLOR)
        {
            let (a, b) = (tint(self.shown[i]), tint(self.shown[j]));
            c[0] = a;
            c[1] = b;
            c[2] = b;
            c[3] = a;
        }
    }
}

/// Vertex colour for a ring point's `(opacity, glow)`.
fn tint((alpha, glow): (f32, f32)) -> [f32; 4] {
    let mix = |i: usize| FAR_TINT[i] + (NEAR_TINT[i] - FAR_TINT[i]) * glow;
    [mix(0), mix(1), mix(2), alpha]
}

/// Opacity and glow of a ring point at `dist` yards from the hero, with the hit flash on top.
fn glow_at(dist: f32, flash: f32) -> (f32, f32) {
    let t = (1.0 - dist / NEAR).clamp(0.0, 1.0);
    let near = t * t * (3.0 - 2.0 * t);
    let alpha = (FAINT + (1.0 - FAINT) * near + 0.4 * flash * near).min(1.0);
    (alpha, (near * (1.0 + 0.8 * flash)).min(1.0))
}

/// A rune-like lattice of fine diamond lines over a soft fill, bright at the foot and dissolving
/// toward the top. Tiles horizontally; white, so the vertex colours give the hue.
fn pattern() -> Image {
    const W: usize = 128;
    const H: usize = 128;
    let line = |x: f32| {
        let d = (x - x.round()).abs();
        (1.0 - d / 0.07).clamp(0.0, 1.0)
    };
    let mut data = Vec::with_capacity(W * H * 4);
    for py in 0..H {
        let y = (py as f32 + 0.5) / H as f32;
        let fade = 0.1 + 0.9 * y.powf(1.5);
        for px in 0..W {
            let x = (px as f32 + 0.5) / W as f32;
            let lattice = line(x * 2.0 + y * 3.0).max(line(x * 2.0 - y * 3.0));
            let rune = line(x * 4.0) * 0.5;
            let foot = ((y - 0.9) / 0.1).clamp(0.0, 1.0);
            let rim = (1.0 - y / 0.025).clamp(0.0, 1.0);
            let a = ((0.35 + 0.65 * lattice.max(rune)) * fade + foot * 0.5 + rim * 0.6).min(1.0);
            data.extend_from_slice(&[255, 255, 255, (a * 255.0) as u8]);
        }
    }
    let mut image = Image::new(
        Extent3d {
            width: W as u32,
            height: H as u32,
            depth_or_array_layers: 1,
        },
        TextureDimension::D2,
        data,
        TextureFormat::Rgba8UnormSrgb,
        RenderAssetUsages::default(),
    );
    image.sampler = ImageSampler::Descriptor(ImageSamplerDescriptor {
        address_mode_u: ImageAddressMode::Repeat,
        address_mode_v: ImageAddressMode::ClampToEdge,
        mag_filter: ImageFilterMode::Linear,
        min_filter: ImageFilterMode::Linear,
        mipmap_filter: ImageFilterMode::Linear,
        ..default()
    });
    image
}

/// A segment's mesh with its layout in place; [`Barrier::write_segment`] fills in the numbers.
fn blank_segment(i: usize, step: f32) -> Mesh {
    let (u0, u1) = (i as f32 * step / TILE, (i + 1) as f32 * step / TILE);
    let mut mesh = Mesh::new(
        PrimitiveTopology::TriangleList,
        RenderAssetUsages::default(),
    );
    mesh.insert_attribute(Mesh::ATTRIBUTE_POSITION, vec![[0.0f32; 3]; 4]);
    mesh.insert_attribute(Mesh::ATTRIBUTE_NORMAL, vec![[0.0f32, 1.0, 0.0]; 4]);
    mesh.insert_attribute(
        Mesh::ATTRIBUTE_UV_0,
        vec![[u0, 1.0f32], [u1, 1.0], [u1, 0.0], [u0, 0.0]],
    );
    mesh.insert_attribute(Mesh::ATTRIBUTE_COLOR, vec![[0.0f32; 4]; 4]);
    mesh.insert_indices(Indices::U32(vec![0, 1, 2, 0, 2, 3]));
    mesh
}

/// Raise the wall when a run is in its battleground, and replace it when the battleground
/// changes. The run's end despawns it with the other [`RunEntity`]s.
fn build_barrier(
    mut commands: Commands,
    selection: Res<Selection>,
    loading: Res<crate::loading_screen::LoadingScreen>,
    hero: Query<&Transform, With<Hero>>,
    existing: Query<(Entity, &Barrier)>,
    world: WorldPoint,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut images: ResMut<Assets<Image>>,
) {
    let stale = existing.iter().any(|(_, b)| b.stage != selection.stage);
    if !stale && !existing.is_empty() {
        return;
    }
    // The ground under the hero is only trustworthy once the loading screen has lifted.
    let (Ok(hero), false) = (hero.single(), loading.covering()) else {
        return;
    };
    for (e, b) in &existing {
        if b.stage != selection.stage {
            commands.entity(e).despawn();
        }
    }
    if stale {
        // The old wall goes this frame; the new one is raised next.
        return;
    }
    let map = selection.stage_def();
    let center = arena(map);
    let radius = map.leash;
    let n = ((std::f32::consts::TAU * radius / 12.0) as usize).clamp(24, 48);
    let step = std::f32::consts::TAU * radius / n as f32;
    let material = materials.add(StandardMaterial {
        base_color: Color::WHITE,
        base_color_texture: Some(images.add(pattern())),
        unlit: true,
        alpha_mode: AlphaMode::Blend,
        cull_mode: None,
        double_sided: true,
        fog_enabled: false,
        ..default()
    });
    let mut barrier = Barrier {
        stage: selection.stage,
        center,
        radius,
        ground: Vec::new(),
        base: hero.translation.y,
        segments: Vec::new(),
        shown: vec![glow_at(f32::MAX, 0.0); n],
        material: material.clone(),
        flash: 0.0,
        touching: false,
        nudge: 0.0,
        resample: RESAMPLE,
    };
    // `point` and `ground_at` need the ring's size before the segments exist.
    barrier.segments = vec![Handle::default(); n];
    barrier.ground = (0..n)
        .map(|i| {
            let p = barrier.point(i);
            world.terrain_height_under(Vec3::new(p.x, hero.translation.y, p.z))
        })
        .collect();
    for i in 0..n {
        let mut mesh = blank_segment(i, step);
        barrier.write_segment(&mut mesh, i);
        barrier.segments[i] = meshes.add(mesh);
    }
    let handles = barrier.segments.clone();
    commands
        .spawn((
            barrier,
            Transform::default(),
            Visibility::default(),
            RunEntity,
        ))
        .with_children(|root| {
            for mesh in handles {
                root.spawn((
                    Mesh3d(mesh),
                    MeshMaterial3d(material.clone()),
                    Transform::default(),
                    // The vertices move with the terrain; the stored bounds would not.
                    NoFrustumCulling,
                ));
            }
        });
}

/// Brighten the wall around the hero, flash it when he is held back, follow the terrain as it
/// streams in and drift the pattern.
fn tend_barrier(
    time: Res<Time<Real>>,
    phase: Res<State<Phase>>,
    hero: Query<&Transform, With<Hero>>,
    mut barrier: Query<&mut Barrier>,
    world: WorldPoint,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
) {
    let Ok(mut barrier) = barrier.single_mut() else {
        return;
    };
    let b = &mut *barrier;
    let dt = time.delta_secs();
    let hero = hero.single().ok().map(|t| t.translation);

    if let Some(m) = materials.get_mut(&b.material) {
        let t = time.elapsed_secs();
        m.uv_transform = Affine2::from_translation(Vec2::new((t * DRIFT).fract(), 0.0));
        m.base_color = Color::WHITE.with_alpha(0.88 + 0.12 * (t * 1.7).sin());
    }

    // Held against the wall: flash on contact, and again now and then while he keeps pressing.
    let touching = *phase.get() == Phase::Playing
        && hero.is_some_and(|h| {
            Vec2::new(h.x - b.center.x, h.z - b.center.z).length() >= b.radius - 0.05
        });
    if touching && !b.touching {
        b.flash = 1.0;
        b.nudge = NUDGE;
    } else if touching {
        b.nudge -= dt;
        if b.nudge <= 0.0 {
            b.flash = b.flash.max(0.6);
            b.nudge = NUDGE;
        }
    }
    b.touching = touching;
    b.flash = (b.flash - dt * 2.0).max(0.0);

    // One bit per segment that has to be rewritten; at most 48 segments.
    let n = b.count();
    let mut dirty = 0u64;
    let mut mark = |i: usize| dirty |= 1 << i | 1 << ((i + n - 1) % n);

    b.resample -= dt;
    if b.resample <= 0.0 {
        b.resample = RESAMPLE;
        let y = hero.map_or(b.base, |h| h.y);
        for i in 0..n {
            let p = b.point(i);
            if let Some(h) = world.terrain_height_under(Vec3::new(p.x, y, p.z)) {
                if b.ground[i].is_none_or(|g| (g - h).abs() > 0.05) {
                    b.ground[i] = Some(h);
                    mark(i);
                }
            }
        }
    }

    for i in 0..n {
        let dist = hero.map_or(f32::MAX, |h| {
            let p = b.point(i);
            Vec2::new(p.x - h.x, p.z - h.z).length()
        });
        let now = glow_at(dist, b.flash);
        let was = b.shown[i];
        if (now.0 - was.0).abs() > 0.004 || (now.1 - was.1).abs() > 0.004 {
            b.shown[i] = now;
            mark(i);
        }
    }

    for i in (0..n).filter(|i| dirty >> i & 1 == 1) {
        if let Some(mesh) = meshes.get_mut(&b.segments[i]) {
            b.write_segment(mesh, i);
        }
    }
}
