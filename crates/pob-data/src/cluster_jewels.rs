//! Cluster jewel definitions — mirrors PoB's `Data/ClusterJewels.lua`.
//!
//! Slice 1 of [#21](https://github.com/jonatanferm/PathOfBuildingMK2/issues/21) ports the
//! static cluster jewel data: the three jewel categories (Small / Medium / Large), their
//! small-passive options, the notable sort-order lookup, the keystone whitelist, and the
//! per-notable orbit offset table. Sub-graph synthesis (which actually places these into
//! a passive tree when a cluster jewel is socketed) is a follow-up slice.
//!
//! The `_indicies` fields preserve PoB's spelling — they index into the jewel's
//! totalIndicies-slot ring so the synthesis pass can place small / notable / socket
//! nodes at the right positions on the orbit.

use indexmap::IndexMap;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ClusterJewelData {
    pub jewels: IndexMap<String, ClusterJewelType>,
    /// Notable name → numeric ID (PoB uses these for stable sort order across leagues).
    #[serde(default)]
    pub notable_sort_order: IndexMap<String, u32>,
    /// Keystones eligible to roll on a Large cluster jewel.
    #[serde(default)]
    pub keystones: Vec<String>,
    /// Orbit offset table — keyed by node identifier. Values are per-orbit slot offsets.
    #[serde(default)]
    pub orbit_offsets: IndexMap<u32, IndexMap<u32, u32>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ClusterJewelType {
    pub size: String,
    pub size_index: u8,
    pub min_nodes: u8,
    pub max_nodes: u8,
    /// Slot indices on the jewel's ring where small passives can land.
    #[serde(default)]
    pub small_indicies: Vec<u8>,
    /// Slot indices where notables can land.
    #[serde(default)]
    pub notable_indicies: Vec<u8>,
    /// Slot indices where jewel sockets (sub-jewel nesting) can land.
    #[serde(default)]
    pub socket_indicies: Vec<u8>,
    /// Total slots in the jewel's ring (typically 6/8/12).
    pub total_indicies: u8,
    /// Available small-passive options keyed by PoB skill id (`affliction_*`).
    pub skills: IndexMap<String, ClusterSkill>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ClusterSkill {
    pub name: String,
    pub icon: String,
    pub tag: String,
    /// Stat lines applied by each small-passive of this category.
    #[serde(default)]
    pub stats: Vec<String>,
    /// Enchantment text shown on the cluster jewel item itself.
    #[serde(default)]
    pub enchant: Vec<String>,
}

pub fn load_cluster_jewels(json: &str) -> serde_json::Result<ClusterJewelData> {
    serde_json::from_str(json)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trips_minimal_payload() {
        let json = r#"{
          "jewels": {
            "Small Cluster Jewel": {
              "size": "Small", "size_index": 0,
              "min_nodes": 2, "max_nodes": 3,
              "small_indicies": [0,4,2], "notable_indicies": [4],
              "socket_indicies": [4], "total_indicies": 6,
              "skills": {
                "affliction_maximum_life": {
                  "name": "Life", "icon": "Art/...png", "tag": "affliction_maximum_life",
                  "stats": ["4% increased maximum Life"],
                  "enchant": ["Added Small Passive Skills grant: 4% increased maximum Life"]
                }
              }
            }
          },
          "keystones": ["Disciple of Kitava"],
          "notable_sort_order": {"Force Multiplier": 11271}
        }"#;
        let data = load_cluster_jewels(json).expect("decode");
        assert_eq!(data.jewels.len(), 1);
        let small = data.jewels.get("Small Cluster Jewel").unwrap();
        assert_eq!(small.size, "Small");
        assert_eq!(small.total_indicies, 6);
        assert_eq!(small.skills.len(), 1);
        assert_eq!(
            small.skills.get("affliction_maximum_life").unwrap().name,
            "Life"
        );
        assert_eq!(data.keystones, vec!["Disciple of Kitava"]);
        assert_eq!(
            data.notable_sort_order.get("Force Multiplier"),
            Some(&11271)
        );
    }

    #[test]
    fn missing_optional_fields_default_cleanly() {
        // Older payloads may have only a `jewels` map.
        let data: ClusterJewelData = serde_json::from_str(
            r#"{"jewels":{"Small Cluster Jewel":{"size":"Small","size_index":0,"min_nodes":2,"max_nodes":3,"total_indicies":6,"skills":{}}}}"#,
        )
        .unwrap();
        assert!(data.keystones.is_empty());
        assert!(data.notable_sort_order.is_empty());
        assert!(data.orbit_offsets.is_empty());
        let small = data.jewels.get("Small Cluster Jewel").unwrap();
        assert!(small.small_indicies.is_empty());
    }
}
