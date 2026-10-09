//! T5 (ui doc §11.1): `pob-sg-wgpu` read-back vs the CPU reference rasteriser
//! (`pob_sg::raster`) for the same `Frame`. Gate: every channel within ±2/255 on ≥ 99.9 % of
//! the pixels. Owner: U6.
//!
//! Six synthetic frames (U6 acceptance): tinted quads, rotated quads, repeat/clamp/flipped UVs
//! (plus mips), nested viewports and scissor, layered alpha, text (at DPI 1.0, plus a 1.5 run).
//! The frames are recorded through `pob_sg::Sg`, so they carry exactly what PoB would submit.
//!
//! The backend comes from `WGPU_BACKEND` (`vulkan`, `gl`, ...). Without a GPU adapter or
//! without PoB's fonts the tests fail, unless `POB_T5_ALLOW_SKIP=1`. Every run writes
//! `<frame>_{gpu,cpu,diff}.png` into `$CARGO_TARGET_TMPDIR/t5/<backend>/` and prints the stats.
#![cfg(not(target_arch = "wasm32"))]
#![allow(
    clippy::too_many_lines,
    clippy::cast_precision_loss,
    clippy::many_single_char_names
)]

use std::path::PathBuf;
use std::rc::Rc;

use pob_sg::raster::Raster;
use pob_sg::{
    Align, AssetSource, Font, FontSet, Frame, ImageData, ImageFlags, ImageHandle, ImageLoader,
    ImageRegistry, LoadEvent, Num, ScreenInfo, Sg, TexId,
};
use pob_sg_wgpu::{request_device, SgRenderer, RT_FORMAT};

/// ±2/255 per channel ...
const TOLERANCE: u8 = 2;
/// ... on at least this fraction of the pixels.
const MIN_FRACTION: f64 = 0.999;

fn allow_skip() -> bool {
    std::env::var("POB_T5_ALLOW_SKIP").is_ok_and(|v| v == "1")
}

// ---------------------------------------------------------------------------------------------
// Synthetic textures (deterministic mode: every load completes synchronously, ui doc §4.5).

fn gen_texture(name: &str) -> Option<(u32, u32, Vec<u8>)> {
    let mut px = Vec::new();
    let (w, h) = match name {
        // Smooth gradient, opaque.
        "grad16.png" => {
            for y in 0..16u32 {
                for x in 0..16u32 {
                    px.extend_from_slice(&[
                        (x * 17) as u8,
                        (y * 17) as u8,
                        (255 - (x + y) * 8) as u8,
                        255,
                    ]);
                }
            }
            (16, 16)
        }
        // Radial alpha.
        "alpha16.png" => {
            for y in 0..16 {
                for x in 0..16 {
                    let d = ((x as f64 - 7.5).powi(2) + (y as f64 - 7.5).powi(2)).sqrt();
                    let a = (255.0 * (1.0 - d / 8.0)).clamp(0.0, 255.0) as u8;
                    px.extend_from_slice(&[255, 200, 40, a]);
                }
            }
            (16, 16)
        }
        // One-texel checker.
        "checker8.png" => {
            for y in 0..8 {
                for x in 0..8 {
                    if (x + y) % 2 == 0 {
                        px.extend_from_slice(&[230, 40, 40, 255]);
                    } else {
                        px.extend_from_slice(&[30, 60, 220, 255]);
                    }
                }
            }
            (8, 8)
        }
        // Smooth + high-frequency content, for mips and minification.
        "tex64.png" => {
            for y in 0..64 {
                for x in 0..64 {
                    let r = 128.0 + 127.0 * (x as f64 / 5.0).sin();
                    let g = 128.0 + 127.0 * (y as f64 / 7.0).cos();
                    let b = if (x / 4 + y / 4) % 2 == 0 { 220 } else { 40 };
                    let a = if (x / 8 + y / 8) % 3 == 0 { 160 } else { 255 };
                    px.extend_from_slice(&[r as u8, g as u8, b, a]);
                }
            }
            (64, 64)
        }
        _ => return None,
    };
    Some((w, h, px))
}

/// Serves [`gen_texture`]; `pending*` stays pending (an ASYNC upload that never lands, so it
/// binds `@black`); anything else fails (the default texture).
struct SynthLoader;

