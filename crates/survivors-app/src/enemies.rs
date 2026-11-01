//! The horde: spawning on a ring outside the view, the chase, contact damage, death, and the
//! experience gems, gold and health the dead leave.

use std::collections::HashMap;

use benilla_world::rig_anim::GlobalSeqDrive;
use benilla_world::world_point::WorldPoint;
use bevy::prelude::*;

use super::abilities::Hit;
use super::chatlog::ChatLine;
use super::data::{self, EnemyDef};
use super::hero::{self, Hero, Peer, HERO_RADIUS};
use super::nav::{NavGrid, SpawnProbe};
use super::threats::{self, Stance};
use super::units::{self, flat, yaw_toward, Fx, Marks, UnitSpec, COLOR_HURT};
use super::{HeroState, Phase, Rng, Run, Selection};
use benilla_app::survivors_host::creature_anim::{move_flags, MovementState};
use benilla_app::survivors_host::net::{FieldChanged, Guid, ObjectStore};

#[derive(Component)]
pub(super) struct Enemy {
    pub def: &'static EnemyDef,
    pub hp: f32,
    pub max_hp: f32,
    pub speed: f32,
    pub damage: f32,
    pub radius: f32,
    pub xp: u32,
    pub boss: bool,
    pub attack_cd: f32,
    pub slow: f32,
    pub slow_t: f32,
    pub root_t: f32,
    pub stun_t: f32,
    pub fear_t: f32,
    pub weaken: f32,
    pub weaken_t: f32,
    pub dots: Vec<Dot>,
    /// Horizontal knock-back velocity in yards per second, set by a hit and decayed by
    /// [`move_enemies`].
    pub knock: Vec3,
    pub dead: bool,
}

/// Marks an elite: the plate and the loot know it by this.
#[derive(Component)]
pub(super) struct Elite;

/// An elite is this much larger, tougher and worth more than the grunt it was rolled from.
const ELITE_SCALE: f32 = 1.35;
const ELITE_HP: f32 = 5.0;
const ELITE_DAMAGE: f32 = 1.4;
const ELITE_XP: u32 = 6;
/// The aura an elite wears: Divine Shield's golden bubble.
pub(super) const ELITE_AURA: u32 = 642;
/// Elites alive at once.
const MAX_ELITES: usize = 8;

/// Chance a spawn is an elite, growing with the run.
fn elite_chance(elapsed: f32) -> f32 {
    0.008 + data::progress(elapsed).min(1.0) * 0.02
}

pub(super) struct Dot {
    pub dps: f32,
    pub left: f32,
    pub tick: f32,
    /// The ability whose damage this is, for the damage meter.
    pub source: Option<&'static data::AbilityDef>,
    /// The co-op guest's puppet whose curse it is; `None` is this machine's hero.
    pub by: Option<Entity>,
}

/// A corpse playing its death before it fades.
#[derive(Component)]
pub(super) struct Dying(pub(super) f32);

#[derive(Resource, Default)]
pub(super) struct Spawner {
    timer: f32,
}

/// An enemy died here, worth this much.
#[derive(Message, Clone, Copy)]
pub(super) struct Loot {
    pub pos: Vec3,
    pub xp: u32,
    pub boss: bool,
    /// The creature's level, which sizes its gold.
    pub level: u32,
    /// What died, for quests and drops keyed by creature.
    pub def: &'static data::EnemyDef,
}

#[derive(Clone, Copy)]
pub(super) enum GemKind {
    Xp(u32),
    Heal(f32),
    /// Coins worth this much gold, before the profile's greed.
    Gold(u32),
}

/// Yards per second a gem starts at when it is pulled toward the hero.
const GEM_START_SPEED: f32 = 12.0;

/// Yards per second each second a pulled gem speeds up, so a pull is quick but still visible.
const GEM_ACCEL: f32 = 60.0;

/// Seconds after a boss falls during which every gem on the field is pulled, long enough to
/// catch the boss's own drops, which spawn a frame later.
const BOSS_SWEEP_SECS: f32 = 1.5;

#[derive(Component)]
pub(super) struct Gem {
    kind: GemKind,
    /// Flying to the hero; the Hearthstone shard sets it on every gem.
    pub pulled: bool,
    speed: f32,
}

/// The `GameObjectDisplayInfo` row and scale a pickup wears: a wisp per experience tier, the
/// bigger tiers rarer, a turkey leg for health and a pile of gold that grows with its worth.
fn look_for(kind: GemKind) -> (u32, f32) {
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

/// Where `def` is listed for battleground `map`, so another machine finds the same creature:
/// `(0, i)` the roster's i-th, `(1, i)` its i-th boss, `(2, i)` the clock's i-th boss, `(3, 0)` the
/// finale, `(4, 0)` the reaper.
#[cfg_attr(not(all(feature = "coop", not(target_arch = "wasm32"))), allow(dead_code))]
pub(super) fn def_ref(map: usize, def: &'static EnemyDef) -> Option<(u8, u16)> {
    let roster = data::roster(map);
    let plan = super::finale::plan(map);
    let find = |list: &[EnemyDef]| list.iter().position(|d| std::ptr::eq(d, def)).map(|i| i as u16);
    if let Some(i) = find(&roster.enemies) {
        return Some((0, i));
    }
    if let Some(i) = find(&roster.bosses) {
        return Some((1, i));
    }
    if let Some(i) = find(&plan.timed) {
        return Some((2, i));
    }
    if std::ptr::eq(&plan.finale, def) {
        return Some((3, 0));
    }
    if std::ptr::eq(super::finale::reaper(), def) {
        return Some((4, 0));
    }
    None
}

/// The creature [`def_ref`] named.
#[cfg_attr(not(all(feature = "coop", not(target_arch = "wasm32"))), allow(dead_code))]
pub(super) fn def_at(map: usize, r: (u8, u16)) -> Option<&'static EnemyDef> {
    let roster = data::roster(map);
    let plan = super::finale::plan(map);
    let i = usize::from(r.1);
    match r.0 {
        0 => roster.enemies.get(i),
        1 => roster.bosses.get(i),
        2 => plan.timed.get(i),
        3 => Some(&plan.finale),
        4 => Some(super::finale::reaper()),
        _ => None,
    }
}

