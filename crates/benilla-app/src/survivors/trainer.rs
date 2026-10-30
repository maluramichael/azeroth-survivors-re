//! The trainer and the class shop: permanent upgrades bought with banked gold, and the classes
//! that start locked. Both live on the pick screen. The trainer is a panel in the glue frame art,
//! opened from a button under the class tower; a class not bought yet shows greyed with a lock on
//! its button, and an unlock button appears at the bottom of the screen while one is selected.
//!
//! The ranks are kept in [`Profile::trainer`] by [`Upgrade::key`] and folded into the run's
//! [`MetaBonus`] by [`apply_ranks`]. Everything here is built into the existing
//! [`MenuRoot`] once [`menu::build_menu`] has finished, with its own [`TrainerAction`] buttons, so
//! the menu's own systems need not know about it.

use std::collections::BTreeMap;

use bevy::prelude::*;
use bevy::ui::FocusPolicy;

use super::hud::{self, HudFont, MenuRoot};
use super::menu::{self, MenuAction, MenuBuilt};
use super::profile::{MetaBonus, Profile};
use super::{data, Phase, Selection};
use crate::glue::art::{GlueArt, DIM, GOLD, NAME_EDGE};
use crate::glue::backdrop::{backdrop_border, tiled_bg_node};
use crate::glue::widgets::{
    abs, glue_button, outlined_text, GlueBtnKind, GlueCaption, GlueDisabled, GlueText,
};
use crate::glue::GlueClicks;

/// Classes every profile starts with ([`Profile`]'s starters); the shop prices the ones beyond.
const STARTERS: usize = 3;
/// The first extra class costs this much, each further one a multiple more.
const CLASS_BASE_PRICE: u64 = 500;

/// One permanent upgrade: ranks bought with gold, each adding `apply`'s effect to the run.
pub(super) struct Upgrade {
    /// The stable key in [`Profile::trainer`]; never rename one, a save would lose its ranks.
    pub key: &'static str,
    pub name: &'static str,
    /// What one rank does, for the row.
    pub effect: &'static str,
    pub max: u8,
    /// The next rank costs this times (current rank + 1).
    pub price: u64,
    /// Adds `ranks` ranks' worth of the effect.
    apply: fn(&mut MetaBonus, f32),
}

pub(super) const UPGRADES: [Upgrade; 8] = [
    Upgrade {
        key: "might",
        name: "Might",
        effect: "+5% damage per rank",
        max: 5,
        price: 400,
        apply: |b, r| b.might += 0.05 * r,
    },
    Upgrade {
        key: "health",
        name: "Health",
        effect: "+8% maximum health per rank",
        max: 5,
        price: 400,
        apply: |b, r| b.max_hp += 0.08 * r,
    },
    Upgrade {
        key: "haste",
        name: "Haste",
        effect: "+3% cooldown reduction per rank",
        max: 3,
        price: 450,
        apply: |b, r| b.haste += 0.03 * r,
    },
    Upgrade {
        key: "swiftness",
        name: "Swiftness",
        effect: "+4% movement speed per rank",
        max: 3,
        price: 300,
        apply: |b, r| b.speed += 0.04 * r,
    },
    Upgrade {
        key: "magnet",
        name: "Magnet",
        effect: "+15% pickup radius per rank",
        max: 3,
        price: 250,
        apply: |b, r| b.magnet += 0.15 * r,
    },
    Upgrade {
        key: "armor",
        name: "Armor",
        effect: "+1 armor per rank",
        max: 3,
        price: 450,
        apply: |b, r| b.armor += r,
    },
    Upgrade {
        key: "regeneration",
        name: "Regeneration",
        effect: "+0.3 health per second per rank",
        max: 3,
        price: 350,
        apply: |b, r| b.regen += 0.3 * r,
    },
    Upgrade {
        key: "greed",
        name: "Greed",
        effect: "+10% gold from runs per rank",
        max: 5,
        price: 300,
        apply: |b, r| b.greed += 0.1 * r,
    },
];

