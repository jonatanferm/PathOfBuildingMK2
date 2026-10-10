//! TGF/TGA bitmap fonts (ui doc §4.4, port of SimpleGraphic `engine/render/r_font.cpp`). Owner: U2.
//!
//! Verified by U2 statement by statement against PathOfBuilding-SimpleGraphic @ 3b1a346 (the
//! commit the shipped `SimpleGraphic.dll` names, `SimpleGraphic v2.5-3b1a346`); `r_font.cpp`
//! and `r_font.h` there are byte-identical to the research copy (master @ 8694a07) the pre-port
//! code was written from. All coordinates are *physical* pixels; the Lua-facing DPI conversion
//! lives in [`crate::api::Sg`].
//!
//! Moved from the pre-port `pob-sg/src/font.rs` by UI-U0 and split into `tgf.rs` (parser),
//! `tga.rs` (atlas decoder), `layout.rs` (width / cursor / draw) and this file (faces, height
//! map, atlas registration, `IndexUTF8ToUTF32`). Atlases are registered with the
//! [`ImageRegistry`] at load time.
//!
//! Text is processed as UTF-32 codepoints, like SimpleGraphic (`IndexUTF8ToUTF32`). Codepoints
//! without a glyph (>= numGlyph) are drawn as "tofu" (`[U+XXXX]` in a smaller height).
//!
//! DIVERGENCE(missing atlas): SG creates each `r_tex_c` when it parses `HEIGHT` and a missing or
//! broken `.tga` silently becomes the default texture with `fileWidth == 0` (texcoords inf/NaN);
//! [`FontSet::load`] returns an error instead (the assets ship with PoB).
//! DIVERGENCE(empty font set): [`FontSet::empty`] (no atlases) is not an SG state; see layout.rs.

pub mod layout;
pub mod tga;
pub mod tgf;

use crate::assets::AssetSource;
use crate::image::{ImageData, ImageFlags, ImageRegistry, TexId};
use crate::types::Font;

/// One glyph (`f_glyph_s`).
/// sg: r_font.cpp:19-28 @3b1a346
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Glyph {
    pub tc_l: f32,
    pub tc_r: f32,
    pub tc_t: f32,
    pub tc_b: f32,
    pub width: i32,
    pub sp_l: i32,
    pub sp_r: i32,
}

impl Glyph {
    /// `measureCodepoint`: `width + spLeft + spRight`.
    pub fn advance(&self) -> i32 {
        self.width + self.sp_l + self.sp_r
    }
}

const DEF_GLYPH: Glyph = Glyph {
    tc_l: 0.0,
    tc_r: 0.0,
    tc_t: 0.0,
    tc_b: 0.0,
    width: 0,
    sp_l: 0,
    sp_r: 0,
};

/// One atlas height of a face (`f_fontHeight_s`).
/// sg: r_font.cpp:30-44 @3b1a346
pub struct FontHeight {
    pub tex: TexId,
    pub height: i32,
    pub num_glyph: usize,
    pub glyphs: [Glyph; 128],
}

impl FontHeight {
    /// `f_fontHeight_s::Glyph`: the codepoint is narrowed to a byte (`(char)(unsigned char)cp`);
    /// codes `>= numGlyph` give the all-zero `defGlyph`.
    /// sg: r_font.cpp:38-43 @3b1a346
    pub fn glyph(&self, cp: u32) -> &Glyph {
        let b = (cp & 0xff) as usize;
        if b < self.num_glyph {
            &self.glyphs[b]
        } else {
            &DEF_GLYPH
        }
    }
}

/// A loaded face: all its heights plus the height lookup table.
pub struct FontFace {
    heights: Vec<FontHeight>,
    height_map: Vec<usize>,
    max_height: i32,
}

/// All seven faces.
pub struct FontSet {
    faces: [FontFace; 7],
}

/// `tofuSizeReduction`.
/// sg: r_font.cpp:141 @3b1a346
pub const TOFU_SIZE_REDUCTION: i32 = 3;

