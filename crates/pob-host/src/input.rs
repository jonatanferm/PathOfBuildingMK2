//! winit input -> SimpleGraphic key semantics (ui doc §2 "Key names", "Extra OnChar", "Double
//! click", "Wheel", "Swallowed keys"; §5.2 desktop host rules). Owner: U7.
//!
//! Port of PathOfBuilding-SimpleGraphic `3b1a346`:
//! - `engine/system/win/sys_main.cpp:250-345` `ImGui_ImplGlfw_TranslateUntranslatedKey`,
//!   `GlfwKeyToKey`, `GlfwKeyExtraChar` ([`key_from_winit`], [`extra_char`]);
//! - `engine/system/win/sys_video.cpp:476-620` the GLFW callbacks (cursor enter/pos, char, key,
//!   mouse button with double-click detection, scroll) ([`WinitInput::translate`]);
//! - GLFW `_glfwInputChar` (`input.c`): codepoints `< 32` and `127..=159` are dropped, and a char
//!   is "plain" only without Alt on Windows (`WM_SYSCHAR`) ([`char_key`], [`chars_allowed`]);
//! - `ui_main.cpp:16-61, 516-572` `KeyEvent` / `CallKeyHandler` / `NameForKey`
//!   ([`swallow_key_down`], [`swallow_key_up`], [`char_key`]); `ui_console.cpp:99-111`.
//!
//! The SG-level events are [`Input`]s; `crate::deliver` turns them into `App` callbacks. The
//! headless host and the `.pobscript` expansion produce the same [`Input`]s.
//!
//! DIVERGENCE(no SG console): `ui_console.cpp:101-104` toggles SimpleGraphic's developer console
//! on KeyDown of `` ` ``/`F1` with Ctrl held and the open console then takes every event. The
//! port has no console: the toggle KeyDown is swallowed ([`swallow_key_down`]) so Lua sees the
//! same events as with a closed console, and nothing opens.
//! DIVERGENCE(winit wheel): a `PixelDelta` scroll (touchpads on macOS/Wayland) accumulates and
//! gives one Down+Up pair per 40 logical px (ui doc §5.2); GLFW gives one pair per callback.
//! DIVERGENCE(cursor outside the window): `GetRelativeCursor` (`sys_video.cpp:735-741`) queries
//! the OS cursor even outside the window; winit reports no motion there, so the last position
//! inside the window is kept.

use pob_sg::Key;
use winit::event::{ElementState, MouseButton, MouseScrollDelta, WindowEvent};
use winit::keyboard::{Key as WKey, KeyCode, PhysicalKey};

/// One SimpleGraphic input event (what `sys->core->KeyEvent` receives).
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Input {
    /// A key or mouse button changes state (the GLFW key/mouse-button callbacks): `heldKeyState`
    /// is written first, then `KeyEvent(key, down ? (double_click ? KE_DBLCLK : KE_KEYDOWN) :
    /// KE_KEYUP)`. The extra char of BACK/TAB/RETURN/ESCAPE is a separate [`Input::Char`].
    Key {
        key: Key,
        down: bool,
        double_click: bool,
    },
    /// One scroll callback: `KeyEvent(WHEEL*, KE_KEYDOWN)` then `KeyEvent(WHEEL*, KE_KEYUP)`;
    /// the held state is not touched (`sys_video.cpp:608-620`).
    Wheel { up: bool },
    /// `KeyEvent(codepoint, KE_CHAR)`, already converted by [`char_key`].
    Char(Key),
}

/// lua: sys_main.cpp:252-275 `ImGui_ImplGlfw_TranslateUntranslatedKey`: when the layout names the
/// key with one character that is a digit, a letter or one of `` `-=[]\,;'./ ``, the key is
/// re-mapped to the US key carrying that character. Keypad keys are never translated.
fn translate_untranslated(code: KeyCode, key_name: Option<char>) -> KeyCode {
    use KeyCode as K;
    // GLFW_KEY_KP_0 ..= GLFW_KEY_KP_EQUAL
    if matches!(
        code,
        K::Numpad0
            | K::Numpad1
            | K::Numpad2
            | K::Numpad3
            | K::Numpad4
            | K::Numpad5
            | K::Numpad6
            | K::Numpad7
            | K::Numpad8
            | K::Numpad9
            | K::NumpadDecimal
            | K::NumpadDivide
            | K::NumpadMultiply
            | K::NumpadSubtract
            | K::NumpadAdd
            | K::NumpadEnter
            | K::NumpadEqual
    ) {
        return code;
    }
    let Some(ch) = key_name else { return code };
    const DIGITS: [KeyCode; 10] = [
        K::Digit0,
        K::Digit1,
        K::Digit2,
        K::Digit3,
        K::Digit4,
        K::Digit5,
        K::Digit6,
        K::Digit7,
        K::Digit8,
        K::Digit9,
    ];
    if ch.is_ascii_digit() {
        return DIGITS[(ch as u8 - b'0') as usize];
    }
    if ch.is_ascii_alphabetic() {
        return LETTERS[(ch.to_ascii_lowercase() as u8 - b'a') as usize];
    }
    // const char char_names[] = "`-=[]\\,;\'./";
    match ch {
        '`' => K::Backquote,
        '-' => K::Minus,
        '=' => K::Equal,
        '[' => K::BracketLeft,
        ']' => K::BracketRight,
        '\\' => K::Backslash,
        ',' => K::Comma,
        ';' => K::Semicolon,
        '\'' => K::Quote,
        '.' => K::Period,
        '/' => K::Slash,
        _ => code,
    }
}

