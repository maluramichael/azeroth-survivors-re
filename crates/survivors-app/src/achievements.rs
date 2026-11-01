//! Achievements and the rewards they buy. Every achievement is worth points; earned points are
//! spent on perks that change the level-up screen ([`RunPerks`]): more rerolls, banishes, a bigger
//! first hand. Opened from the hub's Achievements tab. Earned achievements, the lifetime counters
//! and the perk ranks live in [`super::profile::Profile`].
//!
//! Almost every achievement reads a lifetime counter ([`record_run`] bumps them when a run ends):
//! `kills`, `gold`, `bosses`, `runs`, `won_zone_<map>`, `won_class_<class>`, `won_heroic`,
//! `won_hardcore`, and the best of a single run (`best_kills`, `best_level`, `best_minutes`,
//! `best_bosses`). The rest read the collection, the characters' levels or the bought classes.
//! Never rename an achievement or perk key: the earned ones and the ranks are saved under it.

use std::collections::BTreeMap;
use std::sync::LazyLock;

use bevy::prelude::*;
use bevy::ui::FocusPolicy;

use super::chatlog::ChatLine;
use super::difficulty::Difficulty;
use super::hud::{self, MenuRoot};
use super::kit::{self, KitButton, KitFonts, KitStyle};
use super::menu::MenuBuilt;
use super::profile::Profile;
use super::{data, Phase, Run, Selection};
use benilla_app::survivors_host::glue::GlueVisuals;

/// Whether the achievements window is open; the hub's tab sets it.
#[derive(Resource, Default)]
pub(crate) struct AchievementsUi {
    pub(crate) open: bool,
}

/// What the level-up screen allows this run, from the perks bought. Set when a run starts; the
/// level-up screen ([`super::cards`]) spends `rerolls` and `banishes` and reads the rest.
#[derive(Resource, Clone, Debug, PartialEq)]
pub(crate) struct RunPerks {
    /// Hands that can still be dealt again this run.
    pub(crate) rerolls: u32,
    /// Cards that can still be banished this run: a banished choice never comes back this run.
    pub(crate) banishes: u32,
    /// Whether a hand can be skipped for gold.
    pub(crate) skip: bool,
    /// Cards in the first hand of the run (the others hold the usual three).
    pub(crate) first_hand: usize,
}

impl Default for RunPerks {
    /// A run with no perks bought.
    fn default() -> Self {
        Self {
            rerolls: 1,
            banishes: 0,
            skip: true,
            first_hand: 3,
        }
    }
}

// --- The achievements. ---

/// Zones first, then the classes, the single-run feats, the lifetime totals and the collection.
const CATEGORIES: [&str; 5] = ["Zones", "Classes", "Feats", "Totals", "Collection"];

/// What an achievement counts.
#[derive(Clone, Debug, PartialEq)]
enum Goal {
    /// A lifetime counter in [`Profile::counters`] reaching a target.
    Counter(String, u64),
    /// Distinct items found ([`Profile::collection`]).
    Collection(u64),
    /// The highest character level over all classes.
    CharLevel(u64),
    /// Classes bought.
    Classes(u64),
}

struct Achievement {
    /// Saved with the day it was earned; never change one.
    key: String,
    name: String,
    desc: String,
    /// 10, 25 or 50.
    points: u32,
    category: &'static str,
    /// A name under `Interface\Icons`.
    icon: &'static str,
    goal: Goal,
}

fn counter(key: &str, need: u64) -> Goal {
    Goal::Counter(key.to_string(), need)
}

fn zone_points(name: &str) -> u32 {
    match name {
        "Mount Hyjal" | "Naxxramas" | "Undying Run" => 50,
        "The Dark Portal" | "Gates of Ahn'Qiraj" | "Fire Plume Ridge" | "Gurubashi Arena"
        | "Redridge Mountains" | "Duskwood" => 25,
        _ => 10,
    }
}

fn zone_icon(name: &str) -> &'static str {
    match name {
        "The Dark Portal" => "Spell_Arcane_PortalOrgrimmar",
        "Gates of Ahn'Qiraj" => "INV_Misc_Head_Dragon_Bronze",
        "Fire Plume Ridge" => "Spell_Fire_Fire",
        "Gurubashi Arena" => "INV_Misc_Bone_HumanSkull_01",
        "Mount Hyjal" => "INV_Misc_Head_Dragon_01",
        "Naxxramas" => "Spell_Shadow_DeathScream",
        "Undying Run" => "INV_Misc_Map_01",
        _ => "INV_Misc_Map_01",
    }
}

fn class_icon(name: &str) -> &'static str {
    match name {
        "Warrior" => "Ability_Warrior_BattleShout",
        "Paladin" => "Spell_Holy_SealOfMight",
        "Hunter" => "Ability_Hunter_BeastTaming",
        "Rogue" => "Ability_Rogue_Eviscerate",
        "Priest" => "Spell_Holy_PowerWordShield",
        "Shaman" => "Spell_Nature_BloodLust",
        "Mage" => "Spell_Frost_FrostBolt02",
        "Warlock" => "Spell_Shadow_DeathScream",
        "Druid" => "Spell_Nature_Regeneration",
        _ => "INV_Misc_QuestionMark",
    }
}

fn class_points(name: &str) -> u32 {
    match name {
        "Warrior" | "Hunter" | "Mage" => 10,
        _ => 25,
    }
}

static TABLE: LazyLock<Vec<Achievement>> = LazyLock::new(build_table);

fn table() -> &'static [Achievement] {
    &TABLE
}

