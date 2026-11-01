//! Guard posts: elite camps at fixed posts around the zone, each guarding a locked chest
//! ([`super::chests::Locked`]) or a shrine (ticket #1121).
//!
//! A post is laid out once per run ([`post_offsets`]), as soon as the world stands there. Its camp
//! is an elite that walks a slow triangle round the prize and two grunts that stand, all
//! [`Steered`]: [`super::enemies::move_enemies`] leaves them be, so they stay put until a hero
//! comes within [`wake_radius`] or one of them is hurt. Then the whole camp wakes and chases with
//! the horde. When the last one falls the prize unlocks (a shrine heals, shields and pays when
//! touched), and three minutes after, once the prize is taken, the camp stands again, a little
//! stronger. The idea of posts with a leash and a wake radius is after Dean's `treasure.rs`
//! (`deanmathieson/azeroth-survivors-re`, branch `progression`, `10c6a77`, MIT OR Apache-2.0); the
//! code is ours.

use std::f32::consts::{PI, TAU};

use benilla_app::survivors_host::creature_anim::{move_flags, MovementState};
use benilla_world::schedule::WorldStage;
use benilla_world::world_point::WorldPoint;
use bevy::prelude::*;

use super::chatlog::{ChatKind, ChatLine};
use super::chests::{self, ChestTier, Locked};
use super::data::{self, EnemyDef};
use super::enemies::{self, Enemy, Steered};
use super::hero::{self, Hero, Peer};
use super::minimap::MapMarker;
use super::nav::{NavGrid, SpawnProbe};
use super::units::{self, flat, yaw_toward, Fx};
use super::{GuestMail, HeroState, Mail, Phase, Rng, Run, RunEntity, Selection};

/// Posts sit this share of the leash out from the arena's centre, at most [`POST_MAX`] yards.
const POST_SHARE: f32 = 0.7;
const POST_MAX: f32 = 100.0;
/// A hero this close to a post wakes its camp: this share of the leash, at most [`WAKE_MAX`].
const WAKE_SHARE: f32 = 0.35;
const WAKE_MAX: f32 = 14.0;
/// The banner "Guarded by ..." shows this much farther out than the camp wakes.
const BANNER_EXTRA: f32 = 10.0;
/// Seconds a cleared camp waits, after its prize is taken, before it is manned again, and the
/// distance every standing hero must keep for that.
const RETURN_SECS: f32 = 180.0;
const RETURN_CLEAR: f32 = 30.0;
/// The elite's patrol: the radius of its triangle round the post, its pace as a share of its
/// speed, and the seconds it stops at each corner.
const PATROL_RADIUS: f32 = 5.0;
const PATROL_PACE: f32 = 0.4;
const PATROL_PAUSE: f32 = 1.5;
/// How far the grunts stand from the post (the nearer spot is the fallback).
const GUARD_RADIUS: [f32; 2] = [3.5, 2.5];
/// The ranks a post's chest holds.
const PRIZE_RANKS: usize = 3;
/// `GameObjectDisplayInfo` 218, `UtherShrinePedestal.mdx`, and 1309, `AuraBlue.mdx`: the pillar of
/// light over it. Neither is sized above a yard or two, so they take the plain scale.
const SHRINE_DISPLAY: u32 = 218;
const SHRINE_AURA: u32 = 1309;
/// How near a hero must stand to take a shrine.
const TOUCH_RADIUS: f32 = 2.6;
/// A shrine's shield, as a share of the hero's health, and the seconds it lasts.
const SHRINE_SHIELD: f32 = 0.3;
const SHRINE_SHIELD_SECS: f32 = 20.0;
/// Gold a shrine pays per ten levels of the run.
const SHRINE_GOLD: u32 = 15;
/// Renew's sparkle, as a chest's flourish ([`super::chests`]).
const SPELL_BLESS: u32 = 139;
/// Seconds of a run before the posts are laid, and between two tries at a post the world is not
/// ready for.
const PLANT_AFTER: f32 = 2.0;
const RETRY_SECS: f32 = 0.4;
/// A post whose spot never fits (water, a wall) is given up after this many full searches.
const MAX_MISSES: u32 = 5;
/// Yards a post may slide to find a free spot: rings of this step, up to the count.
const SLIDE_STEP: f32 = 4.0;
const SLIDE_RINGS: usize = 4;
/// Yards a post keeps from the leash's edge.
const EDGE_MARGIN: f32 = 4.0;
/// The share of the roster's ladder a first camp's elite stands at, lowest to highest post.
const TIER_SHARE: (f32, f32) = (0.4, 0.7);

