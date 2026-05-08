//! Behavioural tests for `Character::allocate_path` and `Character::unallocate`.
//!
//! Two invariants the rest of the app depends on:
//!   1. Allocating a far node also allocates every node on the shortest path
//!      from the existing allocation to the target (PoB / poeplanner UX).
//!   2. Unallocating a node also removes any allocated nodes that are now
//!      disconnected from the character's class start (no orphan dangles).
//!
//! Both bugs have shipped to users in the past; these tests pin them.

use std::collections::HashSet;

use ahash::HashMap as AHashMap;
use pob_data::{
    Class, Node, NodeId, NodeKind, PassiveTree, TreeConstants, TreePoints,
};
use pob_engine::{Character, ClassRef};
use smallvec::SmallVec;

fn empty_tree() -> PassiveTree {
    PassiveTree {
        version: "test".into(),
        tree: "test".into(),
        classes: vec![],
        groups: AHashMap::default(),
        nodes: AHashMap::default(),
        jewel_slots: vec![],
        min_x: 0,
        min_y: 0,
        max_x: 0,
        max_y: 0,
        constants: TreeConstants {
            skills_per_orbit: vec![],
            orbit_radii: vec![],
            classes: AHashMap::default(),
            character_attributes: AHashMap::default(),
            pss_centre_inner_radius: None,
        },
        points: TreePoints::default(),
    }
}

fn add_node(tree: &mut PassiveTree, id: NodeId, neighbours: &[NodeId]) {
    let node = Node {
        id,
        name: Some(format!("n{id}")),
        icon: None,
        ascendancy_name: None,
        stats: vec![],
        reminder_text: vec![],
        kind: NodeKind::Normal,
        class_start_index: None,
        group: None,
        orbit: None,
        orbit_index: None,
        out_edges: neighbours.iter().copied().collect::<SmallVec<_>>(),
        in_edges: SmallVec::new(),
        mastery_effects: vec![],
        expansion_jewel_size: None,
        jewel_radius: None,
    };
    tree.nodes.insert(id, node);
}

/// Synthetic tree:
///
/// ```text
///   1 — 2 — 3 — 4 — 5
///           |
///          10 — 11
///
///       50 (island)
/// ```
///
/// Class "Test" anchors on node 1. Node 50 is unreachable.
fn build_tree() -> PassiveTree {
    let mut tree = empty_tree();
    tree.classes.push(Class {
        name: "Test".into(),
        base_str: 0,
        base_dex: 0,
        base_int: 0,
        ascendancies: vec![],
    });
    add_node(&mut tree, 1, &[2]);
    add_node(&mut tree, 2, &[1, 3]);
    add_node(&mut tree, 3, &[2, 4, 10]);
    add_node(&mut tree, 4, &[3, 5]);
    add_node(&mut tree, 5, &[4]);
    add_node(&mut tree, 10, &[3, 11]);
    add_node(&mut tree, 11, &[10]);
    add_node(&mut tree, 50, &[]);
    if let Some(n) = tree.nodes.get_mut(&1) {
        n.class_start_index = Some(0);
        n.kind = NodeKind::ClassStart;
    }
    tree
}

fn test_character() -> Character {
    let mut c = Character {
        class: ClassRef("Test".into()),
        ..Character::default()
    };
    c.allocate(1); // class start, manually seeded
    c
}

fn alloc_set(c: &Character) -> HashSet<NodeId> {
    c.allocated.iter().copied().collect()
}

#[test]
fn allocate_path_inserts_every_node_on_route_to_target() {
    let tree = build_tree();
    let mut c = test_character();

    let added = c
        .allocate_path(&tree, 5)
        .expect("node 5 is reachable from node 1");

    // Newly inserted: 2, 3, 4, 5 — node 1 was already allocated.
    assert_eq!(added, vec![2, 3, 4, 5]);
    assert_eq!(
        alloc_set(&c),
        [1, 2, 3, 4, 5].into_iter().collect::<HashSet<_>>(),
    );
}

#[test]
fn allocate_path_returns_empty_for_already_allocated() {
    let tree = build_tree();
    let mut c = test_character();
    c.allocate(2);
    let added = c.allocate_path(&tree, 2).expect("ok");
    assert!(added.is_empty());
    assert_eq!(alloc_set(&c), [1, 2].into_iter().collect());
}

