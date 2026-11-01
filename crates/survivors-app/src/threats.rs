//! Threats beyond the chasing horde: ranged enemies with dodgeable missiles, boss abilities with
//! a ground telegraph first, and elite affixes (ticket #1118).
//!
//! - **Shooters.** A creature with a [`Ranged`] profile ([`super::data::EnemyDef::ranged`]) walks
//!   in to four fifths of its range, backs off from a hero closer than half of it
//!   ([`stance`], read by [`super::enemies::move_enemies`]) and, every cooldown, sends a
//!   [`Missile`] at where the hero stands right then. The missile flies straight on, so a hero
//!   who keeps moving outruns it, and hurts any hero it passes within [`MISSILE_HIT`] yards.
//! - **Boss abilities.** Every boss but the reaper and the ones with a script of their own
//!   ([`super::bosses`]) acts every seven to ten seconds: a [`Telegraph`] (a red circle that fills
//!   for a moment on the ground under a hero, then strikes whoever still stands in it) or a volley
//!   of missiles in a ring. The bosses take turns by place on the zone's schedule ([`plan_for`]);
//!   the finale does both. A telegraph is any [`Shape`]: the scripts lay cones, lines and rings
//!   with it too, and let a blow leave a burning pool behind.
//! - **Elite affixes.** An elite rolls one [`Affix`]: Swift (faster), Shielded (a shield that
//!   takes the first blows) or Volatile (a telegraphed blast where it dies). The plate names it.
//!
//! Everything random draws from the run's [`Rng`], so a seeded run plays the same.

use std::f32::consts::TAU;

use benilla_world::schedule::WorldStage;
use benilla_world::world_point::WorldPoint;
use bevy::asset::RenderAssetUsages;
use bevy::ecs::schedule::ScheduleConfigs;
use bevy::ecs::system::{ScheduleSystem, SystemParam};
use bevy::light::NotShadowCaster;
use bevy::mesh::{Indices, PrimitiveTopology};
use bevy::prelude::*;

use super::bosses::Shape;
use super::data::{Ranged, School};
use super::enemies::{self, Dying, Elite, Enemy};
use super::finale::{Finale, Reaper};
use super::hero::{self, Hero, Peer};
use super::prefs::Shake;
use super::units::{flat, Fx, Marks, COLOR_HURT};
use super::{HeroState, Rng, Run, RunEntity, Selection};

pub(super) fn plugin(app: &mut App) {
    app.init_resource::<Art>().add_systems(
        Update,
        systems()
            .after(WorldStage::Input)
            .run_if(super::run_on)
            .run_if(super::simulates)
            .run_if(
                |l: Res<benilla_app::survivors_host::loading_screen::LoadingScreen>| !l.covering(),
            ),
    );
}

/// The threat systems in the order they run.
fn systems() -> ScheduleConfigs<ScheduleSystem> {
    (
        arm,
        fire_casters,
        boss_moves,
        fly_missiles,
        tick_telegraphs,
        shield_break,
        volatile_burst,
    )
        .chain()
        .into_configs()
}

/// The run's threat systems, for the balance simulator ([`super::balance`]).
#[cfg(test)]
pub(super) fn sim_systems() -> ScheduleConfigs<ScheduleSystem> {
    systems()
}

/// The simulator's pilot steers out of what it can see coming, as a player does: it leaves a
/// telegraph's circle after a moment, and steps aside of a missile heading for it, though not of
/// every one (a player misses some). Without it the pilot would take every shot, and nothing
/// tuned against it would say how the threats play.
#[cfg(test)]
#[derive(SystemParam)]
pub(super) struct Dodger<'w, 's> {
    missiles: Query<'w, 's, (Entity, &'static Transform, &'static Missile), Without<Hero>>,
    circles: Query<'w, 's, &'static Telegraph>,
    pools: Query<'w, 's, &'static super::bosses::Pool>,
}

#[cfg(test)]
impl Dodger<'_, '_> {
    /// Seconds a telegraph is on the ground before the pilot reacts to it.
    const REACTION: f32 = 0.35;
    /// Of every five missiles, how many the pilot sees in time to dodge.
    const SEEN: u64 = 3;

    /// `dir`, the walk the pilot wanted, bent away from the threats around `at`.
    pub fn steer(&self, at: Vec3, dir: Vec3) -> Vec3 {
        if std::env::var_os("SIM_NO_DODGE").is_some() {
            return dir;
        }
        let mut push = Vec3::ZERO;
        // Out of every shape the way its own geometry says (sideways of a cone or a line),
        // with a yard to spare, and out of a burning pool at once.
        for t in &self.circles {
            if t.flash.is_none()
                && t.t >= Self::REACTION
                && t.shape.grown(1.0).contains(t.origin, t.yaw, at)
            {
                push += t.shape.escape(t.origin, t.yaw, at) * 3.0;
            }
        }
        for p in &self.pools {
            if p.shape.grown(1.0).contains(p.origin, p.yaw, at) {
                push += p.shape.escape(p.origin, p.yaw, at) * 3.0;
            }
        }
        for (ent, tf, m) in &self.missiles {
            if ent.to_bits() % 5 >= Self::SEEN {
                continue;
            }
            let to = flat(at - tf.translation);
            let ahead = to.dot(m.dir);
            if ahead <= 0.0 || ahead > 14.0 {
                continue;
            }
            let side = to - m.dir * ahead;
            if side.length() < MISSILE_HIT + 1.0 {
                let aside = Vec3::new(-m.dir.z, 0.0, m.dir.x);
                push += side.normalize_or(aside) * 3.0;
            }
        }
        if push == Vec3::ZERO {
            dir
        } else {
            (dir + push).normalize_or_zero()
        }
    }
}

/// The threat resources, for the balance simulator.
#[cfg(test)]
pub(super) fn sim_init(app: &mut App) {
    app.init_resource::<Art>();
}

// --- Tuning ---

/// A shooter walks in to this share of its range, and backs off from a hero closer than
/// [`BACK_OFF_BELOW`] of it.
const APPROACH_TO: f32 = 0.8;
const BACK_OFF_BELOW: f32 = 0.5;

/// Seconds into the run before the first shooter comes.
const SHOOTERS_FROM: f32 = 150.0;
/// The shooters alive at once when they first come, and how many more the run adds by its end.
const SHOOTERS_AT_START: f32 = 2.0;
const SHOOTERS_GROWTH: f32 = 3.0;

/// Yards within which a missile hurts a hero, measured on the ground plane.
const MISSILE_HIT: f32 = 1.2;
/// How high above the ground a missile flies, in yards.
const MISSILE_LIFT: f32 = 1.1;
/// A shooter's missile flies this much farther than its range before it is gone.
const MISSILE_SPARE: f32 = 8.0;

/// A boss acts every so many seconds; the first time a little sooner, as it arrives.
const BOSS_EVERY: (f32, f32) = (7.0, 10.0);
const BOSS_FIRST: (f32, f32) = (4.0, 7.0);
/// The circle a boss drops under a hero: its radius range in yards, the seconds it takes to fill,
/// and the blow as a share of the boss's own damage.
const CIRCLE_RADIUS: (f32, f32) = (5.0, 6.0);
const CIRCLE_SECS: f32 = 1.5;
const CIRCLE_DAMAGE: f32 = 2.0;
/// The ring of missiles: how many, their pace, how far they fly, and each one's blow as a share
/// of the boss's damage.
const VOLLEY_COUNT: usize = 8;
const VOLLEY_SPEED: f32 = 9.0;
const VOLLEY_RANGE: f32 = 38.0;
const VOLLEY_DAMAGE: f32 = 0.2;
/// The spell a boss casts for its ability and the impact of its missiles. Assumption, not
/// verified against the client's spell data: the creature Shadow Bolt.
const BOSS_SPELL: u32 = 9613;

