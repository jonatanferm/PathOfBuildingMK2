//! Issue #220: pure UI-side data layer mapping `(NodeId, score)` pairs
//! into normalised 0.0..=1.0 values and heatmap colours, ready for the
//! tree renderer slice to consume.
//!
//! The engine half of this feature lives in
//! [`pob_engine::power::score_node_addition`] — it produces an `f64`
//! score per candidate node. This module turns a batch of those scores
//! into per-node colours without touching any rendering code, so it can
//! be unit-tested in isolation and reused by both the eframe and wgpu
//! tree renderers later.
//!
//! ## Normalisation rule
//!
//! [`normalise_scores`] linearly maps each score onto `0.0..=1.0` using
//! the batch's min and max as the endpoints (min → 0.0, max → 1.0).
//!
//! Edge cases:
//! - **Empty input**: returns an empty map.
//! - **Single element**: maps to `1.0`. A lone score is by definition the
//!   maximum, so it deserves the "hottest" colour rather than an
//!   ambiguous mid-tone.
//! - **All scores equal** (more than one element, including all-zero):
//!   maps every node to `0.5`. There is no spread to express, so we
//!   pick the neutral midpoint deterministically rather than dividing
//!   by zero.
//! - **Duplicate `NodeId`s**: the last `(NodeId, score)` pair wins.
//!   Callers shouldn't pass duplicates, but we don't panic if they do.
//! - **NaN scores**: skipped entirely (excluded from min/max and from
//!   the output map). NaN can't be sensibly placed on a 1-D gradient.
//!
//! ## Colour gradient
//!
//! [`score_to_colour`] maps `0.0..=1.0` to a four-stop gradient:
//! blue → green → yellow → red. Inputs outside the range are clamped.

use ahash::AHashMap;
use eframe::egui;
use pob_data::NodeId;
use pob_engine::{rank_node_additions, Character, ClusterContext, NodeScore, SkillRegistry};

/// Issue #220 follow-up: which scoring axis the heatmap colours nodes by.
/// PoB's `TreeTab.lua:195-275` exposes a `treeHeatMapStatSelect` dropdown
/// so the user can isolate offensive vs defensive impact — useful when
/// looking at a defence-focused build where DPS-only ranking washes out
/// the actually-helpful nodes.
///
/// `Combined` is the historical default (max of dps/ehp deltas) so a
/// pure-EHP and pure-DPS node tint at comparable intensity. The two
/// scalar modes pick a single axis so the gradient reflects one stat
/// directly — handy for "what gets me the most life?" / "what bumps
/// my DPS the most?" cases.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum HeatmapStat {
    /// `max(dps_delta, ehp_delta)` — historical behaviour. Reads as
    /// "single best-impact axis per node" so offence and defence
    /// compare on the same scale.
    #[default]
    Combined,
    /// `dps_delta` only — EHP-only nodes go cold.
    Dps,
    /// `ehp_delta` only — DPS-only nodes go cold.
    Ehp,
}

impl HeatmapStat {
    /// Human-readable label for the UI selector.
    #[must_use]
    pub fn label(self) -> &'static str {
        match self {
            Self::Combined => "Combined",
            Self::Dps => "DPS",
            Self::Ehp => "EHP",
        }
    }
}

/// Issue #220 (heatmap pipeline slice): engine-to-paint glue that turns
/// a [`Character`] + [`pob_data::PassiveTree`] into a per-node colour
/// map ready for the tree renderer to tint unallocated nodes with.
///
/// Composition:
/// 1. Rank every unallocated, allocatable node via
///    [`pob_engine::rank_node_additions`] — the existing Power-Report
///    driver from #207.
/// 2. Reduce each [`NodeScore`] to a single impact value via
///    [`score_impact_key`] (currently `max(dps_delta, ehp_delta)` to
///    match the ranker's own sort key, so a pure-EHP node and a
///    pure-DPS node tint at the same intensity).
/// 3. Pipe the `(NodeId, impact)` pairs through [`normalise_scores`]
///    and [`score_to_colour`] to land on `egui::Color32` values.
///
/// **Filtering**: `rank_node_additions` already drops zero-impact and
/// non-allocatable nodes, so the returned map only contains nodes the
/// renderer should actually tint. Nodes the player has already
/// allocated never appear in the output — the heatmap is a
/// "what-to-take-next" overlay, not a paid-points display.
///
/// **Performance**: this is N+1 perform calls inside
/// `rank_node_additions`, multi-second on a real ~2000-node tree. The
/// renderer slice that consumes this map should call it on an explicit
/// "Refresh heatmap" button click, not every frame. Caching /
/// incremental compute is a separate slice.
#[must_use]
pub fn compute_heatmap_inputs(
    character: &Character,
    tree: &pob_data::PassiveTree,
    skills: Option<&SkillRegistry>,
    bases: Option<&pob_data::bases::ItemBaseSet>,
    cluster_ctx: Option<ClusterContext<'_>>,
    timeless: Option<&pob_data::TimelessJewelData>,
    stat: HeatmapStat,
    top_n: Option<usize>,
) -> AHashMap<NodeId, egui::Color32> {
    let ranked = rank_node_additions(character, tree, skills, bases, cluster_ctx, timeless);
    compute_heatmap_inputs_from_ranked(&ranked, stat, top_n)
}

/// Issue #207 follow-up: turn an already-ranked
/// [`Vec<NodeScore>`](pob_engine::NodeScore) into a per-node colour
/// map. Same pipeline as [`compute_heatmap_inputs`] minus the costly
/// `rank_node_additions` walk — useful when the caller already has the
/// ranked list cached and wants to re-colour with a different
/// [`HeatmapStat`] / `top_n` cheaply.
#[must_use]
pub fn compute_heatmap_inputs_from_ranked(
    ranked: &[NodeScore],
    stat: HeatmapStat,
    top_n: Option<usize>,
) -> AHashMap<NodeId, egui::Color32> {
    let scores: Vec<(NodeId, f64)> = ranked
        .iter()
        .map(|s| (s.node_id, score_impact_key(s, stat)))
        .collect();
    let scores = match top_n {
        Some(n) => truncate_to_top_n(scores, n),
        None => scores,
    };
    let normalised = normalise_scores(&scores);
    normalised
        .into_iter()
        .map(|(id, t)| (id, score_to_colour(t)))
        .collect()
}

