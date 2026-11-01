//! The collection book: every item template ([`super::items::table::TEMPLATES`]) by slot and
//! quality, the ones ever picked up shown, the rest as question marks, with a detail pane for the
//! clicked piece. Opened from the hub's Collection tab; what was found lives in
//! [`super::profile::Profile::collection`].
//!
//! The window is a kit window ([`super::kit`]) built into the pick screen, as the talent tree is:
//! a dimmed full-screen layer, hidden until [`CollectionUi::open`]. The grid has one column per
//! slot and one row per quality; the templates carry no icon of their own, so a cell shows its
//! slot's icon, dark and with a question mark while the piece is unfound.

use bevy::prelude::*;
use bevy::ui::FocusPolicy;

use super::hud::{self, MenuRoot, UiArt};
use super::items::table::TEMPLATES;
use super::items::{Quality, Slot, WEIGHTS_BOSS, WEIGHTS_ELITE, WEIGHTS_FINALE, WEIGHTS_GRUNT};
use super::kit::{self, KitButton, KitFonts, KitStyle};
use super::menu::MenuBuilt;
use super::profile::Profile;
use super::Phase;
use benilla_app::survivors_host::glue::GlueVisuals;

/// Whether the collection window is open; the hub's tab sets it.
#[derive(Resource, Default)]
pub(crate) struct CollectionUi {
    pub(crate) open: bool,
    /// The clicked template, an index into [`TEMPLATES`].
    selected: Option<usize>,
}

/// The dimmed full-screen layer holding the window, hidden until opened.
#[derive(Component)]
struct CollectionPanel;
/// On [`MenuRoot`] once the window is built into it.
#[derive(Component)]
struct CollectionBuilt;

/// A button of the window.
#[derive(Component, Clone, Copy, PartialEq, Eq, Debug)]
enum CollectionAction {
    Close,
}

/// A grid cell's button: the template it shows, an index into [`TEMPLATES`].
#[derive(Component, Clone, Copy)]
struct Cell(usize);

/// A cell's icon, painted dark until its piece is found.
#[derive(Component, Clone, Copy)]
struct CellIcon(usize);

/// The question mark over an unfound cell.
#[derive(Component, Clone, Copy)]
struct CellMark(usize);

/// A text the refresh keeps current.
#[derive(Component, Clone, Copy)]
enum Label {
    /// "n of 60 pieces found".
    Found,
    /// "x/12" in a quality's row header.
    Row(usize),
    /// The bar's "n / 60".
    Progress,
    /// The detail pane: the selected piece's name.
    Name,
    /// "<Quality> <Slot>".
    Kind,
    /// "Rolls: Damage, Crit".
    Rolls,
    /// Where it drops.
    Drops,
}

/// The bar's gold fill.
#[derive(Component)]
struct BarFill;
/// The detail pane's slot icon.
#[derive(Component)]
struct PaneIcon;
/// The detail pane's body, shown while a piece is selected.
#[derive(Component)]
struct PaneBody;
/// The detail pane's hint, shown while none is.
#[derive(Component)]
struct PaneIdle;

const PANEL_W: f32 = 1000.0;
const PANEL_H: f32 = 660.0;
const CELL: f32 = 44.0;
const CELL_GAP: f32 = 2.0;
/// The width of a row header, before the gap to the first cell.
const HEAD_W: f32 = 96.0;
const PANE_W: f32 = 290.0;
const IDLE_TEXT: &str = "Click a piece to read it. Pick items up in a run to fill the book.";
/// The gold of the progress bar's fill.
const BAR_FILL: Color = Color::srgb(0.80, 0.62, 0.24);
/// An unfound piece's icon.
const DARK_ICON: Color = Color::srgb(0.2, 0.2, 0.2);

pub(crate) fn plugin(app: &mut App) {
    app.init_resource::<CollectionUi>().add_systems(
        Update,
        (build, show_panel, clicks, refresh)
            .chain()
            // After the keys, which read Enter off the window flags first.
            .after(hud::keys)
            .before(GlueVisuals),
    );
}