/// Swift elites walk this much faster.
const SWIFT_SPEED: f32 = 1.4;
/// A Shielded elite's shield, as a share of its health (it takes the first blows).
const SHIELD_SHARE: f32 = 0.5;
/// The aura a shield wears: Power Word: Shield's bubble, beside the elite's golden one.
/// Assumption, not verified against the client's spell data.
const SHIELD_AURA: u32 = 17;
/// A Volatile elite's blast where it dies: radius in yards, seconds to fill, and the blow as a
/// share of the elite's own damage.
const BLAST_RADIUS: f32 = 4.0;
const BLAST_SECS: f32 = 1.0;
const BLAST_DAMAGE: f32 = 1.2;

/// How long a telegraph flashes when it goes off.
const FLASH_SECS: f32 = 0.25;
/// Rings of the telegraph's fill and columns around it.
const BANDS: usize = 8;
const COLS: usize = 32;
/// How far above the ground the fill and its edge lie, and the edge's width, in yards.
const FILL_LIFT: f32 = 0.25;
const EDGE_LIFT: f32 = 0.3;
const EDGE_WIDTH: f32 = 0.5;
/// The fill's opacity where it has filled and where it has not yet.
const FILL_ON: f32 = 0.6;
const FILL_OFF: f32 = 0.18;
/// A telegraph's colour: a crimson, linear. Kept clear of orange and yellow, which vanish on
/// the Barrens' grass and in the spell effects.
const DANGER: [f32; 3] = [1.0, 0.0, 0.08];
/// The edge: near white with a red cast, so the circle's reach reads on any ground.
const EDGE: [f32; 4] = [1.0, 0.75, 0.75, 1.0];

// --- Geometry and rules, pure ---

/// Shooters alive at once at this point of the run: none in the first minutes, while a new hero
/// is still finding its feet; then they keep their distance and so die only when chased, and a
/// field of them would shoot from every side.
pub(super) fn max_shooters(elapsed: f32) -> usize {
    if elapsed < SHOOTERS_FROM {
        return 0;
    }
    (SHOOTERS_AT_START + super::data::progress(elapsed).min(1.0) * SHOOTERS_GROWTH) as usize
}

/// The creature of `pool` (indices into `roster`) that fights in melee: `pick` itself if it does,
/// else the first of the pool that does, else `pick`.
pub(super) fn melee_of<'a>(
    roster: &'a [super::data::EnemyDef],
    pool: &[usize],
    pick: usize,
) -> &'a super::data::EnemyDef {
    let index = if roster[pick].ranged.is_none() {
        pick
    } else {
        pool.iter()
            .copied()
            .find(|&i| roster[i].ranged.is_none())
            .unwrap_or(pick)
    };
    &roster[index]
}

/// What a shooter does about its distance from the hero.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(super) enum Stance {
    /// Walk in: a melee creature always, a shooter until it is in position.
    Close,
    /// Stand and shoot.
    Hold,
    /// Too close: walk away.
    Back,
}

/// The stance of a creature with this `range` (none: melee) at `dist` yards from its quarry.
pub(super) fn stance(range: Option<f32>, dist: f32) -> Stance {
    let Some(range) = range else {
        return Stance::Close;
    };
    if dist > range * APPROACH_TO {
        Stance::Close
    } else if dist < range * BACK_OFF_BELOW {
        Stance::Back
    } else {
        Stance::Hold
    }
}

/// How near a missile flying from `a` to `b` comes to `p`, on the ground plane. The whole step is
/// tested, so a fast missile cannot tunnel through a hero between two frames.
pub(super) fn pass_distance(a: Vec3, b: Vec3, p: Vec3) -> f32 {
    let (a, b, p) = (flat(a), flat(b), flat(p));
    let ab = b - a;
    let len2 = ab.length_squared();
    let t = if len2 > 1e-6 {
        ((p - a).dot(ab) / len2).clamp(0.0, 1.0)
    } else {
        0.0
    };
    (a + ab * t - p).length()
}

/// An elite's affix.
#[derive(Component, Clone, Copy, PartialEq, Eq, Debug)]
pub(super) enum Affix {
    Swift,
    Shielded,
    Volatile,
}

impl Affix {
    pub const ALL: [Affix; 3] = [Affix::Swift, Affix::Shielded, Affix::Volatile];

    /// What the elite's plate calls it.
    pub fn label(self) -> &'static str {
        match self {
            Affix::Swift => "Swift",
            Affix::Shielded => "Shielded",
            Affix::Volatile => "Volatile",
        }
    }
}

/// The affix a roll of `0..1` picks: the three are equally likely.
pub(super) fn roll_affix(roll: f32) -> Affix {
    let n = Affix::ALL.len();
    Affix::ALL[((roll * n as f32) as usize).min(n - 1)]
}

/// What a boss does with its turn.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Plan {
    Circle,
    Volley,
    /// One after the other.
    Both,
}

/// What the clock's `index`-th boss (from 0) does: the odd ones fire volleys, the finale does
/// both.
fn plan_for(index: u32, finale: bool) -> Plan {
    if finale {
        Plan::Both
    } else if index % 2 == 1 {
        Plan::Volley
    } else {
        Plan::Circle
    }
}

/// Whether this turn is a volley, given the plan and whether the last turn of a [`Plan::Both`]
/// boss was one.
fn is_volley(plan: Plan, last_was_volley: bool) -> bool {
    match plan {
        Plan::Circle => false,
        Plan::Volley => true,
        Plan::Both => !last_was_volley,
    }
}

// --- Components ---

/// A shooter's reload.
#[derive(Component)]
struct Caster {
    cd: f32,
}

/// A boss's next turn.
#[derive(Component)]
struct BossMoves {
    cd: f32,
    plan: Plan,
    last_volley: bool,
}

/// A Shielded elite's shield: the health it had before it, so the shield left is `hp - base`.
/// The shield is extra health on top ([`apply_affix`]), which the blows take first.
#[derive(Component)]
struct Shield {
    base: f32,
}

/// A missile in flight.
#[derive(Component)]
pub(super) struct Missile {
    /// Unit vector on the ground plane.
    pub dir: Vec3,
    pub speed: f32,
    /// Yards it has left to fly.
    pub left: f32,
    pub damage: f32,
    /// The spell whose impact plays on the hero it hits (0: none).
    pub spell: u32,
}

/// A shape on the ground that fills and then strikes: a circle, a cone, a line or a ring.
#[derive(Component)]
struct Telegraph {
    shape: Shape,
    /// Where the shape lies: its middle, or for a cone and a line the foot it points from.
    origin: Vec3,
    /// The way a cone or a line points ([`super::units::yaw_toward`]).
    yaw: f32,
    secs: f32,
    t: f32,
    damage: f32,
    /// What the blow leaves burning on the ground: damage per second and seconds
    /// ([`super::bosses::Pool`]).
    pool: Option<(f32, f32)>,
    /// Set once it has gone off: seconds of flash left.
    flash: Option<f32>,
}

/// What a telegraph does this step.
#[derive(Clone, Copy, PartialEq, Debug)]
enum Beat {
    /// Filling, this far along (0 to 1).
    Charge(f32),
    /// Goes off now, once.
    Strike,
    /// Flashing, this share of the flash left.
    Flash(f32),
    /// Over.
    Done,
}

impl Telegraph {
    fn new(
        shape: Shape,
        origin: Vec3,
        yaw: f32,
        secs: f32,
        damage: f32,
        pool: Option<(f32, f32)>,
    ) -> Self {
        Self {
            shape,
            origin,
            yaw,
            secs,
            t: 0.0,
            damage,
            pool,
            flash: None,
        }
    }

