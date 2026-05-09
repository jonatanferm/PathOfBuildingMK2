//! Timeless jewel keystone-replacement catalogue — slice 1 of
//! [#30](https://github.com/jonatanferm/PathOfBuildingMK2/issues/30).
//!
//! Mirrors PoB's `Data/TimelessJewelData/LegionPassives.lua` for the **keystone**
//! variants only. When a Timeless jewel is socketed into a tree socket, every
//! keystone whose tree position falls inside the jewel's radius is replaced by
//! the conqueror's keystone (e.g. Glorious Vanity / Doryani replaces every
//! allocated keystone in radius with `vaal_keystone_3` → "Corrupted Soul").
//!
//! Notable replacement and small-node replacement also exist in PoB but require
//! per-seed lookup tables (`data.readLUT(conqueror.id, node.id, jewelType)`)
//! that come from compressed binary blobs in
//! `.PathOfBuilding/src/Data/TimelessJewelData/*.zip`. This slice ships only the
//! keystone half of the mechanic, which is plain text and accounts for the
//! highest-impact Timeless plays in practice (Glorious Vanity / Doryani →
//! Corrupted Soul stacking, Militant Faith / Maxarius → Transcendence, etc.).
//!
//! Notable / small-node replacement is reserved for a follow-up slice. The data
//! layout here keeps a `keystones` map keyed by `<conqueror_type>_keystone_<id>`
//! so adding `notables` / `small_nodes` maps later is purely additive.
//!
//! ## Data shape
//!
//! ```text
//! {
//!   "version": 1,
//!   "jewels": {
//!     "Glorious Vanity": {
//!       "conqueror_type": "vaal",
//!       "conquerors": [
//!         { "name": "Doryani", "conqueror_id": "3", "keystone_id": "vaal_keystone_3" },
//!         …
//!       ]
//!     },
//!     …
//!   },
//!   "keystones": {
//!     "vaal_keystone_3": {
//!       "name": "Corrupted Soul",
//!       "stats": ["50% of Non-Chaos Damage taken bypasses Energy Shield", …]
//!     },
//!     …
//!   }
//! }
//! ```

use indexmap::IndexMap;
use serde::{Deserialize, Serialize};

/// Loaded timeless-jewel catalogue. Indexed lookups by jewel base name and by
/// conqueror keystone id are O(1).
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct TimelessJewelData {
    /// Schema version. Currently `1`.
    #[serde(default = "default_version")]
    pub version: u32,
    /// Free-form comment so the on-disk JSON is self-describing for humans.
    #[serde(default, rename = "_comment", skip_serializing_if = "String::is_empty")]
    pub comment: String,
    /// Per-jewel-base configuration. Keyed by the jewel's base name as it
    /// appears on the item (`"Glorious Vanity"`, `"Lethal Pride"`, …).
    pub jewels: IndexMap<String, TimelessJewelConfig>,
    /// Conqueror-keystone mod text, keyed by the canonical PoB keystone id
    /// (`"vaal_keystone_1"` → Divine Flesh, `"vaal_keystone_3"` → Corrupted
    /// Soul, …). Each entry's `stats` lines flow into the player's modDB
    /// when the keystone replaces an allocated in-radius keystone.
    pub keystones: IndexMap<String, ConquerorKeystone>,
}

fn default_version() -> u32 {
    1
}

/// Per-jewel-base config. The jewel's mod text (`"Bathed in the blood of N
/// sacrificed in the name of <conqueror>"`) reveals which conqueror is at play;
/// the parser then looks up the matching `TimelessConqueror` to find the
/// replacement keystone id.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct TimelessJewelConfig {
    /// Conqueror nation name, used to build the keystone-id prefix when joining
    /// `<type>_keystone_<conqueror.id>`. One of `"vaal"` / `"karui"` /
    /// `"maraketh"` / `"templar"` / `"eternal"` / `"kalguur"`. Mirrors
    /// `conqueror.type` in PoB's `Data/ModCache.lua` `JewelData` payloads.
    pub conqueror_type: String,
    /// All conquerors a jewel of this base can be carved by (one variant per
    /// row in PoB's `Variant: …` block on the unique jewel template).
    pub conquerors: Vec<TimelessConqueror>,
}

/// One conqueror variant for a Timeless jewel. The `name` is what appears in
/// the jewel's mod text (`"…in the name of Doryani"` → `name = "Doryani"`).
/// `conqueror_id` matches PoB's `conqueror.id` field (sometimes a string like
/// `"2_v2"` for the post-3.11 v2 keystones, hence string-not-int).
/// `keystone_id` resolves into `TimelessJewelData.keystones[<id>]`.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct TimelessConqueror {
    pub name: String,
    pub conqueror_id: String,
    pub keystone_id: String,
}

/// Mod text for one conqueror keystone. PoB stores per-stat scaling info too
/// (`min` / `max` / `fmt`); for keystones the values are always fixed (PoB's
/// keystones never roll), so we keep just the rendered stat lines that flow
/// into `mod_parser`.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ConquerorKeystone {
    /// Display name (`"Corrupted Soul"`, `"Transcendence"`, …).
    pub name: String,
    /// One stat line per element of the upstream `sd[]` array.
    #[serde(default)]
    pub stats: Vec<String>,
}

