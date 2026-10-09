//! CPU reference rasteriser (ui doc §3 `raster.rs`, §11.1 T5): `Frame -> RGBA`. Feature `raster`.
//! Owner: U6.
//!
//! Port of what PathOfBuilding-SimpleGraphic `3b1a346` does with a frame on the GPU, written as
//! a software rasteriser so that `pob-sg-wgpu` can be checked against it (T5) and the oracle can
//! draw PNGs without a GPU (ui doc §11.2 `--png`):
//! - `r_main.cpp:1415-1443` (`EndFrame`): the render target is cleared to opaque black, then
//!   the sorted layers render in order (`r_layer_c::Render`, `:678-719`);
//! - `r_main.cpp:468-569` (`AdjacentMergeStrategy::ProcessCommand`): `VIEWPORT`/`BIND`/`COLOR`
//!   latch state, a `QUAD` is first culled against its viewport (`r_drawCull` = 1, `:501-509`;
//!   [`Quad::is_culled`]) and then becomes the triangles `0,1,2 / 0,2,3` of its TL,TR,BR,BL corners
//!   (`:556-564`) offset by the viewport origin (vertex shader `a_vertex + a_viewport.xy`, `:810`);
//! - `r_main.cpp:816-843` (fragment shader): fragments whose centre lies outside
//!   `[vp.x, vp.x+w) x [vp.y, vp.y+h)` are discarded (here: a scissor rectangle, which selects
//!   the same pixel centres for integer viewports), `colour = texture(uv) * tint`;
//! - `r_main.cpp:602-609`, `:1202` (`BeginFrame` latches `RB_ALPHA`): blend
//!   `SRC_ALPHA, ONE_MINUS_SRC_ALPHA` for colour and alpha, in gamma space (the target is
//!   `GL_RGB`/UNORM, `:1178`, ui doc D12);
//! - `r_texture.cpp:340-347` (`r_tex_c::Bind`): a texture that is not uploaded binds `@black`
//!   (all zero, transparent);
//! - `r_texture.cpp:620-673` (`r_tex_c::Upload`): min filter `LINEAR`, `LINEAR_MIPMAP_LINEAR`
//!   with mips; mag filter `NEAREST` with `TF_NEAREST`, else `LINEAR`; wrap `CLAMP_TO_EDGE` with
//!   `TF_CLAMP`, else `REPEAT`.
//!
//! The rasterisation rules are the GL/Vulkan ones the GPU applies: vertex positions snap to a
//! 1/256 px grid ([`SUBPIXEL_BITS`]), a pixel is covered when its centre is inside the triangle,
//! ties on an edge go to top and left edges only (no double hits on the shared diagonal),
//! attributes interpolate linearly (w = 1), the level of detail comes from the screen-space UV
//! derivatives (exact per triangle, as the GPU's 2x2 differences are for an affine mapping),
//! `λ <= 0` magnifies, `λ > 0` minifies (trilinear between mip levels), and every blend reads
//! and writes the 8-bit target.
//!
//! DIVERGENCE(anisotropy): SG asks for 16x anisotropic filtering (`r_texture.cpp:657-663`); the
//! rasteriser filters isotropically (the T5 frames keep minification isotropic, where both agree).
//! DIVERGENCE(max texture size): textures larger than the GPU limit are downscaled by the
//! backend (`pob-sg-wgpu` `fit_texture`); the rasteriser has no limit.

use std::collections::HashMap;
use std::rc::Rc;

use crate::frame::{Cmd, Frame, Quad};
use crate::image::{ImageData, ImageFlags, TexId};
use crate::types::IRect;

/// Sub-pixel precision of the vertex snap (GL/Vulkan implementations use 4-8 bits; 8 here).
pub const SUBPIXEL_BITS: u32 = 8;
const SUB: i64 = 1 << SUBPIXEL_BITS;

/// Textures the rasteriser has seen (fed from `Frame::uploads`, pruned by `Frame::releases`).
#[derive(Default)]
pub struct Raster {
    pub textures: HashMap<TexId, Rc<ImageData>>,
}

/// The 8-bit RGBA render target (alpha is blended like the GPU's, then forced to 255 on output:
/// SG's target is RGB).
struct Target {
    w: u32,
    h: u32,
    px: Vec<u8>,
}