/// `BuildTofuString`: `[U+{:04X}]` (higher planes get more digits).
/// sg: r_font.cpp:131-139 @3b1a346
pub fn tofu_string(cp: u32) -> String {
    format!("[U+{cp:04X}]")
}

impl FontSet {
    /// Parses the 7 .tgf files and registers the 112 atlases synchronously (UVs need the atlas size).
    pub fn load(src: &dyn AssetSource, images: &mut ImageRegistry) -> std::io::Result<FontSet> {
        let [a, b, c, d, e, f, g] = Font::ALL;
        Ok(FontSet {
            faces: [
                FontFace::load(src, images, a)?,
                FontFace::load(src, images, b)?,
                FontFace::load(src, images, c)?,
                FontFace::load(src, images, d)?,
                FontFace::load(src, images, e)?,
                FontFace::load(src, images, f)?,
                FontFace::load(src, images, g)?,
            ],
        })
    }

    /// No faces loaded (tests and tools without PoB's runtime assets). Not an SG state: every
    /// face has no atlases, so measuring returns 0 and drawing only applies colour escapes.
    pub fn empty() -> FontSet {
        let e = || FontFace {
            heights: Vec::new(),
            height_map: Vec::new(),
            max_height: 0,
        };
        FontSet {
            faces: [e(), e(), e(), e(), e(), e(), e()],
        }
    }

    pub fn face(&self, f: Font) -> &FontFace {
        &self.faces[f.index()]
    }
}

impl FontFace {
    /// The `r_font_c` constructor: parse `Fonts/<stem>.tgf`, register each `Fonts/<stem>.<h>.tga`
    /// (TF_NOMIPMAP, REPEAT, linear: no flags), then build `fontHeightMap`.
    /// sg: r_font.cpp:50-115 @3b1a346
    pub fn load(
        src: &dyn AssetSource,
        images: &mut ImageRegistry,
        font: Font,
    ) -> std::io::Result<FontFace> {
        let stem = font.file_stem();
        let raw = src.read_runtime(&format!("Fonts/{stem}.tgf"))?;
        let mut heights: Vec<FontHeight> = Vec::new();
        let mut max_height = 0;
        for th in tgf::parse(&raw) {
            let name = format!("Fonts/{stem}.{}.tga", th.height);
            let bytes = src.read_runtime(&name)?;
            let img = tga::decode(&bytes).map_err(|e| {
                std::io::Error::new(std::io::ErrorKind::InvalidData, format!("{name}: {e}"))
            })?;
            let (fw, fhh) = (img.width as f32, img.height as f32);
            let mut glyphs = [Glyph::default(); 128];
            for (i, g) in th.glyphs.iter().enumerate() {
                glyphs[i] = Glyph {
                    tc_l: g.x as f32 / fw,
                    tc_r: (g.x + g.width) as f32 / fw,
                    tc_t: g.y as f32 / fhh,
                    tc_b: (g.y + th.height as u32) as f32 / fhh,
                    width: g.width as i32,
                    sp_l: g.sp_left,
                    sp_r: g.sp_right,
                };
            }
            let tex = images.insert_ready(
                &name,
                ImageData {
                    width: img.width,
                    height: img.height,
                    levels: vec![img.pixels],
                    flags: ImageFlags::empty(),
                },
            );
            max_height = max_height.max(th.height);
            heights.push(FontHeight {
                tex,
                height: th.height,
                num_glyph: th.glyphs.len(),
                glyphs,
            });
        }
        if heights.is_empty() {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                format!("font \"{stem}\" has no heights"),
            ));
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
        Ok(FontFace {
            heights,
            height_map,
            max_height,
        })
    }

    /// The atlas heights in `.tgf` order.
    pub fn heights(&self) -> &[FontHeight] {
        &self.heights
    }

    /// FindFontHeight: h > max => last; h < 0 => 0; else height_map[h].
    /// sg: r_font.cpp:304-319 @3b1a346
    pub fn find(&self, h: i32) -> usize {
        if h > self.max_height {
            self.heights.len().saturating_sub(1)
        } else if h < 0 {
            0
        } else {
            self.height_map.get(h as usize).copied().unwrap_or(0)
        }
    }

    /// FindSmallerFontHeight(height, idx, 3) => (index, y_pad): the first lower atlas height that
    /// is at least `reduction` smaller than `h`, padded to centre it.
    /// sg: r_font.cpp:288-302 @3b1a346
    pub fn find_smaller(&self, h: i32, idx: usize, reduction: i32) -> (usize, i32) {
        let mut ret = (idx, 0);
        for i in (0..idx).rev() {
            let diff = h - self.heights[i].height;
            if diff >= reduction {
                ret = (i, (diff as f32 / 2.0).ceil() as i32);
                break;
            }
        }
        ret
    }
}