/// Issue #220 follow-up: sample the heatmap gradient at evenly-spaced
/// stops for the on-screen legend strip. Returns `count` `(t, colour)`
/// pairs where `t` is the position in `0.0..=1.0`. Pure / no egui
/// state — the renderer paints the strip from the returned colours.
///
/// A minimum of two stops is enforced so the strip is always
/// drawable; smaller requests are clamped silently.
#[must_use]
pub fn heatmap_legend_stops(count: usize) -> Vec<(f32, egui::Color32)> {
    let n = count.max(2);
    (0..n)
        .map(|i| {
            let t = i as f32 / (n - 1) as f32;
            (t, score_to_colour(t))
        })
        .collect()
}

/// Issue #220 follow-up: filter out ascendancy nodes from a ranked
/// list when the user wants the heatmap focused on the main tree
/// only. Ascendancy nodes have an `ascendancy_name` set; nodes
/// missing from `tree.nodes` pass through (defensive — stale `ranked`
/// data referencing a node that's since been removed from the tree
/// shouldn't trip the filter).
///
/// Pure / no egui; when `hide` is `false` the function is a clone.
#[must_use]
pub fn filter_ascendancy_from_ranked(
    ranked: &[NodeScore],
    tree: &pob_data::PassiveTree,
    hide: bool,
) -> Vec<NodeScore> {
    if !hide {
        return ranked.to_vec();
    }
    ranked
        .iter()
        .filter(|s| {
            tree.nodes
                .get(&s.node_id)
                .is_none_or(|n| n.ascendancy_name.is_none())
        })
        .copied()
        .collect()
}

/// Issue #220 follow-up: BFS-reachable node set from any `allocated`
/// seed, expanded up to `max_depth` edges. Used by the heatmap to
/// restrict the colour overlay to "candidates the user can plausibly
/// take with their unspent points". Allocated nodes themselves count
/// as depth 0 and always appear in the result.
///
/// Treats the tree as undirected (`out_edges + in_edges`), matching
/// the engine's `connected_allocations` walk — so a leaf node that
/// lists only its inbound edge still surfaces from a higher-up seed.
///
/// `max_depth == 0` returns just the allocated set; an empty
/// allocated set returns empty regardless of depth.
#[must_use]
pub fn nodes_within_depth(
    tree: &pob_data::PassiveTree,
    allocated: &std::collections::HashSet<NodeId>,
    max_depth: u32,
) -> ahash::HashSet<NodeId> {
    let mut visited: ahash::HashSet<NodeId> = allocated.iter().copied().collect();
    if allocated.is_empty() {
        return visited;
    }
    let mut frontier: Vec<NodeId> = allocated.iter().copied().collect();
    for _ in 0..max_depth {
        let mut next_frontier = Vec::new();
        for &id in &frontier {
            if let Some(node) = tree.nodes.get(&id) {
                for &neighbour in node.out_edges.iter().chain(node.in_edges.iter()) {
                    if visited.insert(neighbour) {
                        next_frontier.push(neighbour);
                    }
                }
            }
        }
        if next_frontier.is_empty() {
            break;
        }
        frontier = next_frontier;
    }
    visited
}

/// Issue #220 follow-up: count how many unallocated nodes the
/// depth filter would keep at the given `max_depth`. Lets the UI
/// preview "Reachable: N" alongside the depth combo so the user
/// sees the effect of the filter before clicking Refresh.
///
/// Pure — wraps [`nodes_within_depth`] and subtracts the allocated
/// seeds (which always sit at depth 0 inside the BFS result).
#[must_use]
pub fn count_unallocated_within_depth(
    tree: &pob_data::PassiveTree,
    allocated: &std::collections::HashSet<NodeId>,
    max_depth: u32,
) -> usize {
    let reachable = nodes_within_depth(tree, allocated, max_depth);
    reachable
        .iter()
        .filter(|id| !allocated.contains(id))
        .count()
}

/// Issue #207 follow-up: format the top-N candidate nodes as
/// human-readable strings for the tree-tab "Top candidate nodes"
/// panel. Each line shows the rank, signed DPS / EHP deltas, and the
/// node's display name (or `#<id>` for unknown ids). Pure helper —
/// the renderer walks the returned strings.
///
/// `ranked` is the raw [`rank_node_additions`] output. This helper
/// re-sorts a local view by [`score_impact_key`] for `stat` so the
/// panel's ranking matches the heatmap overlay's colouring when the
/// user switches the axis selector between Combined / DPS / EHP.
/// `NodeScore` is `Copy`, so the local sort is cheap and the caller's
/// cached slice is left untouched.
/// Issue #220 follow-up: build the formatted lines for the heatmap's
/// "top candidates" panel, paired with their source `NodeId` so the
/// renderer can attach a hover tooltip per row.
///
/// Re-sorts a local view by [`score_impact_key`] for `stat` so the
/// panel's ranking matches the heatmap overlay's colouring when the
/// user switches the axis selector between Combined / DPS / EHP.
/// `NodeScore` is `Copy`, so the local sort is cheap and the caller's
/// cached slice is left untouched. NaN scores fall to the bottom so a
/// stray engine NaN can't pin garbage to rank 1 — matches
/// [`truncate_to_top_n`].
///
/// Unknown node ids (stale rank list against a refreshed tree) fall
/// back to `#<id>` so the user still sees something rather than a
/// blank line.
#[must_use]
pub fn format_top_node_candidate_rows(
    ranked: &[NodeScore],
    tree: &pob_data::PassiveTree,
    top_n: usize,
    stat: HeatmapStat,
) -> Vec<(NodeId, String)> {
    let mut sorted: Vec<NodeScore> = ranked.to_vec();
    sorted.sort_by(|a, b| {
        let ka = score_impact_key(a, stat);
        let kb = score_impact_key(b, stat);
        match (ka.is_nan(), kb.is_nan()) {
            (true, true) => a.node_id.cmp(&b.node_id),
            (true, false) => std::cmp::Ordering::Greater,
            (false, true) => std::cmp::Ordering::Less,
            (false, false) => kb
                .partial_cmp(&ka)
                .unwrap_or(std::cmp::Ordering::Equal)
                .then_with(|| a.node_id.cmp(&b.node_id)),
        }
    });
    sorted
        .into_iter()
        .take(top_n)
        .enumerate()
        .map(|(i, score)| {
            let rank = i + 1;
            let name = tree
                .nodes
                .get(&score.node_id)
                .and_then(|n| n.name.clone())
                .unwrap_or_else(|| format!("#{}", score.node_id));
            let line = format!(
                "{rank}. {:>+8.0} DPS {:>+8.0} EHP  {name}",
                score.dps_delta, score.ehp_delta,
            );
            (score.node_id, line)
        })
        .collect()
}

