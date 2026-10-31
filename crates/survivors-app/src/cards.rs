//! The level-up picker, as a survivors game deals it: three tall cards that flip up out of the
//! deck one after another, tilt toward the cursor, lift under it and burst on the pick. Each card
//! wears its rarity in WoW's item-quality colours, so what is better reads at a glance: a fresh
//! spell is Rare, a rank up Uncommon, a rank that adds a target or reaches the top Epic.

use bevy::prelude::*;
use bevy::ui::RelativeCursorPosition;

use super::data::{self, Kind};
use super::achievements::RunPerks;
use super::hud::{self, Choice, Offer, SpellIcon};
use super::kit::{self, KitButton, KitFonts, KitStyle};
use super::{Phase, Rng, Run};

/// WoW's item-quality colours (`ITEM_QUALITY_COLORS`).
pub(super) const COMMON: Color = Color::srgb(1.0, 1.0, 1.0);
pub(super) const UNCOMMON: Color = Color::srgb(0.118, 1.0, 0.0);
pub(super) const RARE: Color = Color::srgb(0.0, 0.439, 0.867);
pub(super) const EPIC: Color = Color::srgb(0.639, 0.208, 0.933);
pub(super) const LEGENDARY: Color = Color::srgb(1.0, 0.502, 0.0);

const CARD_W: f32 = 250.0;
/// A hand of four (the perk) gets narrower cards so the row still fits a 1024-wide screen.
const CARD_W_FOUR: f32 = 220.0;
/// Gold per level for skipping a hand.
const SKIP_GOLD_PER_LEVEL: u32 = 10;
const CARD_H: f32 = 370.0;
/// Seconds between one card landing and the next.
const DEAL_STAGGER: f32 = 0.09;
const DEAL_SECS: f32 = 0.38;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(super) enum Rarity {
    Common,
    Uncommon,
    Rare,
    Epic,
    Legendary,
}

impl Rarity {
    pub(super) fn color(self) -> Color {
        match self {
            Self::Common => COMMON,
            Self::Uncommon => UNCOMMON,
            Self::Rare => RARE,
            Self::Epic => EPIC,
            Self::Legendary => LEGENDARY,
        }
    }

    fn label(self) -> &'static str {
        match self {
            Self::Common => "Common",
            Self::Uncommon => "Uncommon",
            Self::Rare => "Rare",
            Self::Epic => "Epic",
            Self::Legendary => "Legendary",
        }
    }
}

/// A spell's rough damage per second at rank 1 against a crowd: per-cast damage times the
/// enemies it can reach, over its cooldown. Only for ranking a class's own spells against each
/// other; the defensive kinds have none and sit in the middle.
fn power(def: &data::AbilityDef) -> Option<f32> {
    let area = |r: f32| 1.0 + r * 0.6;
    let reach = match def.kind {
        Kind::Bolt | Kind::Strike => def.count as f32 + def.radius * 0.4,
        Kind::Melee | Kind::Orbit => def.count as f32,
        Kind::Nova | Kind::Cone | Kind::Ground | Kind::GroundSelf | Kind::Aura => area(def.radius),
        Kind::Chain => def.count as f32 + 1.0,
        Kind::Dot => def.count as f32,
        _ => return None,
    };
    let per = if def.damage > 0.0 {
        def.damage
    } else {
        def.dot * def.duration.max(1.0)
    };
    let ticks = if matches!(def.kind, Kind::Ground | Kind::GroundSelf) && def.duration > 0.0 {
        def.duration
    } else {
        1.0
    };
    Some(per * ticks * reach / def.cooldown.max(0.2))
}

/// Where a new spell stands among its class's: the strongest third Epic, the weakest third
/// Uncommon, the rest (and the defensive spells) Rare.
fn spell_tier(def: &data::AbilityDef, run: &Run) -> Rarity {
    let Some(mine) = power(def) else {
        return Rarity::Rare;
    };
    let mut all: Vec<f32> = run
        .class
        .pool
        .iter()
        .filter_map(|k| power(data::ability(k)))
        .collect();
    all.sort_by(f32::total_cmp);
    let rank = all.iter().filter(|&&p| p < mine).count() as f32 / all.len().max(1) as f32;
    if rank >= 0.66 {
        Rarity::Epic
    } else if rank < 0.33 {
        Rarity::Uncommon
    } else {
        Rarity::Rare
    }
}

/// What a choice is worth, by what it brings: a top rank is Legendary, a rank that adds a target
/// Epic, a new spell by its power among the class's ([`spell_tier`]), a plain rank Uncommon.
pub(super) fn rarity(choice: Choice, run: &Run) -> Rarity {
    match choice {
        Choice::NewAbility(def) => spell_tier(def, run),
        Choice::Upgrade(k) => {
            let o = &run.abilities[k];
            let next = o.level + 1;
            let targets = [3, 5, 7].contains(&next)
                && matches!(
                    o.def.kind,
                    Kind::Bolt | Kind::Strike | Kind::Melee | Kind::Dot | Kind::Chain | Kind::Orbit
                );
            if next == data::MAX_ABILITY_LEVEL {
                Rarity::Legendary
            } else if targets {
                Rarity::Epic
            } else {
                Rarity::Uncommon
            }
        }
        Choice::NewPassive(_) => Rarity::Uncommon,
        Choice::UpPassive(k) => match run.passives[k].1 + 1 {
            n if n == data::MAX_PASSIVE_LEVEL => Rarity::Epic,
            n if n + 1 == data::MAX_PASSIVE_LEVEL => Rarity::Rare,
            2 => Rarity::Common,
            _ => Rarity::Uncommon,
        },
        Choice::Evolve(_) => Rarity::Legendary,
        Choice::Heal => Rarity::Common,
    }
}

