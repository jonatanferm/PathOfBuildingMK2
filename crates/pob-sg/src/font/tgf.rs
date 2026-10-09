//! `.tgf` font description parser (ui doc §3 `font/tgf.rs`). Owner: U2.
//!
//! Port of the parsing loop of the `r_font_c` constructor (`engine/render/r_font.cpp`,
//! SimpleGraphic research copy master @ 8694a07): lines `HEIGHT %u;` start an atlas height,
//! lines `GLYPH %u %u %u %d %d;` (x, y, width, spLeft, spRight) add a glyph to the last height
//! (at most 128 per height). Moved from the pre-port `pob-sg/src/font.rs` by UI-U0.

/// One `GLYPH x y width spLeft spRight;` line.
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

/// Equivalent of the sscanf formats "HEIGHT %u;" and "GLYPH %u %u %u %d %d;": keyword,
/// whitespace separated integers, terminated by ';'.
fn parse_tgf_line(line: &str) -> Option<(&str, Vec<i64>)> {
    let body = line.split(';').next()?;
    let mut it = body.split_whitespace();
    let kw = it.next()?;
    let nums: Option<Vec<i64>> = it.map(|t| t.parse::<i64>().ok()).collect();
    Some((kw, nums?))
}

/// Parse a whole `.tgf` file.
pub fn parse(text: &str) -> Vec<TgfHeight> {
    let mut heights: Vec<TgfHeight> = Vec::new();
    for line in text.lines() {
        let Some((kw, nums)) = parse_tgf_line(line) else {
            continue;
        };
        if kw == "HEIGHT" && nums.len() == 1 {
            heights.push(TgfHeight {
                height: nums[0] as i32,
                glyphs: Vec::with_capacity(128),
            });
        } else if kw == "GLYPH" && nums.len() == 5 {
            let Some(fh) = heights.last_mut() else {
                continue;
            };
            if fh.glyphs.len() >= 128 {
                continue;
            }
            fh.glyphs.push(TgfGlyph {
                x: nums[0] as u32,
                y: nums[1] as u32,
                width: nums[2] as u32,
                sp_left: nums[3] as i32,
                sp_right: nums[4] as i32,
            });
        }
    }
    heights
}