/// The ranks bought in `u`, clamped to its maximum so an edited save cannot overshoot.
pub(super) fn rank(trainer: &BTreeMap<String, u8>, u: &Upgrade) -> u8 {
    trainer.get(u.key).copied().unwrap_or(0).min(u.max)
}

/// What the next rank of `u` costs, or `None` at the maximum.
pub(super) fn next_price(trainer: &BTreeMap<String, u8>, u: &Upgrade) -> Option<u64> {
    let r = rank(trainer, u);
    (r < u.max).then(|| u.price * (u64::from(r) + 1))
}

/// All gold put into ranks, what Reset hands back.
pub(super) fn spent(trainer: &BTreeMap<String, u8>) -> u64 {
    UPGRADES
        .iter()
        .map(|u| (0..u64::from(rank(trainer, u))).map(|r| u.price * (r + 1)).sum::<u64>())
        .sum()
}

/// Fold the bought ranks into a run's bonus ([`Profile::bonus`]).
pub(super) fn apply_ranks(bonus: &mut MetaBonus, trainer: &BTreeMap<String, u8>) {
    for u in &UPGRADES {
        (u.apply)(bonus, f32::from(rank(trainer, u)));
    }
}

/// The price of the next class: 500 for the first beyond the starters, then 1000, 1500, and so on.
pub(super) fn class_price(profile: &Profile) -> u64 {
    let bought = profile.classes.iter().filter(|b| **b).count();
    CLASS_BASE_PRICE * (bought.saturating_sub(STARTERS) as u64 + 1)
}

/// A button of the trainer or the class shop.
#[derive(Component, Clone, Copy, PartialEq, Eq, Debug)]
enum TrainerAction {
    Open,
    Close,
    Reset,
    /// An index into [`UPGRADES`].
    Buy(usize),
    /// Buy the selected class.
    Unlock,
}

/// A text the refresh keeps current.
#[derive(Component, Clone, Copy)]
enum Label {
    /// The bank, on the pick screen and in the panel.
    Gold,
    /// An upgrade's next price.
    Price(usize),
}

/// The dimmed full-screen layer holding the trainer, hidden until opened.
#[derive(Component)]
struct TrainerPanel;
/// The row holding the unlock button, shown while a locked class is selected.
#[derive(Component)]
struct UnlockWrap;
/// A rank pip: upgrade index and its position in the row.
#[derive(Component)]
struct Pip(usize, u8);
/// The gold coin beside an upgrade's price.
#[derive(Component)]
struct PriceIcon(usize);
/// A class button's lock badge.
#[derive(Component)]
struct LockIcon(usize);
/// On [`MenuRoot`] once the trainer is built into it.
#[derive(Component)]
struct TrainerBuilt;

/// Whether the trainer panel is open.
#[derive(Resource, Default)]
pub(super) struct TrainerUi {
    pub(super) open: bool,
}

/// Textures loaded once for the trainer.
#[derive(Resource, Default)]
struct TrainerArt {
    /// `UI-GoldIcon`; without it the bank reads "1,234 g".
    gold: Option<Handle<Image>>,
}

const PANEL_W: f32 = 600.0;
const PANEL_H: f32 = 530.0;
const ROWS_TOP: f32 = 92.0;
const ROW_PITCH: f32 = 46.0;
const PIPS_X: f32 = 300.0;
const PRICE_X: f32 = 392.0;
const BUY_X: f32 = 486.0;

const FRAME_BORDER: Color = Color::srgb(0.8, 0.8, 0.8);
const FRAME_FILL: Color = Color::srgba(0.09, 0.09, 0.09, 0.95);
const PIP_FULL: Color = Color::srgb(1.0, 0.78, 0.0);
const PIP_EMPTY: Color = Color::srgb(0.12, 0.12, 0.12);
const PIP_EDGE: Color = Color::srgb(0.5, 0.4, 0.2);
const SHORT: Color = Color::srgb(1.0, 0.3, 0.3);
const MAXED: Color = Color::srgb(0.4, 1.0, 0.4);
/// A locked class's face.
const LOCKED_FACE: Color = Color::srgb(0.3, 0.3, 0.3);

