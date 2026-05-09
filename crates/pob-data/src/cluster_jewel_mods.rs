//! Cluster jewel notable / corrupted mods — mirrors PoB's `Data/ModJewelCluster.lua`.
//!
//! Slice 2 of [#21](https://github.com/jonatanferm/PathOfBuildingMK2/issues/21). Pairs with
//! the [`crate::cluster_jewels`] catalogue: the `ClusterJewelData::small_indicies` /
//! `notable_indicies` ring slots tell the synthesis pass *where* to place nodes, and these
//! mods are *what* the placed notables grant. Each `ClusterMod` entry roughly corresponds
//! to a single rolled prefix / suffix / corruption on a cluster jewel — its `stat_lines`
//! are the lines that flow into the synthesised notable's `sd[]`.
//!
//! The `weight_keys` / `weight_values` parallel arrays mirror PoB's per-tag rolling
//! weights (e.g. `weightKey = { "affliction_chance_to_block", "default" }`,
//! `weightVal = { 600, 0 }`). They're not used by the calc engine but are preserved so a
//! future jewel-rolling UI / probability tool can reuse them.

use indexmap::IndexMap;
use serde::{Deserialize, Serialize};

/// Top-level: one entry per cluster mod, keyed by PoB's internal mod id
/// (e.g. `"AfflictionNotableProdigiousDefense__"`).
pub type ClusterModSet = IndexMap<String, ClusterMod>;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ClusterMod {
    /// `"Corrupted"` / `"Prefix"` / `"Suffix"`.
    #[serde(default, rename = "type")]
    pub mod_type: String,
    /// User-facing affix label. `"Notable"` for the notable-grant prefixes, otherwise
    /// often empty.
    #[serde(default)]
    pub affix: String,
    /// Stat description line(s) the mod grants. Most rolls have one line; multi-line
    /// suffixes (e.g. "Avoid being Chilled" + "Avoid being Frozen") have two.
    #[serde(default)]
    pub stat_lines: Vec<String>,
    /// PoB's stable stat-order numbers used for sorting.
    #[serde(default)]
    pub stat_order: Vec<u32>,
    /// Minimum item level the mod can appear on.
    #[serde(default)]
    pub level: u32,
    /// PoB's `group` identifier — mods in the same group don't roll together.
    #[serde(default)]
    pub group: String,
    /// Tag → weight parallel arrays from upstream `weightKey` / `weightVal`.
    #[serde(default)]
    pub weight_keys: Vec<String>,
    #[serde(default)]
    pub weight_values: Vec<i32>,
    /// Optional `weightMultiplierKey` / `weightMultiplierVal` table — PoB layers these
    /// on top of the base weights for jewel-size-aware rolling.
    #[serde(default)]
    pub weight_multiplier_keys: Vec<String>,
    #[serde(default)]
    pub weight_multiplier_values: Vec<i32>,
    /// Free-form tags PoB attaches to the rolled mod (e.g. `"has_affliction_notable"`).
    #[serde(default)]
    pub tags: Vec<String>,
    /// Mod-classification tags the search UI uses (e.g. `"chaos"`, `"resistance"`).
    #[serde(default)]
    pub mod_tags: Vec<String>,
}

pub fn load_cluster_jewel_mods(json: &str) -> serde_json::Result<ClusterModSet> {
    serde_json::from_str(json)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trips_minimal_payload() {
        let json = r#"{
          "ChaosResistJewelCorrupted": {
            "type": "Corrupted",
            "affix": "",
            "stat_lines": ["+(1-3)% to Chaos Resistance"],
            "stat_order": [1641],
            "level": 1,
            "group": "ChaosResistance",
            "weight_keys": ["jewel", "default"],
            "weight_values": [0, 0],
            "mod_tags": ["chaos", "resistance"]
          }
        }"#;
        let set = load_cluster_jewel_mods(json).expect("decode");
        assert_eq!(set.len(), 1);
        let m = set.get("ChaosResistJewelCorrupted").unwrap();
        assert_eq!(m.mod_type, "Corrupted");
        assert_eq!(m.stat_lines.len(), 1);
        assert_eq!(m.group, "ChaosResistance");
        assert_eq!(m.weight_keys.len(), 2);
        assert!(m.weight_multiplier_keys.is_empty());
    }

    #[test]
    fn missing_optional_fields_default_cleanly() {
        let set: ClusterModSet = serde_json::from_str(r#"{"X":{"type":"Suffix"}}"#).unwrap();
        let x = set.get("X").unwrap();
        assert_eq!(x.mod_type, "Suffix");
        assert!(x.stat_lines.is_empty());
        assert_eq!(x.level, 0);
    }
}
