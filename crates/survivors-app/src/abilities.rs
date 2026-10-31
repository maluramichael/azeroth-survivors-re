//! Auto-cast: every owned ability fires on its own cooldown at the targets its kind picks, with
//! the spell's own visuals; damage lands when its missile would.

use std::cmp::Ordering;

use bevy::prelude::*;

use super::data::{AbilityDef, Effect, Kind};
use super::dmgmeter::DamageDone;
use super::enemies::{self, Enemy, Loot};
use super::focus::{Focus, Target};
use super::hero::{self, Hero, Peer};
use super::units::{self, flat, Fx, Marks, UnitSpec, COLOR_HEAL, COLOR_MELEE, COLOR_SPELL};
use super::{HeroState, Rng, Run, RunEntity};
use benilla_app::survivors_host::net::{FieldChanged, Guid, ObjectStore};

/// One blow landing on one enemy.
#[derive(Message, Clone)]
pub(super) struct Hit {
    pub target: Entity,
    pub damage: f32,
    pub dot: f32,
    pub dot_dur: f32,
    pub effect: Effect,
    pub mark: u32,
    pub mark_dur: f32,
    /// Also hits everything this close to the target, for less.
    pub splash: f32,
    pub crit: bool,
    pub color: u32,
    /// The ability that threw it, for the damage meter; `None` for what no ability owns.
    pub source: Option<&'static AbilityDef>,
    /// The hero whose blow it is; `None` is this machine's. The damage, a drain and the knock-back
    /// are that hero's.
    pub by: Option<Entity>,
}

impl Hit {
    pub fn direct(target: Entity, damage: f32, crit: bool) -> Self {
        Self {
            target,
            damage,
            dot: 0.0,
            dot_dur: 0.0,
            effect: Effect::None,
            mark: 0,
            mark_dur: 0.0,
            splash: 0.0,
            crit,
            color: COLOR_MELEE,
            source: None,
            by: None,
        }
    }
}

/// Hits in flight, by the run time they land.
#[derive(Resource, Default)]
pub(super) struct PendingHits(pub Vec<(f32, Hit)>);

/// A ground spell's area, ticking on whatever stands in it.
#[derive(Component)]
pub(super) struct GroundArea {
    damage: f32,
    dot: f32,
    dot_dur: f32,
    radius: f32,
    left: f32,
    every: f32,
    next_tick: f32,
    effect: Effect,
    mark: u32,
    /// The spell whose impact plays on each enemy a tick hits, 0 for none.
    impact: u32,
    source: &'static AbilityDef,
    /// The hero who placed it.
    by: Entity,
}

#[derive(Component)]
pub(super) struct Totem {
    cast: u32,
    damage: f32,
    range: f32,
    radius: f32,
    left: f32,
    tick: f32,
    pulse: bool,
    source: &'static AbilityDef,
    /// The hero who set it down.
    by: Entity,
}

#[derive(Clone, Copy)]
struct Foe {
    e: Entity,
    pos: Vec3,
    hp: f32,
    max_hp: f32,
    boss: bool,
    dist: f32,
    radius: f32,
    /// The enemy the hero clicked ([`Target`]); the picks take it before the focus's own order.
    targeted: bool,
}

/// The numbers one cast uses, from the ability's level and the hero's stats.
struct Params {
    dmg: f32,
    dot: f32,
    count: usize,
    radius: f32,
    duration: f32,
    cooldown: f32,
}

fn mark_secs(def: &AbilityDef, duration: f32) -> f32 {
    match def.effect {
        Effect::Slow(_, t) | Effect::Weaken(_, t) => t,
        Effect::Root(t) | Effect::Stun(t) | Effect::Fear(t) => t,
        _ if duration > 0.0 => duration,
        _ => 3.0,
    }
}

fn hit_for(
    def: &'static AbilityDef,
    p: &Params,
    by: Entity,
    target: Entity,
    damage: f32,
    crit: bool,
) -> Hit {
    Hit {
        target,
        damage,
        dot: p.dot,
        dot_dur: p.duration,
        effect: def.effect,
        mark: def.mark,
        mark_dur: mark_secs(def, p.duration),
        splash: if matches!(def.kind, Kind::Bolt | Kind::Strike) {
            p.radius
        } else {
            0.0
        },
        crit,
        color: if def.kind == Kind::Melee {
            COLOR_MELEE
        } else {
            COLOR_SPELL
        },
        source: Some(def),
        by: Some(by),
    }
}

fn schedule(
    pending: &mut PendingHits,
    hits: &mut MessageWriter<Hit>,
    now: f32,
    delay: f32,
    hit: Hit,
) {
    if delay <= 0.0 {
        hits.write(hit);
    } else {
        pending.0.push((now + delay, hit));
    }
}

fn roll(rng: &mut Rng, crit: f32, dmg: f32) -> (f32, bool) {
    if rng.f32() < crit {
        (dmg * 2.0, true)
    } else {
        (dmg, false)
    }
}