/// Decode a `data/timeless_jewels.json` payload.
pub fn load_timeless_jewels(json: &str) -> serde_json::Result<TimelessJewelData> {
    serde_json::from_str(json)
}

impl TimelessJewelData {
    /// Find the conqueror entry on `jewel_base` whose textual name matches
    /// `conqueror_name` (case-insensitive). Returns `None` if either the jewel
    /// base or the conqueror name is unknown.
    pub fn find_conqueror(
        &self,
        jewel_base: &str,
        conqueror_name: &str,
    ) -> Option<(&TimelessJewelConfig, &TimelessConqueror)> {
        let cfg = self.jewels.get(jewel_base)?;
        let want = conqueror_name.trim().to_ascii_lowercase();
        let conq = cfg
            .conquerors
            .iter()
            .find(|c| c.name.eq_ignore_ascii_case(&want))?;
        Some((cfg, conq))
    }

    /// Look up the replacement keystone's stat lines for `(jewel_base,
    /// conqueror_name)`. Returns `None` for unknown entries.
    pub fn replacement_for(
        &self,
        jewel_base: &str,
        conqueror_name: &str,
    ) -> Option<&ConquerorKeystone> {
        let (_, conq) = self.find_conqueror(jewel_base, conqueror_name)?;
        self.keystones.get(&conq.keystone_id)
    }

    /// True if `jewel_base` is a known Timeless jewel base. Used by the
    /// engine's identification step to short-circuit on names like
    /// `"Crimson Jewel"`.
    pub fn is_timeless_base(&self, jewel_base: &str) -> bool {
        self.jewels.contains_key(jewel_base)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample() -> TimelessJewelData {
        let json = r#"{
          "version": 1,
          "jewels": {
            "Glorious Vanity": {
              "conqueror_type": "vaal",
              "conquerors": [
                {"name": "Doryani",  "conqueror_id": "3",    "keystone_id": "vaal_keystone_3"},
                {"name": "Xibaqua",  "conqueror_id": "1",    "keystone_id": "vaal_keystone_1"},
                {"name": "Ahuana",   "conqueror_id": "2_v2", "keystone_id": "vaal_keystone_2_v2"}
              ]
            }
          },
          "keystones": {
            "vaal_keystone_1": {"name": "Divine Flesh",
              "stats": [
                "All Damage taken bypasses Energy Shield",
                "50% of Elemental Damage taken as Chaos Damage",
                "+5% to maximum Chaos Resistance"
              ]
            },
            "vaal_keystone_3": {"name": "Corrupted Soul",
              "stats": [
                "50% of Non-Chaos Damage taken bypasses Energy Shield",
                "Gain 15% of Maximum Life as Extra Maximum Energy Shield"
              ]
            },
            "vaal_keystone_2_v2": {"name": "Immortal Ambition",
              "stats": ["Energy Shield starts at zero"]
            }
          }
        }"#;
        load_timeless_jewels(json).expect("decode")
    }

    #[test]
    fn loads_minimal_payload() {
        let d = sample();
        assert_eq!(d.version, 1);
        assert!(d.is_timeless_base("Glorious Vanity"));
        assert!(!d.is_timeless_base("Crimson Jewel"));
        assert_eq!(d.jewels.len(), 1);
        assert_eq!(d.keystones.len(), 3);
    }

    #[test]
    fn lookup_resolves_conqueror_to_keystone() {
        let d = sample();
        let ks = d
            .replacement_for("Glorious Vanity", "Doryani")
            .expect("found");
        assert_eq!(ks.name, "Corrupted Soul");
        assert_eq!(ks.stats.len(), 2);
    }

    #[test]
    fn lookup_is_case_insensitive_on_conqueror() {
        let d = sample();
        assert!(d.replacement_for("Glorious Vanity", "doryani").is_some());
        assert!(d.replacement_for("Glorious Vanity", "DORYANI").is_some());
    }

    #[test]
    fn lookup_handles_v2_conqueror_id() {
        let d = sample();
        let ks = d
            .replacement_for("Glorious Vanity", "Ahuana")
            .expect("v2 keystone resolves");
        assert_eq!(ks.name, "Immortal Ambition");
    }

    #[test]
    fn unknown_jewel_or_conqueror_returns_none() {
        let d = sample();
        assert!(d.replacement_for("Crimson Jewel", "Doryani").is_none());
        assert!(d.replacement_for("Glorious Vanity", "NotAName").is_none());
    }

    #[test]
    fn empty_optional_fields_round_trip() {
        let d = sample();
        let json = serde_json::to_string(&d).expect("encode");
        let round: TimelessJewelData = serde_json::from_str(&json).expect("decode");
        assert_eq!(round.jewels.len(), d.jewels.len());
        assert_eq!(round.keystones.len(), d.keystones.len());
    }
}
