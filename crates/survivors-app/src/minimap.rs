//! The minimap, laid out as `Minimap.xml` lays out `MinimapCluster` at the top right: the zone line
//! on the border's top strip, the 140-unit map in its ring with the zoom buttons, the minimize
//! button, and `GameTime.xml`'s day clock.
//!
//! Bevy UI clips only to rectangles, so the round map is composited here each frame: the ADT tiles
//! through `md5translate.trs` (north up, 533.33 yd per 256-px tile), masked by `MinimapMask.blp`,
//! then the player arrow, then the red `ObjectIcons` cell per live enemy, the dot the reference
//! draws for a tracked creature, then the [`MapMarker`]s: guard posts, chests, shrines, bosses,
//! rares and portals, the far ones of the important kinds pinned to the ring. Sizes are the
//! engine minimap's ([`crate::minimap`]), measured against its 140.8-px blip basis.

use std::collections::HashMap;

use bevy::asset::RenderAssetUsages;
use bevy::prelude::*;
use bevy::render::render_resource::{Extent3d, TextureDimension, TextureFormat};

use benilla_assets::{LockRecover, MapCatalogRes, WorldAssets};
use benilla_formats::{tile_to_world, world_to_tile, MinimapTranslate};
use benilla_world::world_map::CurrentMap;
use benilla_world::world_point::WorldPoint;

use super::enemies::Enemy;
use super::hero::Hero;
use super::hud::UiArt;
use super::Run;
use benilla_app::survivors_host::area::AreaTableRes;

/// The composite's edge in texels; the 140-unit widget is ~200 px at 1080p.
const SIDE: usize = 192;
const TILE_YARDS: f32 = 533.333_3;
const TILE_PX: usize = 256;
const CHUNK_YARDS: f32 = TILE_YARDS / 16.0;
/// The outdoor zoom table (`0x8116d0`): view diameter in chunks per `Minimap:SetZoom` index.
const ZOOM_CHUNKS: [f32; 6] = [14.0, 12.0, 10.0, 8.0, 6.0, 4.0];
/// Fully in: the packs that matter close in from ~30 yd, which the wider zooms crowd to the middle.
const START_ZOOM: usize = 5;

/// The engine minimap's blip basis: the stock widget at 140.8 px on the 1024×768 screen.
const BLIP_BASIS_PX: f32 = 140.8;
const DOT_PX: f32 = 8.0;
/// `ObjectIcons.blp` cell 1 (left, right, top, bottom): a unit passing creature tracking.
const TRACKED_UNIT_CELL: [f32; 4] = [0.25, 0.5, 0.0, 0.25];
/// `ObjectIcons.blp` is a 4 by 4 sheet of round buttons, 32 px each: cell `i` sits in column
/// `i % 4`, row `i / 4`. Seen: 0 gold, 1 red, 2 green, 4 blue.
const fn object_cell(i: usize) -> [f32; 4] {
    let (c, r) = ((i % 4) as f32, (i / 4) as f32);
    [c * 0.25, (c + 1.0) * 0.25, r * 0.25, (r + 1.0) * 0.25]
}
/// `UI-RaidTargetingIcons.blp`: 4 by 4 cells of 64 px, the skull the eighth (column 3, row 1).
const RAID_SKULL: [f32; 4] = [0.75, 1.0, 0.25, 0.5];
/// Marker sizes on the 140.8-px basis, and the opacity of a marker pinned to the ring.
const MARK_PX: f32 = 10.0;
const MARK_BIG_PX: f32 = 13.0;
const PINNED_ALPHA: f32 = 0.55;
/// Gap between a pinned marker's edge and the ring's rim, in texels.
const RIM_GAP: f32 = 6.0;
/// The route's next station: a gold dot as big as a boss mark, more opaque than the other
/// pinned marks because it is the way on.
const STOP_PX: f32 = MARK_BIG_PX;
const STOP_PINNED_ALPHA: f32 = 0.85;
const STOP_GOLD: [f32; 3] = [255.0, 209.0, 0.0];
const STOP_RIM: [f32; 3] = [92.0, 58.0, 0.0];
const ARROW_PX: f32 = 33.6;
const ARROW_OFFSET_PX: Vec2 = Vec2::new(0.51, -1.73);

/// `GAMETIME_DAWN` and `GAMETIME_DUSK`, minutes into the day.
const DAWN: u32 = 5 * 60 + 30;
const DUSK: u32 = 21 * 60;

