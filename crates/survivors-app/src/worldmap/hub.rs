//! The hub's World Map window: the continent's page with a pin per battleground, a click picks
//! the zone as the zone list does, locked ones grey, the best run and the lock in the strip below.
//!
//! The window is a dimmed layer on the pick screen like the quest log ([`crate::questlog`]), opened
//! by the World Map tab or M ([`HubMapUi::open`]). The page is built when the window opens and
//! torn down when it closes. A zone under the cursor lights up with its `<Zone>Highlight` art,
//! seated as the client's `continent_hover` seats it (`benilla-ui/src/script/worldmap.rs`).
//!
//! [`highlight_rect`] follows `continent_hover` in this codebase. The pin and zone lookups sit on
//! [`atlas`], whose projections follow matguitarist/hearthside (Apache-2.0, see its module head).

use bevy::prelude::*;
use bevy::ui::{FocusPolicy, RelativeCursorPosition};

use benilla_app::survivors_host::area::AreaTableRes;
use benilla_app::survivors_host::glue::GlueVisuals;
use benilla_assets::WorldAssets;
use benilla_formats::WorldMapArea;

use super::atlas::{self, Atlas, PAGE};
use super::HubMapUi;
use crate::hud::{self, MenuRoot};
use crate::kit::{self, KitButton, KitFonts, KitStyle};
use crate::menu::{self, MenuBuilt};
use crate::profile::Profile;
use crate::textfield::TextFocus;
use crate::{data, questlog, records, Phase, Selection};

/// Canvas units per art pixel: the page shows at 721 by 481.
pub(super) const HUB_SCALE: f32 = 0.72;
/// The page frame in canvas units.
const FRAME: Vec2 = Vec2::new(PAGE.x * HUB_SCALE, PAGE.y * HUB_SCALE);
/// The window: the page, 24 on each side, and the border.
const PANEL_W: f32 = 772.0;
/// The info strip under the page.
const STRIP_H: f32 = 104.0;
/// The width of a pin's name label.
const LABEL_W: f32 = 170.0;
/// The continent tabs: `Map.dbc` id and caption.
const CONTINENTS: [(u32, &str); 2] = [(0, "Eastern Kingdoms"), (1, "Kalimdor")];

// --- Markers. ---

/// The dimmed full-screen layer holding the window, hidden until opened.
#[derive(Component)]
struct HubPanel;
/// On [`MenuRoot`] once the window is built into it.
#[derive(Component)]
struct HubBuilt;
/// The page frame: clips the page, reads the cursor.
#[derive(Component)]
struct HubFrame;
/// The frame's child holding the page's tiles, emptied when the window closes.
#[derive(Component)]
struct HubPage;
/// The frame's child holding the pins.
#[derive(Component)]
struct HubPins;
/// The zone under the cursor (or the picked one), seated over the page.
#[derive(Component)]
struct HubHighlight;
/// A battleground's pin: its `data::MAPS` index and its centre in the frame.
#[derive(Component)]
struct HubPin {
    map: usize,
    center: Vec2,
}
/// The name above a pin, shown for the picked and the hovered one.
#[derive(Component)]
struct HubPinLabel;
/// A continent tab.
#[derive(Component)]
struct HubTab(u32);
/// The Close button.
#[derive(Component)]
struct HubClose;
/// A line of the info strip, in the order [`describe`] answers.
#[derive(Component, Clone, Copy)]
enum InfoLine {
    Name,
    Level,
    Place,
    Best,
    Lock,
    Quest,
}

/// What the window knows between frames.
#[derive(Resource, Default)]
struct HubState {
    /// The continent the page was built for; `None` while the window is closed.
    built: Option<u32>,
    /// The window was open at the last frame.
    was_open: bool,
    /// The zone (`AreaTable` id) each battleground's page shows, by `data::MAPS` index.
    zones: Vec<Option<u32>>,
}

pub(super) fn plugin(app: &mut App) {
    app.init_resource::<HubState>()
        .add_systems(OnExit(Phase::Menu), close)
        .add_systems(
            Update,
            (
                build, keys, fill, hover, clicks, show, highlight, pins_style, tabs_style, info,
            )
                .chain()
                // After the keys, which read the other windows' flags first.
                .after(hud::keys)
                .before(GlueVisuals)
                .run_if(in_state(Phase::Menu)),
        );
}

// --- The pure parts. ---

