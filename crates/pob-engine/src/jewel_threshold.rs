//! Threshold-jewel infrastructure scaffold (issue #196 follow-up).
//!
//! Threshold jewels — distinct from radius jewels — grant their text only when
//! a minimum count of a single attribute (Strength, Dexterity, or Intelligence)
//! is present on Allocated Passives within the jewel's radius. PoB models
//! these as `data.jewelData.funcList` entries with a check like
//! `data.Dex >= 40` before emitting the gated mods (see
//! `.PathOfBuilding/src/Modules/CalcSetup.lua` and
//! `.PathOfBuilding/src/Data/Uniques/jewel.lua`).
//!
//! This module defines the **data shape** only. Wiring into the jewel-radius
//! dispatch (i.e. actually emitting the granted mods when the threshold is met)
//! is a deliberate next slice — keeping the shape and the dispatch in
//! separate files lets each land independently and keeps `jewel_radius.rs`
//! growth bounded.
//!
//! ## Worked example
//!
//! Volley Fire (Viridian Jewel, current variant):
//!
//! ```text
//! Limited to: 1
//! Radius: Medium
//! (7-10)% increased Projectile Damage
//! With at least 40 Dexterity in Radius, Barrage fires an additional 6
//!   projectiles simultaneously on the first and final attacks
//! ```
//!
//! The first line is a normal global jewel mod; the gated line is what this
//! scaffold tracks. The gated text is stored verbatim — parsing into engine
//! `Mod`s lives in the dispatch slice that follows.
//!
//! ## What lives here
//!
//! - [`Attribute`]: Str / Dex / Int discriminator.
//! - [`ThresholdJewelHandler`]: name, attribute, threshold count, gated mod
//!   text.
//! - [`identify_threshold_jewel`]: matches an [`Item`] against the static
//!   registry by *item name* (mirrors PoB's `funcList` table key).
//! - [`threshold_met`]: pure comparator with `>=` semantics.
//! - [`registered_threshold_jewels`]: the static registry. Starts with one
//!   entry (Volley Fire current); future slices append rows as they ship.

use pob_data::item::Item;

/// One of the three primary attributes that gate threshold jewels.
///
/// Mirrors PoB's `data.Str` / `data.Dex` / `data.Int` keys populated by the
/// radius-attribute scan.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Attribute {
    Strength,
    Dexterity,
    Intelligence,
}

impl Attribute {
    /// Long node-stat name as it appears in the passive tree text
    /// (`"+10 to Strength"` etc.). Used by the radius scan to attribute sums.
    pub fn long_name(self) -> &'static str {
        match self {
            Self::Strength => "Strength",
            Self::Dexterity => "Dexterity",
            Self::Intelligence => "Intelligence",
        }
    }
}

/// Static descriptor for a threshold jewel.
///
/// One row per gated effect. A jewel like Endless Misery that has *three*
/// gated lines (each conditional on the same 40-Int threshold) registers
/// three rows; the dispatch slice will fan-out granted mods accordingly.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ThresholdJewelHandler {
    /// Item name (matches `Item.name`, e.g. `"Volley Fire"`).
    pub item_name: &'static str,
    /// Which attribute the threshold is checked against.
    pub attribute: Attribute,
    /// Minimum attribute count from Allocated Passives in Radius
    /// (PoB compares with `>=`).
    pub threshold: u32,
    /// Verbatim gated mod text, exactly as it appears after the
    /// `With at least N <Attribute> in Radius, ` prefix in the unique's
    /// item text. The dispatch slice will feed this through
    /// [`crate::mod_parser::parse_mod_line`].
    pub granted_mod: &'static str,
}

/// Static registry of known threshold jewels.
///
/// Sorted by item name for stable diffs as new entries land. Each new slice
/// appends rows here; the dispatch slice consumes the registry to wire
/// emissions. Starts with **Volley Fire** (current variant) as the canonical
/// reference shape.
pub fn registered_threshold_jewels() -> &'static [ThresholdJewelHandler] {
    &[ThresholdJewelHandler {
        item_name: "Volley Fire",
        attribute: Attribute::Dexterity,
        threshold: 40,
        granted_mod:
            "Barrage fires an additional 6 projectiles simultaneously on the first and final attacks",
    }]
}

/// Look up a threshold-jewel handler by item *name* (mirrors PoB's
/// `funcList` key on `data.jewelData`).
///
/// Returns the first registry row whose `item_name` matches the item's name
/// exactly. Returns `None` for items the scaffold doesn't yet know about —
/// callers in the eventual dispatch site should fall through to the existing
/// radius-jewel handling for those.
///
/// Item *base name* (e.g. `"Viridian Jewel"`) is intentionally **not**
/// inspected here: many threshold jewels share a base, and the gating mod text
/// is unique-name specific.
pub fn identify_threshold_jewel(item: &Item) -> Option<&'static ThresholdJewelHandler> {
    registered_threshold_jewels()
        .iter()
        .find(|h| h.item_name == item.name)
}

