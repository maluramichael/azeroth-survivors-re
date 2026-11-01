//! Combat visuals on the creatures: a hit flashes the struck enemy, a crit flashes it longer,
//! and the dead fade and sink into the ground instead of vanishing. Switched by
//! [`super::prefs::Prefs::hit_flash`] and [`super::prefs::Prefs::death_fade`].
//!
//! The flash is the engine's own mouseover brighten: bit 31 of every part's `MeshTag`
//! ([`HIGHLIGHT_BIT`]) adds the scene ambient to the lighting sum, per instance, for the price of
//! one bit write per part. There is no per-instance colour that could go brighter than white
//! (`benilla_world::instance_tint` only multiplies and is keyed by rig slot, which the aura kits
//! also write), so the flash is on or off and a crit is told by lasting longer. Hits are read off
//! the damage numbers ([`CombatTextSpawn`]), which a co-op guest replays too, so no hook in the
//! combat code is needed.
//!
//! A corpse already stays a [`Dying`](super::enemies::Dying) enemy with `dead` set, which every
//! query that counts, targets or hits an enemy skips, and plays its Death animation (the unit's
//! health field is zero). What this module adds is the timeline: the body lies for
//! [`corpse_hold`], then the engine's teardown fade ([`DespawnFade`]) runs and the body sinks
//! with it, and it is despawned when the fade has all but ended. A co-op guest gets the sink
//! through the host's replicated position and starts the fade by itself.

use benilla_app::survivors_host::combat_text::CombatTextSpawn;
use benilla_world::mesh_tag::HIGHLIGHT_BIT;
use benilla_world::model_fade::{DespawnFade, APPEAR_FADE_SECS};
use bevy::mesh::MeshTag;
use bevy::prelude::*;

use super::enemies::{Elite, Enemy};
use super::prefs::Prefs;
use super::NetRole;

pub(super) fn plugin(app: &mut App) {
    app.add_systems(Update, (start_flashes, lay_guest_corpses, sink_corpses))
        // In PostUpdate, where the engine's own highlight writer runs, after the Update writers
        // that rewrite whole tags.
        .add_systems(PostUpdate, drive_flashes);
}

/// Seconds a plain hit lights its target, and a crit.
const FLASH_HIT: f32 = 0.1;
const FLASH_CRIT: f32 = 0.18;
/// Seconds an elite or a boss stays dark between two flashes, so a creature that is hit all the
/// time does not strobe.
const FLASH_GAP_HEAVY: f32 = 0.2;
/// Enemies that flash at once; past it a plain hit on a grunt shows nothing, so a hail of damage
/// over time on a whole crowd costs no more than this many bodies' tags a frame.
const MAX_FLASHES: usize = 48;
/// The `CombatTextSpawn` category of a crit.
const CATEGORY_CRIT: u8 = 2;

/// How long a hit lights its target.
fn flash_secs(crit: bool) -> f32 {
    if crit {
        FLASH_CRIT
    } else {
        FLASH_HIT
    }
}

/// How long after its light a creature takes no new flash.
fn flash_gap(heavy: bool) -> f32 {
    if heavy {
        FLASH_GAP_HEAVY
    } else {
        0.0
    }
}

/// Whether a flash `age` seconds old of `secs` is still lit.
fn flash_lit(age: f32, secs: f32) -> bool {
    age < secs
}

/// A struck enemy lit for a moment ([`start_flashes`], [`drive_flashes`]).
#[derive(Component)]
struct Flash {
    age: f32,
    secs: f32,
    gap: f32,
    crit: bool,
    /// The bit is on the body's parts now.
    lit: bool,
}

impl Flash {
    fn new(crit: bool, heavy: bool) -> Self {
        Self {
            age: 0.0,
            secs: flash_secs(crit),
            gap: flash_gap(heavy),
            crit,
            lit: false,
        }
    }
}

