//! WoW pickups: a Hearthstone shard pulls every gem in, Divine Intervention clears the screen,
//! Frost Nova freezes the horde. They drop rarely from the dead and work when the hero walks over
//! them. This module also owns the [`Beacons`] every ground drop (items, chests, shards) is lit
//! with: a tall, faint beam and a glowing orb in one colour.

use benilla_world::schedule::WorldStage;
use bevy::prelude::*;

use super::abilities::Hit;
use super::chatlog::ChatLine;
use super::enemies::{Dying, Elite, Enemy, Gem};
use super::finale::Reaper;
use super::hero::Hero;
use super::items::drop_boost;
use super::units::{flat, Fx, Marks};
use super::{Phase, Rng, Run, RunEntity};

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

const BEAM_RADIUS: f32 = 0.14;
const ORB_RADIUS: f32 = 0.3;
const BEAM_ALPHA: f32 = 0.28;
const ORB_ALPHA: f32 = 0.9;
/// How high the orb floats over the ground.
const ORB_LIFT: f32 = 0.7;

pub(super) fn plugin(app: &mut App) {
    app.init_resource::<Beacons>().add_systems(
        Update,
        (drop_shards, collect_shards)
            .chain()
            .after(WorldStage::Input)
            .run_if(in_state(Phase::Playing))
            .run_if(|l: Res<crate::loading_screen::LoadingScreen>| !l.covering()),
    );
}

/// The meshes and materials the beacons share: one beam and one orb mesh, a material per colour.
#[derive(Resource, Default)]
pub(super) struct Beacons {
    meshes: Option<(Handle<Mesh>, Handle<Mesh>)>,
    /// Colour as bytes, whether it is the beam's (faint) material, and the material.
    materials: Vec<([u8; 3], bool, Handle<StandardMaterial>)>,
}

impl Beacons {
    /// Light `parent` with a beam `height` yards tall and an orb, both `color`.
    pub fn attach(
        &mut self,
        commands: &mut Commands,
        meshes: &mut Assets<Mesh>,
        materials: &mut Assets<StandardMaterial>,
        parent: Entity,
        color: Color,
        height: f32,
    ) {
        let (beam, orb) = self
            .meshes
            .get_or_insert_with(|| {
                (
                    meshes.add(Cylinder::new(BEAM_RADIUS, 1.0)),
                    meshes.add(Sphere::new(ORB_RADIUS)),
                )
            })
            .clone();
        let glass = self.material(materials, color, true);
        let solid = self.material(materials, color, false);
        commands.entity(parent).with_children(|p| {
            p.spawn((
                Mesh3d(beam),
                MeshMaterial3d(glass),
                Transform::from_xyz(0.0, height * 0.5, 0.0).with_scale(Vec3::new(1.0, height, 1.0)),
            ));
            p.spawn((
                Mesh3d(orb),
                MeshMaterial3d(solid),
                Transform::from_xyz(0.0, ORB_LIFT, 0.0),
            ));
        });
    }

    fn material(
        &mut self,
        materials: &mut Assets<StandardMaterial>,
        color: Color,
        glass: bool,
    ) -> Handle<StandardMaterial> {
        let rgba = color.to_srgba();
        let key = [rgba.red, rgba.green, rgba.blue].map(|c| (c * 255.0).round() as u8);
        if let Some((_, _, h)) = self
            .materials
            .iter()
            .find(|(k, g, _)| *k == key && *g == glass)
        {
            return h.clone();
        }
        let alpha = if glass { BEAM_ALPHA } else { ORB_ALPHA };
        let h = materials.add(StandardMaterial {
            base_color: Color::srgba(rgba.red, rgba.green, rgba.blue, alpha),
            unlit: true,
            alpha_mode: AlphaMode::Blend,
            cull_mode: None,
            fog_enabled: false,
            ..default()
        });
        self.materials.push((key, glass, h.clone()));
        h
    }
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

    fn color(self) -> Color {
        match self {
            Kind::Hearthstone => Color::srgb(0.35, 0.6, 1.0),
            Kind::Intervention => Color::srgb(1.0, 1.0, 0.8),
            Kind::Nova => Color::srgb(0.55, 0.9, 1.0),
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
#[allow(clippy::too_many_arguments)]
fn drop_shards(
    mut commands: Commands,
    mut rng: ResMut<Rng>,
    mut beacons: ResMut<Beacons>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
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
        let e = commands
            .spawn((
                Shard {
                    kind,
                    life: SHARD_LIFE,
                },
                Transform::from_translation(at),
                Visibility::default(),
                RunEntity,
            ))
            .id();
        beacons.attach(
            &mut commands,
            &mut meshes,
            &mut materials,
            e,
            kind.color(),
            6.0,
        );
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
    mut shards: Query<(Entity, &Transform, &mut Shard)>,
    mut gems: Query<&mut Gem>,
    mut foes: Query<(Entity, &Transform, &mut Enemy, &mut Marks, Has<Reaper>)>,
    mut hits: MessageWriter<Hit>,
    mut fx: Fx,
    mut chat: MessageWriter<ChatLine>,
) {
    let Ok((hero_e, htf)) = hero.single() else {
        return;
    };
    if run.dead_t.is_some() {
        return;
    }
    let hpos = htf.translation;
    let dt = time.delta_secs();
    for (e, tf, mut shard) in &mut shards {
        shard.life -= dt;
        if shard.life <= 0.0 {
            commands.entity(e).despawn();
            continue;
        }
        if flat(tf.translation - hpos).length() > PICKUP_RADIUS {
            continue;
        }
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
                    hits.write(Hit::direct(foe, damage, false));
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
        run.banner = Some((format!("{}!", kind.name()), 2.5));
        chat.write(ChatLine::system(format!(
            "You use {}. {}",
            kind.name(),
            kind.effect()
        )));
    }
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
