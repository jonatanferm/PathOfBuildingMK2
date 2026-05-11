//! Extract `Data/EnchantmentHelmet.lua` into the typed
//! [`pob_data::HelmetEnchantSet`].
//!
//! Foundation slice for [#221](https://github.com/jonatanferm/PathOfBuildingMK2/issues/221)
//! "Apply Enchantment picker". The UI follow-up wires a picker dialog
//! against the JSON this extractor emits.
//!
//! Upstream shape:
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
//! Unknown / future tier keys are ignored — we only mirror the two PoB
//! actually ships today.

use std::path::Path;

use anyhow::{anyhow, Context, Result};
use indexmap::IndexMap;
use pob_data::{HelmetEnchant, HelmetEnchantSet};
use serde_json::Value as J;

use crate::{load_lua_file_returning, make_lua};

pub fn extract(pob_root: &Path) -> Result<HelmetEnchantSet> {
    let path = pob_root.join("src/Data/EnchantmentHelmet.lua");
    let lua = make_lua()?;
    let json = load_lua_file_returning(&lua, &path)
        .with_context(|| format!("evaluating {}", path.display()))?;
    let obj = json
        .as_object()
        .ok_or_else(|| anyhow!("EnchantmentHelmet.lua did not return a table"))?;
    let mut by_skill: IndexMap<String, HelmetEnchant> = IndexMap::with_capacity(obj.len());
    for (skill, tiers) in obj {
        let tiers_obj = tiers
            .as_object()
            .ok_or_else(|| anyhow!("EnchantmentHelmet.lua: entry `{skill}` is not a table"))?;
        let merciless = lines_for_tier(tiers_obj, "MERCILESS");
        let endgame = lines_for_tier(tiers_obj, "ENDGAME");
        if merciless.is_empty() && endgame.is_empty() {
            // Skip skills that ship with neither tier — upstream
            // shouldn't, but defensive against a future entry that
            // only carries the ignored tier keys we don't model.
            continue;
        }
        by_skill.insert(skill.clone(), HelmetEnchant { merciless, endgame });
    }
    // Sort alphabetically so the JSON ordering is deterministic
    // regardless of Lua iteration order (Lua tables aren't ordered;
    // the file source happens to be alphabetised but the parser
    // doesn't guarantee that surfaces through serde_json).
    by_skill.sort_keys();
    Ok(HelmetEnchantSet { by_skill })
}

/// Pull a tier's mod-line list. Returns `Vec::new()` if the tier key
/// is missing, the value isn't a JSON array, or any element isn't a
/// string — the conservative reading keeps a malformed entry from
/// poisoning the rest of the catalogue.
fn lines_for_tier(tiers: &serde_json::Map<String, J>, tier_key: &str) -> Vec<String> {
    let Some(arr) = tiers.get(tier_key).and_then(J::as_array) else {
        return Vec::new();
    };
    arr.iter()
        .filter_map(|v| v.as_str().map(str::to_owned))
        .collect()
}