fn build_table() -> Vec<Achievement> {
    let mut t = Vec::new();
    let mut add = |key: &str,
                   name: &str,
                   desc: &str,
                   points: u32,
                   category: &'static str,
                   icon: &'static str,
                   goal: Goal| {
        t.push(Achievement {
            key: key.to_string(),
            name: name.to_string(),
            desc: desc.to_string(),
            points,
            category,
            icon,
            goal,
        });
    };

    for (i, map) in data::MAPS.iter().enumerate() {
        add(
            &format!("zone_{i}"),
            &format!("{} Conquered", map.name),
            &if super::route::route_of(i).is_some() {
                format!("Reach the end of the {}", map.name)
            } else {
                format!("Win a run in {}", map.name)
            },
            zone_points(map.name),
            "Zones",
            zone_icon(map.name),
            counter(&format!("won_zone_{i}"), 1),
        );
    }
    for (i, class) in data::CLASSES.iter().enumerate() {
        add(
            &format!("class_{i}"),
            &format!("Victorious {}", class.name),
            &format!("Win a run as a {}", class.name),
            class_points(class.name),
            "Classes",
            class_icon(class.name),
            counter(&format!("won_class_{i}"), 1),
        );
    }

    add("feat_kills_1000", "Massacre", "Kill 1,000 enemies in one run", 25, "Feats",
        "INV_Sword_04", counter("best_kills", 1000));
    add("feat_kills_3000", "Walking Apocalypse", "Kill 3,000 enemies in one run", 50, "Feats",
        "Spell_Fire_SelfDestruct", counter("best_kills", 3000));
    add("feat_level_20", "Rising Star", "Reach level 20 in a run", 10, "Feats",
        "Spell_Holy_Heal", counter("best_level", 20));
    add("feat_level_30", "Fully Stacked", "Reach level 30 in a run", 25, "Feats",
        "Spell_Holy_GreaterHeal", counter("best_level", 30));
    add("feat_minutes_10", "Still Standing", "Survive 10 minutes", 10, "Feats",
        "INV_Misc_PocketWatch_02", counter("best_minutes", 10));
    add("feat_minutes_20", "Stubborn", "Survive 20 minutes", 25, "Feats",
        "INV_Misc_PocketWatch_01", counter("best_minutes", 20));
    add("feat_bosses_5", "Bounty Hunter", "Slay 5 bosses in one run", 25, "Feats",
        "INV_Misc_Head_Dragon_Blue", counter("best_bosses", 5));
    add("feat_heroic", "Heroic Deeds", "Win a run on Heroic or Hardcore", 25, "Feats",
        "INV_Shield_06", counter("won_heroic", 1));
    add("feat_hardcore", "No Mercy", "Win a run on Hardcore", 50, "Feats",
        "INV_Misc_Bone_HumanSkull_01", counter("won_hardcore", 1));

    add("total_kills_1k", "Pest Control", "Kill 1,000 enemies", 10, "Totals",
        "INV_Misc_Bone_01", counter("kills", 1_000));
    add("total_kills_10k", "Exterminator", "Kill 10,000 enemies", 25, "Totals",
        "INV_Sword_04", counter("kills", 10_000));
    add("total_kills_100k", "Scourge of Azeroth", "Kill 100,000 enemies", 50, "Totals",
        "Spell_Shadow_SoulLeech_3", counter("kills", 100_000));
    add("total_gold_5k", "Pocket Change", "Bank 5,000 gold", 10, "Totals",
        "INV_Misc_Coin_01", counter("gold", 5_000));
    add("total_gold_50k", "Fat Purse", "Bank 50,000 gold", 25, "Totals",
        "INV_Misc_Coin_02", counter("gold", 50_000));
    add("total_gold_500k", "Goblin Tycoon", "Bank 500,000 gold", 50, "Totals",
        "INV_Misc_Bag_10", counter("gold", 500_000));
    add("total_bosses_10", "Boss Hunter", "Slay 10 bosses", 10, "Totals",
        "INV_Misc_Head_Dragon_Bronze", counter("bosses", 10));
    add("total_bosses_100", "Bane of Bosses", "Slay 100 bosses", 25, "Totals",
        "INV_Misc_Head_Dragon_01", counter("bosses", 100));
    add("total_runs_10", "Seasoned", "Finish 10 runs", 10, "Totals",
        "INV_Misc_Note_01", counter("runs", 10));
    add("total_runs_50", "Veteran", "Finish 50 runs", 25, "Totals",
        "INV_Misc_Book_09", counter("runs", 50));
    add("total_level_30", "Hero in the Making", "Take a character to level 30", 25, "Totals",
        "INV_Chest_Plate_06", Goal::CharLevel(30));
    add("total_level_60", "Level 60", "Take a character to level 60", 50, "Totals",
        "INV_Chest_Plate_16", Goal::CharLevel(60));
    add("total_classes", "Full Roster", "Unlock every class", 25, "Totals",
        "INV_Banner_01", Goal::Classes(data::CLASSES.len() as u64));

    add("coll_10", "Magpie", "Find 10 items", 10, "Collection",
        "INV_Misc_Gem_01", Goal::Collection(10));
    add("coll_25", "Hoarder", "Find 25 items", 25, "Collection",
        "INV_Misc_Gem_Ruby_01", Goal::Collection(25));
    add("coll_60", "Treasure Hunter", "Find 60 items", 50, "Collection",
        "INV_Misc_Gem_Diamond_01", Goal::Collection(60));

    t
}

/// How far the profile is on a goal: `(have, need)`. `have` may exceed `need`.
fn progress(profile: &Profile, goal: &Goal) -> (u64, u64) {
    match goal {
        Goal::Counter(key, need) => (profile.counters.get(key).copied().unwrap_or(0), *need),
        Goal::Collection(need) => (profile.collection.len() as u64, *need),
        Goal::CharLevel(need) => (
            profile
                .characters
                .iter()
                .map(|c| u64::from(c.level))
                .max()
                .unwrap_or(1),
            *need,
        ),
        Goal::Classes(need) => (profile.classes.iter().filter(|&&b| b).count() as u64, *need),
    }
}

fn frac(have: u64, need: u64) -> f32 {
    if need == 0 {
        1.0
    } else {
        (have as f32 / need as f32).clamp(0.0, 1.0)
    }
}

/// What a finished run adds to the counters.
#[derive(Clone, Copy, Debug)]
struct RunEnd {
    kills: u32,
    level: u32,
    secs: f32,
    bosses: u32,
    /// The gold the run paid into the bank.
    gold: u64,
    won: bool,
    difficulty: Difficulty,
    /// Index into [`data::CLASSES`].
    class: usize,
    /// Index into [`data::MAPS`].
    map: usize,
}

fn bump(counters: &mut BTreeMap<String, u64>, key: &str, by: u64) {
    *counters.entry(key.to_string()).or_insert(0) += by;
}

fn best(counters: &mut BTreeMap<String, u64>, key: &str, value: u64) {
    let slot = counters.entry(key.to_string()).or_insert(0);
    *slot = (*slot).max(value);
}

/// Fold a finished run into the lifetime counters.
fn record_run(counters: &mut BTreeMap<String, u64>, end: &RunEnd) {
    bump(counters, "kills", u64::from(end.kills));
    bump(counters, "gold", end.gold);
    bump(counters, "bosses", u64::from(end.bosses));
    bump(counters, "runs", 1);
    best(counters, "best_kills", u64::from(end.kills));
    best(counters, "best_level", u64::from(end.level));
    best(counters, "best_minutes", (end.secs.max(0.0) / 60.0) as u64);
    best(counters, "best_bosses", u64::from(end.bosses));
    if end.won {
        bump(counters, &format!("won_zone_{}", end.map), 1);
        bump(counters, &format!("won_class_{}", end.class), 1);
        if end.difficulty != Difficulty::Normal {
            bump(counters, "won_heroic", 1);
        }
        if end.difficulty == Difficulty::Hardcore {
            bump(counters, "won_hardcore", 1);
        }
    }
}

