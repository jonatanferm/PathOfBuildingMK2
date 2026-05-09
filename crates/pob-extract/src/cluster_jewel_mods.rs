//! Extract `Data/ModJewelCluster.lua` into the typed [`pob_data::ClusterModSet`].
//!
//! Slice 2 of [#21](https://github.com/jonatanferm/PathOfBuildingMK2/issues/21). Each
//! entry is a Lua mixed table — named keys (`type`, `affix`, `statOrder`, `level`,
//! `group`, `weightKey`, `weightVal`, …) plus a positional sub-table that carries the
//! stat description text. We pick out the named fields and gather the positional entries
//! as `stat_lines`.

use std::path::Path;

use anyhow::{anyhow, Context, Result};
use pob_data::{ClusterMod, ClusterModSet};
use serde_json::Value as J;

use crate::{load_lua_file_returning, make_lua};

pub fn extract(pob_root: &Path) -> Result<ClusterModSet> {
    let path = pob_root.join("src/Data/ModJewelCluster.lua");
    let lua = make_lua()?;
    let json = load_lua_file_returning(&lua, &path)
        .with_context(|| format!("evaluating {}", path.display()))?;
    let obj = json
        .as_object()
        .ok_or_else(|| anyhow!("ModJewelCluster.lua did not return a table"))?;

    let mut out = ClusterModSet::with_capacity(obj.len());
    for (mod_id, raw) in obj {
        let parsed =
            parse_cluster_mod(raw).with_context(|| format!("parsing cluster mod `{mod_id}`"))?;
        out.insert(mod_id.clone(), parsed);
    }
    Ok(out)
}

fn parse_cluster_mod(v: &J) -> Result<ClusterMod> {
    let obj = v
        .as_object()
        .ok_or_else(|| anyhow!("cluster mod entry is not a table"))?;

    let mod_type = obj.get("type").and_then(J::as_str).unwrap_or("").to_owned();
    let affix = obj
        .get("affix")
        .and_then(J::as_str)
        .unwrap_or("")
        .to_owned();
    let level = obj.get("level").and_then(J::as_u64).unwrap_or(0) as u32;
    let group = obj
        .get("group")
        .and_then(J::as_str)
        .unwrap_or("")
        .to_owned();
    let stat_order = u32_array(obj.get("statOrder"));
    let weight_keys = string_array(obj.get("weightKey"));
    let weight_values = i32_array(obj.get("weightVal"));
    let weight_multiplier_keys = string_array(obj.get("weightMultiplierKey"));
    let weight_multiplier_values = i32_array(obj.get("weightMultiplierVal"));
    let tags = string_array(obj.get("tags"));
    let mod_tags = string_array(obj.get("modTags"));

    // Stat lines come in via positional integer keys (1, 2, ...). lua_to_json renders
    // those as stringified-integer keys on the same object; collect in numeric order.
    let mut indexed_lines: Vec<(u64, String)> = obj
        .iter()
        .filter_map(|(k, v)| {
            let n = k.parse::<u64>().ok()?;
            let s = v.as_str()?.to_owned();
            Some((n, s))
        })
        .collect();
    indexed_lines.sort_by_key(|(k, _)| *k);
    let stat_lines: Vec<String> = indexed_lines.into_iter().map(|(_, s)| s).collect();

    Ok(ClusterMod {
        mod_type,
        affix,
        stat_lines,
        stat_order,
        level,
        group,
        weight_keys,
        weight_values,
        weight_multiplier_keys,
        weight_multiplier_values,
        tags,
        mod_tags,
    })
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

fn u32_array(v: Option<&J>) -> Vec<u32> {
    let Some(v) = v else { return Vec::new() };
    if let Some(arr) = v.as_array() {
        return arr
            .iter()
            .filter_map(|n| n.as_u64().map(|n| n as u32))
            .collect();
    }
    if let Some(o) = v.as_object() {
        let mut indexed: Vec<(u64, u32)> = o
            .iter()
            .filter_map(|(k, v)| {
                let n = k.parse::<u64>().ok()?;
                let val = v.as_u64()?;
                Some((n, val as u32))
            })
            .collect();
        indexed.sort_by_key(|(k, _)| *k);
        return indexed.into_iter().map(|(_, v)| v).collect();
    }
    Vec::new()
}

fn i32_array(v: Option<&J>) -> Vec<i32> {
    let Some(v) = v else { return Vec::new() };
    if let Some(arr) = v.as_array() {
        return arr
            .iter()
            .filter_map(|n| n.as_i64().map(|n| n as i32))
            .collect();
    }
    if let Some(o) = v.as_object() {
        let mut indexed: Vec<(u64, i32)> = o
            .iter()
            .filter_map(|(k, v)| {
                let n = k.parse::<u64>().ok()?;
                let val = v.as_i64()?;
                Some((n, val as i32))
            })
            .collect();
        indexed.sort_by_key(|(k, _)| *k);
        return indexed.into_iter().map(|(_, v)| v).collect();
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
            if candidate.join("src/Data/ModJewelCluster.lua").exists() {
                return Some(candidate);
            }
        }
        None
    }

    #[test]
    fn extracts_canonical_cluster_mods() {
        let Some(root) = pob_root() else {
            eprintln!("skipping: .PathOfBuilding checkout not found next to workspace");
            return;
        };
        let set = extract(&root).expect("extracts cleanly");
        assert!(
            set.len() >= 400,
            "expected 400+ cluster mods, got {}",
            set.len()
        );

        // Spot-check a well-known corruption mod.
        let chaos = set
            .get("ChaosResistJewelCorrupted")
            .expect("missing canonical ChaosResistJewelCorrupted");
        assert_eq!(chaos.mod_type, "Corrupted");
        assert!(chaos
            .stat_lines
            .iter()
            .any(|s| s.contains("Chaos Resistance")));
        assert!(chaos.mod_tags.contains(&"chaos".to_owned()));
        assert_eq!(chaos.group, "ChaosResistance");

        // Spot-check a notable-grant prefix from the Affliction expansion.
        let prodigious = set
            .iter()
            .find(|(k, _)| k.starts_with("AfflictionNotableProdigiousDefense"))
            .map(|(_, v)| v)
            .expect("missing canonical AfflictionNotableProdigiousDefense* prefix");
        assert_eq!(prodigious.mod_type, "Prefix");
        assert_eq!(prodigious.affix, "Notable");
        assert!(!prodigious.weight_multiplier_keys.is_empty());
        assert_eq!(
            prodigious.weight_multiplier_keys.len(),
            prodigious.weight_multiplier_values.len(),
            "multiplier key/value arrays must be parallel"
        );

        // Sanity: every mod has a recognised type.
        for (id, m) in &set {
            assert!(
                matches!(m.mod_type.as_str(), "Corrupted" | "Prefix" | "Suffix"),
                "mod `{id}` has unexpected type `{}`",
                m.mod_type
            );
            assert_eq!(
                m.weight_keys.len(),
                m.weight_values.len(),
                "mod `{id}`: weight_keys / weight_values length mismatch"
            );
        }
    }
}
