//! Canonical jewel-radius table — mirrors PoB's `data.jewelRadii` from
//! `Modules/Data.lua:526`.
//!
//! A radius jewel socketed into a tree socket affects passive nodes whose Cartesian
//! distance from the socket falls inside `[inner, outer]`. The same table feeds:
//!
//! * Vanilla node-modifying jewels ("X% increased Y to all Z passives in Radius") —
//!   their mod text picks the radius via the `Only affects Passives in <Size> Ring`
//!   tag (`Small` → 1, `Medium` → 2, `Large` → 3, `Very Large` → 4, `Massive` → 5).
//! * Cluster jewels — base `Small/Medium/Large` radii match indices 1..=3.
//! * Timeless / Watcher's Eye / Threshold jewels — same dispatch surface.
//!
//! `pob-engine`'s radius framework consumes `RADII_3_16` directly. The 3.15 table is
//! kept here in case a build is loaded against an older tree version (PoB switches
//! tables on the major/minor version pair in `setJewelRadiiGlobally`).

use serde::{Deserialize, Serialize};

/// One entry in the jewel-radius table. Distances are in the same Cartesian space as
/// node positions (group `(x, y)` + orbit + orbit_index). PoB squares both bounds at
/// load time for cheaper `dist² ∈ [inner², outer²]` checks; we expose the squared
/// helpers via [`JewelRadiusInfo::contains`].
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct JewelRadiusInfo {
    pub inner: f64,
    pub outer: f64,
    pub label: &'static str,
}

impl JewelRadiusInfo {
    pub const fn new(inner: f64, outer: f64, label: &'static str) -> Self {
        Self {
            inner,
            outer,
            label,
        }
    }

    /// Returns true iff `dist_sq` (squared distance from socket) is inside this band.
    /// Equivalent to `inner <= dist <= outer` without the sqrt.
    pub fn contains(&self, dist_sq: f64) -> bool {
        let inner_sq = self.inner * self.inner;
        let outer_sq = self.outer * self.outer;
        dist_sq >= inner_sq && dist_sq <= outer_sq
    }
}

/// PoB-3.16+ radii table. Indices 0..=4 are the canonical fixed-size buckets
/// (Small / Medium / Large / Very Large / Massive); 5..=9 are the "Variable"
/// donut bands timeless / threshold jewels use to gate sub-effects.
///
/// Mirrors `data.jewelRadii["3_16"]` in `Modules/Data.lua:538-550`.
pub const RADII_3_16: &[JewelRadiusInfo] = &[
    JewelRadiusInfo::new(0.0, 960.0, "Small"),
    JewelRadiusInfo::new(0.0, 1440.0, "Medium"),
    JewelRadiusInfo::new(0.0, 1800.0, "Large"),
    JewelRadiusInfo::new(0.0, 2400.0, "Very Large"),
    JewelRadiusInfo::new(0.0, 2880.0, "Massive"),
    JewelRadiusInfo::new(960.0, 1320.0, "Variable"),
    JewelRadiusInfo::new(1320.0, 1680.0, "Variable"),
    JewelRadiusInfo::new(1680.0, 2040.0, "Variable"),
    JewelRadiusInfo::new(2040.0, 2400.0, "Variable"),
    JewelRadiusInfo::new(2400.0, 2880.0, "Variable"),
];

/// PoB-3.15 (and older) radii table. Pre-3.16 used a tighter scale.
/// Mirrors `data.jewelRadii["3_15"]` in `Modules/Data.lua:527-537`.
pub const RADII_3_15: &[JewelRadiusInfo] = &[
    JewelRadiusInfo::new(0.0, 800.0, "Small"),
    JewelRadiusInfo::new(0.0, 1200.0, "Medium"),
    JewelRadiusInfo::new(0.0, 1500.0, "Large"),
    JewelRadiusInfo::new(850.0, 1100.0, "Variable"),
    JewelRadiusInfo::new(1150.0, 1400.0, "Variable"),
    JewelRadiusInfo::new(1450.0, 1700.0, "Variable"),
    JewelRadiusInfo::new(1750.0, 2000.0, "Variable"),
    JewelRadiusInfo::new(1750.0, 2000.0, "Variable"),
];