pub(super) fn plugin(app: &mut App) {
    app.init_resource::<Camps>()
        .add_systems(OnEnter(Phase::Menu), forget_camps)
        .add_systems(
            Update,
            (plant_posts, tend_posts, touch_shrines)
                .chain()
                .after(WorldStage::Input)
                .after(enemies::move_enemies)
                .run_if(super::run_on)
                .run_if(super::simulates)
                .run_if(|l: Res<benilla_app::survivors_host::loading_screen::LoadingScreen>| !l.covering()),
        )
        .add_systems(Update, debug_to_post.run_if(super::run_on));
}

/// The guard posts' systems, for the balance simulator ([`super::balance`]). The simulator has no
/// terrain or colliders for a post to stand on, so it plays without posts; a camp is optional
/// anyway, and it is not part of the horde ([`Steered`]).
#[cfg(test)]
#[allow(dead_code)]
pub(super) fn sim_systems() -> bevy::ecs::schedule::ScheduleConfigs<bevy::ecs::system::ScheduleSystem> {
    idle.into_configs()
}

#[cfg(test)]
fn idle() {}

/// Where a zone's posts stand relative to the arena's centre: two on a small leash, three on a
/// middling one, four on a large one, evenly round at [`POST_SHARE`] of the leash (at most
/// [`POST_MAX`]), turned by `phase` radians.
pub(super) fn post_offsets(leash: f32, phase: f32) -> Vec<Vec3> {
    let n = if leash < 60.0 {
        2
    } else if leash < 120.0 {
        3
    } else {
        4
    };
    let radius = (leash * POST_SHARE).min(POST_MAX);
    (0..n)
        .map(|i| {
            let a = phase + i as f32 / n as f32 * TAU;
            Vec3::new(a.cos(), 0.0, a.sin()) * radius
        })
        .collect()
}

/// How close a standing hero must come to wake a camp.
pub(super) fn wake_radius(leash: f32) -> f32 {
    (leash * WAKE_SHARE).min(WAKE_MAX)
}

/// What a post guards.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Prize {
    Chest,
    Shrine,
}

/// The prize of the `index`-th post: about one in three is a shrine, the rest chests, so a zone
/// has both (the second post is always the shrine).
fn prize_for(index: usize) -> Prize {
    if index % 3 == 1 {
        Prize::Shrine
    } else {
        Prize::Chest
    }
}

/// Where a camp is in its cycle.
#[derive(Clone, Copy, PartialEq, Debug)]
enum Watch {
    /// Standing at its post, hunting no one.
    Asleep,
    /// Woken: on the chase with the horde.
    Awake,
    /// Wiped out at this second of the run.
    Cleared(f32),
}

/// A guard post: where it stands, who holds it and what it guards.
#[derive(Component)]
struct Post {
    /// The post's ground point, the camp's centre.
    at: Vec3,
    /// The turn of the patrol's corners and the grunts' places, radians.
    turn: f32,
    prize: Prize,
    members: Vec<Entity>,
    /// The chest or the shrine, while it exists.
    reward: Option<Entity>,
    watch: Watch,
    /// "Guarded by ..." was shown for this camp.
    told: bool,
    /// The elite's name, for the lines about the camp.
    elite: &'static str,
}

/// A camp member ([`Post::members`]).
#[derive(Component)]
struct Guard;

/// The elite's walk round the post: the corner it goes to and the seconds it still stands.
#[derive(Component)]
struct Patrol {
    leg: usize,
    pause: f32,
    home: Vec3,
    turn: f32,
}

/// A shrine and the models that stand with it, taken down with it.
#[derive(Component)]
struct Shrine {
    props: Vec<Entity>,
}

/// The `leg`-th corner of the patrol triangle round `home`.
fn patrol_point(home: Vec3, turn: f32, leg: usize) -> Vec3 {
    let a = turn + leg as f32 / 3.0 * TAU;
    home + Vec3::new(a.cos(), 0.0, a.sin()) * PATROL_RADIUS
}

/// The patrol's move for this tick: `None` while it stands at a corner, else the direction to the
/// next one and the distance left. Reaching a corner starts the pause and picks the next.
fn patrol_dir(p: &mut Patrol, pos: Vec3, dt: f32) -> Option<(Vec3, f32)> {
    if p.pause > 0.0 {
        p.pause -= dt;
        return None;
    }
    let to = flat(patrol_point(p.home, p.turn, p.leg) - pos);
    let d = to.length();
    if d <= 0.3 {
        p.leg = (p.leg + 1) % 3;
        p.pause = PATROL_PAUSE;
        return None;
    }
    Some((to / d, d))
}

