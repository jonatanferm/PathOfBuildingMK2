//! Serde for the modifier model, in PoB's own table shape.
//!
//! A [`Mod`] serialises as the JSON image of the Lua table
//! `modLib.createMod` builds, with the tag array part under `"tags"`:
//!
//! ```json
//! { "name": "Life", "type": "INC", "value": 10, "flags": 0, "keywordFlags": 0,
//!   "source": "Tree:1234", "tags": [ { "type": "Condition", "var": "FullLife" } ] }
//! ```
//!
//! * `source` / `sourceSlot` are omitted when unset; `replaced` / `converted` appear only when
//!   true.
//! * A [`Tag`] is `{ "type": …, <PoB field>: … }` using PoB's field names
//!   (`limitVar`, `skillNameList`, …); unset / false / zero fields are
//!   omitted, and unmodelled fields ([`Tag::extra`]) appear under their own
//!   names.
//! * A [`Value`] is `null` / bool / number / string; a nested mod (`{ mod = … }`
//!   holds one) is a mod object; a [`Table`] is a JSON array when it has only
//!   an array part, otherwise an object whose array part sits under the keys
//!   `"1"`, `"2"`, …. Non-finite numbers are `{"$num": "inf" | "-inf" | "nan"}`.
//!
//! Decoding mirrors PoB's conventions: an object with string `name` and a
//! `type` that is a known mod type is a mod; `{"$num": …}` is a number;
//! contiguous `"1".."n"` keys form the array part. This is exactly the shape
//! `pob-oracle`'s Lua dumper writes, so PoB's own mods deserialise directly.

use serde::de::Error as _;
use serde::{Deserialize, Deserializer, Serialize, Serializer};
use serde_json::{Map, Value as J};

use crate::modifier::{Mod, ModType, Sockets, Table, Tag, TagType, Value};

// ---- encode ----------------------------------------------------------------

fn num_to_json(n: f64) -> J {
    if n.is_finite() {
        if n.fract() == 0.0 && n.abs() < 9.0e15 {
            J::from(n as i64)
        } else {
            serde_json::Number::from_f64(n).map_or(J::Null, J::Number)
        }
    } else {
        let s = if n.is_nan() {
            "nan"
        } else if n > 0.0 {
            "inf"
        } else {
            "-inf"
        };
        let mut m = Map::new();
        m.insert("$num".into(), J::from(s));
        J::Object(m)
    }
}

fn strs(v: &[String]) -> J {
    J::Array(v.iter().map(|s| J::from(s.as_str())).collect())
}

fn nums(v: &[f64]) -> J {
    J::Array(v.iter().map(|n| num_to_json(*n)).collect())
}

/// JSON image of a mod [`Value`].
#[must_use]
pub fn value_to_json(v: &Value) -> J {
    match v {
        Value::Nil => J::Null,
        Value::Bool(b) => J::Bool(*b),
        Value::Num(n) => num_to_json(*n),
        Value::Str(s) => J::from(s.as_str()),
        Value::Mod(m) => mod_to_json(m),
        Value::Table(t) => table_to_json(t),
    }
}

fn table_to_json(t: &Table) -> J {
    if t.fields.is_empty() && !t.array.is_empty() {
        return J::Array(t.array.iter().map(value_to_json).collect());
    }
    let mut m = Map::new();
    for (i, v) in t.array.iter().enumerate() {
        m.insert((i + 1).to_string(), value_to_json(v));
    }
    for (k, v) in &t.fields {
        m.insert(k.clone(), value_to_json(v));
    }
    J::Object(m)
}

