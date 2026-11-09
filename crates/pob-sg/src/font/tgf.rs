//! `.tgf` font description parser (ui doc §3 `font/tgf.rs`). Owner: U2.
//!
//! Port of the parsing loop of the `r_font_c` constructor (PathOfBuilding-SimpleGraphic @ 3b1a346
//! `engine/render/r_font.cpp:66-97`): `std::getline` splits on `'\n'`; a line for which
//! `sscanf(line, "HEIGHT %u;", &h) == 1` starts an atlas height; otherwise, once a height exists,
//! a line for which `sscanf(line, "GLYPH %u %u %u %d %d;", ...) == 5` adds a glyph
//! (x, y, width, spLeft, spRight) to the last height, at most 128 per height.
//! Moved from the pre-port `pob-sg/src/font.rs` by UI-U0; U2 replaced the token parser with a
//! `sscanf` emulation (the literal must start the line, trailing text after the last conversion
//! is ignored, `%u` accepts a sign and wraps like `strtoul`).
//!
//! The texture coordinates need the atlas size, so they are computed in `FontFace::load`.

/// One `GLYPH x y width spLeft spRight;` line (the `%u`/`%d` values as the C `int`s they land in).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct TgfGlyph {
    pub x: u32,
    pub y: u32,
    pub width: u32,
    pub sp_left: i32,
    pub sp_right: i32,
}

/// One `HEIGHT h;` block.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct TgfHeight {
    pub height: i32,
    pub glyphs: Vec<TgfGlyph>,
}

/// One `sscanf` format directive of the two formats used.
#[derive(Clone, Copy)]
enum Dir {
    Lit(u8),
    Space,
    Int,
}

/// `sscanf(line, fmt, ...)` for formats built from literals, single spaces and `%u`/`%d`:
/// returns the converted values (their count is `sscanf`'s return value).
/// - a space directive skips any amount of whitespace (including none);
/// - an ordinary character must match exactly, else scanning stops;
/// - `%u`/`%d` skip leading whitespace, accept an optional sign and at least one decimal digit,
///   and convert with `strtoul`/`strtol` wrap-around into the 32-bit `int` target.
fn scan(line: &[u8], fmt: &[Dir]) -> Vec<i32> {
    let mut vals = Vec::new();
    let mut p = 0;
    let is_space = |c: u8| matches!(c, b' ' | b'\t' | b'\n' | b'\r' | 0x0b | 0x0c);
    for d in fmt {
        match *d {
            Dir::Space => {
                while p < line.len() && is_space(line[p]) {
                    p += 1;
                }
            }
            Dir::Lit(c) => {
                if p < line.len() && line[p] == c {
                    p += 1;
                } else {
                    return vals;
                }
            }
            Dir::Int => {
                while p < line.len() && is_space(line[p]) {
                    p += 1;
                }
                let mut neg = false;
                if p < line.len() && (line[p] == b'+' || line[p] == b'-') {
                    neg = line[p] == b'-';
                    p += 1;
                }
                let start = p;
                let mut v: u32 = 0;
                while p < line.len() && line[p].is_ascii_digit() {
                    v = v.wrapping_mul(10).wrapping_add(u32::from(line[p] - b'0'));
                    p += 1;
                }
                if p == start {
                    return vals;
                }
                let v = if neg { v.wrapping_neg() } else { v };
                vals.push(v as i32);
            }
        }
    }
    vals
}

fn format(f: &str) -> Vec<Dir> {
    let mut out = Vec::new();
    let b = f.as_bytes();
    let mut i = 0;
    while i < b.len() {
        if b[i] == b'%' {
            out.push(Dir::Int);
            i += 2;
        } else if b[i] == b' ' {
            out.push(Dir::Space);
            i += 1;
        } else {
            out.push(Dir::Lit(b[i]));
            i += 1;
        }
    }
    out
}

/// Parse a whole `.tgf` file (bytes; the files are ASCII).
/// sg: r_font.cpp:66-97 @3b1a346
pub fn parse(text: &[u8]) -> Vec<TgfHeight> {
    let height_fmt = format("HEIGHT %u;");
    let glyph_fmt = format("GLYPH %u %u %u %d %d;");
    let mut heights: Vec<TgfHeight> = Vec::new();
    // Parse info file: std::getline splits on '\n' only.
    for sub in text.split(|&b| b == b'\n') {
        let h = scan(sub, &height_fmt);
        if h.len() == 1 {
            // New height
            heights.push(TgfHeight {
                height: h[0],
                glyphs: Vec::with_capacity(128),
            });
            continue;
        }
        let Some(fh) = heights.last_mut() else {
            continue;
        };
        let g = scan(sub, &glyph_fmt);
        if g.len() == 5 {
            // Add glyph
            if fh.glyphs.len() >= 128 {
                continue;
            }
            fh.glyphs.push(TgfGlyph {
                x: g[0] as u32,
                y: g[1] as u32,
                width: g[2] as u32,
                sp_left: g[3],
                sp_right: g[4],
            });
        }
    }
    heights
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sscanf_semantics() {
        let t = "HEIGHT 10;\r\nGLYPH   0   0  6  0  1;\t// 0\n GLYPH 1 1 1 1 1;\nGLYPH 8 0 6 -1 +2 trailing\n\
                 GLYPH 1 2 3 4;\nHEIGHT12\nGLYPH 3 4 5 6 7;\nHEIGHT 14 99;\nHEIGHT x;\nGLYPHS 1 2 3 4 5;";
        let hs = parse(t.as_bytes());
        assert_eq!(hs.len(), 3);
        assert_eq!(hs[0].height, 10);
        assert_eq!(
            hs[0].glyphs,
            vec![
                TgfGlyph {
                    x: 0,
                    y: 0,
                    width: 6,
                    sp_left: 0,
                    sp_right: 1
                },
                TgfGlyph {
                    x: 8,
                    y: 0,
                    width: 6,
                    sp_left: -1,
                    sp_right: 2
                },
            ]
        );
        // "HEIGHT12": the space directive matches zero whitespace.
        assert_eq!(hs[1].height, 12);
        assert_eq!(hs[1].glyphs.len(), 1);
        // "HEIGHT 14 99;": one conversion, then ';' fails to match: still a height.
        assert_eq!(hs[2].height, 14);
        assert!(hs[2].glyphs.is_empty());
    }

    #[test]
    fn glyphs_capped_at_128_and_need_a_height() {
        let mut t = String::from("GLYPH 1 1 1 1 1;\nHEIGHT 10;\n");
        t.extend((0..130).map(|i| format!("GLYPH {i} 0 1 0 0;\n")));
        let hs = parse(t.as_bytes());
        assert_eq!(hs.len(), 1);
        assert_eq!(hs[0].glyphs.len(), 128);
        assert_eq!(hs[0].glyphs[127].x, 127);
    }
}