/// Every enemy a damage number was shown over this frame gets a [`Flash`]; one already lit only
/// takes the crit's longer light.
fn start_flashes(
    mut commands: Commands,
    prefs: Res<Prefs>,
    mut texts: MessageReader<CombatTextSpawn>,
    mut foes: Query<(&Enemy, Has<Elite>, Option<&mut Flash>)>,
) {
    if !prefs.hit_flash {
        texts.clear();
        return;
    }
    let mut struck: Vec<(Entity, bool)> = Vec::new();
    for t in texts.read() {
        if !foes.contains(t.anchor) {
            continue;
        }
        let crit = t.category == CATEGORY_CRIT;
        match struck.iter_mut().find(|(e, _)| *e == t.anchor) {
            Some(s) => s.1 |= crit,
            None => struck.push((t.anchor, crit)),
        }
    }
    if struck.is_empty() {
        return;
    }
    let mut live = foes.iter().filter(|(_, _, f)| f.is_some()).count();
    for (e, crit) in struck {
        let Ok((enemy, elite, flash)) = foes.get_mut(e) else {
            continue;
        };
        let heavy = enemy.boss || elite;
        match flash {
            Some(mut f) => {
                if crit && !f.crit && f.age < f.secs {
                    f.crit = true;
                    f.secs = flash_secs(true);
                }
            }
            None => {
                if live >= MAX_FLASHES && !(crit || heavy) {
                    continue;
                }
                live += 1;
                commands.entity(e).try_insert(Flash::new(crit, heavy));
            }
        }
    }
}

/// Sets or clears the highlight bit on `root` and every part below it, as the engine's own writer
/// does for the mouseover.
fn set_lit(root: Entity, on: bool, children: &Query<&Children>, tags: &mut Query<&mut MeshTag>) {
    for e in std::iter::once(root).chain(children.iter_descendants(root)) {
        if let Ok(mut tag) = tags.get_mut(e) {
            let bits = if on {
                tag.0 | HIGHLIGHT_BIT
            } else {
                tag.0 & !HIGHLIGHT_BIT
            };
            if tag.0 != bits {
                tag.0 = bits;
            }
        }
    }
}

/// Ages the flashes and writes their light onto the parts every frame, as the Update writers
/// that rewrite a whole tag drop the bit; a flash that ended clears it.
fn drive_flashes(
    mut commands: Commands,
    time: Res<Time>,
    prefs: Res<Prefs>,
    mut flashes: Query<(Entity, &mut Flash)>,
    children: Query<&Children>,
    mut tags: Query<&mut MeshTag>,
) {
    let dt = time.delta_secs();
    for (e, mut f) in &mut flashes {
        f.age += dt;
        let lit = prefs.hit_flash && flash_lit(f.age, f.secs);
        if lit {
            set_lit(e, true, &children, &mut tags);
        } else if f.lit {
            set_lit(e, false, &children, &mut tags);
        }
        f.lit = lit;
        if !lit && (f.age >= f.secs + f.gap || !prefs.hit_flash) {
            commands.entity(e).try_remove::<Flash>();
        }
    }
}

/// Seconds a body lies after it fell before it starts to fade, while its Death animation plays.
/// A boss lies longer; with the fade switched off it is the corpse's old time.
pub(super) fn corpse_hold(fade: bool, boss: bool, elite: bool) -> f32 {
    if !fade {
        HOLD_PLAIN
    } else if boss {
        HOLD_BOSS
    } else if elite {
        HOLD_ELITE
    } else {
        HOLD_GRUNT
    }
}

/// The corpse's time before the fade without the new look: what the game always did.
const HOLD_PLAIN: f32 = 2.5;
const HOLD_GRUNT: f32 = 0.4;
const HOLD_ELITE: f32 = 0.7;
const HOLD_BOSS: f32 = 1.8;
/// The share of the engine's fade after which the body is despawned: its alpha is a smoothstep
/// and a few hundredths by then, so the last stretch only costs frames.
const FADE_CUT: f32 = 0.85;
/// A body sinks this share of its radius, and a boss this much more.
const SINK_PER_RADIUS: f32 = 1.2;
const SINK_BOSS: f32 = 1.3;
/// The least and most a body sinks, in yards.
const SINK_MIN: f32 = 0.5;
const SINK_MAX: f32 = 4.0;

