//! T2 hand-derived Draw (glyph quads) cases (ui doc §11.1, U2 acceptance: >= 40 per function).
//!
//! Expected values are derived from the shipped `.tgf` glyph metrics (advance = width + spLeft +
//! spRight, shown per case as `char=advance` for the atlas the height maps to) and the
//! arithmetic of SimpleGraphic `r_font.cpp` @ 3b1a346 (Draw/DrawTextLine, r_font.cpp:321-457; r_main.cpp:1793-1809), computed in f32 by the
//! independent transcription `crates/pob-ui-oracle/probe/rfont_ref.py` and spot-checked by hand
//! (see the worked examples below). `b"..."` inputs are Lua byte strings: invalid UTF-8 decodes
//! to U+FFFD per byte (`IndexUTF8ToUTF32`), and the text ends at the first NUL (`const char*`).
//!
//! Each case lists the whole layer command stream: `C r g b a` (COLOR), `B FONT.h` (BIND of that
//! atlas), `Q ch x0 y0 x1 y1` (QUAD of glyph `ch`, TL = (x0, y0), BR = (x1, y1), physical px).
//! The test also checks every quad's corner order (TL, TR, BR, BL) and its texcoords against the
//! glyph's `.tgf` rectangle in the bound atlas. Covered: all 5 aligns (CENTER/RIGHT against the
//! screen width), `round(x)`/`floor(y)`, tabs (no ceil), `\n`, colour escapes (alpha forced to 1,
//! the colour leaks out), off-screen lines (escapes only, both edges), x culling with the BIND
//! still emitted, per-line re-BIND, tofu (smaller atlas, y pad, unscaled), scaled heights, NUL.
//!
//! Worked examples:
//! - VAR 16 "Hi" at (10.4, 20.7): x = round(10.4) = 10, y = floor(20.7) = 20; H spLeft 1 -> quad
//!   11..18, spRight 1 -> 19; i spLeft 1 -> 20..21.
//! - VAR 16 "Hello" CENTER at x 10 on a 1000 px screen: floor((1000 - 31) / 2 + 10) = 494, then
//!   H's spLeft -> 495.
//! - VAR 16 "AB" at x 2000: both glyphs are culled, but `drawCodepoint` binds before culling.
//! - VAR 13 "AB" at 0.25: atlas 12, scale 13/12; A (7, 0, 1) -> quad 0..7.5833335, then
//!   ceil(7.5833335 + 1.0833334) = 9; B (6, 0, 1) -> 9..15.5.

#[path = "font_util.rs"]
mod font_util;

use font_util::{atlas_of, fonts, glyph, Ev, Rec};
use pob_sg::Align::{self, *};
use pob_sg::Font::{self, *};

struct D {
    f: Font,
    h: i32,
    text: &'static [u8],
    pos: [f32; 2],
    align: Align,
    sw: i32,
    wh: i32,
    col: [f32; 4],
    expect: &'static str,
    col_after: [f32; 4],
}

