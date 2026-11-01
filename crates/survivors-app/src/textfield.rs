//! A one-line text field for the UI kit: click to type, Enter to submit, Esc to let go.
//!
//! [`spawn`] puts an inset box under a parent; the caller keeps the returned entity and reads or
//! sets [`TextField::value`]. Only one field has the keyboard at a time ([`TextFocus`]). Other
//! systems that read keys (hotkeys, the window's Esc) must stand aside while [`TextFocus`] is set.
//! A window closed by Esc must check [`TextFocus`] before handling
//! the key: the field lets go of the keyboard only at the end of the frame (in `Last`), so on the
//! frame Esc is pressed in a field `TextFocus` is still `Some` and the window can leave it open.
//!
//! Paste (Cmd+V on macOS, Ctrl+V elsewhere) goes through the engine's OS clipboard on native
//! builds. The browser build has no synchronous clipboard, so paste does nothing there.

use bevy::input::keyboard::{Key, KeyboardInput};
use bevy::input::ButtonState;
use bevy::prelude::*;
use bevy::window::{PrimaryWindow, RawHandleWrapper};
use benilla_app::survivors_host::textinput::{wayland_display, HostClipboard};

use crate::kit;

/// Text size in the kit's 768-unit canvas.
const FONT_SIZE: f32 = 13.0;
/// The blink period of the caret: shown for half of it, hidden for the other half.
const BLINK_SECS: f32 = 1.0;
/// What a secret field draws per char. A plain asterisk: the 1.12 fonts have no bullet glyph.
const MASK: char = '*';
/// The caret, drawn after the last char while the field has the keyboard.
const CARET: char = '|';

/// A one-line text field. `value` is what was typed.
#[derive(Component)]
pub(crate) struct TextField {
    pub value: String,
    pub placeholder: String,
    pub max_len: usize,
    /// Shown as dots (passwords).
    pub secret: bool,
}

/// Which field has the keyboard; `None` when no field does.
#[derive(Resource, Default)]
pub(crate) struct TextFocus(pub Option<Entity>);

/// Enter pressed in a field.
#[derive(Message, Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct TextSubmitted(pub Entity);

/// The field's text node, a child of the box. Private: the window never touches it.
#[derive(Component)]
struct FieldText;

/// Set when Esc was pressed in a field this frame; [`release_on_escape`] acts on it in `Last`.
#[derive(Resource, Default)]
struct EscapePressed(bool);

/// Appends `s` to `value`, dropping control characters and stopping at `max_len` chars.
fn insert(value: &mut String, s: &str, max_len: usize) {
    let room = max_len.saturating_sub(value.chars().count());
    value.extend(s.chars().filter(|c| !c.is_control()).take(room));
}

/// Removes the last char (not the last byte) of `value`.
fn backspace(value: &mut String) {
    value.pop();
}

/// What a field draws: the text and whether it is drawn muted (the placeholder).
fn shown(value: &str, secret: bool, placeholder: &str, focused: bool, caret_on: bool) -> (String, bool) {
    if value.is_empty() && !focused {
        return (placeholder.to_string(), true);
    }
    let mut out: String = if secret {
        value.chars().map(|_| MASK).collect()
    } else {
        value.to_string()
    };
    if focused && caret_on {
        out.push(CARET);
    }
    (out, false)
}

/// Spawn a field under `parent`: an inset box (kit style) holding the text, `node` sets its size.
/// The text is clipped at the box's edge, so pick `max_len` to fit the width.
pub(crate) fn spawn(
    parent: &mut ChildSpawnerCommands,
    fonts: &kit::KitFonts,
    placeholder: &str,
    value: &str,
    max_len: usize,
    secret: bool,
    node: Node,
) -> Entity {
    let mut node = node;
    if node.padding == UiRect::DEFAULT {
        node.padding = UiRect::horizontal(Val::Px(6.0));
    }
    node.align_items = AlignItems::Center;
    node.overflow = Overflow::clip();
    let (text, muted) = shown(value, secret, placeholder, false, false);
    let color = if muted { kit::MUTED } else { kit::CREAM };
    let field = TextField {
        value: value.to_string(),
        placeholder: placeholder.to_string(),
        max_len,
        secret,
    };
    parent
        .spawn((kit::inset(node), Interaction::default(), field))
        .with_children(|f| {
            f.spawn((
                kit::body(fonts, text, FONT_SIZE, color),
                Node {
                    flex_shrink: 0.0,
                    ..default()
                },
                FieldText,
            ));
        })
        .id()
}

/// A left press on a field gives it the keyboard; a press anywhere else takes it away. A field
/// that was despawned loses it too.
fn focus_on_click(
    mouse: Res<ButtonInput<MouseButton>>,
    fields: Query<(Entity, &Interaction), With<TextField>>,
    mut focus: ResMut<TextFocus>,
) {
    if let Some(e) = focus.0 {
        if fields.get(e).is_err() {
            focus.0 = None;
        }
    }
    if !mouse.just_pressed(MouseButton::Left) {
        return;
    }
    let hit = fields
        .iter()
        .find(|(_, i)| **i == Interaction::Pressed)
        .map(|(e, _)| e);
    if focus.0 != hit {
        focus.0 = hit;
    }
}