/// `IndexedUTF32String`: the codepoints and, per codepoint, the byte offset it came from.
/// sg: common.h:495-498 @3b1a346
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct IndexedUtf32 {
    pub text: Vec<u32>,
    pub source_code_unit_offsets: Vec<usize>,
}

/// `IndexUTF8ToUTF32`: decode UTF-8 bytes. Overlong forms and surrogates decode as their bit
/// patterns; any byte that does not start a complete sequence becomes U+FFFD and advances one
/// byte.
/// sg: common.cpp:440-500 @3b1a346
pub fn index_utf8_to_utf32(input: &[u8]) -> IndexedUtf32 {
    let byte_count = input.len();
    let mut ret = IndexedUtf32 {
        text: Vec::with_capacity(byte_count),
        source_code_unit_offsets: Vec::with_capacity(byte_count), // conservative reservation
    };
    let mut byte_idx = 0;
    while byte_idx < byte_count {
        let b = &input[byte_idx..];
        let left = byte_count - byte_idx;
        ret.source_code_unit_offsets.push(byte_idx);
        let cont = |c: u8| c >> 6 == 0b10;
        let codepoint;
        if b[0] >> 7 == 0b0 {
            // 0xxx'xxxx
            codepoint = u32::from(b[0]);
            byte_idx += 1;
        } else if left >= 2 && b[0] >> 5 == 0b110 && cont(b[1]) {
            let p0 = u32::from(b[0]) & 0b1_1111;
            let p1 = u32::from(b[1]) & 0b11_1111;
            codepoint = p0 << 6 | p1;
            byte_idx += 2;
        } else if left >= 3 && b[0] >> 4 == 0b1110 && cont(b[1]) && cont(b[2]) {
            let p0 = u32::from(b[0]) & 0b1111;
            let p1 = u32::from(b[1]) & 0b11_1111;
            let p2 = u32::from(b[2]) & 0b11_1111;
            codepoint = p0 << 12 | p1 << 6 | p2;
            byte_idx += 3;
        } else if left >= 4 && b[0] >> 3 == 0b11110 && cont(b[1]) && cont(b[2]) && cont(b[3]) {
            let p0 = u32::from(b[0]) & 0b111;
            let p1 = u32::from(b[1]) & 0b11_1111;
            let p2 = u32::from(b[2]) & 0b11_1111;
            let p3 = u32::from(b[3]) & 0b11_1111;
            codepoint = p0 << 18 | p1 << 12 | p2 << 6 | p3;
            byte_idx += 4;
        } else {
            codepoint = 0xFFFD;
            byte_idx += 1;
        }
        ret.text.push(codepoint);
    }
    ret
}

