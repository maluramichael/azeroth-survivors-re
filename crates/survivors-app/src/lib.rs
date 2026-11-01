//! Warcraft Survivors: a top-down survival mode on the benilla world, started by
//! `benilla_app::run_survivors` with [`plugin`]. There is no server: the hero, the enemies and the
//! spells are local entities dressed by the same attach, animation and spell-visual systems a
//! streamed unit gets, on a battleground picked at the menu ([`data::MAPS`]), with the player
//! controller and FrameXML off (`CaptureMode`).

// A Bevy system's query types are its dependency declaration, as its parameter list is (the
// workspace allows `too_many_arguments` for that reason); factoring them into aliases hides them.
#![allow(clippy::type_complexity)]

mod abilities;
mod achievements;
mod antialias;
mod auto;
mod balance;
mod barrier;
mod bossbar;
mod bosses;
mod cards;
mod charpanel;
mod chests;
mod chatlog;
#[cfg(all(feature = "coop", not(target_arch = "wasm32")))]
mod coop;
mod collection;
mod data;
mod debug;
mod difficulty;
mod dmgmeter;
mod dropart;
mod enrage;
mod enemies;
mod evolutions;
mod fill;
mod finale;
mod focus;
mod framerate;
mod gfxpresets;
mod frames;
mod grade;
mod guards;
mod hero;
mod hud;
mod items;
mod kit;
mod loottables;
mod menu;
mod minimap;
mod modding;
mod mobility;
mod mods;
mod nav;
mod options;
mod pickups;
mod plates;
mod prefs;
mod talents;
mod threats;
mod textfield;
mod profile;
mod quests;
mod questlog;
mod races;
mod rares;
mod route;
mod route_hud;
mod route_run;
#[cfg(not(target_arch = "wasm32"))]
mod shots;
mod records;
mod trainer;
mod surge;
mod units;
mod vfx;
mod worldbuffs;
mod worldmap;

use benilla_world::schedule::WorldStage;
use bevy::prelude::*;

use data::{AbilityDef, ClassDef, PassiveDef, Stat};

/// The mode's entry, handed to `benilla_app::run_survivors` by the launcher and the browser entry.
pub fn plugin(app: &mut App) {
    app.add_plugins(SurvivorsPlugin);
}

pub(crate) struct SurvivorsPlugin;