/// An enemy as a co-op guest keeps it: what it is and how it stands, for the plates, frames and
/// minimap. It never moves or fights here; the host's does.
#[cfg_attr(not(all(feature = "coop", not(target_arch = "wasm32"))), allow(dead_code))]
pub(super) fn shown_enemy(
    def: &'static EnemyDef,
    hp: f32,
    max_hp: f32,
    boss: bool,
    elite: bool,
    dead: bool,
) -> Enemy {
    Enemy {
        def,
        hp,
        max_hp,
        speed: def.speed,
        damage: def.damage,
        radius: radius_for(def) * if elite { ELITE_SCALE } else { 1.0 },
        xp: def.xp,
        boss,
        attack_cd: 0.0,
        slow: 1.0,
        slow_t: 0.0,
        root_t: 0.0,
        stun_t: 0.0,
        fear_t: 0.0,
        weaken: 1.0,
        weaken_t: 0.0,
        dots: Vec::new(),
        knock: Vec3::ZERO,
        dead,
    }
}

/// Body radius: the big beasts need more room.
fn radius_for(def: &EnemyDef) -> f32 {
    (if def.big { 1.3 } else { 0.6 }) * def.scale
}

pub(super) fn spawn_enemy(
    commands: &mut Commands,
    run: &mut Run,
    def: &'static EnemyDef,
    pos: Vec3,
    hero: Vec3,
    boss: bool,
    elite: bool,
) -> Entity {
    let scale = data::hp_scale(run.elapsed);
    let serial = run.guid();
    let size = if elite { ELITE_SCALE } else { 1.0 };
    let e = units::spawn_unit(
        commands,
        &UnitSpec {
            serial,
            display: def.display,
            scale: def.scale * size,
            level: def.level,
            faction: units::FACTION_MONSTER,
            bytes0: 1 << 8,
            weapons: &[],
            sheath: 0,
        },
        pos,
        yaw_toward(hero - pos),
    );
    let hp = def.hp
        * scale
        * run.difficulty.enemy_hp_mult()
        * party_hp(run.party)
        * if elite { ELITE_HP } else { 1.0 };
    let mut marks = Marks::debuffs();
    if elite {
        marks.set(&[ELITE_AURA]);
        commands.entity(e).insert(Elite);
    }
    commands.entity(e).insert((
        Enemy {
            def,
            hp,
            max_hp: hp,
            speed: def.speed * run.difficulty.enemy_speed_mult(),
            damage: def.damage
                * data::damage_scale(run.elapsed)
                * run.difficulty.enemy_damage_mult()
                * if elite { ELITE_DAMAGE } else { 1.0 },
            radius: radius_for(def) * size,
            xp: if elite { def.xp * ELITE_XP } else { def.xp },
            boss,
            attack_cd: 0.5,
            slow: 1.0,
            slow_t: 0.0,
            root_t: 0.0,
            stun_t: 0.0,
            fear_t: 0.0,
            weaken: 1.0,
            weaken_t: 0.0,
            dots: Vec::new(),
            knock: Vec3::ZERO,
            dead: false,
        },
        marks,
    ));
    e
}

/// The share more health each enemy has per hero past the first.
const PARTY_HP: f32 = 0.5;
/// The share more enemies alive at once per hero past the first.
const PARTY_COUNT: f32 = 0.35;
/// The most enemies alive at once, whatever the party.
const MAX_ALIVE: usize = 400;
/// A spawn never lands this close to a hero other than the one it rings.
pub(super) const SPAWN_CLEAR: f32 = 20.0;
/// The ring the horde is topped up on, from and to this many yards around a hero.
pub(super) const SPAWN_NEAR: f32 = 34.0;
pub(super) const SPAWN_FAR: f32 = 42.0;
/// The radius of the closing ring between bosses, and the distance a boss spawns at.
pub(super) const RING_RADIUS: f32 = 26.0;
pub(super) const BOSS_RADIUS: f32 = 30.0;

/// The health multiplier of a party of `n` heroes.
pub(super) fn party_hp(n: u32) -> f32 {
    1.0 + PARTY_HP * n.saturating_sub(1) as f32
}

/// How many enemies a party of `n` keeps alive where one hero has `target`.
fn party_count(target: usize, n: u32) -> usize {
    if n <= 1 {
        return target;
    }
    let scaled = (target as f32 * (1.0 + PARTY_COUNT * (n - 1) as f32)).round() as usize;
    scaled.min(MAX_ALIVE.max(target))
}

