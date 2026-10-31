//! Evolutions: an ability at its top rank plus its matching talent turns into a stronger spell,
//! offered on a level-up card ([`super::hud::Choice::Evolve`]); found recipes are kept in a
//! spellbook, saved as `survivors-spellbook.toml` through [`crate::local_state`], and listed on
//! the level-up screen. The recipes themselves are [`data::EVOLUTIONS`].

use bevy::prelude::*;
use serde::{Deserialize, Serialize};

use super::chatlog::{ChatKind, ChatLine};
use super::data::{self, Evolution};
use super::hud;
use super::kit;
use super::{Phase, Run};

/// Level with the level-up screen's vignette (420) and over it.
const Z: i32 = 425;
const FOUND: Color = super::cards::LEGENDARY;
const HINT: Color = kit::MUTED;
/// Narrow enough to sit left of four cards, so the reroll row under them stays free.
const WIDTH: f32 = 210.0;
/// Most recipes a class has; the panel holds this many lines.
const ROWS: usize = 4;

pub(super) fn plugin(app: &mut App) {
    app.init_resource::<Spellbook>()
        .init_resource::<BookUi>()
        .add_systems(Startup, load_book)
        .add_systems(Update, (record, show).chain().run_if(in_run))
        .add_systems(OnEnter(Phase::Menu), teardown)
        .add_systems(OnEnter(Phase::GameOver), teardown);
}

/// The phases that put the HUD on screen.
fn in_run(phase: Res<State<Phase>>) -> bool {
    matches!(phase.get(), Phase::Playing | Phase::LevelUp | Phase::Paused)
}

/// Whether `evo` holds for these abilities and passives, `(key, rank)` each: its ability at the
/// top rank, its passive at the recipe's rank, and the evolved spell not owned yet.
fn holds(evo: &Evolution, abilities: &[(&str, u8)], passives: &[(&str, u8)]) -> bool {
    abilities
        .iter()
        .any(|&(k, l)| k == evo.from && l >= data::MAX_ABILITY_LEVEL)
        && passives
            .iter()
            .any(|&(k, l)| k == evo.passive && l >= evo.rank)
        && !abilities.iter().any(|&(k, _)| k == evo.into)
}

/// The recipes that hold for the run now.
pub(super) fn ready(run: &Run) -> Vec<&'static Evolution> {
    let abilities: Vec<(&str, u8)> = run.abilities.iter().map(|o| (o.def.key, o.level)).collect();
    let passives: Vec<(&str, u8)> = run.passives.iter().map(|&(p, l)| (p.key, l)).collect();
    data::EVOLUTIONS
        .iter()
        .filter(|e| holds(e, &abilities, &passives))
        .collect()
}

/// Whether the run's `key` ability has already evolved into its stronger spell.
pub(super) fn evolved_from(run: &Run, key: &str) -> bool {
    data::EVOLUTIONS
        .iter()
        .any(|e| e.from == key && run.abilities.iter().any(|o| o.def.key == e.into))
}

/// The recipes whose ability is in `pool`: a class's own.
fn recipes(pool: &[&str]) -> Vec<&'static Evolution> {
    data::EVOLUTIONS
        .iter()
        .filter(|e| pool.contains(&e.from))
        .collect()
}

/// The evolved spells found so far, by key. Saved after every find.
#[derive(Resource, Default, Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub(super) struct Spellbook {
    found: Vec<String>,
}

impl Spellbook {
    fn path() -> Option<std::path::PathBuf> {
        benilla_app::survivors_host::local_state::home().map(|h| h.join("survivors-spellbook.toml"))
    }

    fn load() -> Self {
        Self::path()
            .and_then(|path| {
                let text = benilla_app::survivors_host::local_state::read_to_string(&path).ok()?;
                match toml::from_str::<Self>(&text) {
                    Ok(book) => Some(book),
                    Err(e) => {
                        warn!("survivors: spellbook unreadable, starting fresh ({e})");
                        let _ =
                            benilla_app::survivors_host::local_state::write_atomic(&path.with_extension("bad"), &text);
                        None
                    }
                }
            })
            .unwrap_or_default()
    }

    /// An automated run (`WOW_SURVIVORS_AUTO`) saves nothing, as the profile does.
    fn save(&self) {
        if benilla_app::survivors_host::webenv::var("WOW_SURVIVORS_AUTO").is_some() {
            return;
        }
        let Some(path) = Self::path() else { return };
        match toml::to_string(self) {
            Ok(text) => {
                if let Err(e) = benilla_app::survivors_host::local_state::write_atomic(&path, &text) {
                    warn!("survivors: spellbook not saved ({e})");
                }
            }
            Err(e) => warn!("survivors: spellbook not serialized ({e})"),
        }
    }

