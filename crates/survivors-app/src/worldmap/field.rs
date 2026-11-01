//! The battlefield up close in the run's map window: the ground from the minimap's art cut to
//! the arena, the barrier's ring, the hero's arrow and everything on the field.
//!
//! The ground is composed once per battleground ([`compose_ground`]) from the minimap tiles
//! (`md5translate.trs`, north up, 533.33 yd per 256-px tile) into a [`FIELD_PX`] square: the
//! arena's circle with a margin, dark outside the barrier and ringed in its colour. Ten times a
//! second ([`refresh_field`]) that ground is copied into the shown image and the field's dots
//! go on it (the horde in red, the creatures a quest hunts in yellow, drops in their colour);
//! the things with an icon (bosses, rares, elites, guard posts, chests, shrines, the portal, quest
//! items, the party, the hero's arrow) are UI nodes from a pool, moved and hidden as the field
//! changes. A marker sits in the ground image's frame, so one outside the view is not shown.

use std::collections::HashMap;
use std::ops::RangeInclusive;

use bevy::asset::RenderAssetUsages;
use bevy::platform::time::Instant;
use bevy::prelude::*;
use bevy::render::render_resource::{Extent3d, TextureDimension, TextureFormat};

use benilla_assets::coords::bevy_to_wow;
use benilla_assets::{LockRecover, MapCatalogRes, WorldAssets};
use benilla_formats::{load_minimap_translate, tile_to_world, MinimapTranslate};

use super::atlas::{self, at, cell_image, round};
use super::{loot_tint, FieldSlot, MapLevel, MapPin, WorldMapUi, BARRIER_TINT};
use crate::dropart::DropArt;
use crate::enemies::{Elite, Enemy};
use crate::hero::Hero;
use crate::kit::{self, KitFonts};
use crate::minimap::MapMarker;
use crate::quests::QuestLog;
use crate::Selection;

/// The ground image's edge in texels.
pub(super) const FIELD_PX: usize = 512;
/// The edge it is shown at, in canvas units.
const VIEW: f32 = 568.0;
const TILE_PX: usize = 256;
const TILE_YARDS: f32 = 533.333_3;
/// Minimap texels per yard.
const TEX_PER_YD: f32 = TILE_PX as f32 / TILE_YARDS;
/// What lies under a tile that was never authored: dark parchment (`srgb(0.16, 0.13, 0.09)`), 0..255.
const PARCHMENT: [f32; 3] = [40.8, 33.15, 22.95];
/// How bright the ground outside the barrier stays.
const OUTSIDE_LIGHT: f32 = 0.45;
/// The barrier's ring is this many texels to each side of the circle at full strength, then fades
/// out over [`RING_SOFT`] more.
const RING_HALF: f32 = 1.5;
const RING_SOFT: f32 = 1.0;
/// Seconds between refreshes of the dots and the icons.
const TICK: f32 = 0.1;

/// The ground and what it is made of.
#[derive(Resource, Default)]
struct Field {
    /// The bounds the ground was composed for ([`crate::route::Bounds::key`]): the battleground,
    /// and on a route the leg under way.
    key: Option<(usize, u8)>,
    translate: Option<MinimapTranslate>,
    /// `md5translate.trs` failed to load; it is not tried again.
    translate_failed: bool,
    /// Decoded 256² tiles by ADT index, `None` where no art was authored, for the map `dir` names.
    tiles: HashMap<(u32, u32), Option<Vec<u8>>>,
    dir: String,
    /// The composed ground, RGBA8, [`FIELD_PX`] square.
    base: Vec<u8>,
    image: Option<Handle<Image>>,
}

impl Field {
    /// The shown image, made the first time it is asked for.
    fn handle(&mut self, images: &mut Assets<Image>) -> Handle<Image> {
        self.image
            .get_or_insert_with(|| {
                images.add(Image::new(
                    Extent3d {
                        width: FIELD_PX as u32,
                        height: FIELD_PX as u32,
                        depth_or_array_layers: 1,
                    },
                    TextureDimension::D2,
                    vec![0; FIELD_PX * FIELD_PX * 4],
                    TextureFormat::Rgba8UnormSrgb,
                    RenderAssetUsages::default(),
                ))
            })
            .clone()
    }
}

