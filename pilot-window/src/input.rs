//! A human's mouse and keyboard in the window, as `Input.*` parameters for
//! the page under it. Coordinates are page CSS pixels
//! ([`crate::geometry::window_to_page`]).

use serde_json::{Value, json};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MouseButton {
    Left,
    Middle,
    Right,
}

impl MouseButton {
    fn name(self) -> &'static str {
        match self {
            Self::Left => "left",
            Self::Middle => "middle",
            Self::Right => "right",
        }
    }

    /// The button's bit in CDP's `buttons` mask.
    pub fn mask(self) -> u32 {
        match self {
            Self::Left => 1,
            Self::Right => 2,
            Self::Middle => 4,
        }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Modifiers {
    pub alt: bool,
    pub ctrl: bool,
    pub meta: bool,
    pub shift: bool,
}

impl Modifiers {
    /// CDP's modifier bit field.
    pub fn bits(self) -> u32 {
        u32::from(self.alt)
            | u32::from(self.ctrl) << 1
            | u32::from(self.meta) << 2
            | u32::from(self.shift) << 3
    }
}

/// `Input.dispatchMouseEvent` for a move. `buttons` is the held-button mask.
pub fn mouse_moved(x: f64, y: f64, buttons: u32, modifiers: Modifiers) -> Value {
    json!({"type": "mouseMoved", "x": x, "y": y, "button": "none", "buttons": buttons, "modifiers": modifiers.bits()})
}

/// `Input.dispatchMouseEvent` for a press or release. `buttons` is the mask
/// after the change; `click_count` counts quick repeats (2 = double click).
pub fn mouse_button(
    pressed: bool,
    x: f64,
    y: f64,
    button: MouseButton,
    buttons: u32,
    click_count: u32,
    modifiers: Modifiers,
) -> Value {
    json!({
        "type": if pressed { "mousePressed" } else { "mouseReleased" },
        "x": x, "y": y,
        "button": button.name(),
        "buttons": buttons,
        "clickCount": click_count,
        "modifiers": modifiers.bits(),
    })
}

/// `Input.dispatchMouseEvent` for a wheel scroll, in CSS pixels.
pub fn mouse_wheel(x: f64, y: f64, delta_x: f64, delta_y: f64, modifiers: Modifiers) -> Value {
    json!({"type": "mouseWheel", "x": x, "y": y, "deltaX": delta_x, "deltaY": delta_y, "modifiers": modifiers.bits()})
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum NamedKey {
    Enter,
    Backspace,
    Tab,
    Escape,
    Space,
    ArrowLeft,
    ArrowUp,
    ArrowRight,
    ArrowDown,
    Delete,
    Home,
    End,
    PageUp,
    PageDown,
}

impl NamedKey {
    /// `(key, code, windowsVirtualKeyCode, text)`.
    fn description(self) -> (&'static str, &'static str, u32, Option<&'static str>) {
        match self {
            Self::Enter => ("Enter", "Enter", 13, Some("\r")),
            Self::Backspace => ("Backspace", "Backspace", 8, None),
            Self::Tab => ("Tab", "Tab", 9, None),
            Self::Escape => ("Escape", "Escape", 27, None),
            Self::Space => (" ", "Space", 32, Some(" ")),
            Self::ArrowLeft => ("ArrowLeft", "ArrowLeft", 37, None),
            Self::ArrowUp => ("ArrowUp", "ArrowUp", 38, None),
            Self::ArrowRight => ("ArrowRight", "ArrowRight", 39, None),
            Self::ArrowDown => ("ArrowDown", "ArrowDown", 40, None),
            Self::Delete => ("Delete", "Delete", 46, None),
            Self::Home => ("Home", "Home", 36, None),
            Self::End => ("End", "End", 35, None),
            Self::PageUp => ("PageUp", "PageUp", 33, None),
            Self::PageDown => ("PageDown", "PageDown", 34, None),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Key<'a> {
    Named(NamedKey),
    /// What the key types, as the platform reports it (`"a"`, `"A"`, `"é"`).
    Character(&'a str),
}

/// `Input.dispatchKeyEvent` for a key going down or up. A key down carries
/// the text it types, except under Ctrl or Meta, where it is a shortcut.
pub fn key(key: Key<'_>, pressed: bool, modifiers: Modifiers) -> Value {
    let (key_name, code, key_code, text) = match key {
        Key::Named(named) => {
            let (key_name, code, key_code, text) = named.description();
            (
                key_name.to_owned(),
                code.to_owned(),
                key_code,
                text.map(str::to_owned),
            )
        }
        Key::Character(text) => {
            let (code, key_code) = character_code(text);
            (text.to_owned(), code, key_code, Some(text.to_owned()))
        }
    };
    let mut event = json!({
        "type": if pressed { "keyDown" } else { "keyUp" },
        "key": key_name,
        "code": code,
        "windowsVirtualKeyCode": key_code,
        "modifiers": modifiers.bits(),
    });
    if pressed
        && !modifiers.ctrl
        && !modifiers.meta
        && let Some(text) = text
    {
        event["text"] = json!(text);
    }
    event
}

/// `code` and key code for a typed character: letters and digits map to
/// their US-layout keys, anything else has none.
fn character_code(text: &str) -> (String, u32) {
    let mut chars = text.chars();
    match (chars.next(), chars.next()) {
        (Some(letter), None) if letter.is_ascii_alphabetic() => {
            let upper = letter.to_ascii_uppercase();
            (format!("Key{upper}"), u32::from(upper))
        }
        (Some(digit), None) if digit.is_ascii_digit() => {
            (format!("Digit{digit}"), u32::from(digit))
        }
        _ => (String::new(), 0),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SHIFT: Modifiers = Modifiers {
        alt: false,
        ctrl: false,
        meta: false,
        shift: true,
    };

    #[test]
    fn modifier_bits_follow_cdp() {
        assert_eq!(Modifiers::default().bits(), 0);
        assert_eq!(SHIFT.bits(), 8);
        let all = Modifiers {
            alt: true,
            ctrl: true,
            meta: true,
            shift: true,
        };
        assert_eq!(all.bits(), 15);
    }

    #[test]
    fn mouse_events_carry_button_state_and_click_count() {
        assert_eq!(
            mouse_moved(10.0, 20.0, 1, SHIFT),
            json!({"type": "mouseMoved", "x": 10.0, "y": 20.0, "button": "none", "buttons": 1, "modifiers": 8})
        );
        assert_eq!(
            mouse_button(
                true,
                1.0,
                2.0,
                MouseButton::Left,
                1,
                2,
                Modifiers::default()
            ),
            json!({"type": "mousePressed", "x": 1.0, "y": 2.0, "button": "left", "buttons": 1, "clickCount": 2, "modifiers": 0})
        );
        assert_eq!(
            mouse_button(
                false,
                1.0,
                2.0,
                MouseButton::Right,
                0,
                1,
                Modifiers::default()
            )["type"],
            "mouseReleased"
        );
        assert_eq!(MouseButton::Middle.mask(), 4);
        assert_eq!(
            mouse_wheel(5.0, 6.0, 0.0, 120.0, Modifiers::default()),
            json!({"type": "mouseWheel", "x": 5.0, "y": 6.0, "deltaX": 0.0, "deltaY": 120.0, "modifiers": 0})
        );
    }

    #[test]
    fn typed_characters_carry_text_and_us_key_codes() {
        assert_eq!(
            key(Key::Character("a"), true, Modifiers::default()),
            json!({"type": "keyDown", "key": "a", "code": "KeyA", "windowsVirtualKeyCode": 65, "modifiers": 0, "text": "a"})
        );
        assert_eq!(key(Key::Character("A"), true, SHIFT)["text"], "A");
        assert_eq!(
            key(Key::Character("7"), true, Modifiers::default())["code"],
            "Digit7"
        );
        let accented = key(Key::Character("é"), true, Modifiers::default());
        assert_eq!(
            (accented["code"].as_str(), accented["text"].as_str()),
            (Some(""), Some("é"))
        );
        assert!(
            key(Key::Character("a"), false, Modifiers::default())
                .get("text")
                .is_none()
        );
    }

    #[test]
    fn shortcuts_and_named_keys_type_no_stray_text() {
        let ctrl = Modifiers {
            ctrl: true,
            ..Modifiers::default()
        };
        assert!(key(Key::Character("a"), true, ctrl).get("text").is_none());
        let enter = key(Key::Named(NamedKey::Enter), true, Modifiers::default());
        assert_eq!(
            (
                enter["key"].as_str(),
                enter["windowsVirtualKeyCode"].as_u64(),
                enter["text"].as_str()
            ),
            (Some("Enter"), Some(13), Some("\r"))
        );
        let backspace = key(Key::Named(NamedKey::Backspace), true, Modifiers::default());
        assert!(backspace.get("text").is_none());
        assert_eq!(backspace["windowsVirtualKeyCode"], 8);
    }
}
