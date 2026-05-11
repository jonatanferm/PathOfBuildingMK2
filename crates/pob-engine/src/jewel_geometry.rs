//! Pure geometry helpers for jewel-radius work — issue #196 follow-up.
//!
//! `jewel_radius.rs` already owns the full radius-jewel dispatch (orbit-angle
//! tables, in-radius enumeration, handler kinds, mod application). This module
//! is intentionally narrower: it exposes a small additive surface that future
//! jewel slices (and any non-radius caller that needs a single-pair distance
//! check) can lean on without going through the full `nodes_in_radius` scan.
//!
//! All functions here are pure projections of [`jewel_radius::node_position`]
//! and [`pob_data::JewelRadiusInfo::contains`] — there is no new geometry
//! logic. The point of factoring them out is testability:
//!
//! * `distance_sq(tree, a, b)` is a one-pair primitive. The full
//!   `nodes_in_radius` scan in `jewel_radius` couples distance-computation
//!   with a `for (id, node) in &tree.nodes` filter loop and the
//!   socket-/mastery-/root-skip rules. Anything that just wants "how far apart
//!   are these two nodes?" had to either copy the math or pay for the full
//!   scan; now it can call this.
//! * `node_in_jewel_radius(tree, socket, candidate, radius)` is a single-pair
//!   inclusion check. It's what `nodes_in_radius` does per element, exposed
//!   so a follow-up that already has a candidate node in hand (e.g. a
//!   timeless-jewel notable lookup, or a UI hover-tooltip "is this in
//!   radius?" query) doesn't need to materialise the whole `Vec`.
//!
//! Both helpers return `false` / `None` for nodes lacking a group, orbit, or
//! orbit-index — same contract as `node_position`.

use pob_data::{JewelRadiusInfo, NodeId, PassiveTree};

use crate::jewel_radius::node_position;

/// Squared Cartesian distance between two tree nodes, in the same coordinate
/// space as `node_position`. Returns `None` if either node is missing or
/// lacks the group/orbit data needed to compute a position (cluster-jewel
/// notable templates, the synthetic root, etc.).
///
/// Squared (rather than `sqrt`'d) because every caller in the jewel-radius
/// dispatch compares against a squared radius; matching their convention
/// avoids a needless `sqrt` per call. Use `.sqrt()` if you need raw distance.
pub fn distance_sq(tree: &PassiveTree, a: NodeId, b: NodeId) -> Option<f64> {
    let (ax, ay) = node_position(tree, a)?;
    let (bx, by) = node_position(tree, b)?;
    let dx = ax - bx;
    let dy = ay - by;
    Some(dx * dx + dy * dy)
}

/// Returns true iff `candidate` falls inside `radius`'s `[inner, outer]` band
/// when measured from `socket_id`. Equivalent to
/// `nodes_in_radius(tree, socket_id, radius).iter().any(|(id, _)| *id == candidate)`
/// minus the `for (id, node) in &tree.nodes` scan and the
/// socket-/mastery-/root-skip filters — callers that already have a specific
/// candidate in hand pay one position lookup, not `O(|nodes|)`.
///
/// Returns `false` (not `None`) for nodes that lack a position; in the
/// jewel-radius dispatch a missing position means "skip", and this matches
/// that: a node that can't be located is by definition not in any radius.
///
/// **Note** — this does *not* re-implement the socket-self / mastery / root
/// exclusions that [`crate::jewel_radius::nodes_in_radius`] applies. Those
/// are handler-policy, not geometry; if your caller needs them, filter
/// before calling, or use `nodes_in_radius` directly.
pub fn node_in_jewel_radius(
    tree: &PassiveTree,
    socket_id: NodeId,
    candidate: NodeId,
    radius: &JewelRadiusInfo,
) -> bool {
    let Some(dist_sq) = distance_sq(tree, socket_id, candidate) else {
        return false;
    };
    radius.contains(dist_sq)
}

#[cfg(test)]
mod tests {
    use super::*;
    use ahash::HashMap;
    use pob_data::{Group, Node, NodeKind, PassiveTree, TreeConstants, TreePoints};
    use smallvec::smallvec;

    /// Toy tree shared by these tests. Lays out nodes at orbit-0 of their own
    /// groups so positions are exactly the group's `(x, y)` — no orbit-angle
    /// math gets in the way of distance assertions.
    ///
    /// Layout:
    /// * `1` — socket at (0, 0)
    /// * `2` — node at (300, 400)  → distance 500 from socket
    /// * `3` — node at (1000, 0)   → distance 1000 from socket
    /// * `4` — node at (0, 1500)   → distance 1500 from socket
    /// * `5` — orphan node with no group (returns None from node_position)
    fn flat_tree() -> PassiveTree {
        let mut groups: HashMap<u32, Group> = HashMap::default();
        for (gid, x, y, node_id) in [
            (10, 0.0, 0.0, 1u32),
            (20, 300.0, 400.0, 2),
            (30, 1000.0, 0.0, 3),
            (40, 0.0, 1500.0, 4),
        ] {
            groups.insert(
                gid,
                Group {
                    x,
                    y,
                    orbits: smallvec![0],
                    background: None,
                    nodes: vec![node_id],
                    is_proxy: false,
                },
            );
        }
        let mut nodes: HashMap<NodeId, Node> = HashMap::default();
        for (id, group_id, kind) in [
            (1u32, Some(10u32), NodeKind::JewelSocket),
            (2, Some(20), NodeKind::Notable),
            (3, Some(30), NodeKind::Notable),
            (4, Some(40), NodeKind::Notable),
            // Orphan: no group → node_position returns None.
            (5, None, NodeKind::Notable),
        ] {
            nodes.insert(
                id,
                Node {
                    id,
                    name: None,
                    icon: None,
                    ascendancy_name: None,
                    stats: vec![],
                    reminder_text: vec![],
                    kind,
                    class_start_index: None,
                    group: group_id,
                    orbit: group_id.map(|_| 0),
                    orbit_index: group_id.map(|_| 0),
                    out_edges: smallvec![],
                    in_edges: smallvec![],
                    mastery_effects: vec![],
                    expansion_jewel_size: None,
                    jewel_radius: None,
                },
            );
        }
        PassiveTree {
            version: "3_25".into(),
            tree: "Default".into(),
            classes: vec![],
            groups,
            nodes,
            jewel_slots: vec![1],
            min_x: -2000,
            min_y: -2000,
            max_x: 2000,
            max_y: 2000,
            constants: TreeConstants {
                skills_per_orbit: vec![1, 6, 16, 16, 40, 72, 72],
                orbit_radii: vec![0, 82, 162, 335, 493, 662, 846],
                classes: HashMap::default(),
                character_attributes: HashMap::default(),
                pss_centre_inner_radius: None,
            },
            points: TreePoints::default(),
        }
    }

