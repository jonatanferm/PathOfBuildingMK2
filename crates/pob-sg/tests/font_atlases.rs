//! U2 acceptance: all 7×16 atlases parse; the TGA is flipped bottom-up with alpha = coverage; the
//! height map matches a hand table (ui doc §2 "Atlas choice", §12.3 U2).
//!
//! Sources: SimpleGraphic @ 3b1a346 `r_font.cpp:50-115` (constructor), `:288-319`
//! (FindSmallerFontHeight / FindFontHeight), `core_image.cpp:162-254` (targa_c::Load).
//! The checks below re-read the raw `.tgf`/`.tga` bytes with their own minimal decoders, so they
//! do not trust the crate's parser or decoder.

#[path = "font_util.rs"]
mod font_util;

use font_util::{fonts_and_images, locate};
use pob_sg::font::tga;
use pob_sg::{AssetSource, Font};

const HEIGHTS: [i32; 16] = [
    10, 12, 14, 16, 18, 20, 22, 24, 26, 28, 32, 36, 40, 48, 56, 64,
];

/// Raw `GLYPH x y w sl sr;` numbers per `HEIGHT`, by plain whitespace splitting.
fn raw_tgf(text: &str) -> Vec<(i32, Vec<[i64; 5]>)> {
    let mut out: Vec<(i32, Vec<[i64; 5]>)> = Vec::new();
    for l in text.lines() {
        let l = l.split(';').next().unwrap();
        let t: Vec<&str> = l.split_whitespace().collect();
        match t.first() {
            Some(&"HEIGHT") => out.push((t[1].parse().unwrap(), Vec::new())),
            Some(&"GLYPH") => {
                let n: Vec<i64> = t[1..6].iter().map(|v| v.parse().unwrap()).collect();
                out.last_mut()
                    .unwrap()
                    .1
                    .push([n[0], n[1], n[2], n[3], n[4]]);
            }
            _ => {}
        }
    }
    out
}

#[test]
fn all_112_atlases_parse_with_tgf_metrics() {
    let src = locate().expect("PoB runtime");
    let (fs, images) = fonts_and_images();
    let mut atlases = 0;
    for f in Font::ALL {
        let face = fs.face(f);
        let raw = raw_tgf(
            &String::from_utf8(
                src.read_runtime(&format!("Fonts/{}.tgf", f.file_stem()))
                    .unwrap(),
            )
            .unwrap(),
        );
        let hs: Vec<i32> = face.heights().iter().map(|h| h.height).collect();
        assert_eq!(hs, HEIGHTS, "{f:?}");
        assert_eq!(raw.len(), 16);
        for (fh, (rh, rglyphs)) in face.heights().iter().zip(&raw) {
            assert_eq!(fh.height, *rh);
            assert_eq!(fh.num_glyph, 128, "{f:?} {rh}");
            assert_eq!(rglyphs.len(), 128);
            // Atlas size from the TGA header (bytes 12..16) and the registry.
            let tga_bytes = src
                .read_runtime(&format!("Fonts/{}.{rh}.tga", f.file_stem()))
                .unwrap();
            let (w, h) = (
                u32::from(u16::from_le_bytes([tga_bytes[12], tga_bytes[13]])),
                u32::from(u16::from_le_bytes([tga_bytes[14], tga_bytes[15]])),
            );
            assert_eq!(images.tex_size(fh.tex), Some((w, h)));
            for (i, g) in rglyphs.iter().enumerate() {
                let [x, y, gw, sl, sr] = *g;
                let have = fh.glyph(i as u32);
                // glyph->tcLeft = (float)x / fileWidth, ... (r_font.cpp:89-95)
                assert_eq!(have.tc_l, x as f32 / w as f32);
                assert_eq!(have.tc_r, (x + gw) as f32 / w as f32);
                assert_eq!(have.tc_t, y as f32 / h as f32);
                assert_eq!(have.tc_b, (y + i64::from(*rh)) as f32 / h as f32);
                assert_eq!(
                    (have.width, have.sp_l, have.sp_r),
                    (gw as i32, sl as i32, sr as i32)
                );
            }
            // Codepoints are narrowed to a byte: 0x141 looks up glyph 0x41.
            assert_eq!(fh.glyph(0x141), fh.glyph(0x41));
            atlases += 1;
        }
    }
    assert_eq!(atlases, 112);
}

