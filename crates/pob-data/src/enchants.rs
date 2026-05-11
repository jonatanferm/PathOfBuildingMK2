//! Helmet (and future glove / boot / belt / body / weapon / flask)
//! enchantment catalogue, mirroring PoB's `Data/EnchantmentHelmet.lua`
//! family.
//!
//! Foundation slice for [#221](https://github.com/jonatanferm/PathOfBuildingMK2/issues/221)
//! "Apply Enchantment picker". This module ships the data layer only —
//! struct shapes + JSON loader + tests — so the UI follow-up can wire
//! a picker dialog against a typed catalogue instead of parsing Lua
//! at runtime.
//!
//! Lua shape (per slot file):
//!
//! ```text
//! return {
//!   ["Absolution"] = {
//!     ["MERCILESS"] = { "20% increased ...", ... },
//!     ["ENDGAME"]   = { "30% increased ...", ... },
//!   },
//!   ...
//! }
//! ```
//!
//! The two tiers are PoB's `MERCILESS` (Lab) and `ENDGAME` (Eternal
//! Lab) tiers — the only ones the upstream tables expose. JSON shape:
//!
//! ```json
//! { "by_skill": { "Absolution": { "merciless": [...], "endgame": [...] } } }
//! ```

use indexmap::IndexMap;
use serde::{Deserialize, Serialize};

/// Catalogue of helmet enchantments, keyed by the skill they affect.
///
/// PoB's source file is alphabetical; we preserve that order on the
/// `IndexMap` so a future picker dialog can render rows in the same
/// sequence as the in-game lab vendor.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct HelmetEnchantSet {
    pub by_skill: IndexMap<String, HelmetEnchant>,
}

/// The two-tier mod set for one skill's helmet enchant. PoB exposes
/// both tiers so a future UI can show the user what they'd get from
/// Merciless vs Eternal Lab; the picker would commit one tier's mods
/// onto the equipped helmet's `enchant_mods` set.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct HelmetEnchant {
    /// Merciless / level-3 Lab tier. Empty when upstream lists nothing
    /// for the skill (shouldn't happen for shipped helmets, but
    /// defensive against new entries).
    #[serde(default)]
    pub merciless: Vec<String>,
    /// Eternal / level-4 Lab tier. Usually a stronger version of the
    /// merciless mod set with the same line shape.
    #[serde(default)]
    pub endgame: Vec<String>,
}

impl HelmetEnchantSet {
    /// Number of distinct skills with at least one enchant tier
    /// present. Mirrors `Tattoo`-style accessor shape for symmetry
    /// across data sets.
    #[must_use]
    pub fn len(&self) -> usize {
        self.by_skill.len()
    }

    /// Whether the catalogue is empty. Defensive — a missing data
    /// file loads as a default-constructed set; callers can branch on
    /// `is_empty()` to show a "data not extracted yet" hint instead
    /// of an empty picker.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.by_skill.is_empty()
    }

    /// Lookup by skill name. Case-sensitive (matches PoB's Lua keys
    /// exactly).
    #[must_use]
    pub fn get(&self, skill: &str) -> Option<&HelmetEnchant> {
        self.by_skill.get(skill)
    }

    /// Iterate `(skill_name, enchant)` pairs in the catalogue's
    /// declared order.
    pub fn iter(&self) -> impl Iterator<Item = (&String, &HelmetEnchant)> {
        self.by_skill.iter()
    }
}

/// Parse a JSON document produced by `pob-extract` into a typed
/// [`HelmetEnchantSet`]. The serde-default fields cover sparse
/// entries — e.g. a skill that only has a Merciless tier listed loads
/// with `endgame = Vec::new()` rather than failing.
pub fn load_helmet_enchants(json: &str) -> Result<HelmetEnchantSet, serde_json::Error> {
    serde_json::from_str(json)
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE: &str = r#"{
        "by_skill": {
            "Absolution": {
                "merciless": [
                    "20% increased Sentinel of Absolution Duration",
                    "8% increased Absolution Cast Speed"
                ],
                "endgame": [
                    "30% increased Sentinel of Absolution Duration",
                    "12% increased Absolution Cast Speed"
                ]
            },
            "Alchemist's Mark": {
                "merciless": ["20% increased Alchemist's Mark Curse Effect"],
                "endgame": ["30% increased Alchemist's Mark Curse Effect"]
            }
        }
    }"#;

    #[test]
    fn load_helmet_enchants_round_trips_a_two_skill_fixture() {
        let set = load_helmet_enchants(SAMPLE).expect("parse");
        assert_eq!(set.len(), 2);
        let absolution = set.get("Absolution").expect("Absolution present");
        assert_eq!(absolution.merciless.len(), 2);
        assert_eq!(absolution.endgame.len(), 2);
        assert!(absolution.merciless[0].contains("20% increased"));
        assert!(absolution.endgame[0].contains("30% increased"));
        let mark = set.get("Alchemist's Mark").expect("Mark present");
        assert_eq!(mark.merciless.len(), 1);
        assert_eq!(mark.endgame.len(), 1);
    }

    #[test]
    fn load_helmet_enchants_preserves_lua_declaration_order() {
        // PoB's source file is alphabetised; the IndexMap keeps the
        // catalogue in declaration order so a picker can render
        // rows in the same sequence as the in-game lab vendor.
        let set = load_helmet_enchants(SAMPLE).expect("parse");
        let keys: Vec<&str> = set.iter().map(|(k, _)| k.as_str()).collect();
        assert_eq!(keys, vec!["Absolution", "Alchemist's Mark"]);
    }

    #[test]
    fn load_helmet_enchants_serde_defaults_cover_sparse_tier_entries() {
        // Defensive: a skill listed with only one tier should load
        // with the other tier defaulted to empty, not error. Upstream
        // never ships this shape today, but the loader has to be
        // resilient if a future GGG patch slims an entry.
        let one_tier_only = r#"{
            "by_skill": {
                "Stub Skill": {
                    "merciless": ["+10 to Stub"]
                }
            }
        }"#;
        let set = load_helmet_enchants(one_tier_only).expect("parse");
        let stub = set.get("Stub Skill").expect("Stub present");
        assert_eq!(stub.merciless, vec!["+10 to Stub"]);
        assert!(stub.endgame.is_empty());
    }

    #[test]
    fn load_helmet_enchants_handles_empty_catalogue() {
        // A file that exists but contains no entries should load to
        // an empty set — the loader's `is_empty()` lets callers
        // distinguish "data not extracted yet" from "extracted but
        // empty" without crashing.
        let empty = r#"{ "by_skill": {} }"#;
        let set = load_helmet_enchants(empty).expect("parse");
        assert!(set.is_empty());
        assert_eq!(set.len(), 0);
        assert!(set.get("Anything").is_none());
    }
}
