//! `r_font.cpp` layout: StringWidth, StringCursorIndex, Draw / DrawTextLine (ui doc §4.4). Owner: U2.
//!
//! Port of PathOfBuilding-SimpleGraphic @ 3b1a346 `engine/render/r_font.cpp:131-457` (the commit
//! the shipped `SimpleGraphic.dll` names; the file is byte-identical to the research copy, master
//! @ 8694a07), plus the `std::u32string_view` colour-escape helpers of `common.cpp:220-299` and
//! the text entry points of `r_renderer_c` (`r_main.cpp:1793-1860`). Every `float` of the C++ is
//! an `f32` here and every operation keeps the C++ evaluation order, so results are bit-exact.
//!
//! Layers, mirroring SG:
//! - `*_u32` methods are `r_font_c`'s (`std::u32string_view` input, codepoint indices);
//! - [`FontFace::string_width`], [`FontFace::cursor_index`], [`FontFace::draw`] (and their
//!   `*_bytes` twins for Lua byte strings) are the `r_renderer_c::DrawString*` wrappers: the text
//!   is a C string (it ends at the first NUL, as `lua_tostring` → `const char*` does), it is
//!   decoded with `IndexUTF8ToUTF32`, and cursor results map back to BYTE offsets.
//!
//! U2 verification notes (deltas closed against the moved pre-port code):
//! - `drawCodepoint` binds the atlas at the START of every glyph whose atlas differs from the
//!   line's current one, before the glyph lookup and independent of width and culling, and the
//!   current atlas (`curTex`) is local to `DrawTextLine`, so every drawn line binds again
//!   (r_font.cpp:371-377). The moved code bound only before an emitted quad and across lines.
//! - Escape recognition uses the u32 rules of `common.cpp:220-246` directly.
//! - NUL truncation and `IndexUTF8ToUTF32`'s exact decoding (invalid bytes → U+FFFD, one byte).
//!
//! DIVERGENCE(empty font set): a [`FontFace`] without atlases (only [`super::FontSet::empty`],
//! never SG) measures 0 and draws every line as if it were off screen (escapes only).

use super::{index_utf8_to_utf32, tofu_string, FontFace, TOFU_SIZE_REDUCTION};
use crate::color::PALETTE;
use crate::frame::{Quad, QuadSink};
use crate::image::TexId;
use crate::types::Align;

const NL: u32 = '\n' as u32;
const TAB: u32 = '\t' as u32;
const SPACE: u32 = ' ' as u32;

/// `IsColorEscape(std::u32string_view)`: 0, 2 (`^N`, ASCII digit) or 8 (`^x`/`^X` + 6 hex digits).
/// sg: common.cpp:220-246 @3b1a346
pub fn is_color_escape_u32(s: &[u32]) -> usize {
    if s.len() < 2 || s[0] != u32::from('^') {
        return 0;
    }
    let discrim = s[1];
    // Check for indexed colour escape like ^7 (arabic numerals only).
    if (u32::from('0')..=u32::from('9')).contains(&discrim) {
        return 2;
    }
    // Check for direct colour escape like ^x123ABC.
    if s.len() >= 8 && (discrim == u32::from('x') || discrim == u32::from('X')) {
        for c in 0..6 {
            let ch = s[c + 2];
            let is_hex_digit = (u32::from('0')..=u32::from('9')).contains(&ch)
                || (u32::from('A')..=u32::from('F')).contains(&ch)
                || (u32::from('a')..=u32::from('f')).contains(&ch);
            if !is_hex_digit {
                return 0;
            }
        }
        return 8;
    }
    // Fallthrough indicates no recognized colour code.
    0
}

/// `ReadColorEscape(std::u32string_view, col3_t)`: writes RGB only and returns the tail.
/// sg: common.cpp:276-299 @3b1a346
pub fn read_color_escape_u32<'a>(s: &'a [u32], out: &mut [f32; 4]) -> &'a [u32] {
    let len = is_color_escape_u32(s);
    match len {
        2 => {
            // VectorCopy(colorEscape[str[1] - U'0'], out)
            let p = PALETTE[(s[1] - u32::from('0')) as usize];
            out[0] = p[0];
            out[1] = p[1];
            out[2] = p[2];
        }
        8 => {
            // sscanf(buf, "%2x%2x%2x"): the six characters are validated hex digits.
            let hex = |c: u32| char::from_u32(c).and_then(|c| c.to_digit(16)).unwrap_or(0) as i32;
            let xr = hex(s[2]) << 4 | hex(s[3]);
            let xg = hex(s[4]) << 4 | hex(s[5]);
            let xb = hex(s[6]) << 4 | hex(s[7]);
            out[0] = xr as f32 / 255.0;
            out[1] = xg as f32 / 255.0;
            out[2] = xb as f32 / 255.0;
        }
        _ => {}
    }
    &s[len..]
}