/// `FactionGroup.dbc` masks.
const GROUP_ALLIANCE: u32 = 2;
const GROUP_HORDE: u32 = 4;

#[derive(Component)]
pub(super) struct ZoneText;

/// The map body: hidden by the minimize button with everything in its ring.
#[derive(Component)]
pub(super) struct MapBody;

/// Something the minimap marks with an icon of its own, set by the module that spawns it: a guard
/// post ([`super::guards`]), its chest or shrine, a boss, a rare ([`super::bossbar`],
/// [`super::rares`]) and the bonus boss's portal. The entity needs a `Transform`; one that is an
/// [`Enemy`] and dead is not drawn. Posts, bosses and portals out of the view are pinned to the
/// ring, half transparent; the rest only show inside it.
#[derive(Component, Clone, Copy, PartialEq, Eq, Debug)]
pub(super) enum MapMarker {
    Post,
    Chest,
    Shrine,
    Boss,
    Rare,
    Portal,
}

/// The art sheets a marker is cut from.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Sheet {
    Objects,
    Raid,
    Skull,
}

/// How a [`MapMarker`] is drawn: the sheet and cell, the size on the 140.8-px basis, an optional
/// colour its luma is turned into, whether it pins to the ring out of view, and the order (the
/// higher on top).
#[derive(Clone, Copy, Debug)]
struct Glyph {
    sheet: Sheet,
    uv: [f32; 4],
    px: f32,
    tint: Option<[f32; 3]>,
    pinned: bool,
    rank: u8,
}

fn glyph(m: MapMarker) -> Glyph {
    let objects = |cell: usize, px: f32, rank: u8| Glyph {
        sheet: Sheet::Objects,
        uv: object_cell(cell),
        px,
        tint: None,
        pinned: false,
        rank,
    };
    match m {
        MapMarker::Rare => Glyph {
            tint: Some([190.0, 215.0, 255.0]),
            ..objects(4, MARK_PX, 0)
        },
        MapMarker::Chest => objects(0, MARK_PX, 1),
        MapMarker::Shrine => objects(2, MARK_PX, 2),
        MapMarker::Post => Glyph {
            sheet: Sheet::Raid,
            uv: RAID_SKULL,
            px: MARK_BIG_PX,
            tint: None,
            pinned: true,
            rank: 3,
        },
        MapMarker::Portal => Glyph {
            pinned: true,
            ..objects(4, MARK_BIG_PX, 4)
        },
        MapMarker::Boss => Glyph {
            sheet: Sheet::Skull,
            uv: [0.0, 1.0, 0.0, 1.0],
            px: MARK_BIG_PX,
            tint: None,
            pinned: true,
            rank: 5,
        },
    }
}

/// Where a marker `px` texels from the map's centre is drawn on a ring of `ring` texels: there
/// when inside, else on the ring in the same direction (and `true`).
fn pin(px: Vec2, ring: f32) -> (Vec2, bool) {
    let len = px.length();
    if len <= ring {
        (px, false)
    } else {
        (px / len * ring, true)
    }
}

#[derive(Component, Clone, Copy, PartialEq, Eq)]
pub(super) enum MapButton {
    ZoomIn,
    ZoomOut,
    Toggle,
}

/// Decoded RGBA8 art the composite samples.
struct Rgba {
    w: usize,
    h: usize,
    px: Vec<u8>,
}

impl Rgba {
    fn decode(assets: &mut WorldAssets, path: &str) -> Option<Self> {
        let (w, h, px) = assets.decode_rgba(path)?;
        Some(Self {
            w: w as usize,
            h: h as usize,
            px,
        })
    }

    /// Bilinear RGBA at texel-space `(x, y)`, clamped to the edge.
    fn sample(&self, x: f32, y: f32) -> [f32; 4] {
        let x = (x - 0.5).clamp(0.0, (self.w - 1) as f32);
        let y = (y - 0.5).clamp(0.0, (self.h - 1) as f32);
        let (x0, y0) = (x as usize, y as usize);
        let (x1, y1) = ((x0 + 1).min(self.w - 1), (y0 + 1).min(self.h - 1));
        let (fx, fy) = (x - x0 as f32, y - y0 as f32);
        let at = |x: usize, y: usize| {
            let i = (y * self.w + x) * 4;
            [0, 1, 2, 3].map(|c| f32::from(self.px[i + c]))
        };
        let (a, b, c, d) = (at(x0, y0), at(x1, y0), at(x0, y1), at(x1, y1));
        [0, 1, 2, 3].map(|k| {
            let top = a[k] + (b[k] - a[k]) * fx;
            let bottom = c[k] + (d[k] - c[k]) * fx;
            top + (bottom - top) * fy
        })
    }
}