/// Record every achievement the counters now reach and are not yet earned, with `day`.
fn grant(profile: &mut Profile, day: u32) -> Vec<&'static Achievement> {
    let mut new = Vec::new();
    for a in table() {
        if profile.achievements.contains_key(&a.key) {
            continue;
        }
        let (have, need) = progress(profile, &a.goal);
        if have >= need {
            profile.achievements.insert(a.key.clone(), day);
            new.push(a);
        }
    }
    new
}

/// Days since 1970 today.
fn today() -> u32 {
    web_time::SystemTime::now()
        .duration_since(web_time::UNIX_EPOCH)
        .map_or(0, |d| (d.as_secs() / 86_400) as u32)
}

/// "28 Oct 2026" for a day count since 1970.
fn date_string(day: u32) -> String {
    const MONTHS: [&str; 12] = [
        "Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec",
    ];
    // Days to civil date (proleptic Gregorian), after Howard Hinnant.
    let z = i64::from(day) + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = yoe + era * 400 + i64::from(m <= 2);
    format!("{d} {} {y}", MONTHS[(m - 1) as usize])
}

fn earned_points(profile: &Profile) -> u32 {
    table()
        .iter()
        .filter(|a| profile.achievements.contains_key(&a.key))
        .map(|a| a.points)
        .sum()
}

fn total_points() -> u32 {
    table().iter().map(|a| a.points).sum()
}

fn earned_count(profile: &Profile) -> usize {
    table()
        .iter()
        .filter(|a| profile.achievements.contains_key(&a.key))
        .count()
}

/// `(earned, all)` in a category.
fn category_counts(profile: &Profile, category: &str) -> (usize, usize) {
    let all = table().iter().filter(|a| a.category == category);
    let done = all
        .clone()
        .filter(|a| profile.achievements.contains_key(&a.key))
        .count();
    (done, all.count())
}

/// The newest earned first, up to `n`.
fn recent(profile: &Profile, n: usize) -> Vec<&'static Achievement> {
    let mut v: Vec<(u32, usize, &'static Achievement)> = table()
        .iter()
        .enumerate()
        .filter_map(|(i, a)| profile.achievements.get(&a.key).map(|&d| (d, i, a)))
        .collect();
    v.sort_by(|a, b| b.0.cmp(&a.0).then(b.1.cmp(&a.1)));
    v.into_iter().take(n).map(|(_, _, a)| a).collect()
}

/// The unearned ones that are furthest along, up to `n`.
fn almost(profile: &Profile, n: usize) -> Vec<&'static Achievement> {
    let mut v: Vec<(f32, &'static Achievement)> = table()
        .iter()
        .filter(|a| !profile.achievements.contains_key(&a.key))
        .map(|a| {
            let (have, need) = progress(profile, &a.goal);
            (frac(have, need), a)
        })
        .filter(|(f, _)| *f > 0.0)
        .collect();
    v.sort_by(|a, b| b.0.partial_cmp(&a.0).unwrap_or(std::cmp::Ordering::Equal));
    v.into_iter().take(n).map(|(_, a)| a).collect()
}

// --- The perks. ---

/// A reward bought with points. The ranks are saved under `key`.
struct Perk {
    key: &'static str,
    name: &'static str,
    icon: &'static str,
    max: u8,
    /// Points per rank.
    cost: u32,
    desc: &'static str,
    /// What the run gets at a given rank.
    effect: fn(u8) -> String,
}

fn plural(n: u32, word: &str) -> String {
    format!("{n} {word}{}", if n == 1 { "" } else { "s" })
}

fn effect_rerolls(rank: u8) -> String {
    format!("{} per run", plural(1 + u32::from(rank), "reroll"))
}

fn effect_banishes(rank: u8) -> String {
    format!("{} per run", plural(u32::from(rank), "banish"))
}

fn effect_first_hand(rank: u8) -> String {
    format!("The first level-up deals {} cards", if rank > 0 { 4 } else { 3 })
}

static PERKS: [Perk; 3] = [
    Perk {
        key: "second_thoughts",
        name: "Second Thoughts",
        icon: "Spell_Nature_Reincarnation",
        max: 3,
        cost: 15,
        desc: "Deal a level-up hand again.",
        effect: effect_rerolls,
    },
    Perk {
        key: "banish",
        name: "Banish",
        icon: "Spell_Shadow_Cripple",
        max: 3,
        cost: 20,
        desc: "A banished choice never comes back this run.",
        effect: effect_banishes,
    },
    Perk {
        key: "starting_hand",
        name: "Starting Hand",
        icon: "INV_Misc_Note_01",
        max: 1,
        cost: 25,
        desc: "One more card to pick from at the first level-up.",
        effect: effect_first_hand,
    },
];

fn rank(perks: &BTreeMap<String, u8>, key: &str) -> u8 {
    let max = PERKS.iter().find(|p| p.key == key).map_or(0, |p| p.max);
    perks.get(key).copied().unwrap_or(0).min(max)
}

/// What a run gets from the ranks bought.
pub(crate) fn run_perks(perks: &BTreeMap<String, u8>) -> RunPerks {
    RunPerks {
        rerolls: 1 + u32::from(rank(perks, "second_thoughts")),
        banishes: u32::from(rank(perks, "banish")),
        skip: true,
        first_hand: if rank(perks, "starting_hand") > 0 { 4 } else { 3 },
    }
}

fn spent_points(perks: &BTreeMap<String, u8>) -> u32 {
    PERKS
        .iter()
        .map(|p| u32::from(rank(perks, p.key)) * p.cost)
        .sum()
}

fn available_points(profile: &Profile) -> u32 {
    earned_points(profile).saturating_sub(spent_points(&profile.perks))
}

fn can_buy(profile: &Profile, index: usize) -> bool {
    PERKS.get(index).is_some_and(|p| {
        rank(&profile.perks, p.key) < p.max && available_points(profile) >= p.cost
    })
}

/// Buy the next rank of a perk; false when maxed or too poor.
fn buy_perk(profile: &mut Profile, index: usize) -> bool {
    if !can_buy(profile, index) {
        return false;
    }
    let key = PERKS[index].key;
    let next = rank(&profile.perks, key) + 1;
    profile.perks.insert(key.to_string(), next);
    true
}

// --- Systems. ---