impl Plugin for SurvivorsPlugin {
    fn build(&self, app: &mut App) {
        // Present before `OnEnter(InWorld)`, where the UI lifecycle decides not to load FrameXML.
        #[cfg(all(feature = "coop", not(target_arch = "wasm32")))]
        coop::plugin(app);
        // F12 and the scripted test timeline; a browser has no folder to save into.
        #[cfg(not(target_arch = "wasm32"))]
        app.add_plugins(shots::plugin);
        app.insert_resource(benilla_app::survivors_host::run_mode::CaptureMode)
            .add_plugins((grade::plugin, kit::plugin, dmgmeter::plugin, worldmap::plugin))
            .add_plugins((collection::plugin, loottables::plugin, achievements::plugin))
            .add_plugins((modding::plugin, questlog::plugin, threats::plugin, framerate::plugin))
            .add_plugins((antialias::plugin, gfxpresets::plugin, vfx::plugin))
            .add_plugins((bosses::plugin, bossbar::plugin, guards::plugin, dropart::plugin, rares::plugin))
            .add_plugins((route::plugin, route_run::plugin, route_hud::plugin))
            .add_plugins((races::plugin, records::plugin, surge::plugin, enrage::plugin, prefs::plugin, textfield::plugin))
            .add_plugins((
                trainer::plugin,
                plates::plugin,
                barrier::plugin,
                menu::plugin,
                charpanel::plugin,
                nav::plugin,
                focus::plugin,
                chatlog::plugin,
                options::plugin,
            ))
            .add_plugins((
                talents::plugin,
                worldbuffs::plugin,
                difficulty::plugin,
                items::plugin,
                chests::plugin,
                pickups::plugin,
                finale::plugin,
                evolutions::plugin,
                quests::plugin,
                frames::plugin,
                debug::plugin,
            ))
            .init_state::<Phase>()
            .init_resource::<Rng>()
            .init_resource::<Selection>()
            .init_resource::<Run>()
            .init_resource::<hero::CamRig>()
            .init_resource::<hero::AimPoint>()
            .init_resource::<abilities::PendingHits>()
            .init_resource::<NetRole>()
            .init_resource::<GuestMail>()
            .init_resource::<enemies::Spawner>()
            .init_resource::<hud::Offer>()
            .init_resource::<cards::PendingPick>()
            .add_message::<abilities::Hit>()
            .add_message::<enemies::Loot>()
            .add_plugins(bevy::ui_render::UiMaterialPlugin::<hud::SweepUiMaterial>::default())
            .add_plugins(bevy::ui_render::UiMaterialPlugin::<fill::FillMaterial>::default())
            .add_systems(Startup, (hud::setup_hud, profile::load_profile))
            .add_systems(Update, hero::daylight)
            .add_systems(
                Update,
                (
                    hero::drop_stray_buttons,
                    hero::aim_cursor,
                    mobility::cast_mobility,
                    hero::drive_hero,
                    hero::place_camera,
                )
                    .chain()
                    .in_set(WorldStage::Input),
            )
            .add_systems(
                Update,
                (
                    hero::mark_picking,
                    enemies::spawn_enemies,
                    enemies::move_enemies,
                    abilities::cast_abilities,
                    abilities::tick_areas,
                    abilities::apply_hits,
                    enemies::tick_marks,
                    enemies::drop_loot,
                    enemies::road_heals,
                    enemies::collect_gems,
                    hero::hero_vitals,
                    hero::revive,
                )
                    .chain()
                    .after(WorldStage::Input)
                    .run_if(run_on)
                    .run_if(simulates)
                    // Entering a battleground raises its loading screen: the run waits behind it.
                    .run_if(|l: Res<benilla_app::survivors_host::loading_screen::LoadingScreen>| !l.covering()),
            )
            .add_systems(
                Update,
                (enemies::reap_dead, enemies::still_gems).after(WorldStage::Input),
            )
            .add_systems(
                Update,
                (
                    hero::follow_map,
                    hud::menu_preview,
                    hud::buttons,
                    hud::keys,
                    hud::refresh_menu,
                    menu::restore_pick,
                    menu::build_menu,
                    menu::menu_clicks,
                    menu::refresh_menu_ui.before(benilla_app::survivors_host::glue::GlueVisuals),
                    cards::deal_cards,
                    cards::choose_card,
                    cards::pick_in_time,
                    cards::animate_cards,
                    hud::refresh_hud,
                    hud::refresh_results,
                    hud::fill_icons,
                    hud::update_cooldown_sweeps,
                    hud::refresh_mobility,
                    hud::show_panels,
                )
                    .chain()
                    .after(WorldStage::Input),
            )
            .add_systems(
                Update,
                (
                    hud::apply_ui_scale,
                    hud::fill_art,
                    hud::xp_hover,
                    hud::icon_tooltip,
                    hud::hero_plate,
                    minimap::map_buttons,
                    minimap::zone_text,
                    minimap::compose,
                )
                    .chain()
                    .after(hud::show_panels),
            )
            .add_systems(OnEnter(Phase::Playing), unpause)
            .add_systems(OnEnter(Phase::LevelUp), (pause, hud::roll_offer))
            .add_systems(OnEnter(Phase::Paused), pause)
            .add_systems(OnEnter(Phase::GameOver), (unpause, profile::award_run))
            .add_systems(OnEnter(Phase::Menu), (unpause, reset_run))
            .add_systems(OnExit(Phase::Menu), seed_rng);
        if let Some(auto) = auto::from_env() {
            app.insert_resource(auto)
                .add_systems(
                    Update,
                    auto::drive.after(hud::keys).before(hud::refresh_menu),
                )
                .add_systems(Update, auto::plan.before(WorldStage::Input))
                .add_systems(Update, auto::tour.after(auto::drive));
        }
    }
}

/// The screen the mode is on.
#[derive(States, Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub(crate) enum Phase {
    #[default]
    Menu,
    Playing,
    LevelUp,
    Paused,
    GameOver,
}