/// How much of its sink a body has made `t` of the way through the fade, 0..1: slow at first so
/// the fall can be seen, then quicker.
fn sink_fraction(t: f32) -> f32 {
    let t = t.clamp(0.0, 1.0);
    t * t
}

/// How deep a body of `radius` yards sinks.
fn sink_depth(radius: f32, boss: bool) -> f32 {
    (radius * SINK_PER_RADIUS * if boss { SINK_BOSS } else { 1.0 }).clamp(SINK_MIN, SINK_MAX)
}

/// The height of a body that stood at `y0` and has been fading for `age` seconds.
fn sink_y(y0: f32, depth: f32, age: f32) -> f32 {
    y0 - depth * sink_fraction(age / APPEAR_FADE_SECS)
}

/// Whether a body that has been fading for `age` seconds is done.
fn faded(age: f32) -> bool {
    age >= APPEAR_FADE_SECS * FADE_CUT
}

/// Where a corpse stood and how deep it goes.
#[derive(Component)]
struct Sinking {
    y0: f32,
    depth: f32,
}

/// A fallen enemy whose fade the engine has armed ([`DespawnFade`], inserted by
/// [`super::enemies::reap_dead`]) sinks with it and goes when it has all but ended. A co-op
/// guest's bodies take their height from the host's replicated position, so only the host and a
/// solo run move them.
fn sink_corpses(
    mut commands: Commands,
    time: Res<Time>,
    prefs: Res<Prefs>,
    role: Res<NetRole>,
    mut corpses: Query<(Entity, &mut Transform, &DespawnFade, &Enemy, Option<&Sinking>)>,
) {
    if !prefs.death_fade || *role == NetRole::Client {
        return;
    }
    let now = time.elapsed_secs();
    for (e, mut tf, fade, enemy, sinking) in &mut corpses {
        if !enemy.dead {
            continue;
        }
        // Armed on the engine's next pass after the insert.
        let Some(started) = fade.armed() else {
            continue;
        };
        let age = now - started;
        match sinking {
            None => {
                commands.entity(e).try_insert(Sinking {
                    y0: tf.translation.y,
                    depth: sink_depth(enemy.radius, enemy.boss),
                });
            }
            Some(s) => {
                let y = sink_y(s.y0, s.depth, age);
                if tf.translation.y != y {
                    tf.translation.y = y;
                }
                if faded(age) {
                    commands.entity(e).try_despawn();
                }
            }
        }
    }
}

/// Seconds a co-op guest's replicated corpse has lain since it saw the host say it fell.
#[derive(Component)]
struct Lying(f32);

