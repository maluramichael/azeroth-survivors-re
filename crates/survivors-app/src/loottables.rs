//! The loot tables: per zone, what drops there and how likely, from grunts, elites, the timed
//! bosses and the finale ([`super::items`] weights, [`super::finale::plan`]). Opened from the hub's
//! Loot Tables tab.
//!
//! A kit window ([`super::kit`]) on the pick screen, built the way [`super::talents`] builds its
//! own: a zone list on the left, and for the picked zone the boss timeline, the quality shares per
//! source and the items each quality can roll, the ones in [`Profile::collection`] in full colour.

use std::collections::BTreeSet;

use bevy::input::mouse::{MouseScrollUnit, MouseWheel};
use bevy::prelude::*;
use bevy::ui::{FocusPolicy, RelativeCursorPosition};

use super::hud::{self, MenuRoot};
use super::items::{self, Quality, Slot};
use super::kit::{self, KitButton, KitFonts, KitStyle};
use super::menu::MenuBuilt;
use super::profile::Profile;
use super::{data, finale, Phase, Selection};
use benilla_app::survivors_host::glue::GlueVisuals;

/// Whether the loot tables window is open; the hub's tab sets it.
#[derive(Resource, Default)]
pub(crate) struct LootUi {
    pub(crate) open: bool,
}

const PANEL_W: f32 = 1000.0;
const PANEL_H: f32 = 660.0;
const LIST_W: f32 = 260.0;
const ZONE_ROW_H: f32 = 46.0;
/// The source column of the drop table, one quality cell, and the icon of an item chip.
const SOURCE_W: f32 = 190.0;
const CELL_W: f32 = 68.0;
const ITEM_ICON: f32 = 16.0;
/// Pixels a wheel notch scrolls.
const WHEEL_LINE: f32 = 28.0;
/// Mirrors the reaper's hour in [`super::finale`].
const REAPER_AT_SECS: f32 = 1800.0;
const QUALITY_NAMES: [&str; 5] = ["Poor", "Common", "Uncommon", "Rare", "Epic"];

/// The dimmed full-screen layer holding the window, hidden until opened.
#[derive(Component)]
struct LootPanel;
/// On [`MenuRoot`] once the window is built into it.
#[derive(Component)]
struct LootBuilt;

/// A zone's row in the list, the index into [`data::MAPS`].
#[derive(Component, Clone, Copy)]
struct LootZoneRow(usize);

/// The right area, whose children [`fill`] rebuilds for the picked zone.
#[derive(Component)]
struct LootBody;

/// A box the mouse wheel scrolls.
#[derive(Component)]
struct LootScroll;

#[derive(Component, Clone, Copy, PartialEq, Eq, Debug)]
enum LootAction {
    Close,
}

/// The zone the window shows, and what [`fill`] last built.
#[derive(Resource, Default)]
struct LootView {
    map: usize,
    /// The zone and the number of found items the body was built for.
    shown: Option<(usize, usize)>,
    was_open: bool,
}

pub(crate) fn plugin(app: &mut App) {
    app.init_resource::<LootUi>()
        .init_resource::<LootView>()
        .add_systems(
            Update,
            (build, show_panel, clicks, fill, paint_rows, scroll)
                .chain()
                // After the keys, which read Enter off the windows' flags first.
                .after(hud::keys)
                .before(GlueVisuals),
        );
}

/// The lowest and highest creature level of `map`, as the hub lists it.
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

/// "2:15".
fn clock(secs: f32) -> String {
    let s = secs.max(0.0).round() as u32;
    format!("{}:{:02}", s / 60, s % 60)
}

/// A percentage with a decimal only where it has one: "45%", "4.5%".
fn pct_text(percent: f32) -> String {
    if (percent - percent.round()).abs() < 0.05 {
        format!("{:.0}%", percent)
    } else {
        format!("{percent:.1}%")
    }
}

/// Build the window into the pick screen once it is built.
fn build(
    mut commands: Commands,
    roots: Query<Entity, (With<MenuRoot>, With<MenuBuilt>, Without<LootBuilt>)>,
    fonts: Res<KitFonts>,
) {
    let Ok(root) = roots.single() else { return };
    commands
        .entity(root)
        .insert(LootBuilt)
        .with_children(|ui| {
            panel(ui, &fonts);
        });
}