/// The icons the field shows, in drawing order: the first is the lowest layer.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Kind {
    Shrine,
    Chest,
    Rare,
    Quest,
    Elite,
    Post,
    Portal,
    Ally,
    Boss,
    Hero,
}

const KINDS: [Kind; 10] = [
    Kind::Shrine,
    Kind::Chest,
    Kind::Rare,
    Kind::Quest,
    Kind::Elite,
    Kind::Post,
    Kind::Portal,
    Kind::Ally,
    Kind::Boss,
    Kind::Hero,
];

/// How an icon looks: a cell of an art sheet (`[left, right, top, bottom]`), a colour it is
/// multiplied with, and the colour of the dot it falls back to while the sheet is missing.
struct Art {
    path: &'static str,
    cell: [f32; 4],
    tint: Color,
    dot: Color,
}

/// `ObjectIcons.blp` is a 4 by 4 sheet: cell `i` sits in column `i % 4`, row `i / 4`. Seen: 0
/// gold, 1 red, 2 green, 4 blue.
const fn object_cell(i: usize) -> [f32; 4] {
    let (c, r) = ((i % 4) as f32, (i / 4) as f32);
    [c * 0.25, (c + 1.0) * 0.25, r * 0.25, (r + 1.0) * 0.25]
}

const WHOLE: [f32; 4] = [0.0, 1.0, 0.0, 1.0];
/// `UI-RaidTargetingIcons.blp`: 4 by 4 cells, the star the first and the skull the eighth.
const RAID_STAR: [f32; 4] = [0.0, 0.25, 0.0, 0.25];
const RAID_SKULL: [f32; 4] = [0.75, 1.0, 0.25, 0.5];

impl Kind {
    /// Icons of this kind the pool holds; more on the field than that are not shown.
    fn cap(self) -> usize {
        match self {
            Kind::Shrine | Kind::Post => 16,
            Kind::Chest => 32,
            Kind::Rare | Kind::Boss => 12,
            Kind::Quest => 24,
            Kind::Elite => 48,
            Kind::Portal => 2,
            Kind::Ally => 8,
            Kind::Hero => 1,
        }
    }

    /// Edge in canvas units.
    fn size(self) -> f32 {
        match self {
            Kind::Boss | Kind::Portal => 22.0,
            Kind::Post => 18.0,
            Kind::Quest => 14.0,
            Kind::Hero => 26.0,
            _ => 16.0,
        }
    }

    fn art(self) -> Art {
        let plain = |path, cell, dot| Art {
            path,
            cell,
            tint: Color::WHITE,
            dot,
        };
        match self {
            Kind::Shrine => plain(
                atlas::OBJECT_ICONS,
                object_cell(2),
                Color::srgb(0.3, 0.9, 0.3),
            ),
            Kind::Chest => plain(
                atlas::OBJECT_ICONS,
                object_cell(0),
                Color::srgb(1.0, 0.8, 0.2),
            ),
            Kind::Rare => plain(
                atlas::OBJECT_ICONS,
                object_cell(4),
                Color::srgb(0.5, 0.7, 1.0),
            ),
            Kind::Quest => Art {
                tint: Color::srgb(1.0, 0.9, 0.35),
                ..plain(
                    atlas::OBJECT_ICONS,
                    object_cell(3),
                    Color::srgb(1.0, 0.9, 0.35),
                )
            },
            Kind::Elite => plain(atlas::RAID_ICONS, RAID_STAR, Color::srgb(1.0, 0.85, 0.2)),
            Kind::Post => plain(atlas::RAID_ICONS, RAID_SKULL, Color::srgb(0.9, 0.3, 0.2)),
            Kind::Portal => Art {
                tint: Color::srgb(0.8, 0.55, 1.0),
                ..plain(
                    atlas::OBJECT_ICONS,
                    object_cell(4),
                    Color::srgb(0.8, 0.55, 1.0),
                )
            },
            Kind::Ally => plain(atlas::PARTY_ICON, WHOLE, Color::srgb(0.3, 0.9, 0.4)),
            Kind::Boss => plain(atlas::SKULL_TEXTURE, WHOLE, Color::srgb(0.95, 0.2, 0.15)),
            Kind::Hero => plain(atlas::ARROW_TEXTURE, WHOLE, Color::WHITE),
        }
    }
}