    fn knows(&self, key: &str) -> bool {
        self.found.iter().any(|k| k == key)
    }

    /// Writes `key` in; whether it was new.
    fn learn(&mut self, key: &str) -> bool {
        if self.knows(key) {
            return false;
        }
        self.found.push(key.to_string());
        true
    }
}

fn load_book(mut book: ResMut<Spellbook>) {
    *book = Spellbook::load();
}

/// An evolved spell the hero owns and the book does not know yet is written in.
fn record(run: Res<Run>, mut book: ResMut<Spellbook>, mut chat: MessageWriter<ChatLine>) {
    let mut learned = false;
    for evo in data::EVOLUTIONS {
        if run.abilities.iter().any(|o| o.def.key == evo.into) && book.learn(evo.into) {
            learned = true;
            chat.write(ChatLine::text(
                ChatKind::System,
                format!("You have found the recipe for {}.", evo.name),
            ));
        }
    }
    if learned {
        book.save();
    }
}

/// The panel at the bottom of the level-up screen.
#[derive(Resource, Default)]
struct BookUi {
    root: Option<Entity>,
    rows: Vec<Entity>,
}

/// Back at the menu or the results: the panel goes, and the next run builds its own.
fn teardown(mut commands: Commands, mut ui: ResMut<BookUi>) {
    if let Some(root) = ui.root.take() {
        commands.entity(root).try_despawn();
    }
    ui.rows.clear();
}

fn build(commands: &mut Commands, ui: &mut BookUi, fonts: &kit::KitFonts) {
    let mut rows = Vec::with_capacity(ROWS);
    let root = commands
        .spawn((
            kit::panel(Node {
                position_type: PositionType::Absolute,
                left: Val::Px(10.0),
                top: Val::Percent(36.0),
                width: Val::Px(WIDTH),
                flex_direction: FlexDirection::Column,
                align_items: AlignItems::Center,
                padding: UiRect::axes(Val::Px(12.0), Val::Px(8.0)),
                row_gap: Val::Px(3.0),
                ..default()
            }),
            GlobalZIndex(Z),
            Visibility::Hidden,
        ))
        .with_children(|p| {
            kit::heading(p, fonts, "Spellbook", 20.0);
            for _ in 0..ROWS {
                rows.push(
                    p.spawn((
                        kit::body(fonts, "", 12.0, HINT),
                        Node {
                            display: Display::None,
                            ..default()
                        },
                    ))
                    .id(),
                );
            }
        })
        .id();
    ui.root = Some(root);
    ui.rows = rows;
}

/// The class's recipes: found ones whole, the rest with the talent hidden.
fn lines(
    run: &Run,
    book: &Spellbook,
    spells: Option<&benilla_app::survivors_host::ui_action::Spells>,
) -> Vec<(String, Color)> {
    recipes(run.class.pool)
        .into_iter()
        .map(|e| {
            let from = data::ability(e.from);
            let from = hud::spell_name(spells, from.spell, from.key);
            if book.knows(e.into) {
                let passive = data::PASSIVES
                    .iter()
                    .find(|p| p.key == e.passive)
                    .map_or(e.passive, |p| p.name);
                (
                    format!(
                        "{}: {from} (rank {}) + {passive} (rank {})",
                        e.name,
                        data::MAX_ABILITY_LEVEL,
                        e.rank
                    ),
                    FOUND,
                )
            } else {
                (
                    format!("???: {from} (rank {}) + ???", data::MAX_ABILITY_LEVEL),
                    HINT,
                )
            }
        })
        .collect()
}

