//! The whole PassiveSpec after load, unstubbed, and its operations (GOLDEN-PLAN §1.3).
//!
//! Sets: `fn.spec`. Filled by GOLD8. Until then [`SETS`] is empty and [`lookup`] has no
//! adapter for them, so a committed file of these sets fails in the runner ("no adapter").
//! The behaviour-level functions this module will provide (GOLDEN-PLAN §3.4):
//!
//! ```text
//! fn spec_records(b: &Build, spec: usize) -> Vec<(RecordKey, Json)>;
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
