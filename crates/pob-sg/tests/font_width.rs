//! T2 hand-derived DrawStringWidth cases (ui doc §11.1, U2 acceptance: >= 40 per function).
//!
//! Expected values are derived from the shipped `.tgf` glyph metrics (advance = width + spLeft +
//! spRight, shown per case as `char=advance` for the atlas the height maps to) and the
//! arithmetic of SimpleGraphic `r_font.cpp` @ 3b1a346 (StringWidthInternal/StringWidth, r_font.cpp:143-203; r_main.cpp:1833-1843), computed in f32 by the
//! independent transcription `crates/pob-ui-oracle/probe/rfont_ref.py` and spot-checked by hand
//! (see the worked examples below). `b"..."` inputs are Lua byte strings: invalid UTF-8 decodes
//! to U+FFFD per byte (`IndexUTF8ToUTF32`), and the text ends at the first NUL (`const char*`).
//!
//! Worked examples:
//! - VAR 16 "Hello": H 9 + e 8 + l 3 + l 3 + o 8 = 31 (scale 1, every ceil is a no-op).
//! - VAR 11 "Hello": atlas 10 (11 maps down), scale 1.1: ceil(7.7)=8, ceil(8+5.5)=14,
//!   ceil(14+2.2)=17, ceil(17+2.2)=20, ceil(20+5.5)=26.
//! - VAR -4 "Hello": atlas 10 with scale -0.4, every line width is negative; `max` starts at 0.
//! - VAR 16 "\t": 4 × space advance (1+0+3) = 16; "^^1" is one '^' (5) plus the escape `^1`.
//! - VAR 16 "é": tofu "[U+00E9]" measured unscaled in the 12 atlas (16-12 >= 3).
//! - VAR 16 b"\xc0\xaf": SG decodes the overlong pair as U+002F '/', advance 4.

#[path = "font_util.rs"]
mod font_util;

use font_util::fonts;
use pob_sg::Font::{self, *};