/// JSON image of a [`Tag`] (PoB field names, defaults omitted).
#[must_use]
pub fn tag_to_json(t: &Tag) -> J {
    let mut m = Map::new();
    m.insert("type".into(), J::from(t.ty.as_str()));
    macro_rules! opt_str {
        ($($f:ident => $k:literal),* $(,)?) => {$(
            if let Some(v) = &t.$f { m.insert($k.into(), J::from(v.as_str())); }
        )*};
    }
    macro_rules! opt_num {
        ($($f:ident => $k:literal),* $(,)?) => {$(
            if let Some(v) = t.$f { m.insert($k.into(), num_to_json(v)); }
        )*};
    }
    macro_rules! opt_bool {
        ($($f:ident => $k:literal),* $(,)?) => {$(
            if let Some(v) = t.$f { m.insert($k.into(), J::Bool(v)); }
        )*};
    }
    macro_rules! flag {
        ($($f:ident => $k:literal),* $(,)?) => {$(
            if t.$f { m.insert($k.into(), J::Bool(true)); }
        )*};
    }
    macro_rules! str_list {
        ($($f:ident => $k:literal),* $(,)?) => {$(
            if let Some(v) = &t.$f { m.insert($k.into(), strs(v)); }
        )*};
    }
    opt_str!(
        var => "var", actor => "actor", stat => "stat", div_var => "divVar",
        limit_var => "limitVar", limit_stat => "limitStat", limit_actor => "limitActor",
        percent_var => "percentVar", threshold_var => "thresholdVar",
        threshold_stat => "thresholdStat", threshold_actor => "thresholdActor",
        threshold_percent_var => "thresholdPercentVar", item_slot => "itemSlot",
        search_cond => "searchCond", rarity_cond => "rarityCond", name_cond => "nameCond",
        slot_name => "slotName", keyword => "keyword", socket_color => "socketColor",
        skill_name => "skillName", skill_id => "skillId", base_flag => "baseFlag",
        monster_tag => "monsterTag", global_limit_key => "globalLimitKey",
        effect_type => "effectType", effect_name => "effectName", effect_cond => "effectCond",
        effect_enemy_cond => "effectEnemyCond", effect_stack_var => "effectStackVar",
        mod_cond => "modCond",
    );
    opt_num!(
        div => "div", base => "base", limit => "limit", percent => "percent",
        threshold => "threshold", threshold_percent => "thresholdPercent",
        skill_part => "skillPart", global_limit => "globalLimit", num => "num",
        effect_stack_limit => "effectStackLimit",
    );
    opt_bool!(corrupted_cond => "corruptedCond", shaper_cond => "shaperCond", elder_cond => "elderCond");
    flag!(
        neg => "neg", no_floor => "noFloor", invert => "invert", floor => "floor",
        limit_total => "limitTotal", limit_neg_total => "limitNegTotal", upper => "upper",
        equals => "equals", all_slots => "allSlots", both_slots => "bothSlots",
        exclude_self => "excludeSelf", include_transfigured => "includeTransfigured",
        summon_skill => "summonSkill", unscalable => "unscalable",
        allow_totem_buff => "allowTotemBuff", apply_not_player => "applyNotPlayer",
        apply_minions => "applyMinions",
    );
    str_list!(
        var_list => "varList", stat_list => "statList", slot_name_list => "slotNameList",
        skill_name_list => "skillNameList", monster_tag_list => "monsterTagList",
    );
    if let Some(r) = &t.ramp {
        // MeleeProximity keeps PoB's flat `{max, min}`; DistanceRamp rows.
        let flat = t.ty == TagType::MeleeProximity && r.len() == 1;
        m.insert(
            "ramp".into(),
            if flat {
                nums(&r[0])
            } else {
                J::Array(r.iter().map(|row| nums(row)).collect())
            },
        );
    }
    if let Some(s) = &t.sockets {
        m.insert(
            "sockets".into(),
            match s {
                Sockets::All => J::from("all"),
                Sockets::Count(n) => num_to_json(*n),
                Sockets::List(l) => nums(l),
            },
        );
    }
    if let Some(v) = &t.skill_part_list {
        m.insert("skillPartList".into(), nums(v));
    }
    if let Some(v) = t.skill_type {
        m.insert("skillType".into(), J::from(v));
    }
    if let Some(v) = &t.skill_type_list {
        m.insert(
            "skillTypeList".into(),
            J::Array(v.iter().map(|n| J::from(*n)).collect()),
        );
    }
    if t.mod_flags != 0 {
        m.insert("modFlags".into(), J::from(t.mod_flags));
    }
    if t.keyword_flags != 0 {
        m.insert("keywordFlags".into(), J::from(t.keyword_flags));
    }
    for (k, v) in &t.extra {
        m.insert(k.clone(), value_to_json(v));
    }
    J::Object(m)
}

/// JSON image of a [`Mod`].
#[must_use]
pub fn mod_to_json(md: &Mod) -> J {
    let mut m = Map::new();
    m.insert("name".into(), J::from(md.name.as_str()));
    m.insert("type".into(), J::from(md.ty.as_str()));
    m.insert("value".into(), value_to_json(&md.value));
    m.insert("flags".into(), J::from(md.flags));
    m.insert("keywordFlags".into(), J::from(md.keyword_flags));
    if let Some(s) = &md.source {
        m.insert("source".into(), J::from(s.as_str()));
    }
    if let Some(s) = &md.source_slot {
        m.insert("sourceSlot".into(), J::from(s.as_str()));
    }
    m.insert(
        "tags".into(),
        J::Array(md.tags.iter().map(tag_to_json).collect()),
    );
    if md.replaced {
        m.insert("replaced".into(), J::Bool(true));
    }
    if md.converted {
        m.insert("converted".into(), J::Bool(true));
    }
    J::Object(m)
}