pub(crate) fn plugin(app: &mut App) {
    app.init_resource::<AchievementsUi>()
        .init_resource::<RunPerks>()
        .init_resource::<AchView>()
        .add_systems(
            OnEnter(Phase::GameOver),
            count_run.after(super::profile::award_run),
        )
        // The only way out of the menu is into a run.
        .add_systems(OnExit(Phase::Menu), set_run_perks)
        .add_systems(
            Update,
            (build, show_panel, clicks, rebuild, button_states)
                .chain()
                // After the keys, which read the other windows' flags first.
                .after(hud::keys)
                .before(GlueVisuals),
        );
}

/// `OnEnter(GameOver)`, after the payout: count the run, earn what it reaches, say so in chat.
fn count_run(
    run: Res<Run>,
    selection: Res<Selection>,
    mut profile: ResMut<Profile>,
    mut chat: MessageWriter<ChatLine>,
) {
    let Some(class) = data::CLASSES
        .iter()
        .position(|c| c.id == run.class.id && c.name == run.class.name)
    else {
        return;
    };
    let gold = profile
        .last_award
        .map_or(u64::from(run.gold), |award| award.gold);
    let end = RunEnd {
        kills: run.kills,
        level: run.level,
        secs: run.elapsed,
        bosses: run.bosses,
        gold,
        won: run.won,
        difficulty: run.difficulty,
        class,
        map: selection.stage,
    };
    record_run(&mut profile.counters, &end);
    for a in grant(&mut profile, today()) {
        chat.write(ChatLine::system(format!(
            "Achievement earned: {} ({})",
            a.name, a.points
        )));
    }
    // Saves nothing for an automated run.
    profile.save();
}

/// `OnExit(Menu)`: the run starts with the perks bought so far.
fn set_run_perks(profile: Res<Profile>, mut perks: ResMut<RunPerks>) {
    *perks = run_perks(&profile.perks);
}

// --- The window. ---

/// What the window's right side shows.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
enum View {
    #[default]
    Overview,
    /// An index into [`CATEGORIES`].
    Category(usize),
    Rewards,
}

#[derive(Resource, Default)]
struct AchView(View);

/// The dimmed full-screen layer holding the window, hidden until opened.
#[derive(Component)]
struct AchPanel;
/// The row inside the window whose children are rebuilt whenever the state changes.
#[derive(Component)]
struct AchBody;
/// On [`MenuRoot`] once the window is built into it.
#[derive(Component)]
struct AchBuilt;

#[derive(Component, Clone, Copy, Debug, PartialEq)]
enum AchAction {
    Close,
    Select(View),
    /// Buy the next rank of [`PERKS`]`[i]`.
    Buy(usize),
    Reset,
}

const PANEL_W: f32 = 1000.0;
const PANEL_H: f32 = 660.0;
const LEFT_W: f32 = 280.0;
const QUESTION: &str = "Interface\\Icons\\INV_Misc_QuestionMark.blp";
const GOLD: Color = kit::TITLE;
const BAR_FILL: Color = Color::srgb(0.78, 0.60, 0.25);
const SELECTED_FILL: Color = Color::srgba(0.26, 0.18, 0.07, 0.6);

fn px(v: f32) -> Val {
    Val::Px(v)
}

fn nowrap() -> TextLayout {
    TextLayout::new(Justify::Left, LineBreak::NoWrap)
}

/// Icon textures from the game data.
struct Gfx<'a> {
    assets: Option<&'a mut benilla_assets::WorldAssets>,
    images: &'a mut Assets<Image>,
}

impl Gfx<'_> {
    /// An icon image; invisible when the data is not there, grey when `dim`.
    fn icon(&mut self, name: &str, dim: bool) -> ImageNode {
        let path = format!("Interface\\Icons\\{name}.blp");
        let handle = self.assets.as_deref_mut().and_then(|a| {
            a.sprite_texture(&path, self.images)
                .or_else(|| a.sprite_texture(QUESTION, self.images))
        });
        match handle {
            Some(h) => ImageNode {
                color: if dim {
                    Color::srgb(0.4, 0.4, 0.4)
                } else {
                    Color::WHITE
                },
                ..ImageNode::new(h)
            },
            None => ImageNode {
                color: Color::NONE,
                ..default()
            },
        }
    }
}

/// Build the window into the pick screen once it is built.
fn build(
    mut commands: Commands,
    roots: Query<Entity, (With<MenuRoot>, With<MenuBuilt>, Without<AchBuilt>)>,
    fonts: Res<KitFonts>,
) {
    let Ok(root) = roots.single() else { return };
    commands
        .entity(root)
        .insert(AchBuilt)
        .with_children(|ui| {
            window(ui, &fonts);
        });
}

/// The frame: heading, the body row (filled by [`rebuild`]) and Close.
fn window(ui: &mut ChildSpawnerCommands, fonts: &KitFonts) {
    ui.spawn((
        AchPanel,
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
            kit::heading(win, fonts, "Achievements", 26.0);
            win.spawn((
                AchBody,
                Node {
                    width: Val::Percent(100.0),
                    flex_grow: 1.0,
                    min_height: px(0.0),
                    column_gap: px(20.0),
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
                    AchAction::Close,
                );
            });
        });
    });
}