/// Build the window into the pick screen once it is built.
fn build(
    mut commands: Commands,
    roots: Query<Entity, (With<MenuRoot>, With<MenuBuilt>, Without<CollectionBuilt>)>,
    fonts: Res<KitFonts>,
) {
    let Ok(root) = roots.single() else { return };
    commands
        .entity(root)
        .insert(CollectionBuilt)
        .with_children(|ui| {
            window(ui, &fonts);
        });
}

/// The window: heading, the count, the grid beside the detail pane, the progress bar and Close.
fn window(ui: &mut ChildSpawnerCommands, fonts: &KitFonts) {
    let px = Val::Px;
    ui.spawn((
        CollectionPanel,
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
            kit::heading(win, fonts, "Collection", 26.0);
            win.spawn(Node {
                width: Val::Percent(100.0),
                justify_content: JustifyContent::Center,
                ..default()
            })
            .with_children(|row| {
                row.spawn((kit::body(fonts, "", 14.0, kit::CREAM), Label::Found));
            });

            // The grid and the detail pane.
            win.spawn(Node {
                width: Val::Percent(100.0),
                flex_grow: 1.0,
                column_gap: px(14.0),
                margin: UiRect::top(px(6.0)),
                ..default()
            })
            .with_children(|body| {
                grid(body, fonts);
                pane(body, fonts);
            });

            // The progress bar, its count and Close.
            win.spawn(Node {
                width: Val::Percent(100.0),
                align_items: AlignItems::Center,
                column_gap: px(14.0),
                ..default()
            })
            .with_children(|foot| {
                foot.spawn(kit::inset(Node {
                    width: px(560.0),
                    height: px(20.0),
                    padding: UiRect::all(px(2.0)),
                    ..default()
                }))
                .with_children(|bar| {
                    bar.spawn((
                        BarFill,
                        BackgroundColor(BAR_FILL),
                        Node {
                            width: Val::Percent(0.0),
                            height: Val::Percent(100.0),
                            border_radius: BorderRadius::all(px(2.0)),
                            ..default()
                        },
                    ));
                });
                foot.spawn((kit::body(fonts, "", 15.0, kit::CREAM), Label::Progress));
                foot.spawn(Node {
                    flex_grow: 1.0,
                    ..default()
                });
                kit::button(
                    foot,
                    fonts,
                    KitStyle::Normal,
                    "Close",
                    None,
                    Node {
                        width: px(150.0),
                        height: px(30.0),
                        ..default()
                    },
                    CollectionAction::Close,
                );
            });
        });
    });
}

/// The book: a header row of slot names over one row per quality.
fn grid(body: &mut ChildSpawnerCommands, fonts: &KitFonts) {
    let px = Val::Px;
    body.spawn(Node {
        width: px(HEAD_W + CELL_GAP + 12.0 * CELL + 11.0 * CELL_GAP),
        flex_direction: FlexDirection::Column,
        row_gap: px(14.0),
        ..default()
    })
    .with_children(|col| {
        col.spawn(Node {
            column_gap: px(CELL_GAP),
            ..default()
        })
        .with_children(|row| {
            row.spawn(Node {
                width: px(HEAD_W),
                ..default()
            });
            for slot in Slot::ALL {
                row.spawn(Node {
                    width: px(CELL),
                    justify_content: JustifyContent::Center,
                    ..default()
                })
                .with_children(|head| {
                    head.spawn((
                        kit::text(&fonts.body, short(slot), 10.0, kit::MUTED),
                        TextLayout::new(Justify::Center, LineBreak::NoWrap),
                        Node {
                            flex_shrink: 0.0,
                            ..default()
                        },
                    ));
                });
            }
        });
        for (qi, quality) in Quality::ALL.into_iter().enumerate() {
            col.spawn(Node {
                column_gap: px(CELL_GAP),
                align_items: AlignItems::Center,
                ..default()
            })
            .with_children(|row| {
                row.spawn(Node {
                    width: px(HEAD_W),
                    flex_direction: FlexDirection::Column,
                    ..default()
                })
                .with_children(|head| {
                    head.spawn(kit::body(fonts, format!("{quality:?}"), 16.0, quality.color()));
                    head.spawn((kit::body(fonts, "", 11.0, kit::MUTED), Label::Row(qi)));
                });
                for slot in Slot::ALL {
                    let Some(idx) = TEMPLATES
                        .iter()
                        .position(|t| t.slot == slot && t.quality == quality)
                    else {
                        // A gap in the table: an empty square keeps the columns aligned.
                        row.spawn(Node {
                            width: px(CELL),
                            height: px(CELL),
                            ..default()
                        });
                        continue;
                    };
                    cell(row, fonts, idx, slot);
                }
            });
        }
    });
}

