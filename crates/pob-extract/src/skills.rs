//! Extract skill data from `src/Data/Skills/*.lua`.
//!
//! Each skill file is a chunk of the form:
//!
//! ```lua
//! local skills, mod, flag, skill = ...
//! skills["Arc"] = {
//!     name = "Arc",
//!     ...
//!     statMap = {
//!         ["arc_damage_+%_final_for_each_remaining_chain"] = {
//!             mod("Damage", "MORE", nil, 0, bit.bor(KeywordFlag.Hit, KeywordFlag.Ailment),
//!                 { type = "PerStat", stat = "ChainRemaining" }),
//!         },
//!     },
//!     ...
//! }
//! ```
//!
//! ADR 0002 deferred this because `mod` / `flag` / `skill` are functions called at load
//! time. The fix is to register identical helpers in our Lua sandbox — they're just
//! constructor sugar around table literals (see `Modules/Data.lua:52-67`).

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use mlua::{Lua, Value};

use crate::{lua_to_json, make_lua};

/// Returns a list of (skill type / file name → produced JSON path).
pub fn extract_all(pob_root: &Path, out_dir: &Path) -> Result<Vec<String>> {
    let dir = pob_root.join("src/Data/Skills");
    let mut files: Vec<PathBuf> = Vec::new();
    for entry in std::fs::read_dir(&dir).with_context(|| format!("reading {}", dir.display()))? {
        let entry = entry?;
        let p = entry.path();
        if p.extension().and_then(|s| s.to_str()) == Some("lua") {
            files.push(p);
        }
    }
    files.sort();

    let mut produced = Vec::new();
    for file in files {
        let stem = file
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or("unknown")
            .to_owned();
        let json = extract_one(&file).with_context(|| format!("extracting {}", file.display()))?;
        let out_path = out_dir.join(format!("{stem}.json"));
        std::fs::write(&out_path, serde_json::to_string(&json)?)?;
        produced.push(stem);
    }
    Ok(produced)
}

fn extract_one(path: &Path) -> Result<serde_json::Value> {
    let lua = make_lua()?;
    install_skill_helpers(&lua)?;
    let src =
        std::fs::read_to_string(path).with_context(|| format!("reading {}", path.display()))?;
    let chunk = lua.load(&src).set_name(path.to_string_lossy().as_ref());
    let func = chunk.into_function()?;

    let skills_table = lua.create_table()?;
    let mod_fn = lua_globals_get::<mlua::Function>(&lua, "__pob_mod")?;
    let flag_fn = lua_globals_get::<mlua::Function>(&lua, "__pob_flag")?;
    let skill_fn = lua_globals_get::<mlua::Function>(&lua, "__pob_skill")?;

    func.call::<()>((skills_table.clone(), mod_fn, flag_fn, skill_fn))
        .with_context(|| format!("executing {}", path.display()))?;

    let value: Value = Value::Table(skills_table);
    let json = lua_to_json(value)?;
    // Convert into a deterministic-ordering BTreeMap before re-emitting so two extraction
    // runs are byte-identical.
    let map = json
        .as_object()
        .cloned()
        .ok_or_else(|| anyhow::anyhow!("skills root not an object"))?;
    let sorted: BTreeMap<String, serde_json::Value> = map.into_iter().collect();
    Ok(serde_json::Value::Object(sorted.into_iter().collect()))
}

fn lua_globals_get<T: mlua::FromLua>(lua: &Lua, name: &str) -> Result<T> {
    Ok(lua.globals().get::<T>(name)?)
}

fn install_skill_helpers(lua: &Lua) -> Result<()> {
    let globals = lua.globals();

    // mod(name, type, value, flags, keywordFlags, ...tags) → table identical in shape to
    // PoB's `makeSkillMod` so we can convert directly to our `Mod` type later.
    let mod_fn = lua.create_function(|lua, args: mlua::MultiValue| {
        let mut iter = args.into_iter();
        let name: Value = iter.next().unwrap_or(Value::Nil);
        let mtype: Value = iter.next().unwrap_or(Value::Nil);
        let value: Value = iter.next().unwrap_or(Value::Nil);
        let flags: Value = iter.next().unwrap_or(Value::Integer(0));
        let kw: Value = iter.next().unwrap_or(Value::Integer(0));
        let t = lua.create_table()?;
        t.set("__kind", "mod")?;
        t.set("name", name)?;
        t.set("type", mtype)?;
        t.set("value", value)?;
        t.set("flags", flags)?;
        t.set("keywordFlags", kw)?;
        // Trailing tags.
        let mut idx = 1i64;
        for v in iter {
            t.set(idx, v)?;
            idx += 1;
        }
        Ok(t)
    })?;
    globals.set("__pob_mod", mod_fn.clone())?;

    let flag_fn = lua.create_function(|lua, args: mlua::MultiValue| {
        let mut iter = args.into_iter();
        let name: Value = iter.next().unwrap_or(Value::Nil);
        let t = lua.create_table()?;
        t.set("__kind", "flag")?;
        t.set("name", name)?;
        t.set("type", "FLAG")?;
        t.set("value", true)?;
        let mut idx = 1i64;
        for v in iter {
            t.set(idx, v)?;
            idx += 1;
        }
        Ok(t)
    })?;
    globals.set("__pob_flag", flag_fn)?;

    let skill_fn = lua.create_function(|lua, args: mlua::MultiValue| {
        let mut iter = args.into_iter();
        let key: Value = iter.next().unwrap_or(Value::Nil);
        let val: Value = iter.next().unwrap_or(Value::Nil);
        let t = lua.create_table()?;
        t.set("__kind", "skillData")?;
        t.set("name", "SkillData")?;
        t.set("type", "LIST")?;
        let inner = lua.create_table()?;
        inner.set("key", key)?;
        inner.set("value", val)?;
        t.set("value", inner)?;
        let mut idx = 1i64;
        for v in iter {
            t.set(idx, v)?;
            idx += 1;
        }
        Ok(t)
    })?;
    globals.set("__pob_skill", skill_fn)?;

    Ok(())
}
