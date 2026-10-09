//! TGF/TGA bitmap fonts: parsing, metrics and glyph layout.
//!
//! Everything here is a port of SimpleGraphic `engine/render/r_font.cpp`
//! (master @ 8694a07; the file is unchanged since 2025-10-23 "Use Static
//! Casts", so it matches the DLL shipped with PoB 2.67.2). All coordinates are
//! *physical* pixels (SimpleGraphic's "virtual screen" when `DPI_AWARE`); the
//! Lua-facing DPI conversion lives in [`crate::Sg`].
//!
//! Text is processed as Unicode scalar values, like SimpleGraphic, which
//! converts UTF-8 to UTF-32 (`common.cpp` `IndexUTF8ToUTF32`) before layout.
//! Codepoints without a glyph (>= 128) are drawn as "tofu": the text
//! `[U+XXXX]` in a smaller font height (`BuildTofuString`,
//! `FindSmallerFontHeight`).

use std::path::{Path, PathBuf};

use crate::color::{is_color_escape, read_color_escape, Rgba};
use crate::tga;

/// PoB font names. Order and spelling follow `ui_api.cpp` `fontMap`
/// (`"FIXED", "VAR", "VAR BOLD", "FONTIN SC", "FONTIN SC ITALIC", "FONTIN",
/// "FONTIN ITALIC"`), which is also the `F_*` enum order.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
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

    /// The Lua name (`luaL_checkoption` against `fontMap`; exact, case-sensitive).
    pub fn from_name(name: &str) -> Option<Font> {
        Some(match name {
            "FIXED" => Font::Fixed,
            "VAR" => Font::Var,
            "VAR BOLD" => Font::VarBold,
            "FONTIN SC" => Font::FontinSc,
            "FONTIN SC ITALIC" => Font::FontinScItalic,
            "FONTIN" => Font::Fontin,
            "FONTIN ITALIC" => Font::FontinItalic,
            _ => return None,
        })
    }

    pub fn name(self) -> &'static str {
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

    /// File base name under `Fonts/` (`r_main.cpp` `r_renderer_c::Init`).
    pub fn file_name(self) -> &'static str {
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

    pub fn index(self) -> usize {
        self as usize
    }
}

/// Text alignment (`ui_api.cpp` `alignMap`: LEFT, CENTER, RIGHT, CENTER_X,
/// RIGHT_X; `F_LEFT`.. in `r_font.h`).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum Align {
    #[default]
    Left,
    Center,
    Right,
    CenterX,
    RightX,
}

impl Align {
    /// `luaL_checkoption(L, 3, "LEFT", alignMap)`: nil means LEFT.
    pub fn from_name(name: Option<&str>) -> Option<Align> {
        Some(match name {
            None | Some("LEFT") => Align::Left,
            Some("CENTER") => Align::Center,
            Some("RIGHT") => Align::Right,
            Some("CENTER_X") => Align::CenterX,
            Some("RIGHT_X") => Align::RightX,
            Some(_) => return None,
        })
    }
}

/// One glyph (`r_font.cpp` `f_glyph_s`).
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Glyph {
    pub tc_left: f32,
    pub tc_right: f32,
    pub tc_top: f32,
    pub tc_bottom: f32,
    pub width: i32,
    pub sp_left: i32,
    pub sp_right: i32,
    /// Raw atlas cell position from the `.tgf` (for tests and tools).
    pub x: u32,
    pub y: u32,
}

impl Glyph {
    /// `measureCodepoint`: `width + spLeft + spRight`.
    pub fn advance(&self) -> i32 {
        self.width + self.sp_left + self.sp_right
    }
}

/// One atlas height of a face (`r_font.cpp` `f_fontHeight_s`).
#[derive(Clone, Debug)]
pub struct FontHeight {
    pub height: i32,
    pub atlas_path: PathBuf,
    pub atlas_width: u32,
    pub atlas_height: u32,
    pub glyphs: Vec<Glyph>,
}

