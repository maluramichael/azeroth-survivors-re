//! Walkability and pathfinding for the horde: a grid over the battleground marking the cells that
//! buildings, trees and other solid world pieces block, and a flow field over it toward the hero,
//! so enemies walk around obstacles instead of through them.
//!
//! The mask is probed once per battleground with a body-sized capsule at walking height against
//! the world's colliders, a few milliseconds a frame, nearest the arena centre first; cells not
//! probed yet read as free. The terrain floor is not an obstacle: the capsule rides above it, so
//! only a slope steeper than about 55 degrees, a building, a tree or a rock reaches it. The flow
//! field is a distance field over a window around the hero, rebuilt a few times a second; an
//! enemy reads it by walking a few cells downhill and heading for the farthest one it can see.
//!
//! A spawn does not trust the mask alone, which reads free where the scan has not been yet or
//! where colliders streamed in after it: [`SpawnProbe`] asks the world right at the spot, and the
//! field says whether the spot is joined to the hero, so nothing is set down inside a trunk, a
//! wall, or a sealed room. One that ends up there anyway is [`stuck`](NavGrid::stuck).

use std::time::Duration;

use avian3d::prelude::{Collider, SpatialQuery, SpatialQueryFilter};
use benilla_world::collision::{ColliderEpoch, WorldCollision};
use benilla_world::schedule::WorldStage;
use benilla_world::terrain_stream::WorldLoadProgress;
use benilla_world::world_point::WorldPoint;
use bevy::ecs::system::SystemParam;
use bevy::platform::time::Instant;
use bevy::prelude::*;

use super::hero::{arena, Hero};
use super::units::flat;
use super::{RunEntity, Selection};

/// Yards per cell.
const CELL: f32 = 1.0;
/// Yards past the leash the grid still covers: the horde spawns and mills outside the barrier.
const MARGIN: f32 = 24.0;

/// The probe, a capsule standing on the ground: radius, and the heights its underside and top sit
/// at above the feet. The underside is lifted so the terrain floor and a kerb pass under it.
const BODY_RADIUS: f32 = 0.6;
const BODY_BOTTOM: f32 = 0.4;
const BODY_TOP: f32 = 1.8;
/// The height of the probe's centre above the feet.
const BODY_MID: f32 = (BODY_BOTTOM + BODY_TOP) * 0.5;

/// Most the scan spends in a frame.
const SCAN_BUDGET: Duration = Duration::from_micros(2000);
/// Frames the collider set must hold still before a scan starts, so the trees and houses around
/// the arena are in the world's spatial tree.
const SETTLE_FRAMES: u32 = 30;
/// Frames of quiet before the world is scanned again after more colliders streamed in, and how
/// many times that may happen on one battleground.
const REFRESH_QUIET: u32 = 120;
const MAX_REFRESHES: u32 = 4;

/// Half the field's window, in cells: past it the horde heads straight for the hero.
const FIELD_RADIUS: i32 = 64;
/// Cells an enemy looks ahead along the field for a waypoint it can see.
const LOOKAHEAD: usize = 6;
/// Fewest seconds between field rebuilds, and the most a stale one is kept.
const FLOW_EVERY: f32 = 0.15;
/// Rings searched for the nearest free cell around a blocked one.
const FREE_SEARCH: i32 = 8;
/// Fewest cells a field must reach before "the field did not reach this cell" means walled off,
/// not a hero shut in a closet whose horde has nothing to compare with.
const MIN_REACH: usize = 400;

/// Path costs in tenths of a cell: a step along an axis and across a diagonal.
const STRAIGHT: u32 = 10;
const DIAGONAL: u32 = 14;
/// Dial's queue: edge costs are under 16, so 16 buckets hold every pending distance.
const BUCKETS: usize = 16;
/// A cell the field has not reached.
const UNREACHED: u16 = u16::MAX;

const NEIGHBOURS: [(i32, i32, u32); 8] = [
    (1, 0, STRAIGHT),
    (-1, 0, STRAIGHT),
    (0, 1, STRAIGHT),
    (0, -1, STRAIGHT),
    (1, 1, DIAGONAL),
    (1, -1, DIAGONAL),
    (-1, 1, DIAGONAL),
    (-1, -1, DIAGONAL),
];

