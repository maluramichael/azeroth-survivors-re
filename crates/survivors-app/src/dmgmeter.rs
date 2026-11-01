//! The damage meter: a small window in the run that ranks the hero's spells by the damage they
//! dealt this run, with each spell's share. Also the line under the clock that counts down to the
//! next boss ([`super::hud::NextBossText`]).

use std::collections::HashMap;

use bevy::prelude::*;

use super::data::{self, AbilityDef, Kind};
use super::finale;
use super::hud::{self, IconDone, NextBossText, SpellIcon};
use super::kit::{self, KitFonts};
use super::{Phase, Run, Selection};

/// How many spells the window lists.
const ROWS: usize = 8;
const WIDTH: f32 = 240.0;
const ROW_H: f32 = 16.0;
const ICON: f32 = 14.0;
const Z: i32 = 401;

/// The damage each ability dealt this run, by the ability's key: filled by
/// [`super::abilities::apply_hits`], emptied when the run is reset.
#[derive(Resource, Default)]
pub(super) struct DamageDone(pub HashMap<&'static str, (&'static AbilityDef, f32)>);

impl DamageDone {
    /// Count `damage` for `source`; a hit no ability owns is not counted.
    pub(super) fn add(&mut self, source: Option<&'static AbilityDef>, damage: f32) {
        if let Some(def) = source {
            self.0.entry(def.key).or_insert((def, 0.0)).1 += damage;
        }
    }
}

/// One line of the window.
struct Row {
    node: Entity,
    fill: Entity,
    icon: Entity,
    name: Entity,
    value: Entity,
}

/// The window's entities, once built.
#[derive(Resource)]
struct Meter {
    root: Option<Entity>,
    total: Entity,
    rows: Vec<Row>,
}

impl Default for Meter {
    fn default() -> Self {
        Self {
            root: None,
            total: Entity::PLACEHOLDER,
            rows: Vec::new(),
        }
    }
}

impl Default for Row {
    fn default() -> Self {
        Self {
            node: Entity::PLACEHOLDER,
            fill: Entity::PLACEHOLDER,
            icon: Entity::PLACEHOLDER,
            name: Entity::PLACEHOLDER,
            value: Entity::PLACEHOLDER,
        }
    }
}

pub(crate) fn plugin(app: &mut App) {
    app.init_resource::<DamageDone>()
        .init_resource::<Meter>()
        .add_systems(Update, (refresh_meter, refresh_next_boss))
        .add_systems(OnEnter(Phase::Menu), forget);
}

/// A new run starts from the menu: the tally starts over.
fn forget(mut done: ResMut<DamageDone>) {
    done.0.clear();
}

/// The phases that put the HUD on screen.
fn in_run(phase: &State<Phase>) -> bool {
    matches!(
        phase.get(),
        Phase::Playing | Phase::LevelUp | Phase::Paused
    )
}

/// 2.4k, 1.3M, 870.
fn short(v: f32) -> String {
    if v >= 1e6 {
        format!("{:.1}M", v / 1e6)
    } else if v >= 1e3 {
        format!("{:.1}k", v / 1e3)
    } else {
        format!("{}", v.round() as u64)
    }
}

/// The bar's fill: a dark warm red, a brown for the melee swings.
fn tint(kind: Kind) -> Color {
    if kind == Kind::Melee {
        Color::srgba(0.47, 0.32, 0.12, 0.85)
    } else {
        Color::srgba(0.50, 0.13, 0.09, 0.85)
    }
}

fn build(commands: &mut Commands, fonts: &KitFonts, meter: &mut Meter) {
    let mut total = Entity::PLACEHOLDER;
    let mut rows: Vec<Row> = Vec::with_capacity(ROWS);
    let root = commands
        .spawn((
            Node {
                position_type: PositionType::Absolute,
                right: Val::Px(14.0),
                top: Val::Percent(48.0),
                width: Val::Px(WIDTH),
                flex_direction: FlexDirection::Column,
                row_gap: Val::Px(2.0),
                padding: UiRect::all(Val::Px(5.0)),
                border: UiRect::all(Val::Px(1.0)),
                border_radius: BorderRadius::all(Val::Px(6.0)),
                display: Display::None,
                ..default()
            },
            BackgroundColor(Color::srgba(0.07, 0.055, 0.04, 0.8)),
            BorderColor::all(kit::PANEL_EDGE),
            GlobalZIndex(Z),
        ))
        .with_children(|win| {
            // The title bar: the name left, the run's total right.
            win.spawn(Node {
                width: Val::Percent(100.0),
                justify_content: JustifyContent::SpaceBetween,
                align_items: AlignItems::Center,
                margin: UiRect::bottom(Val::Px(2.0)),
                ..default()
            })
            .with_children(|bar| {
                bar.spawn(kit::body(fonts, "Damage Done", 12.0, kit::TITLE));
                total = bar.spawn(kit::body(fonts, "", 12.0, kit::CREAM)).id();
            });
            for _ in 0..ROWS {
                let mut row = Row::default();
                row.node = win
                    .spawn((
                        Node {
                            width: Val::Percent(100.0),
                            height: Val::Px(ROW_H),
                            justify_content: JustifyContent::SpaceBetween,
                            align_items: AlignItems::Center,
                            padding: UiRect::horizontal(Val::Px(2.0)),
                            display: Display::None,
                            ..default()
                        },
                        BackgroundColor(Color::srgba(0.0, 0.0, 0.0, 0.35)),
                    ))
                    .with_children(|r| {
                        // The share bar, behind the row's texts.
                        row.fill = r
                            .spawn((
                                Node {
                                    position_type: PositionType::Absolute,
                                    left: Val::Px(0.0),
                                    top: Val::Px(0.0),
                                    width: Val::Percent(0.0),
                                    height: Val::Percent(100.0),
                                    ..default()
                                },
                                BackgroundColor(tint(Kind::Bolt)),
                            ))
                            .id();
                        r.spawn(Node {
                            align_items: AlignItems::Center,
                            column_gap: Val::Px(3.0),
                            ..default()
                        })
                        .with_children(|left| {
                            row.icon = left
                                .spawn((
                                    Node {
                                        width: Val::Px(ICON),
                                        height: Val::Px(ICON),
                                        ..default()
                                    },
                                    ImageNode {
                                        color: Color::srgba(0.0, 0.0, 0.0, 0.6),
                                        ..default()
                                    },
                                    SpellIcon(0),
                                ))
                                .id();
                            row.name = left.spawn(kit::body(fonts, "", 11.0, kit::CREAM)).id();
                        });
                        row.value = r.spawn(kit::body(fonts, "", 11.0, kit::CREAM)).id();
                    })
                    .id();
                rows.push(row);
            }
        })
        .id();
    meter.root = Some(root);
    meter.total = total;
    meter.rows = rows;
}

