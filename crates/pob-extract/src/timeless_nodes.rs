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
pub(crate) mod tests {
    use super::*;

    /// `data.timelessJewelAdditions` from PoB's `Modules/Data.lua` (96 in
    /// 3.25-era data, 337 once Foulborn/Vestigial additions landed).
    pub(crate) fn legion_additions_count(pob: &std::path::Path) -> u32 {
        std::fs::read_to_string(pob.join("src/Modules/Data.lua"))
            .ok()
            .and_then(|src| {
                src.lines().find_map(|l| {
                    l.trim()
                        .strip_prefix("data.timelessJewelAdditions = ")
                        .and_then(|v| v.split_whitespace().next()?.parse().ok())
                })
            })
            .unwrap_or(96)
    }
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

    /// Definitive parity check (self-skips without the PoB checkout): run PoB's
    /// *actual* `readLUT` (from `DataLegionLookUpTableHelper.lua`) against its
    /// own Lethal Pride `.zip`, and assert it agrees with our Rust
    /// `flat_notable_replacement` decode for a sample of real notable nodes.
    ///
    /// This is the cross-check the prescribed `pob_diff` gate can't provide —
    /// that harness doesn't stub `Inflate`/`NewFileSearch`, so PoB's own LUT
    /// loading can't run there. Here we supply minimal `GetScriptPath` /
    /// `Inflate` / `NewFileSearch` shims (pointing at PoB's data dir) so its
    /// real decode executes byte-for-byte.
    #[test]
    fn readlut_matches_pob_lua_flat_and_glorious_vanity() {
        use mlua::{Function, Table, Value, Variadic};

        let pob = std::path::Path::new("../../.PathOfBuilding");
        let tjd = pob.join("src/Data/TimelessJewelData");
        let zip = tjd.join("LethalPride.zip");
        let helper = pob.join("src/Modules/DataLegionLookUpTableHelper.lua");
        if !zip.exists() || !tjd.join("NodeIndexMapping.lua").exists() || !helper.exists() {
            return;
        }

        // Rust side: our node index + the inflated Lethal Pride LUT.
        let node_index = extract(pob).expect("extract node index");
        let inflated = pob_engine::inflate_timeless_lut(&std::fs::read(&zip).expect("read zip"))
            .expect("inflate");

        // Lua side: minimal PoB runtime shims so readLUT's loadJewelFile runs.
        let lua = crate::make_lua().expect("lua");
        let g = lua.globals();
        let script_path = std::fs::canonicalize(pob.join("src"))
            .expect("canon")
            .to_string_lossy()
            .replace('\\', "/");
        let sp = script_path.clone();
        g.set(
            "GetScriptPath",
            lua.create_function(move |_, ()| Ok(sp.clone())).unwrap(),
        )
        .unwrap();
        g.set(
            "Inflate",
            lua.create_function(|lua, s: mlua::String| {
                let out = pob_engine::inflate_timeless_lut(&s.as_bytes())
                    .map_err(mlua::Error::external)?;
                lua.create_string(&out)
            })
            .unwrap(),
        )
        .unwrap();
        // NewFileSearch: resolve a plain file or a `prefix*` glob (the GV
        // `.zip.part*` probe) to the matching basenames, returning a stateful
        // handle (`files` + `idx`) whose `GetFileName`/`NextFile` iterate them;
        // nil when nothing matches.
        g.set(
            "NewFileSearch",
            lua.create_function(|lua, pattern: String| {
                let p = std::path::Path::new(&pattern);
                let mut files: Vec<String> = if let Some(prefix) = pattern.strip_suffix('*') {
                    let pp = std::path::Path::new(prefix);
                    let (dir, base) = (
                        pp.parent().unwrap_or_else(|| std::path::Path::new(".")),
                        pp.file_name().map(|n| n.to_string_lossy().to_string()),
                    );
                    match (base, std::fs::read_dir(dir)) {
                        (Some(base), Ok(rd)) => {
                            let mut v: Vec<String> = rd
                                .filter_map(|e| e.ok())
                                .filter_map(|e| e.file_name().to_str().map(str::to_owned))
                                .filter(|n| n.starts_with(&base))
                                .collect();
                            v.sort();
                            v
                        }
                        _ => Vec::new(),
                    }
                } else if p.exists() {
                    p.file_name()
                        .map(|n| vec![n.to_string_lossy().to_string()])
                        .unwrap_or_default()
                } else {
                    Vec::new()
                };
                if files.is_empty() {
                    return Ok(Value::Nil);
                }
                let handle = lua.create_table()?;
                let arr = lua.create_table()?;
                for (i, f) in files.drain(..).enumerate() {
                    arr.set(i + 1, f)?;
                }
                handle.set("files", arr)?;
                handle.set("idx", 1i64)?;
                handle.set(
                    "GetFileName",
                    lua.create_function(|_, this: Table| {
                        let idx: i64 = this.get("idx")?;
                        let files: Table = this.get("files")?;
                        files.get::<String>(idx)
                    })?,
                )?;
                handle.set(
                    "GetFileModifiedTime",
                    lua.create_function(|_, _: Variadic<Value>| Ok(0i64))?,
                )?;
                handle.set(
                    "NextFile",
                    lua.create_function(|_, this: Table| {
                        let idx: i64 = this.get("idx")?;
                        let files: Table = this.get("files")?;
                        let next = idx + 1;
                        this.set("idx", next)?;
                        Ok(next <= files.raw_len() as i64)
                    })?,
                )?;
                Ok(Value::Table(handle))
            })
            .unwrap(),
        )
        .unwrap();

        // Build the `data` table readLUT reads (PoB's Modules/Data.lua values).
        let data = lua.create_table().unwrap();
        let set_arr = |key: &str, vals: &[(i64, mlua::Value)]| {
            let t = lua.create_table().unwrap();
            for (i, v) in vals {
                t.set(*i, v.clone()).unwrap();
            }
            data.set(key, t).unwrap();
        };
        let s = |x: &str| Value::String(lua.create_string(x).unwrap());
        let n = |x: i64| Value::Integer(x);
        set_arr(
            "timelessJewelTypes",
            &[
                (1, s("Glorious Vanity")),
                (2, s("Lethal Pride")),
                (3, s("Brutal Restraint")),
                (4, s("Militant Faith")),
                (5, s("Elegant Hubris")),
                (6, s("Heroic Tragedy")),
            ],
        );
        set_arr(
            "timelessJewelSeedMin",
            &[
                (1, n(100)),
                (2, n(10000)),
                (3, n(500)),
                (4, n(2000)),
                (5, n(100)),
                (6, n(100)),
            ],
        );
        set_arr(
            "timelessJewelSeedMax",
            &[
                (1, n(8000)),
                (2, n(18000)),
                (3, n(8000)),
                (4, n(10000)),
                (5, n(8000)),
                (6, n(8000)),
            ],
        );
        data.set("timelessJewelAdditions", legion_additions_count(pob) as i64)
            .unwrap();
        data.set("timelessJewelLUTs", lua.create_table().unwrap())
            .unwrap();
        let nim_src = std::fs::read_to_string(tjd.join("NodeIndexMapping.lua")).unwrap();
        let node_id_list: Value = lua.load(&nim_src).eval().expect("eval NodeIndexMapping");
        data.set("nodeIDList", node_id_list).unwrap();
        g.set("data", data).unwrap();

        // Lua 5.4 rejects LuaJIT's lenient `"wb+"` mode (the `+` must precede
        // the `b`); loadJewelFile writes a `.bin` cache with it. Normalise.
        lua.load(
            r#"
            local raw_open = io.open
            io.open = function(path, mode)
                if mode then mode = mode:gsub("([rwa])b%+", "%1+b") end
                return raw_open(path, mode)
            end
            "#,
        )
        .exec()
        .unwrap();

        // Current PoB split the jewel-file loading out of the helper into
        // `Modules/DataJewelFileLoader.lua`, pulled in via `require`.
        let loader = pob.join("src/Modules/DataJewelFileLoader.lua");
        if loader.exists() {
            let loader_src = std::fs::read_to_string(&loader).unwrap();
            let loader_fn = lua
                .load(&loader_src)
                .set_name("DataJewelFileLoader")
                .into_function()
                .unwrap();
            let preload: Table = lua
                .globals()
                .get::<Table>("package")
                .unwrap()
                .get("preload")
                .unwrap();
            preload
                .set("Modules.DataJewelFileLoader", loader_fn)
                .unwrap();
        }

        // readLUT, repairLUTs = require("Modules/DataLegionLookUpTableHelper").
        let helper_src = std::fs::read_to_string(&helper).unwrap();
        // Older PoB returns `readLUT, repairLUTs`; current PoB returns a
        // `{ readLUT = …, repairLUTs = … }` table.
        let read_lut: Function = match lua
            .load(&helper_src)
            .eval::<mlua::MultiValue>()
            .expect("eval helper")
            .into_iter()
            .next()
        {
            Some(Value::Function(f)) => f,
            Some(Value::Table(t)) => t.get("readLUT").expect("readLUT"),
            other => panic!("unexpected helper return: {other:?}"),
        };

        let seed = 14000u32;
        let mut checked = 0;
        let mut matched = 0;
        for slot in node_index
            .nodes
            .iter()
            .filter(|s| s.index <= node_index.notable_count)
        {
            if checked >= 40 {
                break;
            }
            let res: Table = read_lut
                .call((seed, slot.node_id, 2u32))
                .expect("call readLUT");
            let pob = if res.raw_len() >= 1 {
                res.get::<u32>(1).ok()
            } else {
                None
            };
            let mine = node_index.flat_notable_replacement(&inflated, 2, slot.node_id, seed);
            if pob.is_none() && mine.is_none() {
                continue;
            }
            checked += 1;
            assert_eq!(
                pob, mine,
                "readLUT disagreement for node {} seed {seed}",
                slot.node_id
            );
            if pob == mine {
                matched += 1;
            }
        }
        assert!(
            matched >= 20,
            "expected ≥20 cross-checked notables, got {matched}"
        );

        // --- Glorious Vanity (jewel type 1) — the #227 acceptance case. The
        // variable-length GV entry: readLUT returns the full row (ids converted
        // in their positions, rolls raw); our `gv_lut_entry` must match it
        // element-for-element. Exercises the `.zip.part*` glob + the GV layout
        // walk against PoB's own decode.
        if tjd.join("GloriousVanity.zip.part0").exists() {
            let mut gv_compressed = Vec::new();
            for part in 0.. {
                match std::fs::read(tjd.join(format!("GloriousVanity.zip.part{part}"))) {
                    Ok(b) => gv_compressed.extend_from_slice(&b),
                    Err(_) => break,
                }
            }
            let gv_lut = pob_engine::inflate_timeless_lut(&gv_compressed).expect("inflate GV");
            let gv_seed = 6500u32; // the #227 acceptance seed
            let mut gv_checked = 0;
            let mut gv_matched = 0;
            for slot in node_index
                .nodes
                .iter()
                .filter(|s| s.index <= node_index.notable_count)
            {
                if gv_checked >= 40 {
                    break;
                }
                let res: Table = read_lut
                    .call((gv_seed, slot.node_id, 1u32))
                    .expect("call readLUT GV");
                let pob: Vec<u32> = (1..=res.raw_len())
                    .filter_map(|i| res.get::<u32>(i as i64).ok())
                    .collect();
                let mine = node_index.gv_lut_entry(&gv_lut, slot.node_id, gv_seed);
                if pob.is_empty() && mine.is_empty() {
                    continue;
                }
                gv_checked += 1;
                assert_eq!(
                    pob, mine,
                    "GV readLUT disagreement for node {} seed {gv_seed}",
                    slot.node_id
                );
                gv_matched += 1;
            }
            assert!(
                gv_matched >= 20,
                "expected ≥20 cross-checked GV notables, got {gv_matched}"
            );
        }

        // Clean up the .bin caches PoB's loadJewelFile writes next to the data.
        let _ = std::fs::remove_file(tjd.join("LethalPride.bin"));
        let _ = std::fs::remove_file(tjd.join("GloriousVanity.bin"));
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
        // The file header documents `size` / `sizeNotable` (1931 / 452 in
        // 3.25-era data; they grow with new tree nodes).
        let src = std::fs::read_to_string(
            pob.join("src/Data/TimelessJewelData/NodeIndexMapping.lua"),
        )
        .unwrap();
        let header = |key: &str| -> u32 {
            let prefix = format!("nodeIDList[\"{key}\"] = ");
            src.lines()
                .find_map(|l| l.strip_prefix(prefix.as_str()))
                .and_then(|v| v.trim().parse().ok())
                .expect("size header")
        };
        let size = header("size");
        assert!(size >= 1931);
        assert_eq!(out.node_count, size);
        assert_eq!(out.notable_count, header("sizeNotable"));
        assert_eq!(out.nodes.len() as u32, size);
        // Indices are dense 0..node_count after the sort.
        assert_eq!(out.nodes[0].index, 0);
        assert_eq!(out.nodes.last().unwrap().index, size - 1);
        // Glorious Vanity (type 1) has a localIdToGlobalId table.
        assert!(!out.local_to_global[0].is_empty());
    }
}
