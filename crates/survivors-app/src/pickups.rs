//! WoW pickups: a Hearthstone shard pulls every gem in, Divine Intervention clears the screen,
//! Frost Nova freezes the horde. They drop rarely from the dead and work when the hero walks over
//! them. How a shard looks on the ground is [`super::dropart`]'s.

use benilla_world::schedule::WorldStage;
use bevy::prelude::*;

use super::abilities::Hit;
use super::chatlog::ChatLine;
use super::dropart::{DropArt, ShardArt};
use super::enemies::{Dying, Elite, Enemy, Gem};
use super::finale::Reaper;
use super::hero::{Hero, Peer};
use super::items::drop_boost;
use super::units::{flat, Fx, Marks};
use super::{GuestMail, HeroState, Mail, Rng, Run, RunEntity};

/// Shards lying on the ground at once; past it the dead drop none.
const MAX_SHARDS: usize = 4;
/// Seconds a shard waits to be picked up.
const SHARD_LIFE: f32 = 60.0;
/// How near the hero must walk, in yards.
const PICKUP_RADIUS: f32 = 2.0;
/// What the screen holds, in yards from the hero: Divine Intervention and Frost Nova reach this far.
const SCREEN_RADIUS: f32 = 42.0;
/// Seconds Frost Nova holds the horde.
const FREEZE_SECS: f32 = 5.0;
/// A boss is slowed to this speed by Frost Nova instead, and loses this share of its health to
/// Divine Intervention.
const BOSS_SLOW: f32 = 0.4;
const BOSS_BLAST: f32 = 0.1;
/// Spells whose visuals play: Holy Nova's ring, Frost Nova's ring and the Hearthstone's swirl.
const SPELL_INTERVENTION: u32 = 15237;
const SPELL_NOVA: u32 = 122;
const SPELL_HEARTHSTONE: u32 = 8690;

/// Chances a normal kill drops each, in a row of dice; an elite rolls [`ELITE_LUCK`] times better
/// and a boss always drops one.
const CHANCE_HEARTHSTONE: f32 = 0.0025;
const CHANCE_INTERVENTION: f32 = 0.001;
const CHANCE_NOVA: f32 = 0.002;
const ELITE_LUCK: f32 = 8.0;

pub(super) fn plugin(app: &mut App) {
    app.add_systems(
        Update,
        (drop_shards, collect_shards)
            .chain()
            .after(WorldStage::Input)
            .run_if(super::run_on)
            .run_if(super::simulates)
            .run_if(|l: Res<benilla_app::survivors_host::loading_screen::LoadingScreen>| !l.covering()),
    );
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Kind {
    Hearthstone,
    Intervention,
    Nova,
}

impl Kind {
    const ALL: [Kind; 3] = [Kind::Hearthstone, Kind::Intervention, Kind::Nova];

    fn name(self) -> &'static str {
        match self {
            Kind::Hearthstone => "Hearthstone Shard",
            Kind::Intervention => "Divine Intervention",
            Kind::Nova => "Frost Nova",
        }
    }

    /// How the shard looks on the ground ([`super::dropart`]).
    fn art(self) -> ShardArt {
        match self {
            Kind::Hearthstone => ShardArt::Hearthstone,
            Kind::Intervention => ShardArt::Intervention,
            Kind::Nova => ShardArt::Nova,
        }
    }

    fn effect(self) -> &'static str {
        match self {
            Kind::Hearthstone => "Every gem on the map flies to you.",
            Kind::Intervention => "The light strikes down everything near.",
            Kind::Nova => "The horde freezes.",
        }
    }
}

/// What a roll of the dice (0 to 1, lower is luckier) drops from a normal kill.
fn roll(r: f32) -> Option<Kind> {
    let mut at = CHANCE_HEARTHSTONE;
    if r < at {
        return Some(Kind::Hearthstone);
    }
    at += CHANCE_INTERVENTION;
    if r < at {
        return Some(Kind::Intervention);
    }
    at += CHANCE_NOVA;
    (r < at).then_some(Kind::Nova)
}

/// A shard on the ground.
#[derive(Component)]
struct Shard {
    kind: Kind,
    life: f32,
}

/// The dead drop shards: rarely a normal kill, more often an elite, always a boss.
fn drop_shards(
    mut commands: Commands,
    mut rng: ResMut<Rng>,
    dead: Query<(&Transform, &Enemy, Has<Elite>), Added<Dying>>,
    shards: Query<(), With<Shard>>,
) {
    let mut on_ground = shards.iter().count();
    for (tf, enemy, elite) in &dead {
        if on_ground >= MAX_SHARDS {
            break;
        }
        let luck = drop_boost() * if elite { ELITE_LUCK } else { 1.0 };
        let kind = if enemy.boss {
            Some(Kind::ALL[rng.index(Kind::ALL.len())])
        } else {
            roll(rng.f32() / luck)
        };
        let Some(kind) = kind else { continue };
        let a = rng.range(0.0, std::f32::consts::TAU);
        let at = tf.translation + Vec3::new(a.cos(), 0.0, a.sin()) * 1.2;
        commands.spawn((
            Shard {
                kind,
                life: SHARD_LIFE,
            },
            DropArt::Shard(kind.art()),
            Transform::from_translation(at),
            Visibility::default(),
            RunEntity,
        ));
        on_ground += 1;
    }
}