pub(super) fn plugin(app: &mut App) {
    app.init_resource::<TrainerUi>()
        .init_resource::<TrainerArt>()
        .add_systems(
            Update,
            (
                build_trainer,
                show_panel,
                trainer_clicks,
                refresh_rows,
                refresh_unlock,
                refresh_class_locks,
            )
                .chain()
                // After the menu has set the Enter World button, so a locked class can disable it.
                .after(menu::refresh_menu_ui)
                .before(crate::glue::GlueVisuals),
        );
}

/// A coin and the bank (or a price) beside it, in a row.
fn gold_line(row: &mut ChildSpawnerCommands, art: &TrainerArt, size: f32) {
    if let Some(icon) = &art.gold {
        row.spawn((
            ImageNode::new(icon.clone()),
            Node {
                width: Val::Px(size),
                height: Val::Px(size),
                ..default()
            },
        ));
    }
}

/// A built pick screen the trainer has not been added to yet.
type MenuWithoutTrainer = (With<MenuRoot>, With<MenuBuilt>, Without<TrainerBuilt>);

/// Build the trainer button, the bank line, the unlock button and the panel into the pick screen,
/// and the lock badges onto the class buttons.
#[allow(clippy::too_many_arguments)]
fn build_trainer(
    mut commands: Commands,
    roots: Query<Entity, MenuWithoutTrainer>,
    class_buttons: Query<(Entity, &MenuAction)>,
    assets: Option<ResMut<benilla_assets::WorldAssets>>,
    glue: Res<GlueArt>,
    mut images: ResMut<Assets<Image>>,
    font: Res<HudFont>,
    mut art: ResMut<TrainerArt>,
) {
    let Ok(root) = roots.single() else { return };
    let Some(mut assets) = assets else { return };
    art.gold = assets.sprite_texture("Interface\\MoneyFrame\\UI-GoldIcon", &mut images);
    let lock = assets.sprite_texture("Interface\\Buttons\\LockButton-Locked-Up", &mut images);
    let f = &font.0;
    let art = &*art;
    let px = Val::Px;

    for (button, action) in &class_buttons {
        let (MenuAction::Class(i), Some(lock)) = (*action, &lock) else {
            continue;
        };
        commands.entity(button).with_children(|b| {
            b.spawn((
                LockIcon(i),
                ImageNode::new(lock.clone()),
                abs(1.0, 28.0, 28.0, 18.0, 18.0),
            ));
        });
    }

    commands.entity(root).insert(TrainerBuilt).with_children(|ui| {
        // Under the class tower: the Trainer button and the bank.
        ui.spawn(Node {
            position_type: PositionType::Absolute,
            left: px(58.0),
            top: px(676.0),
            ..default()
        })
        .with_children(|b| {
            glue_button(
                b,
                &glue,
                f,
                TrainerAction::Open,
                "Trainer",
                146.0,
                30.0,
                GlueBtnKind::Small,
                1.0,
            );
        });
        ui.spawn(Node {
            position_type: PositionType::Absolute,
            left: px(28.0),
            top: px(712.0),
            width: px(206.0),
            justify_content: JustifyContent::Center,
            align_items: AlignItems::Center,
            column_gap: px(4.0),
            ..default()
        })
        .with_children(|row| {
            gold_line(row, art, 14.0);
            gold_text(row, f, 14.0);
        });

        // The unlock button above the name line, only while a locked class is selected.
        ui.spawn((
            UnlockWrap,
            Node {
                position_type: PositionType::Absolute,
                bottom: px(140.0),
                width: Val::Percent(100.0),
                justify_content: JustifyContent::Center,
                display: Display::None,
                ..default()
            },
        ))
        .with_children(|c| {
            glue_button(
                c,
                &glue,
                f,
                TrainerAction::Unlock,
                "Unlock",
                340.0,
                40.0,
                GlueBtnKind::Normal,
                1.0,
            );
        });

        panel(ui, &glue, art, f);
    });
}