/// One card's motion: dealt in, tilted to the cursor, lifted under it, burst on the pick.
#[derive(Component)]
pub(super) struct Card {
    index: usize,
    /// Seconds since this card's deal began; negative while it waits its turn.
    t: f32,
    hover: f32,
    tilt: Vec2,
    picked: Option<f32>,
    rarity: Rarity,
}

/// The shine that sweeps across a card as it lands.
#[derive(Component)]
pub(super) struct CardShine(usize);

/// The radiant burst behind the row, turning slowly.
#[derive(Component)]
pub(super) struct Rays;

/// The pending pick: the card bursts, then the choice applies.
#[derive(Resource, Default)]
pub(super) struct PendingPick(Option<(usize, f32)>);

const PICK_SECS: f32 = 0.28;

/// The small buttons around the cards: banish the card above, deal a new hand, skip the hand.
#[derive(Component, Clone, Copy, PartialEq, Eq, Debug)]
pub(super) enum LevelUpAction {
    /// Banish the card at this index and deal a new hand.
    Banish(usize),
    Reroll,
    Skip,
}

/// Upper case with a space between the letters, the closest thing to letter-spacing the text
/// has: "UNCOMMON" becomes "U N C O M M O N".
fn spaced(s: &str) -> String {
    s.to_uppercase()
        .chars()
        .map(String::from)
        .collect::<Vec<_>>()
        .join(" ")
}

fn font_at(font: &Handle<Font>, size: f32) -> TextFont {
    TextFont {
        font: font.clone(),
        font_size: size,
        ..default()
    }
}

/// A centred, wrapping line of kit text as wide as its parent.
fn wrapped(font: &Handle<Font>, s: impl Into<String>, size: f32, color: Color) -> impl Bundle {
    (
        kit::text(font, s, size, color),
        TextLayout::new_with_justify(Justify::Center),
        Node {
            width: Val::Percent(100.0),
            ..default()
        },
    )
}

/// One effect line of a card: "Damage 20 » 26" shows the new value in the bonus green, any other
/// line is plain cream.
fn effect_line(parent: &mut ChildSpawnerCommands, font: &Handle<Font>, line: &str) {
    const SIZE: f32 = 15.0;
    let Some((before, after)) = line.split_once(" » ") else {
        parent.spawn(wrapped(font, line, SIZE, kit::CREAM));
        return;
    };
    parent
        .spawn((
            Text::new(format!("{before} » ")),
            font_at(font, SIZE),
            TextColor(kit::CREAM),
            TextShadow {
                offset: Vec2::splat(1.0),
                color: Color::srgba(0.0, 0.0, 0.0, 0.9),
            },
            TextLayout::new_with_justify(Justify::Center),
            Node {
                width: Val::Percent(100.0),
                ..default()
            },
        ))
        .with_children(|t| {
            t.spawn((
                TextSpan::new(after),
                font_at(font, SIZE),
                TextColor(kit::BONUS),
            ));
        });
}

