//! winit input -> SimpleGraphic key semantics (ui doc §2 "Key names", "Extra OnChar", "Double
//! click", "Wheel"; §5.2 desktop host rules). Owner: U7.
//!
//! Ported from SimpleGraphic (research copy master @ 8694a07; U7 re-verifies at `3b1a346`):
//! `sys_main.cpp` `GlfwKeyToKey` (printable keys are delivered as their lowercase character,
//! others by name; "GLFW defines no plus key": '=' maps to '+'), `GlfwKeyExtraChar` (BACK, TAB,
//! RETURN, ESCAPE also send a char, on repeat too), `sys_video.cpp:568-620` (mouse buttons,
//! double click: same button within 500 ms and 5 px; a detected double click resets the memory).
//! Moved from the pre-port `pob-sg/src/host.rs` by UI-U0 (key names now produce [`Key`]).

use std::time::{Duration, Instant};

use pob_sg::Key;
use winit::event::MouseButton;
use winit::keyboard::KeyCode;

/// The SG key for a physical key, as delivered to `OnKeyDown`/`OnKeyUp`. `logical` is the
/// layout's character for the key, used for letters like GLFW's translated key names.
pub fn key_from_winit(code: KeyCode, logical: Option<char>) -> Option<Key> {
    use KeyCode as K;
    let c = |b: u8| Some(Key::Char(b));
    match code {
        K::Backspace => Some(Key::Back),
        K::Tab => Some(Key::Tab),
        K::Enter | K::NumpadEnter => Some(Key::Return),
        K::ShiftLeft | K::ShiftRight => Some(Key::Shift),
        K::ControlLeft | K::ControlRight => Some(Key::Ctrl),
        K::AltLeft | K::AltRight => Some(Key::Alt),
        K::Pause => Some(Key::Pause),
        K::Escape => Some(Key::Escape),
        K::Space => c(b' '),
        K::PageUp => Some(Key::PageUp),
        K::PageDown => Some(Key::PageDown),
        K::End => Some(Key::End),
        K::Home => Some(Key::Home),
        K::ArrowLeft => Some(Key::Left),
        K::ArrowUp => Some(Key::Up),
        K::ArrowRight => Some(Key::Right),
        K::ArrowDown => Some(Key::Down),
        K::PrintScreen => Some(Key::PrintScreen),
        K::Insert => Some(Key::Insert),
        K::Delete => Some(Key::Delete),
        K::NumLock => Some(Key::NumLock),
        K::ScrollLock => Some(Key::ScrollLock),
        K::Semicolon => c(b';'),
        // "GLFW defines no plus key": '=' is mapped to '+'.
        K::Equal => c(b'+'),
        K::Comma => c(b','),
        K::Minus => c(b'-'),
        K::Period => c(b'.'),
        K::Slash => c(b'/'),
        K::Backquote => c(b'`'),
        K::BracketLeft => c(b'['),
        K::Backslash => c(b'\\'),
        K::BracketRight => c(b']'),
        K::Quote => c(b'\''),
        K::Numpad0 => c(b'0'),
        K::NumpadSubtract => c(b'-'),
        K::NumpadAdd => c(b'+'),
        K::F1 => Some(Key::F(1)),
        K::F2 => Some(Key::F(2)),
        K::F3 => Some(Key::F(3)),
        K::F4 => Some(Key::F(4)),
        K::F5 => Some(Key::F(5)),
        K::F6 => Some(Key::F(6)),
        K::F7 => Some(Key::F(7)),
        K::F8 => Some(Key::F(8)),
        K::F9 => Some(Key::F(9)),
        K::F10 => Some(Key::F(10)),
        K::F11 => Some(Key::F(11)),
        K::F12 => Some(Key::F(12)),
        K::F13 => Some(Key::F(13)),
        K::F14 => Some(Key::F(14)),
        K::F15 => Some(Key::F(15)),
        K::Digit0 => c(b'0'),
        K::Digit1 => c(b'1'),
        K::Digit2 => c(b'2'),
        K::Digit3 => c(b'3'),
        K::Digit4 => c(b'4'),
        K::Digit5 => c(b'5'),
        K::Digit6 => c(b'6'),
        K::Digit7 => c(b'7'),
        K::Digit8 => c(b'8'),
        K::Digit9 => c(b'9'),
        _ => {
            let phys = physical_letter(code)?;
            let ch = match logical {
                Some(l) if l.is_ascii_alphabetic() => l.to_ascii_lowercase(),
                _ => phys,
            };
            c(ch as u8)
        }
    }
}

fn physical_letter(code: KeyCode) -> Option<char> {
    use KeyCode as K;
    const LETTERS: [(KeyCode, char); 26] = [
        (K::KeyA, 'a'),
        (K::KeyB, 'b'),
        (K::KeyC, 'c'),
        (K::KeyD, 'd'),
        (K::KeyE, 'e'),
        (K::KeyF, 'f'),
        (K::KeyG, 'g'),
        (K::KeyH, 'h'),
        (K::KeyI, 'i'),
        (K::KeyJ, 'j'),
        (K::KeyK, 'k'),
        (K::KeyL, 'l'),
        (K::KeyM, 'm'),
        (K::KeyN, 'n'),
        (K::KeyO, 'o'),
        (K::KeyP, 'p'),
        (K::KeyQ, 'q'),
        (K::KeyR, 'r'),
        (K::KeyS, 's'),
        (K::KeyT, 't'),
        (K::KeyU, 'u'),
        (K::KeyV, 'v'),
        (K::KeyW, 'w'),
        (K::KeyX, 'x'),
        (K::KeyY, 'y'),
        (K::KeyZ, 'z'),
    ];
    LETTERS.iter().find(|(k, _)| *k == code).map(|(_, c)| *c)
}

