//! The run's pick screen, built from the mode's own kit ([`super::kit`]): a title and the
//! Talents, Trainer and Options tabs with the bank on top, the class and race picks in a panel on
//! the left, the zone list in a panel on the right, and the character line, the difficulty and the
//! Enter button at the bottom. The hero model stands in the open middle; it is a world scene
//! behind the UI.
//!
//! The survivors HUD lays out on the 768-unit canvas through `UiScale`, so every size here is in
//! those units.

use bevy::prelude::*;

use super::hud::{self, looks_for, rgb, start_run, MenuRoot};
use super::kit::{self, KitButton, KitFonts, KitStyle};
use super::profile::{xp_to_next, zone_min_level, Profile, MAX_LEVEL};
use super::{data, races, Phase, Rng, Run, Selection};
use benilla_app::survivors_host::char_select::race_name;
use benilla_app::survivors_host::glue::art::{class_tc, race_tc, tc_rect, GlueArt};
use benilla_app::survivors_host::glue::GlueVisuals;
use benilla_app::survivors_host::ui_action::Spells;

/// The create screen's two race columns, here two rows.
const ALLIANCE: [u8; 4] = [1, 3, 4, 7];
const HORDE: [u8; 4] = [2, 5, 6, 8];

/// The side panels' width, and the room above them for the title bar.
const PANEL_W: f32 = 300.0;
const PANEL_TOP: f32 = 96.0;
const PANEL_EDGE_GAP: f32 = 12.0;

/// A class button's icon size and the cell around it (icon and name).
const CLASS_ICON: f32 = 50.0;
const CLASS_CELL_W: f32 = 88.0;
/// A race face button.
const RACE_ICON: f32 = 40.0;
/// A zone row's height.
const ZONE_ROW_H: f32 = 30.0;

#[derive(Component, Clone, Copy, PartialEq, Eq, Debug)]
pub(super) enum MenuAction {
    Race(u8),
    Sex(u8),
    Class(usize),
    Map(usize),
    Randomize,
    Enter,
}

/// On [`MenuRoot`] once the glue art is in and the screen is built.
#[derive(Component)]
pub(super) struct MenuBuilt;

/// A race button's face, re-cropped to the picked gender.
#[derive(Component)]
pub(super) struct RaceFace(u8);

/// The character line above the Enter button ("Level 12 Night Elf Warrior").
#[derive(Component)]
pub(super) struct PickName;

/// The saved pick, `benilla-config/survivors-pick.txt` (`localStorage` on the page):
/// `class race sex map`, restored at the menu so a reload keeps the last hero and zone.
fn pick_path() -> Option<std::path::PathBuf> {
    benilla_app::survivors_host::local_state::home().map(|h| h.join("survivors-pick.txt"))
}

fn save_pick(sel: &Selection) {
    let (Some(class), Some(&(_, race, sex))) = (sel.class, sel.look.and_then(|i| sel.looks.get(i)))
    else {
        return;
    };
    if let Some(path) = pick_path() {
        let _ =
            benilla_app::survivors_host::local_state::write_atomic(&path, &format!("{class} {race} {sex} {}", sel.map));
    }
}

/// Restore the saved pick once the creature data can resolve its looks; a stale or missing file
/// leaves the default.
pub(super) fn restore_pick(
    creatures: Option<Res<benilla_app::survivors_host::entities::Creatures>>,
    mut selection: ResMut<Selection>,
    mut done: Local<bool>,
) {
    if *done {
        return;
    }
    let Some(creatures) = creatures.as_deref() else {
        return;
    };
    *done = true;
    let Some(text) = pick_path().and_then(|p| benilla_app::survivors_host::local_state::read_to_string(&p).ok()) else {
        return;
    };
    let v: Vec<usize> = text
        .split_whitespace()
        .filter_map(|t| t.parse().ok())
        .collect();
    let [class, race, sex, map] = v[..] else {
        return;
    };
    if map < data::MAPS.len() {
        selection.map = map;
        selection.stage = map;
        selection.version += 1;
    }
    if class < data::CLASSES.len() {
        let all = all_looks(creatures);
        if let Some(l) = find(&all[class], Some(race as u8), Some(sex as u8)) {
            set(&mut selection, &all, class, l);
        }
    }
}

/// Every class's looks, `(display, race, sex)`, indexed like [`data::CLASSES`].
fn all_looks(creatures: &benilla_app::survivors_host::entities::Creatures) -> Vec<Vec<(u32, u8, u8)>> {
    data::CLASSES
        .iter()
        .map(|c| looks_for(c, creatures))
        .collect()
}

fn find(looks: &[(u32, u8, u8)], race: Option<u8>, sex: Option<u8>) -> Option<usize> {
    looks
        .iter()
        .position(|&(_, r, s)| race.is_none_or(|x| x == r) && sex.is_none_or(|x| x == s))
}

fn set(sel: &mut Selection, all: &[Vec<(u32, u8, u8)>], class: usize, look: usize) {
    sel.class = Some(class);
    sel.looks = all[class].clone();
    sel.look = Some(look);
    sel.version += 1;
}

