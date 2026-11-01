//! The quest log: zone quests that open the next battleground on top of the character's level,
//! and class quests that open a class without buying it (tickets #1104, #1105).
//!
//! Quests are derived from the profile's history ([`Profile::records`], the characters' levels),
//! never saved as progress of their own, so an old profile just works. The one thing saved is the
//! set of quests already announced in chat ([`Profile::quests_announced`]), so each is said once.
//!
//! The zones, sorted by difficulty ([`zone_min_level`], ties by index), form a chain: the zone
//! after the first is opened by a quest fought in the zone before it. Every third quest asks for a
//! win there, the others for ten minutes of survival. A class that is not a starter has a quest
//! of its own; buying it at the trainer stays possible. `unlock_all` still opens everything.
//!
//! Opened from the hub's Quest Log tab; the window follows [`QuestLogUi::open`].

use std::sync::LazyLock;

use bevy::prelude::*;
use bevy::ui::FocusPolicy;

use super::chatlog::ChatLine;
use super::hud::{self, MenuRoot};
use super::kit::{self, KitButton, KitFonts, KitStyle};
use super::menu::MenuBuilt;
use super::profile::{zone_min_level, Profile};
use super::records::Best;
use super::{data, Phase};
use benilla_app::survivors_host::glue::GlueVisuals;

/// Whether the quest log is open; the hub's tab sets it.
#[derive(Resource, Default)]
pub(crate) struct QuestLogUi {
    pub(crate) open: bool,
}

/// Seconds of survival a "survive" quest asks for.
const SURVIVE_SECS: u32 = 600;
/// Every this many zones along the chain the quest asks for a win instead of survival.
const WIN_EVERY: usize = 3;
/// The level the level quests ask for.
const QUEST_LEVEL: u32 = 10;

// --- The quests. ---

/// What a quest asks for. `None` means any zone or any class.
#[derive(Clone, Copy, Debug, PartialEq)]
enum Goal {
    /// Survive `secs` in one run (a won run survived too).
    Survive {
        map: Option<usize>,
        class: Option<usize>,
        secs: u32,
    },
    /// Win a run.
    Win {
        map: Option<usize>,
        class: Option<usize>,
    },
    /// Reach a character level.
    Level { class: Option<usize>, level: u32 },
}

/// What a finished quest opens.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Opens {
    /// An index into [`data::MAPS`].
    Zone(usize),
    /// An index into [`data::CLASSES`].
    Class(usize),
}

pub(crate) struct Quest {
    /// Stable key for the saved announcements: `zone:<map index>` or `class:<class id>`.
    pub(crate) id: String,
    pub(crate) title: String,
    /// The one-line goal: "Survive 10 minutes in The Barrens".
    pub(crate) goal: String,
    pub(crate) opens: Opens,
    goal_def: Goal,
}

/// What the profile says about one quest: done, and how far along if that can be counted.
pub(crate) type Status = (bool, Option<(u32, u32)>);

impl Quest {
    /// Done, and the progress as `(now, needed)`: seconds for survival, the level for levels, none
    /// for a win.
    pub(crate) fn status(&self, profile: &Profile) -> Status {
        match self.goal_def {
            Goal::Survive { map, class, secs } => {
                let (best, wins) = best_of(profile, map, class);
                let now = best.round() as u32;
                (wins > 0 || now >= secs, Some((now.min(secs), secs)))
            }
            Goal::Win { map, class } => (best_of(profile, map, class).1 > 0, None),
            Goal::Level { class, level } => {
                let now = match class {
                    Some(c) => profile.character(c).level,
                    None => profile
                        .characters
                        .iter()
                        .map(|c| c.level)
                        .max()
                        .unwrap_or(1),
                };
                (now >= level, Some((now.min(level), level)))
            }
        }
    }

    pub(crate) fn done(&self, profile: &Profile) -> bool {
        self.status(profile).0
    }

    /// "6:12 / 10:00" for survival, "6 / 10" for levels, empty when there is nothing to count.
    pub(crate) fn progress_text(&self, progress: Option<(u32, u32)>) -> String {
        match (self.goal_def, progress) {
            (Goal::Survive { .. }, Some((now, need))) => {
                format!("{} / {}", clock(now), clock(need))
            }
            (Goal::Level { .. }, Some((now, need))) => format!("{now} / {need}"),
            _ => String::new(),
        }
    }
}

/// "6:12".
fn clock(secs: u32) -> String {
    format!("{}:{:02}", secs / 60, secs % 60)
}