/// How the focus ranks two foes, the better first; ties go to the nearer. Strongest goes by max
/// health, not current, so a volley keeps its target while the blows land.
fn by_focus(focus: Focus, a: &Foe, b: &Foe) -> Ordering {
    b.targeted
        .cmp(&a.targeted)
        .then(match focus {
            Focus::Nearest => Ordering::Equal,
            Focus::Lowest => a.hp.total_cmp(&b.hp),
            Focus::Strongest => b.boss.cmp(&a.boss).then(b.max_hp.total_cmp(&a.max_hp)),
        })
        .then(a.dist.total_cmp(&b.dist))
}

/// Whether the clicked enemy is one of `foes` within `range`.
fn target_in_range(foes: &[Foe], range: f32) -> bool {
    foes.iter().any(|f| f.targeted && f.dist <= range + f.radius)
}

/// Up to `n` foes within `range`, the focus's first. `foes` is sorted by distance.
fn pick(foes: &[Foe], range: f32, n: usize, focus: Focus) -> Vec<Foe> {
    let in_range = foes.iter().filter(|f| f.dist <= range + f.radius).copied();
    if focus == Focus::Nearest && !foes.iter().any(|f| f.targeted) {
        return in_range.take(n).collect();
    }
    let mut v: Vec<Foe> = in_range.collect();
    v.sort_by(|a, b| by_focus(focus, a, b));
    v.truncate(n);
    v
}

fn toughest(foes: &[Foe], range: f32, n: usize) -> Vec<Foe> {
    let mut v: Vec<Foe> = foes
        .iter()
        .filter(|f| f.dist <= range + f.radius)
        .copied()
        .collect();
    v.sort_by(|a, b| b.hp.total_cmp(&a.hp));
    v.truncate(n);
    v
}

fn around(foes: &[Foe], center: Vec3, r: f32) -> Vec<Foe> {
    foes.iter()
        .filter(|f| flat(f.pos - center).length() <= r + f.radius * 0.5)
        .copied()
        .collect()
}

/// The point among the nearest enemies in range with the most others within `r` of it.
fn densest(foes: &[Foe], range: f32, r: f32) -> Option<Vec3> {
    let near: Vec<&Foe> = foes.iter().filter(|f| f.dist <= range).take(40).collect();
    near.iter()
        .map(|f| {
            (
                f.pos,
                near.iter()
                    .filter(|o| flat(o.pos - f.pos).length() <= r)
                    .count(),
            )
        })
        .max_by_key(|&(_, n)| n)
        .map(|(p, _)| p)
}

fn entities(foes: &[Foe]) -> Vec<Entity> {
    foes.iter().map(|f| f.e).collect()
}

/// The spell whose visual plays on the hero each time an aura ability pulses, by ability key;
/// `None` when the ability's own spell already shows on the caster. None of these ids could be
/// checked against Spell.dbc, so they are all unverified.
fn aura_caster_visual(key: &str) -> Option<u32> {
    match key {
        // Hellfire Rank 1, the channelled spell with the ring of fire around the caster; 5857
        // ("Hellfire Effect") is only the damage tick. Unverified (11683 and 11684 are ranks 2, 3).
        "hellfire" => Some(1949),
        // Sweeping Strikes itself, as a cast kit on the caster. Unverified that it has one.
        "sweeping_strikes" => Some(12292),
        _ => None,
    }
}

#[allow(clippy::too_many_arguments)]
pub(super) fn cast_abilities(
    mut commands: Commands,
    time: Res<Time>,
    mut run: ResMut<Run>,
    mut rng: ResMut<Rng>,
    focus: Res<Focus>,
    target: Res<Target>,
    spells: Option<Res<benilla_app::survivors_host::ui_action::Spells>>,
    hero: Query<(Entity, &Transform), With<Hero>>,
    mut peers: Query<(Entity, &Transform, &mut HeroState), (With<Peer>, Without<Hero>)>,
    enemies: Query<(Entity, &Transform, &Enemy), (Without<Hero>, Without<Peer>)>,
    mut fx: Fx,
    mut pending: ResMut<PendingHits>,
    mut hits: MessageWriter<Hit>,
) {
    let dt = time.delta_secs();
    if run.dead_t.is_some() || dt <= 0.0 {
        return;
    }
    let Ok((me, htf)) = hero.single() else { return };
    let speed_of = |spell: u32| {
        spells
            .as_deref()
            .and_then(|s| s.catalog.get(spell))
            .map_or(0.0, |d| d.speed)
    };
    let run = &mut *run;
    let now = run.elapsed;
    let mut cast = Caster {
        commands: &mut commands,
        rng: &mut rng,
        fx: &mut fx,
        pending: &mut pending,
        hits: &mut hits,
        next_guid: &mut run.next_guid,
        speed_of: &speed_of,
        now,
        dt,
    };
    // This machine's hero casts with the focus and the target its player chose.
    let foes = foes_from(&enemies, htf.translation, target.0);
    cast.cast(me, htf.translation, &mut run.me, &foes, *focus);
    // A co-op guest's spells are cast here too, at the nearest.
    for (e, tf, mut state) in &mut peers {
        if state.dead_t.is_some() {
            continue;
        }
        let foes = foes_from(&enemies, tf.translation, None);
        cast.cast(e, tf.translation, &mut state, &foes, Focus::Nearest);
    }
}

