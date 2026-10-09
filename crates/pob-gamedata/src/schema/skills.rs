//! `data.skills` (Data/Skills/*.lua after Modules/Data.lua): granted effects,
//! their levels, parts and stat maps (calc doc §4.4). Field lists are the U0
//! census of the dump (contracts.md#schema); every struct is strict.
#![allow(non_snake_case)]

use std::fmt;
use std::sync::Arc;

use serde::de::{self, Deserializer, MapAccess, Visitor};
use serde::Deserialize;

use crate::generated::global::{SkillType, SkillTypeExpr, SkillTypeSet};
use crate::loader;
use crate::modifier::{gen_to_mod, gen_to_value, looks_like_mod, Gen};
use crate::value::{AStr, Value};
use crate::{LuaFnRef, ModRef, Name, NameSet};

pub type GrantedEffectRef = Arc<GrantedEffect>;
pub type SkillLevelRef = Arc<SkillLevel>;

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct GrantedEffect {
    pub id: Name,
    pub name: Name,
    #[serde(default)]
    pub baseTypeName: Option<Name>,
    #[serde(default)]
    pub color: Option<u8>,
    #[serde(default)]
    pub description: Option<AStr>,
    #[serde(default)]
    pub support: bool,
    #[serde(default)]
    pub hidden: bool,
    #[serde(default)]
    pub fromItem: bool,
    #[serde(default)]
    pub fromTree: bool,
    #[serde(default)]
    pub legacy: bool,
    #[serde(default)]
    pub hideFromGemList: bool,
    #[serde(default)]
    pub cannotBeSupported: bool,
    #[serde(default)]
    pub plusVersionOf: Option<Name>,
    #[serde(default)]
    pub isTrigger: bool,
    #[serde(default, deserialize_with = "loader::skill_type_set")]
    pub skillTypes: SkillTypeSet,
    #[serde(default, deserialize_with = "loader::skill_type_set_opt")]
    pub minionSkillTypes: Option<SkillTypeSet>,
    #[serde(default)]
    pub addSkillTypes: Vec<SkillType::T>,
    #[serde(default, deserialize_with = "skill_type_exprs")]
    pub requireSkillTypes: Vec<SkillTypeExpr>,
    #[serde(default, deserialize_with = "skill_type_exprs")]
    pub excludeSkillTypes: Vec<SkillTypeExpr>,
    #[serde(default)]
    pub addMinionTypes: Vec<SkillType::T>,
    #[serde(default)]
    pub ignoreMinionTypes: bool,
    #[serde(default)]
    pub supportGemsOnly: bool,
    #[serde(default, deserialize_with = "loader::name_set_opt")]
    pub weaponTypes: Option<NameSet>,
    #[serde(default = "empty_set", deserialize_with = "loader::name_set")]
    pub baseFlags: NameSet,
    #[serde(default, deserialize_with = "loader::name_set_opt")]
    pub addFlags: Option<NameSet>,
    #[serde(default)]
    pub statDescriptionScope: Option<Name>,
    #[serde(default)]
    pub castTime: Option<f64>,
    #[serde(default)]
    pub baseEffectiveness: Option<f64>,
    #[serde(default)]
    pub incrementalEffectiveness: Option<f64>,
    #[serde(default)]
    pub parts: Vec<SkillPart>,
    #[serde(default)]
    pub baseMods: Vec<ModRef>,
    #[serde(default, deserialize_with = "loader::stat_pairs")]
    pub qualityStats: Vec<(Name, f64)>,
    #[serde(default, deserialize_with = "loader::stat_pairs")]
    pub constantStats: Vec<(Name, f64)>,
    #[serde(default)]
    pub stats: Vec<Name>,
    #[serde(default)]
    pub notMinionStat: Vec<Name>,
    /// 1-based, holes preserved.
    pub levels: LevelTable,
    /// The skill's OWN entries only; use `statMapLookup()` for the
    /// `data.skillStatMap` fallback (Data.lua:1033-1046).
    #[serde(default)]
    pub statMap: StatMap,
    #[serde(default)]
    pub minionList: Vec<Name>,
    #[serde(default)]
    pub addMinionList: Vec<Name>,
    #[serde(default)]
    pub minionHasItemSet: bool,
    #[serde(default, deserialize_with = "loader::name_set_opt")]
    pub minionUses: Option<NameSet>,
    #[serde(default)]
    pub skillTotemId: Option<f64>,
    /// `"Skill:<id>"`.
    pub modSource: AStr,
    #[serde(default)]
    pub hasGlobalEffect: bool,
    #[serde(default)]
    pub flavourText: Vec<AStr>,
    /// `levelMods = {}` on three minion skills (always empty in the dump).
    #[serde(default, deserialize_with = "empty_table")]
    pub levelMods: (),
    // closures: identity only; the implementations live in pob_calc::data_fns
    #[serde(default)]
    pub initialFunc: Option<LuaFnRef>,
    #[serde(default)]
    pub preSkillTypeFunc: Option<LuaFnRef>,
    #[serde(default)]
    pub preDamageFunc: Option<LuaFnRef>,
    #[serde(default)]
    pub postCritFunc: Option<LuaFnRef>,
    #[serde(default)]
    pub explosiveArrowFunc: Option<LuaFnRef>,
    #[serde(default)]
    pub preDotFunc: Option<LuaFnRef>,
}

