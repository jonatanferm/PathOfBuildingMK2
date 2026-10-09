//! The runtime load of `data.bin` (calc doc §11.1 U0: "data.bin loads in < 1 s").
//!
//! Run with `scripts/port/cargo.sh test --release -p pob-gamedata --test dump_load -- --nocapture`;
//! the time gate applies to optimised builds only. Skips when `data/dump/data.bin` (written by
//! `pob-dump --compile`) is absent.

use std::path::PathBuf;
use std::time::Instant;

fn data_bin() -> Option<PathBuf> {
    if let Some(d) = std::env::var_os("POB_DUMP_DIR") {
        let p = PathBuf::from(d).join("data.bin");
        return p.is_file().then_some(p);
    }
    let here = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    here.ancestors()
        .map(|a| a.join("data/dump/data.bin"))
        .find(|p| p.is_file())
}

#[test]
fn data_bin_loads_fast() {
    let Some(path) = data_bin() else {
        eprintln!("SKIPPED: data/dump/data.bin not found (run `pob-dump --compile`)");
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