#[test]
fn allocate_path_returns_none_when_unreachable() {
    let tree = build_tree();
    let mut c = test_character();
    let before = alloc_set(&c);
    let result = c.allocate_path(&tree, 50);
    assert!(result.is_none(), "island node should be unreachable");
    // Allocation must not have changed when the path is rejected.
    assert_eq!(alloc_set(&c), before);
}

#[test]
fn allocate_path_when_empty_inserts_only_target() {
    let tree = build_tree();
    let mut c = Character {
        class: ClassRef("Test".into()),
        ..Character::default()
    };
    let added = c.allocate_path(&tree, 3).expect("ok");
    assert_eq!(added, vec![3]);
    assert_eq!(alloc_set(&c), [3].into_iter().collect());
}

#[test]
fn unallocate_leaf_only_removes_that_leaf() {
    let tree = build_tree();
    let mut c = test_character();
    c.allocate_path(&tree, 5).unwrap();
    // 1, 2, 3, 4, 5 allocated — removing 5 leaves 1..=4 anchored to start.

    let removed = c.unallocate(&tree, 5);

    assert_eq!(removed, vec![5]);
    assert_eq!(alloc_set(&c), [1, 2, 3, 4].into_iter().collect());
}

#[test]
fn unallocate_pivot_drops_orphans_beyond_it() {
    let tree = build_tree();
    let mut c = test_character();
    c.allocate_path(&tree, 5).unwrap();
    c.allocate_path(&tree, 11).unwrap();
    // 1, 2, 3, 4, 5, 10, 11 allocated.

    let mut removed = c.unallocate(&tree, 3);
    removed.sort_unstable();

    // Removing 3 disconnects 4, 5, 10, 11 from the start at 1.
    // Only 1 and 2 stay anchored.
    assert_eq!(removed, vec![3, 4, 5, 10, 11]);
    assert_eq!(alloc_set(&c), [1, 2].into_iter().collect());
}

#[test]
fn unallocate_class_start_keeps_chain_anchored_through_tree_edges() {
    // The class-start node id is the anchor regardless of whether the user
    // has it in `allocated` (PoB doesn't even let you toggle it off, but
    // imports / resets can leave it absent). So unallocating node 1 still
    // leaves the chain 2..=5 connected through tree-graph edges to the
    // class-start anchor, matching the rules `perform::connected_allocations`
    // uses when crediting stats. Without this, a stale `allocated` set
    // would silently lose everyone's mods on a click.
    let tree = build_tree();
    let mut c = test_character();
    c.allocate_path(&tree, 5).unwrap();

    let removed = c.unallocate(&tree, 1);

    assert_eq!(removed, vec![1]);
    assert_eq!(alloc_set(&c), [2, 3, 4, 5].into_iter().collect());
}

#[test]
fn unallocate_unallocated_node_is_noop() {
    let tree = build_tree();
    let mut c = test_character();
    c.allocate_path(&tree, 5).unwrap();
    let before = alloc_set(&c);

    let removed = c.unallocate(&tree, 11); // 11 is not allocated

    assert!(removed.is_empty());
    assert_eq!(alloc_set(&c), before);
}

#[test]
fn unallocate_keeps_branch_anchored_via_alternate_path() {
    // Build a small loop:  1 — 2 — 3 — 4
    //                          \   /
    //                           20
    // Removing node 3 still leaves 4 connected through 2 — 20 — 4.
    let mut tree = empty_tree();
    tree.classes.push(Class {
        name: "Test".into(),
        base_str: 0,
        base_dex: 0,
        base_int: 0,
        ascendancies: vec![],
    });
    add_node(&mut tree, 1, &[2]);
    add_node(&mut tree, 2, &[1, 3, 20]);
    add_node(&mut tree, 3, &[2, 4]);
    add_node(&mut tree, 4, &[3, 20]);
    add_node(&mut tree, 20, &[2, 4]);
    if let Some(n) = tree.nodes.get_mut(&1) {
        n.class_start_index = Some(0);
        n.kind = NodeKind::ClassStart;
    }

    let mut c = Character {
        class: ClassRef("Test".into()),
        ..Character::default()
    };
    c.allocate(1);
    c.allocate_path(&tree, 4).unwrap();
    c.allocate_path(&tree, 20).unwrap();
    // All five nodes allocated.
    assert_eq!(alloc_set(&c), [1, 2, 3, 4, 20].into_iter().collect());

    let removed = c.unallocate(&tree, 3);

    assert_eq!(removed, vec![3]);
    // 4 stays — reachable via 1 → 2 → 20 → 4.
    assert_eq!(alloc_set(&c), [1, 2, 4, 20].into_iter().collect());
}