impl ImageLoader for SynthLoader {
    fn request(
        &mut self,
        id: TexId,
        path: &str,
        flags: ImageFlags,
        _sync: bool,
        _priority: i32,
    ) -> Option<LoadEvent> {
        if path.starts_with("pending") {
            return None;
        }
        let Some((w, h, px)) = gen_texture(path) else {
            return Some(LoadEvent::Failed(id));
        };
        let levels = if flags.contains(ImageFlags::MIPMAP) {
            pob_sg::image::build_mips(w, h, px)
        } else {
            vec![px]
        };
        Some(LoadEvent::Done(
            id,
            ImageData {
                width: w,
                height: h,
                levels,
                flags,
            },
        ))
    }
    fn read_size(&mut self, _id: TexId, path: &str) -> Option<(u32, u32)> {
        gen_texture(path).map(|(w, h, _)| (w, h))
    }
    fn set_priority(&mut self, _id: TexId, _priority: i32) {}
    fn poll(&mut self) -> Vec<LoadEvent> {
        Vec::new()
    }
}

// ---------------------------------------------------------------------------------------------
// Fonts (PoB's runtime/SimpleGraphic/Fonts; gitignored, so found from the main checkout).

struct DirSource {
    src: PathBuf,
    runtime: PathBuf,
}

impl AssetSource for DirSource {
    fn read_src(&self, path: &str) -> std::io::Result<Vec<u8>> {
        std::fs::read(self.src.join(path))
    }
    fn read_runtime(&self, path: &str) -> std::io::Result<Vec<u8>> {
        std::fs::read(self.runtime.join(path))
    }
}