const DEF_GLYPH: Glyph = Glyph {
    tc_left: 0.0,
    tc_right: 0.0,
    tc_top: 0.0,
    tc_bottom: 0.0,
    width: 0,
    sp_left: 0,
    sp_right: 0,
    x: 0,
    y: 0,
};

impl FontHeight {
    /// `f_fontHeight_s::Glyph`: codes `>= numGlyph` give the all-zero `defGlyph`.
    /// The codepoint is first narrowed to a byte (`(char)(unsigned char)cp`).
    pub fn glyph(&self, cp: u32) -> &Glyph {
        let b = (cp & 0xff) as usize;
        self.glyphs.get(b).unwrap_or(&DEF_GLYPH)
    }

    pub fn num_glyph(&self) -> u32 {
        self.glyphs.len() as u32
    }
}

/// A loaded face: all its heights plus the height lookup table.
#[derive(Clone, Debug)]
pub struct FontFace {
    pub font: Font,
    pub heights: Vec<FontHeight>,
    pub max_height: i32,
    /// `fontHeightMap`, indexed by requested height `0..=max_height`.
    pub height_map: Vec<usize>,
}

/// `r_font_c::EmbeddedFontSpec`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TofuFont {
    pub height_idx: usize,
    pub y_pad: i32,
}

/// A glyph quad produced by layout, in physical pixels relative to the
/// current viewport, with the colour current when it was emitted.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct GlyphQuad {
    pub height_idx: usize,
    /// TL, TR, BR, BL positions as x,y pairs.
    pub pos: [f32; 8],
    /// Matching texture coordinates.
    pub uv: [f32; 8],
    pub color: Rgba,
}

fn parse_tgf_line(line: &str) -> Option<(&str, Vec<i64>)> {
    // Equivalent of the sscanf formats "HEIGHT %u;" and "GLYPH %u %u %u %d %d;":
    // keyword, whitespace separated integers, terminated by ';'.
    let body = line.split(';').next()?;
    let mut it = body.split_whitespace();
    let kw = it.next()?;
    let nums: Option<Vec<i64>> = it.map(|t| t.parse::<i64>().ok()).collect();
    Some((kw, nums?))
}

impl FontFace {
    /// Load `<dir>/<file_name>.tgf` and the TGA headers of every height.
    /// Port of the `r_font_c` constructor.
    pub fn load(dir: &Path, font: Font) -> Result<FontFace, String> {
        let base = dir.join(font.file_name());
        let tgf_path = base.with_file_name(format!("{}.tgf", font.file_name()));
        let text = std::fs::read_to_string(&tgf_path)
            .map_err(|e| format!("font \"{}\" not found ({}): {e}", font.file_name(), tgf_path.display()))?;
        let mut heights: Vec<FontHeight> = Vec::new();
        let mut max_height = 0;
        for line in text.lines() {
            let Some((kw, nums)) = parse_tgf_line(line) else { continue };
            if kw == "HEIGHT" && nums.len() == 1 {
                let h = nums[0] as i32;
                let atlas_path = dir.join(format!("{}.{}.tga", font.file_name(), h));
                let bytes = read_prefix(&atlas_path, 18)?;
                let hdr = tga::read_header(&bytes)?;
                heights.push(FontHeight {
                    height: h,
                    atlas_path,
                    atlas_width: u32::from(hdr.width),
                    atlas_height: u32::from(hdr.height),
                    glyphs: Vec::with_capacity(128),
                });
                max_height = max_height.max(h);
            } else if kw == "GLYPH" && nums.len() == 5 {
                let Some(fh) = heights.last_mut() else { continue };
                if fh.glyphs.len() >= 128 {
                    continue;
                }
                let (x, y, w) = (nums[0] as u32, nums[1] as u32, nums[2] as u32);
                let fw = fh.atlas_width as f32;
                let fhh = fh.atlas_height as f32;
                fh.glyphs.push(Glyph {
                    tc_left: x as f32 / fw,
                    tc_right: (x + w) as f32 / fw,
                    tc_top: y as f32 / fhh,
                    tc_bottom: (y + fh.height as u32) as f32 / fhh,
                    width: w as i32,
                    sp_left: nums[3] as i32,
                    sp_right: nums[4] as i32,
                    x,
                    y,
                });
            }
        }
        if heights.is_empty() {
            return Err(format!("font \"{}\" has no heights", font.file_name()));
        }
        // Generate mapping of text height to font height (constructor tail).
        let mut height_map = vec![0usize; max_height as usize + 1];
        for i in 0..heights.len() {
            let gh = heights[i].height;
            for h in gh..=max_height {
                height_map[h as usize] = i;
            }
            if i > 0 {
                let below = heights[i - 1].height;
                let lim = (gh - below - 1) / 2;
                for b in 0..lim {
                    height_map[(gh - b - 1) as usize] = i;
                }
            }
        }
        Ok(FontFace { font, heights, max_height, height_map })
    }