// ---- decode ----------------------------------------------------------------

fn json_num(v: &J) -> Option<f64> {
    match v {
        J::Number(n) => n.as_f64(),
        J::Object(m) if m.len() == 1 => match m.get("$num")?.as_str()? {
            "inf" => Some(f64::INFINITY),
            "-inf" => Some(f64::NEG_INFINITY),
            "nan" => Some(f64::NAN),
            _ => None,
        },
        _ => None,
    }
}

/// Lua truthiness of a JSON value (`null` and `false` are false).
fn json_truthy(v: &J) -> bool {
    !matches!(v, J::Null | J::Bool(false))
}

fn json_str(v: &J) -> Option<String> {
    v.as_str().map(str::to_owned)
}

/// Elements of a Lua sequence: a JSON array, or an object with `"1".."n"`.
fn json_seq(v: &J) -> Option<Vec<&J>> {
    match v {
        J::Array(a) => Some(a.iter().collect()),
        J::Object(m) => {
            let mut out = Vec::new();
            while let Some(x) = m.get(&(out.len() + 1).to_string()) {
                out.push(x);
            }
            Some(out)
        }
        _ => None,
    }
}

fn json_strs(v: &J) -> Option<Vec<String>> {
    json_seq(v).map(|s| s.into_iter().filter_map(json_str).collect())
}

fn json_nums(v: &J) -> Option<Vec<f64>> {
    json_seq(v).map(|s| s.into_iter().filter_map(json_num).collect())
}

fn looks_like_mod(m: &Map<String, J>) -> bool {
    m.get("name").is_some_and(J::is_string)
        && m.get("type")
            .and_then(J::as_str)
            .is_some_and(|t| ModType::parse(t).is_some())
}

/// Decode a mod [`Value`] from its JSON image.
pub fn value_from_json(v: &J) -> Result<Value, String> {
    Ok(match v {
        J::Null => Value::Nil,
        J::Bool(b) => Value::Bool(*b),
        J::Number(_) => Value::Num(json_num(v).unwrap_or(0.0)),
        J::String(s) => Value::Str(s.clone()),
        J::Array(a) => Value::Table(Box::new(Table {
            fields: Vec::new(),
            array: a.iter().map(value_from_json).collect::<Result<_, _>>()?,
        })),
        J::Object(m) => {
            if let Some(n) = json_num(v) {
                return Ok(Value::Num(n));
            }
            if looks_like_mod(m) {
                return Ok(Value::Mod(Box::new(mod_from_json(v)?)));
            }
            let mut t = Table::default();
            while let Some(x) = m.get(&(t.array.len() + 1).to_string()) {
                t.array.push(value_from_json(x)?);
            }
            let n = t.array.len();
            for (k, x) in m {
                let in_array = k
                    .parse::<usize>()
                    .is_ok_and(|i| i >= 1 && i <= n && i.to_string() == *k);
                if !in_array {
                    t.fields.push((k.clone(), value_from_json(x)?));
                }
            }
            Value::Table(Box::new(t))
        }
    })
}

