//! Shared SimpleGraphic value types (ui doc §4.1). Complete in UI-U0; owned by U1 afterwards.
//!
//! Sources (PathOfBuilding-SimpleGraphic `3b1a346`, re-verified by U1; see
//! `docs/port/ui/sg-semantics.md`): `ui_api.cpp:1145-1146` `alignMap` / `fontMap` (font and
//! alignment names, `luaL_checkoption` order), `ui_main.cpp:16-61` `ui_keyNameMap`,
//! `:548-591` `CallKeyHandler` / `NameForKey` / `KeyForName` (key names),
//! `engine/common/keylist.h` (`keyList_e`: `KEY_LMOUSE = 128` .. `KEY_SCROLL = 170`),
//! `engine/render/r_main.h:24-29` `r_viewport_s` (physical viewport), `sys_video.cpp` (screen
//! info).

use std::borrow::Cow;

/// Every number the Lua UI computes (D11).
pub type Num = f64;

/// A rectangle in physical pixels (`r_viewport_s` `lo` / `extent`).
#[derive(
    Clone, Copy, Debug, Default, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize,
)]
pub struct IRect {
    pub x: i32,
    pub y: i32,
    pub w: i32,
    pub h: i32,
}

/// What the host tells `Sg` at `begin_frame`.
#[derive(Clone, Copy, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct ScreenInfo {
    /// Framebuffer px (`vid.fbSize`) == VirtualScreenWidth in DPI_AWARE mode.
    pub fb_w: i32,
    pub fb_h: i32,
    /// `vid.size[1]` (line-visibility test; == fb_h on non-Windows, see ui doc §2).
    pub win_h: i32,
    /// OS content scale (`vid.dpiScale`).
    pub dpi_scale: f32,
}

/// Text alignment (`alignMap` order: LEFT, CENTER, RIGHT, CENTER_X, RIGHT_X). A nil align is Left.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub enum Align {
    Left,
    Center,
    Right,
    CenterX,
    RightX,
}

impl Align {
    /// `luaL_checkoption(L, 3, "LEFT", alignMap)`: nil means LEFT; an unknown name is a Lua error
    /// in SG (returns None here).
    pub fn from_lua(s: Option<&str>) -> Option<Align> {
        Some(match s {
            None | Some("LEFT") => Align::Left,
            Some("CENTER") => Align::Center,
            Some("RIGHT") => Align::Right,
            Some("CENTER_X") => Align::CenterX,
            Some("RIGHT_X") => Align::RightX,
            Some(_) => return None,
        })
    }

    /// The Lua name.
    pub const fn lua_name(self) -> &'static str {
        match self {
            Align::Left => "LEFT",
            Align::Center => "CENTER",
            Align::Right => "RIGHT",
            Align::CenterX => "CENTER_X",
            Align::RightX => "RIGHT_X",
        }
    }
}

/// The seven PoB fonts in `fontMap` order (also the `F_*` enum order).
#[derive(
    Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, serde::Serialize, serde::Deserialize,
)]
#[repr(u8)]
pub enum Font {
    Fixed,
    Var,
    VarBold,
    FontinSc,
    FontinScItalic,
    Fontin,
    FontinItalic,
}

impl Font {
    pub const ALL: [Font; 7] = [
        Font::Fixed,
        Font::Var,
        Font::VarBold,
        Font::FontinSc,
        Font::FontinScItalic,
        Font::Fontin,
        Font::FontinItalic,
    ];

    /// File base name under `runtime/SimpleGraphic/Fonts/` (`r_renderer_c::Init`).
    pub const fn file_stem(self) -> &'static str {
        match self {
            Font::Fixed => "Bitstream Vera Sans Mono",
            Font::Var => "Liberation Sans",
            Font::VarBold => "Liberation Sans Bold",
            Font::FontinSc => "Fontin SmallCaps",
            Font::FontinScItalic => "Fontin SmallCaps Italic",
            Font::Fontin => "Fontin",
            Font::FontinItalic => "Fontin Italic",
        }
    }

    /// "FIXED" | "VAR" | "VAR BOLD" | "FONTIN SC" | "FONTIN SC ITALIC" | "FONTIN" | "FONTIN ITALIC".
    /// Unknown names are a Lua error in SG (`luaL_checkoption`, exact and case-sensitive); returns None.
    pub fn from_lua(s: &str) -> Option<Font> {
        Font::ALL.into_iter().find(|f| f.lua_name() == s)
    }

    /// The Lua name.
    pub const fn lua_name(self) -> &'static str {
        match self {
            Font::Fixed => "FIXED",
            Font::Var => "VAR",
            Font::VarBold => "VAR BOLD",
            Font::FontinSc => "FONTIN SC",
            Font::FontinScItalic => "FONTIN SC ITALIC",
            Font::Fontin => "FONTIN",
            Font::FontinItalic => "FONTIN ITALIC",
        }
    }

    /// Index into a `[_; 7]` table in `fontMap` order.
    pub const fn index(self) -> usize {
        self as usize
    }
}