/// The window: heading, the zone list and the zone's tables side by side, a note and Close.
fn panel(ui: &mut ChildSpawnerCommands, fonts: &KitFonts) {
    let px = Val::Px;
    ui.spawn((
        LootPanel,
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
            row_gap: px(8.0),
            ..default()
        }))
        .with_children(|win| {
            kit::heading(win, fonts, "Loot Tables", 26.0);

            win.spawn(Node {
                width: Val::Percent(100.0),
                flex_grow: 1.0,
                min_height: px(0.0),
                column_gap: px(16.0),
                ..default()
            })
            .with_children(|cols| {
                zone_list(cols, fonts);
                cols.spawn((
                    LootBody,
                    LootScroll,
                    RelativeCursorPosition::default(),
                    Node {
                        flex_grow: 1.0,
                        min_width: px(0.0),
                        min_height: px(0.0),
                        flex_direction: FlexDirection::Column,
                        row_gap: px(8.0),
                        padding: UiRect::right(px(8.0)),
                        overflow: Overflow::scroll_y(),
                        ..default()
                    },
                ));
            });

            win.spawn(Node {
                width: Val::Percent(100.0),
                justify_content: JustifyContent::SpaceBetween,
                align_items: AlignItems::Center,
                ..default()
            })
            .with_children(|foot| {
                foot.spawn(Node {
                    width: px(760.0),
                    ..default()
                })
                .with_children(|note| {
                    note.spawn(kit::body(
                        fonts,
                        "Every item rolls its quality by the source's chances; later in a run items roll stronger.",
                        12.0,
                        kit::MUTED,
                    ));
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
                    LootAction::Close,
                );
            });
        });
    });
}

/// The left column: one row per zone, lowest level first as the hub lists them.
fn zone_list(cols: &mut ChildSpawnerCommands, fonts: &KitFonts) {
    let px = Val::Px;
    let mut order: Vec<(usize, u32, u32)> = (0..data::MAPS.len())
        .map(|i| {
            let (lo, hi) = zone_levels(i);
            (i, lo, hi)
        })
        .collect();
    order.sort_by_key(|&(i, lo, _)| (lo, data::MAPS[i].name));

    cols.spawn((
        LootScroll,
        RelativeCursorPosition::default(),
        Node {
            width: px(LIST_W),
            flex_shrink: 0.0,
            min_height: px(0.0),
            flex_direction: FlexDirection::Column,
            row_gap: px(4.0),
            overflow: Overflow::scroll_y(),
            ..default()
        },
    ))
    .with_children(|list| {
        for &(i, lo, hi) in &order {
            list.spawn((
                Button,
                LootZoneRow(i),
                kit::inset(Node {
                    width: Val::Percent(100.0),
                    height: px(ZONE_ROW_H),
                    flex_shrink: 0.0,
                    flex_direction: FlexDirection::Column,
                    justify_content: JustifyContent::Center,
                    padding: UiRect::axes(px(10.0), px(0.0)),
                    row_gap: px(1.0),
                    ..default()
                }),
            ))
            .with_children(|row| {
                row.spawn(kit::title(fonts, data::MAPS[i].name, 17.0));
                row.spawn(kit::body(fonts, level_range(lo, hi), 11.0, kit::MUTED));
            });
        }
    });
}