/// Build the panel on first need, show it on the level-up screen and keep its lines current.
#[allow(clippy::too_many_arguments)]
fn show(
    mut commands: Commands,
    mut ui: ResMut<BookUi>,
    phase: Res<State<Phase>>,
    run: Res<Run>,
    book: Res<Spellbook>,
    fonts: Option<Res<kit::KitFonts>>,
    spells: Option<Res<benilla_app::survivors_host::ui_action::Spells>>,
    mut vis: Query<&mut Visibility>,
    mut rows: Query<(&mut Text, &mut TextColor, &mut Node)>,
) {
    let ui = &mut *ui;
    let Some(root) = ui.root else {
        if let Some(fonts) = fonts {
            // Seated from the next frame on, once its nodes exist.
            build(&mut commands, ui, &fonts);
        }
        return;
    };
    let level_up = *phase.get() == Phase::LevelUp;
    if let Ok(mut v) = vis.get_mut(root) {
        let want = if level_up {
            Visibility::Inherited
        } else {
            Visibility::Hidden
        };
        if *v != want {
            *v = want;
        }
    }
    if !level_up {
        return;
    }
    let lines = lines(&run, &book, spells.as_deref());
    for (i, &row) in ui.rows.iter().enumerate() {
        let Ok((mut text, mut color, mut node)) = rows.get_mut(row) else {
            continue;
        };
        let line = lines.get(i);
        let display = if line.is_some() {
            Display::Flex
        } else {
            Display::None
        };
        if node.display != display {
            node.display = display;
        }
        let Some((s, c)) = line else { continue };
        if text.0 != *s {
            text.0.clone_from(s);
        }
        if color.0 != *c {
            color.0 = *c;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const TOP: u8 = data::MAX_ABILITY_LEVEL;

    fn frost() -> &'static Evolution {
        data::EVOLUTIONS
            .iter()
            .find(|e| e.from == "frostbolt")
            .unwrap()
    }

    #[test]
    fn a_recipe_needs_the_top_rank_and_the_talent() {
        let e = frost();
        let talent = [(e.passive, e.rank)];
        assert!(holds(e, &[("frostbolt", TOP)], &talent));
        assert!(!holds(e, &[("frostbolt", TOP - 1)], &talent), "rank short");
        assert!(
            !holds(e, &[("frostbolt", TOP)], &[(e.passive, e.rank - 1)]),
            "talent short"
        );
        assert!(!holds(e, &[("frostbolt", TOP)], &[]), "no talent");
        assert!(!holds(e, &[("fireball", TOP)], &talent), "other ability");
    }

    #[test]
    fn an_evolved_spell_does_not_offer_itself_again() {
        let e = frost();
        let talent = [(e.passive, e.rank + 1)];
        assert!(holds(e, &[("frostbolt", TOP)], &talent));
        assert!(!holds(e, &[("frostbolt", TOP), (e.into, 5)], &talent));
    }

    #[test]
    fn every_recipe_resolves_and_belongs_to_a_class() {
        for e in data::EVOLUTIONS {
            // `ability` panics on an unknown key.
            let (from, into) = (data::ability(e.from), data::ability(e.into));
            assert_ne!(from.spell, 0);
            assert_ne!(into.spell, 0);
            assert!(
                data::PASSIVES.iter().any(|p| p.key == e.passive),
                "{} has no talent {}",
                e.name,
                e.passive
            );
            assert!(e.rank >= 1 && e.rank <= data::MAX_PASSIVE_LEVEL);
            assert!(
                data::CLASSES.iter().any(|c| c.pool.contains(&e.from)),
                "{} is in no class pool",
                e.from
            );
            assert!(
                !data::CLASSES.iter().any(|c| c.pool.contains(&e.into)),
                "{} must stay out of the pools",
                e.into
            );
            assert_eq!(data::evolved_name(e.into), Some(e.name));
        }
    }

    #[test]
    fn every_class_has_a_recipe_and_the_panel_holds_them() {
        for c in data::CLASSES {
            let n = recipes(c.pool).len();
            assert!(n >= 1, "{} has no evolution", c.name);
            assert!(n <= ROWS, "{} has {n} recipes", c.name);
        }
    }

    #[test]
    fn an_evolved_spell_beats_the_one_it_replaces_at_the_top_rank() {
        // Damage over the targets reached, per second, at the start rank against rank 8.
        let rate = |def: &'static data::AbilityDef, level: u8| {
            let o = super::super::Owned {
                def,
                level,
                cd: 0.0,
            };
            let dmg = (def.damage + def.dot * def.duration.max(1.0)) * o.damage_mult();
            dmg * o.count() as f32 / (def.cooldown * 0.93f32.powi(i32::from(level) - 1))
        };
        for e in data::EVOLUTIONS {
            let (old, new) = (data::ability(e.from), data::ability(e.into));
            let (a, b) = (rate(old, TOP), rate(new, data::EVOLVED_LEVEL));
            assert!(b > a * 1.3, "{}: {b:.0} against {a:.0}", e.name);
        }
    }

    #[test]
    fn the_spellbook_learns_a_recipe_once() {
        let mut book = Spellbook::default();
        assert!(!book.knows("glacial_volley"));
        assert!(book.learn("glacial_volley"));
        assert!(!book.learn("glacial_volley"));
        assert!(book.knows("glacial_volley"));
        let text = toml::to_string(&book).unwrap();
        assert_eq!(toml::from_str::<Spellbook>(&text).unwrap(), book);
    }
}
