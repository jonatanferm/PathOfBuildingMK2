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
fn parser_covers_floor_of_every_tree_version() {
    let trees_dir = data_root().join("trees");
    let Ok(idx_json) = std::fs::read_to_string(trees_dir.join("index.json")) else {
        eprintln!("skip: data missing");
        return;
    };
    let index: Vec<String> = pob_data::load_tree_index(&idx_json).unwrap();
    let mut totals: Vec<(String, u64, u64)> = Vec::new();
    for v in &index {
        let path = trees_dir.join(format!("{v}.json"));
        let Ok(json) = std::fs::read_to_string(&path) else {
            continue;
        };
        let tree: pob_data::PassiveTree = serde_json::from_str(&json).unwrap();
        let mut t = 0u64;
        let mut p = 0u64;
        for node in tree.nodes.values() {
            for raw in &node.stats {
                for line in raw.lines() {
                    let line = line.trim();
                    if line.is_empty() {
                        continue;
                    }
                    t += 1;
                    if pob_engine::parse_mod_line(line).is_some() {
                        p += 1;
                    }
                }
            }
        }
        totals.push((v.clone(), p, t));
    }
    println!("Per-version coverage:");
    for (v, p, t) in &totals {
        let pct = (*p as f64 / *t as f64) * 100.0;
        println!("  {v:<24} {p}/{t} ({pct:.1}%)");
    }
    // Fallback ensures everything parses to *some* mod, so 100% is the floor.
    for (v, p, t) in &totals {
        assert_eq!(p, t, "tree {v}: {p}/{t} should be 100%");
    }
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

    // Tree data sometimes packs several stats into a single newline-joined string
    // (e.g. mastery effects, keystone descriptions). Split on newlines before parsing
    // so each stat is counted independently.
    for node in tree.nodes.values() {
        for raw in &node.stats {
            for line in raw.lines() {
                let line = line.trim();
                if line.is_empty() {
                    continue;
                }
                total += 1;
                if parse_mod_line(line).is_some() {
                    parsed += 1;
                } else {
                    *unparsed_examples.entry(line.to_owned()).or_insert(0) += 1;
                }
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

    // Print the actual byte sequence of any unparsed lines so we can see hidden
    // characters (carriage returns, NBSPs, etc.) that diagnostic-mode plain-text
    // dumps would hide.
    for (line, _) in top.iter().take(5) {
        println!("DEBUG bytes: {:?}", line.as_bytes());
    }

    // We hit 100% coverage with the fallback path; assert that as the floor.
    assert_eq!(
        parsed, total,
        "regression: parser coverage dropped — {parsed}/{total}"
    );
}
