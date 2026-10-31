//! The horde: spawning on a ring outside the view, the chase, contact damage, death, and the
//! experience gems, gold and health the dead leave.

use std::collections::HashMap;

use benilla_world::rig_anim::GlobalSeqDrive;
use benilla_world::world_point::WorldPoint;
use bevy::prelude::*;

use super::abilities::Hit;
use super::chatlog::ChatLine;
use super::data::{self, EnemyDef};
use super::hero::{self, Hero, HERO_RADIUS};
use super::nav::{NavGrid, SpawnProbe};
use super::units::{self, flat, yaw_toward, Fx, Marks, UnitSpec, COLOR_HURT};
use super::{Phase, Rng, Run, Selection};
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
const ELITE_AURA: u32 = 642;
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
}

/// A corpse playing its death before it fades.
#[derive(Component)]
pub(super) struct Dying(f32);

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
    let hp = def.hp * scale * run.difficulty.enemy_hp_mult() * if elite { ELITE_HP } else { 1.0 };
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
            dead: false,
        },
        marks,
    ));
    e
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
        let off = ((i + 1) / 2) as f32 * SPAWN_FAN * if i % 2 == 1 { 1.0 } else { -1.0 };
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
    nav: Res<NavGrid>,
    probe: SpawnProbe,
    hero: Query<&Transform, With<Hero>>,
    enemies: Query<(&Enemy, Has<Elite>)>,
    world: WorldPoint,
    mut chat: MessageWriter<ChatLine>,
) {
    let Ok(h) = hero.single() else { return };
    if run.dead_t.is_some() {
        return;
    }
    let hpos = h.translation;
    let elapsed = run.elapsed;
    let roster = data::roster(selection.map);
    let pool = data::wave_pool(elapsed, roster.enemies.len());

    // The zone's last boss is the finale's ([`super::finale`]); the clock spawns the rest.
    let timed = &super::finale::plan(selection.map).timed;
    let due_bosses = ((elapsed / data::BOSS_EVERY_SECS) as u32).min(timed.len() as u32);
    while run.bosses < due_bosses {
        let def = &timed[run.bosses as usize];
        let a = rng.range(0.0, std::f32::consts::TAU);
        // No free spot yet (the world is still coming in): the boss waits for the next frame.
        let Some(pos) = spawn_spot(&nav, &probe, &world, hpos, a, 30.0) else {
            break;
        };
        spawn_enemy(&mut commands, &mut run, def, pos, hpos, true, false);
        run.bosses += 1;
        run.banner = Some((format!("{} approaches!", def.name), 4.0));
        chat.write(ChatLine::text(
            super::chatlog::ChatKind::Boss,
            format!("{} approaches!", def.name),
        ));
    }

    // Half way between bosses, a closing ring of the moment's weakest.
    let due_rings = ((elapsed + data::BOSS_EVERY_SECS / 2.0) / data::BOSS_EVERY_SECS) as u32;
    if run.rings < due_rings {
        run.rings = due_rings;
        let def = &roster.enemies[pool[0]];
        let n = 28 + (data::progress(elapsed) * 60.0) as usize;
        for i in 0..n {
            let a = i as f32 / n as f32 * std::f32::consts::TAU;
            let Some(pos) = spawn_spot(&nav, &probe, &world, hpos, a, 26.0) else {
                continue;
            };
            spawn_enemy(&mut commands, &mut run, def, pos, hpos, false, false);
        }
        run.banner = Some(("They're surrounding you!".into(), 3.0));
    }

    spawner.timer -= time.delta_secs();
    if spawner.timer > 0.0 {
        return;
    }
    spawner.timer = 0.12;
    let alive = enemies.iter().filter(|(e, _)| !e.dead).count();
    let mut elites = enemies.iter().filter(|(e, el)| *el && !e.dead).count();
    let target = data::target_alive(elapsed);
    if alive >= target {
        return;
    }
    let n = (((target - alive) as f32) / 6.0).ceil().min(4.0) as usize;
    for _ in 0..n {
        let def = &roster.enemies[pool[rng.index(pool.len())]];
        let a = rng.range(0.0, std::f32::consts::TAU);
        let r = rng.range(34.0, 42.0);
        let Some(pos) = spawn_spot(&nav, &probe, &world, hpos, a, r) else {
            continue;
        };
        let elite =
            elites < MAX_ELITES && rng.f32() < elite_chance(elapsed) * super::items::drop_boost();
        elites += usize::from(elite);
        spawn_enemy(&mut commands, &mut run, def, pos, hpos, false, elite);
    }
}