/// Pick the right radii table for a given tree version string (`"3_25"`, `"3_16"`,
/// `"3_15_ruthless"`, …). PoB's `setJewelRadiiGlobally` flips on `<= 3.15`. We do the
/// same comparison on the leading `<major>_<minor>` of the version directory name.
///
/// Falls back to `RADII_3_16` if the version string doesn't parse — the modern table
/// is the safer default for tooling.
pub fn radii_for_tree_version(version: &str) -> &'static [JewelRadiusInfo] {
    let prefix = version.split(['_', '.', '-']).take(2).collect::<Vec<_>>();
    if prefix.len() >= 2 {
        let (Ok(major), Ok(minor)) = (prefix[0].parse::<u32>(), prefix[1].parse::<u32>()) else {
            return RADII_3_16;
        };
        if major < 3 || (major == 3 && minor <= 15) {
            return RADII_3_15;
        }
    }
    RADII_3_16
}

/// Look up the radius index that PoB's `Only affects Passives in <Size> Ring` mod
/// text resolves to. Returns the 1-based PoB index (matches `radiusIndex` in
/// `Data/ModCache.lua:10235-10239`) — Small=1, Medium=2, Large=3, Very Large=4,
/// Massive=5. Returns `None` for unknown labels.
pub fn radius_index_for_label(label: &str) -> Option<usize> {
    Some(match label {
        "Small" => 0,
        "Medium" => 1,
        "Large" => 2,
        "Very Large" => 3,
        "Massive" => 4,
        _ => return None,
    })
}

/// The largest `outer` across the modern table — used to bound bounding-box prefilters
/// when iterating in-radius nodes.
pub fn max_outer(radii: &[JewelRadiusInfo]) -> f64 {
    radii.iter().map(|r| r.outer).fold(0.0_f64, f64::max)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn small_band_contains_zero_and_960() {
        let small = RADII_3_16[0];
        assert!(small.contains(0.0));
        assert!(small.contains(959.0 * 959.0));
        assert!(small.contains(960.0 * 960.0));
        assert!(!small.contains(961.0 * 961.0));
    }

    #[test]
    fn variable_donut_excludes_centre() {
        let donut = RADII_3_16[5]; // 960..1320 variable
        assert!(!donut.contains(0.0));
        assert!(!donut.contains(959.0 * 959.0));
        assert!(donut.contains(1000.0 * 1000.0));
        assert!(donut.contains(1320.0 * 1320.0));
    }

    #[test]
    fn label_to_index() {
        assert_eq!(radius_index_for_label("Small"), Some(0));
        assert_eq!(radius_index_for_label("Medium"), Some(1));
        assert_eq!(radius_index_for_label("Large"), Some(2));
        assert_eq!(radius_index_for_label("Very Large"), Some(3));
        assert_eq!(radius_index_for_label("Massive"), Some(4));
        assert_eq!(radius_index_for_label("Variable"), None);
    }

    #[test]
    fn version_picker_prefers_modern_table() {
        // Compare by value. Pointer-equality across const slices isn't
        // guaranteed across translation units even when the data is the same.
        assert_eq!(radii_for_tree_version("3_25"), RADII_3_16);
        assert_eq!(radii_for_tree_version("3_16"), RADII_3_16);
        assert_eq!(radii_for_tree_version("3_15"), RADII_3_15);
        assert_eq!(radii_for_tree_version("3_10_ruthless"), RADII_3_15);
        // Unknown / malformed → modern table.
        assert_eq!(radii_for_tree_version("Default"), RADII_3_16);
    }
}
