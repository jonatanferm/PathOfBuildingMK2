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
use pob_data::{Class, Node, NodeId, NodeKind, PassiveTree, TreeConstants, TreePoints};
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
fn allocate_path_first_click_grows_from_class_start() {
    // First click on a fresh character (nothing allocated) must grow a path
    // from the class-start anchor — the synthetic seed isn't itself
    // allocated, but the chain to the target is. Without this, a freshly
    // rolled Marauder click-jumping to a far node would leave a disconnected
    // island that gives no stats until the user manually fills the gap.
    let tree = build_tree();
    let mut c = Character {
        class: ClassRef("Test".into()),
        ..Character::default()
    };
    let added = c.allocate_path(&tree, 3).expect("ok");
    assert_eq!(added, vec![2, 3]);
    // Anchor (node 1) stays out of `allocated` — it doesn't cost a point.
    assert_eq!(alloc_set(&c), [2, 3].into_iter().collect());
}

#[test]
fn allocate_path_falls_back_to_target_when_no_class_set() {
    // Synthetic-tree case: no class assigned and nothing allocated means
    // there are no seeds at all. Preserve the bare-target fallback so test
    // fixtures and imported-without-class characters can still grow a
    // build by clicking.
    let tree = build_tree();
    let mut c = Character::default(); // class is empty string
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

/// Issue #196: Intuitive Leap test scaffold. Build a tree where node 200
/// is a "free-floating" notable that has no graph edge connecting it to
/// the class start. With Intuitive Leap socketed at jewel-socket node 100
/// (positioned next to node 200), node 200 should be allocatable directly.
fn build_intuitive_leap_tree() -> PassiveTree {
    use pob_data::Group;
    let mut tree = empty_tree();
    tree.classes.push(Class {
        name: "Test".into(),
        base_str: 0,
        base_dex: 0,
        base_int: 0,
        ascendancies: vec![],
    });
    // Allocation backbone: class start (1) → 2 → 100 (jewel socket).
    add_node(&mut tree, 1, &[2]);
    add_node(&mut tree, 2, &[1, 100]);
    add_node(&mut tree, 100, &[2]);
    // Free-floating notable: graph edges go nowhere.
    add_node(&mut tree, 200, &[]);
    // Distant notable with no in-radius proximity to the IL socket.
    add_node(&mut tree, 300, &[]);

    // Class start.
    if let Some(n) = tree.nodes.get_mut(&1) {
        n.class_start_index = Some(0);
        n.kind = NodeKind::ClassStart;
    }
    // Jewel socket positioned at (1000, 0).
    let mut groups = AHashMap::default();
    groups.insert(
        100,
        Group {
            x: 1000.0,
            y: 0.0,
            orbits: smallvec::smallvec![0],
            background: None,
            nodes: vec![100],
            is_proxy: false,
        },
    );
    // Floater notable at (1100, 0) — within the Small radius (≤ 960).
    groups.insert(
        200,
        Group {
            x: 1100.0,
            y: 0.0,
            orbits: smallvec::smallvec![0],
            background: None,
            nodes: vec![200],
            is_proxy: false,
        },
    );
    // Distant notable at (5000, 0) — well outside the Small radius.
    groups.insert(
        300,
        Group {
            x: 5000.0,
            y: 0.0,
            orbits: smallvec::smallvec![0],
            background: None,
            nodes: vec![300],
            is_proxy: false,
        },
    );
    tree.groups = groups;
    if let Some(n) = tree.nodes.get_mut(&100) {
        n.kind = NodeKind::JewelSocket;
        n.group = Some(100);
        n.orbit = Some(0);
        n.orbit_index = Some(0);
    }
    if let Some(n) = tree.nodes.get_mut(&200) {
        n.kind = NodeKind::Notable;
        n.group = Some(200);
        n.orbit = Some(0);
        n.orbit_index = Some(0);
    }
    if let Some(n) = tree.nodes.get_mut(&300) {
        n.kind = NodeKind::Notable;
        n.group = Some(300);
        n.orbit = Some(0);
        n.orbit_index = Some(0);
    }
    tree.constants = TreeConstants {
        skills_per_orbit: vec![1, 6, 16, 16, 40, 72, 72],
        orbit_radii: vec![0, 82, 162, 335, 493, 662, 846],
        classes: AHashMap::default(),
        character_attributes: AHashMap::default(),
        pss_centre_inner_radius: None,
    };
    tree
}

fn intuitive_leap_item() -> pob_data::Item {
    use pob_data::{item::Rarity, Item};
    Item {
        name: "Intuitive Leap".into(),
        base_name: "Viridian Jewel".into(),
        rarity: Rarity::Unique,
        item_level: 84,
        quality: 0,
        tags: ahash::HashSet::default(),
        mod_lines: vec![],
        sockets: String::new(),
        raw: String::new(),
        corrupted: false,
        mirrored: false,
    }
}

#[test]
fn intuitive_leap_allocates_floating_notable_without_path() {
    let tree = build_intuitive_leap_tree();
    let mut c = Character {
        class: ClassRef("Test".into()),
        ..Character::default()
    };
    c.allocate(1);
    c.allocate_path(&tree, 100)
        .expect("jewel socket reachable from class start");
    c.socketed_jewels.socket(100, intuitive_leap_item());

    // Node 200 has no graph edges connecting it to allocated nodes —
    // without IL the existing pathfind logic would return None.
    let added = c
        .allocate_path(&tree, 200)
        .expect("IL must let an in-radius node allocate without a path");
    assert_eq!(added, vec![200]);
    assert!(c.allocated.contains(&200));
}

#[test]
fn intuitive_leap_does_not_short_circuit_out_of_radius_targets() {
    let tree = build_intuitive_leap_tree();
    let mut c = Character {
        class: ClassRef("Test".into()),
        ..Character::default()
    };
    c.allocate(1);
    c.allocate_path(&tree, 100).expect("ok");
    c.socketed_jewels.socket(100, intuitive_leap_item());

    // Node 300 is well outside the Small radius — IL should NOT bypass.
    // It also has no graph edges, so the fallback path-find returns None.
    let result = c.allocate_path(&tree, 300);
    assert!(
        result.is_none(),
        "IL must not auto-allocate nodes outside its radius"
    );
}

#[test]
fn intuitive_leap_requires_socket_to_be_allocated() {
    let tree = build_intuitive_leap_tree();
    let mut c = Character {
        class: ClassRef("Test".into()),
        ..Character::default()
    };
    c.allocate(1);
    // Socket the jewel BUT don't allocate node 100.
    c.socketed_jewels.socket(100, intuitive_leap_item());

    // Without the socket allocated, IL is inactive — node 200 has no edges
    // so the path-find fails and `allocate_path` returns None.
    let result = c.allocate_path(&tree, 200);
    assert!(
        result.is_none(),
        "IL effect requires the host socket to itself be allocated"
    );
}

#[test]
fn intuitive_leap_protects_floater_from_orphan_removal() {
    let tree = build_intuitive_leap_tree();
    let mut c = Character {
        class: ClassRef("Test".into()),
        ..Character::default()
    };
    c.allocate(1);
    c.allocate_path(&tree, 100).expect("ok");
    c.socketed_jewels.socket(100, intuitive_leap_item());
    c.allocate_path(&tree, 200)
        .expect("IL allocation of floater succeeds");
    assert_eq!(alloc_set(&c), [1, 2, 100, 200].into_iter().collect());

    // Unallocate node 2. Without IL protection, both 100 and 200 would
    // orphan-cascade off (100 because the path through 2 broke; 200
    // because IL only protects when the socket is anchored — and we
    // remove the IL socket along with 100). With IL protection, 200
    // would survive only if 100 itself were anchored. In this scenario
    // 100 loses its anchor, so 200 *should* drop too — that's PoB's
    // behaviour.
    let removed = c.unallocate(&tree, 2);
    let removed_set: HashSet<NodeId> = removed.into_iter().collect();
    assert!(removed_set.contains(&2));
    assert!(removed_set.contains(&100), "jewel socket loses anchor");
    assert!(
        removed_set.contains(&200),
        "floater drops when IL socket itself orphans"
    );
}

#[test]
fn intuitive_leap_floater_survives_unrelated_unallocate() {
    // Build a tree where a side branch has a redundant IL socket so
    // unallocating an unrelated node doesn't disturb the floater. The
    // floater's protection holds because its IL socket stays anchored.
    let mut tree = build_intuitive_leap_tree();
    // Add a side branch off node 1: 1 — 50.
    add_node(&mut tree, 50, &[]);
    if let Some(n) = tree.nodes.get_mut(&1) {
        n.out_edges.push(50);
    }
    if let Some(n) = tree.nodes.get_mut(&50) {
        n.in_edges.push(1);
    }

    let mut c = Character {
        class: ClassRef("Test".into()),
        ..Character::default()
    };
    c.allocate(1);
    c.allocate_path(&tree, 100).expect("ok");
    c.socketed_jewels.socket(100, intuitive_leap_item());
    c.allocate_path(&tree, 200).expect("ok");
    c.allocate_path(&tree, 50).expect("ok");

    let removed = c.unallocate(&tree, 50);
    let removed_set: HashSet<NodeId> = removed.into_iter().collect();
    // Only node 50 dropped — floater 200 is still IL-protected.
    assert_eq!(removed_set, [50].into_iter().collect());
    assert!(c.allocated.contains(&200));
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