/// Everything spawned for a run, despawned when the next begins.
#[derive(Component)]
pub(crate) struct RunEntity;

/// The class, look and battleground picked at the menu; the battleground holds for the run.
#[derive(Resource, Default)]
pub(crate) struct Selection {
    pub class: Option<usize>,
    /// `(display, race, sex)`, one per race and sex the class's trainers come in.
    pub looks: Vec<(u32, u8, u8)>,
    pub look: Option<usize>,
    /// An index into [`data::MAPS`].
    pub map: usize,
    /// The battleground the world is standing in: the last one entered (or the saved pick at
    /// boot). Picking another at the menu changes only `map`, so the menu hero stays put and no
    /// loading screen rises until Enter World.
    pub stage: usize,
    /// Bumped on every change, so the menu rebuilds once.
    pub version: u32,
}

impl Selection {
    pub fn class_def(&self) -> Option<&'static ClassDef> {
        self.class.map(|i| &data::CLASSES[i])
    }

    pub fn map_def(&self) -> &'static data::MapDef {
        &data::MAPS[self.map.min(data::MAPS.len() - 1)]
    }

    pub fn stage_def(&self) -> &'static data::MapDef {
        &data::MAPS[self.stage.min(data::MAPS.len() - 1)]
    }
}

/// An ability the hero has, with its own cooldown.
pub(crate) struct Owned {
    pub def: &'static AbilityDef,
    pub level: u8,
    pub cd: f32,
}

impl Owned {
    pub fn damage_mult(&self) -> f32 {
        1.0 + 0.3 * f32::from(self.level - 1)
    }
    pub fn cooldown(&self, stats: &Stats) -> f32 {
        let base = self.def.cooldown * 0.93f32.powi(i32::from(self.level) - 1);
        (base * (1.0 - stats.haste).max(0.35)).max(0.2)
    }
    pub fn count(&self) -> u32 {
        let extra = [3u8, 5, 7].iter().filter(|&&l| self.level >= l).count() as u32;
        self.def.count + extra
    }
    pub fn radius(&self, stats: &Stats) -> f32 {
        self.def.radius * (1.0 + 0.08 * f32::from(self.level - 1)) * stats.area
    }
    pub fn duration(&self) -> f32 {
        self.def.duration * (1.0 + 0.1 * f32::from(self.level - 1))
    }
}

/// The hero's derived numbers, from the base and the passives.
#[derive(Clone, Copy)]
pub(crate) struct Stats {
    pub might: f32,
    pub max_hp: f32,
    pub speed: f32,
    pub regen: f32,
    pub haste: f32,
    pub area: f32,
    pub magnet: f32,
    pub growth: f32,
    pub armor: f32,
    pub crit: f32,
}

pub(crate) const BASE_HP: f32 = 120.0;
pub(crate) const BASE_SPEED: f32 = 7.0;
pub(crate) const BASE_MAGNET: f32 = 4.5;

/// This machine's part in a co-op session ([`coop`]): alone, hosting, or a guest of a host.
/// Always present, so a build without co-op reads `Solo`.
#[derive(Resource, Default, Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) enum NetRole {
    #[default]
    Solo,
    Host,
    Client,
}

/// Whether the co-op window is open ([`coop`]); outside the module so the hub's tab and the
/// menu keys can see it in a build without co-op too (it is never opened there).
#[derive(Resource, Default)]
pub(crate) struct CoopUi {
    pub open: bool,
}

/// Words for one co-op guest about its own hero, kept on the host until [`coop`] sends them: a
/// chest it opened, an item or a shard it picked up, a quest's reward. Keyed by the guest's
/// puppet. Nothing is kept alone, where there are no puppets.
#[derive(Resource, Default)]
pub(crate) struct GuestMail(pub Vec<(Entity, Mail)>);

/// One word for a guest ([`GuestMail`]).
#[cfg_attr(not(all(feature = "coop", not(target_arch = "wasm32"))), allow(dead_code))]
#[derive(Clone, Debug, PartialEq)]
pub(crate) enum Mail {
    /// A centre-screen banner.
    Banner(String),
    /// A system line in the chat.
    Line(String),
    /// Gold looted, for the chat's money line.
    Gold(u32),
    /// An item picked up: the guest wears it as it would alone.
    Item(items::Item),
}