/// Minimal independent RLE reader: the rows in FILE order, BGRA.
#[allow(clippy::type_complexity, clippy::many_single_char_names)]
fn file_rows(b: &[u8]) -> (usize, usize, u8, Vec<Vec<u8>>) {
    assert_eq!(
        (b[1], b[2] & 7, b[2] & 8, b[16]),
        (0, 2, 8, 32),
        "font atlases are 32 bpp RLE"
    );
    let w = usize::from(u16::from_le_bytes([b[12], b[13]]));
    let h = usize::from(u16::from_le_bytes([b[14], b[15]]));
    let mut p = 18 + usize::from(b[0]);
    let mut rows = Vec::new();
    for _ in 0..h {
        let mut row = Vec::with_capacity(w * 4);
        while row.len() < w * 4 {
            let hdr = b[p];
            p += 1;
            let n = usize::from(hdr & 0x7f) + 1;
            if hdr & 0x80 != 0 {
                for _ in 0..n {
                    row.extend_from_slice(&b[p..p + 4]);
                }
                p += 4;
            } else {
                row.extend_from_slice(&b[p..p + 4 * n]);
                p += 4 * n;
            }
        }
        rows.push(row);
    }
    (w, h, b[17], rows)
}

#[test]
fn tga_flip_and_alpha_coverage() {
    let src = locate().expect("PoB runtime");
    let (mut bottom_up, mut top_down) = (0, 0);
    for f in Font::ALL {
        for h in HEIGHTS {
            let bytes = src
                .read_runtime(&format!("Fonts/{}.{h}.tga", f.file_stem()))
                .unwrap();
            let img = tga::decode(&bytes).unwrap();
            let (w, ht, desc, rows) = file_rows(&bytes);
            assert_eq!((img.width as usize, img.height as usize), (w, ht));
            // flipV = !(descriptor & 0x20): row r of the image is file row ht-1-r when flipped.
            let flip = desc & 0x20 == 0;
            if flip {
                bottom_up += 1;
            } else {
                top_down += 1;
            }
            for r in 0..ht {
                let fr = &rows[if flip { ht - 1 - r } else { r }];
                let out = &img.pixels[r * w * 4..(r + 1) * w * 4];
                for x in 0..w {
                    let (bgra, rgba) = (&fr[x * 4..x * 4 + 4], &out[x * 4..x * 4 + 4]);
                    assert_eq!(
                        rgba,
                        [bgra[2], bgra[1], bgra[0], bgra[3]],
                        "{f:?} {h} ({x},{r})"
                    );
                }
            }
            // White glyphs, alpha = coverage: RGB is white wherever alpha > 0 and the alpha
            // channel carries anti-aliasing levels.
            let px = &img.pixels;
            assert!(
                px.chunks_exact(4)
                    .filter(|p| p[3] > 0)
                    .all(|p| p[..3] == [255, 255, 255]),
                "{f:?} {h}"
            );
            let mut levels = [false; 256];
            for p in px.chunks_exact(4) {
                levels[usize::from(p[3])] = true;
            }
            assert!(
                levels.iter().filter(|&&l| l).count() > 16,
                "{f:?} {h}: coverage levels"
            );
        }
    }
    // Both orientations ship (e.g. Liberation Sans 16 is bottom-up, Liberation Sans 32 top-down).
    assert_eq!(bottom_up + top_down, 112);
    assert!(bottom_up > 0 && top_down > 0, "{bottom_up} / {top_down}");
}

