//! Extract PoB's timeless replacement-passive pool.
//!
//! `Data/TimelessJewelData/LegionPassives.lua` returns `{ additions = { [1] =
//! { dn, id, sd = {...}, stats = {...} }, ... } }`. The per-seed LUT (#227)
//! resolves to a *global id* that indexes this `additions` list; this
//! extractor flattens it into `data/legion_passives.json` (a `Vec`, global id
//! `N` → element `N - 1`) carrying the name + `sd[]` stat lines that
//! `mod_parser` consumes when a timeless jewel replaces an in-radius node.
//!
//! Keystone replacement (#219 / #30) uses the separate `timeless_jewels.json`
//! path; this is the notable/small-node pool (#227).

use std::path::Path;

use anyhow::{anyhow, Context, Result};
use mlua::Value;
use pob_data::LegionPassive;
use serde_json::Value as J;

use crate::{lua_to_json, make_lua};

/// Load + evaluate `LegionPassives.lua` and flatten its `additions` table.
pub fn extract(pob_root: &Path) -> Result<Vec<LegionPassive>> {
    let path = pob_root.join("src/Data/TimelessJewelData/LegionPassives.lua");
    let src =
        std::fs::read_to_string(&path).with_context(|| format!("reading {}", path.display()))?;
    let lua = make_lua()?;
    let value: Value = lua
        .load(&src)
        .set_name(path.to_string_lossy().as_ref())
        .eval()
        .with_context(|| format!("evaluating {}", path.display()))?;
    let json = lua_to_json(value)?;
    parse_legion_passives(&json)
}

/// Pure flattening of the `lua_to_json` output. `additions` is a 1-based Lua
/// array; `lua_to_json` renders a contiguous one as a JSON array and a
/// non-contiguous one as an object keyed by stringified index — handle both so
/// the global-id ordering is preserved either way.
pub fn parse_legion_passives(json: &J) -> Result<Vec<LegionPassive>> {
    let additions = json
        .get("additions")
        .ok_or_else(|| anyhow!("LegionPassives.lua: missing `additions` table"))?;

    let ordered: Vec<&J> = match additions {
        J::Array(a) => a.iter().collect(),
        J::Object(map) => {
            // Sort by numeric key so global id N lands at position N-1.
            let mut entries: Vec<(u64, &J)> = map
                .iter()
                .filter_map(|(k, v)| k.parse::<u64>().ok().map(|n| (n, v)))
                .collect();
            entries.sort_by_key(|(n, _)| *n);
            entries.into_iter().map(|(_, v)| v).collect()
        }
        _ => return Err(anyhow!("`additions` is neither an array nor an object")),
    };

    let mut out = Vec::with_capacity(ordered.len());
    for (i, raw) in ordered.iter().enumerate() {
        let name = raw
            .get("dn")
            .and_then(J::as_str)
            .unwrap_or_default()
            .to_owned();
        let id = raw
            .get("id")
            .and_then(J::as_str)
            .unwrap_or_default()
            .to_owned();
        let stats = raw
            .get("sd")
            .and_then(J::as_array)
            .map(|a| a.iter().filter_map(J::as_str).map(str::to_owned).collect())
            .unwrap_or_default();
        if name.is_empty() && id.is_empty() {
            return Err(anyhow!("addition #{} has neither `dn` nor `id`", i + 1));
        }
        out.push(LegionPassive { name, id, stats });
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn flattens_additions_preserving_global_id_order() {
        let v = json!({
            "additions": {
                "1": { "dn": "Fire Damage", "id": "vaal_small_fire_damage",
                       "sd": ["(7-12)% increased Fire Damage"] },
                "2": { "dn": "Doryani", "id": "doryani",
                       "sd": ["Corrupted Soul", "+1 to maximum Power Charge"] },
            }
        });
        let out = parse_legion_passives(&v).expect("parse");
        assert_eq!(out.len(), 2);
        // Global id 1 → index 0.
        assert_eq!(out[0].name, "Fire Damage");
        assert_eq!(out[0].id, "vaal_small_fire_damage");
        assert_eq!(out[0].stats, ["(7-12)% increased Fire Damage"]);
        assert_eq!(out[1].name, "Doryani");
        assert_eq!(out[1].stats.len(), 2);
    }

    #[test]
    fn handles_array_rendering_too() {
        let v = json!({
            "additions": [
                { "dn": "A", "id": "a", "sd": ["x"] },
                { "dn": "B", "id": "b", "sd": [] },
            ]
        });
        let out = parse_legion_passives(&v).expect("parse");
        assert_eq!(out.len(), 2);
        assert_eq!(out[1].id, "b");
        assert!(out[1].stats.is_empty());
    }

    #[test]
    fn missing_additions_is_an_error() {
        assert!(parse_legion_passives(&json!({})).is_err());
    }

    /// Real-data check against the upstream PoB checkout (self-skips in CI,
    /// where `.PathOfBuilding/` isn't present). Verifies the Lua actually
    /// evaluates and the first addition matches the file head.
    #[test]
    fn extracts_real_legion_passives_when_pob_present() {
        let pob = std::path::Path::new("../../.PathOfBuilding");
        if !pob
            .join("src/Data/TimelessJewelData/LegionPassives.lua")
            .exists()
        {
            return;
        }
        let out = extract(pob).expect("extract real LegionPassives");
        assert!(
            out.len() >= 50,
            "expected the additions pool, got {}",
            out.len()
        );
        // The file head shows global id 1 = the Vaal small "Fire Damage".
        assert_eq!(out[0].name, "Fire Damage");
        assert_eq!(out[0].id, "vaal_small_fire_damage");
        assert!(out[0].stats.iter().any(|s| s.contains("Fire Damage")));
    }
}