/// `const char*` view of a Lua string: everything before the first NUL.
fn c_str(text: &[u8]) -> &[u8] {
    match text.iter().position(|&b| b == 0) {
        Some(z) => &text[..z],
        None => text,
    }
}

/// The pieces of `str` between `'\n'`s, as `Draw`/`StringWidth` iterate them (a trailing
/// `'\n'` yields a final empty piece, which both skip).
fn lines(text: &[u32]) -> impl Iterator<Item = &[u32]> {
    text.split(|&c| c == NL)
}

impl FontFace {
    /// `measureCodepoint`: `width + spLeft + spRight` of `Glyph((char)(unsigned char)cp)`.
    fn measure_codepoint(&self, idx: usize, cp: u32) -> i32 {
        let glyph = self.heights[idx].glyph(cp);
        glyph.width + glyph.sp_l + glyph.sp_r
    }

    /// `r_font_c::StringWidthInternal`: one line (no `'\n'`), in physical px.
    /// sg: r_font.cpp:143-182 @3b1a346
    fn string_width_internal(&self, idx: usize, line: &[u32], height: i32, scale: f32) -> i32 {
        let fh = &self.heights[idx];
        let (tofu_idx, _) = self.find_smaller(height, idx, TOFU_SIZE_REDUCTION);
        let mut width = 0.0f32;
        let mut i = 0;
        while i < line.len() {
            let ch = line[i];
            let esc_len = is_color_escape_u32(&line[i..]);
            if esc_len > 0 {
                i += esc_len;
            } else if ch as usize >= fh.num_glyph {
                for cp in tofu_string(ch).chars() {
                    width += self.measure_codepoint(tofu_idx, cp as u32) as f32;
                    width = width.ceil();
                }
                i += 1;
            } else if ch == TAB {
                let glyph = fh.glyph(SPACE);
                let sp_width = glyph.width + glyph.sp_l + glyph.sp_r;
                width += (sp_width * 4) as f32 * scale;
                width = width.ceil();
                i += 1;
            } else {
                width += self.measure_codepoint(idx, ch) as f32 * scale;
                width = width.ceil();
                i += 1;
            }
        }
        width as i32
    }

    /// `r_font_c::StringWidth`: the widest line, in physical px.
    /// sg: r_font.cpp:184-203 @3b1a346
    pub fn string_width_u32(&self, height: i32, text: &[u32]) -> i32 {
        if self.heights.is_empty() {
            return 0; // DIVERGENCE(empty font set)
        }
        let idx = self.find(height);
        let fh = &self.heights[idx];
        let mut max = 0;
        let scale = height as f32 / fh.height as f32;
        for line in lines(text) {
            if !line.is_empty() {
                let lw = self.string_width_internal(idx, line, height, scale);
                max = max.max(lw);
            }
        }
        max
    }

    /// `r_font_c::StringCursorInternal`: codepoint index within one line.
    /// sg: r_font.cpp:205-261 @3b1a346
    fn string_cursor_internal(
        &self,
        idx: usize,
        line: &[u32],
        height: i32,
        scale: f32,
        cur_x: i32,
    ) -> usize {
        let fh = &self.heights[idx];
        let (tofu_idx, _) = self.find_smaller(height, idx, TOFU_SIZE_REDUCTION);
        let cur_x = cur_x as f32; // `curX <= x`: int converted to float
        let mut x = 0.0f32;
        // `lineEnd = std::find(I, str.end(), U'\n')`: callers pass one line, so this is the end.
        let line_end = line.iter().position(|&c| c == NL).unwrap_or(line.len());
        let mut i = 0;
        while i != line_end {
            let esc_len = is_color_escape_u32(&line[i..]);
            if esc_len > 0 {
                i += esc_len;
            } else if line[i] as usize >= fh.num_glyph {
                for cp in tofu_string(line[i]).chars() {
                    x += self.measure_codepoint(tofu_idx, cp as u32) as f32;
                    x = x.ceil();
                    if cur_x <= x {
                        return i;
                    }
                }
                i += 1;
            } else if line[i] == TAB {
                let glyph = fh.glyph(SPACE);
                let full_width = (glyph.width + glyph.sp_l + glyph.sp_r) as f32 * 4.0 * scale;
                let half_width = (full_width / 2.0).ceil();
                x += half_width;
                x = x.ceil();
                if cur_x <= x {
                    break;
                }
                x += full_width - half_width;
                x = x.ceil();
                if cur_x <= x {
                    break;
                }
                i += 1;
            } else {
                x += self.measure_codepoint(idx, line[i]) as f32 * scale;
                x = x.ceil();
                if cur_x <= x {
                    break;
                }
                i += 1;
            }
        }
        i
    }

