//! The run's difficulty, picked at the pick screen: Normal, Heroic (tougher enemies, better
//! rewards) and Hardcore (toughest, no health gems, the best rewards). The enemy multipliers are
//! read where enemies spawn ([`super::enemies`]), the reward multiplier where the run pays out
//! ([`super::profile`]). The pick is kept in [`Profile::difficulty`] and copied into
//! [`Run::difficulty`] when the run starts.

use bevy::prelude::*;
use serde::{Deserialize, Serialize};

use super::chatlog::ChatLine;
use super::hud::{self, OverPayout};
use super::kit;
use super::profile::Profile;
use super::{Phase, Run};
use benilla_app::survivors_host::glue::GlueVisuals;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub(crate) enum Difficulty {
    #[default]
    Normal,
    Heroic,
    Hardcore,
}

impl Difficulty {
    pub const ALL: [Difficulty; 3] = [Self::Normal, Self::Heroic, Self::Hardcore];

    pub fn label(self) -> &'static str {
        match self {
            Self::Normal => "Normal",
            Self::Heroic => "Heroic",
            Self::Hardcore => "Hardcore",
        }
    }

    /// One line under the selector at the pick screen.
    pub fn blurb(self) -> &'static str {
        match self {
            Self::Normal => "Standard enemies and rewards",
            Self::Heroic => "Tougher enemies, x1.5 rewards",
            Self::Hardcore => "No health gems, x2.5 rewards",
        }
    }

    fn color(self) -> Color {
        match self {
            Self::Normal => kit::MUTED,
            Self::Heroic => Color::srgb(1.0, 0.6, 0.15),
            Self::Hardcore => kit::WARN,
        }
    }

    /// Whether enemies drop health gems; Hardcore has none.
    pub fn health_gems(self) -> bool {
        self != Self::Hardcore
    }

    pub fn enemy_hp_mult(self) -> f32 {
        match self {
            Self::Normal => 1.0,
            Self::Heroic => 1.6,
            Self::Hardcore => 2.0,
        }
    }
    pub fn enemy_damage_mult(self) -> f32 {
        match self {
            Self::Normal => 1.0,
            Self::Heroic => 1.3,
            Self::Hardcore => 1.6,
        }
    }
    pub fn enemy_speed_mult(self) -> f32 {
        match self {
            Self::Normal => 1.0,
            Self::Heroic => 1.1,
            Self::Hardcore => 1.15,
        }
    }
    /// Character experience and gold at the payout.
    pub fn reward_mult(self) -> f32 {
        match self {
            Self::Normal => 1.0,
            Self::Heroic => 1.5,
            Self::Hardcore => 2.5,
        }
    }
}

/// A selector button at the pick screen.
#[derive(Component, Clone, Copy, PartialEq, Eq, Debug)]
struct DifficultyAction(Difficulty);
/// The line under the buttons: what the pick means, in the pick's colour.
#[derive(Component)]
struct DifficultyLine;
/// The red skull beside the line, shown for Hardcore.
#[derive(Component)]
struct Skull;

/// Whether this run's start was announced in the chat; reset at the menu.
#[derive(Resource, Default)]
struct Announced(bool);

/// The icon of the Hardcore skull.
pub(super) const SKULL: &str = "Interface\\Icons\\INV_Misc_Bone_HumanSkull_01.blp";
const SKULL_RED: Color = Color::srgb(1.0, 0.2, 0.15);

pub(super) fn plugin(app: &mut App) {
    app.init_resource::<Announced>()
        .add_systems(
            Update,
            (clicks, refresh)
                .chain()
                // After the keys, like the other pick-screen panels.
                .after(hud::keys)
                .before(GlueVisuals),
        )
        .add_systems(Update, announce.run_if(in_state(Phase::Playing)))
        .add_systems(
            Update,
            tag_payout
                .after(hud::refresh_results)
                .run_if(in_state(Phase::GameOver)),
        )
        .add_systems(OnEnter(Phase::Menu), |mut announced: ResMut<Announced>| {
            announced.0 = false;
        });
}

