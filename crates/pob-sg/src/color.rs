//! Colour escapes (`^0`-`^9`, `^xRRGGBB`) and PoB's `colorCodes` table.
//!
//! Source references (SimpleGraphic master @ 8694a07, which matches the
//! `r_font.cpp` shipped with PoB 2.67.2: that file is unchanged since
//! 2025-10-23):
//! - palette: `engine/common/common.cpp` `colorEscape[10]` table, values from
//!   `engine/common.h` (`colorBlack` .. `colorDarkGray`). The same float table
//!   is present in the shipped `SimpleGraphic.dll` (offset 0x185380, see
//!   docs/port/research/map_pob-ui-lua.md).
//! - escape recognition: `engine/common/common.cpp` `IsColorEscape(const char*)`
//!   and `IsColorEscape(std::u32string_view)`.
//! - escape decoding: `engine/common/common.cpp` `ReadColorEscape`.
//! - stripping: `ui_api.cpp` `l_StripEscapes`.
//! - `colorCodes`: PoB `src/Data/Global.lua:7-77`.

/// RGBA colour, components in 0..=1 (SimpleGraphic `col4_t`).
pub type Rgba = [f32; 4];

/// The `^0`..`^9` palette (`common.cpp` `colorEscape`, `common.h` colour
/// constants). All entries have alpha 1.
pub const PALETTE: [Rgba; 10] = [
    [0.0, 0.0, 0.0, 1.0], // ^0 colorBlack
    [1.0, 0.0, 0.0, 1.0], // ^1 colorRed
    [0.0, 1.0, 0.0, 1.0], // ^2 colorGreen
    [0.0, 0.0, 1.0, 1.0], // ^3 colorBlue
    [1.0, 1.0, 0.0, 1.0], // ^4 colorYellow
    [1.0, 0.0, 1.0, 1.0], // ^5 colorPurple
    [0.0, 1.0, 1.0, 1.0], // ^6 colorAqua
    [1.0, 1.0, 1.0, 1.0], // ^7 colorWhite
    [0.7, 0.7, 0.7, 1.0], // ^8 colorGray
    [0.4, 0.4, 0.4, 1.0], // ^9 colorDarkGray
];

fn is_hex(b: u8) -> bool {
    b.is_ascii_hexdigit()
}

/// Length of the colour escape at the start of `s` (0 if none, 2 for `^N`,
/// 8 for `^xRRGGBB` / `^XRRGGBB`).
///
/// Port of `common.cpp` `IsColorEscape`: `^` followed by an ASCII digit is a
/// 2-byte escape; `^x`/`^X` followed by exactly 6 hex digits is an 8-byte
/// escape; anything else is not an escape.
pub fn is_color_escape(s: &[u8]) -> usize {
    if s.len() < 2 || s[0] != b'^' {
        return 0;
    }
    let d = s[1];
    if d.is_ascii_digit() {
        return 2;
    }
    if s.len() >= 8 && (d == b'x' || d == b'X') && s[2..8].iter().all(|&b| is_hex(b)) {
        return 8;
    }
    0
}

fn hex_val(c: u8) -> u8 {
    match c {
        b'0'..=b'9' => c - b'0',
        b'a'..=b'f' => c - b'a' + 10,
        b'A'..=b'F' => c - b'A' + 10,
        _ => 0,
    }
}

/// Decode the escape at the start of `s` (which must have the given `len`
/// from [`is_color_escape`]) into RGB, leaving `out[3]` untouched.
///
/// Port of `common.cpp` `ReadColorEscape`: `VectorCopy` copies only the three
/// colour components; `^x` components are `byte / 255.0f`.
pub fn read_color_escape(s: &[u8], len: usize, out: &mut Rgba) {
    match len {
        2 => {
            let p = PALETTE[(s[1] - b'0') as usize];
            out[0] = p[0];
            out[1] = p[1];
            out[2] = p[2];
        }
        8 => {
            for i in 0..3 {
                let v = (hex_val(s[2 + i * 2]) << 4) | hex_val(s[3 + i * 2]);
                out[i] = f32::from(v) / 255.0;
            }
        }
        _ => {}
    }
}

/// Parse a whole escape string as used by `SetDrawColor("^7")`.
///
/// Port of `ui_api.cpp` `l_SetDrawColor` string branch: the escape at the
/// start of the string is decoded and alpha is forced to 1. Trailing bytes are
/// ignored (SimpleGraphic only looks at the prefix). Returns `None` where
/// SimpleGraphic raises "invalid color escape sequence".
pub fn parse_escape_color(s: &str) -> Option<Rgba> {
    let b = s.as_bytes();
    let len = is_color_escape(b);
    if len == 0 {
        return None;
    }
    let mut c = [0.0, 0.0, 0.0, 1.0];
    read_color_escape(b, len, &mut c);
    c[3] = 1.0;
    Some(c)
}