    /// `r_font_c::StringCursorIndex`: CODEPOINT index for a point relative to the text origin.
    /// Lines are `height` tall; the first line whose bottom (`lineY`) is `>= curY` is used, else
    /// the last one. After a trailing `'\n'` the result is `len + lastIndex` (past the end).
    /// sg: r_font.cpp:263-286 @3b1a346
    pub fn string_cursor_index_u32(
        &self,
        height: i32,
        text: &[u32],
        cur_x: i32,
        cur_y: i32,
    ) -> usize {
        if self.heights.is_empty() {
            return 0; // DIVERGENCE(empty font set)
        }
        let idx = self.find(height);
        let fh = &self.heights[idx];
        let mut last_index = 0;
        let mut line_y = height;
        let scale = height as f32 / fh.height as f32;

        let mut i = 0;
        while i != text.len() {
            let line_end = text[i..]
                .iter()
                .position(|&c| c == NL)
                .map_or(text.len(), |p| i + p);
            let line = &text[i..line_end];
            last_index = self.string_cursor_internal(idx, line, height, scale, cur_x);
            if cur_y <= line_y {
                break;
            }
            if line_end == text.len() {
                break;
            }
            i = line_end + 1;
            line_y += height;
        }
        i + last_index
    }

    /// `r_font_c::Draw`. `col` is the renderer's draw colour and is updated by escapes, exactly as
    /// SimpleGraphic mutates `drawColor` through the pointer it passes (r_main.cpp:1807).
    /// sg: r_font.cpp:434-457 @3b1a346
    pub fn draw_u32(
        &self,
        out: &mut dyn QuadSink,
        pos: [f32; 2],
        align: Align,
        height: i32,
        col: &mut [f32; 4],
        text: &[u32],
        screen_w: i32,
        win_h: i32,
    ) {
        let mut pos = pos;
        if text.is_empty() {
            // `pos[Y] += height` on SG's local copy: no visible effect.
            return;
        }

        // Prepare for rendering
        out.color(*col);

        // Separate into lines and render them
        for line in lines(text) {
            if !line.is_empty() {
                self.draw_text_line(out, pos, align, height, col, line, screen_w, win_h);
            }
            pos[1] += height as f32;
        }
    }

    /// `r_font_c::DrawTextLine`. `screen_w` is `renderer->VirtualScreenWidth()`, `win_h` is
    /// `vid.size[1]` (ui doc §2 "Line visibility").
    /// sg: r_font.cpp:321-432 @3b1a346
    fn draw_text_line(
        &self,
        out: &mut dyn QuadSink,
        pos: [f32; 2],
        align: Align,
        height: i32,
        col: &mut [f32; 4],
        line: &[u32],
        screen_w: i32,
        win_h: i32,
    ) {
        // Check if the line is visible
        if pos[1] >= win_h as f32 || pos[1] <= -height as f32 || self.heights.is_empty() {
            // Just process the colour codes
            let mut s = line;
            while !s.is_empty() {
                // Check for escape character
                let esc_len = is_color_escape_u32(s);
                if esc_len > 0 {
                    s = read_color_escape_u32(s, col);
                    col[3] = 1.0;
                    out.color(*col);
                    continue;
                }
                s = &s[1..];
            }
            return;
        }

        // Find best height to use
        let main_idx = self.find(height);
        let fh = &self.heights[main_idx];
        let scale = height as f32 / fh.height as f32;
        let (tofu_idx, tofu_pad) = self.find_smaller(height, main_idx, TOFU_SIZE_REDUCTION);
        let vsw = screen_w as f32;

        // Calculate the string position
        let mut x = pos[0];
        let y = pos[1].floor();
        if align != Align::Left {
            // Calculate the real width of the string
            let width = self.string_width_internal(main_idx, line, height, scale) as f32;
            match align {
                Align::Center => x = ((vsw - width) / 2.0 + pos[0]).floor(),
                Align::Right => x = (vsw - width - pos[0]).floor(),
                Align::CenterX => x = (pos[0] - width / 2.0).floor(),
                Align::RightX => x = (pos[0] - width).floor(),
                Align::Left => {}
            }
        }

        // Snap the starting x position to the pixel grid so the leading glyph isn't blurred.
        x = x.round();

        let mut cur_tex: Option<TexId> = None;

        let mut draw_codepoint = |out: &mut dyn QuadSink,
                                  x: &mut f32,
                                  fh_idx: usize,
                                  height: i32,
                                  scale: f32,
                                  y_shift: i32,
                                  cp: u32| {
            let fh = &self.heights[fh_idx];
            let cp_y = y + y_shift as f32;
            if cur_tex != Some(fh.tex) {
                cur_tex = Some(fh.tex);
                out.bind(fh.tex);
            }
            let glyph = fh.glyph(cp);
            *x += glyph.sp_l as f32 * scale;
            if glyph.width != 0 {
                let w = glyph.width as f32 * scale;
                if *x + w >= 0.0 && *x < vsw {
                    let h = height as f32;
                    out.quad(Quad {
                        s: [glyph.tc_l, glyph.tc_r, glyph.tc_r, glyph.tc_l],
                        t: [glyph.tc_t, glyph.tc_t, glyph.tc_b, glyph.tc_b],
                        x: [*x, *x + w, *x + w, *x],
                        y: [cp_y, cp_y, cp_y + h, cp_y + h],
                    });
                }
                *x += w;
            }
            *x += glyph.sp_r as f32 * scale;
            *x = x.ceil();
        };

        // Render the string
        let mut tail = line;
        while !tail.is_empty() {
            // Draw unprintable characters as tofu placeholders
            let ch = tail[0];
            if ch as usize >= fh.num_glyph {
                let tofu_h = self.heights[tofu_idx].height;
                for t in tofu_string(ch).chars() {
                    draw_codepoint(out, &mut x, tofu_idx, tofu_h, 1.0, tofu_pad, t as u32);
                }
                tail = &tail[1..];
                continue;
            }

            // Check for escape character
            let esc_len = is_color_escape_u32(tail);
            if esc_len > 0 {
                tail = read_color_escape_u32(tail, col);
                col[3] = 1.0;
                out.color(*col);
                continue;
            }

            // Handle tabs
            if ch == TAB {
                let glyph = fh.glyph(SPACE);
                let sp_width = glyph.width + glyph.sp_l + glyph.sp_r;
                x += (sp_width << 2) as f32 * scale;
                tail = &tail[1..];
                continue;
            }

            // Draw glyph
            draw_codepoint(out, &mut x, main_idx, height, scale, 0, ch);
            tail = &tail[1..];
        }
    }

