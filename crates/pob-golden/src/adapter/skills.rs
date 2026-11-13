//! Active skills per socket group and weapon setup, and the tabs (GOLDEN-PLAN §1.3).
//!
//! Sets: `fn.skills`, `fn.tabs`. Filled by GOLD11 (`fn.skills`), GOLD5 (`fn.tabs`). Until then [`SETS`] is empty and [`lookup`] has no
//! adapter for them, so a committed file of these sets fails in the runner ("no adapter").
//! The behaviour-level functions this module will provide (GOLDEN-PLAN §3.4):
//!
//! ```text
//! fn active_skill(b: &Build, group: usize, weapons: u8) -> Json;
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
