//! Colour escapes (`^0`-`^9`, `^xRRGGBB`): is_color_escape, read_color_escape, strip_escapes,
//! PALETTE (ui doc §3, §2 "SetDrawColor" / "Escape leak"). Owner: U1.
//!
//! Moved from the pre-port `pob-sg/src/color.rs` by UI-U0; PoB's `colorCodes` table
//! (Data/Global.lua) moved to `pob-ui-core/src/color_codes.rs`.
//!
//! Port of PathOfBuilding-SimpleGraphic `3b1a346` (the shipped DLL's commit; `common.cpp` and
//! `common.h` are byte-identical to the research copy, master @ 8694a07):
//! - palette: `engine/common/common.cpp:188-199` `colorEscape[10]`, values from
//!   `engine/common.h:110-119` (`colorBlack` .. `colorDarkGray`). The same 160-byte float
//!   table is present once in the shipped `SimpleGraphic.dll` (offset 0x185380); the test
//!   `palette_equals_dll_table` checks it.
//! - escape recognition: `engine/common/common.cpp:201-218` `IsColorEscape(const char*)` and
//!   `:220-248` `IsColorEscape(std::u32string_view)` (equivalent on valid UTF-8: every byte of
//!   an escape is ASCII).
//! - escape decoding: `engine/common/common.cpp:250-274` `ReadColorEscape`.
//! - stripping: `ui_api.cpp:1259-1281` `l_StripEscapes` (the NUL cut of the C string is applied
//!   by `api::strip_escapes`).
//!
//! PoB's own colour constants (`Data/Global.lua:7-100`: `colorCodes`, `hexToRGB`) live in
//! `pob-ui-core/src/color_codes.rs` (U4).

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
        assert_eq!(
            parse_escape_color("^xFF8000"),
            Some([1.0, 128.0 / 255.0, 0.0, 1.0])
        );
        assert_eq!(parse_escape_color("7"), None);
        // `xr / 255.0f` in float, every byte value; upper and lower case hex.
        for v in 0..=255u8 {
            let s = format!("^x{v:02x}{v:02X}00");
            let c = parse_escape_color(&s).unwrap();
            assert_eq!(c[0], f32::from(v) / 255.0f32);
            assert_eq!(c[1], f32::from(v) / 255.0f32);
            assert_eq!(c[2], 0.0);
            assert_eq!(c[3], 1.0);
        }
        // ReadColorEscape copies only RGB.
        let mut out = [0.5, 0.5, 0.5, 0.25];
        read_color_escape(b"^1", 2, &mut out);
        assert_eq!(out, [1.0, 0.0, 0.0, 0.25]);
    }

    /// Finds the shipped `SimpleGraphic.dll` (gitignored; worktrees search their ancestors,
    /// `POB_ROOT` overrides).
    fn shipped_dll() -> Option<Vec<u8>> {
        let mut starts = Vec::new();
        if let Some(p) = std::env::var_os("POB_ROOT") {
            starts.push(std::path::PathBuf::from(p));
        }
        starts.push(std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")));
        for s in starts {
            for d in s.ancestors() {
                let p = d.join(".PathOfBuilding/runtime/SimpleGraphic.dll");
                if p.is_file() {
                    return std::fs::read(p).ok();
                }
            }
        }
        None
    }

    /// Acceptance (ui doc §12.3 U1): the palette equals the DLL's `colorEscape` float table,
    /// and the DLL is the pinned `3b1a346`.
    #[test]
    fn palette_equals_dll_table() {
        let Some(dll) = shipped_dll() else {
            eprintln!("SKIP: .PathOfBuilding/runtime/SimpleGraphic.dll not found (set POB_ROOT)");
            return;
        };
        let ver = b"SimpleGraphic v2.5-3b1a346";
        assert!(
            dll.windows(ver.len()).any(|w| w == ver),
            "DLL is not 3b1a346"
        );
        let table: Vec<u8> = PALETTE
            .iter()
            .flat_map(|c| c.iter().flat_map(|v| v.to_le_bytes()))
            .collect();
        assert_eq!(table.len(), 160);
        let hits: Vec<usize> = dll
            .windows(table.len())
            .enumerate()
            .filter(|(_, w)| *w == table.as_slice())
            .map(|(i, _)| i)
            .collect();
        assert_eq!(
            hits,
            vec![0x185380],
            "palette table not found once in the DLL"
        );
    }
}