/// One icon of the pool: the `index`-th of its kind on the field.
#[derive(Component, Clone, Copy)]
struct Mark {
    kind: Kind,
    index: usize,
}

/// The line that names the icon under the cursor.
#[derive(Component)]
struct Tip;

/// Where something is on the view, 0..1 across (east) and down (south), and what it is called.
#[derive(Clone)]
struct Spot {
    uv: Vec2,
    label: &'static str,
}

pub(super) fn plugin(app: &mut App) {
    app.init_resource::<Field>().add_systems(
        Update,
        (
            compose_ground.run_if(showing),
            fill_field.run_if(showing),
            refresh_field,
        )
            .chain(),
    );
}

/// The window is open on the battlefield level.
fn showing(ui: Res<WorldMapUi>) -> bool {
    ui.open && ui.level == MapLevel::Field
}

/// The view's centre (WoW x, y), its half side and the barrier's radius (infinite where the bounds
/// are open: no ring, nothing dark): the arena on a zone, the leg under way on a route's road.
fn framing(bounds: &crate::route::Bounds) -> ((f32, f32), f32, f32) {
    let [wx, wy, _] = bevy_to_wow(bounds.centre);
    let ring = if bounds.fenced {
        bounds.leash
    } else {
        f32::INFINITY
    };
    ((wx, wy), view_half(bounds.leash), ring)
}

/// Half the side of the view in yards: the barrier with room around it, but never a postage
/// stamp over a small arena.
fn view_half(leash: f32) -> f32 {
    // 1.15, as a fraction so that 150 yd come out as exactly 172.5.
    (leash * 23.0 / 20.0).max(60.0)
}

/// Where WoW `(wx, wy)` falls on a view of half-side `half` around `center`, 0..1 across and
/// down: north up, east right (x runs north, y west).
fn field_uv(center: (f32, f32), half: f32, wx: f32, wy: f32) -> Vec2 {
    Vec2::new(
        0.5 + (center.1 - wy) / (2.0 * half),
        0.5 + (center.0 - wx) / (2.0 * half),
    )
}

/// The inverse: the WoW `(wx, wy)` at view fraction `uv`.
fn field_world(center: (f32, f32), half: f32, uv: Vec2) -> (f32, f32) {
    (
        center.0 - (uv.y - 0.5) * 2.0 * half,
        center.1 - (uv.x - 0.5) * 2.0 * half,
    )
}

/// How the ground looks at distance `d` from the arena's centre when the barrier stands at
/// `radius` (both in texels): the brightness it keeps, and the ring's strength over it, 1 at the
/// circle and fading to 0 a few texels to either side.
fn shade_outside(d: f32, radius: f32) -> (f32, f32) {
    let off = d - radius;
    let light = 1.0 - (1.0 - OUTSIDE_LIGHT) * off.clamp(0.0, 1.0);
    let ring = (1.0 - (off.abs() - RING_HALF) / RING_SOFT).clamp(0.0, 1.0);
    (light, ring)
}

/// The minimap texel (x grows east, y south, over the whole 64 by 64 tile world) under WoW
/// `(wx, wy)`.
fn texel_at(wx: f32, wy: f32) -> Vec2 {
    let (north, west) = tile_to_world(0, 0);
    Vec2::new((west - wy) * TEX_PER_YD, (north - wx) * TEX_PER_YD)
}

/// The ADT tiles `(x, y)` the view spans, a texel to spare, clamped to the map.
fn view_tiles(center: (f32, f32), half: f32) -> (RangeInclusive<i32>, RangeInclusive<i32>) {
    let nw = texel_at(center.0 + half, center.1 + half);
    let se = texel_at(center.0 - half, center.1 - half);
    let tile = |t: f32| ((t / TILE_PX as f32).floor() as i32).clamp(0, 63);
    (
        tile(nw.x - 1.0)..=tile(se.x + 1.0),
        tile(nw.y - 1.0)..=tile(se.y + 1.0),
    )
}

