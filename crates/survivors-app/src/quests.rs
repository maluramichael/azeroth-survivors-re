//! Quests per zone, tracked under the minimap: kill, collect and survive objectives with gold and
//! experience rewards.
//!
//! Three quests are rolled when a run starts, from the zone's roster: slay some of one creature,
//! gather quest items that another drops now and then (a wisp that lies where it fell and is
//! picked up by walking over it), and outlast a number of minutes. The tracker is WoW's
//! `QuestWatchFrame`: titles in gold, objectives under them, a finished objective in grey.
//! A finished quest pays gold and experience at once; the experience goes into [`Run::xp`], where
//! the gem pass turns it into level-ups as for any other experience.

use bevy::prelude::*;

use super::chatlog::{ChatKind, ChatLine};
use super::data::{self, EnemyDef};
use super::enemies::Loot;
use super::hero::{Hero, Peer};
use super::hud::{HudFont, GOLD_TONE};
use super::units::{self, flat};
use super::worldmap::MapPin;
use super::{GuestMail, HeroState, Mail, Phase, Rng, Run, Selection};

const QUESTS: usize = 3;
/// Chance a kill of the quest's creature drops one quest item.
const DROP_CHANCE: f32 = 0.45;
/// Quest items on the ground at once.
const MAX_ITEMS: usize = 24;
/// Seconds an item waits to be picked up.
const LIFETIME: f32 = 75.0;
/// How near the hero picks an item up (yd).
const PICKUP: f32 = 1.8;
/// `GameObjectDisplayInfo` row and scale of a quest item: the green wisp, larger than an
/// experience wisp and left animated, so it reads as something else.
const ITEM_DISPLAY: u32 = 1267;
const ITEM_SCALE: f32 = 1.7;
/// What an item is called after its creature.
const PARTS: [&str; 7] = ["Claw", "Fang", "Hide", "Feather", "Eye", "Tooth", "Heart"];

/// Right under the minimap cluster (192 units tall at the top right).
const TOP: f32 = 204.0;
const RIGHT: f32 = 14.0;
const WIDTH: f32 = 200.0;
const Z: i32 = 401;
const WHITE: Color = Color::WHITE;
const DONE: Color = Color::srgb(0.6, 0.6, 0.6);

pub(super) fn plugin(app: &mut App) {
    app.init_resource::<QuestLog>()
        .init_resource::<Tracker>()
        .add_message::<benilla_app::survivors_host::sound::GlueSound>()
        .add_systems(
            Update,
            (
                // A co-op guest's quests come with stage three; the host's are the host's.
                roll_quests.run_if(playing).run_if(super::simulates),
                count_kills,
                pick_up.run_if(playing),
                settle.run_if(playing),
                show,
            )
                .chain()
                .run_if(in_run),
        )
        .add_systems(OnEnter(Phase::Menu), reset)
        .add_systems(OnEnter(Phase::GameOver), teardown);
}

/// The phases that put the HUD on screen.
fn in_run(phase: Res<State<Phase>>) -> bool {
    matches!(phase.get(), Phase::Playing | Phase::LevelUp | Phase::Paused)
}

fn playing(phase: Res<State<Phase>>) -> bool {
    *phase.get() == Phase::Playing
}

#[derive(Clone, Debug, PartialEq)]
enum Goal {
    /// Kill this many of a roster creature.
    Kill { creature: &'static str },
    /// Pick up this many of an item the creature drops.
    Collect {
        creature: &'static str,
        item: String,
    },
    /// Live this many seconds.
    Survive,
}

#[derive(Clone, Debug, PartialEq)]
struct Quest {
    title: String,
    goal: Goal,
    /// Kills, items or seconds.
    need: u32,
    have: u32,
    done: bool,
    gold: u32,
    /// The experience, as a share of what the hero's level needs when the quest is handed in.
    xp: f32,
}

impl Quest {
    fn add(&mut self, n: u32) {
        self.have = (self.have + n).min(self.need);
    }

    fn finished(&self) -> bool {
        self.have >= self.need
    }