/// A pin for every battleground on continent `continent`, as `(data::MAPS index, page fraction)`.
fn pins(atlas: &Atlas, continent: u32) -> Vec<(usize, Vec2)> {
    let Some((_, page)) = atlas.continent(continent) else {
        return Vec::new();
    };
    data::MAPS
        .iter()
        .enumerate()
        .filter(|(_, def)| def.map == continent)
        .map(|(i, def)| (i, atlas::project(page, def.center[0], def.center[1])))
        .collect()
}

/// The zone (`AreaTable` id) each battleground's page shows; `None` for one that only has the
/// continent's page.
fn zone_table(atlas: &Atlas) -> Vec<Option<u32>> {
    data::MAPS
        .iter()
        .map(|def| {
            atlas
                .page_for(def, None)
                .map(|(_, page)| page.area_id)
                .filter(|&zone| zone != 0)
        })
        .collect()
}

/// The battleground of `zone` on `continent`, from [`zone_table`].
fn zone_index(zones: &[Option<u32>], continent: u32, zone: u32) -> Option<usize> {
    data::MAPS
        .iter()
        .enumerate()
        .find(|&(i, def)| def.map == continent && zones.get(i).copied().flatten() == Some(zone))
        .map(|(i, _)| i)
}

/// Where the `<Zone>Highlight` art sits over the continent page, as `continent_hover` seats it:
/// the corner and the size as page fractions, and how much of the art's height is picture
/// (`texPctY`: the file is padded to a power of two).
fn highlight_rect(continent: &WorldMapArea, zone: &WorldMapArea) -> (Vec2, Vec2, f32) {
    let (cont_w, cont_h) = (
        continent.loc_left - continent.loc_right,
        continent.loc_top - continent.loc_bottom,
    );
    let (w, h) = (
        zone.loc_left - zone.loc_right,
        zone.loc_top - zone.loc_bottom,
    );
    if cont_w == 0.0 || cont_h == 0.0 || w == 0.0 {
        return (Vec2::ZERO, Vec2::ZERO, 1.0);
    }
    let corner = Vec2::new(
        (continent.loc_left - zone.loc_left) / cont_w,
        (continent.loc_top - zone.loc_top) / cont_h,
    );
    let size = Vec2::new(w / cont_w, h / cont_h);
    // The art is 128 wide; the picture is as tall as the zone's aspect makes it.
    let dim = ((h * 128.0) / w) as i64;
    let mut pot = 1i64;
    while pot < dim {
        pot <<= 1;
    }
    let pct = if dim > 0 {
        dim as f32 / pot as f32
    } else {
        1.0
    };
    (corner, size, pct)
}

/// A pin's diameter, fill and edge: gold, grey when locked; the picked one larger with a light
/// edge, the one under the cursor with a warmer edge.
fn pin_look(selected: bool, hot: bool, locked: bool) -> (f32, Color, Color) {
    let diameter = if selected { 22.0 } else { 16.0 };
    let fill = if locked { kit::DISABLED } else { kit::TITLE };
    let edge = if selected {
        Color::srgb(1.0, 0.96, 0.82)
    } else if hot {
        Color::srgb(0.85, 0.68, 0.36)
    } else {
        Color::srgb(0.10, 0.07, 0.03)
    };
    (diameter, fill, edge)
}

/// Where a pin's name label starts, from the pin's own left edge, so that the label stays inside
/// the frame: centred over the pin where it can be.
fn label_left(pin_x: f32, pin_d: f32, frame_w: f32, label_w: f32) -> f32 {
    let want = pin_x - label_w * 0.5;
    let left = want.clamp(2.0, (frame_w - label_w - 2.0).max(2.0));
    left - (pin_x - pin_d * 0.5)
}

/// "Level 12" or "Level 12-20".
fn level_range(lo: u32, hi: u32) -> String {
    if lo == hi {
        format!("Level {lo}")
    } else {
        format!("Level {lo}-{hi}")
    }
}

/// The second line of the strip: the zone the name stands in, and the continent when the zone is
/// not the continent itself.
fn place_line(zone: &str, continent: &str) -> String {
    if zone == continent {
        zone.to_string()
    } else {
        format!("{zone} \u{b7} {continent}")
    }
}

fn continent_name(map: u32) -> &'static str {
    CONTINENTS
        .iter()
        .find(|(id, _)| *id == map)
        .map_or("Azeroth", |(_, name)| name)
}