/// Walking over a shard uses it.
#[allow(clippy::too_many_arguments)]
fn collect_shards(
    mut commands: Commands,
    time: Res<Time>,
    mut run: ResMut<Run>,
    hero: Query<(Entity, &Transform), With<Hero>>,
    peers: Query<(Entity, &Transform, &HeroState), (With<Peer>, Without<Hero>)>,
    mut shards: Query<(Entity, &Transform, &mut Shard), (Without<Hero>, Without<Peer>)>,
    mut gems: Query<&mut Gem>,
    mut foes: Query<
        (Entity, &Transform, &mut Enemy, &mut Marks, Has<Reaper>),
        (Without<Hero>, Without<Peer>, Without<Shard>),
    >,
    mut hits: MessageWriter<Hit>,
    mut fx: Fx,
    mut chat: MessageWriter<ChatLine>,
    mut mail: ResMut<GuestMail>,
) {
    let Ok((local_e, local_tf)) = hero.single() else {
        return;
    };
    // Every standing hero may take a shard, a co-op guest's too; it works around that hero.
    let mut takers: Vec<(Entity, Vec3, bool)> = Vec::new();
    if run.dead_t.is_none() {
        takers.push((local_e, local_tf.translation, true));
    }
    takers.extend(
        peers
            .iter()
            .filter(|(_, _, s)| s.dead_t.is_none())
            .map(|(p, tf, _)| (p, tf.translation, false)),
    );
    if takers.is_empty() {
        return;
    }
    let dt = time.delta_secs();
    for (e, tf, mut shard) in &mut shards {
        shard.life -= dt;
        if shard.life <= 0.0 {
            commands.entity(e).despawn();
            continue;
        }
        let Some(&(hero_e, hpos, local)) = takers
            .iter()
            .find(|(_, at, _)| flat(tf.translation - *at).length() <= PICKUP_RADIUS)
        else {
            continue;
        };
        let by = (!local).then_some(hero_e);
        commands.entity(e).despawn();
        let kind = shard.kind;
        let mut reached: Vec<Entity> = Vec::new();
        match kind {
            Kind::Hearthstone => {
                for mut gem in &mut gems {
                    gem.pulled = true;
                }
                fx.cast(hero_e, SPELL_HEARTHSTONE, &[hero_e], None);
            }
            Kind::Intervention => {
                for (foe, ftf, enemy, _, reaper) in &foes {
                    if enemy.dead || reaper || flat(ftf.translation - hpos).length() > SCREEN_RADIUS
                    {
                        continue;
                    }
                    // Everything but a boss falls; a boss is only hurt.
                    let damage = if enemy.boss {
                        enemy.max_hp * BOSS_BLAST
                    } else {
                        enemy.hp + 1.0
                    };
                    hits.write(Hit {
                        by,
                        ..Hit::direct(foe, damage, false)
                    });
                    reached.push(foe);
                }
                fx.cast(hero_e, SPELL_INTERVENTION, &reached, None);
            }
            Kind::Nova => {
                for (foe, ftf, mut enemy, mut marks, reaper) in &mut foes {
                    if enemy.dead || reaper || flat(ftf.translation - hpos).length() > SCREEN_RADIUS
                    {
                        continue;
                    }
                    if enemy.boss {
                        enemy.slow = enemy.slow.min(BOSS_SLOW);
                        enemy.slow_t = enemy.slow_t.max(FREEZE_SECS);
                    } else {
                        enemy.stun_t = enemy.stun_t.max(FREEZE_SECS);
                    }
                    marks.add(SPELL_NOVA, FREEZE_SECS);
                    reached.push(foe);
                }
                fx.cast(hero_e, SPELL_NOVA, &reached, None);
            }
        }
        let line = format!("You use {}. {}", kind.name(), kind.effect());
        if local {
            run.banner = Some((format!("{}!", kind.name()), 2.5));
            chat.write(ChatLine::system(line));
        } else {
            mail.0.push((hero_e, Mail::Banner(format!("{}!", kind.name()))));
            mail.0.push((hero_e, Mail::Line(line)));
        }
    }
}


/// The shard systems, for the balance simulator ([`super::balance`]).
#[cfg(test)]
pub(super) fn sim_systems() -> bevy::ecs::schedule::ScheduleConfigs<bevy::ecs::system::ScheduleSystem> {
    (drop_shards, collect_shards).chain().into_configs()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dice_thresholds() {
        assert_eq!(roll(0.0), Some(Kind::Hearthstone));
        assert_eq!(roll(CHANCE_HEARTHSTONE + 0.0001), Some(Kind::Intervention));
        assert_eq!(
            roll(CHANCE_HEARTHSTONE + CHANCE_INTERVENTION + 0.0001),
            Some(Kind::Nova)
        );
        assert_eq!(roll(0.5), None);
        assert_eq!(
            roll(CHANCE_HEARTHSTONE + CHANCE_INTERVENTION + CHANCE_NOVA),
            None
        );
    }
}