    fn advance(&mut self, dt: f32) -> Beat {
        if let Some(left) = &mut self.flash {
            *left -= dt;
            return if *left <= 0.0 {
                Beat::Done
            } else {
                Beat::Flash(*left / FLASH_SECS)
            };
        }
        self.t += dt;
        if self.t >= self.secs {
            self.flash = Some(FLASH_SECS);
            Beat::Strike
        } else {
            Beat::Charge(self.t / self.secs)
        }
    }
}

// --- Heroes ---

/// A hero a threat may hit.
pub(super) struct Body {
    pub e: Entity,
    pub pos: Vec3,
    pub local: bool,
    /// Under Fade: only bosses still see it.
    pub vanished: bool,
}

/// The standing heroes and what a blow does to one: this machine's and every co-op puppet, as
/// the horde's melee finds them ([`enemies::move_enemies`]).
#[derive(SystemParam)]
pub(super) struct Heroes<'w, 's> {
    local: Query<
        'w,
        's,
        (Entity, &'static Transform),
        (With<Hero>, Without<Peer>, Without<Missile>, Without<Enemy>),
    >,
    peers: Query<
        'w,
        's,
        (Entity, &'static Transform, &'static mut HeroState),
        (With<Peer>, Without<Hero>, Without<Missile>, Without<Enemy>),
    >,
    run: ResMut<'w, Run>,
    shake: ResMut<'w, Shake>,
}

impl Heroes<'_, '_> {
    /// The run, for a script that spawns into it.
    pub fn run(&mut self) -> &mut Run {
        &mut self.run
    }

    /// The heroes on their feet, this machine's first.
    pub fn standing(&self) -> Vec<Body> {
        let mut out = Vec::new();
        if let Ok((e, tf)) = self.local.single() {
            if self.run.dead_t.is_none() {
                out.push(Body {
                    e,
                    pos: tf.translation,
                    local: true,
                    vanished: self.run.mobile.vanish_t > 0.0,
                });
            }
        }
        out.extend(
            self.peers
                .iter()
                .filter(|(_, _, s)| s.dead_t.is_none())
                .map(|(e, tf, s)| Body {
                    e,
                    pos: tf.translation,
                    local: false,
                    vanished: s.mobile.vanish_t > 0.0,
                }),
        );
        out
    }

    /// A blow of `raw` on `body`, through armor and shield like a bite: the number over its head
    /// and, for this machine's hero, the camera's shake. Returns what got through.
    pub fn strike(&mut self, body: &Body, raw: f32, fx: &mut Fx) -> f32 {
        let victim = if body.local {
            Some(&mut self.run.me)
        } else {
            self.peers
                .get_mut(body.e)
                .ok()
                .map(|(_, _, s)| s.into_inner())
        };
        let Some(victim) = victim else { return 0.0 };
        let taken = hero::hurt_hero(victim, raw);
        if taken > 0.0 {
            fx.number(body.e, taken, false, COLOR_HURT);
            if body.local {
                self.shake
                    .add(enemies::hurt_trauma(taken, victim.stats().max_hp));
            }
        }
        taken
    }
}

/// The hero nearest `from` on the ground plane; one under Fade only if `sees_vanished`.
pub(super) fn nearest(bodies: &[Body], from: Vec3, sees_vanished: bool) -> Option<&Body> {
    bodies
        .iter()
        .filter(|b| sees_vanished || !b.vanished)
        .min_by(|a, b| {
            flat(a.pos - from)
                .length()
                .total_cmp(&flat(b.pos - from).length())
        })
}

// --- Art ---

/// How a missile glows.
#[derive(Clone, Copy)]
enum Glow {
    Shadow,
    Frost,
    Fire,
    Nature,
    Arrow,
    Boss,
}

impl Glow {
    const COUNT: usize = 6;

    fn index(self) -> usize {
        self as usize
    }

    /// Linear colour.
    fn tint(self) -> [f32; 3] {
        match self {
            Glow::Shadow => [0.65, 0.25, 1.0],
            Glow::Frost => [0.35, 0.7, 1.0],
            Glow::Fire => [1.0, 0.45, 0.08],
            Glow::Nature => [0.5, 1.0, 0.3],
            Glow::Arrow => [0.9, 0.75, 0.4],
            Glow::Boss => [1.0, 0.18, 0.08],
        }
    }
}

impl From<School> for Glow {
    fn from(school: School) -> Self {
        match school {
            School::Shadow => Glow::Shadow,
            School::Frost => Glow::Frost,
            School::Fire => Glow::Fire,
            School::Nature => Glow::Nature,
            School::Arrow => Glow::Arrow,
        }
    }
}

/// The meshes and materials the threats share: a core and a halo mesh for the missiles, a pair
/// of materials per glow, and one blended material the telegraphs wear (their vertex colours
/// carry the colour and the fill).
#[derive(Resource, Default)]
pub(super) struct Art {
    missile: Option<(Handle<Mesh>, Handle<Mesh>)>,
    glows: [Option<(Handle<StandardMaterial>, Handle<StandardMaterial>)>; Glow::COUNT],
    blend: Option<Handle<StandardMaterial>>,
}

/// The core's radius and the halo's, in yards.
const CORE_RADIUS: f32 = 0.4;
const HALO_RADIUS: f32 = 0.9;

fn unlit(color: Color, blend: bool) -> StandardMaterial {
    StandardMaterial {
        base_color: color,
        unlit: true,
        alpha_mode: if blend {
            AlphaMode::Blend
        } else {
            AlphaMode::Opaque
        },
        cull_mode: None,
        double_sided: true,
        fog_enabled: false,
        ..default()
    }
}

impl Art {
    /// The core and halo meshes and materials of a missile that glows `glow`.
    fn missile(
        &mut self,
        glow: Glow,
        meshes: &mut Assets<Mesh>,
        materials: &mut Assets<StandardMaterial>,
    ) -> (
        (Handle<Mesh>, Handle<Mesh>),
        (Handle<StandardMaterial>, Handle<StandardMaterial>),
    ) {
        let shapes = self
            .missile
            .get_or_insert_with(|| {
                (
                    meshes.add(Sphere::new(CORE_RADIUS)),
                    meshes.add(Sphere::new(HALO_RADIUS)),
                )
            })
            .clone();
        let looks = self.glows[glow.index()]
            .get_or_insert_with(|| {
                let [r, g, b] = glow.tint();
                // The core runs toward white, the halo is the colour itself and faint.
                let core = Color::srgb(
                    r + (1.0 - r) * 0.55,
                    g + (1.0 - g) * 0.55,
                    b + (1.0 - b) * 0.55,
                );
                (
                    materials.add(unlit(core, false)),
                    materials.add(unlit(Color::srgba(r, g, b, 0.3), true)),
                )
            })
            .clone();
        (shapes, looks)
    }