/// Decode a [`Tag`] from its JSON image (same field mapping as PoB's Lua
/// tables; see `pob_oracle::mod_parser::lua_to_tag`).
pub fn tag_from_json(v: &J) -> Result<Tag, String> {
    let m = v.as_object().ok_or("tag is not an object")?;
    let ty = m
        .get("type")
        .and_then(J::as_str)
        .map(TagType::parse)
        .ok_or("tag without type")?;
    let mut tag = Tag::new(ty);
    for (k, val) in m {
        let lstr = || json_str(val);
        let lnum = || json_num(val);
        let lbool = || json_truthy(val);
        match k.as_str() {
            "type" => {}
            "var" => tag.var = lstr(),
            "varList" => tag.var_list = json_strs(val),
            "neg" => tag.neg = lbool(),
            "actor" => tag.actor = lstr(),
            "stat" => tag.stat = lstr(),
            "statList" => tag.stat_list = json_strs(val),
            "div" => tag.div = lnum(),
            "divVar" => tag.div_var = lstr(),
            "base" => tag.base = lnum(),
            "noFloor" => tag.no_floor = lbool(),
            "invert" => tag.invert = lbool(),
            "floor" => tag.floor = lbool(),
            "limit" => tag.limit = lnum(),
            "limitVar" => tag.limit_var = lstr(),
            "limitStat" => tag.limit_stat = lstr(),
            "limitTotal" => tag.limit_total = lbool(),
            "limitNegTotal" => tag.limit_neg_total = lbool(),
            "limitActor" => tag.limit_actor = lstr(),
            "percent" => tag.percent = lnum(),
            "percentVar" => tag.percent_var = lstr(),
            "threshold" => tag.threshold = lnum(),
            "thresholdVar" => tag.threshold_var = lstr(),
            "thresholdStat" => tag.threshold_stat = lstr(),
            "thresholdActor" => tag.threshold_actor = lstr(),
            "thresholdPercent" => tag.threshold_percent = lnum(),
            "thresholdPercentVar" => tag.threshold_percent_var = lstr(),
            "upper" => tag.upper = lbool(),
            "equals" => tag.equals = lbool(),
            "ramp" => {
                tag.ramp = json_seq(val).map(|rows| {
                    if rows.iter().all(|x| json_num(x).is_some()) {
                        vec![rows.iter().filter_map(|x| json_num(x)).collect()]
                    } else {
                        rows.iter().filter_map(|r| json_nums(r)).collect()
                    }
                });
            }
            "itemSlot" => tag.item_slot = lstr(),
            "allSlots" => tag.all_slots = lbool(),
            "bothSlots" => tag.both_slots = lbool(),
            "excludeSelf" => tag.exclude_self = lbool(),
            "searchCond" => tag.search_cond = lstr(),
            "rarityCond" => tag.rarity_cond = lstr(),
            "corruptedCond" => tag.corrupted_cond = Some(lbool()),
            "shaperCond" => tag.shaper_cond = Some(lbool()),
            "elderCond" => tag.elder_cond = Some(lbool()),
            "nameCond" => tag.name_cond = lstr(),
            "slotName" => tag.slot_name = lstr(),
            "slotNameList" => tag.slot_name_list = json_strs(val),
            "keyword" => tag.keyword = lstr(),
            "socketColor" => tag.socket_color = lstr(),
            "sockets" => {
                tag.sockets = match val {
                    J::String(s) if s == "all" => Some(Sockets::All),
                    J::Array(_) | J::Object(_) if json_num(val).is_none() => {
                        json_nums(val).map(Sockets::List)
                    }
                    _ => lnum().map(Sockets::Count),
                }
            }
            "skillName" => tag.skill_name = lstr(),
            "skillNameList" => tag.skill_name_list = json_strs(val),
            "includeTransfigured" => tag.include_transfigured = lbool(),
            "summonSkill" => tag.summon_skill = lbool(),
            "skillId" => tag.skill_id = lstr(),
            "skillPart" => tag.skill_part = lnum(),
            "skillPartList" => tag.skill_part_list = json_nums(val),
            "skillType" => tag.skill_type = lnum().map(|n| n as u32),
            "skillTypeList" => {
                tag.skill_type_list =
                    json_nums(val).map(|v| v.into_iter().map(|n| n as u32).collect());
            }
            "baseFlag" => tag.base_flag = lstr(),
            "modFlags" => tag.mod_flags = lnum().unwrap_or(0.0) as u32,
            "keywordFlags" => tag.keyword_flags = lnum().unwrap_or(0.0) as u32,
            "monsterTag" => tag.monster_tag = lstr(),
            "monsterTagList" => tag.monster_tag_list = json_strs(val),
            "globalLimit" => tag.global_limit = lnum(),
            "globalLimitKey" => tag.global_limit_key = lstr(),
            "num" => tag.num = lnum(),
            "effectType" => tag.effect_type = lstr(),
            "effectName" => tag.effect_name = lstr(),
            "effectCond" => tag.effect_cond = lstr(),
            "effectEnemyCond" => tag.effect_enemy_cond = lstr(),
            "effectStackVar" => tag.effect_stack_var = lstr(),
            "effectStackLimit" => tag.effect_stack_limit = lnum(),
            "modCond" => tag.mod_cond = lstr(),
            "unscalable" => tag.unscalable = lbool(),
            "allowTotemBuff" => tag.allow_totem_buff = lbool(),
            "applyNotPlayer" => tag.apply_not_player = lbool(),
            "applyMinions" => tag.apply_minions = lbool(),
            other => tag.extra.push((other.to_owned(), value_from_json(val)?)),
        }
    }
    Ok(tag)
}