/// Run condition: the run's world moves. Alone only while playing; in a co-op party also behind
/// the level-up cards and the pause menu, which stop no one there.
pub(crate) fn run_on(phase: Res<State<Phase>>, role: Res<NetRole>) -> bool {
    match phase.get() {
        Phase::Playing => true,
        Phase::LevelUp | Phase::Paused => *role != NetRole::Solo,
        Phase::Menu | Phase::GameOver => false,
    }
}

/// The player's state folder (profile, bank, prefs, spellbook, the last pick, mods, shots). A unit
/// test of this crate gets none unless it pins `$BENILLA_HOME`: the engine's own `cfg!(test)` guard
/// only holds when the engine itself is under test, and without this the suite saved its fixture
/// profile (every character at level 60) over the player's.
pub(crate) fn state_home() -> Option<std::path::PathBuf> {
    if cfg!(test) && std::env::var_os("BENILLA_HOME").is_none() {
        return None;
    }
    benilla_app::survivors_host::local_state::home()
}

/// A test run that must not touch the player's progress: an automated run (`WOW_SURVIVORS_AUTO`)
/// or the debug mode (`WOW_SURVIVORS_DEBUG`). Its profile opens everything and saves nothing.
pub(crate) fn throwaway() -> bool {
    benilla_app::survivors_host::webenv::var("WOW_SURVIVORS_AUTO").is_some() || debug::active()
}

/// Run condition: this machine simulates the run (alone or as the host); a guest only shows the
/// host's.
pub(crate) fn simulates(role: Res<NetRole>) -> bool {
    *role != NetRole::Client
}

/// One run's state: what the party shares, and the local hero's own ([`Run::me`]). The run
/// derefs to the local hero, so `run.hp` is this machine's hero's health.
#[derive(Resource)]
pub(crate) struct Run {
    pub elapsed: f32,
    pub kills: u32,
    pub level: u32,
    pub xp: f32,
    pub pending_levels: u32,
    pub bosses: u32,
    pub rings: u32,
    pub won: bool,
    pub next_guid: u64,
    /// A centre-screen announcement and its seconds left.
    pub banner: Option<(String, f32)>,
    /// The last announcement for the whole party ([`Run::announce`]), numbered so a co-op guest
    /// shows each once; a banner about this hero alone goes straight into `banner`.
    pub party_banner: (u32, String),
    /// Picked at the pick screen ([`difficulty`]).
    pub difficulty: difficulty::Difficulty,
    /// Heroes in the run, this one included: 1 alone, up to 4 in a co-op party. Sizes the horde.
    pub party: u32,
    /// Every hero of the run is down (alone: this one): the horde, the drops and the clock stop
    /// until the results. Kept by [`hero::hero_vitals`].
    pub party_down: bool,
    /// A co-op guest's run ended because its host went away ([`coop`]), not by a fall.
    pub host_gone: bool,
    /// The player left from the pause screen: the run ends as a loss and still pays out what it
    /// earned ([`profile::award_run`]).
    pub abandoned: bool,
    /// A bonus fight past the finale is on ([`rares`]): the bell at [`data::VICTORY_SECS`] waits
    /// for it.
    pub encore: bool,
    /// The share of a route walked, 0 to 1 ([`route`]); 0 off a route.
    pub route_frac: f32,
    /// This machine's hero.
    pub me: HeroState,
}