/// The longest survival and the wins in the records of `map` and `class` (`None` is any).
fn best_of(profile: &Profile, map: Option<usize>, class: Option<usize>) -> (f32, u32) {
    let class_id = class.and_then(|c| data::CLASSES.get(c)).map(|c| c.id);
    if class.is_some() && class_id.is_none() {
        return (0.0, 0);
    }
    let mut secs = 0.0f32;
    let mut wins = 0u32;
    for (key, best) in &profile.records {
        let Some((m, c)) = key.split_once(':') else {
            continue;
        };
        let (Ok(m), Ok(c)) = (m.parse::<usize>(), c.parse::<u8>()) else {
            continue;
        };
        if map.is_some_and(|x| x != m) || class_id.is_some_and(|x| x != c) {
            continue;
        }
        let Best {
            secs: s, wins: w, ..
        } = *best;
        secs = secs.max(s);
        wins += w;
    }
    (secs, wins)
}

fn zone_name(map: usize) -> &'static str {
    data::MAPS[map.min(data::MAPS.len() - 1)].name
}

fn class_index(id: u8) -> Option<usize> {
    data::CLASSES.iter().position(|c| c.id == id)
}

fn map_named(name: &str) -> Option<usize> {
    data::MAPS.iter().position(|m| m.name == name)
}

/// "Survive 10 minutes in The Barrens", "Win a run as a Warrior", "Reach level 10 with any
/// character".
fn goal_text(goal: Goal) -> String {
    let class_name = |c: usize| data::CLASSES.get(c).map_or("hero", |d| d.name);
    match goal {
        Goal::Survive { map, class, secs } => {
            let mut s = format!("Survive {} minutes", secs / 60);
            if let Some(m) = map {
                s += &format!(" in {}", zone_name(m));
            }
            if let Some(c) = class {
                s += &format!(" as a {}", class_name(c));
            }
            s
        }
        Goal::Win {
            map: Some(m),
            class: None,
        } => {
            format!("Defeat the final boss of {}", zone_name(m))
        }
        Goal::Win { map, class } => {
            let mut s = "Win a run".to_string();
            if let Some(m) = map {
                s += &format!(" in {}", zone_name(m));
            }
            if let Some(c) = class {
                s += &format!(" as a {}", class_name(c));
            }
            s
        }
        Goal::Level {
            class: Some(c),
            level,
        } => {
            format!("Reach level {level} with the {}", class_name(c))
        }
        Goal::Level { class: None, level } => format!("Reach level {level} with any character"),
    }
}

/// The zones from easiest to hardest: by the lowest creature level, ties by index. A route
/// ([`super::route`]) is no link in the chain: it opens with its zones ([`zone_lock`]).
pub(crate) fn zone_order() -> &'static [usize] {
    static ORDER: LazyLock<Vec<usize>> = LazyLock::new(|| {
        let mut order: Vec<usize> = (0..data::MAPS.len())
            .filter(|&m| super::route::route_of(m).is_none())
            .collect();
        order.sort_by_key(|&m| (zone_min_level(m), m));
        order
    });
    &ORDER
}

/// The class quests: class id, title, goal. A class missing here has no quest and is bought with
/// gold only. The goals lean on the starter classes and on the zones, so each can be reached
/// with what is open from the start; the flavour is the class's own (Light, shadows, elements).
fn class_quest_table() -> Vec<(u8, &'static str, Option<Goal>)> {
    let warrior = class_index(1);
    let hunter = class_index(3);
    let mage = class_index(8);
    vec![
        // Alliance: the Light answers to a warrior's courage.
        (
            2,
            "Oath of the Light",
            Some(Goal::Win {
                map: None,
                class: warrior,
            }),
        ),
        // Shadows and daggers: outlast the hunt first.
        (
            4,
            "Blades in the Dark",
            Some(Goal::Survive {
                map: None,
                class: hunter,
                secs: SURVIVE_SECS,
            }),
        ),
        // Faith is taught at the mage's tower.
        (
            5,
            "A Call to Faith",
            mage.map(|m| Goal::Level {
                class: Some(m),
                level: QUEST_LEVEL,
            }),
        ),
        // Horde: the elements listen to anyone who has seen enough.
        (
            7,
            "Voice of the Elements",
            Some(Goal::Level {
                class: None,
                level: QUEST_LEVEL,
            }),
        ),
        (
            9,
            "Pact with the Shadow",
            map_named("Duskwood").map(|m| Goal::Win {
                map: Some(m),
                class: None,
            }),
        ),
        // The wild is a zone away from the plains.
        (
            11,
            "Call of the Wild",
            map_named("Mulgore").map(|m| Goal::Win {
                map: Some(m),
                class: None,
            }),
        ),
    ]
}

