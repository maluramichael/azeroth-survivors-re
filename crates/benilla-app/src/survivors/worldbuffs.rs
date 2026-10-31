//! World buffs: a pick of WoW world buffs (Songflower Serenade, Rallying Cry of the Dragonslayer,
//! ...) at the start of a run and every ten minutes, into [`super::Run::buffs`].
//!
//! The offer is a small panel top centre under the clock with three buffs; it does not pause the
//! run. Click one or press **8**, **9** or **0**; after [`PICK_SECS`] the first is taken. A buff
//! is offered once per run. The timer only runs while the run does (pause and level-up stop it).

use bevy::prelude::*;

use super::chatlog::ChatLine;
use super::hud::{HudFont, GOLD_TONE};
use super::mods::StatMods;
use super::talents::Fx;
use super::{Phase, Rng, Run};

/// Seconds of run time between offers.
const INTERVAL: f32 = 600.0;
/// Seconds into the run (or into an interval) before the offer shows, so it clears the loading
/// screen.
const SETTLE: f32 = 0.5;
/// Seconds before the first buff is taken for the player.
const PICK_SECS: f32 = 15.0;
/// Buffs on offer.
const OFFERED: usize = 3;
/// Over the HUD (400) and the focus icon, under the character window (405) and the cards (410).
const Z: i32 = 401;
const TOP: f32 = 80.0;
const ROW_W: f32 = 280.0;
const ROW_H: f32 = 38.0;
const ICON: f32 = 28.0;
const KEYS: [KeyCode; OFFERED] = [KeyCode::Digit8, KeyCode::Digit9, KeyCode::Digit0];
const KEY_LABELS: [&str; OFFERED] = ["8", "9", "0"];

const PANEL_FILL: Color = Color::srgba(0.02, 0.02, 0.03, 0.8);
const ROW_FILL: Color = Color::srgba(0.12, 0.1, 0.05, 0.9);
const ROW_HOT: Color = Color::srgba(0.35, 0.28, 0.1, 0.95);
const EDGE: Color = Color::srgb(0.55, 0.45, 0.25);
const HINT: Color = Color::srgb(0.6, 0.6, 0.6);
const QUESTION: &str = "Interface\\Icons\\INV_Misc_QuestionMark.blp";

/// One world buff and what it adds to the run.
pub(super) struct Buff {
    pub name: &'static str,
    /// A file in `Interface\Icons`, without the folder and the extension.
    pub icon: &'static str,
    pub effects: &'static [(Fx, f32)],
}

impl Buff {
    pub fn mods(&self) -> StatMods {
        let mut m = StatMods::default();
        for &(fx, v) in self.effects {
            fx.apply(&mut m, v);
        }
        m
    }

    /// "+15% damage, +5% crit".
    pub fn describe(&self) -> String {
        self.effects
            .iter()
            .map(|&(fx, v)| fx.describe(v))
            .collect::<Vec<_>>()
            .join(", ")
    }
}

use Fx::*;

/// The pool. Values are the 1.12 buffs' flavour mapped onto the mode's stats.
pub(super) static BUFFS: &[Buff] = &[
    Buff {
        name: "Songflower Serenade",
        icon: "Spell_Holy_MindVision",
        effects: &[(Crit, 0.05), (Might, 0.08)],
    },
    Buff {
        name: "Rallying Cry of the Dragonslayer",
        icon: "INV_Misc_Head_Dragon_01",
        effects: &[(Crit, 0.05), (Might, 0.15)],
    },
    Buff {
        name: "Spirit of Zandalar",
        icon: "Ability_Mount_JungleTiger",
        effects: &[(Speed, 0.15), (Might, 0.10)],
    },
    Buff {
        name: "Warchief's Blessing",
        icon: "Spell_Arcane_TeleportOrgrimmar",
        effects: &[(MaxHp, 0.15), (Haste, 0.10), (Regen, 0.5)],
    },
    Buff {
        name: "Fengus' Ferocity",
        icon: "Ability_Warrior_BattleShout",
        effects: &[(Might, 0.20)],
    },
    Buff {
        name: "Mol'dar's Moxie",
        icon: "Ability_Racial_BloodRage",
        effects: &[(MaxHp, 0.15), (Armor, 0.5)],
    },
    Buff {
        name: "Slip'kik's Savvy",
        icon: "Spell_Nature_Brilliance",
        effects: &[(Crit, 0.03), (Growth, 0.10)],
    },
    Buff {
        name: "Sayge's Dark Fortune of Damage",
        icon: "INV_Misc_Note_01",
        effects: &[(Might, 0.10)],
    },
    Buff {
        name: "Sayge's Dark Fortune of Stamina",
        icon: "INV_Misc_Note_01",
        effects: &[(MaxHp, 0.10), (Regen, 0.3)],
    },
    Buff {
        name: "Sayge's Dark Fortune of Armor",
        icon: "INV_Misc_Note_01",
        effects: &[(Armor, 1.0)],
    },
    Buff {
        name: "Boon of Blackfathom",
        icon: "Spell_Frost_SummonWaterElemental",
        effects: &[(Crit, 0.02), (Might, 0.05), (Area, 0.10)],
    },
    Buff {
        name: "Gordok's Tribute",
        icon: "INV_Misc_Coin_02",
        effects: &[(Greed, 0.25), (Magnet, 0.30)],
    },
    Buff {
        name: "Spirit of Zanza",
        icon: "Spell_Nature_Regeneration",
        effects: &[(Growth, 0.10), (Regen, 0.3), (Speed, 0.05)],
    },
];