/// A tab in the top bar: opens one of the windows that are built into the screen.
#[derive(Component, Clone, Copy, PartialEq, Eq, Debug)]
enum MenuTab {
    Talents,
    Trainer,
    Collection,
    Loot,
    Achievements,
    /// The co-op window ([`super::coop`]); the tab exists only in a build with co-op.
    Coop,
    Options,
}

/// The bank's number in the top bar.
#[derive(Component)]
struct BankText;

/// A zone row's name text.
#[derive(Component)]
struct ZoneName(usize);

/// A zone row's level text; `lo` and `hi` are the lowest and highest creature level of the zone.
#[derive(Component)]
struct ZoneLevels {
    map: usize,
    lo: u32,
    hi: u32,
}

/// A zone row's second line: the selected class's best there ([`super::records::note`]).
#[derive(Component)]
struct ZoneBest(usize);

/// A zone row, painted by selection and hover.
#[derive(Component)]
struct ZoneRow(usize);

/// A zone row's padlock, shown while the selected class's character is below the zone.
#[derive(Component)]
struct ZoneLock(usize);

/// The red "Requires level X" line above the character line.
#[derive(Component)]
struct ZoneWarn;

/// The fill of the character's XP bar under [`PickName`].
#[derive(Component)]
struct PickXpFill;

/// The Enter button, whose caption names the zone picked.
#[derive(Component)]
struct EnterButton;

/// The picked class's name in the left panel, in its class colour.
#[derive(Component)]
struct ClassTitle;

/// A stat box's value text in the left panel.
#[derive(Component, Clone, Copy, PartialEq, Eq)]
enum StatValue {
    Health,
    Start,
    Zones,
    /// The picked race's racial trait.
    Racial,
}

/// A zone row's fill and edge: picked, and hovered.
const ROW_FILL_ON: Color = Color::srgba(0.22, 0.16, 0.07, 0.6);
const ROW_FILL_HOT: Color = Color::srgba(0.12, 0.09, 0.04, 0.55);
const ROW_EDGE_ON: Color = Color::srgb(0.98, 0.82, 0.45);
const ROW_EDGE_HOT: Color = Color::srgb(0.85, 0.68, 0.36);
/// The HUD's XP bar colour.
const XP_PURPLE: Color = Color::srgb(0.58, 0.0, 0.55);

/// A face (sheet and crop) for a race button, or `None` while the art is not in.
fn race_face(art: &GlueArt, race: u8) -> Option<(Handle<Image>, Rect)> {
    let (sheet, size) = art.races.as_ref()?;
    race_tc(race, 0).map(|tc| (sheet.clone(), tc_rect(*size, tc)))
}

/// A face (sheet and crop) for a class button, or `None` while the art is not in.
fn class_face(art: &GlueArt, class: u8) -> Option<(Handle<Image>, Rect)> {
    let (sheet, size) = art.classes.as_ref()?;
    class_tc(class).map(|tc| (sheet.clone(), tc_rect(*size, tc)))
}

/// The lowest and highest creature level of `map`.
fn zone_levels(map: usize) -> (u32, u32) {
    let levels = data::roster(map).enemies.iter().map(|e| e.level);
    let lo = levels.clone().min().unwrap_or(1);
    let hi = levels.max().unwrap_or(lo);
    (lo, hi)
}

/// "Level 12" or "Level 12-20".
fn level_range(lo: u32, hi: u32) -> String {
    if lo == hi {
        format!("Level {lo}")
    } else {
        format!("Level {lo}-{hi}")
    }
}

/// A square kit button with a picture in it (a race face, a class icon). `face_marker` goes on
/// the picture, so a refresh can find it. The button carries [`KitButton`], so `selected` lights
/// its edge.
fn icon_button(
    parent: &mut ChildSpawnerCommands,
    size: f32,
    action: MenuAction,
    face: Option<(Handle<Image>, Rect)>,
    face_marker: impl Bundle,
) {
    let px = Val::Px;
    parent
        .spawn((
            Button,
            action,
            KitButton::default(),
            Node {
                width: px(size),
                height: px(size),
                border: UiRect::all(px(2.0)),
                border_radius: BorderRadius::all(px(3.0)),
                ..default()
            },
            BackgroundGradient::from(LinearGradient::to_bottom(vec![
                ColorStop::percent(Color::srgb(0.20, 0.075, 0.05), 0.0),
                ColorStop::percent(Color::srgb(0.11, 0.04, 0.03), 100.0),
            ])),
            BorderColor::all(Color::srgb(0.55, 0.42, 0.21)),
        ))
        .with_children(|b| {
            let image = match face {
                Some((sheet, rect)) => ImageNode {
                    image: sheet,
                    rect: Some(rect),
                    ..default()
                },
                None => ImageNode::default(),
            };
            b.spawn((
                image,
                Node {
                    width: Val::Percent(100.0),
                    height: Val::Percent(100.0),
                    ..default()
                },
                face_marker,
            ));
        });
}

