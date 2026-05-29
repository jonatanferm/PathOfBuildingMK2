//! Extract PoB's timeless replacement-passive pools.
//!
//! `Data/TimelessJewelData/LegionPassives.lua` returns `{ additions = {...},
//! nodes = {...} }`. Both are 1-based arrays of `{ dn, id, sd = {...},
//! sortedStats = {...}, stats = {...} }`:
//!
//! * `additions` (96 entries) → `data/legion_passives.json` — the small-node
//!   stat-injection pool (`legionAdditions`); a per-seed LUT global id `g <
//!   timelessJewelAdditions` selects `additions[g]`.
//! * `nodes` (the `legionNodes` replacement-notable pool) →
//!   `data/legion_nodes.json`; global id `g >= timelessJewelAdditions` selects
//!   `nodes[g - timelessJewelAdditions]` (and the GV multiple-choice branch
//!   hard-codes `nodes[77]`/`nodes[78]` for Might/Legacy of the Vaal).
//!
//! Each entry carries the `sd[]` stat lines plus the `sortedStats` + `stats`
//! roll metadata (`{ index, min, max, fmt }`) PoB's `replaceHelperFunc` uses to
//! substitute Glorious Vanity roll values into `(min-max)` ranges (#227).
//!
//! Keystone replacement (#219 / #30) uses the separate `timeless_jewels.json`
//! path; this is the notable/small-node pool (#227).

use std::path::Path;

use anyhow::{anyhow, Context, Result};
use mlua::Value;
use pob_data::{LegionPassive, LegionStat};
use serde_json::Value as J;

use crate::{lua_to_json, make_lua};

/// Load + evaluate `LegionPassives.lua` and flatten both pools:
/// `(additions, nodes)`.
pub fn extract(pob_root: &Path) -> Result<(Vec<LegionPassive>, Vec<LegionPassive>)> {
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
    let additions = parse_pool(&json, "additions")?;
    let nodes = parse_pool(&json, "nodes")?;
    Ok((additions, nodes))
}

/// Backwards-compatible alias: flatten just the `additions` pool.
pub fn parse_legion_passives(json: &J) -> Result<Vec<LegionPassive>> {
    parse_pool(json, "additions")
}

/// Pure flattening of one pool (`additions` or `nodes`) from the `lua_to_json`
/// output. Each is a 1-based Lua array; `lua_to_json` renders a contiguous one
/// as a JSON array and a non-contiguous one as an object keyed by stringified
/// index — handle both so the global-id ordering is preserved either way.
pub fn parse_pool(json: &J, pool: &str) -> Result<Vec<LegionPassive>> {
    let table = json
        .get(pool)
        .ok_or_else(|| anyhow!("LegionPassives.lua: missing `{pool}` table"))?;

    let ordered: Vec<&J> = match table {
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
        _ => return Err(anyhow!("`{pool}` is neither an array nor an object")),
    };

    let mut out = Vec::with_capacity(ordered.len());
    for (i, raw) in ordered.iter().enumerate() {
        let passive = parse_passive(raw)
            .with_context(|| format!("{pool} #{}", i + 1))?
            .ok_or_else(|| anyhow!("{pool} #{} has neither `dn` nor `id`", i + 1))?;
        out.push(passive);
    }
    Ok(out)
}