/// Where the standing heroes are: the local hero (if standing) and every standing co-op puppet.
/// The local one is first.
fn standing(
    run: &Run,
    hero: &Query<&Transform, With<Hero>>,
    peers: &Query<(&Transform, &HeroState), (With<Peer>, Without<Hero>)>,
) -> Vec<Vec3> {
    let mut at: Vec<Vec3> = Vec::new();
    if let Ok(h) = hero.single() {
        if run.dead_t.is_none() {
            at.push(h.translation);
        }
    }
    at.extend(
        peers
            .iter()
            .filter(|(_, s)| s.dead_t.is_none())
            .map(|(tf, _)| tf.translation),
    );
    at
}

/// Whether `p` stands clear of every hero but `anchor`.
fn clear_of_others(p: Vec3, anchor: Vec3, heroes: &[Vec3]) -> bool {
    heroes
        .iter()
        .all(|h| *h == anchor || flat(*h - p).length() >= SPAWN_CLEAR)
}

fn ground(world: &WorldPoint, mut p: Vec3, fallback: f32) -> Vec3 {
    p.y = world.terrain_height_under(p).unwrap_or(fallback);
    p
}

/// Angles a spawn tries around the hero before it gives up for this tick, and the step between
/// them in radians.
const SPAWN_TRIES: u32 = 12;
const SPAWN_FAN: f32 = 0.35;

/// [`ground`] on the ring `r` yards from `hero` at angle `a`, fanning out to the angles either
/// side until a spot is free by the mask, free by the world right there and joined to the hero;
/// `None` when none is. A spawn never lands inside a tree, a wall or a sealed room.
pub(super) fn spawn_spot(
    nav: &NavGrid,
    probe: &SpawnProbe,
    world: &WorldPoint,
    hero: Vec3,
    a: f32,
    r: f32,
) -> Option<Vec3> {
    (0..SPAWN_TRIES).find_map(|i| {
        let off = i.div_ceil(2) as f32 * SPAWN_FAN * if i % 2 == 1 { 1.0 } else { -1.0 };
        let (s, c) = (a + off).sin_cos();
        let p = ground(world, hero + Vec3::new(c, 0.0, s) * r, hero.y);
        (nav.fits(p) && probe.clear(p)).then_some(p)
    })
}

/// Keep the horde topped up on a ring outside the view, with the bosses and the encircling waves
/// on the clock.
pub(super) fn spawn_enemies(
    mut commands: Commands,
    time: Res<Time>,
    mut run: ResMut<Run>,
    mut rng: ResMut<Rng>,
    mut spawner: ResMut<Spawner>,
    selection: Res<Selection>,
    surge: Res<super::surge::Surge>,
    nav: Res<NavGrid>,
    probe: SpawnProbe,
    hero: Query<&Transform, With<Hero>>,
    peers: Query<(&Transform, &HeroState), (With<Peer>, Without<Hero>)>,
    enemies: Query<(&Enemy, Has<Elite>)>,
    world: WorldPoint,
    mut chat: MessageWriter<ChatLine>,
    mut turn: Local<usize>,
) {
    // The debug mode's F5 holds the waves ([`super::debug`]).
    if hero.single().is_err() || run.party_down || super::debug::waves_held() {
        return;
    }
    // Each spawn rings the next standing hero in turn; alone, always this one.
    let heroes = standing(&run, &hero, &peers);
    if heroes.is_empty() {
        return;
    }
    let mut next_anchor = || {
        *turn = turn.wrapping_add(1);
        heroes[*turn % heroes.len()]
    };
    let elapsed = run.elapsed;
    let roster = data::roster(selection.map);
    let pool = data::wave_pool(elapsed, roster.enemies.len());

    // The zone's last boss is the finale's ([`super::finale`]); the clock spawns the rest.
    let timed = &super::finale::plan(selection.map).timed;
    let due_bosses = ((elapsed / data::BOSS_EVERY_SECS) as u32).min(timed.len() as u32);
    while run.bosses < due_bosses {
        let def = &timed[run.bosses as usize];
        let hpos = next_anchor();
        let a = rng.range(0.0, std::f32::consts::TAU);
        // No free spot yet (the world is still coming in): the boss waits for the next frame.
        let Some(pos) = spawn_spot(&nav, &probe, &world, hpos, a, BOSS_RADIUS) else {
            break;
        };
        spawn_enemy(&mut commands, &mut run, def, pos, hpos, true, false);
        run.bosses += 1;
        run.announce(format!("{} approaches!", def.name), 4.0);
        chat.write(ChatLine::text(
            super::chatlog::ChatKind::Boss,
            format!("{} approaches!", def.name),
        ));
    }

    // Half way between bosses, a closing ring of the moment's weakest.
    let due_rings = ((elapsed + data::BOSS_EVERY_SECS / 2.0) / data::BOSS_EVERY_SECS) as u32;
    if run.rings < due_rings {
        run.rings = due_rings;
        // The ring is melee: a ring of shooters would pin the hero from every side.
        let def = threats::melee_of(&roster.enemies, &pool, pool[0]);
        let hpos = next_anchor();
        let n = 28 + (data::progress(elapsed) * 60.0) as usize;
        for i in 0..n {
            let a = i as f32 / n as f32 * std::f32::consts::TAU;
            let Some(pos) = spawn_spot(&nav, &probe, &world, hpos, a, RING_RADIUS)
                .filter(|p| clear_of_others(*p, hpos, &heroes))
            else {
                continue;
            };
            spawn_enemy(&mut commands, &mut run, def, pos, hpos, false, false);
        }
        run.announce("They're surrounding you!".into(), 3.0);
    }

    spawner.timer -= time.delta_secs();
    if spawner.timer > 0.0 {
        return;
    }
    // A surge ([`super::surge`]) keeps more of the horde alive, fills the gap quicker and in
    // bigger batches.
    spawner.timer = if surge.active { 0.07 } else { 0.12 };
    let alive = enemies.iter().filter(|(e, _)| !e.dead).count();
    let mut elites = enemies.iter().filter(|(e, el)| *el && !e.dead).count();
    let mut shooters = enemies
        .iter()
        .filter(|(e, _)| e.def.ranged.is_some() && !e.dead)
        .count();
    let mut target = data::target_alive(elapsed);
    if surge.active {
        target = target * 3 / 2;
    }
    let target = party_count(target, run.party);
    if alive >= target {
        return;
    }
    let batch_cap = if surge.active { 8.0 } else { 4.0 };
    let n = (((target - alive) as f32) / 6.0).ceil().min(batch_cap) as usize;
    for _ in 0..n {
        let pick = pool[rng.index(pool.len())];
        // Shooters keep their distance and only die when they are chased, so the field holds a
        // few at a time; past that, the pool's melee creature comes instead.
        let def = if shooters >= threats::max_shooters(elapsed) {
            threats::melee_of(&roster.enemies, &pool, pick)
        } else {
            &roster.enemies[pick]
        };
        let hpos = next_anchor();
        let a = rng.range(0.0, std::f32::consts::TAU);
        let r = rng.range(SPAWN_NEAR, SPAWN_FAR);
        let Some(pos) =
            spawn_spot(&nav, &probe, &world, hpos, a, r).filter(|p| clear_of_others(*p, hpos, &heroes))
        else {
            continue;
        };
        let elite =
            elites < MAX_ELITES && rng.f32() < elite_chance(elapsed) * super::items::drop_boost();
        elites += usize::from(elite);
        shooters += usize::from(def.ranged.is_some());
        spawn_enemy(&mut commands, &mut run, def, pos, hpos, false, elite);
    }
}