/// One triangle vertex: position (physical px, viewport offset applied), texture coordinate.
#[derive(Clone, Copy)]
struct Vert {
    x: f32,
    y: f32,
    s: f32,
    t: f32,
}

/// Pixel rectangle `[x0, x1) x [y0, y1)`.
#[derive(Clone, Copy)]
struct Scissor {
    x0: i64,
    y0: i64,
    x1: i64,
    y1: i64,
}

impl Raster {
    pub fn new() -> Raster {
        Raster::default()
    }

    /// Apply the frame's uploads/releases, then render it: `(width, height, RGBA8)` at framebuffer size.
    /// sg: r_main.cpp:1415-1443 (EndFrame: clear, layers in order), :468-569 (ProcessCommand).
    pub fn render(&mut self, frame: &Frame) -> (u32, u32, Vec<u8>) {
        for id in &frame.releases {
            self.textures.remove(id);
        }
        for (id, data) in &frame.uploads {
            self.textures.insert(*id, data.clone());
        }
        let (w, h) = (
            frame.screen.fb_w.max(1) as u32,
            frame.screen.fb_h.max(1) as u32,
        );
        // glClear with glClearColor(0, 0, 0, 1) (r_main.cpp:906, :1417).
        let mut target = Target {
            w,
            h,
            px: [0u8, 0, 0, 255].repeat((w * h) as usize),
        };
        let full = IRect {
            x: 0,
            y: 0,
            w: frame.screen.fb_w,
            h: frame.screen.fb_h,
        };
        for q in &frame.layers {
            // A fresh AdjacentMergeStrategy per layer (r_main.cpp:683); every queue starts with
            // the VIEWPORT that SetDrawLayer / BeginFrame emitted.
            let (mut vp, mut tex, mut tint) = (full, TexId::WHITE, [1.0f32; 4]);
            for c in &q.cmds {
                match c {
                    Cmd::Viewport(v) => vp = *v,
                    Cmd::Bind(t) => tex = *t,
                    Cmd::Color(c) => tint = *c,
                    Cmd::Quad(quad) => {
                        if quad.is_culled(vp) {
                            continue;
                        }
                        // A texture without pixels yet binds @black (r_texture.cpp:340-347).
                        let bound = if self.textures.contains_key(&tex) {
                            tex
                        } else {
                            TexId::BLACK
                        };
                        let Some(img) = self.textures.get(&bound) else {
                            continue;
                        };
                        let sc = Scissor {
                            x0: i64::from(vp.x.clamp(0, w as i32)),
                            y0: i64::from(vp.y.clamp(0, h as i32)),
                            x1: i64::from(vp.x.saturating_add(vp.w).clamp(0, w as i32)),
                            y1: i64::from(vp.y.saturating_add(vp.h).clamp(0, h as i32)),
                        };
                        if sc.x1 <= sc.x0 || sc.y1 <= sc.y0 {
                            continue;
                        }
                        let v = quad_verts(quad, vp);
                        target.triangle(sc, [v[0], v[1], v[2]], tint, img);
                        target.triangle(sc, [v[0], v[2], v[3]], tint, img);
                    }
                }
            }
        }
        for p in target.px.chunks_exact_mut(4) {
            p[3] = 255;
        }
        (w, h, target.px)
    }
}

/// The quad's corners offset by the viewport origin (`a_vertex + a_viewport.xy`).
/// sg: r_main.cpp:534-564 (vertices, triangles 0,1,2 / 0,2,3), :810 (vertex shader).
fn quad_verts(q: &Quad, vp: IRect) -> [Vert; 4] {
    let (ox, oy) = (vp.x as f32, vp.y as f32);
    std::array::from_fn(|i| Vert {
        x: q.x[i] + ox,
        y: q.y[i] + oy,
        s: q.s[i],
        t: q.t[i],
    })
}

/// `edge(a, b, p)`: twice the signed area of `(a, b, p)`; positive inside a triangle of positive area.
fn edge(a: (i64, i64), b: (i64, i64), p: (i64, i64)) -> i64 {
    (b.0 - a.0) * (p.1 - a.1) - (b.1 - a.1) * (p.0 - a.0)
}

/// Top-left rule (y down, positive-area orientation): a top edge runs horizontally to the
/// right, a left edge runs upwards.
fn is_top_left(a: (i64, i64), b: (i64, i64)) -> bool {
    let (dx, dy) = (b.0 - a.0, b.1 - a.1);
    dy < 0 || (dy == 0 && dx > 0)
}