/// Close on Esc or when the pick screen goes, and the display follows [`LootUi::open`]. Opening
/// starts on the zone picked at the hub.
fn show_panel(
    mut ui: ResMut<LootUi>,
    phase: Res<State<Phase>>,
    keys: Res<ButtonInput<KeyCode>>,
    selection: Res<Selection>,
    mut view: ResMut<LootView>,
    mut panel: Query<&mut Node, With<LootPanel>>,
) {
    if ui.open && (*phase.get() != Phase::Menu || keys.just_pressed(KeyCode::Escape)) {
        ui.open = false;
    }
    if view.was_open != ui.open {
        view.was_open = ui.open;
        if ui.open {
            view.map = selection.map;
            view.shown = None;
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

/// Close, and a click on a zone row picks that zone.
fn clicks(
    actions: Query<(&Interaction, &LootAction, &KitButton), Changed<Interaction>>,
    rows: Query<(&Interaction, &LootZoneRow), Changed<Interaction>>,
    phase: Res<State<Phase>>,
    mut ui: ResMut<LootUi>,
    mut view: ResMut<LootView>,
) {
    if *phase.get() != Phase::Menu || !ui.open {
        return;
    }
    for (interaction, action, button) in &actions {
        if *interaction != Interaction::Pressed || button.disabled {
            continue;
        }
        match action {
            LootAction::Close => ui.open = false,
        }
        return;
    }
    for (interaction, &LootZoneRow(map)) in &rows {
        if *interaction == Interaction::Pressed && view.map != map {
            view.map = map;
        }
    }
}

/// The picked zone's row gets the gold edge.
fn paint_rows(
    ui: Res<LootUi>,
    view: Res<LootView>,
    mut rows: Query<(&LootZoneRow, &mut BorderColor, &mut BackgroundColor)>,
) {
    if !ui.open {
        return;
    }
    for (row, mut edge, mut fill) in &mut rows {
        let on = row.0 == view.map;
        let want_edge = BorderColor::all(if on { kit::TITLE } else { kit::INSET_EDGE });
        if *edge != want_edge {
            *edge = want_edge;
        }
        let want_fill = if on {
            Color::srgba(0.30, 0.22, 0.08, 0.55)
        } else {
            kit::INSET_FILL
        };
        if fill.0 != want_fill {
            fill.0 = want_fill;
        }
    }
}

/// The wheel scrolls whichever scroll box the mouse is over.
fn scroll(
    ui: Res<LootUi>,
    mut wheel: MessageReader<MouseWheel>,
    mut areas: Query<(&RelativeCursorPosition, &mut ScrollPosition), With<LootScroll>>,
) {
    let mut dy = 0.0;
    for w in wheel.read() {
        dy += match w.unit {
            MouseScrollUnit::Line => w.y * WHEEL_LINE,
            MouseScrollUnit::Pixel => w.y,
        };
    }
    if !ui.open || dy == 0.0 {
        return;
    }
    for (cursor, mut pos) in &mut areas {
        if cursor.cursor_over() {
            pos.y = (pos.y - dy).max(0.0);
        }
    }
}

/// While open, build the right area for the picked zone, again when the zone or the number of
/// found items changes.
#[allow(clippy::too_many_arguments)]
fn fill(
    mut commands: Commands,
    ui: Res<LootUi>,
    mut view: ResMut<LootView>,
    fonts: Res<KitFonts>,
    profile: Option<Res<Profile>>,
    assets: Option<ResMut<benilla_assets::WorldAssets>>,
    mut images: ResMut<Assets<Image>>,
    body: Query<Entity, With<LootBody>>,
    mut scrolls: Query<&mut ScrollPosition, With<LootBody>>,
) {
    if !ui.open {
        view.shown = None;
        return;
    }
    let map = view.map.min(data::MAPS.len() - 1);
    let found_n = profile.as_deref().map_or(0, |p| p.collection.len());
    if view.shown == Some((map, found_n)) {
        return;
    }
    let Ok(body) = body.single() else { return };
    if view.shown.is_none_or(|(m, _)| m != map) {
        if let Ok(mut pos) = scrolls.get_mut(body) {
            pos.0 = Vec2::ZERO;
        }
    }
    view.shown = Some((map, found_n));

    let icons: Vec<Option<Handle<Image>>> = match assets {
        Some(mut assets) => Slot::ALL
            .iter()
            .map(|s| assets.sprite_texture(s.icon(), &mut images))
            .collect(),
        None => vec![None; Slot::ALL.len()],
    };
    let empty = BTreeSet::new();
    let found = profile.as_deref().map_or(&empty, |p| &p.collection);
    commands
        .entity(body)
        .despawn_related::<Children>()
        .with_children(|ui| {
            content(ui, &fonts, map, found, &icons);
        });
}

/// The right area for `map`: its name, the boss timeline, the drop table and the items.
fn content(
    ui: &mut ChildSpawnerCommands,
    fonts: &KitFonts,
    map: usize,
    found: &BTreeSet<String>,
    icons: &[Option<Handle<Image>>],
) {
    let px = Val::Px;
    let def = &data::MAPS[map];
    let (lo, hi) = zone_levels(map);
    ui.spawn(kit::title(fonts, def.name, 28.0));
    ui.spawn(kit::body(
        fonts,
        format!("{}, {}", def.zone, level_range(lo, hi)),
        12.0,
        kit::MUTED,
    ));

    // The bosses, on the clock.
    ui.spawn(kit::label(fonts, "Bosses"));
    let plan = finale::plan(map);
    let finale_at = finale::finale_at();
    ui.spawn(Node {
        width: Val::Percent(100.0),
        flex_wrap: FlexWrap::Wrap,
        column_gap: px(6.0),
        row_gap: px(6.0),
        ..default()
    })
    .with_children(|row| {
        for (n, boss) in plan.timed.iter().enumerate() {
            let at = (n + 1) as f32 * data::BOSS_EVERY_SECS;
            // Past the finale's slot the run is over.
            if at >= finale_at {
                break;
            }
            boss_chip(row, fonts, at, boss, false);
        }
        boss_chip(row, fonts, finale_at, &plan.finale, true);
    });
    ui.spawn(kit::body(
        fonts,
        format!("Lord Kazzak hunts the hero after {}.", clock(REAPER_AT_SECS)),
        12.0,
        kit::MUTED,
    ));

    // The quality shares per source.
    ui.spawn(kit::label(fonts, "Drop chances"));
    let sources: [(&str, String, &[f32; 5]); 4] = [
        (
            "Grunts",
            format!(
                "{} of kills drop an item",
                pct_text(items::GRUNT_CHANCE * 100.0)
            ),
            &items::WEIGHTS_GRUNT,
        ),
        (
            "Elites",
            format!(
                "{} of kills drop an item",
                pct_text(items::ELITE_CHANCE * 100.0)
            ),
            &items::WEIGHTS_ELITE,
        ),
        (
            "Bosses",
            "Every boss drops an item".to_string(),
            &items::WEIGHTS_BOSS,
        ),
        (
            "Finale",
            "Always drops an item, worn at once".to_string(),
            &items::WEIGHTS_FINALE,
        ),
    ];
    ui.spawn(kit::inset(Node {
        width: Val::Percent(100.0),
        flex_direction: FlexDirection::Column,
        padding: UiRect::axes(px(10.0), px(8.0)),
        row_gap: px(8.0),
        ..default()
    }))
    .with_children(|table| {
        // The header: the qualities in their colours.
        table
            .spawn(Node {
                width: Val::Percent(100.0),
                align_items: AlignItems::Center,
                ..default()
            })
            .with_children(|head| {
                head.spawn(Node {
                    width: px(SOURCE_W),
                    flex_shrink: 0.0,
                    ..default()
                });
                for (q, name) in Quality::ALL.iter().zip(QUALITY_NAMES) {
                    head.spawn(cell(CELL_W)).with_children(|c| {
                        c.spawn(kit::body(fonts, name, 12.0, q.color()));
                    });
                }
            });
        for (name, how, weights) in sources {
            let total: f32 = weights.iter().sum::<f32>().max(f32::EPSILON);
            table
                .spawn(Node {
                    width: Val::Percent(100.0),
                    align_items: AlignItems::Center,
                    ..default()
                })
                .with_children(|row| {
                    row.spawn(Node {
                        width: px(SOURCE_W),
                        flex_shrink: 0.0,
                        flex_direction: FlexDirection::Column,
                        ..default()
                    })
                    .with_children(|src| {
                        src.spawn(kit::body(fonts, name, 15.0, kit::CREAM));
                        src.spawn(kit::body(fonts, how, 11.0, kit::MUTED));
                    });
                    for (q, &w) in Quality::ALL.iter().zip(weights.iter()) {
                        row.spawn(cell(CELL_W)).with_children(|c| {
                            if w > 0.0 {
                                c.spawn(kit::body(fonts, pct_text(w / total * 100.0), 14.0, q.color()));
                            } else {
                                c.spawn(kit::body(fonts, "-", 14.0, kit::DISABLED));
                            }
                        });
                    }
                    // The same shares as one bar.
                    row.spawn((
                        BackgroundColor(kit::INSET_FILL),
                        Node {
                            flex_grow: 1.0,
                            min_width: px(60.0),
                            height: px(10.0),
                            margin: UiRect::left(px(8.0)),
                            overflow: Overflow::clip(),
                            border_radius: BorderRadius::all(px(2.0)),
                            ..default()
                        },
                    ))
                    .with_children(|bar| {
                        for (q, &w) in Quality::ALL.iter().zip(weights.iter()) {
                            if w <= 0.0 {
                                continue;
                            }
                            bar.spawn((
                                BackgroundColor(q.color()),
                                Node {
                                    width: Val::Percent(w / total * 100.0),
                                    height: Val::Percent(100.0),
                                    ..default()
                                },
                            ));
                        }
                    });
                });
        }
    });

    // The items each quality can roll.
    ui.spawn(kit::label(fonts, "Items"));
    for (q, name) in Quality::ALL.iter().copied().zip(QUALITY_NAMES) {
        let list: Vec<&items::Tpl> = items::table::TEMPLATES
            .iter()
            .filter(|t| t.quality == q)
            .collect();
        let got = list.iter().filter(|t| found.contains(t.name)).count();
        ui.spawn(Node {
            align_items: AlignItems::Baseline,
            column_gap: px(8.0),
            margin: UiRect::top(px(4.0)),
            ..default()
        })
        .with_children(|head| {
            head.spawn(kit::body(fonts, name, 14.0, q.color()));
            head.spawn(kit::body(
                fonts,
                format!("{got}/{} found", list.len()),
                11.0,
                kit::MUTED,
            ));
        });
        ui.spawn(Node {
            width: Val::Percent(100.0),
            flex_wrap: FlexWrap::Wrap,
            column_gap: px(4.0),
            row_gap: px(4.0),
            ..default()
        })
        .with_children(|flow| {
            for tpl in list {
                let have = found.contains(tpl.name);
                let icon = Slot::ALL
                    .iter()
                    .position(|s| *s == tpl.slot)
                    .and_then(|i| icons.get(i))
                    .and_then(|h| h.clone());
                let (text, tint) = if have {
                    (q.color(), Color::WHITE)
                } else {
                    (q.color().with_alpha(0.4), Color::srgba(1.0, 1.0, 1.0, 0.4))
                };
                flow.spawn(kit::inset(Node {
                    padding: UiRect::axes(px(6.0), px(3.0)),
                    column_gap: px(5.0),
                    align_items: AlignItems::Center,
                    ..default()
                }))
                .with_children(|chip| {
                    let square = Node {
                        width: px(ITEM_ICON),
                        height: px(ITEM_ICON),
                        ..default()
                    };
                    match icon {
                        Some(image) => {
                            chip.spawn((
                                ImageNode {
                                    image,
                                    color: tint,
                                    ..default()
                                },
                                square,
                            ));
                        }
                        None => {
                            chip.spawn(square);
                        }
                    }
                    chip.spawn(kit::body(fonts, tpl.name, 12.0, text));
                });
            }
        });
    }
}

/// One boss on the timeline: when it comes, its name and level; the finale has a gold edge.
fn boss_chip(
    row: &mut ChildSpawnerCommands,
    fonts: &KitFonts,
    at: f32,
    boss: &data::EnemyDef,
    finale: bool,
) {
    let px = Val::Px;
    let mut chip = row.spawn(kit::inset(Node {
        padding: UiRect::axes(px(8.0), px(4.0)),
        column_gap: px(8.0),
        align_items: AlignItems::Center,
        ..default()
    }));
    if finale {
        chip.insert(BorderColor::all(kit::TITLE));
    }
    chip.with_children(|c| {
        c.spawn(kit::body(fonts, clock(at), 13.0, kit::MUTED));
        c.spawn(kit::body(
            fonts,
            boss.name,
            13.0,
            if finale { kit::TITLE } else { kit::CREAM },
        ));
        c.spawn(kit::body(fonts, format!("Lv {}", boss.level), 11.0, kit::MUTED));
        if finale {
            c.spawn(kit::body(fonts, "(finale)", 11.0, kit::TITLE));
        }
    });
}

/// A table cell `w` wide with its content centred.
fn cell(w: f32) -> Node {
    Node {
        width: Val::Px(w),
        flex_shrink: 0.0,
        justify_content: JustifyContent::Center,
        ..default()
    }
}
