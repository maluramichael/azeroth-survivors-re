//! Boss scripts: a boss with a [`kits`] row fights with its own moves instead of the shared turns
//! of circle and volley ([`super::threats`]).
//!
//! The data model is taken from `raid/` of deanmathieson/azeroth-survivors-re (branch
//! `progression`, commit `10c6a77`, MIT OR Apache-2.0): a boss is a [`Kit`] of [`BossAbility`]
//! rows, each with a timer or a health threshold ([`When`]), a place to aim ([`Aim`]) and what
//! it does ([`Does`]); [`BossBrain::due`] is Dean's `RaidBoss::due`. Left out: Dean's debuffs with
//! dispels, tank swaps and forced movement, and his damage as a share of the hero's health; here
//! a blow is a share of the boss's own damage, which the zone ladder, the clock and the
//! difficulty already scale.
//!
//! What the abilities do:
//! - **Ground**: a shape warns on the ground ([`Shape`], drawn by [`super::threats`]) and then
//!   strikes whoever still stands in it: a circle under the hero or round the boss (a stomp), a
//!   bomb or several round the hero, a cone or a line from the boss to the hero, a ring that is
//!   safe near the boss. A blow may leave a burning [`Pool`].
//! - **Volley**: missiles in a ring round the boss, or in a fan at the hero.
//! - **Adds**: creatures from the zone's [`super::data::extras`], round the boss.
//! - **Charge**: a line warns, then the boss runs along it ([`Steered`], so the chase leaves it
//!   be) and strikes each hero it meets once.
//! - **Frenzy**: the boss runs faster for a while.
//! - **Phase**: switches the rows that run ([`BossAbility::in_phases`]).
//!
//! The brain runs where the run is simulated (the host or a solo player); a co-op guest sees the
//! boss and its objects as it sees any others.

mod kits;
mod shapes;

use std::f32::consts::TAU;

use benilla_app::survivors_host::creature_anim::{move_flags, MovementState};
use benilla_app::survivors_host::sound::GlueSound;
use benilla_world::schedule::WorldStage;
use benilla_world::world_point::WorldPoint;
use bevy::ecs::schedule::ScheduleConfigs;
use bevy::ecs::system::ScheduleSystem;
use bevy::prelude::*;

use super::chatlog::{ChatKind, ChatLine};
use super::data::{self, EnemyDef, School};
use super::enemies::{self, Dying, Enemy, Steered};
use super::finale::Finale;
use super::hero::{Hero, HERO_RADIUS};
use super::items::{BossDrop, Tpl};
use super::nav::{NavGrid, SpawnProbe};
use super::threats::{self, Canvas, Heroes};
use super::units::{self, flat, yaw_toward, Fx, Marks};
use super::{Rng, Run, Selection};

pub(crate) use shapes::Shape;

pub(super) fn plugin(app: &mut App) {
    app.add_message::<GlueSound>().add_systems(
        Update,
        (systems(), debug_summon.after(attach_kits))
            .after(WorldStage::Input)
            .run_if(super::run_on)
            .run_if(super::simulates)
            .run_if(
                |l: Res<benilla_app::survivors_host::loading_screen::LoadingScreen>| !l.covering(),
            ),
    );
}

/// The boss scripts' systems in the order they run.
fn systems() -> ScheduleConfigs<ScheduleSystem> {
    (attach_kits, run_kits, charge, burn_pools, boss_loot)
        .chain()
        .into_configs()
}

/// The boss scripts' systems, for the balance simulator ([`super::balance`]). The simulator needs
/// the [`GlueSound`] and [`BossDrop`] messages registered.
#[cfg(test)]
#[allow(dead_code)]
pub(super) fn sim_systems() -> ScheduleConfigs<ScheduleSystem> {
    systems()
}

// --- The script model ---

/// When an ability fires, on the fight's own clock (seconds since the boss walked out).
#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) enum When {
    /// First at `first`, then every `every` seconds.
    Every { first: f32, every: f32 },
    /// Once, this many seconds in.
    #[allow(dead_code)]
    At(f32),
    /// Once, as the boss's health first falls below this share of its maximum.
    Health(f32),
}

/// Where an ability is aimed.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Aim {
    /// At the boss: a stomp round it, a ring of missiles from it, its own frenzy.
    Boss,
    /// At the nearest hero: a circle under it, a cone, a line or a fan toward it.
    Hero,
    /// This many circles round the nearest hero, the first right under it.
    AroundHero(u8),
}

/// What an ability does.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) enum Does {
    /// A shape on the ground that warns for `windup` seconds and then strikes for `damage` times
    /// the boss's own. `pool`: the blow leaves a pool burning for the seconds that does that
    /// share of the boss's damage every second. A circle or a ring lies under the hero or round
    /// the boss ([`Aim`]); a cone or a line starts at the boss and points at the hero.
    Ground {
        shape: Shape,
        windup: f32,
        damage: f32,
        pool: Option<(f32, f32)>,
    },
    /// `count` missiles of `school`: a ring round the boss, or with `spread` (radians across) a
    /// fan at the hero.
    Volley {
        count: u8,
        school: School,
        spread: Option<f32>,
    },
    /// `count` of the creature called `name` from the zone's [`data::extras`], on a ring of
    /// `radius` yards round the boss.
    Adds {
        name: &'static str,
        count: u8,
        radius: f32,
    },
    /// A line warns for `windup` seconds, then the boss runs along it at `speed` yards a second
    /// and strikes each hero it meets once.
    Charge { windup: f32, speed: f32 },
    /// The boss runs `speed` times as fast for `secs` seconds.
    Frenzy { secs: f32, speed: f32 },
    /// Switches to a phase: the rows [`BossAbility::in_phases`] names for it run from now on.
    #[allow(dead_code)]
    Phase(u8),
}

/// Every phase.
const ALL_PHASES: u8 = 0xFF;