/// The strip's lines for battleground `i`, in the order of [`InfoLine`].
fn describe(i: usize, selection: &Selection, profile: Option<&Profile>) -> [(String, Color); 6] {
    let def = &data::MAPS[i.min(data::MAPS.len() - 1)];
    let lock = menu::zone_lock(selection, profile, i);
    let levels = data::roster(i).enemies.iter().map(|e| e.level);
    let lo = levels.clone().min().unwrap_or(1);
    let hi = levels.max().unwrap_or(lo);
    let best = selection
        .class_def()
        .map(|c| c.id)
        .zip(profile)
        .and_then(|(id, p)| p.records.get(&records::key(i, id)));
    let note = records::note(best);
    let (best_text, best_color) = if note.is_empty() {
        ("No run here yet".to_string(), kit::MUTED)
    } else if best.is_some_and(|b| b.wins > 0) {
        (note, kit::TITLE)
    } else {
        (note, kit::MUTED)
    };
    let (quest_text, quest_color) = match questlog::zone_quest(i) {
        Some(q) if profile.is_some_and(|p| q.done(p)) => {
            (format!("Quest: {} (done)", q.title), kit::BONUS)
        }
        Some(q) => (format!("Quest: {} (to do)", q.title), kit::MUTED),
        None => (String::new(), kit::MUTED),
    };
    [
        (
            def.name.to_string(),
            if lock.is_some() {
                kit::DISABLED
            } else {
                kit::TITLE
            },
        ),
        (level_range(lo, hi), kit::MUTED),
        (place_line(def.zone, continent_name(def.map)), kit::MUTED),
        (best_text, best_color),
        (lock.map(|l| l.long()).unwrap_or_default(), kit::WARN),
        (quest_text, quest_color),
    ]
}

// --- The systems. ---

/// Build the window into the pick screen once it is built.
fn build(
    mut commands: Commands,
    roots: Query<Entity, (With<MenuRoot>, With<MenuBuilt>, Without<HubBuilt>)>,
    fonts: Res<KitFonts>,
) {
    let Ok(root) = roots.single() else { return };
    commands.entity(root).insert(HubBuilt).with_children(|ui| {
        window(ui, &fonts);
    });
}

fn px(v: f32) -> Val {
    Val::Px(v)
}

fn fill_parent() -> Node {
    Node {
        position_type: PositionType::Absolute,
        width: Val::Percent(100.0),
        height: Val::Percent(100.0),
        ..default()
    }
}

fn nowrap() -> TextLayout {
    TextLayout::new(Justify::Left, LineBreak::NoWrap)
}

/// The frame: heading, the continent tabs, the page, and the strip with Close.
fn window(ui: &mut ChildSpawnerCommands, fonts: &KitFonts) {
    ui.spawn((
        HubPanel,
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
            flex_direction: FlexDirection::Column,
            align_items: AlignItems::Center,
            padding: UiRect::axes(px(24.0), px(12.0)),
            row_gap: px(6.0),
            ..default()
        }))
        .with_children(|win| {
            kit::heading(win, fonts, "World Map", 26.0);
            win.spawn(Node {
                column_gap: px(8.0),
                ..default()
            })
            .with_children(|tabs| {
                for (id, caption) in CONTINENTS {
                    kit::button(
                        tabs,
                        fonts,
                        KitStyle::Tab,
                        caption,
                        None,
                        Node {
                            min_width: px(150.0),
                            height: px(28.0),
                            ..default()
                        },
                        HubTab(id),
                    );
                }
            });
            // The page: tiles, the highlight over them, the pins on top.
            win.spawn((
                HubFrame,
                Interaction::default(),
                RelativeCursorPosition::default(),
                BackgroundColor(Color::srgb(0.06, 0.05, 0.04)),
                Outline::new(px(1.0), px(0.0), kit::INSET_EDGE),
                Node {
                    width: px(FRAME.x),
                    height: px(FRAME.y),
                    flex_shrink: 0.0,
                    overflow: Overflow::clip(),
                    ..default()
                },
            ))
            .with_children(|frame| {
                frame.spawn((HubPage, fill_parent()));
                frame.spawn((
                    HubHighlight,
                    ImageNode::default(),
                    BackgroundColor(Color::NONE),
                    BorderColor::all(Color::NONE),
                    Node {
                        position_type: PositionType::Absolute,
                        display: Display::None,
                        ..default()
                    },
                ));
                frame.spawn((HubPins, fill_parent()));
            });
            win.spawn(kit::inset(Node {
                width: Val::Percent(100.0),
                height: px(STRIP_H),
                flex_shrink: 0.0,
                padding: UiRect::axes(px(12.0), px(6.0)),
                justify_content: JustifyContent::SpaceBetween,
                ..default()
            }))
            .with_children(|strip| {
                strip
                    .spawn(Node {
                        flex_direction: FlexDirection::Column,
                        flex_grow: 1.0,
                        row_gap: px(2.0),
                        ..default()
                    })
                    .with_children(|col| {
                        col.spawn(Node {
                            align_items: AlignItems::FlexEnd,
                            column_gap: px(12.0),
                            ..default()
                        })
                        .with_children(|row| {
                            row.spawn((InfoLine::Name, kit::title(fonts, "", 20.0), nowrap()));
                            row.spawn((
                                InfoLine::Level,
                                kit::body(fonts, "", 14.0, kit::MUTED),
                                nowrap(),
                            ));
                        });
                        for line in [
                            InfoLine::Place,
                            InfoLine::Best,
                            InfoLine::Lock,
                            InfoLine::Quest,
                        ] {
                            col.spawn((line, kit::body(fonts, "", 13.0, kit::MUTED), nowrap()));
                        }
                    });
                strip
                    .spawn(Node {
                        flex_direction: FlexDirection::Column,
                        justify_content: JustifyContent::SpaceBetween,
                        align_items: AlignItems::FlexEnd,
                        ..default()
                    })
                    .with_children(|right| {
                        right.spawn((
                            kit::body(
                                fonts,
                                "Click a zone to pick it\nM or Esc to close",
                                11.0,
                                kit::MUTED,
                            ),
                            TextLayout::new(Justify::Right, LineBreak::NoWrap),
                        ));
                        kit::button(
                            right,
                            fonts,
                            KitStyle::Normal,
                            "Close",
                            None,
                            Node {
                                width: px(110.0),
                                height: px(28.0),
                                ..default()
                            },
                            HubClose,
                        );
                    });
            });
        });
    });
}