/// Issue #220 follow-up: keep only the top-N highest-scoring entries.
/// NaN scores fall to the bottom of the ranking so a stray engine NaN
/// can't displace a real top entry. Pure helper for testability.
///
/// `n == 0` returns an empty vector (defensive — the renderer treats
/// "no top selected" as `None`, never `Some(0)`, but the helper handles
/// it cleanly anyway).
#[must_use]
pub fn truncate_to_top_n(mut scores: Vec<(NodeId, f64)>, n: usize) -> Vec<(NodeId, f64)> {
    scores.sort_by(|a, b| {
        // NaN sinks to the bottom: treat NaN as less than every finite
        // value so the top-N picks finite scores first.
        match (a.1.is_nan(), b.1.is_nan()) {
            (true, true) => std::cmp::Ordering::Equal,
            (true, false) => std::cmp::Ordering::Greater,
            (false, true) => std::cmp::Ordering::Less,
            (false, false) => b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal),
        }
    });
    scores.truncate(n);
    scores
}

/// Reduce a [`NodeScore`]'s `(dps_delta, ehp_delta)` pair to a single
/// scalar suitable for normalisation. Mirrors the sort key used inside
/// [`pob_engine::rank_node_additions`] — `max(dps_delta, ehp_delta)`
/// — so a pure-defensive node and a pure-offensive node of comparable
/// "value to the player" tint at comparable intensity, rather than the
/// defensive node washing out because DPS dominates the magnitude.
///
/// Kept separate from [`compute_heatmap_inputs`] so future slices can
/// experiment with weighted combinations (e.g. user-tunable
/// `dps_weight` × `ehp_weight`) without rewriting the pipeline.
#[must_use]
pub fn score_impact_key(score: &NodeScore, stat: HeatmapStat) -> f64 {
    match stat {
        HeatmapStat::Combined => score.dps_delta.max(score.ehp_delta),
        HeatmapStat::Dps => score.dps_delta,
        HeatmapStat::Ehp => score.ehp_delta,
    }
}

/// Normalise a batch of `(NodeId, score)` pairs to `0.0..=1.0`.
///
/// See the module-level docs for the edge-case rules.
#[must_use]
pub fn normalise_scores(scores: &[(NodeId, f64)]) -> AHashMap<NodeId, f32> {
    // Filter NaN up-front so it can't poison min/max.
    let finite: Vec<(NodeId, f64)> = scores
        .iter()
        .copied()
        .filter(|(_, s)| !s.is_nan())
        .collect();

    if finite.is_empty() {
        return AHashMap::new();
    }

    if finite.len() == 1 {
        let mut out = AHashMap::with_capacity(1);
        out.insert(finite[0].0, 1.0);
        return out;
    }

    let mut min = f64::INFINITY;
    let mut max = f64::NEG_INFINITY;
    for &(_, s) in &finite {
        if s < min {
            min = s;
        }
        if s > max {
            max = s;
        }
    }

    let range = max - min;
    let mut out = AHashMap::with_capacity(finite.len());

    if range == 0.0 {
        // All scores identical — flat gradient, midpoint colour.
        for (id, _) in finite {
            out.insert(id, 0.5);
        }
        return out;
    }

    for (id, s) in finite {
        let norm = ((s - min) / range) as f32;
        out.insert(id, norm);
    }
    out
}

