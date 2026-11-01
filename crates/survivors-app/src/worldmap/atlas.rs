//! The world map's atlas: `WorldMapArea.dbc` places every zone and continent page on the world,
//! `WorldMapOverlay.dbc` lists the areas each page shows once explored, and the continents' `.zmp`
//! grids name the zone under a point. Loaded once, at the first opening of either map window;
//! [`spawn_page`] builds a page from its twelve tiles and every overlay into any UI node.
//!
//! [`project`], [`zone_for`], the overlay cutting in [`spawn_page`] and the box helpers [`at`] and
//! [`round`] follow matguitarist/hearthside (`crates/benilla-app/src/survivors/worldmap.rs` at
//! `7b689f96e84b6bc7ee1778b895de1d27c86735e8`, Apache-2.0).

use std::collections::{HashMap, HashSet};
use std::sync::{LazyLock, Mutex};

use bevy::prelude::*;

use benilla_app::survivors_host::area::AreaTableRes;
use benilla_assets::{LockRecover, WorldAssets};
use benilla_formats::{
    load_world_map_area_catalog, load_world_map_overlay_catalog, load_zone_map, AreaTableCatalog,
    Chain, WorldMapArea, WorldMapOverlay, WorldMapOverlayCatalog, ZONE_MAP_EDGE,
};

use crate::data::MapDef;

/// The page the WorldMap art covers in its own pixels: four 256-px tiles across, three down, the
/// last column and row cut short (`WorldMapFrame.xml`'s `WorldMapDetailTile1` to `12`).
pub(crate) const PAGE: Vec2 = Vec2::new(1002.0, 668.0);
pub(crate) const TILE: f32 = 256.0;

/// The marker art the map windows draw with: the minimap's arrow and blips, the boss skull, the
/// raid icons (the star in cell `[0, 0.25, 0, 0.25]`) and the party member's dot.
pub(crate) const ARROW_TEXTURE: &str =
    benilla_app::survivors_host::minimap::blips::PLAYER_ARROW_TEXTURE;
pub(crate) const OBJECT_ICONS: &str = "Interface\\Minimap\\ObjectIcons";
pub(crate) const SKULL_TEXTURE: &str = "Interface\\TargetingFrame\\UI-TargetingFrame-Skull";
pub(crate) const RAID_ICONS: &str = "Interface\\TargetingFrame\\UI-RaidTargetingIcons";
pub(crate) const PARTY_ICON: &str = "Interface\\WorldMap\\WorldMapPartyIcon";

/// The `0x4a6ec0` cell law's scale, 1 / (64 * 533.33 yd): a world coordinate to a grid fraction.
const GRID_K: f32 = 2.929_687_6e-5;

/// The world-map DBCs, loaded at the first opening of either window.
#[derive(Resource, Default)]
pub(crate) struct Atlas {
    loaded: bool,
    /// Every `WorldMapArea` row as `(id, row)`, in file order, cloned out of the catalog.
    areas: Vec<(u32, WorldMapArea)>,
    overlays: Option<WorldMapOverlayCatalog>,
    /// `.zmp` per continent map id, cells rolled up one step to their zone (`ui_world_map.rs:188`).
    grids: HashMap<u32, Vec<u32>>,
    /// Whether `grids` went through `AreaTable` yet: the table may come in after the first opening,
    /// and the grids are read again once it is there.
    rolled: bool,
}

impl Atlas {
    /// Load the DBCs once; a missing table logs one `warn!` and leaves it empty.
    pub(crate) fn ensure(&mut self, assets: &mut WorldAssets, areas: Option<&AreaTableRes>) {
        if self.loaded && (self.rolled || areas.is_none()) {
            return;
        }
        let mut chain = assets.chain.lock_recover();
        self.load(&mut chain, areas.map(|a| &a.0));
    }

