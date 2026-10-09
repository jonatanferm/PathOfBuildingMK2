//! CPU reference rasteriser (ui doc §3 `raster.rs`, §11.1 T5): `Frame -> RGBA`. Feature `raster`.
//! Owner: U6. UI-U0 stub: final signature, body pending.
//!
//! Semantics to match (ui doc §2, §5.1): replay the sorted queues, translate quads by the
//! viewport origin, scissor to the viewport, cull (`Quad::is_culled`), sample with the texture's
//! CLAMP/NEAREST/MIPMAP state, `color = texture * tint`, blend `SrcAlpha, OneMinusSrcAlpha` in
//! gamma space into an opaque black RGB target.

use std::collections::HashMap;
use std::rc::Rc;

use crate::frame::Frame;
use crate::image::{ImageData, TexId};

/// Textures the rasteriser has seen (fed from `Frame::uploads`, pruned by `Frame::releases`).
#[derive(Default)]
pub struct Raster {
    pub textures: HashMap<TexId, Rc<ImageData>>,
}

impl Raster {
    pub fn new() -> Raster {
        Raster::default()
    }

    /// Apply the frame's uploads/releases, then render it: `(width, height, RGBA8)` at framebuffer size.
    pub fn render(&mut self, frame: &Frame) -> (u32, u32, Vec<u8>) {
        let _ = frame;
        todo!("U6: CPU reference rasteriser")
    }
}