/// What Lua receives in OnKeyDown/OnKeyUp/OnChar and passes to IsKeyDown (ui_main.cpp:16-61, 547-590).
#[derive(
    Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, serde::Serialize, serde::Deserialize,
)]
pub enum Key {
    LeftButton,
    MiddleButton,
    RightButton,
    Mouse4,
    Mouse5,
    WheelUp,
    WheelDown,
    Back,
    Tab,
    Return,
    Escape,
    Shift,
    Ctrl,
    Alt,
    Pause,
    PageUp,
    PageDown,
    End,
    Home,
    PrintScreen,
    Insert,
    Delete,
    Up,
    Down,
    Left,
    Right,
    /// F1..=F15
    F(u8),
    NumLock,
    ScrollLock,
    /// key < 128 => "%c": lowercase letters, digits and punctuation on KeyDown; any byte < 128
    /// (including b'\x08', b'\t', b'\r', b'\x1b') on Char
    Char(u8),
    /// NameForKey fallback "?" (e.g. OnChar of a codepoint >= 128)
    Unknown,
}

/// `ui_keyNameMap` order (the named keys; F1..F15 are expanded in place).
const NAMED: [(Key, &str); 28] = [
    (Key::LeftButton, "LEFTBUTTON"),
    (Key::MiddleButton, "MIDDLEBUTTON"),
    (Key::RightButton, "RIGHTBUTTON"),
    (Key::Mouse4, "MOUSE4"),
    (Key::Mouse5, "MOUSE5"),
    (Key::WheelUp, "WHEELUP"),
    (Key::WheelDown, "WHEELDOWN"),
    (Key::Back, "BACK"),
    (Key::Tab, "TAB"),
    (Key::Return, "RETURN"),
    (Key::Escape, "ESCAPE"),
    (Key::Shift, "SHIFT"),
    (Key::Ctrl, "CTRL"),
    (Key::Alt, "ALT"),
    (Key::Pause, "PAUSE"),
    (Key::PageUp, "PAGEUP"),
    (Key::PageDown, "PAGEDOWN"),
    (Key::End, "END"),
    (Key::Home, "HOME"),
    (Key::PrintScreen, "PRINTSCREEN"),
    (Key::Insert, "INSERT"),
    (Key::Delete, "DELETE"),
    (Key::Up, "UP"),
    (Key::Down, "DOWN"),
    (Key::Left, "LEFT"),
    (Key::Right, "RIGHT"),
    (Key::NumLock, "NUMLOCK"),
    (Key::ScrollLock, "SCROLLLOCK"),
];

impl Key {
    /// The exact Lua string ("LEFTBUTTON", "z", "?").
    pub fn name(self) -> Cow<'static, str> {
        match self {
            Key::F(n) => Cow::Owned(format!("F{n}")),
            Key::Char(b) if b < 128 => Cow::Owned(char::from(b).to_string()),
            Key::Char(_) | Key::Unknown => Cow::Borrowed("?"),
            k => Cow::Borrowed(
                NAMED
                    .iter()
                    .find(|(nk, _)| *nk == k)
                    .map_or("?", |(_, n)| *n),
            ),
        }
    }

    /// KeyForName (case-insensitive for multi-char names, as IsKeyDown accepts). A single
    /// character: letters case-insensitively (as their lowercase key), digits and space as
    /// themselves; any other single character is not recognised.
    pub fn from_name(s: &str) -> Option<Key> {
        let b = s.as_bytes();
        if b.len() == 1 {
            let c = b[0];
            return if c.is_ascii_alphabetic() {
                Some(Key::Char(c.to_ascii_lowercase()))
            } else if c == b' ' || c.is_ascii_digit() {
                Some(Key::Char(c))
            } else {
                None
            };
        }
        if let Some((k, _)) = NAMED.iter().find(|(_, n)| n.eq_ignore_ascii_case(s)) {
            return Some(*k);
        }
        if b.len() >= 2 && (b[0] == b'F' || b[0] == b'f') {
            if let Ok(n) = s[1..].parse::<u8>() {
                if (1..=15).contains(&n) && s[1..] == n.to_string() {
                    return Some(Key::F(n));
                }
            }
        }
        None
    }

    /// `key:match("BUTTON")`
    pub const fn is_button(self) -> bool {
        matches!(self, Key::LeftButton | Key::MiddleButton | Key::RightButton)
    }

    /// What `CallKeyHandler` hands Lua for an SG key code or an `OnChar` codepoint: `< 128` is
    /// `"%c"`, otherwise `NameForKey`, which searches `ui_keyNameMap` by **value**. Key codes
    /// share one number space with codepoints (`keyList_e` starts at `KEY_LMOUSE = 128`), so
    /// an `OnChar` of U+0080..=U+00AA arrives as a key *name* (U+00A3 '£' is `"F10"`, U+00A7
    /// '§' is `"F14"`, U+00A0 NBSP is `"F7"`); every other codepoint `>= 128` is `"?"`.
    ///
    /// sg: ui_main.cpp:522-525, :548-572, engine/common/keylist.h:7-51 @3b1a346
    pub fn from_sg_code(code: u32) -> Key {
        if code < 128 {
            return Key::Char(code as u8);
        }
        match code - 128 {
            i @ 0..=25 => NAMED[i as usize].0,
            i @ 26..=40 => Key::F((i - 25) as u8),
            41 => Key::NumLock,
            42 => Key::ScrollLock,
            _ => Key::Unknown,
        }
    }

    /// The SG key code (`keyList_e`, or the byte for `Char`); `None` for `Unknown`.
    ///
    /// sg: engine/common/keylist.h:7-51 @3b1a346
    pub fn sg_code(self) -> Option<u32> {
        match self {
            Key::Char(b) => Some(u32::from(b)),
            Key::F(n) if (1..=15).contains(&n) => Some(128 + 25 + u32::from(n)),
            Key::F(_) | Key::Unknown => None,
            Key::NumLock => Some(128 + 41),
            Key::ScrollLock => Some(128 + 42),
            k => NAMED[..26]
                .iter()
                .position(|(nk, _)| *nk == k)
                .map(|i| 128 + i as u32),
        }
    }
}