struct Art {
    translate: MinimapTranslate,
    mask: Option<Rgba>,
    arrow: Option<Rgba>,
    icons: Option<Rgba>,
    /// The raid target icons (the guard post's skull) and the target frame's boss skull.
    raid: Option<Rgba>,
    skull: Option<Rgba>,
}

#[derive(Resource)]
pub(super) struct MiniMap {
    zoom: usize,
    shown: bool,
    image: Handle<Image>,
    art: Option<Art>,
    /// Decoded 256² tiles by ADT index, `None` where no art was authored, for the map `dir` names.
    tiles: HashMap<(u32, u32), Option<Vec<u8>>>,
    dir: String,
    buf: Vec<u8>,
}

impl MiniMap {
    pub(super) fn new(images: &mut Assets<Image>) -> Self {
        let image = Image::new(
            Extent3d {
                width: SIDE as u32,
                height: SIDE as u32,
                depth_or_array_layers: 1,
            },
            TextureDimension::D2,
            vec![0; SIDE * SIDE * 4],
            TextureFormat::Rgba8UnormSrgb,
            RenderAssetUsages::default(),
        );
        Self {
            zoom: START_ZOOM,
            shown: true,
            image: images.add(image),
            art: None,
            tiles: HashMap::new(),
            dir: String::new(),
            buf: vec![0; SIDE * SIDE * 4],
        }
    }

    fn view_radius(&self) -> f32 {
        ZOOM_CHUNKS[self.zoom] * 0.5 * CHUNK_YARDS
    }
}

/// `MinimapCluster` and its children, in cluster-local units (top-left origin) from `Minimap.xml`.
pub(super) fn spawn_cluster(root: &mut ChildSpawnerCommands, font: &Handle<Font>, map: &MiniMap) {
    let at = |left: f32, top: f32, w: f32, h: f32| Node {
        position_type: PositionType::Absolute,
        left: Val::Px(left),
        top: Val::Px(top),
        width: Val::Px(w),
        height: Val::Px(h),
        ..default()
    };
    let art = |path: &'static str, coords: [f32; 4]| {
        (
            ImageNode {
                color: Color::NONE,
                ..default()
            },
            UiArt::crop(path, coords),
        )
    };
    root.spawn(Node {
        position_type: PositionType::Absolute,
        right: Val::Px(0.0),
        top: Val::Px(0.0),
        width: Val::Px(192.0),
        height: Val::Px(192.0),
        ..default()
    })
    .with_children(|cluster| {
        cluster.spawn((
            at(0.0, 0.0, 192.0, 32.0),
            art(
                "Interface\\Minimap\\UI-Minimap-Border",
                [0.25, 1.0, 0.0, 0.125],
            ),
        ));
        cluster
            .spawn(at(29.0, 7.0, 128.0, 12.0))
            .with_children(|zone| {
                zone.spawn((
                    ZoneText,
                    Text::new(""),
                    TextFont {
                        font: font.clone(),
                        font_size: 12.0,
                        ..default()
                    },
                    TextColor(Color::srgb(1.0, 0.82, 0.0)),
                    TextShadow {
                        offset: Vec2::new(1.0, 1.0),
                        color: Color::BLACK,
                    },
                    TextLayout::new(Justify::Center, LineBreak::NoWrap),
                    Node {
                        width: Val::Percent(100.0),
                        ..default()
                    },
                ));
            });
        cluster
            .spawn((MapBody, at(35.0, 22.0, 140.0, 140.0)))
            .with_children(|body| {
                body.spawn((
                    Node {
                        width: Val::Percent(100.0),
                        height: Val::Percent(100.0),
                        ..default()
                    },
                    ImageNode::new(map.image.clone()),
                ));
                // `MinimapBackdrop`: the ring, 192² at the cluster's (0, 20).
                body.spawn((
                    at(-35.0, -2.0, 192.0, 192.0),
                    art(
                        "Interface\\Minimap\\UI-Minimap-Border",
                        [0.25, 1.0, 0.125, 0.875],
                    ),
                ));
                body.spawn((
                    MapButton::ZoomIn,
                    Button,
                    at(122.0, 91.0, 32.0, 32.0),
                    ImageNode::default(),
                ));
                body.spawn((
                    MapButton::ZoomOut,
                    Button,
                    at(96.0, 119.0, 32.0, 32.0),
                    ImageNode::default(),
                ));
            });
        // `GameTimeFrame`, 50² at the cluster's top right, offset (4, -19).
        let tod = if (DAWN..DUSK).contains(&super::hero::MINUTE_OF_DAY) {
            0.0
        } else {
            0.5
        };
        cluster.spawn((
            at(146.0, 19.0, 50.0, 50.0),
            art(
                "Interface\\Minimap\\UI-TOD-Indicator",
                [tod, tod + 50.0 / 128.0, 0.0, 50.0 / 64.0],
            ),
        ));
        cluster.spawn((
            MapButton::Toggle,
            Button,
            at(161.0, -3.0, 32.0, 32.0),
            ImageNode::default(),
        ));
    });
}

