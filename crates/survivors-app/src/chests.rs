//! Chests from elites and bosses: opened on pickup, they rank up one, three or five abilities and
//! talents at once. A normal elite's chest holds one rank (now and then three), a boss's three
//! (now and then five); a chest with nothing left to rank up pays gold and a full heal instead.

use benilla_world::schedule::WorldStage;
use bevy::prelude::*;

use super::chatlog::{ChatKind, ChatLine};
use super::data;
use super::enemies::{Dying, Elite, Enemy};
use super::finale::Finale;
use super::hero::Hero;
use super::items::drop_boost;
use super::pickups::Beacons;
use super::units::{self, flat, Fx};
use super::{Phase, Rng, Run};

/// `GameObjectDisplayInfo` 259, `TreasureChest01.mdx`.
const CHEST_DISPLAY: u32 = 259;
const CHEST_SCALE: f32 = 1.0;
const CHEST_COLOR: Color = Color::srgb(1.0, 0.8, 0.2);
const BEAM_HEIGHT: f32 = 6.0;
/// Seconds a chest waits.
const LIFE: f32 = 120.0;
/// How near the hero must walk, in yards.
const OPEN_RADIUS: f32 = 2.2;
/// Chance an elite drops a chest; a boss always does.
const ELITE_CHANCE: f32 = 0.6;
/// Chance a chest holds the larger count.
const BONUS_CHANCE: f32 = 0.25;
/// What an emptied chest pays instead.
const CONSOLATION_GOLD: u32 = 40;
/// The flourish on the hero when a chest opens: Renew's sparkle.
const SPELL_OPEN: u32 = 139;

pub(super) fn plugin(app: &mut App) {
    app.add_systems(
        Update,
        (drop_chests, open_chests)
            .chain()
            .after(WorldStage::Input)
            .run_if(in_state(Phase::Playing))
            .run_if(|l: Res<benilla_app::survivors_host::loading_screen::LoadingScreen>| !l.covering()),
    );
}

/// A chest on the ground, holding this many rank-ups.
#[derive(Component)]
struct Chest {
    ranks: usize,
    life: f32,
}

/// Something a chest can rank up, by index into the run's abilities or passives.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Target {
    Ability(usize),
    Passive(usize),
}

/// Every owned ability and passive below its maximum rank.
fn candidates(abilities: &[u8], passives: &[u8]) -> Vec<Target> {
    let open = |levels: &[u8], max: u8, make: fn(usize) -> Target| {
        levels
            .iter()
            .enumerate()
            .filter(move |(_, l)| **l < max)
            .map(move |(i, _)| make(i))
            .collect::<Vec<_>>()
    };
    let mut out = open(abilities, data::MAX_ABILITY_LEVEL, Target::Ability);
    out.extend(open(passives, data::MAX_PASSIVE_LEVEL, Target::Passive));
    out
}

/// Up to `n` distinct targets drawn from `pool`; `pick(len)` gives an index below `len`.
fn draw(mut pool: Vec<Target>, n: usize, mut pick: impl FnMut(usize) -> usize) -> Vec<Target> {
    let mut out = Vec::with_capacity(n.min(pool.len()));
    while out.len() < n && !pool.is_empty() {
        out.push(pool.swap_remove(pick(pool.len())));
    }
    out
}

/// Elites and bosses leave a chest where they fall.
#[allow(clippy::too_many_arguments)]
fn drop_chests(
    mut commands: Commands,
    mut run: ResMut<Run>,
    mut rng: ResMut<Rng>,
    mut beacons: ResMut<Beacons>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    dead: Query<(&Transform, &Enemy, Has<Elite>, Has<Finale>), Added<Dying>>,
) {
    for (tf, enemy, elite, finale) in &dead {
        // The finale ends the run on the spot; there is no one left to open its chest.
        if finale || !(enemy.boss || elite) {
            continue;
        }
        if !enemy.boss && rng.f32() >= ELITE_CHANCE * drop_boost() {
            continue;
        }
        let bonus = rng.f32() < BONUS_CHANCE;
        let ranks = match (enemy.boss, bonus) {
            (false, false) => 1,
            (false, true) | (true, false) => 3,
            (true, true) => 5,
        };
        let e = units::spawn_object(
            &mut commands,
            run.guid(),
            CHEST_DISPLAY,
            CHEST_SCALE,
            tf.translation,
        );
        commands.entity(e).insert(Chest { ranks, life: LIFE });
        beacons.attach(
            &mut commands,
            &mut meshes,
            &mut materials,
            e,
            CHEST_COLOR,
            BEAM_HEIGHT,
        );
    }
}