/// The battleground as a grid in the ground plane (Bevy X and Z), which cells are solid, and the
/// field toward the hero. A cell is solid when a body standing at its centre would touch the
/// world; anything off the grid, or not probed yet, reads as free.
#[derive(Resource, Default)]
pub(super) struct NavGrid {
    /// The [`Selection::stage`] this grid was made for.
    stage: Option<usize>,
    /// The arena centre and the radius of the circle probed, in yards.
    center: Vec2,
    radius: f32,
    /// The ground-plane corner of cell (0, 0).
    origin: Vec2,
    /// Cells per side, and half of it: the centre cell the scan rings around.
    w: i32,
    h: i32,
    half: i32,
    solid: Vec<bool>,
    /// Bumped whenever a cell flips, so the field notices the mask moved.
    version: u32,
    field: Field,
}

/// Distance to the hero (tenths of a cell) over a window of the grid.
#[derive(Default)]
struct Field {
    /// The grid cell the window's corner is.
    min: IVec2,
    w: i32,
    h: i32,
    dist: Vec<u16>,
    buckets: Vec<Vec<IVec2>>,
    /// The cell the field flows to; `None` when the hero is off the grid.
    source: Option<IVec2>,
    /// The hero's own cell, the grid version and the seconds at the last rebuild.
    hero: IVec2,
    version: u32,
    age: f32,
    /// Cells the field reached.
    reached: usize,
}

impl Field {
    /// The distance at a grid cell, `None` off the window or where the field did not reach.
    fn dist(&self, c: IVec2) -> Option<u16> {
        let l = c - self.min;
        if l.x < 0 || l.y < 0 || l.x >= self.w || l.y >= self.h {
            return None;
        }
        let d = self.dist[(l.y * self.w + l.x) as usize];
        (d != UNREACHED).then_some(d)
    }

    /// Whether the field reached a grid cell: `None` when it cannot say (no field, a field that
    /// reached too little to judge by, or a cell off its window).
    fn reaches(&self, c: IVec2) -> Option<bool> {
        if self.source.is_none() || self.reached < MIN_REACH {
            return None;
        }
        let l = c - self.min;
        if l.x < 0 || l.y < 0 || l.x >= self.w || l.y >= self.h {
            return None;
        }
        Some(self.dist[(l.y * self.w + l.x) as usize] != UNREACHED)
    }
}

impl NavGrid {
    /// An open grid of `w` by `h` cells with cell (0, 0) at `origin`.
    fn new(stage: usize, origin: Vec2, w: i32, h: i32) -> Self {
        Self {
            stage: Some(stage),
            center: origin + Vec2::new(w as f32, h as f32) * 0.5 * CELL,
            radius: f32::MAX,
            origin,
            w,
            h,
            half: w / 2,
            solid: vec![false; (w * h) as usize],
            ..default()
        }
    }

    /// The grid cell holding a world position.
    fn cell(&self, p: Vec3) -> IVec2 {
        IVec2::new(
            ((p.x - self.origin.x) / CELL).floor() as i32,
            ((p.z - self.origin.y) / CELL).floor() as i32,
        )
    }

    /// The centre of a cell, at height 0.
    fn center_of(&self, c: IVec2) -> Vec3 {
        Vec3::new(
            self.origin.x + (c.x as f32 + 0.5) * CELL,
            0.0,
            self.origin.y + (c.y as f32 + 0.5) * CELL,
        )
    }

    fn inside(&self, c: IVec2) -> bool {
        c.x >= 0 && c.y >= 0 && c.x < self.w && c.y < self.h
    }

    fn at(&self, c: IVec2) -> usize {
        (c.y * self.w + c.x) as usize
    }

    /// Whether a cell is on the grid and solid.
    fn is_solid(&self, c: IVec2) -> bool {
        self.inside(c) && self.solid[self.at(c)]
    }

    fn set(&mut self, c: IVec2, solid: bool) {
        let i = self.at(c);
        if self.solid[i] != solid {
            self.solid[i] = solid;
            self.version = self.version.wrapping_add(1);
        }
    }

    /// Whether a walker may go from cell `c` to its neighbour `n`: `n` is free, and a diagonal
    /// does not cut the corner of a solid cell.
    fn passable(&self, c: IVec2, n: IVec2) -> bool {
        self.inside(n)
            && !self.is_solid(n)
            && (n.x == c.x
                || n.y == c.y
                || (!self.is_solid(IVec2::new(n.x, c.y)) && !self.is_solid(IVec2::new(c.x, n.y))))
    }

    /// Whether the world blocks a body at `pos`. Off the grid, or before the scan reached it, a
    /// position is free.
    #[allow(dead_code)]
    pub fn blocked(&self, pos: Vec3) -> bool {
        self.is_solid(self.cell(pos))
    }