/// The zoom and minimize buttons: their `Up`/`Down`/`Disabled` art, and a press acts.
pub(super) fn map_buttons(
    mut map: ResMut<MiniMap>,
    mut buttons: Query<(Ref<Interaction>, &MapButton, &mut ImageNode)>,
    mut bodies: Query<&mut Node, With<MapBody>>,
    assets: Option<ResMut<WorldAssets>>,
    mut images: ResMut<Assets<Image>>,
) {
    let Some(mut assets) = assets else { return };
    for (interaction, button, mut img) in &mut buttons {
        let pressed = *interaction == Interaction::Pressed;
        if pressed && interaction.is_changed() {
            match button {
                MapButton::ZoomIn => map.zoom = (map.zoom + 1).min(ZOOM_CHUNKS.len() - 1),
                MapButton::ZoomOut => map.zoom = map.zoom.saturating_sub(1),
                MapButton::Toggle => map.shown = !map.shown,
            }
        }
        let disabled = match button {
            MapButton::ZoomIn => map.zoom == ZOOM_CHUNKS.len() - 1,
            MapButton::ZoomOut => map.zoom == 0,
            MapButton::Toggle => false,
        };
        let stem = match button {
            MapButton::ZoomIn => "Interface\\Minimap\\UI-Minimap-ZoomInButton",
            MapButton::ZoomOut => "Interface\\Minimap\\UI-Minimap-ZoomOutButton",
            MapButton::Toggle => "Interface\\Buttons\\UI-Panel-MinimizeButton",
        };
        let state = match (disabled, pressed) {
            (true, _) => "Disabled",
            (false, true) => "Down",
            (false, false) => "Up",
        };
        if let Some(h) = assets.sprite_texture(&format!("{stem}-{state}"), &mut images) {
            if img.image != h {
                img.image = h;
            }
        }
    }
    let display = if map.shown {
        Display::Flex
    } else {
        Display::None
    };
    for mut node in &mut bodies {
        if node.display != display {
            node.display = display;
        }
    }
}

/// `Minimap_Update`: `GetMinimapZoneText` (the subzone, else the zone) in `GetZonePVPInfo`'s
/// colour, the zone's owner against the hero's faction.
pub(super) fn zone_text(
    world: WorldPoint,
    areas: Option<Res<AreaTableRes>>,
    run: Res<Run>,
    mut texts: Query<(&mut Text, &mut TextColor), With<ZoneText>>,
) {
    let Some(areas) = areas else { return };
    let Some(leaf) = world.area() else { return };
    let Some(row) = areas.0.get(leaf) else { return };
    let zone = if row.zone_id == 0 { leaf } else { row.zone_id };
    let name = if leaf == zone {
        areas.0.name(zone).unwrap_or_default().to_string()
    } else {
        row.name.clone()
    };
    let (friend, enemy) = match run.race {
        1 | 3 | 4 | 7 => (GROUP_ALLIANCE, GROUP_HORDE),
        _ => (GROUP_HORDE, GROUP_ALLIANCE),
    };
    let owner = areas.0.get(zone).map_or(0, |r| r.faction_group_mask);
    let color = if row.flags & 0x80 != 0 || owner & enemy != 0 {
        Color::srgb(1.0, 0.1, 0.1)
    } else if owner & friend != 0 {
        Color::srgb(0.1, 1.0, 0.1)
    } else {
        Color::srgb(1.0, 0.7, 0.0)
    };
    for (mut text, mut tc) in &mut texts {
        if text.0 != name {
            text.0 = name.clone();
        }
        if tc.0 != color {
            tc.0 = color;
        }
    }
}