/// Every quest: first the zone chain in order, then the class quests.
pub(crate) fn quests() -> &'static [Quest] {
    static QUESTS: LazyLock<Vec<Quest>> = LazyLock::new(|| {
        let mut out = Vec::new();
        let order = zone_order();
        for k in 1..order.len() {
            let (from, opens) = (order[k - 1], order[k]);
            let goal = if k % WIN_EVERY == 0 {
                Goal::Win {
                    map: Some(from),
                    class: None,
                }
            } else {
                Goal::Survive {
                    map: Some(from),
                    class: None,
                    secs: SURVIVE_SECS,
                }
            };
            out.push(Quest {
                id: format!("zone:{opens}"),
                title: format!("The road to {}", zone_name(opens)),
                goal: goal_text(goal),
                opens: Opens::Zone(opens),
                goal_def: goal,
            });
        }
        for (id, title, goal) in class_quest_table() {
            let (Some(goal), Some(class)) = (goal, class_index(id)) else {
                continue;
            };
            out.push(Quest {
                id: format!("class:{id}"),
                title: title.to_string(),
                goal: goal_text(goal),
                opens: Opens::Class(class),
                goal_def: goal,
            });
        }
        out
    });
    &QUESTS
}

/// The quest that opens `map`; the first zone along the chain has none.
pub(crate) fn zone_quest(map: usize) -> Option<&'static Quest> {
    quests().iter().find(|q| q.opens == Opens::Zone(map))
}

/// The quest that opens `class`; a starter, or a class without one, has none.
pub(crate) fn class_quest(class: usize) -> Option<&'static Quest> {
    quests().iter().find(|q| q.opens == Opens::Class(class))
}

/// Whether `class`'s quest is done; a class without one is not opened this way.
pub(crate) fn class_quest_done(profile: &Profile, class: usize) -> bool {
    class_quest(class).is_some_and(|q| q.done(profile))
}

/// Why a zone is closed to a class: its character is too low, its quest is open, or both.
#[derive(Clone, Copy)]
pub(crate) struct ZoneLock {
    /// The level the zone asks for, when the character is below it.
    pub(crate) level: Option<u32>,
    /// The quest still to do.
    pub(crate) quest: Option<&'static Quest>,
}

impl ZoneLock {
    pub(crate) fn is_locked(&self) -> bool {
        self.level.is_some() || self.quest.is_some()
    }

    /// A row's short note: "Locked, level 12", "Locked, quest", "Locked, level 12 + quest".
    pub(crate) fn short(&self) -> String {
        match (self.level, self.quest) {
            (Some(l), Some(_)) => format!("Locked, level {l} + quest"),
            (Some(l), None) => format!("Locked, level {l}"),
            (None, Some(_)) => "Locked, quest".to_string(),
            (None, None) => String::new(),
        }
    }

    /// The pick screen's warning under the zones.
    pub(crate) fn long(&self) -> String {
        match (self.level, self.quest) {
            (Some(l), Some(q)) => format!("Requires level {l} and the quest: {}", q.goal),
            (Some(l), None) => format!("Requires level {l}"),
            (None, Some(q)) => format!("Requires the quest: {}", q.goal),
            (None, None) => String::new(),
        }
    }
}

/// What keeps `class`'s character out of `map`; nothing locked with `unlock_all`.
pub(crate) fn zone_lock(profile: &Profile, class: usize, map: usize) -> ZoneLock {
    if profile.unlock_all {
        return ZoneLock {
            level: None,
            quest: None,
        };
    }
    // A route is open once every zone on it is: else the first closed one says why.
    if let Some(route) = super::route::route_of(map) {
        return route
            .zones()
            .into_iter()
            .map(|zone| zone_lock(profile, class, zone))
            .find(ZoneLock::is_locked)
            .unwrap_or(ZoneLock {
                level: None,
                quest: None,
            });
    }
    let need = zone_min_level(map);
    ZoneLock {
        level: (profile.character(class).level < need).then_some(need),
        // A zone the profile could enter before the quest log needs no quest
        // ([`Profile::keep_open_zones`]).
        quest: zone_quest(map)
            .filter(|q| !q.done(profile) && !profile.zones_kept.contains(data::MAPS[map].name)),
    }
}

// --- Announcements. ---

