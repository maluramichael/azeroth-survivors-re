//! The mode's own window kit, shared by every screen: dark warm panels with a thin gold edge,
//! Morpheus titles between two fading rules, dark red buttons with a gold edge, small-caps
//! labels and stat boxes. The glue widgets ([`benilla_app::survivors_host::glue`]) copy the 1.12
//! create screen; this kit is for the mode's own windows (the hub, talents, gear, trainer).
//!
//! Sizes are in the 768-unit canvas the HUD lays out on (`UiScale`, [`super::hud::apply_ui_scale`]).

use bevy::prelude::*;

/// `Fonts\FRIZQT__.TTF` for text, `Fonts\MORPHEUS.TTF` for titles.
#[derive(Resource, Clone)]
pub(crate) struct KitFonts {
    pub(crate) body: Handle<Font>,
    pub(crate) title: Handle<Font>,
}

// --- Colours. ---

/// Title gold, the Morpheus headings.
pub(crate) const TITLE: Color = Color::srgb(0.93, 0.80, 0.47);
/// Body text, a warm off-white.
pub(crate) const CREAM: Color = Color::srgb(0.94, 0.90, 0.80);
/// Secondary text and small-caps labels.
pub(crate) const MUTED: Color = Color::srgb(0.66, 0.62, 0.55);
/// Locked or unavailable.
pub(crate) const DISABLED: Color = Color::srgb(0.42, 0.40, 0.37);
/// Bonus lines ("+4.7% crit"), the tooltip green.
pub(crate) const BONUS: Color = Color::srgb(0.38, 0.90, 0.33);
/// A penalty or a missing requirement.
pub(crate) const WARN: Color = Color::srgb(0.95, 0.32, 0.28);

/// Item quality colours, `ITEM_QUALITY_COLORS`: poor to legendary.
pub(crate) const QUALITY: [Color; 6] = [
    Color::srgb(0.62, 0.62, 0.62),
    Color::srgb(1.0, 1.0, 1.0),
    Color::srgb(0.12, 1.0, 0.0),
    Color::srgb(0.0, 0.44, 0.87),
    Color::srgb(0.64, 0.21, 0.93),
    Color::srgb(1.0, 0.50, 0.0),
];

/// A panel's edge and its fill, top to bottom.
pub(crate) const PANEL_EDGE: Color = Color::srgba(0.47, 0.37, 0.19, 0.95);
const PANEL_TOP: Color = Color::srgba(0.085, 0.065, 0.045, 0.94);
const PANEL_BOTTOM: Color = Color::srgba(0.035, 0.027, 0.02, 0.96);

/// An inset box inside a panel (a stat box, a list row).
pub(crate) const INSET_FILL: Color = Color::srgba(0.0, 0.0, 0.0, 0.35);
pub(crate) const INSET_EDGE: Color = Color::srgba(0.42, 0.33, 0.18, 0.6);

/// A button's edge: resting, hovered, selected.
const BTN_EDGE: Color = Color::srgb(0.55, 0.42, 0.21);
const BTN_EDGE_HOT: Color = Color::srgb(0.85, 0.68, 0.36);
const BTN_EDGE_ON: Color = Color::srgb(0.98, 0.82, 0.45);
/// A button's fill, top to bottom: resting, hovered, selected.
const BTN_FILL: (Color, Color) = (Color::srgb(0.20, 0.075, 0.05), Color::srgb(0.11, 0.04, 0.03));
const BTN_FILL_HOT: (Color, Color) = (Color::srgb(0.30, 0.11, 0.07), Color::srgb(0.17, 0.06, 0.04));
const BTN_FILL_ON: (Color, Color) = (Color::srgb(0.32, 0.17, 0.07), Color::srgb(0.18, 0.09, 0.04));
const BTN_FILL_OFF: (Color, Color) = (Color::srgb(0.10, 0.08, 0.07), Color::srgb(0.06, 0.05, 0.04));

/// What a button looks like: a plain one, a tab in a tab bar, or the screen's main action.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub(crate) enum KitStyle {
    #[default]
    Normal,
    Tab,
    Primary,
}

/// On every kit button: [`kit_visuals`] repaints the edge and the fill from it and the
/// `Interaction`. Set `selected` for the open tab or the picked option, `disabled` for one that
/// does nothing; the caller's click handler checks `disabled` itself.
#[derive(Component, Clone, Copy, Debug, Default)]
pub(crate) struct KitButton {
    pub(crate) style: KitStyle,
    pub(crate) selected: bool,
    pub(crate) disabled: bool,
}

