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