/// `.PathOfBuilding` from `$POB_ROOT` or an ancestor of this crate (worktrees live inside the
/// main checkout).
fn locate_pob() -> Option<DirSource> {
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

fn new_sg(with_fonts: bool) -> Option<Sg> {
    let mut images = ImageRegistry::new(Box::new(SynthLoader));
    let fonts = if with_fonts {
        let Some(src) = locate_pob() else {
            assert!(
                allow_skip(),
                "T5 text frame: PoB fonts not found (.PathOfBuilding/runtime/SimpleGraphic/Fonts \
                 in an ancestor directory or $POB_ROOT); POB_T5_ALLOW_SKIP=1 skips"
            );
            eprintln!("SKIP: PoB fonts not found");
            return None;
        };
        FontSet::load(&src, &mut images).expect("load PoB fonts")
    } else {
        FontSet::empty()
    };
    Some(Sg::new(Rc::new(fonts), images))
}

fn screen(w: i32, h: i32, dpi: f32) -> ScreenInfo {
    ScreenInfo {
        fb_w: w,
        fb_h: h,
        win_h: h,
        dpi_scale: dpi,
    }
}

fn load(sg: &mut Sg, path: &str, flags: ImageFlags) -> ImageHandle {
    let h = sg.new_image_handle();
    h.load(sg, path, flags);
    h
}

/// Corners TL, TR, BR, BL of a `w` x `h` rectangle centred on `(cx, cy)`, rotated by `deg`.
fn rot(cx: Num, cy: Num, w: Num, h: Num, deg: Num) -> [Num; 8] {
    let (s, c) = deg.to_radians().sin_cos();
    let mut p = [0.0; 8];
    for (i, (dx, dy)) in [
        (-w / 2.0, -h / 2.0),
        (w / 2.0, -h / 2.0),
        (w / 2.0, h / 2.0),
        (-w / 2.0, h / 2.0),
    ]
    .into_iter()
    .enumerate()
    {
        p[2 * i] = cx + dx * c - dy * s;
        p[2 * i + 1] = cy + dx * s + dy * c;
    }
    p
}

// ---------------------------------------------------------------------------------------------
// The six frames.

/// 1. Tinted quads: solid tints (incl. alpha and out-of-range components), textured quads at
///    1:1, magnified and at fractional positions, a pending (black-bound) and a failed (default
///    texture) image.
fn frame_tinted(sg: &mut Sg) -> Frame {
    let grad = load(sg, "grad16.png", ImageFlags::empty());
    let alpha = load(sg, "alpha16.png", ImageFlags::empty());
    let pending = load(sg, "pending.png", ImageFlags::ASYNC);
    let missing = load(sg, "missing.png", ImageFlags::empty());
    sg.begin_frame(screen(320, 240, 1.0));
    sg.set_draw_color(0.2, 0.3, 0.4);
    sg.draw_image(None, 0.0, 0.0, 320.0, 240.0);
    let tints: [[Num; 4]; 6] = [
        [1.0, 0.0, 0.0, 1.0],
        [0.0, 1.0, 0.0, 0.5],
        [0.0, 0.0, 1.0, 0.25],
        [1.0, 1.0, 0.0, 0.75],
        [0.3, 0.6, 0.9, 0.9],
        [1.0, 1.0, 1.0, 0.1],
    ];
    for (i, t) in tints.iter().enumerate() {
        sg.set_draw_color_a(t[0], t[1], t[2], t[3]);
        sg.draw_image(None, 10.0 + i as Num * 50.0, 10.0, 40.0, 40.0);
    }
    sg.set_draw_color_a(0.9, 0.9, 0.9, 0.6);
    sg.draw_image(None, 20.0, 30.0, 270.0, 15.0);
    // Out-of-range tint: the fragment output is clamped to [0, 1] before blending.
    sg.set_draw_color_a(1.5, 0.5, -0.25, 0.8);
    sg.draw_image(None, 250.0, 60.0, 50.0, 30.0);
    sg.set_draw_color_a(1.0, 0.5, 1.0, 1.0);
    sg.draw_image(Some(&grad), 10.0, 80.0, 16.0, 16.0);
    sg.set_draw_color(1.0, 1.0, 1.0);
    sg.draw_image(Some(&grad), 40.0, 80.0, 56.0, 56.0);
    sg.draw_image(Some(&grad), 110.3, 80.6, 47.5, 33.25);
    sg.set_draw_color_a(0.5, 1.0, 0.8, 0.9);
    sg.draw_image(Some(&alpha), 170.0, 80.0, 64.0, 64.0);
    sg.set_draw_color(1.0, 1.0, 1.0);
    sg.draw_image(Some(&pending), 0.0, 0.0, 320.0, 240.0);
    sg.draw_image(Some(&missing), 250.0, 100.0, 32.0, 32.0);
    sg.draw_image(Some(&missing), 250.0, 150.0, 64.0, 64.0);
    sg.set_draw_color_a(0.0, 0.0, 0.0, 0.5);
    sg.draw_image(None, 5.5, 150.5, 200.0, 0.75);
    sg.draw_image(None, 5.25, 160.25, 0.5, 60.0);
    sg.end_frame()
}

/// 2. Rotated quads: magnified NEAREST checker, LINEAR gradient, thin lines, a trapezoid,
///    permuted UVs, a mip-mapped texture minified isotropically, a quad leaving the screen.
fn frame_rotated(sg: &mut Sg) -> Frame {
    let checker = load(sg, "checker8.png", ImageFlags::NEAREST);
    let grad = load(sg, "grad16.png", ImageFlags::empty());
    let tex = load(sg, "tex64.png", ImageFlags::MIPMAP);
    sg.begin_frame(screen(320, 240, 1.0));
    sg.set_draw_color(0.1, 0.1, 0.1);
    sg.draw_image(None, 0.0, 0.0, 320.0, 240.0);
    sg.set_draw_color(1.0, 1.0, 1.0);
    sg.draw_image_quad(Some(&checker), rot(55.0, 55.0, 64.0, 64.0, 30.0));
    sg.draw_image_quad(Some(&grad), rot(150.0, 50.0, 40.0, 40.0, 45.0));
    sg.set_draw_color_a(1.0, 0.8, 0.2, 0.7);
    for i in 0..6 {
        sg.draw_image_quad(None, rot(250.0, 50.0, 120.0, 1.5, i as Num * 31.0 + 3.0));
    }
    sg.set_draw_color(1.0, 1.0, 1.0);
    sg.draw_image_quad(
        Some(&grad),
        [20.0, 120.0, 110.0, 130.0, 125.0, 215.0, 10.0, 200.0],
    );
    sg.draw_image_quad_tc(
        Some(&grad),
        rot(170.0, 160.0, 48.0, 32.0, -20.0),
        [1.0, 0.0, 1.0, 1.0, 0.0, 1.0, 0.0, 0.0],
    );
    sg.draw_image_quad(Some(&tex), rot(250.0, 150.0, 32.0, 32.0, 30.0));
    sg.set_draw_color_a(0.6, 1.0, 1.0, 0.8);
    sg.draw_image_quad(Some(&checker), rot(310.0, 230.0, 60.0, 40.0, 20.0));
    // Degenerate: two corners coincide (a triangle) and a zero-area quad.
    sg.set_draw_color(0.2, 0.9, 0.4);
    sg.draw_image_quad(
        None,
        [200.0, 190.0, 240.0, 190.0, 240.0, 230.0, 240.0, 230.0],
    );
    sg.draw_image_quad(None, [10.0, 10.0, 20.0, 10.0, 30.0, 10.0, 40.0, 10.0]);
    sg.end_frame()
}

/// 3. Texture coordinates: REPEAT vs CLAMP beyond [0, 1], NEAREST vs LINEAR, flipped UVs,
///    mip levels (λ = 2, ≈1.4, 0, ≈1.7 with REPEAT) and minification without mips.
fn frame_uvs(sg: &mut Sg) -> Frame {
    let grad = load(sg, "grad16.png", ImageFlags::empty());
    let grad_c = load(sg, "grad16.png", ImageFlags::CLAMP);
    let checker = load(sg, "checker8.png", ImageFlags::NEAREST);
    let checker_c = load(sg, "checker8.png", ImageFlags::NEAREST | ImageFlags::CLAMP);
    let mip = load(sg, "tex64.png", ImageFlags::MIPMAP);
    let nomip = load(sg, "tex64.png", ImageFlags::empty());
    sg.begin_frame(screen(320, 240, 1.0));
    sg.set_draw_color(1.0, 1.0, 1.0);
    sg.draw_image_tc(Some(&grad), 0.0, 0.0, 96.0, 96.0, -1.0, -1.0, 2.0, 2.0);
    sg.draw_image_tc(Some(&grad_c), 100.0, 0.0, 96.0, 96.0, -1.0, -1.0, 2.0, 2.0);
    sg.draw_image_tc(Some(&checker), 200.0, 0.0, 96.0, 96.0, -0.5, -0.5, 1.5, 1.5);
    sg.draw_image_tc(
        Some(&checker_c),
        200.0,
        100.0,
        96.0,
        56.0,
        -0.5,
        -0.5,
        1.5,
        1.5,
    );
    sg.draw_image_tc(Some(&grad), 0.0, 100.0, 48.0, 48.0, 1.0, 0.0, 0.0, 1.0);
    sg.draw_image_tc(Some(&grad), 50.0, 100.0, 48.0, 48.0, 0.0, 1.0, 1.0, 0.0);
    sg.draw_image_tc(Some(&grad), 100.0, 100.0, 48.0, 48.0, 1.0, 1.0, 0.0, 0.0);
    sg.draw_image_tc(
        Some(&grad),
        150.0,
        100.0,
        40.0,
        40.0,
        0.25,
        0.25,
        0.75,
        0.75,
    );
    sg.draw_image(Some(&mip), 0.0, 160.0, 16.0, 16.0);
    sg.draw_image(Some(&mip), 20.0, 160.0, 24.0, 24.0);
    sg.draw_image(Some(&mip), 50.0, 160.0, 64.0, 64.0);
    sg.draw_image_tc(Some(&mip), 120.0, 160.0, 40.0, 40.0, 0.0, 0.0, 2.0, 2.0);
    sg.draw_image(Some(&nomip), 170.0, 160.0, 32.0, 32.0);
    sg.draw_image(Some(&mip), 210.0, 160.0, 8.0, 8.0);
    sg.draw_image(Some(&mip), 230.0, 160.0, 2.0, 2.0);
    sg.end_frame()
}

/// 4. Viewports (at DPI 1.25): quads clipped to nested viewports, a layer switch re-emitting the
///    viewport, partly and fully off-screen viewports, `height == 0` (full screen at x, y),
///    and a reset.
fn frame_viewports(sg: &mut Sg) -> Frame {
    let grad = load(sg, "grad16.png", ImageFlags::empty());
    sg.begin_frame(screen(400, 300, 1.25));
    sg.set_draw_color(0.25, 0.25, 0.25);
    sg.draw_image(None, 0.0, 0.0, 320.0, 240.0);
    sg.set_viewport(10.0, 10.0, 150.0, 100.0);
    sg.set_draw_color(0.8, 0.2, 0.2);
    sg.draw_image(None, -20.0, 5.0, 220.0, 20.0);
    sg.draw_image(None, 100.0, -30.0, 20.0, 200.0);
    sg.set_viewport(40.0, 40.0, 60.0, 30.0);
    sg.set_draw_color(1.0, 1.0, 1.0);
    sg.draw_image(Some(&grad), -10.0, -10.0, 90.0, 60.0);
    sg.set_draw_layer(Some(2), None);
    sg.set_draw_color_a(0.2, 0.2, 1.0, 0.6);
    sg.draw_image(None, 10.0, -5.0, 30.0, 50.0);
    sg.set_draw_layer(Some(0), None);
    sg.set_viewport(-30.0, 150.0, 100.0, 50.0);
    sg.set_draw_color(0.2, 0.9, 0.3);
    sg.draw_image(None, 0.0, 0.0, 200.0, 50.0);
    sg.set_viewport(280.0, 200.0, 100.0, 100.0);
    sg.set_draw_color(0.9, 0.9, 0.2);
    sg.draw_image(Some(&grad), 0.0, 0.0, 100.0, 100.0);
    sg.set_viewport(170.0, 20.0, 30.0, 0.0);
    sg.set_draw_color(0.9, 0.4, 0.9);
    sg.draw_image(None, 0.0, 0.0, 20.0, 20.0);
    sg.draw_image(None, 130.0, 200.0, 40.0, 40.0);
    sg.set_viewport(500.0, 500.0, 10.0, 10.0);
    sg.set_draw_color(1.0, 1.0, 1.0);
    sg.draw_image(None, -600.0, -600.0, 1000.0, 1000.0);
    sg.set_viewport(200.3, 100.6, 50.5, 40.4);
    sg.set_draw_color(0.3, 0.8, 0.9);
    sg.draw_image_quad(None, rot(25.0, 20.0, 80.0, 20.0, 35.0));
    sg.reset_viewport();
    sg.set_draw_color_a(1.0, 1.0, 1.0, 0.3);
    sg.draw_image(None, 150.0, 210.0, 100.0, 20.0);
    sg.end_frame()
}

/// 5. Layered alpha: layers and sub-layers submitted out of order, overlapping translucent
///    quads in submission order, a translucent texture over them.
fn frame_layers(sg: &mut Sg) -> Frame {
    let alpha = load(sg, "alpha16.png", ImageFlags::empty());
    let grad = load(sg, "grad16.png", ImageFlags::CLAMP);
    sg.begin_frame(screen(320, 240, 1.0));
    sg.set_draw_layer(Some(5), None);
    sg.set_draw_color_a(1.0, 0.0, 0.0, 0.5);
    sg.draw_image(None, 20.0, 20.0, 150.0, 100.0);
    sg.set_draw_layer(Some(-1), None);
    sg.set_draw_color(0.1, 0.2, 0.6);
    sg.draw_image(None, 0.0, 0.0, 320.0, 240.0);
    sg.set_draw_layer(Some(0), Some(3));
    sg.set_draw_color_a(0.0, 1.0, 0.0, 0.4);
    sg.draw_image(None, 60.0, 60.0, 150.0, 100.0);
    sg.set_draw_layer(None, Some(-2));
    sg.set_draw_color_a(1.0, 1.0, 0.0, 0.7);
    sg.draw_image(None, 100.0, 40.0, 150.0, 150.0);
    sg.set_draw_layer(Some(5), Some(1));
    sg.set_draw_color_a(1.0, 1.0, 1.0, 0.8);
    sg.draw_image(Some(&alpha), 120.0, 80.0, 96.0, 96.0);
    sg.set_draw_layer(Some(0), None);
    for i in 0..5 {
        sg.set_draw_color_a(0.2 * i as Num, 0.5, 1.0 - 0.2 * i as Num, 0.3);
        sg.draw_image(
            None,
            180.0 + i as Num * 12.0,
            120.0 + i as Num * 10.0,
            80.0,
            60.0,
        );
    }
    sg.set_draw_layer(Some(1), None);
    sg.set_draw_color_a(1.0, 1.0, 1.0, 0.5);
    sg.draw_image(Some(&grad), 10.0, 150.0, 80.0, 80.0);
    sg.set_draw_color_a(0.5, 0.5, 1.0, 0.5);
    sg.draw_image(Some(&alpha), 40.0, 170.0, 64.0, 64.0);
    sg.draw_image(Some(&alpha), 60.0, 180.0, 64.0, 64.0);
    sg.end_frame()
}

/// 6. Text: every face, atlas heights at scale 1 and between atlases, all alignments, colour
///    escapes (leaking into later draws), tabs, newlines, tofu, translucent text.
fn frame_text(sg: &mut Sg, w: i32, h: i32, dpi: f32) -> Frame {
    sg.begin_frame(screen(w, h, dpi));
    let (vw, _) = sg.get_virtual_screen_size();
    sg.set_draw_color(0.05, 0.05, 0.08);
    sg.draw_image(None, 0.0, 0.0, 480.0, 240.0);
    sg.set_draw_color(1.0, 1.0, 1.0);
    let mut y = 2.0;
    for (i, f) in Font::ALL.into_iter().enumerate() {
        let height = [10.0, 12.0, 14.0, 16.0, 18.0, 20.0, 24.0][i];
        sg.draw_string(
            4.0,
            y,
            Align::Left,
            height,
            f,
            "The quick ^1brown ^xFF8020fox^7 jumps over 0123456789",
        );
        y += height + 2.0;
    }
    sg.draw_string(0.0, y, Align::Center, 16.0, Font::Var, "^2Centred^7 line");
    sg.draw_string(0.0, y, Align::Right, 16.0, Font::VarBold, "right ^3aligned");
    y += 18.0;
    sg.draw_string(
        vw / 2.0,
        y,
        Align::CenterX,
        14.0,
        Font::Fixed,
        "CENTER_X\tTab",
    );
    sg.draw_string(vw - 4.0, y, Align::RightX, 14.0, Font::Fontin, "RIGHT_X");
    y += 16.0;
    sg.draw_string(4.0, y, Align::Left, 30.0, Font::FontinSc, "Height 30");
    sg.draw_string(200.0, y, Align::Left, 34.0, Font::Var, "h34 ^xC0C0FFg");
    y += 36.0;
    sg.draw_string(
        4.0,
        y,
        Align::Left,
        11.0,
        Font::Var,
        "Two\nlines (11 -> 12)",
    );
    sg.set_draw_color_a(1.0, 1.0, 1.0, 0.6);
    sg.draw_string(
        160.0,
        y,
        Align::Left,
        15.0,
        Font::Var,
        "Translucent \u{00dc}n\u{00ef}c\u{00f8}de \u{2713}",
    );
    sg.draw_string(330.0, y - 30.0, Align::Left, 66.0, Font::FontinItalic, "Ag");
    // The escape colour leaked into the draw colour: this quad is drawn in it.
    sg.draw_image(None, 440.0, 200.0, 30.0, 30.0);
    sg.end_frame()
}

// ---------------------------------------------------------------------------------------------
// Harness.

struct Gpu {
    device: wgpu::Device,
    queue: wgpu::Queue,
    renderer: SgRenderer,
    backend: String,
}

fn gpu() -> Option<Gpu> {
    let instance =
        wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle_from_env());
    match pollster::block_on(request_device(&instance, None)) {
        Ok((a, device, queue)) => {
            let info = a.get_info();
            eprintln!(
                "T5 adapter: {} ({:?}, driver {} {})",
                info.name, info.backend, info.driver, info.driver_info
            );
            let renderer = SgRenderer::new(&device, &a.limits(), RT_FORMAT);
            Some(Gpu {
                device,
                queue,
                renderer,
                backend: format!("{:?}", info.backend).to_lowercase(),
            })
        }
        Err(e) => {
            assert!(
                allow_skip(),
                "T5: no GPU adapter ({e}); POB_T5_ALLOW_SKIP=1 skips"
            );
            eprintln!("SKIP: no GPU adapter: {e}");
            None
        }
    }
}