/// The chase: at the hero, straight in the open and round trees and buildings by the nav grid's
/// flow field, never into a blocked cell, pushed apart from each other, stopped by crowd control,
/// and swinging when in reach.
#[allow(clippy::type_complexity)]
pub(super) fn move_enemies(
    time: Res<Time>,
    mut run: ResMut<Run>,
    nav: Res<NavGrid>,
    probe: SpawnProbe,
    world: WorldPoint,
    hero: Query<(Entity, &Transform), (With<Hero>, Without<Enemy>)>,
    mut enemies: Query<(Entity, &mut Transform, &mut Enemy, &mut MovementState), Without<Hero>>,
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
    let hpos = htf.translation;
    let hero_down = run.dead_t.is_some();

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

        // Under Fade the horde has lost the hero: it mills where it stands.
        let lost = run.mobile.vanish_t > 0.0 && !e.boss;
        let held = e.stun_t > 0.0 || e.root_t > 0.0 || lost;
        let contact = dist <= HERO_RADIUS + e.radius + 0.35;
        let mut step = Vec3::ZERO;
        let mut speed = 0.0;
        if !held && !hero_down {
            let dir = if e.fear_t > 0.0 {
                -to_hero.normalize_or_zero()
            } else {
                nav.chase(pos, hpos)
            };
            if !contact || e.fear_t > 0.0 {
                speed = e.speed * e.slow;
                step = dir * speed;
            }
            if dir != Vec3::ZERO {
                let target = Quat::from_rotation_y(yaw_toward(dir));
                tf.rotation = tf.rotation.slerp(target, (dt * 10.0).min(1.0));
            }
        }
        let shove = push.get(&ent).copied().unwrap_or_default();
        // The walk and the shove both stop at a blocked cell, sliding along it.
        let mut next = nav.step(pos, pos + step * dt + shove * (dt * 8.0).min(0.5));
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
            let taken = hero::hurt_hero(&mut run, raw);
            fx.swing(ent, hero_e, taken);
            if taken > 0.0 {
                fx.number(hero_e, taken, false, COLOR_HURT);
            }
            if run.thorns_t > 0.0 {
                hits.write(Hit::direct(ent, run.thorns * run.stats().might, false));
            }
        }
    }
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
    commands.entity(e).insert(Gem {
        kind,
        pulled: false,
        speed: 6.0,
    });
}

/// Gems in the magnet's reach fly to the hero; levels bank as they land.
pub(super) fn collect_gems(
    mut commands: Commands,
    time: Res<Time>,
    mut run: ResMut<Run>,
    mut rng: ResMut<Rng>,
    mut next: ResMut<NextState<Phase>>,
    hero: Query<&Transform, (With<Hero>, Without<Gem>)>,
    mut gems: Query<(Entity, &mut Transform, &mut Gem)>,
    mut chat: MessageWriter<ChatLine>,
) {
    let Ok(h) = hero.single() else { return };
    if run.dead_t.is_some() {
        return;
    }
    let dt = time.delta_secs();
    let stats = run.stats();
    let target = h.translation + Vec3::Y * 1.0;
    for (e, mut tf, mut g) in &mut gems {
        let d = target - tf.translation;
        let dist = d.length();
        if !g.pulled && flat(d).length() <= stats.magnet {
            g.pulled = true;
        }
        if !g.pulled {
            continue;
        }
        if dist < 0.9 {
            match g.kind {
                GemKind::Xp(xp) => {
                    let gain = xp as f32 * data::XP_RATE * stats.growth;
                    run.xp += gain;
                    chat.write(ChatLine::Xp(gain));
                }
                GemKind::Heal(hp) => run.hp = (run.hp + hp).min(stats.max_hp),
                GemKind::Gold(gold) => {
                    // Greed rounds at random, so a small pile still feels a +10 %.
                    let greed =
                        run.meta.greed + run.gear.greed + run.buffs.greed + run.talents.greed;
                    let worth = gold as f32 * greed;
                    let looted = worth.floor() as u32 + u32::from(rng.f32() < worth.fract());
                    run.gold += looted;
                    chat.write(ChatLine::Gold(looted));
                }
            }
            commands.entity(e).despawn();
            continue;
        }
        g.speed += 30.0 * dt;
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
    if run.pending_levels > 0 {
        next.set(Phase::LevelUp);
    }
}

/// Pickups hang still: a wisp's own loop (its circling motes and their trails) stops on its first
/// frame, clip and global sequences alike, as soon as the model attaches.
pub(super) fn still_gems(
    mut players: Query<&mut AnimationPlayer, (With<Gem>, Added<AnimationPlayer>)>,
    mut drives: Query<&mut GlobalSeqDrive, (With<Gem>, Added<GlobalSeqDrive>)>,
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