/// Build the screen into [`MenuRoot`] once the archives are readable, so the glue art is there.
#[allow(clippy::too_many_arguments)]
pub(super) fn build_menu(
    mut commands: Commands,
    roots: Query<Entity, (With<MenuRoot>, Without<MenuBuilt>)>,
    assets: Option<ResMut<benilla_assets::WorldAssets>>,
    mut art: ResMut<GlueArt>,
    mut images: ResMut<Assets<Image>>,
    mut add_mats: ResMut<Assets<benilla_app::survivors_host::glue::add_material::AddUiMaterial>>,
    kit_fonts: Res<KitFonts>,
) {
    let Ok(root) = roots.single() else { return };
    let Some(mut assets) = assets else { return };
    art.ensure_loaded(&mut assets, &mut images, &mut add_mats);
    let lock = assets.sprite_texture("Interface\\ChatFrame\\UI-ChatFrame-LockIcon", &mut images);
    let skull = assets.sprite_texture(super::difficulty::SKULL, &mut images);
    let art = &*art;
    let fonts = &*kit_fonts;
    commands.entity(root).insert(MenuBuilt).with_children(|ui| {
        top_bar(ui, fonts);
        class_panel(ui, fonts, art);
        zone_panel(ui, fonts, lock);
        bottom_block(ui, fonts, skull);
    });
}

/// The top bar: the title, the three tabs under it and the bank at the right end.
fn top_bar(ui: &mut ChildSpawnerCommands, fonts: &KitFonts) {
    let px = Val::Px;
    ui.spawn(Node {
        position_type: PositionType::Absolute,
        top: px(8.0),
        width: Val::Percent(100.0),
        flex_direction: FlexDirection::Column,
        align_items: AlignItems::Center,
        row_gap: px(6.0),
        ..default()
    })
    .with_children(|bar| {
        bar.spawn(kit::title(fonts, "Azeroth Survivors", 40.0));
        bar.spawn(Node {
            column_gap: px(8.0),
            ..default()
        })
        .with_children(|tabs| {
            for (tab, caption) in [
                (MenuTab::Talents, "Talents"),
                (MenuTab::Trainer, "Trainer"),
                (MenuTab::Collection, "Collection"),
                (MenuTab::Loot, "Loot Tables"),
                (MenuTab::Achievements, "Achievements"),
                (MenuTab::Coop, "Co-op"),
                (MenuTab::Options, "Options"),
            ]
            .into_iter()
            .filter(|(tab, _)| *tab != MenuTab::Coop || cfg!(all(feature = "coop", not(target_arch = "wasm32"))))
            {
                kit::button(
                    tabs,
                    fonts,
                    KitStyle::Tab,
                    caption,
                    None,
                    Node {
                        min_width: px(100.0),
                        height: px(28.0),
                        ..default()
                    },
                    tab,
                );
            }
        });
    });
    // The bank, at the right end of the bar.
    ui.spawn(Node {
        position_type: PositionType::Absolute,
        right: px(20.0),
        top: px(18.0),
        align_items: AlignItems::Center,
        column_gap: px(5.0),
        ..default()
    })
    .with_children(|bank| {
        bank.spawn(hud::coin_icon(14.0));
        bank.spawn((BankText, kit::body(fonts, "", 16.0, kit::CREAM)));
    });
}