/// Copies a texture into tightly packed RGBA8.
fn read_texture(g: &Gpu, tex: &wgpu::Texture) -> Vec<u8> {
    let (w, h) = (tex.width(), tex.height());
    let row = 4 * w;
    let padded =
        row.div_ceil(wgpu::COPY_BYTES_PER_ROW_ALIGNMENT) * wgpu::COPY_BYTES_PER_ROW_ALIGNMENT;
    let buf = g.device.create_buffer(&wgpu::BufferDescriptor {
        label: None,
        size: u64::from(padded) * u64::from(h),
        usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
        mapped_at_creation: false,
    });
    let mut enc = g
        .device
        .create_command_encoder(&wgpu::CommandEncoderDescriptor { label: None });
    enc.copy_texture_to_buffer(
        wgpu::TexelCopyTextureInfo {
            texture: tex,
            mip_level: 0,
            origin: wgpu::Origin3d::ZERO,
            aspect: wgpu::TextureAspect::All,
        },
        wgpu::TexelCopyBufferInfo {
            buffer: &buf,
            layout: wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(padded),
                rows_per_image: Some(h),
            },
        },
        wgpu::Extent3d {
            width: w,
            height: h,
            depth_or_array_layers: 1,
        },
    );
    g.queue.submit(Some(enc.finish()));
    let slice = buf.slice(..);
    slice.map_async(wgpu::MapMode::Read, |_| {});
    let _ = g.device.poll(wgpu::PollType::wait_indefinitely());
    let mut out = Vec::with_capacity((row * h) as usize);
    {
        let data = slice.get_mapped_range().expect("map");
        for y in 0..h {
            let s = (y * padded) as usize;
            out.extend_from_slice(&data[s..s + row as usize]);
        }
    }
    buf.unmap();
    out
}