/// How a camp member stands.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Member {
    Whole,
    Hurt,
    Down,
}

fn judge(hp: f32, max_hp: f32, dead: bool) -> Member {
    if dead || hp <= 0.0 {
        Member::Down
    } else if hp < max_hp - 0.001 {
        Member::Hurt
    } else {
        Member::Whole
    }
}

/// Whether a sleeping camp wakes: a standing hero (`heroes` are their distances from the post)
/// within `wake` yards, or any member hurt or down. One woken member wakes all.
fn wakes(heroes: &[f32], wake: f32, members: &[Member]) -> bool {
    heroes.iter().any(|d| *d <= wake) || members.iter().any(|m| *m != Member::Whole)
}

/// Whether every member has fallen.
fn fallen(members: &[Member]) -> bool {
    !members.is_empty() && members.iter().all(|m| *m == Member::Down)
}

/// The roster index a first camp's elite takes: the `index`-th of `count` posts stands from
/// [`TIER_SHARE`]`.0` to `.1` of the way up the ladder of `len` creatures.
fn camp_tier(len: usize, index: usize, count: usize) -> usize {
    let t = if count > 1 {
        index as f32 / (count - 1) as f32
    } else {
        0.0
    };
    let share = TIER_SHARE.0 + (TIER_SHARE.1 - TIER_SHARE.0) * t;
    ((len as f32 * share) as usize).min(len.saturating_sub(1))
}

/// The roster index a camp that returns takes: two above the strongest of the horde's pool.
fn return_tier(len: usize, elapsed: f32) -> usize {
    (data::wave_pool(elapsed, len)[2] + 2).min(len.saturating_sub(1))
}

/// The melee creature at or nearest `i`: a camp never has a shooter, which would fire on a
/// sleeping camp's hero from range, wake or not.
fn melee_tier(roster: &[EnemyDef], i: usize) -> usize {
    (0..=i)
        .rev()
        .chain(i + 1..roster.len())
        .find(|&k| roster[k].ranged.is_none())
        .unwrap_or(i)
}

/// The compass word for where `off` (a Bevy offset: north is -z, east +x) lies.
fn compass(off: Vec3) -> &'static str {
    const NAMES: [&str; 8] = [
        "north",
        "north-east",
        "east",
        "south-east",
        "south",
        "south-west",
        "west",
        "north-west",
    ];
    let clockwise = off.x.atan2(-off.z);
    let i = (clockwise / (PI / 4.0)).round() as i32;
    NAMES[i.rem_euclid(8) as usize]
}

/// The gold a shrine pays a run at `level`: [`SHRINE_GOLD`] for each ten levels begun.
fn shrine_gold(level: u32) -> u32 {
    SHRINE_GOLD * level.div_ceil(10).max(1)
}

/// What the world has at a spot, for a post.
#[derive(Clone, Copy, PartialEq, Debug)]
enum Spot {
    /// No terrain there yet.
    Unseen,
    /// Terrain, but a wall, a tree or a sealed room is in the way.
    Blocked,
    /// Free ground at this point, set on the terrain.
    Free(Vec3),
}

/// [`Spot`] for the point `p`: the terrain under it, then the nav mask and the world's colliders.
fn ground_spot(world: &WorldPoint, nav: &NavGrid, probe: &SpawnProbe, p: Vec3) -> Spot {
    let Some(y) = world.terrain_height_under(p) else {
        return Spot::Unseen;
    };
    let p = Vec3::new(p.x, y, p.z);
    if nav.fits(p) && probe.clear(p) {
        Spot::Free(p)
    } else {
        Spot::Blocked
    }
}

/// What [`find_site`] came to.
#[derive(Clone, Copy, PartialEq, Debug)]
enum Found {
    At(Vec3),
    /// The world is not in at the wanted spot yet: try again.
    Wait,
    /// The world is in and nothing near fits.
    Miss,
}