fn empty_set() -> NameSet {
    Arc::new(ahash::AHashSet::new())
}

fn empty_table<'de, D: Deserializer<'de>>(d: D) -> Result<(), D::Error> {
    let m: indexmap::IndexMap<String, crate::loader::AnyValue> =
        indexmap::IndexMap::deserialize(d)?;
    if m.is_empty() {
        Ok(())
    } else {
        Err(de::Error::custom("expected an empty table"))
    }
}

/// requireSkillTypes / excludeSkillTypes: SkillType numbers, where OR (73),
/// AND (74) and NOT (75) are operators (CCR-0003, formerly 0001-p1, §1).
pub fn skill_type_exprs<'de, D: Deserializer<'de>>(d: D) -> Result<Vec<SkillTypeExpr>, D::Error> {
    let v: Vec<SkillType::T> = Vec::deserialize(d)?;
    Ok(v.into_iter().map(SkillTypeExpr::from_skill_type).collect())
}

/// `levels` with holes: index = level - 1.
#[derive(Debug, Default)]
pub struct LevelTable(pub Vec<Option<SkillLevelRef>>);

impl LevelTable {
    /// `levels[level]` (1-based).
    pub fn get(&self, level: f64) -> Option<&SkillLevelRef> {
        if level < 1.0 || level.fract() != 0.0 {
            return None;
        }
        self.0.get(level as usize - 1).and_then(Option::as_ref)
    }

    /// `#levels` (the border of the sequence part, as `#` sees it).
    pub fn len(&self) -> usize {
        self.0.iter().take_while(|l| l.is_some()).count()
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }
}

impl<'de> Deserialize<'de> for LevelTable {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        struct V;
        impl<'de> Visitor<'de> for V {
            type Value = LevelTable;
            fn expecting(&self, f: &mut fmt::Formatter) -> fmt::Result {
                f.write_str("a level table")
            }
            fn visit_seq<A: de::SeqAccess<'de>>(self, mut a: A) -> Result<LevelTable, A::Error> {
                let mut v = Vec::new();
                while let Some(l) = a.next_element::<SkillLevel>()? {
                    v.push(Some(Arc::new(l)));
                }
                Ok(LevelTable(v))
            }
            fn visit_map<A: MapAccess<'de>>(self, mut a: A) -> Result<LevelTable, A::Error> {
                let mut v: Vec<Option<SkillLevelRef>> = Vec::new();
                while let Some(k) = a.next_key::<String>()? {
                    match k.as_str() {
                        "$seq" => {
                            let s: Vec<SkillLevel> = a.next_value()?;
                            for (i, l) in s.into_iter().enumerate() {
                                if v.len() <= i {
                                    v.resize(i + 1, None);
                                }
                                v[i] = Some(Arc::new(l));
                            }
                        }
                        "$num" => {
                            let m: indexmap::IndexMap<String, SkillLevel> = a.next_value()?;
                            for (k, l) in m {
                                let i: usize = k
                                    .parse()
                                    .map_err(|_| de::Error::custom(format!("level key {k}")))?;
                                if i == 0 {
                                    return Err(de::Error::custom("level 0"));
                                }
                                if v.len() < i {
                                    v.resize(i, None);
                                }
                                v[i - 1] = Some(Arc::new(l));
                            }
                        }
                        other => {
                            return Err(de::Error::custom(format!(
                                "unexpected key {other} in levels"
                            )))
                        }
                    }
                }
                Ok(LevelTable(v))
            }
        }
        d.deserialize_any(V)
    }
}