/// A float position on the 1/256 px grid the rasteriser works on (GL/Vulkan sub-pixel snap).
fn snap(v: f32) -> Option<i64> {
    let f = (f64::from(v) * SUB as f64).round();
    // Far outside any target: the GPU clips; positions this large never occur in PoB.
    if f.is_finite() && f.abs() < (1u64 << 40) as f64 {
        Some(f as i64)
    } else {
        None
    }
}

impl Target {
    /// One triangle inside the scissor: coverage at pixel centres (top-left rule), linear UV
    /// interpolation, `texture * tint` clamped to [0, 1] (UNORM output), then the blend.
    /// sg: r_main.cpp:816-843 (fragment shader: viewport discard, `color * v_tint`).
    fn triangle(&mut self, sc: Scissor, v: [Vert; 3], tint: [f32; 4], img: &ImageData) {
        let snapped: Option<Vec<(i64, i64)>> =
            v.iter().map(|p| Some((snap(p.x)?, snap(p.y)?))).collect();
        let Some(mut p) = snapped else { return };
        let mut v = v;
        let mut area = edge(p[0], p[1], p[2]);
        if area == 0 {
            return;
        }
        if area < 0 {
            p.swap(1, 2);
            v.swap(1, 2);
            area = -area;
        }
        let (p0, p1, p2) = (p[0], p[1], p[2]);
        // Bounding box of the pixel centres that can be covered, clipped to the scissor.
        let min_x = p0.0.min(p1.0).min(p2.0);
        let max_x = p0.0.max(p1.0).max(p2.0);
        let min_y = p0.1.min(p1.1).min(p2.1);
        let max_y = p0.1.max(p1.1).max(p2.1);
        let half = SUB / 2;
        let bx0 = (min_x - half).div_euclid(SUB).max(sc.x0);
        let bx1 = ((max_x - half).div_euclid(SUB) + 1).min(sc.x1);
        let by0 = (min_y - half).div_euclid(SUB).max(sc.y0);
        let by1 = ((max_y - half).div_euclid(SUB) + 1).min(sc.y1);
        if bx1 <= bx0 || by1 <= by0 {
            return;
        }
        let tl = [
            is_top_left(p1, p2),
            is_top_left(p2, p0),
            is_top_left(p0, p1),
        ];
        // Screen-space derivatives of the barycentrics (per pixel), hence of (s, t).
        let fa = area as f64 / (SUB * SUB) as f64;
        let fp = |q: (i64, i64)| (q.0 as f64 / SUB as f64, q.1 as f64 / SUB as f64);
        let (q0, q1, q2) = (fp(p0), fp(p1), fp(p2));
        let dl_dx = [
            -(q2.1 - q1.1) / fa,
            -(q0.1 - q2.1) / fa,
            -(q1.1 - q0.1) / fa,
        ];
        let dl_dy = [(q2.0 - q1.0) / fa, (q0.0 - q2.0) / fa, (q1.0 - q0.0) / fa];
        let ds_dx = v[0].s as f64 * dl_dx[0] + v[1].s as f64 * dl_dx[1] + v[2].s as f64 * dl_dx[2];
        let dt_dx = v[0].t as f64 * dl_dx[0] + v[1].t as f64 * dl_dx[1] + v[2].t as f64 * dl_dx[2];
        let ds_dy = v[0].s as f64 * dl_dy[0] + v[1].s as f64 * dl_dy[1] + v[2].s as f64 * dl_dy[2];
        let dt_dy = v[0].t as f64 * dl_dy[0] + v[1].t as f64 * dl_dy[1] + v[2].t as f64 * dl_dy[2];
        let lod = lambda(img, [ds_dx, dt_dx], [ds_dy, dt_dy]);
        for py in by0..by1 {
            let cy = py * SUB + half;
            for px in bx0..bx1 {
                let c = (px * SUB + half, cy);
                let w = [edge(p1, p2, c), edge(p2, p0, c), edge(p0, p1, c)];
                if (0..3).any(|i| w[i] < 0 || (w[i] == 0 && !tl[i])) {
                    continue;
                }
                let l = w.map(|e| e as f64 / area as f64);
                let s = (l[0] * v[0].s as f64 + l[1] * v[1].s as f64 + l[2] * v[2].s as f64) as f32;
                let t = (l[0] * v[0].t as f64 + l[1] * v[1].t as f64 + l[2] * v[2].t as f64) as f32;
                let texel = sample(img, s, t, lod);
                let src: [f32; 4] = std::array::from_fn(|i| (texel[i] * tint[i]).clamp(0.0, 1.0));
                self.blend(px as u32, py as u32, src);
            }
        }
    }

