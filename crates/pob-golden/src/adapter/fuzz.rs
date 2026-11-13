//! Fuzz output sets (GOLDEN-PLAN §1.4): the frozen DF-A and OF-A generators and the helpers they
//! drive; `fuzz.defence` builds its damage tables in each stored `pairs` order (`$perOrder`).
//!
//! Sets: `fuzz.defence`, `fuzz.offence`. Filled by GOLD12. Until then [`SETS`] is empty and [`lookup`] has no
//! adapter for them, so a committed file of these sets fails in the runner ("no adapter").
//! The behaviour-level functions this module will provide (GOLDEN-PLAN §3.4):
//!
//! ```text
//! fn fuzz_helper(name: &str, input: &Json) -> Result<Json, Error>;
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