/// Typing, backspace, paste, Enter and Esc for the focused field.
fn type_keys(
    mut keys: MessageReader<KeyboardInput>,
    held: Res<ButtonInput<KeyCode>>,
    focus: Res<TextFocus>,
    mut fields: Query<&mut TextField>,
    mut submitted: MessageWriter<TextSubmitted>,
    mut escape: ResMut<EscapePressed>,
    clipboard: Option<NonSendMut<HostClipboard>>,
    window: Query<&RawHandleWrapper, With<PrimaryWindow>>,
) {
    // Drain the reader even when nothing is focused, so old keys are not replayed on focus.
    let events: Vec<KeyboardInput> = keys.read().cloned().collect();
    let Some(entity) = focus.0 else { return };
    let Ok(mut field) = fields.get_mut(entity) else {
        return;
    };
    let ctrl = held.any_pressed([KeyCode::ControlLeft, KeyCode::ControlRight]);
    let alt = held.any_pressed([KeyCode::AltLeft, KeyCode::AltRight]);
    let sup = held.any_pressed([KeyCode::SuperLeft, KeyCode::SuperRight]);
    // Cmd+V on macOS, Ctrl+V elsewhere.
    let paste_mod = if cfg!(target_os = "macos") { sup } else { ctrl };
    // A command chord never types; Ctrl+Alt (AltGr) does.
    let types = !(sup || (ctrl && !alt));
    let mut clipboard = clipboard;

    let mut value = field.value.clone();
    for ev in &events {
        if ev.state != ButtonState::Pressed {
            continue;
        }
        if paste_mod && ev.key_code == KeyCode::KeyV {
            let display = wayland_display(window.single().ok());
            let text = clipboard.as_mut().and_then(|c| c.read(display));
            if let Some(line) = text.as_deref().and_then(|t| t.lines().next()) {
                insert(&mut value, line, field.max_len);
            }
            continue;
        }
        match &ev.logical_key {
            Key::Character(s) if types => insert(&mut value, s, field.max_len),
            Key::Space if types => insert(&mut value, " ", field.max_len),
            Key::Backspace => backspace(&mut value),
            Key::Enter => {
                submitted.write(TextSubmitted(entity));
            }
            Key::Escape => escape.0 = true,
            _ => {}
        }
    }
    if value != field.value {
        field.value = value;
    }
}

/// Lets go of the keyboard after the frame Esc was pressed in, so that frame's other systems still
/// see [`TextFocus`] set.
fn release_on_escape(mut escape: ResMut<EscapePressed>, mut focus: ResMut<TextFocus>) {
    if std::mem::take(&mut escape.0) {
        focus.0 = None;
    }
}

/// Keeps every field's drawn text, text colour and edge in step with its value, the focus and the
/// caret blink.
fn paint(
    time: Res<Time<Real>>,
    focus: Res<TextFocus>,
    mut fields: Query<(Entity, &TextField, &Children, &mut BorderColor)>,
    mut texts: Query<(&mut Text, &mut TextColor), With<FieldText>>,
) {
    let caret_on = time.elapsed_secs().rem_euclid(BLINK_SECS) < BLINK_SECS * 0.5;
    for (entity, field, children, mut border) in &mut fields {
        let focused = focus.0 == Some(entity);
        let edge = if focused { kit::TITLE } else { kit::INSET_EDGE };
        if border.top != edge {
            *border = BorderColor::all(edge);
        }
        let (want, muted) = shown(&field.value, field.secret, &field.placeholder, focused, caret_on);
        let color = if muted { kit::MUTED } else { kit::CREAM };
        for child in children.iter() {
            if let Ok((mut text, mut tc)) = texts.get_mut(child) {
                if text.0 != want {
                    text.0 = want.clone();
                }
                if tc.0 != color {
                    tc.0 = color;
                }
            }
        }
    }
}

pub(super) fn plugin(app: &mut App) {
    app.init_resource::<TextFocus>()
        .init_resource::<EscapePressed>()
        .add_message::<TextSubmitted>()
        .add_systems(Update, (focus_on_click, type_keys, paint).chain())
        .add_systems(Last, release_on_escape);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn insert_appends_and_caps_in_chars() {
        let mut v = String::from("ab");
        insert(&mut v, "cdef", 4);
        assert_eq!(v, "abcd");
        insert(&mut v, "x", 4);
        assert_eq!(v, "abcd");
    }

    #[test]
    fn insert_counts_chars_not_bytes() {
        let mut v = String::new();
        insert(&mut v, "äö😀ü", 3);
        assert_eq!(v, "äö😀");
        assert_eq!(v.chars().count(), 3);
    }

    #[test]
    fn insert_drops_control_chars() {
        let mut v = String::new();
        insert(&mut v, "a\tb\nc\u{7f}d\u{0}", 10);
        assert_eq!(v, "abcd");
    }

    #[test]
    fn insert_over_a_full_value_changes_nothing() {
        let mut v = String::from("abc");
        insert(&mut v, "d", 2);
        assert_eq!(v, "abc");
    }

    #[test]
    fn backspace_removes_whole_chars() {
        let mut v = String::from("a😀ä");
        backspace(&mut v);
        assert_eq!(v, "a😀");
        backspace(&mut v);
        assert_eq!(v, "a");
        backspace(&mut v);
        backspace(&mut v);
        assert_eq!(v, "");
    }

    #[test]
    fn shown_placeholder_only_when_empty_and_unfocused() {
        assert_eq!(shown("", false, "Name", false, true), ("Name".into(), true));
        assert_eq!(shown("", false, "Name", true, false), (String::new(), false));
        assert_eq!(shown("", false, "Name", true, true), ("|".into(), false));
    }

    #[test]
    fn shown_caret_follows_focus_and_blink() {
        assert_eq!(shown("ab", false, "", true, true), ("ab|".into(), false));
        assert_eq!(shown("ab", false, "", true, false), ("ab".into(), false));
        assert_eq!(shown("ab", false, "", false, true), ("ab".into(), false));
    }

    #[test]
    fn shown_secret_is_one_mask_char_per_char() {
        assert_eq!(shown("äö😀", true, "", false, false), ("***".into(), false));
        assert_eq!(shown("äö", true, "", true, true), ("**|".into(), false));
    }
}