/// Pure comparator: is the threshold met by `attribute_in_radius`?
///
/// Wraps `attribute_in_radius >= threshold` as a named function so future
/// edge cases (PoB has a tiny number of `>` vs `>=` quirks) have a single
/// touchpoint. The current rule is plain `>=` — see
/// `.PathOfBuilding/src/Modules/CalcSetup.lua` `data.Dex >= 40` style checks.
pub fn threshold_met(attribute_in_radius: u32, threshold: u32) -> bool {
    attribute_in_radius >= threshold
}

#[cfg(test)]
mod tests {
    use super::*;
    use pob_data::item::{ModLine, ModSection, Rarity};

    fn mk_item_named(name: &str, base: &str, lines: &[(&str, ModSection)]) -> Item {
        Item {
            name: name.into(),
            base_name: base.into(),
            rarity: Rarity::Unique,
            item_level: 84,
            quality: 0,
            tags: ahash::HashSet::default(),
            mod_lines: lines
                .iter()
                .map(|(l, s)| ModLine {
                    line: (*l).to_string(),
                    section: *s,
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

    #[test]
    fn attribute_long_name_matches_tree_text() {
        assert_eq!(Attribute::Strength.long_name(), "Strength");
        assert_eq!(Attribute::Dexterity.long_name(), "Dexterity");
        assert_eq!(Attribute::Intelligence.long_name(), "Intelligence");
    }

    #[test]
    fn threshold_met_uses_geq_semantics() {
        assert!(!threshold_met(0, 40));
        assert!(!threshold_met(39, 40));
        // Exactly the threshold counts (PoB uses `>=`).
        assert!(threshold_met(40, 40));
        assert!(threshold_met(120, 40));
    }

    #[test]
    fn threshold_met_zero_threshold_is_always_true() {
        // No known threshold jewel has threshold = 0, but the comparator
        // should be well-defined for the edge case.
        assert!(threshold_met(0, 0));
        assert!(threshold_met(5, 0));
    }

    #[test]
    fn registry_has_volley_fire_with_correct_shape() {
        let registry = registered_threshold_jewels();
        let vf = registry
            .iter()
            .find(|h| h.item_name == "Volley Fire")
            .expect("Volley Fire registered");
        assert_eq!(vf.attribute, Attribute::Dexterity);
        assert_eq!(vf.threshold, 40);
        // Granted mod is verbatim post-prefix text, ready for the dispatch
        // slice to feed through the mod parser. Spot-check the substantive
        // payload rather than the full string so future wording tweaks don't
        // silently break the registry.
        assert!(vf.granted_mod.contains("Barrage"));
        assert!(vf.granted_mod.contains("additional 6 projectiles"));
    }

    #[test]
    fn registry_entries_are_unique_by_name_and_granted_mod() {
        // Two rows that gate the same effect on the same item would
        // double-count emissions in the eventual dispatch.
        let registry = registered_threshold_jewels();
        let mut seen = ahash::HashSet::default();
        for h in registry {
            let key = (h.item_name, h.granted_mod);
            assert!(
                seen.insert(key),
                "duplicate registry row: {} / {}",
                h.item_name,
                h.granted_mod
            );
        }
    }

    #[test]
    fn identify_matches_by_item_name() {
        let item = mk_item_named(
            "Volley Fire",
            "Viridian Jewel",
            &[
                ("8% increased Projectile Damage", ModSection::Explicit),
                (
                    "With at least 40 Dexterity in Radius, Barrage fires an additional \
                     6 projectiles simultaneously on the first and final attacks",
                    ModSection::Explicit,
                ),
            ],
        );
        let h = identify_threshold_jewel(&item).expect("Volley Fire matched");
        assert_eq!(h.item_name, "Volley Fire");
        assert_eq!(h.attribute, Attribute::Dexterity);
        assert_eq!(h.threshold, 40);
    }

    #[test]
    fn identify_returns_none_for_unknown_item() {
        let item = mk_item_named(
            "Some Random Jewel",
            "Viridian Jewel",
            &[("10% increased Cold Damage", ModSection::Explicit)],
        );
        assert!(identify_threshold_jewel(&item).is_none());
    }

    #[test]
    fn identify_returns_none_for_radius_jewel_with_matching_base() {
        // Many threshold jewels share a base with non-threshold radius jewels.
        // Identification must key on the *unique name*, not the base, otherwise
        // the dispatch slice will mis-route radius-only jewels through the
        // threshold path.
        let item = mk_item_named(
            "Inertia",
            "Viridian Jewel",
            &[("Some attribute transform text", ModSection::Explicit)],
        );
        assert!(identify_threshold_jewel(&item).is_none());
    }

    #[test]
    fn identify_is_case_sensitive_to_match_pob_funclist_keys() {
        // PoB keys `funcList` by exact unique name. Mirroring exact-match here
        // avoids accidentally matching mis-cased imports.
        let item = mk_item_named(
            "volley fire",
            "Viridian Jewel",
            &[("anything", ModSection::Explicit)],
        );
        assert!(identify_threshold_jewel(&item).is_none());
    }
}