    /// The objective line, as `QuestWatchFrame` words them.
    fn objective(&self) -> String {
        match &self.goal {
            Goal::Kill { creature } => format!("{creature} slain: {}/{}", self.have, self.need),
            Goal::Collect { item, .. } => format!("{item}: {}/{}", self.have, self.need),
            Goal::Survive => format!("Survive: {}/{}", clock(self.have), clock(self.need)),
        }
    }
}

fn clock(secs: u32) -> String {
    format!("{}:{:02}", secs / 60, secs % 60)
}

fn plural(name: &str) -> String {
    if name.ends_with('s') {
        name.to_string()
    } else {
        format!("{name}s")
    }
}

#[derive(Resource, Default)]
pub(crate) struct QuestLog {
    quests: Vec<Quest>,
    /// This run's quests are rolled.
    rolled: bool,
    /// A co-op guest's view of its host's quests ([`QuestLine`]); the party shares them.
    pub(crate) shown: Vec<QuestLine>,
}

/// One quest as the tracker shows it.
#[derive(Clone, Debug, Default, PartialEq, serde::Serialize, serde::Deserialize)]
pub(crate) struct QuestLine {
    pub title: String,
    pub objective: String,
    pub done: bool,
}

impl QuestLog {
    /// The tracker's lines: this machine's quests, or the host's as a guest sees them.
    pub(crate) fn lines(&self) -> Vec<QuestLine> {
        if self.quests.is_empty() {
            return self.shown.clone();
        }
        self.quests
            .iter()
            .map(|q| QuestLine {
                title: q.title.clone(),
                objective: q.objective(),
                done: q.done,
            })
            .collect()
    }
}

/// A quest item lying on the ground.
#[derive(Component)]
struct QuestItem {
    /// An index into [`QuestLog::quests`].
    quest: usize,
    age: f32,
}

/// A creature from the roster's `lo..hi` share, a regular one (the great beasts take too long to
/// meet), not `skip`.
fn creature(
    enemies: &'static [EnemyDef],
    lo: f32,
    hi: f32,
    skip: Option<&str>,
    rng: &mut Rng,
) -> &'static EnemyDef {
    let n = enemies.len() as f32;
    let (a, b) = (
        (n * lo) as usize,
        ((n * hi).ceil() as usize).min(enemies.len()),
    );
    let fit = |e: &&EnemyDef| !e.big && Some(e.name) != skip;
    let mut pool: Vec<&EnemyDef> = enemies[a.min(b)..b].iter().filter(fit).collect();
    if pool.is_empty() {
        pool = enemies.iter().filter(fit).collect();
    }
    match pool.len() {
        0 => &enemies[0],
        len => pool[rng.index(len)],
    }
}

/// The run's three quests on `map`'s roster: a slaying early in the roster, a gathering a little
/// later, and a survival.
fn roll(map: usize, rng: &mut Rng) -> Vec<Quest> {
    let enemies = &data::roster(map).enemies;
    let slay = creature(enemies, 0.12, 0.3, None, rng);
    let gather = creature(enemies, 0.3, 0.5, Some(slay.name), rng);
    let part = PARTS[rng.index(PARTS.len())];
    let item = format!("{} {part}", gather.name);
    let minutes = 4;
    vec![
        Quest {
            title: format!("Cull the {}", plural(slay.name)),
            goal: Goal::Kill {
                creature: slay.name,
            },
            need: 10 + rng.index(6) as u32,
            have: 0,
            done: false,
            gold: 10 + slay.level * 2,
            xp: 0.5,
        },
        Quest {
            title: format!("{} Wanted", plural(&item)),
            goal: Goal::Collect {
                creature: gather.name,
                item,
            },
            need: 5 + rng.index(3) as u32,
            have: 0,
            done: false,
            gold: 12 + gather.level * 3,
            xp: 0.6,
        },
        Quest {
            title: "Hold the Line".into(),
            goal: Goal::Survive,
            need: minutes * 60,
            have: 0,
            done: false,
            gold: 20 + gather.level * 4,
            xp: 0.9,
        },
    ]
}

/// The first frame of a run: draw the quests.
fn roll_quests(mut log: ResMut<QuestLog>, selection: Res<Selection>, mut rng: ResMut<Rng>) {
    if log.rolled {
        return;
    }
    log.rolled = true;
    log.quests = roll(selection.map, &mut rng);
}

/// Kills of a quest's creature count, and a collecting quest's creature may drop its item where
/// it fell.
fn count_kills(
    mut commands: Commands,
    mut drops: MessageReader<Loot>,
    mut log: ResMut<QuestLog>,
    mut run: ResMut<Run>,
    mut rng: ResMut<Rng>,
    items: Query<(), With<QuestItem>>,
) {
    let mut on_ground = items.iter().count();
    for d in drops.read() {
        for (i, q) in log.quests.iter_mut().enumerate() {
            if q.done {
                continue;
            }
            let (kill, drop) = match &q.goal {
                Goal::Kill { creature } => (*creature == d.def.name, false),
                Goal::Collect { creature, .. } => (false, *creature == d.def.name),
                Goal::Survive => (false, false),
            };
            if kill {
                q.add(1);
            }
            if drop && on_ground < MAX_ITEMS && rng.f32() < DROP_CHANCE {
                on_ground += 1;
                let a = rng.range(0.0, std::f32::consts::TAU);
                let at = d.pos + Vec3::new(a.cos(), 0.0, a.sin()) * 0.6 + Vec3::Y * 0.5;
                let serial = run.guid();
                let e = units::spawn_object(&mut commands, serial, ITEM_DISPLAY, ITEM_SCALE, at);
                commands
                    .entity(e)
                    .insert((QuestItem { quest: i, age: 0.0 }, MapPin::QuestItem));
            }
        }
    }
}