const LETTERS: [KeyCode; 26] = {
    use KeyCode as K;
    [
        K::KeyA,
        K::KeyB,
        K::KeyC,
        K::KeyD,
        K::KeyE,
        K::KeyF,
        K::KeyG,
        K::KeyH,
        K::KeyI,
        K::KeyJ,
        K::KeyK,
        K::KeyL,
        K::KeyM,
        K::KeyN,
        K::KeyO,
        K::KeyP,
        K::KeyQ,
        K::KeyR,
        K::KeyS,
        K::KeyT,
        K::KeyU,
        K::KeyV,
        K::KeyW,
        K::KeyX,
        K::KeyY,
        K::KeyZ,
    ]
};

/// lua: sys_main.cpp:277-345 `GlfwKeyToKey`: the SG key delivered to `OnKeyDown`/`OnKeyUp` for a
/// physical key (`None` = SG key 0, no event at all). `key_name` is the layout's character for the
/// key without modifiers (`glfwGetKeyName`), used by `TranslateUntranslatedKey`.
pub fn key_from_winit(code: KeyCode, key_name: Option<char>) -> Option<Key> {
    use KeyCode as K;
    let c = |b: u8| Some(Key::Char(b));
    let code = translate_untranslated(code, key_name);
    // s_lookup
    match code {
        K::Backspace => return Some(Key::Back),
        K::Tab => return Some(Key::Tab),
        K::Enter => return Some(Key::Return),
        K::ShiftLeft | K::ShiftRight => return Some(Key::Shift),
        K::ControlLeft | K::ControlRight => return Some(Key::Ctrl),
        K::AltLeft | K::AltRight => return Some(Key::Alt),
        K::Pause => return Some(Key::Pause),
        K::Escape => return Some(Key::Escape),
        K::Space => return c(b' '),
        K::PageUp => return Some(Key::PageUp),
        K::PageDown => return Some(Key::PageDown),
        K::End => return Some(Key::End),
        K::Home => return Some(Key::Home),
        K::ArrowLeft => return Some(Key::Left),
        K::ArrowUp => return Some(Key::Up),
        K::ArrowRight => return Some(Key::Right),
        K::ArrowDown => return Some(Key::Down),
        K::PrintScreen => return Some(Key::PrintScreen),
        K::Insert => return Some(Key::Insert),
        K::Delete => return Some(Key::Delete),
        K::NumLock => return Some(Key::NumLock),
        K::ScrollLock => return Some(Key::ScrollLock),
        K::Semicolon => return c(b';'),
        // "GLFW defines no plus key"
        K::Equal => return c(b'+'),
        K::Comma => return c(b','),
        K::Minus => return c(b'-'),
        K::Period => return c(b'.'),
        K::Slash => return c(b'/'),
        K::Backquote => return c(b'`'),
        K::BracketLeft => return c(b'['),
        K::Backslash => return c(b'\\'),
        K::BracketRight => return c(b']'),
        K::Quote => return c(b'\''),
        K::Numpad0 => return c(b'0'),
        K::NumpadSubtract => return c(b'-'),
        K::NumpadAdd => return c(b'+'),
        K::NumpadEnter => return Some(Key::Return),
        _ => {}
    }
    // GLFW_KEY_F1 ..= GLFW_KEY_F15
    const FKEYS: [KeyCode; 15] = [
        K::F1,
        K::F2,
        K::F3,
        K::F4,
        K::F5,
        K::F6,
        K::F7,
        K::F8,
        K::F9,
        K::F10,
        K::F11,
        K::F12,
        K::F13,
        K::F14,
        K::F15,
    ];
    if let Some(i) = FKEYS.iter().position(|k| *k == code) {
        return Some(Key::F(i as u8 + 1));
    }
    // GLFW_KEY_0 ..= GLFW_KEY_9
    const DIGITS: [(KeyCode, u8); 10] = [
        (K::Digit0, b'0'),
        (K::Digit1, b'1'),
        (K::Digit2, b'2'),
        (K::Digit3, b'3'),
        (K::Digit4, b'4'),
        (K::Digit5, b'5'),
        (K::Digit6, b'6'),
        (K::Digit7, b'7'),
        (K::Digit8, b'8'),
        (K::Digit9, b'9'),
    ];
    if let Some((_, d)) = DIGITS.iter().find(|(k, _)| *k == code) {
        return c(*d);
    }
    // GLFW_KEY_A ..= GLFW_KEY_Z
    if let Some(i) = LETTERS.iter().position(|k| *k == code) {
        return c(b'a' + i as u8);
    }
    None
}