/// The slot's name cut to fit over a 44-unit column ("SHOULDER" would run into its neighbours).
fn short(slot: Slot) -> &'static str {
    match slot {
        Slot::Shoulder => "SHLDR",
        Slot::Trinket => "TRINK",
        Slot::Weapon => "WPN",
        Slot::Hands => "HANDS",
        Slot::Chest => "CHEST",
        Slot::Waist => "WAIST",
        Slot::Head => "HEAD",
        Slot::Neck => "NECK",
        Slot::Back => "BACK",
        Slot::Legs => "LEGS",
        Slot::Feet => "FEET",
        Slot::Ring => "RING",
    }
}

/// One cell: the slot's icon, a question mark over it while unfound.
fn cell(row: &mut ChildSpawnerCommands, fonts: &KitFonts, idx: usize, slot: Slot) {
    let px = Val::Px;
    row.spawn((
        Cell(idx),
        Button,
        BackgroundColor(Color::srgba(0.0, 0.0, 0.0, 0.5)),
        BorderColor::all(kit::INSET_EDGE),
        Node {
            width: px(CELL),
            height: px(CELL),
            border: UiRect::all(px(1.0)),
            border_radius: BorderRadius::all(px(3.0)),
            justify_content: JustifyContent::Center,
            align_items: AlignItems::Center,
            ..default()
        },
    ))
    .with_children(|c| {
        c.spawn((
            CellIcon(idx),
            UiArt::crop(slot.icon(), [0.0, 1.0, 0.0, 1.0]),
            ImageNode {
                color: Color::NONE,
                ..default()
            },
            Node {
                position_type: PositionType::Absolute,
                left: px(0.0),
                top: px(0.0),
                width: Val::Percent(100.0),
                height: Val::Percent(100.0),
                ..default()
            },
        ));
        c.spawn((
            CellMark(idx),
            kit::body(fonts, "?", 22.0, kit::MUTED),
            TextLayout::new(Justify::Center, LineBreak::NoWrap),
        ));
    });
}

/// The detail pane: the selected piece, or the hint.
fn pane(body: &mut ChildSpawnerCommands, fonts: &KitFonts) {
    let px = Val::Px;
    body.spawn(kit::inset(Node {
        width: px(PANE_W),
        flex_direction: FlexDirection::Column,
        padding: UiRect::all(px(12.0)),
        row_gap: px(8.0),
        ..default()
    }))
    .with_children(|pane| {
        pane.spawn((
            PaneIdle,
            kit::body(fonts, IDLE_TEXT, 14.0, kit::MUTED),
        ));
        pane.spawn((
            PaneBody,
            Node {
                flex_direction: FlexDirection::Column,
                row_gap: px(8.0),
                display: Display::None,
                ..default()
            },
        ))
        .with_children(|sel| {
            sel.spawn(Node {
                align_items: AlignItems::Center,
                column_gap: px(10.0),
                ..default()
            })
            .with_children(|head| {
                head.spawn((
                    PaneIcon,
                    ImageNode {
                        color: Color::NONE,
                        ..default()
                    },
                    Node {
                        width: px(48.0),
                        height: px(48.0),
                        flex_shrink: 0.0,
                        ..default()
                    },
                ));
                head.spawn(Node {
                    flex_direction: FlexDirection::Column,
                    flex_shrink: 1.0,
                    ..default()
                })
                .with_children(|names| {
                    names.spawn((kit::body(fonts, "", 16.0, kit::CREAM), Label::Name));
                    names.spawn((kit::body(fonts, "", 12.0, kit::MUTED), Label::Kind));
                });
            });
            sel.spawn((kit::body(fonts, "", 13.0, kit::CREAM), Label::Rolls));
            sel.spawn(kit::label(fonts, "Drops"));
            sel.spawn((kit::body(fonts, "", 13.0, kit::CREAM), Label::Drops));
        });
    });
}

