//! Passive-tree graph helpers — shortest-path BFS and orphan-set computation.
//!
//! Previously lived in `pob-ui::pathfind`; moved here so the engine's
//! `Character::allocate_path` / `Character::unallocate` can reuse the same logic
//! and so non-UI tests can exercise the allocation rules.
//!
//! Edges are treated as undirected (PoB stores both `out` and `in` lists).

use std::collections::{HashMap, HashSet, VecDeque};

use pob_data::{NodeId, PassiveTree};

/// Returns a path `[from_node, ..., to]` where:
/// - `from_node` is the closest already-allocated node to `to`.
/// - The remainder is the shortest sequence of *unallocated* nodes leading to `to`.
///
/// Returns `None` if `to` is unreachable from any allocated node, or if `allocated`
/// is empty.
pub fn shortest_path_from_allocated(
    tree: &PassiveTree,
    allocated: &HashSet<NodeId>,
    to: NodeId,
) -> Option<Vec<NodeId>> {
    if allocated.contains(&to) {
        return Some(vec![to]);
    }
    if allocated.is_empty() {
        return None;
    }
    // BFS *backwards* from `to`. We can step through any node, but stop when we hit
    // any allocated node. This avoids exploring the whole tree N times.
    let mut prev: HashMap<NodeId, NodeId> = HashMap::new();
    let mut q: VecDeque<NodeId> = VecDeque::new();
    q.push_back(to);
    let mut found_root: Option<NodeId> = None;
    while let Some(cur) = q.pop_front() {
        if cur != to && allocated.contains(&cur) {
            found_root = Some(cur);
            break;
        }
        let Some(node) = tree.nodes.get(&cur) else {
            continue;
        };
        for nb in node.out_edges.iter().chain(node.in_edges.iter()) {
            if *nb == cur {
                continue;
            }
            if prev.contains_key(nb) || *nb == to {
                continue;
            }
            prev.insert(*nb, cur);
            q.push_back(*nb);
        }
    }
    let root = found_root?;
    // Reconstruct from root → to via the `prev` chain. `prev[x] = y` means we
    // reached y from x during the backward BFS, so following prev from root lands
    // on `to`.
    let mut path = vec![root];
    let mut cur = root;
    while cur != to {
        let next = *prev.get(&cur)?;
        path.push(next);
        cur = next;
    }
    Some(path)
}

/// Anchor nodes for orphan detection: the character's class start (matched by
/// `class_name` against `tree.classes`) plus an optional ascendancy start
/// (matched by `ascendancy_name`). Returned as a `Vec` because callers need
/// to treat them as a BFS seed.
pub fn anchor_nodes(
    tree: &PassiveTree,
    class_name: &str,
    ascendancy_name: Option<&str>,
) -> Vec<NodeId> {
    let mut starts = Vec::new();
    if let Some(class_idx) = tree
        .classes
        .iter()
        .position(|c| c.name.eq_ignore_ascii_case(class_name))
    {
        let class_idx = class_idx as u32;
        for (id, node) in tree.nodes.iter() {
            if node.class_start_index == Some(class_idx) {
                starts.push(*id);
            }
        }
    }
    if let Some(picked) = ascendancy_name {
        for (id, node) in tree.nodes.iter() {
            if matches!(node.kind, pob_data::NodeKind::AscendancyStart)
                && node
                    .ascendancy_name
                    .as_deref()
                    .is_some_and(|s| s.eq_ignore_ascii_case(picked))
            {
                starts.push(*id);
            }
        }
    }
    starts
}

