//! Extract PoB's timeless `NodeIndexMapping.lua` → `data/timeless_node_index.json`.
//!
//! The file builds a global `nodeIDList` table and `return`s it:
//! * `nodeIDList[nodeID] = { index, size }` — the node's slot in the per-jewel
//!   binary LUT (`index` = 0-based position, `size` = byte length of its seed
//!   data).
//! * `nodeIDList["size"]` / `["sizeNotable"]` — total node count + the
//!   notable/small cutoff.
//! * `nodeIDList["localIdToGlobalId"][jewelType][localId] = globalId` — the
//!   per-jewel remap applied to every replacement id (PoB's
//!   `convertLocalIdToGlobalId`).
//!
//! Together with the additions pool (`timeless_legion`) this is the static
//! metadata the #227 `readLUT` decode needs; the bulk seed bytes live in the
//! `*.zip` LUTs decoded at compute time.

use std::path::Path;

use anyhow::{anyhow, Context, Result};
use mlua::Value;
use pob_data::{TimelessNodeIndex, TimelessNodeSlot};
use serde_json::Value as J;

use crate::{lua_to_json, make_lua};

/// Load + evaluate `NodeIndexMapping.lua` and flatten it.
pub fn extract(pob_root: &Path) -> Result<TimelessNodeIndex> {
    let path = pob_root.join("src/Data/TimelessJewelData/NodeIndexMapping.lua");
    let src =
        std::fs::read_to_string(&path).with_context(|| format!("reading {}", path.display()))?;
    let lua = make_lua()?;
    let value: Value = lua
        .load(&src)
        .set_name(path.to_string_lossy().as_ref())
        .eval()
        .with_context(|| format!("evaluating {}", path.display()))?;
    let json = lua_to_json(value)?;
    parse_node_index(&json)
}

/// Pure flattening of the `lua_to_json` output (an object: numeric node keys +
/// the `size` / `sizeNotable` / `localIdToGlobalId` string keys).
pub fn parse_node_index(json: &J) -> Result<TimelessNodeIndex> {
    let obj = json
        .as_object()
        .ok_or_else(|| anyhow!("NodeIndexMapping did not evaluate to a table"))?;

    let node_count = obj.get("size").and_then(J::as_u64).unwrap_or(0) as u32;
    let notable_count = obj.get("sizeNotable").and_then(J::as_u64).unwrap_or(0) as u32;

    let mut nodes: Vec<TimelessNodeSlot> = obj
        .iter()
        .filter_map(|(k, v)| {
            let node_id = k.parse::<u32>().ok()?;
            let index = v.get("index").and_then(J::as_u64)? as u32;
            let size = v.get("size").and_then(J::as_u64).unwrap_or(0) as u32;
            Some(TimelessNodeSlot {
                node_id,
                index,
                size,
            })
        })
        .collect();
    // Deterministic ascending-index order (the blob is laid out by index).
    nodes.sort_by_key(|s| s.index);

    // localIdToGlobalId[jewelType][localId] = globalId. The 1-based jewel-type
    // table is contiguous, so `lua_to_json` may render the outer level as a
    // JSON array (index i → jewel type i+1) or, if sparse, as an object keyed
    // by stringified type. Each inner table has a `["size"]` plus 0-based ids,
    // so it always renders as an object. Flatten each inner into a dense Vec
    // indexed by localId.
    let parse_inner = |table: &J| -> Vec<u32> {
        let Some(table) = table.as_object() else {
            return Vec::new();
        };
        let pairs: Vec<(usize, u32)> = table
            .iter()
            .filter_map(|(k, v)| Some((k.parse::<usize>().ok()?, v.as_u64()? as u32)))
            .collect();
        let len = pairs.iter().map(|(id, _)| id + 1).max().unwrap_or(0);
        let mut dense = vec![0u32; len];
        for (id, global) in pairs {
            dense[id] = global;
        }
        dense
    };
    let local_to_global: Vec<Vec<u32>> = match obj.get("localIdToGlobalId") {
        Some(J::Array(types)) => types.iter().map(&parse_inner).collect(),
        Some(J::Object(types)) => {
            let max_type = types
                .keys()
                .filter_map(|k| k.parse::<usize>().ok())
                .max()
                .unwrap_or(0);
            let mut out = vec![Vec::new(); max_type];
            for (type_key, table) in types {
                if let Ok(jt) = type_key.parse::<usize>() {
                    if jt >= 1 {
                        out[jt - 1] = parse_inner(table);
                    }
                }
            }
            out
        }
        _ => Vec::new(),
    };

    Ok(TimelessNodeIndex {
        node_count,
        notable_count,
        nodes,
        local_to_global,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn parses_nodes_counts_and_remap() {
        let v = json!({
            "size": 3,
            "sizeNotable": 1,
            "6": { "index": 0, "size": 28419 },
            "529": { "index": 1, "size": 28520 },
            "544": { "index": 2, "size": 28401 },
            "localIdToGlobalId": {
                "1": { "size": 3, "0": 10, "1": 11, "2": 12 },
                "2": { "size": 2, "0": 5, "1": 6 }
            }
        });
        let out = parse_node_index(&v).expect("parse");
        assert_eq!(out.node_count, 3);
        assert_eq!(out.notable_count, 1);
        assert_eq!(out.nodes.len(), 3);
        // Ordered by index; slot() finds by node id.
        assert_eq!(out.nodes[0].node_id, 6);
        assert_eq!(out.slot(529).map(|s| s.index), Some(1));
        assert_eq!(out.slot(544).map(|s| s.size), Some(28401));
        // Remap is 1-based by jewel type; falls back to identity when absent.
        assert_eq!(out.to_global_id(1, 2), 12);
        assert_eq!(out.to_global_id(2, 1), 6);
        assert_eq!(out.to_global_id(3, 7), 7); // no table → identity
        assert_eq!(out.to_global_id(1, 99), 99); // out of range → identity
    }

    #[test]
    fn non_table_is_an_error() {
        assert!(parse_node_index(&json!(42)).is_err());
    }

    /// Real-data check (self-skips in CI).
    #[test]
    fn extracts_real_node_index_when_pob_present() {
        let pob = std::path::Path::new("../../.PathOfBuilding");
        if !pob
            .join("src/Data/TimelessJewelData/NodeIndexMapping.lua")
            .exists()
        {
            return;
        }
        let out = extract(pob).expect("extract real NodeIndexMapping");
        // The file header documents size=1931, sizeNotable=452.
        assert_eq!(out.node_count, 1931);
        assert_eq!(out.notable_count, 452);
        assert_eq!(out.nodes.len(), 1931);
        // Indices are dense 0..node_count after the sort.
        assert_eq!(out.nodes[0].index, 0);
        assert_eq!(out.nodes.last().unwrap().index, 1930);
        // Glorious Vanity (type 1) has a localIdToGlobalId table.
        assert!(!out.local_to_global[0].is_empty());
    }
}
