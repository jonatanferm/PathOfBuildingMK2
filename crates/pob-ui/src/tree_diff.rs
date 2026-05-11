//! Issue #220 slice 1 — pure data layer for the "Spec comparison
//! overlay" affordance. PoB lets users overlay a second passive-tree
//! spec on the current tree (`TreeTab.lua:106-125`), with added,
//! removed, and common nodes coloured differently.
//!
//! MK2's `Character` model is single-spec today (no `specList`), so
//! this slice ships only the diff helper plus a Compare-tab row that
//! surfaces "+N / -N nodes vs snapshot". The actual in-tree overlay
//! rendering using the diff is a follow-up slice once a multi-spec
//! model lands.
//!
//! The lists are sorted ascending so the output is deterministic for
//! snapshot tests, mirroring the precedent set by
//! `compute_version_swap_diff` in `lib.rs`.
//!
//! Pure function — no egui, no `PassiveTree`, no I/O. Trivially unit-
//! testable.

use pob_data::NodeId;
use std::collections::HashSet;

/// Set-difference between two allocated-node sets.
///
/// * `added` — in `b` but not `a` (the "current" gained these vs the
///   "snapshot" / `a` baseline).
/// * `removed` — in `a` but not `b` (lost vs baseline).
/// * `common` — in both.
///
/// All three vectors are sorted ascending.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct TreeDiff {
    pub added: Vec<NodeId>,
    pub removed: Vec<NodeId>,
    pub common: Vec<NodeId>,
}

/// Diff `b` against the `a` baseline.
///
/// Convention matches the Compare tab: `a` is the snapshot, `b` is the
/// live build. So `added` = "live gained these", `removed` = "live no
/// longer has these".
#[must_use]
pub fn tree_diff(allocated_a: &HashSet<NodeId>, allocated_b: &HashSet<NodeId>) -> TreeDiff {
    let mut added: Vec<NodeId> = allocated_b.difference(allocated_a).copied().collect();
    let mut removed: Vec<NodeId> = allocated_a.difference(allocated_b).copied().collect();
    let mut common: Vec<NodeId> = allocated_a.intersection(allocated_b).copied().collect();
    added.sort_unstable();
    removed.sort_unstable();
    common.sort_unstable();
    TreeDiff {
        added,
        removed,
        common,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ids(xs: &[NodeId]) -> HashSet<NodeId> {
        xs.iter().copied().collect()
    }

    fn vec_ids(xs: &[NodeId]) -> Vec<NodeId> {
        xs.to_vec()
    }

    #[test]
    fn identical_sets_have_only_common() {
        let a = ids(&[10, 20, 30]);
        let b = ids(&[10, 20, 30]);
        let diff = tree_diff(&a, &b);
        assert_eq!(diff.added, Vec::<NodeId>::new());
        assert_eq!(diff.removed, Vec::<NodeId>::new());
        assert_eq!(diff.common, vec_ids(&[10, 20, 30]));
    }

    #[test]
    fn a_superset_of_b_marks_extras_as_removed() {
        // `a` (snapshot) had extra nodes that `b` (live) no longer has.
        let a = ids(&[1, 2, 3, 4, 5]);
        let b = ids(&[1, 2, 3]);
        let diff = tree_diff(&a, &b);
        assert_eq!(diff.added, Vec::<NodeId>::new());
        assert_eq!(diff.removed, vec_ids(&[4, 5]));
        assert_eq!(diff.common, vec_ids(&[1, 2, 3]));
    }

    #[test]
    fn b_superset_of_a_marks_extras_as_added() {
        let a = ids(&[1, 2, 3]);
        let b = ids(&[1, 2, 3, 4, 5]);
        let diff = tree_diff(&a, &b);
        assert_eq!(diff.added, vec_ids(&[4, 5]));
        assert_eq!(diff.removed, Vec::<NodeId>::new());
        assert_eq!(diff.common, vec_ids(&[1, 2, 3]));
    }

    #[test]
    fn partial_overlap_splits_into_three_buckets() {
        // Insertion order is intentionally scrambled to prove the
        // function sorts each output vector.
        let a = ids(&[30, 10, 20, 40]);
        let b = ids(&[20, 50, 10, 60]);
        let diff = tree_diff(&a, &b);
        assert_eq!(diff.added, vec_ids(&[50, 60]));
        assert_eq!(diff.removed, vec_ids(&[30, 40]));
        assert_eq!(diff.common, vec_ids(&[10, 20]));
    }

    #[test]
    fn both_empty_yields_empty_diff() {
        let a: HashSet<NodeId> = HashSet::new();
        let b: HashSet<NodeId> = HashSet::new();
        let diff = tree_diff(&a, &b);
        assert_eq!(diff, TreeDiff::default());
    }

    #[test]
    fn empty_a_treats_everything_in_b_as_added() {
        let a: HashSet<NodeId> = HashSet::new();
        let b = ids(&[7, 3, 11]);
        let diff = tree_diff(&a, &b);
        assert_eq!(diff.added, vec_ids(&[3, 7, 11]));
        assert_eq!(diff.removed, Vec::<NodeId>::new());
        assert_eq!(diff.common, Vec::<NodeId>::new());
    }

    #[test]
    fn empty_b_treats_everything_in_a_as_removed() {
        let a = ids(&[7, 3, 11]);
        let b: HashSet<NodeId> = HashSet::new();
        let diff = tree_diff(&a, &b);
        assert_eq!(diff.added, Vec::<NodeId>::new());
        assert_eq!(diff.removed, vec_ids(&[3, 7, 11]));
        assert_eq!(diff.common, Vec::<NodeId>::new());
    }

    #[test]
    fn disjoint_sets_have_no_common() {
        let a = ids(&[1, 2, 3]);
        let b = ids(&[10, 20, 30]);
        let diff = tree_diff(&a, &b);
        assert_eq!(diff.added, vec_ids(&[10, 20, 30]));
        assert_eq!(diff.removed, vec_ids(&[1, 2, 3]));
        assert_eq!(diff.common, Vec::<NodeId>::new());
    }
}