/// Knock-back speed in yards per second a hit gives its target: a plain hit, a crit, and the
/// shares an elite and a boss keep of it.
const KNOCK_HIT: f32 = 3.0;
const KNOCK_CRIT: f32 = 5.0;
const KNOCK_ELITE: f32 = 0.3;
/// Knock-back dies off by this rate per second, exponentially: a hit moves its target about half
/// a yard in all.
const KNOCK_DECAY: f32 = 10.0;
/// The most knock speed hits may pile up on one enemy.
pub(super) const KNOCK_MAX: f32 = 8.0;
/// Trauma the camera takes per share of max health a swing takes off the hero, and the least
/// and most one swing gives, so a nibble barely moves it and no blow shakes it to the limit.
const HURT_TRAUMA_PER_HP: f32 = 3.0;
const HURT_TRAUMA_MIN: f32 = 0.05;
const HURT_TRAUMA_MAX: f32 = 0.4;

/// The knock-back speed a landed hit gives, in yards per second; a boss takes none.
pub(super) fn knock_impulse(crit: bool, elite: bool, boss: bool) -> f32 {
    if boss {
        return 0.0;
    }
    let base = if crit { KNOCK_CRIT } else { KNOCK_HIT };
    if elite {
        base * KNOCK_ELITE
    } else {
        base
    }
}

/// `knock` after `dt` seconds of decay.
fn decay_knock(knock: Vec3, dt: f32) -> Vec3 {
    let k = knock * (-KNOCK_DECAY * dt).exp();
    if k.length_squared() < 0.01 {
        Vec3::ZERO
    } else {
        k
    }
}

/// Camera trauma for a swing that took `taken` health off a hero of `max_hp`.
pub(super) fn hurt_trauma(taken: f32, max_hp: f32) -> f32 {
    (taken / max_hp.max(1.0) * HURT_TRAUMA_PER_HP).clamp(HURT_TRAUMA_MIN, HURT_TRAUMA_MAX)
}