#[rustfmt::skip]
const CASES: &[(Font, i32, &[u8], i32)] = &[
        (Var, 16, b"Hello", 31), // atlas 16 scale 1: H=9 e=8 l=3 l=3 o=8
        (Var, 16, b"", 0), // atlas 16 scale 1: 
        (Var, 16, b"A", 9), // atlas 16 scale 1: A=9
        (Var, 10, b"Hello", 21), // atlas 10 scale 1: H=7 e=5 l=2 l=2 o=5
        (Var, 11, b"Hello", 26), // atlas 10 scale 1.1: H=7 e=5 l=2 l=2 o=5
        (Var, 13, b"Hello", 28), // atlas 12 scale 1.0833334: H=7 e=6 l=2 l=2 o=6
        (Var, 29, b"Hello", 63), // atlas 28 scale 1.0357143: H=18 e=14 l=6 l=6 o=14
        (Var, 30, b"Hello", 64), // atlas 28 scale 1.0714285: H=18 e=14 l=6 l=6 o=14
        (Var, 31, b"Hello", 62), // atlas 32 scale 0.96875: H=18 e=16 l=6 l=6 o=16
        (Var, 34, b"AAA", 63), // atlas 32 scale 1.0625: A=19 A=19 A=19
        (Var, 35, b"AAA", 63), // atlas 36 scale 0.9722222: A=21 A=21 A=21
        (Var, 100, b"Hello", 199), // atlas 64 scale 1.5625: H=36 e=32 l=13 l=13 o=32
        (Var, 1, b"Hello", 5), // atlas 10 scale 0.1: H=7 e=5 l=2 l=2 o=5
        (Var, 0, b"Hello", 0), // atlas 10 scale 0: H=7 e=5 l=2 l=2 o=5
        (Var, -4, b"Hello", 0), // atlas 10 scale -0.4: H=7 e=5 l=2 l=2 o=5
        (Var, 16, b"\t", 16), // atlas 16 scale 1: tab=4*4
        (Var, 16, b"a\tb", 32), // atlas 16 scale 1: a=8 tab=4*4 b=8
        (Var, 30, b"\t\t", 60), // atlas 28 scale 1.0714285: tab=4*7 tab=4*7
        (Var, 13, b"\tx", 20), // atlas 12 scale 1.0833334: tab=4*3 x=6
        (Var, 16, b"Hello\nA", 31), // atlas 16 scale 1: H=9 e=8 l=3 l=3 o=8 \n A=9
        (Var, 16, b"A\nHello\n", 31), // atlas 16 scale 1: A=9 \n H=9 e=8 l=3 l=3 o=8 \n
        (Var, 16, b"\n\n", 0), // atlas 16 scale 1: \n \n
        (Var, 16, b"^7Hel^xFF0000lo", 31), // atlas 16 scale 1: ^=5 7=8 H=9 e=8 l=3 ^=5 x=6 F=9 F=9 0=8 0=8 0=8 0=8 l=3 o=8
        (Var, 16, b"^", 5), // atlas 16 scale 1: ^=5
        (Var, 16, b"^x12345", 51), // atlas 16 scale 1: ^=5 x=6 1=8 2=8 3=8 4=8 5=8
        (Var, 16, b"^xGGGGGG", 77), // atlas 16 scale 1: ^=5 x=6 G=11 G=11 G=11 G=11 G=11 G=11
        (Var, 16, b"^^1", 5), // atlas 16 scale 1: ^=5 ^=5 1=8
        (Var, 16, b"a^", 13), // atlas 16 scale 1: a=8 ^=5
        (Var, 16, b"\xc3\xa9", 43), // atlas 16 scale 1: U+00E9=tofu@12
        (Var, 12, b"\xc3\xa9", 43), // atlas 12 scale 1: U+00E9=tofu@12
        (Var, 30, b"\xc3\xa9", 96), // atlas 28 scale 1.0714285: U+00E9=tofu@26
        (Var, 16, b"\xe6\x97\xa5\xe6\x9c\xac", 87), // atlas 16 scale 1: U+65E5=tofu@12 U+672C=tofu@12
        (Var, 16, b"\xf0\x9f\x98\x80", 49), // atlas 16 scale 1: U+1F600=tofu@12
        (Var, 16, b"a\xf0\x9f\x98\x80b", 65), // atlas 16 scale 1: a=8 U+1F600=tofu@12 b=8
        (Var, 16, b"\xff", 44), // atlas 16 scale 1: U+FFFD=tofu@12
        (Var, 16, b"ab\x00cd", 16), // atlas 16 scale 1: a=8 b=8
        (Var, 16, b"\x00abc", 0), // atlas 16 scale 1: 
        (Var, 16, b"^1\xc3\xa9", 43), // atlas 16 scale 1: ^=5 1=8 U+00E9=tofu@12
        (Var, 16, b"\xc3\xa9^1", 43), // atlas 16 scale 1: U+00E9=tofu@12 ^=5 1=8
        (Var, 16, b"\xc2\x80", 43), // atlas 16 scale 1: U+0080=tofu@12
        (Var, 16, b"\xc3", 44), // atlas 16 scale 1: U+FFFD=tofu@12
        (Var, 16, b"\xed\xa0\x80", 44), // atlas 16 scale 1: U+D800=tofu@12
        (Var, 16, b"\xc0\xaf", 4), // atlas 16 scale 1: /=4
        (Fixed, 12, b"AAAA", 24), // atlas 12 scale 1: A=6 A=6 A=6 A=6
        (Fixed, 16, b"Hello World", 88), // atlas 16 scale 1: H=8 e=8 l=8 l=8 o=8 sp=8 W=8 o=8 r=8 l=8 d=8
        (VarBold, 16, b"Hello", 36), // atlas 16 scale 1: H=10 e=9 l=4 l=4 o=9
        (FontinSc, 18, b"Path of Building", 125), // atlas 18 scale 1: P=9 a=8 t=7 h=11 sp=4 o=10 f=7 sp=4 B=9 u=10 i=5 l=7 d=9 i=5 n=11 g=9
        (FontinItalic, 20, b"Fontin", 46), // atlas 20 scale 1: F=8 o=9 n=9 t=6 i=5 n=9
        (FontinScItalic, 24, b"Italic", 54), // atlas 24 scale 1: I=7 t=10 a=11 l=9 i=6 c=11
        (Fontin, 14, b"Life", 19), // atlas 14 scale 1: L=6 i=3 f=4 e=6
        (Var, 22, b"Str: 100\tDex: 50", 163), // atlas 22 scale 1: S=13 t=6 r=7 :=6 sp=6 1=11 0=11 0=11 tab=4*6 D=14 e=11 x=9 :=6 sp=6 5=11 0=11
        (Var, 16, b"line one\nline two is longer\nthree", 105), // atlas 16 scale 1: l=3 i=3 n=8 e=8 sp=4 o=8 n=8 e=8 \n l=3 i=3 n=8 e=8 sp=4 t=4 w=9 o=8 sp=4 i=3 s=7 sp=4 l=3 o=8 n=8 g=8 e=8 r=5 \n t=4 h=8 r=5 e=8 e=8
];

#[test]
fn hand_widths() {
    assert!(CASES.len() >= 40);
    let fs = fonts();
    let mut bad = Vec::new();
    for &(f, h, text, want) in CASES {
        let face = fs.face(f);
        let got = face.string_width_bytes(h, text);
        if got != want {
            bad.push(format!(
                "{f:?} {h} {:?}: {got} != {want}",
                String::from_utf8_lossy(text)
            ));
        }
        if let Ok(s) = std::str::from_utf8(text) {
            assert_eq!(face.string_width(h, s), got, "&str entry point");
        }
    }
    assert!(
        bad.is_empty(),
        "{} of {} width cases differ:\n{}",
        bad.len(),
        CASES.len(),
        bad.join("\n")
    );
}
