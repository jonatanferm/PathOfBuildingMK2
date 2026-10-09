//! `pob-sg`: the PURE SimpleGraphic equivalent of the PoB UI Rust port (ui doc §3, §4, D2).
//! No GPU, no winit, no threads, no std::fs; builds for wasm32.
//!
//! Restructured by UI-U0 (AMENDMENTS A4) from the pre-port crate (commit 6e883cb, ported from
//! SimpleGraphic master 8694a07): `sg.rs` became [`api`] (+ [`frame`]), `font.rs`/`tga.rs` became
//! [`font`], `images.rs` became [`image`] (+ the loaders in pob-host), `gpu/` became
//! `pob-sg-wgpu`, `host.rs` became `pob-host`, and PoB's `colorCodes` moved to pob-ui-core.
//! Wave-1 units U1/U2/U3/U6/U7 close the deltas listed in ui doc §2 (pin `3b1a346`).
//!
//! Module owners after UI-U0 (ui doc §12): `types`, `trace` (U0 complete, then U1), `api`,
//! `frame`, `color`, `sys`, trace bodies (U1); `font` (U2); `image`, `assets` (U3); `raster` (U6).
#![allow(
    clippy::many_single_char_names,
    clippy::too_many_arguments,
    clippy::case_sensitive_file_extension_comparisons,
    clippy::new_without_default,
    clippy::needless_range_loop
)]

pub mod api;
pub mod assets;
pub mod color;
pub mod font;
pub mod frame;
pub mod image;
#[cfg(feature = "raster")]
pub mod raster;
pub mod sys;
pub mod trace;
pub mod types;

pub use api::{scaled_text_height, strip_escapes, Sg};
pub use assets::AssetSource;
pub use color::PALETTE;
pub use font::{FontFace, FontSet};
pub use frame::{Cmd, Frame, LayerQueue, Quad, QuadSink};
pub use image::{
    ImageData, ImageFlags, ImageHandle, ImageLoader, ImageRegistry, LoadEvent, TexId, TexStatus,
};
pub use sys::{FileEntry, Fs, HttpRequest, HttpResponse, Net, Platform, Sys};
pub use types::{Align, Font, IRect, Key, Num, ScreenInfo};