#[rustfmt::skip]
const CASES: &[D] = &[
        D { f: Var, h: 16, text: b"", pos: [0.0, 0.0], align: Left, sw: 1000, wh: 800, col: [1.0, 1.0, 1.0, 1.0], expect: "", col_after: [1.0, 1.0, 1.0, 1.0] },
        D { f: Var, h: 16, text: b"A", pos: [0.0, 0.0], align: Left, sw: 1000, wh: 800, col: [1.0, 1.0, 1.0, 1.0], expect: "C 1 1 1 1 | B VAR.16 | Q A 0 0 9 16", col_after: [1.0, 1.0, 1.0, 1.0] },
        D { f: Var, h: 16, text: b"Hi", pos: [10.4, 20.7], align: Left, sw: 1000, wh: 800, col: [1.0, 1.0, 1.0, 1.0], expect: "C 1 1 1 1 | B VAR.16 | Q H 11 20 18 36 | Q i 20 20 21 36", col_after: [1.0, 1.0, 1.0, 1.0] },
        D { f: Var, h: 16, text: b"A", pos: [10.5, 0.0], align: Left, sw: 1000, wh: 800, col: [1.0, 1.0, 1.0, 1.0], expect: "C 1 1 1 1 | B VAR.16 | Q A 11 0 20 16", col_after: [1.0, 1.0, 1.0, 1.0] },
        D { f: Var, h: 16, text: b"A", pos: [-0.5, 0.0], align: Left, sw: 1000, wh: 800, col: [1.0, 1.0, 1.0, 1.0], expect: "C 1 1 1 1 | B VAR.16 | Q A -1 0 8 16", col_after: [1.0, 1.0, 1.0, 1.0] },
        D { f: Var, h: 16, text: b"AB", pos: [-20.0, 0.0], align: Left, sw: 1000, wh: 800, col: [1.0, 1.0, 1.0, 1.0], expect: "C 1 1 1 1 | B VAR.16", col_after: [1.0, 1.0, 1.0, 1.0] },
        D { f: Var, h: 16, text: b"ABC", pos: [995.0, 0.0], align: Left, sw: 1000, wh: 800, col: [1.0, 1.0, 1.0, 1.0], expect: "C 1 1 1 1 | B VAR.16 | Q A 995 0 1004 16", col_after: [1.0, 1.0, 1.0, 1.0] },
        D { f: Var, h: 16, text: b"AB", pos: [2000.0, 0.0], align: Left, sw: 1000, wh: 800, col: [1.0, 1.0, 1.0, 1.0], expect: "C 1 1 1 1 | B VAR.16", col_after: [1.0, 1.0, 1.0, 1.0] },
        D { f: Var, h: 16, text: b"Hello", pos: [10.0, 0.0], align: Center, sw: 1000, wh: 800, col: [1.0, 1.0, 1.0, 1.0], expect: "C 1 1 1 1 | B VAR.16 | Q H 495 0 502 16 | Q e 504 0 510 16 | Q l 512 0 513 16 | Q l 515 0 516 16 | Q o 518 0 524 16", col_after: [1.0, 1.0, 1.0, 1.0] },
        D { f: Var, h: 16, text: b"Hello", pos: [10.0, 0.0], align: Right, sw: 1000, wh: 800, col: [1.0, 1.0, 1.0, 1.0], expect: "C 1 1 1 1 | B VAR.16 | Q H 960 0 967 16 | Q e 969 0 975 16 | Q l 977 0 978 16 | Q l 980 0 981 16 | Q o 983 0 989 16", col_after: [1.0, 1.0, 1.0, 1.0] },
        D { f: Var, h: 16, text: b"Hello", pos: [100.0, 0.0], align: CenterX, sw: 1000, wh: 800, col: [1.0, 1.0, 1.0, 1.0], expect: "C 1 1 1 1 | B VAR.16 | Q H 85 0 92 16 | Q e 94 0 100 16 | Q l 102 0 103 16 | Q l 105 0 106 16 | Q o 108 0 114 16", col_after: [1.0, 1.0, 1.0, 1.0] },
        D { f: Var, h: 16, text: b"Hello", pos: [100.0, 0.0], align: RightX, sw: 1000, wh: 800, col: [1.0, 1.0, 1.0, 1.0], expect: "C 1 1 1 1 | B VAR.16 | Q H 70 0 77 16 | Q e 79 0 85 16 | Q l 87 0 88 16 | Q l 90 0 91 16 | Q o 93 0 99 16", col_after: [1.0, 1.0, 1.0, 1.0] },
        D { f: Var, h: 16, text: b"Hel", pos: [100.0, 0.0], align: CenterX, sw: 1000, wh: 800, col: [1.0, 1.0, 1.0, 1.0], expect: "C 1 1 1 1 | B VAR.16 | Q H 91 0 98 16 | Q e 100 0 106 16 | Q l 108 0 109 16", col_after: [1.0, 1.0, 1.0, 1.0] },
        D { f: Var, h: 16, text: b"Hel", pos: [0.0, 0.0], align: Center, sw: 1001, wh: 800, col: [1.0, 1.0, 1.0, 1.0], expect: "C 1 1 1 1 | B VAR.16 | Q H 491 0 498 16 | Q e 500 0 506 16 | Q l 508 0 509 16", col_after: [1.0, 1.0, 1.0, 1.0] },
        D { f: Var, h: 16, text: b"A\nHello", pos: [0.0, 0.0], align: Center, sw: 1000, wh: 800, col: [1.0, 1.0, 1.0, 1.0], expect: "C 1 1 1 1 | B VAR.16 | Q A 495 0 504 16 | B VAR.16 | Q H 485 16 492 32 | Q e 494 16 500 32 | Q l 502 16 503 32 | Q l 505 16 506 32 | Q o 508 16 514 32", col_after: [1.0, 1.0, 1.0, 1.0] },
        D { f: Var, h: 16, text: b"A\nHello", pos: [50.0, 5.0], align: RightX, sw: 1000, wh: 800, col: [1.0, 1.0, 1.0, 1.0], expect: "C 1 1 1 1 | B VAR.16 | Q A 41 5 50 21 | B VAR.16 | Q H 20 21 27 37 | Q e 29 21 35 37 | Q l 37 21 38 37 | Q l 40 21 41 37 | Q o 43 21 49 37", col_after: [1.0, 1.0, 1.0, 1.0] },
        D { f: Var, h: 16, text: b"A\nB", pos: [0.0, 0.0], align: Left, sw: 1000, wh: 800, col: [1.0, 1.0, 1.0, 1.0], expect: "C 1 1 1 1 | B VAR.16 | Q A 0 0 9 16 | B VAR.16 | Q B 1 16 8 32", col_after: [1.0, 1.0, 1.0, 1.0] },
        D { f: Var, h: 16, text: b"A\n\nB", pos: [0.0, 0.0], align: Left, sw: 1000, wh: 800, col: [1.0, 1.0, 1.0, 1.0], expect: "C 1 1 1 1 | B VAR.16 | Q A 0 0 9 16 | B VAR.16 | Q B 1 32 8 48", col_after: [1.0, 1.0, 1.0, 1.0] },
        D { f: Var, h: 16, text: b"A\n", pos: [0.0, 0.0], align: Left, sw: 1000, wh: 800, col: [1.0, 1.0, 1.0, 1.0], expect: "C 1 1 1 1 | B VAR.16 | Q A 0 0 9 16", col_after: [1.0, 1.0, 1.0, 1.0] },
        D { f: Var, h: 16, text: b"A", pos: [0.0, -16.0], align: Left, sw: 1000, wh: 800, col: [1.0, 1.0, 1.0, 1.0], expect: "C 1 1 1 1", col_after: [1.0, 1.0, 1.0, 1.0] },
        D { f: Var, h: 16, text: b"A", pos: [0.0, -15.0], align: Left, sw: 1000, wh: 800, col: [1.0, 1.0, 1.0, 1.0], expect: "C 1 1 1 1 | B VAR.16 | Q A 0 -15 9 1", col_after: [1.0, 1.0, 1.0, 1.0] },
        D { f: Var, h: 16, text: b"A", pos: [0.0, 800.0], align: Left, sw: 1000, wh: 800, col: [1.0, 1.0, 1.0, 1.0], expect: "C 1 1 1 1", col_after: [1.0, 1.0, 1.0, 1.0] },
        D { f: Var, h: 16, text: b"A", pos: [0.0, 799.5], align: Left, sw: 1000, wh: 800, col: [1.0, 1.0, 1.0, 1.0], expect: "C 1 1 1 1 | B VAR.16 | Q A 0 799 9 815", col_after: [1.0, 1.0, 1.0, 1.0] },
        D { f: Var, h: 16, text: b"^1A\n^2B", pos: [0.0, -16.0], align: Left, sw: 1000, wh: 800, col: [1.0, 1.0, 1.0, 1.0], expect: "C 1 1 1 1 | C 1 0 0 1 | C 0 1 0 1 | B VAR.16 | Q B 1 0 8 16", col_after: [0.0, 1.0, 0.0, 1.0] },
        D { f: Var, h: 16, text: b"^1A^2B", pos: [0.0, 0.0], align: Left, sw: 1000, wh: 800, col: [0.5, 0.25, 0.75, 0.25], expect: "C 0.5 0.25 0.75 0.25 | C 1 0 0 1 | B VAR.16 | Q A 0 0 9 16 | C 0 1 0 1 | Q B 10 0 17 16", col_after: [0.0, 1.0, 0.0, 1.0] },
        D { f: Var, h: 16, text: b"^xFF8000A", pos: [0.0, 0.0], align: Left, sw: 1000, wh: 800, col: [0.5, 0.25, 0.75, 0.25], expect: "C 0.5 0.25 0.75 0.25 | C 1 0.5019608 0 1 | B VAR.16 | Q A 0 0 9 16", col_after: [1.0, 0.5019608, 0.0, 1.0] },
        D { f: Var, h: 16, text: b"^1", pos: [0.0, 0.0], align: Left, sw: 1000, wh: 800, col: [0.5, 0.25, 0.75, 0.25], expect: "C 0.5 0.25 0.75 0.25 | C 1 0 0 1", col_after: [1.0, 0.0, 0.0, 1.0] },
        D { f: Var, h: 16, text: b"\tA", pos: [0.0, 0.0], align: Left, sw: 1000, wh: 800, col: [1.0, 1.0, 1.0, 1.0], expect: "C 1 1 1 1 | B VAR.16 | Q A 16 0 25 16", col_after: [1.0, 1.0, 1.0, 1.0] },
        D { f: Var, h: 30, text: b"a\tb", pos: [0.0, 0.0], align: Left, sw: 1000, wh: 800, col: [1.0, 1.0, 1.0, 1.0], expect: "C 1 1 1 1 | B VAR.28 | Q a 1.0714285 0 14.999999 30 | Q b 46.07143 0 58.928574 30", col_after: [1.0, 1.0, 1.0, 1.0] },
        D { f: Var, h: 16, text: b"\xc3\xa9", pos: [0.0, 0.0], align: Left, sw: 1000, wh: 800, col: [1.0, 1.0, 1.0, 1.0], expect: "C 1 1 1 1 | B VAR.12 | Q [ 0 2 2 14 | Q U 3 2 9 14 | Q + 9 2 16 14 | Q 0 16 2 21 14 | Q 0 22 2 27 14 | Q E 28 2 33 14 | Q 9 34 2 39 14 | Q ] 40 2 42 14", col_after: [1.0, 1.0, 1.0, 1.0] },
        D { f: Var, h: 16, text: b"a\xc3\xa9", pos: [0.0, 0.0], align: Left, sw: 1000, wh: 800, col: [1.0, 1.0, 1.0, 1.0], expect: "C 1 1 1 1 | B VAR.16 | Q a 1 0 8 16 | B VAR.12 | Q [ 8 2 10 14 | Q U 11 2 17 14 | Q + 17 2 24 14 | Q 0 24 2 29 14 | Q 0 30 2 35 14 | Q E 36 2 41 14 | Q 9 42 2 47 14 | Q ] 48 2 50 14", col_after: [1.0, 1.0, 1.0, 1.0] },
        D { f: Var, h: 16, text: b"\xc3\xa9a", pos: [0.0, 0.0], align: Left, sw: 1000, wh: 800, col: [1.0, 1.0, 1.0, 1.0], expect: "C 1 1 1 1 | B VAR.12 | Q [ 0 2 2 14 | Q U 3 2 9 14 | Q + 9 2 16 14 | Q 0 16 2 21 14 | Q 0 22 2 27 14 | Q E 28 2 33 14 | Q 9 34 2 39 14 | Q ] 40 2 42 14 | B VAR.16 | Q a 44 0 51 16", col_after: [1.0, 1.0, 1.0, 1.0] },
        D { f: Var, h: 12, text: b"\xc3\xa9", pos: [0.0, 0.0], align: Left, sw: 1000, wh: 800, col: [1.0, 1.0, 1.0, 1.0], expect: "C 1 1 1 1 | B VAR.12 | Q [ 0 0 2 12 | Q U 3 0 9 12 | Q + 9 0 16 12 | Q 0 16 0 21 12 | Q 0 22 0 27 12 | Q E 28 0 33 12 | Q 9 34 0 39 12 | Q ] 40 0 42 12", col_after: [1.0, 1.0, 1.0, 1.0] },
        D { f: Var, h: 30, text: b"AB", pos: [0.0, 0.0], align: Left, sw: 1000, wh: 800, col: [1.0, 1.0, 1.0, 1.0], expect: "C 1 1 1 1 | B VAR.28 | Q A 0 0 18.214285 30 | Q B 21.142857 0 36.142857 30", col_after: [1.0, 1.0, 1.0, 1.0] },
        D { f: Var, h: 13, text: b"AB", pos: [0.25, 0.0], align: Left, sw: 1000, wh: 800, col: [1.0, 1.0, 1.0, 1.0], expect: "C 1 1 1 1 | B VAR.12 | Q A 0 0 7.5833335 13 | Q B 9 0 15.5 13", col_after: [1.0, 1.0, 1.0, 1.0] },
        D { f: Var, h: 16, text: b" ", pos: [0.0, 0.0], align: Left, sw: 1000, wh: 800, col: [1.0, 1.0, 1.0, 1.0], expect: "C 1 1 1 1 | B VAR.16 | Q U+0020 0 0 1 16", col_after: [1.0, 1.0, 1.0, 1.0] },
        D { f: Fixed, h: 12, text: b"ab", pos: [0.0, 0.0], align: Left, sw: 1000, wh: 800, col: [1.0, 1.0, 1.0, 1.0], expect: "C 1 1 1 1 | B FIXED.12 | Q a 1 0 6 12 | Q b 7 0 12 12", col_after: [1.0, 1.0, 1.0, 1.0] },
        D { f: FontinItalic, h: 20, text: b"fi", pos: [3.0, 0.0], align: Left, sw: 1000, wh: 800, col: [1.0, 1.0, 1.0, 1.0], expect: "C 1 1 1 1 | B FONTIN ITALIC.20 | Q f 4 0 11 20 | Q i 9 0 14 20", col_after: [1.0, 1.0, 1.0, 1.0] },
        D { f: Var, h: 16, text: b"A\x00B", pos: [0.0, 0.0], align: Left, sw: 1000, wh: 800, col: [1.0, 1.0, 1.0, 1.0], expect: "C 1 1 1 1 | B VAR.16 | Q A 0 0 9 16", col_after: [1.0, 1.0, 1.0, 1.0] },
        D { f: Var, h: 16, text: b"\xff", pos: [0.0, 0.0], align: Left, sw: 1000, wh: 800, col: [1.0, 1.0, 1.0, 1.0], expect: "C 1 1 1 1 | B VAR.12 | Q [ 0 2 2 14 | Q U 3 2 9 14 | Q + 9 2 16 14 | Q F 16 2 21 14 | Q F 22 2 27 14 | Q F 28 2 33 14 | Q D 34 2 40 14 | Q ] 41 2 43 14", col_after: [1.0, 1.0, 1.0, 1.0] },
        D { f: Var, h: 16, text: b"\xf0\x9f\x98\x80", pos: [0.0, 0.0], align: Left, sw: 1000, wh: 800, col: [1.0, 1.0, 1.0, 1.0], expect: "C 1 1 1 1 | B VAR.12 | Q [ 0 2 2 14 | Q U 3 2 9 14 | Q + 9 2 16 14 | Q 1 17 2 22 14 | Q F 22 2 27 14 | Q 6 28 2 33 14 | Q 0 34 2 39 14 | Q 0 40 2 45 14 | Q ] 46 2 48 14", col_after: [1.0, 1.0, 1.0, 1.0] },
        D { f: Var, h: 16, text: b"\t^1\xc3\xa9", pos: [0.0, 0.0], align: Left, sw: 1000, wh: 800, col: [1.0, 1.0, 1.0, 1.0], expect: "C 1 1 1 1 | C 1 0 0 1 | B VAR.12 | Q [ 16 2 18 14 | Q U 19 2 25 14 | Q + 25 2 32 14 | Q 0 32 2 37 14 | Q 0 38 2 43 14 | Q E 44 2 49 14 | Q 9 50 2 55 14 | Q ] 56 2 58 14", col_after: [1.0, 0.0, 0.0, 1.0] },
        D { f: Var, h: 16, text: b"A", pos: [0.0, 10.9], align: Left, sw: 1000, wh: 800, col: [1.0, 1.0, 1.0, 1.0], expect: "C 1 1 1 1 | B VAR.16 | Q A 0 10 9 26", col_after: [1.0, 1.0, 1.0, 1.0] },
        D { f: Var, h: 100, text: b"A", pos: [0.0, 0.0], align: Left, sw: 1000, wh: 800, col: [1.0, 1.0, 1.0, 1.0], expect: "C 1 1 1 1 | B VAR.64 | Q A 0 0 57.8125 100", col_after: [1.0, 1.0, 1.0, 1.0] },
        D { f: Var, h: 16, text: b"Hello", pos: [-20.0, 0.0], align: Right, sw: 1000, wh: 800, col: [1.0, 1.0, 1.0, 1.0], expect: "C 1 1 1 1 | B VAR.16 | Q H 990 0 997 16 | Q e 999 0 1005 16", col_after: [1.0, 1.0, 1.0, 1.0] },
        D { f: Var, h: 16, text: b"\xc3\xa9", pos: [0.0, -16.0], align: Left, sw: 1000, wh: 800, col: [1.0, 1.0, 1.0, 1.0], expect: "C 1 1 1 1", col_after: [1.0, 1.0, 1.0, 1.0] },
];

