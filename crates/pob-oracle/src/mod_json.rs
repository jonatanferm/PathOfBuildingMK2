//! The oracle's mod JSON (ORC-X; it replaces the legacy `legacy_modifier_serde`
//! of AMENDMENTS A1 / CCR-0011 §11): PoB mods, tags and mod values as JSON,
//! decoded into `pob_gamedata::{Mod, Tag, Value}`.
//!
//! The shape is the one `lua/state_dump.lua` (`encodeMod`) and `src/oracle.lua`
//! write: `{name, type, value, flags, keywordFlags, source?, tags: [...],
//! <other keys>, "$at"?}`; tables inside values are plain (`$seq`, `$num`,
//! `$float`). The encoders are `pob_calc::probe::{mod_json, tag_json,
//! value_json}`, and `decode ∘ encode` is the identity on every dumped mod of
//! the corpus (tests/ORC-X.rs round trip).

use std::sync::Arc;

use pob_gamedata::{Mod, ModType, Name, Tag, TagType, Value, ValueTable};
use serde_json::{Map, Value as J};

fn short(j: &J) -> String {
    let s = j.to_string();
    if s.len() > 160 {
        format!("{}…", &s[..s.char_indices().nth(160).map_or(s.len(), |(i, _)| i)])
    } else {
        s
    }
}

fn unescape(k: &str) -> Option<&str> {
    match k.strip_prefix('$') {
        Some(rest) if rest.starts_with('$') => Some(rest),
        Some(_) => None,
        None => Some(k),
    }
}

// ---- mods ------------------------------------------------------------------------------------

/// The Tag fields (`pob_gamedata::Tag`), i.e. the keys serde decodes.
const TAG_KEYS: &[&str] = &[
    "type", "var", "varList", "neg", "actor", "stat", "statList", "div", "divVar", "base",
    "noFloor", "invert", "floor", "limit", "limitVar", "limitStat", "limitTotal",
    "limitNegTotal", "limitActor", "percent", "percentVar", "threshold", "thresholdVar",
    "thresholdStat", "thresholdActor", "thresholdPercent", "thresholdPercentVar", "upper",
    "equals", "ramp", "itemSlot", "allSlots", "bothSlots", "excludeSelf", "searchCond",
    "rarityCond", "corruptedCond", "shaperCond", "elderCond", "nameCond", "slotName",
    "slotNameList", "keyword", "socketColor", "sockets", "skillName", "skillNameList",
    "includeTransfigured", "summonSkill", "skillId", "skillPart", "skillPartList", "skillType",
    "skillTypeList", "baseFlag", "modFlags", "keywordFlags", "monsterTag", "monsterTagList",
    "globalLimit", "globalLimitKey", "num", "effectType", "effectName", "effectCond",
    "effectEnemyCond", "effectStackVar", "effectStackLimit", "modCond", "unscalable",
    "allowTotemBuff", "applyNotPlayer", "applyMinions", "excludeItemType",
];

/// A tag table (pob-state shape) as a `Tag`. Keys the typed tag cannot hold
/// exactly (unknown keys, an explicit `false`/`0`, a value of another type)
/// go to `tag.extra` verbatim, so the tag re-encodes to the same table.
pub fn decode_tag(j: &J) -> Result<Tag, String> {
    let J::Object(m) = j else {
        // a non-table tag (LuaNonTable)
        return serde_json::from_value::<Tag>(j.clone()).map_err(|e| e.to_string());
    };
    let mut known = Map::new();
    let mut extra: Vec<(String, J)> = Vec::new();
    for (k, v) in m {
        if TAG_KEYS.contains(&k.as_str()) {
            let mut probe_m = Map::new();
            if let Some(t) = m.get("type") {
                probe_m.insert("type".into(), t.clone());
            }
            probe_m.insert(k.clone(), v.clone());
            if serde_json::from_value::<Tag>(J::Object(probe_m)).is_ok() {
                known.insert(k.clone(), v.clone());
                continue;
            }
        }
        extra.push((k.clone(), v.clone()));
    }
    let mut t: Tag = serde_json::from_value(J::Object(known)).map_err(|e| e.to_string())?;
    // keys whose typed form does not re-encode to the dumped value
    let re = pob_calc::probe::tag_json(&t);
    for (k, v) in m {
        if extra.iter().any(|(x, _)| x == k) {
            continue;
        }
        if re.get(k) != Some(v) && !same_json(re.get(k), v) {
            extra.push((k.clone(), v.clone()));
        }
    }
    if !extra.is_empty() {
        let mut ex = t.extra.take().map(|b| *b).unwrap_or_default();
        for (k, v) in extra {
            ex.set(Name::from(k.as_str()), j_to_value(&v));
        }
        t.extra = Some(Box::new(ex));
    }
    if t.r#type == TagType::LuaNonTable {
        return Ok(t);
    }
    Ok(t)
}

fn same_json(a: Option<&J>, b: &J) -> bool {
    match (a, b) {
        (Some(J::Number(x)), J::Number(y)) => x.as_f64() == y.as_f64(),
        _ => false,
    }
}