/// A kit button's caption, recoloured with its state.
#[derive(Component)]
pub(crate) struct KitCaption;

/// A kit button's second line, under the caption.
#[derive(Component)]
pub(crate) struct KitSubCaption;

pub(crate) fn plugin(app: &mut App) {
    app.add_systems(Startup, load_fonts)
        .add_systems(Update, kit_visuals);
}

fn load_fonts(mut commands: Commands, assets: Res<AssetServer>) {
    commands.insert_resource(KitFonts {
        body: benilla_app::survivors_host::char_select::wow_font(&assets),
        title: assets.load("mpq://Fonts/MORPHEUS.ttf"),
    });
}

fn vertical(top: Color, bottom: Color) -> BackgroundGradient {
    BackgroundGradient::from(LinearGradient::to_bottom(vec![
        ColorStop::percent(top, 0.0),
        ColorStop::percent(bottom, 100.0),
    ]))
}

/// A text with the kit's drop shadow.
pub(crate) fn text(font: &Handle<Font>, s: impl Into<String>, size: f32, color: Color) -> impl Bundle {
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
            color: Color::srgba(0.0, 0.0, 0.0, 0.9),
        },
    )
}

/// Body text in Friz.
pub(crate) fn body(fonts: &KitFonts, s: impl Into<String>, size: f32, color: Color) -> impl Bundle {
    text(&fonts.body, s, size, color)
}

/// A title in Morpheus.
pub(crate) fn title(fonts: &KitFonts, s: impl Into<String>, size: f32) -> impl Bundle {
    text(&fonts.title, s, size, TITLE)
}

/// A small-caps section label ("SPELLS", "HEALTH"): upper case, small, muted.
pub(crate) fn label(fonts: &KitFonts, s: &str) -> impl Bundle {
    text(&fonts.body, s.to_uppercase(), 11.0, MUTED)
}

/// A panel: the warm dark fill, the gold edge, rounded corners and a soft shadow. `node` sets the
/// size and the layout; the border and the radius are the kit's.
pub(crate) fn panel(node: Node) -> impl Bundle {
    (
        Node {
            border: UiRect::all(Val::Px(1.0)),
            border_radius: BorderRadius::all(Val::Px(6.0)),
            ..node
        },
        vertical(PANEL_TOP, PANEL_BOTTOM),
        BorderColor::all(PANEL_EDGE),
        BoxShadow::new(
            Color::srgba(0.0, 0.0, 0.0, 0.7),
            Val::Px(0.0),
            Val::Px(4.0),
            Val::Px(2.0),
            Val::Px(14.0),
        ),
    )
}

/// An inset box inside a panel: a darker fill and a faint edge.
pub(crate) fn inset(node: Node) -> impl Bundle {
    (
        Node {
            border: UiRect::all(Val::Px(1.0)),
            border_radius: BorderRadius::all(Val::Px(3.0)),
            ..node
        },
        BackgroundColor(INSET_FILL),
        BorderColor::all(INSET_EDGE),
    )
}

/// A thin gold rule that fades out to both ends, `width` wide.
pub(crate) fn rule(width: Val) -> impl Bundle {
    (
        Node {
            width,
            height: Val::Px(1.0),
            ..default()
        },
        BackgroundGradient::from(LinearGradient::to_right(vec![
            ColorStop::percent(PANEL_EDGE.with_alpha(0.0), 0.0),
            ColorStop::percent(PANEL_EDGE, 50.0),
            ColorStop::percent(PANEL_EDGE.with_alpha(0.0), 100.0),
        ])),
    )
}

/// A panel heading: a Morpheus title between two rules that fade out away from it.
pub(crate) fn heading(parent: &mut ChildSpawnerCommands, fonts: &KitFonts, s: &str, size: f32) {
    parent
        .spawn(Node {
            width: Val::Percent(100.0),
            align_items: AlignItems::Center,
            column_gap: Val::Px(12.0),
            margin: UiRect::vertical(Val::Px(6.0)),
            ..default()
        })
        .with_children(|row| {
            let side = |fade_left: bool| {
                let (a, b) = if fade_left {
                    (PANEL_EDGE.with_alpha(0.0), PANEL_EDGE)
                } else {
                    (PANEL_EDGE, PANEL_EDGE.with_alpha(0.0))
                };
                (
                    Node {
                        flex_grow: 1.0,
                        height: Val::Px(1.0),
                        ..default()
                    },
                    BackgroundGradient::from(LinearGradient::to_right(vec![
                        ColorStop::percent(a, 0.0),
                        ColorStop::percent(b, 100.0),
                    ])),
                )
            };
            row.spawn(side(true));
            row.spawn(title(fonts, s, size));
            row.spawn(side(false));
        });
}