/// The spot for a post wanted at `want`: free ground with the patrol's three corners free as well,
/// found there or by sliding out in rings, never past the leash's edge nor so near the arena's
/// centre that the hero starts inside the wake radius.
fn find_site(
    want: Vec3,
    turn: f32,
    arena: Vec3,
    leash: f32,
    spot: &dyn Fn(Vec3) -> Spot,
) -> Found {
    let fits = |c: Vec3| -> Spot {
        match spot(c) {
            Spot::Free(g) => {
                for leg in 0..3 {
                    match spot(patrol_point(g, turn, leg)) {
                        Spot::Free(_) => {}
                        other => return other,
                    }
                }
                Spot::Free(g)
            }
            other => other,
        }
    };
    match fits(want) {
        Spot::Free(g) => return Found::At(g),
        Spot::Unseen => return Found::Wait,
        Spot::Blocked => {}
    }
    let (near, far) = (wake_radius(leash) + 6.0, leash - EDGE_MARGIN);
    for ring in 1..=SLIDE_RINGS {
        for k in 0..8 {
            let a = k as f32 / 8.0 * TAU + ring as f32;
            let c = want + Vec3::new(a.cos(), 0.0, a.sin()) * (ring as f32 * SLIDE_STEP);
            let d = flat(c - arena).length();
            if d > far || d < near {
                continue;
            }
            if let Spot::Free(g) = fits(c) {
                return Found::At(g);
            }
        }
    }
    Found::Miss
}

/// A camp as it was raised.
struct Raised {
    members: Vec<Entity>,
    reward: Entity,
}

/// Raise a camp at `at`: the elite `def` at a corner of its patrol, two of its grunts standing
/// where `spot` lets them, and the prize at the centre. Everything but the prize is [`Steered`]
/// and a [`Guard`]; the prize is [`Locked`] until the camp falls.
fn raise(
    commands: &mut Commands,
    run: &mut Run,
    at: Vec3,
    turn: f32,
    prize: Prize,
    def: &'static EnemyDef,
    spot: &dyn Fn(Vec3) -> Spot,
) -> Raised {
    let mut members = Vec::with_capacity(3);
    let corner = match spot(patrol_point(at, turn, 0)) {
        Spot::Free(p) => p,
        _ => at,
    };
    let elite = enemies::spawn_enemy(commands, run, def, corner, at, false, true);
    commands.entity(elite).insert((
        Steered,
        Guard,
        Patrol {
            leg: 1,
            pause: 0.0,
            home: at,
            turn,
        },
    ));
    members.push(elite);
    for side in [PI / 3.0, PI] {
        let a = turn + side;
        let dir = Vec3::new(a.cos(), 0.0, a.sin());
        let Some(p) = GUARD_RADIUS.iter().find_map(|r| match spot(at + dir * *r) {
            Spot::Free(p) => Some(p),
            _ => None,
        }) else {
            continue;
        };
        // Facing out from the post, as a guard watches the way in.
        let grunt = enemies::spawn_enemy(commands, run, def, p, p + dir, false, false);
        commands.entity(grunt).insert((Steered, Guard));
        members.push(grunt);
    }
    let reward = match prize {
        Prize::Chest => {
            let e = chests::spawn_chest(commands, run, at, PRIZE_RANKS, ChestTier::Post);
            commands.entity(e).insert((Locked, MapMarker::Chest));
            e
        }
        Prize::Shrine => {
            let e = units::spawn_object(commands, run.guid(), SHRINE_DISPLAY, 1.0, at);
            let pillar = units::spawn_object(commands, run.guid(), SHRINE_AURA, 1.0, at);
            commands.entity(e).insert((
                Locked,
                Shrine {
                    props: vec![pillar],
                },
                MapMarker::Shrine,
            ));
            e
        }
    };
    Raised { members, reward }
}

/// The posts still to be set down in this run.
#[derive(Resource, Default)]
struct Camps {
    /// The posts have been laid out ([`post_offsets`]).
    laid: bool,
    /// Seconds into the run.
    age: f32,
    /// Seconds until the next try at a site.
    retry: f32,
    sites: Vec<Site>,
}

/// A post not set down yet.
struct Site {
    want: Vec3,
    index: usize,
    count: usize,
    turn: f32,
    misses: u32,
}

/// A new run starts with no posts.
fn forget_camps(mut camps: ResMut<Camps>) {
    *camps = Camps::default();
}