/// The left panel: the class grid, the races and genders, the class title with its stat boxes,
/// and Randomize.
fn class_panel(ui: &mut ChildSpawnerCommands, fonts: &KitFonts, art: &GlueArt) {
    let px = Val::Px;
    ui.spawn(kit::panel(Node {
        position_type: PositionType::Absolute,
        left: px(PANEL_EDGE_GAP),
        top: px(PANEL_TOP),
        bottom: px(PANEL_EDGE_GAP),
        width: px(PANEL_W),
        flex_direction: FlexDirection::Column,
        padding: UiRect::all(px(14.0)),
        row_gap: px(6.0),
        ..default()
    }))
    .with_children(|panel| {
        kit::heading(panel, fonts, "Class", 20.0);

        // The class grid, three to a row; a class the race cannot be leaves it.
        panel
            .spawn(Node {
                width: Val::Percent(100.0),
                flex_direction: FlexDirection::Row,
                flex_wrap: FlexWrap::Wrap,
                justify_content: JustifyContent::Center,
                column_gap: px(4.0),
                row_gap: px(6.0),
                ..default()
            })
            .with_children(|grid| {
                for (i, class) in data::CLASSES.iter().enumerate() {
                    grid.spawn(Node {
                        width: px(CLASS_CELL_W),
                        flex_direction: FlexDirection::Column,
                        align_items: AlignItems::Center,
                        row_gap: px(2.0),
                        ..default()
                    })
                    .with_children(|cell| {
                        icon_button(cell, CLASS_ICON, MenuAction::Class(i), class_face(art, class.id), ());
                        cell.spawn((
                            kit::body(fonts, class.name, 11.0, kit::CREAM),
                            TextLayout::new_with_justify(Justify::Center),
                        ));
                    });
                }
            });

        // The races, one row per faction.
        for (faction, races) in [("Alliance", ALLIANCE), ("Horde", HORDE)] {
            panel
                .spawn(Node {
                    width: Val::Percent(100.0),
                    align_items: AlignItems::Center,
                    column_gap: px(6.0),
                    ..default()
                })
                .with_children(|row| {
                    row.spawn(Node {
                        width: px(62.0),
                        ..default()
                    })
                    .with_children(|l| {
                        l.spawn(kit::label(fonts, faction));
                    });
                    for race in races {
                        icon_button(row, RACE_ICON, MenuAction::Race(race), race_face(art, race), RaceFace(race));
                    }
                });
        }

        // The gender pair.
        panel
            .spawn(Node {
                width: Val::Percent(100.0),
                justify_content: JustifyContent::Center,
                column_gap: px(6.0),
                ..default()
            })
            .with_children(|row| {
                for (sex, caption) in [(0u8, "Male"), (1u8, "Female")] {
                    kit::button(
                        row,
                        fonts,
                        KitStyle::Normal,
                        caption,
                        None,
                        Node {
                            width: px(84.0),
                            height: px(26.0),
                            ..default()
                        },
                        MenuAction::Sex(sex),
                    );
                }
            });

        panel.spawn(kit::rule(Val::Percent(100.0)));

        // The pick, named in its class colour, and what it brings.
        panel
            .spawn(Node {
                width: Val::Percent(100.0),
                justify_content: JustifyContent::Center,
                ..default()
            })
            .with_children(|row| {
                row.spawn((ClassTitle, kit::text(&fonts.title, "", 22.0, kit::TITLE)));
            });
        panel
            .spawn(Node {
                width: Val::Percent(100.0),
                column_gap: px(6.0),
                ..default()
            })
            .with_children(|row| {
                for (caption, marker) in [("Health", StatValue::Health), ("Zones open", StatValue::Zones)] {
                    kit::stat_box(
                        row,
                        fonts,
                        caption,
                        "",
                        Node {
                            flex_grow: 1.0,
                            flex_basis: px(0.0),
                            ..default()
                        },
                        marker,
                    );
                }
            });
        kit::stat_box(
            panel,
            fonts,
            "Starts with",
            "",
            Node {
                width: Val::Percent(100.0),
                ..default()
            },
            StatValue::Start,
        );
        kit::stat_box(
            panel,
            fonts,
            "Racial",
            "",
            Node {
                width: Val::Percent(100.0),
                ..default()
            },
            StatValue::Racial,
        );

        // Randomize sits at the foot of the panel.
        panel.spawn(Node {
            flex_grow: 1.0,
            ..default()
        });
        kit::button(
            panel,
            fonts,
            KitStyle::Normal,
            "Randomize",
            None,
            Node {
                width: Val::Percent(100.0),
                height: px(28.0),
                ..default()
            },
            MenuAction::Randomize,
        );
    });
}

/// The right panel: one row per zone, lowest level first. A locked zone is dimmed, with its
/// padlock; `refresh_zone_rows` keeps them current.
fn zone_panel(ui: &mut ChildSpawnerCommands, fonts: &KitFonts, lock: Option<Handle<Image>>) {
    let px = Val::Px;
    let mut order: Vec<(usize, u32, u32)> = (0..data::MAPS.len())
        .map(|i| {
            let (lo, hi) = zone_levels(i);
            (i, lo, hi)
        })
        .collect();
    order.sort_by_key(|&(i, lo, _)| (lo, data::MAPS[i].name));

    ui.spawn(kit::panel(Node {
        position_type: PositionType::Absolute,
        right: px(PANEL_EDGE_GAP),
        top: px(PANEL_TOP),
        bottom: px(PANEL_EDGE_GAP),
        width: px(PANEL_W),
        flex_direction: FlexDirection::Column,
        padding: UiRect::all(px(14.0)),
        row_gap: px(6.0),
        ..default()
    }))
    .with_children(|panel| {
        kit::heading(panel, fonts, "Zone", 20.0);
        panel
            .spawn(Node {
                width: Val::Percent(100.0),
                flex_grow: 1.0,
                min_height: px(0.0),
                flex_direction: FlexDirection::Column,
                row_gap: px(4.0),
                overflow: Overflow::scroll_y(),
                ..default()
            })
            .with_children(|list| {
                for &(i, lo, hi) in &order {
                    let map = &data::MAPS[i];
                    list.spawn((
                        Button,
                        MenuAction::Map(i),
                        ZoneRow(i),
                        kit::inset(Node {
                            width: Val::Percent(100.0),
                            height: px(ZONE_ROW_H),
                            flex_shrink: 0.0,
                            justify_content: JustifyContent::SpaceBetween,
                            align_items: AlignItems::Center,
                            padding: UiRect::axes(px(10.0), px(0.0)),
                            ..default()
                        }),
                    ))
                    .with_children(|row| {
                        row.spawn((ZoneName(i), kit::title(fonts, map.name, 18.0)));
                        // The level range over the best run, right-aligned; the best's line
                        // keeps its height when empty so the rows stay level.
                        row.spawn(Node {
                            flex_direction: FlexDirection::Column,
                            align_items: AlignItems::FlexEnd,
                            ..default()
                        })
                        .with_children(|right| {
                            right
                                .spawn(Node {
                                    align_items: AlignItems::Center,
                                    column_gap: px(4.0),
                                    ..default()
                                })
                                .with_children(|top| {
                                    // The padlock of a zone the character is too low for.
                                    top.spawn((
                                        ZoneLock(i),
                                        Visibility::Hidden,
                                        lock.clone().map(ImageNode::new).unwrap_or_default(),
                                        Node {
                                            width: px(13.0),
                                            height: px(13.0),
                                            ..default()
                                        },
                                    ));
                                    top.spawn((
                                        ZoneLevels { map: i, lo, hi },
                                        kit::body(fonts, level_range(lo, hi), 11.0, kit::MUTED),
                                    ));
                                });
                            right
                                .spawn(Node {
                                    height: px(12.0),
                                    ..default()
                                })
                                .with_children(|line| {
                                    line.spawn((
                                        ZoneBest(i),
                                        kit::body(fonts, "", 10.0, kit::MUTED),
                                    ));
                                });
                        });
                    });
                }
            });
    });
}

