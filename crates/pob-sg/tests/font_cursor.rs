//! T2 hand-derived DrawStringCursorIndex cases (ui doc §11.1, U2 acceptance: >= 40 per function).
//!
//! Expected values are derived from the shipped `.tgf` glyph metrics (advance = width + spLeft +
//! spRight, shown per case as `char=advance` for the atlas the height maps to) and the
//! arithmetic of SimpleGraphic `r_font.cpp` @ 3b1a346 (StringCursorInternal/StringCursorIndex, r_font.cpp:205-286; r_main.cpp:1845-1860), computed in f32 by the
//! independent transcription `crates/pob-ui-oracle/probe/rfont_ref.py` and spot-checked by hand
//! (see the worked examples below). `b"..."` inputs are Lua byte strings: invalid UTF-8 decodes
//! to U+FFFD per byte (`IndexUTF8ToUTF32`), and the text ends at the first NUL (`const char*`).
//! Results are 0-based BYTE offsets (ui_api.cpp adds the `+ 1` Lua sees).
//!
//! Worked examples:
//! - VAR 16 "Hello" (right edges 9, 17, 20, 23, 31): x <= 9 -> 0, 10..=17 -> 1, 31 -> 4, 32 -> 5.
//! - "a\tb" (a 8, tab 16, half 8): x <= 8 -> 0; 9..=16 -> 1 (first half); 17..=24 -> 1 (second
//!   half breaks before ++I as well); 25 -> 2.
//! - "Hello\nHe": the first line ends at lineY 16, so y 16 stays on line 1 and y 17 moves on.
//! - "ab\n" with y below the text: the loop leaves I at the end and adds the first line's index:
//!   past the end, so the byte length (3).
//! - "éa": é is 2 bytes; inside its tofu box the cursor sits before é (byte 0), after it 'a' is
//!   byte 2; "^1ab" at x 0 lands after the escape (byte 2).

#[path = "font_util.rs"]
mod font_util;

use font_util::fonts;
use pob_sg::Font::{self, *};

