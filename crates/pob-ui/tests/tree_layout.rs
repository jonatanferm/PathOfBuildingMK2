//! Verify the tree-layout math against the real 3.25 tree.

use std::path::PathBuf;

use pob_data::PassiveTree;
use pob_ui::TreeView;

fn data_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .parent()
        .unwrap()
        .join("data")
}

fn load_tree() -> Option<PassiveTree> {
    let path = data_root().join("trees/3_25.json");
    let json = std::fs::read_to_string(&path).ok()?;
    pob_data::load_passive_tree(&json).ok()
}

fn load_sprites() -> Option<pob_data::sprites::SpriteSet> {
    let path = data_root().join("sprites/3_25.json");
    let json = std::fs::read_to_string(&path).ok()?;
    pob_data::sprites::load_sprites(&json).ok()
}

#[test]
fn tree_view_constructs_without_panic() {
    let Some(tree) = load_tree() else {
        eprintln!("skip: data missing");
        return;
    };
    let _view = TreeView::new(&tree, None);
}

#[test]
fn ascendancy_instances_emit_one_per_ascendancy_start() {
    // Issue #110: each AscendancyStart node should yield one
    // medallion instance against the `ascendancy.png` atlas. PoB
    // 3.25 ships 19 ascendancies (Scion's Ascendant + 6 classes
    // × 3 ascendancies each), each with one AscendancyStart node.
    let Some(tree) = load_tree() else {
        eprintln!("skip: data missing");
        return;
    };
    let Some(sprites) = load_sprites() else {
        eprintln!("skip: sprites missing");
        return;
    };
    let view = TreeView::new(&tree, Some(&sprites));
    let asc_starts = tree
        .nodes
        .values()
        .filter(|n| matches!(n.kind, pob_data::NodeKind::AscendancyStart))
        .count();
    assert!(
        asc_starts > 0,
        "3.25 tree should contain AscendancyStart nodes"
    );
    assert_eq!(
        view.ascendancy_instance_count(),
        asc_starts,
        "one medallion instance per AscendancyStart node"
    );
}

#[test]
fn ascendancy_instances_empty_without_sprites() {
    // Sprite metadata absent (e.g. headless tests with no atlas):
    // the ascendancy instances vec must stay empty so the wgpu
    // callback becomes a no-op draw rather than rendering with
    // garbage UVs.
    let Some(tree) = load_tree() else {
        eprintln!("skip: data missing");
        return;
    };
    let view = TreeView::new(&tree, None);
    assert_eq!(view.ascendancy_instance_count(), 0);
}