/// The bottom centre: the red zone warning, the character line with its XP bar, and under them
/// the difficulty control left of the Enter button. The row wraps, so a narrow screen puts the
/// difficulty above Enter.
fn bottom_block(ui: &mut ChildSpawnerCommands, fonts: &KitFonts, skull: Option<Handle<Image>>) {
    let px = Val::Px;
    ui.spawn(Node {
        position_type: PositionType::Absolute,
        left: px(PANEL_W + 2.0 * PANEL_EDGE_GAP),
        right: px(PANEL_W + 2.0 * PANEL_EDGE_GAP),
        bottom: px(14.0),
        flex_direction: FlexDirection::Column,
        align_items: AlignItems::Center,
        row_gap: px(4.0),
        ..default()
    })
    .with_children(|block| {
        block.spawn((ZoneWarn, kit::body(fonts, "", 14.0, kit::WARN)));
        block.spawn((PickName, kit::text(&fonts.title, "", 22.0, kit::TITLE)));
        block
            .spawn(kit::inset(Node {
                width: px(220.0),
                height: px(10.0),
                padding: UiRect::all(px(1.0)),
                ..default()
            }))
            .with_children(|frame| {
                frame.spawn((
                    PickXpFill,
                    Node {
                        width: Val::Percent(0.0),
                        height: Val::Percent(100.0),
                        border_radius: BorderRadius::all(px(2.0)),
                        ..default()
                    },
                    BackgroundColor(XP_PURPLE),
                ));
            });
        block
            .spawn(Node {
                width: Val::Percent(100.0),
                flex_wrap: FlexWrap::Wrap,
                justify_content: JustifyContent::Center,
                align_items: AlignItems::Center,
                column_gap: px(14.0),
                row_gap: px(8.0),
                margin: UiRect::top(px(6.0)),
                ..default()
            })
            .with_children(|row| {
                super::difficulty::spawn_control(row, fonts, skull);
                kit::button(
                    row,
                    fonts,
                    KitStyle::Primary,
                    "Enter World",
                    Some(""),
                    Node {
                        width: px(360.0),
                        max_width: Val::Percent(100.0),
                        height: px(64.0),
                        ..default()
                    },
                    (MenuAction::Enter, EnterButton),
                );
            });
    });
}

/// A press on a pick-screen button: race, gender, class, zone, Randomize or Enter.
#[allow(clippy::too_many_arguments)]
pub(super) fn menu_clicks(
    pressed: Query<(&Interaction, &MenuAction), Changed<Interaction>>,
    phase: Res<State<Phase>>,
    creatures: Option<Res<benilla_app::survivors_host::entities::Creatures>>,
    mut selection: ResMut<Selection>,
    mut run: ResMut<Run>,
    profile: Res<super::profile::Profile>,
    mut rng: ResMut<Rng>,
    mut next: ResMut<NextState<Phase>>,
    role: Res<super::NetRole>,
) {
    if *phase.get() != Phase::Menu {
        return;
    }
    let Some(creatures) = creatures.as_deref() else {
        return;
    };
    let Some(action) = pressed
        .iter()
        .find(|(interaction, _)| **interaction == Interaction::Pressed)
        .map(|(_, a)| *a)
    else {
        return;
    };
    let all = all_looks(creatures);
    let cur = selection
        .look
        .and_then(|i| selection.looks.get(i))
        .map(|&(_, r, s)| (r, s));
    let (race, sex) = (cur.map(|c| c.0), cur.map_or(0, |c| c.1));
    match action {
        MenuAction::Map(i) => {
            if selection.map != i {
                selection.map = i;
                selection.version += 1;
            }
        }
        // A co-op guest enters the host's run when the host starts it ([`super::coop`]).
        MenuAction::Enter if *role == super::NetRole::Client => {}
        MenuAction::Enter => {
            if start_run(&mut run, &mut selection, &profile) {
                next.set(Phase::Playing);
            }
        }
        MenuAction::Randomize => {
            let picks: Vec<(usize, usize)> = all
                .iter()
                .enumerate()
                .flat_map(|(c, looks)| (0..looks.len()).map(move |l| (c, l)))
                .collect();
            if !picks.is_empty() {
                let (c, l) = picks[rng.index(picks.len())];
                set(&mut selection, &all, c, l);
            }
        }
        // The class keeps the race and gender where it can.
        MenuAction::Class(c) => {
            let looks = &all[c];
            let l = find(looks, race, Some(sex))
                .or_else(|| find(looks, race, None))
                .or_else(|| find(looks, None, Some(sex)))
                .or_else(|| find(looks, None, None));
            if let Some(l) = l {
                set(&mut selection, &all, c, l);
            }
        }
        // The race keeps the class where it can, else takes the first class it has.
        MenuAction::Race(r) => {
            let classes = selection.class.into_iter().chain(0..all.len());
            for c in classes {
                let l = find(&all[c], Some(r), Some(sex)).or_else(|| find(&all[c], Some(r), None));
                if let Some(l) = l {
                    set(&mut selection, &all, c, l);
                    break;
                }
            }
        }
        MenuAction::Sex(s) => {
            if let Some(c) = selection.class {
                if let Some(l) = find(&all[c], race, Some(s)) {
                    set(&mut selection, &all, c, l);
                }
            }
        }
    }
    save_pick(&selection);
}

