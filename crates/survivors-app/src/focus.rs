//! The hero's focus: which enemies the abilities pick, nearest, lowest health or strongest. **Tab**
//! cycles it, as it cycles targets in WoW, and so does a click on the icon at the bottom right.

use bevy::prelude::*;

use super::hud::{HudFont, GOLD_TONE};
use super::Phase;

const ICON: f32 = 40.0;
/// Level with the HUD (400), under the character window and the level-up cards.
const Z: i32 = 400;
const FILL: Color = Color::srgba(0.02, 0.02, 0.03, 0.85);
const HINT: Color = Color::srgb(0.6, 0.6, 0.6);

/// How the abilities that pick targets order the enemies within their range.
#[derive(Resource, Default, Clone, Copy, PartialEq, Eq, Debug)]
pub(super) enum Focus {
    /// The closest first.
    #[default]
    Nearest,
    /// The least current health first.
    Lowest,
    /// Bosses first, then the most maximum health.
    Strongest,
}

impl Focus {
    const ALL: [Focus; 3] = [Focus::Nearest, Focus::Lowest, Focus::Strongest];

    fn index(self) -> usize {
        Self::ALL.iter().position(|&f| f == self).unwrap_or(0)
    }

    pub(super) fn next(self) -> Self {
        Self::ALL[(self.index() + 1) % Self::ALL.len()]
    }

    fn label(self) -> &'static str {
        match self {
            Focus::Nearest => "Nearest",
            Focus::Lowest => "Lowest",
            Focus::Strongest => "Strongest",
        }
    }

    fn hint(self) -> &'static str {
        match self {
            Focus::Nearest => "closest first",
            Focus::Lowest => "lowest health first",
            Focus::Strongest => "bosses, then toughest",
        }
    }

    fn color(self) -> Color {
        match self {
            Focus::Nearest => GOLD_TONE,
            Focus::Lowest => Color::srgb(0.4, 0.9, 0.4),
            Focus::Strongest => Color::srgb(1.0, 0.45, 0.2),
        }
    }

    fn icon(self) -> &'static str {
        match self {
            Focus::Nearest => "Interface\\Icons\\Ability_Marksmanship.blp",
            Focus::Lowest => "Interface\\Icons\\Ability_Rogue_Eviscerate.blp",
            Focus::Strongest => "Interface\\Icons\\INV_Misc_Bone_HumanSkull_01.blp",
        }
    }
}

/// The enemy the hero has clicked ([`super::frames`]). The abilities pick it first while it is in
/// range, then the focus's order.
#[derive(Resource, Default, Clone, Copy, PartialEq, Eq, Debug)]
pub(super) struct Target(pub(super) Option<Entity>);

/// A co-op guest's focus and clicked target, on its puppet on the host: its spells pick by them
/// as this machine's hero's pick by [`Focus`] and [`Target`].
#[cfg_attr(not(all(feature = "coop", not(target_arch = "wasm32"))), allow(dead_code))]
#[derive(Component, Default, Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) struct PeerAim {
    pub focus: Focus,
    pub target: Option<Entity>,
}

impl Focus {
    /// As one byte, for the wire.
    #[cfg_attr(not(all(feature = "coop", not(target_arch = "wasm32"))), allow(dead_code))]
    pub(crate) fn to_byte(self) -> u8 {
        match self {
            Focus::Nearest => 0,
            Focus::Lowest => 1,
            Focus::Strongest => 2,
        }
    }

    /// From [`Self::to_byte`]; anything else is the nearest.
    #[cfg_attr(not(all(feature = "coop", not(target_arch = "wasm32"))), allow(dead_code))]
    pub(crate) fn from_byte(b: u8) -> Self {
        match b {
            1 => Focus::Lowest,
            2 => Focus::Strongest,
            _ => Focus::Nearest,
        }
    }
}

pub(super) fn plugin(app: &mut App) {
    app.init_resource::<Focus>()
        .init_resource::<Target>()
        .init_resource::<FocusUi>()
        .add_systems(Update, (cycle, show).chain().run_if(in_run))
        .add_systems(OnEnter(Phase::Menu), teardown)
        .add_systems(OnEnter(Phase::GameOver), teardown);
}

/// The phases that put the HUD on screen.
fn in_run(phase: Res<State<Phase>>) -> bool {
    matches!(phase.get(), Phase::Playing | Phase::LevelUp | Phase::Paused)
}

/// The icon button.
#[derive(Component)]
struct FocusButton;

/// The nodes the icon updates, once built.
struct Parts {
    image: Entity,
    label: Entity,
    hint: Entity,
    /// One icon per focus, in [`Focus::ALL`] order; none when the archive has not got it.
    icons: [Option<Handle<Image>>; 3],
}

#[derive(Resource, Default)]
struct FocusUi {
    root: Option<Entity>,
    parts: Option<Parts>,
}