/// Lay the posts out at the run's start, and set each down as soon as the world stands there:
/// one try every [`RETRY_SECS`], so a slow stream costs a few probes, not a stall.
#[allow(clippy::too_many_arguments)]
fn plant_posts(
    mut commands: Commands,
    time: Res<Time>,
    mut run: ResMut<Run>,
    mut rng: ResMut<Rng>,
    mut camps: ResMut<Camps>,
    selection: Res<Selection>,
    nav: Res<NavGrid>,
    probe: SpawnProbe,
    world: WorldPoint,
    hero: Query<(), With<Hero>>,
) {
    // A route ([`super::route`]) has no arena for the posts to stand round.
    if hero.is_empty() || super::route::route_of(selection.map).is_some() {
        return;
    }
    let map = selection.map_def();
    let arena = hero::arena(map);
    if !camps.laid {
        camps.age += time.delta_secs();
        if camps.age < PLANT_AFTER {
            return;
        }
        camps.laid = true;
        let offsets = post_offsets(map.leash, rng.range(0.0, TAU));
        let count = offsets.len();
        camps.sites = offsets
            .into_iter()
            .enumerate()
            .map(|(index, o)| Site {
                want: arena + o,
                index,
                count,
                turn: rng.range(0.0, TAU),
                misses: 0,
            })
            .collect();
    }
    if camps.sites.is_empty() {
        return;
    }
    camps.retry -= time.delta_secs();
    if camps.retry > 0.0 {
        return;
    }
    camps.retry = RETRY_SECS;
    let spot = |p: Vec3| ground_spot(&world, &nav, &probe, p);
    let (want, turn, index, count) = {
        let s = &camps.sites[0];
        (s.want, s.turn, s.index, s.count)
    };
    match find_site(want, turn, arena, map.leash, &spot) {
        Found::Wait => {}
        Found::Miss => {
            camps.sites[0].misses += 1;
            if camps.sites[0].misses >= MAX_MISSES {
                camps.sites.remove(0);
            }
        }
        Found::At(at) => {
            camps.sites.remove(0);
            let roster = &data::roster(selection.map).enemies;
            let def = &roster[melee_tier(roster, camp_tier(roster.len(), index, count))];
            let prize = prize_for(index);
            let raised = raise(&mut commands, &mut run, at, turn, prize, def, &spot);
            commands.spawn((
                Post {
                    at,
                    turn,
                    prize,
                    members: raised.members,
                    reward: Some(raised.reward),
                    watch: Watch::Asleep,
                    told: false,
                    elite: def.name,
                },
                MapMarker::Post,
                Transform::from_translation(at),
                RunEntity,
            ));
        }
    }
}