/// lua: sys_main.cpp:347-361 `GlfwKeyExtraChar`: the char sent to `OnChar` after the KeyDown (also
/// on repeat). Keyed on the *untranslated* GLFW key, so keypad Enter sends none.
pub fn extra_char(code: KeyCode) -> Option<Key> {
    match code {
        KeyCode::Backspace => Some(Key::Char(b'\x08')),
        KeyCode::Tab => Some(Key::Char(b'\t')),
        KeyCode::Enter => Some(Key::Char(b'\r')),
        KeyCode::Escape => Some(Key::Char(b'\x1b')),
        _ => None,
    }
}

/// lua: sys_video.cpp:576-597 the mouse button switch (`ui_keyNameMap` keys); other buttons
/// are ignored.
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

/// What Lua's `OnChar` receives for a typed codepoint, or `None` when GLFW drops it.
///
/// GLFW `_glfwInputChar` drops `cp < 32` and `127..=159`. `CallKeyHandler`
/// (`ui_main.cpp:548-557`) formats `cp < 128` with `"%c"`, otherwise `NameForKey(cp)`, which
/// looks the codepoint up in `keyList_e` by value: U+00A0..U+00AA arrive as key names (`£`
/// U+00A3 → `"F10"`, `©` U+00A9 → `"NUMLOCK"`), anything else as `"?"` (CCR-0021 item 6).
/// The naming is [`Key::from_sg_code`]; only the GLFW filter is applied here.
pub fn char_key(cp: u32) -> Option<Key> {
    if cp < 32 || (cp > 126 && cp < 160) {
        return None;
    }
    Some(Key::from_sg_code(cp))
}

/// The `OnChar` keys for a text event (one per codepoint, GLFW-filtered).
pub fn text_chars(text: &str) -> impl Iterator<Item = Key> + '_ {
    text.chars().filter_map(|c| char_key(c as u32))
}

/// Whether a text event reaches the char callback, given the modifier state.
///
/// SG's reference platform is Windows: `WM_CHAR` is delivered ("plain"), `WM_SYSCHAR` (Alt
/// without Ctrl) is not, and Ctrl without Alt yields only control characters (dropped by the
/// codepoint filter). AltGr is Ctrl+Alt on Windows and neither on X11/Wayland, and types
/// normally. winit's `text` ignores Ctrl, so the rule is applied here.
pub const fn chars_allowed(ctrl: bool, alt: bool) -> bool {
    ctrl == alt
}

/// `ui_console.cpp:101-104`: KeyDown of `` ` `` or F1 with Ctrl held toggles SG's console and
/// is not delivered (see the module DIVERGENCE note).
pub fn swallow_key_down(key: Key, ctrl_held: bool) -> bool {
    ctrl_held && (key == Key::Char(b'`') || key == Key::F(1))
}

/// `ui_main.cpp:530-541`: KeyUp(F10) (debug UI toggle) and KeyUp(PAUSE) with Shift held are not
/// delivered to Lua.
pub fn swallow_key_up(key: Key, shift_held: bool) -> bool {
    key == Key::F(10) || (key == Key::Pause && shift_held)
}