/// One row of a boss's script.
#[derive(Clone, Copy, Debug)]
pub(super) struct BossAbility {
    /// The row's name, for the reader of a kit and its tests.
    #[cfg_attr(not(test), allow(dead_code))]
    pub name: &'static str,
    /// The spell whose cast the boss plays when the row fires (0: none); for a frenzy also the
    /// aura it wears. Only the look: the blow is the row's own.
    pub spell: u32,
    pub when: When,
    pub aim: Aim,
    pub does: Does,
    /// A line the boss calls when the row fires: the text for the chat, and the `SoundEntries`
    /// kit it plays (empty: none).
    pub yell: Option<(&'static str, &'static str)>,
    /// Bit `p` set: the row runs in phase `p`.
    pub phases: u8,
}

impl BossAbility {
    pub(super) const fn new(
        name: &'static str,
        spell: u32,
        when: When,
        aim: Aim,
        does: Does,
    ) -> Self {
        Self {
            name,
            spell,
            when,
            aim,
            does,
            yell: None,
            phases: ALL_PHASES,
        }
    }

    /// The row calls out `text` as it fires, with the sound kit `kit` (empty: silent).
    pub(super) const fn yell(mut self, text: &'static str, kit: &'static str) -> Self {
        self.yell = Some((text, kit));
        self
    }

    /// The row runs only in the phases whose bits are set in `phases`.
    #[allow(dead_code)]
    pub(super) const fn in_phases(mut self, phases: u8) -> Self {
        self.phases = phases;
        self
    }

    fn runs_in(&self, phase: u8) -> bool {
        self.phases & (1u8 << phase.min(7)) != 0
    }
}

/// A boss's script: who it is, what it does, what it says on arrival and what it may drop.
pub(super) struct Kit {
    /// The boss's name in the zone's roster (or the finale's).
    pub boss: &'static str,
    pub abilities: &'static [BossAbility],
    /// What the boss calls as it arrives: the chat text and the `SoundEntries` kit.
    pub arrive: Option<(&'static str, &'static str)>,
    /// Pieces only this boss drops ([`BossDrop`]).
    pub loot: &'static [Tpl],
}

/// The script of the boss called `name`, if it has one.
pub(super) fn kit_for(name: &str) -> Option<&'static Kit> {
    kits::KITS.iter().find(|k| k.boss == name)
}

/// Whether the boss called `name` has a script of its own, so [`super::threats`] gives it none of
/// the shared turns.
pub(super) fn scripted(name: &str) -> bool {
    kit_for(name).is_some()
}

/// The adds battleground `map`'s boss scripts call, the first part of [`super::data::extras`]
/// (found again by co-op as `(5, i)`). Their numbers come from [`super::data::rung`] at their
/// place on the zone's ladder; name, look and pace are their own.
pub(super) fn adds_for(map: usize) -> Vec<EnemyDef> {
    let zone = data::MAPS[map].name;
    kits::ADDS
        .iter()
        .filter(|a| a.zone == zone)
        .map(|a| EnemyDef {
            name: a.name,
            display: a.display,
            scale: a.scale,
            speed: a.speed,
            big: false,
            ..data::rung(map, a.at)
        })
        .collect()
}

// --- The brain ---

/// A boss's place in its script.
#[derive(Component)]
pub(super) struct BossBrain {
    kit: &'static Kit,
    /// Seconds since the boss arrived.
    clock: f32,
    /// Per row: when a timed row fires next.
    next: Vec<f32>,
    /// Per row: a one-shot row has fired.
    done: Vec<bool>,
    phase: u8,
    /// Seconds of frenzy left, and the factor it speeds the boss by (1: none).
    frenzy_t: f32,
    frenzy_mult: f32,
}

impl BossBrain {
    fn new(kit: &'static Kit) -> Self {
        Self {
            kit,
            clock: 0.0,
            next: kit
                .abilities
                .iter()
                .map(|a| match a.when {
                    When::Every { first, .. } => first,
                    _ => f32::INFINITY,
                })
                .collect(),
            done: vec![false; kit.abilities.len()],
            phase: 0,
            frenzy_t: 0.0,
            frenzy_mult: 1.0,
        }
    }

    /// The clock on by `dt` with the boss at `health` (a share of its maximum): the rows due now,
    /// in script order. A phase change takes effect at once, for the rows after it; a timed row
    /// whose phase is off still keeps its beat.
    fn due(&mut self, dt: f32, health: f32) -> Vec<&'static BossAbility> {
        self.clock += dt;
        let kit = self.kit;
        let mut out = Vec::new();
        for (i, a) in kit.abilities.iter().enumerate() {
            let ready = match a.when {
                When::Every { every, .. } => {
                    let ready = self.clock >= self.next[i];
                    if ready {
                        self.next[i] += every.max(0.1);
                    }
                    ready
                }
                When::At(secs) => {
                    let ready = !self.done[i] && self.clock >= secs;
                    self.done[i] |= ready;
                    ready
                }
                When::Health(below) => {
                    let ready = !self.done[i] && health < below;
                    self.done[i] |= ready;
                    ready
                }
            };
            if ready && a.runs_in(self.phase) {
                if let Does::Phase(p) = a.does {
                    self.phase = p;
                }
                out.push(a);
            }
        }
        out
    }

    /// The frenzy on by `dt`: when it has just run out, the factor to take off the boss's speed.
    fn frenzy_over(&mut self, dt: f32) -> Option<f32> {
        if self.frenzy_mult == 1.0 {
            return None;
        }
        self.frenzy_t -= dt;
        if self.frenzy_t > 0.0 {
            return None;
        }
        let mult = self.frenzy_mult;
        self.frenzy_mult = 1.0;
        self.frenzy_t = 0.0;
        Some(mult)
    }
}

/// Chat line and sound for a boss calling out: `kit` is the `SoundEntries` kit (empty: none).
fn yell(
    chat: &mut MessageWriter<ChatLine>,
    sound: &mut MessageWriter<GlueSound>,
    who: &str,
    text: &str,
    kit: &'static str,
) {
    chat.write(ChatLine::text(ChatKind::Boss, format!("{who} yells: {text}")));
    if !kit.is_empty() {
        sound.write(GlueSound(kit));
    }
}

