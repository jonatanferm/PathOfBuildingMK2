//! Validation harness — for now, hard-coded reference values from running PoB on
//! known-simple characters. Phase 6 will swap this for a Lua-driven harness that
//! computes the references on the fly from the upstream PathOfBuilding repo.
//!
//! Skips silently if the data dir is missing.

use std::path::PathBuf;

use pob_engine::{character::ClassRef, compute, Character};

fn data_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .parent()
        .unwrap()
        .join("data")
}

fn load_3_25_tree() -> Option<pob_data::PassiveTree> {
    let path = data_root().join("trees/3_25.json");
    let json = std::fs::read_to_string(&path).ok()?;
    pob_data::load_passive_tree(&json).ok()
}

#[test]
fn marauder_l1_naked_baseline() {
    let Some(tree) = load_3_25_tree() else {
        eprintln!("skip: data missing");
        return;
    };
    let c = Character::new(ClassRef::marauder(), 1);
    let out = compute(&c, &tree);

    // Reference values from PoB 3.25 with default config:
    //   Marauder L1, no items, no allocations.
    //   Strength = 32, Dex = 14, Int = 14
    //   Life = 50 (level 1) + 32/2 (Str bonus) = 66
    //   Mana = 40 + 14/2 = 47
    assert_eq!(out.get("Strength"), 32.0);
    assert_eq!(out.get("Dexterity"), 14.0);
    assert_eq!(out.get("Intelligence"), 14.0);
    assert_eq!(out.get("Life"), 66.0);
    assert_eq!(out.get("Mana"), 47.0);
    assert_eq!(out.get("FireResistMax"), 75.0);
    assert_eq!(out.get("FireResistTotal"), 0.0);
}

#[test]
fn witch_l68_naked_baseline() {
    let Some(tree) = load_3_25_tree() else {
        return;
    };
    let c = Character::new(ClassRef::witch(), 68);
    let out = compute(&c, &tree);

    // Witch base: 14 Str / 14 Dex / 32 Int
    // Life: 50 + 12*67 + 14/2 = 854 + 7 = 861
    // Mana: 40 + 6*67 + 32/2 = 442 + 16 = 458
    assert_eq!(out.get("Strength"), 14.0);
    assert_eq!(out.get("Intelligence"), 32.0);
    assert_eq!(out.get("Life"), 50.0 + 12.0 * 67.0 + 7.0);
    assert_eq!(out.get("Mana"), 40.0 + 6.0 * 67.0 + 16.0);
}

/// Performance smoke test, release-only. ~3000 nodes, every passive allocated, computing
/// every basic stat. Skipped in debug builds where it's ~10× slower (and CI unhelpful).
#[test]
#[cfg_attr(debug_assertions, ignore = "release-only perf check")]
fn compute_is_under_5ms_with_full_tree_allocation() {
    let Some(tree) = load_3_25_tree() else {
        return;
    };
    let mut c = Character::new(ClassRef::marauder(), 90);
    for id in tree.nodes.keys() {
        c.allocated.insert(*id);
    }
    let start = std::time::Instant::now();
    let n_iter = 50;
    for _ in 0..n_iter {
        let _ = compute(&c, &tree);
    }
    let per = start.elapsed() / n_iter;
    eprintln!("compute() avg: {per:?}");
    assert!(
        per < std::time::Duration::from_millis(5),
        "compute() too slow: {per:?}"
    );
}

#[test]
fn class_attribute_split_matches_pob() {
    // Per PoB src/TreeData/3_25/tree.lua, every class has known base stats. Verify a
    // sweep so a tree-version bump that changes them doesn't go unnoticed.
    let Some(tree) = load_3_25_tree() else {
        return;
    };
    let cases: &[(&str, i32, i32, i32)] = &[
        ("Scion", 20, 20, 20),
        ("Marauder", 32, 14, 14),
        ("Ranger", 14, 32, 14),
        ("Witch", 14, 14, 32),
        ("Duelist", 23, 23, 14),
        ("Templar", 23, 14, 23),
        ("Shadow", 14, 23, 23),
    ];
    for (name, str_, dex, int_) in cases {
        let c = Character::new(ClassRef((*name).into()), 1);
        let out = compute(&c, &tree);
        assert_eq!(out.get("Strength"), f64::from(*str_), "{name} Str");
        assert_eq!(out.get("Dexterity"), f64::from(*dex), "{name} Dex");
        assert_eq!(out.get("Intelligence"), f64::from(*int_), "{name} Int");
    }
}