    fn blend(&mut self, materials: &mut Assets<StandardMaterial>) -> Handle<StandardMaterial> {
        self.blend
            .get_or_insert_with(|| materials.add(unlit(Color::WHITE, true)))
            .clone()
    }
}

/// Send a missile from `from` along `dir` (a unit vector on the ground plane).
#[allow(clippy::too_many_arguments)]
fn launch(
    commands: &mut Commands,
    art: &mut Art,
    meshes: &mut Assets<Mesh>,
    materials: &mut Assets<StandardMaterial>,
    glow: Glow,
    from: Vec3,
    dir: Vec3,
    missile: Missile,
) {
    let ((core, halo), (core_mat, halo_mat)) = art.missile(glow, meshes, materials);
    commands
        .spawn((
            missile_at(from, dir, &missile),
            missile,
            Mesh3d(core),
            MeshMaterial3d(core_mat),
            NotShadowCaster,
            Visibility::default(),
            RunEntity,
        ))
        .with_child((
            Mesh3d(halo),
            MeshMaterial3d(halo_mat),
            NotShadowCaster,
            Transform::default(),
        ));
}

fn missile_at(from: Vec3, dir: Vec3, _missile: &Missile) -> Transform {
    Transform::from_translation(from + dir * 0.6 + Vec3::Y * MISSILE_LIFT)
}

/// One sample of the ground under `p`, or `fallback` while the terrain has not streamed in.
pub(super) fn ground_y(world: &WorldPoint, p: Vec3, fallback: f32) -> f32 {
    world
        .terrain_height_under(Vec3::new(p.x, fallback, p.z))
        .unwrap_or(fallback)
}

/// Where lattice vertex (`k`-th ring, `i`-th column) of `shape` lies, relative to its origin, in the
/// frame where the shape points along -Z. Every shape is the same lattice of `BANDS + 1` rings by
/// `COLS + 1` columns, so one fill, one set of colours and one way to grow it serve them all:
///
/// - a circle: rings of growing radius, columns round the circle;
/// - a ring: the same between its inner and outer radius;
/// - a cone: rings of growing radius, columns across the angle;
/// - a line: rings of growing distance, columns across its width.
fn lattice(shape: Shape, k: usize, i: usize) -> Vec3 {
    let along = k as f32 / BANDS as f32;
    let across = i as f32 / COLS as f32;
    match shape {
        Shape::Circle { radius } => {
            let (s, c) = (across * TAU).sin_cos();
            Vec3::new(c, 0.0, s) * (radius * along)
        }
        Shape::Donut { inner, outer } => {
            let (s, c) = (across * TAU).sin_cos();
            Vec3::new(c, 0.0, s) * (inner + (outer - inner) * along)
        }
        Shape::Cone { radius, half_angle } => {
            let (s, c) = ((across * 2.0 - 1.0) * half_angle).sin_cos();
            Vec3::new(s, 0.0, -c) * (radius * along)
        }
        Shape::Line { length, width } => Vec3::new((across - 0.5) * width, 0.0, -length * along),
    }
}

/// The telegraph's fill: the shape's lattice, each vertex at its ground height plus `FILL_LIFT`.
/// `heights` holds one per vertex, ring by ring. Positions are relative to the origin's x and z
/// (and turned by the shape's yaw, which the entity's transform carries), in the world's y.
fn fill_mesh(shape: Shape, heights: &[f32]) -> Mesh {
    let per = COLS + 1;
    let mut pos: Vec<[f32; 3]> = Vec::with_capacity((BANDS + 1) * per);
    for k in 0..=BANDS {
        for i in 0..=COLS {
            let p = lattice(shape, k, i);
            pos.push([p.x, heights[k * per + i] + FILL_LIFT, p.z]);
        }
    }
    let n = pos.len();
    let mut idx = Vec::with_capacity(BANDS * COLS * 6);
    for k in 0..BANDS {
        for i in 0..COLS {
            let a = (k * per + i) as u32;
            let (b, c, d) = (a + 1, a + per as u32, a + per as u32 + 1);
            idx.extend_from_slice(&[a, c, d, a, d, b]);
        }
    }
    let mut mesh = Mesh::new(
        PrimitiveTopology::TriangleList,
        RenderAssetUsages::default(),
    );
    mesh.insert_attribute(Mesh::ATTRIBUTE_POSITION, pos);
    mesh.insert_attribute(Mesh::ATTRIBUTE_NORMAL, vec![[0.0f32, 1.0, 0.0]; n]);
    mesh.insert_attribute(Mesh::ATTRIBUTE_UV_0, vec![[0.0f32; 2]; n]);
    mesh.insert_attribute(
        Mesh::ATTRIBUTE_COLOR,
        fill_colors(0.0, None).collect::<Vec<_>>(),
    );
    mesh.insert_indices(Indices::U32(idx));
    mesh
}

/// The lattice runs (as `(ring, column)` vertices) that outline `shape`: the outer rim for all of
/// them, the inner rim of a ring, the sides of a cone and a line, and a line's foot.
fn outline(shape: Shape) -> Vec<Vec<(usize, usize)>> {
    let ring = |k: usize| (0..=COLS).map(|i| (k, i)).collect::<Vec<_>>();
    let side = |i: usize| (0..=BANDS).map(|k| (k, i)).collect::<Vec<_>>();
    match shape {
        Shape::Circle { .. } => vec![ring(BANDS)],
        Shape::Donut { .. } => vec![ring(0), ring(BANDS)],
        Shape::Cone { .. } => vec![ring(BANDS), side(0), side(COLS)],
        Shape::Line { .. } => vec![ring(BANDS), ring(0), side(0), side(COLS)],
    }
}

/// The telegraph's edge: a ribbon `EDGE_WIDTH` wide along the shape's outline, on the shape's side
/// of it, at the ground heights of the fill (`heights`, ring by ring like [`fill_mesh`]'s).
fn edge_mesh(shape: Shape, heights: &[f32]) -> Mesh {
    let per = COLS + 1;
    let at = |(k, i): (usize, usize)| lattice(shape, k, i) + Vec3::Y * (heights[k * per + i] + EDGE_LIFT);
    let mut pos: Vec<[f32; 3]> = Vec::new();
    let mut idx: Vec<u32> = Vec::new();
    for run in outline(shape) {
        for pair in run.windows(2) {
            let (a, b) = (at(pair[0]), at(pair[1]));
            let seg = flat(b - a);
            // A ring's inner rim shrinks to a point at a circle's middle.
            if seg.length() < 1e-3 {
                continue;
            }
            let n = Vec3::new(-seg.z, 0.0, seg.x).normalize();
            // The ribbon lies on the side where the shape is.
            let mid = flat(a + b) * 0.5;
            let inward = if shape.contains(Vec3::ZERO, 0.0, mid + n * 0.5) {
                n
            } else {
                -n
            };
            let step = inward * EDGE_WIDTH;
            let base = pos.len() as u32;
            for p in [a, b, a + step, b + step] {
                pos.push(p.to_array());
            }
            idx.extend_from_slice(&[base, base + 1, base + 3, base, base + 3, base + 2]);
        }
    }
    let n = pos.len();
    let mut mesh = Mesh::new(
        PrimitiveTopology::TriangleList,
        RenderAssetUsages::default(),
    );
    mesh.insert_attribute(Mesh::ATTRIBUTE_POSITION, pos);
    mesh.insert_attribute(Mesh::ATTRIBUTE_NORMAL, vec![[0.0f32, 1.0, 0.0]; n]);
    mesh.insert_attribute(Mesh::ATTRIBUTE_UV_0, vec![[0.0f32; 2]; n]);
    mesh.insert_attribute(Mesh::ATTRIBUTE_COLOR, vec![EDGE; n]);
    mesh.insert_indices(Indices::U32(idx));
    mesh
}

/// The fill's vertex colours, ring by ring: strong inside `progress` (0 to 1) of the radius,
/// faint outside it, so the circle fills from its centre out. With `flash`, the share of the
/// flash left, the whole disc goes bright and fades.
fn fill_colors(progress: f32, flash: Option<f32>) -> impl Iterator<Item = [f32; 4]> {
    (0..=BANDS).flat_map(move |k| {
        let color = match flash {
            Some(f) => [1.0, 0.7, 0.35, 0.9 * f.clamp(0.0, 1.0)],
            None => {
                let on = k as f32 / BANDS as f32 <= progress;
                [
                    DANGER[0],
                    DANGER[1],
                    DANGER[2],
                    if on { FILL_ON } else { FILL_OFF },
                ]
            }
        };
        std::iter::repeat_n(color, COLS + 1)
    })
}

/// Put a telegraph of `shape` on the ground at `origin` (for a cone or a line, pointing along
/// `yaw`); it strikes after `secs`. A blow of `damage` hurts every hero inside it, and with `pool`
/// (damage per second, seconds) leaves a burning pool there ([`super::bosses::Pool`]).
#[allow(clippy::too_many_arguments)]
pub(super) fn spawn_telegraph(
    commands: &mut Commands,
    art: &mut Art,
    meshes: &mut Assets<Mesh>,
    materials: &mut Assets<StandardMaterial>,
    world: &WorldPoint,
    shape: Shape,
    origin: Vec3,
    yaw: f32,
    secs: f32,
    damage: f32,
    pool: Option<(f32, f32)>,
) {
    let per = COLS + 1;
    let turn = Quat::from_rotation_y(yaw);
    let mut heights = vec![origin.y; (BANDS + 1) * per];
    for k in 0..=BANDS {
        for i in 0..=COLS {
            let local = lattice(shape, k, i);
            // The first ring of a circle or a cone is one point: sampled once.
            heights[k * per + i] = if k == 0 && i > 0 && local == lattice(shape, 0, 0) {
                heights[0]
            } else {
                ground_y(world, origin + turn * local, origin.y)
            };
        }
    }
    let material = art.blend(materials);
    commands
        .spawn((
            Telegraph::new(shape, origin, yaw, secs, damage, pool),
            Mesh3d(meshes.add(fill_mesh(shape, &heights))),
            MeshMaterial3d(material.clone()),
            // The hero is looking at it, and shadows on a ground marker only muddy it.
            NotShadowCaster,
            Transform::from_xyz(origin.x, 0.0, origin.z).with_rotation(turn),
            Visibility::default(),
            RunEntity,
        ))
        .with_child((
            Mesh3d(meshes.add(edge_mesh(shape, &heights))),
            MeshMaterial3d(material),
            NotShadowCaster,
            Transform::default(),
        ));
}

/// What a boss script paints with: the shared meshes and materials, and the world to find the
/// ground in ([`super::bosses`]).
#[derive(SystemParam)]
pub(super) struct Canvas<'w, 's> {
    art: ResMut<'w, Art>,
    meshes: ResMut<'w, Assets<Mesh>>,
    materials: ResMut<'w, Assets<StandardMaterial>>,
    world: WorldPoint<'w, 's>,
}

impl Canvas<'_, '_> {
    /// A telegraph ([`spawn_telegraph`]).
    #[allow(clippy::too_many_arguments)]
    pub fn telegraph(
        &mut self,
        commands: &mut Commands,
        shape: Shape,
        origin: Vec3,
        yaw: f32,
        secs: f32,
        damage: f32,
        pool: Option<(f32, f32)>,
    ) {
        spawn_telegraph(
            commands,
            &mut self.art,
            &mut self.meshes,
            &mut self.materials,
            &self.world,
            shape,
            origin,
            yaw,
            secs,
            damage,
            pool,
        );
    }