/// [`index_utf8_to_utf32`] of a Rust string as `(codepoints, byte offsets)`. For valid UTF-8 the
/// SG decoder and `char_indices` agree.
pub fn index_utf8(text: &str) -> (Vec<u32>, Vec<usize>) {
    let r = index_utf8_to_utf32(text.as_bytes());
    (r.text, r.source_code_unit_offsets)
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use crate::frame::{Quad, QuadSink};
    use crate::types::Align;
    use std::path::PathBuf;

    /// Test-only file-system AssetSource (pob-sg itself has no std::fs).
    pub(crate) struct DirSource {
        pub src: PathBuf,
        pub runtime: PathBuf,
    }
    impl AssetSource for DirSource {
        fn read_src(&self, path: &str) -> std::io::Result<Vec<u8>> {
            std::fs::read(self.src.join(path))
        }
        fn read_runtime(&self, path: &str) -> std::io::Result<Vec<u8>> {
            std::fs::read(self.runtime.join(path))
        }
    }

    /// `.PathOfBuilding` of the main checkout (or `$POB_ROOT`), found from this crate's directory.
    pub(crate) fn locate() -> Option<DirSource> {
        let mut starts = Vec::new();
        if let Some(p) = std::env::var_os("POB_ROOT") {
            starts.push(PathBuf::from(p));
        }
        starts.push(PathBuf::from(env!("CARGO_MANIFEST_DIR")));
        for s in starts {
            for d in s.ancestors() {
                let root = d.join(".PathOfBuilding");
                if root
                    .join("runtime/SimpleGraphic/Fonts/Liberation Sans.tgf")
                    .is_file()
                {
                    return Some(DirSource {
                        src: root.join("src"),
                        runtime: root.join("runtime/SimpleGraphic"),
                    });
                }
            }
        }
        None
    }

    pub(crate) fn fonts_and_images() -> Option<(FontSet, ImageRegistry)> {
        let Some(src) = locate() else {
            eprintln!("SKIP: PoB fonts not found (set POB_ROOT)");
            return None;
        };
        let mut images = ImageRegistry::new(Box::new(crate::image::tests::MemLoader::default()));
        let fs = FontSet::load(&src, &mut images).expect("load fonts");
        Some((fs, images))
    }

    fn fonts() -> Option<FontSet> {
        fonts_and_images().map(|(f, _)| f)
    }

    /// Records quads with the colour and atlas current when they were emitted.
    #[derive(Default)]
    struct Rec {
        tex: Option<TexId>,
        col: [f32; 4],
        quads: Vec<(TexId, [f32; 4], Quad)>,
    }
    impl QuadSink for Rec {
        fn bind(&mut self, t: TexId) {
            self.tex = Some(t);
        }
        fn color(&mut self, c: [f32; 4]) {
            self.col = c;
        }
        fn quad(&mut self, q: Quad) {
            self.quads
                .push((self.tex.expect("bind before quad"), self.col, q));
        }
    }

    #[test]
    fn tgf_parse_counts() {
        let Some((fs, images)) = fonts_and_images() else {
            return;
        };
        let expect = [
            10, 12, 14, 16, 18, 20, 22, 24, 26, 28, 32, 36, 40, 48, 56, 64,
        ];
        for f in Font::ALL {
            let face = fs.face(f);
            assert_eq!(face.heights.len(), 16, "{f:?}");
            let hs: Vec<i32> = face.heights.iter().map(|h| h.height).collect();
            assert_eq!(hs, expect, "{f:?}");
            for h in &face.heights {
                assert_eq!(h.num_glyph, 128, "{f:?} {}", h.height);
                let (w, ah) = images.tex_size(h.tex).unwrap();
                assert!(w > 0 && ah > 0);
            }
            assert_eq!(face.max_height, 64);
        }
        // Spot-check raw values: Liberation Sans 16 'A' = GLYPH 0 120 9 0 0 in a 128x256 atlas.
        let var = fs.face(Font::Var);
        let h16 = &var.heights[3];
        assert_eq!(images.tex_size(h16.tex), Some((128, 256)));
        let a = h16.glyph(u32::from('A'));
        assert_eq!(
            (a.tc_l * 128.0, a.tc_t * 256.0, a.width, a.sp_l, a.sp_r),
            (0.0, 120.0, 9, 0, 0)
        );
        assert_eq!(a.tc_r, 9.0 / 128.0);
        assert_eq!(a.tc_t, 120.0 / 256.0);
        assert_eq!(a.tc_b, 136.0 / 256.0);
    }

    #[test]
    fn height_map() {
        let Some(fs) = fonts() else { return };
        let f = fs.face(Font::Var);
        let h = |req: i32| f.heights[f.find(req)].height;
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
        assert_eq!(var.string_width(16, "Hello"), 31);
        assert_eq!(var.string_width(16, "^7Hel^xFF0000lo"), 31);
        assert_eq!(var.string_width(16, "Hello\nA"), 31);
        assert_eq!(var.string_width(16, "A\nHello\n"), 31);
        assert_eq!(var.string_width(16, ""), 0);
        // Tab = 4 spaces: space at 16 is 1+0+3 = 4 -> 16.
        assert_eq!(var.string_width(16, "\t"), 16);
        // Bitstream Vera Sans Mono 12: 'A' = 5+1+0 = 6.
        let fixed = fs.face(Font::Fixed);
        assert_eq!(fixed.string_width(12, "AAAA"), 24);
        // Scaled: height 34 uses the 32 atlas with scale 34/32 and per-glyph ceil.
        let a32 = var.heights[var.find(32)].glyph(u32::from('A')).advance();
        let scale = 34.0f32 / 32.0;
        let mut w = 0.0f32;
        for _ in 0..3 {
            w = (w + a32 as f32 * scale).ceil();
        }
        assert_eq!(var.string_width(34, "AAA"), w as i32);
    }

    #[test]
    fn tofu_width() {
        let Some(fs) = fonts() else { return };
        let var = fs.face(Font::Var);
        // 'é' (U+00E9) is measured as "[U+00E9]" in the 12 atlas for height 16
        // (first lower height with 16 - h >= 3 is 12).
        let (ti, pad) = var.find_smaller(16, var.find(16), 3);
        assert_eq!(var.heights[ti].height, 12);
        assert_eq!(pad, 2);
        let expect: i32 = "[U+00E9]"
            .chars()
            .map(|c| var.heights[ti].glyph(c as u32).advance())
            .sum();
        assert_eq!(var.string_width(16, "é"), expect);
    }

    #[test]
    fn cursor_index() {
        let Some(fs) = fonts() else { return };
        let var = fs.face(Font::Var);
        // Cumulative right edges: H 9, e 17, l 20, l 23, o 31.
        assert_eq!(var.cursor_index(16, "Hello", 0, 0), 0);
        assert_eq!(var.cursor_index(16, "Hello", 9, 0), 0);
        assert_eq!(var.cursor_index(16, "Hello", 10, 0), 1);
        assert_eq!(var.cursor_index(16, "Hello", 31, 0), 4);
        assert_eq!(var.cursor_index(16, "Hello", 32, 0), 5);
        // Multi-line: second line starts at byte 6; y within line 2.
        assert_eq!(var.cursor_index(16, "Hello\nHe", 100, 17), 8);
        assert_eq!(var.cursor_index(16, "Hello\nHe", 100, 16), 5);
        assert_eq!(var.cursor_index(16, "Hello\nHe", 100, 500), 8);
        // Byte offsets: 'é' is 2 bytes.
        assert_eq!(var.cursor_index(16, "éa", 999, 0), 3);
    }

    #[test]
    fn layout_alignment_and_escapes() {
        let Some(fs) = fonts() else { return };
        let var = fs.face(Font::Var);
        let mut col = [0.5, 0.5, 0.5, 0.25];
        let mut out = Rec::default();
        var.draw(
            &mut out,
            [10.4, 20.7],
            Align::Left,
            16,
            &mut col,
            "^1H^xFFFFFFe",
            1000,
            800,
        );
        assert_eq!(out.quads.len(), 2);
        // x = round(10.4) = 10; y = floor(20.7) = 20; H spLeft 1 -> 11.
        let q0 = out.quads[0].2;
        assert_eq!(q0.x[0], 11.0);
        assert_eq!(q0.y[0], 20.0);
        assert_eq!(q0.y[2], 36.0);
        assert_eq!(out.quads[0].0, var.heights[3].tex);
        // ^1 forces alpha to 1.
        assert_eq!(out.quads[0].1, [1.0, 0.0, 0.0, 1.0]);
        assert_eq!(out.quads[1].1, [1.0, 1.0, 1.0, 1.0]);
        // Draw colour is left at the last escape.
        assert_eq!(col, [1.0, 1.0, 1.0, 1.0]);

        // Alignment math: width("Hello") = 31 at 16.
        let first_x = |align, x: f32| {
            let mut col = [1.0; 4];
            let mut out = Rec::default();
            var.draw(&mut out, [x, 0.0], align, 16, &mut col, "Hello", 1000, 800);
            out.quads[0].2.x[0] - 1.0 // minus H's spLeft
        };
        assert_eq!(first_x(Align::Left, 100.0), 100.0);
        assert_eq!(first_x(Align::CenterX, 100.0), (100.0f32 - 15.5).floor());
        assert_eq!(first_x(Align::RightX, 100.0), 69.0);
        assert_eq!(
            first_x(Align::Center, 10.0),
            ((1000.0f32 - 31.0) / 2.0 + 10.0).floor()
        );
        assert_eq!(first_x(Align::Right, 10.0), 1000.0 - 31.0 - 10.0);
    }

    #[test]
    fn offscreen_line_still_applies_escapes() {
        let Some(fs) = fonts() else { return };
        let var = fs.face(Font::Var);
        let mut col = [1.0, 1.0, 1.0, 0.5];
        let mut out = Rec::default();
        var.draw(
            &mut out,
            [0.0, -100.0],
            Align::Left,
            16,
            &mut col,
            "^2abc",
            100,
            100,
        );
        assert!(out.quads.is_empty());
        assert_eq!(col, [0.0, 1.0, 0.0, 1.0]);
    }

    fn atlas(images: &mut ImageRegistry, tex: TexId) -> std::rc::Rc<ImageData> {
        images
            .take_uploads()
            .into_iter()
            .find(|(t, _)| *t == tex)
            .map(|(_, d)| d)
            .expect("atlas upload")
    }

    #[test]
    fn atlases_white_rgb() {
        let Some((fs, images)) = fonts_and_images() else {
            return;
        };
        let mut images = images;
        let ups: Vec<_> = images.take_uploads();
        // Decode one atlas of each face; RGB is white wherever alpha > 0.
        for f in Font::ALL {
            let fh = &fs.face(f).heights[3];
            let img = &ups.iter().find(|(t, _)| *t == fh.tex).unwrap().1;
            assert_eq!(Some((img.width, img.height)), images.tex_size(fh.tex));
            let px = &img.levels[0];
            assert!(px
                .chunks_exact(4)
                .filter(|p| p[3] > 0)
                .all(|p| p[0] == 255 && p[1] == 255 && p[2] == 255));
            assert!(px.chunks_exact(4).any(|p| p[3] >= 200));
        }
    }

    #[test]
    fn glyph_cell_has_ink_top_down() {
        let Some((fs, mut images)) = fonts_and_images() else {
            return;
        };
        // 'A' cell in Liberation Sans 16 (bottom-up file): after flipping, the apex of 'A' (upper
        // third of the cell) has less ink than the legs and crossbar.
        let fh = &fs.face(Font::Var).heights[3];
        let img = atlas(&mut images, fh.tex);
        let g = *fh.glyph(u32::from('A'));
        let (gx, gy) = (
            (g.tc_l * img.width as f32) as u32,
            (g.tc_t * img.height as f32) as u32,
        );
        let ink = |y: u32| {
            (gx..gx + g.width as u32)
                .map(|x| u32::from(img.levels[0][((y * img.width + x) * 4 + 3) as usize]))
                .sum::<u32>()
        };
        let total: u32 = (gy..gy + 16).map(ink).sum();
        assert!(total > 0);
        let upper: u32 = (gy..gy + 7).map(ink).sum();
        let lower: u32 = (gy + 7..gy + 14).map(ink).sum();
        assert!(upper < lower, "upper {upper} lower {lower}");
    }
}
