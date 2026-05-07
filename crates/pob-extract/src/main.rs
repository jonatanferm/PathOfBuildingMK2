//! Reads PoB's `src/Data/` and `src/TreeData/` Lua tables, emits JSON files under the
//! workspace `data/` directory that the runtime crates can deserialise.
//!
//! Run from the workspace root:
//!
//! ```text
//! cargo run -p pob-extract -- --pob ../PathOfBuilding --out data
//! ```
//!
//! Defaults: `--pob ../PathOfBuilding`, `--out data`. The input directory must be a
//! checkout of PathOfBuildingCommunity/PathOfBuilding (or a fork).

#![allow(dead_code)] // many helpers are convenience that submodules grow into

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use anyhow::{anyhow, bail, Context, Result};
use mlua::{Lua, Table, Value};

mod cli;
mod tree;

fn main() -> Result<()> {
    let args = cli::Args::parse();
    println!("pob-extract: pob={} out={}", args.pob.display(), args.out.display());
    std::fs::create_dir_all(&args.out)
        .with_context(|| format!("creating output dir {}", args.out.display()))?;

    let mut wrote = Vec::new();

    // Bases — many small files merged into one.
    let bases_path = args.out.join("bases.json");
    let bases = bases::extract_all(&args.pob)
        .with_context(|| "extracting item bases".to_string())?;
    let json = serde_json::to_string_pretty(&bases)?;
    std::fs::write(&bases_path, json).with_context(|| format!("writing {}", bases_path.display()))?;
    wrote.push(bases_path);

    // Gems — one file.
    let gems_path = args.out.join("gems.json");
    let gems = gems::extract(&args.pob)?;
    let json = serde_json::to_string_pretty(&gems)?;
    std::fs::write(&gems_path, json).with_context(|| format!("writing {}", gems_path.display()))?;
    wrote.push(gems_path);

    // Trees — one JSON per tree version directory under data/trees/.
    let trees_dir = args.out.join("trees");
    std::fs::create_dir_all(&trees_dir)?;
    let mut tree_versions = Vec::new();
    let mut skipped_trees = Vec::new();
    for version in tree::list_tree_versions(&args.pob)? {
        let tree_path = trees_dir.join(format!("{version}.json"));
        match tree::extract(&args.pob, &version) {
            Ok(tree) => {
                let json = serde_json::to_string(&tree)?;
                std::fs::write(&tree_path, json)
                    .with_context(|| format!("writing {}", tree_path.display()))?;
                tree_versions.push(version);
                wrote.push(tree_path);
            }
            Err(tree::ExtractError::IncompatibleSchema(reason)) => {
                eprintln!("  skip {version}: {reason}");
                skipped_trees.push((version, reason));
            }
            Err(tree::ExtractError::Other(e)) => {
                return Err(e).with_context(|| format!("extracting tree {version}"));
            }
        }
    }
    if !skipped_trees.is_empty() {
        let summary: Vec<_> = skipped_trees
            .iter()
            .map(|(v, r)| format!("{v}: {r}"))
            .collect();
        std::fs::write(
            args.out.join("trees/skipped.txt"),
            summary.join("\n") + "\n",
        )?;
    }
    let index_path = trees_dir.join("index.json");
    std::fs::write(&index_path, serde_json::to_string_pretty(&tree_versions)?)?;
    wrote.push(index_path);

    println!("wrote {} files:", wrote.len());
    for p in &wrote {
        let bytes = std::fs::metadata(p).map(|m| m.len()).unwrap_or(0);
        println!("  {} ({} bytes)", p.display(), bytes);
    }
    Ok(())
}

mod bases;
mod gems;
mod lua_value;

/// Helper used by submodules: open a sandboxed Lua state with stubs for the globals PoB's
/// data files expect.
pub(crate) fn make_lua() -> Result<Lua> {
    let lua = Lua::new();
    {
        let globals = lua.globals();
        // PoB's Bases/* and Gems.lua reference these helpers; provide inert stand-ins so
        // execution doesn't fault. These only matter for files we don't fully extract.
        let stub = lua.create_function(|_, _: mlua::MultiValue| Ok(()))?;
        globals.set("LoadModule", stub.clone())?;
        globals.set("ConPrintf", stub.clone())?;

        // bit.bor / bit.band shims (Lua 5.4 has no `bit` library by default — mlua's
        // `lua54` feature ships LuaJIT-compatible bit ops, but explicit shims are safer.)
        let bit_tbl = lua.create_table()?;
        bit_tbl.set(
            "bor",
            lua.create_function(|_, args: mlua::MultiValue| {
                let mut acc: i64 = 0;
                for v in args {
                    if let Value::Integer(i) = v {
                        acc |= i;
                    } else if let Value::Number(n) = v {
                        acc |= n as i64;
                    }
                }
                Ok(acc)
            })?,
        )?;
        bit_tbl.set(
            "band",
            lua.create_function(|_, args: mlua::MultiValue| {
                let mut acc: i64 = -1;
                for v in args {
                    if let Value::Integer(i) = v {
                        acc &= i;
                    } else if let Value::Number(n) = v {
                        acc &= n as i64;
                    }
                }
                Ok(acc)
            })?,
        )?;
        bit_tbl.set(
            "bnot",
            lua.create_function(|_, v: i64| Ok(!v))?,
        )?;
        bit_tbl.set(
            "bxor",
            lua.create_function(|_, args: mlua::MultiValue| {
                let mut acc: i64 = 0;
                for v in args {
                    if let Value::Integer(i) = v {
                        acc ^= i;
                    }
                }
                Ok(acc)
            })?,
        )?;
        globals.set("bit", bit_tbl)?;
    }
    Ok(lua)
}