/// Walking over an item picks it up; one whose quest is done, or that has lain too long, goes.
fn pick_up(
    mut commands: Commands,
    time: Res<Time>,
    mut log: ResMut<QuestLog>,
    hero: Query<&Transform, (With<Hero>, Without<QuestItem>)>,
    peers: Query<&Transform, (With<Peer>, Without<Hero>, Without<QuestItem>)>,
    mut items: Query<(Entity, &Transform, &mut QuestItem)>,
    mut chat: MessageWriter<ChatLine>,
) {
    let Ok(h) = hero.single() else { return };
    // The party's quests: any hero's walk picks an item up.
    let walkers: Vec<Vec3> = std::iter::once(h.translation)
        .chain(peers.iter().map(|t| t.translation))
        .collect();
    let dt = time.delta_secs();
    for (e, tf, mut item) in &mut items {
        item.age += dt;
        let Some(q) = log.quests.get_mut(item.quest) else {
            commands.entity(e).despawn();
            continue;
        };
        if q.done || item.age > LIFETIME {
            commands.entity(e).despawn();
            continue;
        }
        if walkers
            .iter()
            .all(|w| flat(tf.translation - *w).length() > PICKUP)
        {
            continue;
        }
        q.add(1);
        chat.write(ChatLine::text(ChatKind::System, q.objective()));
        commands.entity(e).despawn();
    }
}

/// Time passes for the survival quest, and a finished quest is handed in.
fn settle(
    mut log: ResMut<QuestLog>,
    mut run: ResMut<Run>,
    mut peers: Query<(Entity, &mut HeroState), With<Peer>>,
    mut chat: MessageWriter<ChatLine>,
    mut sound: MessageWriter<benilla_app::survivors_host::sound::GlueSound>,
    mut mail: ResMut<GuestMail>,
) {
    if run.party_down {
        return;
    }
    let secs = run.elapsed as u32;
    for q in &mut log.quests {
        if q.goal == Goal::Survive {
            q.have = secs.min(q.need);
        }
        if q.done || !q.finished() {
            continue;
        }
        q.done = true;
        // Counted as experience from a gem is, so the gem pass raises the levels.
        let xp = data::xp_to_next(run.level) * q.xp * run.stats().growth;
        run.xp += xp;
        run.gold += q.gold;
        // Every hero of the party is paid in full.
        for (p, mut hero) in &mut peers {
            hero.gold += q.gold;
            mail.0.push((p, Mail::Line(format!("Quest completed: {}.", q.title))));
            mail.0.push((p, Mail::Gold(q.gold)));
        }
        chat.write(ChatLine::system(format!("Quest completed: {}.", q.title)));
        chat.write(ChatLine::Xp(xp));
        chat.write(ChatLine::Gold(q.gold));
        sound.write(benilla_app::survivors_host::sound::GlueSound("QUESTCOMPLETED"));
    }
}

/// The tracker's nodes, once built.
#[derive(Resource, Default)]
struct Tracker {
    root: Option<Entity>,
    /// Per quest: its block, its title and its objective.
    slots: Vec<(Entity, Entity, Entity)>,
}

/// Back at the menu: the quests go with the run.
fn reset(mut commands: Commands, mut log: ResMut<QuestLog>, mut ui: ResMut<Tracker>) {
    *log = QuestLog::default();
    teardown_ui(&mut commands, &mut ui);
}

/// The results: the tracker goes, the log waits for the menu.
fn teardown(mut commands: Commands, mut ui: ResMut<Tracker>) {
    teardown_ui(&mut commands, &mut ui);
}

fn teardown_ui(commands: &mut Commands, ui: &mut Tracker) {
    if let Some(root) = ui.root.take() {
        commands.entity(root).try_despawn();
    }
    ui.slots.clear();
}