/// Every post's camp: the elite's patrol, the waking, the fall, the prize's unlocking and the
/// camp's return.
#[allow(clippy::too_many_arguments)]
fn tend_posts(
    mut commands: Commands,
    time: Res<Time>,
    mut run: ResMut<Run>,
    selection: Res<Selection>,
    nav: Res<NavGrid>,
    probe: SpawnProbe,
    world: WorldPoint,
    hero: Query<&Transform, (With<Hero>, Without<Guard>)>,
    peers: Query<(&Transform, &HeroState), (With<Peer>, Without<Hero>, Without<Guard>)>,
    mut posts: Query<(Entity, &mut Post)>,
    mut guards: Query<
        (&mut Transform, &Enemy, &mut MovementState, Option<&mut Patrol>),
        (With<Guard>, Without<Hero>, Without<Peer>),
    >,
    alive: Query<(), With<RunEntity>>,
    mut chat: MessageWriter<ChatLine>,
) {
    let dt = time.delta_secs();
    let map = selection.map_def();
    let arena = hero::arena(map);
    let wake = wake_radius(map.leash);
    // This machine's hero, and every standing hero with the co-op puppets.
    let local = hero
        .single()
        .ok()
        .filter(|_| run.dead_t.is_none())
        .map(|h| h.translation);
    let mut heroes: Vec<Vec3> = local.into_iter().collect();
    heroes.extend(
        peers
            .iter()
            .filter(|(_, s)| s.dead_t.is_none())
            .map(|(tf, _)| tf.translation),
    );
    let spot = |p: Vec3| ground_spot(&world, &nav, &probe, p);
    for (post_e, mut post) in &mut posts {
        let centre = post.at;
        let away = |h: &Vec3| flat(*h - centre).length();
        if let Watch::Cleared(since) = post.watch {
            let held = post.reward.is_some_and(|r| alive.contains(r));
            if run.elapsed - since < RETURN_SECS || held || heroes.iter().any(|h| away(h) < RETURN_CLEAR) {
                continue;
            }
            let roster = &data::roster(selection.map).enemies;
            let def = &roster[melee_tier(roster, return_tier(roster.len(), run.elapsed))];
            let (at, turn, prize) = (post.at, post.turn, post.prize);
            let raised = raise(&mut commands, &mut run, at, turn, prize, def, &spot);
            post.members = raised.members;
            post.reward = Some(raised.reward);
            post.watch = Watch::Asleep;
            post.told = false;
            post.elite = def.name;
            commands.entity(post_e).insert(MapMarker::Post);
            chat.write(ChatLine::text(
                ChatKind::Boss,
                format!("Guards have taken the post to the {} again.", compass(at - arena)),
            ));
            continue;
        }
        let members: Vec<Member> = post
            .members
            .iter()
            .map(|m| match guards.get(*m) {
                Ok((_, e, _, _)) => judge(e.hp, e.max_hp, e.dead),
                Err(_) => Member::Down,
            })
            .collect();
        if fallen(&members) {
            post.watch = Watch::Cleared(run.elapsed);
            if let Some(r) = post.reward.filter(|r| alive.contains(*r)) {
                commands.entity(r).remove::<Locked>();
            }
            commands.entity(post_e).remove::<MapMarker>();
            let what = match post.prize {
                Prize::Chest => "cache",
                Prize::Shrine => "shrine",
            };
            chat.write(ChatLine::system(format!(
                "The {what} to the {} is unguarded.",
                compass(post.at - arena)
            )));
            continue;
        }
        if post.watch != Watch::Asleep {
            continue;
        }
        let near: Vec<f32> = heroes.iter().map(away).collect();
        if wakes(&near, wake, &members) {
            for &m in &post.members {
                if guards.contains(m) {
                    commands.entity(m).remove::<(Steered, Patrol)>();
                }
            }
            post.watch = Watch::Awake;
            chat.write(ChatLine::text(
                ChatKind::Boss,
                format!("{} and its guards attack!", post.elite),
            ));
            continue;
        }
        for &m in &post.members {
            let Ok((mut tf, e, mut mv, patrol)) = guards.get_mut(m) else {
                continue;
            };
            let Some(mut patrol) = patrol else { continue };
            let walk = if e.stun_t > 0.0 || e.root_t > 0.0 {
                None
            } else {
                patrol_dir(&mut patrol, tf.translation, dt)
            };
            let Some((dir, left)) = walk else {
                if mv.speed != 0.0 || mv.flags != 0 {
                    mv.speed = 0.0;
                    mv.flags = 0;
                }
                continue;
            };
            let pace = e.speed * e.slow * PATROL_PACE;
            let step = (pace * dt).min(left);
            let from = tf.translation;
            let mut next = nav.step(from, from + dir * step);
            next.y = world.terrain_height_under(next).unwrap_or(from.y);
            // Boxed in by a wall: give up this corner for the next.
            if flat(next - from).length() < step * 0.25 {
                patrol.leg = (patrol.leg + 1) % 3;
                patrol.pause = 0.5;
            }
            tf.translation = next;
            tf.rotation = tf
                .rotation
                .slerp(Quat::from_rotation_y(yaw_toward(dir)), (dt * 8.0).min(1.0));
            mv.speed = pace;
            mv.flags = move_flags::FORWARD;
        }
        if !post.told && local.is_some_and(|h| away(&h) <= wake + BANNER_EXTRA) {
            post.told = true;
            run.banner = Some((format!("Guarded by {}", post.elite), 2.5));
        }
    }
}

/// The restoring a shrine does a hero: full health, a shield of [`SHRINE_SHIELD`] of it for
/// [`SHRINE_SHIELD_SECS`] and the gold; returns the gold.
fn bless(h: &mut HeroState, level: u32) -> u32 {
    let max = h.stats().max_hp;
    h.hp = max;
    h.shield = h.shield.max(max * SHRINE_SHIELD);
    h.shield_t = h.shield_t.max(SHRINE_SHIELD_SECS);
    let gold = shrine_gold(level);
    h.gold += gold;
    gold
}