/// Walk a Lua value into a `serde_json::Value`. Keeps integer/string keys distinct so
/// later code can decide how to interpret them.
pub(crate) fn lua_to_json(v: Value) -> Result<serde_json::Value> {
    use serde_json::Value as J;
    match v {
        Value::Nil => Ok(J::Null),
        Value::Boolean(b) => Ok(J::Bool(b)),
        Value::Integer(i) => Ok(J::Number(i.into())),
        Value::Number(n) => Ok(serde_json::Number::from_f64(n)
            .map_or(J::Null, J::Number)),
        Value::String(s) => Ok(J::String(s.to_str()?.to_owned())),
        Value::Table(t) => table_to_json(t),
        other => bail!("unsupported lua value: {other:?}"),
    }
}

fn table_to_json(t: Table) -> Result<serde_json::Value> {
    use serde_json::Value as J;
    // Decide whether this is an array (consecutive 1..=n integer keys, no others) or a
    // map. Lua's `t:sequence_values()` would only return the array part, but we want to
    // detect mixed tables too.
    let mut arr_max: i64 = 0;
    let mut has_string_key = false;
    let mut has_non_seq_int_key = false;

    let pairs: Vec<(Value, Value)> = t.pairs::<Value, Value>().collect::<Result<_, _>>()?;
    for (k, _) in &pairs {
        match k {
            Value::Integer(i) => {
                if *i >= 1 {
                    arr_max = arr_max.max(*i);
                } else {
                    has_non_seq_int_key = true;
                }
            }
            Value::String(_) => has_string_key = true,
            _ => has_non_seq_int_key = true,
        }
    }

    let total = pairs.len() as i64;
    if !has_string_key && !has_non_seq_int_key && arr_max == total && arr_max > 0 {
        // Sparse-free 1..=n array.
        let mut out = vec![J::Null; arr_max as usize];
        for (k, v) in pairs {
            if let Value::Integer(i) = k {
                out[(i - 1) as usize] = lua_to_json(v)?;
            }
        }
        return Ok(J::Array(out));
    }

    let mut map = serde_json::Map::with_capacity(pairs.len());
    for (k, v) in pairs {
        let key = match k {
            Value::String(s) => s.to_str()?.to_owned(),
            Value::Integer(i) => i.to_string(),
            Value::Number(n) => n.to_string(),
            other => bail!("unsupported table key: {other:?}"),
        };
        map.insert(key, lua_to_json(v)?);
    }
    Ok(J::Object(map))
}

/// Read a Lua file and return its top-level `return`ed value, evaluated in our sandbox.
pub(crate) fn load_lua_file_returning(lua: &Lua, path: &Path) -> Result<serde_json::Value> {
    let src = std::fs::read_to_string(path)
        .with_context(|| format!("reading {}", path.display()))?;
    let chunk = lua.load(&src).set_name(path.to_string_lossy().as_ref());
    let v: Value = chunk
        .eval()
        .with_context(|| format!("evaluating {}", path.display()))?;
    lua_to_json(v)
}

/// Read a Lua file like `Bases/sword.lua` that mutates an `itemBases` table passed via
/// varargs. Returns the table after execution.
pub(crate) fn load_lua_file_with_table_arg(
    lua: &Lua,
    path: &Path,
) -> Result<serde_json::Value> {
    let src = std::fs::read_to_string(path)
        .with_context(|| format!("reading {}", path.display()))?;
    let table = lua.create_table()?;
    let chunk = lua.load(&src).set_name(path.to_string_lossy().as_ref());
    let func = chunk
        .into_function()
        .with_context(|| format!("compiling {}", path.display()))?;
    func.call::<()>(table.clone())
        .with_context(|| format!("executing {}", path.display()))?;
    table_to_json(table)
}

/// Convenience: walk a directory and return paths of files matching `predicate`, sorted.
pub(crate) fn list_files(dir: &Path, predicate: impl Fn(&Path) -> bool) -> Result<Vec<PathBuf>> {
    let mut out = Vec::new();
    for entry in std::fs::read_dir(dir).with_context(|| format!("reading {}", dir.display()))? {
        let entry = entry?;
        let p = entry.path();
        if p.is_file() && predicate(&p) {
            out.push(p);
        }
    }
    out.sort();
    Ok(out)
}

/// Used by tree.rs as a typed accessor.
pub(crate) fn obj<'a>(v: &'a serde_json::Value) -> Result<&'a serde_json::Map<String, serde_json::Value>> {
    v.as_object().ok_or_else(|| anyhow!("expected JSON object, got {v:?}"))
}

#[allow(dead_code)]
pub(crate) fn arr<'a>(v: &'a serde_json::Value) -> Result<&'a [serde_json::Value]> {
    v.as_array()
        .map(Vec::as_slice)
        .ok_or_else(|| anyhow!("expected JSON array, got {v:?}"))
}

pub(crate) fn merge_into(
    dest: &mut BTreeMap<String, serde_json::Value>,
    other: serde_json::Value,
) -> Result<()> {
    let map = match other {
        serde_json::Value::Object(m) => m,
        _ => bail!("expected object to merge"),
    };
    for (k, v) in map {
        dest.insert(k, v);
    }
    Ok(())
}