/// Whether another hub window is open over the pick screen.
type Others<'w> = (
    Res<'w, crate::trainer::TrainerUi>,
    Res<'w, crate::options::OptionsUi>,
    Res<'w, crate::talents::TalentsUi>,
    Res<'w, crate::collection::CollectionUi>,
    Res<'w, crate::loottables::LootUi>,
    Res<'w, crate::achievements::AchievementsUi>,
    Res<'w, questlog::QuestLogUi>,
    Option<Res<'w, crate::CoopUi>>,
);

/// M opens and closes the map, Esc closes it. Neither while a text field has the keyboard, and M
/// does not open it from behind another window.
fn keys(
    keys: Res<ButtonInput<KeyCode>>,
    focus: Res<TextFocus>,
    mut ui: ResMut<HubMapUi>,
    others: Others,
) {
    if focus.0.is_some() {
        return;
    }
    if ui.open && keys.just_pressed(KeyCode::Escape) {
        ui.open = false;
        return;
    }
    if !keys.just_pressed(KeyCode::KeyM) {
        return;
    }
    if ui.open {
        ui.open = false;
        return;
    }
    let blocked = others.0.open
        || others.1.open
        || others.2.open
        || others.3.open
        || others.4.open
        || others.5.open
        || others.6.open
        || others.7.as_ref().is_some_and(|c| c.open);
    if !blocked {
        ui.open = true;
    }
}