/// Recomposite the map around the hero: tiles, mask, arrow, the enemy dots, then the markers.
#[allow(clippy::too_many_arguments)]
pub(super) fn compose(
    mut map: ResMut<MiniMap>,
    assets: Option<ResMut<WorldAssets>>,
    catalog: Option<Res<MapCatalogRes>>,
    current: Option<Res<CurrentMap>>,
    phase: Res<State<super::Phase>>,
    hero: Query<&Transform, With<Hero>>,
    enemies: Query<(&Transform, &Enemy)>,
    markers: Query<(&Transform, &MapMarker, Option<&Enemy>)>,
    selection: Res<super::Selection>,
    route: Res<super::route::RouteRun>,
    mut images: ResMut<Assets<Image>>,
    time: Res<Time<Real>>,
    mut next_at: Local<f32>,
) {
    if *phase.get() == super::Phase::Menu || !map.shown {
        return;
    }
    // A CPU composite plus a texture upload: on the browser's one thread it was the single
    // costliest survivors system per frame (~5 %), and the blips read the same at 12 Hz.
    let now = time.elapsed_secs();
    if now < *next_at {
        return;
    }
    *next_at = now + 1.0 / 12.0;
    let (Some(mut assets), Some(catalog), Some(current)) = (assets, catalog, current) else {
        return;
    };
    let Ok(hero) = hero.single() else { return };
    let Some(dir) = catalog.0.directory(current.0).map(str::to_string) else {
        return;
    };
    let map = &mut *map;
    if map.dir != dir {
        map.tiles.clear();
        map.dir.clone_from(&dir);
    }
    if map.art.is_none() {
        let translate = {
            let mut chain = assets.chain.lock_recover();
            benilla_formats::load_minimap_translate(&mut chain)
        };
        let Ok(translate) = translate else {
            warn!("survivors: md5translate.trs failed, no minimap");
            map.shown = false;
            return;
        };
        map.art = Some(Art {
            translate,
            mask: Rgba::decode(&mut assets, "Textures\\MinimapMask"),
            arrow: Rgba::decode(&mut assets, benilla_app::survivors_host::minimap::blips::PLAYER_ARROW_TEXTURE),
            icons: Rgba::decode(&mut assets, "Interface\\Minimap\\ObjectIcons"),
            raid: Rgba::decode(&mut assets, "Interface\\TargetingFrame\\UI-RaidTargetingIcons"),
            skull: Rgba::decode(&mut assets, "Interface\\TargetingFrame\\UI-TargetingFrame-Skull"),
        });
    }
    let art = map.art.as_ref().expect("art loaded above");

    // The hero in WoW coordinates; screen right is east (-y), screen down south (-x).
    let p = hero.translation;
    let (wx, wy) = (-p.z, -p.x);
    let radius = map.view_radius();
    let yd_per_px = 2.0 * radius / SIDE as f32;

    // The 2×2 tile window under the view: its diameter (at most 14 chunks) is under one tile.
    let (tx0, ty0) = world_to_tile(wx + radius + 1.0, wy + radius + 1.0);
    for ty in ty0..=ty0 + 1 {
        for tx in tx0..=tx0 + 1 {
            map.tiles.entry((tx, ty)).or_insert_with(|| {
                let hash = art.translate.tile(&dir, tx, ty)?;
                let tile = Rgba::decode(&mut assets, &format!("textures\\Minimap\\{hash}"))?;
                (tile.w == TILE_PX && tile.h == TILE_PX).then_some(tile.px)
            });
        }
    }
    let window: [[Option<&[u8]>; 2]; 2] = [0, 1].map(|j| {
        [0, 1].map(|i| {
            map.tiles
                .get(&(tx0 + i, ty0 + j))
                .and_then(|t| t.as_deref())
        })
    });
    // Window-local tile texels: x grows east from the window's west edge, y south from its north.
    let (north, west) = tile_to_world(tx0, ty0);
    let tex_per_yd = TILE_PX as f32 / TILE_YARDS;
    let origin = Vec2::new((west - wy) * tex_per_yd, (north - wx) * tex_per_yd);
    let tex_per_px = yd_per_px * tex_per_yd;
    let texel = |x: i32, y: i32| -> [f32; 3] {
        let (cx, cy) = (x.div_euclid(TILE_PX as i32), y.div_euclid(TILE_PX as i32));
        if !(0..2).contains(&cx) || !(0..2).contains(&cy) {
            return [0.0; 3];
        }
        match window[cy as usize][cx as usize] {
            Some(px) => {
                let (lx, ly) = (x.rem_euclid(TILE_PX as i32), y.rem_euclid(TILE_PX as i32));
                let i = (ly as usize * TILE_PX + lx as usize) * 4;
                [f32::from(px[i]), f32::from(px[i + 1]), f32::from(px[i + 2])]
            }
            None => [0.0; 3],
        }
    };

    let half = SIDE as f32 / 2.0;
    let buf = &mut map.buf;
    for v in 0..SIDE {
        for u in 0..SIDE {
            let o = (v * SIDE + u) * 4;
            let mask = match &art.mask {
                Some(m) => {
                    let (mx, my) = (u * m.w / SIDE, v * m.h / SIDE);
                    let i = (my * m.w + mx) * 4;
                    m.px[i].min(m.px[i + 3])
                }
                None => {
                    let d = Vec2::new(u as f32 + 0.5 - half, v as f32 + 0.5 - half).length();
                    ((half - d).clamp(0.0, 1.0) * 255.0) as u8
                }
            };
            if mask == 0 {
                buf[o..o + 4].copy_from_slice(&[0; 4]);
                continue;
            }
            let g = origin + (Vec2::new(u as f32, v as f32) + 0.5 - half) * tex_per_px - 0.5;
            let (x0, y0) = (g.x.floor() as i32, g.y.floor() as i32);
            let (fx, fy) = (g.x - x0 as f32, g.y - y0 as f32);
            let (a, b, c, d) = (
                texel(x0, y0),
                texel(x0 + 1, y0),
                texel(x0, y0 + 1),
                texel(x0 + 1, y0 + 1),
            );
            for k in 0..3 {
                let top = a[k] + (b[k] - a[k]) * fx;
                let bottom = c[k] + (d[k] - c[k]) * fx;
                buf[o + k] = (top + (bottom - top) * fy) as u8;
            }
            buf[o + 3] = mask;
        }
    }

    let scale = SIDE as f32 / BLIP_BASIS_PX;
    let center = Vec2::splat(half);
    // The arrow turns with the hero's facing: counter-clockwise from north, as the yaw runs.
    if let Some(arrow) = &art.arrow {
        let yaw = hero.rotation.to_euler(EulerRot::YXZ).0;
        let (s, c) = yaw.sin_cos();
        let to_screen = |t: Vec2| Vec2::new(c * t.x + s * t.y, -s * t.x + c * t.y);
        let to_tex = |q: Vec2| Vec2::new(c * q.x - s * q.y, s * q.x + c * q.y);
        let size = ARROW_PX * scale;
        blit(
            buf,
            center + to_screen(ARROW_OFFSET_PX * scale),
            size,
            |q| {
                let t = to_tex(q) / size + 0.5;
                (0.0..1.0).contains(&t.x).then_some(())?;
                (0.0..1.0).contains(&t.y).then_some(())?;
                Some(arrow.sample(t.x * arrow.w as f32, t.y * arrow.h as f32))
            },
        );
    }
    if let Some(icons) = &art.icons {
        let size = DOT_PX * scale;
        let [l, r, t, b] = TRACKED_UNIT_CELL;
        for (tf, enemy) in &enemies {
            if enemy.dead {
                continue;
            }
            let off = Vec2::new(tf.translation.x - p.x, tf.translation.z - p.z);
            if off.length() > radius {
                continue;
            }
            blit(buf, center + off / yd_per_px, size, |q| {
                let f = q / size + 0.5;
                let x = (l + (r - l) * f.x) * icons.w as f32;
                let y = (t + (b - t) * f.y) * icons.h as f32;
                Some(icons.sample(x, y))
            });
        }
    }
    // Markers over the dots, the higher ranks last. The ring a pinned one rests on keeps it clear
    // of the border's rim.
    let mut marks: Vec<(Glyph, Vec2)> = markers
        .iter()
        .filter(|(_, _, e)| e.is_none_or(|e| !e.dead))
        .map(|(tf, m, _)| {
            (
                glyph(*m),
                Vec2::new(tf.translation.x - p.x, tf.translation.z - p.z),
            )
        })
        .collect();
    marks.sort_by_key(|(g, _)| g.rank);
    for (g, off) in marks {
        let sheet = match g.sheet {
            Sheet::Objects => art.icons.as_ref(),
            Sheet::Raid => art.raid.as_ref(),
            Sheet::Skull => art.skull.as_ref(),
        };
        let Some(sheet) = sheet else { continue };
        let size = g.px * scale;
        let (at, outside) = if g.pinned {
            pin(off / yd_per_px, half - size * 0.5 - RIM_GAP)
        } else if off.length() > radius {
            continue;
        } else {
            (off / yd_per_px, false)
        };
        let alpha = if outside { PINNED_ALPHA } else { 1.0 };
        draw_icon(buf, sheet, g.uv, center + at, size, alpha, g.tint);
    }
    // On a route the next station, over everything, pinned to the ring while it is out of view.
    if let Some(off) = stop_offset(&selection, &route, p) {
        let size = STOP_PX * scale;
        let (at, outside) = pin(off / yd_per_px, half - size * 0.5 - RIM_GAP);
        let alpha = if outside { STOP_PINNED_ALPHA } else { 1.0 };
        blit(buf, center + at, size, |q| stop_dot(q / size, alpha));
    }

    if let Some(image) = images.get_mut(&map.image) {
        if let Some(data) = image.data.as_mut() {
            data.copy_from_slice(&map.buf);
        }
    }
}