/// The screen's per-frame state: which button is lit, which races and classes the pick allows,
/// the gender on the race faces and whether Enter is off.
pub(super) fn refresh_menu_ui(
    selection: Res<Selection>,
    profile: Option<Res<Profile>>,
    creatures: Option<Res<benilla_app::survivors_host::entities::Creatures>>,
    art: Res<GlueArt>,
    mut buttons: Query<(&MenuAction, Option<&ChildOf>, &mut Node, Option<&mut KitButton>)>,
    mut cells: Query<&mut Node, Without<MenuAction>>,
    mut faces: Query<(&RaceFace, &mut ImageNode)>,
    mut cache: Local<(Option<u32>, bool, Vec<Vec<(u32, u8, u8)>>)>,
) {
    let have = creatures.is_some();
    if cache.0 != Some(selection.version) || cache.1 != have {
        *cache = (
            Some(selection.version),
            have,
            creatures.as_deref().map(all_looks).unwrap_or_default(),
        );
    }
    let all = &cache.2;
    let profile = profile.as_deref();
    let cur = selection
        .look
        .and_then(|i| selection.looks.get(i))
        .map(|&(_, r, s)| (r, s));
    let class_locked = selection
        .class
        .zip(profile)
        .is_some_and(|(c, p)| !p.class_unlocked(c));
    let enter_off = cur.is_none() || class_locked || zone_locked(&selection, profile, selection.map);

    for (action, parent, mut node, kit_button) in &mut buttons {
        let selected = match *action {
            MenuAction::Race(r) => cur.is_some_and(|c| c.0 == r),
            MenuAction::Sex(s) => cur.is_some_and(|c| c.1 == s),
            MenuAction::Class(i) => selection.class == Some(i),
            MenuAction::Map(_) | MenuAction::Randomize | MenuAction::Enter => false,
        };
        if let Some(mut button) = kit_button {
            if button.selected != selected {
                button.selected = selected;
            }
            let off = *action == MenuAction::Enter && enter_off;
            if button.disabled != off {
                button.disabled = off;
            }
        }
        match *action {
            // As on the create screen, a class the race cannot be leaves the grid; its cell
            // (icon and name) goes with it.
            MenuAction::Class(i) => {
                let ok = cur.is_none_or(|(r, _)| {
                    all.get(i).is_some_and(|l| find(l, Some(r), None).is_some())
                });
                let display = if ok { Display::Flex } else { Display::None };
                if let Some(Ok(mut cell)) = parent.map(|p| cells.get_mut(p.parent())) {
                    if cell.display != display {
                        cell.display = display;
                    }
                }
            }
            MenuAction::Race(r) => {
                let ok = all.is_empty() || all.iter().any(|l| find(l, Some(r), None).is_some());
                let display = if ok { Display::Flex } else { Display::None };
                if node.display != display {
                    node.display = display;
                }
            }
            _ => {}
        }
    }

    if let (Some((_, size)), Some((_, sex))) = (&art.races, cur) {
        for (face, mut img) in &mut faces {
            if let Some(tc) = race_tc(face.0, sex) {
                let rect = Some(tc_rect(*size, tc));
                if img.rect != rect {
                    img.rect = rect;
                }
            }
        }
    }
}

/// The menu's own systems beyond the plugin's shared tuple: the tabs, the bank, the class
/// stats, the zone rows and the character line.
pub(super) fn plugin(app: &mut App) {
    app.add_systems(
        Update,
        (
            tab_clicks,
            refresh_tabs,
            refresh_bank,
            refresh_stats,
            refresh_zone_rows,
            refresh_pick_line,
            refresh_enter,
        )
            .after(refresh_menu_ui)
            .before(GlueVisuals)
            .run_if(in_state(Phase::Menu)),
    );
}

/// Whether the selected class's character is too low for `map`. False with no class or profile.
fn zone_locked(selection: &Selection, profile: Option<&Profile>, map: usize) -> bool {
    selection
        .class
        .zip(profile)
        .is_some_and(|(class, p)| !p.zone_unlocked(class, map))
}