/// The cards, rebuilt for each offer.
#[allow(clippy::too_many_arguments)]
pub(super) fn deal_cards(
    mut commands: Commands,
    offer: Res<Offer>,
    run: Res<Run>,
    spells: Option<Res<benilla_app::survivors_host::ui_action::Spells>>,
    fonts: Res<KitFonts>,
    perks: Res<RunPerks>,
    rows: Query<(Entity, &ChildOf), With<hud::OfferRow>>,
    mut siblings: Query<(&ChildOf, &mut Node, &Text, Has<hud::LevelUpSub>), Without<hud::OfferRow>>,
) {
    if !offer.is_changed() {
        return;
    }
    let Ok((row, row_parent)) = rows.single() else {
        return;
    };
    // The overlay's own plain title and subtitle give way to the header built here.
    for (parent, mut node, text, is_sub) in &mut siblings {
        if parent.parent() == row_parent.parent() && (is_sub || text.0 == "LEVEL UP") {
            node.display = Display::None;
        }
    }
    commands.entity(row).despawn_related::<Children>();
    commands.entity(row).insert(Node {
        flex_direction: FlexDirection::Column,
        align_items: AlignItems::Center,
        row_gap: Val::Px(14.0),
        margin: UiRect::new(Val::ZERO, Val::ZERO, Val::Px(4.0), Val::Px(4.0)),
        ..default()
    });
    let kf = &*fonts;
    let f = &kf.body;
    // The level this offer is for: several can wait behind one another.
    let level = hud::hand_level(&run);
    let n = offer.len();
    let (card_w, gap) = if n >= 4 { (CARD_W_FOUR, 16.0) } else { (CARD_W, 22.0) };
    let owned: Vec<u32> = run.abilities.iter().map(|o| o.def.spell).collect();
    commands.entity(row).with_children(|row| {
        // The header: a gold pill, the Morpheus title and a line of help.
        row.spawn(Node {
            flex_direction: FlexDirection::Column,
            align_items: AlignItems::Center,
            row_gap: Val::Px(4.0),
            ..default()
        })
        .with_children(|h| {
            h.spawn((
                Node {
                    padding: UiRect::axes(Val::Px(16.0), Val::Px(3.0)),
                    border: UiRect::all(Val::Px(1.0)),
                    border_radius: BorderRadius::all(Val::Px(12.0)),
                    ..default()
                },
                BackgroundColor(Color::srgba(0.05, 0.04, 0.025, 0.9)),
                BorderColor::all(kit::TITLE.with_alpha(0.75)),
            ))
            .with_children(|p| {
                p.spawn(kit::body(kf, format!("LEVEL {level}"), 13.0, kit::TITLE));
            });
            h.spawn((
                Text::new("Level Up!"),
                font_at(&kf.title, 64.0),
                TextColor(kit::TITLE),
                TextShadow {
                    offset: Vec2::splat(3.0),
                    color: Color::srgba(0.0, 0.0, 0.0, 0.9),
                },
                TextLayout::new_with_justify(Justify::Center),
            ));
            h.spawn(kit::body(
                kf,
                "Choose a power. Its rarity sets how strong it is.",
                16.0,
                kit::CREAM,
            ));
        });

        // The cards.
        row.spawn(Node {
            column_gap: Val::Px(gap),
            // Room for a hovered card's lift.
            margin: UiRect::new(Val::ZERO, Val::ZERO, Val::Px(14.0), Val::Px(4.0)),
            ..default()
        })
        .with_children(|cards| {
            for (i, &choice) in offer.choices().iter().enumerate() {
                let card = hud::describe_card(choice, &run, spells.as_deref());
                let rarity = rarity(choice, &run);
                let rc = rarity.color();
                let (chip, chip_color) = match choice {
                    Choice::NewAbility(_) | Choice::NewPassive(_) => (
                        "NEW".to_string(),
                        if rarity == Rarity::Common {
                            kit::MUTED
                        } else {
                            kit::BONUS
                        },
                    ),
                    Choice::Upgrade(k) => (format!("RANK {}", run.abilities[k].level + 1), rc),
                    Choice::UpPassive(k) => (format!("RANK {}", run.passives[k].1 + 1), rc),
                    Choice::Evolve(_) => ("EVOLVE".to_string(), kit::QUALITY[5]),
                    Choice::Heal => ("RESTORE".to_string(), kit::MUTED),
                };
                let footer = match choice {
                    Choice::NewAbility(_) => format!(
                        "Takes a spell slot, {}/{}",
                        run.abilities.len() + 1,
                        data::MAX_ABILITIES
                    ),
                    Choice::NewPassive(_) => format!(
                        "Takes a passive slot, {}/{}",
                        run.passives.len() + 1,
                        data::MAX_PASSIVES
                    ),
                    Choice::Upgrade(_) => "Improves a spell you know".to_string(),
                    Choice::UpPassive(_) => "Improves a passive you know".to_string(),
                    Choice::Evolve(_) => "Replaces a maxed spell".to_string(),
                    Choice::Heal => "Restores all health".to_string(),
                };
                let dark = Color::srgb(0.04, 0.035, 0.03);
                cards
                    .spawn((
                        Card {
                            index: i,
                            t: -(i as f32) * DEAL_STAGGER,
                            hover: 0.0,
                            tilt: Vec2::ZERO,
                            picked: None,
                            rarity,
                        },
                        Button,
                        Interaction::default(),
                        RelativeCursorPosition::default(),
                        Node {
                            width: Val::Px(card_w),
                            height: Val::Px(CARD_H),
                            flex_direction: FlexDirection::Column,
                            align_items: AlignItems::Center,
                            padding: UiRect::new(
                                Val::Px(14.0),
                                Val::Px(14.0),
                                Val::Px(12.0),
                                Val::Px(10.0),
                            ),
                            row_gap: Val::Px(6.0),
                            border: UiRect::all(Val::Px(2.0)),
                            border_radius: BorderRadius::all(Val::Px(10.0)),
                            ..default()
                        },
                        // Near-black, lit from the top in the rarity's colour.
                        BackgroundGradient::from(LinearGradient::to_bottom(vec![
                            ColorStop::percent(dark.mix(&rc, 0.18).with_alpha(0.97), 0.0),
                            ColorStop::percent(dark.with_alpha(0.97), 100.0),
                        ])),
                        BorderColor::all(rc),
                        BoxShadow::new(
                            rc.with_alpha(0.0),
                            Val::Px(0.0),
                            Val::Px(0.0),
                            Val::Px(0.0),
                            Val::Px(18.0),
                        ),
                        UiTransform {
                            translation: Val2::px(0.0, 260.0),
                            scale: Vec2::splat(0.6),
                            rotation: Rot2::degrees(-14.0),
                        },
                        Visibility::Hidden,
                    ))
                    .with_children(|c| {
                        // The key and the kind of pick on one line along the top.
                        c.spawn(Node {
                            width: Val::Percent(100.0),
                            justify_content: JustifyContent::SpaceBetween,
                            align_items: AlignItems::Center,
                            ..default()
                        })
                        .with_children(|top| {
                            top.spawn((
                                Node {
                                    width: Val::Px(22.0),
                                    height: Val::Px(22.0),
                                    justify_content: JustifyContent::Center,
                                    align_items: AlignItems::Center,
                                    border: UiRect::all(Val::Px(1.0)),
                                    border_radius: BorderRadius::all(Val::Px(11.0)),
                                    ..default()
                                },
                                BackgroundColor(Color::srgba(0.0, 0.0, 0.0, 0.65)),
                                BorderColor::all(kit::INSET_EDGE),
                            ))
                            .with_children(|k| {
                                k.spawn(kit::body(kf, format!("{}", i + 1), 13.0, kit::CREAM));
                            });
                            top.spawn((
                                Node {
                                    padding: UiRect::axes(Val::Px(8.0), Val::Px(2.0)),
                                    border: UiRect::all(Val::Px(1.0)),
                                    border_radius: BorderRadius::all(Val::Px(9.0)),
                                    ..default()
                                },
                                BackgroundColor(Color::srgba(0.0, 0.0, 0.0, 0.55)),
                                BorderColor::all(chip_color.with_alpha(0.85)),
                            ))
                            .with_children(|ch| {
                                ch.spawn(kit::body(kf, chip, 11.0, chip_color));
                            });
                        });
                        // The icon in a dark socket.
                        c.spawn((
                            Node {
                                width: Val::Px(72.0),
                                height: Val::Px(72.0),
                                margin: UiRect::vertical(Val::Px(2.0)),
                                border: UiRect::all(Val::Px(2.0)),
                                border_radius: BorderRadius::all(Val::Px(6.0)),
                                ..default()
                            },
                            BorderColor::all(Color::srgb(0.02, 0.02, 0.02)),
                            BoxShadow::new(
                                Color::srgba(0.0, 0.0, 0.0, 0.6),
                                Val::Px(0.0),
                                Val::Px(3.0),
                                Val::Px(0.0),
                                Val::Px(8.0),
                            ),
                        ))
                        .with_children(|s| {
                            s.spawn((
                                Node {
                                    width: Val::Percent(100.0),
                                    height: Val::Percent(100.0),
                                    border_radius: BorderRadius::all(Val::Px(4.0)),
                                    ..default()
                                },
                                ImageNode {
                                    color: Color::srgba(0.0, 0.0, 0.0, 0.6),
                                    ..default()
                                },
                                SpellIcon(card.spell),
                            ));
                        });
                        c.spawn(wrapped(f, spaced(rarity.label()), 12.0, rc));
                        c.spawn(wrapped(f, card.name, 22.0, rc));
                        c.spawn(wrapped(f, card.tag, 13.0, kit::MUTED));
                        c.spawn((
                            Node {
                                width: Val::Percent(80.0),
                                height: Val::Px(1.0),
                                margin: UiRect::vertical(Val::Px(2.0)),
                                ..default()
                            },
                            BackgroundGradient::from(LinearGradient::to_right(vec![
                                ColorStop::percent(rc.with_alpha(0.0), 0.0),
                                ColorStop::percent(rc.with_alpha(0.8), 50.0),
                                ColorStop::percent(rc.with_alpha(0.0), 100.0),
                            ])),
                        ));
                        // The effect lines take the room left over.
                        c.spawn(Node {
                            width: Val::Percent(100.0),
                            flex_grow: 1.0,
                            flex_direction: FlexDirection::Column,
                            align_items: AlignItems::Center,
                            row_gap: Val::Px(3.0),
                            ..default()
                        })
                        .with_children(|d| {
                            for line in card.desc.lines().filter(|l| !l.trim().is_empty()) {
                                effect_line(d, f, line);
                            }
                        });
                        // The footer, pinned to the bottom above a faint rule.
                        c.spawn(Node {
                            width: Val::Percent(100.0),
                            flex_direction: FlexDirection::Column,
                            align_items: AlignItems::Center,
                            row_gap: Val::Px(6.0),
                            ..default()
                        })
                        .with_children(|b| {
                            b.spawn((
                                Node {
                                    width: Val::Percent(100.0),
                                    height: Val::Px(1.0),
                                    ..default()
                                },
                                BackgroundColor(Color::srgba(1.0, 1.0, 1.0, 0.08)),
                            ));
                            b.spawn(wrapped(f, footer, 12.0, kit::MUTED));
                        });
                        // The landing shine, a bright band that crosses once. Clipped to the
                        // card, or it comes to rest beside it.
                        c.spawn((
                            Node {
                                position_type: PositionType::Absolute,
                                left: Val::Px(0.0),
                                right: Val::Px(0.0),
                                top: Val::Px(0.0),
                                bottom: Val::Px(0.0),
                                overflow: Overflow::clip(),
                                border_radius: BorderRadius::all(Val::Px(8.0)),
                                ..default()
                            },
                            bevy::ui::FocusPolicy::Pass,
                        ))
                        .with_children(|clip| {
                            clip.spawn((
                                CardShine(i),
                                Node {
                                    position_type: PositionType::Absolute,
                                    top: Val::Px(-40.0),
                                    bottom: Val::Px(-40.0),
                                    width: Val::Px(70.0),
                                    left: Val::Px(-120.0),
                                    ..default()
                                },
                                BackgroundGradient::from(LinearGradient::to_right(vec![
                                    ColorStop::percent(Color::srgba(1.0, 1.0, 1.0, 0.0), 0.0),
                                    ColorStop::percent(Color::srgba(1.0, 0.97, 0.85, 0.28), 50.0),
                                    ColorStop::percent(Color::srgba(1.0, 1.0, 1.0, 0.0), 100.0),
                                ])),
                                UiTransform::from_rotation(Rot2::degrees(18.0)),
                                bevy::ui::FocusPolicy::Pass,
                            ));
                        });
                    });
            }
        });

        // A banish button under each card, in a row that lines up with the cards above.
        row.spawn(Node {
            column_gap: Val::Px(gap),
            margin: UiRect::top(Val::Px(-6.0)),
            ..default()
        })
        .with_children(|banish| {
            // Whether a card can be banished is kept up to date by `choose_card`.
            for i in 0..n {
                kit::button(
                    banish,
                    kf,
                    KitStyle::Normal,
                    &format!("Banish ({})", perks.banishes),
                    None,
                    Node {
                        width: Val::Px(card_w),
                        height: Val::Px(26.0),
                        ..default()
                    },
                    LevelUpAction::Banish(i),
                );
            }
        });

        // The hero's spell slots: the ones taken show their icon, the free ones stay dark.
        row.spawn(Node {
            align_items: AlignItems::Center,
            column_gap: Val::Px(6.0),
            ..default()
        })
        .with_children(|r| {
            r.spawn((
                kit::label(kf, "Spells"),
                Node {
                    margin: UiRect::right(Val::Px(6.0)),
                    ..default()
                },
            ));
            for slot in 0..data::MAX_ABILITIES {
                let node = Node {
                    width: Val::Px(28.0),
                    height: Val::Px(28.0),
                    ..default()
                };
                match owned.get(slot) {
                    Some(&spell) => {
                        r.spawn(kit::inset(node)).with_children(|s| {
                            s.spawn((
                                Node {
                                    width: Val::Percent(100.0),
                                    height: Val::Percent(100.0),
                                    border_radius: BorderRadius::all(Val::Px(2.0)),
                                    ..default()
                                },
                                ImageNode {
                                    color: Color::srgba(0.0, 0.0, 0.0, 0.6),
                                    ..default()
                                },
                                SpellIcon(spell),
                            ));
                        });
                    }
                    None => {
                        r.spawn(kit::inset(node));
                    }
                }
            }
        });

        // Deal a new hand, or give the level up for gold.
        row.spawn(Node {
            column_gap: Val::Px(16.0),
            ..default()
        })
        .with_children(|bar| {
            let node = Node {
                min_width: Val::Px(150.0),
                height: Val::Px(30.0),
                ..default()
            };
            kit::button(
                bar,
                kf,
                KitStyle::Normal,
                &format!("Reroll  {}  [R]", perks.rerolls),
                None,
                node.clone(),
                LevelUpAction::Reroll,
            );
            if perks.skip {
                kit::button(
                    bar,
                    kf,
                    KitStyle::Normal,
                    &format!("Skip  +{} gold  [X]", skip_gold(&run)),
                    None,
                    node,
                    LevelUpAction::Skip,
                );
            }
        });
    });
}

