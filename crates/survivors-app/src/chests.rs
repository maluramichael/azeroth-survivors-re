//! Chests from elites and bosses: opened on pickup, they rank up one, three or five abilities and
//! talents at once. A normal elite's chest holds one rank (now and then three), a boss's three
//! (now and then five); a chest with nothing left to rank up pays gold and a full heal instead.

use benilla_world::schedule::WorldStage;
use benilla_world::world_point::WorldPoint;
use bevy::prelude::*;

use super::chatlog::{ChatKind, ChatLine};
use super::data;
use super::enemies::{Dying, Elite, Enemy};
use super::finale::Finale;
use super::hero::{Hero, Peer};
use super::items::drop_boost;
use super::pickups::Beacons;
use super::units::{self, flat, Fx};
use super::{GuestMail, HeroState, Mail, Rng, Run};

/// `GameObjectDisplayInfo` 259, `TreasureChest01.mdx`.
const CHEST_DISPLAY: u32 = 259;
const CHEST_SCALE: f32 = 1.0;
const CHEST_COLOR: Color = Color::srgb(1.0, 0.8, 0.2);
const BEAM_HEIGHT: f32 = 6.0;
/// Seconds a chest waits.
const LIFE: f32 = 120.0;
/// How near the hero must walk, in yards.
const OPEN_RADIUS: f32 = 2.2;
/// A chest lands at least this far from the hero, so one dropped at its feet is not opened by
/// accident (upstream issue #7).
const CLEAR_OF_HERO: f32 = 5.0;
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
            .run_if(super::run_on)
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

/// Where a chest dropped at `at` lands: there, or pushed out along the line from the hero to
/// [`CLEAR_OF_HERO`] yards.
fn clear_of(hero: Vec3, at: Vec3) -> Vec3 {
    let d = flat(at - hero);
    let len = d.length();
    if len >= CLEAR_OF_HERO {
        return at;
    }
    let dir = if len > 0.01 { d / len } else { Vec3::X };
    let p = hero + dir * CLEAR_OF_HERO;
    Vec3::new(p.x, at.y, p.z)
}