/// The bank's number, white as the game's money frames show it.
fn gold_text(row: &mut ChildSpawnerCommands, f: &Handle<Font>, size: f32) {
    outlined_text(
        row,
        Node::default(),
        (),
        Label::Gold,
        GlueText {
            text: "",
            size,
            color: Color::WHITE,
            wrap: false,
        },
        f,
        1.0,
    );
}

/// The trainer panel: title, bank, one row per upgrade, Reset and Close.
fn panel(ui: &mut ChildSpawnerCommands, glue: &GlueArt, art: &TrainerArt, f: &Handle<Font>) {
    let px = Val::Px;
    ui.spawn((
        TrainerPanel,
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
        dim.spawn(Node {
            width: px(PANEL_W),
            height: px(PANEL_H),
            ..default()
        })
        .with_children(|frame| {
            if let (Some(bg), Some(border)) = (&glue.tooltip_bg, &glue.name_border) {
                frame.spawn((
                    tiled_bg_node(bg.clone(), NAME_EDGE, 1.0, FRAME_FILL),
                    BackgroundColor(Color::srgb(0.03, 0.03, 0.04)),
                    Node {
                        position_type: PositionType::Absolute,
                        left: px(10.0),
                        right: px(5.0),
                        top: px(4.0),
                        bottom: px(9.0),
                        ..default()
                    },
                ));
                backdrop_border(frame, border, NAME_EDGE, FRAME_BORDER);
            } else {
                frame.spawn((
                    BackgroundColor(FRAME_FILL),
                    crate::glue::widgets::overlay(),
                ));
            }
            outlined_text(
                frame,
                Node {
                    position_type: PositionType::Absolute,
                    top: px(16.0),
                    width: Val::Percent(100.0),
                    justify_content: JustifyContent::Center,
                    ..default()
                },
                (),
                (),
                GlueText {
                    text: "Trainer",
                    size: 22.0,
                    color: GOLD,
                    wrap: false,
                },
                f,
                1.0,
            );
            frame
                .spawn(Node {
                    position_type: PositionType::Absolute,
                    top: px(52.0),
                    width: Val::Percent(100.0),
                    justify_content: JustifyContent::Center,
                    align_items: AlignItems::Center,
                    column_gap: px(4.0),
                    ..default()
                })
                .with_children(|row| {
                    gold_line(row, art, 16.0);
                    gold_text(row, f, 16.0);
                });

            for (i, u) in UPGRADES.iter().enumerate() {
                upgrade_row(frame, glue, art, f, i, u);
            }

            let foot = PANEL_H - 56.0;
            for (left, action, caption) in [
                (34.0, TrainerAction::Reset, "Reset"),
                (PANEL_W - 34.0 - 146.0, TrainerAction::Close, "Close"),
            ] {
                frame
                    .spawn(Node {
                        position_type: PositionType::Absolute,
                        left: px(left),
                        top: px(foot),
                        ..default()
                    })
                    .with_children(|b| {
                        glue_button(
                            b,
                            glue,
                            f,
                            action,
                            caption,
                            146.0,
                            30.0,
                            GlueBtnKind::Small,
                            1.0,
                        );
                    });
            }
            outlined_text(
                frame,
                Node {
                    position_type: PositionType::Absolute,
                    top: px(foot + 9.0),
                    width: Val::Percent(100.0),
                    justify_content: JustifyContent::Center,
                    ..default()
                },
                (),
                (),
                GlueText {
                    text: "Reset refunds all gold spent",
                    size: 12.0,
                    color: DIM,
                    wrap: false,
                },
                f,
                1.0,
            );
        });
    });
}