/// The chase: at the hero, straight in the open and round trees and buildings by the nav grid's
/// flow field, never into a blocked cell, pushed apart from each other, stopped by crowd control,
/// and swinging when in reach.
pub(super) fn move_enemies(
    time: Res<Time>,
    mut run: ResMut<Run>,
    mut shake: ResMut<super::prefs::Shake>,
    nav: Res<NavGrid>,
    probe: SpawnProbe,
    world: WorldPoint,
    hero: Query<(Entity, &Transform), (With<Hero>, Without<Enemy>)>,
    mut peers: Query<(Entity, &Transform, &mut HeroState), (With<Peer>, Without<Hero>, Without<Enemy>)>,
    mut enemies: Query<
        (Entity, &mut Transform, &mut Enemy, &mut MovementState),
        (Without<Hero>, Without<Peer>),
    >,
    mut fx: Fx,
    mut hits: MessageWriter<Hit>,
) {
    let dt = time.delta_secs();
    if dt <= 0.0 {
        return;
    }
    let Ok((hero_e, htf)) = hero.single() else {
        return;
    };
    // The heroes the horde may go for: this machine's, then every co-op puppet, standing or not.
    let mut prey: Vec<Prey> = vec![Prey {
        e: hero_e,
        pos: htf.translation,
        local: true,
        down: run.dead_t.is_some(),
        vanished: run.mobile.vanish_t > 0.0,
        picking: run.picking,
    }];
    prey.extend(peers.iter().map(|(e, tf, s)| Prey {
        e,
        pos: tf.translation,
        local: false,
        down: s.dead_t.is_some(),
        vanished: s.mobile.vanish_t > 0.0,
        picking: s.picking,
    }));
    let hero_down = prey.iter().all(|p| p.down);

    // Separation over a coarse grid: only neighbours in the same or adjacent cells push.
    const CELL: f32 = 3.0;
    let bodies: Vec<(Entity, Vec3, f32)> = enemies
        .iter()
        .filter(|(_, _, e, _)| !e.dead)
        .map(|(ent, tf, e, _)| (ent, tf.translation, e.radius))
        .collect();
    let key = |p: Vec3| ((p.x / CELL).floor() as i32, (p.z / CELL).floor() as i32);
    let mut grid: HashMap<(i32, i32), Vec<usize>> = HashMap::new();
    for (i, b) in bodies.iter().enumerate() {
        grid.entry(key(b.1)).or_default().push(i);
    }
    let mut push: HashMap<Entity, Vec3> = HashMap::with_capacity(bodies.len());
    for (i, &(ent, pos, r)) in bodies.iter().enumerate() {
        let (cx, cz) = key(pos);
        let mut acc = Vec3::ZERO;
        for dx in -1..=1 {
            for dz in -1..=1 {
                let Some(cell) = grid.get(&(cx + dx, cz + dz)) else {
                    continue;
                };
                for &j in cell {
                    if j == i {
                        continue;
                    }
                    let (_, other, or) = bodies[j];
                    let d = flat(pos - other);
                    let len = d.length();
                    let min = r + or;
                    if len < min && len > 1e-4 {
                        acc += d / len * (min - len);
                    } else if len <= 1e-4 {
                        acc += Vec3::new((i as f32).sin(), 0.0, (i as f32).cos()) * 0.1;
                    }
                }
            }
        }
        push.insert(ent, acc);
    }

    for (ent, mut tf, mut e, mut mv) in &mut enemies {
        let e = &mut *e;
        if e.dead {
            if mv.speed != 0.0 || mv.flags != 0 {
                mv.speed = 0.0;
                mv.flags = 0;
            }
            continue;
        }
        e.attack_cd -= dt;
        for t in [
            &mut e.slow_t,
            &mut e.root_t,
            &mut e.stun_t,
            &mut e.fear_t,
            &mut e.weaken_t,
        ] {
            *t = (*t - dt).max(0.0);
        }
        if e.slow_t <= 0.0 {
            e.slow = 1.0;
        }
        if e.weaken_t <= 0.0 {
            e.weaken = 1.0;
        }
        let pos = tf.translation;
        // The nearest standing hero this enemy can see; under Fade a hero is lost to all but the
        // bosses. With none, it mills where it stands.
        let quarry = nearest_prey(&prey, pos, e.boss);
        let lost = quarry.is_none() && !hero_down;
        let quarry = quarry.or_else(|| prey.iter().find(|p| p.local)).copied().unwrap_or(Prey {
            e: hero_e,
            pos: htf.translation,
            local: true,
            down: true,
            vanished: false,
            picking: false,
        });
        let hpos = quarry.pos;
        let to_hero = flat(hpos - pos);
        let dist = to_hero.length();

        // Strays far behind are brought back in front, as the reference genre does. One stuck in
        // solid space or sealed off from the hero is set down again on its own side of the ring.
        let stray = dist > 75.0;
        if stray || nav.stuck(pos) {
            let (a, r) = if stray {
                (to_hero.z.atan2(to_hero.x), 38.0)
            } else {
                ((-to_hero.z).atan2(-to_hero.x), 36.0)
            };
            if let Some(p) = spawn_spot(&nav, &probe, &world, hpos, a, r) {
                tf.translation = p;
                continue;
            }
        }

        let held = e.stun_t > 0.0 || e.root_t > 0.0 || lost;
        let contact = dist <= HERO_RADIUS + e.radius + 0.35;
        let mut step = Vec3::ZERO;
        let mut speed = 0.0;
        if !held && !hero_down {
            // A shooter walks in to its range, holds there and backs off from a hero too close.
            let stance = threats::stance(e.def.ranged.map(|r| r.range), dist);
            let flee = e.fear_t > 0.0 || stance == Stance::Back;
            let dir = if flee {
                -to_hero.normalize_or_zero()
            } else if stance == Stance::Hold {
                Vec3::ZERO
            } else {
                nav.chase_hero(pos, hpos, (!quarry.local).then_some(quarry.e))
            };
            if (!contact || flee) && stance != Stance::Hold {
                speed = e.speed * e.slow;
                step = dir * speed;
            }
            // Holding its ground or backing off, a shooter keeps its face to the hero.
            let facing = if stance != Stance::Close && e.fear_t <= 0.0 {
                to_hero
            } else {
                dir
            };
            if facing != Vec3::ZERO {
                let target = Quat::from_rotation_y(yaw_toward(facing));
                tf.rotation = tf.rotation.slerp(target, (dt * 10.0).min(1.0));
            }
        }
        let shove = push.get(&ent).copied().unwrap_or_default();
        // The walk and the shove both stop at a blocked cell, sliding along it.
        // A hit's knock-back rides on top of the walk and runs down within a moment.
        e.knock = decay_knock(e.knock, dt);
        let mut next = nav.step(
            pos,
            pos + step * dt + shove * (dt * 8.0).min(0.5) + e.knock * dt,
        );
        next = ground(&world, next, pos.y);
        tf.translation = next;
        if speed > 0.0 {
            mv.speed = speed;
            mv.flags = move_flags::FORWARD;
        } else if mv.speed != 0.0 || mv.flags != 0 {
            mv.speed = 0.0;
            mv.flags = 0;
        }

        if contact
            && !lost
            && e.stun_t <= 0.0
            && e.fear_t <= 0.0
            && e.attack_cd <= 0.0
            && !hero_down
        {
            e.attack_cd = if e.boss { 1.0 } else { 1.3 };
            let raw = e.damage * e.weaken;
            let victim = if quarry.local {
                Some(&mut run.me)
            } else {
                peers.get_mut(quarry.e).ok().map(|(_, _, s)| s.into_inner())
            };
            let Some(victim) = victim else { continue };
            let taken = hero::hurt_hero(victim, raw);
            fx.swing(ent, quarry.e, taken);
            if taken > 0.0 {
                fx.number(quarry.e, taken, false, COLOR_HURT);
                if quarry.local {
                    shake.add(hurt_trauma(taken, victim.stats().max_hp));
                }
            }
            if victim.thorns_t > 0.0 {
                hits.write(Hit {
                    by: Some(quarry.e),
                    ..Hit::direct(ent, victim.thorns * victim.stats().might, false)
                });
            }
        }
    }
}

