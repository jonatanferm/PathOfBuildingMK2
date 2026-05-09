//! Extract `Data/TattooPassives.lua` into the typed [`pob_data::TattooSet`].
//!
//! Foundation slice for [#98](https://github.com/jonatanferm/PathOfBuildingMK2/issues/98).
//! The upstream file returns `{ groups = {...}, nodes = {[name] = {...}} }`. We only
//! extract the `nodes` map — the `groups` block is a placeholder for tattoo-tree
//! geometry that PoB doesn't actually use anywhere meaningful.

use std::path::Path;

use anyhow::{anyhow, Context, Result};
use indexmap::IndexMap;
use pob_data::{Tattoo, TattooSet};
use serde_json::Value as J;

use crate::{load_lua_file_returning, make_lua};

pub fn extract(pob_root: &Path) -> Result<TattooSet> {
    let path = pob_root.join("src/Data/TattooPassives.lua");
    let lua = make_lua()?;
    let json = load_lua_file_returning(&lua, &path)
        .with_context(|| format!("evaluating {}", path.display()))?;
    let obj = json
        .as_object()
        .ok_or_else(|| anyhow!("TattooPassives.lua did not return a table"))?;

    let nodes_raw = obj
        .get("nodes")
        .and_then(J::as_object)
        .ok_or_else(|| anyhow!("TattooPassives.lua: missing `nodes` table"))?;

    let mut nodes = IndexMap::with_capacity(nodes_raw.len());
    for (name, raw) in nodes_raw {
        let mut tattoo = parse_tattoo(raw).with_context(|| format!("parsing tattoo `{name}`"))?;
        // The Lua key is the canonical display name; copy it onto the value if upstream
        // omits the redundant `dn` field.
        if tattoo.display_name.is_empty() {
            tattoo.display_name = name.clone();
        }
        nodes.insert(name.clone(), tattoo);
    }
    Ok(TattooSet { nodes })
}

fn parse_tattoo(v: &J) -> Result<Tattoo> {
    let obj = v
        .as_object()
        .ok_or_else(|| anyhow!("tattoo entry is not a table"))?;
    let display_name = obj.get("dn").and_then(J::as_str).unwrap_or("").to_owned();
    let id = obj.get("id").and_then(J::as_str).unwrap_or("").to_owned();
    let icon = obj.get("icon").and_then(J::as_str).unwrap_or("").to_owned();
    let active_effect_image = obj
        .get("activeEffectImage")
        .and_then(J::as_str)
        .unwrap_or("")
        .to_owned();
    let target_type = obj
        .get("targetType")
        .and_then(J::as_str)
        .unwrap_or("")
        .to_owned();
    let target_value = obj
        .get("targetValue")
        .and_then(J::as_str)
        .unwrap_or("")
        .to_owned();
    let override_type = obj
        .get("overrideType")
        .and_then(J::as_str)
        .unwrap_or("")
        .to_owned();
    let is_keystone = obj.get("ks").and_then(J::as_bool).unwrap_or(false);
    let is_notable = obj.get("not").and_then(J::as_bool).unwrap_or(false);
    let is_mastery = obj.get("m").and_then(J::as_bool).unwrap_or(false);
    let min_connected = obj.get("MinimumConnected").and_then(J::as_u64).unwrap_or(0) as u32;
    let max_connected = obj.get("MaximumConnected").and_then(J::as_u64).unwrap_or(0) as u32;
    let stat_lines = collect_stat_lines(obj.get("sd"));

    Ok(Tattoo {
        display_name,
        id,
        icon,
        active_effect_image,
        stat_lines,
        target_type,
        target_value,
        override_type,
        is_keystone,
        is_notable,
        is_mastery,
        min_connected,
        max_connected,
    })
}

/// `sd` is a Lua positional table: `{ [1] = "...", [2] = "...", ... }`. lua_to_json
/// renders it as a JSON object with stringified integer keys (or sometimes as a JSON
/// array if the keys are dense). Walk both shapes deterministically.
fn collect_stat_lines(v: Option<&J>) -> Vec<String> {
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

#[cfg(test)]
mod tests {
    use super::*;

    fn pob_root() -> Option<std::path::PathBuf> {
        let here = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
        for ancestor in here.ancestors() {
            let candidate = ancestor.join(".PathOfBuilding");
            if candidate.join("src/Data/TattooPassives.lua").exists() {
                return Some(candidate);
            }
        }
        None
    }

    #[test]
    fn extracts_canonical_keystone_tattoos() {
        let Some(root) = pob_root() else {
            eprintln!("skipping: .PathOfBuilding checkout not found next to workspace");
            return;
        };
        let set = extract(&root).expect("extracts cleanly");
        assert!(
            set.nodes.len() >= 100,
            "expected 100+ tattoos, got {}",
            set.nodes.len()
        );

        // Acrobatics is a well-known keystone tattoo present across leagues.
        let acro = set
            .nodes
            .get("Acrobatics")
            .expect("missing canonical `Acrobatics` keystone tattoo");
        assert!(acro.is_keystone, "Acrobatics should be flagged keystone");
        assert!(!acro.is_notable);
        assert!(!acro.is_mastery);
        assert_eq!(acro.target_type, "Keystone");
        assert_eq!(acro.override_type, "KeystoneTattoo");
        assert!(!acro.stat_lines.is_empty(), "Acrobatics has no stat lines");
        assert!(!acro.id.is_empty());
        assert!(
            acro.stat_lines
                .iter()
                .any(|s| s.contains("Suppress Spell Damage")),
            "Acrobatics tattoo must mention spell suppression; got {:?}",
            acro.stat_lines
        );

        // Sanity: every tattoo has a non-empty display name and a target type.
        for (key, t) in &set.nodes {
            assert!(
                !t.display_name.is_empty(),
                "tattoo `{key}` has empty display_name"
            );
            assert!(
                !t.target_type.is_empty(),
                "tattoo `{key}` has empty target_type"
            );
            // Exactly one of ks / not / m flags is the kind of node this tattoo
            // replaces. Not all tattoos set those bools though — some use
            // overrideType only. Allow zero matches but never two.
            let flag_count =
                u8::from(t.is_keystone) + u8::from(t.is_notable) + u8::from(t.is_mastery);
            assert!(
                flag_count <= 1,
                "tattoo `{key}` has multiple kind flags set"
            );
        }
    }
}
