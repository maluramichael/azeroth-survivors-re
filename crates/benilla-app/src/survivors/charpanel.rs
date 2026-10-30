//! The character window: **C** opens and closes it during a run, as the character pane does in
//! WoW. It lists the hero's numbers as they stand, every source counted (the class, the
//! character's level and the trainer, the passives picked this run), and the abilities and
//! passives with their rank.

use bevy::prelude::*;

use super::hud::{self, HudFont, GOLD_TONE};
use super::profile::Profile;
use super::{Phase, Run, Selection};

/// Lines the window can hold; the build stops at well under this.
const ROWS: usize = 48;
const WIDTH: f32 = 300.0;
/// Over the HUD (400), under the level-up cards and the pause screen (410 and up).
const Z: i32 = 405;
const FILL: Color = Color::srgba(0.02, 0.02, 0.03, 0.85);
const EDGE: Color = Color::srgba(0.8, 0.8, 0.8, 0.9);
/// Labels in the pane's yellow, values in white, as the WoW character pane prints them.
const LABEL: Color = Color::srgb(1.0, 0.82, 0.0);
const VALUE: Color = Color::WHITE;
const RANK: Color = Color::srgb(0.6, 0.6, 0.6);
/// A section heading.
const HEAD: Color = Color::srgb(0.9, 0.9, 0.9);
/// Cooldowns go no lower than this share of their base, as [`super::Owned::cooldown`] holds them.
const HASTE_CAP: f32 = 0.65;

pub(super) fn plugin(app: &mut App) {
    app.init_resource::<CharPanel>()
        .add_systems(Update, (toggle, refresh).chain().run_if(in_run))
        .add_systems(OnEnter(Phase::Menu), teardown)
        .add_systems(OnEnter(Phase::GameOver), teardown);
}

/// The phases that put the HUD on screen.
fn in_run(phase: Res<State<Phase>>) -> bool {
    matches!(
        phase.get(),
        Phase::Playing | Phase::LevelUp | Phase::Paused
    )
}

#[derive(Resource, Default)]
struct CharPanel {
    /// C: whether the window shows.
    open: bool,
    root: Option<Entity>,
    /// Per line: the row, its label and its value.
    rows: Vec<(Entity, Entity, Entity)>,
}

/// One line of the window.
struct Line {
    left: String,
    right: String,
    left_color: Color,
    right_color: Color,
}

impl Line {
    fn head(text: &str) -> Self {
        Self {
            left: text.to_string(),
            right: String::new(),
            left_color: HEAD,
            right_color: VALUE,
        }
    }
    fn stat(label: &str, value: String) -> Self {
        Self {
            left: label.to_string(),
            right: value,
            left_color: LABEL,
            right_color: VALUE,
        }
    }
    fn gap() -> Self {
        Self::head(" ")
    }
}

fn toggle(keys: Res<ButtonInput<KeyCode>>, mut panel: ResMut<CharPanel>) {
    if keys.just_pressed(KeyCode::KeyC) {
        panel.open = !panel.open;
    }
}

/// Back at the menu or the results: the window goes and closes, and the next run builds its own.
fn teardown(mut commands: Commands, mut panel: ResMut<CharPanel>) {
    if let Some(root) = panel.root.take() {
        commands.entity(root).try_despawn();
    }
    panel.rows.clear();
    panel.open = false;
}

/// The window's frame, its title and a pool of empty lines, hidden.
fn build(commands: &mut Commands, panel: &mut CharPanel, font: &Handle<Font>) {
    let text = |size: f32, color: Color| {
        (
            Text::new(""),
            TextFont {
                font: font.clone(),
                font_size: size,
                ..default()
            },
            TextColor(color),
            TextShadow {
                offset: Vec2::splat(1.0),
                color: Color::BLACK,
            },
        )
    };
    let mut rows = Vec::with_capacity(ROWS);
    let root = commands
        .spawn((
            Node {
                position_type: PositionType::Absolute,
                left: Val::Px(16.0),
                top: Val::Px(120.0),
                width: Val::Px(WIDTH),
                flex_direction: FlexDirection::Column,
                padding: UiRect::axes(Val::Px(12.0), Val::Px(10.0)),
                row_gap: Val::Px(2.0),
                border: UiRect::all(Val::Px(1.0)),
                border_radius: BorderRadius::all(Val::Px(6.0)),
                ..default()
            },
            BackgroundColor(FILL),
            BorderColor::all(EDGE),
            GlobalZIndex(Z),
            Visibility::Hidden,
        ))
        .with_children(|p| {
            p.spawn((
                Text::new("Character"),
                TextFont {
                    font: font.clone(),
                    font_size: 16.0,
                    ..default()
                },
                TextColor(GOLD_TONE),
                TextShadow {
                    offset: Vec2::splat(1.0),
                    color: Color::BLACK,
                },
                Node {
                    margin: UiRect::bottom(Val::Px(4.0)),
                    ..default()
                },
            ));
            for _ in 0..ROWS {
                let mut row = p.spawn(Node {
                    justify_content: JustifyContent::SpaceBetween,
                    column_gap: Val::Px(12.0),
                    display: Display::None,
                    ..default()
                });
                let row_id = row.id();
                let (mut left, mut right) = (Entity::PLACEHOLDER, Entity::PLACEHOLDER);
                row.with_children(|r| {
                    left = r.spawn(text(12.0, LABEL)).id();
                    right = r.spawn(text(12.0, VALUE)).id();
                });
                rows.push((row_id, left, right));
            }
        })
        .id();
    panel.root = Some(root);
    panel.rows = rows;
}