/// The gold a skipped hand pays: more the later the level.
fn skip_gold(run: &Run) -> u32 {
    SKIP_GOLD_PER_LEVEL * hud::hand_level(run)
}

fn ease_out_back(t: f32) -> f32 {
    let c1 = 1.70158;
    let c3 = c1 + 1.0;
    1.0 + c3 * (t - 1.0).powi(3) + c1 * (t - 1.0).powi(2)
}

/// Deal, tilt, lift and burst: real time, since the game clock is paused under the picker.
#[allow(clippy::type_complexity, clippy::too_many_arguments)]
pub(super) fn animate_cards(
    time: Res<Time<Real>>,
    phase: Res<State<Phase>>,
    mut cards: Query<(
        &mut Card,
        &Interaction,
        &RelativeCursorPosition,
        &mut UiTransform,
        &mut BoxShadow,
        &mut Visibility,
    )>,
    mut shines: Query<(&CardShine, &mut Node)>,
    mut rays: Query<&mut UiTransform, (With<Rays>, Without<Card>)>,
    mut pending: ResMut<PendingPick>,
    mut run: ResMut<Run>,
    mut offer: ResMut<Offer>,
    mut rng: ResMut<Rng>,
    mut next: ResMut<NextState<Phase>>,
) {
    if *phase.get() != Phase::LevelUp {
        pending.0 = None;
        return;
    }
    let dt = time.delta_secs().min(0.05);
    let now = time.elapsed_secs();
    for mut r in &mut rays {
        r.rotation = Rot2::radians(now * 0.12);
        r.scale = Vec2::splat(1.0 + 0.03 * (now * 1.7).sin());
    }
    let chosen = pending.0.map(|(i, _)| i);
    let mut shine_at = [None::<f32>; 4];
    for (mut card, interaction, cursor, mut tf, mut shadow, mut vis) in &mut cards {
        let card = &mut *card;
        card.t += dt;
        if card.t < 0.0 {
            *vis = Visibility::Hidden;
            continue;
        }
        *vis = Visibility::Inherited;
        let deal = (card.t / DEAL_SECS).clamp(0.0, 1.0);
        let e = ease_out_back(deal);
        let hot = *interaction != Interaction::None && chosen.is_none() && deal >= 1.0;
        let k = 1.0 - (-dt * 14.0).exp();
        let hv = card.hover;
        card.hover = hv + ((if hot { 1.0 } else { 0.0 }) - hv) * k;
        let want_tilt = if hot {
            cursor
                .normalized
                .unwrap_or_default()
                .clamp(Vec2::splat(-0.5), Vec2::splat(0.5))
        } else {
            Vec2::ZERO
        };
        let tl = card.tilt;
        card.tilt = tl + (want_tilt - tl) * k;
        let h = card.hover;
        // The pick: the chosen card swells and rises, the others drop away.
        let (pick_scale, pick_y, pick_fade) = match (chosen, card.picked) {
            (Some(c), _) if c == card.index => {
                let p = card.picked.get_or_insert(0.0);
                *p += dt;
                let u = (*p / PICK_SECS).clamp(0.0, 1.0);
                (1.0 + 0.18 * u, -30.0 * u, 0.0)
            }
            (Some(_), _) => {
                let p = card.picked.get_or_insert(0.0);
                *p += dt;
                let u = (*p / PICK_SECS).clamp(0.0, 1.0);
                (1.0 - 0.15 * u, 140.0 * u * u, u)
            }
            _ => (1.0, 0.0, 0.0),
        };
        // A perspective tilt, faked: the card turns about the cursor's side and leans with it.
        let lean = card.tilt.x * 7.0;
        let squash = 1.0 - card.tilt.x.abs() * 0.08;
        let bob = (now * 2.2 + card.index as f32 * 1.3).sin() * 1.5 * (1.0 - h);
        tf.translation = Val2::px(
            0.0,
            (1.0 - e) * 260.0 - 8.0 * h + card.tilt.y * 8.0 + bob + pick_y,
        );
        let s = (0.6 + 0.4 * e) * (1.0 + 0.06 * h) * pick_scale;
        tf.scale = Vec2::new(s * squash, s * (1.0 + card.tilt.y.abs() * 0.02));
        tf.rotation = Rot2::degrees((1.0 - e) * -14.0 + lean);
        let rc = card.rarity.color();
        if let Some(sh) = shadow.0.first_mut() {
            sh.color = rc.with_alpha((0.35 + 0.45 * h) * (1.0 - pick_fade));
            sh.blur_radius = Val::Px(18.0 + 16.0 * h);
            sh.spread_radius = Val::Px(2.0 * h);
            sh.y_offset = Val::Px(0.0);
        }
        if pick_fade >= 1.0 {
            *vis = Visibility::Hidden;
        }
        if card.index < shine_at.len() {
            shine_at[card.index] = Some(card.t);
        }
    }
    for (shine, mut node) in &mut shines {
        let Some(t) = shine_at.get(shine.0).copied().flatten() else {
            continue;
        };
        let u = ((t - DEAL_SECS * 0.6) / 0.5).clamp(0.0, 1.0);
        node.left = Val::Px(-120.0 + u * (CARD_W + 200.0));
    }
    if let Some((i, t)) = pending.0.as_mut() {
        *t += dt;
        if *t >= PICK_SECS {
            let i = *i;
            pending.0 = None;
            hud::pick(i, &mut run, &mut offer, &mut rng, &mut next);
        }
    }
}