/// Say once what a quest opened: "Ashenvale is open.", or when no character is high enough yet,
/// the level it opens at.
fn opened_text(quest: &Quest, profile: &Profile) -> String {
    match quest.opens {
        Opens::Zone(m) => {
            let need = zone_min_level(m);
            let top = profile
                .characters
                .iter()
                .map(|c| c.level)
                .max()
                .unwrap_or(1);
            if top >= need {
                format!("{} is open.", zone_name(m))
            } else {
                format!("{} opens at level {need}.", zone_name(m))
            }
        }
        Opens::Class(c) => format!(
            "{} is open.",
            data::CLASSES.get(c).map_or("A class", |d| d.name)
        ),
    }
}

/// At the first frame with a profile: the quests already done are old news, mark them announced
/// without a word, so a profile from before the quest log does not flood the chat.
fn prime(mut profile: ResMut<Profile>, mut primed: Local<bool>) {
    if *primed {
        return;
    }
    *primed = true;
    let old: Vec<String> = quests()
        .iter()
        .filter(|q| q.done(&profile) && !profile.quests_announced.contains(&q.id))
        .map(|q| q.id.clone())
        .collect();
    if !old.is_empty() {
        profile.quests_announced.extend(old);
    }
}

/// While the results show, after the run was folded into the records: every quest that became
/// done says so once in the chat, and the profile remembers it.
fn announce(mut profile: ResMut<Profile>, mut chat: MessageWriter<ChatLine>) {
    // Read through the shared borrow: a write would mark the profile changed every frame.
    let fresh: Vec<(String, String)> = {
        let p: &Profile = &profile;
        quests()
            .iter()
            .filter(|q| !p.quests_announced.contains(&q.id) && q.done(p))
            .map(|q| {
                (
                    q.id.clone(),
                    format!("Quest complete: {}. {}", q.goal, opened_text(q, p)),
                )
            })
            .collect()
    };
    if fresh.is_empty() {
        return;
    }
    for (id, line) in fresh {
        profile.quests_announced.insert(id);
        chat.write(ChatLine::system(line));
    }
    // Saves nothing for an automated run.
    profile.save();
}

// --- The window. ---

/// The dimmed full-screen layer holding the window, hidden until opened.
#[derive(Component)]
struct QlPanel;
/// The column inside the window whose children are rebuilt whenever the state changes.
#[derive(Component)]
struct QlBody;
/// On [`MenuRoot`] once the window is built into it.
#[derive(Component)]
struct QlBuilt;
/// The Close button.
#[derive(Component)]
struct QlClose;

const PANEL_W: f32 = 1000.0;
const PANEL_H: f32 = 680.0;
const ROW_H: f32 = 38.0;

fn px(v: f32) -> Val {
    Val::Px(v)
}

fn nowrap() -> TextLayout {
    TextLayout::new(Justify::Left, LineBreak::NoWrap)
}

pub(super) fn plugin(app: &mut App) {
    app.init_resource::<QuestLogUi>()
        .add_systems(
            Update,
            (prime, announce.run_if(in_state(Phase::GameOver)))
                .chain()
                .run_if(resource_exists::<Profile>),
        )
        .add_systems(
            Update,
            (build, show_panel, clicks, rebuild)
                .chain()
                // After the keys, which read the other windows' flags first.
                .after(hud::keys)
                .before(GlueVisuals),
        );
}

/// Build the window into the pick screen once it is built.
fn build(
    mut commands: Commands,
    roots: Query<Entity, (With<MenuRoot>, With<MenuBuilt>, Without<QlBuilt>)>,
    fonts: Res<KitFonts>,
) {
    let Ok(root) = roots.single() else { return };
    commands.entity(root).insert(QlBuilt).with_children(|ui| {
        window(ui, &fonts);
    });
}

