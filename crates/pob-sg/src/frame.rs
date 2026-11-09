//! Render-level command stream (ui doc §4.3): SimpleGraphic's per-`(layer, subLayer)` queues
//! of `VIEWPORT/BIND/COLOR/QUAD` commands (`r_main.cpp` `r_layer_c`). Owner: U1.
//!
//! Port of PathOfBuilding-SimpleGraphic `3b1a346` (the commit the shipped `SimpleGraphic.dll`
//! identifies as, ui doc §2): `engine/render/r_main.cpp:128-276` (`r_layerCmd_s` and the
//! `r_layer_c` command writers), `:282-319` (`AabbFromCmdQuad`/`AabbFromViewport`/
//! `AabbAabbIntersects`, the draw cull), `:1208-1224` + `:1298-1302` (`layerCompFunc`, `qsort`).
//! Differences from the research copy (master) are listed in `docs/port/ui/sg-semantics.md`.
//!
//! DIVERGENCE(blend): `3b1a346` also queues a `BLEND` command (`r_main.cpp:241-247`) on every
//! layer switch and at frame start. PoB never calls `SetBlendMode` (only `_SimpleGraphic.def.lua`
//! names it), so the mode is always `RB_ALPHA` and `Cmd` has no `Blend` variant; the backend
//! uses `SRC_ALPHA, ONE_MINUS_SRC_ALPHA` throughout (ui doc D12). `Sg` still counts the BLEND
//! command's bytes against the per-layer command storage (see `api.rs`).
//!
//! DIVERGENCE(digest): `3b1a346` elides identical frames with a MurmurHash64A digest of the raw
//! command bytes (`r_main.cpp:1385-1403`); [`digest`] is XXH3 over the typed commands. Frame
//! elision changes no pixels (ui doc §2 "Frame loop"), so only "equal frames give equal digests"
//! matters.

use crate::image::{ImageData, TexId};
use crate::types::{IRect, ScreenInfo};

/// One quad: TL, TR, BR, BL corners in physical px relative to the current viewport, with
/// their texture coordinates (`r_layerCmd_s` QUAD).
#[derive(Clone, Copy, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct Quad {
    pub s: [f32; 4],
    pub t: [f32; 4],
    pub x: [f32; 4],
    pub y: [f32; 4],
}

impl Quad {
    /// SimpleGraphic's draw culling (`AdjacentMergeStrategy::ProcessCommand`, `r_drawCull`
    /// defaults to 1): a quad whose bounding box, translated by the viewport origin, does not
    /// touch the viewport rectangle is dropped by the backend. At `3b1a346` the test is
    /// **inclusive** on both edges (`A.lo <= B.hi && A.hi >= B.lo`) and runs in screen space;
    /// master's strict `< extent && > 0` test in viewport space is not the shipped behaviour.
    /// Culling never changes pixels (a quad that only touches an edge covers no pixel inside
    /// the viewport). Moved from the pre-port `DrawCmd::is_culled`.
    ///
    /// sg: r_main.cpp:282-319 (`AabbFromCmdQuad`, `AabbFromViewport`, `AabbAabbIntersects`),
    /// :501-509 (the cull) @3b1a346
    pub fn is_culled(&self, vp: IRect) -> bool {
        // AabbFromCmdQuad: r = {{+FLT_MAX, +FLT_MAX}, {-FLT_MAX, -FLT_MAX}}; std::min/std::max
        let mut a_lo = [f32::MAX, f32::MAX];
        let mut a_hi = [-f32::MAX, -f32::MAX];
        for i in 0..4 {
            a_lo[0] = std_min(a_lo[0], self.x[i]);
            a_lo[1] = std_min(a_lo[1], self.y[i]);
            a_hi[0] = std_max(a_hi[0], self.x[i]);
            a_hi[1] = std_max(a_hi[1], self.y[i]);
        }
        a_lo[0] += vp.x as f32;
        a_lo[1] += vp.y as f32;
        a_hi[0] += vp.x as f32;
        a_hi[1] += vp.y as f32;
        // AabbFromViewport
        let b_lo = [vp.x as f32, vp.y as f32];
        let b_hi = [(vp.x + vp.w) as f32, (vp.y + vp.h) as f32];
        // AabbAabbIntersects: A.lo <= B.hi && A.hi >= B.lo
        let intersects =
            a_lo[0] <= b_hi[0] && a_lo[1] <= b_hi[1] && a_hi[0] >= b_lo[0] && a_hi[1] >= b_lo[1];
        !intersects
    }
}

/// `(std::min)(a, b)`: `(b < a) ? b : a` (keeps `a` when either is NaN).
fn std_min(a: f32, b: f32) -> f32 {
    if b < a {
        b
    } else {
        a
    }
}

/// `(std::max)(a, b)`: `(a < b) ? b : a` (keeps `a` when either is NaN).
fn std_max(a: f32, b: f32) -> f32 {
    if a < b {
        b
    } else {
        a
    }
}

/// One queued command (`r_layerCmd_s`).
#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
pub enum Cmd {
    Viewport(IRect),
    Bind(TexId),
    Color([f32; 4]),
    Quad(Quad),
}

/// A persistent `r_layer_c(layer, subLayer)`; replayed in submission order.
#[derive(Clone, Debug, PartialEq)]
pub struct LayerQueue {
    pub layer: i32,
    pub sub_layer: i32,
    pub cmds: Vec<Cmd>,
}

impl LayerQueue {
    pub fn new(layer: i32, sub_layer: i32) -> LayerQueue {
        LayerQueue {
            layer,
            sub_layer,
            cmds: Vec::new(),
        }
    }
}

