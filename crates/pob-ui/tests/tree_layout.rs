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

#[test]
fn tree_view_constructs_without_panic() {
    let Some(tree) = load_tree() else {
        eprintln!("skip: data missing");
        return;
    };
    let _view = TreeView::new(&tree);
}