/// The live enemies as seen from `from`, nearest first; `target` is the one clicked.
fn foes_from(
    enemies: &Query<(Entity, &Transform, &Enemy), (Without<Hero>, Without<Peer>)>,
    from: Vec3,
    target: Option<Entity>,
) -> Vec<Foe> {
    let mut foes: Vec<Foe> = enemies
        .iter()
        .filter(|(_, _, e)| !e.dead)
        .map(|(e, tf, en)| Foe {
            e,
            pos: tf.translation,
            hp: en.hp,
            max_hp: en.max_hp,
            boss: en.boss,
            dist: hero::reach(from, tf.translation),
            radius: en.radius,
            targeted: target == Some(e),
        })
        .collect();
    foes.sort_by(|a, b| a.dist.total_cmp(&b.dist));
    foes
}

/// What one hero's casts write to: shared by every hero cast in a frame.
struct Caster<'a, 'w, 's, 'f, 'h> {
    commands: &'a mut Commands<'w, 's>,
    rng: &'a mut Rng,
    fx: &'a mut Fx<'f>,
    pending: &'a mut PendingHits,
    hits: &'a mut MessageWriter<'h, Hit>,
    next_guid: &'a mut u64,
    speed_of: &'a dyn Fn(u32) -> f32,
    now: f32,
    dt: f32,
}

