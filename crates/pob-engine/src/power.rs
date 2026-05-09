//! Build-power scoring — issue [#207](https://github.com/jonatanferm/PathOfBuildingMK2/issues/207).
//!
//! For a given candidate node, this module recomputes the build with that node
//! added to the allocation set and reports the delta on `MainSkillDPS` and
//! `TotalEHP`. Mirrors the engine half of upstream PoB's
//! `Modules/CalcsTab.lua` power-report driver.
//!
//! This slice covers the **single-node addition** path — the foundational
//! primitive a future tree-overlay heatmap and Power Report list would call
//! once per candidate. Removal-scoring (per-node DPS / EHP contribution by
//! "what if I didn't have this") and per-modline scoring on equipped items are
//! tracked as follow-ups under the same issue.
//!
//! ## Performance
//!
//! Each call clones the [`Character`] and runs a full
//! [`compute_full_with_clusters_and_timeless`] pass, so a tree-wide overlay
//! is N+1 perform calls — measurable but acceptable for one-shot scoring on
//! the click of a "Show node power" button. The future heatmap will need
//! caching / incremental compute to stay smooth at hover-over rates.

use pob_data::{NodeId, PassiveTree};

use crate::character::Character;
use crate::perform::{compute_full_with_clusters_and_timeless, ClusterContext};
use crate::skill::SkillRegistry;

/// Power-score result for a single candidate node.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct NodeScore {
    /// The candidate node the score was computed for.
    pub node_id: NodeId,
    /// `MainSkillDPS_after − MainSkillDPS_before`. Negative for nodes whose
    /// allocation accidentally drops DPS (rare but possible — e.g. a
    /// keystone with a downside, or a path mistake the user is exploring).
    pub dps_delta: f64,
    /// `TotalEHP_after − TotalEHP_before`. Captures pure-defence nodes the
    /// `dps_delta` reading would miss.
    pub ehp_delta: f64,
}

/// Score the marginal value of adding `target_node` to the character's
/// allocation. Returns `None` when the target is already allocated (the
/// score for a no-op is zero everywhere — caller should branch upstream).
///
/// The scoring is purely additive: it inserts `target_node` into the
/// allocated set without growing a path through neighbours. This matches
/// PoB's Power Report semantics — "what does this node alone contribute"
/// — but assumes the node is reachable in the player's current tree (or
/// will be allocated as part of a longer click-chain). Tree-overlay UI
/// callers can guard with [`crate::Character::pathfind_seeds`] before
/// scoring to skip definitely-unreachable nodes.
///
/// `cluster_ctx` and `timeless` are threaded through verbatim so cluster-
/// jewel sub-graphs and timeless keystone overrides match the build's
/// active calc context — without them the after-pass would silently shed
/// any cluster / timeless contributions the baseline picked up.
pub fn score_node_addition(
    character: &Character,
    tree: &PassiveTree,
    target_node: NodeId,
    skills: Option<&SkillRegistry>,
    bases: Option<&pob_data::bases::ItemBaseSet>,
    cluster_ctx: Option<ClusterContext<'_>>,
    timeless: Option<&pob_data::TimelessJewelData>,
) -> Option<NodeScore> {
    if character.allocated.contains(&target_node) {
        return None;
    }
    let (baseline, _) = compute_full_with_clusters_and_timeless(
        character,
        tree,
        skills,
        bases,
        cluster_ctx,
        timeless,
    );
    let mut probe = character.clone();
    probe.allocated.insert(target_node);
    let (after, _) =
        compute_full_with_clusters_and_timeless(&probe, tree, skills, bases, cluster_ctx, timeless);
    Some(NodeScore {
        node_id: target_node,
        dps_delta: after.get("MainSkillDPS") - baseline.get("MainSkillDPS"),
        ehp_delta: after.get("TotalEHP") - baseline.get("TotalEHP"),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use ahash::HashMap as AHashMap;
    use pob_data::{Class, Node, NodeKind, TreeConstants, TreePoints};
    use smallvec::SmallVec;

    use crate::character::{Character, ClassRef};

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

    /// Tree with one class-start anchor and two leaf nodes — one grants
    /// `+50 to maximum Life` (impactful for EHP), the other has no stats
    /// (zero contribution). Both are reachable from the start.
    fn life_notable_tree() -> PassiveTree {
        let mut tree = empty_tree();
        tree.classes.push(Class {
            name: "Test".into(),
            base_str: 32,
            base_dex: 14,
            base_int: 14,
            ascendancies: vec![],
        });
        let mut add = |id: NodeId, neighbours: &[NodeId], stats: Vec<String>| {
            let node = Node {
                id,
                name: Some(format!("n{id}")),
                icon: None,
                ascendancy_name: None,
                stats,
                reminder_text: vec![],
                kind: NodeKind::Notable,
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
        };
        add(1, &[2, 3], vec![]);
        add(2, &[1], vec!["+50 to maximum Life".into()]);
        add(3, &[1], vec![]);
        if let Some(n) = tree.nodes.get_mut(&1) {
            n.class_start_index = Some(0);
            n.kind = NodeKind::ClassStart;
        }
        tree
    }

    fn fresh_character() -> Character {
        Character {
            class: ClassRef("Test".into()),
            level: 90,
            ..Character::default()
        }
    }

    /// Issue #207 (slice 1): scoring an unallocated Life notable returns a
    /// positive `ehp_delta` because `+50 to maximum Life` raises the EHP
    /// pool the basic-stats pass computes. The DPS delta stays at zero —
    /// no main-skill is set on this character so MainSkillDPS = 0 in
    /// both passes.
    #[test]
    fn score_node_addition_picks_up_life_notable_ehp_gain() {
        let tree = life_notable_tree();
        let c = fresh_character();
        let score = score_node_addition(&c, &tree, 2, None, None, None, None).expect("scored");
        assert_eq!(score.node_id, 2);
        assert!(
            score.ehp_delta > 0.0,
            "+50 Life notable should raise EHP; got {}",
            score.ehp_delta
        );
        assert_eq!(score.dps_delta, 0.0);
    }

    /// Issue #207 (slice 1): scoring a node with no stats returns a
    /// zero-delta score — allocating it costs a point but contributes
    /// nothing. UI consumers can hide zero-score rows so the heatmap
    /// only highlights worthwhile candidates.
    #[test]
    fn score_node_addition_zero_for_stat_less_node() {
        let tree = life_notable_tree();
        let c = fresh_character();
        let score = score_node_addition(&c, &tree, 3, None, None, None, None).expect("scored");
        assert_eq!(score.node_id, 3);
        assert_eq!(score.ehp_delta, 0.0);
        assert_eq!(score.dps_delta, 0.0);
    }

    /// Issue #207 (slice 1): scoring an already-allocated node returns
    /// `None`. This lets UI callers distinguish "no contribution" (zero
    /// score) from "no-op — already in your tree" (None) without
    /// branching on a sentinel value.
    #[test]
    fn score_node_addition_returns_none_for_already_allocated() {
        let tree = life_notable_tree();
        let mut c = fresh_character();
        c.allocate(2);
        let score = score_node_addition(&c, &tree, 2, None, None, None, None);
        assert!(score.is_none());
    }
}