    /// [`Self::ensure`] over a chain of its own, for a test without the asset server.
    pub(crate) fn load(&mut self, chain: &mut Chain, areas: Option<&AreaTableCatalog>) {
        if !self.loaded {
            self.loaded = true;
            match load_world_map_area_catalog(chain) {
                Ok(c) => self.areas = c.iter().map(|(id, a)| (id, a.clone())).collect(),
                Err(e) => warn!("survivors world map: WorldMapArea.dbc: {e:#}"),
            }
            self.overlays = load_world_map_overlay_catalog(chain)
                .inspect_err(|e| warn!("survivors world map: WorldMapOverlay.dbc: {e:#}"))
                .ok();
        }
        if self.rolled {
            return;
        }
        // The first read warns for a missing grid; the re-read with the area table stays quiet.
        let first = self.grids.is_empty();
        self.grids.clear();
        for (_, c) in self.areas.iter().filter(|(_, a)| a.area_id == 0) {
            match load_zone_map(chain, &c.name) {
                Ok(raw) => {
                    let grid = raw
                        .into_iter()
                        .map(|leaf| match areas.and_then(|t| t.get(leaf)) {
                            Some(row) if row.zone_id != 0 => row.zone_id,
                            _ => leaf,
                        })
                        .collect();
                    self.grids.insert(c.map_id, grid);
                }
                Err(e) if first => warn!("survivors world map: no zone grid for {}: {e:#}", c.name),
                Err(_) => {}
            }
        }
        self.rolled = areas.is_some();
    }

    /// The zone page of a battleground: the zone `standing` (an `AreaTable` zone id) if it has a
    /// page, else the zone the continent's grid names under `def.center`, else the smallest zone
    /// page of `def.map` holding the centre, else the continent. The grid comes before the rects:
    /// a page shows its neighbours' edges, so the smallest rect over Westfall is Elwynn's.
    pub(crate) fn page_for(
        &self,
        def: &MapDef,
        standing: Option<u32>,
    ) -> Option<(u32, &WorldMapArea)> {
        let [wx, wy, _] = def.center;
        let page_of = |zone: u32| {
            self.rows()
                .find(|(_, a)| a.map_id == def.map && a.area_id == zone && zone != 0)
        };
        standing
            .and_then(page_of)
            .or_else(|| self.zone_at(def.map, wx, wy).and_then(page_of))
            .or_else(|| zone_for(self.rows(), def.map, None, def.center))
            .or_else(|| self.continent(def.map))
    }

    /// The continent page of a `Map.dbc` id (`area_id == 0`).
    pub(crate) fn continent(&self, map_id: u32) -> Option<(u32, &WorldMapArea)> {
        self.rows()
            .find(|(_, a)| a.map_id == map_id && a.area_id == 0)
    }

    /// The page with `WorldMapArea` id `id`.
    pub(crate) fn page(&self, id: u32) -> Option<&WorldMapArea> {
        self.rows().find(|&(i, _)| i == id).map(|(_, a)| a)
    }

    /// Every page as `(id, row)`, in file order.
    pub(crate) fn rows(&self) -> impl Iterator<Item = (u32, &WorldMapArea)> + Clone {
        self.areas.iter().map(|(id, a)| (*id, a))
    }

    /// The explored areas drawn over page `id`.
    pub(crate) fn overlays(&self, id: u32) -> &[WorldMapOverlay] {
        self.overlays.as_ref().map_or(&[], |o| o.for_area(id))
    }

    /// The zone (`AreaTable` id) under WoW `(wx, wy)` on continent `map_id`, from its `.zmp`.
    pub(crate) fn zone_at(&self, map_id: u32, wx: f32, wy: f32) -> Option<u32> {
        let grid = self.grids.get(&map_id)?;
        grid_cell(wx, wy)
            .and_then(|i| grid.get(i).copied())
            .filter(|&z| z != 0)
    }
}

/// The zone the hero stands in, as the minimap's zone line reads it: the area under the hero
/// (`WorldPoint::area`) rolled up to its zone, or the area itself when it is a zone.
pub(crate) fn standing(leaf: Option<u32>, areas: Option<&AreaTableRes>) -> Option<u32> {
    let leaf = leaf?;
    Some(match areas.and_then(|a| a.0.get(leaf)) {
        Some(row) if row.zone_id != 0 => row.zone_id,
        _ => leaf,
    })
}

