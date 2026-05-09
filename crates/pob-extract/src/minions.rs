//! Extract `Data/Minions.lua` into the typed [`pob_data::MinionData`].
//!
//! Foundation slice for [#20](https://github.com/jonatanferm/PathOfBuildingMK2/issues/20).
//! The upstream file is a chunk of the form:
//!
//! ```lua
//! local minions, mod = ...
//! minions["RaisedZombie"] = { name = ..., life = ..., modList = { mod(...), ... } }
//! ```
//!
//! We feed it the same `__pob_mod` recording function the skills extractor uses, so the
//! `mod()` calls inside `modList` capture into JSON tables matching skill statMap entries
//! (decodable via `pob_engine::skill::parse_extractor_mod`).

use std::path::Path;

use anyhow::{anyhow, Context, Result};
use indexmap::IndexMap;
use mlua::{Function as LuaFn, Lua, Value};
use pob_data::{MinionData, MinionType};
use serde_json::Value as J;

use crate::{lua_to_json, make_lua};

pub fn extract(pob_root: &Path) -> Result<MinionData> {
    let lua = make_lua()?;
    install_mod_recorder(&lua)?;
    install_flag_recorder(&lua)?;

    let mut minions = IndexMap::new();

    // Standard minion catalogue (`local minions, mod = ...`).
    let std_path = pob_root.join("src/Data/Minions.lua");
    load_minions_lua(&lua, &std_path, /*pass_flag=*/ false, &mut minions)
        .with_context(|| format!("loading {}", std_path.display()))?;

    // Spectres (`local minions, mod, flag = ...`). 264 entries with extra fields like
    // `lifeScaling`, `weaponType1`, `baseDamageIgnoresAttackSpeed`. We merge into the
    // same map; spectre keys are full metadata paths
    // (`Metadata/Monsters/Axis/AxisCaster`) and don't collide with standard minion ids.
    let spectres_path = pob_root.join("src/Data/Spectres.lua");
    if spectres_path.exists() {
        load_minions_lua(&lua, &spectres_path, /*pass_flag=*/ true, &mut minions)
            .with_context(|| format!("loading {}", spectres_path.display()))?;
    }

    Ok(MinionData { minions })
}

/// Load one PoB minion-data Lua chunk and merge each entry into `minions`.
/// `pass_flag = true` invokes the chunk with `(minions_table, __pob_mod, __pob_flag)`
/// (Spectres.lua takes 3 positional args); `false` invokes it with `(minions_table,
/// __pob_mod)` (Minions.lua takes 2).
fn load_minions_lua(
    lua: &Lua,
    path: &Path,
    pass_flag: bool,
    minions: &mut IndexMap<String, MinionType>,
) -> Result<()> {
    let src =
        std::fs::read_to_string(path).with_context(|| format!("reading {}", path.display()))?;
    let chunk = lua.load(&src).set_name(path.to_string_lossy().as_ref());
    let func = chunk
        .into_function()
        .with_context(|| format!("compiling {}", path.display()))?;

    let minions_table = lua.create_table()?;
    let mod_fn = lua.globals().get::<LuaFn>("__pob_mod")?;
    if pass_flag {
        let flag_fn = lua.globals().get::<LuaFn>("__pob_flag")?;
        func.call::<()>((minions_table.clone(), mod_fn, flag_fn))
            .with_context(|| format!("executing {}", path.display()))?;
    } else {
        func.call::<()>((minions_table.clone(), mod_fn))
            .with_context(|| format!("executing {}", path.display()))?;
    }

    let json = lua_to_json(Value::Table(minions_table))?;
    let raw_obj = json
        .as_object()
        .ok_or_else(|| anyhow!("{} did not return a table", path.display()))?;

    for (id, raw) in raw_obj {
        let parsed = parse_minion(raw).with_context(|| format!("parsing minion `{id}`"))?;
        minions.insert(id.clone(), parsed);
    }
    Ok(())
}