/// Load one minimap tile: 256² RGBA, or `None` where none was authored.
fn load_tile(
    assets: &mut WorldAssets,
    translate: &MinimapTranslate,
    dir: &str,
    tx: u32,
    ty: u32,
) -> Option<Vec<u8>> {
    let hash = translate.tile(dir, tx, ty)?;
    let (w, h, px) = assets.decode_rgba(&format!("textures\\Minimap\\{hash}"))?;
    (w as usize == TILE_PX && h as usize == TILE_PX).then_some(px)
}

fn srgb_bytes(c: Color) -> [f32; 3] {
    let s = c.to_srgba();
    [s.red, s.green, s.blue].map(|v| v.clamp(0.0, 1.0) * 255.0)
}

/// Compose the ground into `base` ([`FIELD_PX`] square RGBA): every texel samples the minimap
/// tiles bilinearly (across tile seams too), dark parchment where a tile is missing, then goes
/// dark outside the barrier and takes the ring's colour on it.
fn paint_ground(
    base: &mut Vec<u8>,
    center: (f32, f32),
    half: f32,
    leash: f32,
    tiles: &HashMap<(u32, u32), Option<Vec<u8>>>,
) {
    base.clear();
    base.resize(FIELD_PX * FIELD_PX * 4, 255);
    let (xs, ys) = view_tiles(center, half);
    let (x0, y0) = (*xs.start(), *ys.start());
    let (nx, ny) = (xs.end() - x0 + 1, ys.end() - y0 + 1);
    let window: Vec<Option<&[u8]>> = ys
        .clone()
        .flat_map(|ty| xs.clone().map(move |tx| (tx, ty)))
        .map(|(tx, ty)| {
            tiles
                .get(&(tx as u32, ty as u32))
                .and_then(|t| t.as_deref())
        })
        .collect();
    let texel = |x: i32, y: i32| -> [f32; 3] {
        let (cx, cy) = (
            x.div_euclid(TILE_PX as i32) - x0,
            y.div_euclid(TILE_PX as i32) - y0,
        );
        if !(0..nx).contains(&cx) || !(0..ny).contains(&cy) {
            return PARCHMENT;
        }
        match window[(cy * nx + cx) as usize] {
            Some(px) => {
                let (lx, ly) = (
                    x.rem_euclid(TILE_PX as i32) as usize,
                    y.rem_euclid(TILE_PX as i32) as usize,
                );
                let i = (ly * TILE_PX + lx) * 4;
                [f32::from(px[i]), f32::from(px[i + 1]), f32::from(px[i + 2])]
            }
            None => PARCHMENT,
        }
    };

    let px_per_yd = FIELD_PX as f32 / (2.0 * half);
    let ring_radius = leash * px_per_yd;
    let ring_rgb = srgb_bytes(BARRIER_TINT);
    for v in 0..FIELD_PX {
        for u in 0..FIELD_PX {
            let uv = Vec2::new(u as f32 + 0.5, v as f32 + 0.5) / FIELD_PX as f32;
            let (wx, wy) = field_world(center, half, uv);
            let g = texel_at(wx, wy) - 0.5;
            let (gx, gy) = (g.x.floor() as i32, g.y.floor() as i32);
            let (fx, fy) = (g.x - gx as f32, g.y - gy as f32);
            let (a, b, c, d) = (
                texel(gx, gy),
                texel(gx + 1, gy),
                texel(gx, gy + 1),
                texel(gx + 1, gy + 1),
            );
            let dist = (wx - center.0).hypot(wy - center.1) * px_per_yd;
            let (light, ring) = shade_outside(dist, ring_radius);
            let o = (v * FIELD_PX + u) * 4;
            for k in 0..3 {
                let top = a[k] + (b[k] - a[k]) * fx;
                let bottom = c[k] + (d[k] - c[k]) * fx;
                let ground = (top + (bottom - top) * fy) * light;
                base[o + k] = (ground + (ring_rgb[k] - ground) * ring).clamp(0.0, 255.0) as u8;
            }
        }
    }
}

