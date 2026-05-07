//! Round-trip tests: load every extracted JSON file, verify nothing panics, sanity-check
//! a few invariants.
//!
//! These tests require the workspace `data/` directory to be populated. Run
//! `cargo run -p pob-extract --release` first if it isn't.

use std::path::{Path, PathBuf};

use pob_data::{load_bases, load_gems, load_passive_tree, load_tree_index, NodeKind};

fn data_root() -> PathBuf {
    // CARGO_MANIFEST_DIR is the crate dir; data/ is at the workspace root.
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .parent()
        .unwrap()
        .join("data")
}

fn read(path: &Path) -> Option<String> {
    std::fs::read_to_string(path).ok()
}

#[test]
fn bases_load() {
    let path = data_root().join("bases.json");
    let Some(json) = read(&path) else {
        eprintln!("skip: {} not found — run pob-extract first", path.display());
        return;
    };
    let bases = load_bases(&json).expect("bases parse");
    assert!(bases.len() > 500, "expected hundreds of bases, got {}", bases.len());
    assert!(
        bases.iter().any(|(_, b)| b.weapon.is_some()),
        "expected at least one weapon base"
    );
    assert!(
        bases.iter().any(|(_, b)| b.armour.is_some()),
        "expected at least one armour base"
    );
    assert!(
        bases.iter().any(|(_, b)| b.flask.is_some()),
        "expected at least one flask base"
    );
}

#[test]
fn gems_load() {
    let path = data_root().join("gems.json");
    let Some(json) = read(&path) else {
        eprintln!("skip: {} not found — run pob-extract first", path.display());
        return;
    };
    let gems = load_gems(&json).expect("gems parse");
    assert!(gems.len() > 500, "expected hundreds of gems, got {}", gems.len());
    assert!(
        gems.iter().any(|(_, g)| g.tags.contains("grants_active_skill")),
        "expected at least one active skill gem"
    );
    let fireball = gems
        .iter()
        .find(|(_, g)| g.name == "Fireball")
        .expect("Fireball must exist");
    assert!(fireball.1.tags.contains("fire"), "fireball is fire");
    assert!(fireball.1.tags.contains("projectile"), "fireball is projectile");
    assert!(fireball.1.tags.contains("spell"), "fireball is spell");
}

#[test]
fn tree_index_load() {
    let path = data_root().join("trees/index.json");
    let Some(json) = read(&path) else {
        eprintln!("skip: {} not found — run pob-extract first", path.display());
        return;
    };
    let index = load_tree_index(&json).expect("index parse");
    assert!(index.len() >= 10, "expected several tree versions");
    // Each must be a directory under data/trees/
    for v in &index {
        let p = data_root().join("trees").join(format!("{v}.json"));
        assert!(p.is_file(), "{} should exist", p.display());
    }
}

#[test]
fn every_tree_loads() {
    let path = data_root().join("trees/index.json");
    let Some(idx_json) = read(&path) else {
        eprintln!("skip: {} not found — run pob-extract first", path.display());
        return;
    };
    let index = load_tree_index(&idx_json).unwrap();
    for v in &index {
        let tree_path = data_root().join("trees").join(format!("{v}.json"));
        let json = std::fs::read_to_string(&tree_path).expect("read tree");
        let tree = load_passive_tree(&json)
            .unwrap_or_else(|e| panic!("loading {v}: {e}"));
        assert_eq!(tree.version, *v);
        assert!(!tree.classes.is_empty(), "{v}: classes");
        assert!(!tree.groups.is_empty(), "{v}: groups");
        assert!(!tree.nodes.is_empty(), "{v}: nodes");
        assert_eq!(
            tree.constants.skills_per_orbit.len(),
            tree.constants.orbit_radii.len(),
            "{v}: orbit constants must align"
        );

        // The root node, if present, should be marked Root.
        if let Some(root) = tree.nodes.get(&pob_data::ROOT_NODE_ID) {
            assert!(matches!(root.kind, NodeKind::Root));
        }
    }
}

#[test]
fn current_tree_has_expected_classes() {
    let path = data_root().join("trees/3_25.json");
    let Some(json) = read(&path) else {
        eprintln!("skip: {} not found — run pob-extract first", path.display());
        return;
    };
    let tree = load_passive_tree(&json).unwrap();
    let names: Vec<_> = tree.classes.iter().map(|c| c.name.as_str()).collect();
    assert_eq!(names, vec![
        "Scion", "Marauder", "Ranger", "Witch", "Duelist", "Templar", "Shadow"
    ]);
    // Each class except Scion has 3 ascendancies; Scion has 1.
    let scion = tree.classes.iter().find(|c| c.name == "Scion").unwrap();
    assert_eq!(scion.ascendancies.len(), 1);
    let marauder = tree.classes.iter().find(|c| c.name == "Marauder").unwrap();
    assert_eq!(marauder.ascendancies.len(), 3);
}