/// lua: sys_video.cpp:529-541 (the GLFW key callback) followed by the char callback, for one
/// physical key press, auto-repeat (`GLFW_REPEAT` counts as down) or release. `key_name` is
/// the layout's unmodified character (`glfwGetKeyName`), `text` what the press typed, `mods`
/// the (Ctrl, Alt) state.
///
/// - `if (byte k = GlfwKeyToKey(..))`: KeyDown/KeyUp, then on down the extra char of
///   BACK/TAB/RETURN/ESCAPE ([`extra_char`]), on press **and** on every repeat;
/// - then the char callback (`WM_CHAR` follows the key message) for the typed text, unless the
///   key already sent its extra char (Windows types the same control character, which GLFW
///   drops) or the modifiers make it a non-character message ([`chars_allowed`]).
pub fn keyboard(
    code: KeyCode,
    key_name: Option<char>,
    down: bool,
    text: Option<&str>,
    mods: (bool, bool),
    out: &mut Vec<Input>,
) {
    if let Some(key) = key_from_winit(code, key_name) {
        out.push(Input::Key {
            key,
            down,
            double_click: false,
        });
        if down {
            if let Some(ch) = extra_char(code) {
                out.push(Input::Char(ch));
            }
        }
    }
    if down && extra_char(code).is_none() && chars_allowed(mods.0, mods.1) {
        if let Some(text) = text {
            out.extend(text_chars(text).map(Input::Char));
        }
    }
}

/// Double-click detection (`sys_video.cpp:598-616`): same button within 500 ms and ±5 px of the
/// last click; a detected double click resets the memory, so a triple click is dbl, single.
#[derive(Default, Debug)]
pub struct DoubleClick {
    last: Option<(f64, (i32, i32), Key)>,
}

impl DoubleClick {
    /// `DOUBLECLICK_DELAY` in ms.
    pub const DELAY_MS: f64 = 500.0;
    /// `DOUBLECLICK_RANGE` in px.
    pub const RANGE: i32 = 5;

    /// Register a button press at `pos` (`lastCursorPos`: window px truncated to int; `None`
    /// when the cursor left the window, which disables detection and keeps the memory) at
    /// `now_ms`. Returns whether it is a double click.
    pub fn press(&mut self, button: Key, pos: Option<(i32, i32)>, now_ms: f64) -> bool {
        let Some(pos) = pos else { return false };
        let dbl = matches!(self.last, Some((t, p, b)) if b == button
            && now_ms - t <= Self::DELAY_MS
            && (p.0 - pos.0).abs() <= Self::RANGE
            && (p.1 - pos.1).abs() <= Self::RANGE);
        self.last = if dbl {
            None
        } else {
            Some((now_ms, pos, button))
        };
        dbl
    }
}

/// The GLFW callback state of one window, fed with winit events (desktop and web hosts).
#[derive(Debug)]
pub struct WinitInput {
    pub dbl: DoubleClick,
    wheel_acc: f64,
    /// `lastCursorPos` (`(int)x, (int)y`; reset when the cursor leaves).
    last_cursor: Option<(i32, i32)>,
    /// The last physical cursor position seen (`GetRelativeCursor`).
    cursor: (f64, f64),
    /// `IsActive()` (window focused).
    pub active: bool,
    /// `cursorInWindow`.
    pub cursor_over: bool,
    ctrl: bool,
    alt: bool,
}

impl Default for WinitInput {
    fn default() -> Self {
        WinitInput {
            dbl: DoubleClick::default(),
            wheel_acc: 0.0,
            last_cursor: None,
            cursor: (0.0, 0.0),
            active: true,
            cursor_over: false,
            ctrl: false,
            alt: false,
        }
    }
}

impl WinitInput {
    /// `GetRelativeCursor` (`sys_video.cpp:735-741`): `floor` of the physical position.
    pub fn cursor_phys(&self) -> (i32, i32) {
        (self.cursor.0.floor() as i32, self.cursor.1.floor() as i32)
    }