/// The page: built when the window opens, emptied when it closes, rebuilt when the continent
/// changes. A fresh opening follows the picked zone's continent.
#[allow(clippy::too_many_arguments)]
fn fill(
    mut commands: Commands,
    mut ui: ResMut<HubMapUi>,
    mut state: ResMut<HubState>,
    selection: Res<Selection>,
    mut atlas: ResMut<Atlas>,
    assets: Option<ResMut<WorldAssets>>,
    areas: Option<Res<AreaTableRes>>,
    mut images: ResMut<Assets<Image>>,
    fonts: Res<KitFonts>,
    page: Query<Entity, With<HubPage>>,
    pin_box: Query<Entity, With<HubPins>>,
) {
    if !ui.open {
        if state.was_open || state.built.is_some() {
            for e in page.iter().chain(pin_box.iter()) {
                commands.entity(e).despawn_related::<Children>();
            }
            state.built = None;
            state.was_open = false;
        }
        return;
    }
    if !state.was_open {
        state.was_open = true;
        if ui.continent.is_some() {
            ui.continent = None;
        }
    }
    let want = ui.continent.unwrap_or(selection.map_def().map);
    if state.built == Some(want) {
        return;
    }
    let (Some(mut assets), Ok(page), Ok(pin_box)) = (assets, page.single(), pin_box.single())
    else {
        return;
    };
    atlas.ensure(&mut assets, areas.as_deref());
    state.zones = zone_table(&atlas);
    state.built = Some(want);

    commands
        .entity(page)
        .despawn_related::<Children>()
        .with_children(|p| match atlas.continent(want) {
            Some((_, continent)) => {
                // Continents have no overlays.
                atlas::spawn_page(p, &mut assets, &mut images, continent, &[], HUB_SCALE);
            }
            None => {
                p.spawn((
                    kit::body(&fonts, "No map for this continent", 16.0, kit::MUTED),
                    atlas::at(Vec2::new(20.0, 20.0), 300.0, 24.0),
                ));
            }
        });
    commands
        .entity(pin_box)
        .despawn_related::<Children>()
        .with_children(|p| {
            for (i, uv) in pins(&atlas, want) {
                let center = uv * FRAME;
                let (d, fill, edge) = pin_look(false, false, false);
                p.spawn((
                    Button,
                    HubPin { map: i, center },
                    Node {
                        border: UiRect::all(px(2.0)),
                        ..atlas::round(center - Vec2::splat(d * 0.5), d, d)
                    },
                    BackgroundColor(fill),
                    BorderColor::all(edge),
                    ZIndex(0),
                ))
                .with_children(|pin| {
                    pin.spawn((
                        HubPinLabel,
                        Node {
                            position_type: PositionType::Absolute,
                            display: Display::None,
                            top: px(-16.0),
                            width: px(LABEL_W),
                            justify_content: JustifyContent::Center,
                            ..default()
                        },
                    ))
                    .with_children(|label| {
                        label.spawn((
                            kit::body(&fonts, data::MAPS[i].name, 11.0, kit::CREAM),
                            TextLayout::new(Justify::Center, LineBreak::NoWrap),
                        ));
                    });
                });
            }
        });
}

/// The zone under the cursor into [`HubMapUi::hover`]: a pin first, else the zone the continent's
/// `.zmp` names there, if one of the battlegrounds lies in it.
fn hover(
    mut ui: ResMut<HubMapUi>,
    state: Res<HubState>,
    atlas: Res<Atlas>,
    frame: Query<(&RelativeCursorPosition, &Interaction), With<HubFrame>>,
    pins: Query<(&HubPin, &Interaction)>,
) {
    if !ui.open {
        if ui.hover.is_some() {
            ui.hover = None;
        }
        return;
    }
    let Some(continent) = state.built else { return };
    let mut want = pins
        .iter()
        .find(|(_, interaction)| **interaction != Interaction::None)
        .map(|(pin, _)| pin.map);
    if want.is_none() {
        // The frame is only hot where no pin blocks it.
        want = frame.single().ok().and_then(|(cursor, interaction)| {
            if !cursor.cursor_over() || *interaction == Interaction::None {
                return None;
            }
            // Bevy's `normalized` is zero at the centre, so the page fraction is `+ 0.5`.
            let uv = cursor.normalized? + Vec2::splat(0.5);
            let (_, page) = atlas.continent(continent)?;
            let (wx, wy) = atlas::unproject(page, uv);
            let zone = atlas.zone_at(continent, wx, wy)?;
            zone_index(&state.zones, continent, zone)
        });
    }
    if ui.hover != want {
        ui.hover = want;
    }
}

/// Clicks: a pin or a zone picks the battleground as the list does (a locked one too: Enter World
/// stays off), a tab changes the continent, Close closes.
#[allow(clippy::too_many_arguments)]
fn clicks(
    mut ui: ResMut<HubMapUi>,
    mut selection: ResMut<Selection>,
    pins: Query<(&Interaction, &HubPin), Changed<Interaction>>,
    frame: Query<&Interaction, (With<HubFrame>, Changed<Interaction>)>,
    tabs: Query<(&Interaction, &HubTab), Changed<Interaction>>,
    close: Query<&Interaction, (With<HubClose>, Changed<Interaction>)>,
    buttons: Query<&KitButton, With<HubClose>>,
) {
    if !ui.open {
        return;
    }
    let mut pick = pins
        .iter()
        .find(|(interaction, _)| **interaction == Interaction::Pressed)
        .map(|(_, pin)| pin.map);
    if pick.is_none() && frame.iter().any(|i| *i == Interaction::Pressed) {
        pick = ui.hover;
    }
    if let Some(i) = pick {
        if selection.map != i {
            menu::pick_map(&mut selection, i);
        }
    }
    if let Some((_, tab)) = tabs
        .iter()
        .find(|(interaction, _)| **interaction == Interaction::Pressed)
    {
        if ui.continent != Some(tab.0) {
            ui.continent = Some(tab.0);
            ui.hover = None;
        }
    }
    let pressed =
        close.iter().any(|i| *i == Interaction::Pressed) && buttons.iter().all(|b| !b.disabled);
    if pressed {
        ui.open = false;
    }
}