/// Compose the ground when the map opens on a battleground it has not been composed for. Once
/// per battleground, not per opening.
fn compose_ground(
    selection: Res<Selection>,
    bounds: Res<crate::route::Bounds>,
    assets: Option<ResMut<WorldAssets>>,
    catalog: Option<Res<MapCatalogRes>>,
    mut field: ResMut<Field>,
    mut images: ResMut<Assets<Image>>,
) {
    if field.key == Some(bounds.key) {
        return;
    }
    let (Some(mut assets), Some(catalog)) = (assets, catalog) else {
        return;
    };
    let started = Instant::now();
    let def = selection.stage_def();
    let dir = catalog.0.directory(def.map).unwrap_or_default().to_string();
    let field = &mut *field;
    if field.dir != dir {
        field.tiles.clear();
        field.dir.clone_from(&dir);
    }
    if field.translate.is_none() && !field.translate_failed {
        let loaded = {
            let mut chain = assets.chain.lock_recover();
            load_minimap_translate(&mut chain)
        };
        match loaded {
            Ok(t) => field.translate = Some(t),
            Err(e) => {
                warn!("survivors world map: md5translate.trs: {e:#}, the field has no ground art");
                field.translate_failed = true;
            }
        }
    }
    let (center, half, ring) = framing(&bounds);
    if let Some(translate) = &field.translate {
        let (xs, ys) = view_tiles(center, half);
        for ty in ys {
            for tx in xs.clone() {
                field
                    .tiles
                    .entry((tx as u32, ty as u32))
                    .or_insert_with(|| {
                        load_tile(&mut assets, translate, &dir, tx as u32, ty as u32)
                    });
            }
        }
    }
    paint_ground(&mut field.base, center, half, ring, &field.tiles);
    field.key = Some(bounds.key);
    // Show it at once; the next tick lays the dots over it.
    let handle = field.handle(&mut images);
    if let Some(data) = images.get_mut(&handle).and_then(|i| i.data.as_mut()) {
        if data.len() == field.base.len() {
            data.copy_from_slice(&field.base);
        }
    }
    info!(
        "survivors world map: ground of {} composed in {:?}",
        def.name,
        started.elapsed()
    );
}

/// Build the field's page when its slot is empty: the ground image, centred, with the icon
/// pool and the tooltip on top of it.
fn fill_field(
    mut commands: Commands,
    slots: Query<(Entity, Option<&Children>), With<FieldSlot>>,
    mut field: ResMut<Field>,
    assets: Option<ResMut<WorldAssets>>,
    fonts: Option<Res<KitFonts>>,
    mut images: ResMut<Assets<Image>>,
) {
    let Ok((slot, children)) = slots.single() else {
        return;
    };
    if children.is_some_and(|c| !c.is_empty()) {
        return;
    }
    let (Some(mut assets), Some(fonts)) = (assets, fonts) else {
        return;
    };
    let image = field.handle(&mut images);
    commands.entity(slot).with_children(|slot| {
        slot.spawn(Node {
            width: Val::Percent(100.0),
            height: Val::Percent(100.0),
            justify_content: JustifyContent::Center,
            align_items: AlignItems::Center,
            ..default()
        })
        .with_children(|wrap| {
            wrap.spawn((
                Node {
                    width: Val::Px(VIEW),
                    height: Val::Px(VIEW),
                    overflow: Overflow::clip(),
                    ..default()
                },
                ImageNode::new(image),
            ))
            .with_children(|view| {
                for kind in KINDS {
                    let art = kind.art();
                    let size = kind.size();
                    let icon = cell_image(&mut assets, &mut images, art.path, art.cell);
                    for index in 0..kind.cap() {
                        let mut node = match icon {
                            Some(_) => at(Vec2::ZERO, size, size),
                            None => round(Vec2::ZERO, size * 0.6, size * 0.6),
                        };
                        node.display = Display::None;
                        let mut mark = view.spawn((Mark { kind, index }, Interaction::None, node));
                        match &icon {
                            Some(img) => mark.insert(ImageNode {
                                color: art.tint,
                                ..img.clone()
                            }),
                            None => mark.insert(BackgroundColor(art.dot)),
                        };
                    }
                }
                view.spawn((
                    Tip,
                    kit::body(&fonts, "", 12.0, kit::CREAM),
                    Node {
                        position_type: PositionType::Absolute,
                        display: Display::None,
                        padding: UiRect::axes(Val::Px(5.0), Val::Px(2.0)),
                        border_radius: BorderRadius::all(Val::Px(3.0)),
                        ..default()
                    },
                    BackgroundColor(Color::srgba(0.0, 0.0, 0.0, 0.8)),
                ));
            });
        });
    });
}