/// Where WoW `(wx, wy)` falls on a page, 0..1 across (east) and down (south): x runs north (up
/// the page), y west (left).
pub(crate) fn project(page: &WorldMapArea, wx: f32, wy: f32) -> Vec2 {
    Vec2::new(
        (page.loc_left - wy) / (page.loc_left - page.loc_right),
        (page.loc_top - wx) / (page.loc_top - page.loc_bottom),
    )
}

/// The inverse: WoW `(wx, wy)` at page fraction `uv`.
pub(crate) fn unproject(page: &WorldMapArea, uv: Vec2) -> (f32, f32) {
    (
        page.loc_top - uv.y * (page.loc_top - page.loc_bottom),
        page.loc_left - uv.x * (page.loc_left - page.loc_right),
    )
}

/// The `.zmp` cell under WoW `(wx, wy)`: `area_grid_cell` (`benilla-ui/src/script/worldmap.rs:84`)
/// with the page lerp folded out, `col + 128 * row` of `0.5 - w * 2.929_687_6e-5`.
pub(crate) fn grid_cell(wx: f32, wy: f32) -> Option<usize> {
    let across = 0.5 - wy * GRID_K;
    let down = 0.5 - wx * GRID_K;
    if !(0.0..=1.0).contains(&across) || !(0.0..=1.0).contains(&down) {
        return None;
    }
    let edge = ZONE_MAP_EDGE as f32;
    let (col, row) = ((across * edge) as usize, (down * edge) as usize);
    Some(col + ZONE_MAP_EDGE * row).filter(|&i| i < ZONE_MAP_EDGE * ZONE_MAP_EDGE)
}

/// The page choice behind [`Atlas::page_for`], over plain rows so a test can feed it: the page of
/// the zone `standing` (an `AreaTable` id), else the smallest zone page of `map` that holds
/// `center` (WoW coordinates), as the rects overlap (Mulgore's reaches into The Barrens).
pub(crate) fn zone_for<'a>(
    pages: impl Iterator<Item = (u32, &'a WorldMapArea)> + Clone,
    map: u32,
    standing: Option<u32>,
    center: [f32; 3],
) -> Option<(u32, &'a WorldMapArea)> {
    let zones = pages.filter(move |(_, a)| a.map_id == map && a.area_id != 0);
    if let Some(found) = zones.clone().find(|(_, a)| Some(a.area_id) == standing) {
        return Some(found);
    }
    let size = |z: &WorldMapArea| ((z.loc_left - z.loc_right) * (z.loc_top - z.loc_bottom)).abs();
    zones
        .filter(|(_, a)| {
            let p = project(a, center[0], center[1]);
            (0.0..=1.0).contains(&p.x) && (0.0..=1.0).contains(&p.y)
        })
        .min_by(|(_, a), (_, b)| size(a).total_cmp(&size(b)))
}

/// The page's twelve tiles and every overlay (as if all of it were explored), at `scale`
/// canvas units per art pixel, as children of `parent`. Missing art is skipped.
pub(crate) fn spawn_page(
    parent: &mut ChildSpawnerCommands,
    assets: &mut WorldAssets,
    images: &mut Assets<Image>,
    page: &WorldMapArea,
    overlays: &[WorldMapOverlay],
    scale: f32,
) {
    let tile = TILE * scale;
    for i in 0..12 {
        let path = format!("Interface\\WorldMap\\{0}\\{0}{1}", page.name, i + 1);
        let Some(image) = assets.sprite_texture(&path, images) else {
            continue;
        };
        let (col, row) = ((i % 4) as f32, (i / 4) as f32);
        parent.spawn((
            ImageNode::new(image),
            at(Vec2::new(col, row) * tile, tile, tile),
        ));
    }
    for o in overlays {
        overlay(parent, assets, images, &page.name, o, scale);
    }
}

