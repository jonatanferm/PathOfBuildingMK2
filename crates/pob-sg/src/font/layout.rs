//! `r_font.cpp` layout: StringWidth, StringCursorIndex, Draw / DrawTextLine (ui doc §4.4). Owner: U2.
//!
//! Moved from the pre-port `pob-sg/src/font.rs` by UI-U0 and adapted to the frozen surface:
//! `&str` input (decoded with `IndexUTF8ToUTF32` semantics), byte-offset cursor results, and a
//! [`QuadSink`] that receives SG's BIND / COLOR / QUAD commands.
//!
//! Delta U2 closes: the moved code binds the atlas whenever it changes and emits COLOR at the start
//! and after each escape; U2 checks this statement by statement against `r_font.cpp:288-455`.

use super::{index_utf8, tofu_string, FontFace, TOFU_SIZE_REDUCTION};
use crate::color::{is_color_escape, read_color_escape};
use crate::frame::{Quad, QuadSink};
use crate::image::TexId;
use crate::types::Align;

const NL: u32 = '\n' as u32;
const TAB: u32 = '\t' as u32;
const SPACE: u32 = ' ' as u32;

fn is_color_escape_u32(s: &[u32]) -> usize {
    // IsColorEscape(std::u32string_view): same rules as the byte version but on codepoints; only
    // ASCII digits / hex digits qualify.
    if s.len() < 2 || s[0] != u32::from('^') {
        return 0;
    }
    let mut buf = [0u8; 8];
    for (i, &c) in s.iter().take(8).enumerate() {
        buf[i] = if c < 128 { c as u8 } else { 0 };
    }
    is_color_escape(&buf[..s.len().min(8)])
}

fn read_color_escape_u32(s: &[u32], len: usize, out: &mut [f32; 4]) {
    let mut buf = [0u8; 8];
    for (i, &c) in s.iter().take(len).enumerate() {
        buf[i] = c as u8;
    }
    read_color_escape(&buf[..len], len, out);
}

impl FontFace {
    fn measure(&self, idx: usize, cp: u32) -> i32 {
        self.heights[idx].glyph(cp).advance()
    }

    /// `r_font_c::StringWidthInternal` for one line (no `'\n'`).
    fn line_width(&self, idx: usize, line: &[u32], height: i32, scale: f32) -> i32 {
        let fh = &self.heights[idx];
        let (tofu_idx, _) = self.find_smaller(height, idx, TOFU_SIZE_REDUCTION);
        let mut width = 0.0f32;
        let mut i = 0;
        while i < line.len() {
            let ch = line[i];
            let esc = is_color_escape_u32(&line[i..]);
            if esc > 0 {
                i += esc;
            } else if ch as usize >= fh.num_glyph {
                for cp in tofu_string(ch).chars() {
                    width += self.measure(tofu_idx, cp as u32) as f32;
                    width = width.ceil();
                }
                i += 1;
            } else if ch == TAB {
                let sp = fh.glyph(SPACE).advance();
                width += sp as f32 * 4.0 * scale;
                width = width.ceil();
                i += 1;
            } else {
                width += self.measure(idx, ch) as f32 * scale;
                width = width.ceil();
                i += 1;
            }
        }
        width as i32
    }

    /// `r_font_c::StringWidth`: widest line, in physical pixels.
    pub fn string_width(&self, h: i32, text: &str) -> i32 {
        if self.heights.is_empty() {
            return 0;
        }
        let (cps, _) = index_utf8(text);
        let idx = self.find(h);
        let scale = h as f32 / self.heights[idx].height as f32;
        cps.split(|&c| c == NL)
            .filter(|l| !l.is_empty())
            .map(|l| self.line_width(idx, l, h, scale))
            .max()
            .unwrap_or(0)
    }

    /// `r_font_c::StringCursorInternal`: codepoint index within one line.
    fn line_cursor(&self, idx: usize, line: &[u32], height: i32, scale: f32, cur_x: i32) -> usize {
        let fh = &self.heights[idx];
        let (tofu_idx, _) = self.find_smaller(height, idx, TOFU_SIZE_REDUCTION);
        let cur_x = cur_x as f32;
        let mut x = 0.0f32;
        let mut i = 0;
        while i < line.len() {
            let ch = line[i];
            let esc = is_color_escape_u32(&line[i..]);
            if esc > 0 {
                i += esc;
            } else if ch as usize >= fh.num_glyph {
                for cp in tofu_string(ch).chars() {
                    x += self.measure(tofu_idx, cp as u32) as f32;
                    x = x.ceil();
                    if cur_x <= x {
                        return i;
                    }
                }
                i += 1;
            } else if ch == TAB {
                let full = fh.glyph(SPACE).advance() as f32 * 4.0 * scale;
                let half = (full / 2.0).ceil();
                x += half;
                x = x.ceil();
                if cur_x <= x {
                    break;
                }
                x += full - half;
                x = x.ceil();
                if cur_x <= x {
                    break;
                }
                i += 1;
            } else {
                x += self.measure(idx, ch) as f32 * scale;
                x = x.ceil();
                if cur_x <= x {
                    break;
                }
                i += 1;
            }
        }
        i
    }