/// A hero the horde may chase ([`move_enemies`]).
#[derive(Clone, Copy)]
struct Prey {
    e: Entity,
    pos: Vec3,
    /// This machine's hero, whose state is the run's.
    local: bool,
    down: bool,
    /// Under Fade: only bosses still see it.
    vanished: bool,
    /// Choosing cards in a co-op run: no one goes for it.
    picking: bool,
}

/// The nearest standing hero an enemy at `pos` sees, on the ground plane.
fn nearest_prey(prey: &[Prey], pos: Vec3, boss: bool) -> Option<&Prey> {
    prey.iter()
        .filter(|p| !p.down && !p.picking && (boss || !p.vanished))
        .min_by(|a, b| flat(a.pos - pos).length().total_cmp(&flat(b.pos - pos).length()))
}

/// Mark timers run down; the state kits reap with them.
pub(super) fn tick_marks(
    time: Res<Time>,
    mut units: Query<(Entity, &Guid, &mut ObjectStore, &mut Marks), With<Enemy>>,
    mut fields: MessageWriter<FieldChanged>,
) {
    let dt = time.delta_secs();
    for (entity, guid, mut store, mut marks) in &mut units {
        marks.tick(dt);
        marks.flush(entity, guid.0, &mut store, &mut fields);
    }
}

/// Corpses fade once their death has played.
pub(super) fn reap_dead(
    mut commands: Commands,
    time: Res<Time>,
    mut dying: Query<(Entity, &mut Dying)>,
) {
    for (e, mut d) in &mut dying {
        d.0 += time.delta_secs();
        if d.0 > 2.5 {
            commands
                .entity(e)
                .remove::<Dying>()
                .insert(benilla_world::model_fade::DespawnFade::default());
        }
    }
}

pub(super) fn mark_dying(commands: &mut Commands, e: Entity) {
    commands.entity(e).insert(Dying(0.0));
}

/// Most experience and health gems on the ground at once; past it, a kill's worth joins an
/// existing gem.
const MAX_GEMS: usize = 350;

/// Most coin piles on the ground at once; past it, a drop's gold joins an existing pile.
const MAX_COINS: usize = 60;

/// Chance a normal kill drops coins.
const GOLD_CHANCE: f32 = 0.12;

/// The gold a kill drops: about a quarter gold per creature level for a normal kill, so the
/// later zones pay more, and a fat pile from a boss.
fn gold_for(level: u32, boss: bool, rng: &mut Rng) -> u32 {
    let level = level.max(1) as f32;
    let gold = if boss {
        (25.0 + level * 4.0) * rng.range(0.9, 1.1)
    } else {
        level * 0.25 * rng.range(0.6, 1.4)
    };
    gold.round().max(1.0) as u32
}