/// Up to `n` distinct buffs from [`BUFFS`], none of them in `taken`.
fn roll(rng: &mut Rng, taken: &[usize], n: usize) -> Vec<usize> {
    let mut pool: Vec<usize> = (0..BUFFS.len()).filter(|i| !taken.contains(i)).collect();
    let mut out = Vec::with_capacity(n);
    while out.len() < n && !pool.is_empty() {
        out.push(pool.swap_remove(rng.index(pool.len())));
    }
    out
}

/// The run's offers: when the next one is due, the one on screen, and what is taken.
#[derive(Resource, Default)]
struct Buffs {
    /// Run seconds of the next offer.
    next_at: f32,
    /// Indices into [`BUFFS`] on offer now; empty when none is.
    offer: Vec<usize>,
    /// Seconds before the first one is taken.
    left: f32,
    taken: Vec<usize>,
    root: Option<Entity>,
}

/// One row of the offer: the slot (0 to 2) it takes the key of.
#[derive(Component)]
struct BuffRow(usize);
/// The countdown in the panel's title line.
#[derive(Component)]
struct BuffTimer;

pub(super) fn plugin(app: &mut App) {
    app.init_resource::<Buffs>()
        .add_systems(Update, drive.run_if(in_state(Phase::Playing)))
        .add_systems(OnEnter(Phase::Menu), teardown)
        .add_systems(OnEnter(Phase::GameOver), teardown);
}