/// Remove all colour escapes. Port of `ui_api.cpp` `l_StripEscapes`, which
/// scans bytes with `IsColorEscape(const char*)`.
pub fn strip_escapes(s: &str) -> String {
    let b = s.as_bytes();
    let mut out = Vec::with_capacity(b.len());
    let mut i = 0;
    while i < b.len() {
        let n = is_color_escape(&b[i..]);
        if n > 0 {
            i += n;
        } else {
            out.push(b[i]);
            i += 1;
        }
    }
    // Escapes are pure ASCII, so removing them keeps UTF-8 valid.
    String::from_utf8(out).unwrap_or_default()
}

/// PoB `colorCodes` (src/Data/Global.lua:7-64), in source order.
pub const COLOR_CODES: &[(&str, &str)] = &[
    ("NORMAL", "^xC8C8C8"),
    ("MAGIC", "^x8888FF"),
    ("RARE", "^xFFFF77"),
    ("UNIQUE", "^xAF6025"),
    ("RELIC", "^x60C060"),
    ("GEM", "^x1AA29B"),
    ("PROPHECY", "^xB54BFF"),
    ("CURRENCY", "^xAA9E82"),
    ("CRAFTED", "^xB8DAF1"),
    ("CUSTOM", "^x5CF0BB"),
    ("SOURCE", "^x88FFFF"),
    ("UNSUPPORTED", "^xF05050"),
    ("DISABLED", "^x7F7F7F"),
    ("WARNING", "^xFF9922"),
    ("TIP", "^x80A080"),
    ("FIRE", "^xB97123"),
    ("COLD", "^x3F6DB3"),
    ("LIGHTNING", "^xADAA47"),
    ("CHAOS", "^xD02090"),
    ("POSITIVE", "^x33FF77"),
    ("NEGATIVE", "^xDD0022"),
    ("HIGHLIGHT", "^xFF0000"),
    ("OFFENCE", "^xE07030"),
    ("DEFENCE", "^x8080E0"),
    ("SCION", "^xFFF0F0"),
    ("MARAUDER", "^xE05030"),
    ("RANGER", "^x70FF70"),
    ("WITCH", "^x7070FF"),
    ("DUELIST", "^xE0E070"),
    ("TEMPLAR", "^xC040FF"),
    ("SHADOW", "^x30C0D0"),
    ("MAINHAND", "^x50FF50"),
    ("MAINHANDBG", "^x071907"),
    ("OFFHAND", "^xB7B7FF"),
    ("OFFHANDBG", "^x070719"),
    ("SHAPER", "^x55BBFF"),
    ("ELDER", "^xAA77CC"),
    ("FRACTURED", "^xA29160"),
    ("MUTATED", "^xCD2285"),
    ("ADJUDICATOR", "^xE9F831"),
    ("BASILISK", "^x00CB3A"),
    ("CRUSADER", "^x2946FC"),
    ("EYRIE", "^xAAB7B8"),
    ("CLEANSING", "^xF24141"),
    ("TANGLE", "^x038C8C"),
    ("CHILLBG", "^x151e26"),
    ("FREEZEBG", "^x0c262b"),
    ("SHOCKBG", "^x191732"),
    ("SCORCHBG", "^x270b00"),
    ("BRITTLEBG", "^x00122b"),
    ("SAPBG", "^x261500"),
    ("SCOURGE", "^xFF6E25"),
    ("CRUCIBLE", "^xFFA500"),
    ("SPLITPERSONALITY", "^xFFD62A"),
    ("VESTIGIAL", "^xCBA5F1"),
    ("INTANGIBILITY", "^x9BF4BD"),
    ("MEMORY", "^xBFE2FA"),
];

/// Aliases assigned after the table (src/Data/Global.lua:65-77): `(alias, target)`.
pub const COLOR_CODE_ALIASES: &[(&str, &str)] = &[
    ("STRENGTH", "MARAUDER"),
    ("DEXTERITY", "RANGER"),
    ("INTELLIGENCE", "WITCH"),
    ("LIFE", "MARAUDER"),
    ("MANA", "WITCH"),
    ("ES", "SOURCE"),
    ("WARD", "RARE"),
    ("ARMOUR", "NORMAL"),
    ("EVASION", "POSITIVE"),
    ("RAGE", "WARNING"),
    ("PHYS", "NORMAL"),
];

/// The default (unmodified) escape string for a `colorCodes` key, including aliases.
pub fn default_color_code(name: &str) -> Option<&'static str> {
    let target = COLOR_CODE_ALIASES.iter().find(|(a, _)| *a == name).map_or(name, |(_, t)| *t);
    COLOR_CODES.iter().find(|(k, _)| *k == target).map(|(_, v)| *v)
}