/// Renders on the GPU: (RT read-back, blitted output read-back).
fn render_gpu(g: &mut Gpu, frame: &Frame) -> (Vec<u8>, Vec<u8>) {
    g.renderer.prepare(&g.device, &g.queue, frame);
    let out = g.device.create_texture(&wgpu::TextureDescriptor {
        label: Some("t5 output"),
        size: wgpu::Extent3d {
            width: frame.screen.fb_w as u32,
            height: frame.screen.fb_h as u32,
            depth_or_array_layers: 1,
        },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: RT_FORMAT,
        usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
        view_formats: &[],
    });
    let view = out.create_view(&wgpu::TextureViewDescriptor::default());
    let mut enc = g
        .device
        .create_command_encoder(&wgpu::CommandEncoderDescriptor { label: None });
    g.renderer.render(&mut enc, &view);
    g.queue.submit(Some(enc.finish()));
    let rt = g.renderer.read_back(&g.device, &g.queue).2;
    let blit = read_texture(g, &out);
    (rt, blit)
}

#[derive(Debug)]
struct Stats {
    total: usize,
    within: usize,
    exact: usize,
    max_diff: u8,
    lit: usize,
}

fn compare(gpu: &[u8], cpu: &[u8]) -> Stats {
    assert_eq!(gpu.len(), cpu.len());
    let mut s = Stats {
        total: gpu.len() / 4,
        within: 0,
        exact: 0,
        max_diff: 0,
        lit: 0,
    };
    for (a, b) in gpu.chunks_exact(4).zip(cpu.chunks_exact(4)) {
        let d = (0..4).map(|i| a[i].abs_diff(b[i])).max().unwrap_or(0);
        s.max_diff = s.max_diff.max(d);
        if d <= TOLERANCE {
            s.within += 1;
        }
        if d == 0 {
            s.exact += 1;
        }
        if b[..3].iter().any(|&c| c > 8) {
            s.lit += 1;
        }
    }
    s
}