/// (font, height, text, x, y, expected 0-based byte offset)
type CursorCase = (Font, i32, &'static [u8], i32, i32, usize);

#[rustfmt::skip]
const CASES: &[CursorCase] = &[
        (Var, 16, b"Hello", -5, 0, 0), // atlas 16 scale 1: H=9 e=8 l=3 l=3 o=8
        (Var, 16, b"Hello", 0, 0, 0), // atlas 16 scale 1: H=9 e=8 l=3 l=3 o=8
        (Var, 16, b"Hello", 1, 0, 0), // atlas 16 scale 1: H=9 e=8 l=3 l=3 o=8
        (Var, 16, b"Hello", 9, 0, 0), // atlas 16 scale 1: H=9 e=8 l=3 l=3 o=8
        (Var, 16, b"Hello", 10, 0, 1), // atlas 16 scale 1: H=9 e=8 l=3 l=3 o=8
        (Var, 16, b"Hello", 17, 0, 1), // atlas 16 scale 1: H=9 e=8 l=3 l=3 o=8
        (Var, 16, b"Hello", 18, 0, 2), // atlas 16 scale 1: H=9 e=8 l=3 l=3 o=8
        (Var, 16, b"Hello", 31, 0, 4), // atlas 16 scale 1: H=9 e=8 l=3 l=3 o=8
        (Var, 16, b"Hello", 32, 0, 5), // atlas 16 scale 1: H=9 e=8 l=3 l=3 o=8
        (Var, 16, b"Hello", 999, 0, 5), // atlas 16 scale 1: H=9 e=8 l=3 l=3 o=8
        (Var, 16, b"Hello\nHe", 100, 16, 5), // atlas 16 scale 1: H=9 e=8 l=3 l=3 o=8 \n H=9 e=8
        (Var, 16, b"Hello\nHe", 100, 17, 8), // atlas 16 scale 1: H=9 e=8 l=3 l=3 o=8 \n H=9 e=8
        (Var, 16, b"Hello\nHe", 100, 500, 8), // atlas 16 scale 1: H=9 e=8 l=3 l=3 o=8 \n H=9 e=8
        (Var, 16, b"Hello\nHe", 0, 17, 6), // atlas 16 scale 1: H=9 e=8 l=3 l=3 o=8 \n H=9 e=8
        (Var, 16, b"Hello\nHe", 9, -10, 0), // atlas 16 scale 1: H=9 e=8 l=3 l=3 o=8 \n H=9 e=8
        (Var, 16, b"ab\n", 999, 999, 3), // atlas 16 scale 1: a=8 b=8 \n
        (Var, 16, b"ab\n", 1, 999, 3), // atlas 16 scale 1: a=8 b=8 \n
        (Var, 16, b"ab\n", 1, 0, 0), // atlas 16 scale 1: a=8 b=8 \n
        (Var, 16, b"\nab", 999, 0, 0), // atlas 16 scale 1: \n a=8 b=8
        (Var, 16, b"\nab", 999, 17, 3), // atlas 16 scale 1: \n a=8 b=8
        (Var, 16, b"a\n\nb", 999, 20, 2), // atlas 16 scale 1: a=8 \n \n b=8
        (Var, 16, b"a\n\nb", 999, 40, 4), // atlas 16 scale 1: a=8 \n \n b=8
        (Var, 16, b"\xc3\xa9a", 0, 0, 0), // atlas 16 scale 1: U+00E9=tofu@12 a=8
        (Var, 16, b"\xc3\xa9a", 5, 0, 0), // atlas 16 scale 1: U+00E9=tofu@12 a=8
        (Var, 16, b"\xc3\xa9a", 30, 0, 0), // atlas 16 scale 1: U+00E9=tofu@12 a=8
        (Var, 16, b"\xc3\xa9a", 47, 0, 2), // atlas 16 scale 1: U+00E9=tofu@12 a=8
        (Var, 16, b"\xc3\xa9a", 48, 0, 2), // atlas 16 scale 1: U+00E9=tofu@12 a=8
        (Var, 16, b"\xc3\xa9a", 52, 0, 3), // atlas 16 scale 1: U+00E9=tofu@12 a=8
        (Var, 16, b"\xc3\xa9a", 999, 0, 3), // atlas 16 scale 1: U+00E9=tofu@12 a=8
        (Var, 16, b"a\xc3\xa9", 5, 0, 0), // atlas 16 scale 1: a=8 U+00E9=tofu@12
        (Var, 16, b"a\xc3\xa9", 9, 0, 1), // atlas 16 scale 1: a=8 U+00E9=tofu@12
        (Var, 16, b"a\xc3\xa9", 999, 0, 3), // atlas 16 scale 1: a=8 U+00E9=tofu@12
        (Var, 16, b"a\tb", 7, 0, 0), // atlas 16 scale 1: a=8 tab=4*4 b=8
        (Var, 16, b"a\tb", 8, 0, 0), // atlas 16 scale 1: a=8 tab=4*4 b=8
        (Var, 16, b"a\tb", 15, 0, 1), // atlas 16 scale 1: a=8 tab=4*4 b=8
        (Var, 16, b"a\tb", 16, 0, 1), // atlas 16 scale 1: a=8 tab=4*4 b=8
        (Var, 16, b"a\tb", 23, 0, 1), // atlas 16 scale 1: a=8 tab=4*4 b=8
        (Var, 16, b"a\tb", 24, 0, 1), // atlas 16 scale 1: a=8 tab=4*4 b=8
        (Var, 16, b"a\tb", 25, 0, 2), // atlas 16 scale 1: a=8 tab=4*4 b=8
        (Var, 16, b"a\tb", 999, 0, 3), // atlas 16 scale 1: a=8 tab=4*4 b=8
        (Var, 30, b"\tX", 8, 0, 0), // atlas 28 scale 1.0714285: tab=4*7 X=17
        (Var, 30, b"\tX", 9, 0, 0), // atlas 28 scale 1.0714285: tab=4*7 X=17
        (Var, 30, b"\tX", 17, 0, 0), // atlas 28 scale 1.0714285: tab=4*7 X=17
        (Var, 30, b"\tX", 18, 0, 0), // atlas 28 scale 1.0714285: tab=4*7 X=17
        (Var, 30, b"\tX", 999, 0, 2), // atlas 28 scale 1.0714285: tab=4*7 X=17
        (Var, 16, b"^1ab", 0, 0, 2), // atlas 16 scale 1: ^=5 1=8 a=8 b=8
        (Var, 16, b"^1ab", 9, 0, 3), // atlas 16 scale 1: ^=5 1=8 a=8 b=8
        (Var, 16, b"ab^1", 999, 0, 4), // atlas 16 scale 1: a=8 b=8 ^=5 1=8
        (Var, 16, b"a^1b", 9, 0, 3), // atlas 16 scale 1: a=8 ^=5 1=8 b=8
        (Var, 16, b"\xffa", 999, 0, 2), // atlas 16 scale 1: U+FFFD=tofu@12 a=8
        (Var, 16, b"\xffa", 0, 0, 0), // atlas 16 scale 1: U+FFFD=tofu@12 a=8
        (Var, 16, b"ab\x00cd", 999, 0, 2), // atlas 16 scale 1: a=8 b=8
        (Var, 16, b"\xe6\x97\xa5\xe6\x9c\xac", 999, 0, 6), // atlas 16 scale 1: U+65E5=tofu@12 U+672C=tofu@12
        (Var, 16, b"\xe6\x97\xa5\xe6\x9c\xac", 60, 0, 3), // atlas 16 scale 1: U+65E5=tofu@12 U+672C=tofu@12
        (Var, 16, b"\xf0\x9f\x98\x80x", 999, 0, 5), // atlas 16 scale 1: U+1F600=tofu@12 x=6
        (Var, 16, b"", 5, 0, 0), // atlas 16 scale 1: 
        (Fixed, 12, b"AAAA", 13, 0, 2), // atlas 12 scale 1: A=6 A=6 A=6 A=6
        (Fixed, 12, b"AAAA", 12, 0, 1), // atlas 12 scale 1: A=6 A=6 A=6 A=6
        (FontinSc, 18, b"Path of Building", 50, 0, 6), // atlas 18 scale 1: P=9 a=8 t=7 h=11 sp=4 o=10 f=7 sp=4 B=9 u=10 i=5 l=7 d=9 i=5 n=11 g=9
        (FontinItalic, 20, b"fifi", 20, 0, 3), // atlas 20 scale 1: f=6 i=5 f=6 i=5
        (Var, 13, b"Hello", 20, 0, 3), // atlas 12 scale 1.0833334: H=7 e=6 l=2 l=2 o=6
        (Var, 13, b"Hello", 21, 0, 3), // atlas 12 scale 1.0833334: H=7 e=6 l=2 l=2 o=6
        (Var, 29, b"AB", 19, 0, 1), // atlas 28 scale 1.0357143: A=17 B=17
        (Var, 29, b"AB", 20, 0, 1), // atlas 28 scale 1.0357143: A=17 B=17
];

#[test]
fn hand_cursor_indices() {
    assert!(CASES.len() >= 40);
    let fs = fonts();
    let mut bad = Vec::new();
    for &(f, h, text, x, y, want) in CASES {
        let face = fs.face(f);
        let got = face.cursor_index_bytes(h, text, x, y);
        if got != want {
            bad.push(format!(
                "{f:?} {h} {:?} ({x}, {y}): {got} != {want}",
                String::from_utf8_lossy(text)
            ));
        }
        if let Ok(s) = std::str::from_utf8(text) {
            assert_eq!(face.cursor_index(h, s, x, y), got, "&str entry point");
        }
    }
    assert!(
        bad.is_empty(),
        "{} of {} cursor cases differ:\n{}",
        bad.len(),
        CASES.len(),
        bad.join("\n")
    );
}

/// Cursor offsets always land on UTF-8 boundaries of valid text, and walking x across a line
/// visits every boundary in order.
#[test]
fn cursor_offsets_are_char_boundaries() {
    let fs = fonts();
    let face = fs.face(Var);
    let s = "aé日😀b\tc^1d";
    let mut last = 0;
    for x in -2..400 {
        let i = face.cursor_index(16, s, x, 0);
        assert!(s.is_char_boundary(i), "x {x}: {i}");
        assert!(i >= last, "x {x}: {i} < {last}");
        last = i;
    }
    assert_eq!(last, s.len());
}
