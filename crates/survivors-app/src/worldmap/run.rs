//! The map window in a run: M opens it on the zone's page with the battleground a circle on it,
//! alone over a paused run, in co-op over a running one; Z, the wheel and the tabs switch to the
//! battlefield up close ([`super::field`]).
//!
//! Keys: **M** opens and closes, **Esc** closes, **Z** flips between the two views, the wheel
//! zooms in (up) and out (down), a right click on the map goes one view out and a left click on
//! the battleground's circle goes in. The first opening shows the zone; later ones the view last
//! used ([`WorldMapUi::level`]).
//!
//! The window is built once per run into its own root, hidden until M. This file owns the frame
//! both views share (title, tabs, hint, legend) and the zone view; the battlefield view fills the
//! [`FieldSlot`] the frame leaves it. The zone page is built on opening and torn down on closing
//! or on leaving the view; only the arrow, the skulls and the party dots move afterwards, at
//! 10 Hz on the wall clock so a co-op window stays live.
//!
//! The arena circle, the arrow's turn and the M and Esc handling follow matguitarist/hearthside
//! (`crates/benilla-app/src/survivors/worldmap.rs` at `7b689f96e84b6bc7ee1778b895de1d27c86735e8`,
//! Apache-2.0).

use bevy::input::mouse::{MouseScrollUnit, MouseWheel};
use bevy::prelude::*;
use bevy::ui::{FocusPolicy, RelativeCursorPosition};

use benilla_app::survivors_host::area::AreaTableRes;
use benilla_app::survivors_host::loading_screen::LoadingScreen;
use benilla_app::survivors_host::sound::GlueSound;
use benilla_assets::coords::bevy_to_wow;
use benilla_assets::WorldAssets;
use benilla_formats::WorldMapArea;
use benilla_world::world_point::WorldPoint;

use super::atlas::{
    self, at, cell_image, project, round, spawn_page, Atlas, ARROW_TEXTURE, OBJECT_ICONS, PAGE,
    PARTY_ICON, RAID_ICONS, SKULL_TEXTURE,
};
use super::{FieldSlot, MapLevel, MapPin, WorldMapUi, ZoneSlot, BARRIER_TINT};
use crate::enemies::Enemy;
use crate::hero::Hero;
use crate::kit::{self, KitButton, KitFonts, KitStyle};
use crate::minimap::MapMarker;
use crate::textfield::TextFocus;
use crate::{hud, NetRole, Phase, RunEntity, Selection};

/// Over the HUD (400), the character window (405) and the level-up cards (420), under the pause
/// screen (430): the pause menu never shows over the map anyway.
const Z: i32 = 425;
/// The page on the 768-unit canvas: 852 by 568.
pub(super) const PAGE_SCALE: f32 = 0.85;
/// Skulls the zone view can show at once, bosses first, then rares.
const SKULLS: usize = 12;
/// Party dots the zone view can show at once.
const ALLIES: usize = 4;
const ARROW_PX: f32 = 24.0;
const SKULL_PX: f32 = 14.0;
const ALLY_PX: f32 = 12.0;
/// The moving marks are placed this often (seconds, wall clock).
const TICK: f32 = 0.1;
/// The panel's height over the map frame: the heading, the title row, the hint, the legend, the
/// gaps and the padding.
const PANEL_EXTRA: f32 = 132.0;
/// The battleground's circle is never smaller than this across: the small arenas would vanish on
/// a page that spans thousands of yards.
const CIRCLE_MIN: f32 = 10.0;
const CIRCLE_RESTING: f32 = 0.10;
const CIRCLE_HOT: f32 = 0.28;
const HINT: &str = "M or Esc to close \u{b7} Z, the wheel or right-click to change the view";

/// Whether the map stops the run: alone the clock stands still while it is open, as with the pause
/// menu; in a co-op party nothing pauses and the map shows the live world.
pub(super) fn pauses(role: NetRole) -> bool {
    role == NetRole::Solo
}

pub(super) fn plugin(app: &mut App) {
    app.init_resource::<RunMap>()
        .add_systems(
            Update,
            (
                build_window,
                map_keys,
                fill_zone,
                refresh_zone,
                show_level,
                circle_hover,
            )
                .chain()
                // After the panels: closing resumes the run, and the pause menu must not get a
                // frame between the map going and the phase coming back.
                .after(hud::show_panels),
        )
        .add_systems(OnEnter(Phase::Menu), teardown)
        .add_systems(OnEnter(Phase::GameOver), teardown);
}