/// On a co-op guest nothing reaps the host's dead (they carry no `Dying` here), so a body whose
/// `dead` the host sent lies for its hold and then starts the engine's fade by itself. The host's
/// despawn arrives about as the fade ends; a body already gone locally is ignored by the
/// replication.
fn lay_guest_corpses(
    mut commands: Commands,
    time: Res<Time>,
    prefs: Res<Prefs>,
    role: Res<NetRole>,
    mut bodies: Query<(Entity, &Enemy, Has<Elite>, Option<&mut Lying>), Without<DespawnFade>>,
) {
    if !prefs.death_fade || *role != NetRole::Client {
        return;
    }
    let dt = time.delta_secs();
    for (e, enemy, elite, lying) in &mut bodies {
        if !enemy.dead {
            continue;
        }
        match lying {
            None => {
                commands.entity(e).try_insert(Lying(0.0));
            }
            Some(mut l) => {
                l.0 += dt;
                if l.0 >= corpse_hold(true, enemy.boss, elite) {
                    commands.entity(e).try_insert(DespawnFade::default());
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_hit_flashes_for_about_a_tenth_of_a_second_and_a_crit_longer() {
        assert!((0.08..=0.12).contains(&flash_secs(false)));
        assert!((0.15..=0.25).contains(&flash_secs(true)));
        assert!(flash_secs(true) > flash_secs(false));
    }

    #[test]
    fn a_flash_is_lit_until_its_time_is_up() {
        let secs = flash_secs(false);
        assert!(flash_lit(0.0, secs));
        assert!(flash_lit(secs - 0.001, secs));
        assert!(!flash_lit(secs, secs));
        assert!(!flash_lit(1.0, secs));
    }

    #[test]
    fn only_the_heavy_wait_between_two_flashes() {
        assert_eq!(flash_gap(false), 0.0);
        assert!(flash_gap(true) > 0.0);
        let f = Flash::new(true, true);
        assert!(f.crit && f.secs == FLASH_CRIT && f.gap == FLASH_GAP_HEAVY && !f.lit);
        let f = Flash::new(false, false);
        assert!(!f.crit && f.secs == FLASH_HIT && f.gap == 0.0);
    }

    #[test]
    fn the_switched_off_corpse_keeps_its_old_time() {
        for (boss, elite) in [(false, false), (false, true), (true, false)] {
            assert_eq!(corpse_hold(false, boss, elite), HOLD_PLAIN);
        }
    }

    #[test]
    fn bosses_lie_longer_than_elites_and_elites_longer_than_grunts() {
        let grunt = corpse_hold(true, false, false);
        let elite = corpse_hold(true, false, true);
        let boss = corpse_hold(true, true, false);
        assert!(grunt < elite && elite < boss);
        assert!(boss < HOLD_PLAIN, "no corpse stays longer than it did");
    }

    #[test]
    fn a_grunt_is_gone_within_about_two_seconds() {
        let total = corpse_hold(true, false, false) + APPEAR_FADE_SECS * FADE_CUT;
        assert!((1.5..=2.2).contains(&total), "{total}");
    }

    #[test]
    fn the_sink_starts_at_rest_ends_at_full_depth_and_never_rises() {
        assert_eq!(sink_fraction(0.0), 0.0);
        assert_eq!(sink_fraction(1.0), 1.0);
        assert_eq!(sink_fraction(-1.0), 0.0);
        assert_eq!(sink_fraction(5.0), 1.0);
        let mut prev = 0.0;
        for i in 0..=40 {
            let f = sink_fraction(i as f32 / 40.0);
            assert!(f >= prev, "at {i}");
            prev = f;
        }
        assert!(sink_fraction(0.5) < 0.5, "slow at first");
    }

    #[test]
    fn a_body_sinks_from_where_it_lay_to_its_depth() {
        assert_eq!(sink_y(10.0, 1.5, 0.0), 10.0);
        assert!((sink_y(10.0, 1.5, APPEAR_FADE_SECS) - 8.5).abs() < 1e-5);
        assert!((sink_y(10.0, 1.5, 10.0) - 8.5).abs() < 1e-5, "it stops at its depth");
        let mid = sink_y(10.0, 1.5, APPEAR_FADE_SECS * 0.5);
        assert!(mid < 10.0 && mid > 8.5);
    }

    #[test]
    fn the_big_and_the_bosses_sink_deeper_within_bounds() {
        assert!(sink_depth(1.3, false) > sink_depth(0.6, false));
        assert!(sink_depth(1.3, true) > sink_depth(1.3, false));
        assert_eq!(sink_depth(0.0, false), SINK_MIN);
        assert_eq!(sink_depth(100.0, true), SINK_MAX);
    }

    #[test]
    fn the_body_goes_before_the_fade_is_quite_over() {
        assert!(!faded(0.0));
        assert!(!faded(APPEAR_FADE_SECS * FADE_CUT - 0.01));
        assert!(faded(APPEAR_FADE_SECS * FADE_CUT));
        assert!(faded(APPEAR_FADE_SECS));
    }
}