/// The frame: heading, a hint, the body (filled by [`rebuild`]) and Close.
fn window(ui: &mut ChildSpawnerCommands, fonts: &KitFonts) {
    ui.spawn((
        QlPanel,
        // Swallows the clicks on the pick screen under it.
        FocusPolicy::Block,
        BackgroundColor(Color::srgba(0.0, 0.0, 0.0, 0.6)),
        Node {
            position_type: PositionType::Absolute,
            width: Val::Percent(100.0),
            height: Val::Percent(100.0),
            justify_content: JustifyContent::Center,
            align_items: AlignItems::Center,
            display: Display::None,
            ..default()
        },
    ))
    .with_children(|dim| {
        dim.spawn(kit::panel(Node {
            width: px(PANEL_W),
            height: px(PANEL_H),
            flex_direction: FlexDirection::Column,
            padding: UiRect::axes(px(24.0), px(12.0)),
            row_gap: px(6.0),
            ..default()
        }))
        .with_children(|win| {
            kit::heading(win, fonts, "Quest Log", 26.0);
            win.spawn(Node {
                width: Val::Percent(100.0),
                justify_content: JustifyContent::Center,
                ..default()
            })
            .with_children(|row| {
                row.spawn((
                    kit::body(
                        fonts,
                        "A zone opens when its quest is done and the character has the level. A class opens with its quest or with gold.",
                        13.0,
                        kit::MUTED,
                    ),
                    nowrap(),
                ));
            });
            win.spawn((
                QlBody,
                Node {
                    width: Val::Percent(100.0),
                    flex_grow: 1.0,
                    min_height: px(0.0),
                    flex_direction: FlexDirection::Column,
                    row_gap: px(6.0),
                    ..default()
                },
            ));
            win.spawn(Node {
                width: Val::Percent(100.0),
                justify_content: JustifyContent::FlexEnd,
                ..default()
            })
            .with_children(|foot| {
                kit::button(
                    foot,
                    fonts,
                    KitStyle::Normal,
                    "Close",
                    None,
                    Node {
                        width: px(120.0),
                        height: px(32.0),
                        ..default()
                    },
                    QlClose,
                );
            });
        });
    });
}

/// Close on Esc or when the pick screen goes. For an automated tour of the windows
/// (`WOW_SURVIVORS_WINDOWS`) the log gets made-up progress first, which is never saved. The
/// display follows [`QuestLogUi::open`].
fn show_panel(
    mut ui: ResMut<QuestLogUi>,
    phase: Res<State<Phase>>,
    keys: Res<ButtonInput<KeyCode>>,
    mut profile: ResMut<Profile>,
    mut panel: Query<&mut Node, With<QlPanel>>,
    mut seeded: Local<bool>,
) {
    if ui.open && (*phase.get() != Phase::Menu || keys.just_pressed(KeyCode::Escape)) {
        ui.open = false;
    }
    if ui.open
        && !*seeded
        && profile.ephemeral
        && benilla_app::survivors_host::webenv::var("WOW_SURVIVORS_WINDOWS").is_some()
    {
        *seeded = true;
        seed_demo(&mut profile);
    }
    let display = if ui.open {
        Display::Flex
    } else {
        Display::None
    };
    for mut node in &mut panel {
        if node.display != display {
            node.display = display;
        }
    }
}

/// Made-up progress for a screenshot: the first zones survived, the next half way, a warrior and
/// a mage with some levels and a hunter's run. Only into an empty history.
fn seed_demo(profile: &mut Profile) {
    if !profile.records.is_empty() {
        return;
    }
    let order = zone_order();
    let id_of = |class: Option<usize>| class.and_then(|c| data::CLASSES.get(c)).map(|c| c.id);
    let (warrior, hunter, mage) = (class_index(1), class_index(3), class_index(8));
    let mut put = |zone: usize, class: Option<usize>, secs: f32| {
        if let (Some(&map), Some(id)) = (order.get(zone), id_of(class)) {
            profile.records.insert(
                super::records::key(map, id),
                Best {
                    secs,
                    kills: (secs * 1.4) as u32,
                    ..default()
                },
            );
        }
    };
    put(0, warrior, 700.0);
    put(1, mage, 640.0);
    put(2, warrior, 372.0);
    put(3, hunter, 560.0);
    if let Some(c) = warrior.and_then(|w| profile.characters.get_mut(w)) {
        c.level = 12;
    }
    if let Some(c) = mage.and_then(|m| profile.characters.get_mut(m)) {
        c.level = 6;
    }
}

fn clicks(
    close: Query<&Interaction, (With<QlClose>, Changed<Interaction>)>,
    buttons: Query<&KitButton, With<QlClose>>,
    mut ui: ResMut<QuestLogUi>,
) {
    if !ui.open {
        return;
    }
    let pressed =
        close.iter().any(|i| *i == Interaction::Pressed) && buttons.iter().all(|b| !b.disabled);
    if pressed {
        ui.open = false;
    }
}