/// The window of this run and what is built into it.
#[derive(Resource, Default)]
struct RunMap {
    root: Option<Entity>,
    zone_slot: Option<Entity>,
    field_slot: Option<Entity>,
    legend: Option<Entity>,
    /// Whether the zone view is built (also when there is no page for it).
    zone_built: bool,
    zone: Option<ZoneView>,
    /// The wall-clock second the moving marks are placed next.
    next_at: f32,
}

/// The zone view's page and the marks that move on it.
struct ZoneView {
    page: WorldMapArea,
    arrow: Entity,
    skulls: Vec<Entity>,
    allies: Vec<Entity>,
}

/// The window's root, a full-screen layer that centres the panel.
#[derive(Component)]
struct MapRoot;
/// The clipped box the views live in.
#[derive(Component)]
struct MapFrame;
/// The line under the heading: the page, or the zone on the battlefield view.
#[derive(Component)]
struct ZoneTitle;
/// The legend, shown on the battlefield view.
#[derive(Component)]
struct LegendRow;
/// A view's tab.
#[derive(Component)]
struct LevelTab(MapLevel);
/// The battleground's circle on the zone page: a click goes in.
#[derive(Component)]
struct ArenaCircle;

// --- What a key does. ---

/// The map state the keys act on.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
struct View {
    open: bool,
    level: MapLevel,
    /// The map paused the run and resumes it on closing.
    paused: bool,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum MapKey {
    /// M.
    Toggle,
    Esc,
    /// Z.
    Flip,
    /// The wheel up, or a click on the battleground's circle.
    In,
    /// The wheel down, or a right click on the map.
    Out,
    Tab(MapLevel),
}

/// What the run's phase does along with it.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Step {
    Stay,
    Pause,
    Resume,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
struct Transition {
    view: View,
    step: Step,
}

/// What `key` does to the map: the new state and whether the run is paused or resumed with it.
/// Only M does anything while the map is closed.
fn on_key(view: View, key: MapKey, role: NetRole) -> Transition {
    let stay = Transition {
        view,
        step: Step::Stay,
    };
    if !view.open {
        if key != MapKey::Toggle {
            return stay;
        }
        let pause = pauses(role);
        return Transition {
            view: View {
                open: true,
                paused: pause,
                ..view
            },
            step: if pause { Step::Pause } else { Step::Stay },
        };
    }
    let level = |level| Transition {
        view: View { level, ..view },
        step: Step::Stay,
    };
    match key {
        MapKey::Toggle | MapKey::Esc => Transition {
            view: View {
                open: false,
                paused: false,
                ..view
            },
            step: if view.paused { Step::Resume } else { Step::Stay },
        },
        MapKey::Flip => level(match view.level {
            MapLevel::Zone => MapLevel::Field,
            MapLevel::Field => MapLevel::Zone,
        }),
        MapKey::In => level(MapLevel::Field),
        MapKey::Out => level(MapLevel::Zone),
        MapKey::Tab(l) => level(l),
    }
}

/// Read the keys, the wheel and the clicks, and open or close the map. The resource is only
/// written when something changed: the views fill themselves on its change.
#[allow(clippy::too_many_arguments)]
fn map_keys(
    keys: Res<ButtonInput<KeyCode>>,
    mouse: Res<ButtonInput<MouseButton>>,
    mut wheel: MessageReader<MouseWheel>,
    phase: Res<State<Phase>>,
    mut next: ResMut<NextState<Phase>>,
    mut ui: ResMut<WorldMapUi>,
    role: Res<NetRole>,
    focus: Option<Res<TextFocus>>,
    loading: Res<LoadingScreen>,
    tabs: Query<(&Interaction, &LevelTab), Changed<Interaction>>,
    circle: Query<&Interaction, (Changed<Interaction>, With<ArenaCircle>)>,
    frame: Query<&RelativeCursorPosition, With<MapFrame>>,
    mut sound: Option<MessageWriter<GlueSound>>,
) {
    let notches: f32 = wheel
        .read()
        .map(|w| match w.unit {
            MouseScrollUnit::Line => w.y,
            MouseScrollUnit::Pixel => w.y * 0.02,
        })
        .sum();
    let phase = *phase.get();
    // A level-up, the results or the menu take the screen: the map goes without touching the phase.
    if ui.open && !matches!(phase, Phase::Playing | Phase::Paused) {
        ui.open = false;
        ui.paused = false;
        return;
    }
    if !matches!(phase, Phase::Playing | Phase::Paused) {
        return;
    }
    if focus.is_some_and(|f| f.0.is_some()) {
        return;
    }

    let mut inputs: Vec<MapKey> = Vec::new();
    if keys.just_pressed(KeyCode::KeyM) {
        inputs.push(MapKey::Toggle);
    }
    if keys.just_pressed(KeyCode::Escape) {
        inputs.push(MapKey::Esc);
    }
    if keys.just_pressed(KeyCode::KeyZ) {
        inputs.push(MapKey::Flip);
    }
    if notches > 0.0 {
        inputs.push(MapKey::In);
    } else if notches < 0.0 {
        inputs.push(MapKey::Out);
    }
    if mouse.just_pressed(MouseButton::Right) && frame.iter().any(|f| f.cursor_over()) {
        inputs.push(MapKey::Out);
    }
    for (interaction, tab) in &tabs {
        if *interaction == Interaction::Pressed {
            inputs.push(MapKey::Tab(tab.0));
        }
    }
    if circle.iter().any(|i| *i == Interaction::Pressed) {
        inputs.push(MapKey::In);
    }
    if inputs.is_empty() {
        return;
    }

    let mut view = View {
        open: ui.open,
        level: ui.level,
        paused: ui.paused,
    };
    let mut step = Step::Stay;
    for key in inputs {
        // It opens over a running world only, and not behind a loading screen.
        if key == MapKey::Toggle && !view.open && (phase != Phase::Playing || loading.covering()) {
            continue;
        }
        let t = on_key(view, key, *role);
        view = t.view;
        if t.step != Step::Stay {
            step = t.step;
        }
    }

    if view.open != ui.open {
        if let Some(sound) = sound.as_mut() {
            sound.write(GlueSound(if view.open {
                "igQuestLogOpen"
            } else {
                "igQuestLogClose"
            }));
        }
        ui.open = view.open;
    }
    if view.level != ui.level {
        ui.level = view.level;
    }
    if view.paused != ui.paused {
        ui.paused = view.paused;
    }
    match step {
        Step::Pause => next.set(Phase::Paused),
        Step::Resume => next.set(Phase::Playing),
        Step::Stay => {}
    }
}

// --- The window. ---

/// Back at the menu or the results: the window goes, and the next run builds its own.
fn teardown(
    mut commands: Commands,
    mut win: ResMut<RunMap>,
    mut ui: ResMut<WorldMapUi>,
) {
    if let Some(root) = win.root {
        commands.entity(root).try_despawn();
    }
    *win = RunMap::default();
    if ui.open {
        ui.open = false;
    }
    if ui.paused {
        ui.paused = false;
    }
}

/// Build the window once the run's HUD is up: hidden, its two slots empty.
fn build_window(
    mut commands: Commands,
    mut win: ResMut<RunMap>,
    phase: Res<State<Phase>>,
    selection: Res<Selection>,
    fonts: Option<Res<KitFonts>>,
    assets: Option<ResMut<WorldAssets>>,
    mut images: ResMut<Assets<Image>>,
) {
    if win.root.is_some() || !matches!(phase.get(), Phase::Playing | Phase::Paused | Phase::LevelUp)
    {
        return;
    }
    let (Some(fonts), Some(mut assets)) = (fonts, assets) else {
        return;
    };
    let assets = &mut *assets;
    let images = &mut *images;
    let size = PAGE * PAGE_SCALE;
    let name = selection.stage_def().name;
    let (mut zone_slot, mut field_slot, mut legend) =
        (Entity::PLACEHOLDER, Entity::PLACEHOLDER, Entity::PLACEHOLDER);
    let root = commands
        .spawn((
            MapRoot,
            RunEntity,
            GlobalZIndex(Z),
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
        .with_children(|wrap| {
            wrap.spawn((
                kit::panel(Node {
                    width: Val::Px(size.x + 30.0),
                    // Pinned: left to itself the column grew by a third past its content.
                    height: Val::Px(size.y + PANEL_EXTRA),
                    flex_direction: FlexDirection::Column,
                    padding: UiRect::axes(Val::Px(14.0), Val::Px(8.0)),
                    row_gap: Val::Px(5.0),
                    ..default()
                }),
                // Swallows the clicks on the HUD under it.
                FocusPolicy::Block,
            ))
            .with_children(|panel| {
                kit::heading(panel, &fonts, name, 24.0);
                // The page's name on the left, the views on the right.
                panel.spawn(Node {
                    width: Val::Percent(100.0),
                    justify_content: JustifyContent::SpaceBetween,
                    align_items: AlignItems::Center,
                    ..default()
                })
                .with_children(|row| {
                    row.spawn((ZoneTitle, kit::body(&fonts, "", 13.0, kit::MUTED)));
                    row.spawn(Node {
                        column_gap: Val::Px(6.0),
                        ..default()
                    })
                    .with_children(|tabs| {
                        for (level, caption) in
                            [(MapLevel::Zone, "Zone"), (MapLevel::Field, "Battlefield")]
                        {
                            kit::button(
                                tabs,
                                &fonts,
                                KitStyle::Tab,
                                caption,
                                None,
                                Node {
                                    width: Val::Px(112.0),
                                    height: Val::Px(26.0),
                                    ..default()
                                },
                                LevelTab(level),
                            );
                        }
                    });
                });
                panel.spawn((
                    MapFrame,
                    RelativeCursorPosition::default(),
                    BackgroundColor(Color::BLACK),
                    Node {
                        width: Val::Px(size.x),
                        height: Val::Px(size.y),
                        overflow: Overflow::clip(),
                        ..default()
                    },
                ))
                .with_children(|frame| {
                    let slot = |display| Node {
                        display,
                        justify_content: JustifyContent::Center,
                        align_items: AlignItems::Center,
                        ..at(Vec2::ZERO, size.x, size.y)
                    };
                    zone_slot = frame.spawn((ZoneSlot, slot(Display::Flex))).id();
                    field_slot = frame.spawn((FieldSlot, slot(Display::None))).id();
                });
                panel.spawn(Node {
                    width: Val::Percent(100.0),
                    justify_content: JustifyContent::Center,
                    ..default()
                })
                .with_children(|row| {
                    row.spawn(kit::body(&fonts, HINT, 12.0, kit::MUTED));
                });
                legend = panel
                    .spawn((
                        LegendRow,
                        Node {
                            width: Val::Percent(100.0),
                            justify_content: JustifyContent::Center,
                            column_gap: Val::Px(16.0),
                            display: Display::None,
                            ..default()
                        },
                    ))
                    .with_children(|row| legend_items(row, &fonts, assets, images))
                    .id();
            });
        })
        .id();
    win.root = Some(root);
    win.zone_slot = Some(zone_slot);
    win.field_slot = Some(field_slot);
    win.legend = Some(legend);
}

/// The battlefield view's legend: one symbol and a word each, the symbols as that view draws them.
fn legend_items(
    row: &mut ChildSpawnerCommands,
    fonts: &KitFonts,
    assets: &mut WorldAssets,
    images: &mut Assets<Image>,
) {
    let yellow = Color::srgb(1.0, 0.85, 0.1);
    let objects = |cell: usize| {
        let (col, line) = ((cell % 4) as f32, (cell / 4) as f32);
        [col * 0.25, col * 0.25 + 0.25, line * 0.25, line * 0.25 + 0.25]
    };
    let whole = [0.0, 1.0, 0.0, 1.0];
    let items: [(&str, Option<ImageNode>, Color); 7] = [
        (
            "Hero",
            cell_image(assets, images, ARROW_TEXTURE, whole),
            yellow,
        ),
        (
            "Party",
            cell_image(assets, images, PARTY_ICON, whole),
            Color::srgb(0.3, 0.7, 1.0),
        ),
        (
            "Boss",
            cell_image(assets, images, SKULL_TEXTURE, whole),
            Color::srgb(1.0, 0.5, 0.0),
        ),
        (
            "Elite",
            cell_image(assets, images, RAID_ICONS, [0.0, 0.25, 0.0, 0.25]),
            Color::srgb(1.0, 0.85, 0.3),
        ),
        (
            "Chest",
            cell_image(assets, images, OBJECT_ICONS, objects(0)),
            Color::srgb(0.9, 0.7, 0.2),
        ),
        (
            "Quest",
            cell_image(assets, images, OBJECT_ICONS, objects(3)).map(|i| ImageNode {
                color: yellow,
                ..i
            }),
            yellow,
        ),
        ("Loot", None, Color::srgb(0.45, 0.85, 1.0)),
    ];
    for (label, image, fallback) in items {
        row.spawn(Node {
            align_items: AlignItems::Center,
            column_gap: Val::Px(4.0),
            ..default()
        })
        .with_children(|item| {
            let node = Node {
                width: Val::Px(14.0),
                height: Val::Px(14.0),
                ..default()
            };
            match image {
                Some(image) => {
                    item.spawn((image, node));
                }
                None => {
                    item.spawn((
                        Node {
                            border_radius: BorderRadius::MAX,
                            width: Val::Px(8.0),
                            height: Val::Px(8.0),
                            margin: UiRect::all(Val::Px(3.0)),
                            ..default()
                        },
                        BackgroundColor(fallback),
                    ));
                }
            }
            item.spawn(kit::body(fonts, label, 12.0, kit::CREAM));
        });
    }
}

/// Show the window and the view `ui` names, and mark the tab. Runs when the map's state changed.
#[allow(clippy::too_many_arguments)]
fn show_level(
    ui: Res<WorldMapUi>,
    win: Res<RunMap>,
    selection: Res<Selection>,
    mut nodes: Query<&mut Node>,
    mut tabs: Query<(&LevelTab, &mut KitButton)>,
    mut titles: Query<&mut Text, With<ZoneTitle>>,
) {
    if !ui.is_changed() {
        return;
    }
    let mut display = |entity: Option<Entity>, on: bool| {
        let Some(mut node) = entity.and_then(|e| nodes.get_mut(e).ok()) else {
            return;
        };
        let want = if on { Display::Flex } else { Display::None };
        if node.display != want {
            node.display = want;
        }
    };
    display(win.root, ui.open);
    display(win.zone_slot, ui.level == MapLevel::Zone);
    display(win.field_slot, ui.level == MapLevel::Field);
    display(win.legend, ui.level == MapLevel::Field);
    for (tab, mut button) in &mut tabs {
        let selected = tab.0 == ui.level;
        if button.selected != selected {
            button.selected = selected;
        }
    }
    // The zone view names its page when it is built; the battlefield view the battleground's zone.
    if ui.level == MapLevel::Field {
        let zone = selection.stage_def().zone;
        for mut text in &mut titles {
            if text.0 != zone {
                text.0 = zone.to_string();
            }
        }
    }
}

// --- The zone view. ---

/// The continent a `Map.dbc` id is, by name.
fn continent_name(map: u32) -> &'static str {
    match map {
        0 => "Eastern Kingdoms",
        1 => "Kalimdor",
        _ => "",
    }
}

/// The line under the heading for a page: "The Barrens · Kalimdor" for a zone, the continent's
/// name alone for a continent page.
fn page_title(zone: Option<&str>, map: u32) -> String {
    let continent = continent_name(map);
    match zone {
        Some(zone) if !continent.is_empty() => format!("{zone} \u{b7} {continent}"),
        Some(zone) => zone.to_string(),
        None => continent.to_string(),
    }
}

/// Where a point of the world falls on a page, 0..1 across and down.
fn spot_on_page(page: &WorldMapArea, bevy_pos: Vec3) -> Vec2 {
    let [wx, wy, _] = bevy_to_wow(bevy_pos);
    project(page, wx, wy)
}

/// The battleground's circle on a page of `size` canvas units: its centre and its two radii
/// (the page's axes do not scale alike), neither under half of [`CIRCLE_MIN`].
fn circle_on_page(page: &WorldMapArea, center: [f32; 3], leash: f32, size: Vec2) -> (Vec2, Vec2) {
    let c = project(page, center[0], center[1]) * size;
    let reach = Vec2::new(
        leash / (page.loc_left - page.loc_right),
        leash / (page.loc_top - page.loc_bottom),
    )
    .abs()
        * size;
    (c, reach.max(Vec2::splat(CIRCLE_MIN / 2.0)))
}

/// Build the zone page as the map opens on it, and take it down again when the map closes or
/// the view changes. Only the pool of moving marks is left for [`refresh_zone`].
#[allow(clippy::too_many_arguments)]
fn fill_zone(
    mut commands: Commands,
    ui: Res<WorldMapUi>,
    mut win: ResMut<RunMap>,
    selection: Res<Selection>,
    fonts: Option<Res<KitFonts>>,
    assets: Option<ResMut<WorldAssets>>,
    mut images: ResMut<Assets<Image>>,
    mut book: ResMut<Atlas>,
    areas: Option<Res<AreaTableRes>>,
    world: WorldPoint,
    mut titles: Query<&mut Text, With<ZoneTitle>>,
) {
    let want = ui.open && ui.level == MapLevel::Zone;
    if want == win.zone_built {
        return;
    }
    let Some(slot) = win.zone_slot else { return };
    if !want {
        commands.entity(slot).despawn_related::<Children>();
        win.zone_built = false;
        win.zone = None;
        return;
    }
    let (Some(fonts), Some(mut assets)) = (fonts, assets) else {
        return;
    };
    let assets = &mut *assets;
    let images = &mut *images;
    book.ensure(assets, areas.as_deref());
    let book = &*book;
    let def = selection.stage_def();
    let stood = atlas::standing(world.area(), areas.as_deref());
    let found = book.page_for(def, stood);

    let win = &mut *win;
    win.zone_built = true;
    win.next_at = 0.0;
    let mut entry = commands.entity(slot);
    entry.despawn_related::<Children>();
    let Some((id, page)) = found else {
        win.zone = None;
        for mut text in &mut titles {
            text.0 = def.zone.to_string();
        }
        entry.with_children(|b| {
            b.spawn(kit::body(&fonts, "No map for this zone", 18.0, kit::MUTED));
        });
        return;
    };

    let zone_name = (page.area_id != 0)
        .then(|| areas.as_ref().and_then(|a| a.0.name(page.area_id)))
        .flatten()
        .or_else(|| (page.area_id != 0).then_some(page.name.as_str()));
    let title = page_title(zone_name, page.map_id);
    for mut text in &mut titles {
        if text.0 != title {
            text.0.clone_from(&title);
        }
    }

    let size = PAGE * PAGE_SCALE;
    let (center, reach) = circle_on_page(page, def.center, def.leash, size);
    let skull = cell_image(assets, images, SKULL_TEXTURE, [0.0, 1.0, 0.0, 1.0]);
    let party = cell_image(assets, images, PARTY_ICON, [0.0, 1.0, 0.0, 1.0]);
    let arrow = cell_image(assets, images, ARROW_TEXTURE, [0.0, 1.0, 0.0, 1.0]);
    let (mut arrow_id, mut skull_ids, mut ally_ids) = (Entity::PLACEHOLDER, Vec::new(), Vec::new());
    entry.with_children(|b| {
        spawn_page(b, assets, images, page, book.overlays(id), PAGE_SCALE);
        // The battleground: the ground the run is fought on, a click zooms into it.
        b.spawn((
            Button,
            ArenaCircle,
            Node {
                border: UiRect::all(Val::Px(2.0)),
                ..round(center - reach, reach.x * 2.0, reach.y * 2.0)
            },
            BorderColor::all(BARRIER_TINT),
            BackgroundColor(BARRIER_TINT.with_alpha(CIRCLE_RESTING)),
        ));
        for _ in 0..ALLIES {
            ally_ids.push(mark(b, party.clone(), ALLY_PX, Color::srgb(0.3, 0.7, 1.0)));
        }
        for _ in 0..SKULLS {
            skull_ids.push(mark(b, skull.clone(), SKULL_PX, Color::srgb(1.0, 0.5, 0.0)));
        }
        arrow_id = mark(b, arrow, ARROW_PX, Color::srgb(1.0, 0.85, 0.1));
    });
    win.zone = Some(ZoneView {
        page: page.clone(),
        arrow: arrow_id,
        skulls: skull_ids,
        allies: ally_ids,
    });
}

/// One mark of the pool: hidden, `px` square, an image or else a dot of `fallback`.
fn mark(
    b: &mut ChildSpawnerCommands,
    image: Option<ImageNode>,
    px: f32,
    fallback: Color,
) -> Entity {
    let node = Node {
        display: Display::None,
        ..round(Vec2::ZERO, px, px)
    };
    match image {
        Some(image) => b.spawn((image, node)).id(),
        None => b.spawn((node, BackgroundColor(fallback))).id(),
    }
}

/// Put a pooled mark with its centre at `at` (canvas units on the page), or hide it.
fn put(nodes: &mut Query<&mut Node>, entity: Entity, at: Option<Vec2>, px: f32) {
    let Ok(mut node) = nodes.get_mut(entity) else {
        return;
    };
    let Some(p) = at else {
        if node.display != Display::None {
            node.display = Display::None;
        }
        return;
    };
    let (left, top) = (
        Val::Px((p.x - px / 2.0).round()),
        Val::Px((p.y - px / 2.0).round()),
    );
    if node.display != Display::Flex {
        node.display = Display::Flex;
    }
    if node.left != left {
        node.left = left;
    }
    if node.top != top {
        node.top = top;
    }
}

/// Place the arrow, the skulls and the party dots on the zone page, ten times a second.
#[allow(clippy::too_many_arguments)]
fn refresh_zone(
    time: Res<Time<Real>>,
    ui: Res<WorldMapUi>,
    mut win: ResMut<RunMap>,
    hero: Query<&Transform, With<Hero>>,
    pins: Query<(&Transform, &MapPin)>,
    markers: Query<(&Transform, &MapMarker, Option<&Enemy>)>,
    mut nodes: Query<&mut Node>,
    mut turns: Query<&mut UiTransform>,
) {
    if !ui.open || ui.level != MapLevel::Zone {
        return;
    }
    let win = &mut *win;
    let Some(view) = win.zone.as_ref() else {
        return;
    };
    let now = time.elapsed_secs();
    if now < win.next_at {
        return;
    }
    win.next_at = now + TICK;

    let size = PAGE * PAGE_SCALE;
    let spot = |t: &Transform| {
        let uv = spot_on_page(&view.page, t.translation);
        ((0.0..=1.0).contains(&uv.x) && (0.0..=1.0).contains(&uv.y)).then_some(uv * size)
    };

    // The hero's arrow turns with the facing: counter-clockwise from north, as the yaw runs.
    match hero.single() {
        Ok(hero) => {
            put(&mut nodes, view.arrow, spot(hero), ARROW_PX);
            if let Ok(mut turn) = turns.get_mut(view.arrow) {
                let yaw = hero.rotation.to_euler(EulerRot::YXZ).0;
                let want = Rot2::radians(-yaw);
                if turn.rotation != want {
                    turn.rotation = want;
                }
            }
        }
        Err(_) => put(&mut nodes, view.arrow, None, ARROW_PX),
    }

    let allies = pins
        .iter()
        .filter(|(_, p)| **p == MapPin::Ally)
        .filter_map(|(t, _)| spot(t));
    fill_pool(&mut nodes, &view.allies, allies, ALLY_PX);

    // Bosses first, so the rares never take their place when the pool runs short.
    let mut skulls = Vec::new();
    for kind in [MapMarker::Boss, MapMarker::Rare] {
        for (t, m, enemy) in &markers {
            if *m == kind && enemy.is_none_or(|e| !e.dead) {
                skulls.extend(spot(t));
            }
        }
    }
    fill_pool(&mut nodes, &view.skulls, skulls.into_iter(), SKULL_PX);
}

/// Spread `spots` over a pool of marks and hide the ones left over.
fn fill_pool(
    nodes: &mut Query<&mut Node>,
    pool: &[Entity],
    spots: impl Iterator<Item = Vec2>,
    px: f32,
) {
    let mut spots = spots.fuse();
    for &entity in pool {
        put(nodes, entity, spots.next(), px);
    }
}

/// The battleground's circle brightens under the cursor.
fn circle_hover(
    mut circles: Query<(&Interaction, &mut BackgroundColor), (Changed<Interaction>, With<ArenaCircle>)>,
) {
    for (interaction, mut fill) in &mut circles {
        let alpha = if *interaction == Interaction::None {
            CIRCLE_RESTING
        } else {
            CIRCLE_HOT
        };
        fill.0 = BARRIER_TINT.with_alpha(alpha);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn closed() -> View {
        View {
            open: false,
            level: MapLevel::Zone,
            paused: false,
        }
    }

    fn open(paused: bool) -> View {
        View {
            open: true,
            paused,
            ..closed()
        }
    }

    fn row(left: f32, right: f32, top: f32, bottom: f32) -> WorldMapArea {
        WorldMapArea {
            map_id: 1,
            area_id: 17,
            name: "Barrens".into(),
            loc_left: left,
            loc_right: right,
            loc_top: top,
            loc_bottom: bottom,
        }
    }

    #[test]
    fn the_map_pauses_only_alone() {
        // Closed, M: alone it opens and pauses; in a party it opens over the running world.
        let t = on_key(closed(), MapKey::Toggle, NetRole::Solo);
        assert_eq!((t.view, t.step), (open(true), Step::Pause));
        for role in [NetRole::Host, NetRole::Client] {
            let t = on_key(closed(), MapKey::Toggle, role);
            assert_eq!((t.view, t.step), (open(false), Step::Stay), "{role:?}");
        }
        // Open, Esc or M: closes, and resumes only what the map paused.
        for key in [MapKey::Esc, MapKey::Toggle] {
            let t = on_key(open(true), key, NetRole::Solo);
            assert_eq!((t.view, t.step), (closed(), Step::Resume));
            let t = on_key(open(false), key, NetRole::Host);
            assert_eq!((t.view, t.step), (closed(), Step::Stay));
        }
        // Z flips the view and nothing else.
        let t = on_key(open(true), MapKey::Flip, NetRole::Solo);
        assert_eq!(
            (t.view, t.step),
            (
                View {
                    level: MapLevel::Field,
                    ..open(true)
                },
                Step::Stay
            )
        );
        let back = on_key(t.view, MapKey::Flip, NetRole::Solo);
        assert_eq!(back.view, open(true));
    }

    #[test]
    fn only_m_opens_a_closed_map() {
        for key in [
            MapKey::Esc,
            MapKey::Flip,
            MapKey::In,
            MapKey::Out,
            MapKey::Tab(MapLevel::Field),
        ] {
            let t = on_key(closed(), key, NetRole::Solo);
            assert_eq!((t.view, t.step), (closed(), Step::Stay), "{key:?}");
        }
    }

    #[test]
    fn the_views_follow_the_wheel_the_tabs_and_the_right_click() {
        let field = View {
            level: MapLevel::Field,
            ..open(false)
        };
        assert_eq!(on_key(open(false), MapKey::In, NetRole::Host).view, field);
        assert_eq!(on_key(field, MapKey::In, NetRole::Host).view, field);
        assert_eq!(on_key(field, MapKey::Out, NetRole::Host).view, open(false));
        assert_eq!(on_key(open(false), MapKey::Out, NetRole::Host).view, open(false));
        assert_eq!(
            on_key(open(true), MapKey::Tab(MapLevel::Field), NetRole::Solo).view.level,
            MapLevel::Field
        );
    }

    #[test]
    fn the_title_names_the_zone_and_its_continent() {
        assert_eq!(page_title(Some("The Barrens"), 1), "The Barrens \u{b7} Kalimdor");
        assert_eq!(page_title(None, 0), "Eastern Kingdoms");
        assert_eq!(page_title(Some("Somewhere"), 99), "Somewhere");
    }

    #[test]
    fn the_hero_lands_where_the_world_point_projects() {
        use benilla_assets::coords::wow_to_bevy;
        let z = row(100.0, -100.0, 200.0, 0.0);
        let centre = spot_on_page(&z, wow_to_bevy([100.0, 0.0, 7.0]));
        assert!((centre - Vec2::splat(0.5)).length() < 1e-5);
        // Walking east lowers the world y and moves right on the page, north moves up.
        let east = spot_on_page(&z, wow_to_bevy([100.0, -50.0, 7.0]));
        assert!((east - Vec2::new(0.75, 0.5)).length() < 1e-5);
        let north = spot_on_page(&z, wow_to_bevy([150.0, 0.0, 7.0]));
        assert!((north - Vec2::new(0.5, 0.25)).length() < 1e-5);
    }

    #[test]
    fn the_circle_is_never_a_speck() {
        let z = row(1000.0, -1000.0, 1000.0, -1000.0);
        let size = Vec2::new(852.0, 568.0);
        // A 150 yd leash on a 2000 yd page: 150 / 2000 of the page each way.
        let (c, reach) = circle_on_page(&z, [0.0, 0.0, 0.0], 150.0, size);
        assert!((c - size / 2.0).length() < 1e-3);
        assert!((reach - Vec2::new(150.0 / 2000.0 * 852.0, 150.0 / 2000.0 * 568.0)).length() < 1e-3);
        // A 30 yd arena on a page of 100000 yd stays CIRCLE_MIN across.
        let big = row(50000.0, -50000.0, 50000.0, -50000.0);
        let (_, reach) = circle_on_page(&big, [0.0, 0.0, 0.0], 30.0, size);
        assert_eq!(reach, Vec2::splat(CIRCLE_MIN / 2.0));
    }
}