/// One `levels[n]` table.
#[derive(Debug, Default)]
pub struct SkillLevel {
    /// Positional `[1]..[n]` stat values.
    pub values: Vec<f64>,
    pub levelRequirement: f64,
    pub statInterpolation: Vec<u8>,
    /// `cost = { Mana = 5 }` in dump (bytewise key) order.
    pub cost: Vec<(Name, f64)>,
    /// Every other key, in dump order: manaMultiplier, critChance,
    /// damageEffectiveness, cooldown, storedUses, … (the census list
    /// [`SkillLevel::EXTRA_KEYS`]).
    pub extra: Vec<(Name, Value)>,
}

impl SkillLevel {
    /// The level keys besides values/levelRequirement/statInterpolation/cost
    /// found by the U0 census.
    pub const EXTRA_KEYS: &'static [&'static str] = &[
        "PvPDamageMultiplier",
        "attackSpeedMultiplier",
        "attackTime",
        "baseMultiplier",
        "cooldown",
        "critChance",
        "damageEffectiveness",
        "duration",
        "lifeReservationPercent",
        "manaMultiplier",
        "manaReservationFlat",
        "manaReservationPercent",
        "soulPreventionDuration",
        "storedUses",
        "vaalStoredUses",
    ];

    /// `level[k]` for a non-positional key.
    pub fn get(&self, k: Name) -> &Value {
        static NIL: Value = Value::Nil;
        match k.as_str() {
            "levelRequirement" => {
                // levelRequirement is typed; callers wanting a Value use num()
                &NIL
            }
            _ => self
                .extra
                .iter()
                .find(|(n, _)| *n == k)
                .map_or(&NIL, |(_, v)| v),
        }
    }

    /// `level[k]` as a number (`levelRequirement` included).
    pub fn num(&self, k: Name) -> Option<f64> {
        if k.as_str() == "levelRequirement" {
            return Some(self.levelRequirement);
        }
        self.get(k).num()
    }
}

impl<'de> Deserialize<'de> for SkillLevel {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        struct V;
        impl<'de> Visitor<'de> for V {
            type Value = SkillLevel;
            fn expecting(&self, f: &mut fmt::Formatter) -> fmt::Result {
                f.write_str("a skill level table")
            }
            fn visit_map<A: MapAccess<'de>>(self, mut a: A) -> Result<SkillLevel, A::Error> {
                let mut l = SkillLevel::default();
                while let Some(k) = a.next_key::<String>()? {
                    match k.as_str() {
                        "$seq" => l.values = a.next_value()?,
                        "levelRequirement" => l.levelRequirement = a.next_value()?,
                        "statInterpolation" => l.statInterpolation = a.next_value()?,
                        "cost" => l.cost = a.next_value::<CostMap>()?.0,
                        k if SkillLevel::EXTRA_KEYS.contains(&k) => {
                            let v: Value = a.next_value()?;
                            l.extra.push((Name::from(k), v));
                        }
                        other => {
                            return Err(de::Error::custom(format!(
                                "unknown skill level key {other:?} (extend SkillLevel::EXTRA_KEYS)"
                            )))
                        }
                    }
                }
                Ok(l)
            }
        }
        d.deserialize_map(V)
    }
}

struct CostMap(Vec<(Name, f64)>);
impl<'de> Deserialize<'de> for CostMap {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        loader::name_num_pairs(d).map(CostMap)
    }
}

/// A skill's own `statMap` (the `_grantedEffect` back-reference and the
/// `$meta` = `data.skillStatMapMeta` are dropped; the fallback is
/// `statMapLookup`).
#[derive(Debug, Default)]
pub struct StatMap(pub ahash::AHashMap<Name, Arc<StatMapEntry>>);

impl<'de> Deserialize<'de> for StatMap {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        struct V;
        impl<'de> Visitor<'de> for V {
            type Value = StatMap;
            fn expecting(&self, f: &mut fmt::Formatter) -> fmt::Result {
                f.write_str("a statMap table")
            }
            fn visit_map<A: MapAccess<'de>>(self, mut a: A) -> Result<StatMap, A::Error> {
                let mut m = ahash::AHashMap::new();
                while let Some(k) = a.next_key::<String>()? {
                    match k.as_str() {
                        "_grantedEffect" => {
                            let _: de::IgnoredAny = a.next_value()?;
                        }
                        "$meta" => {
                            // {"$ref": "data.skillStatMapMeta"}: the __index fallback
                            let _: de::IgnoredAny = a.next_value()?;
                        }
                        _ => {
                            let e: StatMapEntry = a.next_value()?;
                            m.insert(Name::from(k.as_str()), Arc::new(e));
                        }
                    }
                }
                Ok(StatMap(m))
            }
        }
        d.deserialize_map(V)
    }
}

