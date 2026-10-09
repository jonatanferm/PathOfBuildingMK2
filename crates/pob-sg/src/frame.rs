//! Render-level command stream (ui doc §4.3): SimpleGraphic's per-`(layer, subLayer)` queues
//! of `VIEWPORT/BIND/COLOR/QUAD` commands (`r_main.cpp` `r_layer_c`). Owner: U1.

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
    /// defaults to 1): a quad whose bounding box does not intersect `(0, 0)..extent` of its
    /// viewport is dropped by the backend. Moved from the pre-port `DrawCmd::is_culled`.
    pub fn is_culled(&self, vp: IRect) -> bool {
        let min_x = self.x.iter().copied().fold(f32::INFINITY, f32::min);
        let max_x = self.x.iter().copied().fold(f32::NEG_INFINITY, f32::max);
        let min_y = self.y.iter().copied().fold(f32::INFINITY, f32::min);
        let max_y = self.y.iter().copied().fold(f32::NEG_INFINITY, f32::max);
        !(min_x < vp.w as f32 && max_x > 0.0 && min_y < vp.h as f32 && max_y > 0.0)
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