    /// One missile of `school` from `from` along each of `dirs` (unit vectors on the ground plane),
    /// flying `speed` yards a second for `range` yards, each a blow of `damage`; `spell` plays its
    /// impact on the hero it hits (0: none).
    #[allow(clippy::too_many_arguments)]
    pub fn missiles(
        &mut self,
        commands: &mut Commands,
        school: School,
        from: Vec3,
        dirs: &[Vec3],
        speed: f32,
        range: f32,
        damage: f32,
        spell: u32,
    ) {
        for &dir in dirs {
            launch(
                commands,
                &mut self.art,
                &mut self.meshes,
                &mut self.materials,
                school.into(),
                from,
                dir,
                Missile {
                    dir,
                    speed,
                    left: range,
                    damage,
                    spell,
                },
            );
        }
    }

    /// A spot `r` yards from `center` at angle `a` where a creature may stand ([`enemies::spawn_spot`]).
    pub fn spot(
        &self,
        nav: &super::nav::NavGrid,
        probe: &super::nav::SpawnProbe,
        center: Vec3,
        a: f32,
        r: f32,
    ) -> Option<Vec3> {
        enemies::spawn_spot(nav, probe, &self.world, center, a, r)
    }
}

// --- Systems ---

/// New enemies get what makes them a threat: a shooter its reload, a boss its turns, an elite its
/// affix.
fn arm(
    mut commands: Commands,
    mut rng: ResMut<Rng>,
    mut fresh: Query<
        (
            Entity,
            &mut Enemy,
            &mut Marks,
            Has<Elite>,
            Has<Finale>,
            Has<Reaper>,
        ),
        Added<Enemy>,
    >,
    selection: Res<Selection>,
) {
    for (ent, mut e, mut marks, elite, finale, reaper) in &mut fresh {
        if let Some(r) = e.def.ranged {
            commands.entity(ent).insert(Caster {
                cd: rng.range(0.5, r.cooldown),
            });
        }
        // A boss with a script of its own ([`super::bosses`]) fights by that instead.
        if e.boss && !reaper && !super::bosses::scripted(e.def.name) {
            // The clock's bosses are numbered by their place on the zone's schedule.
            let index = match enemies::def_ref(selection.map, e.def) {
                Some((2, i)) => u32::from(i),
                _ => 0,
            };
            commands.entity(ent).insert(BossMoves {
                cd: rng.range(BOSS_FIRST.0, BOSS_FIRST.1),
                plan: plan_for(index, finale),
                last_volley: false,
            });
        }
        if elite && !e.boss {
            let affix = roll_affix(rng.f32());
            apply_affix(&mut commands, ent, &mut e, &mut marks, affix);
        }
    }
}

/// Make the elite `ent` wear `affix`.
fn apply_affix(
    commands: &mut Commands,
    ent: Entity,
    e: &mut Enemy,
    marks: &mut Marks,
    affix: Affix,
) {
    commands.entity(ent).insert(affix);
    match affix {
        Affix::Swift => e.speed *= SWIFT_SPEED,
        Affix::Shielded => {
            commands.entity(ent).insert(Shield { base: e.max_hp });
            let shield = e.max_hp * SHIELD_SHARE;
            e.max_hp += shield;
            e.hp += shield;
            marks.set(&[enemies::ELITE_AURA, SHIELD_AURA]);
        }
        Affix::Volatile => {}
    }
}

/// Shooters reload and fire at the nearest hero they see, once it is within range.
#[allow(clippy::too_many_arguments)]
fn fire_casters(
    mut commands: Commands,
    time: Res<Time>,
    mut rng: ResMut<Rng>,
    mut art: ResMut<Art>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    heroes: Heroes,
    mut fx: Fx,
    mut casters: Query<(Entity, &Transform, &Enemy, &mut Caster)>,
) {
    let dt = time.delta_secs();
    if dt <= 0.0 {
        return;
    }
    let bodies = heroes.standing();
    for (ent, tf, e, mut caster) in &mut casters {
        caster.cd = (caster.cd - dt).max(0.0);
        let Some(r) = e.def.ranged else { continue };
        if e.dead || caster.cd > 0.0 || e.stun_t > 0.0 || e.fear_t > 0.0 {
            continue;
        }
        let Some(target) = nearest(&bodies, tf.translation, false) else {
            continue;
        };
        let to = flat(target.pos - tf.translation);
        if to.length() > r.range {
            continue;
        }
        caster.cd = r.cooldown * rng.range(0.9, 1.15);
        shoot(
            &mut commands,
            &mut art,
            &mut meshes,
            &mut materials,
            &mut fx,
            ent,
            tf.translation,
            to.normalize_or_zero(),
            &r,
            e.damage * e.weaken * r.damage,
        );
    }
}

/// `ent` fires one of its missiles from `from` along `dir`.
#[allow(clippy::too_many_arguments)]
fn shoot(
    commands: &mut Commands,
    art: &mut Art,
    meshes: &mut Assets<Mesh>,
    materials: &mut Assets<StandardMaterial>,
    fx: &mut Fx,
    ent: Entity,
    from: Vec3,
    dir: Vec3,
    r: &Ranged,
    damage: f32,
) {
    if r.spell != 0 {
        fx.cast(ent, r.spell, &[], None);
    }
    launch(
        commands,
        art,
        meshes,
        materials,
        r.school.into(),
        from,
        dir,
        Missile {
            dir,
            speed: r.speed,
            left: r.range + MISSILE_SPARE,
            damage,
            spell: r.spell,
        },
    );
}

/// Bosses take their turns: a circle dropped under the nearest hero, or a ring of missiles.
#[allow(clippy::too_many_arguments)]
fn boss_moves(
    mut commands: Commands,
    time: Res<Time>,
    mut rng: ResMut<Rng>,
    mut art: ResMut<Art>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    world: WorldPoint,
    heroes: Heroes,
    mut fx: Fx,
    mut bosses: Query<(Entity, &Transform, &Enemy, &mut BossMoves)>,
) {
    let dt = time.delta_secs();
    if dt <= 0.0 {
        return;
    }
    let bodies = heroes.standing();
    for (ent, tf, e, mut moves) in &mut bosses {
        moves.cd = (moves.cd - dt).max(0.0);
        if e.dead || moves.cd > 0.0 || e.stun_t > 0.0 {
            continue;
        }
        // A boss sees every hero, Fade or not.
        let Some(target) = nearest(&bodies, tf.translation, true) else {
            continue;
        };
        moves.cd = rng.range(BOSS_EVERY.0, BOSS_EVERY.1);
        let volley = is_volley(moves.plan, moves.last_volley);
        moves.last_volley = volley;
        fx.cast(ent, BOSS_SPELL, &[], None);
        let damage = e.damage * e.weaken;
        if volley {
            let phase = rng.range(0.0, TAU);
            for i in 0..VOLLEY_COUNT {
                let a = phase + i as f32 / VOLLEY_COUNT as f32 * TAU;
                let dir = Vec3::new(a.cos(), 0.0, a.sin());
                launch(
                    &mut commands,
                    &mut art,
                    &mut meshes,
                    &mut materials,
                    Glow::Boss,
                    tf.translation,
                    dir,
                    Missile {
                        dir,
                        speed: VOLLEY_SPEED,
                        left: VOLLEY_RANGE,
                        damage: damage * VOLLEY_DAMAGE,
                        spell: BOSS_SPELL,
                    },
                );
            }
        } else {
            spawn_telegraph(
                &mut commands,
                &mut art,
                &mut meshes,
                &mut materials,
                &world,
                Shape::Circle {
                    radius: rng.range(CIRCLE_RADIUS.0, CIRCLE_RADIUS.1),
                },
                target.pos,
                0.0,
                CIRCLE_SECS,
                damage * CIRCLE_DAMAGE,
                None,
            );
        }
    }
}

/// Missiles fly on, hurt the first hero they pass and are gone at the end of their range.
fn fly_missiles(
    mut commands: Commands,
    time: Res<Time>,
    world: WorldPoint,
    mut heroes: Heroes,
    mut fx: Fx,
    mut missiles: Query<(Entity, &mut Transform, &mut Missile)>,
) {
    let dt = time.delta_secs();
    if dt <= 0.0 {
        return;
    }
    let bodies = heroes.standing();
    for (ent, mut tf, mut m) in &mut missiles {
        let step = m.speed * dt;
        let from = tf.translation;
        let mut to = from + m.dir * step;
        // It rides the ground's rise and fall at the same height.
        to.y = ground_y(&world, to, from.y - MISSILE_LIFT) + MISSILE_LIFT;
        if let Some(body) = bodies
            .iter()
            .find(|b| pass_distance(from, to, b.pos) <= MISSILE_HIT)
        {
            heroes.strike(body, m.damage, &mut fx);
            if m.spell != 0 {
                fx.impact(body.e, m.spell);
            }
            commands.entity(ent).despawn();
            continue;
        }
        m.left -= step;
        if m.left <= 0.0 {
            commands.entity(ent).despawn();
        } else {
            tf.translation = to;
        }
    }
}

/// Telegraphs fill, strike every hero still inside, flash and go. A blow with a pool leaves it
/// burning where it landed.
fn tick_telegraphs(
    mut commands: Commands,
    time: Res<Time>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut heroes: Heroes,
    mut fx: Fx,
    mut circles: Query<(Entity, &mut Telegraph, &Mesh3d)>,
) {
    let dt = time.delta_secs();
    if dt <= 0.0 {
        return;
    }
    let bodies = heroes.standing();
    for (ent, mut t, mesh) in &mut circles {
        let beat = t.advance(dt);
        let colors: Vec<[f32; 4]> = match beat {
            Beat::Done => {
                commands.entity(ent).despawn();
                continue;
            }
            Beat::Charge(p) => fill_colors(p, None).collect(),
            Beat::Strike => {
                // A warning that only marks the way (a charge's line) has no blow of its own.
                if t.damage > 0.0 {
                    for body in &bodies {
                        if t.shape.contains(t.origin, t.yaw, body.pos) {
                            heroes.strike(body, t.damage, &mut fx);
                        }
                    }
                }
                if let Some((dps, secs)) = t.pool {
                    let serial = heroes.run.guid();
                    super::bosses::Pool::lay(&mut commands, serial, t.shape, t.origin, t.yaw, dps, secs);
                }
                fill_colors(1.0, Some(1.0)).collect()
            }
            Beat::Flash(left) => fill_colors(1.0, Some(left)).collect(),
        };
        if let Some(mesh) = meshes.get_mut(&mesh.0) {
            mesh.insert_attribute(Mesh::ATTRIBUTE_COLOR, colors);
        }
    }
}

/// A shield that has taken its share of blows is gone, and with it its bubble.
fn shield_break(
    mut commands: Commands,
    mut shielded: Query<(Entity, &Enemy, &Shield, &mut Marks)>,
) {
    for (ent, e, shield, mut marks) in &mut shielded {
        if e.hp <= shield.base {
            commands.entity(ent).remove::<Shield>();
            marks.set(&[enemies::ELITE_AURA]);
        }
    }
}

/// A Volatile elite that has just fallen leaves a blast where it died.
#[allow(clippy::too_many_arguments)]
fn volatile_burst(
    mut commands: Commands,
    mut art: ResMut<Art>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    world: WorldPoint,
    fallen: Query<(&Transform, &Enemy, &Affix), Added<Dying>>,
) {
    for (tf, e, affix) in &fallen {
        if *affix != Affix::Volatile {
            continue;
        }
        spawn_telegraph(
            &mut commands,
            &mut art,
            &mut meshes,
            &mut materials,
            &world,
            Shape::Circle {
                radius: BLAST_RADIUS,
            },
            tf.translation,
            0.0,
            BLAST_SECS,
            e.damage * BLAST_DAMAGE,
            None,
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn shooters_wait_for_the_first_minutes_and_then_grow_to_a_handful() {
        assert_eq!(max_shooters(0.0), 0);
        assert_eq!(max_shooters(SHOOTERS_FROM - 1.0), 0);
        assert_eq!(max_shooters(SHOOTERS_FROM), 2);
        assert!((4..=6).contains(&max_shooters(super::super::data::VICTORY_SECS)));
        assert!(
            max_shooters(1.0e6) <= 6,
            "the run's end is the most it grows"
        );
    }

    #[test]
    fn a_pool_without_room_for_a_shooter_sends_its_melee_creature() {
        use super::super::data::ENEMIES;
        // The Barrens' pool of 3..5: a Razormane Hunter (shooter) among melee.
        let pool = [3, 4, 5];
        assert!(ENEMIES[3].ranged.is_some() && ENEMIES[4].ranged.is_none());
        assert_eq!(melee_of(ENEMIES, &pool, 3).name, ENEMIES[4].name);
        assert_eq!(melee_of(ENEMIES, &pool, 5).name, ENEMIES[5].name);
        // A pool of shooters only has no melee creature to send: the pick stays.
        assert_eq!(melee_of(ENEMIES, &[3, 3], 3).name, ENEMIES[3].name);
    }

    #[test]
    fn a_melee_creature_always_closes() {
        for d in [0.5, 5.0, 40.0] {
            assert_eq!(stance(None, d), Stance::Close);
        }
    }

    #[test]
    fn a_shooter_closes_holds_and_backs_off() {
        let range = Some(20.0);
        assert_eq!(stance(range, 30.0), Stance::Close);
        assert_eq!(stance(range, 16.1), Stance::Close);
        assert_eq!(stance(range, 16.0), Stance::Hold);
        assert_eq!(stance(range, 10.0), Stance::Hold);
        assert_eq!(stance(range, 9.9), Stance::Back);
        assert_eq!(stance(range, 1.0), Stance::Back);
    }

    #[test]
    fn a_shooter_settles_between_half_and_four_fifths_of_its_range() {
        // A shooter walks at 4 yd/s toward a hero standing still, then the hero rushes it.
        let range = 20.0;
        let mut dist = 40.0f32;
        for _ in 0..400 {
            match stance(Some(range), dist) {
                Stance::Close => dist -= 4.0 * 0.05,
                Stance::Back => dist += 4.0 * 0.05,
                Stance::Hold => {}
            }
        }
        assert!((0.5 * range..=0.8 * range).contains(&dist), "{dist}");
        // The hero walks up to 6 yards; the shooter backs off to half its range and holds.
        dist = 6.0;
        for _ in 0..400 {
            match stance(Some(range), dist) {
                Stance::Close => dist -= 4.0 * 0.05,
                Stance::Back => dist += 4.0 * 0.05,
                Stance::Hold => {}
            }
        }
        assert!((0.5 * range..=0.8 * range).contains(&dist), "{dist}");
    }

    #[test]
    fn a_missile_hits_what_it_passes_within_reach_and_nothing_else() {
        let a = Vec3::new(0.0, 1.0, 0.0);
        let b = Vec3::new(10.0, 1.0, 0.0);
        let near = Vec3::new(5.0, 0.0, 1.1);
        let far = Vec3::new(5.0, 0.0, 1.3);
        assert!(pass_distance(a, b, near) <= MISSILE_HIT);
        assert!(pass_distance(a, b, far) > MISSILE_HIT);
        // Behind the start and past the end it is measured to the nearest end.
        assert!(pass_distance(a, b, Vec3::new(-2.0, 0.0, 0.0)) > MISSILE_HIT);
        assert!(pass_distance(a, b, Vec3::new(12.0, 0.0, 0.0)) > MISSILE_HIT);
        // Height does not count.
        assert!(pass_distance(a, b, Vec3::new(5.0, 30.0, 0.0)) < 1e-4);
    }

    #[test]
    fn a_fast_missile_cannot_tunnel_through_a_hero() {
        // A step of 30 yards, far past the hero's 1.2 yards: the swept test still catches it.
        let hero = Vec3::new(15.0, 0.0, 0.5);
        assert!(pass_distance(Vec3::ZERO, Vec3::new(30.0, 0.0, 0.0), hero) <= MISSILE_HIT);
        // A degenerate step is a point test.
        assert!((pass_distance(Vec3::ZERO, Vec3::ZERO, hero) - hero.length()).abs() < 1e-4);
    }

    #[test]
    fn a_hero_who_keeps_moving_outruns_a_missile_aimed_at_where_it_stood() {
        // Fired from 16 yards at a hero walking sideways at 7 yd/s; the missile flies 11 yd/s
        // straight at where the hero stood when it was fired.
        let from = Vec3::ZERO;
        let aim = Vec3::new(16.0, 0.0, 0.0);
        let dir = flat(aim - from).normalize();
        let (mut m, mut h) = (from, aim);
        let mut hit = false;
        for _ in 0..80 {
            let next = m + dir * 11.0 * 0.05;
            hit |= pass_distance(m, next, h) <= MISSILE_HIT;
            m = next;
            h += Vec3::Z * 7.0 * 0.05;
        }
        assert!(!hit);
        // One who stands still is hit.
        let (mut m, h) = (from, aim);
        let mut hit = false;
        for _ in 0..80 {
            let next = m + dir * 11.0 * 0.05;
            hit |= pass_distance(m, next, h) <= MISSILE_HIT;
            m = next;
        }
        assert!(hit);
    }

    #[test]
    fn a_telegraph_fills_then_strikes_once_then_flashes_and_goes() {
        let mut t = Telegraph::new(Shape::Circle { radius: 5.0 }, Vec3::ZERO, 0.0, 1.5, 10.0, None);
        let dt = 0.05;
        let mut beats = Vec::new();
        let mut at = 0.0;
        let mut struck_at = None;
        for _ in 0..200 {
            at += dt;
            let beat = t.advance(dt);
            if beat == Beat::Strike {
                struck_at = Some(at);
            }
            beats.push(beat);
            if beat == Beat::Done {
                break;
            }
        }
        assert_eq!(beats.iter().filter(|b| **b == Beat::Strike).count(), 1);
        let struck = struck_at.expect("it strikes");
        assert!((1.45..=1.6).contains(&struck), "{struck}");
        assert_eq!(*beats.last().unwrap(), Beat::Done);
        // It charged from empty toward full, in order, before the strike.
        let charge: Vec<f32> = beats
            .iter()
            .filter_map(|b| match b {
                Beat::Charge(p) => Some(*p),
                _ => None,
            })
            .collect();
        assert!(charge.first().unwrap() < &0.1);
        assert!(charge.last().unwrap() > &0.9);
        assert!(charge.windows(2).all(|w| w[0] < w[1]));
        // Flashing, the share left only falls.
        let flash: Vec<f32> = beats
            .iter()
            .filter_map(|b| match b {
                Beat::Flash(f) => Some(*f),
                _ => None,
            })
            .collect();
        assert!(!flash.is_empty());
        assert!(flash.windows(2).all(|w| w[0] > w[1]));
    }

    #[test]
    fn the_fill_grows_from_the_centre_out() {
        let ring = |colors: &[[f32; 4]], k: usize| colors[k * (COLS + 1)][3];
        let half: Vec<[f32; 4]> = fill_colors(0.5, None).collect();
        assert_eq!(half.len(), (BANDS + 1) * (COLS + 1));
        assert_eq!(ring(&half, 0), FILL_ON);
        assert_eq!(ring(&half, BANDS / 2), FILL_ON);
        assert_eq!(ring(&half, BANDS), FILL_OFF);
        let full: Vec<[f32; 4]> = fill_colors(1.0, None).collect();
        assert!((0..=BANDS).all(|k| ring(&full, k) == FILL_ON));
        let empty: Vec<[f32; 4]> = fill_colors(0.0, None).collect();
        assert_eq!(ring(&empty, 1), FILL_OFF);
    }

    /// One of each shape the scripts lay.
    fn every_shape() -> [Shape; 4] {
        [
            Shape::Circle { radius: 5.0 },
            Shape::Cone {
                radius: 9.0,
                half_angle: 0.6,
            },
            Shape::Line {
                length: 20.0,
                width: 4.0,
            },
            Shape::Donut {
                inner: 4.0,
                outer: 12.0,
            },
        ]
    }

    #[test]
    fn the_meshes_have_a_vertex_for_every_colour() {
        let per = COLS + 1;
        let heights = vec![0.0; (BANDS + 1) * per];
        for shape in every_shape() {
            let fill = fill_mesh(shape, &heights);
            assert_eq!(fill.count_vertices(), (BANDS + 1) * per, "{shape:?}");
            let colors = fill_colors(0.5, None).count();
            assert_eq!(colors, fill.count_vertices(), "{shape:?}");
        }
        // A circle's edge is one ribbon round the rim; the other shapes outline their sides too.
        let edge = |shape| edge_mesh(shape, &heights).count_vertices();
        let [circle, cone, line, ring] = every_shape();
        assert_eq!(edge(circle), COLS * 4);
        assert_eq!(edge(ring), COLS * 4 * 2);
        assert_eq!(edge(line), (COLS * 4 + BANDS * 4) * 2);
        // A cone outlines its rim and both sides.
        assert_eq!(edge(cone), (COLS + BANDS * 2) * 4);
    }

    #[test]
    fn the_lattice_follows_the_shape_it_draws() {
        let reach = |shape: Shape| {
            (0..=BANDS)
                .flat_map(|k| (0..=COLS).map(move |i| (k, i)))
                .map(|(k, i)| lattice(shape, k, i))
                .collect::<Vec<_>>()
        };
        for shape in every_shape() {
            let points = reach(shape);
            // Every vertex of the fill lies in the shape (a hair of rounding allowed) ...
            let wide = match shape {
                Shape::Cone { radius, half_angle } => Shape::Cone {
                    radius: radius + 0.01,
                    half_angle: half_angle + 0.01,
                },
                other => other.grown(0.01),
            };
            assert!(
                points.iter().all(|p| wide.contains(Vec3::ZERO, 0.0, *p)),
                "{shape:?}"
            );
            // ... and the last ring reaches as far as the shape does.
            let far = points
                .iter()
                .map(|p| p.length())
                .fold(0.0f32, f32::max);
            assert!((far - shape.reach()).abs() < 0.2, "{shape:?}: {far}");
        }
        // A line points along -Z and starts at its foot.
        let line = Shape::Line {
            length: 20.0,
            width: 4.0,
        };
        assert_eq!(lattice(line, 0, COLS / 2), Vec3::ZERO);
        assert!((lattice(line, BANDS, COLS / 2) - Vec3::new(0.0, 0.0, -20.0)).length() < 1e-4);
    }

    #[test]
    fn affixes_are_rolled_evenly() {
        // A sweep of the whole range is exactly even.
        let mut count = [0u32; 3];
        for i in 0..3000 {
            let affix = roll_affix(i as f32 / 3000.0);
            count[Affix::ALL.iter().position(|a| *a == affix).unwrap()] += 1;
        }
        assert_eq!(count, [1000, 1000, 1000]);
        // The edges stay in range.
        assert_eq!(roll_affix(0.0), Affix::Swift);
        assert_eq!(roll_affix(1.0), Affix::Volatile);
        // And the run's own dice give each about a third.
        let mut rng = Rng(0x1234_5678_9abc_def1);
        let mut count = [0u32; 3];
        for _ in 0..3000 {
            let affix = roll_affix(rng.f32());
            count[Affix::ALL.iter().position(|a| *a == affix).unwrap()] += 1;
        }
        assert!(count.iter().all(|c| (850..=1150).contains(c)), "{count:?}");
    }

    #[test]
    fn every_affix_has_a_label() {
        for a in Affix::ALL {
            assert!(!a.label().is_empty());
        }
    }

    #[test]
    fn bosses_take_turns_by_schedule_and_the_finale_does_both() {
        assert_eq!(plan_for(0, false), Plan::Circle);
        assert_eq!(plan_for(1, false), Plan::Volley);
        assert_eq!(plan_for(2, false), Plan::Circle);
        assert_eq!(plan_for(5, false), Plan::Volley);
        assert_eq!(plan_for(0, true), Plan::Both);
        let mut last = false;
        let turns: Vec<bool> = (0..4)
            .map(|_| {
                last = is_volley(Plan::Both, last);
                last
            })
            .collect();
        assert_eq!(turns, [true, false, true, false]);
        assert!(!is_volley(Plan::Circle, true));
        assert!(is_volley(Plan::Volley, false));
    }

    /// A grunt's elite, as [`enemies::spawn_enemy`] makes it: 100 health, 4 yd/s.
    fn elite() -> Enemy {
        let def = &super::super::data::ENEMIES[0];
        let mut e = enemies::shown_enemy(def, 100.0, 100.0, false, true, false);
        e.speed = 4.0;
        e
    }

    #[test]
    fn a_shielded_elite_has_half_again_the_health_and_the_shield_is_the_top_third() {
        let mut world = World::new();
        let ent = world.spawn_empty().id();
        let mut e = elite();
        let mut marks = Marks::debuffs();
        apply_affix(
            &mut world.commands(),
            ent,
            &mut e,
            &mut marks,
            Affix::Shielded,
        );
        world.flush();
        assert_eq!((e.hp, e.max_hp), (150.0, 150.0));
        let shield = world.get::<Shield>(ent).expect("a shield");
        assert_eq!(shield.base, 100.0);
        assert_eq!(world.get::<Affix>(ent), Some(&Affix::Shielded));
        // The shield wears a bubble beside the elite's own.
        assert_eq!(marks.spells()[..2], [enemies::ELITE_AURA, SHIELD_AURA]);
        // Blows take the first 50 off the top: until then the shield stands.
        assert!(150.0 - 49.0 > shield.base);
        assert!(150.0 - 50.0 <= shield.base);
    }

    #[test]
    fn a_swift_elite_is_forty_percent_faster_and_the_rest_are_untouched() {
        let mut world = World::new();
        let ent = world.spawn_empty().id();
        let (mut e, mut marks) = (elite(), Marks::debuffs());
        apply_affix(&mut world.commands(), ent, &mut e, &mut marks, Affix::Swift);
        world.flush();
        assert!((e.speed - 5.6).abs() < 1e-5);
        assert_eq!(e.max_hp, 100.0);
        assert!(world.get::<Shield>(ent).is_none());
        let ent = world.spawn_empty().id();
        let (mut e, mut marks) = (elite(), Marks::debuffs());
        apply_affix(
            &mut world.commands(),
            ent,
            &mut e,
            &mut marks,
            Affix::Volatile,
        );
        world.flush();
        assert_eq!((e.speed, e.max_hp), (4.0, 100.0));
        assert_eq!(world.get::<Affix>(ent), Some(&Affix::Volatile));
    }

    #[test]
    fn the_casters_are_named_by_their_words() {
        use super::super::data::school_of;
        assert_eq!(school_of("Razormane Mystic"), Some(School::Nature));
        assert_eq!(school_of("Frostmane Seer"), Some(School::Frost));
        assert_eq!(school_of("Frostmane Snowstrider"), None);
        assert_eq!(school_of("Death Cultist"), Some(School::Shadow));
        assert_eq!(school_of("Gurubashi Axe Thrower"), Some(School::Arrow));
        // Beasts that share a word with a caster are not casters.
        assert_eq!(school_of("Witchwing Harpy"), None);
        assert_eq!(school_of("Ravasaur Hunter"), None);
        assert_eq!(school_of("Savannah Huntress"), None);
    }

    #[test]
    fn every_zone_has_a_few_shooters_among_its_horde_and_none_among_its_bosses() {
        for map in 0..super::super::data::MAPS.len() {
            let roster = super::super::data::roster(map);
            let shooters = roster.enemies.iter().filter(|e| e.ranged.is_some()).count();
            assert!(
                (1..=6).contains(&shooters),
                "{}: {shooters} shooters of {}",
                super::super::data::MAPS[map].name,
                roster.enemies.len()
            );
            assert!(roster.bosses.iter().all(|b| b.ranged.is_none()));
        }
    }
}
