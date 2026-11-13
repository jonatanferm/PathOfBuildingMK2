//! E2E build goldens (GOLDEN-PLAN §1.2): build XML in, behaviour-level outputs out.
//!
//! Sets: `e2e.ca`, `e2e.d13`, `e2e.levelcost`, `e2e.int3`, `e2e.bm`, `e2e.pwr`, `e2e.mcq`.
//! Filled by GOLD3 (`e2e.ca`, `e2e.d13`, `e2e.levelcost`, the mirage block), GOLD4
//! (`e2e.int3`), GOLD5 (`e2e.bm`), GOLD6 (`e2e.pwr`, `e2e.mcq`). Until then [`SETS`] is empty
//! and [`lookup`] has no adapter for them, so a committed file of these sets fails in the runner
//! ("no adapter").
//! The behaviour-level functions this module will provide (GOLDEN-PLAN §3.4):
//!
//! ```text
//! fn load_build(xml: &str, rounds: usize) -> Result<Build, Error>;   // Build:Init + the OnFrame rebuilds PoB made
//! fn main_output(b: &Build) -> Flat;
//! fn calcs_output(b: &Build) -> Flat;
//! fn calcs_lists(b: &Build) -> Flat;
//! fn used_keys(b: &Build) -> BTreeMap<String, BTreeSet<String>>;
//! fn misc_calc(b: &Build, o: &OverrideSpec, full_dps: Option<bool>) -> Result<Flat, Error>;
//! fn breakdowns(b: &Build, actor: Actor) -> Vec<(String, Json)>;
//! fn tabulate(b: &Build, actor: Actor, q: &SectionQuery) -> Result<Rows, Error>;
//! fn save_xml(b: &Build) -> String;
//! fn model(b: &Build) -> Flat;
//! fn power_report(b: &Build, mode: &PowerMode) -> Flat;
//! fn mirage_output(b: &Build) -> Option<Flat>;
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