    // ------------------------------------------------------------------ r_renderer_c wrappers

    /// `r_renderer_c::DrawStringWidth` minus the font-index fallback (the caller picked the face):
    /// an empty C string is 0, else `StringWidth` of the decoded text. Physical px.
    /// sg: r_main.cpp:1833-1843 @3b1a346
    pub fn string_width(&self, h: i32, text: &str) -> i32 {
        self.string_width_bytes(h, text.as_bytes())
    }

    /// [`FontFace::string_width`] for a Lua byte string (may be invalid UTF-8 or contain NUL).
    /// sg: r_main.cpp:1833-1843 @3b1a346
    pub fn string_width_bytes(&self, h: i32, text: &[u8]) -> i32 {
        let text = c_str(text);
        if text.is_empty() {
            return 0;
        }
        let idx_str = index_utf8_to_utf32(text);
        self.string_width_u32(h, &idx_str.text)
    }

    /// `r_renderer_c::DrawStringCursorIndex`: the 0-based BYTE offset (ui_api adds the `+ 1`).
    /// A codepoint index past the end maps to the byte length.
    /// sg: r_main.cpp:1845-1860 @3b1a346
    pub fn cursor_index(&self, h: i32, text: &str, cx: i32, cy: i32) -> usize {
        self.cursor_index_bytes(h, text.as_bytes(), cx, cy)
    }

    /// [`FontFace::cursor_index`] for a Lua byte string.
    /// sg: r_main.cpp:1845-1860 @3b1a346
    pub fn cursor_index_bytes(&self, h: i32, text: &[u8], cx: i32, cy: i32) -> usize {
        let narrow_view = c_str(text);
        if narrow_view.is_empty() {
            return 0;
        }
        let idx_str = index_utf8_to_utf32(narrow_view);
        let index = self.string_cursor_index_u32(h, &idx_str.text, cx, cy);
        if index < idx_str.source_code_unit_offsets.len() {
            return idx_str.source_code_unit_offsets[index];
        }
        narrow_view.len()
    }

    /// `r_renderer_c::DrawString` with `col == NULL` (PoB's only form): decode, then
    /// [`FontFace::draw_u32`] with the renderer's draw colour.
    /// sg: r_main.cpp:1793-1809 @3b1a346
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
        self.draw_bytes(out, pos, align, h, col, text.as_bytes(), screen_w, win_h);
    }

    /// [`FontFace::draw`] for a Lua byte string.
    /// sg: r_main.cpp:1793-1809 @3b1a346
    pub fn draw_bytes(
        &self,
        out: &mut dyn QuadSink,
        pos: [f32; 2],
        align: Align,
        h: i32,
        col: &mut [f32; 4],
        text: &[u8],
        screen_w: i32,
        win_h: i32,
    ) {
        let idx_str = index_utf8_to_utf32(c_str(text));
        self.draw_u32(out, pos, align, h, col, &idx_str.text, screen_w, win_h);
    }
}