    /// The nearest free cell to `to` within `max` rings of `c`, one the field reaches when it can
    /// tell: a sealed room next door is no way out.
    fn nearest_free(&self, c: IVec2, max: i32, to: Vec3) -> Option<IVec2> {
        for r in 1..=max {
            let mut best: Option<(f32, IVec2)> = None;
            for k in 0..ring_len(r) {
                let n = c + ring_cell(r, k);
                if !self.inside(n) || self.is_solid(n) || self.field.reaches(n) == Some(false) {
                    continue;
                }
                let d = flat(self.center_of(n) - to).length_squared();
                if best.is_none_or(|(b, _)| d < b) {
                    best = Some((d, n));
                }
            }
            if let Some((_, n)) = best {
                return Some(n);
            }
        }
        None
    }

    /// Whether a spawn may stand at `pos` as far as the mask knows: its cell is free and, where
    /// the field can tell, joined to the hero. The world itself is [`SpawnProbe`]'s to ask.
    pub fn fits(&self, pos: Vec3) -> bool {
        let c = self.cell(pos);
        !self.is_solid(c) && self.field.reaches(c) != Some(false)
    }

    /// Whether a walker at `pos` has no way to the hero by walking: it stands in solid space with
    /// no free cell near, or in a free pocket the field does not reach. The caller sets it down
    /// elsewhere. A walker in a wall with free ground near is not stuck: [`chase`](Self::chase)
    /// and [`step`](Self::step) walk it out.
    pub fn stuck(&self, pos: Vec3) -> bool {
        if self.solid.is_empty() {
            return false;
        }
        let c = self.cell(pos);
        if self.is_solid(c) {
            return self.nearest_free(c, FREE_SEARCH, pos).is_none();
        }
        self.field.reaches(c) == Some(false)
    }

    /// The point a walker at `from` reaches moving toward `to` (height of `to`) without entering
    /// a solid cell or cutting its corner: it slides along the free axis when the way is shut. A
    /// walker already inside a solid cell moves freely, so it can leave.
    pub fn step(&self, from: Vec3, to: Vec3) -> Vec3 {
        if self.solid.is_empty() {
            return to;
        }
        // A long move is taken in sub-steps under a cell, so it cannot jump a thin wall.
        let d = flat(to - from);
        let n = (d.length() / (CELL * 0.9)).ceil().max(1.0);
        let mut p = from;
        for i in 1..=n as u32 {
            let t = from.lerp(to, i as f32 / n);
            let q = self.step_one(p, t);
            if flat(q - t).length_squared() > 1e-8 {
                // Blocked or slid: the rest of the move would aim from the wrong place.
                return q;
            }
            p = q;
        }
        p
    }

    /// One sub-step of [`step`](Self::step).
    fn step_one(&self, from: Vec3, to: Vec3) -> Vec3 {
        let here = self.cell(from);
        if self.is_solid(here) {
            return to;
        }
        let free = |p: Vec3| {
            let c = self.cell(p);
            c == here || self.passable(here, c)
        };
        if free(to) {
            return to;
        }
        let d = to - from;
        let along_x = Vec3::new(to.x, to.y, from.z);
        let along_z = Vec3::new(from.x, to.y, to.z);
        let (a, b) = if d.x.abs() >= d.z.abs() {
            (along_x, along_z)
        } else {
            (along_z, along_x)
        };
        if free(a) {
            a
        } else if free(b) {
            b
        } else {
            Vec3::new(from.x, to.y, from.z)
        }
    }

    /// Whether the straight line from `a` to `b` crosses only free cells. The two end cells are
    /// not tested, so a hero hugging a wall is still seen and an enemy standing in one can leave.
    pub fn line_clear(&self, a: Vec3, b: Vec3) -> bool {
        if self.solid.is_empty() {
            return true;
        }
        let p0 = Vec2::new(a.x - self.origin.x, a.z - self.origin.y) / CELL;
        let p1 = Vec2::new(b.x - self.origin.x, b.z - self.origin.y) / CELL;
        let (mut x, mut z) = (p0.x.floor() as i32, p0.y.floor() as i32);
        let (ex, ez) = (p1.x.floor() as i32, p1.y.floor() as i32);
        let d = p1 - p0;
        let (sx, sz) = (
            if d.x > 0.0 { 1 } else { -1 },
            if d.y > 0.0 { 1 } else { -1 },
        );
        let delta_x = if d.x != 0.0 {
            (1.0 / d.x).abs()
        } else {
            f32::MAX
        };
        let delta_z = if d.y != 0.0 {
            (1.0 / d.y).abs()
        } else {
            f32::MAX
        };
        let mut t_x = match d.x {
            v if v > 0.0 => (x as f32 + 1.0 - p0.x) / v,
            v if v < 0.0 => (p0.x - x as f32) / -v,
            _ => f32::MAX,
        };
        let mut t_z = match d.y {
            v if v > 0.0 => (z as f32 + 1.0 - p0.y) / v,
            v if v < 0.0 => (p0.y - z as f32) / -v,
            _ => f32::MAX,
        };
        let (sx0, sz0) = (x, z);
        // Bounded by the cells the line can cross, so a float slip cannot loop.
        for _ in 0..((ex - sx0).abs() + (ez - sz0).abs() + 2) {
            if x == ex && z == ez {
                return true;
            }
            if t_x < t_z {
                t_x += delta_x;
                x += sx;
            } else {
                t_z += delta_z;
                z += sz;
            }
            if (x != ex || z != ez) && self.is_solid(IVec2::new(x, z)) {
                return false;
            }
        }
        true
    }