/// While open, rebuild the body whenever a quest's state changed.
fn rebuild(
    mut commands: Commands,
    ui: Res<QuestLogUi>,
    profile: Res<Profile>,
    fonts: Res<KitFonts>,
    body: Query<Entity, With<QlBody>>,
    mut last: Local<Option<Vec<Status>>>,
) {
    if !ui.open {
        return;
    }
    let Ok(body) = body.single() else { return };
    let all = quests();
    let states: Vec<Status> = all.iter().map(|q| q.status(&profile)).collect();
    if last.as_ref() == Some(&states) {
        return;
    }
    let done = |zone: bool| {
        all.iter()
            .zip(&states)
            .filter(|(q, _)| matches!(q.opens, Opens::Zone(_)) == zone)
            .filter(|(_, s)| s.0)
            .count()
    };
    let total = |zone: bool| {
        all.iter()
            .filter(|q| matches!(q.opens, Opens::Zone(_)) == zone)
            .count()
    };
    let rows = |zone: bool| -> Vec<(&Quest, Status)> {
        all.iter()
            .zip(states.iter().copied())
            .filter(|(q, _)| matches!(q.opens, Opens::Zone(_)) == zone)
            .collect()
    };
    let (zones, classes) = (rows(true), rows(false));
    let heads = (
        format!("Zones, {} of {} done", done(true), total(true)),
        format!("Classes, {} of {} done", done(false), total(false)),
    );
    *last = Some(states);

    commands.entity(body).despawn_children();
    commands.entity(body).with_children(|col| {
        section(col, &fonts, &heads.0, &zones);
        section(col, &fonts, &heads.1, &classes);
    });
}

/// A section: its label and its rows in two columns, filled down the left one first.
fn section(
    col: &mut ChildSpawnerCommands,
    fonts: &KitFonts,
    head: &str,
    rows: &[(&Quest, Status)],
) {
    col.spawn(kit::label(fonts, head));
    let half = rows.len().div_ceil(2);
    col.spawn(Node {
        width: Val::Percent(100.0),
        column_gap: px(14.0),
        ..default()
    })
    .with_children(|grid| {
        for chunk in rows.chunks(half.max(1)) {
            grid.spawn(Node {
                flex_grow: 1.0,
                flex_basis: px(0.0),
                min_width: px(0.0),
                flex_direction: FlexDirection::Column,
                row_gap: px(5.0),
                ..default()
            })
            .with_children(|side| {
                for (quest, status) in chunk {
                    quest_row(side, fonts, quest, *status);
                }
            });
        }
    });
}