fn write_png(path: &std::path::Path, w: u32, h: u32, px: &[u8]) {
    let img = image::RgbaImage::from_raw(w, h, px.to_vec()).expect("size");
    let _ = img.save(path);
}

/// Renders `frame` on both sides and applies the T5 gate.
fn t5(name: &str, frame: &Frame, min_lit: f64) {
    let Some(mut g) = gpu() else { return };
    let (gpu_px, blit_px) = render_gpu(&mut g, frame);
    let (w, h, cpu_px) = Raster::new().render(frame);
    assert_eq!((w, h), (frame.screen.fb_w as u32, frame.screen.fb_h as u32));
    let s = compare(&gpu_px, &cpu_px);
    let dir = PathBuf::from(env!("CARGO_TARGET_TMPDIR"))
        .join("t5")
        .join(&g.backend);
    let _ = std::fs::create_dir_all(&dir);
    write_png(&dir.join(format!("{name}_gpu.png")), w, h, &gpu_px);
    write_png(&dir.join(format!("{name}_cpu.png")), w, h, &cpu_px);
    let diff: Vec<u8> = gpu_px
        .chunks_exact(4)
        .zip(cpu_px.chunks_exact(4))
        .flat_map(|(a, b)| {
            let d = (0..3).map(|i| a[i].abs_diff(b[i])).max().unwrap_or(0);
            let v = if d > TOLERANCE {
                255
            } else {
                d.saturating_mul(60)
            };
            [
                v,
                if d > TOLERANCE { 0 } else { v },
                if d > TOLERANCE { 0 } else { v },
                255,
            ]
        })
        .collect();
    write_png(&dir.join(format!("{name}_diff.png")), w, h, &diff);
    let frac = s.within as f64 / s.total as f64;
    eprintln!(
        "T5 {name} [{}] {w}x{h}: {}/{} within ±{TOLERANCE} ({:.4} %), {} exact, max diff {}, {} lit; pngs in {}",
        g.backend,
        s.within,
        s.total,
        frac * 100.0,
        s.exact,
        s.max_diff,
        s.lit,
        dir.display()
    );
    assert!(
        frac >= MIN_FRACTION,
        "T5 {name}: only {:.4} % of pixels within ±{TOLERANCE}/255 (gate {} %): {s:?}",
        frac * 100.0,
        MIN_FRACTION * 100.0
    );
    assert!(
        s.lit as f64 >= min_lit * s.total as f64,
        "T5 {name}: frame is nearly empty ({s:?}); the comparison would be vacuous"
    );
    // The blit copies the target's RGB with alpha 1 (r_main.cpp:857-870), pixel for pixel.
    assert!(
        gpu_px == blit_px,
        "T5 {name}: blitted output differs from the render target"
    );
}