impl Caster<'_, '_, '_, '_, '_> {
    fn guid(&mut self) -> u64 {
        *self.next_guid += 1;
        *self.next_guid
    }

    /// Every ability of the hero `me` at `hpos` that is off cooldown fires at `foes` (nearest
    /// first), by its kind.
    fn cast(&mut self, me: Entity, hpos: Vec3, run: &mut HeroState, foes: &[Foe], focus: Focus) {
        let dt = self.dt;
        let now = self.now;
        let stats = run.stats();
        let foes = foes.to_vec();
        for i in 0..run.abilities.len() {
            run.abilities[i].cd -= dt;
            if run.abilities[i].cd > 0.0 {
                continue;
            }
            let o = &run.abilities[i];
            let def = o.def;
            let mult = o.damage_mult() * stats.might;
            let p = Params {
                dmg: def.damage * mult,
                dot: def.dot * mult,
                count: o.count() as usize,
                radius: o.radius(&stats),
                duration: o.duration(),
                cooldown: o.cooldown(&stats),
            };
            let speed = (self.speed_of)(def.spell);
            let fired = match def.kind {
                Kind::Bolt | Kind::Strike | Kind::Melee => {
                    let reach = if def.kind == Kind::Melee {
                        def.range + 0.5
                    } else {
                        def.range
                    };
                    // The heavy hitters go for the toughest on their own; a focus the player chose
                    // outranks that.
                    let targets = if focus == Focus::Nearest
                        && !target_in_range(&foes, reach)
                        && matches!(
                            def.key,
                            "pyroblast" | "aimed_shot" | "exorcism" | "starfire"
                        ) {
                        toughest(&foes, reach, p.count)
                    } else {
                        pick(&foes, reach, p.count, focus)
                    };
                    if targets.is_empty() {
                        false
                    } else {
                        if def.kind == Kind::Melee {
                            self.fx.swing(me, targets[0].e, p.dmg);
                            for f in &targets {
                                self.fx.impact(f.e, def.spell);
                            }
                        } else {
                            self.fx.cast(me, def.spell, &entities(&targets), None);
                        }
                        for f in &targets {
                            let (dmg, crit) = roll(self.rng, stats.crit, p.dmg);
                            let delay = if speed > 0.0 { f.dist / speed } else { 0.0 };
                            schedule(
                                self.pending,
                                self.hits,
                                now,
                                delay,
                                hit_for(def, &p, me, f.e, dmg, crit),
                            );
                        }
                        true
                    }
                }
                Kind::Nova | Kind::Cone => {
                    let mut targets = around(&foes, hpos, p.radius);
                    if def.kind == Kind::Cone {
                        // The cone points at the focus, then keeps whatever stands in it.
                        if let Some(first) = pick(&targets, f32::INFINITY, 1, focus).first().copied()
                        {
                            let aim = flat(first.pos - hpos).normalize_or_zero();
                            targets.retain(|f| {
                                let d = flat(f.pos - hpos);
                                d.length() < 2.0 || d.normalize_or_zero().dot(aim) >= 0.8
                            });
                        }
                    }
                    if targets.is_empty() {
                        false
                    } else {
                        self.fx.cast(me, def.spell, &entities(&targets), None);
                        for f in &targets {
                            let (dmg, crit) = roll(self.rng, stats.crit, p.dmg);
                            let delay = if speed > 0.0 { f.dist / speed } else { 0.0 };
                            schedule(
                                self.pending,
                                self.hits,
                                now,
                                delay,
                                hit_for(def, &p, me, f.e, dmg, crit),
                            );
                        }
                        true
                    }
                }
                Kind::Ground | Kind::GroundSelf => {
                    let center = if def.kind == Kind::GroundSelf {
                        foes.first()
                            .filter(|f| f.dist <= p.radius + 4.0)
                            .map(|_| hpos)
                    } else {
                        densest(&foes, def.range, p.radius)
                    };
                    match center {
                        None => false,
                        Some(center) => {
                            self.fx.cast(me, def.spell, &[], Some(center));
                            let area = if def.overhead {
                                self.commands
                                    .spawn((Transform::from_translation(center), RunEntity))
                                    .id()
                            } else {
                                let serial = self.guid();
                                units::spawn_area_object(
                                    self.commands,
                                    serial,
                                    def.spell,
                                    p.radius,
                                    center,
                                )
                            };
                            let burst = def.duration <= 0.0;
                            self.commands.entity(area).insert(GroundArea {
                                damage: p.dmg,
                                dot: p.dot,
                                dot_dur: if burst { 5.0 } else { 0.0 },
                                radius: p.radius,
                                left: if burst { 1.5 } else { p.duration },
                                every: if burst { f32::INFINITY } else { 1.0 },
                                next_tick: if burst { 0.0 } else { 0.4 },
                                effect: def.effect,
                                mark: def.mark,
                                impact: if def.overhead { def.spell } else { 0 },
                                source: def,
                                by: me,
                            });
                            true
                        }
                    }
                }
                Kind::Chain => match pick(&foes, def.range, 1, focus).first().copied() {
                    None => false,
                    Some(first) => {
                        let mut chain = vec![first];
                        while chain.len() < p.count + 1 {
                            let last = chain[chain.len() - 1].pos;
                            let next = foes
                                .iter()
                                .filter(|f| !chain.iter().any(|c| c.e == f.e))
                                .filter(|f| flat(f.pos - last).length() <= p.radius)
                                .min_by(|a, b| {
                                    flat(a.pos - last)
                                        .length()
                                        .total_cmp(&flat(b.pos - last).length())
                                });
                            match next {
                                Some(f) => chain.push(*f),
                                None => break,
                            }
                        }
                        self.fx.cast(me, def.spell, &entities(&chain), None);
                        for (k, f) in chain.iter().enumerate() {
                            let (dmg, crit) =
                                roll(self.rng, stats.crit, p.dmg * 0.85f32.powi(k as i32));
                            schedule(
                                self.pending,
                                self.hits,
                                now,
                                0.08 * k as f32,
                                hit_for(def, &p, me, f.e, dmg, crit),
                            );
                        }
                        true
                    }
                },
                Kind::Dot => {
                    // Nearest spreads the curses over a few of the closest at random; a chosen
                    // focus puts them on its first picks.
                    let targets = if focus == Focus::Nearest && !target_in_range(&foes, def.range) {
                        let mut pool: Vec<Foe> = foes
                            .iter()
                            .filter(|f| f.dist <= def.range)
                            .take(p.count * 3)
                            .copied()
                            .collect();
                        let mut targets = Vec::with_capacity(p.count);
                        while targets.len() < p.count && !pool.is_empty() {
                            targets.push(pool.swap_remove(self.rng.index(pool.len())));
                        }
                        targets
                    } else {
                        pick(&foes, def.range, p.count, focus)
                    };
                    if targets.is_empty() {
                        false
                    } else {
                        self.fx.cast(me, def.spell, &entities(&targets), None);
                        for f in &targets {
                            let delay = if speed > 0.0 { f.dist / speed } else { 0.0 };
                            schedule(
                                self.pending,
                                self.hits,
                                now,
                                delay,
                                hit_for(def, &p, me, f.e, 0.0, false),
                            );
                        }
                        true
                    }
                }
                Kind::Aura => {
                    let targets = around(&foes, hpos, p.radius);
                    if targets.is_empty() {
                        false
                    } else {
                        // The damage tick spell has no caster visual of its own, so the ring
                        // around the hero is a second spell, played on the hero each pulse.
                        if let Some(visual) = aura_caster_visual(def.key) {
                            self.fx.cast(me, visual, &[me], None);
                        }
                        for f in targets.iter().take(6) {
                            self.fx.impact(f.e, def.spell);
                        }
                        for f in &targets {
                            let (dmg, crit) = roll(self.rng, stats.crit, p.dmg);
                            self.hits.write(hit_for(def, &p, me, f.e, dmg, crit));
                        }
                        true
                    }
                }
                Kind::Orbit => {
                    let targets = pick(&foes, p.radius, p.count, focus);
                    if targets.is_empty() {
                        false
                    } else {
                        for f in &targets {
                            self.fx.impact(f.e, 403);
                            let (dmg, crit) = roll(self.rng, stats.crit, p.dmg);
                            self.hits.write(hit_for(def, &p, me, f.e, dmg, crit));
                        }
                        true
                    }
                }
                Kind::Shield => {
                    run.shield = p.dmg;
                    run.shield_t = p.duration;
                    self.fx.cast(me, def.spell, &[me], None);
                    true
                }
                Kind::Immune => {
                    if foes.first().is_some_and(|f| f.dist <= 5.0) {
                        run.immune_t = p.duration;
                        self.fx.cast(me, def.spell, &[me], None);
                        true
                    } else {
                        false
                    }
                }
                Kind::Thorns => {
                    if foes.first().is_some_and(|f| f.dist <= 8.0) {
                        run.thorns = p.dmg;
                        run.thorns_t = p.duration;
                        run.thorns_spell = def.spell;
                        self.fx.cast(me, def.spell, &[me], None);
                        true
                    } else {
                        false
                    }
                }
                Kind::Heal => {
                    if run.hp < stats.max_hp * 0.85 {
                        run.heal_rate = p.dmg / p.duration.max(1.0);
                        run.heal_t = p.duration;
                        self.fx.cast(me, def.spell, &[me], None);
                        self.fx.number(me, p.dmg, false, COLOR_HEAL);
                        true
                    } else {
                        false
                    }
                }
                Kind::Totem => {
                    if foes
                        .first()
                        .is_some_and(|f| f.dist <= def.range.max(p.radius) + 6.0)
                    {
                        self.fx.cast(me, def.spell, &[], None);
                        let a = self.rng.range(0.0, std::f32::consts::TAU);
                        let pos = hpos + Vec3::new(a.cos(), 0.0, a.sin()) * 2.0;
                        let serial = self.guid();
                        let totem = units::spawn_unit(
                            self.commands,
                            &UnitSpec {
                                serial,
                                display: def.totem.0,
                                scale: 1.0,
                                level: 60,
                                faction: units::FACTION_FRIENDLY,
                                bytes0: 0,
                                weapons: &[],
                                sheath: 0,
                            },
                            pos,
                            0.0,
                        );
                        self.commands.entity(totem).insert(Totem {
                            cast: def.totem.1,
                            damage: p.dmg,
                            range: def.range,
                            radius: p.radius,
                            left: p.duration,
                            tick: 0.3,
                            pulse: def.key == "magma_totem",
                            source: def,
                            by: me,
                        });
                        true
                    } else {
                        false
                    }
                }
            };
            run.abilities[i].cd = if fired { p.cooldown } else { 0.2 };
        }
    }
}