/// Tab or a click on the icon moves to the next focus, while the run is going.
fn cycle(
    keys: Res<ButtonInput<KeyCode>>,
    phase: Res<State<Phase>>,
    buttons: Query<&Interaction, (Changed<Interaction>, With<FocusButton>)>,
    mut focus: ResMut<Focus>,
) {
    if *phase.get() != Phase::Playing {
        return;
    }
    let clicked = buttons.iter().any(|i| *i == Interaction::Pressed);
    if keys.just_pressed(KeyCode::Tab) || clicked {
        *focus = focus.next();
    }
}

/// Back at the menu or the results: the icon goes, and the next run builds its own.
fn teardown(mut commands: Commands, mut ui: ResMut<FocusUi>) {
    if let Some(root) = ui.root.take() {
        commands.entity(root).try_despawn();
    }
    ui.parts = None;
}

fn text(font: &Handle<Font>, s: &str, size: f32, color: Color) -> impl Bundle {
    (
        Text::new(s),
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
}

/// The icon in a gold-edged slot with its key above and its name and rule below.
fn build(
    commands: &mut Commands,
    ui: &mut FocusUi,
    font: &Handle<Font>,
    icons: [Option<Handle<Image>>; 3],
) {
    let (mut image, mut label, mut hint) = (
        Entity::PLACEHOLDER,
        Entity::PLACEHOLDER,
        Entity::PLACEHOLDER,
    );
    let root = commands
        .spawn((
            Node {
                position_type: PositionType::Absolute,
                right: Val::Px(16.0),
                bottom: Val::Px(24.0),
                flex_direction: FlexDirection::Column,
                align_items: AlignItems::Center,
                row_gap: Val::Px(2.0),
                ..default()
            },
            GlobalZIndex(Z),
        ))
        .with_children(|col| {
            col.spawn(text(font, "Tab", 14.0, Color::WHITE));
            col.spawn((
                Button,
                FocusButton,
                Node {
                    width: Val::Px(ICON),
                    height: Val::Px(ICON),
                    border: UiRect::all(Val::Px(1.0)),
                    ..default()
                },
                BackgroundColor(FILL),
                BorderColor::all(GOLD_TONE),
            ))
            .with_children(|b| {
                image = b
                    .spawn((
                        Node {
                            width: Val::Percent(100.0),
                            height: Val::Percent(100.0),
                            ..default()
                        },
                        ImageNode {
                            color: Color::NONE,
                            ..default()
                        },
                    ))
                    .id();
            });
            label = col.spawn(text(font, "", 13.0, GOLD_TONE)).id();
            hint = col.spawn(text(font, "", 10.0, HINT)).id();
        })
        .id();
    ui.root = Some(root);
    ui.parts = Some(Parts {
        image,
        label,
        hint,
        icons,
    });
}

/// Build the icon on the first frame of a run, then keep it showing the current focus.
#[allow(clippy::too_many_arguments)]
fn show(
    mut commands: Commands,
    mut ui: ResMut<FocusUi>,
    focus: Res<Focus>,
    font: Option<Res<HudFont>>,
    assets: Option<ResMut<benilla_assets::WorldAssets>>,
    mut images: ResMut<Assets<Image>>,
    mut icons: Query<&mut ImageNode>,
    mut texts: Query<(&mut Text, &mut TextColor)>,
) {
    let ui = &mut *ui;
    let Some(parts) = &ui.parts else {
        if let (Some(font), Some(mut assets)) = (font, assets) {
            let art = Focus::ALL.map(|f| assets.sprite_texture(f.icon(), &mut images));
            // Seated from the next frame on, once its nodes exist.
            build(&mut commands, ui, &font.0, art);
        }
        return;
    };
    let now = *focus;
    if let (Ok(mut img), Some(handle)) = (icons.get_mut(parts.image), &parts.icons[now.index()]) {
        if img.image != *handle {
            img.image = handle.clone();
        }
        if img.color != Color::WHITE {
            img.color = Color::WHITE;
        }
    }
    for (entity, value, color) in [
        (parts.label, now.label(), now.color()),
        (parts.hint, now.hint(), HINT),
    ] {
        if let Ok((mut text, mut tc)) = texts.get_mut(entity) {
            if text.0 != value {
                text.0 = value.to_string();
            }
            if tc.0 != color {
                tc.0 = color;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_cycle_visits_every_focus_and_wraps() {
        let mut f = Focus::default();
        assert_eq!(f, Focus::Nearest);
        let mut seen = vec![f];
        for _ in 0..3 {
            f = f.next();
            seen.push(f);
        }
        assert_eq!(
            seen,
            [
                Focus::Nearest,
                Focus::Lowest,
                Focus::Strongest,
                Focus::Nearest
            ]
        );
    }
}
