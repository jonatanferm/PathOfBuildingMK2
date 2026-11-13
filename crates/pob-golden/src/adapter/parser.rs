//! Mod parser query sets (GOLDEN-PLAN §1.3).
//!
//! Sets: `fn.parser.lines`, `fn.parser.fuzz`, `fn.parser.jewel`. Filled by GOLD7. Until then [`SETS`] is empty and [`lookup`] has no
//! adapter for them, so a committed file of these sets fails in the runner ("no adapter").
//! The behaviour-level functions this module will provide (GOLDEN-PLAN §3.4):
//!
//! ```text
//! fn parse_line(line: &str) -> (Vec<ModCanon>, Option<String>);
//! fn jewel_func(func_line: &str, tree: &str, node: u32) -> Result<JewelOut, Error>;
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
