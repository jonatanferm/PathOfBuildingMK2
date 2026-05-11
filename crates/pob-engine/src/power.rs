//! Build-power scoring — issue [#207](https://github.com/jonatanferm/PathOfBuildingMK2/issues/207).
//!
//! For a given candidate node, this module recomputes the build with that node
//! added to the allocation set and reports the delta on `MainSkillDPS` and
//! `TotalEHP`. Mirrors the engine half of upstream PoB's
//! `Modules/CalcsTab.lua` power-report driver.
//!
//! Slices in this module:
//!
//! - **single-node addition** ([`score_node_addition`]) — "what would
//!   allocating this node be worth"
//! - **single-node removal** ([`score_node_removal`]) — "what is this
//!   already-allocated node currently worth to the build"
//! - **tree-wide ranking** ([`rank_node_additions`]) — Power-Report-style
//!   sorted list of every unallocated allocatable node
//! - **per-modline contribution** ([`score_item_modline_removal`] /
//!   [`rank_item_modlines`]) — items-tab "top-N contributing modlines for
//!   this slot" view, scored by removing one mod line at a time
//! - **item swap** ([`score_item_swap`]) — single-call DPS/EHP delta for
//!   swapping a candidate item into a slot, powering the items-tab
//!   "vs equipped" hover (#203 follow-up)
//! - **shared-item ranking** ([`rank_shared_items_for_slot`]) — batch
//!   ranker over a candidate list, returning `(candidate_index, score)`
//!   pairs sorted by impact for the items-tab "best swap-in" panel
//!
//! ## Performance
//!
//! Each call clones the [`Character`] and runs a full
//! [`compute_full_with_clusters_and_timeless`] pass, so a tree-wide overlay
//! is N+1 perform calls — measurable but acceptable for one-shot scoring on
//! the click of a "Show node power" button. The future heatmap will need
//! caching / incremental compute to stay smooth at hover-over rates.

use pob_data::{NodeId, PassiveTree, Slot};

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