    /// The unit direction (ground plane) from `from` along the flow field toward the hero: the
    /// farthest of the next few cells downhill that `from` can see. `None` when the field has no
    /// value there (off its window, unreachable, or the hero's own cell), where the caller heads
    /// straight for the hero.
    pub fn flow(&self, from: Vec3) -> Option<Vec3> {
        self.field.source?;
        let mut at = self.cell(from);
        if self.is_solid(at) || self.field.dist(at)? == 0 {
            return None;
        }
        let mut path = [IVec2::ZERO; LOOKAHEAD];
        let mut n = 0;
        while n < LOOKAHEAD {
            let Some(next) = self.downhill(at) else { break };
            path[n] = next;
            n += 1;
            at = next;
        }
        // The first step is taken whether or not it is seen: it is the way the field goes.
        for i in (0..n).rev() {
            let wp = self.center_of(path[i]);
            if i == 0 || self.line_clear(from, wp) {
                return flat(wp - from).try_normalize();
            }
        }
        None
    }

    /// The neighbour of `c` the field falls fastest toward.
    fn downhill(&self, c: IVec2) -> Option<IVec2> {
        let mut best_d = self.field.dist(c)?;
        let mut best = None;
        for (dx, dz, _) in NEIGHBOURS {
            let n = c + IVec2::new(dx, dz);
            if !self.passable(c, n) {
                continue;
            }
            if let Some(d) = self.field.dist(n) {
                if d < best_d {
                    best_d = d;
                    best = Some(n);
                }
            }
        }
        best
    }

    /// The unit direction (ground plane) a walker at `from` should take to reach `to`: out of the
    /// wall when it stands in one, straight when it sees the target, else along the flow field,
    /// and straight again when the field has no value.
    pub fn chase(&self, from: Vec3, to: Vec3) -> Vec3 {
        let straight = flat(to - from).normalize_or_zero();
        if self.solid.is_empty() {
            return straight;
        }
        let c = self.cell(from);
        if self.is_solid(c) {
            return self
                .nearest_free(c, FREE_SEARCH, from)
                .and_then(|n| flat(self.center_of(n) - from).try_normalize())
                .unwrap_or(straight);
        }
        if self.line_clear(from, to) {
            return straight;
        }
        self.flow(from).unwrap_or(straight)
    }

    /// Rebuilds the field when the hero changed cell or the mask moved, at most every
    /// [`FLOW_EVERY`] seconds.
    fn track(&mut self, hero: Vec3, dt: f32) {
        self.field.age += dt;
        if self.solid.is_empty() {
            return;
        }
        let c = self.cell(hero);
        let stale = self.field.source.is_none()
            || c != self.field.hero
            || self.version != self.field.version;
        if stale && self.field.age >= FLOW_EVERY {
            self.rebuild_field(c);
        }
    }

