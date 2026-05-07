//! Telemetry test: how many of the real PoB passive-tree stat lines does our Phase 2
//! parser handle?
//!
//! This is *not* an assertion-on-coverage test — it's a regression detector. It prints a
//! summary so we can see Phase 2's narrow scope at a glance, and asserts only that we
//! cover at least some defined floor (so a regression that breaks parsing entirely fails
//! loudly).

use std::collections::HashMap;
use std::path::PathBuf;

use pob_engine::mod_parser::parse_mod_line;

fn data_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .parent()
        .unwrap()
        .join("data")
}

#[test]
fn parser_covers_floor_of_3_25_passives() {
    let path = data_root().join("trees/3_25.json");
    let Ok(json) = std::fs::read_to_string(&path) else {
        eprintln!("skip: {} missing", path.display());
        return;
    };
    let tree: pob_data::PassiveTree = serde_json::from_str(&json).unwrap();

    let mut total: u64 = 0;
    let mut parsed: u64 = 0;
    let mut unparsed_examples: HashMap<String, u32> = HashMap::new();

    for (_, node) in &tree.nodes {
        for line in &node.stats {
            total += 1;
            if parse_mod_line(line).is_some() {
                parsed += 1;
            } else {
                *unparsed_examples.entry(line.clone()).or_insert(0) += 1;
            }
        }
    }

    let pct = (parsed as f64 / total as f64) * 100.0;
    println!("3_25 passive stat lines: {parsed}/{total} parsed ({pct:.1}%)");

    // Show the top-30 unparsed lines so any regression is easy to spot.
    let mut top: Vec<_> = unparsed_examples.into_iter().collect();
    top.sort_by_key(|(_, n)| std::cmp::Reverse(*n));
    println!("Top unparsed lines:");
    for (line, n) in top.iter().take(30) {
        println!("  {n:>4}× {line}");
    }

    // Floor: Phase 2 parser handles a small subset, but a regression to zero would mean
    // something broke. The floor is intentionally loose.
    assert!(parsed > 200, "expected to parse at least 200 lines, got {parsed}");
}