/// One explored area's art, cut into 256-px pieces as `WorldMapFrame_Update` cuts it: numbered
/// across then down, the last row and column only as wide as what is left, each in a file
/// rounded up to a power of two.
fn overlay(
    parent: &mut ChildSpawnerCommands,
    assets: &mut WorldAssets,
    images: &mut Assets<Image>,
    zone: &str,
    o: &WorldMapOverlay,
    scale: f32,
) {
    if o.texture_name.is_empty() || o.texture_width == 0 || o.texture_height == 0 {
        return;
    }
    let (w, h) = (o.texture_width as f32, o.texture_height as f32);
    let across = (w / TILE).ceil() as u32;
    let down = (h / TILE).ceil() as u32;
    for j in 0..down {
        for k in 0..across {
            let path = format!(
                "Interface\\WorldMap\\{zone}\\{}{}",
                o.texture_name,
                j * across + k + 1
            );
            let Some(image) = assets.sprite_texture(&path, images) else {
                continue;
            };
            let pw = (w - k as f32 * TILE).min(TILE);
            let ph = (h - j as f32 * TILE).min(TILE);
            let p = Vec2::new(
                o.offset_x as f32 + k as f32 * TILE,
                o.offset_y as f32 + j as f32 * TILE,
            );
            parent.spawn((
                ImageNode {
                    rect: Some(Rect::new(0.0, 0.0, pw, ph)),
                    ..ImageNode::new(image)
                },
                at(p * scale, pw * scale, ph * scale),
            ));
        }
    }
}

/// An absolutely placed box: its top left corner at `p` in the parent, `w` by `h`.
pub(crate) fn at(p: Vec2, w: f32, h: f32) -> Node {
    Node {
        position_type: PositionType::Absolute,
        left: Val::Px(p.x),
        top: Val::Px(p.y),
        width: Val::Px(w),
        height: Val::Px(h),
        ..default()
    }
}

/// The same, an ellipse.
pub(crate) fn round(p: Vec2, w: f32, h: f32) -> Node {
    Node {
        border_radius: BorderRadius::MAX,
        ..at(p, w, h)
    }
}

