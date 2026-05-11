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

/// Which tier of a [`HelmetEnchant`] to apply. Picker UI surfaces both
/// so the user can preview both rolls before committing.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum HelmetEnchantTier {
    /// Level-3 Lab tier. PoB labels this `MERCILESS`.
    Merciless,
    /// Level-4 Lab tier (Eternal Lab). PoB labels this `ENDGAME`.
    #[default]
    Endgame,
}

impl HelmetEnchantTier {
    /// Pick the matching tier's mod-line slice off a [`HelmetEnchant`].
    /// Returns the empty slice when the chosen tier wasn't shipped for
    /// that skill (defensive against future sparse entries).
    #[must_use]
    pub fn lines(self, enchant: &HelmetEnchant) -> &[String] {
        match self {
            Self::Merciless => &enchant.merciless,
            Self::Endgame => &enchant.endgame,
        }
    }

    /// Human-readable label for the tier, for UI radio buttons.
    #[must_use]
    pub fn label(self) -> &'static str {
        match self {
            Self::Merciless => "Merciless (Lab)",
            Self::Endgame => "Endgame (Eternal Lab)",
        }
    }
}

/// Parse a JSON document produced by `pob-extract` into a typed
/// [`HelmetEnchantSet`]. The serde-default fields cover sparse
/// entries — e.g. a skill that only has a Merciless tier listed loads
/// with `endgame = Vec::new()` rather than failing.
pub fn load_helmet_enchants(json: &str) -> Result<HelmetEnchantSet, serde_json::Error> {
    serde_json::from_str(json)
}

/// Catalogue of "flat" enchantments — those whose Lua source is a
/// `{ tier: [mods] }` table rather than the skill-keyed shape helmet
/// enchants use. Gloves (`EnchantmentGloves.lua`) and boots
/// (`EnchantmentBoots.lua`) both fit this shape: every tier lists a
/// pool of single-line mods, and the user picks one.
///
/// IndexMap preserves Lua declaration order so a picker dialog can
/// render rows in the same sequence the in-game lab vendor uses.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct FlatEnchantSet {
    /// Tier name (matches upstream Lua keys: `NORMAL` / `CRUEL` /
    /// `MERCILESS` / `ENDGAME`) → list of mod lines available at
    /// that tier.
    pub by_tier: IndexMap<String, Vec<String>>,
}

impl FlatEnchantSet {
    /// Number of distinct tiers in the catalogue. Mirrors the
    /// accessor shape on [`HelmetEnchantSet`] / `TattooSet`.
    #[must_use]
    pub fn tier_count(&self) -> usize {
        self.by_tier.len()
    }

    /// Whether the catalogue is empty — used by the picker UI to
    /// distinguish "data file missing" from "loaded but vacant".
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.by_tier.is_empty()
    }

    /// Mod-line pool for a single tier. Returns an empty slice when
    /// the tier isn't present — defensive against a hand-edited JSON
    /// file or a future GGG patch that trims a tier.
    #[must_use]
    pub fn lines_for(&self, tier: &str) -> &[String] {
        self.by_tier
            .get(tier)
            .map_or(&[][..], std::vec::Vec::as_slice)
    }

    /// Iterate `(tier_name, mods)` pairs in the catalogue's declared
    /// order. The UI surfaces tiers as radio buttons against this
    /// list so adding a new tier upstream doesn't need a code change.
    pub fn iter(&self) -> impl Iterator<Item = (&String, &Vec<String>)> {
        self.by_tier.iter()
    }
}

/// Parse a JSON document produced by `pob-extract` into a typed
/// [`FlatEnchantSet`]. Same transport as
/// [`load_helmet_enchants`]; the struct shape differs because the
/// upstream Lua files do.
pub fn load_flat_enchants(json: &str) -> Result<FlatEnchantSet, serde_json::Error> {
    serde_json::from_str(json)
}

/// Convenience alias — glove enchants live in this format. Distinct
/// loader names give the caller (`LoadedApp`) a self-documenting site
/// for each slot type without growing per-slot newtypes.
pub fn load_glove_enchants(json: &str) -> Result<FlatEnchantSet, serde_json::Error> {
    load_flat_enchants(json)
}

/// Convenience alias for boot enchants — same JSON shape as gloves.
pub fn load_boot_enchants(json: &str) -> Result<FlatEnchantSet, serde_json::Error> {
    load_flat_enchants(json)
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

    const GLOVE_SAMPLE: &str = r#"{
        "by_tier": {
            "NORMAL": [
                "Trigger Word of Blades on Hit",
                "Trigger Word of Flames on Hit"
            ],
            "CRUEL": [
                "Trigger Edict of Blades on Hit"
            ],
            "MERCILESS": [
                "Trigger Decree of Blades on Hit"
            ]
        }
    }"#;

    #[test]
    fn load_flat_enchants_round_trips_glove_fixture() {
        // Issue #221 (glove/boot follow-up): glove enchants use a
        // flat `{ tier: [mods] }` shape — every tier carries its own
        // pool of single-mod choices, unlike helmet enchants which
        // are skill-keyed packages.
        let set = load_glove_enchants(GLOVE_SAMPLE).expect("parse glove");
        assert_eq!(set.tier_count(), 3);
        assert_eq!(set.lines_for("NORMAL").len(), 2);
        assert_eq!(
            set.lines_for("CRUEL"),
            &["Trigger Edict of Blades on Hit".to_owned()][..]
        );
        // Missing tier returns an empty slice rather than panicking
        // (`load_helmet_enchants` parity — picker won't crash on a
        // stale data file).
        assert!(set.lines_for("ETERNAL").is_empty());
    }

    #[test]
    fn load_flat_enchants_preserves_tier_declaration_order() {
        // Picker rows depend on this — IndexMap iteration order
        // must match the Lua source so the UI presents tiers in
        // the same sequence as the in-game lab vendor.
        let set = load_glove_enchants(GLOVE_SAMPLE).expect("parse glove");
        let tiers: Vec<&str> = set.iter().map(|(k, _)| k.as_str()).collect();
        assert_eq!(tiers, vec!["NORMAL", "CRUEL", "MERCILESS"]);
    }

    #[test]
    fn load_boot_enchants_uses_same_loader_as_glove() {
        // The boot loader is a convenience alias — confirm it parses
        // a boot-shaped fixture (CRUEL / MERCILESS tiers, single
        // mod each).
        let boot_sample = r#"{
            "by_tier": {
                "CRUEL": ["Adds 16 to 24 Fire Damage if you've Killed Recently"],
                "MERCILESS": ["Adds 33 to 50 Fire Damage if you've Killed Recently"]
            }
        }"#;
        let set = load_boot_enchants(boot_sample).expect("parse boot");
        assert_eq!(set.tier_count(), 2);
        assert!(set.lines_for("CRUEL")[0].contains("Killed Recently"));
        assert!(set.lines_for("MERCILESS")[0].contains("33 to 50"));
    }

    #[test]
    fn load_flat_enchants_handles_empty_catalogue() {
        let set = load_flat_enchants(r#"{ "by_tier": {} }"#).expect("parse empty");
        assert!(set.is_empty());
        assert_eq!(set.tier_count(), 0);
        assert!(set.lines_for("any").is_empty());
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