#[test]
fn t5_tinted_quads() {
    let mut sg = new_sg(false).unwrap();
    let f = frame_tinted(&mut sg);
    t5("1_tinted", &f, 0.5);
}

#[test]
fn t5_rotated_quads() {
    let mut sg = new_sg(false).unwrap();
    let f = frame_rotated(&mut sg);
    t5("2_rotated", &f, 0.5);
}

#[test]
fn t5_uvs_repeat_clamp_flip_mips() {
    let mut sg = new_sg(false).unwrap();
    let f = frame_uvs(&mut sg);
    t5("3_uvs", &f, 0.3);
}

#[test]
fn t5_nested_viewports_and_scissor() {
    let mut sg = new_sg(false).unwrap();
    let f = frame_viewports(&mut sg);
    t5("4_viewports", &f, 0.5);
}

#[test]
fn t5_layered_alpha() {
    let mut sg = new_sg(false).unwrap();
    let f = frame_layers(&mut sg);
    t5("5_layers", &f, 0.5);
}

#[test]
fn t5_text() {
    let Some(mut sg) = new_sg(true) else { return };
    let f = frame_text(&mut sg, 480, 240, 1.0);
    t5("6_text", &f, 0.5);
}

#[test]
fn t5_text_dpi_1_5() {
    let Some(mut sg) = new_sg(true) else { return };
    let f = frame_text(&mut sg, 720, 360, 1.5);
    t5("6_text_dpi1.5", &f, 0.5);
}