#[test]
fn glyph_cells_are_upright_in_both_orientations() {
    let src = locate().expect("PoB runtime");
    let (fs, _) = fonts_and_images();
    let face = fs.face(Font::Var);
    for h in [16, 32] {
        let bytes = src
            .read_runtime(&format!("Fonts/Liberation Sans.{h}.tga"))
            .unwrap();
        assert_eq!(
            bytes[17] & 0x20 != 0,
            h == 32,
            "orientation of Liberation Sans {h}"
        );
        let img = tga::decode(&bytes).unwrap();
        let g = face
            .heights()
            .iter()
            .find(|fh| fh.height == h)
            .unwrap()
            .glyph(u32::from('A'));
        let (gx, gy) = (
            (g.tc_l * img.width as f32) as u32,
            (g.tc_t * img.height as f32) as u32,
        );
        let ink = |y: u32| -> u32 {
            (gx..gx + g.width as u32)
                .map(|x| u32::from(img.pixels[((y * img.width + x) * 4 + 3) as usize]))
                .sum()
        };
        // The apex of 'A' (upper part of the cell) has less ink than its legs and crossbar.
        let third = h as u32 * 2 / 5;
        let upper: u32 = (gy..gy + third).map(ink).sum();
        let lower: u32 = (gy + third..gy + 2 * third).map(ink).sum();
        assert!(
            upper > 0 && upper < lower,
            "{h}: upper {upper} lower {lower}"
        );
    }
}

/// The atlas height each requested height maps to, written out by hand from the rule
/// "nearest, ties to the lower" (r_font.cpp:99-114; lim = (gh - below - 1) / 2 heights below
/// each atlas round up): 10..=28 step 2, then 32 (gap 4, lim 1), 36 (lim 1), 40 (lim 1), 48 (gap
/// 8, lim 3), 56 (lim 3), 64 (lim 3). Heights 0..=9 use index 0 (10), negative heights index 0,
/// and heights above 64 the last atlas.
const HAND_MAP: &[(i32, i32, i32)] = &[
    // (from, to inclusive, atlas)
    (-50, -1, 10),
    (0, 11, 10),
    (12, 13, 12),
    (14, 15, 14),
    (16, 17, 16),
    (18, 19, 18),
    (20, 21, 20),
    (22, 23, 22),
    (24, 25, 24),
    (26, 27, 26),
    (28, 30, 28), // 29 and 30 map down to 28
    (31, 34, 32), // 31 maps up
    (35, 38, 36),
    (39, 44, 40),
    (45, 52, 48), // 45..=47 map up
    (53, 60, 56),
    (61, 64, 64),
    (65, 1000, 64),
];

#[test]
fn height_map_matches_hand_table() {
    let (fs, _) = fonts_and_images();
    for f in Font::ALL {
        let face = fs.face(f);
        for &(a, b, atlas) in HAND_MAP {
            for h in a..=b {
                assert_eq!(
                    face.heights()[face.find(h)].height,
                    atlas,
                    "{f:?} height {h}"
                );
            }
        }
        // The ui doc's examples.
        let at = |h: i32| face.heights()[face.find(h)].height;
        assert_eq!(
            [at(11), at(29), at(30), at(31), at(65), at(200)],
            [10, 28, 28, 32, 64, 64]
        );
    }
}

/// FindSmallerFontHeight(height, idx, 3) for the tofu atlas: the first lower atlas at least 3 px
/// smaller, y pad = ceil(diff / 2); none found keeps the main atlas with pad 0.
#[test]
fn tofu_atlas_hand_table() {
    let (fs, _) = fonts_and_images();
    let face = fs.face(Font::Var);
    let cases = [
        (10, 10, 0),
        (12, 12, 0), // 12 - 10 = 2 < 3
        (14, 10, 2),
        (16, 12, 2),
        (18, 14, 2),
        (20, 16, 2),
        (28, 24, 2),
        (30, 26, 2), // main atlas 28
        (32, 28, 2),
        (34, 28, 3), // main atlas 32: 34 - 28 = 6
        (36, 32, 2),
        (40, 36, 2),
        (48, 40, 4),
        (64, 56, 4),
        (100, 56, 22), // main atlas 64
        (11, 10, 0),   // main atlas 10 is index 0: nothing smaller
        (13, 10, 2),   // main atlas 12: 13 - 10 = 3 -> pad ceil(1.5) = 2
    ];
    for (h, atlas, pad) in cases {
        let idx = face.find(h);
        let (t, p) = face.find_smaller(h, idx, pob_sg::font::TOFU_SIZE_REDUCTION);
        assert_eq!((face.heights()[t].height, p), (atlas, pad), "height {h}");
    }
}