/// One upgrade's row: name, effect, rank pips, next price and the Buy button.
fn upgrade_row(
    frame: &mut ChildSpawnerCommands,
    glue: &GlueArt,
    art: &TrainerArt,
    f: &Handle<Font>,
    i: usize,
    u: &Upgrade,
) {
    let px = Val::Px;
    let top = ROWS_TOP + i as f32 * ROW_PITCH;
    outlined_text(
        frame,
        abs(1.0, 34.0, top + 2.0, 250.0, 17.0),
        (),
        (),
        GlueText {
            text: u.name,
            size: 15.0,
            color: GOLD,
            wrap: false,
        },
        f,
        1.0,
    );
    outlined_text(
        frame,
        abs(1.0, 34.0, top + 21.0, 250.0, 14.0),
        (),
        (),
        GlueText {
            text: u.effect,
            size: 12.0,
            color: Color::WHITE,
            wrap: false,
        },
        f,
        1.0,
    );
    frame
        .spawn(Node {
            position_type: PositionType::Absolute,
            left: px(PIPS_X),
            top: px(top + 11.0),
            column_gap: px(3.0),
            ..default()
        })
        .with_children(|pips| {
            for k in 0..u.max {
                pips.spawn((
                    Pip(i, k),
                    BackgroundColor(PIP_EMPTY),
                    BorderColor::all(PIP_EDGE),
                    Node {
                        width: px(13.0),
                        height: px(13.0),
                        border: UiRect::all(px(1.0)),
                        ..default()
                    },
                ));
            }
        });
    frame
        .spawn(Node {
            position_type: PositionType::Absolute,
            left: px(PRICE_X),
            top: px(top + 10.0),
            width: px(90.0),
            align_items: AlignItems::Center,
            column_gap: px(4.0),
            ..default()
        })
        .with_children(|row| {
            if let Some(icon) = &art.gold {
                row.spawn((
                    PriceIcon(i),
                    ImageNode::new(icon.clone()),
                    Node {
                        width: px(14.0),
                        height: px(14.0),
                        ..default()
                    },
                ));
            }
            outlined_text(
                row,
                Node::default(),
                (),
                Label::Price(i),
                GlueText {
                    text: "",
                    size: 14.0,
                    color: Color::WHITE,
                    wrap: false,
                },
                f,
                1.0,
            );
        });
    frame
        .spawn(Node {
            position_type: PositionType::Absolute,
            left: px(BUY_X),
            top: px(top + 4.0),
            ..default()
        })
        .with_children(|b| {
            glue_button(
                b,
                glue,
                f,
                TrainerAction::Buy(i),
                "Buy",
                80.0,
                30.0,
                GlueBtnKind::Small,
                1.0,
            );
        });
}

/// Show or hide the panel; it closes on Escape and whenever the menu is left.
fn show_panel(
    mut ui: ResMut<TrainerUi>,
    phase: Res<State<Phase>>,
    keys: Res<ButtonInput<KeyCode>>,
    mut panel: Query<&mut Node, With<TrainerPanel>>,
) {
    if ui.open && (*phase.get() != Phase::Menu || keys.just_pressed(KeyCode::Escape)) {
        ui.open = false;
    }
    let display = if ui.open { Display::Flex } else { Display::None };
    for mut node in &mut panel {
        if node.display != display {
            node.display = display;
        }
    }
}

/// The trainer's and the shop's clicks: open, close, buy a rank, reset, unlock the selected class.
fn trainer_clicks(
    clicks: Res<GlueClicks>,
    actions: Query<(Entity, &TrainerAction)>,
    phase: Res<State<Phase>>,
    selection: Res<Selection>,
    mut profile: ResMut<Profile>,
    mut ui: ResMut<TrainerUi>,
) {
    if *phase.get() != Phase::Menu {
        return;
    }
    let Some(action) = actions
        .iter()
        .find(|(e, _)| clicks.hit(*e))
        .map(|(_, a)| *a)
    else {
        return;
    };
    match action {
        TrainerAction::Open => ui.open = true,
        TrainerAction::Close => ui.open = false,
        TrainerAction::Buy(i) => {
            let Some(u) = UPGRADES.get(i) else { return };
            let Some(price) = next_price(&profile.trainer, u) else {
                return;
            };
            if profile.gold < price {
                return;
            }
            let bought = rank(&profile.trainer, u) + 1;
            profile.gold -= price;
            profile.trainer.insert(u.key.to_string(), bought);
            profile.save();
        }
        TrainerAction::Reset => {
            let refund = spent(&profile.trainer);
            if refund == 0 && profile.trainer.is_empty() {
                return;
            }
            profile.gold += refund;
            profile.trainer.clear();
            profile.save();
        }
        TrainerAction::Unlock => {
            let Some(class) = selection.class else { return };
            if profile.class_unlocked(class) {
                return;
            }
            let price = class_price(&profile);
            if profile.gold < price {
                return;
            }
            profile.gold -= price;
            if let Some(bought) = profile.classes.get_mut(class) {
                *bought = true;
            }
            profile.save();
        }
    }
}