/// Elites and bosses leave a chest where they fall, but never at the hero's feet.
#[allow(clippy::too_many_arguments)]
fn drop_chests(
    mut commands: Commands,
    mut run: ResMut<Run>,
    mut rng: ResMut<Rng>,
    mut beacons: ResMut<Beacons>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    hero: Query<&Transform, (With<Hero>, Without<Enemy>)>,
    world: WorldPoint,
    dead: Query<(&Transform, &Enemy, Has<Elite>, Has<Finale>), Added<Dying>>,
) {
    let hpos = hero.single().map(|h| h.translation).ok();
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
        let mut at = tf.translation;
        if let Some(h) = hpos {
            at = clear_of(h, at);
            at.y = world.terrain_height_under(at).unwrap_or(at.y);
        }
        let e = units::spawn_object(&mut commands, run.guid(), CHEST_DISPLAY, CHEST_SCALE, at);
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

/// Walking over a chest opens it: for whichever standing hero reaches it, a co-op guest's too.
fn open_chests(
    mut commands: Commands,
    time: Res<Time>,
    mut run: ResMut<Run>,
    mut rng: ResMut<Rng>,
    hero: Query<(Entity, &Transform), With<Hero>>,
    mut peers: Query<(Entity, &Transform, &mut HeroState), (With<Peer>, Without<Hero>)>,
    mut chests: Query<(Entity, &Transform, &mut Chest), (Without<Hero>, Without<Peer>)>,
    spells: Option<Res<benilla_app::survivors_host::ui_action::Spells>>,
    mut fx: Fx,
    mut chat: MessageWriter<ChatLine>,
    mut mail: ResMut<GuestMail>,
) {
    let Ok((hero_e, htf)) = hero.single() else {
        return;
    };
    if run.dead_t.is_some() && peers.iter().all(|(_, _, s)| s.dead_t.is_some()) {
        return;
    }
    let dt = time.delta_secs();
    for (e, tf, mut chest) in &mut chests {
        chest.life -= dt;
        if chest.life <= 0.0 {
            commands.entity(e).despawn();
            continue;
        }
        let near = |at: Vec3| flat(tf.translation - at).length() <= OPEN_RADIUS;
        let opened = if run.dead_t.is_none() && near(htf.translation) {
            let opened = open(&mut run.me, chest.ranks, &mut rng, spells.as_deref());
            if let Some(gold) = opened.gold {
                chat.write(ChatLine::Gold(gold));
            }
            let secs = if opened.gold.is_some() { 2.5 } else { 3.0 };
            run.banner = Some((opened.banner.clone(), secs));
            chat.write(ChatLine::text(ChatKind::System, opened.line.clone()));
            Some(hero_e)
        } else if let Some((p, _, mut state)) = peers
            .iter_mut()
            .find(|(_, ptf, s)| s.dead_t.is_none() && near(ptf.translation))
        {
            let opened = open(&mut state, chest.ranks, &mut rng, spells.as_deref());
            if let Some(gold) = opened.gold {
                mail.0.push((p, Mail::Gold(gold)));
            }
            mail.0.push((p, Mail::Banner(opened.banner)));
            mail.0.push((p, Mail::Line(opened.line)));
            Some(p)
        } else {
            None
        };
        if let Some(by) = opened {
            commands.entity(e).despawn();
            fx.impact(by, SPELL_OPEN);
        }
    }
}

/// What a chest gave, in words.
struct Opened {
    banner: String,
    line: String,
    /// The consolation, when nothing was left to rank up.
    gold: Option<u32>,
}

/// `hero` opens a chest of `ranks`: that many ranks drawn among its spells and passives, or gold
/// and a full heal when everything is at its top.
fn open(
    hero: &mut HeroState,
    ranks: usize,
    rng: &mut Rng,
    spells: Option<&benilla_app::survivors_host::ui_action::Spells>,
) -> Opened {
    let abilities: Vec<u8> = hero.abilities.iter().map(|o| o.level).collect();
    let passives: Vec<u8> = hero.passives.iter().map(|&(_, l)| l).collect();
    let drawn = draw(candidates(&abilities, &passives), ranks, |n| rng.index(n));
    if drawn.is_empty() {
        hero.gold += CONSOLATION_GOLD;
        hero.hp = hero.stats().max_hp;
        return Opened {
            banner: "Chest opened".into(),
            line: "You open the chest. Everything is at its best already; you are made whole."
                .into(),
            gold: Some(CONSOLATION_GOLD),
        };
    }
    let mut names = Vec::with_capacity(drawn.len());
    for t in drawn {
        match t {
            Target::Ability(i) => {
                let o = &mut hero.abilities[i];
                o.level += 1;
                names.push(super::hud::spell_name(spells, o.def.spell, o.def.key));
            }
            Target::Passive(i) => {
                let (p, level) = &mut hero.passives[i];
                *level += 1;
                names.push(p.name.to_string());
            }
        }
    }
    // The HUD's icon rows rebuild on a new version; the chat window prints each new rank.
    hero.version += 1;
    Opened {
        banner: banner(&names),
        line: format!("You open the chest: {}.", names.join(", ")),
        gold: None,
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


/// The chest systems, for the balance simulator ([`super::balance`]).
#[cfg(test)]
pub(super) fn sim_systems() -> bevy::ecs::schedule::ScheduleConfigs<bevy::ecs::system::ScheduleSystem> {
    (drop_chests, open_chests).chain().into_configs()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_chest_lands_clear_of_the_hero() {
        let hero = Vec3::new(10.0, 3.0, 10.0);
        let near = clear_of(hero, Vec3::new(11.0, 2.0, 10.0));
        assert!((flat(near - hero).length() - CLEAR_OF_HERO).abs() < 1e-4);
        assert_eq!(near.y, 2.0);
        let on_top = clear_of(hero, hero);
        assert!((flat(on_top - hero).length() - CLEAR_OF_HERO).abs() < 1e-4);
        let far = Vec3::new(30.0, 0.0, 10.0);
        assert_eq!(clear_of(hero, far), far);
    }

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
