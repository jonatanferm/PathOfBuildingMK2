//! The runtime load of `data.bin` (calc doc §11.1 U0: "data.bin loads in < 1 s").
//!
//! Run with `scripts/port/cargo.sh test --release -p pob-gamedata --test dump_load -- --nocapture`;
//! the time gate applies to optimised builds only. Needs `data/dump/data.bin` (written by
//! `pob-dump --compile`), found by `pob_testpaths` (`$POB_DUMP_DIR`, this workspace, the main
//! checkout); a missing file fails the test unless `POB_ALLOW_SKIP=1`.

use std::time::Instant;

#[test]
fn data_bin_loads_fast() {
    let Some(path) = pob_testpaths::require(
        pob_testpaths::data_bin(),
        "data/dump/data.bin (run `pob-dump --compile`)",
    ) else {
        return;
    };
    let bytes = std::fs::read(&path).unwrap();
    let t = Instant::now();
    let tree = pob_gamedata::loader::decode_bin(&bytes).unwrap_or_else(|e| panic!("{e}"));
    let data = pob_gamedata::Data::from_tree(&tree).unwrap_or_else(|e| panic!("{e}"));
    let secs = t.elapsed().as_secs_f64();
    eprintln!(
        "data.bin {:.1} MB: decode + typed load {secs:.3}s ({} skills, {} gems)",
        bytes.len() as f64 / 1e6,
        data.skills.len(),
        data.gems.len()
    );
    assert_eq!(data.gems.len(), 871);
    if !cfg!(debug_assertions) {
        assert!(secs < 1.0, "data.bin load took {secs:.3}s (gate: < 1 s)");
    }
}