/// Score the contribution of an allocated `target_node` by recomputing the
/// build with that node removed and reporting the resulting losses. Returns
/// `None` when the target isn't currently allocated (you can't measure the
/// contribution of a node that isn't pulling its weight).
///
/// **Sign convention**: matches [`score_node_addition`] — positive deltas
/// mean "the player gains by acting on this node". For removal, that
/// translates to "the player loses by not having this node", so the
/// implementation returns `baseline − after` (rather than the raw
/// `after − baseline` used for additions). Both functions therefore agree
/// that a positive number is *good for the player*: take an
/// addition-positive node, keep a removal-positive node.
///
/// **Cascade handling**: removing a node that bridges to other notables
/// will orphan-cascade those notables out of the active calc through the
/// existing `connected_allocations` BFS, so the reported delta naturally
/// includes the chain's full contribution. This matches PoB's Power
/// Report semantics — a single-node bridge "contributes" everything its
/// subtree was carrying. Callers that want pure per-node scoring should
/// pre-filter to leaf or notable-with-no-downstream candidates.
pub fn score_node_removal(
    character: &Character,
    tree: &PassiveTree,
    target_node: NodeId,
    skills: Option<&SkillRegistry>,
    bases: Option<&pob_data::bases::ItemBaseSet>,
    cluster_ctx: Option<ClusterContext<'_>>,
    timeless: Option<&pob_data::TimelessJewelData>,
) -> Option<NodeScore> {
    if !character.allocated.contains(&target_node) {
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
    probe.allocated.remove(&target_node);
    let (after, _) =
        compute_full_with_clusters_and_timeless(&probe, tree, skills, bases, cluster_ctx, timeless);
    Some(NodeScore {
        node_id: target_node,
        dps_delta: baseline.get("MainSkillDPS") - after.get("MainSkillDPS"),
        ehp_delta: baseline.get("TotalEHP") - after.get("TotalEHP"),
    })
}

/// Score every unallocated, allocatable tree node and return the results
/// sorted by maximum impact descending. The "impact" sort key is
/// `max(dps_delta, ehp_delta)` so a node that boosts only EHP ranks
/// alongside one that boosts only DPS — both surface to the user as
/// "this is a good thing to take next".
///
/// **Filtering**: nodes whose `kind` isn't allocatable
/// (`Mastery` / `Root` / `ClassStart` / `AscendancyStart`) are skipped
/// outright. Nodes that score zero on both axes are also dropped — the
/// list returned represents only candidates with measurable impact, so
/// callers can render it directly without re-filtering.
///
/// **Performance**: N+1 perform calls — one baseline + one per
/// candidate. On a real ~2000-node tree this is multi-second; the
/// future tree-overlay heatmap will need caching / incremental compute
/// to stay smooth at hover-over rates. Acceptable for one-shot
/// "show me the Power Report" button clicks.
///
/// Mirrors PoB's `Modules/CalcsTab.lua:powerReport` driver.
pub fn rank_node_additions(
    character: &Character,
    tree: &PassiveTree,
    skills: Option<&SkillRegistry>,
    bases: Option<&pob_data::bases::ItemBaseSet>,
    cluster_ctx: Option<ClusterContext<'_>>,
    timeless: Option<&pob_data::TimelessJewelData>,
) -> Vec<NodeScore> {
    use pob_data::NodeKind;
    let (baseline, _) = compute_full_with_clusters_and_timeless(
        character,
        tree,
        skills,
        bases,
        cluster_ctx,
        timeless,
    );
    let baseline_dps = baseline.get("MainSkillDPS");
    let baseline_ehp = baseline.get("TotalEHP");

    let mut scores: Vec<NodeScore> = Vec::new();
    for (&id, node) in &tree.nodes {
        if character.allocated.contains(&id) {
            continue;
        }
        // Skip non-allocatable kinds. The runtime engine filters these
        // anyway via `connected_allocations`, but pre-skipping saves an
        // entire perform call per non-allocatable node.
        if matches!(
            node.kind,
            NodeKind::Mastery | NodeKind::Root | NodeKind::ClassStart | NodeKind::AscendancyStart
        ) {
            continue;
        }
        let mut probe = character.clone();
        probe.allocated.insert(id);
        let (after, _) = compute_full_with_clusters_and_timeless(
            &probe,
            tree,
            skills,
            bases,
            cluster_ctx,
            timeless,
        );
        let dps_delta = after.get("MainSkillDPS") - baseline_dps;
        let ehp_delta = after.get("TotalEHP") - baseline_ehp;
        if dps_delta.abs() < 1e-9 && ehp_delta.abs() < 1e-9 {
            continue;
        }
        scores.push(NodeScore {
            node_id: id,
            dps_delta,
            ehp_delta,
        });
    }
    scores.sort_by(|a, b| {
        let ka = a.dps_delta.max(a.ehp_delta);
        let kb = b.dps_delta.max(b.ehp_delta);
        // Descending impact; tie-break on node id so the order is stable
        // across runs (HashMap iteration is not).
        kb.partial_cmp(&ka)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then(a.node_id.cmp(&b.node_id))
    });
    scores
}

/// Issue #207 (slice 5): power-score result for swapping a candidate
/// item into a given slot. Mirrors [`ItemModlineScore`] but with the
/// **swap** sign convention: positive deltas mean "this swap is an
/// upgrade for the player" — the natural reading for the items-tab
/// "vs equipped" hover (#203 follow-up).
#[derive(Debug, Clone, PartialEq)]
pub struct ItemSwapScore {
    pub slot: Slot,
    /// `MainSkillDPS_after − MainSkillDPS_baseline` (positive = swap improves DPS).
    pub dps_delta: f64,
    /// `TotalEHP_after − TotalEHP_baseline` (positive = swap improves EHP).
    pub ehp_delta: f64,
}

/// Score swapping `candidate` into `slot`, replacing whatever's
/// currently equipped (or filling an empty slot). Returns deltas with
/// the **swap** sign convention — positive = upgrade. Pure helper:
/// the input character is unchanged, the candidate is cloned into a
/// probe.
///
/// **Performance**: 2 perform calls — one baseline, one with the
/// swap applied. Cheap enough for hover-rate refresh on a single
/// item, but the items-tab "compare every shared item" view would
/// want batching.
pub fn score_item_swap(
    character: &Character,
    tree: &PassiveTree,
    slot: Slot,
    candidate: &pob_data::Item,
    skills: Option<&SkillRegistry>,
    bases: Option<&pob_data::bases::ItemBaseSet>,
    cluster_ctx: Option<ClusterContext<'_>>,
    timeless: Option<&pob_data::TimelessJewelData>,
) -> ItemSwapScore {
    let (baseline, _) = compute_full_with_clusters_and_timeless(
        character,
        tree,
        skills,
        bases,
        cluster_ctx,
        timeless,
    );
    let mut probe = character.clone();
    probe.items.equip(slot, candidate.clone());
    let (after, _) =
        compute_full_with_clusters_and_timeless(&probe, tree, skills, bases, cluster_ctx, timeless);
    ItemSwapScore {
        slot,
        dps_delta: after.get("MainSkillDPS") - baseline.get("MainSkillDPS"),
        ehp_delta: after.get("TotalEHP") - baseline.get("TotalEHP"),
    }
}

/// Issue #207 (slice 6): rank a list of candidate items for a slot
/// by [`score_item_swap`] impact, returning `(candidate_index, score)`
/// pairs sorted by `max(dps_delta, ehp_delta)` descending.
///
/// The pairing preserves the caller's original candidate ordering so
/// the items-tab can map a result row straight back to the source
/// shared-item entry without re-running the score lookup.
///
/// **Filtering**: nothing is dropped — even negative-impact swaps are
/// returned. A swap that would *lose* DPS or EHP is just as
/// informative as one that would gain it; the consumer's UI can pick
/// where to draw the cutoff.
///
/// **Performance**: 1 baseline + N candidate perform calls.
pub fn rank_shared_items_for_slot(
    character: &Character,
    tree: &PassiveTree,
    slot: Slot,
    candidates: &[pob_data::Item],
    skills: Option<&SkillRegistry>,
    bases: Option<&pob_data::bases::ItemBaseSet>,
    cluster_ctx: Option<ClusterContext<'_>>,
    timeless: Option<&pob_data::TimelessJewelData>,
) -> Vec<(usize, ItemSwapScore)> {
    if candidates.is_empty() {
        return Vec::new();
    }
    let (baseline, _) = compute_full_with_clusters_and_timeless(
        character,
        tree,
        skills,
        bases,
        cluster_ctx,
        timeless,
    );
    let baseline_dps = baseline.get("MainSkillDPS");
    let baseline_ehp = baseline.get("TotalEHP");

    let mut scored: Vec<(usize, ItemSwapScore)> = Vec::with_capacity(candidates.len());
    for (idx, candidate) in candidates.iter().enumerate() {
        let mut probe = character.clone();
        probe.items.equip(slot, candidate.clone());
        let (after, _) = compute_full_with_clusters_and_timeless(
            &probe,
            tree,
            skills,
            bases,
            cluster_ctx,
            timeless,
        );
        scored.push((
            idx,
            ItemSwapScore {
                slot,
                dps_delta: after.get("MainSkillDPS") - baseline_dps,
                ehp_delta: after.get("TotalEHP") - baseline_ehp,
            },
        ));
    }
    scored.sort_by(|(_, a), (_, b)| {
        let a_max = a.dps_delta.max(a.ehp_delta);
        let b_max = b.dps_delta.max(b.ehp_delta);
        b_max
            .partial_cmp(&a_max)
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    scored
}

/// Power-score result for a single mod line on an equipped item.
#[derive(Debug, Clone, PartialEq)]
pub struct ItemModlineScore {
    /// Slot of the equipped item the mod line lives on.
    pub slot: Slot,
    /// Index into the original `Item::mod_lines` vector — stable across
    /// the call so callers can map a result row back to the source line
    /// without re-parsing.
    pub mod_index: usize,
    /// The mod line text itself, copied so callers can render it without
    /// holding a borrow on the character.
    pub mod_line: String,
    /// `MainSkillDPS_baseline − MainSkillDPS_after` (sign convention from
    /// [`score_node_removal`]: positive = good for the player to keep).
    pub dps_delta: f64,
    /// `TotalEHP_baseline − TotalEHP_after`.
    pub ehp_delta: f64,
}

/// Score the contribution of a single mod line on an equipped item by
/// recomputing the build with that line stripped from the slot's item
/// and reporting the resulting losses. Returns `None` when the slot is
/// empty or `mod_index` is out of range.
///
/// The sign convention matches [`score_node_removal`]: positive deltas
/// mean "this mod is pulling its weight". A neutral or negative delta
/// flags a mod line that's contributing nothing or actively hurting
/// (rare — e.g. a "Take X% more Damage from Hits" corruption that costs
/// EHP).
///
/// Only the mod line at `mod_index` is removed; every other line on the
/// item, every other equipped item, and the rest of the calc context
/// (cluster sub-graphs, timeless replacements, skills, etc.) flow
/// through verbatim.
pub fn score_item_modline_removal(
    character: &Character,
    tree: &PassiveTree,
    slot: Slot,
    mod_index: usize,
    skills: Option<&SkillRegistry>,
    bases: Option<&pob_data::bases::ItemBaseSet>,
    cluster_ctx: Option<ClusterContext<'_>>,
    timeless: Option<&pob_data::TimelessJewelData>,
) -> Option<ItemModlineScore> {
    let item = character.items.get(slot)?;
    if mod_index >= item.mod_lines.len() {
        return None;
    }
    let line_text = item.mod_lines[mod_index].line.clone();
    let (baseline, _) = compute_full_with_clusters_and_timeless(
        character,
        tree,
        skills,
        bases,
        cluster_ctx,
        timeless,
    );
    let mut probe = character.clone();
    if let Some(probe_item) = probe.items.items.get_mut(&slot) {
        probe_item.mod_lines.remove(mod_index);
    }
    let (after, _) =
        compute_full_with_clusters_and_timeless(&probe, tree, skills, bases, cluster_ctx, timeless);
    Some(ItemModlineScore {
        slot,
        mod_index,
        mod_line: line_text,
        dps_delta: baseline.get("MainSkillDPS") - after.get("MainSkillDPS"),
        ehp_delta: baseline.get("TotalEHP") - after.get("TotalEHP"),
    })
}

/// Score every mod line on the item equipped at `slot` and return the
/// results sorted by maximum impact descending. Empty (whitespace-only)
/// mod lines are skipped — they'd score zero by definition and clutter
/// the items-tab "top contributors" list. Zero-score lines are *not*
/// dropped: a corrupted or veiled line that contributes nothing is
/// itself useful information for the user.
///
/// Returns an empty vector when the slot is empty.
///
/// **Performance**: M+1 perform calls for an item with M mod lines. A
/// fully-modded rare has 6 explicits + implicits + crafted ≈ 8–10 calls
/// per slot, so the full items-tab pass is ~10 × #equipped ≈ 100
/// perform calls — fine for an explicit "Show modline contributions"
/// click, painful for a hover-rate refresh.
pub fn rank_item_modlines(
    character: &Character,
    tree: &PassiveTree,
    slot: Slot,
    skills: Option<&SkillRegistry>,
    bases: Option<&pob_data::bases::ItemBaseSet>,
    cluster_ctx: Option<ClusterContext<'_>>,
    timeless: Option<&pob_data::TimelessJewelData>,
) -> Vec<ItemModlineScore> {
    let Some(item) = character.items.get(slot) else {
        return Vec::new();
    };
    let line_count = item.mod_lines.len();
    if line_count == 0 {
        return Vec::new();
    }
    let (baseline, _) = compute_full_with_clusters_and_timeless(
        character,
        tree,
        skills,
        bases,
        cluster_ctx,
        timeless,
    );
    let baseline_dps = baseline.get("MainSkillDPS");
    let baseline_ehp = baseline.get("TotalEHP");

    let mut scores: Vec<ItemModlineScore> = Vec::with_capacity(line_count);
    for idx in 0..line_count {
        // Re-borrow per iteration to keep the immutable view fresh — the
        // outer `item` borrow would conflict with the mutable probe
        // clone below if we held it across iterations.
        let line_text = match character.items.get(slot) {
            Some(it) => it.mod_lines[idx].line.clone(),
            None => continue,
        };
        if line_text.trim().is_empty() {
            continue;
        }
        let mut probe = character.clone();
        if let Some(probe_item) = probe.items.items.get_mut(&slot) {
            probe_item.mod_lines.remove(idx);
        }
        let (after, _) = compute_full_with_clusters_and_timeless(
            &probe,
            tree,
            skills,
            bases,
            cluster_ctx,
            timeless,
        );
        scores.push(ItemModlineScore {
            slot,
            mod_index: idx,
            mod_line: line_text,
            dps_delta: baseline_dps - after.get("MainSkillDPS"),
            ehp_delta: baseline_ehp - after.get("TotalEHP"),
        });
    }
    scores.sort_by(|a, b| {
        let ka = a.dps_delta.max(a.ehp_delta);
        let kb = b.dps_delta.max(b.ehp_delta);
        // Descending impact; tie-break on mod index so the order is
        // stable when several lines tie at zero.
        kb.partial_cmp(&ka)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then(a.mod_index.cmp(&b.mod_index))
    });
    scores
}

/// Render a slice of [`ItemModlineScore`] as one display string per row,
/// suitable for dropping into a UI list panel. The first slice of #207's
/// items-tab "top contributing modlines" view consumes this directly —
/// the renderer is the next slice on top of it.
///
/// Format per row: `"<sign><dps>  <sign><ehp>  <slot label>  <mod line>"`
/// where deltas are formatted to one decimal, padded with thousands
/// commas, and prefixed `+` / `-` so positive/negative impact reads at a
/// glance. Mirrors the spacing PoB's `PowerReportListControl.lua` uses
/// for its column layout.
///
/// The input is consumed in order — callers pass the already-sorted
/// output of [`rank_item_modlines`] (or a flattened multi-slot
/// concatenation re-sorted by impact). `top_n` truncates the result;
/// pass `usize::MAX` for "all rows".
///
/// Whitespace-only mod lines are filtered out at the engine layer
/// already, so this helper does no further dropping — every row in
/// `scores[..top_n]` becomes one output string.
pub fn format_top_contributors(scores: &[ItemModlineScore], top_n: usize) -> Vec<String> {
    scores
        .iter()
        .take(top_n)
        .map(|s| {
            format!(
                "{}  {}  {}  {}",
                format_signed_delta(s.dps_delta),
                format_signed_delta(s.ehp_delta),
                s.slot.label(),
                s.mod_line.trim(),
            )
        })
        .collect()
}

/// `+1234.5` / `-12.0` / `+0.0` — one-decimal, sign-prefixed. Used by
/// [`format_top_contributors`] for both DPS and EHP columns; pulled out
/// so the format is unit-testable in isolation.
fn format_signed_delta(value: f64) -> String {
    // `format!("{:+.1}", x)` would do most of this, but we want `+0.0`
    // (not `-0.0`) for cleanliness when a mod line scores zero impact —
    // f64's signed zero would otherwise leak through.
    let v = if value == 0.0 { 0.0 } else { value };
    format!("{:+.1}", v)
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

    /// Issue #207 (slice 2): scoring the removal of an allocated Life
    /// notable returns a *positive* `ehp_delta`. The sign convention
    /// flips — for removal, "positive = good for the player to keep" —
    /// so a notable contributing +50 Life shows up as a positive delta
    /// the same magnitude `score_node_addition` would have reported on
    /// the inverse direction.
    #[test]
    fn score_node_removal_picks_up_life_notable_loss() {
        let tree = life_notable_tree();
        let mut c = fresh_character();
        c.allocate(2);
        let score = score_node_removal(&c, &tree, 2, None, None, None, None).expect("scored");
        assert_eq!(score.node_id, 2);
        assert!(
            score.ehp_delta > 0.0,
            "removing a +50 Life notable should report a positive (lost) EHP; got {}",
            score.ehp_delta
        );
        assert_eq!(score.dps_delta, 0.0);
    }

    /// Issue #207 (slice 2): symmetry check — scoring the addition of
    /// node 2 starting from an empty allocation, then scoring the removal
    /// of node 2 starting from an allocated set with node 2 present,
    /// should report the *same* `ehp_delta` magnitude (with the sign
    /// flipped). This pins both sign conventions: addition reports
    /// `after - before`, removal reports `before - after`, so both
    /// surface the same positive number for the same effect.
    #[test]
    fn score_node_addition_and_removal_are_symmetric_for_simple_node() {
        let tree = life_notable_tree();
        let baseline = fresh_character();
        let mut allocated = fresh_character();
        allocated.allocate(2);
        let added = score_node_addition(&baseline, &tree, 2, None, None, None, None).unwrap();
        let removed = score_node_removal(&allocated, &tree, 2, None, None, None, None).unwrap();
        // Magnitude matches; both report a positive number under the
        // "good for the player" sign convention.
        assert!((added.ehp_delta - removed.ehp_delta).abs() < 1e-6);
    }

    /// Issue #207 (slice 2): scoring removal of a node that isn't in
    /// the allocation returns `None`. Mirrors the addition's "None for
    /// already allocated" guard so the API uses the same idiom on both
    /// directions.
    #[test]
    fn score_node_removal_returns_none_for_unallocated() {
        let tree = life_notable_tree();
        let c = fresh_character(); // nothing allocated
        let score = score_node_removal(&c, &tree, 2, None, None, None, None);
        assert!(score.is_none());
    }

    /// Tree with a Life notable, a Strength notable, and a stat-less
    /// notable — used to verify ranking sorts impactful nodes ahead of
    /// inert ones and excludes zero-score candidates.
    fn ranking_tree() -> PassiveTree {
        let mut tree = empty_tree();
        tree.classes.push(Class {
            name: "Test".into(),
            base_str: 32,
            base_dex: 14,
            base_int: 14,
            ascendancies: vec![],
        });
        let mut add = |id: NodeId, stats: Vec<String>, kind: NodeKind, neighbours: &[NodeId]| {
            let node = Node {
                id,
                name: Some(format!("n{id}")),
                icon: None,
                ascendancy_name: None,
                stats,
                reminder_text: vec![],
                kind,
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
        // Class start anchored on node 1 with edges to every candidate so
        // the engine's `connected_allocations` BFS picks them up when
        // they're allocated.
        add(1, vec![], NodeKind::Normal, &[2, 3, 4, 5]);
        add(
            2,
            vec!["+50 to maximum Life".into()],
            NodeKind::Notable,
            &[1],
        );
        add(3, vec!["+10 to Strength".into()], NodeKind::Notable, &[1]);
        add(4, vec![], NodeKind::Notable, &[1]); // stat-less — should be excluded
        add(5, vec![], NodeKind::Mastery, &[1]); // wrong kind — should be excluded
        if let Some(n) = tree.nodes.get_mut(&1) {
            n.class_start_index = Some(0);
            n.kind = NodeKind::ClassStart;
        }
        tree
    }

    /// Issue #207 (slice 3): the ranker walks every allocatable
    /// unallocated node, scores it, drops zero-score candidates, and
    /// returns the rest sorted by impact desc. The Life notable
    /// (largest EHP impact) ranks first.
    #[test]
    fn rank_node_additions_returns_impactful_nodes_sorted() {
        let tree = ranking_tree();
        let c = fresh_character();
        let ranked = rank_node_additions(&c, &tree, None, None, None, None);
        // Both notables score non-zero; the stat-less notable and the
        // mastery node drop out via the kind / zero-score filters.
        let ids: Vec<NodeId> = ranked.iter().map(|s| s.node_id).collect();
        assert_eq!(ids, vec![2, 3]);
        // Spot-check the first entry's EHP delta is positive.
        assert!(ranked[0].ehp_delta > 0.0);
    }

    /// Issue #207 (slice 3): ranking on an empty tree returns an empty
    /// list — guards against a panic when there's nothing to score.
    #[test]
    fn rank_node_additions_empty_tree_is_no_op() {
        let tree = empty_tree();
        let c = fresh_character();
        let ranked = rank_node_additions(&c, &tree, None, None, None, None);
        assert!(ranked.is_empty());
    }

    /// Issue #207 (slice 3): allocated nodes are skipped in the ranker
    /// — only unallocated candidates appear. Without this guard, the
    /// caller would have to filter the result by allocation status, and
    /// the score values would be wrong (0 for already-allocated since
    /// inserting them into a clone is a no-op for the basic-stats pass).
    #[test]
    fn rank_node_additions_skips_already_allocated() {
        let tree = ranking_tree();
        let mut c = fresh_character();
        c.allocate(2); // pre-allocate the Life notable.
        let ranked = rank_node_additions(&c, &tree, None, None, None, None);
        let ids: Vec<NodeId> = ranked.iter().map(|s| s.node_id).collect();
        // Only node 3 (Strength) remains allocatable + impactful.
        assert_eq!(ids, vec![3]);
    }

    use pob_data::{Item, ModLine, ModSection, Rarity};

    /// Build an amulet with the given mod lines, all classified as
    /// Explicit. Mirrors the `mk_item` helpers in `timeless.rs` /
    /// `jewel_radius.rs`.
    fn mk_amulet(mod_lines: &[&str]) -> Item {
        Item {
            name: "Test Amulet".into(),
            base_name: "Onyx Amulet".into(),
            rarity: Rarity::Rare,
            item_level: 84,
            quality: 0,
            tags: ahash::HashSet::default(),
            mod_lines: mod_lines
                .iter()
                .map(|l| ModLine {
                    line: (*l).to_string(),
                    section: ModSection::Explicit,
                    variant_list: None,
                })
                .collect(),
            sockets: String::new(),
            raw: String::new(),
            corrupted: false,
            mirrored: false,
            variants: Vec::new(),
            variant: None,
        }
    }

    /// Issue #207 (slice 4): scoring removal of a `+50 to maximum Life`
    /// modline on an equipped amulet returns a positive `ehp_delta` —
    /// pulling that line out costs the player 50 max Life, so the
    /// "what is this contributing" reading is positive.
    #[test]
    fn score_item_modline_removal_picks_up_life_modline() {
        let tree = empty_tree();
        let mut c = fresh_character();
        c.items
            .equip(Slot::Amulet, mk_amulet(&["+50 to maximum Life"]));
        let score = score_item_modline_removal(&c, &tree, Slot::Amulet, 0, None, None, None, None)
            .expect("scored");
        assert_eq!(score.slot, Slot::Amulet);
        assert_eq!(score.mod_index, 0);
        assert_eq!(score.mod_line, "+50 to maximum Life");
        assert!(
            score.ehp_delta > 0.0,
            "+50 Life mod should report positive contribution; got {}",
            score.ehp_delta
        );
    }

    /// Issue #207 (slice 4): scoring removal on an empty slot returns
    /// `None`. Mirrors the "None when not allocated" guard on the node
    /// helpers — keeps the API uniform.
    #[test]
    fn score_item_modline_removal_returns_none_for_empty_slot() {
        let tree = empty_tree();
        let c = fresh_character(); // nothing equipped
        let score = score_item_modline_removal(&c, &tree, Slot::Amulet, 0, None, None, None, None);
        assert!(score.is_none());
    }

    /// Issue #207 (slice 4): scoring removal at an out-of-range
    /// `mod_index` returns `None`. Without this guard, callers walking
    /// `(0..mod_lines.len())` are fine, but a stale index across a
    /// re-equip would otherwise panic on the `mod_lines.remove(idx)`
    /// path — defensive `None` is friendlier.
    #[test]
    fn score_item_modline_removal_returns_none_for_out_of_range_index() {
        let tree = empty_tree();
        let mut c = fresh_character();
        c.items
            .equip(Slot::Amulet, mk_amulet(&["+50 to maximum Life"]));
        let score = score_item_modline_removal(&c, &tree, Slot::Amulet, 5, None, None, None, None);
        assert!(score.is_none());
    }

    /// Issue #207 (slice 4): the ranker walks every mod line on the
    /// equipped item, scores it, and returns the list sorted by impact
    /// descending. The Life line (large EHP impact) ranks ahead of the
    /// resist line (smaller EHP impact via the elemental EHP folding).
    #[test]
    fn rank_item_modlines_orders_by_impact_desc() {
        let tree = empty_tree();
        let mut c = fresh_character();
        c.items.equip(
            Slot::Amulet,
            mk_amulet(&["+50 to maximum Life", "+10 to Strength"]),
        );
        let ranked = rank_item_modlines(&c, &tree, Slot::Amulet, None, None, None, None);
        assert_eq!(ranked.len(), 2);
        // Life line should rank first (larger EHP swing than the +10
        // Strength line which only nudges Life via the str/2 conversion).
        assert_eq!(ranked[0].mod_line, "+50 to maximum Life");
        assert!(ranked[0].ehp_delta > ranked[1].ehp_delta);
    }

    /// Issue #207 (slice 4): empty / whitespace-only mod lines are
    /// dropped from the ranking. Removing a blank line costs zero by
    /// definition and rendering it as a row would clutter the items-
    /// tab "top contributors" list with no information.
    #[test]
    fn rank_item_modlines_skips_empty_lines() {
        let tree = empty_tree();
        let mut c = fresh_character();
        c.items
            .equip(Slot::Amulet, mk_amulet(&["+50 to maximum Life", "", "   "]));
        let ranked = rank_item_modlines(&c, &tree, Slot::Amulet, None, None, None, None);
        // Only the real mod line survives — the blank and whitespace-
        // only lines are filtered out before scoring.
        assert_eq!(ranked.len(), 1);
        assert_eq!(ranked[0].mod_line, "+50 to maximum Life");
    }

    /// Issue #207 (slice 5): equipping a `+50 to maximum Life` amulet
    /// into an empty slot raises EHP by the matching delta. Sign
    /// convention for swap differs from removal — positive = "swap is
    /// an upgrade" — because the consumer is the items-tab "vs
    /// equipped" hover that the user reads as "this would gain you N".
    #[test]
    fn score_item_swap_into_empty_slot_reports_positive_ehp_gain() {
        let tree = empty_tree();
        let c = fresh_character();
        let candidate = mk_amulet(&["+50 to maximum Life"]);
        let score = score_item_swap(&c, &tree, Slot::Amulet, &candidate, None, None, None, None);
        assert_eq!(score.slot, Slot::Amulet);
        assert!(
            score.ehp_delta > 0.0,
            "swapping in a +50 Life amulet should raise EHP; got {}",
            score.ehp_delta
        );
        assert_eq!(score.dps_delta, 0.0);
    }

    /// Issue #207 (slice 5): swapping the *same* item back in (clone of
    /// what's currently equipped) reports zero deltas. The engine is
    /// deterministic across identical inputs, so the baseline + after
    /// outputs match exactly. UI consumers can suppress zero-delta
    /// rows from the "vs equipped" hover.
    #[test]
    fn score_item_swap_with_clone_of_equipped_reports_zero_deltas() {
        let tree = empty_tree();
        let mut c = fresh_character();
        let amulet = mk_amulet(&["+50 to maximum Life"]);
        c.items.equip(Slot::Amulet, amulet.clone());
        let score = score_item_swap(&c, &tree, Slot::Amulet, &amulet, None, None, None, None);
        assert_eq!(score.dps_delta, 0.0);
        assert_eq!(score.ehp_delta, 0.0);
    }

    /// Issue #207 (slice 5): swap-in of a *stronger* amulet (more
    /// Life) onto a slot with a weaker one already equipped reports a
    /// positive `ehp_delta` — the swap raises EHP. Pins the swap-sign
    /// convention and the across-item comparison consumers will rely
    /// on.
    #[test]
    fn score_item_swap_to_stronger_item_reports_positive_ehp_delta() {
        let tree = empty_tree();
        let mut c = fresh_character();
        c.items
            .equip(Slot::Amulet, mk_amulet(&["+30 to maximum Life"]));
        let stronger = mk_amulet(&["+80 to maximum Life"]);
        let score = score_item_swap(&c, &tree, Slot::Amulet, &stronger, None, None, None, None);
        assert!(
            score.ehp_delta > 0.0,
            "swap from +30 Life → +80 Life should raise EHP; got {}",
            score.ehp_delta
        );
    }

    /// Issue #207 (slice 6): ranking with an empty candidate slice is
    /// a no-op. Guards the items-tab "scan shared store" loop against
    /// a panic when the store has nothing for the slot.
    #[test]
    fn rank_shared_items_for_slot_empty_candidates_returns_empty() {
        let tree = empty_tree();
        let c = fresh_character();
        let ranked =
            rank_shared_items_for_slot(&c, &tree, Slot::Amulet, &[], None, None, None, None);
        assert!(ranked.is_empty());
    }

    /// Issue #207 (slice 6): the candidate with the larger Life
    /// contribution should rank ahead of the smaller one. Pins the
    /// max(dps_delta, ehp_delta) sort key.
    #[test]
    fn rank_shared_items_for_slot_orders_candidates_by_impact_desc() {
        let tree = empty_tree();
        let c = fresh_character();
        let small = mk_amulet(&["+10 to maximum Life"]);
        let big = mk_amulet(&["+200 to maximum Life"]);
        let medium = mk_amulet(&["+50 to maximum Life"]);
        // Intentionally not pre-sorted so the ranker has to do work.
        let candidates = [small, big, medium];
        let ranked = rank_shared_items_for_slot(
            &c,
            &tree,
            Slot::Amulet,
            &candidates,
            None,
            None,
            None,
            None,
        );
        assert_eq!(ranked.len(), 3);
        let order: Vec<usize> = ranked.iter().map(|(i, _)| *i).collect();
        // Indices: small=0, big=1, medium=2 → expected order big, medium, small.
        assert_eq!(order, vec![1, 2, 0], "got {ranked:?}");
    }

    /// Issue #207 (slice 6): negative-impact candidates (a swap that
    /// would *cost* the player Life) are kept in the result. They're
    /// useful information; the consumer's UI can grey them out.
    #[test]
    fn rank_shared_items_for_slot_includes_negative_impact_candidates() {
        let tree = empty_tree();
        let mut c = fresh_character();
        c.items
            .equip(Slot::Amulet, mk_amulet(&["+200 to maximum Life"]));
        let downgrade = mk_amulet(&["+10 to maximum Life"]);
        let ranked = rank_shared_items_for_slot(
            &c,
            &tree,
            Slot::Amulet,
            std::slice::from_ref(&downgrade),
            None,
            None,
            None,
            None,
        );
        assert_eq!(ranked.len(), 1);
        let (_, score) = &ranked[0];
        assert!(
            score.ehp_delta < 0.0,
            "downgrade should report negative ehp_delta; got {}",
            score.ehp_delta
        );
    }

    /// Issue #207 (slice 4): ranking on an empty slot returns an empty
    /// list — guards against a panic when there's nothing to score.
    #[test]
    fn rank_item_modlines_empty_slot_is_no_op() {
        let tree = empty_tree();
        let c = fresh_character();
        let ranked = rank_item_modlines(&c, &tree, Slot::Amulet, None, None, None, None);
        assert!(ranked.is_empty());
    }

    // ── Issue #207 (slice 5): pure formatter for the items-tab list ──
    //
    // No engine machinery — these tests build `ItemModlineScore` values
    // by hand and assert the rendered strings. The renderer (next slice)
    // wires `rank_item_modlines` → `format_top_contributors` and drops
    // the resulting strings into an items-tab `ListBox`.

    fn mk_score(slot: Slot, idx: usize, line: &str, dps: f64, ehp: f64) -> ItemModlineScore {
        ItemModlineScore {
            slot,
            mod_index: idx,
            mod_line: line.into(),
            dps_delta: dps,
            ehp_delta: ehp,
        }
    }

    #[test]
    fn format_top_contributors_renders_each_row() {
        let scores = vec![
            mk_score(Slot::Amulet, 0, "+50 to maximum Life", 1234.5, 67.8),
            mk_score(Slot::Helmet, 1, "10% increased Spell Damage", 800.0, 0.0),
        ];
        let lines = format_top_contributors(&scores, 10);
        assert_eq!(lines.len(), 2);
        assert_eq!(lines[0], "+1234.5  +67.8  Amulet  +50 to maximum Life");
        assert_eq!(lines[1], "+800.0  +0.0  Helmet  10% increased Spell Damage");
    }

    #[test]
    fn format_top_contributors_truncates_to_top_n() {
        let scores = vec![
            mk_score(Slot::Ring1, 0, "a", 5.0, 0.0),
            mk_score(Slot::Ring1, 1, "b", 4.0, 0.0),
            mk_score(Slot::Ring1, 2, "c", 3.0, 0.0),
            mk_score(Slot::Ring1, 3, "d", 2.0, 0.0),
        ];
        let lines = format_top_contributors(&scores, 2);
        assert_eq!(lines.len(), 2);
        assert!(lines[0].ends_with("Ring (1)  a"));
        assert!(lines[1].ends_with("Ring (1)  b"));
    }

    #[test]
    fn format_top_contributors_signs_negative_deltas() {
        // Corrupted "+X% Damage taken" mod actively hurts the build —
        // negative dps_delta means the player is losing DPS by keeping
        // the line, and the column should make that obvious.
        let scores = vec![mk_score(
            Slot::BodyArmour,
            0,
            "Take 10% increased Damage from Hits",
            -250.0,
            -1500.0,
        )];
        let lines = format_top_contributors(&scores, 10);
        assert_eq!(lines.len(), 1);
        assert_eq!(
            lines[0],
            "-250.0  -1500.0  Body Armour  Take 10% increased Damage from Hits"
        );
    }

    #[test]
    fn format_top_contributors_zero_delta_is_plus_zero() {
        // f64 has signed zero — a mod that scores exactly 0.0 with a
        // negative-zero result of subtraction would otherwise render as
        // `-0.0` and look like a regression. The formatter normalises.
        let scores = vec![mk_score(Slot::Belt, 0, "Veiled prefix", -0.0, -0.0)];
        let lines = format_top_contributors(&scores, 10);
        assert_eq!(lines[0], "+0.0  +0.0  Belt  Veiled prefix");
    }

    #[test]
    fn format_top_contributors_empty_input_is_empty_output() {
        let lines = format_top_contributors(&[], 10);
        assert!(lines.is_empty());
    }

    #[test]
    fn format_top_contributors_top_n_zero_returns_empty() {
        let scores = vec![mk_score(Slot::Amulet, 0, "x", 1.0, 1.0)];
        let lines = format_top_contributors(&scores, 0);
        assert!(lines.is_empty());
    }

    #[test]
    fn format_top_contributors_trims_modline_whitespace() {
        // `rank_item_modlines` skips whitespace-only lines but a real
        // line with trailing whitespace ("+50 to Life\n") should still
        // render cleanly without the trailing blank.
        let scores = vec![mk_score(
            Slot::Amulet,
            0,
            "  +50 to maximum Life  \n",
            10.0,
            10.0,
        )];
        let lines = format_top_contributors(&scores, 10);
        assert_eq!(lines[0], "+10.0  +10.0  Amulet  +50 to maximum Life");
    }
}
