//! Shared helpers for the unit acceptance tests (`tests/<unit>.rs`, calc doc
//! §9.4 L4/L5): load the data and a build, run one Rust segment from an
//! injected `<seg>:in` dump and diff it against `<seg>:out`.
//!
//! Owner: ORC-X. Stub created by U0 with the final signatures.

use std::path::Path;

use pob_calc::model::BuildRef;

/// Initialise `pob_gamedata::data()` from `data/dump` (data.bin, else the JSON dump).
pub fn init_data(dump_dir: &Path) -> bool {
    let _ = dump_dir;
    pob_calc::unported!("testkit::init_data (ORC-X)")
}

/// A build model from a build XML file (BM-A's loader).
pub fn load_build(xml: &Path) -> Option<BuildRef> {
    let _ = xml;
    pob_calc::unported!("testkit::load_build (ORC-X)")
}

/// L4: inject `<stage>:in#n`, run the Rust segment, diff against the next stage's dump.
/// Returns the mismatching keys (empty = pass).
pub fn segment_diff(build: &Path, stage_in: &str, stage_out: &str) -> Vec<String> {
    let _ = (build, stage_in, stage_out);
    pob_calc::unported!("testkit::segment_diff (ORC-X)")
}