/// A `statMap[stat]` table: an array of mods/groups plus `value`/`mult`/`div`/`base`.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct StatMapEntry {
    pub items: Vec<StatMapItem>,
    pub value: Option<Value>,
    pub mult: Option<f64>,
    pub div: Option<f64>,
    pub base: Option<f64>,
    /// `skillFlag = "permanentMinion"` (data.skillStatMap, census addition).
    pub skillFlag: Option<Name>,
}

#[derive(Debug, Clone, PartialEq)]
pub enum StatMapItem {
    Mod(ModRef),
    /// A nested table of mods with its own value/mult/div/base (Lua passes it
    /// to processMod as if it were a mod; CT ports that literally).
    Group {
        mods: Vec<ModRef>,
        value: Option<Value>,
        mult: Option<f64>,
        div: Option<f64>,
        base: Option<f64>,
    },
}

fn gen_f64<E: de::Error>(g: &Gen, k: &str) -> Result<f64, E> {
    match g {
        Gen::Num(n) => Ok(*n),
        _ => Err(E::custom(format!("statMap key {k}: expected a number"))),
    }
}

fn entry_from_gen<E: de::Error>(g: Gen) -> Result<StatMapEntry, E> {
    let mut e = StatMapEntry::default();
    let (items, rest): (Vec<Gen>, Vec<(String, Gen)>) = match g {
        Gen::Seq(s) => (s, Vec::new()),
        Gen::Map(m) => {
            let mut items = Vec::new();
            let mut rest = Vec::new();
            for (k, v) in m {
                if k == "$seq" {
                    if let Gen::Seq(s) = v {
                        items = s;
                    }
                } else {
                    rest.push((k, v));
                }
            }
            (items, rest)
        }
        other => return Err(E::custom(format!("statMap entry: unexpected {other:?}"))),
    };
    for (k, v) in rest {
        match k.as_str() {
            "value" => e.value = Some(gen_to_value(v)?),
            "mult" => e.mult = Some(gen_f64(&v, &k)?),
            "div" => e.div = Some(gen_f64(&v, &k)?),
            "base" => e.base = Some(gen_f64(&v, &k)?),
            "skillFlag" => match v {
                Gen::Str(s) => e.skillFlag = Some(Name::from(s.as_str())),
                _ => return Err(E::custom("skillFlag: expected a string")),
            },
            other => return Err(E::custom(format!("unknown statMap entry key {other:?}"))),
        }
    }
    for it in items {
        e.items.push(item_from_gen(it)?);
    }
    Ok(e)
}

fn item_from_gen<E: de::Error>(g: Gen) -> Result<StatMapItem, E> {
    match g {
        Gen::Map(m) if looks_like_mod(&m) => Ok(StatMapItem::Mod(Arc::new(gen_to_mod(m)?))),
        other => {
            let inner = entry_from_gen::<E>(other)?;
            let mut mods = Vec::new();
            for i in inner.items {
                match i {
                    StatMapItem::Mod(m) => mods.push(m),
                    StatMapItem::Group { .. } => return Err(E::custom("nested statMap groups")),
                }
            }
            Ok(StatMapItem::Group {
                mods,
                value: inner.value,
                mult: inner.mult,
                div: inner.div,
                base: inner.base,
            })
        }
    }
}

impl<'de> Deserialize<'de> for StatMapEntry {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        entry_from_gen(Gen::deserialize(d)?)
    }
}

/// One `parts[i]`: its name and the boolean skill flags it sets/clears
/// (CalcActiveSkill.lua:257-263), in dump order.
#[derive(Debug, Clone, PartialEq)]
pub struct SkillPart {
    pub name: AStr,
    pub flags: Vec<(Name, bool)>,
}

impl<'de> Deserialize<'de> for SkillPart {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        struct V;
        impl<'de> Visitor<'de> for V {
            type Value = SkillPart;
            fn expecting(&self, f: &mut fmt::Formatter) -> fmt::Result {
                f.write_str("a skill part")
            }
            fn visit_map<A: MapAccess<'de>>(self, mut a: A) -> Result<SkillPart, A::Error> {
                let mut name = None;
                let mut flags = Vec::new();
                while let Some(k) = a.next_key::<String>()? {
                    if k == "name" {
                        name = Some(AStr::from(a.next_value::<String>()?));
                    } else if k.starts_with('$') {
                        return Err(de::Error::custom(format!("unexpected {k} in a skill part")));
                    } else {
                        flags.push((Name::from(k.as_str()), a.next_value::<bool>()?));
                    }
                }
                Ok(SkillPart {
                    name: name.ok_or_else(|| de::Error::custom("part without name"))?,
                    flags,
                })
            }
        }
        d.deserialize_map(V)
    }
}
