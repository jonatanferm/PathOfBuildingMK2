//! Walk `src/Data/Bases/*.lua` and emit one big map keyed by base-item name.

use std::collections::BTreeMap;
use std::path::Path;

use anyhow::{Context, Result};
use indexmap::IndexMap;
use pob_data::{ArmourStats, FlaskStats, ItemBase, ItemReq, WeaponStats};
use serde_json::Value;

use crate::lua_value as lv;
use crate::{list_files, load_lua_file_with_table_arg, make_lua};

pub fn extract_all(pob_root: &Path) -> Result<IndexMap<String, ItemBase>> {
    let bases_dir = pob_root.join("src/Data/Bases");
    let lua = make_lua()?;

    let files = list_files(&bases_dir, |p| {
        p.extension().and_then(|s| s.to_str()) == Some("lua")
    })?;

    // BTreeMap for deterministic ordering across runs.
    let mut out: BTreeMap<String, ItemBase> = BTreeMap::new();
    for file in files {
        let table = load_lua_file_with_table_arg(&lua, &file)
            .with_context(|| format!("loading {}", file.display()))?;
        let map = table
            .as_object()
            .with_context(|| format!("{} did not produce a table", file.display()))?;
        for (name, raw) in map {
            let base =
                parse_one(raw).with_context(|| format!("parsing {name} in {}", file.display()))?;
            out.insert(name.clone(), base);
        }
    }
    // Preserve insertion order in IndexMap (BTreeMap iter is sorted).
    Ok(out.into_iter().collect())
}

fn parse_one(v: &Value) -> Result<ItemBase> {
    let r#type = lv::req_str(v, "type")?;
    let sub_type = lv::opt_str(v, "subType");
    let socket_limit = lv::opt_u64(v, "socketLimit").map(|n| n as u8);
    let tags = lv::opt_object(v, "tags")
        .map(|o| {
            o.iter()
                .filter(|(_, v)| v.as_bool() == Some(true))
                .map(|(k, _)| k.clone())
                .collect()
        })
        .unwrap_or_default();
    let influence_tags = lv::opt_object(v, "influenceTags")
        .map(|o| {
            o.iter()
                .filter_map(|(k, v)| v.as_str().map(|s| (k.clone(), s.to_owned())))
                .collect::<IndexMap<_, _>>()
        })
        .unwrap_or_default();
    let implicit = lv::opt_str(v, "implicit");
    let implicit_mod_types = parse_implicit_mod_types(v);
    let req = parse_req(v);

    let weapon = lv::opt_object(v, "weapon").map(parse_weapon).transpose()?;
    let armour = lv::opt_object(v, "armour").map(parse_armour).transpose()?;
    let flask = lv::opt_object(v, "flask").map(parse_flask).transpose()?;

    Ok(ItemBase {
        r#type,
        sub_type,
        socket_limit,
        tags,
        influence_tags,
        implicit,
        implicit_mod_types,
        req,
        weapon,
        armour,
        flask,
    })
}

fn parse_implicit_mod_types(v: &Value) -> Vec<Vec<String>> {
    let Some(arr) = v
        .as_object()
        .and_then(|m| m.get("implicitModTypes"))
        .and_then(Value::as_array)
    else {
        return Vec::new();
    };
    arr.iter()
        .filter_map(|item| {
            item.as_array().map(|inner| {
                inner
                    .iter()
                    .filter_map(|s| s.as_str().map(str::to_owned))
                    .collect()
            })
        })
        .collect()
}

fn parse_req(v: &Value) -> ItemReq {
    let Some(o) = lv::opt_object(v, "req") else {
        return ItemReq::default();
    };
    let host = Value::Object(o.clone());
    ItemReq {
        level: lv::opt_u64(&host, "level").map(|n| n as u32),
        str: lv::opt_u64(&host, "str").map(|n| n as u32),
        dex: lv::opt_u64(&host, "dex").map(|n| n as u32),
        int: lv::opt_u64(&host, "int").map(|n| n as u32),
    }
}

fn parse_weapon(o: &serde_json::Map<String, Value>) -> Result<WeaponStats> {
    let host = Value::Object(o.clone());
    Ok(WeaponStats {
        physical_min: lv::opt_f64(&host, "PhysicalMin").unwrap_or(0.0) as f32,
        physical_max: lv::opt_f64(&host, "PhysicalMax").unwrap_or(0.0) as f32,
        crit_chance_base: lv::opt_f64(&host, "CritChanceBase").unwrap_or(0.0) as f32,
        attack_rate_base: lv::opt_f64(&host, "AttackRateBase").unwrap_or(0.0) as f32,
        range: lv::opt_f64(&host, "Range").unwrap_or(0.0) as f32,
    })
}

fn parse_armour(o: &serde_json::Map<String, Value>) -> Result<ArmourStats> {
    let host = Value::Object(o.clone());
    Ok(ArmourStats {
        armour_base_min: lv::opt_f64(&host, "ArmourBaseMin").unwrap_or(0.0) as f32,
        armour_base_max: lv::opt_f64(&host, "ArmourBaseMax").unwrap_or(0.0) as f32,
        evasion_base_min: lv::opt_f64(&host, "EvasionBaseMin").unwrap_or(0.0) as f32,
        evasion_base_max: lv::opt_f64(&host, "EvasionBaseMax").unwrap_or(0.0) as f32,
        energy_shield_base_min: lv::opt_f64(&host, "EnergyShieldBaseMin").unwrap_or(0.0) as f32,
        energy_shield_base_max: lv::opt_f64(&host, "EnergyShieldBaseMax").unwrap_or(0.0) as f32,
        ward_base_min: lv::opt_f64(&host, "WardBaseMin").unwrap_or(0.0) as f32,
        ward_base_max: lv::opt_f64(&host, "WardBaseMax").unwrap_or(0.0) as f32,
        block_chance_base: lv::opt_f64(&host, "BlockChance").unwrap_or(0.0) as f32,
        movement_penalty: lv::opt_f64(&host, "MovementPenalty").unwrap_or(0.0) as f32,
    })
}

fn parse_flask(o: &serde_json::Map<String, Value>) -> Result<FlaskStats> {
    let host = Value::Object(o.clone());
    Ok(FlaskStats {
        life: lv::opt_f64(&host, "life").map(|n| n as f32),
        mana: lv::opt_f64(&host, "mana").map(|n| n as f32),
        duration: lv::opt_f64(&host, "duration").unwrap_or(0.0) as f32,
        charges_used: lv::opt_u64(&host, "chargesUsed").unwrap_or(0) as u32,
        charges_max: lv::opt_u64(&host, "chargesMax").unwrap_or(0) as u32,
    })
}