    /// Translate one winit event into SG input events (in delivery order). `now_ms` is the
    /// host clock (double clicks), `scale_factor` the window's (pixel-delta wheel).
    pub fn translate(&mut self, event: &WindowEvent, now_ms: f64, scale_factor: f64) -> Vec<Input> {
        let mut out = Vec::new();
        match event {
            WindowEvent::Focused(f) => self.active = *f,
            WindowEvent::CursorEntered { .. } => self.cursor_over = true,
            WindowEvent::CursorLeft { .. } => {
                // glfwSetCursorEnterCallback: lastCursorPos.reset() on leave.
                self.cursor_over = false;
                self.last_cursor = None;
            }
            WindowEvent::CursorMoved { position, .. } => {
                self.cursor = (position.x, position.y);
                self.last_cursor = Some((position.x as i32, position.y as i32));
            }
            WindowEvent::ModifiersChanged(m) => {
                self.ctrl = m.state().control_key();
                self.alt = m.state().alt_key();
            }
            WindowEvent::MouseInput { state, button, .. } => {
                let Some(key) = mouse_button_key(*button) else {
                    return out;
                };
                let down = *state == ElementState::Pressed;
                let double_click = down && self.dbl.press(key, self.last_cursor, now_ms);
                out.push(Input::Key {
                    key,
                    down,
                    double_click,
                });
            }
            WindowEvent::MouseWheel { delta, .. } => match delta {
                MouseScrollDelta::LineDelta(_, y) => {
                    if *y > 0.0 {
                        out.push(Input::Wheel { up: true });
                    } else if *y < 0.0 {
                        out.push(Input::Wheel { up: false });
                    }
                }
                MouseScrollDelta::PixelDelta(p) => {
                    let step = 40.0 * scale_factor;
                    self.wheel_acc += p.y;
                    while self.wheel_acc >= step {
                        self.wheel_acc -= step;
                        out.push(Input::Wheel { up: true });
                    }
                    while self.wheel_acc <= -step {
                        self.wheel_acc += step;
                        out.push(Input::Wheel { up: false });
                    }
                }
            },
            WindowEvent::KeyboardInput { event, .. } => {
                let PhysicalKey::Code(code) = event.physical_key else {
                    return out;
                };
                let down = event.state == ElementState::Pressed;
                // glfwGetKeyName: the layout's character without modifiers.
                #[cfg(not(target_arch = "wasm32"))]
                let unmodified = {
                    use winit::platform::modifier_supplement::KeyEventExtModifierSupplement;
                    event.key_without_modifiers()
                };
                #[cfg(target_arch = "wasm32")]
                let unmodified = event.logical_key.clone();
                let key_name = match &unmodified {
                    WKey::Character(s) => {
                        let mut it = s.chars();
                        match (it.next(), it.next()) {
                            (Some(c), None) => Some(c),
                            _ => None,
                        }
                    }
                    _ => None,
                };
                keyboard(
                    code,
                    key_name,
                    down,
                    event.text.as_deref(),
                    (self.ctrl, self.alt),
                    &mut out,
                );
            }
            _ => {}
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn n(code: KeyCode, l: Option<char>) -> Option<String> {
        key_from_winit(code, l).map(|k| k.name().into_owned())
    }

    /// Named keys (`ui_keyNameMap`) from the physical keys of `GlfwKeyToKey`.
    #[test]
    fn named_keys() {
        let table: &[(KeyCode, &str)] = &[
            (KeyCode::Backspace, "BACK"),
            (KeyCode::Tab, "TAB"),
            (KeyCode::Enter, "RETURN"),
            (KeyCode::NumpadEnter, "RETURN"),
            (KeyCode::ShiftLeft, "SHIFT"),
            (KeyCode::ShiftRight, "SHIFT"),
            (KeyCode::ControlLeft, "CTRL"),
            (KeyCode::ControlRight, "CTRL"),
            (KeyCode::AltLeft, "ALT"),
            (KeyCode::AltRight, "ALT"),
            (KeyCode::Pause, "PAUSE"),
            (KeyCode::Escape, "ESCAPE"),
            (KeyCode::PageUp, "PAGEUP"),
            (KeyCode::PageDown, "PAGEDOWN"),
            (KeyCode::End, "END"),
            (KeyCode::Home, "HOME"),
            (KeyCode::ArrowLeft, "LEFT"),
            (KeyCode::ArrowUp, "UP"),
            (KeyCode::ArrowRight, "RIGHT"),
            (KeyCode::ArrowDown, "DOWN"),
            (KeyCode::PrintScreen, "PRINTSCREEN"),
            (KeyCode::Insert, "INSERT"),
            (KeyCode::Delete, "DELETE"),
            (KeyCode::NumLock, "NUMLOCK"),
            (KeyCode::ScrollLock, "SCROLLLOCK"),
            (KeyCode::F1, "F1"),
            (KeyCode::F10, "F10"),
            (KeyCode::F12, "F12"),
            (KeyCode::F15, "F15"),
        ];
        for (code, name) in table {
            assert_eq!(n(*code, None).as_deref(), Some(*name), "{code:?}");
        }
        // Keys GLFW (or GlfwKeyToKey) leaves at 0: no event.
        for code in [
            KeyCode::F16,
            KeyCode::Numpad5,
            KeyCode::NumpadMultiply,
            KeyCode::NumpadDecimal,
            KeyCode::CapsLock,
            KeyCode::SuperLeft,
            KeyCode::ContextMenu,
            KeyCode::IntlBackslash,
        ] {
            assert_eq!(n(code, None), None, "{code:?}");
        }
        assert_eq!(n(KeyCode::Numpad5, Some('5')), None); // keypad never translated
    }

    /// Printable keys arrive as their lowercase character ("%c"), '=' as '+'.
    #[test]
    fn lowercase_chars() {
        assert_eq!(n(KeyCode::KeyA, Some('a')).as_deref(), Some("a"));
        assert_eq!(n(KeyCode::KeyA, Some('A')).as_deref(), Some("a"));
        assert_eq!(n(KeyCode::KeyZ, None).as_deref(), Some("z"));
        assert_eq!(n(KeyCode::Space, Some(' ')).as_deref(), Some(" "));
        assert_eq!(n(KeyCode::Digit7, None).as_deref(), Some("7"));
        assert_eq!(n(KeyCode::Numpad0, None).as_deref(), Some("0"));
        assert_eq!(n(KeyCode::NumpadAdd, None).as_deref(), Some("+"));
        assert_eq!(n(KeyCode::NumpadSubtract, None).as_deref(), Some("-"));
        let punct = [
            (KeyCode::Semicolon, ";"),
            (KeyCode::Equal, "+"),
            (KeyCode::Comma, ","),
            (KeyCode::Minus, "-"),
            (KeyCode::Period, "."),
            (KeyCode::Slash, "/"),
            (KeyCode::Backquote, "`"),
            (KeyCode::BracketLeft, "["),
            (KeyCode::Backslash, "\\"),
            (KeyCode::BracketRight, "]"),
            (KeyCode::Quote, "'"),
        ];
        for (code, s) in punct {
            assert_eq!(n(code, None).as_deref(), Some(s), "{code:?}");
        }
        // TranslateUntranslatedKey: the layout's character wins for letters, digits and the
        // 11 punctuation characters (AZERTY: physical Q types 'a', physical M position types ',').
        assert_eq!(n(KeyCode::KeyQ, Some('a')).as_deref(), Some("a"));
        assert_eq!(n(KeyCode::KeyM, Some(',')).as_deref(), Some(","));
        assert_eq!(n(KeyCode::Semicolon, Some('m')).as_deref(), Some("m"));
        assert_eq!(n(KeyCode::Equal, Some('=')).as_deref(), Some("+"));
        // ...anything else keeps the physical key (AZERTY '&' on Digit1, German 'ß' on Minus,
        // shifted characters reported by the web backend).
        assert_eq!(n(KeyCode::Digit1, Some('&')).as_deref(), Some("1"));
        assert_eq!(n(KeyCode::Minus, Some('ß')).as_deref(), Some("-"));
        assert_eq!(n(KeyCode::Digit1, Some('!')).as_deref(), Some("1"));
        assert_eq!(n(KeyCode::Equal, Some('+')).as_deref(), Some("+"));
    }

    #[test]
    fn extra_chars() {
        assert_eq!(extra_char(KeyCode::Backspace), Some(Key::Char(8)));
        assert_eq!(extra_char(KeyCode::Tab), Some(Key::Char(b'\t')));
        assert_eq!(extra_char(KeyCode::Enter), Some(Key::Char(b'\r')));
        assert_eq!(extra_char(KeyCode::Escape), Some(Key::Char(0x1b)));
        assert_eq!(extra_char(KeyCode::NumpadEnter), None);
        assert_eq!(extra_char(KeyCode::Delete), None);
    }

    #[test]
    fn chars_non_ascii() {
        assert_eq!(char_key(u32::from('a')), Some(Key::Char(b'a')));
        assert_eq!(char_key(u32::from('A')), Some(Key::Char(b'A')));
        assert_eq!(char_key(u32::from('~')), Some(Key::Char(b'~')));
        // GLFW filter: C0, DEL, C1
        for cp in [0u32, 8, 10, 13, 27, 31, 127, 128, 150, 159] {
            assert_eq!(char_key(cp), None, "{cp}");
        }
        // NameForKey by value: U+00A0..U+00AA are keyList_e entries.
        assert_eq!(char_key(0xA0), Some(Key::F(7)));
        assert_eq!(char_key(0xA3).unwrap().name(), "F10"); // '£'
        assert_eq!(char_key(0xA7).unwrap().name(), "F14"); // '§'
        assert_eq!(char_key(0xA8).unwrap().name(), "F15");
        assert_eq!(char_key(0xA9).unwrap().name(), "NUMLOCK"); // '©'
        assert_eq!(char_key(0xAA).unwrap().name(), "SCROLLLOCK");
        // Everything else non-ASCII is "?"
        for cp in [0xABu32, 0xE9, 0x2192, 0x1F600] {
            assert_eq!(char_key(cp), Some(Key::Unknown), "{cp:x}");
            assert_eq!(char_key(cp).unwrap().name(), "?");
        }
        assert_eq!(
            text_chars("aé£\u{1}\u{7f}").collect::<Vec<_>>(),
            vec![Key::Char(b'a'), Key::Unknown, Key::F(10)]
        );
        // keyList_e by value: every named code maps back to its name.
        assert_eq!(Key::from_sg_code(128), Key::LeftButton);
        assert_eq!(Key::from_sg_code(153), Key::Right);
        assert_eq!(Key::from_sg_code(154), Key::F(1));
        assert_eq!(Key::from_sg_code(171), Key::Unknown);
    }

    #[test]
    fn modifiers_and_swallowing() {
        assert!(chars_allowed(false, false));
        assert!(chars_allowed(true, true)); // AltGr on Windows
        assert!(!chars_allowed(true, false));
        assert!(!chars_allowed(false, true));
        assert!(swallow_key_up(Key::F(10), false));
        assert!(swallow_key_up(Key::Pause, true));
        assert!(!swallow_key_up(Key::Pause, false));
        assert!(!swallow_key_up(Key::F(9), true));
        assert!(swallow_key_down(Key::Char(b'`'), true));
        assert!(swallow_key_down(Key::F(1), true));
        assert!(!swallow_key_down(Key::F(1), false));
        assert!(!swallow_key_down(Key::F(2), true));
    }

    #[test]
    fn double_click() {
        let mut d = DoubleClick::default();
        assert!(!d.press(Key::LeftButton, Some((10, 10)), 0.0));
        assert!(d.press(Key::LeftButton, Some((14, 6)), 400.0)); // +4,-4 within 5 px, 400 ms
                                                                 // consumed: a third click is a fresh single click
        assert!(!d.press(Key::LeftButton, Some((14, 6)), 450.0));
        assert!(d.press(Key::LeftButton, Some((14, 6)), 950.0)); // exactly 500 ms: still double
        assert!(!d.press(Key::LeftButton, Some((14, 6)), 1000.0));
        // other button, too far, too late
        assert!(!d.press(Key::RightButton, Some((14, 6)), 1100.0));
        assert!(!d.press(Key::RightButton, Some((20, 6)), 1200.0));
        assert!(!d.press(Key::RightButton, Some((20, 6)), 1701.0));
        // unknown position: no detection, memory kept
        assert!(!d.press(Key::RightButton, None, 1702.0));
        assert!(d.press(Key::RightButton, Some((20, 11)), 1703.0));
    }

    /// The key callback + char callback sequence: KeyDown then the extra OnChar on press and on
    /// every repeat, KeyUp without one; typed text as OnChar; Ctrl or Alt chords type nothing;
    /// non-ASCII text as `?`.
    #[test]
    fn keyboard_sequences() {
        let run = |code, name, down, text: Option<&str>, mods| {
            let mut v = Vec::new();
            keyboard(code, name, down, text, mods, &mut v);
            v
        };
        let kd = |key| Input::Key {
            key,
            down: true,
            double_click: false,
        };
        let ku = |key| Input::Key {
            key,
            down: false,
            double_click: false,
        };
        let none = (false, false);
        // Backspace: press and two repeats (winit repeats are Pressed events), then release.
        for _ in 0..3 {
            assert_eq!(
                run(KeyCode::Backspace, None, true, Some("\u{8}"), none),
                vec![kd(Key::Back), Input::Char(Key::Char(8))]
            );
        }
        assert_eq!(
            run(KeyCode::Backspace, None, false, None, none),
            vec![ku(Key::Back)]
        );
        for (code, key, ch) in [
            (KeyCode::Tab, Key::Tab, b'\t'),
            (KeyCode::Enter, Key::Return, b'\r'),
            (KeyCode::Escape, Key::Escape, 0x1b),
        ] {
            assert_eq!(
                run(code, None, true, Some("x"), none),
                vec![kd(key), Input::Char(Key::Char(ch))]
            );
            assert_eq!(run(code, None, false, None, none), vec![ku(key)]);
        }
        // Keypad Enter: RETURN without the extra char; its "\r" text is a control character.
        assert_eq!(
            run(KeyCode::NumpadEnter, None, true, Some("\r"), none),
            vec![kd(Key::Return)]
        );
        // Shift+a: KeyDown "a" (lowercase), OnChar "A".
        assert_eq!(
            run(KeyCode::KeyA, Some('a'), true, Some("A"), none),
            vec![kd(Key::Char(b'a')), Input::Char(Key::Char(b'A'))]
        );
        // Ctrl+c and Alt+c: KeyDown only.
        for mods in [(true, false), (false, true)] {
            assert_eq!(
                run(KeyCode::KeyC, Some('c'), true, Some("c"), mods),
                vec![kd(Key::Char(b'c'))]
            );
        }
        // AltGr on Windows (Ctrl+Alt) types.
        assert_eq!(
            run(KeyCode::KeyQ, Some('q'), true, Some("@"), (true, true)),
            vec![kd(Key::Char(b'q')), Input::Char(Key::Char(b'@'))]
        );
        // A key GlfwKeyToKey leaves at 0 that still types: only the char, "?" when non-ASCII.
        assert_eq!(
            run(KeyCode::IntlBackslash, None, true, Some("é<"), none),
            vec![Input::Char(Key::Unknown), Input::Char(Key::Char(b'<'))]
        );
        // F10 and Pause KeyUps are translated; `crate::deliver` swallows them.
        assert_eq!(
            run(KeyCode::F10, None, false, None, none),
            vec![ku(Key::F(10))]
        );
    }

    fn ev_mouse(down: bool) -> WindowEvent {
        WindowEvent::MouseInput {
            device_id: winit::event::DeviceId::dummy(),
            state: if down {
                ElementState::Pressed
            } else {
                ElementState::Released
            },
            button: MouseButton::Left,
        }
    }

    fn ev_move(x: f64, y: f64) -> WindowEvent {
        WindowEvent::CursorMoved {
            device_id: winit::event::DeviceId::dummy(),
            position: winit::dpi::PhysicalPosition::new(x, y),
        }
    }

    /// The winit-event layer: double click incl. the reset, leave disabling it, wheel pairs,
    /// cursor flooring.
    #[test]
    fn winit_mouse_and_wheel() {
        let mut w = WinitInput::default();
        let k = |down, dbl| Input::Key {
            key: Key::LeftButton,
            down,
            double_click: dbl,
        };
        assert!(w.translate(&ev_move(10.7, -0.5), 0.0, 1.0).is_empty());
        assert_eq!(w.cursor_phys(), (10, -1)); // floor
        assert_eq!(w.translate(&ev_mouse(true), 0.0, 1.0), vec![k(true, false)]);
        assert_eq!(
            w.translate(&ev_mouse(false), 50.0, 1.0),
            vec![k(false, false)]
        );
        assert_eq!(
            w.translate(&ev_mouse(true), 100.0, 1.0),
            vec![k(true, true)]
        );
        assert_eq!(
            w.translate(&ev_mouse(false), 150.0, 1.0),
            vec![k(false, false)]
        );
        assert_eq!(
            w.translate(&ev_mouse(true), 200.0, 1.0),
            vec![k(true, false)]
        ); // reset
           // leaving resets lastCursorPos: no double click until the cursor moves again
        w.translate(
            &WindowEvent::CursorLeft {
                device_id: winit::event::DeviceId::dummy(),
            },
            210.0,
            1.0,
        );
        assert_eq!(
            w.translate(&ev_mouse(true), 220.0, 1.0),
            vec![k(true, false)]
        );
        assert_eq!(w.cursor_phys(), (10, -1)); // last known position kept
        let wheel = |dy: f32| WindowEvent::MouseWheel {
            device_id: winit::event::DeviceId::dummy(),
            delta: MouseScrollDelta::LineDelta(0.0, dy),
            phase: winit::event::TouchPhase::Moved,
        };
        assert_eq!(
            w.translate(&wheel(3.0), 0.0, 1.0),
            vec![Input::Wheel { up: true }]
        );
        assert_eq!(
            w.translate(&wheel(-0.1), 0.0, 1.0),
            vec![Input::Wheel { up: false }]
        );
        assert!(w.translate(&wheel(0.0), 0.0, 1.0).is_empty());
        let px = |dy: f64| WindowEvent::MouseWheel {
            device_id: winit::event::DeviceId::dummy(),
            delta: MouseScrollDelta::PixelDelta(winit::dpi::PhysicalPosition::new(0.0, dy)),
            phase: winit::event::TouchPhase::Moved,
        };
        assert!(w.translate(&px(50.0), 0.0, 1.5).is_empty()); // 50 < 60
        assert_eq!(
            w.translate(&px(70.0), 0.0, 1.5),
            vec![Input::Wheel { up: true }, Input::Wheel { up: true }]
        );
    }
}