    /// The distance from `hero` (a grid cell) to every cell of a window around it, by Dial's
    /// bucket queue over eight neighbours with no corner cutting.
    fn rebuild_field(&mut self, hero: IVec2) {
        let mut f = std::mem::take(&mut self.field);
        f.source = None;
        f.hero = hero;
        f.version = self.version;
        f.age = 0.0;
        f.reached = 0;
        let src = if !self.inside(hero) {
            None
        } else if !self.is_solid(hero) {
            Some(hero)
        } else {
            self.nearest_free(hero, FREE_SEARCH, self.center_of(hero))
        };
        if let Some(src) = src {
            f.min = (src - IVec2::splat(FIELD_RADIUS)).max(IVec2::ZERO);
            let max = (src + IVec2::splat(FIELD_RADIUS)).min(IVec2::new(self.w - 1, self.h - 1));
            f.w = max.x - f.min.x + 1;
            f.h = max.y - f.min.y + 1;
            f.dist.clear();
            f.dist.resize((f.w * f.h) as usize, UNREACHED);
            f.buckets.resize_with(BUCKETS, Vec::new);
            let (min, w) = (f.min, f.w);
            let slot = |c: IVec2| ((c.y - min.y) * w + (c.x - min.x)) as usize;
            let in_window = |c: IVec2| c.x >= min.x && c.y >= min.y && c.x <= max.x && c.y <= max.y;
            f.dist[slot(src)] = 0;
            f.buckets[0].push(src);
            let mut pending = 1usize;
            let mut cost = 0u32;
            while pending > 0 {
                let b = cost as usize % BUCKETS;
                // An edge costs under a full lap of buckets, so nothing lands in this one while
                // it drains.
                let mut now = std::mem::take(&mut f.buckets[b]);
                pending -= now.len();
                for c in now.drain(..) {
                    // Stale: a cheaper way reached this cell after it was queued.
                    if u32::from(f.dist[slot(c)]) != cost {
                        continue;
                    }
                    f.reached += 1;
                    for (dx, dz, step) in NEIGHBOURS {
                        let n = c + IVec2::new(dx, dz);
                        if !in_window(n) || !self.passable(c, n) {
                            continue;
                        }
                        let nd = cost + step;
                        if nd < u32::from(f.dist[slot(n)]) && nd < u32::from(UNREACHED) {
                            f.dist[slot(n)] = nd as u16;
                            f.buckets[nd as usize % BUCKETS].push(n);
                            pending += 1;
                        }
                    }
                }
                f.buckets[b] = now;
                cost += 1;
            }
            f.source = Some(src);
        }
        self.field = f;
    }
}

/// Cells in the square ring `r` cells from its centre.
fn ring_len(r: i32) -> i32 {
    if r == 0 {
        1
    } else {
        8 * r
    }
}

/// The `k`th cell (of [`ring_len`]) of the ring `r` from the origin, clockwise from the corner.
fn ring_cell(r: i32, k: i32) -> IVec2 {
    if r == 0 {
        return IVec2::ZERO;
    }
    let side = 2 * r;
    let j = k % side;
    match k / side {
        0 => IVec2::new(-r + j, -r),
        1 => IVec2::new(r, -r + j),
        2 => IVec2::new(r - j, r),
        _ => IVec2::new(-r, r - j),
    }
}

/// Where a scan stands, and what it last saw of the world.
#[derive(Default)]
struct Scan {
    /// The collider epoch last seen, and the frames it has held.
    seen: u64,
    quiet: u32,
    /// The epoch of the last scan started, and how many repeats this battleground has had.
    scanned: Option<u64>,
    refreshes: u32,
    running: bool,
    /// The ring and the cell in it the scan is at.
    ring: i32,
    k: i32,
}

/// The probe: a capsule the size of a walker, centred on [`BODY_MID`].
fn body_probe() -> Collider {
    Collider::capsule(BODY_RADIUS, BODY_TOP - BODY_BOTTOM - 2.0 * BODY_RADIUS)
}

/// The world asked directly whether a walker fits at a spot, for a spawn: one capsule overlap
/// query, against the colliders as they are now.
#[derive(SystemParam)]
pub(super) struct SpawnProbe<'w, 's> {
    spatial: SpatialQuery<'w, 's>,
    run: Query<'w, 's, (), With<RunEntity>>,
}

impl SpawnProbe<'_, '_> {
    /// Whether no solid world collider touches a walker standing with its feet at `feet`.
    pub fn clear(&self, feet: Vec3) -> bool {
        !solid_at(
            &self.spatial,
            &self.run,
            &body_probe(),
            &WorldCollision::body_filter(),
            feet + Vec3::Y * BODY_MID,
        )
    }
}

/// Whether a solid, non-run collider touches the probe at `at`.
fn solid_at(
    spatial: &SpatialQuery,
    run: &Query<(), With<RunEntity>>,
    probe: &Collider,
    filter: &SpatialQueryFilter,
    at: Vec3,
) -> bool {
    let mut hit = false;
    spatial.shape_intersections_callback(probe, at, Quat::IDENTITY, filter, |e| {
        // The run's own objects (pickups with a hull) are not terrain.
        if run.contains(e) {
            return true;
        }
        hit = true;
        false
    });
    hit
}

