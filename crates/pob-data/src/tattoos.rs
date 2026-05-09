//! Tattoo passive definitions — mirrors PoB's `Data/TattooPassives.lua`.
//!
//! Foundation slice for [#98](https://github.com/jonatanferm/PathOfBuildingMK2/issues/98)
//! (the data-extraction half of the deferred work from [#29](https://github.com/jonatanferm/PathOfBuildingMK2/issues/29) /
//! [PR #93](https://github.com/jonatanferm/PathOfBuildingMK2/pull/93)). Captures the
//! tattoo catalogue so the upcoming Tree-tab right-click picker can browse + apply
//! tattoos. The engine-side override mechanism (`Character::tattoo_overrides`) already
//! exists; this PR adds the catalogue.
//!
//! Each tattoo replaces an existing tree node's stat lines. `target_type` indicates
//! which node kind it can replace (`"Keystone"`, `"Notable"`, `"Small"`); the picker
//! filters available tattoos by the right-clicked node's kind.

use indexmap::IndexMap;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TattooSet {
    /// Tattoo definitions keyed by display name (matches the upstream Lua key).
    pub nodes: IndexMap<String, Tattoo>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Tattoo {
    /// Display name (matches the map key; kept on the value too for self-contained rows).
    #[serde(default)]
    pub display_name: String,
    /// PoB's internal id (matches the underlying passive node identifier the tattoo
    /// is "based on", e.g. `acrobatics1136`).
    #[serde(default)]
    pub id: String,
    /// Icon path inside the upstream PoB asset tree.
    #[serde(default)]
    pub icon: String,
    /// Active-effect overlay shown in PoB's tree view when the tattoo is applied.
    #[serde(default)]
    pub active_effect_image: String,
    /// Stat description lines (the mod text the tattoo grants).
    #[serde(default)]
    pub stat_lines: Vec<String>,
    /// Which existing node kind the tattoo can replace: typically `"Keystone"`,
    /// `"Notable"`, or `"Small"`.
    #[serde(default)]
    pub target_type: String,
    /// Optional target name discriminator (rarely populated; usually empty).
    #[serde(default)]
    pub target_value: String,
    /// PoB's `overrideType` field — `"KeystoneTattoo"`, `"NotableTattoo"`, or `"Tattoo"`.
    #[serde(default)]
    pub override_type: String,
    /// True if this tattoo replaces a keystone node.
    #[serde(default)]
    pub is_keystone: bool,
    /// True if it replaces a notable.
    #[serde(default)]
    pub is_notable: bool,
    /// True if it replaces a mastery.
    #[serde(default)]
    pub is_mastery: bool,
    /// Connectivity constraints from PoB. `max_connected = 100` is PoB's "no cap".
    #[serde(default)]
    pub min_connected: u32,
    #[serde(default)]
    pub max_connected: u32,
}

pub fn load_tattoos(json: &str) -> serde_json::Result<TattooSet> {
    serde_json::from_str(json)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trips_minimal_payload() {
        let json = r#"{
          "nodes": {
            "Acrobatics": {
              "display_name": "Acrobatics",
              "id": "acrobatics1136",
              "icon": "Art/.../Acrobatics.png",
              "active_effect_image": "Art/.../Keystone.png",
              "stat_lines": [
                "Modifiers to Chance to Suppress Spell Damage instead apply...",
                "Maximum Chance to Dodge Spell Hits is 75%",
                "Limited to 1 Keystone Tattoo"
              ],
              "target_type": "Keystone",
              "override_type": "KeystoneTattoo",
              "is_keystone": true,
              "min_connected": 0, "max_connected": 100
            }
          }
        }"#;
        let set = load_tattoos(json).expect("decode");
        assert_eq!(set.nodes.len(), 1);
        let acro = set.nodes.get("Acrobatics").unwrap();
        assert!(acro.is_keystone);
        assert_eq!(acro.target_type, "Keystone");
        assert_eq!(acro.stat_lines.len(), 3);
        assert_eq!(acro.id, "acrobatics1136");
    }

    #[test]
    fn missing_optional_fields_default_cleanly() {
        let set: TattooSet =
            serde_json::from_str(r#"{"nodes":{"X":{"display_name":"X"}}}"#).unwrap();
        let x = set.nodes.get("X").unwrap();
        assert_eq!(x.display_name, "X");
        assert!(!x.is_keystone);
        assert!(x.stat_lines.is_empty());
        assert_eq!(x.max_connected, 0);
    }
}