    /// `glBlendFunc(GL_SRC_ALPHA, GL_ONE_MINUS_SRC_ALPHA)` on all four channels of a UNORM8 target.
    /// sg: r_main.cpp:607-609 (RB_ALPHA, latched by BeginFrame at :1202).
    fn blend(&mut self, x: u32, y: u32, src: [f32; 4]) {
        debug_assert!(x < self.w && y < self.h);
        let i = ((y * self.w + x) * 4) as usize;
        let a = src[3];
        for c in 0..4 {
            let d = f32::from(self.px[i + c]) / 255.0;
            let o = src[c] * a + d * (1.0 - a);
            self.px[i + c] = (o * 255.0).round().clamp(0.0, 255.0) as u8;
        }
    }
}

/// Level of detail `λ = log2(ρ)`, `ρ` the larger texel-space length of the two UV derivatives.
fn lambda(img: &ImageData, d_dx: [f64; 2], d_dy: [f64; 2]) -> f32 {
    let (w, h) = (f64::from(img.width), f64::from(img.height));
    let rx = (d_dx[0] * w).hypot(d_dx[1] * h);
    let ry = (d_dy[0] * w).hypot(d_dy[1] * h);
    let rho = rx.max(ry);
    if rho > 0.0 && rho.is_finite() {
        rho.log2() as f32
    } else {
        f32::NEG_INFINITY
    }
}

/// `texture(s_tex, uv)` with the sampler state of `r_tex_c::Upload` (r_texture.cpp:640-673).
fn sample(img: &ImageData, s: f32, t: f32, lod: f32) -> [f32; 4] {
    let nearest = img.flags.contains(ImageFlags::NEAREST);
    if lod <= 0.0 {
        // Magnification: GL_NEAREST with TF_NEAREST, else GL_LINEAR.
        return if nearest {
            fetch_nearest(img, 0, s, t)
        } else {
            fetch_linear(img, 0, s, t)
        };
    }
    let levels = img.levels.len();
    if levels <= 1 {
        // GL_LINEAR minification.
        return fetch_linear(img, 0, s, t);
    }
    // GL_LINEAR_MIPMAP_LINEAR.
    let d = lod.min((levels - 1) as f32);
    let l0 = d.floor() as usize;
    let f = d - l0 as f32;
    let a = fetch_linear(img, l0, s, t);
    if l0 + 1 >= levels || f == 0.0 {
        return a;
    }
    let b = fetch_linear(img, l0 + 1, s, t);
    std::array::from_fn(|i| a[i] * (1.0 - f) + b[i] * f)
}

fn level_size(img: &ImageData, level: usize) -> (i64, i64) {
    (
        i64::from((img.width >> level).max(1)),
        i64::from((img.height >> level).max(1)),
    )
}

/// `GL_CLAMP_TO_EDGE` with `TF_CLAMP`, else `GL_REPEAT` (r_texture.cpp:666-673).
fn wrap(i: i64, n: i64, clamp: bool) -> i64 {
    if clamp {
        i.clamp(0, n - 1)
    } else {
        i.rem_euclid(n)
    }
}

fn texel(img: &ImageData, level: usize, x: i64, y: i64) -> [f32; 4] {
    let (w, _) = level_size(img, level);
    let i = ((y * w + x) * 4) as usize;
    let px = &img.levels[level];
    std::array::from_fn(|c| f32::from(px[i + c]) / 255.0)
}

/// `GL_NEAREST` at one level: texel `floor(s * w)`, wrapped.
fn fetch_nearest(img: &ImageData, level: usize, s: f32, t: f32) -> [f32; 4] {
    let clamp = img.flags.contains(ImageFlags::CLAMP);
    let (w, h) = level_size(img, level);
    let x = wrap((s * w as f32).floor() as i64, w, clamp);
    let y = wrap((t * h as f32).floor() as i64, h, clamp);
    texel(img, level, x, y)
}