/// A boss with a script gets its brain as it arrives, and calls out if its kit says so.
fn attach_kits(
    mut commands: Commands,
    mut chat: MessageWriter<ChatLine>,
    mut sound: MessageWriter<GlueSound>,
    fresh: Query<(Entity, &Enemy), Added<Enemy>>,
) {
    for (ent, e) in &fresh {
        if !e.boss {
            continue;
        }
        let Some(kit) = kit_for(e.def.name) else {
            continue;
        };
        commands.entity(ent).insert(BossBrain::new(kit));
        if let Some((text, voice)) = kit.arrive {
            yell(&mut chat, &mut sound, e.def.name, text, voice);
        }
    }
}

// --- Tuning ---

/// A ring of missiles: their pace, how far they fly and each one's blow as a share of the boss's
/// damage (the shared turns' numbers).
const RING_SPEED: f32 = 9.0;
const RING_RANGE: f32 = 38.0;
const RING_DAMAGE: f32 = 0.2;
/// A fan of missiles is quicker and each hurts more, for few of them reach.
const FAN_SPEED: f32 = 12.0;
const FAN_RANGE: f32 = 40.0;
const FAN_DAMAGE: f32 = 0.3;
/// How far from the hero the bombs after the first fall, in yards.
const BOMB_RING: f32 = 6.5;

/// The charge's warning is a line at least this wide (it is as wide as the boss strikes), and at
/// least and at most this long, reaching this far past the hero it aims at.
const CHARGE_WIDTH: f32 = 4.0;
const CHARGE_MIN: f32 = 18.0;
const CHARGE_MAX: f32 = 40.0;
const CHARGE_OVERRUN: f32 = 8.0;
/// A charge's blow as a share of the boss's damage, and how much closer than touching it lands.
const CHARGE_DAMAGE: f32 = 2.0;
const CHARGE_PAD: f32 = 0.8;
/// A charge that gets this long nowhere (a wall) is over.
const CHARGE_STUCK: f32 = 0.3;

/// The ground spell a burning pool wears: Flamestrike's area. The plan checked the id against
/// `Spell.dbc`; that the area visual stays up as long as the object does is an assumption.
const POOL_SPELL: u32 = 2120;
/// A pool hurts every hero in it this often.
const POOL_TICK: f32 = 0.5;

/// Chance a boss with signature loot drops a piece (the finale's always does).
const LOOT_CHANCE: f32 = 0.6;

// --- Pure rules ---

/// Where `n` bombs fall round `center`: the first right on it, the rest on a ring of
/// [`BOMB_RING`] yards, evenly spaced from `phase` radians.
fn bomb_spots(center: Vec3, n: u8, phase: f32) -> Vec<Vec3> {
    let n = n.max(1);
    let outer = f32::from(n - 1);
    (0..n)
        .map(|i| {
            if i == 0 {
                center
            } else {
                let a = phase + f32::from(i - 1) / outer * TAU;
                center + Vec3::new(a.cos(), 0.0, a.sin()) * BOMB_RING
            }
        })
        .collect()
}

/// The directions of `count` missiles: evenly round the circle from `phase`, or with `spread`
/// radians across a fan about `toward` (a ground-plane direction).
fn volley_dirs(count: u8, spread: Option<f32>, toward: Vec3, phase: f32) -> Vec<Vec3> {
    let n = usize::from(count.max(1));
    let dir = |a: f32| Vec3::new(a.cos(), 0.0, a.sin());
    match spread {
        None => (0..n).map(|i| dir(phase + i as f32 / n as f32 * TAU)).collect(),
        Some(spread) => {
            let mid = toward.z.atan2(toward.x);
            (0..n)
                .map(|i| {
                    let t = if n == 1 { 0.5 } else { i as f32 / (n - 1) as f32 };
                    dir(mid + (t - 0.5) * spread)
                })
                .collect()
        }
    }
}

/// How long a charge's line is for a hero `dist` yards away.
fn charge_length(dist: f32) -> f32 {
    (dist + CHARGE_OVERRUN).clamp(CHARGE_MIN, CHARGE_MAX)
}

// --- Running a script ---

/// What a row is fired at: the boss and the hero it picked.
struct Cast {
    pos: Vec3,
    /// The way the boss faces.
    facing: f32,
    /// The boss's blow before the row's share of it.
    damage: f32,
    target: Vec3,
}

impl Cast {
    /// The way from the boss to the hero it picked, as a yaw.
    fn toward(&self) -> f32 {
        let to = flat(self.target - self.pos);
        if to.length_squared() > 0.01 {
            yaw_toward(to)
        } else {
            self.facing
        }
    }
}