/// Probes the world around the arena into the mask. A battleground is scanned once its world is
/// in and its colliders have held still, and again (a few times at most) when more colliders
/// streamed in after; the cells nearest the centre go first.
#[allow(clippy::too_many_arguments)]
fn scan_world(
    selection: Res<Selection>,
    loading: Res<benilla_app::survivors_host::loading_screen::LoadingScreen>,
    progress: Res<WorldLoadProgress>,
    epoch: Res<ColliderEpoch>,
    world: WorldPoint,
    spatial: SpatialQuery,
    run_entities: Query<(), With<RunEntity>>,
    mut grid: ResMut<NavGrid>,
    mut scan: Local<Scan>,
) {
    if grid.stage != Some(selection.stage) {
        let map = selection.stage_def();
        let radius = map.leash + MARGIN;
        let c = arena(map);
        let half = (radius / CELL).ceil() as i32;
        let origin = Vec2::new(c.x, c.z) - Vec2::splat(half as f32 * CELL);
        *grid = NavGrid::new(selection.stage, origin, half * 2, half * 2);
        grid.center = Vec2::new(c.x, c.z);
        grid.radius = radius;
        *scan = Scan::default();
    }
    let now = epoch.get();
    if now != scan.seen {
        scan.seen = now;
        scan.quiet = 0;
    } else {
        scan.quiet = scan.quiet.saturating_add(1);
    }
    if !scan.running {
        let ready = !loading.covering() && progress.presentable() && progress.focus_resident;
        let first = scan.scanned.is_none() && scan.quiet >= SETTLE_FRAMES;
        let again = scan.scanned.is_some_and(|s| s != now)
            && scan.quiet >= REFRESH_QUIET
            && scan.refreshes < MAX_REFRESHES;
        if !ready || !(first || again) {
            return;
        }
        if again {
            scan.refreshes += 1;
        }
        scan.scanned = Some(now);
        scan.running = true;
        scan.ring = 0;
        scan.k = 0;
    }

    let probe = body_probe();
    let filter = WorldCollision::body_filter();
    let started = Instant::now();
    let mut steps = 0u32;
    loop {
        if scan.ring > grid.half {
            scan.running = false;
            info!(
                "survivors nav: scan done, {} of {} cells blocked",
                grid.solid.iter().filter(|&&s| s).count(),
                grid.solid.len()
            );
            break;
        }
        steps += 1;
        if steps.is_multiple_of(16) && started.elapsed() >= SCAN_BUDGET {
            break;
        }
        let c = IVec2::splat(grid.half) + ring_cell(scan.ring, scan.k);
        scan.k += 1;
        if scan.k >= ring_len(scan.ring) {
            scan.ring += 1;
            scan.k = 0;
        }
        if !grid.inside(c) {
            continue;
        }
        let at = grid.center_of(c);
        if Vec2::new(at.x, at.z).distance(grid.center) > grid.radius {
            continue;
        }
        // No terrain here (not streamed, or a hole): leave the cell as it was.
        let Some(ground) = world.terrain_height_under(at) else {
            continue;
        };
        let solid = solid_at(
            &spatial,
            &run_entities,
            &probe,
            &filter,
            Vec3::new(at.x, ground + BODY_MID, at.z),
        );
        grid.set(c, solid);
    }
}

/// Keeps the flow field on the hero.
fn track_hero(time: Res<Time>, hero: Query<&Transform, With<Hero>>, mut grid: ResMut<NavGrid>) {
    let Ok(h) = hero.single() else { return };
    grid.track(h.translation, time.delta_secs());
}