/// What `Sg::end_frame` hands to a backend.
pub struct Frame {
    pub screen: ScreenInfo,
    /// Sorted by (layer, sub_layer); empty queues dropped.
    pub layers: Vec<LayerQueue>,
    /// Became ready this frame.
    pub uploads: Vec<(TexId, std::rc::Rc<ImageData>)>,
    pub releases: Vec<TexId>,
    /// XXH3 over cmds (optional frame elision).
    pub digest: u64,
}

/// Where the font layout writes (the current layer queue, or a test recorder).
pub trait QuadSink {
    fn bind(&mut self, t: TexId);
    fn color(&mut self, c: [f32; 4]);
    fn quad(&mut self, q: Quad);
}

impl QuadSink for LayerQueue {
    fn bind(&mut self, t: TexId) {
        self.cmds.push(Cmd::Bind(t));
    }
    fn color(&mut self, c: [f32; 4]) {
        self.cmds.push(Cmd::Color(c));
    }
    fn quad(&mut self, q: Quad) {
        self.cmds.push(Cmd::Quad(q));
    }
}

/// `Sg::draw_string`'s sink over the current queue (ui doc §4.2 reference body).
pub struct LayerSink<'a>(pub &'a mut LayerQueue);

impl QuadSink for LayerSink<'_> {
    fn bind(&mut self, t: TexId) {
        self.0.bind(t);
    }
    fn color(&mut self, c: [f32; 4]) {
        self.0.color(c);
    }
    fn quad(&mut self, q: Quad) {
        self.0.quad(q);
    }
}

/// XXH3-64 over the command stream of the sorted queues (frame elision, ui doc §2 "Frame loop").
pub fn digest(layers: &[LayerQueue]) -> u64 {
    let mut h = xxhash_rust::xxh3::Xxh3::new();
    for q in layers {
        h.update(&q.layer.to_le_bytes());
        h.update(&q.sub_layer.to_le_bytes());
        for c in &q.cmds {
            match c {
                Cmd::Viewport(r) => {
                    h.update(&[0]);
                    for v in [r.x, r.y, r.w, r.h] {
                        h.update(&v.to_le_bytes());
                    }
                }
                Cmd::Bind(t) => {
                    h.update(&[1]);
                    h.update(&t.0.to_le_bytes());
                }
                Cmd::Color(c) => {
                    h.update(&[2]);
                    for v in c {
                        h.update(&v.to_bits().to_le_bytes());
                    }
                }
                Cmd::Quad(q) => {
                    h.update(&[3]);
                    for a in [q.s, q.t, q.x, q.y] {
                        for v in a {
                            h.update(&v.to_bits().to_le_bytes());
                        }
                    }
                }
            }
        }
    }
    h.digest()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rect_quad(x0: f32, y0: f32, x1: f32, y1: f32) -> Quad {
        Quad {
            s: [0.0, 1.0, 1.0, 0.0],
            t: [0.0, 0.0, 1.0, 1.0],
            x: [x0, x1, x1, x0],
            y: [y0, y0, y1, y1],
        }
    }

    /// `AabbAabbIntersects` at 3b1a346 is inclusive and works in screen space.
    #[test]
    fn cull_is_inclusive_at_3b1a346() {
        let vp = IRect {
            x: 100,
            y: 50,
            w: 200,
            h: 100,
        };
        assert!(!rect_quad(10.0, 10.0, 20.0, 20.0).is_culled(vp));
        // Touching the left / top edge (max == 0) and the right / bottom edge (min == extent)
        // is kept (master culled these).
        assert!(!rect_quad(-10.0, 10.0, 0.0, 20.0).is_culled(vp));
        assert!(!rect_quad(10.0, -10.0, 20.0, 0.0).is_culled(vp));
        assert!(!rect_quad(200.0, 10.0, 210.0, 20.0).is_culled(vp));
        assert!(!rect_quad(10.0, 100.0, 20.0, 110.0).is_culled(vp));
        // Strictly outside on any side is culled.
        assert!(rect_quad(-10.0, 10.0, -0.5, 20.0).is_culled(vp));
        assert!(rect_quad(200.5, 10.0, 210.0, 20.0).is_culled(vp));
        assert!(rect_quad(10.0, -10.0, 20.0, -0.5).is_culled(vp));
        assert!(rect_quad(10.0, 100.5, 20.0, 110.0).is_culled(vp));
        // A rotated quad: only the bounding box counts.
        let q = Quad {
            x: [-5.0, 5.0, -5.0, -15.0],
            y: [-5.0, 5.0, 15.0, 5.0],
            ..rect_quad(0.0, 0.0, 0.0, 0.0)
        };
        assert!(!q.is_culled(vp));
        // Degenerate viewport (zero size) at its origin still "intersects" a quad touching it.
        let empty = IRect {
            x: 0,
            y: 0,
            w: 0,
            h: 0,
        };
        assert!(!rect_quad(0.0, 0.0, 1.0, 1.0).is_culled(empty));
        assert!(rect_quad(0.5, 0.5, 1.0, 1.0).is_culled(empty));
    }

    #[test]
    fn digest_tracks_content() {
        let mut a = LayerQueue::new(0, 0);
        a.color([1.0, 0.0, 0.0, 1.0]);
        a.quad(rect_quad(0.0, 0.0, 1.0, 1.0));
        let b = a.clone();
        assert_eq!(digest(&[a.clone()]), digest(&[b]));
        let mut c = a.clone();
        c.sub_layer = 1;
        assert_ne!(digest(&[a.clone()]), digest(&[c]));
        let mut d = a.clone();
        d.cmds[0] = Cmd::Color([1.0, 0.0, 0.0, 0.5]);
        assert_ne!(digest(&[a]), digest(&[d]));
    }
}
