use std::path::Path;

use anyhow::{Context, Result};
use indexmap::IndexMap;
use pob_data::Gem;
use serde_json::Value;

use crate::lua_value as lv;
use crate::{load_lua_file_returning, make_lua};

pub fn extract(pob_root: &Path) -> Result<IndexMap<String, Gem>> {
    let path = pob_root.join("src/Data/Gems.lua");
    let lua = make_lua()?;
    let value = load_lua_file_returning(&lua, &path)?;
    let map = value
        .as_object()
        .with_context(|| format!("{} did not return an object", path.display()))?;
    let mut out = IndexMap::with_capacity(map.len());
    for (id, raw) in map {
        let gem = parse_one(raw).with_context(|| format!("parsing gem {id}"))?;
        out.insert(id.clone(), gem);
    }
    Ok(out)
}

fn parse_one(v: &Value) -> Result<Gem> {
    let tags = lv::opt_object(v, "tags")
        .map(|o| {
            o.iter()
                .filter(|(_, v)| v.as_bool() == Some(true))
                .map(|(k, _)| k.clone())
                .collect()
        })
        .unwrap_or_default();
    Ok(Gem {
        name: lv::req_str(v, "name")?,
        base_type_name: lv::opt_str(v, "baseTypeName").unwrap_or_default(),
        game_id: lv::req_str(v, "gameId")?,
        variant_id: lv::opt_str(v, "variantId"),
        granted_effect_id: lv::opt_str(v, "grantedEffectId").unwrap_or_default(),
        secondary_granted_effect_id: lv::opt_str(v, "secondaryGrantedEffectId"),
        vaal_gem: lv::opt_bool(v, "vaalGem").unwrap_or(false),
        tags,
        tag_string: lv::opt_str(v, "tagString").unwrap_or_default(),
        req_str: lv::opt_u64(v, "reqStr").unwrap_or(0) as u32,
        req_dex: lv::opt_u64(v, "reqDex").unwrap_or(0) as u32,
        req_int: lv::opt_u64(v, "reqInt").unwrap_or(0) as u32,
        natural_max_level: lv::opt_u64(v, "naturalMaxLevel").unwrap_or(0) as u32,
    })
}