/// `GL_LINEAR` at one level: the 2x2 texels around `s * w - 0.5`, wrapped, bilinear weights.
fn fetch_linear(img: &ImageData, level: usize, s: f32, t: f32) -> [f32; 4] {
    let clamp = img.flags.contains(ImageFlags::CLAMP);
    let (w, h) = level_size(img, level);
    let u = s * w as f32 - 0.5;
    let v = t * h as f32 - 0.5;
    let (fu, fv) = (u.floor(), v.floor());
    let (a, b) = (u - fu, v - fv);
    let (i0, j0) = (fu as i64, fv as i64);
    let (x0, x1) = (wrap(i0, w, clamp), wrap(i0 + 1, w, clamp));
    let (y0, y1) = (wrap(j0, h, clamp), wrap(j0 + 1, h, clamp));
    let t00 = texel(img, level, x0, y0);
    let t10 = texel(img, level, x1, y0);
    let t01 = texel(img, level, x0, y1);
    let t11 = texel(img, level, x1, y1);
    std::array::from_fn(|c| {
        (1.0 - a) * (1.0 - b) * t00[c]
            + a * (1.0 - b) * t10[c]
            + (1.0 - a) * b * t01[c]
            + a * b * t11[c]
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::frame::LayerQueue;
    use crate::types::ScreenInfo;

    fn screen(w: i32, h: i32) -> ScreenInfo {
        ScreenInfo {
            fb_w: w,
            fb_h: h,
            win_h: h,
            dpi_scale: 1.0,
        }
    }

    fn rect(x0: f32, y0: f32, x1: f32, y1: f32) -> Quad {
        Quad {
            s: [0.0, 1.0, 1.0, 0.0],
            t: [0.0, 0.0, 1.0, 1.0],
            x: [x0, x1, x1, x0],
            y: [y0, y0, y1, y1],
        }
    }

    fn white() -> Rc<ImageData> {
        Rc::new(ImageData {
            width: 8,
            height: 8,
            levels: vec![vec![255; 256]],
            flags: ImageFlags::empty(),
        })
    }

    fn frame(
        w: i32,
        h: i32,
        layers: Vec<LayerQueue>,
        uploads: Vec<(TexId, Rc<ImageData>)>,
    ) -> Frame {
        Frame {
            screen: screen(w, h),
            layers,
            uploads,
            releases: Vec::new(),
            digest: 0,
        }
    }

    fn px(img: &[u8], w: u32, x: u32, y: u32) -> [u8; 4] {
        let i = ((y * w + x) * 4) as usize;
        [img[i], img[i + 1], img[i + 2], img[i + 3]]
    }

    fn full(w: i32, h: i32) -> Cmd {
        Cmd::Viewport(IRect { x: 0, y: 0, w, h })
    }

    #[test]
    fn clear_is_opaque_black() {
        let (w, h, img) = Raster::new().render(&frame(3, 2, Vec::new(), Vec::new()));
        assert_eq!((w, h), (3, 2));
        assert!(img.chunks_exact(4).all(|p| p == [0, 0, 0, 255]));
    }

    #[test]
    fn gamma_blend_and_layer_order() {
        let mut l0 = LayerQueue::new(0, 0);
        l0.cmds = vec![
            full(4, 4),
            Cmd::Bind(TexId::WHITE),
            Cmd::Color([1.0, 1.0, 1.0, 1.0]),
            Cmd::Quad(rect(0.0, 0.0, 2.0, 4.0)),
        ];
        let mut l1 = LayerQueue::new(1, 0);
        l1.cmds = vec![
            full(4, 4),
            Cmd::Bind(TexId::WHITE),
            Cmd::Color([1.0, 0.0, 0.0, 0.5]),
            Cmd::Quad(rect(0.0, 0.0, 4.0, 1.0)),
        ];
        let f = frame(4, 4, vec![l0, l1], vec![(TexId::WHITE, white())]);
        let (w, _, img) = Raster::new().render(&f);
        // 50 % red over white: 255, 127.5 -> 128 (round half away), 128.
        assert_eq!(px(&img, w, 0, 0), [255, 128, 128, 255]);
        // 50 % red over the black clear.
        assert_eq!(px(&img, w, 3, 0), [128, 0, 0, 255]);
        assert_eq!(px(&img, w, 1, 2), [255, 255, 255, 255]);
        assert_eq!(px(&img, w, 2, 2), [0, 0, 0, 255]);
    }

    #[test]
    fn viewport_offsets_and_scissors() {
        let mut l0 = LayerQueue::new(0, 0);
        l0.cmds = vec![
            Cmd::Viewport(IRect {
                x: 2,
                y: 1,
                w: 2,
                h: 2,
            }),
            Cmd::Bind(TexId::WHITE),
            Cmd::Color([0.0, 1.0, 0.0, 1.0]),
            Cmd::Quad(rect(-5.0, 0.0, 10.0, 1.0)),
        ];
        let f = frame(6, 4, vec![l0], vec![(TexId::WHITE, white())]);
        let (w, _, img) = Raster::new().render(&f);
        for x in 0..6 {
            let want = if (2..4).contains(&x) {
                [0, 255, 0, 255]
            } else {
                [0, 0, 0, 255]
            };
            assert_eq!(px(&img, w, x, 1), want, "x={x}");
            assert_eq!(px(&img, w, x, 2), [0, 0, 0, 255]);
        }
    }

    #[test]
    fn unknown_texture_binds_transparent_black() {
        let mut l0 = LayerQueue::new(0, 0);
        l0.cmds = vec![
            full(2, 2),
            Cmd::Bind(TexId::WHITE),
            Cmd::Color([1.0; 4]),
            Cmd::Quad(rect(0.0, 0.0, 2.0, 2.0)),
            Cmd::Bind(TexId(99)),
            Cmd::Quad(rect(0.0, 0.0, 2.0, 2.0)),
        ];
        let black = Rc::new(ImageData {
            width: 8,
            height: 8,
            levels: vec![vec![0; 256]],
            flags: ImageFlags::empty(),
        });
        let f = frame(
            2,
            2,
            vec![l0],
            vec![(TexId::WHITE, white()), (TexId::BLACK, black)],
        );
        let (_, _, img) = Raster::new().render(&f);
        assert!(img.chunks_exact(4).all(|p| p == [255, 255, 255, 255]));
    }

    #[test]
    fn shared_diagonal_is_hit_once() {
        // A 25 % white quad over black: a pixel hit twice on the diagonal would read 112, not 64.
        let mut l0 = LayerQueue::new(0, 0);
        l0.cmds = vec![
            full(16, 16),
            Cmd::Bind(TexId::WHITE),
            Cmd::Color([1.0, 1.0, 1.0, 0.25]),
            Cmd::Quad(rect(0.0, 0.0, 16.0, 16.0)),
        ];
        let f = frame(16, 16, vec![l0], vec![(TexId::WHITE, white())]);
        let (_, _, img) = Raster::new().render(&f);
        assert!(img.chunks_exact(4).all(|p| p == [64, 64, 64, 255]));
    }

    #[test]
    fn half_pixel_edges_follow_top_left_rule() {
        // Edges through pixel centres: x 0.5..2.5 covers centres 0.5 (left edge, in) and 1.5,
        // not 2.5 (right edge, out); likewise y.
        let mut l0 = LayerQueue::new(0, 0);
        l0.cmds = vec![
            full(4, 4),
            Cmd::Bind(TexId::WHITE),
            Cmd::Color([1.0; 4]),
            Cmd::Quad(rect(0.5, 0.5, 2.5, 2.5)),
        ];
        let f = frame(4, 4, vec![l0], vec![(TexId::WHITE, white())]);
        let (w, _, img) = Raster::new().render(&f);
        for y in 0..4 {
            for x in 0..4 {
                let lit = x < 2 && y < 2;
                assert_eq!(px(&img, w, x, y)[0] == 255, lit, "({x},{y})");
            }
        }
    }

    #[test]
    fn sampling_repeat_clamp_nearest_linear_and_mips() {
        // 2x1: red | blue.
        let tex = |flags| {
            Rc::new(ImageData {
                width: 2,
                height: 1,
                levels: vec![vec![255, 0, 0, 255, 0, 0, 255, 255]],
                flags,
            })
        };
        let near = tex(ImageFlags::NEAREST);
        assert_eq!(fetch_nearest(&near, 0, 0.25, 0.5), [1.0, 0.0, 0.0, 1.0]);
        assert_eq!(fetch_nearest(&near, 0, 1.75, 0.5), [0.0, 0.0, 1.0, 1.0]);
        assert_eq!(fetch_nearest(&near, 0, -0.25, 0.5), [0.0, 0.0, 1.0, 1.0]);
        // Linear at s = 0 sits between texel 1 (wrapped) and texel 0 with REPEAT ...
        let lin = tex(ImageFlags::empty());
        assert_eq!(fetch_linear(&lin, 0, 0.0, 0.5), [0.5, 0.0, 0.5, 1.0]);
        // ... and on texel 0 with CLAMP.
        let cl = tex(ImageFlags::CLAMP);
        assert_eq!(fetch_linear(&cl, 0, 0.0, 0.5), [1.0, 0.0, 0.0, 1.0]);
        assert_eq!(fetch_linear(&cl, 0, 5.0, 0.5), [0.0, 0.0, 1.0, 1.0]);
        // Mips: level 1 is a single texel; λ = 1 reads it, λ = 0.5 blends halfway.
        let mip = Rc::new(ImageData {
            width: 2,
            height: 1,
            levels: vec![vec![255, 0, 0, 255, 255, 0, 0, 255], vec![0, 0, 255, 255]],
            flags: ImageFlags::MIPMAP,
        });
        assert_eq!(sample(&mip, 0.25, 0.5, 1.0), [0.0, 0.0, 1.0, 1.0]);
        assert_eq!(sample(&mip, 0.25, 0.5, 0.5), [0.5, 0.0, 0.5, 1.0]);
        assert_eq!(sample(&mip, 0.25, 0.5, 9.0), [0.0, 0.0, 1.0, 1.0]);
        // NEAREST only magnifies; minification is linear.
        assert_eq!(sample(&near, 0.5, 0.5, -1.0), [0.0, 0.0, 1.0, 1.0]);
        assert_eq!(sample(&near, 0.5, 0.5, 1.0), [0.5, 0.0, 0.5, 1.0]);
    }

    #[test]
    fn culled_quads_draw_nothing_and_releases_drop_textures() {
        let mut l0 = LayerQueue::new(0, 0);
        l0.cmds = vec![
            full(2, 2),
            Cmd::Bind(TexId::WHITE),
            Cmd::Color([1.0; 4]),
            Cmd::Quad(rect(2.0, 0.0, 4.0, 2.0)),
        ];
        let mut r = Raster::new();
        let f = frame(2, 2, vec![l0.clone()], vec![(TexId::WHITE, white())]);
        let (_, _, img) = r.render(&f);
        assert!(img.chunks_exact(4).all(|p| p == [0, 0, 0, 255]));
        assert!(r.textures.contains_key(&TexId::WHITE));
        let mut f = frame(2, 2, vec![l0], Vec::new());
        f.releases.push(TexId::WHITE);
        r.render(&f);
        assert!(r.textures.is_empty());
    }

    #[test]
    fn rotated_textured_quad_interpolates_uvs() {
        // A 4x4 NEAREST checker drawn as a 45° diamond 8x magnified: the centre pixel reads the
        // texel at uv (0.5, 0.5) and the corners stay black.
        let mut px4 = Vec::new();
        for y in 0..4 {
            for x in 0..4 {
                let v = if (x + y) % 2 == 0 { 255 } else { 0 };
                px4.extend_from_slice(&[v, v, v, 255]);
            }
        }
        let tex = Rc::new(ImageData {
            width: 4,
            height: 4,
            levels: vec![px4],
            flags: ImageFlags::NEAREST,
        });
        let mut l0 = LayerQueue::new(0, 0);
        l0.cmds = vec![
            full(64, 64),
            Cmd::Bind(TexId(3)),
            Cmd::Color([1.0; 4]),
            Cmd::Quad(Quad {
                s: [0.0, 1.0, 1.0, 0.0],
                t: [0.0, 0.0, 1.0, 1.0],
                x: [32.0, 63.0, 32.0, 1.0],
                y: [1.0, 32.0, 63.0, 32.0],
            }),
        ];
        let f = frame(64, 64, vec![l0], vec![(TexId(3), tex)]);
        let (w, _, img) = Raster::new().render(&f);
        assert_eq!(px(&img, w, 0, 0), [0, 0, 0, 255]);
        assert_eq!(px(&img, w, 63, 63), [0, 0, 0, 255]);
        // Just below the top corner: uv ~ (0.03, 0.03): texel (0,0) = white.
        assert_eq!(px(&img, w, 32, 4), [255, 255, 255, 255]);
        // Just right of the top corner along the TL->TR edge: uv ~ (0.3, 0.0): texel (1,0) = black.
        assert_eq!(px(&img, w, 42, 12), [0, 0, 0, 255]);
    }
}