/// Map a normalised score in `0.0..=1.0` to a heatmap colour using a
/// four-stop blue → green → yellow → red gradient.
///
/// Inputs outside the range are clamped. NaN is treated as `0.0`.
#[must_use]
pub fn score_to_colour(normalised: f32) -> egui::Color32 {
    // Clamp + handle NaN. `clamp` panics on NaN, so guard manually.
    let t = if normalised.is_nan() {
        0.0
    } else {
        normalised.clamp(0.0, 1.0)
    };

    // Stops: 0.0 = blue, 1/3 = green, 2/3 = yellow, 1.0 = red.
    const BLUE: [u8; 3] = [0, 64, 255];
    const GREEN: [u8; 3] = [0, 200, 0];
    const YELLOW: [u8; 3] = [255, 220, 0];
    const RED: [u8; 3] = [255, 32, 32];

    let (a, b, local) = if t < 1.0 / 3.0 {
        (BLUE, GREEN, t * 3.0)
    } else if t < 2.0 / 3.0 {
        (GREEN, YELLOW, (t - 1.0 / 3.0) * 3.0)
    } else {
        (YELLOW, RED, (t - 2.0 / 3.0) * 3.0)
    };

    let lerp = |x: u8, y: u8, k: f32| -> u8 {
        let xf = f32::from(x);
        let yf = f32::from(y);
        (xf + (yf - xf) * k).round().clamp(0.0, 255.0) as u8
    };

    egui::Color32::from_rgb(
        lerp(a[0], b[0], local),
        lerp(a[1], b[1], local),
        lerp(a[2], b[2], local),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn score(dps: f64, ehp: f64) -> NodeScore {
        NodeScore {
            node_id: 0,
            dps_delta: dps,
            ehp_delta: ehp,
        }
    }

    #[test]
    fn score_impact_combined_takes_max_of_axes() {
        // Historical behaviour: pure-DPS and pure-EHP nodes tint at
        // comparable intensity. Negative deltas are kept verbatim so
        // the normaliser can still relativise them.
        assert!((score_impact_key(&score(10.0, 4.0), HeatmapStat::Combined) - 10.0).abs() < 1e-9);
        assert!((score_impact_key(&score(2.0, 7.0), HeatmapStat::Combined) - 7.0).abs() < 1e-9);
        assert!(
            (score_impact_key(&score(-3.0, -10.0), HeatmapStat::Combined) - -3.0).abs() < 1e-9,
            "max of two negatives picks the closer-to-zero one",
        );
    }

    #[test]
    fn score_impact_dps_isolates_dps_axis() {
        // EHP-only nodes go cold; DPS-only nodes drive the gradient.
        assert!((score_impact_key(&score(15.0, 100.0), HeatmapStat::Dps) - 15.0).abs() < 1e-9);
        assert!((score_impact_key(&score(0.0, 50.0), HeatmapStat::Dps)).abs() < 1e-9);
    }

    #[test]
    fn score_impact_ehp_isolates_ehp_axis() {
        // DPS-only nodes go cold; EHP-only nodes drive the gradient.
        assert!((score_impact_key(&score(100.0, 25.0), HeatmapStat::Ehp) - 25.0).abs() < 1e-9);
        assert!((score_impact_key(&score(50.0, 0.0), HeatmapStat::Ehp)).abs() < 1e-9);
    }

    #[test]
    fn heatmap_stat_default_is_combined() {
        // Existing call sites that haven't been migrated to choose a
        // stat should get the historical reducer back.
        assert_eq!(HeatmapStat::default(), HeatmapStat::Combined);
    }

    #[test]
    fn normalise_empty_input_returns_empty_map() {
        let out = normalise_scores(&[]);
        assert!(out.is_empty());
    }

    #[test]
    fn normalise_single_element_maps_to_one() {
        let out = normalise_scores(&[(7u32, 42.0)]);
        assert_eq!(out.len(), 1);
        assert!((out[&7] - 1.0).abs() < 1e-6);
    }

    #[test]
    fn normalise_all_equal_scores_map_to_half() {
        let out = normalise_scores(&[(1u32, 5.0), (2, 5.0), (3, 5.0)]);
        assert_eq!(out.len(), 3);
        for id in [1u32, 2, 3] {
            assert!(
                (out[&id] - 0.5).abs() < 1e-6,
                "id {id} should be 0.5, got {}",
                out[&id]
            );
        }
    }

    #[test]
    fn normalise_all_zero_scores_map_to_half() {
        // All-zero is just a special case of "all equal" — make sure
        // we don't divide by zero or special-case it differently.
        let out = normalise_scores(&[(10u32, 0.0), (20, 0.0)]);
        assert!((out[&10] - 0.5).abs() < 1e-6);
        assert!((out[&20] - 0.5).abs() < 1e-6);
    }

    #[test]
    fn normalise_two_element_split_maps_to_zero_and_one() {
        let out = normalise_scores(&[(1u32, 10.0), (2, 30.0)]);
        assert!((out[&1] - 0.0).abs() < 1e-6);
        assert!((out[&2] - 1.0).abs() < 1e-6);
    }

    #[test]
    fn normalise_negative_and_positive_scores_span_full_range() {
        let out = normalise_scores(&[(1u32, -10.0), (2, 0.0), (3, 10.0)]);
        assert!((out[&1] - 0.0).abs() < 1e-6);
        assert!((out[&2] - 0.5).abs() < 1e-6);
        assert!((out[&3] - 1.0).abs() < 1e-6);
    }

    #[test]
    fn normalise_skips_nan_scores() {
        let out = normalise_scores(&[(1u32, 1.0), (2, f64::NAN), (3, 2.0)]);
        assert!(!out.contains_key(&2), "NaN entry must not appear in output");
        assert_eq!(out.len(), 2);
        assert!((out[&1] - 0.0).abs() < 1e-6);
        assert!((out[&3] - 1.0).abs() < 1e-6);
    }

    #[test]
    fn normalise_big_batch_preserves_score_ordering() {
        // Build 100 nodes with monotonically increasing scores. The
        // normalised values must therefore also be monotonically
        // increasing when sorted by NodeId.
        let scores: Vec<(NodeId, f64)> =
            (0u32..100).map(|i| (i, f64::from(i) * 1.5 + 7.0)).collect();
        let out = normalise_scores(&scores);
        assert_eq!(out.len(), 100);

        let mut sorted: Vec<(NodeId, f32)> = out.iter().map(|(&k, &v)| (k, v)).collect();
        sorted.sort_by_key(|(id, _)| *id);

        for window in sorted.windows(2) {
            assert!(
                window[0].1 <= window[1].1,
                "scores should be non-decreasing by NodeId order, got {} then {}",
                window[0].1,
                window[1].1,
            );
        }

        // First and last must hit the endpoints.
        assert!((sorted.first().unwrap().1 - 0.0).abs() < 1e-6);
        assert!((sorted.last().unwrap().1 - 1.0).abs() < 1e-6);
    }

    #[test]
    fn colour_at_zero_is_blue_end() {
        let c = score_to_colour(0.0);
        assert_eq!(c, egui::Color32::from_rgb(0, 64, 255));
    }

    #[test]
    fn colour_at_one_is_red_end() {
        let c = score_to_colour(1.0);
        assert_eq!(c, egui::Color32::from_rgb(255, 32, 32));
    }

    #[test]
    fn colour_at_half_is_between_green_and_yellow() {
        // 0.5 sits in the green→yellow segment. Don't pin the exact RGB
        // — the local interpolation parameter is `(0.5 - 1/3) * 3` in
        // f32, which isn't bit-exactly 0.5, so the round-to-u8 step
        // can land on either neighbour. Instead assert the channels
        // sit *between* GREEN [0,200,0] and YELLOW [255,220,0].
        let c = score_to_colour(0.5);
        let [r, g, b, _] = c.to_array();
        assert!(
            (1..=254).contains(&r),
            "red channel {r} not strictly between 0 and 255"
        );
        assert!(
            (200..=220).contains(&g),
            "green channel {g} not between 200 and 220"
        );
        assert_eq!(b, 0, "blue channel must stay at 0 in this segment");
    }

    #[test]
    fn colour_clamps_out_of_range_inputs() {
        assert_eq!(score_to_colour(-5.0), score_to_colour(0.0));
        assert_eq!(score_to_colour(2.5), score_to_colour(1.0));
    }

    #[test]
    fn colour_handles_nan_as_zero() {
        // NaN → 0.0 (cold end) so we never panic on stray FP weirdness.
        assert_eq!(score_to_colour(f32::NAN), score_to_colour(0.0));
    }

    // ---------------------------------------------------------------
    // Issue #220 (heatmap pipeline): `compute_heatmap_inputs` glues
    // the engine's `rank_node_additions` to the pure normalise +
    // colour helpers above. The fixture mirrors the one in
    // `pob_engine::power::tests` so we exercise the real composition,
    // not a mocked-out scoring layer.
    // ---------------------------------------------------------------

    use ahash::HashMap as InnerAHashMap;
    use pob_data::{Class, Node, NodeKind, PassiveTree, TreeConstants, TreePoints};
    use pob_engine::{Character, ClassRef, NodeScore};
    use smallvec::SmallVec;

    fn empty_tree() -> PassiveTree {
        PassiveTree {
            version: "test".into(),
            tree: "test".into(),
            classes: vec![],
            groups: InnerAHashMap::default(),
            nodes: InnerAHashMap::default(),
            jewel_slots: vec![],
            min_x: 0,
            min_y: 0,
            max_x: 0,
            max_y: 0,
            constants: TreeConstants {
                skills_per_orbit: vec![],
                orbit_radii: vec![],
                classes: InnerAHashMap::default(),
                character_attributes: InnerAHashMap::default(),
                pss_centre_inner_radius: None,
            },
            points: TreePoints::default(),
        }
    }

    /// Two-impactful + two-inert tree: Life notable, Strength notable,
    /// stat-less notable (zero score → filtered), mastery (wrong kind →
    /// filtered). Anchored at a class-start node.
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
        add(1, vec![], NodeKind::Normal, &[2, 3, 4, 5]);
        add(
            2,
            vec!["+50 to maximum Life".into()],
            NodeKind::Notable,
            &[1],
        );
        add(3, vec!["+10 to Strength".into()], NodeKind::Notable, &[1]);
        add(4, vec![], NodeKind::Notable, &[1]); // stat-less — filtered.
        add(5, vec![], NodeKind::Mastery, &[1]); // wrong kind — filtered.
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

    /// Empty tree → empty colour map. Guards against a panic in the
    /// `normalise_scores` empty-input path when the engine reports no
    /// candidates at all (e.g. a freshly-imported character whose tree
    /// hasn't loaded yet).
    #[test]
    fn compute_heatmap_inputs_empty_tree_returns_empty_map() {
        let tree = empty_tree();
        let c = fresh_character();
        let out = compute_heatmap_inputs(
            &c,
            &tree,
            None,
            None,
            None,
            None,
            HeatmapStat::default(),
            None,
        );
        assert!(out.is_empty());
    }

    /// Ranking tree → only the impactful, allocatable, unallocated
    /// nodes appear in the output. Stat-less notables, masteries, and
    /// already-allocated nodes are filtered out by the engine ranker
    /// before normalisation, so the renderer can paint the map
    /// directly without re-filtering.
    #[test]
    fn compute_heatmap_inputs_only_includes_impactful_unallocated_nodes() {
        let tree = ranking_tree();
        let c = fresh_character();
        let out = compute_heatmap_inputs(
            &c,
            &tree,
            None,
            None,
            None,
            None,
            HeatmapStat::default(),
            None,
        );
        // Nodes 2 (Life) and 3 (Strength) score; 4 (stat-less notable),
        // 5 (mastery), and 1 (class start, anchored) drop out.
        let mut ids: Vec<NodeId> = out.keys().copied().collect();
        ids.sort_unstable();
        assert_eq!(ids, vec![2, 3]);
    }

    /// The top-impact node maps to the gradient's hot end (red), the
    /// bottom-impact node to the cold end (blue). This pins the
    /// "hottest = best to take next" reading the renderer relies on.
    #[test]
    fn compute_heatmap_inputs_top_node_is_red_bottom_is_blue() {
        let tree = ranking_tree();
        let c = fresh_character();
        let out = compute_heatmap_inputs(
            &c,
            &tree,
            None,
            None,
            None,
            None,
            HeatmapStat::default(),
            None,
        );
        // Node 2 (+50 Life) carries a much larger EHP delta than node 3
        // (+10 Strength), so it should land at the hot end.
        let red = score_to_colour(1.0);
        let blue = score_to_colour(0.0);
        assert_eq!(
            out.get(&2),
            Some(&red),
            "top-impact Life notable should be the red end"
        );
        assert_eq!(
            out.get(&3),
            Some(&blue),
            "lower-impact Strength notable should be the blue end"
        );
    }

    /// Already-allocated nodes never appear in the heatmap. The
    /// overlay is a "what to take next" guide, so painting allocated
    /// nodes would clutter the rendering with stale info.
    #[test]
    fn compute_heatmap_inputs_excludes_already_allocated_nodes() {
        let tree = ranking_tree();
        let mut c = fresh_character();
        c.allocate(2);
        let out = compute_heatmap_inputs(
            &c,
            &tree,
            None,
            None,
            None,
            None,
            HeatmapStat::default(),
            None,
        );
        assert!(
            !out.contains_key(&2),
            "allocated node 2 must not appear in heatmap"
        );
        // Node 3 still ranks; it's the only remaining impactful candidate.
        assert!(out.contains_key(&3));
    }

    /// `score_impact_key` mirrors the ranker's sort key:
    /// `max(dps_delta, ehp_delta)`. A pure-EHP node and a pure-DPS
    /// node of the same magnitude report the same impact, so the
    /// heatmap tints them at the same intensity rather than washing
    /// out defensive nodes.
    #[test]
    fn score_impact_key_uses_max_of_dps_and_ehp() {
        let pure_dps = NodeScore {
            node_id: 1,
            dps_delta: 100.0,
            ehp_delta: 0.0,
        };
        let pure_ehp = NodeScore {
            node_id: 2,
            dps_delta: 0.0,
            ehp_delta: 100.0,
        };
        let mixed = NodeScore {
            node_id: 3,
            dps_delta: 60.0,
            ehp_delta: 40.0,
        };
        assert!((score_impact_key(&pure_dps, HeatmapStat::Combined) - 100.0).abs() < 1e-9);
        assert!((score_impact_key(&pure_ehp, HeatmapStat::Combined) - 100.0).abs() < 1e-9);
        // Mixed picks the larger axis (DPS here).
        assert!((score_impact_key(&mixed, HeatmapStat::Combined) - 60.0).abs() < 1e-9);
    }

    #[test]
    fn heatmap_legend_stops_returns_requested_count_and_endpoints() {
        // 5 stops at t = 0.0, 0.25, 0.5, 0.75, 1.0 — endpoints should
        // hit the gradient's cold + hot anchors.
        let stops = heatmap_legend_stops(5);
        assert_eq!(stops.len(), 5);
        assert!((stops[0].0 - 0.0).abs() < 1e-6);
        assert!((stops[4].0 - 1.0).abs() < 1e-6);
        assert_eq!(stops[0].1, score_to_colour(0.0));
        assert_eq!(stops[4].1, score_to_colour(1.0));
        // Middle stops carry sane positions for the renderer to paint.
        assert!((stops[2].0 - 0.5).abs() < 1e-6);
    }

    #[test]
    fn heatmap_legend_stops_clamps_to_minimum_of_two() {
        // A renderer that asks for 0 or 1 stops should still get a
        // drawable strip — silently clamp to 2 (endpoints).
        let zero = heatmap_legend_stops(0);
        assert_eq!(zero.len(), 2);
        assert!((zero[0].0 - 0.0).abs() < 1e-6);
        assert!((zero[1].0 - 1.0).abs() < 1e-6);
        let one = heatmap_legend_stops(1);
        assert_eq!(one.len(), 2);
    }

    #[test]
    fn heatmap_legend_stops_colours_are_distinct_along_gradient() {
        // Sample 6 stops; the gradient should advance monotonically
        // so no two adjacent stops collapse to the same colour.
        let stops = heatmap_legend_stops(6);
        for w in stops.windows(2) {
            assert_ne!(
                w[0].1, w[1].1,
                "adjacent legend stops should differ — got duplicate at t={}",
                w[0].0,
            );
        }
    }

    #[test]
    fn count_unallocated_within_depth_skips_seeds() {
        // ranking_tree node 1 is the seed; depth 1 reaches nodes 2-5.
        // The seed itself is allocated so the count should drop it.
        let tree = ranking_tree();
        let allocated: std::collections::HashSet<NodeId> = [1].into_iter().collect();
        let count = count_unallocated_within_depth(&tree, &allocated, 1);
        assert_eq!(count, 4, "expected 4 unallocated neighbours, got {count}");
    }

    #[test]
    fn count_unallocated_within_depth_depth_zero_is_zero() {
        // Depth 0 only seeds the allocated set itself — no unallocated
        // candidates reach the result.
        let tree = ranking_tree();
        let allocated: std::collections::HashSet<NodeId> = [1].into_iter().collect();
        assert_eq!(count_unallocated_within_depth(&tree, &allocated, 0), 0);
    }

    #[test]
    fn count_unallocated_within_depth_empty_allocated_returns_zero() {
        // No seed → BFS empty → no candidates.
        let tree = ranking_tree();
        let allocated: std::collections::HashSet<NodeId> = Default::default();
        assert_eq!(count_unallocated_within_depth(&tree, &allocated, 5), 0);
    }

    #[test]
    fn count_unallocated_within_depth_excludes_already_allocated_neighbours() {
        // When a neighbour is *also* allocated, it should still drop
        // from the count — the filter is "unallocated within depth".
        let tree = ranking_tree();
        let allocated: std::collections::HashSet<NodeId> = [1, 2].into_iter().collect();
        // Direct neighbours of 1+2: {3, 4, 5} (plus 1 and 2 which are
        // allocated). Count is 3.
        assert_eq!(count_unallocated_within_depth(&tree, &allocated, 1), 3);
    }

    #[test]
    fn filter_ascendancy_passthrough_when_hide_is_false() {
        // Disabled filter is a clone — every input survives.
        let tree = ranking_tree();
        let ranked = vec![
            NodeScore {
                node_id: 2,
                dps_delta: 5.0,
                ehp_delta: 0.0,
            },
            NodeScore {
                node_id: 3,
                dps_delta: 3.0,
                ehp_delta: 0.0,
            },
        ];
        let out = filter_ascendancy_from_ranked(&ranked, &tree, false);
        assert_eq!(out.len(), 2);
    }

    #[test]
    fn filter_ascendancy_drops_nodes_with_ascendancy_name() {
        // Build a tiny tree with one ascendancy node + one main-tree
        // node. The filter should drop just the ascendancy.
        use ahash::HashMap;
        use pob_data::{Node, NodeKind, TreeConstants, TreePoints};
        use smallvec::SmallVec;
        let mut nodes: HashMap<NodeId, Node> = HashMap::default();
        nodes.insert(
            10,
            Node {
                id: 10,
                name: Some("Main notable".into()),
                icon: None,
                ascendancy_name: None,
                stats: vec![],
                reminder_text: vec![],
                kind: NodeKind::Notable,
                class_start_index: None,
                group: None,
                orbit: None,
                orbit_index: None,
                out_edges: SmallVec::new(),
                in_edges: SmallVec::new(),
                mastery_effects: vec![],
                expansion_jewel_size: None,
                jewel_radius: None,
            },
        );
        nodes.insert(
            20,
            Node {
                id: 20,
                name: Some("Asc notable".into()),
                icon: None,
                ascendancy_name: Some("Slayer".into()),
                stats: vec![],
                reminder_text: vec![],
                kind: NodeKind::Notable,
                class_start_index: None,
                group: None,
                orbit: None,
                orbit_index: None,
                out_edges: SmallVec::new(),
                in_edges: SmallVec::new(),
                mastery_effects: vec![],
                expansion_jewel_size: None,
                jewel_radius: None,
            },
        );
        let tree = PassiveTree {
            version: "test".into(),
            tree: "test".into(),
            classes: vec![],
            groups: HashMap::default(),
            nodes,
            jewel_slots: vec![],
            min_x: 0,
            min_y: 0,
            max_x: 0,
            max_y: 0,
            constants: TreeConstants {
                skills_per_orbit: vec![],
                orbit_radii: vec![],
                classes: HashMap::default(),
                character_attributes: HashMap::default(),
                pss_centre_inner_radius: None,
            },
            points: TreePoints::default(),
        };
        let ranked = vec![
            NodeScore {
                node_id: 10,
                dps_delta: 5.0,
                ehp_delta: 0.0,
            },
            NodeScore {
                node_id: 20,
                dps_delta: 50.0,
                ehp_delta: 0.0,
            },
        ];
        let out = filter_ascendancy_from_ranked(&ranked, &tree, true);
        let ids: Vec<NodeId> = out.iter().map(|s| s.node_id).collect();
        assert_eq!(ids, vec![10], "ascendancy entry should be filtered out");
    }

    #[test]
    fn filter_ascendancy_keeps_unknown_node_ids() {
        // Defensive: a stale ranked list pointing at a node that's
        // since been removed from the tree shouldn't trip the filter.
        // We keep it (the `tree.nodes.get` returns None → map_or true).
        let tree = ranking_tree();
        let ranked = vec![NodeScore {
            node_id: 999,
            dps_delta: 1.0,
            ehp_delta: 0.0,
        }];
        let out = filter_ascendancy_from_ranked(&ranked, &tree, true);
        assert_eq!(out.len(), 1);
    }

    #[test]
    fn nodes_within_depth_zero_returns_just_allocated() {
        // Depth 0 = "don't expand the frontier" — the result is the
        // allocated set itself, no neighbours.
        let tree = ranking_tree();
        let allocated: std::collections::HashSet<NodeId> = [1].into_iter().collect();
        let reachable = nodes_within_depth(&tree, &allocated, 0);
        let mut ids: Vec<NodeId> = reachable.into_iter().collect();
        ids.sort_unstable();
        assert_eq!(ids, vec![1]);
    }

    #[test]
    fn nodes_within_depth_one_reaches_direct_neighbours() {
        // ranking_tree has node 1 with neighbours [2, 3, 4, 5]. Depth
        // 1 should grab all four. Depth 1 starts from `allocated`
        // (just node 1).
        let tree = ranking_tree();
        let allocated: std::collections::HashSet<NodeId> = [1].into_iter().collect();
        let reachable = nodes_within_depth(&tree, &allocated, 1);
        let mut ids: Vec<NodeId> = reachable.into_iter().collect();
        ids.sort_unstable();
        assert_eq!(ids, vec![1, 2, 3, 4, 5]);
    }

    #[test]
    fn nodes_within_depth_empty_allocated_returns_empty() {
        // No seed, nothing to walk from. Defensive against a fresh
        // build that hasn't allocated yet.
        let tree = ranking_tree();
        let allocated: std::collections::HashSet<NodeId> = Default::default();
        let reachable = nodes_within_depth(&tree, &allocated, 5);
        assert!(reachable.is_empty());
    }

    #[test]
    fn nodes_within_depth_stops_at_leaves() {
        // Depth 100 with a small tree should converge well before the
        // cap — every reachable node lands in the set and the BFS
        // halts when the frontier empties.
        let tree = ranking_tree();
        let allocated: std::collections::HashSet<NodeId> = [1].into_iter().collect();
        let reachable = nodes_within_depth(&tree, &allocated, 100);
        // All 5 nodes are reachable from node 1 in the ranking_tree.
        assert_eq!(reachable.len(), 5);
    }

    #[test]
    fn nodes_within_depth_walks_edges_in_both_directions() {
        // Build a 3-node chain `A → B → C` with the leaf only listing
        // `in_edges`. The BFS should still reach C from A at depth 2
        // because the passive tree is undirected — mirrors
        // `connected_allocations` in `perform.rs`.
        use ahash::HashMap;
        use pob_data::{Node, NodeKind, TreeConstants, TreePoints};
        use smallvec::SmallVec;
        let mut nodes: HashMap<NodeId, Node> = HashMap::default();
        let mut add = |id: NodeId, out: &[NodeId], in_: &[NodeId]| {
            nodes.insert(
                id,
                Node {
                    id,
                    name: None,
                    icon: None,
                    ascendancy_name: None,
                    stats: vec![],
                    reminder_text: vec![],
                    kind: NodeKind::Normal,
                    class_start_index: None,
                    group: None,
                    orbit: None,
                    orbit_index: None,
                    out_edges: out.iter().copied().collect::<SmallVec<_>>(),
                    in_edges: in_.iter().copied().collect::<SmallVec<_>>(),
                    mastery_effects: vec![],
                    expansion_jewel_size: None,
                    jewel_radius: None,
                },
            );
        };
        add(1, &[2], &[]);
        add(2, &[3], &[1]);
        // C lists only its inbound edge — the BFS should still reach it.
        add(3, &[], &[2]);
        let tree = PassiveTree {
            version: "test".into(),
            tree: "test".into(),
            classes: vec![],
            groups: HashMap::default(),
            nodes,
            jewel_slots: vec![],
            min_x: 0,
            min_y: 0,
            max_x: 0,
            max_y: 0,
            constants: TreeConstants {
                skills_per_orbit: vec![],
                orbit_radii: vec![],
                classes: HashMap::default(),
                character_attributes: HashMap::default(),
                pss_centre_inner_radius: None,
            },
            points: TreePoints::default(),
        };
        let allocated: std::collections::HashSet<NodeId> = [1].into_iter().collect();
        let reachable = nodes_within_depth(&tree, &allocated, 2);
        let mut ids: Vec<NodeId> = reachable.into_iter().collect();
        ids.sort_unstable();
        assert_eq!(ids, vec![1, 2, 3]);
    }

    #[test]
    fn format_top_node_candidate_rows_pairs_node_ids_with_lines() {
        // The per-row formatter mirrors `format_top_node_candidates`
        // but also carries the `NodeId` so the renderer can wire a
        // hover tooltip per row.
        let tree = ranking_tree();
        let ranked = vec![
            NodeScore {
                node_id: 2,
                dps_delta: 0.0,
                ehp_delta: 50.0,
            },
            NodeScore {
                node_id: 3,
                dps_delta: 30.0,
                ehp_delta: 0.0,
            },
        ];
        let rows = format_top_node_candidate_rows(&ranked, &tree, 10, HeatmapStat::Combined);
        assert_eq!(rows.len(), 2);
        assert_eq!(rows[0].0, 2);
        assert!(rows[0].1.contains("+50 EHP"));
        assert!(rows[0].1.ends_with("n2"));
        assert_eq!(rows[1].0, 3);
        assert!(rows[1].1.contains("+30 DPS"));
    }

    #[test]
    fn format_top_node_candidate_rows_respects_axis_and_top_n() {
        // Axis-aware sort changes the order; top_n truncates.
        let tree = ranking_tree();
        let ranked = vec![
            NodeScore {
                node_id: 2,
                dps_delta: 0.0,
                ehp_delta: 100.0,
            },
            NodeScore {
                node_id: 3,
                dps_delta: 500.0,
                ehp_delta: 0.0,
            },
        ];
        // EHP axis: #2 first (100 ehp_delta beats #3's 0)
        let rows = format_top_node_candidate_rows(&ranked, &tree, 10, HeatmapStat::Ehp);
        assert_eq!(rows.first().map(|(id, _)| *id), Some(2));
        // top_n=1 keeps just the head.
        let one = format_top_node_candidate_rows(&ranked, &tree, 1, HeatmapStat::Combined);
        assert_eq!(one.len(), 1);
    }

    #[test]
    fn format_top_node_candidate_rows_lists_rank_deltas_and_name() {
        // Build a tiny tree with two named notables, score them, and
        // confirm the formatter emits one row per node with the
        // expected rank ordering and signed deltas. Pair-shape (the
        // NodeId on each row) is covered by the test above; this one
        // pins the *line content*.
        let tree = ranking_tree();
        let ranked = vec![
            NodeScore {
                node_id: 2,
                dps_delta: 0.0,
                ehp_delta: 50.0,
            },
            NodeScore {
                node_id: 3,
                dps_delta: 30.0,
                ehp_delta: 0.0,
            },
        ];
        let rows = format_top_node_candidate_rows(&ranked, &tree, 10, HeatmapStat::Combined);
        let lines: Vec<&str> = rows.iter().map(|(_, line)| line.as_str()).collect();
        assert_eq!(lines.len(), 2);
        assert!(
            lines[0].starts_with("1. "),
            "expected rank 1 prefix: {}",
            lines[0]
        );
        assert!(
            lines[0].contains("+50 EHP"),
            "EHP delta in line 1: {}",
            lines[0]
        );
        assert!(lines[0].ends_with("n2"), "name 'n2' at tail: {}", lines[0]);
        assert!(
            lines[1].starts_with("2. "),
            "expected rank 2 prefix: {}",
            lines[1]
        );
        assert!(lines[1].contains("+30 DPS"));
        assert!(lines[1].ends_with("n3"));
    }

    #[test]
    fn format_top_node_candidate_rows_fall_back_to_hash_id_for_unknown() {
        // Stale rank list against a tree that no longer carries the id
        // — fall back to `#<id>` so the user still sees something.
        let tree = empty_tree();
        let ranked = vec![NodeScore {
            node_id: 999,
            dps_delta: 10.0,
            ehp_delta: 5.0,
        }];
        let rows = format_top_node_candidate_rows(&ranked, &tree, 10, HeatmapStat::Combined);
        assert_eq!(rows.len(), 1);
        assert!(rows[0].1.contains("#999"));
    }

    #[test]
    fn format_top_node_candidate_rows_truncate_to_top_n() {
        let tree = empty_tree();
        let ranked: Vec<NodeScore> = (0u32..5)
            .map(|i| NodeScore {
                node_id: i,
                dps_delta: f64::from(i),
                ehp_delta: 0.0,
            })
            .collect();
        let rows = format_top_node_candidate_rows(&ranked, &tree, 2, HeatmapStat::Combined);
        assert_eq!(rows.len(), 2);
        assert!(rows[0].1.starts_with("1. "));
        assert!(rows[1].1.starts_with("2. "));
    }

    #[test]
    fn format_top_node_candidate_rows_respect_stat_axis() {
        // Regression for the stat/overlay disagreement: switching the
        // heatmap stat axis must re-rank the panel so its order
        // matches `score_impact_key` for the chosen axis. Two nodes,
        // one DPS-strong, one EHP-strong; Combined keeps the input
        // order (both score 100), but DPS surfaces the DPS-strong one
        // first and EHP surfaces the EHP-strong one first.
        let tree = empty_tree();
        let ranked = vec![
            NodeScore {
                node_id: 1,
                dps_delta: 100.0,
                ehp_delta: 10.0,
            },
            NodeScore {
                node_id: 2,
                dps_delta: 10.0,
                ehp_delta: 100.0,
            },
        ];

        let dps = format_top_node_candidate_rows(&ranked, &tree, 2, HeatmapStat::Dps);
        assert!(
            dps[0].1.contains("+100 DPS"),
            "DPS axis rank 1: {}",
            dps[0].1
        );
        assert!(
            dps[1].1.contains("+10 DPS"),
            "DPS axis rank 2: {}",
            dps[1].1
        );

        let ehp = format_top_node_candidate_rows(&ranked, &tree, 2, HeatmapStat::Ehp);
        assert!(
            ehp[0].1.contains("+100 EHP"),
            "EHP axis rank 1: {}",
            ehp[0].1
        );
        assert!(
            ehp[1].1.contains("+10 EHP"),
            "EHP axis rank 2: {}",
            ehp[1].1
        );
    }

    #[test]
    fn top_n_filter_keeps_highest_scoring_nodes() {
        // Five synthetic nodes, descending scores. Top-N=2 keeps the
        // two hottest entries and drops the rest. Pure helper so we
        // don't need a real character / tree.
        let scores: Vec<(NodeId, f64)> = vec![(1, 10.0), (2, 50.0), (3, 5.0), (4, 99.0), (5, 1.0)];
        let out = truncate_to_top_n(scores, 2);
        let kept: ahash::HashSet<NodeId> = out.iter().map(|(id, _)| *id).collect();
        assert_eq!(kept.len(), 2);
        assert!(kept.contains(&4), "highest-score node should survive");
        assert!(kept.contains(&2), "second-highest should survive");
    }

    #[test]
    fn top_n_filter_with_n_greater_than_len_returns_all() {
        // Asking for more than we have is a no-op rather than a panic.
        let scores: Vec<(NodeId, f64)> = vec![(1, 10.0), (2, 20.0)];
        let out = truncate_to_top_n(scores, 100);
        assert_eq!(out.len(), 2);
    }

    #[test]
    fn top_n_filter_with_zero_returns_empty() {
        // Defensive: explicit zero clears the heatmap rather than
        // panicking via an underflowed `take`.
        let scores: Vec<(NodeId, f64)> = vec![(1, 10.0), (2, 20.0)];
        let out = truncate_to_top_n(scores, 0);
        assert!(out.is_empty());
    }

    #[test]
    fn top_n_filter_ignores_nan_when_picking_top() {
        // NaN scores fall to the bottom so a stray engine NaN can't
        // displace a real top entry.
        let scores: Vec<(NodeId, f64)> =
            vec![(1, 5.0), (2, f64::NAN), (3, 10.0), (4, f64::NAN), (5, 1.0)];
        let out = truncate_to_top_n(scores, 2);
        let kept: ahash::HashSet<NodeId> = out.iter().map(|(id, _)| *id).collect();
        assert!(kept.contains(&3));
        assert!(kept.contains(&1));
        assert!(!kept.contains(&2));
        assert!(!kept.contains(&4));
    }

    #[test]
    fn compute_heatmap_inputs_top_n_keeps_only_best() {
        // End-to-end: ranking_tree has 2 impactful nodes (2 and 3).
        // Asking for top 1 keeps the larger-impact Life notable (2)
        // and drops the Strength notable (3). Defends the wiring
        // between rank_node_additions and the truncate helper.
        let tree = ranking_tree();
        let c = fresh_character();
        let out = compute_heatmap_inputs(
            &c,
            &tree,
            None,
            None,
            None,
            None,
            HeatmapStat::default(),
            Some(1),
        );
        assert_eq!(out.len(), 1);
        assert!(out.contains_key(&2), "top-1 should be the Life notable");
    }

    #[test]
    fn compute_heatmap_inputs_top_n_none_keeps_everything() {
        // `None` means "no cap" — the existing behaviour. Confirms the
        // refactor doesn't silently truncate.
        let tree = ranking_tree();
        let c = fresh_character();
        let out = compute_heatmap_inputs(
            &c,
            &tree,
            None,
            None,
            None,
            None,
            HeatmapStat::default(),
            None,
        );
        assert_eq!(out.len(), 2, "Life + Strength notables both kept");
    }

    #[test]
    fn colour_changes_monotonically_along_gradient() {
        // Sample 11 evenly-spaced points and assert each step actually
        // moves in colour space — guards against accidentally
        // collapsing the gradient to a single colour.
        let mut last: Option<egui::Color32> = None;
        for i in 0..=10 {
            let t = i as f32 / 10.0;
            let c = score_to_colour(t);
            if let Some(prev) = last {
                assert_ne!(prev, c, "gradient should advance at step {i}");
            }
            last = Some(c);
        }
    }
}