/// The compact selector the pick screen puts left of its Enter button: three kit buttons in a
/// row and, under them, the pick's one-line description with the Hardcore skull. `skull` is the
/// loaded [`SKULL`] icon, if any.
pub(super) fn spawn_control(
    parent: &mut ChildSpawnerCommands,
    fonts: &kit::KitFonts,
    skull: Option<Handle<Image>>,
) {
    let px = Val::Px;
    parent
        .spawn(Node {
            flex_direction: FlexDirection::Column,
            align_items: AlignItems::Center,
            row_gap: px(4.0),
            ..default()
        })
        .with_children(|col| {
            col.spawn(Node {
                column_gap: px(4.0),
                ..default()
            })
            .with_children(|row| {
                for d in Difficulty::ALL {
                    kit::button(
                        row,
                        fonts,
                        kit::KitStyle::Normal,
                        d.label(),
                        None,
                        Node {
                            width: px(88.0),
                            height: px(26.0),
                            ..default()
                        },
                        DifficultyAction(d),
                    );
                }
            });
            col.spawn(Node {
                height: px(14.0),
                align_items: AlignItems::Center,
                column_gap: px(4.0),
                ..default()
            })
            .with_children(|line| {
                let mut image = skull.map(ImageNode::new).unwrap_or_default();
                image.color = SKULL_RED;
                line.spawn((
                    Skull,
                    image,
                    Node {
                        width: px(12.0),
                        height: px(12.0),
                        display: Display::None,
                        ..default()
                    },
                ));
                line.spawn((DifficultyLine, kit::body(fonts, "", 11.0, kit::MUTED)));
            });
        });
}

/// A click on a selector button sets the pick and saves it.
fn clicks(
    actions: Query<(&Interaction, &DifficultyAction), Changed<Interaction>>,
    phase: Res<State<Phase>>,
    mut profile: ResMut<Profile>,
) {
    if *phase.get() != Phase::Menu {
        return;
    }
    let Some(picked) = actions
        .iter()
        .find(|(i, _)| **i == Interaction::Pressed)
        .map(|(_, a)| a.0)
    else {
        return;
    };
    if profile.difficulty != picked {
        profile.difficulty = picked;
        profile.save();
    }
}

/// The picked button is lit, the line and the skull follow the pick.
fn refresh(
    profile: Res<Profile>,
    mut buttons: Query<(&DifficultyAction, &mut kit::KitButton)>,
    mut line: Query<(&mut Text, &mut TextColor), With<DifficultyLine>>,
    mut skull: Query<&mut Node, With<Skull>>,
) {
    let picked = profile.difficulty;
    for (action, mut button) in &mut buttons {
        let on = action.0 == picked;
        if button.selected != on {
            button.selected = on;
        }
    }
    for (mut text, mut color) in &mut line {
        if text.0 != picked.blurb() {
            text.0 = picked.blurb().to_string();
        }
        if color.0 != picked.color() {
            color.0 = picked.color();
        }
    }
    let display = if picked == Difficulty::Hardcore {
        Display::Flex
    } else {
        Display::None
    };
    for mut node in &mut skull {
        if node.display != display {
            node.display = display;
        }
    }
}

/// The first tick of a run: a chat line naming the difficulty.
fn announce(run: Res<Run>, mut announced: ResMut<Announced>, mut chat: MessageWriter<ChatLine>) {
    if announced.0 || run.elapsed <= 0.0 {
        return;
    }
    announced.0 = true;
    let d = run.difficulty;
    chat.write(ChatLine::system(format!(
        "Difficulty: {}. {}.",
        d.label(),
        d.blurb()
    )));
}

/// The results' level line gets the difficulty and its reward multiplier appended. The level line
/// is the payout text that is neither "+n" nor "Bank n"; [`hud::refresh_results`] rewrites it every
/// frame, so the tag goes on again after it.
fn tag_payout(run: Res<Run>, mut lines: Query<&mut Text, With<OverPayout>>) {
    let d = run.difficulty;
    let tag = if d == Difficulty::Normal {
        format!("   {}", d.label())
    } else {
        format!("   {}: rewards x{}", d.label(), hud::num(d.reward_mult()))
    };
    for mut text in &mut lines {
        let s = &text.0;
        if s.is_empty() || s.starts_with('+') || s.starts_with("Bank") || s.ends_with(&tag) {
            continue;
        }
        text.0.push_str(&tag);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn harder_modes_pay_more_and_hit_harder() {
        let [n, h, c] = Difficulty::ALL;
        for f in [
            Difficulty::enemy_hp_mult,
            Difficulty::enemy_damage_mult,
            Difficulty::enemy_speed_mult,
            Difficulty::reward_mult,
        ] {
            assert!(f(n) == 1.0 && f(h) > f(n) && f(c) > f(h));
        }
    }

    #[test]
    fn only_hardcore_drops_no_health() {
        assert!(Difficulty::Normal.health_gems() && Difficulty::Heroic.health_gems());
        assert!(!Difficulty::Hardcore.health_gems());
    }

    #[test]
    fn serializes_by_lowercase_name() {
        #[derive(Serialize, Deserialize, PartialEq, Debug)]
        struct T {
            d: Difficulty,
        }
        let text = toml::to_string(&T {
            d: Difficulty::Hardcore,
        })
        .unwrap();
        assert!(text.contains("hardcore"));
        assert_eq!(
            toml::from_str::<T>(&text).unwrap(),
            T {
                d: Difficulty::Hardcore
            }
        );
    }
}