/// Walking up to an unlocked shrine takes it, for whichever standing hero reaches it first, a
/// co-op guest's too.
#[allow(clippy::too_many_arguments)]
fn touch_shrines(
    mut commands: Commands,
    mut run: ResMut<Run>,
    hero: Query<(Entity, &Transform), (With<Hero>, Without<Shrine>)>,
    mut peers: Query<
        (Entity, &Transform, &mut HeroState),
        (With<Peer>, Without<Hero>, Without<Shrine>),
    >,
    shrines: Query<(Entity, &Transform, &Shrine), (Without<Locked>, Without<Hero>, Without<Peer>)>,
    mut fx: Fx,
    mut chat: MessageWriter<ChatLine>,
    mut mail: ResMut<GuestMail>,
) {
    let Ok((hero_e, htf)) = hero.single() else {
        return;
    };
    let level = run.level;
    for (e, tf, shrine) in &shrines {
        let near = |at: Vec3| flat(tf.translation - at).length() <= TOUCH_RADIUS;
        let by = if run.dead_t.is_none() && near(htf.translation) {
            let gold = bless(&mut run.me, level);
            chat.write(ChatLine::Gold(gold));
            chat.write(ChatLine::system("The shrine restores you and wards you for a while."));
            run.banner = Some(("Shrine: restored and shielded".into(), 3.0));
            Some(hero_e)
        } else if let Some((p, _, mut state)) = peers
            .iter_mut()
            .find(|(_, ptf, s)| s.dead_t.is_none() && near(ptf.translation))
        {
            let gold = bless(&mut state, level);
            mail.0.push((p, Mail::Gold(gold)));
            mail.0.push((p, Mail::Banner("Shrine: restored and shielded".into())));
            mail.0.push((
                p,
                Mail::Line("The shrine restores you and wards you for a while.".into()),
            ));
            Some(p)
        } else {
            None
        };
        if let Some(by) = by {
            fx.impact(by, SPELL_BLESS);
            commands.entity(e).despawn();
            for p in &shrine.props {
                commands.entity(*p).try_despawn();
            }
        }
    }
}

