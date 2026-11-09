//! Gallery "u14_keys" (U14): the Rust side of `lua/gallery/u14_keys.lua` (`_u14.rs`, real
//! screen size).

use pob_host::{App, PlatformParts};

/// The gallery app.
pub fn app(parts: PlatformParts) -> Box<dyn App> {
    super::_u14::app(parts, None)
}