/// Walking over a chest opens it.
#[allow(clippy::too_many_arguments)]
fn open_chests(
    mut commands: Commands,
    time: Res<Time>,
    mut run: ResMut<Run>,
    mut rng: ResMut<Rng>,
    hero: Query<(Entity, &Transform), With<Hero>>,
    mut chests: Query<(Entity, &Transform, &mut Chest)>,
    spells: Option<Res<benilla_app::survivors_host::ui_action::Spells>>,
    mut fx: Fx,
    mut chat: MessageWriter<ChatLine>,
) {
    let Ok((hero_e, htf)) = hero.single() else {
        return;
    };
    if run.dead_t.is_some() {
        return;
    }
    let dt = time.delta_secs();
    for (e, tf, mut chest) in &mut chests {
        chest.life -= dt;
        if chest.life <= 0.0 {
            commands.entity(e).despawn();
            continue;
        }
        if flat(tf.translation - htf.translation).length() > OPEN_RADIUS {
            continue;
        }
        commands.entity(e).despawn();
        fx.impact(hero_e, SPELL_OPEN);

        let abilities: Vec<u8> = run.abilities.iter().map(|o| o.level).collect();
        let passives: Vec<u8> = run.passives.iter().map(|&(_, l)| l).collect();
        let drawn = draw(candidates(&abilities, &passives), chest.ranks, |n| {
            rng.index(n)
        });
        if drawn.is_empty() {
            run.gold += CONSOLATION_GOLD;
            run.hp = run.stats().max_hp;
            chat.write(ChatLine::Gold(CONSOLATION_GOLD));
            chat.write(ChatLine::system(
                "You open the chest. Everything is at its best already; you are made whole.",
            ));
            run.banner = Some(("Chest opened".into(), 2.5));
            continue;
        }
        let mut names = Vec::with_capacity(drawn.len());
        for t in drawn {
            match t {
                Target::Ability(i) => {
                    let o = &mut run.abilities[i];
                    o.level += 1;
                    names.push(super::hud::spell_name(
                        spells.as_deref(),
                        o.def.spell,
                        o.def.key,
                    ));
                }
                Target::Passive(i) => {
                    let (p, level) = &mut run.passives[i];
                    *level += 1;
                    names.push(p.name.to_string());
                }
            }
        }
        // The HUD's icon rows rebuild on a new version; the chat window prints each new rank.
        run.version += 1;
        run.banner = Some((banner(&names), 3.0));
        chat.write(ChatLine::text(
            ChatKind::System,
            format!("You open the chest: {}.", names.join(", ")),
        ));
    }
}

/// The centre-screen text for the ranks a chest gave.
fn banner(names: &[String]) -> String {
    if names.len() <= 3 {
        format!("Chest: {} +1", names.join(", "))
    } else {
        format!("Chest: {} ranks gained", names.len())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn maxed_ranks_are_not_candidates() {
        let c = candidates(
            &[1, data::MAX_ABILITY_LEVEL, 7],
            &[data::MAX_PASSIVE_LEVEL, 2],
        );
        assert_eq!(
            c,
            vec![Target::Ability(0), Target::Ability(2), Target::Passive(1)]
        );
    }

    #[test]
    fn draw_is_distinct_and_capped_by_the_pool() {
        let pool = candidates(&[1, 2, 3], &[1, 1]);
        let mut next = 0usize;
        let three = draw(pool.clone(), 3, |n| {
            next += 1;
            next % n
        });
        assert_eq!(three.len(), 3);
        for (i, a) in three.iter().enumerate() {
            assert!(!three[i + 1..].contains(a));
        }
        assert_eq!(draw(pool, 9, |_| 0).len(), 5);
        assert!(draw(Vec::new(), 3, |_| 0).is_empty());
    }

    #[test]
    fn banner_summarises_big_hauls() {
        let n = |k: usize| vec!["x".to_string(); k];
        assert_eq!(banner(&n(1)), "Chest: x +1");
        assert_eq!(banner(&n(5)), "Chest: 5 ranks gained");
    }
}