/// A press on a tab opens its window by setting the window's own flag.
fn tab_clicks(
    tabs: Query<(&Interaction, &MenuTab), Changed<Interaction>>,
    mut talents: ResMut<super::talents::TalentsUi>,
    mut trainer: ResMut<super::trainer::TrainerUi>,
    mut options: ResMut<super::options::OptionsUi>,
    mut collection: ResMut<super::collection::CollectionUi>,
    mut loot: ResMut<super::loottables::LootUi>,
    mut achievements: ResMut<super::achievements::AchievementsUi>,
    mut coop: Option<ResMut<super::CoopUi>>,
) {
    let Some(tab) = tabs
        .iter()
        .find(|(interaction, _)| **interaction == Interaction::Pressed)
        .map(|(_, t)| *t)
    else {
        return;
    };
    match tab {
        MenuTab::Talents => talents.open = true,
        MenuTab::Trainer => trainer.open = true,
        MenuTab::Options => options.open = true,
        MenuTab::Collection => collection.open = true,
        MenuTab::Loot => loot.open = true,
        MenuTab::Achievements => achievements.open = true,
        MenuTab::Coop => {
            if let Some(coop) = coop.as_mut() {
                coop.open = true;
            }
        }
    }
}

/// The Talents tab shows how many points the picked class has left to spend.
fn refresh_tabs(
    selection: Res<Selection>,
    profile: Option<Res<Profile>>,
    tabs: Query<(&MenuTab, &Children)>,
    mut captions: Query<&mut Text, With<kit::KitCaption>>,
) {
    let Some(profile) = profile else { return };
    let left = selection.class.map_or(0, |c| {
        super::talents::available(&profile.talents, c, profile.character(c).level)
    });
    let want = if left > 0 {
        format!("Talents ({left})")
    } else {
        "Talents".to_string()
    };
    for (tab, children) in &tabs {
        if *tab != MenuTab::Talents {
            continue;
        }
        for child in children.iter() {
            if let Ok(mut text) = captions.get_mut(child) {
                if text.0 != want {
                    text.0 = want.clone();
                }
            }
        }
    }
}

/// The bank in the top bar.
fn refresh_bank(profile: Option<Res<Profile>>, mut bank: Query<&mut Text, With<BankText>>) {
    let Some(profile) = profile else { return };
    let want = hud::grouped(profile.gold);
    for mut text in &mut bank {
        if text.0 != want {
            text.0 = want.clone();
        }
    }
}

/// The class title and its stat boxes: starting health, the first ability, the zones open.
fn refresh_stats(
    selection: Res<Selection>,
    profile: Option<Res<Profile>>,
    spells: Option<Res<Spells>>,
    mut values: Query<(&StatValue, &mut Text), Without<ClassTitle>>,
    mut title: Query<(&mut Text, &mut TextColor), (With<ClassTitle>, Without<StatValue>)>,
) {
    let class = selection.class_def();
    let race = selection
        .look
        .and_then(|i| selection.looks.get(i))
        .map_or(0, |&(_, r, _)| r);
    if let Ok((mut text, mut color)) = title.single_mut() {
        let want = class.map_or("", |c| c.name);
        if text.0 != want {
            text.0 = want.to_string();
        }
        let tint = class.map_or(kit::TITLE, |c| rgb(c.color));
        if color.0 != tint {
            color.0 = tint;
        }
    }
    // Health builds a whole `Run`, so it is only worked out when something it reads changed.
    let stale = selection.is_changed()
        || profile.as_ref().is_some_and(|p| p.is_changed())
        || values.iter().any(|(_, t)| t.0.is_empty());
    if !stale {
        return;
    }
    let profile = profile.as_deref();
    for (value, mut text) in &mut values {
        let (Some(def), Some(index)) = (class, selection.class) else {
            if !text.0.is_empty() {
                text.0.clear();
            }
            continue;
        };
        let want = match value {
            StatValue::Health => {
                let mut run = Run {
                    me: crate::HeroState {
                        class: def,
                        race,
                        ..Default::default()
                    },
                    ..Run::default()
                };
                if let Some(p) = profile {
                    run.meta = p.bonus(index);
                    run.talents = p.talent_bonus(index);
                }
                format!("{}", run.stats().max_hp.round() as u32)
            }
            StatValue::Start => {
                let ability = data::ability(def.start);
                hud::spell_name(spells.as_deref(), ability.spell, def.start)
            }
            StatValue::Racial => races::trait_text(race).unwrap_or_default(),
            StatValue::Zones => {
                let total = data::MAPS.len();
                let open = profile.map_or(total, |p| {
                    (0..total).filter(|&m| p.zone_unlocked(index, m)).count()
                });
                format!("{open} of {total}")
            }
        };
        if text.0 != want {
            text.0 = want;
        }
    }
}

