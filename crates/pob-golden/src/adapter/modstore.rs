//! Mod store queries over recorded store graphs (GOLDEN-PLAN §1.3; the trivial answers are
//! elided by the recorder and regenerated through `Adapter::elided`, §2.3).
//!
//! Sets: `fn.modstore`. Filled by GOLD10. Until then [`SETS`] is empty and [`lookup`] has no
//! adapter for them, so a committed file of these sets fails in the runner ("no adapter").
//! The behaviour-level functions this module will provide (GOLDEN-PLAN §3.4):
//!
//! ```text
//! fn modstore_rebuild(inputs: &Json) -> Stores;
//! fn modstore_query(s: &Stores, q: &StoreQuery) -> Result<Json, Error>;
//! ```

use super::Adapter;

/// The sets this module has an adapter for.
pub const SETS: &[&str] = &[];

/// The adapter of `set`, when this module has one.
#[must_use]
pub fn lookup(set: &str) -> Option<&'static dyn Adapter> {
    let _ = set;
    None
}