/// Ground areas tick, and totems fight, until they run out.
#[allow(clippy::type_complexity)]
pub(super) fn tick_areas(
    mut commands: Commands,
    time: Res<Time>,
    run: Res<Run>,
    mut rng: ResMut<Rng>,
    focus: Res<Focus>,
    spells: Option<Res<benilla_app::survivors_host::ui_action::Spells>>,
    mut areas: Query<(Entity, &Transform, &mut GroundArea)>,
    mut totems: Query<(Entity, &Transform, &mut Totem), Without<GroundArea>>,
    enemies: Query<(Entity, &Transform, &Enemy), (Without<GroundArea>, Without<Totem>)>,
    peers: Query<&HeroState, With<Peer>>,
    mut fx: Fx,
    mut pending: ResMut<PendingHits>,
    mut hits: MessageWriter<Hit>,
) {
    let dt = time.delta_secs();
    if dt <= 0.0 {
        return;
    }
    // An area or a totem crits as often as the hero who placed it.
    let crit = run.stats().crit;
    let crit_of = |by: Entity| peers.get(by).map_or(crit, |s| s.stats().crit);
    let now = run.elapsed;
    let live: Vec<Foe> = enemies
        .iter()
        .filter(|(_, _, e)| !e.dead)
        .map(|(e, tf, en)| Foe {
            e,
            pos: tf.translation,
            hp: en.hp,
            max_hp: en.max_hp,
            boss: en.boss,
            dist: 0.0,
            radius: en.radius,
            targeted: false,
        })
        .collect();

    for (e, tf, mut area) in &mut areas {
        area.left -= dt;
        area.next_tick -= dt;
        if area.next_tick <= 0.0 {
            area.next_tick += area.every;
            for (i, f) in around(&live, tf.translation, area.radius)
                .into_iter()
                .enumerate()
            {
                if area.impact != 0 && i < 12 {
                    fx.impact(f.e, area.impact);
                }
                let (dmg, crit) = roll(&mut rng, crit_of(area.by), area.damage);
                hits.write(Hit {
                    target: f.e,
                    damage: dmg,
                    dot: area.dot,
                    dot_dur: area.dot_dur,
                    effect: area.effect,
                    mark: area.mark,
                    mark_dur: 1.5,
                    splash: 0.0,
                    crit,
                    color: COLOR_SPELL,
                    source: Some(area.source),
                    by: Some(area.by),
                });
            }
        }
        if area.left <= 0.0 {
            commands.entity(e).despawn();
        }
    }

    for (e, tf, mut totem) in &mut totems {
        totem.left -= dt;
        if totem.left <= 0.0 {
            commands
                .entity(e)
                .remove::<Totem>()
                .insert(benilla_world::model_fade::DespawnFade::default());
            continue;
        }
        totem.tick -= dt;
        if totem.tick > 0.0 {
            continue;
        }
        let pos = tf.translation;
        if totem.pulse {
            let targets = around(&live, pos, totem.radius);
            totem.tick = 2.0;
            if targets.is_empty() {
                continue;
            }
            fx.cast(e, totem.cast, &[], None);
            for f in &targets {
                let (dmg, crit) = roll(&mut rng, crit_of(totem.by), totem.damage);
                hits.write(Hit {
                    color: COLOR_SPELL,
                    source: Some(totem.source),
                    by: Some(totem.by),
                    ..Hit::direct(f.e, dmg, crit)
                });
            }
        } else {
            totem.tick = 1.0;
            // In the totem's range, the focus's first, measured from the totem.
            let target = live
                .iter()
                .map(|f| Foe {
                    dist: flat(f.pos - pos).length(),
                    ..*f
                })
                .filter(|f| f.dist <= totem.range)
                .min_by(|a, b| by_focus(*focus, a, b));
            let Some(f) = target else { continue };
            let dist = f.dist;
            fx.cast(e, totem.cast, &[f.e], None);
            let speed = spells
                .as_deref()
                .and_then(|s| s.catalog.get(totem.cast))
                .map_or(0.0, |d| d.speed);
            let delay = if speed > 0.0 { dist / speed } else { 0.0 };
            let (dmg, crit) = roll(&mut rng, crit_of(totem.by), totem.damage);
            schedule(
                &mut pending,
                &mut hits,
                now,
                delay,
                Hit {
                    color: COLOR_SPELL,
                    source: Some(totem.source),
                    by: Some(totem.by),
                    ..Hit::direct(f.e, dmg, crit)
                },
            );
        }
    }
}

