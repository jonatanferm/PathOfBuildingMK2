//! Gallery "u14_mouse" (U14): the Rust side of `lua/gallery/u14_mouse.lua` (`_u14.rs`,
//! `main.screenH` pinned to 720).

use pob_host::{App, PlatformParts};

/// The gallery app.
pub fn app(parts: PlatformParts) -> Box<dyn App> {
    super::_u14::app(parts, Some(720.0))
}