fn set<T: PartialEq>(slot: &mut T, v: T) {
    if *slot != v {
        *slot = v;
    }
}

/// The window's lines for the run as it stands.
fn lines(run: &Run, char_level: u32, spells: Option<&crate::ui_action::Spells>) -> Vec<Line> {
    let s = run.stats();
    let haste = s.haste.clamp(0.0, HASTE_CAP);
    let mut out = vec![Line {
        left: format!("Level {char_level} {}", run.class.name),
        right: format!("Run level {}", run.level),
        left_color: Color::srgb_u8(
            (run.class.color >> 16) as u8,
            (run.class.color >> 8) as u8,
            run.class.color as u8,
        ),
        right_color: VALUE,
    }];

    out.push(Line::gap());
    out.push(Line::head("General"));
    out.push(Line::stat(
        "Health",
        // Whole points, as the unit frames count them.
        format!("{} / {}", run.hp.max(0.0).ceil(), s.max_hp.round()),
    ));
    out.push(Line::stat("Health per second", hud::num(s.regen)));
    out.push(Line::stat("Damage blocked per hit", hud::num(s.armor)));
    out.push(Line::stat(
        "Movement speed",
        format!("{} yd/s", hud::num(s.speed * run.haste_mult())),
    ));

    out.push(Line::gap());
    out.push(Line::head("Offense"));
    out.push(Line::stat("Damage", hud::pct(s.might)));
    // Haste shortens every cooldown, so the abilities fire this much more often.
    out.push(Line::stat(
        "Attack and cast speed",
        format!("+{}", hud::pct(1.0 / (1.0 - haste) - 1.0)),
    ));
    out.push(Line::stat("Cooldowns", format!("-{}", hud::pct(haste))));
    out.push(Line::stat("Critical strike chance", hud::pct(s.crit)));
    out.push(Line::stat("Area of effect", hud::pct(s.area)));

    out.push(Line::gap());
    out.push(Line::head("Utility"));
    out.push(Line::stat(
        "Pickup range",
        format!("{} yd", hud::num(s.magnet)),
    ));
    out.push(Line::stat("Experience gained", hud::pct(s.growth)));
    out.push(Line::stat("Gold found", hud::pct(run.meta.greed)));
    out.push(Line::stat(
        "Gold this run",
        hud::grouped(u64::from(run.gold)),
    ));

    if !run.abilities.is_empty() {
        out.push(Line::gap());
        out.push(Line::head("Abilities"));
        for o in &run.abilities {
            out.push(Line {
                left: hud::spell_name(spells, o.def.spell, o.def.key),
                right: format!(
                    "Rank {}, every {} s",
                    o.level,
                    hud::num(o.cooldown(&s))
                ),
                left_color: VALUE,
                right_color: RANK,
            });
        }
    }
    if !run.passives.is_empty() {
        out.push(Line::gap());
        out.push(Line::head("Talents"));
        for &(p, level) in &run.passives {
            out.push(Line {
                left: p.name.to_string(),
                right: format!("Rank {level}"),
                left_color: VALUE,
                right_color: RANK,
            });
        }
    }
    out
}

/// Fill the window's lines while it is open, and hide it while it is closed.
#[allow(clippy::too_many_arguments)]
fn refresh(
    mut commands: Commands,
    mut panel: ResMut<CharPanel>,
    font: Option<Res<HudFont>>,
    run: Res<Run>,
    selection: Res<Selection>,
    profile: Option<Res<Profile>>,
    spells: Option<Res<crate::ui_action::Spells>>,
    mut vis: Query<&mut Visibility>,
    mut nodes: Query<&mut Node>,
    mut texts: Query<(&mut Text, &mut TextColor)>,
) {
    let panel = &mut *panel;
    let Some(root) = panel.root else {
        if panel.open {
            if let Some(font) = font {
                // Seated from the next frame on, once its nodes exist.
                build(&mut commands, panel, &font.0);
            }
        }
        return;
    };
    if let Ok(mut v) = vis.get_mut(root) {
        set(
            &mut *v,
            if panel.open {
                Visibility::Inherited
            } else {
                Visibility::Hidden
            },
        );
    }
    if !panel.open {
        return;
    }
    let char_level = selection
        .class
        .zip(profile.as_deref())
        .map_or(1, |(class, p)| p.character(class).level);
    let lines = lines(&run, char_level, spells.as_deref());
    for (i, &(row, left, right)) in panel.rows.iter().enumerate() {
        let line = lines.get(i);
        if let Ok(mut node) = nodes.get_mut(row) {
            set(
                &mut node.display,
                if line.is_some() {
                    Display::Flex
                } else {
                    Display::None
                },
            );
        }
        let Some(line) = line else { continue };
        for (entity, value, color) in [
            (left, &line.left, line.left_color),
            (right, &line.right, line.right_color),
        ] {
            if let Ok((mut text, mut tc)) = texts.get_mut(entity) {
                if text.0 != *value {
                    text.0.clone_from(value);
                }
                set(&mut tc.0, color);
            }
        }
    }
}