/// Bosses with a script fire the rows that are due.
#[allow(clippy::too_many_arguments)]
fn run_kits(
    mut commands: Commands,
    time: Res<Time>,
    mut rng: ResMut<Rng>,
    selection: Res<Selection>,
    nav: Res<NavGrid>,
    probe: SpawnProbe,
    mut canvas: Canvas,
    mut heroes: Heroes,
    mut fx: Fx,
    mut chat: MessageWriter<ChatLine>,
    mut sound: MessageWriter<GlueSound>,
    mut bosses: Query<
        (Entity, &Transform, &mut Enemy, &mut BossBrain, &mut Marks),
        (Without<Dying>, Without<Charging>),
    >,
) {
    let dt = time.delta_secs();
    if dt <= 0.0 {
        return;
    }
    let bodies = heroes.standing();
    for (ent, tf, mut e, mut brain, mut marks) in &mut bosses {
        if let Some(mult) = brain.frenzy_over(dt) {
            e.speed /= mult;
        }
        if e.dead || e.stun_t > 0.0 {
            continue;
        }
        // A boss sees every hero, Fade or not. With none standing the script waits.
        let Some(target) = threats::nearest(&bodies, tf.translation, true) else {
            continue;
        };
        let due = brain.due(dt, e.hp / e.max_hp.max(1.0));
        if due.is_empty() {
            continue;
        }
        let cast = Cast {
            pos: tf.translation,
            facing: tf.rotation.to_euler(EulerRot::YXZ).0,
            damage: e.damage * e.weaken,
            target: target.pos,
        };
        for a in due {
            if a.spell != 0 {
                fx.cast(ent, a.spell, &[], None);
            }
            match a.does {
                Does::Ground {
                    shape,
                    windup,
                    damage,
                    pool,
                } => {
                    let phase = rng.range(0.0, TAU);
                    let blow = cast.damage * damage;
                    let pool = pool.map(|(share, secs)| (cast.damage * share, secs));
                    lay_ground(&mut canvas, &mut commands, &cast, a.aim, shape, windup, blow, pool, phase);
                }
                Does::Volley {
                    count,
                    school,
                    spread,
                } => {
                    let phase = rng.range(0.0, TAU);
                    let toward = flat(cast.target - cast.pos).normalize_or(Vec3::X);
                    let dirs = volley_dirs(count, spread, toward, phase);
                    let (speed, range, share) = if spread.is_some() {
                        (FAN_SPEED, FAN_RANGE, FAN_DAMAGE)
                    } else {
                        (RING_SPEED, RING_RANGE, RING_DAMAGE)
                    };
                    // Arrows and thrown daggers have no spell to land.
                    let impact = if school == School::Arrow { 0 } else { a.spell };
                    canvas.missiles(
                        &mut commands,
                        school,
                        cast.pos,
                        &dirs,
                        speed,
                        range,
                        cast.damage * share,
                        impact,
                    );
                }
                Does::Adds {
                    name,
                    count,
                    radius,
                } => {
                    let phase = rng.range(0.0, TAU);
                    call_adds(
                        &mut commands,
                        &mut heroes,
                        &canvas,
                        &nav,
                        &probe,
                        selection.map,
                        &cast,
                        name,
                        count,
                        radius,
                        phase,
                    );
                }
                Does::Charge { windup, speed } => {
                    let to = flat(cast.target - cast.pos);
                    let dir = to.normalize_or(forward_of(cast.facing));
                    let length = charge_length(to.length());
                    let yaw = yaw_toward(dir);
                    // The line is as wide as the boss strikes: whoever stands in it is hit.
                    let reach = e.radius + HERO_RADIUS + CHARGE_PAD;
                    canvas.telegraph(
                        &mut commands,
                        Shape::Line {
                            length,
                            width: (2.0 * reach).max(CHARGE_WIDTH),
                        },
                        cast.pos,
                        yaw,
                        windup,
                        0.0,
                        None,
                    );
                    commands.entity(ent).insert((
                        Steered,
                        Charging {
                            dir,
                            yaw,
                            wind: windup,
                            length,
                            reach,
                            speed,
                            run: 0.0,
                            stuck: 0.0,
                            hit: Vec::new(),
                        },
                    ));
                }
                Does::Frenzy { secs, speed } => {
                    if brain.frenzy_mult == 1.0 {
                        e.speed *= speed;
                        brain.frenzy_mult = speed;
                    }
                    brain.frenzy_t = secs;
                    if a.spell != 0 {
                        marks.add(a.spell, secs);
                    }
                }
                Does::Phase(_) => {}
            }
            if let Some((text, voice)) = a.yell {
                yell(&mut chat, &mut sound, e.def.name, text, voice);
            }
        }
    }
}

/// The ground-plane direction a yaw faces.
fn forward_of(yaw: f32) -> Vec3 {
    shapes::forward(yaw)
}

/// Warn of a ground row's shape and let it strike: `blow` is the damage, `pool` the burning pool
/// it leaves as (damage per second, seconds).
#[allow(clippy::too_many_arguments)]
fn lay_ground(
    canvas: &mut Canvas,
    commands: &mut Commands,
    cast: &Cast,
    aim: Aim,
    shape: Shape,
    windup: f32,
    blow: f32,
    pool: Option<(f32, f32)>,
    phase: f32,
) {
    match shape {
        // A cone or a line starts at the boss and points at the hero.
        Shape::Cone { .. } | Shape::Line { .. } => {
            canvas.telegraph(commands, shape, cast.pos, cast.toward(), windup, blow, pool);
        }
        Shape::Circle { .. } | Shape::Donut { .. } => {
            let spots = match aim {
                Aim::Boss => vec![cast.pos],
                Aim::Hero => vec![cast.target],
                Aim::AroundHero(n) => bomb_spots(cast.target, n, phase),
            };
            for at in spots {
                canvas.telegraph(commands, shape, at, 0.0, windup, blow, pool);
            }
        }
    }
}

/// `count` of the creature called `name` stand up on a ring of `radius` yards round the boss.
#[allow(clippy::too_many_arguments)]
fn call_adds(
    commands: &mut Commands,
    heroes: &mut Heroes,
    canvas: &Canvas,
    nav: &NavGrid,
    probe: &SpawnProbe,
    map: usize,
    cast: &Cast,
    name: &str,
    count: u8,
    radius: f32,
    phase: f32,
) {
    let Some(def) = data::extras(map).iter().find(|d| d.name == name) else {
        warn!("survivors: no add called {name} in zone {map}");
        return;
    };
    for i in 0..count {
        let a = phase + f32::from(i) / f32::from(count.max(1)) * TAU;
        // No free spot (a wall, a tree): that one stays away.
        let Some(at) = canvas.spot(nav, probe, cast.pos, a, radius) else {
            continue;
        };
        enemies::spawn_enemy(commands, heroes.run(), def, at, cast.target, false, false);
    }
}

// --- Charges ---