/// The layer follows [`HubMapUi::open`].
fn show(ui: Res<HubMapUi>, mut panel: Query<&mut Node, With<HubPanel>>) {
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

/// Seat the highlight over the hovered zone, else the picked one. Without the art the zone's
/// rectangle is outlined in gold.
#[allow(clippy::too_many_arguments)]
fn highlight(
    ui: Res<HubMapUi>,
    selection: Res<Selection>,
    state: Res<HubState>,
    atlas: Res<Atlas>,
    assets: Option<ResMut<WorldAssets>>,
    mut images: ResMut<Assets<Image>>,
    mut marks: Query<
        (
            &mut Node,
            &mut ImageNode,
            &mut BackgroundColor,
            &mut BorderColor,
        ),
        With<HubHighlight>,
    >,
    mut last: Local<Option<(Option<usize>, u32)>>,
    mut glows: Local<std::collections::HashMap<String, Option<Handle<Image>>>>,
) {
    if !ui.open {
        *last = None;
        return;
    }
    let Some(continent) = state.built else { return };
    let want = ui
        .hover
        .or(Some(selection.map))
        .filter(|&i| data::MAPS.get(i).is_some_and(|d| d.map == continent));
    if *last == Some((want, continent)) {
        return;
    }
    let (Ok((mut node, mut image, mut fill, mut edge)), Some(mut assets)) =
        (marks.single_mut(), assets)
    else {
        return;
    };
    *last = Some((want, continent));
    let zone_page = want
        .and_then(|i| state.zones.get(i).copied().flatten())
        .and_then(|zone| {
            atlas
                .rows()
                .find(|(_, a)| a.map_id == continent && a.area_id == zone)
                .map(|(_, a)| a)
        });
    let Some(((_, continent_page), zone_page)) = atlas.continent(continent).zip(zone_page) else {
        node.display = Display::None;
        return;
    };
    let (corner, size, pct) = highlight_rect(continent_page, zone_page);
    let path = format!("Interface\\WorldMap\\{0}\\{0}Highlight", zone_page.name);
    let glow = glows
        .entry(path.clone())
        .or_insert_with(|| {
            let source = assets.sprite_texture(&path, &mut images)?;
            let glow = additive_as_alpha(images.get(&source)?)?;
            Some(images.add(glow))
        })
        .clone();
    let art = glow.and_then(|h| images.get(&h).map(|i| (h, i.size_f32())));
    let mut placed = atlas::at(corner * FRAME, size.x * FRAME.x, size.y * FRAME.y);
    match art {
        Some((handle, px)) => {
            *image = ImageNode {
                rect: Some(Rect::new(0.0, 0.0, px.x, px.y * pct)),
                color: kit::TITLE.with_alpha(0.8),
                ..ImageNode::new(handle)
            };
            fill.0 = Color::NONE;
            *edge = BorderColor::all(Color::NONE);
        }
        None => {
            *image = ImageNode::default();
            fill.0 = kit::TITLE.with_alpha(0.16);
            placed.border = UiRect::all(Val::Px(2.0));
            *edge = BorderColor::all(kit::TITLE);
        }
    }
    *node = placed;
}

/// The client draws a `<Zone>Highlight` additively: black is nothing, bright is light. The UI
/// only blends by alpha, so the art becomes white with its brightness as the alpha, and the node
/// tints it. `None` for a format other than plain 8-bit RGBA.
fn additive_as_alpha(source: &Image) -> Option<Image> {
    use bevy::render::render_resource::TextureFormat;
    if !matches!(
        source.texture_descriptor.format,
        TextureFormat::Rgba8Unorm | TextureFormat::Rgba8UnormSrgb
    ) {
        return None;
    }
    let mut out = source.clone();
    let data = out.data.as_mut()?;
    for px in data.as_chunks_mut::<4>().0 {
        let light = px[0].max(px[1]).max(px[2]);
        let a = (u16::from(light) * u16::from(px[3]) / 255) as u8;
        *px = [255, 255, 255, a];
    }
    Some(out)
}

/// Each pin's size, colours, stacking and name label from the pick, the cursor and the locks.
fn pins_style(
    ui: Res<HubMapUi>,
    selection: Res<Selection>,
    profile: Option<Res<Profile>>,
    mut pins: Query<(
        &HubPin,
        &Interaction,
        &Children,
        &mut Node,
        &mut BackgroundColor,
        &mut BorderColor,
        &mut ZIndex,
    )>,
    mut labels: Query<&mut Node, (With<HubPinLabel>, Without<HubPin>)>,
) {
    if !ui.open {
        return;
    }
    let profile = profile.as_deref();
    for (pin, interaction, children, mut node, mut fill, mut edge, mut z) in &mut pins {
        let selected = selection.map == pin.map;
        let hot = *interaction != Interaction::None || ui.hover == Some(pin.map);
        let locked = menu::zone_locked(&selection, profile, pin.map);
        let (d, color, rim) = pin_look(selected, hot, locked);
        let at = pin.center - Vec2::splat(d * 0.5);
        if node.width != Val::Px(d) || node.left != Val::Px(at.x) || node.top != Val::Px(at.y) {
            node.width = Val::Px(d);
            node.height = Val::Px(d);
            node.left = Val::Px(at.x);
            node.top = Val::Px(at.y);
        }
        if fill.0 != color {
            fill.0 = color;
        }
        if edge.top != rim {
            *edge = BorderColor::all(rim);
        }
        let want_z = if selected {
            2
        } else if hot {
            1
        } else {
            0
        };
        if z.0 != want_z {
            z.0 = want_z;
        }
        let named = selected || hot;
        for child in children.iter() {
            let Ok(mut label) = labels.get_mut(child) else {
                continue;
            };
            let display = if named { Display::Flex } else { Display::None };
            if label.display != display {
                label.display = display;
            }
            let left = Val::Px(label_left(pin.center.x, d, FRAME.x, LABEL_W));
            if label.left != left {
                label.left = left;
            }
        }
    }
}

/// The tab of the shown continent is lit.
fn tabs_style(state: Res<HubState>, mut tabs: Query<(&HubTab, &mut KitButton)>) {
    for (tab, mut button) in &mut tabs {
        let on = state.built == Some(tab.0);
        if button.selected != on {
            button.selected = on;
        }
    }
}

/// The strip: the zone under the cursor, else the picked one.
fn info(
    ui: Res<HubMapUi>,
    selection: Res<Selection>,
    profile: Option<Res<Profile>>,
    mut lines: Query<(&InfoLine, &mut Text, &mut TextColor)>,
) {
    if !ui.open {
        return;
    }
    let i = ui.hover.unwrap_or(selection.map);
    let rows = describe(i, &selection, profile.as_deref());
    for (line, mut text, mut color) in &mut lines {
        let (want, tint) = &rows[*line as usize];
        if text.0 != *want {
            text.0.clone_from(want);
        }
        if color.0 != *tint {
            color.0 = *tint;
        }
    }
}

/// Leaving the pick screen closes the window.
fn close(mut ui: ResMut<HubMapUi>, mut state: ResMut<HubState>) {
    ui.open = false;
    ui.hover = None;
    state.built = None;
    state.was_open = false;
}

#[cfg(test)]
mod tests {
    use super::*;

    fn row(left: f32, right: f32, top: f32, bottom: f32) -> WorldMapArea {
        WorldMapArea {
            map_id: 0,
            area_id: 12,
            name: "Elwynn".into(),
            loc_left: left,
            loc_right: right,
            loc_top: top,
            loc_bottom: bottom,
        }
    }

    fn near(a: Vec2, b: Vec2) -> bool {
        (a - b).length() < 1e-5
    }

    #[test]
    fn the_highlight_covers_the_zone_rect() {
        let continent = row(1000.0, 0.0, 1000.0, 0.0);
        // The north-west quarter: square, so the whole art is picture.
        let nw = row(1000.0, 500.0, 1000.0, 500.0);
        let (corner, size, pct) = highlight_rect(&continent, &nw);
        assert!(near(corner, Vec2::ZERO), "{corner}");
        assert!(near(size, Vec2::splat(0.5)), "{size}");
        assert_eq!(pct, 1.0);
        // Wider than tall: 200 by 150 makes a 128 by 96 picture in a 128 by 128 file.
        let inner = row(600.0, 400.0, 500.0, 350.0);
        let (corner, size, pct) = highlight_rect(&continent, &inner);
        assert!(near(corner, Vec2::new(0.4, 0.5)), "{corner}");
        assert!(near(size, Vec2::new(0.2, 0.15)), "{size}");
        assert!((pct - 0.75).abs() < 1e-6, "{pct}");
        // A degenerate row never divides by zero.
        let flat = row(10.0, 10.0, 5.0, 0.0);
        assert_eq!(highlight_rect(&continent, &flat).1, Vec2::ZERO);
    }

    #[test]
    fn a_pin_is_gold_or_grey_and_the_pick_is_larger() {
        let (d_on, fill_on, edge_on) = pin_look(true, false, false);
        let (d_off, fill_off, edge_off) = pin_look(false, false, false);
        assert!(d_on > d_off);
        assert_eq!(fill_on, kit::TITLE);
        assert_ne!(edge_on, edge_off);
        assert_ne!(fill_off, kit::DISABLED, "an open zone is not greyed");
        assert_eq!(pin_look(false, false, true).1, kit::DISABLED);
        // A locked pick is still the larger one.
        assert_eq!(pin_look(true, false, true).0, d_on);
        assert_ne!(pin_look(false, true, false).2, edge_off);
    }

    #[test]
    fn the_label_stays_inside_the_frame() {
        // Over the middle the label is centred on the pin.
        let mid = label_left(360.0, 16.0, 721.0, 170.0);
        assert!((mid - (-(170.0 - 16.0) * 0.5)).abs() < 1e-4, "{mid}");
        // At the left edge it starts at the frame's edge, at the right it ends there.
        let left = label_left(4.0, 16.0, 721.0, 170.0);
        assert!((left - (2.0 - (4.0 - 8.0))).abs() < 1e-4, "{left}");
        let right = label_left(719.0, 16.0, 721.0, 170.0);
        let page_x = right + (719.0 - 8.0);
        assert!((page_x + 170.0 - (721.0 - 2.0)).abs() < 1e-3, "{page_x}");
    }

    #[test]
    fn the_strip_words_are_short_and_plain() {
        assert_eq!(level_range(5, 5), "Level 5");
        assert_eq!(level_range(5, 12), "Level 5-12");
        assert_eq!(place_line("Kalimdor", "Kalimdor"), "Kalimdor");
        assert_eq!(
            place_line("Un'Goro Crater", "Kalimdor"),
            "Un'Goro Crater \u{b7} Kalimdor"
        );
        assert_eq!(continent_name(0), "Eastern Kingdoms");
        assert_eq!(continent_name(1), "Kalimdor");
        assert!(![place_line("a", "b"), level_range(1, 2)]
            .iter()
            .any(|s| s.contains('\u{2014}')));
    }

    #[test]
    fn a_zone_names_its_battleground_on_its_own_continent() {
        let elwynn = data::MAPS
            .iter()
            .position(|d| d.name == "Elwynn Forest")
            .expect("Elwynn Forest is a battleground");
        let mut zones = vec![None; data::MAPS.len()];
        zones[elwynn] = Some(12);
        assert_eq!(zone_index(&zones, data::MAPS[elwynn].map, 12), Some(elwynn));
        // The same zone id on the other continent names nothing.
        assert_eq!(zone_index(&zones, 1, 12), None);
        assert_eq!(zone_index(&zones, 0, 99), None);
    }

    #[test]
    fn every_battleground_is_a_pin_on_its_continent() {
        let data = benilla_formats::wow_data_or_skip!();
        let mut chain = benilla_formats::open_chain(&data).expect("open chain");
        let areas = benilla_formats::load_area_table_catalog(&mut chain).ok();
        let mut atlas = Atlas::default();
        atlas.load(&mut chain, areas.as_ref());
        let mut listed = 0;
        for continent in [0, 1] {
            for (i, uv) in pins(&atlas, continent) {
                assert!(
                    uv.x > 0.0 && uv.x < 1.0 && uv.y > 0.0 && uv.y < 1.0,
                    "{}: pin off its continent page ({uv})",
                    data::MAPS[i].name
                );
                listed += 1;
            }
        }
        assert_eq!(listed, data::MAPS.len(), "a battleground lacks a pin");
        // Two battlegrounds in one zone would make the zone hover name only the first.
        let zones = zone_table(&atlas);
        for (i, def) in data::MAPS.iter().enumerate() {
            let grid = atlas.zone_at(def.map, def.center[0], def.center[1]);
            println!(
                "{:<24} page zone {:?}, grid zone {grid:?}",
                def.name, zones[i]
            );
            if let Some(zone) = zones[i] {
                assert_eq!(
                    zone_index(&zones, def.map, zone),
                    Some(i),
                    "{}: another battleground shares zone {zone}",
                    def.name
                );
            }
        }
    }
}