/// A button: a caption and, under it, an optional second line. `node` sets the size; `extra`
/// goes on the button entity (the caller's action marker). Returns the button entity.
pub(crate) fn button(
    parent: &mut ChildSpawnerCommands,
    fonts: &KitFonts,
    style: KitStyle,
    caption: &str,
    sub: Option<&str>,
    node: Node,
    extra: impl Bundle,
) -> Entity {
    let (font, size) = match style {
        KitStyle::Primary => (&fonts.title, 26.0),
        KitStyle::Tab => (&fonts.body, 15.0),
        KitStyle::Normal => (&fonts.body, 15.0),
    };
    let edge = if style == KitStyle::Primary { 2.0 } else { 1.0 };
    parent
        .spawn((
            Button,
            KitButton {
                style,
                ..default()
            },
            Node {
                flex_direction: FlexDirection::Column,
                justify_content: JustifyContent::Center,
                align_items: AlignItems::Center,
                padding: UiRect::axes(Val::Px(14.0), Val::Px(4.0)),
                border: UiRect::all(Val::Px(edge)),
                border_radius: BorderRadius::all(Val::Px(3.0)),
                ..node
            },
            vertical(BTN_FILL.0, BTN_FILL.1),
            BorderColor::all(BTN_EDGE),
            BoxShadow::new(
                Color::srgba(0.0, 0.0, 0.0, 0.6),
                Val::Px(0.0),
                Val::Px(2.0),
                Val::Px(0.0),
                Val::Px(6.0),
            ),
            extra,
        ))
        .with_children(|b| {
            b.spawn((
                KitCaption,
                text(font, caption, size, CREAM),
                TextLayout::new_with_justify(Justify::Center),
            ));
            if let Some(sub) = sub {
                b.spawn((
                    KitSubCaption,
                    text(&fonts.body, sub, 11.0, MUTED),
                    TextLayout::new_with_justify(Justify::Center),
                ));
            }
        })
        .id()
}

/// A stat box: a small-caps label over a value. `value_marker` goes on the value's text, so a
/// refresh system can find it.
pub(crate) fn stat_box(
    parent: &mut ChildSpawnerCommands,
    fonts: &KitFonts,
    caption: &str,
    value: &str,
    node: Node,
    value_marker: impl Bundle,
) {
    parent
        .spawn(inset(Node {
            flex_direction: FlexDirection::Column,
            padding: UiRect::axes(Val::Px(10.0), Val::Px(6.0)),
            row_gap: Val::Px(2.0),
            ..node
        }))
        .with_children(|b| {
            b.spawn(label(fonts, caption));
            b.spawn((body(fonts, value, 17.0, CREAM), value_marker));
        });
}

type Painted<'a> = (
    &'a KitButton,
    &'a Interaction,
    &'a mut BackgroundGradient,
    &'a mut BorderColor,
    &'a Children,
);

/// Repaint every kit button whose state or hover changed: edge, fill and caption colour.
fn kit_visuals(
    mut buttons: Query<Painted, Or<(Changed<KitButton>, Changed<Interaction>)>>,
    mut captions: Query<&mut TextColor, With<KitCaption>>,
) {
    for (button, interaction, mut fill, mut edge, children) in &mut buttons {
        let hot = !button.disabled && *interaction != Interaction::None;
        let (top, bottom) = if button.disabled {
            BTN_FILL_OFF
        } else if button.selected {
            BTN_FILL_ON
        } else if hot {
            BTN_FILL_HOT
        } else {
            BTN_FILL
        };
        *fill = vertical(top, bottom);
        *edge = BorderColor::all(if button.disabled {
            BTN_EDGE.with_alpha(0.4)
        } else if button.selected {
            BTN_EDGE_ON
        } else if hot || button.style == KitStyle::Primary {
            BTN_EDGE_HOT
        } else {
            BTN_EDGE
        });
        let color = if button.disabled {
            DISABLED
        } else if button.selected || button.style == KitStyle::Primary {
            TITLE
        } else {
            CREAM
        };
        for child in children.iter() {
            if let Ok(mut c) = captions.get_mut(child) {
                c.0 = color;
            }
        }
    }
}