/// Decode a [`Mod`] from its JSON image.
pub fn mod_from_json(v: &J) -> Result<Mod, String> {
    let m = v.as_object().ok_or("mod is not an object")?;
    let name = m.get("name").and_then(json_str).ok_or("mod without name")?;
    let ty_s = m
        .get("type")
        .and_then(J::as_str)
        .ok_or("mod without type")?;
    let ty = ModType::parse(ty_s).ok_or_else(|| format!("unknown mod type {ty_s}"))?;
    let value = m.get("value").map_or(Ok(Value::Nil), value_from_json)?;
    let mut md = Mod::new(name, ty, value);
    md.flags = m.get("flags").and_then(json_num).unwrap_or(0.0) as u32;
    md.keyword_flags = m.get("keywordFlags").and_then(json_num).unwrap_or(0.0) as u32;
    md.source = m.get("source").and_then(json_str);
    md.source_slot = m.get("sourceSlot").and_then(json_str);
    md.replaced = m.get("replaced").is_some_and(json_truthy);
    md.converted = m.get("converted").is_some_and(json_truthy);
    if let Some(tags) = m.get("tags").and_then(json_seq) {
        for t in tags {
            if json_truthy(t) {
                md.tags.push(tag_from_json(t)?);
            }
        }
    }
    Ok(md)
}

// ---- serde impls -----------------------------------------------------------

macro_rules! via_json {
    ($ty:ty, $to:ident, $from:ident) => {
        impl Serialize for $ty {
            fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
                $to(self).serialize(s)
            }
        }
        impl<'de> Deserialize<'de> for $ty {
            fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
                let j = J::deserialize(d)?;
                $from(&j).map_err(D::Error::custom)
            }
        }
    };
}

via_json!(Mod, mod_to_json, mod_from_json);
via_json!(Tag, tag_to_json, tag_from_json);
via_json!(Value, value_to_json, value_from_json);

impl Serialize for Table {
    fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        table_to_json(self).serialize(s)
    }
}

impl Serialize for ModType {
    fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        s.serialize_str(self.as_str())
    }
}

impl<'de> Deserialize<'de> for ModType {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        let s = String::deserialize(d)?;
        ModType::parse(&s).ok_or_else(|| D::Error::custom(format!("unknown mod type {s}")))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mod_round_trip() {
        let inner = Mod::new("Damage", ModType::Inc, 5.0).tag(Tag::condition("X"));
        let mut ramp = Tag::new(TagType::MeleeProximity);
        ramp.ramp = Some(vec![vec![1.0, 0.0]]);
        let mut dist = Tag::new(TagType::DistanceRamp);
        dist.ramp = Some(vec![vec![10.0, 1.0], vec![120.0, 0.0]]);
        let mut sock = Tag::new(TagType::SocketedIn);
        sock.slot_name = Some("{SlotName}".into());
        sock.sockets = Some(Sockets::List(vec![1.0, 2.0]));
        sock.extra.push(("weird".into(), Value::Str("x".into())));
        let mut m = Mod::new(
            "ExtraSkillMod",
            ModType::List,
            Value::table(vec![("mod", Value::Mod(Box::new(inner)))]),
        )
        .source("Item:3:Foo")
        .flags(0x40, 0x8)
        .tag(Tag::per_stat("Str", 10.0))
        .tag(ramp)
        .tag(dist)
        .tag(sock);
        m.converted = true;
        let s = serde_json::to_string(&m).unwrap();
        let back: Mod = serde_json::from_str(&s).unwrap();
        assert_eq!(back, m, "{s}");
    }

    #[test]
    fn pob_shapes_decode() {
        let j = r#"{"name":"Life","type":"BASE","value":{"$num":"inf"},"flags":0,
            "keywordFlags":0,"source":"Base","tags":[{"type":"Multiplier","var":"Level","base":38}]}"#;
        let m: Mod = serde_json::from_str(j).unwrap();
        assert_eq!(m.value.num(), Some(f64::INFINITY));
        assert_eq!(m.tags[0].base, Some(38.0));
        let v: Value = serde_json::from_str(r#"{"1":2,"2":3,"key":"x"}"#).unwrap();
        let t = v.as_table().unwrap();
        assert_eq!(t.array, vec![Value::Num(2.0), Value::Num(3.0)]);
        assert_eq!(t.get("key"), Some(&Value::Str("x".into())));
        assert_eq!(
            serde_json::to_value(&v).unwrap(),
            serde_json::json!({"1":2,"2":3,"key":"x"})
        );
    }
}