/// Close on Esc or when the pick screen goes; the display follows [`CollectionUi::open`].
fn show_panel(
    mut ui: ResMut<CollectionUi>,
    phase: Res<State<Phase>>,
    keys: Res<ButtonInput<KeyCode>>,
    mut panel: Query<&mut Node, With<CollectionPanel>>,
) {
    if ui.open && (*phase.get() != Phase::Menu || keys.just_pressed(KeyCode::Escape)) {
        ui.open = false;
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

/// The window's clicks: Close and picking a cell.
fn clicks(
    actions: Query<(&Interaction, &CollectionAction, &KitButton), Changed<Interaction>>,
    cells: Query<(&Interaction, &Cell), Changed<Interaction>>,
    phase: Res<State<Phase>>,
    mut ui: ResMut<CollectionUi>,
) {
    if *phase.get() != Phase::Menu || !ui.open {
        return;
    }
    for (interaction, action, button) in &actions {
        if *interaction != Interaction::Pressed || button.disabled {
            continue;
        }
        match action {
            CollectionAction::Close => ui.open = false,
        }
        return;
    }
    for (interaction, &Cell(idx)) in &cells {
        if *interaction == Interaction::Pressed {
            ui.selected = Some(idx);
            return;
        }
    }
}

/// "Grunts 15% of drops" for each source that can drop `quality`, one per line.
fn drop_lines(quality: Quality) -> String {
    let qi = quality as usize;
    let mut lines = Vec::new();
    for (name, weights) in [
        ("Grunts", &WEIGHTS_GRUNT),
        ("Elites", &WEIGHTS_ELITE),
        ("Bosses", &WEIGHTS_BOSS),
        ("The finale", &WEIGHTS_FINALE),
    ] {
        let total: f32 = weights.iter().sum();
        let w = weights.get(qi).copied().unwrap_or(0.0);
        if w > 0.0 && total > 0.0 {
            lines.push(format!("{name} {}% of drops", hud::num(w / total * 100.0)));
        }
    }
    if lines.is_empty() {
        "Does not drop".to_string()
    } else {
        lines.join("\n")
    }
}

/// While open, the whole window from the profile and the selection.
fn refresh(
    ui: Res<CollectionUi>,
    profile: Res<Profile>,
    assets: Option<ResMut<benilla_assets::WorldAssets>>,
    mut images: ResMut<Assets<Image>>,
    mut cells: Query<
        (&Cell, &mut Node, &mut BorderColor),
        (Without<BarFill>, Without<PaneBody>, Without<PaneIdle>),
    >,
    mut icons: Query<(&CellIcon, &mut ImageNode), Without<PaneIcon>>,
    mut marks: Query<(&CellMark, &mut Node), (Without<Cell>, Without<BarFill>, Without<PaneBody>, Without<PaneIdle>)>,
    mut fill: Query<&mut Node, (With<BarFill>, Without<Cell>, Without<CellMark>)>,
    mut body: Query<&mut Node, (With<PaneBody>, Without<Cell>, Without<CellMark>, Without<BarFill>, Without<PaneIdle>)>,
    mut idle: Query<&mut Node, (With<PaneIdle>, Without<Cell>, Without<CellMark>, Without<BarFill>, Without<PaneBody>)>,
    mut pane_icon: Query<&mut ImageNode, (With<PaneIcon>, Without<CellIcon>)>,
    mut labels: Query<(&Label, &mut Text, &mut TextColor)>,
) {
    if !ui.open {
        return;
    }
    let found = |idx: usize| {
        TEMPLATES
            .get(idx)
            .is_some_and(|t| profile.collection.contains(t.name))
    };
    let total = TEMPLATES.len();
    let have = TEMPLATES
        .iter()
        .filter(|t| profile.collection.contains(t.name))
        .count();

    for (&Cell(idx), mut node, mut edge) in &mut cells {
        let selected = ui.selected == Some(idx);
        let (width, color) = if selected {
            (2.0, kit::TITLE)
        } else if found(idx) {
            (1.0, TEMPLATES[idx].quality.color())
        } else {
            (1.0, kit::INSET_EDGE)
        };
        let want = UiRect::all(Val::Px(width));
        if node.border != want {
            node.border = want;
        }
        let want = BorderColor::all(color);
        if *edge != want {
            *edge = want;
        }
    }
    for (&CellIcon(idx), mut image) in &mut icons {
        // Until the art is read the node holds the default image and stays invisible.
        if image.image == Handle::default() {
            continue;
        }
        let want = if found(idx) { Color::WHITE } else { DARK_ICON };
        if image.color != want {
            image.color = want;
        }
    }
    for (&CellMark(idx), mut node) in &mut marks {
        let display = if found(idx) {
            Display::None
        } else {
            Display::Flex
        };
        if node.display != display {
            node.display = display;
        }
    }
    for mut node in &mut fill {
        let want = Val::Percent(have as f32 / total.max(1) as f32 * 100.0);
        if node.width != want {
            node.width = want;
        }
    }

    // The selected piece.
    let sel = ui.selected.and_then(|i| TEMPLATES.get(i).map(|t| (i, t)));
    for mut node in &mut body {
        let display = if sel.is_some() {
            Display::Flex
        } else {
            Display::None
        };
        if node.display != display {
            node.display = display;
        }
    }
    for mut node in &mut idle {
        let display = if sel.is_some() {
            Display::None
        } else {
            Display::Flex
        };
        if node.display != display {
            node.display = display;
        }
    }
    if let (Some((idx, t)), Some(mut assets)) = (sel, assets) {
        // The sprite cache answers repeats, hits and misses alike.
        if let Some(handle) = assets.sprite_texture(t.slot.icon(), &mut images) {
            for mut image in &mut pane_icon {
                if image.image != handle {
                    image.image = handle.clone();
                }
                let want = if found(idx) { Color::WHITE } else { DARK_ICON };
                if image.color != want {
                    image.color = want;
                }
            }
        }
    }

    for (label, mut text, mut color) in &mut labels {
        let (want, tint) = match *label {
            Label::Found => (
                format!("{have} of {total} pieces found"),
                if have == total { kit::TITLE } else { kit::CREAM },
            ),
            Label::Progress => (format!("{have} / {total}"), kit::CREAM),
            Label::Row(qi) => {
                let in_row = TEMPLATES
                    .iter()
                    .filter(|t| t.quality as usize == qi)
                    .count();
                let got = TEMPLATES
                    .iter()
                    .filter(|t| t.quality as usize == qi && profile.collection.contains(t.name))
                    .count();
                (format!("{got}/{in_row}"), kit::MUTED)
            }
            Label::Name | Label::Kind | Label::Rolls | Label::Drops => match sel {
                None => (String::new(), kit::MUTED),
                Some((idx, t)) => match *label {
                    Label::Name => {
                        if found(idx) {
                            (t.name.to_string(), t.quality.color())
                        } else {
                            ("Unknown item".to_string(), kit::MUTED)
                        }
                    }
                    Label::Kind => (format!("{:?} {}", t.quality, t.slot.label()), kit::MUTED),
                    Label::Rolls => (
                        format!("Rolls: {}, {}", t.stats[0].label(), t.stats[1].label()),
                        kit::CREAM,
                    ),
                    _ => (drop_lines(t.quality), kit::CREAM),
                },
            },
        };
        if text.0 != want {
            text.0 = want;
        }
        if color.0 != tint {
            color.0 = tint;
        }
    }
}