    /// `r_font_c::FindFontHeight`.
    pub fn find_font_height(&self, height: i32) -> usize {
        if height > self.max_height {
            self.heights.len() - 1
        } else if height < 0 {
            0
        } else {
            self.height_map[height as usize]
        }
    }

    /// `r_font_c::FindSmallerFontHeight`: the first lower atlas height that is
    /// at least `size_reduction` smaller than `height`, padded to centre it.
    pub fn find_smaller_font_height(&self, height: i32, height_idx: usize, size_reduction: i32) -> TofuFont {
        let mut ret = TofuFont { height_idx, y_pad: 0 };
        for idx in (0..height_idx).rev() {
            let diff = height - self.heights[idx].height;
            if diff >= size_reduction {
                ret.height_idx = idx;
                ret.y_pad = (diff as f32 / 2.0).ceil() as i32;
                break;
            }
        }
        ret
    }

    fn measure(&self, idx: usize, cp: u32) -> i32 {
        self.heights[idx].glyph(cp).advance()
    }

    /// `r_font_c::StringWidthInternal` for one line (no `'\n'`).
    pub fn line_width(&self, idx: usize, line: &[u32], height: i32, scale: f32) -> i32 {
        let fh = &self.heights[idx];
        let tofu = self.find_smaller_font_height(height, idx, TOFU_SIZE_REDUCTION);
        let mut width = 0.0f32;
        let mut i = 0;
        while i < line.len() {
            let ch = line[i];
            let esc = is_color_escape_u32(&line[i..]);
            if esc > 0 {
                i += esc;
            } else if ch >= fh.num_glyph() {
                for cp in tofu_string(ch).chars() {
                    width += self.measure(tofu.height_idx, cp as u32) as f32;
                    width = width.ceil();
                }
                i += 1;
            } else if ch == u32::from('\t') {
                let sp = fh.glyph(u32::from(' ')).advance();
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
    pub fn string_width(&self, height: i32, text: &[u32]) -> i32 {
        let idx = self.find_font_height(height);
        let scale = height as f32 / self.heights[idx].height as f32;
        text.split(|&c| c == u32::from('\n'))
            .filter(|l| !l.is_empty())
            .map(|l| self.line_width(idx, l, height, scale))
            .max()
            .unwrap_or(0)
    }

    /// `r_font_c::StringCursorInternal`: codepoint index within one line.
    pub fn line_cursor(&self, idx: usize, line: &[u32], height: i32, scale: f32, cur_x: i32) -> usize {
        let fh = &self.heights[idx];
        let tofu = self.find_smaller_font_height(height, idx, TOFU_SIZE_REDUCTION);
        let cur_x = cur_x as f32;
        let mut x = 0.0f32;
        let mut i = 0;
        while i < line.len() {
            let ch = line[i];
            let esc = is_color_escape_u32(&line[i..]);
            if esc > 0 {
                i += esc;
            } else if ch >= fh.num_glyph() {
                for cp in tofu_string(ch).chars() {
                    x += self.measure(tofu.height_idx, cp as u32) as f32;
                    x = x.ceil();
                    if cur_x <= x {
                        return i;
                    }
                }
                i += 1;
            } else if ch == u32::from('\t') {
                let full = fh.glyph(u32::from(' ')).advance() as f32 * 4.0 * scale;
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

    /// `r_font_c::StringCursorIndex`: codepoint index for a point relative to
    /// the text origin. Lines are `height` tall; the first line whose bottom
    /// (`lineY`) is `>= cur_y` is used, else the last line.
    pub fn string_cursor_index(&self, height: i32, text: &[u32], cur_x: i32, cur_y: i32) -> usize {
        let idx = self.find_font_height(height);
        let scale = height as f32 / self.heights[idx].height as f32;
        let mut last_index = 0;
        let mut line_y = height;
        let mut start = 0;
        while start != text.len() {
            let line_end = text[start..].iter().position(|&c| c == u32::from('\n')).map_or(text.len(), |p| start + p);
            last_index = self.line_cursor(idx, &text[start..line_end], height, scale, cur_x);
            if cur_y <= line_y {
                break;
            }
            if line_end == text.len() {
                break;
            }
            start = line_end + 1;
            line_y += height;
        }
        start + last_index
    }

    /// `r_font_c::Draw`: lay out (possibly multi-line) text. `col` is the
    /// renderer's current draw colour and is updated by escapes, exactly as
    /// SimpleGraphic mutates `drawColor` through the pointer it passes.
    pub fn draw(&self, pos: (f32, f32), align: Align, height: i32, col: &mut Rgba, text: &[u32], screen: (i32, i32), out: &mut Vec<GlyphQuad>) {
        if text.is_empty() {
            return;
        }
        let mut y = pos.1;
        for line in text.split(|&c| c == u32::from('\n')) {
            if !line.is_empty() {
                self.draw_text_line((pos.0, y), align, height, col, line, screen, out);
            }
            y += height as f32;
        }
    }

    /// `r_font_c::DrawTextLine`.
    pub fn draw_text_line(&self, pos: (f32, f32), align: Align, height: i32, col: &mut Rgba, line: &[u32], screen: (i32, i32), out: &mut Vec<GlyphQuad>) {
        let (screen_w, screen_h) = (screen.0 as f32, screen.1 as f32);
        // Check if the line is visible; if not, only process the colour codes.
        if pos.1 >= screen_h || pos.1 <= -(height as f32) {
            let mut i = 0;
            while i < line.len() {
                let esc = is_color_escape_u32(&line[i..]);
                if esc > 0 {
                    read_color_escape_u32(&line[i..], esc, col);
                    col[3] = 1.0;
                    i += esc;
                } else {
                    i += 1;
                }
            }
            return;
        }

        let idx = self.find_font_height(height);
        let fh = &self.heights[idx];
        let scale = height as f32 / fh.height as f32;
        let tofu = self.find_smaller_font_height(height, idx, TOFU_SIZE_REDUCTION);

        let mut x = pos.0;
        let y = pos.1.floor();
        if align != Align::Left {
            let width = self.line_width(idx, line, height, scale) as f32;
            x = match align {
                Align::Center => ((screen_w - width) / 2.0 + pos.0).floor(),
                Align::Right => (screen_w - width - pos.0).floor(),
                Align::CenterX => (pos.0 - width / 2.0).floor(),
                Align::RightX => (pos.0 - width).floor(),
                Align::Left => x,
            };
        }
        // Snap the starting x position to the pixel grid (std::round).
        x = x.round();

        let mut draw_cp = |x: &mut f32, hidx: usize, qh: f32, scale: f32, y_shift: i32, cp: u32, col: &Rgba| {
            let fh = &self.heights[hidx];
            let cp_y = y + y_shift as f32;
            let g = fh.glyph(cp);
            *x += g.sp_left as f32 * scale;
            if g.width != 0 {
                let w = g.width as f32 * scale;
                if *x + w >= 0.0 && *x < screen_w {
                    out.push(GlyphQuad {
                        height_idx: hidx,
                        pos: [*x, cp_y, *x + w, cp_y, *x + w, cp_y + qh, *x, cp_y + qh],
                        uv: [g.tc_left, g.tc_top, g.tc_right, g.tc_top, g.tc_right, g.tc_bottom, g.tc_left, g.tc_bottom],
                        color: *col,
                    });
                }
                *x += w;
            }
            *x += g.sp_right as f32 * scale;
            *x = x.ceil();
        };

        let mut i = 0;
        while i < line.len() {
            let ch = line[i];
            // Draw unprintable characters as tofu placeholders.
            if ch >= fh.num_glyph() {
                let tfh = self.heights[tofu.height_idx].height as f32;
                for t in tofu_string(ch).chars() {
                    draw_cp(&mut x, tofu.height_idx, tfh, 1.0, tofu.y_pad, t as u32, col);
                }
                i += 1;
                continue;
            }
            // Check for escape character.
            let esc = is_color_escape_u32(&line[i..]);
            if esc > 0 {
                read_color_escape_u32(&line[i..], esc, col);
                col[3] = 1.0;
                i += esc;
                continue;
            }
            // Handle tabs (note: no ceil here, unlike normal glyphs).
            if ch == u32::from('\t') {
                let sp = fh.glyph(u32::from(' ')).advance();
                x += (sp << 2) as f32 * scale;
                i += 1;
                continue;
            }
            draw_cp(&mut x, idx, height as f32, scale, 0, ch, col);
            i += 1;
        }
    }
}

/// `tofuSizeReduction` in r_font.cpp.
pub const TOFU_SIZE_REDUCTION: i32 = 3;

/// `BuildTofuString`: `[U+{:04X}]`.
pub fn tofu_string(cp: u32) -> String {
    format!("[U+{cp:04X}]")
}

fn is_color_escape_u32(s: &[u32]) -> usize {
    // IsColorEscape(std::u32string_view): same rules as the byte version but
    // on codepoints; only ASCII digits / hex digits qualify.
    if s.len() < 2 || s[0] != u32::from('^') {
        return 0;
    }
    let mut buf = [0u8; 8];
    for (i, &c) in s.iter().take(8).enumerate() {
        buf[i] = if c < 128 { c as u8 } else { 0 };
    }
    is_color_escape(&buf[..s.len().min(8)])
}

fn read_color_escape_u32(s: &[u32], len: usize, out: &mut Rgba) {
    let mut buf = [0u8; 8];
    for (i, &c) in s.iter().take(len).enumerate() {
        buf[i] = c as u8;
    }
    read_color_escape(&buf[..len], len, out);
}

fn read_prefix(path: &Path, n: usize) -> Result<Vec<u8>, String> {
    use std::io::Read;
    let mut f = std::fs::File::open(path).map_err(|e| format!("{}: {e}", path.display()))?;
    let mut buf = vec![0u8; n];
    f.read_exact(&mut buf).map_err(|e| format!("{}: {e}", path.display()))?;
    Ok(buf)
}

/// Convert UTF-8 text to codepoints plus the byte offset of each codepoint
/// (`common.cpp` `IndexUTF8ToUTF32`; Rust strings are valid UTF-8, so the
/// replacement-character path never triggers).
pub fn index_utf8(text: &str) -> (Vec<u32>, Vec<usize>) {
    let mut cps = Vec::with_capacity(text.len());
    let mut offs = Vec::with_capacity(text.len());
    for (o, c) in text.char_indices() {
        cps.push(c as u32);
        offs.push(o);
    }
    (cps, offs)
}

/// All seven faces.
#[derive(Debug)]
pub struct FontSet {
    pub dir: PathBuf,
    pub faces: Vec<FontFace>,
}

impl FontSet {
    pub fn load(dir: impl AsRef<Path>) -> Result<FontSet, String> {
        let dir = dir.as_ref().to_path_buf();
        let faces = Font::ALL.iter().map(|&f| FontFace::load(&dir, f)).collect::<Result<Vec<_>, _>>()?;
        Ok(FontSet { dir, faces })
    }

    pub fn face(&self, font: Font) -> &FontFace {
        &self.faces[font.index()]
    }

    /// Decode the atlas for `font` at height index `idx` (top-down RGBA8).
    pub fn atlas_rgba(&self, font: Font, idx: usize) -> Result<tga::Rgba8Image, String> {
        let fh = &self.face(font).heights[idx];
        let data = std::fs::read(&fh.atlas_path).map_err(|e| format!("{}: {e}", fh.atlas_path.display()))?;
        tga::decode(&data)
    }

    /// Find PoB's `runtime/SimpleGraphic/Fonts` directory: `$POB_SG_FONTS`,
    /// then `.PathOfBuilding/runtime/SimpleGraphic/Fonts` or
    /// `runtime/SimpleGraphic/Fonts` in the current directory, this crate's
    /// directory, or any of their ancestors.
    pub fn locate() -> Option<PathBuf> {
        if let Some(p) = std::env::var_os("POB_SG_FONTS") {
            let p = PathBuf::from(p);
            if p.is_dir() {
                return Some(p);
            }
        }
        let mut starts = Vec::new();
        if let Ok(cwd) = std::env::current_dir() {
            starts.push(cwd);
        }
        starts.push(PathBuf::from(env!("CARGO_MANIFEST_DIR")));
        for start in starts {
            for dir in start.ancestors() {
                for rel in [".PathOfBuilding/runtime/SimpleGraphic/Fonts", "runtime/SimpleGraphic/Fonts"] {
                    let p = dir.join(rel);
                    if p.join("Liberation Sans.tgf").is_file() {
                        return Some(p);
                    }
                }
            }
        }
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    pub(crate) fn fonts() -> Option<FontSet> {
        let Some(dir) = FontSet::locate() else {
            eprintln!("SKIP: PoB fonts not found (set POB_SG_FONTS)");
            return None;
        };
        Some(FontSet::load(dir).expect("load fonts"))
    }

    fn cps(s: &str) -> Vec<u32> {
        index_utf8(s).0
    }

    #[test]
    fn tgf_parse_counts() {
        let Some(fs) = fonts() else { return };
        let expect = [10, 12, 14, 16, 18, 20, 22, 24, 26, 28, 32, 36, 40, 48, 56, 64];
        for face in &fs.faces {
            assert_eq!(face.heights.len(), 16, "{:?}", face.font);
            let hs: Vec<i32> = face.heights.iter().map(|h| h.height).collect();
            assert_eq!(hs, expect, "{:?}", face.font);
            for h in &face.heights {
                assert_eq!(h.glyphs.len(), 128, "{:?} {}", face.font, h.height);
                assert!(h.atlas_width > 0 && h.atlas_height > 0);
            }
            assert_eq!(face.max_height, 64);
        }
        // Spot-check raw values: Liberation Sans 16 'A' = GLYPH 0 120 9 0 0 in a 128x256 atlas.
        let var = fs.face(Font::Var);
        let h16 = &var.heights[3];
        assert_eq!((h16.atlas_width, h16.atlas_height), (128, 256));
        let a = h16.glyph(u32::from('A'));
        assert_eq!((a.x, a.y, a.width, a.sp_left, a.sp_right), (0, 120, 9, 0, 0));
        assert_eq!(a.tc_right, 9.0 / 128.0);
        assert_eq!(a.tc_top, 120.0 / 256.0);
        assert_eq!(a.tc_bottom, 136.0 / 256.0);
    }

    #[test]
    fn height_map() {
        let Some(fs) = fonts() else { return };
        let f = fs.face(Font::Var);
        let h = |req: i32| f.heights[f.find_font_height(req)].height;
        assert_eq!(h(1), 10); // below the smallest maps to index 0
        assert_eq!(h(10), 10);
        assert_eq!(h(11), 10);
        assert_eq!(h(16), 16);
        assert_eq!(h(34), 32);
        assert_eq!(h(35), 36); // lim = (36-32-1)/2 = 1 rounds 35 up
        assert_eq!(h(44), 40);
        assert_eq!(h(45), 48); // lim = (48-40-1)/2 = 3 -> 45..47 round up
        assert_eq!(h(52), 48);
        assert_eq!(h(53), 56); // lim = (56-48-1)/2 = 3 -> 53..55 round up
        assert_eq!(h(100), 64);
        assert_eq!(h(-5), 10);
    }

    #[test]
    fn known_widths() {
        let Some(fs) = fonts() else { return };
        let var = fs.face(Font::Var);
        // Liberation Sans 16: H(7+1+1) e(6+1+1) l(1+1+1) l o(6+1+1) = 31.
        assert_eq!(var.string_width(16, &cps("Hello")), 31);
        assert_eq!(var.string_width(16, &cps("^7Hel^xFF0000lo")), 31);
        assert_eq!(var.string_width(16, &cps("Hello\nA")), 31);
        assert_eq!(var.string_width(16, &cps("A\nHello\n")), 31);
        assert_eq!(var.string_width(16, &cps("")), 0);
        // Tab = 4 spaces: space at 16 is 1+0+3 = 4 -> 16.
        assert_eq!(var.string_width(16, &cps("\t")), 16);
        // Bitstream Vera Sans Mono 12: 'A' = 5+1+0 = 6.
        let fixed = fs.face(Font::Fixed);
        assert_eq!(fixed.string_width(12, &cps("AAAA")), 24);
        // Scaled: height 18 uses the 18 atlas; height 34 uses the 32 atlas
        // with scale 34/32 and per-glyph ceil.
        let a32 = var.heights[var.find_font_height(32)].glyph(u32::from('A')).advance();
        let scale = 34.0f32 / 32.0;
        let mut w = 0.0f32;
        for _ in 0..3 {
            w = (w + a32 as f32 * scale).ceil();
        }
        assert_eq!(var.string_width(34, &cps("AAA")), w as i32);
    }

    #[test]
    fn tofu_width() {
        let Some(fs) = fonts() else { return };
        let var = fs.face(Font::Var);
        // 'é' (U+00E9) is measured as "[U+00E9]" in the 12 atlas for height 16
        // (first lower height with 16 - h >= 3 is 12).
        let t = var.find_smaller_font_height(16, var.find_font_height(16), 3);
        assert_eq!(var.heights[t.height_idx].height, 12);
        assert_eq!(t.y_pad, 2);
        let expect: i32 = "[U+00E9]".chars().map(|c| var.heights[t.height_idx].glyph(c as u32).advance()).sum();
        assert_eq!(var.string_width(16, &cps("é")), expect);
    }

    #[test]
    fn cursor_index() {
        let Some(fs) = fonts() else { return };
        let var = fs.face(Font::Var);
        let t = cps("Hello");
        // Cumulative right edges: H 9, e 17, l 20, l 23, o 31.
        assert_eq!(var.string_cursor_index(16, &t, 0, 0), 0);
        assert_eq!(var.string_cursor_index(16, &t, 9, 0), 0);
        assert_eq!(var.string_cursor_index(16, &t, 10, 0), 1);
        assert_eq!(var.string_cursor_index(16, &t, 31, 0), 4);
        assert_eq!(var.string_cursor_index(16, &t, 32, 0), 5);
        // Multi-line: second line starts at codepoint 6; y within line 2.
        let t2 = cps("Hello\nHe");
        assert_eq!(var.string_cursor_index(16, &t2, 100, 17), 8);
        assert_eq!(var.string_cursor_index(16, &t2, 100, 16), 5);
        assert_eq!(var.string_cursor_index(16, &t2, 100, 500), 8);
    }

    #[test]
    fn layout_alignment_and_escapes() {
        let Some(fs) = fonts() else { return };
        let var = fs.face(Font::Var);
        let t = cps("^1H^xFFFFFFe");
        let screen = (1000, 800);
        let mut col = [0.5, 0.5, 0.5, 0.25];
        let mut out = Vec::new();
        var.draw((10.4, 20.7), Align::Left, 16, &mut col, &t, screen, &mut out);
        assert_eq!(out.len(), 2);
        // x = round(10.4) = 10; y = floor(20.7) = 20; H spLeft 1 -> 11.
        assert_eq!(out[0].pos[0], 11.0);
        assert_eq!(out[0].pos[1], 20.0);
        assert_eq!(out[0].pos[5], 36.0);
        // ^1 forces alpha to 1.
        assert_eq!(out[0].color, [1.0, 0.0, 0.0, 1.0]);
        assert_eq!(out[1].color, [1.0, 1.0, 1.0, 1.0]);
        // Draw colour is left at the last escape.
        assert_eq!(col, [1.0, 1.0, 1.0, 1.0]);

        // Alignment math: width("Hello") = 31 at 16.
        let t = cps("Hello");
        let first_x = |align, x: f32| {
            let mut col = [1.0; 4];
            let mut out = Vec::new();
            var.draw((x, 0.0), align, 16, &mut col, &t, screen, &mut out);
            out[0].pos[0] - 1.0 // minus H's spLeft
        };
        assert_eq!(first_x(Align::Left, 100.0), 100.0);
        assert_eq!(first_x(Align::CenterX, 100.0), (100.0f32 - 15.5).floor());
        assert_eq!(first_x(Align::RightX, 100.0), 69.0);
        assert_eq!(first_x(Align::Center, 10.0), ((1000.0f32 - 31.0) / 2.0 + 10.0).floor());
        assert_eq!(first_x(Align::Right, 10.0), 1000.0 - 31.0 - 10.0);
    }

    #[test]
    fn offscreen_line_still_applies_escapes() {
        let Some(fs) = fonts() else { return };
        let var = fs.face(Font::Var);
        let mut col = [1.0, 1.0, 1.0, 0.5];
        let mut out = Vec::new();
        var.draw((0.0, -100.0), Align::Left, 16, &mut col, &cps("^2abc"), (100, 100), &mut out);
        assert!(out.is_empty());
        assert_eq!(col, [0.0, 1.0, 0.0, 1.0]);
    }

    #[test]
    fn atlases_white_rgb() {
        let Some(fs) = fonts() else { return };
        // Decode one atlas of each face; RGB is white wherever alpha > 0.
        for f in Font::ALL {
            let img = fs.atlas_rgba(f, 3).unwrap();
            let fh = &fs.face(f).heights[3];
            assert_eq!((img.width, img.height), (fh.atlas_width, fh.atlas_height));
            assert!(img.pixels.chunks_exact(4).filter(|p| p[3] > 0).all(|p| p[0] == 255 && p[1] == 255 && p[2] == 255));
            assert!(img.pixels.chunks_exact(4).any(|p| p[3] >= 200));
        }
    }

    #[test]
    fn glyph_cell_has_ink_top_down() {
        let Some(fs) = fonts() else { return };
        // 'A' cell in Liberation Sans 16 (bottom-up file): the cell rows must
        // contain coverage, and the row above the cell's top-left region of
        // a capital letter should be near the top of the cell after flipping.
        let img = fs.atlas_rgba(Font::Var, 3).unwrap();
        let g = *fs.face(Font::Var).heights[3].glyph(u32::from('A'));
        let ink = |y: u32| (g.x..g.x + g.width as u32).map(|x| u32::from(img.pixels[((y * img.width + x) * 4 + 3) as usize])).sum::<u32>();
        let total: u32 = (g.y..g.y + 16).map(ink).sum();
        assert!(total > 0);
        // The apex of 'A' is narrower than its legs: ink in the upper third
        // of the cell must be less than in the lower part where the legs and
        // crossbar are.
        let upper: u32 = (g.y..g.y + 7).map(ink).sum();
        let lower: u32 = (g.y + 7..g.y + 14).map(ink).sum();
        assert!(upper < lower, "upper {upper} lower {lower}");
    }
}