pub(super) fn drop_loot(
    mut commands: Commands,
    mut drops: MessageReader<Loot>,
    mut rng: ResMut<Rng>,
    mut run: ResMut<Run>,
    mut gems: Query<&mut Gem>,
) {
    let mut coins = gems
        .iter()
        .filter(|g| matches!(g.kind, GemKind::Gold(_)))
        .count();
    let mut count = gems.iter().count() - coins;
    let health_gems = run.difficulty.health_gems();
    for d in drops.read() {
        if d.boss || rng.f32() < GOLD_CHANCE {
            let gold = gold_for(d.level, d.boss, &mut rng);
            let merged = if coins >= MAX_COINS && !d.boss {
                gems.iter_mut().find_map(|mut g| match &mut g.kind {
                    GemKind::Gold(have) => {
                        *have += gold;
                        Some(())
                    }
                    _ => None,
                })
            } else {
                None
            };
            if merged.is_none() {
                let a = rng.range(0.0, std::f32::consts::TAU);
                let at = d.pos + Vec3::new(a.cos(), 0.0, a.sin()) * 0.7;
                spawn_gem(&mut commands, run.guid(), at, GemKind::Gold(gold));
                coins += 1;
            }
        }
        // Hardcore has no health to pick up.
        if health_gems && (d.boss || rng.f32() < 0.015) {
            let heal = GemKind::Heal(if d.boss { 1000.0 } else { 30.0 });
            spawn_gem(&mut commands, run.guid(), d.pos + Vec3::X * 0.8, heal);
        }
        if count >= MAX_GEMS && !d.boss {
            if let Some(mut g) = gems.iter_mut().find(|g| matches!(g.kind, GemKind::Xp(_))) {
                if let GemKind::Xp(xp) = &mut g.kind {
                    *xp += d.xp;
                }
                continue;
            }
        }
        spawn_gem(&mut commands, run.guid(), d.pos, GemKind::Xp(d.xp));
        count += 1;
    }
}

fn spawn_gem(commands: &mut Commands, serial: u64, pos: Vec3, kind: GemKind) {
    let (display, scale) = look_for(kind);
    // Wisps and the turkey leg hover; a heap of coins lies on the ground.
    let lift = if matches!(kind, GemKind::Gold(_)) {
        0.1
    } else {
        0.5
    };
    let e = units::spawn_object(commands, serial, display, scale, pos + Vec3::Y * lift);
    commands.entity(e).insert((
        Gem {
            kind,
            pulled: false,
            speed: GEM_START_SPEED,
        },
        units::Still,
    ));
}

/// Gems in the magnet's reach fly to the hero; levels bank as they land. When a boss falls,
/// every gem on the field flies in at once.
pub(super) fn collect_gems(
    mut commands: Commands,
    time: Res<Time>,
    mut run: ResMut<Run>,
    mut rng: ResMut<Rng>,
    phase: Res<State<Phase>>,
    mut next: ResMut<NextState<Phase>>,
    hero: Query<&Transform, (With<Hero>, Without<Gem>)>,
    mut peers: Query<(Entity, &Transform, &mut HeroState), (With<Peer>, Without<Hero>, Without<Gem>)>,
    mut gems: Query<(Entity, &mut Transform, &mut Gem)>,
    fallen: Query<&Enemy, Added<Dying>>,
    mut chat: MessageWriter<ChatLine>,
    mut mail: ResMut<super::GuestMail>,
    mut sweep: Local<f32>,
) {
    let Ok(h) = hero.single() else { return };
    if run.party_down {
        return;
    }
    let dt = time.delta_secs();
    if fallen.iter().any(|e| e.boss) {
        *sweep = BOSS_SWEEP_SECS;
    }
    let sweeping = *sweep > 0.0;
    *sweep = (*sweep - dt).max(0.0);
    // Every standing hero's magnet: this machine's first (`None`), then the co-op puppets'.
    let mut magnets: Vec<(Option<Entity>, Vec3, super::Stats)> = Vec::new();
    if run.dead_t.is_none() {
        magnets.push((None, h.translation + Vec3::Y * 1.0, run.stats()));
    }
    magnets.extend(
        peers
            .iter()
            .filter(|(_, _, s)| s.dead_t.is_none())
            .map(|(e, tf, s)| (Some(e), tf.translation + Vec3::Y * 1.0, s.stats())),
    );
    for (e, mut tf, mut g) in &mut gems {
        // A gem flies to the nearest hero; it is pulled once it is in any hero's reach.
        let at = tf.translation;
        let Some(&(who, target, stats)) = magnets
            .iter()
            .min_by(|a, b| (a.1 - at).length().total_cmp(&(b.1 - at).length()))
        else {
            // The last hero standing fell this frame.
            break;
        };
        let d = target - at;
        let dist = d.length();
        if !g.pulled
            && (sweeping || magnets.iter().any(|m| flat(m.1 - at).length() <= m.2.magnet))
        {
            g.pulled = true;
        }
        if !g.pulled {
            continue;
        }
        if dist < 0.9 {
            let collector = match who {
                None => Some(&mut run.me),
                Some(p) => peers.get_mut(p).ok().map(|(_, _, s)| s.into_inner()),
            };
            match g.kind {
                // Experience is the party's, whoever picks it up.
                GemKind::Xp(xp) => {
                    let gain = xp as f32 * data::XP_RATE * stats.growth;
                    run.xp += gain;
                    chat.write(ChatLine::Xp(gain));
                }
                GemKind::Heal(hp) => {
                    if let Some(c) = collector {
                        c.hp = (c.hp + hp).min(stats.max_hp);
                    }
                }
                GemKind::Gold(gold) => {
                    if let Some(c) = collector {
                        // Greed rounds at random, so a small pile still feels a +10 %.
                        let greed = c.meta.greed + c.gear.greed + c.buffs.greed + c.talents.greed;
                        let worth = gold as f32 * greed;
                        let looted = worth.floor() as u32 + u32::from(rng.f32() < worth.fract());
                        c.gold += looted;
                        match who {
                            None => {
                                chat.write(ChatLine::Gold(looted));
                            }
                            Some(p) => mail.0.push((p, super::Mail::Gold(looted))),
                        }
                    }
                }
            }
            commands.entity(e).despawn();
            continue;
        }
        g.speed += GEM_ACCEL * dt;
        tf.translation += d / dist * (g.speed * dt).min(dist);
    }
    while run.xp >= data::xp_to_next(run.level) {
        run.xp -= data::xp_to_next(run.level);
        run.level += 1;
        run.pending_levels += 1;
        chat.write(ChatLine::system(format!(
            "You have reached level {}.",
            run.level
        )));
        if run.level == super::mobility::LEARN_LEVEL {
            if let Some(m) = super::mobility::for_class(run.class.id) {
                run.banner = Some((format!("You have learned {}! Press F.", m.name), 4.0));
                chat.write(ChatLine::system(format!(
                    "You have learned {}. Press F.",
                    m.name
                )));
            }
        }
    }
    // In a co-op run the world goes on behind the cards: a level reached meanwhile waits in the
    // same hand queue.
    if run.pending_levels > 0 && *phase.get() != Phase::LevelUp {
        next.set(Phase::LevelUp);
    }
}