/// Write `s` only when it differs, so an unchanged text is not laid out again.
fn set_text(t: &mut Mut<Text>, s: String) {
    if t.0 != s {
        t.0 = s;
    }
}

/// Build the window once the fonts are up, show it while a run is on, and keep its rows current.
fn refresh_meter(
    mut commands: Commands,
    phase: Res<State<Phase>>,
    fonts: Option<Res<KitFonts>>,
    done: Res<DamageDone>,
    spells: Option<Res<benilla_app::survivors_host::ui_action::Spells>>,
    mut meter: ResMut<Meter>,
    mut nodes: Query<&mut Node>,
    mut texts: Query<&mut Text>,
    mut fills: Query<&mut BackgroundColor>,
    mut icons: Query<&mut SpellIcon>,
) {
    if meter.root.is_none() {
        if let Some(fonts) = fonts {
            build(&mut commands, &fonts, &mut meter);
        }
        return;
    }
    let show = in_run(&phase);
    if let Some(root) = meter.root {
        if let Ok(mut node) = nodes.get_mut(root) {
            let display = if show { Display::Flex } else { Display::None };
            if node.display != display {
                node.display = display;
            }
        }
    }
    if !show {
        return;
    }

    let mut ranked: Vec<(&'static AbilityDef, f32)> = done.0.values().copied().collect();
    ranked.sort_by(|a, b| b.1.total_cmp(&a.1).then_with(|| a.0.key.cmp(b.0.key)));
    let total: f32 = ranked.iter().map(|r| r.1).sum();
    let top = ranked.first().map_or(1.0, |r| r.1).max(1.0);

    if let Ok(mut t) = texts.get_mut(meter.total) {
        set_text(&mut t, if total > 0.0 { short(total) } else { String::new() });
    }
    for (i, row) in meter.rows.iter().enumerate() {
        let entry = ranked.get(i);
        if let Ok(mut node) = nodes.get_mut(row.node) {
            let display = if entry.is_some() {
                Display::Flex
            } else {
                Display::None
            };
            if node.display != display {
                node.display = display;
            }
        }
        let Some(&(def, dmg)) = entry else { continue };
        if let Ok(mut node) = nodes.get_mut(row.fill) {
            let width = Val::Percent(dmg / top * 100.0);
            if node.width != width {
                node.width = width;
            }
        }
        if let Ok(mut c) = fills.get_mut(row.fill) {
            let color = tint(def.kind);
            if c.0 != color {
                c.0 = color;
            }
        }
        if let Ok(mut icon) = icons.get_mut(row.icon) {
            if icon.0 != def.spell {
                icon.0 = def.spell;
                commands.entity(row.icon).remove::<IconDone>();
            }
        }
        if let Ok(mut t) = texts.get_mut(row.name) {
            let name = hud::spell_name(spells.as_deref(), def.spell, def.key);
            set_text(&mut t, format!("{}. {name}", i + 1));
        }
        if let Ok(mut t) = texts.get_mut(row.value) {
            let pct = (dmg / total.max(1.0) * 100.0).round() as u32;
            set_text(&mut t, format!("{} ({pct}%)", short(dmg)));
        }
    }
}

/// The line under the clock: the next timed boss, else the finale, else nothing.
fn refresh_next_boss(
    run: Res<Run>,
    selection: Res<Selection>,
    mut line: Query<&mut Text, With<NextBossText>>,
) {
    let Ok(mut t) = line.single_mut() else { return };
    let elapsed = run.elapsed;
    let timed = finale::plan(selection.map).timed.len() as u32;
    let finale_at = finale::finale_at();
    // The k-th timed boss (counting from 0) comes at `(k + 1) * BOSS_EVERY_SECS`.
    let next = (run.bosses < timed).then(|| (run.bosses + 1) as f32 * data::BOSS_EVERY_SECS);
    let (caption, at) = match next {
        Some(at) if at < finale_at => ("Next boss", at),
        _ if elapsed < finale_at => ("Final boss", finale_at),
        _ => {
            set_text(&mut t, String::new());
            return;
        }
    };
    let left = (at - elapsed).max(0.0).ceil() as u32;
    set_text(&mut t, format!("{caption} in {:02}:{:02}", left / 60, left % 60));
}