/// Parse one pool entry. Returns `Ok(None)` for an entry with neither `dn` nor
/// `id` (so the caller can reject it with a positional error).
fn parse_passive(raw: &J) -> Result<Option<LegionPassive>> {
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
    if name.is_empty() && id.is_empty() {
        return Ok(None);
    }
    let stats = raw
        .get("sd")
        .and_then(J::as_array)
        .map(|a| a.iter().filter_map(J::as_str).map(str::to_owned).collect())
        .unwrap_or_default();
    let sorted_stats = raw
        .get("sortedStats")
        .and_then(J::as_array)
        .map(|a| a.iter().filter_map(J::as_str).map(str::to_owned).collect())
        .unwrap_or_default();
    // `stats` is a map of stat-key → { index, min, max, fmt }. Flatten to a
    // Vec<LegionStat>, sorted by `index` for deterministic output.
    let mut rolls: Vec<LegionStat> = raw
        .get("stats")
        .and_then(J::as_object)
        .map(|m| {
            m.iter()
                .map(|(key, v)| LegionStat {
                    key: key.clone(),
                    index: v.get("index").and_then(J::as_u64).unwrap_or(0) as u32,
                    min: v.get("min").and_then(J::as_f64).unwrap_or(0.0),
                    max: v.get("max").and_then(J::as_f64).unwrap_or(0.0),
                    fmt: v.get("fmt").and_then(J::as_str).unwrap_or("d").to_owned(),
                })
                .collect()
        })
        .unwrap_or_default();
    rolls.sort_by(|a, b| a.index.cmp(&b.index).then_with(|| a.key.cmp(&b.key)));
    Ok(Some(LegionPassive {
        name,
        id,
        stats,
        sorted_stats,
        rolls,
    }))
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

    #[test]
    fn parses_roll_metadata_and_sorted_stats() {
        let v = json!({
            "additions": {
                "1": {
                    "dn": "Fire Damage", "id": "vaal_small_fire_damage",
                    "sd": ["(7-12)% increased Fire Damage"],
                    "sortedStats": ["fire_damage_+%"],
                    "stats": {
                        "fire_damage_+%": { "fmt": "d", "index": 1, "min": 7, "max": 12 }
                    }
                }
            }
        });
        let out = parse_legion_passives(&v).expect("parse");
        assert_eq!(out[0].sorted_stats, ["fire_damage_+%"]);
        assert_eq!(out[0].rolls.len(), 1);
        let r = &out[0].rolls[0];
        assert_eq!(r.key, "fire_damage_+%");
        assert_eq!(r.index, 1);
        assert_eq!(r.min, 7.0);
        assert_eq!(r.max, 12.0);
        assert_eq!(r.fmt, "d");
    }

    #[test]
    fn parse_pool_reads_nodes_table() {
        let v = json!({
            "additions": [],
            "nodes": {
                "1": { "dn": "Divine Flesh", "id": "vaal_keystone_1",
                       "sd": ["All Damage taken bypasses Energy Shield"],
                       "sortedStats": ["keystone_divine_flesh"],
                       "stats": { "keystone_divine_flesh": { "fmt": "d", "index": 1, "min": 1, "max": 1 } } },
                "77": { "dn": "Might of the Vaal", "id": "might_of_the_vaal", "sd": ["x"] }
            }
        });
        let nodes = parse_pool(&v, "nodes").expect("parse nodes");
        // 1-based key 77 → element 76; sparse object fills gaps? No — we sort by
        // numeric key and pack densely. Two keys → two entries, ordered 1, 77.
        assert_eq!(nodes.len(), 2);
        assert_eq!(nodes[0].name, "Divine Flesh");
        assert_eq!(nodes[1].name, "Might of the Vaal");
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
        let (additions, nodes) = extract(pob).expect("extract real LegionPassives");
        // The additions pool is exactly 96 (= data.timelessJewelAdditions).
        assert_eq!(additions.len(), 96, "additions pool size");
        // The file head shows global id 1 = the Vaal small "Fire Damage".
        assert_eq!(additions[0].name, "Fire Damage");
        assert_eq!(additions[0].id, "vaal_small_fire_damage");
        assert!(additions[0].stats.iter().any(|s| s.contains("Fire Damage")));
        // Roll metadata is captured.
        assert!(!additions[0].rolls.is_empty());
        assert_eq!(additions[0].rolls[0].min, 7.0);
        assert_eq!(additions[0].rolls[0].max, 12.0);
        // The nodes pool (legionNodes): first entry is Divine Flesh keystone;
        // index 76 (legionNodes[77], 1-based) is Might of the Vaal — the GV
        // multiple-choice base that the engine resolves via `node_1based(77)`.
        assert!(nodes.len() >= 78, "nodes pool size: {}", nodes.len());
        assert_eq!(nodes[0].name, "Divine Flesh");
        assert!(
            nodes[76].name.contains("Might"),
            "legionNodes[77] should be Might of the Vaal, got {:?}",
            nodes[76].name
        );
        assert!(
            nodes[77].name.contains("Legacy"),
            "legionNodes[78] should be Legacy of the Vaal, got {:?}",
            nodes[77].name
        );
    }
}