/// Where the next station of the route under way lies from the hero at `hero`, in yards on the
/// map's axes (east right, south down); `None` off a route and once it is walked.
fn stop_offset(
    selection: &super::Selection,
    route: &super::route::RouteRun,
    hero: Vec3,
) -> Option<Vec2> {
    let def = super::route::route_of(selection.stage)?;
    let stop = super::route_hud::next_station(def, route)?;
    Some(Vec2::new(stop.at.x - hero.x, stop.at.z - hero.z))
}

/// The station dot at `f`, the offset from its centre in dot sizes: gold with a dark rim and a
/// soft edge, `None` outside it.
fn stop_dot(f: Vec2, alpha: f32) -> Option<[f32; 4]> {
    let d = f.length() * 2.0;
    if d >= 1.0 {
        return None;
    }
    let [r, g, b] = if d > 0.68 { STOP_RIM } else { STOP_GOLD };
    let edge = ((1.0 - d) / 0.12).min(1.0);
    Some([r, g, b, 255.0 * alpha * edge])
}

/// Blit the cell `uv` (left, right, top, bottom) of `sheet` as a `size`-px square at `at`, its
/// alpha scaled by `alpha`; with a `tint`, the cell's luma takes that colour (0..255).
fn draw_icon(
    buf: &mut [u8],
    sheet: &Rgba,
    uv: [f32; 4],
    at: Vec2,
    size: f32,
    alpha: f32,
    tint: Option<[f32; 3]>,
) {
    let [l, r, t, b] = uv;
    blit(buf, at, size, |q| {
        let f = q / size + 0.5;
        (0.0..1.0).contains(&f.x).then_some(())?;
        (0.0..1.0).contains(&f.y).then_some(())?;
        let x = (l + (r - l) * f.x) * sheet.w as f32;
        let y = (t + (b - t) * f.y) * sheet.h as f32;
        let mut px = sheet.sample(x, y);
        px[3] *= alpha;
        if let Some(c) = tint {
            let luma = (px[0] * 0.3 + px[1] * 0.59 + px[2] * 0.11) / 255.0;
            for k in 0..3 {
                px[k] = (c[k] * luma * 1.25).min(255.0);
            }
        }
        Some(px)
    });
}