/// Camera trauma a crit on an elite or a boss adds, and the level above which a further one adds
/// none, so a hail of them does not pin the shake at its limit.
const CRIT_TRAUMA: f32 = 0.08;
const CRIT_TRAUMA_CAP: f32 = 0.3;
/// Camera trauma the death of a boss and of an elite add.
const BOSS_DEATH_TRAUMA: f32 = 0.5;
const ELITE_DEATH_TRAUMA: f32 = 0.15;

/// Whether a hit of this kind of ability shoves its target. Ground areas, totems, auras, orbits
/// and damage over time land every second or so on a whole crowd, which would only make it
/// vibrate.
fn knocks(kind: Kind) -> bool {
    matches!(
        kind,
        Kind::Bolt | Kind::Strike | Kind::Melee | Kind::Nova | Kind::Cone | Kind::Chain
    )
}

/// Camera trauma a landed hit adds: a crit on an elite or a boss, while the shake is still below
/// [`CRIT_TRAUMA_CAP`].
fn crit_trauma(crit: bool, heavy: bool, current: f32) -> f32 {
    if crit && heavy && current < CRIT_TRAUMA_CAP {
        CRIT_TRAUMA
    } else {
        0.0
    }
}

/// Camera trauma a kill adds: the fall of a boss or an elite is felt.
fn death_trauma(boss: bool, elite: bool) -> f32 {
    if boss {
        BOSS_DEATH_TRAUMA
    } else if elite {
        ELITE_DEATH_TRAUMA
    } else {
        0.0
    }
}