/// Among `allocated`, return the subset that is reachable from any of `seeds`
/// via edges that only step through allocated nodes (or seeds themselves). The
/// "anchored" set — anything in `allocated` not in this set is orphaned.
pub fn anchored_subset(
    tree: &PassiveTree,
    allocated: &HashSet<NodeId>,
    seeds: &[NodeId],
) -> HashSet<NodeId> {
    let mut anchored: HashSet<NodeId> = HashSet::new();
    let mut queue: VecDeque<NodeId> = VecDeque::new();
    for &s in seeds {
        // A seed is only useful if it's a tree node we can walk from. We don't
        // require it to be in `allocated` (class start / ascendancy start are
        // the synthetic anchors).
        if tree.nodes.contains_key(&s) && anchored.insert(s) {
            queue.push_back(s);
        }
    }
    while let Some(cur) = queue.pop_front() {
        let Some(node) = tree.nodes.get(&cur) else {
            continue;
        };
        for &nb in node.out_edges.iter().chain(node.in_edges.iter()) {
            if !allocated.contains(&nb) {
                continue;
            }
            if anchored.insert(nb) {
                queue.push_back(nb);
            }
        }
    }
    // Restrict to actually-allocated nodes — synthetic seeds shouldn't appear
    // in the result unless the user really allocated them.
    anchored.retain(|id| allocated.contains(id));
    anchored
}

#[cfg(test)]
mod tests {
    use super::*;
    use ahash::HashMap as AHashMap;
    use pob_data::{Class, Node, NodeKind, TreeConstants, TreePoints};
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

    /// Build a tiny linear tree:  start(1) — 2 — 3 — 4 — 5  with a side branch
    /// 3 — 10 — 11. Class "Test" has class_start_index 0 anchored on node 1.
    fn linear_with_branch_tree() -> PassiveTree {
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
        // Mark node 1 as the class start.
        if let Some(n) = tree.nodes.get_mut(&1) {
            n.class_start_index = Some(0);
            n.kind = NodeKind::ClassStart;
        }
        tree
    }

    #[test]
    fn empty_allocated_returns_none() {
        let tree = linear_with_branch_tree();
        let allocated = HashSet::new();
        assert!(shortest_path_from_allocated(&tree, &allocated, 5).is_none());
    }

    #[test]
    fn target_already_allocated_returns_singleton() {
        let tree = linear_with_branch_tree();
        let allocated: HashSet<NodeId> = [1, 2, 3].into_iter().collect();
        assert_eq!(
            shortest_path_from_allocated(&tree, &allocated, 3),
            Some(vec![3])
        );
    }

    #[test]
    fn linear_path_is_returned_in_order() {
        let tree = linear_with_branch_tree();
        let allocated: HashSet<NodeId> = [1].into_iter().collect();
        let path = shortest_path_from_allocated(&tree, &allocated, 5).expect("path");
        assert_eq!(path, vec![1, 2, 3, 4, 5]);
    }

    #[test]
    fn unreachable_target_returns_none() {
        let mut tree = linear_with_branch_tree();
        // Add an island node 99 with no edges.
        add_node(&mut tree, 99, &[]);
        let allocated: HashSet<NodeId> = [1].into_iter().collect();
        assert!(shortest_path_from_allocated(&tree, &allocated, 99).is_none());
    }

    #[test]
    fn anchored_subset_keeps_connected_path() {
        let tree = linear_with_branch_tree();
        let allocated: HashSet<NodeId> = [1, 2, 3, 4].into_iter().collect();
        let seeds = anchor_nodes(&tree, "Test", None);
        assert_eq!(seeds, vec![1]);
        let anchored = anchored_subset(&tree, &allocated, &seeds);
        let mut got: Vec<_> = anchored.into_iter().collect();
        got.sort_unstable();
        assert_eq!(got, vec![1, 2, 3, 4]);
    }

    #[test]
    fn anchored_subset_drops_orphan_branch() {
        let tree = linear_with_branch_tree();
        // Allocated set with a gap at node 3 — 10 and 11 dangle.
        let allocated: HashSet<NodeId> = [1, 2, 10, 11].into_iter().collect();
        let seeds = anchor_nodes(&tree, "Test", None);
        let anchored = anchored_subset(&tree, &allocated, &seeds);
        let mut got: Vec<_> = anchored.into_iter().collect();
        got.sort_unstable();
        assert_eq!(got, vec![1, 2]);
    }
}