/// A boss running its charge: first it stands and faces the line (`wind` seconds), then it runs
/// `length` yards along `dir`, striking each hero it meets once. It carries [`Steered`] for the
/// while, so the chase leaves it be.
#[derive(Component)]
pub(super) struct Charging {
    dir: Vec3,
    yaw: f32,
    /// Wind-up left, in seconds.
    wind: f32,
    length: f32,
    /// How near a hero must be to the boss's middle to be struck.
    reach: f32,
    speed: f32,
    /// Yards run so far.
    run: f32,
    /// Seconds spent getting nowhere.
    stuck: f32,
    /// The heroes it has struck.
    hit: Vec<Entity>,
}

/// Charging bosses stand through their warning, then run the line and strike what they meet.
#[allow(clippy::too_many_arguments)]
fn charge(
    mut commands: Commands,
    time: Res<Time>,
    nav: Res<NavGrid>,
    world: WorldPoint,
    mut heroes: Heroes,
    mut fx: Fx,
    mut runners: Query<(
        Entity,
        &mut Transform,
        &Enemy,
        &mut MovementState,
        &mut Charging,
    )>,
) {
    let dt = time.delta_secs();
    if dt <= 0.0 {
        return;
    }
    let bodies = heroes.standing();
    for (ent, mut tf, e, mut mv, mut ch) in &mut runners {
        if e.dead {
            end_charge(&mut commands, ent, &mut mv);
            continue;
        }
        if ch.wind > 0.0 {
            // It faces the line it is about to run.
            ch.wind -= dt;
            tf.rotation = Quat::from_rotation_y(ch.yaw);
            continue;
        }
        let from = tf.translation;
        let want = ch.speed * dt;
        let mut next = nav.step(from, from + ch.dir * want);
        next.y = threats::ground_y(&world, next, from.y);
        let moved = flat(next - from).length();
        ch.run += moved;
        ch.stuck = if moved < want * 0.3 { ch.stuck + dt } else { 0.0 };
        // Each hero it passes within reach is struck once.
        let blow = e.damage * e.weaken * CHARGE_DAMAGE;
        for body in &bodies {
            if ch.hit.contains(&body.e) || threats::pass_distance(from, next, body.pos) > ch.reach {
                continue;
            }
            ch.hit.push(body.e);
            let taken = heroes.strike(body, blow, &mut fx);
            fx.swing(ent, body.e, taken);
        }
        tf.translation = next;
        tf.rotation = Quat::from_rotation_y(ch.yaw);
        mv.speed = ch.speed;
        mv.flags = move_flags::FORWARD;
        if ch.run >= ch.length || ch.stuck > CHARGE_STUCK {
            end_charge(&mut commands, ent, &mut mv);
        }
    }
}

/// The charge is over: the boss walks (and chases) again.
fn end_charge(commands: &mut Commands, ent: Entity, mv: &mut MovementState) {
    commands.entity(ent).remove::<(Charging, Steered)>();
    mv.speed = 0.0;
    mv.flags = 0;
}

// --- Pools ---

/// A pool a blow left burning on the ground: it hurts every hero in it every [`POOL_TICK`]
/// seconds until it has burnt out. Laid by [`Pool::lay`] from a [`super::threats`] telegraph.
#[derive(Component)]
pub(super) struct Pool {
    pub shape: Shape,
    pub origin: Vec3,
    pub yaw: f32,
    /// Damage a second.
    dps: f32,
    /// Seconds left.
    left: f32,
    tick: f32,
}

impl Pool {
    /// Lay a pool of `shape` at `origin` burning for `secs` seconds at `dps` damage a second,
    /// with the ground-area look of [`POOL_SPELL`]. `serial` is the area object's.
    pub(super) fn lay(
        commands: &mut Commands,
        serial: u64,
        shape: Shape,
        origin: Vec3,
        yaw: f32,
        dps: f32,
        secs: f32,
    ) {
        let area = units::spawn_area_object(commands, serial, POOL_SPELL, shape.reach(), origin);
        commands.entity(area).insert(Pool {
            shape,
            origin,
            yaw,
            dps,
            left: secs,
            tick: POOL_TICK,
        });
    }
}

/// Pools burn the heroes in them and go out.
fn burn_pools(
    mut commands: Commands,
    time: Res<Time>,
    mut heroes: Heroes,
    mut fx: Fx,
    mut pools: Query<(Entity, &mut Pool)>,
) {
    let dt = time.delta_secs();
    if dt <= 0.0 {
        return;
    }
    let bodies = heroes.standing();
    for (ent, mut pool) in &mut pools {
        pool.left -= dt;
        if pool.left <= 0.0 {
            commands.entity(ent).despawn();
            continue;
        }
        pool.tick -= dt;
        if pool.tick > 0.0 {
            continue;
        }
        pool.tick += POOL_TICK;
        for body in &bodies {
            if pool.shape.contains(pool.origin, pool.yaw, body.pos) {
                heroes.strike(body, pool.dps * POOL_TICK, &mut fx);
            }
        }
    }
}

// --- Loot ---

/// A scripted boss that has fallen may drop a piece only it has ([`BossDrop`]); the finale's
/// always does, and goes straight on, as the run ends with it.
fn boss_loot(
    mut rng: ResMut<Rng>,
    mut drops: MessageWriter<BossDrop>,
    fallen: Query<(&Transform, &Enemy, Has<Finale>), Added<Dying>>,
) {
    for (tf, e, finale) in &fallen {
        if !e.boss {
            continue;
        }
        let Some(kit) = kit_for(e.def.name) else {
            continue;
        };
        let loot: &'static [Tpl] = kit.loot;
        if loot.is_empty() || (!finale && rng.f32() >= LOOT_CHANCE) {
            continue;
        }
        drops.write(BossDrop {
            pos: tf.translation,
            tpl: &loot[rng.index(loot.len())],
            wear_now: finale,
        });
    }
}

// --- Debug ---

/// How far in front of the hero a summoned boss appears, in yards.
const SUMMON_AHEAD: f32 = 25.0;
/// Seconds into the run a summoned boss appears.
const SUMMON_AT: f32 = 5.0;