/// Back at the menu or the results: the panel goes and the next run starts its offers over.
fn teardown(mut commands: Commands, mut buffs: ResMut<Buffs>) {
    if let Some(root) = buffs.root.take() {
        commands.entity(root).try_despawn();
    }
    *buffs = Buffs::default();
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

/// The panel: a title line with the countdown, then a row per buff.
fn build(
    commands: &mut Commands,
    font: &Handle<Font>,
    offer: &[usize],
    mut assets: Option<ResMut<benilla_assets::WorldAssets>>,
    images: &mut Assets<Image>,
) -> Entity {
    let px = Val::Px;
    commands
        .spawn((
            GlobalZIndex(Z),
            Node {
                position_type: PositionType::Absolute,
                top: px(TOP),
                width: Val::Percent(100.0),
                justify_content: JustifyContent::Center,
                ..default()
            },
        ))
        .with_children(|wrap| {
            wrap.spawn((
                BackgroundColor(PANEL_FILL),
                BorderColor::all(EDGE),
                Node {
                    flex_direction: FlexDirection::Column,
                    row_gap: px(4.0),
                    padding: UiRect::all(px(6.0)),
                    border: UiRect::all(px(1.0)),
                    border_radius: BorderRadius::all(px(4.0)),
                    ..default()
                },
            ))
            .with_children(|col| {
                col.spawn(Node {
                    justify_content: JustifyContent::SpaceBetween,
                    ..default()
                })
                .with_children(|title| {
                    title.spawn(text(font, "World Buffs", 13.0, GOLD_TONE));
                    title.spawn((BuffTimer, text(font, "", 12.0, HINT)));
                });
                for (slot, &i) in offer.iter().enumerate() {
                    let buff = &BUFFS[i];
                    let icon = assets.as_deref_mut().and_then(|a| {
                        a.sprite_texture(&format!("Interface\\Icons\\{}.blp", buff.icon), images)
                            .or_else(|| a.sprite_texture(QUESTION, images))
                    });
                    col.spawn((
                        BuffRow(slot),
                        Button,
                        BackgroundColor(ROW_FILL),
                        BorderColor::all(EDGE),
                        Node {
                            width: px(ROW_W),
                            height: px(ROW_H),
                            align_items: AlignItems::Center,
                            column_gap: px(6.0),
                            padding: UiRect::horizontal(px(6.0)),
                            border: UiRect::all(px(1.0)),
                            ..default()
                        },
                    ))
                    .with_children(|row| {
                        row.spawn(text(font, KEY_LABELS[slot], 16.0, GOLD_TONE));
                        row.spawn((
                            Node {
                                width: px(ICON),
                                height: px(ICON),
                                ..default()
                            },
                            match icon {
                                Some(h) => ImageNode::new(h),
                                None => ImageNode {
                                    color: Color::NONE,
                                    ..default()
                                },
                            },
                        ));
                        row.spawn(Node {
                            flex_direction: FlexDirection::Column,
                            ..default()
                        })
                        .with_children(|lines| {
                            lines.spawn(text(font, buff.name, 12.0, Color::WHITE));
                            lines.spawn(text(font, &buff.describe(), 10.0, HINT));
                        });
                    });
                }
            });
        })
        .id()
}

/// Offer a pick when one is due, count it down and take it by key, click or timeout.
#[allow(clippy::too_many_arguments)]
fn drive(
    mut commands: Commands,
    time: Res<Time>,
    keys: Res<ButtonInput<KeyCode>>,
    mut run: ResMut<Run>,
    mut buffs: ResMut<Buffs>,
    mut rng: ResMut<Rng>,
    font: Option<Res<HudFont>>,
    assets: Option<ResMut<benilla_assets::WorldAssets>>,
    mut images: ResMut<Assets<Image>>,
    mut rows: Query<(&BuffRow, &Interaction, &mut BackgroundColor)>,
    mut timer: Query<&mut Text, With<BuffTimer>>,
    mut chat: MessageWriter<ChatLine>,
) {
    if buffs.offer.is_empty() {
        if run.elapsed < buffs.next_at + SETTLE {
            return;
        }
        let Some(font) = font else { return };
        let offer = roll(&mut rng, &buffs.taken, OFFERED);
        buffs.next_at += INTERVAL;
        if offer.is_empty() {
            return;
        }
        buffs.root = Some(build(&mut commands, &font.0, &offer, assets, &mut images));
        buffs.offer = offer;
        buffs.left = PICK_SECS;
        return;
    }

    buffs.left -= time.delta_secs();
    let mut pick = KEYS
        .iter()
        .position(|&k| keys.just_pressed(k))
        .filter(|&slot| slot < buffs.offer.len());
    for (row, interaction, mut bg) in &mut rows {
        let want = match interaction {
            Interaction::None => ROW_FILL,
            Interaction::Hovered => ROW_HOT,
            Interaction::Pressed => {
                pick = Some(row.0);
                ROW_HOT
            }
        };
        if bg.0 != want {
            bg.0 = want;
        }
    }
    if buffs.left <= 0.0 && pick.is_none() {
        pick = Some(0);
    }
    let Some(slot) = pick else {
        let want = format!("{}s", buffs.left.ceil().max(0.0) as u32);
        for mut t in &mut timer {
            if t.0 != want {
                t.0 = want.clone();
            }
        }
        return;
    };
    let Some(&index) = buffs.offer.get(slot) else {
        return;
    };
    let buff = &BUFFS[index];
    run.buffs.add(&buff.mods());
    buffs.taken.push(index);
    buffs.offer.clear();
    if let Some(root) = buffs.root.take() {
        commands.entity(root).try_despawn();
    }
    chat.write(ChatLine::system(format!("You gain {}.", buff.name)));
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_offer_is_distinct_and_skips_what_is_taken() {
        let mut rng = Rng::default();
        let taken = [0, 1, 2];
        for _ in 0..200 {
            let offer = roll(&mut rng, &taken, OFFERED);
            assert_eq!(offer.len(), OFFERED);
            let mut sorted = offer.clone();
            sorted.sort();
            sorted.dedup();
            assert_eq!(sorted.len(), OFFERED, "distinct");
            assert!(offer.iter().all(|i| !taken.contains(i) && *i < BUFFS.len()));
        }
    }

    #[test]
    fn a_small_pool_offers_what_is_left() {
        let mut rng = Rng::default();
        let taken: Vec<usize> = (0..BUFFS.len() - 2).collect();
        assert_eq!(roll(&mut rng, &taken, OFFERED).len(), 2);
        let all: Vec<usize> = (0..BUFFS.len()).collect();
        assert!(roll(&mut rng, &all, OFFERED).is_empty());
    }

    #[test]
    fn a_buff_adds_its_effects() {
        let songflower = BUFFS[0].mods();
        assert!((songflower.crit - 0.05).abs() < 1e-6);
        assert!((songflower.might - 0.08).abs() < 1e-6);
        assert_eq!(songflower.max_hp, 0.0);
        assert_eq!(BUFFS[0].describe(), "+5% crit, +8% damage");
    }

    #[test]
    fn names_are_unique_and_every_buff_does_something() {
        let mut names: Vec<_> = BUFFS.iter().map(|b| b.name).collect();
        names.sort();
        names.dedup();
        assert_eq!(names.len(), BUFFS.len());
        assert!(BUFFS.iter().all(|b| b.mods() != StatMods::default()));
    }
}