/// A marker image from a sheet cell `[left, right, top, bottom]` (0..1), or `None` while the
/// sheet is missing; the caller falls back to a dot. A missing sheet warns once per path.
pub(crate) fn cell_image(
    assets: &mut WorldAssets,
    images: &mut Assets<Image>,
    path: &str,
    cell: [f32; 4],
) -> Option<ImageNode> {
    static MISSING: LazyLock<Mutex<HashSet<String>>> = LazyLock::new(Default::default);
    let found = assets
        .sprite_texture(path, images)
        .and_then(|h| images.get(&h).map(|i| (h, i.size_f32())));
    let Some((handle, size)) = found else {
        if MISSING.lock_recover().insert(path.to_string()) {
            warn!("survivors world map: no marker art at {path}, drawn as a dot");
        }
        return None;
    };
    let [l, r, t, b] = cell;
    Some(ImageNode {
        rect: Some(Rect::new(l * size.x, t * size.y, r * size.x, b * size.y)),
        ..ImageNode::new(handle)
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn row(left: f32, right: f32, top: f32, bottom: f32) -> WorldMapArea {
        WorldMapArea {
            map_id: 1,
            area_id: 17,
            name: "Barrens".into(),
            loc_left: left,
            loc_right: right,
            loc_top: top,
            loc_bottom: bottom,
        }
    }

    #[test]
    fn north_is_up_and_west_is_left() {
        let z = row(100.0, -100.0, 200.0, 0.0);
        assert_eq!(project(&z, 200.0, 100.0), Vec2::ZERO);
        assert_eq!(project(&z, 0.0, -100.0), Vec2::ONE);
        assert_eq!(project(&z, 100.0, 0.0), Vec2::splat(0.5));
        let (wx, wy) = unproject(&z, Vec2::new(0.25, 0.75));
        assert!((project(&z, wx, wy) - Vec2::new(0.25, 0.75)).length() < 1e-5);
    }

    #[test]
    fn the_smallest_page_holding_the_centre_wins() {
        let big = WorldMapArea {
            area_id: 17,
            ..row(1000.0, -1000.0, 1000.0, -1000.0)
        };
        let small = WorldMapArea {
            area_id: 215,
            ..row(100.0, -100.0, 100.0, -100.0)
        };
        // Another continent's page over the same rect never counts.
        let elsewhere = WorldMapArea {
            map_id: 0,
            area_id: 12,
            ..row(50.0, -50.0, 50.0, -50.0)
        };
        let rows = [(1, big), (2, small), (3, elsewhere)];
        let pages = || rows.iter().map(|(id, a)| (*id, a));
        assert_eq!(
            zone_for(pages(), 1, None, [0.0, 0.0, 0.0]).map(|p| p.0),
            Some(2)
        );
        // Standing in the big one's zone picks it, though the small one holds the centre too.
        assert_eq!(
            zone_for(pages(), 1, Some(17), [0.0, 0.0, 0.0]).map(|p| p.0),
            Some(1)
        );
        // A zone with no page on this continent falls back to the centre.
        assert_eq!(
            zone_for(pages(), 1, Some(12), [0.0, 0.0, 0.0]).map(|p| p.0),
            Some(2)
        );
        assert!(zone_for(pages(), 1, None, [5000.0, 0.0, 0.0]).is_none());
    }

    #[test]
    fn grid_cell_matches_the_engine_cell() {
        // Goldshire (-9450, 60) is cell 12735 (benilla-formats zone_map.rs test).
        assert_eq!(grid_cell(-9450.0, 60.0), Some(12735));
        // Off the 64 by 64 tile world.
        assert_eq!(grid_cell(-20000.0, 0.0), None);
    }

    #[test]
    fn standing_rolls_a_subzone_up_once() {
        assert_eq!(standing(None, None), None);
        // Without the area table the leaf is taken as it is.
        assert_eq!(standing(Some(87), None), Some(87));
    }

    /// Every battleground finds a page, its centre on it, and the page's art is in the data.
    /// `--nocapture` prints which page each one gets.
    #[test]
    fn every_battleground_has_a_page() {
        let data = benilla_formats::wow_data_or_skip!();
        let mut chain = benilla_formats::open_chain(&data).expect("open chain");
        let areas = benilla_formats::load_area_table_catalog(&mut chain).ok();
        let mut atlas = Atlas::default();
        atlas.load(&mut chain, areas.as_ref());
        assert!(atlas.rows().count() > 0, "WorldMapArea loads");
        assert!(atlas.continent(0).is_some() && atlas.continent(1).is_some());
        for def in crate::data::MAPS {
            let (id, page) = atlas
                .page_for(def, None)
                .unwrap_or_else(|| panic!("{}: no page", def.name));
            let uv = project(page, def.center[0], def.center[1]);
            let kind = if page.area_id == 0 {
                "continent"
            } else {
                "zone"
            };
            let grid = atlas.zone_at(def.map, def.center[0], def.center[1]);
            println!(
                "{:<24} -> {:<20} ({kind} page {id}, at {:.2} {:.2}, {} overlays, grid zone {grid:?})",
                def.name,
                page.name,
                uv.x,
                uv.y,
                atlas.overlays(id).len()
            );
            // The grid's zone wins whenever it has a page of its own.
            if let Some(zone) = grid.filter(|&z| atlas.rows().any(|(_, a)| a.area_id == z)) {
                assert_eq!(
                    page.area_id, zone,
                    "{}: page {} is not the grid's zone",
                    def.name, page.name
                );
            }
            assert!(
                (0.0..=1.0).contains(&uv.x) && (0.0..=1.0).contains(&uv.y),
                "{}: centre off its page {} ({uv})",
                def.name,
                page.name
            );
            let tile = format!("Interface\\WorldMap\\{0}\\{0}1.blp", page.name);
            assert!(chain.contains(&tile), "{}: no {tile}", def.name);
        }
    }

    /// The Goldshire cell of the Eastern Kingdoms grid is Elwynn Forest, through the atlas.
    #[test]
    fn the_atlas_names_the_zone_under_a_point() {
        let data = benilla_formats::wow_data_or_skip!();
        let mut chain = benilla_formats::open_chain(&data).expect("open chain");
        let areas = benilla_formats::load_area_table_catalog(&mut chain).ok();
        let mut atlas = Atlas::default();
        atlas.load(&mut chain, areas.as_ref());
        assert_eq!(atlas.zone_at(0, -9450.0, 60.0), Some(12));
    }
}