/// A mod in the pob-state shape (`state_dump.lua` `encodeMod`), plus its `$at`.
pub fn decode_mod(j: &J) -> Result<(Mod, Option<String>), String> {
    let J::Object(m) = j else {
        return Err(format!("not a mod: {}", short(j)));
    };
    let name = m
        .get("name")
        .and_then(J::as_str)
        .ok_or("mod without a name")?;
    let ty = m.get("type").and_then(J::as_str).ok_or("mod without a type")?;
    let ty = ModType::parse(ty).ok_or_else(|| format!("unknown mod type {ty}"))?;
    let mut extra = ValueTable::new();
    let mut flags = 0u32;
    match m.get("flags") {
        Some(J::Number(n)) => flags = n.as_f64().unwrap_or(0.0) as u32,
        Some(other) => extra.set(pob_lua::n!("flags"), j_to_value(other)),
        None => {}
    }
    let keyword_flags = m
        .get("keywordFlags")
        .and_then(J::as_f64)
        .unwrap_or(0.0) as u32;
    let source = m.get("source").and_then(J::as_str).map(Arc::<str>::from);
    if let Some(s) = m.get("source") {
        if !s.is_string() {
            extra.set(pob_lua::n!("source"), j_to_value(s));
        }
    }
    let mut tags = smallvec::SmallVec::new();
    if let Some(J::Array(a)) = m.get("tags") {
        for t in a {
            tags.push(decode_tag(t)?);
        }
    }
    let mut at = None;
    for (k, v) in m {
        match k.as_str() {
            "name" | "type" | "value" | "flags" | "keywordFlags" | "source" | "tags" => {}
            "$at" => at = v.as_str().map(str::to_owned),
            other => {
                let key = unescape(other).unwrap_or(other);
                extra.set(Name::from(key), j_to_value(v));
            }
        }
    }
    let value = m.get("value").map_or(Value::Nil, j_to_value);
    Ok((
        Mod {
            name: Name::from(name),
            r#type: ty,
            value,
            flags,
            keywordFlags: keyword_flags,
            source,
            tags,
            extra: (!extra.is_empty()).then(|| Box::new(extra)),
        },
        at,
    ))
}

/// A dumped plain value (a mod value, a tag field) as a `Value`: tables
/// inline, `$num` members as `#n` fields, other `$`-members kept verbatim
/// (markers inside mod values), jewel closures as `JewelFunc`.
pub fn j_to_value(j: &J) -> Value {
    match j {
        J::Null => Value::Nil,
        J::Bool(b) => Value::Bool(*b),
        J::Number(n) => Value::Num(n.as_f64().unwrap_or(f64::NAN)),
        J::String(s) => Value::Str(Arc::from(s.as_str())),
        J::Array(a) => Value::Table(Arc::new(ValueTable {
            array: a.iter().map(j_to_value).collect(),
            fields: Vec::new(),
        })),
        J::Object(m) => {
            // `{"$num": "inf"}`: pob_oracle outputs / stages non-finite numbers
            if m.len() == 1 {
                if let Some(f) = m.get("$num").and_then(J::as_str) {
                    return Value::Num(match f {
                        "inf" => f64::INFINITY,
                        "-inf" => f64::NEG_INFINITY,
                        "-0" => -0.0,
                        _ => f64::NAN,
                    });
                }
            }
            if let Some(f) = m.get("$float").and_then(J::as_str) {
                if m.len() == 1 {
                    return Value::Num(match f {
                        "inf" => f64::INFINITY,
                        "-inf" => f64::NEG_INFINITY,
                        "-0" => -0.0,
                        _ => f64::NAN,
                    });
                }
            }
            // { type = …, radius = …, func = function … }
            if let Some(src) = m.get("func").and_then(|f| f.get("$fn")).and_then(J::as_str) {
                let only = m
                    .keys()
                    .all(|k| matches!(k.as_str(), "func" | "type" | "radius"))
                    && m["func"].as_object().is_some_and(|f| f.len() == 1);
                if only {
                    let r#type = m.get("type").and_then(J::as_str).map(Name::from);
                    return Value::JewelFunc(Arc::new(pob_gamedata::JewelFuncSpec {
                        key: Name::from(src),
                        kind: pob_gamedata::JewelFuncKind::from_type(
                            r#type.as_ref().map(|n| n.as_str()),
                        ),
                        args: smallvec::SmallVec::new(),
                        r#type,
                        radius: m.get("radius").and_then(J::as_str).map(Name::from),
                    }));
                }
            }
            // a nested mod in the oracle's stages shape (`src/oracle.lua`: tags as a
            // `"tags"` list; the probe dumps never write a nested mod this way)
            let mod_type = m.get("type").and_then(J::as_str).and_then(ModType::parse);
            if m.get("name").is_some_and(J::is_string)
                && mod_type.is_some()
                && m.get("tags").is_some_and(J::is_array)
                && m.contains_key("flags")
            {
                if let Ok((md, _)) = decode_mod(j) {
                    return Value::Mod(Arc::new(md));
                }
            }
            // a nested mod (a plain table: tags under $seq)
            let mod_keys = m.keys().all(|k| {
                matches!(
                    k.as_str(),
                    "name" | "type" | "value" | "flags" | "keywordFlags" | "source" | "$seq"
                )
            });
            if mod_keys && m.contains_key("name") && m.contains_key("flags") && m.contains_key("keywordFlags") {
                if let Ok(md) = serde_json::from_value::<Mod>(j.clone()) {
                    let back = pob_calc::probe::value_json(&Value::Mod(Arc::new(md.clone())));
                    if &back == j {
                        return Value::Mod(Arc::new(md));
                    }
                }
            }
            let mut t = ValueTable::new();
            for (k, v) in m {
                match k.as_str() {
                    "$seq" => {
                        if let J::Array(a) = v {
                            t.array = a.iter().map(j_to_value).collect();
                        }
                    }
                    "$num" => {
                        if let J::Object(n) = v {
                            for (nk, nv) in n {
                                t.fields
                                    .push((Name::from(format!("#{nk}").as_str()), j_to_value(nv)));
                            }
                        }
                    }
                    _ => t.fields.push((Name::from(k.as_str()), j_to_value(v))),
                }
            }
            Value::Table(Arc::new(t))
        }
    }
}