#[test]
fn hand_draws() {
    assert!(CASES.len() >= 40);
    let fs = fonts();
    let mut bad = Vec::new();
    for c in CASES {
        let mut col = c.col;
        let mut rec = Rec::default();
        fs.face(c.f)
            .draw_bytes(&mut rec, c.pos, c.align, c.h, &mut col, c.text, c.sw, c.wh);
        let want: Vec<&str> = if c.expect.is_empty() {
            Vec::new()
        } else {
            c.expect.split(" | ").collect()
        };
        let mut got = Vec::new();
        let mut bound = None;
        for (k, e) in rec.0.iter().enumerate() {
            got.push(match e {
                Ev::Color(v) => format!("C {} {} {} {}", v[0], v[1], v[2], v[3]),
                Ev::Bind(t) => {
                    let (f, h) = atlas_of(&fs, *t);
                    bound = Some((f, h));
                    format!("B {}.{h}", f.lua_name())
                }
                Ev::Quad(q) => {
                    // The glyph comes from the expected token at the same position.
                    let ch = want
                        .get(k)
                        .and_then(|w| w.strip_prefix("Q "))
                        .and_then(|w| w.split(' ').next());
                    let ch = ch.unwrap_or("?");
                    let cp = match ch.strip_prefix("U+") {
                        Some(hex) => u32::from_str_radix(hex, 16).unwrap(),
                        None => u32::from(ch.chars().next().unwrap()),
                    };
                    let (f, h) = bound.expect("BIND before QUAD");
                    let g = glyph(fs.face(f), h, cp);
                    assert_eq!(
                        q.s,
                        [g.tc_l, g.tc_r, g.tc_r, g.tc_l],
                        "{:?} texcoords s",
                        c.expect
                    );
                    assert_eq!(
                        q.t,
                        [g.tc_t, g.tc_t, g.tc_b, g.tc_b],
                        "{:?} texcoords t",
                        c.expect
                    );
                    assert_eq!(
                        [q.x[3], q.x[2], q.y[1], q.y[3]],
                        [q.x[0], q.x[1], q.y[0], q.y[2]],
                        "corner order"
                    );
                    format!("Q {ch} {} {} {} {}", q.x[0], q.y[0], q.x[1], q.y[2])
                }
            });
        }
        if got != want || col != c.col_after {
            bad.push(format!(
                "{:?} {} {:?} {:?} {:?}:\n  got  {}\n  want {}\n  col {:?} want {:?}",
                c.f,
                c.h,
                String::from_utf8_lossy(c.text),
                c.pos,
                c.align,
                got.join(" | "),
                c.expect,
                col,
                c.col_after
            ));
        }
    }
    assert!(
        bad.is_empty(),
        "{} of {} draw cases differ:\n{}",
        bad.len(),
        CASES.len(),
        bad.join("\n")
    );
}