pub(super) fn plugin(app: &mut App) {
    app.init_resource::<NavGrid>()
        .add_systems(Update, scan_world.after(WorldStage::Stream))
        .add_systems(
            Update,
            track_hero
                .after(WorldStage::Input)
                .before(super::enemies::move_enemies)
                .run_if(super::run_on)
                // The same gates as the chase it feeds.
                .run_if(|l: Res<benilla_app::survivors_host::loading_screen::LoadingScreen>| !l.covering()),
        );
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A `w` by `h` grid with its corner at the world origin, so cell (x, z) is the yard square
    /// from (x, z) to (x + 1, z + 1).
    fn grid(w: i32, h: i32) -> NavGrid {
        NavGrid::new(0, Vec2::ZERO, w, h)
    }

    fn at(x: f32, z: f32) -> Vec3 {
        Vec3::new(x, 0.0, z)
    }

    /// A wall down column `x` from row `z0` up to (not including) row `z1`.
    fn wall(g: &mut NavGrid, x: i32, z0: i32, z1: i32) {
        for z in z0..z1 {
            g.set(IVec2::new(x, z), true);
        }
    }

    #[test]
    fn a_wall_blocks_the_line_and_the_step() {
        let mut g = grid(40, 40);
        wall(&mut g, 20, 0, 40);
        assert!(g.blocked(at(20.5, 10.5)));
        assert!(!g.blocked(at(19.5, 10.5)));
        assert!(!g.line_clear(at(10.5, 10.5), at(30.5, 10.5)));
        assert!(g.line_clear(at(10.5, 10.5), at(15.5, 20.5)));
        let from = at(19.9, 10.5);
        assert_eq!(g.step(from, at(20.2, 10.5)), from);
    }

    #[test]
    fn a_step_into_a_wall_slides_along_it() {
        let mut g = grid(40, 40);
        wall(&mut g, 20, 0, 40);
        let from = at(19.5, 10.5);
        let got = g.step(from, at(20.3, 11.3));
        assert!(!g.blocked(got));
        assert!((got.z - 11.3).abs() < 1e-4, "kept its move along the wall");
        assert!(got.x < 20.0, "and gave up the move into it, got {}", got.x);
    }

    #[test]
    fn a_long_step_cannot_jump_a_thin_wall() {
        let mut g = grid(40, 40);
        wall(&mut g, 20, 0, 40);
        let got = g.step(at(18.5, 10.5), at(23.5, 10.5));
        assert!(got.x < 20.0, "stopped at the wall, got {}", got.x);
    }

    #[test]
    fn a_walker_in_a_solid_cell_may_leave_it() {
        let mut g = grid(40, 40);
        wall(&mut g, 20, 0, 40);
        let to = at(19.5, 10.5);
        assert_eq!(g.step(at(20.5, 10.5), to), to);
        let d = g.chase(at(20.5, 10.5), at(30.5, 10.5));
        assert!(d.length() > 0.99);
        let out = g.nearest_free(g.cell(at(20.5, 10.5)), FREE_SEARCH, at(20.5, 10.5));
        assert!(out.is_some_and(|c| !g.is_solid(c)));
        assert!(!g.stuck(at(20.5, 10.5)), "free ground is a step away");
    }

    /// A 7 by 7 block of solid cells with its corner at (10, 10) and a free 3 by 3 pocket in its
    /// middle: a sealed room. The hero stands far off in the open.
    fn sealed_room() -> NavGrid {
        let mut g = grid(60, 60);
        for x in 10..17 {
            wall(&mut g, x, 10, 17);
        }
        for x in 12..15 {
            for z in 12..15 {
                g.set(IVec2::new(x, z), false);
            }
        }
        g.rebuild_field(g.cell(at(40.5, 13.5)));
        g
    }

    #[test]
    fn a_spawn_does_not_fit_in_a_wall_or_a_sealed_room() {
        let g = sealed_room();
        assert!(!g.fits(at(11.5, 13.5)), "inside the wall");
        assert!(!g.fits(at(13.5, 13.5)), "inside the room");
        assert!(g.fits(at(30.5, 13.5)), "in the open");
        assert!(g.fits(at(-5.0, 13.5)), "off the grid reads free");
    }

    #[test]
    fn a_walker_sealed_in_a_room_is_stuck() {
        let g = sealed_room();
        assert!(g.stuck(at(13.5, 13.5)));
        assert!(!g.stuck(at(30.5, 13.5)));
        // Without a field nothing says the room is sealed.
        let mut open = sealed_room();
        open.field = Field::default();
        assert!(!open.stuck(at(13.5, 13.5)));
    }

    #[test]
    fn a_walker_deep_in_solid_space_is_stuck() {
        let mut g = grid(60, 60);
        for x in 5..55 {
            wall(&mut g, x, 5, 55);
        }
        assert!(g.stuck(at(30.5, 30.5)));
        assert!(!g.stuck(at(7.5, 30.5)), "the edge is near");
        assert!(!g.stuck(at(2.5, 30.5)));
    }

    #[test]
    fn a_walker_in_a_wall_leaves_it_away_from_the_sealed_room() {
        let g = sealed_room();
        // The room is one cell away, the open ground two: it walks out, not in.
        let from = at(11.5, 13.5);
        assert!(!g.stuck(from));
        let d = g.chase(from, at(40.5, 13.5));
        assert!(d.x < -0.9, "heads for the open side, got {d:?}");
        let out = g.step(from, from + d * 0.5);
        assert!(out.x < from.x);
    }

    #[test]
    fn a_hero_shut_in_a_closet_does_not_strand_the_horde() {
        let mut g = grid(60, 60);
        // A 3 by 3 closet around the hero with a thick wall.
        for x in 40..47 {
            wall(&mut g, x, 10, 17);
        }
        for x in 42..45 {
            for z in 12..15 {
                g.set(IVec2::new(x, z), false);
            }
        }
        g.rebuild_field(g.cell(at(43.5, 13.5)));
        assert!(
            !g.stuck(at(20.5, 13.5)),
            "the field is too small to judge by"
        );
        assert!(g.fits(at(20.5, 13.5)));
    }

    #[test]
    fn a_diagonal_does_not_cut_a_corner() {
        let mut g = grid(10, 10);
        g.set(IVec2::new(5, 4), true);
        g.set(IVec2::new(4, 5), true);
        assert!(!g.passable(IVec2::new(4, 4), IVec2::new(5, 5)));
        assert!(g.passable(IVec2::new(4, 4), IVec2::new(3, 3)));
        let from = at(4.9, 4.9);
        let got = g.step(from, at(5.1, 5.1));
        assert_eq!(g.cell(got), IVec2::new(4, 4));
    }

    #[test]
    fn the_flow_goes_round_a_wall_to_the_gap() {
        let mut g = grid(60, 60);
        // A wall with its only gap at the top.
        wall(&mut g, 30, 0, 50);
        let hero = at(45.5, 10.5);
        let mut p = at(15.5, 10.5);
        g.rebuild_field(g.cell(hero));
        assert!(g.field.source.is_some());
        assert!(!g.line_clear(p, hero));
        let first = g.chase(p, hero);
        assert!(first.z > 0.1, "heads for the gap, not the wall: {first:?}");
        let mut steps = 0;
        while flat(hero - p).length() > 1.5 && steps < 1000 {
            let d = g.chase(p, hero);
            assert!(d.length() > 0.99);
            p = g.step(p, p + d * 0.2);
            assert!(!g.blocked(p), "walked into a wall at {p:?}");
            steps += 1;
        }
        assert!(steps < 1000, "never arrived, stuck at {p:?}");
    }

    #[test]
    fn a_sealed_hero_gives_no_flow() {
        let mut g = grid(60, 60);
        wall(&mut g, 30, 0, 60);
        let hero = at(45.5, 10.5);
        g.rebuild_field(g.cell(hero));
        assert_eq!(g.flow(at(15.5, 10.5)), None);
        // With no field the chase is the straight line.
        let d = g.chase(at(15.5, 10.5), hero);
        assert!(d.x > 0.99);
    }

    #[test]
    fn a_hero_in_a_wall_is_reached_through_the_nearest_free_cell() {
        let mut g = grid(60, 60);
        g.set(IVec2::new(45, 10), true);
        g.rebuild_field(IVec2::new(45, 10));
        let src = g.field.source.expect("a source");
        assert_ne!(src, IVec2::new(45, 10));
        assert!(!g.is_solid(src));
    }

    #[test]
    fn the_field_distances_are_octile() {
        let mut g = grid(40, 40);
        g.rebuild_field(IVec2::new(10, 10));
        assert_eq!(g.field.dist(IVec2::new(10, 10)), Some(0));
        assert_eq!(g.field.dist(IVec2::new(13, 10)), Some(30));
        assert_eq!(g.field.dist(IVec2::new(13, 13)), Some(42));
        // One diagonal and two straights.
        assert_eq!(g.field.dist(IVec2::new(13, 11)), Some(34));
    }

    #[test]
    fn the_field_is_a_window_around_the_hero() {
        let mut g = grid(300, 300);
        g.rebuild_field(IVec2::new(150, 150));
        assert!(g.field.dist(IVec2::new(150 + FIELD_RADIUS, 150)).is_some());
        assert_eq!(g.field.dist(IVec2::new(150 + FIELD_RADIUS + 1, 150)), None);
        // Outside it the chase is the straight line.
        let d = g.chase(at(5.5, 150.5), at(150.5, 150.5));
        assert!(d.x > 0.99);
    }

    #[test]
    fn the_field_follows_the_hero_a_few_times_a_second() {
        let mut g = grid(40, 40);
        g.track(at(10.5, 10.5), 0.0);
        // The first build waits out the interval, as every later one.
        assert!(g.field.source.is_none());
        g.track(at(10.5, 10.5), FLOW_EVERY);
        assert_eq!(g.field.source, Some(IVec2::new(10, 10)));
        g.track(at(20.5, 10.5), 0.01);
        assert_eq!(g.field.source, Some(IVec2::new(10, 10)), "too soon");
        g.track(at(20.5, 10.5), FLOW_EVERY);
        assert_eq!(g.field.source, Some(IVec2::new(20, 10)));
    }

    #[test]
    fn rings_cover_every_cell_once() {
        let mut seen = std::collections::HashSet::new();
        for r in 0..=5 {
            for k in 0..ring_len(r) {
                assert!(seen.insert(ring_cell(r, k)), "ring {r} repeats at {k}");
                let c = ring_cell(r, k);
                assert_eq!(c.x.abs().max(c.y.abs()), r);
            }
        }
        assert_eq!(seen.len(), 11 * 11);
    }
}
