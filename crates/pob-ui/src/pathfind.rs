//! Breadth-first shortest path between two nodes on the passive tree.
//!
//! Mirrors PoB's allocation pathfinding: from any node in `from` (typically the set of
//! already-allocated nodes), find the shortest sequence of unallocated nodes that
//! connects to `to`. We treat node→node connections as undirected (PoB stores both
//! `out` and `in` edges).

use std::collections::{HashMap, VecDeque};

use pob_data::{NodeId, PassiveTree};

/// Returns a path `[from_node, ..., to]` where:
/// - `from_node` is the closest already-allocated node to `to`.
/// - The remainder is the shortest sequence of *unallocated* nodes leading to `to`.
///
/// Returns `None` if `to` is unreachable (e.g. behind unallocated keystones at a
/// different class start).
pub fn shortest_path_from_allocated(
    tree: &PassiveTree,
    allocated: &std::collections::HashSet<NodeId>,
    to: NodeId,
) -> Option<Vec<NodeId>> {
    if allocated.contains(&to) {
        return Some(vec![to]);
    }
    // BFS *backwards* from `to`. We can step through any node, but stop when we hit any
    // allocated node. This avoids exploring the whole tree N times.
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
    // Reconstruct from root → to via the `prev` chain. `prev[x] = y` means we reached y
    // from x during the backward BFS, so following prev from root lands on `to`.
    let mut path = vec![root];
    let mut cur = root;
    while cur != to {
        let next = *prev.get(&cur)?;
        path.push(next);
        cur = next;
    }
    Some(path)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;

    #[test]
    fn empty_tree_returns_none() {
        let tree = PassiveTree {
            version: "test".into(),
            tree: "test".into(),
            classes: vec![],
            groups: ahash::HashMap::default(),
            nodes: ahash::HashMap::default(),
            jewel_slots: vec![],
            min_x: 0,
            min_y: 0,
            max_x: 0,
            max_y: 0,
            constants: pob_data::TreeConstants {
                skills_per_orbit: vec![],
                orbit_radii: vec![],
                classes: ahash::HashMap::default(),
                character_attributes: ahash::HashMap::default(),
                pss_centre_inner_radius: None,
            },
            points: pob_data::TreePoints::default(),
        };
        let allocated = HashSet::new();
        assert!(shortest_path_from_allocated(&tree, &allocated, 1).is_none());
    }

    #[test]
    fn finds_path_in_3_25_tree() {
        let path = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .unwrap()
            .parent()
            .unwrap()
            .join("data/trees/3_25.json");
        let Ok(json) = std::fs::read_to_string(&path) else {
            eprintln!("skip: data missing");
            return;
        };
        let tree: PassiveTree = pob_data::load_passive_tree(&json).unwrap();
        // Pick the Marauder class start node and any node 4-5 hops away.
        let class_start = tree
            .nodes
            .iter()
            .find(|(_, n)| {
                n.class_start_index.is_some()
                    && n.name
                        .as_deref()
                        .map(|s| s.contains("MARAUDER"))
                        .unwrap_or(false)
            })
            .map(|(id, _)| *id);
        let Some(start) = class_start else {
            eprintln!("no Marauder start found — skip");
            return;
        };
        let allocated: HashSet<NodeId> = [start].into_iter().collect();
        let target = tree
            .nodes
            .get(&start)
            .and_then(|n| n.out_edges.first().copied())
            .expect("start has neighbour");
        let path = shortest_path_from_allocated(&tree, &allocated, target).expect("path");
        assert!(!path.is_empty());
        assert_eq!(path[0], start);
        assert_eq!(*path.last().unwrap(), target);
    }
}