/// Stamp a filled `size`-texel dot at `at` (texel coordinates) into the ground image; the corners
/// of the larger ones are left out so they read round.
fn paint_dot(data: &mut [u8], at: Vec2, size: i32, rgb: [u8; 3]) {
    let lo = (at - Vec2::splat(size as f32 / 2.0)).round().as_ivec2();
    for dy in 0..size {
        for dx in 0..size {
            if size >= 4 && (dx == 0 || dx == size - 1) && (dy == 0 || dy == size - 1) {
                continue;
            }
            let (x, y) = (lo.x + dx, lo.y + dy);
            if !(0..FIELD_PX as i32).contains(&x) || !(0..FIELD_PX as i32).contains(&y) {
                continue;
            }
            let o = (y as usize * FIELD_PX + x as usize) * 4;
            data[o..o + 3].copy_from_slice(&rgb);
            data[o + 3] = 255;
        }
    }
}

fn dot_rgb(c: Color) -> [u8; 3] {
    srgb_bytes(c).map(|v| v.round() as u8)
}

/// Ten times a second while the field is shown: the ground, the dots, then the icons and the
/// tooltip.
#[allow(clippy::too_many_arguments)]
fn refresh_field(
    ui: Res<WorldMapUi>,
    bounds: Res<crate::route::Bounds>,
    field: Res<Field>,
    time: Res<Time<Real>>,
    mut images: ResMut<Assets<Image>>,
    log: Res<QuestLog>,
    hero: Query<&Transform, With<Hero>>,
    enemies: Query<(&Transform, &Enemy, Has<Elite>)>,
    markers: Query<(&Transform, &MapMarker, Option<&Enemy>)>,
    drops: Query<(&Transform, &DropArt, Has<MapMarker>)>,
    pins: Query<(&Transform, &MapPin)>,
    mut pool: Query<(&Mark, &mut Node, &mut UiTransform, &Interaction), Without<Tip>>,
    mut tip: Query<(&mut Node, &mut Text), (With<Tip>, Without<Mark>)>,
    mut next_at: Local<f32>,
    mut shown: Local<bool>,
) {
    if !(ui.open && ui.level == MapLevel::Field) {
        *shown = false;
        return;
    }
    if field.key != Some(bounds.key) {
        return;
    }
    let now = time.elapsed_secs();
    if *shown && now < *next_at {
        return;
    }
    *shown = true;
    *next_at = now + TICK;
    let Ok(hero) = hero.single() else { return };
    let Some(handle) = &field.image else { return };
    let Some(data) = images.get_mut(handle).and_then(|i| i.data.as_mut()) else {
        return;
    };
    if data.len() != field.base.len() {
        return;
    }
    data.copy_from_slice(&field.base);

    let (center, half, _) = framing(&bounds);
    let to_texel = |wx: f32, wy: f32| field_uv(center, half, wx, wy) * FIELD_PX as f32;
    let hunted = log.hunted();
    let (red, yellow) = (
        dot_rgb(Color::srgb(0.85, 0.12, 0.10)),
        dot_rgb(Color::srgb(1.0, 0.85, 0.1)),
    );

    let mut spots: Vec<Vec<Spot>> = vec![Vec::new(); KINDS.len()];
    let mut push = |kind: Kind, t: &Transform, label: &'static str| {
        let [wx, wy, _] = bevy_to_wow(t.translation);
        let uv = field_uv(center, half, wx, wy);
        if (0.0..=1.0).contains(&uv.x) && (0.0..=1.0).contains(&uv.y) {
            spots[kind as usize].push(Spot { uv, label });
        }
    };

    // The horde, a quest's quarry on top of it; elites get a star, bosses and rares come with
    // their marker below.
    for (tf, enemy, elite) in &enemies {
        if enemy.dead || enemy.boss {
            continue;
        }
        if elite {
            push(Kind::Elite, tf, enemy.def.name);
            continue;
        }
        let [wx, wy, _] = bevy_to_wow(tf.translation);
        if hunted.contains(&enemy.def.name) {
            paint_dot(data, to_texel(wx, wy), 3, yellow);
        } else {
            paint_dot(data, to_texel(wx, wy), 2, red);
        }
    }
    // Drops in their colour, over a dark rim so that they hold against the ground. A guard's
    // chest carries a marker as well and is drawn once, as that.
    for (tf, art, marked) in &drops {
        let [wx, wy, _] = bevy_to_wow(tf.translation);
        match loot_tint(art) {
            Some(color) => {
                let at = to_texel(wx, wy);
                paint_dot(data, at, 6, [20, 16, 12]);
                paint_dot(data, at, 4, dot_rgb(color));
            }
            None if !marked => push(Kind::Chest, tf, "Chest"),
            None => {}
        }
    }
    for (tf, marker, enemy) in &markers {
        if enemy.is_some_and(|e| e.dead) {
            continue;
        }
        let name = enemy.map(|e| e.def.name);
        match marker {
            MapMarker::Post => push(Kind::Post, tf, "Guard post"),
            MapMarker::Chest => push(Kind::Chest, tf, "Chest"),
            MapMarker::Shrine => push(Kind::Shrine, tf, "Shrine"),
            MapMarker::Boss => push(Kind::Boss, tf, name.unwrap_or("Boss")),
            MapMarker::Rare => push(Kind::Rare, tf, name.unwrap_or("Rare")),
            MapMarker::Portal => push(Kind::Portal, tf, "Portal"),
        }
    }
    for (tf, pin) in &pins {
        match pin {
            MapPin::QuestItem => push(Kind::Quest, tf, "Quest item"),
            MapPin::Ally => push(Kind::Ally, tf, "Party member"),
        }
    }
    push(Kind::Hero, hero, "You");

    // The arrow turns with the hero's facing: counter-clockwise from north as the yaw runs,
    // while a UI rotation runs clockwise.
    let yaw = hero.rotation.to_euler(EulerRot::YXZ).0;
    let turn = Rot2::radians(-yaw);
    let centre_of = |spot: &Spot| spot.uv * VIEW;

    let mut hovered: Option<(usize, Vec2, &'static str, f32)> = None;
    for (mark, mut node, mut ut, interaction) in &mut pool {
        let kind = mark.kind as usize;
        let Some(spot) = spots[kind].get(mark.index) else {
            if node.display != Display::None {
                node.display = Display::None;
            }
            continue;
        };
        let size = mark.kind.size();
        let c = centre_of(spot);
        let (left, top) = (Val::Px(c.x - size / 2.0), Val::Px(c.y - size / 2.0));
        if node.left != left {
            node.left = left;
        }
        if node.top != top {
            node.top = top;
        }
        if node.display != Display::Flex {
            node.display = Display::Flex;
        }
        if mark.kind == Kind::Hero && ut.rotation != turn {
            ut.rotation = turn;
        }
        if *interaction == Interaction::Hovered && hovered.is_none_or(|(k, ..)| k < kind) {
            hovered = Some((kind, c, spot.label, size));
        }
    }

    for (mut node, mut text) in &mut tip {
        let Some((_, c, label, size)) = hovered else {
            if node.display != Display::None {
                node.display = Display::None;
            }
            continue;
        };
        if text.0 != label {
            text.0 = label.to_string();
        }
        // Beside the icon, on the side that has room.
        let top = Val::Px(c.y - 9.0);
        if node.top != top {
            node.top = top;
        }
        let (left, right) = if c.x > VIEW / 2.0 {
            (Val::Auto, Val::Px(VIEW - c.x + size / 2.0 + 4.0))
        } else {
            (Val::Px(c.x + size / 2.0 + 4.0), Val::Auto)
        };
        if node.left != left {
            node.left = left;
        }
        if node.right != right {
            node.right = right;
        }
        if node.display != Display::Flex {
            node.display = Display::Flex;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_view_frames_the_barrier_with_room() {
        assert_eq!(view_half(150.0), 172.5);
        assert_eq!(view_half(30.0), 60.0); // Gurubashi: not a postage stamp
    }

    #[test]
    fn north_up_east_right_on_the_field() {
        let c = (100.0, 200.0);
        assert_eq!(field_uv(c, 50.0, 100.0, 200.0), Vec2::splat(0.5));
        // North (wx + 50) is the top edge, east (wy - 50) the right edge.
        assert_eq!(field_uv(c, 50.0, 150.0, 150.0), Vec2::new(1.0, 0.0));
        let (wx, wy) = field_world(c, 50.0, Vec2::new(0.25, 0.75));
        assert!((field_uv(c, 50.0, wx, wy) - Vec2::new(0.25, 0.75)).length() < 1e-5);
    }

    #[test]
    fn the_ring_sits_on_the_leash() {
        let radius = 100.0;
        // On the circle the ring is at full strength.
        assert_eq!(shade_outside(radius, radius).1, 1.0);
        // Well inside: the ground as it is, no ring.
        assert_eq!(shade_outside(radius - 40.0, radius), (1.0, 0.0));
        // Well outside: darkened, no ring.
        let (light, ring) = shade_outside(radius + 40.0, radius);
        assert!((light - OUTSIDE_LIGHT).abs() < 1e-5, "{light}");
        assert_eq!(ring, 0.0);
        // The ring fades out away from the circle, on both sides alike.
        let (_, near) = shade_outside(radius + RING_HALF + RING_SOFT * 0.5, radius);
        assert!(near > 0.0 && near < 1.0, "{near}");
        assert_eq!(
            shade_outside(radius + 2.0, radius).1,
            shade_outside(radius - 2.0, radius).1
        );
    }

    /// A tile of one colour under the centre of ADT tile (32, 32), whose view stays inside it.
    #[test]
    fn the_ground_is_lit_inside_and_dark_outside() {
        let (north, west) = tile_to_world(32, 32);
        let centre = (north - TILE_YARDS / 2.0, west - TILE_YARDS / 2.0);
        let (half, leash) = (60.0, 30.0);
        let (xs, ys) = view_tiles(centre, half);
        assert_eq!((xs, ys), (32..=32, 32..=32));

        let mut tiles = HashMap::new();
        tiles.insert(
            (32, 32),
            Some([200, 100, 50, 255].repeat(TILE_PX * TILE_PX)),
        );
        let mut base = Vec::new();
        paint_ground(&mut base, centre, half, leash, &tiles);
        assert_eq!(base.len(), FIELD_PX * FIELD_PX * 4);
        let at = |x: usize, y: usize| {
            let o = (y * FIELD_PX + x) * 4;
            [base[o], base[o + 1], base[o + 2], base[o + 3]]
        };
        let near = |a: [u8; 4], b: [u8; 4]| a.iter().zip(b).all(|(&p, q)| p.abs_diff(q) <= 2);
        // The middle is the tile as it is, the corner (outside the barrier) 45 % of it.
        assert!(
            near(at(FIELD_PX / 2, FIELD_PX / 2), [200, 100, 50, 255]),
            "{:?}",
            at(256, 256)
        );
        assert!(near(at(0, 0), [90, 45, 22, 255]), "{:?}", at(0, 0));
        // On the circle (leash 30 of a 120-yd view: 128 texels out) the barrier's colour.
        let ring = at(FIELD_PX / 2 + 128, FIELD_PX / 2);
        assert!(ring[2] > ring[0], "violet-blue, not parchment: {ring:?}");

        // No tile at all: parchment.
        paint_ground(&mut base, centre, half, leash, &HashMap::new());
        let o = (FIELD_PX / 2 * FIELD_PX + FIELD_PX / 2) * 4;
        assert!(near(
            [base[o], base[o + 1], base[o + 2], 255],
            [40, 33, 22, 255]
        ));
    }

    #[test]
    fn dots_stay_inside_the_image() {
        let mut data = vec![0u8; FIELD_PX * FIELD_PX * 4];
        // Off every edge: nothing panics, nothing is written outside.
        for at in [
            Vec2::splat(-5.0),
            Vec2::splat(FIELD_PX as f32 + 5.0),
            Vec2::ZERO,
        ] {
            paint_dot(&mut data, at, 6, [1, 2, 3]);
        }
        assert_eq!(
            data[3], 255,
            "the corner texel is inside the dot at the origin"
        );
        paint_dot(&mut data, Vec2::splat(100.0), 2, [9, 9, 9]);
        let lit = data.chunks(4).filter(|p| p[3] == 255).count();
        assert!(lit > 0);
    }
}