/// A click on a card, or 1/2/3/4: start its burst; the choice applies when it finishes. Also the
/// banish, reroll and skip buttons (R and X for the last two).
#[allow(clippy::too_many_arguments)]
pub(super) fn choose_card(
    phase: Res<State<Phase>>,
    keys: Res<ButtonInput<KeyCode>>,
    cards: Query<(&Card, &Interaction), Changed<Interaction>>,
    mut buttons: Query<(&LevelUpAction, Ref<Interaction>, &mut KitButton)>,
    mut offer: ResMut<Offer>,
    mut pending: ResMut<PendingPick>,
    mut run: ResMut<Run>,
    mut perks: ResMut<RunPerks>,
    mut rng: ResMut<Rng>,
    mut next: ResMut<NextState<Phase>>,
) {
    if *phase.get() != Phase::LevelUp {
        return;
    }
    // Grey out what cannot be used right now.
    let mut pressed = None;
    for (action, interaction, mut state) in &mut buttons {
        let off = match *action {
            LevelUpAction::Banish(i) => {
                perks.banishes == 0
                    || offer
                        .choices()
                        .get(i)
                        .and_then(|&c| hud::choice_key(c, &run))
                        .is_none()
            }
            LevelUpAction::Reroll => perks.rerolls == 0,
            LevelUpAction::Skip => !perks.skip,
        };
        if state.disabled != off {
            state.disabled = off;
        }
        if !off && interaction.is_changed() && *interaction == Interaction::Pressed {
            pressed = Some(*action);
        }
    }
    if pending.0.is_some() {
        return;
    }
    if keys.just_pressed(KeyCode::KeyR) && perks.rerolls > 0 {
        pressed = Some(LevelUpAction::Reroll);
    }
    if keys.just_pressed(KeyCode::KeyX) && perks.skip {
        pressed = Some(LevelUpAction::Skip);
    }
    if let Some(action) = pressed {
        let first_hand = perks.first_hand;
        match action {
            LevelUpAction::Reroll => {
                perks.rerolls -= 1;
                offer.reroll(&run, &mut rng, first_hand);
            }
            LevelUpAction::Banish(i) => {
                let key = offer
                    .choices()
                    .get(i)
                    .and_then(|&c| hud::choice_key(c, &run));
                if let (Some(key), true) = (key, perks.banishes > 0) {
                    perks.banishes -= 1;
                    offer.banished.push(key);
                    offer.reroll(&run, &mut rng, first_hand);
                }
            }
            LevelUpAction::Skip => {
                let gold = skip_gold(&run);
                run.gold += gold;
                run.banner = Some((format!("Skipped, +{gold} gold"), 2.5));
                run.version += 1;
                hud::advance(&mut run, &mut offer, &mut rng, &mut next);
            }
        }
        return;
    }
    let mut pick = cards
        .iter()
        .find(|(c, i)| **i == Interaction::Pressed && c.t >= DEAL_SECS)
        .map(|(c, _)| c.index);
    for (i, key) in [
        KeyCode::Digit1,
        KeyCode::Digit2,
        KeyCode::Digit3,
        KeyCode::Digit4,
    ]
    .into_iter()
    .enumerate()
    {
        if keys.just_pressed(key) {
            pick = Some(i);
        }
    }
    if let Some(i) = pick.filter(|&i| i < offer.len()) {
        pending.0 = Some((i, 0.0));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Owned;

    fn run_at(level: u32) -> Run {
        Run {
            level,
            pending_levels: 1,
            ..Run::default()
        }
    }

    #[test]
    fn only_the_first_hand_is_bigger() {
        assert_eq!(hud::hand_size(&run_at(2), 4), 4);
        assert_eq!(hud::hand_size(&run_at(2), 3), 3);
        assert_eq!(hud::hand_size(&run_at(3), 4), 3);
        assert_eq!(hud::hand_size(&run_at(30), 4), 3);
        // Two levels waiting: the second hand is for level 3, so it holds three.
        let waiting = Run {
            level: 3,
            pending_levels: 2,
            ..Run::default()
        };
        assert_eq!(hud::hand_size(&waiting, 4), 4);
        let second = Run {
            level: 3,
            pending_levels: 1,
            ..Run::default()
        };
        assert_eq!(hud::hand_size(&second, 4), 3);
    }

    #[test]
    fn a_hand_holds_the_size_asked_for() {
        let run = run_at(2);
        let mut rng = Rng::default();
        assert_eq!(hud::choices_for(&run, &mut rng, &[], 4).len(), 4);
        assert_eq!(hud::choices_for(&run, &mut rng, &[], 3).len(), 3);
    }

    #[test]
    fn a_banished_choice_never_comes_back() {
        let run = run_at(2);
        let mut rng = Rng::default();
        let ability = run.class.pool[0];
        let banished = [ability, data::PASSIVES[0].key];
        for _ in 0..300 {
            for c in hud::choices_for(&run, &mut rng, &banished, 4) {
                let key = hud::choice_key(c, &run);
                assert!(key.is_none_or(|k| !banished.contains(&k)), "{key:?} came back");
            }
        }
    }

    #[test]
    fn evolutions_and_the_heal_cannot_be_banished() {
        let run = run_at(2);
        let evo = data::EVOLUTIONS.iter().next().expect("an evolution");
        assert!(hud::choice_key(Choice::Evolve(evo), &run).is_none());
        assert!(hud::choice_key(Choice::Heal, &run).is_none());
        assert_eq!(
            hud::choice_key(Choice::NewAbility(data::ability(run.class.pool[0])), &run),
            Some(run.class.pool[0])
        );
    }

    #[test]
    fn everything_banished_leaves_the_heal() {
        let run = run_at(2);
        let mut rng = Rng::default();
        let mut banished: Vec<&'static str> = run.class.pool.to_vec();
        banished.extend(data::PASSIVES.iter().map(|p| p.key));
        let hand = hud::choices_for(&run, &mut rng, &banished, 3);
        assert!(matches!(hand.as_slice(), [Choice::Heal]));
    }

    /// `n` of the warrior's spells at `level`, none of them the source of an evolution, so a run
    /// built from them never gets an evolution card in its hand.
    fn plain_spells(n: usize, level: u8) -> Vec<Owned> {
        data::CLASSES[0]
            .pool
            .iter()
            .filter(|key| !data::EVOLUTIONS.iter().any(|e| e.from == **key))
            .take(n)
            .map(|key| Owned {
                def: data::ability(key),
                level,
                cd: 0.0,
            })
            .collect()
    }

    /// The first `n` passives, each at `level`.
    fn passives_at(n: usize, level: u8) -> Vec<(&'static data::PassiveDef, u8)> {
        data::PASSIVES.iter().take(n).map(|p| (p, level)).collect()
    }

    /// Every spell slot and every passive slot full, everything at its top rank.
    fn capped_run() -> Run {
        let run = Run {
            abilities: plain_spells(data::MAX_ABILITIES, data::MAX_ABILITY_LEVEL),
            passives: passives_at(data::MAX_PASSIVES, data::MAX_PASSIVE_LEVEL),
            ..Run::default()
        };
        assert_eq!(run.abilities.len(), data::MAX_ABILITIES);
        assert_eq!(run.passives.len(), data::MAX_PASSIVES);
        run
    }

    #[test]
    fn a_hand_is_never_longer_than_asked_and_never_repeats_a_card() {
        for class in data::CLASSES {
            let mut run = Run {
                class,
                ..Run::default()
            };
            run.abilities.push(Owned {
                def: data::ability(class.start),
                level: 1,
                cd: 0.0,
            });
            run.passives.push((&data::PASSIVES[0], 1));
            for size in [3, 4] {
                for seed in 1..=16 {
                    let hand = hud::choices_for(&run, &mut Rng(seed), &[], size);
                    assert_eq!(hand.len(), size);
                    let mut seen = std::collections::HashSet::new();
                    for choice in hand {
                        let key = match choice {
                            Choice::NewAbility(def) => {
                                assert!(class.pool.contains(&def.key));
                                assert!(!run.abilities.iter().any(|o| o.def.key == def.key));
                                ("ability", def.key)
                            }
                            Choice::Upgrade(i) => {
                                assert!(run.abilities[i].level < data::MAX_ABILITY_LEVEL);
                                ("ability", run.abilities[i].def.key)
                            }
                            Choice::NewPassive(p) => {
                                assert!(!run.passives.iter().any(|(q, _)| q.key == p.key));
                                ("passive", p.key)
                            }
                            Choice::UpPassive(i) => {
                                assert!(run.passives[i].1 < data::MAX_PASSIVE_LEVEL);
                                ("passive", run.passives[i].0.key)
                            }
                            Choice::Evolve(_) | Choice::Heal => {
                                panic!("a fresh run has neither an evolution nor a reason to heal")
                            }
                        };
                        assert!(seen.insert(key), "{key:?} was dealt twice");
                    }
                }
            }
        }
    }

    #[test]
    fn a_maxed_spell_or_passive_is_not_offered_again() {
        let mut spells = plain_spells(2, data::MAX_ABILITY_LEVEL);
        spells[1].level = 3;
        let mut passives = passives_at(2, data::MAX_PASSIVE_LEVEL);
        passives[1].1 = 2;
        let run = Run {
            level: 5,
            abilities: spells,
            passives,
            ..Run::default()
        };
        let mut rng = Rng(7);
        let mut upgrades = 0;
        for _ in 0..300 {
            for choice in hud::choices_for(&run, &mut rng, &[], 4) {
                assert!(!matches!(choice, Choice::Upgrade(0)), "the maxed spell came back");
                assert!(!matches!(choice, Choice::UpPassive(0)), "the maxed passive came back");
                upgrades += usize::from(matches!(choice, Choice::Upgrade(1) | Choice::UpPassive(1)));
            }
        }
        // The ranks still open are on offer.
        assert!(upgrades > 0);
    }

    #[test]
    fn six_spells_stop_the_new_spell_cards() {
        let run = Run {
            level: 5,
            abilities: plain_spells(data::MAX_ABILITIES, 1),
            ..Run::default()
        };
        assert_eq!(run.abilities.len(), data::MAX_ABILITIES);
        let mut rng = Rng(11);
        for _ in 0..300 {
            for choice in hud::choices_for(&run, &mut rng, &[], 4) {
                assert!(!matches!(choice, Choice::NewAbility(_)), "a seventh spell was offered");
            }
        }
    }

    #[test]
    fn six_passives_stop_the_new_passive_cards() {
        let run = Run {
            level: 5,
            abilities: plain_spells(1, 1),
            passives: passives_at(data::MAX_PASSIVES, 1),
            ..Run::default()
        };
        assert_eq!(run.passives.len(), data::MAX_PASSIVES);
        let mut rng = Rng(13);
        for _ in 0..300 {
            for choice in hud::choices_for(&run, &mut rng, &[], 4) {
                assert!(!matches!(choice, Choice::NewPassive(_)), "a seventh passive was offered");
            }
        }
    }

    #[test]
    fn full_slots_offer_only_the_ranks_left_and_do_not_pad_the_hand() {
        let mut run = capped_run();
        run.abilities[0].level -= 1;
        run.passives[0].1 -= 1;
        let hand = hud::choices_for(&run, &mut Rng(1), &[], 3);
        assert_eq!(hand.len(), 2);
        assert!(hand.iter().any(|c| matches!(c, Choice::Upgrade(0))));
        assert!(hand.iter().any(|c| matches!(c, Choice::UpPassive(0))));
    }

    #[test]
    fn with_nothing_left_to_gain_the_hand_is_the_heal() {
        let hand = hud::choices_for(&capped_run(), &mut Rng(1), &[], 3);
        assert!(matches!(hand.as_slice(), [Choice::Heal]));
    }

    #[test]
    fn queued_levels_deal_one_hand_each_and_then_return_to_playing() {
        let mut run = capped_run();
        run.level = 6;
        run.pending_levels = 3;
        let mut offer = hud::Offer::default();
        let mut rng = Rng(1);
        let mut next = NextState::<Phase>::Unchanged;

        hud::advance(&mut run, &mut offer, &mut rng, &mut next);
        assert_eq!(run.pending_levels, 2);
        assert!(matches!(offer.choices(), [Choice::Heal]));
        assert!(matches!(next, NextState::Unchanged));

        hud::advance(&mut run, &mut offer, &mut rng, &mut next);
        assert_eq!(run.pending_levels, 1);
        assert!(matches!(offer.choices(), [Choice::Heal]));
        assert!(matches!(next, NextState::Unchanged));

        hud::advance(&mut run, &mut offer, &mut rng, &mut next);
        assert_eq!(run.pending_levels, 0);
        assert_eq!(offer.len(), 0);
        assert!(matches!(next, NextState::Pending(Phase::Playing)));
    }

    #[test]
    fn picking_ranks_through_queued_levels_applies_each_and_then_requests_playing() {
        let mut run = capped_run();
        run.level = 10;
        run.abilities[0].level -= 1;
        run.passives[0].1 -= 1;
        run.pending_levels = 2;
        let mut offer = hud::Offer::default();
        let mut rng = Rng(1);
        let mut next = NextState::<Phase>::Unchanged;
        offer.reroll(&run, &mut rng, 3);
        assert_eq!(offer.len(), 2);

        hud::pick(0, &mut run, &mut offer, &mut rng, &mut next);
        assert_eq!(run.version, 1);
        assert_eq!(run.pending_levels, 1);
        assert!(matches!(next, NextState::Unchanged));
        // One of the two ranks is taken, the other is the next level's only card.
        assert_eq!(offer.len(), 1);
        assert!(matches!(offer.choices(), [Choice::Upgrade(0) | Choice::UpPassive(0)]));

        hud::pick(0, &mut run, &mut offer, &mut rng, &mut next);
        assert_eq!(run.abilities[0].level, data::MAX_ABILITY_LEVEL);
        assert_eq!(run.passives[0].1, data::MAX_PASSIVE_LEVEL);
        assert_eq!(run.version, 2);
        assert_eq!(run.pending_levels, 0);
        assert_eq!(offer.len(), 0);
        assert!(matches!(next, NextState::Pending(Phase::Playing)));
    }

    #[test]
    fn picking_the_heal_restores_health_and_leaves_no_level_owed() {
        let mut run = capped_run();
        run.hp = 1.0;
        let mut offer = hud::Offer::default();
        let mut rng = Rng(1);
        let mut next = NextState::<Phase>::Unchanged;
        offer.reroll(&run, &mut rng, 3);
        assert!(matches!(offer.choices(), [Choice::Heal]));

        hud::pick(0, &mut run, &mut offer, &mut rng, &mut next);
        assert_eq!(run.hp, run.stats().max_hp);
        // No level was waiting: the count stays at zero instead of wrapping.
        assert_eq!(run.pending_levels, 0);
        assert_eq!(run.version, 1);
        assert_eq!(offer.len(), 0);
        assert!(matches!(next, NextState::Pending(Phase::Playing)));
    }

    #[test]
    fn a_pick_that_is_not_on_the_table_does_nothing() {
        let mut run = capped_run();
        run.hp = 1.0;
        run.pending_levels = 2;
        let mut offer = hud::Offer::default();
        let mut rng = Rng(1);
        let mut next = NextState::<Phase>::Unchanged;
        offer.reroll(&run, &mut rng, 3);
        assert_eq!(offer.len(), 1);

        // Past the end of the hand.
        hud::pick(1, &mut run, &mut offer, &mut rng, &mut next);
        assert_eq!(run.hp, 1.0);
        assert_eq!(run.pending_levels, 2);
        assert_eq!(run.version, 0);
        assert_eq!(offer.len(), 1);
        assert!(matches!(next, NextState::Unchanged));

        // No hand at all, and the dice stay where they were.
        let mut empty = hud::Offer::default();
        let mut untouched = Rng(5);
        hud::pick(0, &mut run, &mut empty, &mut untouched, &mut next);
        assert_eq!(run.version, 0);
        assert_eq!(run.pending_levels, 2);
        assert_eq!(untouched.next_u64(), Rng(5).next_u64());
        assert!(matches!(next, NextState::Unchanged));
    }
}
