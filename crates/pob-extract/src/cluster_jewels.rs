//! Extract `Data/ClusterJewels.lua` into the typed [`pob_data::ClusterJewelData`].
//!
//! Slice 1 of [#21](https://github.com/jonatanferm/PathOfBuildingMK2/issues/21). The
//! upstream file is a single `return { jewels = {...}, notableSortOrder = {...},
//! keystones = {...}, orbitOffsets = {...} }` table; we walk it directly with the
//! generic Lua → JSON converter and re-shape into typed structs.

use std::path::Path;

use anyhow::{anyhow, Context, Result};
use indexmap::IndexMap;
use pob_data::{ClusterJewelData, ClusterJewelType, ClusterSkill};
use serde_json::Value as J;

use crate::{load_lua_file_returning, make_lua};

pub fn extract(pob_root: &Path) -> Result<ClusterJewelData> {
    let path = pob_root.join("src/Data/ClusterJewels.lua");
    let lua = make_lua()?;
    let json = load_lua_file_returning(&lua, &path)
        .with_context(|| format!("evaluating {}", path.display()))?;
    let obj = json
        .as_object()
        .ok_or_else(|| anyhow!("ClusterJewels.lua did not return a table"))?;

    let jewels_raw = obj
        .get("jewels")
        .and_then(J::as_object)
        .ok_or_else(|| anyhow!("ClusterJewels.lua: missing `jewels` table"))?;
    let mut jewels = IndexMap::with_capacity(jewels_raw.len());
    for (name, raw) in jewels_raw {
        jewels.insert(
            name.clone(),
            parse_jewel_type(raw).with_context(|| format!("parsing jewel `{name}`"))?,
        );
    }

    let notable_sort_order = obj
        .get("notableSortOrder")
        .and_then(J::as_object)
        .map(|m| {
            m.iter()
                .filter_map(|(k, v)| v.as_u64().map(|n| (k.clone(), n as u32)))
                .collect()
        })
        .unwrap_or_default();

    let keystones = obj
        .get("keystones")
        .and_then(J::as_array)
        .map(|arr| {
            arr.iter()
                .filter_map(|v| v.as_str().map(str::to_owned))
                .collect()
        })
        .unwrap_or_default();

    let orbit_offsets = obj
        .get("orbitOffsets")
        .and_then(J::as_object)
        .map(parse_orbit_offsets)
        .unwrap_or_default();

    Ok(ClusterJewelData {
        jewels,
        notable_sort_order,
        keystones,
        orbit_offsets,
    })
}

fn parse_jewel_type(v: &J) -> Result<ClusterJewelType> {
    let obj = v
        .as_object()
        .ok_or_else(|| anyhow!("jewel entry is not a table"))?;
    let size = obj
        .get("size")
        .and_then(J::as_str)
        .ok_or_else(|| anyhow!("missing `size`"))?
        .to_owned();
    let size_index = u8_field(obj, "sizeIndex")?;
    let min_nodes = u8_field(obj, "minNodes")?;
    let max_nodes = u8_field(obj, "maxNodes")?;
    let total_indicies = u8_field(obj, "totalIndicies")?;
    let small_indicies = u8_array_field(obj, "smallIndicies");
    let notable_indicies = u8_array_field(obj, "notableIndicies");
    let socket_indicies = u8_array_field(obj, "socketIndicies");

    let skills_raw = obj
        .get("skills")
        .and_then(J::as_object)
        .ok_or_else(|| anyhow!("missing `skills` table"))?;
    let mut skills = IndexMap::with_capacity(skills_raw.len());
    for (id, raw) in skills_raw {
        skills.insert(
            id.clone(),
            parse_skill(raw).with_context(|| format!("skill `{id}`"))?,
        );
    }

    Ok(ClusterJewelType {
        size,
        size_index,
        min_nodes,
        max_nodes,
        small_indicies,
        notable_indicies,
        socket_indicies,
        total_indicies,
        skills,
    })
}

fn parse_skill(v: &J) -> Result<ClusterSkill> {
    let obj = v
        .as_object()
        .ok_or_else(|| anyhow!("skill entry is not a table"))?;
    let name = obj
        .get("name")
        .and_then(J::as_str)
        .ok_or_else(|| anyhow!("skill missing `name`"))?
        .to_owned();
    let icon = obj.get("icon").and_then(J::as_str).unwrap_or("").to_owned();
    let tag = obj.get("tag").and_then(J::as_str).unwrap_or("").to_owned();
    let stats = string_array(obj.get("stats"));
    let enchant = string_array(obj.get("enchant"));
    Ok(ClusterSkill {
        name,
        icon,
        tag,
        stats,
        enchant,
    })
}

