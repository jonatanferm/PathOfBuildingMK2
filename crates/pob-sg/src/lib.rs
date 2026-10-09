//! `pob-sg`: a Rust re-implementation of SimpleGraphic, the host/renderer that
//! Path of Building's Lua UI is written against.
//!
//! Behaviour is ported from the SimpleGraphic sources
//! (<https://github.com/PathOfBuildingCommunity/PathOfBuilding-SimpleGraphic>,
//! master @ 8694a07, 2026-09-14). Each function documents the C++ function it
//! mirrors (`file:function`). Where the shipped DLL of PoB 2.67.2 differs from
//! master (it still has `SetBlendMode`), the shipped behaviour is kept.
//!
//! Layers:
//! - core (always, wasm-clean, no GPU): [`Sg`] records draw commands;
//!   [`font`] parses the TGF/TGA bitmap fonts and lays out text; [`color`]
//!   holds the escape palette and PoB's `colorCodes`; [`images`] is the image
//!   handle / texture registry (decoding with the `image` crate, async on
//!   native threads).
//! - `gpu` feature: [`gpu::Renderer`], a wgpu backend drawing the command list
//!   (works on WebGL2).
//! - `host` feature: [`host`], a winit desktop window + event loop mapping input
//!   to PoB's key names and callbacks.

#![allow(clippy::many_single_char_names, clippy::option_option, clippy::case_sensitive_file_extension_comparisons)]

pub mod color;
pub mod font;
pub mod images;
pub mod sg;
pub mod tga;

#[cfg(feature = "gpu")]
pub mod gpu;

#[cfg(all(feature = "host", not(target_arch = "wasm32")))]
pub mod host;

pub use color::{strip_escapes, ColorCodes, Rgba, PALETTE};
pub use font::{Align, Font, FontSet};
pub use images::{ImageHandle, Images, LoadFlags, TextureData, TextureId};
pub use sg::{BlendMode, DrawCmd, Sg, TexRef, Viewport};