/// Close on Esc or when the pick screen goes; for an automated run, `WOW_SURVIVORS_ACHIEVEMENTS`
/// (`overview`, `zones`, `classes`, `feats`, `totals`, `collection` or `rewards`) opens the window
/// on that page with made-up progress, which is never saved. The display follows
/// [`AchievementsUi::open`].
fn show_panel(
    mut ui: ResMut<AchievementsUi>,
    mut view: ResMut<AchView>,
    phase: Res<State<Phase>>,
    keys: Res<ButtonInput<KeyCode>>,
    mut profile: ResMut<Profile>,
    mut panel: Query<&mut Node, With<AchPanel>>,
    mut auto: Local<bool>,
) {
    if ui.open && (*phase.get() != Phase::Menu || keys.just_pressed(KeyCode::Escape)) {
        ui.open = false;
    }
    if profile.ephemeral && *phase.get() == Phase::Menu && panel.iter().next().is_some() && !*auto
    {
        if let Some(mode) = benilla_app::survivors_host::webenv::var("WOW_SURVIVORS_ACHIEVEMENTS")
        {
            *auto = true;
            view.0 = match mode.to_lowercase().as_str() {
                "rewards" => View::Rewards,
                other => CATEGORIES
                    .iter()
                    .position(|c| c.to_lowercase() == other)
                    .map_or(View::Overview, View::Category),
            };
            seed_demo(&mut profile);
            ui.open = true;
        }
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

/// Made-up progress for a screenshot of an automated run.
fn seed_demo(profile: &mut Profile) {
    if !profile.achievements.is_empty() {
        return;
    }
    for (key, value) in [
        ("kills", 12_400),
        ("gold", 49_449),
        ("bosses", 63),
        ("runs", 18),
        ("best_kills", 1_240),
        ("best_level", 24),
        ("best_minutes", 14),
        ("best_bosses", 3),
        ("won_zone_0", 1),
        ("won_zone_5", 1),
        ("won_zone_6", 1),
        ("won_class_0", 1),
        ("won_class_2", 1),
    ] {
        profile.counters.insert(key.to_string(), value);
    }
    for i in 0..14 {
        profile.collection.insert(format!("Demo Item {i}"));
    }
    let day = today();
    grant(profile, day);
    for (i, d) in profile.achievements.values_mut().enumerate() {
        *d = day.saturating_sub(i as u32 % 12);
    }
    profile.perks.insert("second_thoughts".to_string(), 1);
}

/// The window's buttons and rows: close, pick a page, buy a rank, reset.
fn clicks(
    actions: Query<(&Interaction, &AchAction, Option<&KitButton>), Changed<Interaction>>,
    phase: Res<State<Phase>>,
    mut profile: ResMut<Profile>,
    mut view: ResMut<AchView>,
    mut ui: ResMut<AchievementsUi>,
) {
    if *phase.get() != Phase::Menu || !ui.open {
        return;
    }
    for (interaction, action, button) in &actions {
        if *interaction != Interaction::Pressed || button.is_some_and(|b| b.disabled) {
            continue;
        }
        match *action {
            AchAction::Close => ui.open = false,
            AchAction::Select(v) => view.0 = v,
            AchAction::Buy(i) => {
                if buy_perk(&mut profile, i) {
                    profile.save();
                }
            }
            AchAction::Reset => {
                if !profile.perks.is_empty() {
                    profile.perks.clear();
                    profile.save();
                }
            }
        }
        return;
    }
}

/// The "+" and Reset buttons grey out when they would do nothing.
fn button_states(
    ui: Res<AchievementsUi>,
    profile: Res<Profile>,
    mut buttons: Query<(&AchAction, &mut KitButton)>,
) {
    if !ui.open {
        return;
    }
    for (action, mut button) in &mut buttons {
        let off = match *action {
            AchAction::Buy(i) => !can_buy(&profile, i),
            AchAction::Reset => profile.perks.values().all(|&r| r == 0),
            _ => false,
        };
        if button.disabled != off {
            button.disabled = off;
        }
    }
}

/// What the body shows depends on; rebuilt when any of it changes.
#[derive(Clone, PartialEq)]
struct Signature {
    view: View,
    earned: usize,
    perks: BTreeMap<String, u8>,
    collection: usize,
    counters: u64,
    assets: bool,
}

/// While open, rebuild the body whenever what it shows changed.
#[allow(clippy::too_many_arguments)]
fn rebuild(
    mut commands: Commands,
    ui: Res<AchievementsUi>,
    view: Res<AchView>,
    profile: Res<Profile>,
    fonts: Res<KitFonts>,
    assets: Option<ResMut<benilla_assets::WorldAssets>>,
    mut images: ResMut<Assets<Image>>,
    body: Query<Entity, With<AchBody>>,
    mut last: Local<Option<Signature>>,
) {
    if !ui.open {
        return;
    }
    let Ok(body) = body.single() else { return };
    let sig = Signature {
        view: view.0,
        earned: profile.achievements.len(),
        perks: profile.perks.clone(),
        collection: profile.collection.len(),
        counters: profile.counters.values().sum(),
        assets: assets.is_some(),
    };
    if last.as_ref() == Some(&sig) {
        return;
    }
    *last = Some(sig);

    let mut assets = assets;
    let mut gfx = Gfx {
        assets: assets.as_deref_mut(),
        images: &mut images,
    };
    commands.entity(body).despawn_children();
    commands.entity(body).with_children(|row| {
        left_column(row, &fonts, &profile, view.0);
        row.spawn(Node {
            flex_grow: 1.0,
            flex_basis: px(0.0),
            min_width: px(0.0),
            flex_direction: FlexDirection::Column,
            row_gap: px(8.0),
            ..default()
        })
        .with_children(|col| match view.0 {
            View::Overview => overview(col, &fonts, &mut gfx, &profile),
            View::Category(i) => category(col, &fonts, &mut gfx, &profile, i),
            View::Rewards => rewards(col, &fonts, &mut gfx, &profile),
        });
    });
}

// --- Widgets. ---

/// A round badge with a 2-px edge, `d` across; its text goes in as a child.
fn circle(d: f32, edge: Color) -> impl Bundle {
    (
        Node {
            width: px(d),
            height: px(d),
            flex_shrink: 0.0,
            flex_direction: FlexDirection::Column,
            justify_content: JustifyContent::Center,
            align_items: AlignItems::Center,
            border: UiRect::all(px(2.0)),
            border_radius: BorderRadius::all(Val::Percent(50.0)),
            ..default()
        },
        BackgroundColor(Color::srgba(0.0, 0.0, 0.0, 0.55)),
        BorderColor::all(edge),
    )
}

/// A thin bar filled to `fraction`; `outer` sets where it sits and how wide.
fn bar(parent: &mut ChildSpawnerCommands, fraction: f32, height: f32, fill: Color, outer: Node) {
    parent
        .spawn((
            Node {
                height: px(height),
                border: UiRect::all(px(1.0)),
                border_radius: BorderRadius::all(px(2.0)),
                overflow: Overflow::clip(),
                ..outer
            },
            BackgroundColor(Color::srgba(0.0, 0.0, 0.0, 0.6)),
            BorderColor::all(kit::INSET_EDGE),
        ))
        .with_children(|b| {
            b.spawn((
                Node {
                    width: Val::Percent(fraction.clamp(0.0, 1.0) * 100.0),
                    height: Val::Percent(100.0),
                    ..default()
                },
                BackgroundColor(fill),
            ));
        });
}

fn full_bar(parent: &mut ChildSpawnerCommands, fraction: f32, height: f32) {
    bar(
        parent,
        fraction,
        height,
        BAR_FILL,
        Node {
            width: Val::Percent(100.0),
            ..default()
        },
    );
}

/// The left column: the points badge, the account progress, the pages.
fn left_column(row: &mut ChildSpawnerCommands, fonts: &KitFonts, profile: &Profile, view: View) {
    let earned = earned_points(profile);
    let all = table().len();
    let done = earned_count(profile);
    let pct = (done * 100).checked_div(all).unwrap_or(0);
    let spend = available_points(profile);
    row.spawn(Node {
        width: px(LEFT_W),
        flex_shrink: 0.0,
        flex_direction: FlexDirection::Column,
        row_gap: px(6.0),
        ..default()
    })
    .with_children(|col| {
        col.spawn(Node {
            width: Val::Percent(100.0),
            justify_content: JustifyContent::Center,
            margin: UiRect::vertical(px(2.0)),
            ..default()
        })
        .with_children(|c| {
            c.spawn(circle(100.0, GOLD)).with_children(|b| {
                b.spawn(kit::title(fonts, hud::grouped(u64::from(earned)), 40.0));
                b.spawn(kit::body(
                    fonts,
                    format!("of {}", hud::grouped(u64::from(total_points()))),
                    11.0,
                    kit::MUTED,
                ));
            });
        });
        col.spawn(Node {
            width: Val::Percent(100.0),
            justify_content: JustifyContent::SpaceBetween,
            ..default()
        })
        .with_children(|r| {
            r.spawn(kit::label(fonts, "Account progress"));
            r.spawn(kit::body(fonts, format!("{pct}%"), 12.0, GOLD));
        });
        bar(
            col,
            done as f32 / all.max(1) as f32,
            8.0,
            GOLD,
            Node {
                width: Val::Percent(100.0),
                margin: UiRect::bottom(px(2.0)),
                ..default()
            },
        );

        nav_row(col, fonts, "Overview", String::new(), None, View::Overview, view);
        for (i, name) in CATEGORIES.iter().enumerate() {
            let (d, n) = category_counts(profile, name);
            nav_row(
                col,
                fonts,
                name,
                format!("{d} / {n}"),
                Some(d as f32 / n.max(1) as f32),
                View::Category(i),
                view,
            );
        }

        col.spawn(Node {
            margin: UiRect::top(px(8.0)),
            ..default()
        })
        .with_children(|h| {
            h.spawn(kit::label(fonts, "Rewards"));
        });
        nav_row(
            col,
            fonts,
            "Rewards",
            format!("{spend} points to spend"),
            None,
            View::Rewards,
            view,
        );
    });
}

/// One clickable row of the left column.
fn nav_row(
    col: &mut ChildSpawnerCommands,
    fonts: &KitFonts,
    name: &str,
    right: String,
    fraction: Option<f32>,
    target: View,
    current: View,
) {
    let selected = target == current;
    col.spawn((
        Button,
        AchAction::Select(target),
        Node {
            width: Val::Percent(100.0),
            flex_direction: FlexDirection::Column,
            padding: UiRect::axes(px(10.0), px(4.0)),
            row_gap: px(3.0),
            border: UiRect::all(px(1.0)),
            border_radius: BorderRadius::all(px(3.0)),
            ..default()
        },
        BackgroundColor(if selected {
            SELECTED_FILL
        } else {
            kit::INSET_FILL
        }),
        BorderColor::all(if selected { GOLD } else { kit::INSET_EDGE }),
    ))
    .with_children(|r| {
        r.spawn(Node {
            width: Val::Percent(100.0),
            justify_content: JustifyContent::SpaceBetween,
            align_items: AlignItems::Center,
            ..default()
        })
        .with_children(|line| {
            line.spawn(kit::body(
                fonts,
                name,
                14.0,
                if selected { GOLD } else { kit::CREAM },
            ));
            line.spawn(kit::body(fonts, right, 12.0, kit::MUTED));
        });
        if let Some(f) = fraction {
            full_bar(r, f, 4.0);
        }
    });
}

/// A section's small-caps label.
fn section(col: &mut ChildSpawnerCommands, fonts: &KitFonts, caption: &str) {
    col.spawn(kit::label(fonts, caption));
}

/// A two-column wrapping grid for [`card`]s.
fn grid(col: &mut ChildSpawnerCommands, cards: impl FnOnce(&mut ChildSpawnerCommands)) {
    col.spawn(Node {
        width: Val::Percent(100.0),
        flex_direction: FlexDirection::Row,
        flex_wrap: FlexWrap::Wrap,
        column_gap: px(8.0),
        row_gap: px(5.0),
        ..default()
    })
    .with_children(cards);
}

/// An achievement: icon, name, description and the date earned, or the progress; the points on
/// the right. Dimmed until earned.
fn card(
    col: &mut ChildSpawnerCommands,
    fonts: &KitFonts,
    gfx: &mut Gfx,
    profile: &Profile,
    a: &Achievement,
) {
    let day = profile.achievements.get(&a.key).copied();
    let earned = day.is_some();
    let (have, need) = progress(profile, &a.goal);
    let icon = gfx.icon(a.icon, !earned);
    col.spawn(kit::inset(Node {
        width: Val::Percent(49.0),
        height: px(56.0),
        align_items: AlignItems::Center,
        column_gap: px(8.0),
        padding: UiRect::axes(px(8.0), px(4.0)),
        ..default()
    }))
    .with_children(|c| {
        c.spawn((
            icon,
            Node {
                width: px(38.0),
                height: px(38.0),
                flex_shrink: 0.0,
                border: UiRect::all(px(1.0)),
                ..default()
            },
            BorderColor::all(if earned {
                kit::PANEL_EDGE
            } else {
                kit::INSET_EDGE
            }),
        ));
        c.spawn(Node {
            flex_grow: 1.0,
            flex_basis: px(0.0),
            min_width: px(0.0),
            flex_direction: FlexDirection::Column,
            row_gap: px(1.0),
            overflow: Overflow::clip(),
            ..default()
        })
        .with_children(|m| {
            m.spawn((
                kit::body(fonts, &a.name, 13.0, if earned { GOLD } else { kit::MUTED }),
                nowrap(),
            ));
            m.spawn((
                kit::body(
                    fonts,
                    &a.desc,
                    11.0,
                    if earned { kit::CREAM } else { kit::DISABLED },
                ),
                nowrap(),
            ));
            if let Some(day) = day {
                m.spawn((kit::body(fonts, date_string(day), 10.0, kit::MUTED), nowrap()));
            } else {
                m.spawn(Node {
                    width: Val::Percent(100.0),
                    align_items: AlignItems::Center,
                    column_gap: px(6.0),
                    ..default()
                })
                .with_children(|r| {
                    bar(
                        r,
                        frac(have, need),
                        6.0,
                        BAR_FILL,
                        Node {
                            flex_grow: 1.0,
                            flex_basis: px(0.0),
                            ..default()
                        },
                    );
                    r.spawn((
                        kit::body(
                            fonts,
                            format!(
                                "{} / {}",
                                hud::grouped(have.min(need)),
                                hud::grouped(need)
                            ),
                            10.0,
                            kit::MUTED,
                        ),
                        nowrap(),
                    ));
                });
            }
        });
        c.spawn(circle(30.0, if earned { GOLD } else { kit::INSET_EDGE }))
            .with_children(|b| {
                b.spawn(kit::text(
                    &fonts.title,
                    a.points.to_string(),
                    13.0,
                    if earned { GOLD } else { kit::DISABLED },
                ));
            });
    });
}

fn overview(col: &mut ChildSpawnerCommands, fonts: &KitFonts, gfx: &mut Gfx, profile: &Profile) {
    section(col, fonts, "Recently earned");
    let latest = recent(profile, 4);
    if latest.is_empty() {
        col.spawn(kit::body(
            fonts,
            "Nothing earned yet. Win a run or kill a few enemies.",
            13.0,
            kit::MUTED,
        ));
    } else {
        grid(col, |g| {
            for a in latest {
                card(g, fonts, gfx, profile, a);
            }
        });
    }
    section(col, fonts, "Almost there");
    let close = almost(profile, 4);
    if close.is_empty() {
        col.spawn(kit::body(
            fonts,
            "Nothing in progress yet.",
            13.0,
            kit::MUTED,
        ));
    } else {
        grid(col, |g| {
            for a in close {
                card(g, fonts, gfx, profile, a);
            }
        });
    }
}

fn category(
    col: &mut ChildSpawnerCommands,
    fonts: &KitFonts,
    gfx: &mut Gfx,
    profile: &Profile,
    index: usize,
) {
    let Some(&name) = CATEGORIES.get(index) else {
        return;
    };
    let (done, all) = category_counts(profile, name);
    let points: u32 = table()
        .iter()
        .filter(|a| a.category == name && profile.achievements.contains_key(&a.key))
        .map(|a| a.points)
        .sum();
    col.spawn(Node {
        width: Val::Percent(100.0),
        justify_content: JustifyContent::SpaceBetween,
        align_items: AlignItems::Center,
        ..default()
    })
    .with_children(|h| {
        h.spawn(kit::title(fonts, name, 20.0));
        h.spawn(kit::body(
            fonts,
            format!("{done} / {all} earned, {points} points"),
            12.0,
            kit::MUTED,
        ));
    });
    grid(col, |g| {
        for a in table().iter().filter(|a| a.category == name) {
            card(g, fonts, gfx, profile, a);
        }
    });
}

fn rewards(col: &mut ChildSpawnerCommands, fonts: &KitFonts, gfx: &mut Gfx, profile: &Profile) {
    let earned = earned_points(profile);
    let spent = spent_points(&profile.perks);
    let spend = earned.saturating_sub(spent);
    col.spawn(Node {
        width: Val::Percent(100.0),
        justify_content: JustifyContent::SpaceBetween,
        align_items: AlignItems::Center,
        ..default()
    })
    .with_children(|h| {
        h.spawn(kit::title(fonts, "Rewards", 20.0));
        h.spawn(kit::body(
            fonts,
            format!("{spend} points to spend"),
            15.0,
            GOLD,
        ));
    });
    col.spawn(kit::body(
        fonts,
        format!("{earned} points earned, {spent} spent. Perks apply from the next run."),
        12.0,
        kit::MUTED,
    ));
    for (i, perk) in PERKS.iter().enumerate() {
        perk_row(col, fonts, gfx, profile, i, perk);
    }
    col.spawn(Node {
        width: Val::Percent(100.0),
        justify_content: JustifyContent::FlexEnd,
        margin: UiRect::top(Val::Auto),
        ..default()
    })
    .with_children(|foot| {
        kit::button(
            foot,
            fonts,
            KitStyle::Normal,
            "Reset",
            None,
            Node {
                width: px(120.0),
                height: px(32.0),
                ..default()
            },
            AchAction::Reset,
        );
    });
}

fn perk_row(
    col: &mut ChildSpawnerCommands,
    fonts: &KitFonts,
    gfx: &mut Gfx,
    profile: &Profile,
    index: usize,
    perk: &Perk,
) {
    let have = rank(&profile.perks, perk.key);
    let maxed = have >= perk.max;
    let affordable = available_points(profile) >= perk.cost;
    let icon = gfx.icon(perk.icon, false);
    col.spawn(kit::inset(Node {
        width: Val::Percent(100.0),
        height: px(76.0),
        align_items: AlignItems::Center,
        column_gap: px(12.0),
        padding: UiRect::axes(px(12.0), px(8.0)),
        ..default()
    }))
    .with_children(|r| {
        r.spawn((
            icon,
            Node {
                width: px(44.0),
                height: px(44.0),
                flex_shrink: 0.0,
                border: UiRect::all(px(1.0)),
                ..default()
            },
            BorderColor::all(kit::PANEL_EDGE),
        ));
        r.spawn(Node {
            flex_grow: 1.0,
            flex_basis: px(0.0),
            min_width: px(0.0),
            flex_direction: FlexDirection::Column,
            row_gap: px(2.0),
            ..default()
        })
        .with_children(|m| {
            m.spawn(Node {
                align_items: AlignItems::Center,
                column_gap: px(10.0),
                ..default()
            })
            .with_children(|line| {
                line.spawn(kit::title(fonts, perk.name, 17.0));
                line.spawn(Node {
                    column_gap: px(4.0),
                    ..default()
                })
                .with_children(|pips| {
                    for n in 0..perk.max {
                        pips.spawn((
                            Node {
                                width: px(10.0),
                                height: px(10.0),
                                border: UiRect::all(px(1.0)),
                                border_radius: BorderRadius::all(Val::Percent(50.0)),
                                ..default()
                            },
                            BackgroundColor(if n < have { GOLD } else { kit::INSET_FILL }),
                            BorderColor::all(kit::PANEL_EDGE),
                        ));
                    }
                });
            });
            let (text, color) = if maxed {
                (format!("Maxed: {}", (perk.effect)(have)), kit::BONUS)
            } else {
                (format!("Next rank: {}", (perk.effect)(have + 1)), kit::CREAM)
            };
            m.spawn(kit::body(fonts, text, 13.0, color));
            m.spawn(kit::body(fonts, perk.desc, 11.0, kit::MUTED));
        });
        r.spawn(Node {
            align_items: AlignItems::Center,
            column_gap: px(10.0),
            flex_shrink: 0.0,
            ..default()
        })
        .with_children(|buy| {
            let (cost, color) = if maxed {
                ("Maxed".to_string(), kit::MUTED)
            } else if affordable {
                (format!("{} points", perk.cost), GOLD)
            } else {
                (format!("{} points", perk.cost), kit::WARN)
            };
            buy.spawn(kit::body(fonts, cost, 13.0, color));
            kit::button(
                buy,
                fonts,
                KitStyle::Normal,
                "+",
                None,
                Node {
                    width: px(44.0),
                    height: px(32.0),
                    ..default()
                },
                AchAction::Buy(index),
            );
        });
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    fn end() -> RunEnd {
        RunEnd {
            kills: 500,
            level: 12,
            secs: 650.0,
            bosses: 2,
            gold: 300,
            won: false,
            difficulty: Difficulty::Normal,
            class: 1,
            map: 3,
        }
    }

    #[test]
    fn keys_are_unique_and_points_are_valid() {
        let mut keys: Vec<&str> = table().iter().map(|a| a.key.as_str()).collect();
        let n = keys.len();
        keys.sort_unstable();
        keys.dedup();
        assert_eq!(keys.len(), n);
        assert!(n >= 40);
        for a in table() {
            assert!([10, 25, 50].contains(&a.points), "{}", a.key);
            assert!(CATEGORIES.contains(&a.category), "{}", a.key);
            assert!(!a.name.contains('\u{2014}') && !a.desc.contains('\u{2014}'));
        }
        for c in CATEGORIES {
            assert!(table().iter().any(|a| a.category == c), "{c}");
        }
    }

    #[test]
    fn the_undying_run_is_worth_fifty_and_wears_a_map() {
        let i = data::MAPS
            .iter()
            .position(|m| m.name == "Undying Run")
            .expect("the Undying Run is a battleground");
        let a = table()
            .iter()
            .find(|a| a.key == format!("zone_{i}"))
            .expect("a zone achievement");
        assert_eq!(a.points, 50);
        assert_eq!(a.icon, "INV_Misc_Map_01");
        assert_eq!(a.name, "Undying Run Conquered");
    }

    #[test]
    fn one_run_is_counted() {
        let mut c = BTreeMap::new();
        record_run(&mut c, &end());
        record_run(&mut c, &RunEnd { kills: 200, level: 20, ..end() });
        assert_eq!(c["kills"], 700);
        assert_eq!(c["gold"], 600);
        assert_eq!(c["bosses"], 4);
        assert_eq!(c["runs"], 2);
        assert_eq!(c["best_kills"], 500);
        assert_eq!(c["best_level"], 20);
        assert_eq!(c["best_minutes"], 10);
        assert!(!c.contains_key("won_zone_3"));
    }

    #[test]
    fn a_win_counts_zone_class_and_difficulty() {
        let mut c = BTreeMap::new();
        record_run(&mut c, &RunEnd { won: true, ..end() });
        assert_eq!(c["won_zone_3"], 1);
        assert_eq!(c["won_class_1"], 1);
        assert!(!c.contains_key("won_heroic"));
        record_run(
            &mut c,
            &RunEnd { won: true, difficulty: Difficulty::Hardcore, ..end() },
        );
        assert_eq!(c["won_zone_3"], 2);
        assert_eq!(c["won_heroic"], 1);
        assert_eq!(c["won_hardcore"], 1);
    }

    #[test]
    fn achievements_are_earned_once() {
        let mut p = Profile::default();
        assert!(grant(&mut p, 100).is_empty());
        p.counters.insert("kills".into(), 1_500);
        let got = grant(&mut p, 100);
        assert_eq!(got.len(), 1);
        assert_eq!(got[0].key, "total_kills_1k");
        assert_eq!(p.achievements["total_kills_1k"], 100);
        // Already earned: not again, and the day stays.
        assert!(grant(&mut p, 200).is_empty());
        assert_eq!(p.achievements["total_kills_1k"], 100);
    }

    #[test]
    fn collection_and_levels_count() {
        let mut p = Profile::default();
        for i in 0..10 {
            p.collection.insert(format!("Item {i}"));
        }
        p.characters[2].level = 31;
        let keys: Vec<&str> = grant(&mut p, 1).iter().map(|a| a.key.as_str()).collect();
        assert!(keys.contains(&"coll_10"));
        assert!(keys.contains(&"total_level_30"));
        assert!(!keys.contains(&"coll_25"));
        assert!(!keys.contains(&"total_level_60"));
    }

    #[test]
    fn points_add_up_and_perks_cost() {
        let mut p = Profile::default();
        assert_eq!(earned_points(&p), 0);
        p.achievements.insert("total_kills_1k".into(), 1);
        p.achievements.insert("total_kills_10k".into(), 1);
        p.achievements.insert("total_kills_100k".into(), 1);
        assert_eq!(earned_points(&p), 85);
        assert_eq!(available_points(&p), 85);
        assert!(buy_perk(&mut p, 0));
        assert!(buy_perk(&mut p, 0));
        assert_eq!(spent_points(&p.perks), 30);
        assert_eq!(available_points(&p), 55);
        // Banish costs 20 a rank; the starting hand 25.
        assert!(buy_perk(&mut p, 1));
        assert!(buy_perk(&mut p, 2));
        assert_eq!(available_points(&p), 10);
        assert!(!can_buy(&p, 0));
        assert!(!buy_perk(&mut p, 1));
        // The starting hand has one rank.
        p.achievements.insert("zone_0".into(), 1);
        p.achievements.insert("zone_1".into(), 1);
        p.achievements.insert("zone_2".into(), 1);
        assert!(!can_buy(&p, 2));
        assert!(!can_buy(&p, 99));
    }

    #[test]
    fn run_perks_follow_the_ranks() {
        assert_eq!(run_perks(&BTreeMap::new()), RunPerks::default());
        let perks: BTreeMap<String, u8> = [
            ("second_thoughts".to_string(), 2),
            ("banish".to_string(), 3),
            ("starting_hand".to_string(), 1),
            ("unknown".to_string(), 9),
        ]
        .into();
        assert_eq!(
            run_perks(&perks),
            RunPerks {
                rerolls: 3,
                banishes: 3,
                skip: true,
                first_hand: 4,
            }
        );
        // A rank beyond the maximum counts as the maximum.
        let over: BTreeMap<String, u8> = [("banish".to_string(), 9)].into();
        assert_eq!(run_perks(&over).banishes, 3);
    }

    #[test]
    fn dates_read_like_the_client() {
        assert_eq!(date_string(0), "1 Jan 1970");
        assert_eq!(date_string(10_957), "1 Jan 2000");
        assert_eq!(date_string(20_754), "28 Oct 2026");
        assert_eq!(date_string(19_782), "29 Feb 2024");
    }

    #[test]
    fn recent_and_almost_pick_the_right_ones() {
        let mut p = Profile::default();
        p.achievements.insert("zone_0".into(), 10);
        p.achievements.insert("zone_1".into(), 30);
        p.achievements.insert("class_0".into(), 20);
        let r = recent(&p, 2);
        assert_eq!(r[0].key, "zone_1");
        assert_eq!(r[1].key, "class_0");
        p.counters.insert("gold".into(), 4_000);
        p.counters.insert("kills".into(), 100);
        let a = almost(&p, 4);
        assert_eq!(a[0].key, "total_gold_5k");
        assert!(a.iter().all(|x| !p.achievements.contains_key(&x.key)));
    }
}