/// Set a glue button's caption, which sits in a text under it.
fn set_caption(
    button: Entity,
    want: &str,
    children: &Query<&Children>,
    captions: &mut Query<&mut Text, With<GlueCaption>>,
) {
    for d in children.iter_descendants(button) {
        if let Ok(mut text) = captions.get_mut(d) {
            if text.0 != want {
                text.0 = want.to_string();
            }
            return;
        }
    }
}

/// The bank, the pips, the prices and the Buy and Reset buttons.
#[allow(clippy::too_many_arguments, clippy::type_complexity)]
fn refresh_rows(
    profile: Res<Profile>,
    art: Res<TrainerArt>,
    mut pips: Query<(&Pip, &mut BackgroundColor)>,
    mut labels: Query<(&Label, &mut Text, &mut TextColor), Without<GlueCaption>>,
    mut icons: Query<(&PriceIcon, &mut Visibility)>,
    mut buttons: Query<(Entity, &TrainerAction, &mut GlueDisabled)>,
    children: Query<&Children>,
    mut captions: Query<&mut Text, With<GlueCaption>>,
) {
    for (pip, mut color) in &mut pips {
        let Some(u) = UPGRADES.get(pip.0) else {
            continue;
        };
        let want = if pip.1 < rank(&profile.trainer, u) {
            PIP_FULL
        } else {
            PIP_EMPTY
        };
        if color.0 != want {
            color.0 = want;
        }
    }
    for (label, mut text, mut color) in &mut labels {
        let (want, tint) = match *label {
            Label::Gold => {
                let mut s = hud::grouped(profile.gold);
                if art.gold.is_none() {
                    s.push_str(" g");
                }
                (s, Color::WHITE)
            }
            Label::Price(i) => match UPGRADES
                .get(i)
                .and_then(|u| next_price(&profile.trainer, u))
            {
                Some(price) if profile.gold >= price => (hud::grouped(price), Color::WHITE),
                Some(price) => (hud::grouped(price), SHORT),
                None => ("Maxed".to_string(), MAXED),
            },
        };
        if text.0 != want {
            text.0 = want;
        }
        if color.0 != tint {
            color.0 = tint;
        }
    }
    for (icon, mut vis) in &mut icons {
        let maxed = UPGRADES
            .get(icon.0)
            .is_none_or(|u| next_price(&profile.trainer, u).is_none());
        let want = if maxed {
            Visibility::Hidden
        } else {
            Visibility::Inherited
        };
        if *vis != want {
            *vis = want;
        }
    }
    for (button, action, mut disabled) in &mut buttons {
        let (off, caption) = match *action {
            TrainerAction::Buy(i) => {
                match UPGRADES
                    .get(i)
                    .and_then(|u| next_price(&profile.trainer, u))
                {
                    Some(price) => (profile.gold < price, "Buy"),
                    None => (true, "Max"),
                }
            }
            TrainerAction::Reset => (spent(&profile.trainer) == 0, "Reset"),
            _ => continue,
        };
        if disabled.0 != off {
            disabled.0 = off;
        }
        set_caption(button, caption, &children, &mut captions);
    }
}