/// The zone rows: the picked one has a bright edge and a lighter fill; a zone above the selected
/// character's level is dimmed, shows its padlock and says what level it needs.
fn refresh_zone_rows(
    selection: Res<Selection>,
    profile: Option<Res<Profile>>,
    mut rows: Query<(&ZoneRow, &Interaction, &mut BackgroundColor, &mut BorderColor)>,
    mut names: Query<(&ZoneName, &mut TextColor), Without<ZoneLevels>>,
    mut levels: Query<(&ZoneLevels, &mut Text, &mut TextColor), Without<ZoneName>>,
    mut bests: Query<
        (&ZoneBest, &mut Text, &mut TextColor),
        (Without<ZoneLevels>, Without<ZoneName>),
    >,
    mut locks: Query<(&ZoneLock, &mut Visibility)>,
) {
    let profile = profile.as_deref();
    for (row, interaction, mut fill, mut edge) in &mut rows {
        let (want_fill, want_edge) = if selection.map == row.0 {
            (ROW_FILL_ON, ROW_EDGE_ON)
        } else if *interaction != Interaction::None {
            (ROW_FILL_HOT, ROW_EDGE_HOT)
        } else {
            (kit::INSET_FILL, kit::INSET_EDGE)
        };
        if fill.0 != want_fill {
            fill.0 = want_fill;
        }
        if edge.top != want_edge {
            *edge = BorderColor::all(want_edge);
        }
    }
    for (name, mut color) in &mut names {
        let want = if zone_locked(&selection, profile, name.0) {
            kit::DISABLED
        } else {
            kit::TITLE
        };
        if color.0 != want {
            color.0 = want;
        }
    }
    for (row, mut text, mut color) in &mut levels {
        let want = if zone_locked(&selection, profile, row.map) {
            format!("Locked, level {}", row.lo)
        } else {
            level_range(row.lo, row.hi)
        };
        if text.0 != want {
            text.0 = want;
        }
        if color.0 != kit::MUTED {
            color.0 = kit::MUTED;
        }
    }
    // The selected class's best per zone, in gold once it has won there.
    let class_id = selection.class.map(|c| data::CLASSES[c].id);
    for (zone, mut text, mut color) in &mut bests {
        let best = class_id
            .zip(profile)
            .and_then(|(id, p)| p.records.get(&super::records::key(zone.0, id)));
        let want = super::records::note(best);
        if text.0 != want {
            text.0 = want;
        }
        let want = if best.is_some_and(|b| b.wins > 0) {
            kit::TITLE
        } else {
            kit::MUTED
        };
        if color.0 != want {
            color.0 = want;
        }
    }
    for (lock, mut vis) in &mut locks {
        let want = if zone_locked(&selection, profile, lock.0) {
            Visibility::Inherited
        } else {
            Visibility::Hidden
        };
        if *vis != want {
            *vis = want;
        }
    }
}

/// The character line above the Enter button ("Level 12 Night Elf Warrior"), its XP bar, and the
/// red warning when the zone picked is above the character.
fn refresh_pick_line(
    selection: Res<Selection>,
    profile: Option<Res<Profile>>,
    mut name: Query<(&mut Text, &mut TextColor), (With<PickName>, Without<ZoneWarn>)>,
    mut warn: Query<&mut Text, (With<ZoneWarn>, Without<PickName>)>,
    mut fill: Query<&mut Node, With<PickXpFill>>,
) {
    let profile = profile.as_deref();
    let class = selection.class_def();
    let race = selection
        .look
        .and_then(|i| selection.looks.get(i))
        .map(|&(_, r, _)| r);
    let character = selection
        .class
        .zip(profile)
        .map(|(class, p)| p.character(class));

    if let Ok((mut t, mut color)) = name.single_mut() {
        let want = match (class, race) {
            (Some(c), Some(r)) => match character {
                Some(ch) => format!("Level {} {} {}", ch.level, race_name(r), c.name),
                None => format!("{} {}", race_name(r), c.name),
            },
            _ => String::new(),
        };
        if t.0 != want {
            t.0 = want;
        }
        // In the class's own colour, as the game's raid frames and chat show it.
        let want = class.map_or(kit::TITLE, |c| rgb(c.color));
        if color.0 != want {
            color.0 = want;
        }
    }

    if let Ok(mut node) = fill.single_mut() {
        let pct = character.map_or(0.0, |ch| {
            if ch.level >= MAX_LEVEL {
                100.0
            } else {
                (ch.xp as f32 / xp_to_next(ch.level) as f32 * 100.0).clamp(0.0, 100.0)
            }
        });
        if node.width != Val::Percent(pct) {
            node.width = Val::Percent(pct);
        }
    }

    if let Ok(mut t) = warn.single_mut() {
        let want = if zone_locked(&selection, profile, selection.map) {
            format!("Requires level {}", zone_min_level(selection.map))
        } else {
            String::new()
        };
        if t.0 != want {
            t.0 = want;
        }
    }
}

/// The Enter button names the zone picked and its level range.
fn refresh_enter(
    selection: Res<Selection>,
    enter: Query<&Children, With<EnterButton>>,
    mut captions: Query<&mut Text, (With<kit::KitCaption>, Without<kit::KitSubCaption>)>,
    mut subs: Query<&mut Text, (With<kit::KitSubCaption>, Without<kit::KitCaption>)>,
) {
    let map = selection.map_def();
    let caption = format!("Enter {}", map.name);
    let (lo, hi) = zone_levels(selection.map);
    let sub = format!("{}, {}", level_range(lo, hi), map.zone);
    for children in &enter {
        for child in children.iter() {
            if let Ok(mut text) = captions.get_mut(child) {
                if text.0 != caption {
                    text.0 = caption.clone();
                }
            }
            if let Ok(mut text) = subs.get_mut(child) {
                if text.0 != sub {
                    text.0 = sub.clone();
                }
            }
        }
    }
}