/// Pickups hang still: a wisp's own loop (its circling motes and their trails) stops on its first
/// frame, clip and global sequences alike, as soon as the model attaches.
pub(super) fn still_gems(
    mut players: Query<&mut AnimationPlayer, (With<units::Still>, Added<AnimationPlayer>)>,
    mut drives: Query<&mut GlobalSeqDrive, (With<units::Still>, Added<GlobalSeqDrive>)>,
) {
    for mut player in &mut players {
        for (_, active) in player.playing_animations_mut() {
            active.set_speed(0.0);
        }
    }
    for mut drive in &mut drives {
        drive.set_paused(true);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_creature_of_a_battleground_is_found_again_by_its_ref() {
        for map in 0..data::MAPS.len() {
            let roster = data::roster(map);
            let plan = super::super::finale::plan(map);
            let all = roster
                .enemies
                .iter()
                .chain(&roster.bosses)
                .chain(&plan.timed)
                .chain(std::iter::once(&plan.finale))
                .chain(std::iter::once(super::super::finale::reaper()));
            for def in all {
                let r = def_ref(map, def).expect("listed");
                let back = def_at(map, r).expect("found");
                assert_eq!(back.name, def.name, "map {map}, ref {r:?}");
            }
        }
    }

    #[test]
    fn party_scaling_is_none_alone_and_capped() {
        assert_eq!(party_hp(1), 1.0);
        assert_eq!(party_hp(0), 1.0);
        assert!((party_hp(4) - 2.5).abs() < 1e-6);
        assert_eq!(party_count(100, 1), 100);
        assert_eq!(party_count(100, 2), 135);
        assert_eq!(party_count(350, 4), MAX_ALIVE);
        assert_eq!(party_count(500, 1), 500, "alone the count is never cut");
    }

    #[test]
    fn crits_knock_harder_than_plain_hits() {
        assert!(knock_impulse(true, false, false) > knock_impulse(false, false, false));
        assert!((3.0..=5.0).contains(&knock_impulse(false, false, false)));
        assert!((3.0..=5.0).contains(&knock_impulse(true, false, false)));
    }

    #[test]
    fn elites_barely_move_and_bosses_not_at_all() {
        let plain = knock_impulse(false, false, false);
        assert!(knock_impulse(false, true, false) < plain * 0.5);
        assert!(knock_impulse(false, true, false) > 0.0);
        assert_eq!(knock_impulse(true, true, true), 0.0);
        assert_eq!(knock_impulse(false, false, true), 0.0);
    }

    #[test]
    fn knock_decays_to_rest() {
        let k = Vec3::new(4.0, 0.0, 0.0);
        let one = decay_knock(k, 0.1);
        assert!(one.x > 0.0 && one.x < k.x * 0.5);
        let mut k = k;
        for _ in 0..60 {
            k = decay_knock(k, 1.0 / 60.0);
        }
        assert_eq!(k, Vec3::ZERO);
    }

    #[test]
    fn the_knock_covers_about_half_a_yard() {
        let mut k = Vec3::new(5.0, 0.0, 0.0);
        let mut moved = 0.0;
        for _ in 0..600 {
            moved += k.x / 600.0;
            k = decay_knock(k, 1.0 / 600.0);
        }
        // The integral of v0 * e^(-10 t) is v0 / 10.
        assert!((moved - 0.5).abs() < 0.1, "{moved}");
    }

    #[test]
    fn hurt_trauma_scales_with_the_share_of_health_and_stays_in_bounds() {
        assert_eq!(hurt_trauma(1.0, 1000.0), HURT_TRAUMA_MIN);
        assert!((hurt_trauma(50.0, 1000.0) - 0.15).abs() < 1e-5);
        assert_eq!(hurt_trauma(900.0, 1000.0), HURT_TRAUMA_MAX);
        assert_eq!(hurt_trauma(10.0, 0.0), HURT_TRAUMA_MAX);
    }
}