/// The unlock button: shown for a locked class, with its price or what is still missing. A locked
/// class also holds Enter World disabled.
#[allow(clippy::too_many_arguments, clippy::type_complexity)]
fn refresh_unlock(
    selection: Res<Selection>,
    profile: Res<Profile>,
    mut wrap: Query<&mut Node, With<UnlockWrap>>,
    mut buttons: Query<(Entity, &TrainerAction, &mut GlueDisabled)>,
    mut enter: Query<(&MenuAction, &mut GlueDisabled), Without<TrainerAction>>,
    children: Query<&Children>,
    mut captions: Query<&mut Text, With<GlueCaption>>,
) {
    let locked = selection
        .class
        .filter(|&c| !profile.class_unlocked(c) && c < data::CLASSES.len());
    let display = if locked.is_some() {
        Display::Flex
    } else {
        Display::None
    };
    for mut node in &mut wrap {
        if node.display != display {
            node.display = display;
        }
    }
    if locked.is_some() {
        for (action, mut disabled) in &mut enter {
            if *action == MenuAction::Enter && !disabled.0 {
                disabled.0 = true;
            }
        }
    }
    let Some(class) = locked else { return };
    let name = data::CLASSES[class].name;
    let price = class_price(&profile);
    let (off, caption) = if profile.gold >= price {
        (false, format!("Unlock {name}: {} gold", hud::grouped(price)))
    } else {
        (
            true,
            format!(
                "{name}: {} gold, {} more needed",
                hud::grouped(price),
                hud::grouped(price - profile.gold)
            ),
        )
    };
    for (button, action, mut disabled) in &mut buttons {
        if *action != TrainerAction::Unlock {
            continue;
        }
        if disabled.0 != off {
            disabled.0 = off;
        }
        set_caption(button, &caption, &children, &mut captions);
    }
}

/// Grey the faces of the classes not bought and show their lock badges.
fn refresh_class_locks(
    profile: Res<Profile>,
    buttons: Query<(&MenuAction, &Children)>,
    mut faces: Query<(&Node, &mut ImageNode)>,
    mut locks: Query<(&LockIcon, &mut Visibility)>,
) {
    for (action, kids) in &buttons {
        let MenuAction::Class(i) = *action else {
            continue;
        };
        let locked = !profile.class_unlocked(i);
        let want = if locked { LOCKED_FACE } else { Color::WHITE };
        for &kid in kids {
            // The face is the child that fills the button; the shadow and the badge are fixed-size.
            if let Ok((node, mut img)) = faces.get_mut(kid) {
                if node.width == Val::Percent(100.0) && img.color != want {
                    img.color = want;
                }
            }
        }
    }
    for (lock, mut vis) in &mut locks {
        let want = if profile.class_unlocked(lock.0) {
            Visibility::Hidden
        } else {
            Visibility::Inherited
        };
        if *vis != want {
            *vis = want;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ranks_cost_more_and_reset_refunds_them() {
        let mut t = BTreeMap::new();
        let might = &UPGRADES[0];
        assert_eq!(next_price(&t, might), Some(400));
        t.insert("might".to_string(), 2);
        assert_eq!(next_price(&t, might), Some(1200));
        assert_eq!(spent(&t), 400 + 800);
        t.insert("might".to_string(), 99);
        assert_eq!(rank(&t, might), 5);
        assert_eq!(next_price(&t, might), None);
    }

    #[test]
    fn ranks_fold_into_the_bonus() {
        let mut t = BTreeMap::new();
        t.insert("health".to_string(), 5);
        t.insert("armor".to_string(), 2);
        let mut b = MetaBonus::default();
        apply_ranks(&mut b, &t);
        assert!((b.max_hp - 1.4).abs() < 1e-5 && (b.armor - 2.0).abs() < 1e-5);
    }

    #[test]
    fn classes_get_dearer() {
        let mut p = Profile::default();
        assert_eq!(class_price(&p), 500);
        p.classes[1] = true;
        assert_eq!(class_price(&p), 1000);
    }

    #[test]
    fn thousands_groups_digits() {
        assert_eq!(hud::grouped(0), "0");
        assert_eq!(hud::grouped(999), "999");
        assert_eq!(hud::grouped(1_234_567), "1,234,567");
    }
}