/// `GlfwKeyExtraChar`: the char sent to `OnChar` after the KeyDown (and on repeat).
pub fn extra_char(code: KeyCode) -> Option<Key> {
    match code {
        KeyCode::Backspace => Some(Key::Char(b'\x08')),
        KeyCode::Tab => Some(Key::Char(b'\t')),
        KeyCode::Enter => Some(Key::Char(b'\r')),
        KeyCode::Escape => Some(Key::Char(b'\x1b')),
        _ => None,
    }
}

/// Mouse button keys (`ui_keyNameMap`).
pub fn mouse_button_key(b: MouseButton) -> Option<Key> {
    Some(match b {
        MouseButton::Left => Key::LeftButton,
        MouseButton::Middle => Key::MiddleButton,
        MouseButton::Right => Key::RightButton,
        MouseButton::Back => Key::Mouse4,
        MouseButton::Forward => Key::Mouse5,
        MouseButton::Other(_) => return None,
    })
}

/// One `OnChar` per byte of typed text: ASCII as `Char(b)`, anything else as `Unknown` ("?",
/// NameForKey of a codepoint >= 128); control characters coming from text events are dropped.
pub fn text_chars(text: &str) -> impl Iterator<Item = Key> + '_ {
    text.chars().filter(|c| !c.is_control()).map(|c| {
        if c.is_ascii() {
            Key::Char(c as u8)
        } else {
            Key::Unknown
        }
    })
}

/// `ui_main.cpp:530-541`: KeyUp(F10) (debug UI toggle) and KeyUp(PAUSE) with Shift held are not
/// delivered to Lua.
pub fn swallow_key_up(key: Key, shift_held: bool) -> bool {
    key == Key::F(10) || (key == Key::Pause && shift_held)
}

/// Double-click detection (`sys_video.cpp` mouse button callback).
#[derive(Default, Debug)]
pub struct DoubleClick {
    last: Option<(Instant, (i32, i32), Key)>,
}

impl DoubleClick {
    pub const DELAY: Duration = Duration::from_millis(500);
    pub const RANGE: i32 = 5;

    /// Register a button press at `pos` (window pixels; `None` when the cursor position is
    /// unknown, which disables detection). Returns whether it is a double click.
    pub fn press(&mut self, button: Key, pos: Option<(i32, i32)>, now: Instant) -> bool {
        let Some(pos) = pos else { return false };
        let dbl = matches!(self.last, Some((t, p, b)) if b == button
            && now.duration_since(t) <= Self::DELAY
            && (p.0 - pos.0).abs() <= Self::RANGE
            && (p.1 - pos.1).abs() <= Self::RANGE);
        self.last = if dbl { None } else { Some((now, pos, button)) };
        dbl
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn key_names() {
        let n = |code, l| key_from_winit(code, l).map(|k| k.name().into_owned());
        assert_eq!(n(KeyCode::KeyA, Some('A')).as_deref(), Some("a"));
        assert_eq!(n(KeyCode::KeyQ, Some('a')).as_deref(), Some("a")); // AZERTY: layout letter
        assert_eq!(n(KeyCode::KeyZ, None).as_deref(), Some("z"));
        assert_eq!(n(KeyCode::Digit7, Some('/')).as_deref(), Some("7"));
        assert_eq!(n(KeyCode::Equal, None).as_deref(), Some("+"));
        assert_eq!(n(KeyCode::Enter, None).as_deref(), Some("RETURN"));
        assert_eq!(n(KeyCode::NumpadEnter, None).as_deref(), Some("RETURN"));
        assert_eq!(n(KeyCode::ControlRight, None).as_deref(), Some("CTRL"));
        assert_eq!(n(KeyCode::F12, None).as_deref(), Some("F12"));
        assert_eq!(n(KeyCode::Space, None).as_deref(), Some(" "));
        assert_eq!(n(KeyCode::Numpad5, None), None); // GLFW_KEY_KP_5 is unmapped
        assert_eq!(extra_char(KeyCode::Backspace), Some(Key::Char(8)));
        assert_eq!(extra_char(KeyCode::NumpadEnter), None);
        assert_eq!(
            text_chars("aé\u{1}").collect::<Vec<_>>(),
            vec![Key::Char(b'a'), Key::Unknown]
        );
        assert!(
            swallow_key_up(Key::F(10), false)
                && swallow_key_up(Key::Pause, true)
                && !swallow_key_up(Key::Pause, false)
        );
    }

    #[test]
    fn double_click() {
        let mut d = DoubleClick::default();
        let t0 = Instant::now();
        assert!(!d.press(Key::LeftButton, Some((10, 10)), t0));
        assert!(d.press(
            Key::LeftButton,
            Some((14, 6)),
            t0 + Duration::from_millis(400)
        ));
        // consumed: a third click is a fresh single click
        assert!(!d.press(
            Key::LeftButton,
            Some((14, 6)),
            t0 + Duration::from_millis(450)
        ));
        // other button, too far, too late
        assert!(!d.press(
            Key::RightButton,
            Some((14, 6)),
            t0 + Duration::from_millis(500)
        ));
        assert!(!d.press(
            Key::RightButton,
            Some((20, 6)),
            t0 + Duration::from_millis(600)
        ));
        assert!(!d.press(
            Key::RightButton,
            Some((20, 6)),
            t0 + Duration::from_millis(1200)
        ));
        assert!(!d.press(Key::LeftButton, None, t0));
    }
}