/// Land every hit due: damage and its number, the over-time part, crowd control, the marks, and
/// death with its loot. A direct blow also knocks its target back from the hero, and the heavy
/// moments shake the camera.
pub(super) fn apply_hits(
    mut commands: Commands,
    time: Res<Time>,
    mut run: ResMut<Run>,
    mut pending: ResMut<PendingHits>,
    mut incoming: MessageReader<Hit>,
    mut enemies: Query<(
        Entity,
        &Guid,
        &Transform,
        &mut Enemy,
        &mut ObjectStore,
        &mut Marks,
    )>,
    mut fx: Fx,
    mut loot: MessageWriter<Loot>,
    mut fields: MessageWriter<FieldChanged>,
    mut chat: MessageWriter<super::chatlog::ChatLine>,
    mut meter: ResMut<DamageDone>,
    mut shake: ResMut<super::prefs::Shake>,
    hero: Query<(Entity, &Transform), (With<Hero>, Without<Enemy>)>,
    mut peers: Query<(&Transform, &mut HeroState), (With<Peer>, Without<Hero>, Without<Enemy>)>,
    elites: Query<(), With<enemies::Elite>>,
) {
    let dt = time.delta_secs();
    let now = run.elapsed;
    let local = hero.single().ok();
    let hero_at = local.map(|(_, t)| t.translation);
    // A co-op guest's blow: its puppet, when `by` names one.
    let guest = |h: &Hit| h.by.filter(|b| local.is_none_or(|(e, _)| e != *b));
    let mut due: Vec<Hit> = incoming.read().cloned().collect();
    pending.0.retain(|(t, h)| {
        if *t <= now {
            due.push(h.clone());
            false
        } else {
            true
        }
    });

    // Splash reaches the target's neighbours at seventy percent.
    let mut all = Vec::with_capacity(due.len());
    for h in due {
        if h.splash > 0.0 {
            if let Ok((_, _, center, ..)) = enemies.get(h.target) {
                let c = center.translation;
                for (e, _, tf, en, ..) in enemies.iter() {
                    if e != h.target && !en.dead && flat(tf.translation - c).length() <= h.splash {
                        all.push(Hit {
                            target: e,
                            damage: h.damage * 0.7,
                            splash: 0.0,
                            mark: 0,
                            ..h.clone()
                        });
                    }
                }
            }
        }
        all.push(Hit { splash: 0.0, ..h });
    }

    let mut kill =
        |commands: &mut Commands, run: &mut Run, e: Entity, pos: Vec3, en: &mut Enemy| {
            en.dead = true;
            en.hp = 0.0;
            en.dots.clear();
            enemies::mark_dying(commands, e);
            loot.write(Loot {
                pos,
                xp: en.xp,
                boss: en.boss,
                level: en.def.level,
                def: en.def,
            });
            run.kills += 1;
            if en.boss {
                run.banner = Some((format!("{} is slain!", en.def.name), 3.0));
                chat.write(super::chatlog::ChatLine::text(
                    super::chatlog::ChatKind::Combat,
                    format!("{} dies.", en.def.name),
                ));
            }
        };

    for h in all {
        let Ok((e, guid, tf, mut en, mut store, mut marks)) = enemies.get_mut(h.target) else {
            continue;
        };
        if en.dead {
            continue;
        }
        let mut dmg = h.damage;
        if let Effect::Execute(threshold) = h.effect {
            if !en.boss && en.hp / en.max_hp <= threshold {
                dmg = dmg.max(en.hp);
            }
        }
        if dmg > 0.0 {
            en.hp -= dmg;
            let mut owner = guest(&h).and_then(|b| peers.get_mut(b).ok());
            let from = match &owner {
                Some((tf, _)) => Some(tf.translation),
                None => hero_at,
            };
            let thrower: &mut HeroState = match owner.as_mut() {
                Some((_, state)) => state,
                None => &mut run.me,
            };
            thrower.damage_done += f64::from(dmg);
            if let Effect::Drain(frac) = h.effect {
                let max = thrower.stats().max_hp;
                thrower.hp = (thrower.hp + dmg * frac).min(max);
            }
            fx.number(e, dmg, h.crit, h.color);
            let elite = elites.contains(e);
            if owner.is_none() {
                // The meter and the shake are this machine's hero's.
                meter.add(h.source, dmg);
                let jolt = crit_trauma(h.crit, en.boss || elite, shake.trauma);
                shake.add(jolt);
            }
            if h.source.is_some_and(|d| knocks(d.kind)) {
                let away = from.map_or(Vec3::ZERO, |p| flat(tf.translation - p));
                let push = enemies::knock_impulse(h.crit, elite, en.boss);
                en.knock = (en.knock + away.normalize_or_zero() * push)
                    .clamp_length_max(enemies::KNOCK_MAX);
            }
        }
        if h.dot > 0.0 && h.dot_dur > 0.0 {
            if en.dots.len() >= 6 {
                en.dots.remove(0);
            }
            en.dots.push(enemies::Dot {
                dps: h.dot,
                left: h.dot_dur,
                tick: 1.0,
                source: h.source,
                by: guest(&h),
            });
        }
        let boss = en.boss;
        match h.effect {
            Effect::Slow(m, t) => {
                let m = if boss { m.max(0.75) } else { m };
                en.slow = if en.slow_t > 0.0 { en.slow.min(m) } else { m };
                en.slow_t = en.slow_t.max(t);
            }
            Effect::Root(t) => en.root_t = en.root_t.max(if boss { t * 0.3 } else { t }),
            Effect::Stun(t) => en.stun_t = en.stun_t.max(if boss { t * 0.3 } else { t }),
            Effect::Fear(t) if !boss => en.fear_t = en.fear_t.max(t),
            Effect::Weaken(m, t) => {
                en.weaken = m;
                en.weaken_t = t;
            }
            _ => {}
        }
        if h.mark != 0 {
            marks.add(h.mark, h.mark_dur);
        }
        if en.hp <= 0.0 {
            kill(&mut commands, &mut run, e, tf.translation, &mut en);
            shake.add(death_trauma(en.boss, elites.contains(e)));
            units::kill_unit(&mut store);
            marks.clear();
        }
        marks.flush(e, guid.0, &mut store, &mut fields);
    }

    // The over-time ticks, once a second each.
    for (e, guid, tf, mut en, mut store, mut marks) in &mut enemies {
        if en.dead || en.dots.is_empty() {
            continue;
        }
        let mut dealt = 0.0;
        for d in &mut en.dots {
            d.left -= dt;
            d.tick -= dt;
            if d.tick <= 0.0 {
                d.tick += 1.0;
                dealt += d.dps;
                match d.by.and_then(|b| peers.get_mut(b).ok()) {
                    Some((_, mut state)) => state.damage_done += f64::from(d.dps),
                    None => {
                        meter.add(d.source, d.dps);
                        run.damage_done += f64::from(d.dps);
                    }
                }
            }
        }
        en.dots.retain(|d| d.left > 0.0);
        if dealt > 0.0 {
            en.hp -= dealt;
            fx.number(e, dealt, false, COLOR_SPELL);
            if en.hp <= 0.0 {
                kill(&mut commands, &mut run, e, tf.translation, &mut en);
                shake.add(death_trauma(en.boss, elites.contains(e)));
                units::kill_unit(&mut store);
                marks.clear();
                marks.flush(e, guid.0, &mut store, &mut fields);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn foe(dist: f32, hp: f32, max_hp: f32, boss: bool) -> Foe {
        Foe {
            e: Entity::PLACEHOLDER,
            pos: Vec3::ZERO,
            hp,
            max_hp,
            boss,
            dist,
            radius: 0.0,
            targeted: false,
        }
    }

    /// Sorted by distance, as `cast_abilities` hands them over.
    fn horde() -> Vec<Foe> {
        vec![
            foe(2.0, 90.0, 100.0, false),
            foe(4.0, 10.0, 100.0, false),
            foe(6.0, 50.0, 400.0, false),
            foe(8.0, 900.0, 1000.0, true),
            foe(30.0, 1.0, 100.0, false),
        ]
    }

    fn hps(v: &[Foe]) -> Vec<f32> {
        v.iter().map(|f| f.hp).collect()
    }

    #[test]
    fn nearest_takes_the_closest_in_range() {
        let got = pick(&horde(), 10.0, 2, Focus::Nearest);
        assert_eq!(hps(&got), [90.0, 10.0]);
    }

    #[test]
    fn lowest_orders_by_current_health_within_range() {
        // The foe at 30 yd has the least health but is out of range.
        let got = pick(&horde(), 10.0, 3, Focus::Lowest);
        assert_eq!(hps(&got), [10.0, 50.0, 90.0]);
    }

    #[test]
    fn strongest_puts_bosses_first_then_max_health() {
        let got = pick(&horde(), 10.0, 3, Focus::Strongest);
        assert_eq!(hps(&got), [900.0, 50.0, 90.0]);
    }

    #[test]
    fn ties_fall_to_the_nearer() {
        let foes = vec![foe(1.0, 5.0, 10.0, false), foe(2.0, 5.0, 10.0, false)];
        for focus in [Focus::Lowest, Focus::Strongest] {
            let got = pick(&foes, 10.0, 2, focus);
            assert_eq!(got[0].dist, 1.0);
        }
    }

    #[test]
    fn the_target_goes_first_when_in_range_whatever_the_focus() {
        let mut foes = horde();
        foes[3].targeted = true;
        for focus in [Focus::Nearest, Focus::Lowest, Focus::Strongest] {
            let got = pick(&foes, 10.0, 2, focus);
            assert_eq!(got[0].hp, 900.0, "{focus:?}");
            assert_eq!(got.len(), 2);
        }
        // Out of range, the target waits and the focus picks as before.
        let got = pick(&foes, 5.0, 2, Focus::Nearest);
        assert_eq!(hps(&got), [90.0, 10.0]);
    }

    #[test]
    fn range_counts_the_foes_radius() {
        let mut far = foe(11.0, 1.0, 1.0, false);
        far.radius = 1.5;
        assert_eq!(pick(&[far], 10.0, 1, Focus::Lowest).len(), 1);
    }

    #[test]
    fn only_direct_blows_knock() {
        for kind in [
            Kind::Bolt,
            Kind::Strike,
            Kind::Melee,
            Kind::Nova,
            Kind::Cone,
            Kind::Chain,
        ] {
            assert!(knocks(kind), "{kind:?}");
        }
        for kind in [
            Kind::Ground,
            Kind::GroundSelf,
            Kind::Totem,
            Kind::Aura,
            Kind::Orbit,
            Kind::Dot,
        ] {
            assert!(!knocks(kind), "{kind:?}");
        }
    }

    #[test]
    fn heavy_crits_shake_until_the_cap() {
        assert_eq!(crit_trauma(true, true, 0.0), CRIT_TRAUMA);
        assert_eq!(crit_trauma(true, true, CRIT_TRAUMA_CAP), 0.0);
        assert_eq!(crit_trauma(true, false, 0.0), 0.0);
        assert_eq!(crit_trauma(false, true, 0.0), 0.0);
    }

    #[test]
    fn bosses_shake_harder_in_death_than_elites() {
        assert!(death_trauma(true, false) > death_trauma(false, true));
        assert!(death_trauma(false, true) > 0.0);
        assert_eq!(death_trauma(false, false), 0.0);
    }
}