/// Mutable `colorCodes` table (PoB keeps it as a global that Options can
/// override via `updateColorCode`). Aliases are independent copies, exactly
/// like the Lua assignments (`colorCodes.LIFE = colorCodes.MARAUDER` copies the
/// string, so later edits of MARAUDER do not affect LIFE).
#[derive(Clone, Debug)]
pub struct ColorCodes {
    entries: Vec<(String, String)>,
}

impl Default for ColorCodes {
    fn default() -> Self {
        let mut entries: Vec<(String, String)> =
            COLOR_CODES.iter().map(|(k, v)| ((*k).to_owned(), (*v).to_owned())).collect();
        for (alias, target) in COLOR_CODE_ALIASES {
            let v = default_color_code(target).unwrap_or("^7");
            entries.push(((*alias).to_owned(), v.to_owned()));
        }
        Self { entries }
    }
}

impl ColorCodes {
    pub fn get(&self, name: &str) -> Option<&str> {
        self.entries.iter().find(|(k, _)| k == name).map(|(_, v)| v.as_str())
    }

    /// Port of `updateColorCode(code, color)` (Global.lua:80-87): only existing
    /// keys are updated; `color:gsub("^0", "^")` turns a leading `0` (as in
    /// `0xRRGGBB`) into `^`. Returns the new HIGHLIGHT rgb when HIGHLIGHT
    /// changed (Lua sets the global `rgbColor`).
    pub fn update(&mut self, code: &str, color: &str) -> Option<[f32; 3]> {
        let entry = self.entries.iter_mut().find(|(k, _)| k == code)?;
        entry.1 = if let Some(rest) = color.strip_prefix('0') {
            format!("^{rest}")
        } else {
            color.to_owned()
        };
        if code == "HIGHLIGHT" {
            hex_to_rgb(color)
        } else {
            None
        }
    }

    pub fn iter(&self) -> impl Iterator<Item = (&str, &str)> {
        self.entries.iter().map(|(k, v)| (k.as_str(), v.as_str()))
    }
}

/// Port of `hexToRGB` (Global.lua:89-100): strips `^x`, `0x` and `#`
/// (all occurrences, as `gsub` does), then requires exactly 6 hex digits.
pub fn hex_to_rgb(hex: &str) -> Option<[f32; 3]> {
    let h = hex.replace("^x", "").replace("0x", "").replace('#', "");
    if h.len() != 6 {
        return None;
    }
    let mut out = [0.0; 3];
    for (i, o) in out.iter_mut().enumerate() {
        let v = u8::from_str_radix(&h[i * 2..i * 2 + 2], 16).ok()?;
        *o = f32::from(v) / 255.0;
    }
    Some(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn escape_lengths() {
        assert_eq!(is_color_escape(b"^7abc"), 2);
        assert_eq!(is_color_escape(b"^xFFaa00rest"), 8);
        assert_eq!(is_color_escape(b"^XFFaa00"), 8);
        assert_eq!(is_color_escape(b"^xFFaa0"), 0); // too short
        assert_eq!(is_color_escape(b"^xFFaG00"), 0); // not hex
        assert_eq!(is_color_escape(b"^a"), 0);
        assert_eq!(is_color_escape(b"^"), 0);
        assert_eq!(is_color_escape(b"x^7"), 0);
    }

    #[test]
    fn strip() {
        assert_eq!(strip_escapes("^7Hello ^xFF0000World^1!"), "Hello World!");
        assert_eq!(strip_escapes("^^7a"), "^a");
        assert_eq!(strip_escapes("100^"), "100^");
        assert_eq!(strip_escapes("^xZZ0000x"), "^xZZ0000x");
        assert_eq!(strip_escapes("^8é^9ü"), "éü");
    }

    #[test]
    fn palette_and_hex() {
        assert_eq!(parse_escape_color("^8"), Some([0.7, 0.7, 0.7, 1.0]));
        assert_eq!(parse_escape_color("^9"), Some([0.4, 0.4, 0.4, 1.0]));
        assert_eq!(parse_escape_color("^xFF8000"), Some([1.0, 128.0 / 255.0, 0.0, 1.0]));
        assert_eq!(parse_escape_color("7"), None);
    }

    #[test]
    fn color_codes_table() {
        let mut cc = ColorCodes::default();
        assert_eq!(cc.get("NORMAL"), Some("^xC8C8C8"));
        assert_eq!(cc.get("LIFE"), Some("^xE05030"));
        assert_eq!(cc.get("PHYS"), Some("^xC8C8C8"));
        assert_eq!(cc.update("POSITIVE", "0x112233"), None);
        assert_eq!(cc.get("POSITIVE"), Some("^x112233"));
        assert_eq!(cc.get("EVASION"), Some("^x33FF77"));
        let rgb = cc.update("HIGHLIGHT", "^x00FF00").unwrap();
        assert_eq!(rgb, [0.0, 1.0, 0.0]);
        assert_eq!(cc.update("NOPE", "^x000000"), None);
        assert_eq!(cc.get("NOPE"), None);
    }
}