    #[test]
    fn distance_sq_classic_3_4_5_triangle() {
        let tree = flat_tree();
        // (0,0) ↔ (300, 400) → 500² = 250_000
        let d = distance_sq(&tree, 1, 2).expect("both positions exist");
        assert!((d - 250_000.0).abs() < 1e-6, "got {d}");
    }

    #[test]
    fn distance_sq_zero_for_same_node() {
        let tree = flat_tree();
        let d = distance_sq(&tree, 1, 1).expect("self distance defined");
        assert_eq!(d, 0.0);
    }

    #[test]
    fn distance_sq_is_symmetric() {
        let tree = flat_tree();
        let ab = distance_sq(&tree, 2, 3).expect("ab");
        let ba = distance_sq(&tree, 3, 2).expect("ba");
        assert!((ab - ba).abs() < 1e-9, "{ab} != {ba}");
    }

    #[test]
    fn distance_sq_none_when_either_node_missing() {
        let tree = flat_tree();
        // Node 999 doesn't exist.
        assert!(distance_sq(&tree, 1, 999).is_none());
        assert!(distance_sq(&tree, 999, 1).is_none());
    }

    #[test]
    fn distance_sq_none_when_node_lacks_group() {
        let tree = flat_tree();
        // Node 5 exists but has no group → no position → None.
        assert!(distance_sq(&tree, 1, 5).is_none());
        assert!(distance_sq(&tree, 5, 1).is_none());
    }

    #[test]
    fn node_in_radius_includes_node_inside_outer_bound() {
        let tree = flat_tree();
        // Small band 0..960; node 2 is 500 away → inside.
        let small = pob_data::RADII_3_16[0];
        assert!(node_in_jewel_radius(&tree, 1, 2, &small));
    }

    #[test]
    fn node_in_radius_excludes_node_past_outer_bound() {
        let tree = flat_tree();
        // Small band 0..960; node 3 is 1000 away → outside.
        let small = pob_data::RADII_3_16[0];
        assert!(!node_in_jewel_radius(&tree, 1, 3, &small));
    }

    #[test]
    fn node_in_radius_inclusive_at_outer_boundary() {
        let tree = flat_tree();
        // Custom band whose outer is exactly node 3's distance (1000).
        let band = JewelRadiusInfo::new(0.0, 1000.0, "test");
        assert!(
            node_in_jewel_radius(&tree, 1, 3, &band),
            "outer bound is inclusive — distance² == outer²"
        );
    }

    #[test]
    fn node_in_radius_inclusive_at_inner_boundary() {
        let tree = flat_tree();
        // Donut band whose inner is exactly node 2's distance (500).
        // Node 2 sits exactly on the inner edge → must be included.
        let band = JewelRadiusInfo::new(500.0, 2000.0, "donut");
        assert!(
            node_in_jewel_radius(&tree, 1, 2, &band),
            "inner bound is inclusive — distance² == inner²"
        );
    }

    #[test]
    fn node_in_radius_donut_excludes_inner_hole() {
        let tree = flat_tree();
        // Variable band 960..1320; node 2 (500 away) is in the hole.
        let donut = pob_data::RADII_3_16[5];
        assert!(!node_in_jewel_radius(&tree, 1, 2, &donut));
        // Node 3 (1000 away) is in the band.
        assert!(node_in_jewel_radius(&tree, 1, 3, &donut));
        // Node 4 (1500 away) is past the outer.
        assert!(!node_in_jewel_radius(&tree, 1, 4, &donut));
    }

    #[test]
    fn node_in_radius_false_when_candidate_lacks_position() {
        let tree = flat_tree();
        // Orphan node 5 has no group → no position → not in any radius.
        // This must be `false`, not a panic, and not `true` by accident.
        let huge = JewelRadiusInfo::new(0.0, 1.0e9, "huge");
        assert!(!node_in_jewel_radius(&tree, 1, 5, &huge));
    }

    #[test]
    fn node_in_radius_false_when_socket_missing() {
        let tree = flat_tree();
        let small = pob_data::RADII_3_16[0];
        assert!(!node_in_jewel_radius(&tree, 999, 2, &small));
    }

    #[test]
    fn node_in_radius_self_distance_zero_in_zero_inner_band() {
        let tree = flat_tree();
        // Socket-as-candidate: dist² == 0, which is contained in any
        // 0-inner band. Geometry says yes — `nodes_in_radius` filters the
        // socket separately as a handler policy, which is exactly why this
        // helper deliberately stays geometric-only.
        let small = pob_data::RADII_3_16[0];
        assert!(node_in_jewel_radius(&tree, 1, 1, &small));
    }
}