/// One quest: a box (filled green when done), the title over the goal, the progress at the right.
/// A done quest is greyed.
fn quest_row(side: &mut ChildSpawnerCommands, fonts: &KitFonts, quest: &Quest, status: Status) {
    let (done, progress) = status;
    let class_color = match quest.opens {
        Opens::Class(c) => data::CLASSES.get(c).map(|d| hud::rgb(d.color)),
        Opens::Zone(_) => None,
    };
    let (title, goal) = if done {
        (kit::MUTED, kit::DISABLED)
    } else {
        (class_color.unwrap_or(kit::TITLE), kit::CREAM)
    };
    let right = if done {
        ("Done".to_string(), kit::BONUS)
    } else {
        (quest.progress_text(progress), kit::MUTED)
    };
    side.spawn(kit::inset(Node {
        width: Val::Percent(100.0),
        height: px(ROW_H),
        padding: UiRect::axes(px(8.0), px(2.0)),
        align_items: AlignItems::Center,
        column_gap: px(10.0),
        ..default()
    }))
    .with_children(|row| {
        row.spawn((
            Node {
                width: px(16.0),
                height: px(16.0),
                flex_shrink: 0.0,
                border: UiRect::all(px(1.0)),
                border_radius: BorderRadius::all(px(3.0)),
                ..default()
            },
            BackgroundColor(if done { kit::BONUS } else { Color::NONE }),
            BorderColor::all(if done { kit::BONUS } else { kit::INSET_EDGE }),
        ));
        row.spawn(Node {
            flex_grow: 1.0,
            flex_basis: px(0.0),
            min_width: px(0.0),
            flex_direction: FlexDirection::Column,
            ..default()
        })
        .with_children(|text| {
            text.spawn((kit::body(fonts, quest.title.clone(), 15.0, title), nowrap()));
            text.spawn((kit::body(fonts, quest.goal.clone(), 12.0, goal), nowrap()));
        });
        row.spawn((kit::body(fonts, right.0, 14.0, right.1), nowrap()));
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    fn record(p: &mut Profile, map: usize, class: usize, secs: f32, wins: u32) {
        let id = data::CLASSES[class].id;
        p.records.insert(
            super::super::records::key(map, id),
            Best {
                secs,
                wins,
                ..default()
            },
        );
    }

    fn class_of(id: u8) -> usize {
        class_index(id).unwrap()
    }

    #[test]
    fn zones_run_easiest_first_and_each_but_the_first_has_a_quest() {
        let order = zone_order();
        let routes = super::super::route::ROUTES.len();
        assert_eq!(order.len(), data::MAPS.len() - routes);
        for pair in order.windows(2) {
            assert!(
                (zone_min_level(pair[0]), pair[0]) < (zone_min_level(pair[1]), pair[1]),
                "{} before {}",
                zone_name(pair[0]),
                zone_name(pair[1])
            );
        }
        assert!(zone_quest(order[0]).is_none());
        for &m in &order[1..] {
            assert!(zone_quest(m).is_some(), "{} has no quest", zone_name(m));
        }
        // Every third quest is a win.
        let wins = order[1..]
            .iter()
            .filter(|&&m| matches!(zone_quest(m).unwrap().goal_def, Goal::Win { .. }))
            .count();
        assert_eq!(wins, (order.len() - 1) / WIN_EVERY);
    }

    #[test]
    fn every_quest_has_a_unique_id_and_a_goal() {
        let mut seen = std::collections::BTreeSet::new();
        for q in quests() {
            assert!(seen.insert(q.id.clone()), "duplicate {}", q.id);
            assert!(!q.goal.is_empty() && !q.title.is_empty());
            assert!(!q.goal.contains('\u{2014}') && !q.title.contains('\u{2014}'));
        }
    }

    #[test]
    fn survival_counts_time_in_that_zone_with_any_class() {
        let order = zone_order();
        let (first, second) = (order[0], order[1]);
        let quest = zone_quest(second).unwrap();
        assert_eq!(quest.opens, Opens::Zone(second));
        assert!(quest.goal.starts_with("Survive 10 minutes in "));
        let mut p = Profile::default();
        assert_eq!(quest.status(&p), (false, Some((0, 600))));
        record(&mut p, first, 0, 372.0, 0);
        assert_eq!(quest.status(&p), (false, Some((372, 600))));
        assert_eq!(quest.progress_text(quest.status(&p).1), "6:12 / 10:00");
        // Another zone does not count.
        record(&mut p, second, 2, 900.0, 0);
        assert!(!quest.done(&p));
        // Another class in the right zone does.
        record(&mut p, first, 6, 605.0, 0);
        assert!(quest.done(&p));
    }

    #[test]
    fn a_win_counts_as_a_survived_run_and_opens_the_win_quest() {
        let order = zone_order();
        let quest = zone_quest(order[WIN_EVERY]).unwrap();
        assert!(quest.goal.starts_with("Defeat the final boss of "));
        let mut p = Profile::default();
        record(&mut p, order[WIN_EVERY - 1], 0, 800.0, 0);
        assert!(!quest.done(&p));
        record(&mut p, order[WIN_EVERY - 1], 0, 900.0, 1);
        assert!(quest.done(&p));
        let survive = zone_quest(order[WIN_EVERY - 1]).unwrap();
        record(&mut p, order[WIN_EVERY - 2], 0, 100.0, 1);
        assert!(survive.done(&p));
    }

    #[test]
    fn level_quests_follow_the_characters() {
        let q = class_quest(class_of(5)).unwrap();
        assert_eq!(q.goal, "Reach level 10 with the Mage");
        let mut p = Profile::default();
        assert_eq!(q.status(&p), (false, Some((1, 10))));
        p.characters[class_of(8)].level = 10;
        assert!(q.done(&p));
        // Any character.
        let any = class_quest(class_of(7)).unwrap();
        assert!(!any.done(&p) || p.characters.iter().any(|c| c.level >= 10));
        let mut p = Profile::default();
        p.characters[class_of(3)].level = 11;
        assert!(any.done(&p));
        assert_eq!(any.progress_text(any.status(&p).1), "10 / 10");
    }

    #[test]
    fn a_zone_needs_level_and_quest() {
        let order = zone_order();
        let (first, second) = (order[0], order[1]);
        let class = 0;
        let mut p = Profile::default();
        p.characters[class].level = 60;
        assert!(p.zone_unlocked(class, first));
        // High enough, quest open.
        assert!(!p.zone_unlocked(class, second));
        let lock = zone_lock(&p, class, second);
        assert!(lock.level.is_none() && lock.quest.is_some());
        assert!(lock.short().starts_with("Locked, quest"));
        assert!(lock.long().starts_with("Requires the quest: Survive"));
        record(&mut p, first, 0, 650.0, 0);
        assert!(p.zone_unlocked(class, second));
        // Quest done, level too low: pick a zone whose gate is higher than 1.
        let hard = *order.last().unwrap();
        let mut q = Profile::default();
        for &m in &order[..order.len() - 1] {
            record(&mut q, m, 0, 900.0, 1);
        }
        assert!(zone_min_level(hard) > 1);
        let lock = zone_lock(&q, class, hard);
        assert!(lock.quest.is_none());
        assert_eq!(lock.level, Some(zone_min_level(hard)));
        assert_eq!(
            lock.long(),
            format!("Requires level {}", zone_min_level(hard))
        );
        assert!(!q.zone_unlocked(class, hard));
        q.characters[class].level = zone_min_level(hard);
        assert!(q.zone_unlocked(class, hard));
    }

    #[test]
    fn both_reasons_show_when_both_apply() {
        let order = zone_order();
        let hard = *order.last().unwrap();
        let p = Profile::default();
        let lock = zone_lock(&p, 0, hard);
        assert!(lock.level.is_some() && lock.quest.is_some());
        assert!(lock.short().ends_with("+ quest"));
        assert!(lock.long().contains(" and the quest: "));
    }

    #[test]
    fn unlock_all_opens_every_zone_and_class() {
        let p = Profile {
            unlock_all: true,
            ..Profile::default()
        };
        for m in 0..data::MAPS.len() {
            assert!(p.zone_unlocked(0, m));
            assert!(!zone_lock(&p, 0, m).is_locked());
        }
        for c in 0..data::CLASSES.len() {
            assert!(p.class_unlocked(c));
        }
    }

    #[test]
    fn a_class_opens_with_its_quest() {
        let paladin = class_of(2);
        let warrior = class_of(1);
        let mut p = Profile::default();
        assert!(!p.class_unlocked(paladin));
        // The wrong class's win does not do.
        record(&mut p, 0, class_of(3), 900.0, 1);
        assert!(!p.class_unlocked(paladin));
        record(&mut p, 0, warrior, 900.0, 1);
        assert!(p.class_unlocked(paladin));
        // Opened by the quest, not bought: the price of the next class stays.
        assert!(!p.classes[paladin]);
        // A starter has no quest.
        assert!(class_quest(warrior).is_none());
    }

    #[test]
    fn every_locked_class_with_a_quest_is_reachable_from_the_starters() {
        // The quests may only lean on the starters and on zones, never on another locked class.
        for q in quests() {
            if let Opens::Class(_) = q.opens {
                match q.goal_def {
                    Goal::Survive { class, .. }
                    | Goal::Win { class, .. }
                    | Goal::Level { class, .. } => {
                        if let Some(c) = class {
                            assert!(
                                Profile::default().classes[c],
                                "{} leans on a locked class",
                                q.goal
                            );
                        }
                    }
                }
            }
        }
    }

    #[test]
    fn routes_stay_out_of_the_zone_chain_and_open_with_their_zones() {
        use super::super::route;
        for r in route::ROUTES {
            let map = map_named(r.map).expect("a route's battleground");
            assert!(!zone_order().contains(&map), "{} in the chain", r.map);
            assert!(zone_quest(map).is_none(), "{} has a quest", r.map);
            // Every zone on it open: the route is too.
            let mut p = Profile::default();
            for c in &mut p.characters {
                c.level = 60;
            }
            let zones = route::route_of(map).unwrap().zones();
            for &z in &zones {
                p.zones_kept.insert(data::MAPS[z].name.to_string());
            }
            assert!(!zone_lock(&p, 0, map).is_locked(), "{}", r.map);
            assert!(p.zone_unlocked(0, map));
            // The last zone closed: the route is, for the same reason.
            let last = *zones.last().unwrap();
            p.zones_kept.remove(data::MAPS[last].name);
            let lock = zone_lock(&p, 0, map);
            assert!(lock.is_locked(), "{}", r.map);
            assert!(std::ptr::eq(lock.quest.unwrap(), zone_quest(last).unwrap()));
            assert!(!p.zone_unlocked(0, map));
        }
    }

    #[test]
    fn old_profiles_stay_readable() {
        let p: Profile = toml::from_str("gold = 5").unwrap();
        assert!(p.quests_announced.is_empty());
        let text = toml::to_string(&p).unwrap();
        let back: Profile = toml::from_str(&text).unwrap();
        assert_eq!(back, p);
    }
}