/// Alpha-blend a `size`-px square centred at `at`; `shade` maps a pixel's offset from the centre
/// to its RGBA. The map's own alpha (the mask) is kept.
fn blit(buf: &mut [u8], at: Vec2, size: f32, shade: impl Fn(Vec2) -> Option<[f32; 4]>) {
    let half = size / 2.0;
    let x_lo = (at.x - half).floor().max(0.0) as usize;
    let y_lo = (at.y - half).floor().max(0.0) as usize;
    let x_hi = ((at.x + half).ceil() as usize).min(SIDE);
    let y_hi = ((at.y + half).ceil() as usize).min(SIDE);
    for y in y_lo..y_hi {
        for x in x_lo..x_hi {
            let o = (y * SIDE + x) * 4;
            if buf[o + 3] == 0 {
                continue;
            }
            let q = Vec2::new(x as f32 + 0.5, y as f32 + 0.5) - at;
            let Some(src) = shade(q) else { continue };
            let a = src[3] / 255.0;
            for k in 0..3 {
                let dst = f32::from(buf[o + k]);
                buf[o + k] = (dst + (src[k] - dst) * a) as u8;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn edge_markers_clamp_to_the_ring() {
        let ring = 80.0;
        let (inside, out) = pin(Vec2::new(30.0, -40.0), ring);
        assert_eq!(inside, Vec2::new(30.0, -40.0));
        assert!(!out);
        let far = Vec2::new(300.0, -400.0);
        let (at, out) = pin(far, ring);
        assert!(out);
        assert!((at.length() - ring).abs() < 1e-3, "{}", at.length());
        assert!((at.normalize() - far.normalize()).length() < 1e-5, "same direction");
        // Right on the ring stays put; the centre does not divide by zero.
        assert!(!pin(Vec2::new(ring, 0.0), ring).1);
        assert_eq!(pin(Vec2::ZERO, ring), (Vec2::ZERO, false));
    }

    #[test]
    fn only_the_far_kinds_pin_and_the_important_ones_draw_last() {
        for m in [MapMarker::Post, MapMarker::Boss, MapMarker::Portal] {
            assert!(glyph(m).pinned, "{m:?}");
        }
        for m in [MapMarker::Chest, MapMarker::Shrine, MapMarker::Rare] {
            assert!(!glyph(m).pinned, "{m:?}");
        }
        assert!(glyph(MapMarker::Boss).rank > glyph(MapMarker::Post).rank);
        assert!(glyph(MapMarker::Post).rank > glyph(MapMarker::Chest).rank);
    }

    #[test]
    fn the_station_dot_is_a_gold_disc_with_a_rim() {
        let mid = stop_dot(Vec2::ZERO, 1.0).expect("the middle is in");
        assert_eq!(mid, [255.0, 209.0, 0.0, 255.0]);
        let rim = stop_dot(Vec2::new(0.4, 0.0), 1.0).expect("the rim is in");
        assert_eq!(&rim[..3], &STOP_RIM);
        assert!(stop_dot(Vec2::new(0.5, 0.0), 1.0).is_none());
        assert!(stop_dot(Vec2::new(0.4, 0.4), 1.0).is_none());
        // Pinned, it is fainter.
        assert!(stop_dot(Vec2::ZERO, STOP_PINNED_ALPHA).unwrap()[3] < 255.0);
    }

    #[test]
    fn the_station_dot_follows_the_run_along_its_route() {
        let map = super::super::data::MAPS
            .iter()
            .position(|m| m.name == "Undying Run")
            .expect("the Undying Run is a battleground");
        let def = super::super::route::route_of(map).expect("a route");
        let selection = super::super::Selection {
            stage: map,
            map,
            ..default()
        };
        let mut run = super::super::route::RouteRun {
            next: 1,
            stop: 1,
            arrived: false,
            placed: true,
        };
        let hero = def.points()[0].at;
        let to = def.points()[def.point_of(1)].at - hero;
        assert_eq!(stop_offset(&selection, &run, hero), Some(Vec2::new(to.x, to.z)));
        // The next station moves on with the stops reached.
        run.stop = 2;
        assert_ne!(stop_offset(&selection, &run, hero), Some(Vec2::new(to.x, to.z)));
        // Not on a route, not before the start, not once the end is reached.
        let zone = super::super::Selection::default();
        assert_eq!(stop_offset(&zone, &run, hero), None);
        let unplaced = super::super::route::RouteRun { placed: false, ..run.clone() };
        assert_eq!(stop_offset(&selection, &unplaced, hero), None);
        run.arrived = true;
        assert_eq!(stop_offset(&selection, &run, hero), None);
    }

    #[test]
    fn object_cells_tile_the_sheet() {
        assert_eq!(object_cell(0), [0.0, 0.25, 0.0, 0.25]);
        assert_eq!(object_cell(1), TRACKED_UNIT_CELL);
        assert_eq!(object_cell(4), [0.0, 0.25, 0.25, 0.5]);
    }
}