/// One hero's part of a run. The local hero's is [`Run::me`]; on a co-op host each guest's
/// puppet carries its own.
#[derive(Component)]
pub(crate) struct HeroState {
    pub class: &'static ClassDef,
    pub race: u8,
    pub hp: f32,
    pub abilities: Vec<Owned>,
    pub passives: Vec<(&'static PassiveDef, u8)>,
    pub shield: f32,
    pub shield_t: f32,
    pub immune_t: f32,
    pub thorns: f32,
    pub thorns_t: f32,
    pub thorns_spell: u32,
    pub heal_rate: f32,
    pub heal_t: f32,
    /// Gold picked up this run, banked into the profile when it ends ([`profile::award_run`]).
    pub gold: u32,
    /// The profile's bonus for this class, fixed when the run starts ([`profile::Profile::bonus`]).
    pub meta: profile::MetaBonus,
    /// The class mobility's cooldown and running effect.
    mobile: mobility::MobilityState,
    /// Seconds since the hero fell, while the death plays out.
    pub dead_t: Option<f32>,
    /// Choosing level-up cards in a co-op run, where the world goes on: the horde leaves the hero
    /// be and nothing hurts it meanwhile.
    pub picking: bool,
    /// Seconds a party member has stood by this fallen hero without a break ([`hero::revive`]).
    pub revive: f32,
    pub damage_done: f64,
    /// Bumped whenever abilities or passives change, so the HUD rebuilds the icon rows once.
    pub version: u32,
    /// The gear worn this run ([`items`]).
    pub gear: mods::StatMods,
    /// The world buffs taken this run ([`worldbuffs`]).
    pub buffs: mods::StatMods,
    /// The character's talents, fixed when the run starts ([`talents`]).
    pub talents: mods::StatMods,
}

impl std::ops::Deref for Run {
    type Target = HeroState;
    fn deref(&self) -> &HeroState {
        &self.me
    }
}

impl std::ops::DerefMut for Run {
    fn deref_mut(&mut self) -> &mut HeroState {
        &mut self.me
    }
}

impl Default for Run {
    fn default() -> Self {
        Self {
            elapsed: 0.0,
            kills: 0,
            level: 1,
            xp: 0.0,
            pending_levels: 0,
            bosses: 0,
            rings: 0,
            won: false,
            next_guid: 1,
            banner: None,
            party_banner: (0, String::new()),
            difficulty: difficulty::Difficulty::default(),
            party: 1,
            party_down: false,
            host_gone: false,
            abandoned: false,
            encore: false,
            route_frac: 0.0,
            me: HeroState::default(),
        }
    }
}

impl Default for HeroState {
    fn default() -> Self {
        Self {
            class: &data::CLASSES[0],
            race: 0,
            hp: BASE_HP,
            abilities: Vec::new(),
            passives: Vec::new(),
            shield: 0.0,
            shield_t: 0.0,
            immune_t: 0.0,
            thorns: 0.0,
            thorns_t: 0.0,
            thorns_spell: 0,
            heal_rate: 0.0,
            heal_t: 0.0,
            gold: 0,
            meta: profile::MetaBonus::default(),
            mobile: mobility::MobilityState::default(),
            dead_t: None,
            picking: false,
            revive: 0.0,
            damage_done: 0.0,
            version: 0,
            gear: mods::StatMods::default(),
            buffs: mods::StatMods::default(),
            talents: mods::StatMods::default(),
        }
    }
}

impl HeroState {
    /// A hero setting out: the class's first ability and full health.
    pub fn new(
        class: &'static ClassDef,
        race: u8,
        meta: profile::MetaBonus,
        talents: mods::StatMods,
    ) -> Self {
        let mut hero = Self {
            class,
            race,
            meta,
            talents,
            ..Self::default()
        };
        hero.abilities.push(Owned {
            def: data::ability(class.start),
            level: 1,
            cd: 0.5,
        });
        hero.hp = hero.stats().max_hp;
        hero.version += 1;
        hero
    }