fn build(commands: &mut Commands, ui: &mut Tracker, font: &Handle<Font>) {
    let text = |size: f32, color: Color| {
        (
            Text::new(""),
            TextFont {
                font: font.clone(),
                font_size: size,
                ..default()
            },
            TextColor(color),
            TextShadow {
                offset: Vec2::splat(1.0),
                color: Color::BLACK,
            },
        )
    };
    let mut slots = Vec::with_capacity(QUESTS);
    let root = commands
        .spawn((
            Node {
                position_type: PositionType::Absolute,
                right: Val::Px(RIGHT),
                top: Val::Px(TOP),
                width: Val::Px(WIDTH),
                flex_direction: FlexDirection::Column,
                row_gap: Val::Px(6.0),
                ..default()
            },
            GlobalZIndex(Z),
        ))
        .with_children(|col| {
            for _ in 0..QUESTS {
                let mut block = col.spawn(Node {
                    flex_direction: FlexDirection::Column,
                    display: Display::None,
                    ..default()
                });
                let id = block.id();
                let (mut title, mut goal) = (Entity::PLACEHOLDER, Entity::PLACEHOLDER);
                block.with_children(|b| {
                    title = b.spawn(text(13.0, GOLD_TONE)).id();
                    goal = b
                        .spawn((
                            text(12.0, WHITE),
                            Node {
                                margin: UiRect::left(Val::Px(8.0)),
                                ..default()
                            },
                        ))
                        .id();
                });
                slots.push((id, title, goal));
            }
        })
        .id();
    ui.root = Some(root);
    ui.slots = slots;
}

fn set<T: PartialEq>(slot: &mut T, v: T) {
    if *slot != v {
        *slot = v;
    }
}

/// Build the tracker on the first frame it can, then keep its lines current.
fn show(
    mut commands: Commands,
    mut ui: ResMut<Tracker>,
    log: Res<QuestLog>,
    font: Option<Res<HudFont>>,
    mut nodes: Query<&mut Node>,
    mut texts: Query<(&mut Text, &mut TextColor)>,
) {
    let ui = &mut *ui;
    if ui.root.is_none() {
        if let Some(font) = font {
            if !log.lines().is_empty() {
                // Seated from the next frame on, once its nodes exist.
                build(&mut commands, ui, &font.0);
            }
        }
        return;
    }
    let lines = log.lines();
    for (i, &(block, title, goal)) in ui.slots.iter().enumerate() {
        let quest = lines.get(i);
        if let Ok(mut node) = nodes.get_mut(block) {
            set(
                &mut node.display,
                if quest.is_some() {
                    Display::Flex
                } else {
                    Display::None
                },
            );
        }
        let Some(q) = quest else { continue };
        let name = if q.done {
            format!("{} (Complete)", q.title)
        } else {
            q.title.clone()
        };
        let goal_color = if q.done { DONE } else { WHITE };
        for (entity, value, color) in [(title, name, GOLD_TONE), (goal, q.objective.clone(), goal_color)]
        {
            if let Ok((mut text, mut tc)) = texts.get_mut(entity) {
                set(&mut text.0, value);
                set(&mut tc.0, color);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn progress_stops_at_the_need() {
        let mut rng = Rng::default();
        let mut q = roll(0, &mut rng).remove(0);
        assert!(!q.finished());
        q.add(q.need - 1);
        assert!(!q.finished());
        q.add(5);
        assert_eq!(q.have, q.need);
        assert!(q.finished());
    }

    #[test]
    fn every_zone_rolls_three_distinct_quests() {
        let mut rng = Rng::default();
        for map in 0..data::MAPS.len() {
            let quests = roll(map, &mut rng);
            assert_eq!(quests.len(), QUESTS, "map {map}");
            let (Goal::Kill { creature: a }, Goal::Collect { creature: b, .. }) =
                (&quests[0].goal, &quests[1].goal)
            else {
                panic!("map {map}: wrong goals");
            };
            let roster = &data::roster(map).enemies;
            assert!(roster.iter().any(|e| e.name == *a), "map {map}");
            assert!(roster.iter().any(|e| e.name == *b), "map {map}");
            if roster.iter().filter(|e| !e.big).count() > 1 {
                assert_ne!(a, b, "map {map}");
            }
            assert_eq!(quests[2].goal, Goal::Survive);
            assert!(quests.iter().all(|q| q.need > 0 && q.gold > 0));
        }
    }

    #[test]
    fn objectives_read_as_the_tracker_words_them() {
        let kill = Quest {
            title: String::new(),
            goal: Goal::Kill {
                creature: "Savannah Prowler",
            },
            need: 10,
            have: 4,
            done: false,
            gold: 1,
            xp: 0.5,
        };
        assert_eq!(kill.objective(), "Savannah Prowler slain: 4/10");
        let survive = Quest {
            goal: Goal::Survive,
            need: 240,
            have: 133,
            ..kill.clone()
        };
        assert_eq!(survive.objective(), "Survive: 2:13/4:00");
        let collect = Quest {
            goal: Goal::Collect {
                creature: "Savannah Prowler",
                item: "Savannah Prowler Claw".into(),
            },
            need: 6,
            have: 0,
            ..kill
        };
        assert_eq!(collect.objective(), "Savannah Prowler Claw: 0/6");
        assert_eq!(plural("Savannah Prowler Claw"), "Savannah Prowler Claws");
    }
}