impl std::fmt::Display for Key {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.name())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn key_names_round_trip() {
        for (k, n) in NAMED {
            assert_eq!(k.name(), n);
            assert_eq!(Key::from_name(n), Some(k));
            assert_eq!(Key::from_name(&n.to_ascii_lowercase()), Some(k));
        }
        for n in 1..=15u8 {
            assert_eq!(Key::from_name(&Key::F(n).name()), Some(Key::F(n)));
        }
        assert_eq!(Key::from_name("F16"), None);
        assert_eq!(Key::Char(b'z').name(), "z");
        assert_eq!(Key::Unknown.name(), "?");
        assert_eq!(Key::from_name("A"), Some(Key::Char(b'a')));
        assert_eq!(Key::from_name(";"), None);
        assert_eq!(Key::from_name(""), None);
        assert_eq!(Font::from_lua("VAR BOLD"), Some(Font::VarBold));
        assert_eq!(Font::from_lua("var"), None);
        assert_eq!(Align::from_lua(None), Some(Align::Left));
    }

    /// keylist.h numbering and `NameForKey` by value (ui_main.cpp:555-572).
    #[test]
    fn sg_codes() {
        assert_eq!(Key::from_sg_code(128), Key::LeftButton);
        assert_eq!(Key::from_sg_code(133), Key::WheelUp);
        assert_eq!(Key::from_sg_code(135), Key::Back);
        assert_eq!(Key::from_sg_code(153), Key::Right);
        assert_eq!(Key::from_sg_code(154), Key::F(1));
        assert_eq!(Key::from_sg_code(168), Key::F(15));
        assert_eq!(Key::from_sg_code(169), Key::NumLock);
        assert_eq!(Key::from_sg_code(170), Key::ScrollLock);
        assert_eq!(Key::from_sg_code(171), Key::Unknown);
        assert_eq!(Key::from_sg_code(0xE9), Key::Unknown); // 'é' -> "?"
        assert_eq!(Key::from_sg_code(0x20AC), Key::Unknown); // '€' -> "?"
        assert_eq!(Key::from_sg_code(u32::from('a')), Key::Char(b'a'));
        assert_eq!(Key::from_sg_code(8), Key::Char(8)); // extra OnChar "\b"
                                                        // OnChar codepoints that collide with key codes arrive as names.
        assert_eq!(Key::from_sg_code(0xA3).name(), "F10"); // '£'
        assert_eq!(Key::from_sg_code(0xA7).name(), "F14"); // '§'
        assert_eq!(Key::from_sg_code(0xA0).name(), "F7"); // NBSP
        assert_eq!(Key::from_sg_code(0x80).name(), "LEFTBUTTON");
        for code in 0..=200u32 {
            let k = Key::from_sg_code(code);
            match k.sg_code() {
                Some(c) => assert_eq!(c, code),
                None => assert!(code > 170 && k == Key::Unknown),
            }
        }
        assert_eq!(Key::F(0).sg_code(), None);
        assert_eq!(Key::F(16).sg_code(), None);
    }
}