    pub fn stats(&self) -> Stats {
        let mut s = Stats {
            might: 1.0,
            max_hp: BASE_HP,
            speed: BASE_SPEED,
            regen: 0.0,
            haste: 0.0,
            area: 1.0,
            magnet: BASE_MAGNET,
            growth: 1.0,
            armor: 0.0,
            crit: 0.05,
        };
        // Tanky classes start tougher, cloth starts quicker.
        match self.class.id {
            1 | 2 => s.max_hp += 40.0,
            7 | 11 | 3 | 4 => s.max_hp += 15.0,
            _ => s.speed += 0.5,
        }
        for &(p, level) in &self.passives {
            let v = p.per_level * f32::from(level);
            match p.stat {
                Stat::Might => s.might += v,
                Stat::MaxHp => s.max_hp *= 1.0 + v,
                Stat::Speed => s.speed *= 1.0 + v,
                Stat::Regen => s.regen += v,
                Stat::Haste => s.haste += v,
                Stat::Area => s.area += v,
                Stat::Magnet => s.magnet *= 1.0 + v,
                Stat::Growth => s.growth += v,
                Stat::Armor => s.armor += v,
                Stat::Crit => s.crit += v,
            }
        }
        let m = &self.meta;
        s.max_hp *= m.max_hp;
        s.might += m.might;
        s.speed *= m.speed;
        s.haste += m.haste;
        s.magnet *= m.magnet;
        s.armor += m.armor;
        s.regen += m.regen;
        let race = races::mods(self.race);
        for mods in [&self.gear, &self.buffs, &self.talents, &race] {
            mods.apply(&mut s);
        }
        s
    }
}

impl Run {
    /// How far the run is, in seconds of the clock, for what grows with it (the horde's numbers
    /// and size, the elites): the clock, or on a route the share walked times
    /// [`data::VICTORY_SECS`] when that is further. Off a route it is the clock.
    pub fn pace(&self) -> f32 {
        self.elapsed.max(self.route_frac * data::VICTORY_SECS)
    }

    /// A banner for the whole party: a boss coming or falling, the ring closing in.
    pub fn announce(&mut self, text: String, secs: f32) {
        self.party_banner = (self.party_banner.0.wrapping_add(1), text.clone());
        self.banner = Some((text, secs));
    }

    pub fn guid(&mut self) -> u64 {
        self.next_guid += 1;
        self.next_guid
    }
}

/// xorshift64*: the mode's dice, seeded from the clock.
#[derive(Resource)]
pub(crate) struct Rng(u64);

impl Default for Rng {
    fn default() -> Self {
        let seed = web_time::SystemTime::now()
            .duration_since(web_time::UNIX_EPOCH)
            .map(|d| d.as_nanos() as u64)
            .unwrap_or(0x9E37_79B9_7F4A_7C15);
        Self(seed | 1)
    }
}

impl Rng {
    pub fn next_u64(&mut self) -> u64 {
        let mut x = self.0;
        x ^= x >> 12;
        x ^= x << 25;
        x ^= x >> 27;
        self.0 = x;
        x.wrapping_mul(0x2545_F491_4F6C_DD1D)
    }
    /// Uniform in `[0, 1)`.
    pub fn f32(&mut self) -> f32 {
        (self.next_u64() >> 40) as f32 / (1u64 << 24) as f32
    }
    pub fn range(&mut self, lo: f32, hi: f32) -> f32 {
        lo + (hi - lo) * self.f32()
    }
    pub fn index(&mut self, len: usize) -> usize {
        (self.next_u64() % len.max(1) as u64) as usize
    }
}

/// `WOW_SURVIVORS_SEED=<n>`: every run starts from the same dice, so a run can be played again.
/// Frame timing still varies, so two runs drift apart once the clock matters.
fn seed_rng(mut rng: ResMut<Rng>) {
    if let Some(seed) = benilla_app::survivors_host::webenv::var("WOW_SURVIVORS_SEED")
        .and_then(|s| s.trim().parse::<u64>().ok())
    {
        *rng = Rng(seed | 1);
    }
}

/// The level-up cards and the pause menu stop the clock, when playing alone.
fn pause(mut time: ResMut<Time<Virtual>>, role: Res<NetRole>) {
    if *role == NetRole::Solo {
        time.pause();
    }
}

fn unpause(mut time: ResMut<Time<Virtual>>) {
    time.unpause();
}

/// Back at the menu: everything the last run spawned goes, and the run starts over.
fn reset_run(
    mut commands: Commands,
    spawned: Query<Entity, With<RunEntity>>,
    mut run: ResMut<Run>,
    mut pending: ResMut<abilities::PendingHits>,
    mut spawner: ResMut<enemies::Spawner>,
    mut selection: ResMut<Selection>,
) {
    for e in &spawned {
        commands.entity(e).despawn();
    }
    // Guids stay unique across runs: caches keyed by guid may outlive a despawn by a frame.
    let next_guid = run.next_guid;
    *run = Run {
        next_guid,
        ..Run::default()
    };
    pending.0.clear();
    *spawner = enemies::Spawner::default();
    // The preview hero is respawned for the look still picked.
    selection.version += 1;
}