/// Lua `[u32] = { [u32] = u32 }` table → typed nested IndexMap. Both layers come through
/// `lua_to_json` as objects with stringified integer keys; we parse back to u32.
fn parse_orbit_offsets(obj: &serde_json::Map<String, J>) -> IndexMap<u32, IndexMap<u32, u32>> {
    let mut out: IndexMap<u32, IndexMap<u32, u32>> = IndexMap::with_capacity(obj.len());
    for (outer_key, outer_val) in obj {
        let Ok(node_id) = outer_key.parse::<u32>() else {
            continue;
        };
        let Some(inner) = outer_val.as_object() else {
            continue;
        };
        let mut inner_out = IndexMap::with_capacity(inner.len());
        for (k, v) in inner {
            if let (Ok(k), Some(n)) = (k.parse::<u32>(), v.as_u64()) {
                inner_out.insert(k, n as u32);
            }
        }
        out.insert(node_id, inner_out);
    }
    out
}

fn u8_field(obj: &serde_json::Map<String, J>, key: &str) -> Result<u8> {
    obj.get(key)
        .and_then(J::as_u64)
        .map(|n| n as u8)
        .ok_or_else(|| anyhow!("missing or non-integer `{key}`"))
}

fn u8_array_field(obj: &serde_json::Map<String, J>, key: &str) -> Vec<u8> {
    let Some(val) = obj.get(key) else {
        return Vec::new();
    };
    // Lua tables show up as either arrays (1..=n dense) or stringified-integer-keyed objects.
    if let Some(arr) = val.as_array() {
        return arr
            .iter()
            .filter_map(|v| v.as_u64().map(|n| n as u8))
            .collect();
    }
    if let Some(o) = val.as_object() {
        let mut indexed: Vec<(u64, u8)> = o
            .iter()
            .filter_map(|(k, v)| {
                let n = k.parse::<u64>().ok()?;
                let val = v.as_u64()?;
                Some((n, val as u8))
            })
            .collect();
        indexed.sort_by_key(|(k, _)| *k);
        return indexed.into_iter().map(|(_, v)| v).collect();
    }
    Vec::new()
}

fn string_array(v: Option<&J>) -> Vec<String> {
    let Some(v) = v else { return Vec::new() };
    if let Some(arr) = v.as_array() {
        return arr
            .iter()
            .filter_map(|s| s.as_str().map(str::to_owned))
            .collect();
    }
    if let Some(o) = v.as_object() {
        // 1..=n stringified-integer-keyed strings.
        let mut indexed: Vec<(u64, String)> = o
            .iter()
            .filter_map(|(k, v)| {
                let n = k.parse::<u64>().ok()?;
                let s = v.as_str()?.to_owned();
                Some((n, s))
            })
            .collect();
        indexed.sort_by_key(|(k, _)| *k);
        return indexed.into_iter().map(|(_, s)| s).collect();
    }
    Vec::new()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pob_root() -> Option<std::path::PathBuf> {
        let here = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
        for ancestor in here.ancestors() {
            let candidate = ancestor.join(".PathOfBuilding");
            if candidate.join("src/Data/ClusterJewels.lua").exists() {
                return Some(candidate);
            }
        }
        None
    }

    #[test]
    fn extracts_three_jewel_categories() {
        let Some(root) = pob_root() else {
            eprintln!("skipping: .PathOfBuilding checkout not found next to workspace");
            return;
        };
        let data = extract(&root).expect("extracts cleanly");
        // The three core categories must always be present.
        for expected in &[
            "Small Cluster Jewel",
            "Medium Cluster Jewel",
            "Large Cluster Jewel",
        ] {
            assert!(
                data.jewels.contains_key(*expected),
                "missing jewel category `{expected}` (got {:?})",
                data.jewels.keys().collect::<Vec<_>>()
            );
        }

        // Spot-check Small jewel structure.
        let small = data.jewels.get("Small Cluster Jewel").unwrap();
        assert_eq!(small.size, "Small");
        assert_eq!(small.size_index, 0);
        assert_eq!(small.total_indicies, 6);
        assert!(!small.skills.is_empty(), "Small has no skills");
        assert!(
            small.skills.contains_key("affliction_maximum_life"),
            "Small missing the canonical `affliction_maximum_life` skill"
        );
        let life = &small.skills["affliction_maximum_life"];
        assert_eq!(life.name, "Life");
        assert!(!life.stats.is_empty());
        assert!(!life.enchant.is_empty());

        // Sanity: notable sort order + keystone whitelist were extracted.
        assert!(!data.notable_sort_order.is_empty());
        assert!(data.keystones.contains(&"Disciple of Kitava".to_owned()));

        // Each jewel's index slots fall within the totalIndicies ring.
        for (name, j) in &data.jewels {
            for &i in &j.small_indicies {
                assert!(
                    i < j.total_indicies,
                    "{name}: smallIndicies entry {i} >= totalIndicies {}",
                    j.total_indicies
                );
            }
            for &i in &j.notable_indicies {
                assert!(
                    i < j.total_indicies,
                    "{name}: notableIndicies entry {i} >= totalIndicies {}",
                    j.total_indicies
                );
            }
        }
    }
}