/// Debug mode's F4: stand 18 yards from the nearest post, on the side of the arena's centre. (F10
/// and F11 step the picture there.)
fn debug_to_post(
    keys: Res<ButtonInput<KeyCode>>,
    selection: Res<Selection>,
    posts: Query<&Post>,
    mut hero: Query<&mut Transform, With<Hero>>,
) {
    if !keys.just_pressed(KeyCode::F4) || !super::debug::active() {
        return;
    }
    let Ok(mut tf) = hero.single_mut() else { return };
    let arena = hero::arena(selection.map_def());
    let here = tf.translation;
    let Some(post) = posts.iter().min_by(|a, b| {
        flat(a.at - here)
            .length()
            .total_cmp(&flat(b.at - here).length())
    }) else {
        return;
    };
    let inward = flat(arena - post.at).normalize_or_zero();
    let to = post.at + inward * 18.0;
    // `drive_hero` sets the height from the ground under it.
    tf.translation.x = to.x;
    tf.translation.z = to.z;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn posts_stay_inside_every_leash() {
        for map in data::MAPS {
            let posts = post_offsets(map.leash, 0.3);
            assert!(!posts.is_empty());
            for p in &posts {
                assert!(p.length() <= map.leash - 4.0, "{}", map.name);
                assert!(
                    p.length() > wake_radius(map.leash) + 6.0,
                    "{}: a post wakes at the start",
                    map.name
                );
            }
        }
    }

    #[test]
    fn posts_keep_apart() {
        for map in data::MAPS {
            let wake = wake_radius(map.leash);
            for phase in [0.0, 0.3, 2.0] {
                let posts = post_offsets(map.leash, phase);
                for (i, a) in posts.iter().enumerate() {
                    for b in &posts[i + 1..] {
                        assert!(
                            (*a - *b).length() >= 2.0 * (wake + 3.0),
                            "{}: two camps wake together",
                            map.name
                        );
                    }
                }
            }
        }
    }

    #[test]
    fn every_zone_has_chests_and_a_shrine() {
        for map in data::MAPS {
            let n = post_offsets(map.leash, 0.0).len();
            let prizes: Vec<Prize> = (0..n).map(prize_for).collect();
            assert!(prizes.contains(&Prize::Chest), "{}", map.name);
            assert!(prizes.contains(&Prize::Shrine), "{}", map.name);
        }
    }

    #[test]
    fn a_camp_wakes_together() {
        let whole = [Member::Whole; 3];
        // Nobody near and nobody hurt: it sleeps.
        assert!(!wakes(&[40.0, 90.0], 14.0, &whole));
        assert!(!wakes(&[], 14.0, &whole));
        // A hero at the wake radius wakes it.
        assert!(wakes(&[90.0, 14.0], 14.0, &whole));
        // One hurt member wakes the camp, however far the heroes; so does one that fell outright.
        assert!(wakes(&[90.0], 14.0, &[Member::Whole, Member::Hurt, Member::Whole]));
        assert!(wakes(&[90.0], 14.0, &[Member::Whole, Member::Down, Member::Whole]));
    }

    #[test]
    fn a_member_is_hurt_by_any_lost_health() {
        assert_eq!(judge(100.0, 100.0, false), Member::Whole);
        assert_eq!(judge(99.0, 100.0, false), Member::Hurt);
        assert_eq!(judge(0.0, 100.0, false), Member::Down);
        assert_eq!(judge(50.0, 100.0, true), Member::Down);
    }

    #[test]
    fn a_camp_falls_when_the_last_one_does() {
        assert!(!fallen(&[]));
        assert!(!fallen(&[Member::Down, Member::Hurt]));
        assert!(fallen(&[Member::Down, Member::Down, Member::Down]));
    }

    #[test]
    fn camps_never_field_a_shooter() {
        for map in 0..data::MAPS.len() {
            let roster = &data::roster(map).enemies;
            for index in 0..4 {
                let t = melee_tier(roster, camp_tier(roster.len(), index, 4));
                assert!(roster[t].ranged.is_none(), "map {map}, post {index}");
            }
            for secs in [0.0, 300.0, 600.0, 899.0] {
                let t = melee_tier(roster, return_tier(roster.len(), secs));
                assert!(roster[t].ranged.is_none(), "map {map}, {secs}s");
            }
        }
    }

    #[test]
    fn camps_stand_up_the_ladder_and_return_with_the_clock() {
        for map in 0..data::MAPS.len() {
            let len = data::roster(map).enemies.len();
            let tiers: Vec<usize> = (0..4).map(|i| camp_tier(len, i, 4)).collect();
            assert!(tiers.windows(2).all(|w| w[0] <= w[1]), "{tiers:?}");
            assert!(tiers[0] as f32 >= len as f32 * 0.39 - 1.0);
            assert!(*tiers.last().unwrap() as f32 <= len as f32 * 0.7);
            let early = return_tier(len, 0.0);
            let late = return_tier(len, 800.0);
            assert!(late >= early && late < len, "{early} {late} of {len}");
        }
    }

    #[test]
    fn the_patrol_walks_its_triangle_and_stays_near_the_post() {
        let home = Vec3::new(10.0, 0.0, -4.0);
        let mut p = Patrol {
            leg: 1,
            pause: 0.0,
            home,
            turn: 0.7,
        };
        let mut pos = patrol_point(home, 0.7, 0);
        let mut seen = [false; 3];
        let dt = 0.05;
        for _ in 0..(90.0 / dt) as usize {
            if let Some((dir, left)) = patrol_dir(&mut p, pos, dt) {
                pos += dir * (2.0 * dt).min(left);
            }
            seen[p.leg] = true;
            assert!(flat(pos - home).length() <= PATROL_RADIUS + 0.1);
        }
        assert_eq!(seen, [true; 3], "every corner is visited");
    }

    #[test]
    fn the_compass_names_the_way() {
        assert_eq!(compass(Vec3::new(0.0, 0.0, -10.0)), "north");
        assert_eq!(compass(Vec3::new(10.0, 0.0, -10.0)), "north-east");
        assert_eq!(compass(Vec3::new(10.0, 0.0, 0.0)), "east");
        assert_eq!(compass(Vec3::new(0.0, 0.0, 10.0)), "south");
        assert_eq!(compass(Vec3::new(-10.0, 0.0, 0.0)), "west");
        assert_eq!(compass(Vec3::new(-10.0, 0.0, -10.0)), "north-west");
    }

    #[test]
    fn a_shrine_pays_per_ten_levels() {
        assert_eq!(shrine_gold(1), 15);
        assert_eq!(shrine_gold(10), 15);
        assert_eq!(shrine_gold(11), 30);
        assert_eq!(shrine_gold(35), 60);
    }

    #[test]
    fn a_site_slides_off_blocked_ground_and_waits_for_the_world() {
        let arena = Vec3::ZERO;
        let want = Vec3::new(60.0, 0.0, 0.0);
        let open = |_: Vec3| -> Spot { Spot::Unseen };
        assert_eq!(find_site(want, 0.0, arena, 150.0, &open), Found::Wait);
        let shut = |_: Vec3| -> Spot { Spot::Blocked };
        assert_eq!(find_site(want, 0.0, arena, 150.0, &shut), Found::Miss);
        // A wall in the way of the wanted spot only: the site slides a ring out.
        let walled = |p: Vec3| -> Spot {
            if flat(p - want).length() < 3.0 {
                Spot::Blocked
            } else {
                Spot::Free(p)
            }
        };
        match find_site(want, 0.0, arena, 150.0, &walled) {
            Found::At(p) => assert!(flat(p - want).length() >= 3.0),
            other => panic!("{other:?}"),
        }
    }
}