/// `WOW_SURVIVORS_BOSS=<name>` (debug mode only) calls that boss of the zone, from the clock's
/// bosses, the finale, the roster or the extras, in front of the hero a few seconds into the run.
/// It is no clock boss: `run.bosses` does not count it, and a finale summoned so does not win the
/// run.
#[allow(clippy::too_many_arguments)]
fn debug_summon(
    mut commands: Commands,
    mut run: ResMut<Run>,
    selection: Res<Selection>,
    nav: Res<NavGrid>,
    probe: SpawnProbe,
    world: WorldPoint,
    hero: Query<&Transform, With<Hero>>,
    mut done: Local<bool>,
) {
    if !super::debug::active() {
        return;
    }
    if run.elapsed < SUMMON_AT {
        // A new run, a new summon.
        *done = false;
        return;
    }
    if *done {
        return;
    }
    let Some(name) = benilla_app::survivors_host::webenv::var("WOW_SURVIVORS_BOSS") else {
        *done = true;
        return;
    };
    let Ok(h) = hero.single() else { return };
    let map = selection.map;
    let plan = super::finale::plan(map);
    let wanted = name.trim();
    let found = plan
        .timed
        .iter()
        .chain(std::iter::once(&plan.finale))
        .chain(data::roster(map).bosses.iter())
        .chain(data::extras(map).iter())
        .find(|d| d.name.eq_ignore_ascii_case(wanted));
    let Some(def) = found else {
        warn!("survivors: WOW_SURVIVORS_BOSS: no boss called {wanted:?} in zone {map}");
        *done = true;
        return;
    };
    let ahead = flat(h.rotation * Vec3::NEG_Z);
    let Some(at) = enemies::spawn_spot(&nav, &probe, &world, h.translation, ahead.z.atan2(ahead.x), SUMMON_AHEAD)
    else {
        return;
    };
    enemies::spawn_enemy(&mut commands, &mut run, def, at, h.translation, true, false);
    run.announce(format!("{} approaches!", def.name), 4.0);
    *done = true;
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Every map index and its name.
    fn zones() -> impl Iterator<Item = (usize, &'static str)> {
        data::MAPS.iter().enumerate().map(|(i, m)| (i, m.name))
    }

    /// The map indices where the boss called `name` fights: on the roster, as the finale, or as
    /// an extra (a script's add, a rare, the bonus boss).
    fn zones_of(name: &str) -> Vec<usize> {
        zones()
            .filter(|&(map, _)| {
                data::roster(map).bosses.iter().any(|b| b.name == name)
                    || super::super::finale::plan(map).finale.name == name
                    || data::extras(map).iter().any(|d| d.name == name)
            })
            .map(|(map, _)| map)
            .collect()
    }

    #[test]
    fn every_kit_names_a_boss_that_exists() {
        for kit in kits::KITS {
            assert!(!zones_of(kit.boss).is_empty(), "{}: no zone has this boss", kit.boss);
        }
        // And only one kit to a boss.
        for (i, a) in kits::KITS.iter().enumerate() {
            assert!(
                kits::KITS[i + 1..].iter().all(|b| b.boss != a.boss),
                "{} has two kits",
                a.boss
            );
        }
    }

    #[test]
    fn every_add_is_found_by_def_ref() {
        let mut called = 0;
        for (map, zone) in zones() {
            let extras = data::extras(map);
            for kit in kits::KITS {
                if !zones_of(kit.boss).contains(&map) {
                    continue;
                }
                for a in kit.abilities {
                    let Does::Adds { name, count, radius } = a.does else {
                        continue;
                    };
                    called += 1;
                    assert!(count > 0 && radius >= 2.0, "{zone}: {} calls nothing", kit.boss);
                    let i = extras
                        .iter()
                        .position(|d| d.name == name)
                        .unwrap_or_else(|| panic!("{zone}: {} calls {name}, not in extras", kit.boss));
                    assert_eq!(
                        enemies::def_ref(map, &extras[i]),
                        Some((5, i as u16)),
                        "{zone}: {name}"
                    );
                    assert!(
                        enemies::def_at(map, (5, i as u16)).is_some_and(|d| std::ptr::eq(d, &extras[i]))
                    );
                }
            }
        }
        assert!(called >= 8, "{called} add rows");
    }

    #[test]
    fn every_add_is_called_by_a_kit_of_its_zone() {
        for add in kits::ADDS {
            let map = zones()
                .find(|&(_, zone)| zone == add.zone)
                .unwrap_or_else(|| panic!("{}: no zone called {}", add.name, add.zone))
                .0;
            let called = kits::KITS.iter().any(|k| {
                zones_of(k.boss).contains(&map)
                    && k.abilities
                        .iter()
                        .any(|a| matches!(a.does, Does::Adds { name, .. } if name == add.name))
            });
            assert!(called, "{}: nobody calls {}", add.zone, add.name);
            assert!((0.0..=1.0).contains(&add.at));
        }
        // Each zone's adds come out in table order, with numbers from the zone's ladder.
        let elwynn = zones().find(|&(_, z)| z == "Elwynn Forest").unwrap().0;
        let adds = adds_for(elwynn);
        assert_eq!(adds.len(), 3);
        assert_eq!(adds[0].name, "Mine Spider");
        assert!(adds.iter().all(|d| d.ranged.is_none() && !d.big && d.hp > 0.0));
    }

    #[test]
    fn every_ground_ability_warns_long_enough() {
        for kit in kits::KITS {
            for a in kit.abilities {
                match a.does {
                    Does::Ground {
                        shape,
                        windup,
                        damage,
                        pool,
                    } => {
                        assert!(
                            windup >= shape.min_windup(),
                            "{}: {} warns {windup} s of {shape:?}",
                            kit.boss,
                            a.name
                        );
                        assert!(damage > 0.0, "{}: {}", kit.boss, a.name);
                        if pool.is_some() {
                            assert!(
                                matches!(shape, Shape::Circle { .. }),
                                "{}: {} leaves a pool that is no circle",
                                kit.boss,
                                a.name
                            );
                        }
                    }
                    Does::Charge { windup, speed } => {
                        let line = Shape::Line {
                            length: CHARGE_MIN,
                            width: CHARGE_WIDTH,
                        };
                        assert!(windup >= line.min_windup(), "{}: {}", kit.boss, a.name);
                        assert!(speed > 0.0);
                    }
                    _ => {}
                }
            }
        }
    }

    #[test]
    fn every_row_has_a_beat_and_a_reason() {
        for kit in kits::KITS {
            assert!(!kit.abilities.is_empty(), "{}", kit.boss);
            for a in kit.abilities {
                match a.when {
                    When::Every { first, every } => {
                        assert!(first >= 2.0 && every >= 8.0, "{}: {} is too busy", kit.boss, a.name);
                    }
                    When::Health(below) => assert!((0.05..0.95).contains(&below)),
                    When::At(secs) => assert!(secs > 0.0),
                }
                assert!(a.phases != 0, "{}: {} runs in no phase", kit.boss, a.name);
                if let Some((text, _)) = a.yell {
                    assert!(!text.is_empty());
                }
            }
        }
    }

    #[test]
    fn a_boss_with_a_script_is_scripted_and_one_without_is_not() {
        assert!(scripted("Hogger"));
        assert!(scripted("Edwin VanCleef"));
        assert!(!scripted("Lord Kazzak"));
        assert!(!scripted("Nobody"));
        assert_eq!(kit_for("Ragnaros").map(|k| k.boss), Some("Ragnaros"));
    }

    fn brain(rows: &'static [BossAbility]) -> BossBrain {
        // A kit that outlives the test, for the brain to point at.
        let kit: &'static Kit = Box::leak(Box::new(Kit {
            boss: "Test",
            abilities: rows,
            arrive: None,
            loot: &[],
        }));
        BossBrain::new(kit)
    }

    #[test]
    fn timed_rows_fire_on_their_beat() {
        static ROWS: [BossAbility; 1] = [BossAbility::new(
            "Beat",
            0,
            When::Every {
                first: 2.0,
                every: 5.0,
            },
            Aim::Boss,
            Does::Phase(0),
        )];
        let mut b = brain(&ROWS);
        let mut fired = Vec::new();
        for step in 0..200 {
            let at = (step + 1) as f32 * 0.1;
            if !b.due(0.1, 1.0).is_empty() {
                fired.push(at);
            }
        }
        // 2.0, 7.0, 12.0, 17.0 (rounding of the 0.1 steps allowed).
        assert_eq!(fired.len(), 4, "{fired:?}");
        for (got, want) in fired.iter().zip([2.0, 7.0, 12.0, 17.0]) {
            assert!((got - want).abs() < 0.25, "{fired:?}");
        }
    }

    #[test]
    fn a_health_ability_fires_once_as_health_drops() {
        static ROWS: [BossAbility; 1] = [BossAbility::new(
            "Half",
            0,
            When::Health(0.5),
            Aim::Boss,
            Does::Adds {
                name: "x",
                count: 1,
                radius: 5.0,
            },
        )];
        let mut b = brain(&ROWS);
        assert!(b.due(0.1, 1.0).is_empty());
        assert!(b.due(0.1, 0.5).is_empty(), "exactly half is not below it");
        assert_eq!(b.due(0.1, 0.49).len(), 1);
        assert!(b.due(0.1, 0.4).is_empty(), "once only");
        assert!(b.due(0.1, 0.1).is_empty());
        // A timed-once row fires at its second.
        static AT: [BossAbility; 1] = [BossAbility::new("At", 0, When::At(1.0), Aim::Boss, Does::Phase(0))];
        let mut b = brain(&AT);
        let fired = (0..30).filter(|_| !b.due(0.1, 1.0).is_empty()).count();
        assert_eq!(fired, 1);
    }

    #[test]
    fn a_phase_row_switches_the_rows_that_run() {
        const FIRST: u8 = 0b01;
        const SECOND: u8 = 0b10;
        static ROWS: [BossAbility; 3] = [
            BossAbility::new("Early", 0, When::Every { first: 1.0, every: 1.0 }, Aim::Boss, Does::Frenzy { secs: 1.0, speed: 1.1 })
                .in_phases(FIRST),
            BossAbility::new("Turn", 0, When::Health(0.5), Aim::Boss, Does::Phase(1)),
            BossAbility::new("Late", 0, When::Every { first: 1.0, every: 1.0 }, Aim::Boss, Does::Frenzy { secs: 1.0, speed: 1.2 })
                .in_phases(SECOND),
        ];
        let mut b = brain(&ROWS);
        let names = |due: Vec<&BossAbility>| due.iter().map(|a| a.name).collect::<Vec<_>>();
        assert_eq!(names(b.due(1.0, 1.0)), ["Early"]);
        // The turn fires, and the rows after it already see the new phase.
        assert_eq!(names(b.due(1.0, 0.4)), ["Early", "Turn", "Late"]);
        assert_eq!(names(b.due(1.0, 0.4)), ["Late"]);
        assert_eq!(b.phase, 1);
    }

    #[test]
    fn a_frenzy_ends_and_gives_back_its_factor() {
        static ROWS: [BossAbility; 1] = [BossAbility::new("F", 0, When::At(1.0), Aim::Boss, Does::Phase(0))];
        let mut b = brain(&ROWS);
        assert_eq!(b.frenzy_over(1.0), None, "no frenzy, nothing to give back");
        b.frenzy_mult = 1.4;
        b.frenzy_t = 2.0;
        assert_eq!(b.frenzy_over(1.0), None);
        assert_eq!(b.frenzy_over(1.5), Some(1.4));
        assert_eq!(b.frenzy_over(1.0), None);
    }

    #[test]
    fn bombs_fall_on_the_hero_and_round_it() {
        let hero = Vec3::new(3.0, 1.0, -2.0);
        let spots = bomb_spots(hero, 3, 0.4);
        assert_eq!(spots.len(), 3);
        assert_eq!(spots[0], hero);
        for p in &spots[1..] {
            assert!((flat(*p - hero).length() - BOMB_RING).abs() < 1e-4);
        }
        // Two apart, evenly: opposite sides.
        assert!((flat(spots[1] - spots[2]).length() - 2.0 * BOMB_RING).abs() < 1e-3);
        assert_eq!(bomb_spots(hero, 1, 0.0), [hero]);
        assert_eq!(bomb_spots(hero, 0, 0.0), [hero]);
    }

    #[test]
    fn a_ring_covers_the_circle_and_a_fan_fans_at_the_hero() {
        let ring = volley_dirs(8, None, Vec3::X, 0.3);
        assert_eq!(ring.len(), 8);
        let sum: Vec3 = ring.iter().sum();
        assert!(sum.length() < 1e-4, "a ring cancels out");
        assert!(ring.iter().all(|d| (d.length() - 1.0).abs() < 1e-5));
        // A fan of five across 0.9 radians, at a hero straight along +Z.
        let fan = volley_dirs(5, Some(0.9), Vec3::Z, 1.0);
        assert_eq!(fan.len(), 5);
        assert!((fan[2] - Vec3::Z).length() < 1e-5, "the middle one points at the hero");
        assert!((fan[0].angle_between(fan[4]) - 0.9).abs() < 1e-4);
        assert!(fan.iter().all(|d| d.angle_between(Vec3::Z) <= 0.45 + 1e-4));
        // One missile in a fan goes straight.
        let one = volley_dirs(1, Some(1.0), Vec3::Z, 0.0);
        assert!((one[0] - Vec3::Z).length() < 1e-5);
    }

    #[test]
    fn a_charge_reaches_past_the_hero_but_not_without_end() {
        assert_eq!(charge_length(2.0), CHARGE_MIN);
        assert_eq!(charge_length(20.0), 28.0);
        assert_eq!(charge_length(200.0), CHARGE_MAX);
    }

    /// The names of a few signature pieces are in the item table's style: a unique name and a
    /// quality that fits the boss.
    #[test]
    fn signature_loot_is_named_and_good() {
        for kit in kits::KITS {
            for tpl in kit.loot {
                assert!(!tpl.name.is_empty());
                assert!(tpl.quality >= super::super::items::Quality::Uncommon, "{}", tpl.name);
                assert_ne!(tpl.stats[0], tpl.stats[1], "{}", tpl.name);
            }
        }
        assert!(!kit_for("Hogger").unwrap().loot.is_empty());
        assert!(!kit_for("Edwin VanCleef").unwrap().loot.is_empty());
    }

    /// The scripts' systems take their parameters without clashing (Bevy checks the access as a
    /// system is initialised).
    #[test]
    fn the_boss_systems_initialise_without_conflicting_access() {
        use bevy::ecs::system::{IntoSystem, System};
        let mut world = World::new();
        IntoSystem::into_system(attach_kits).initialize(&mut world);
        IntoSystem::into_system(run_kits).initialize(&mut world);
        IntoSystem::into_system(charge).initialize(&mut world);
        IntoSystem::into_system(burn_pools).initialize(&mut world);
        IntoSystem::into_system(boss_loot).initialize(&mut world);
        IntoSystem::into_system(debug_summon).initialize(&mut world);
    }

    // --- Against the client's data ---

    #[test]
    fn every_voice_kit_is_in_sound_entries() {
        let data = benilla_formats::wow_data_or_skip!();
        let mut chain = benilla_formats::open_chain(&data).expect("open chain");
        let kits_db = benilla_formats::load_sound_kit_catalog(&mut chain).expect("SoundEntries.dbc");
        let mut bad = Vec::new();
        for kit in kits::KITS {
            let voices = kit
                .arrive
                .iter()
                .map(|(_, v)| *v)
                .chain(kit.abilities.iter().filter_map(|a| a.yell.map(|(_, v)| v)));
            for voice in voices.filter(|v| !v.is_empty()) {
                if kits_db.by_name(voice).is_none() {
                    bad.push(format!("{}: sound {voice}", kit.boss));
                }
            }
        }
        assert!(bad.is_empty(), "{bad:#?}");
    }

    #[test]
    fn every_script_spell_is_in_spell_dbc() {
        let data = benilla_formats::wow_data_or_skip!();
        let mut chain = benilla_formats::open_chain(&data).expect("open chain");
        let spells = benilla_formats::load_spell_catalog(&mut chain).expect("Spell.dbc");
        let mut bad = Vec::new();
        for kit in kits::KITS {
            for a in kit.abilities.iter().filter(|a| a.spell != 0) {
                if spells.get(a.spell).is_none() {
                    bad.push(format!("{}: {} ({}) is not in Spell.dbc", kit.boss, a.name, a.spell));
                }
            }
        }
        if spells.get(POOL_SPELL).is_none_or(|d| d.name != "Flamestrike") {
            bad.push(format!("{POOL_SPELL} is not Flamestrike"));
        }
        assert!(bad.is_empty(), "{bad:#?}");
    }

    #[test]
    fn every_boss_display_has_a_model() {
        let data = benilla_formats::wow_data_or_skip!();
        let mut chain = benilla_formats::open_chain(&data).expect("open chain");
        let creatures = benilla_formats::load_creature_catalog(&mut chain).expect("creatures");
        let mut bad = Vec::new();
        for (map, zone) in zones() {
            let finale = &super::super::finale::plan(map).finale;
            let all = data::roster(map)
                .bosses
                .iter()
                .chain(std::iter::once(finale))
                .chain(data::extras(map).iter());
            for def in all {
                if creatures.model(def.display).is_none() {
                    bad.push(format!("{zone}: {} display {} has no model", def.name, def.display));
                }
            }
        }
        assert!(bad.is_empty(), "{bad:#?}");
    }
}