    /// `r_font_c::StringCursorIndex`: 0-based BYTE offset for a point relative to the text origin
    /// (the codepoint index mapped back through the `IndexUTF8ToUTF32` offset table). Lines are
    /// `h` tall; the first line whose bottom (`lineY`) is `>= cy` is used, else the last line.
    pub fn cursor_index(&self, h: i32, text: &str, cx: i32, cy: i32) -> usize {
        if self.heights.is_empty() || text.is_empty() {
            return 0;
        }
        let (text_cps, offs) = index_utf8(text);
        let idx = self.find(h);
        let scale = h as f32 / self.heights[idx].height as f32;
        let mut last_index = 0;
        let mut line_y = h;
        let mut start = 0;
        while start != text_cps.len() {
            let line_end = text_cps[start..]
                .iter()
                .position(|&c| c == NL)
                .map_or(text_cps.len(), |p| start + p);
            last_index = self.line_cursor(idx, &text_cps[start..line_end], h, scale, cx);
            if cy <= line_y {
                break;
            }
            if line_end == text_cps.len() {
                break;
            }
            start = line_end + 1;
            line_y += h;
        }
        let cp = start + last_index;
        if cp < offs.len() {
            offs[cp]
        } else {
            text.len()
        }
    }

    /// Draw + DrawTextLine. `col` is the renderer's draw colour and is updated by escapes, exactly
    /// as SimpleGraphic mutates `drawColor` through the pointer it passes.
    pub fn draw(
        &self,
        out: &mut dyn QuadSink,
        pos: [f32; 2],
        align: Align,
        h: i32,
        col: &mut [f32; 4],
        text: &str,
        screen_w: i32,
        win_h: i32,
    ) {
        if text.is_empty() {
            return;
        }
        let (cps, _) = index_utf8(text);
        out.color(*col);
        let mut bound: Option<TexId> = None;
        let mut y = pos[1];
        for line in cps.split(|&c| c == NL) {
            if !line.is_empty() {
                self.draw_text_line(
                    out,
                    &mut bound,
                    [pos[0], y],
                    align,
                    h,
                    col,
                    line,
                    screen_w,
                    win_h,
                );
            }
            y += h as f32;
        }
    }

    /// `r_font_c::DrawTextLine`.
    fn draw_text_line(
        &self,
        out: &mut dyn QuadSink,
        bound: &mut Option<TexId>,
        pos: [f32; 2],
        align: Align,
        height: i32,
        col: &mut [f32; 4],
        line: &[u32],
        screen_w: i32,
        win_h: i32,
    ) {
        // Check if the line is visible; if not, only process the colour codes.
        if self.heights.is_empty() || pos[1] >= win_h as f32 || pos[1] <= -(height as f32) {
            let mut i = 0;
            while i < line.len() {
                let esc = is_color_escape_u32(&line[i..]);
                if esc > 0 {
                    read_color_escape_u32(&line[i..], esc, col);
                    col[3] = 1.0;
                    out.color(*col);
                    i += esc;
                } else {
                    i += 1;
                }
            }
            return;
        }

        let idx = self.find(height);
        let fh = &self.heights[idx];
        let scale = height as f32 / fh.height as f32;
        let (tofu_idx, tofu_pad) = self.find_smaller(height, idx, TOFU_SIZE_REDUCTION);
        let screen_w = screen_w as f32;

        let mut x = pos[0];
        let y = pos[1].floor();
        if align != Align::Left {
            let width = self.line_width(idx, line, height, scale) as f32;
            x = match align {
                Align::Center => ((screen_w - width) / 2.0 + pos[0]).floor(),
                Align::Right => (screen_w - width - pos[0]).floor(),
                Align::CenterX => (pos[0] - width / 2.0).floor(),
                Align::RightX => (pos[0] - width).floor(),
                Align::Left => x,
            };
        }
        // Snap the starting x position to the pixel grid (std::round).
        x = x.round();

        let mut draw_cp = |out: &mut dyn QuadSink,
                           x: &mut f32,
                           hidx: usize,
                           qh: f32,
                           scale: f32,
                           y_shift: i32,
                           cp: u32| {
            let fh = &self.heights[hidx];
            let cp_y = y + y_shift as f32;
            let g = fh.glyph(cp);
            *x += g.sp_l as f32 * scale;
            if g.width != 0 {
                let w = g.width as f32 * scale;
                if *x + w >= 0.0 && *x < screen_w {
                    if *bound != Some(fh.tex) {
                        out.bind(fh.tex);
                        *bound = Some(fh.tex);
                    }
                    out.quad(Quad {
                        s: [g.tc_l, g.tc_r, g.tc_r, g.tc_l],
                        t: [g.tc_t, g.tc_t, g.tc_b, g.tc_b],
                        x: [*x, *x + w, *x + w, *x],
                        y: [cp_y, cp_y, cp_y + qh, cp_y + qh],
                    });
                }
                *x += w;
            }
            *x += g.sp_r as f32 * scale;
            *x = x.ceil();
        };

        let mut i = 0;
        while i < line.len() {
            let ch = line[i];
            // Draw unprintable characters as tofu placeholders.
            if ch as usize >= fh.num_glyph {
                let tfh = self.heights[tofu_idx].height as f32;
                for t in tofu_string(ch).chars() {
                    draw_cp(out, &mut x, tofu_idx, tfh, 1.0, tofu_pad, t as u32);
                }
                i += 1;
                continue;
            }
            // Check for escape character.
            let esc = is_color_escape_u32(&line[i..]);
            if esc > 0 {
                read_color_escape_u32(&line[i..], esc, col);
                col[3] = 1.0;
                out.color(*col);
                i += esc;
                continue;
            }
            // Handle tabs (note: no ceil here, unlike normal glyphs).
            if ch == TAB {
                let sp = fh.glyph(SPACE).advance();
                x += (sp << 2) as f32 * scale;
                i += 1;
                continue;
            }
            draw_cp(out, &mut x, idx, height as f32, scale, 0, ch);
            i += 1;
        }
    }
}