fn parse_minion(v: &J) -> Result<MinionType> {
    let obj = v
        .as_object()
        .ok_or_else(|| anyhow!("minion entry is not a table"))?;

    let name = obj.get("name").and_then(J::as_str).unwrap_or("").to_owned();
    let monster_tags = string_array(obj.get("monsterTags"));
    let life = obj.get("life").and_then(J::as_f64).unwrap_or(0.0);
    let energy_shield = obj.get("energyShield").and_then(J::as_f64);
    let armour = obj.get("armour").and_then(J::as_f64);
    let fire_resist = obj.get("fireResist").and_then(J::as_i64).unwrap_or(0) as i32;
    let cold_resist = obj.get("coldResist").and_then(J::as_i64).unwrap_or(0) as i32;
    let lightning_resist = obj.get("lightningResist").and_then(J::as_i64).unwrap_or(0) as i32;
    let chaos_resist = obj.get("chaosResist").and_then(J::as_i64).unwrap_or(0) as i32;
    let damage = obj.get("damage").and_then(J::as_f64).unwrap_or(0.0);
    let damage_spread = obj.get("damageSpread").and_then(J::as_f64).unwrap_or(0.0);
    let attack_time = obj.get("attackTime").and_then(J::as_f64).unwrap_or(0.0);
    let attack_range = obj.get("attackRange").and_then(J::as_f64).unwrap_or(0.0);
    let accuracy = obj.get("accuracy").and_then(J::as_f64).unwrap_or(0.0);
    let limit = obj.get("limit").and_then(J::as_str).map(str::to_owned);
    let skill_list = string_array(obj.get("skillList"));
    let mod_list = mod_array(obj.get("modList"));
    let life_scaling = obj
        .get("lifeScaling")
        .and_then(J::as_str)
        .map(str::to_owned);
    let weapon_type1 = obj
        .get("weaponType1")
        .and_then(J::as_str)
        .map(str::to_owned);
    let weapon_type2 = obj
        .get("weaponType2")
        .and_then(J::as_str)
        .map(str::to_owned);
    let base_damage_ignores_attack_speed = obj
        .get("baseDamageIgnoresAttackSpeed")
        .and_then(J::as_bool)
        .unwrap_or(false);

    Ok(MinionType {
        name,
        monster_tags,
        life,
        energy_shield,
        armour,
        fire_resist,
        cold_resist,
        lightning_resist,
        chaos_resist,
        damage,
        damage_spread,
        attack_time,
        attack_range,
        accuracy,
        limit,
        skill_list,
        mod_list,
        life_scaling,
        weapon_type1,
        weapon_type2,
        base_damage_ignores_attack_speed,
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

/// Pull the positional `mod()` recordings out of `modList` in numeric order,
/// preserving them as raw JSON values for downstream `parse_extractor_mod`.
fn mod_array(v: Option<&J>) -> Vec<J> {
    let Some(v) = v else { return Vec::new() };
    if let Some(arr) = v.as_array() {
        return arr.clone();
    }
    if let Some(o) = v.as_object() {
        let mut indexed: Vec<(u64, &J)> = o
            .iter()
            .filter_map(|(k, v)| k.parse::<u64>().ok().map(|n| (n, v)))
            .collect();
        indexed.sort_by_key(|(k, _)| *k);
        return indexed.into_iter().map(|(_, v)| v.clone()).collect();
    }
    Vec::new()
}

/// Mirror the `__pob_mod` helper from the skills extractor so `mod()` calls inside
/// minion `modList` tables capture into the same JSON shape as skill statMap entries.
fn install_mod_recorder(lua: &Lua) -> Result<()> {
    let globals = lua.globals();
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
        let mut idx = 1i64;
        for v in iter {
            t.set(idx, v)?;
            idx += 1;
        }
        Ok(t)
    })?;
    globals.set("__pob_mod", mod_fn)?;
    Ok(())
}

/// Mirror the `__pob_flag` helper from the skills extractor. Spectres.lua's chunk
/// signature is `(minions, mod, flag)`; the flag fn records `flag()` calls into the
/// same JSON shape used by skill statMaps so a future minion perform pass can decode
/// them with `pob_engine::skill::parse_extractor_mod`.
fn install_flag_recorder(lua: &Lua) -> Result<()> {
    let globals = lua.globals();
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
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pob_root() -> Option<std::path::PathBuf> {
        let here = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
        for ancestor in here.ancestors() {
            let candidate = ancestor.join(".PathOfBuilding");
            if candidate.join("src/Data/Minions.lua").exists() {
                return Some(candidate);
            }
        }
        None
    }

    #[test]
    fn extracts_canonical_minions() {
        let Some(root) = pob_root() else {
            eprintln!("skipping: .PathOfBuilding checkout not found next to workspace");
            return;
        };
        let data = extract(&root).expect("extracts cleanly");
        assert!(
            data.minions.len() >= 50,
            "expected 50+ minions, got {}",
            data.minions.len()
        );

        let zombie = data
            .minions
            .get("RaisedZombie")
            .expect("missing canonical RaisedZombie");
        assert_eq!(zombie.name, "Raised Zombie");
        assert!(zombie.life > 0.0);
        assert_eq!(zombie.fire_resist, 40);
        assert_eq!(zombie.chaos_resist, 20);
        assert!(zombie.damage > 0.0);
        assert_eq!(zombie.limit.as_deref(), Some("ActiveZombieLimit"));
        assert!(!zombie.skill_list.is_empty());
        assert!(
            !zombie.mod_list.is_empty(),
            "RaisedZombie has empty mod_list — recording helper didn't fire"
        );
        // mod_list entries should be structured `__kind: "mod"` records.
        for entry in &zombie.mod_list {
            assert_eq!(
                entry.get("__kind").and_then(|v| v.as_str()),
                Some("mod"),
                "mod_list entry should be a recorded mod table"
            );
        }

        // Sanity invariants across the full set.
        for (id, m) in &data.minions {
            assert!(!m.name.is_empty(), "minion `{id}` has empty name");
            assert!(m.life >= 0.0, "minion `{id}` has negative life multiplier");
        }
    }
}
