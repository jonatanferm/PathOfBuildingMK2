//! PoB's modifier model (`modLib.createMod` tables) as typed Rust (calc doc §4.3).
//!
//! Adapted from the old `pob-calc/src/modifier.rs` (AMENDMENTS A1): `Mod` is
//! immutable once a store holds it (`ModRef = Arc<Mod>`, §3.6), keys are
//! interned [`Name`]s, and a tag is one struct whose field names are the
//! verbatim Lua keys, so `tag!({ type = Condition, var = "X" })` reads like
//! `{ type = "Condition", var = "X" }`.
//!
//! Serde uses the dump shape (crates/pob-dump/FORMAT.md): a mod is
//! `{"name", "type", "value", "flags", "keywordFlags", "source", "$seq": [tags]}`.
//! Deserialisation is strict: a key no field models is an error (the U0
//! census in docs/port/contracts.md#tag-census lists every key the dump, the
//! ModCache and the parse oracle use). Lua quirks found by the census are kept
//! losslessly in `extra` (contracts.md#mod-quirks):
//!
//! * tags at non-sequence indices (`mod[2] = tag` with `mod[1] = nil`; ipairs
//!   never sees them) are `extra["#2"]`;
//! * `flags` given as a table (two data mods) is `flags = 0` plus
//!   `extra["flags"]` (Lua's `band` raises on it, MS-A reproduces that);
//! * a non-table tag entry (`$seq: [2, 0, {…}]`) is a [`TagType::LuaNonTable`]
//!   tag whose value is `extra["$value"]` (indexing it raises in Lua);
//! * a numeric tag key holding a Lua string (`div = "10"`) has the coerced
//!   number in its field and the original string in `extra` under the same key.
#![allow(non_snake_case, non_camel_case_types)]

use std::fmt;
use std::sync::Arc;

use serde::de::{self, MapAccess, SeqAccess, Visitor};
use serde::ser::{SerializeMap, SerializeSeq};
use serde::{Deserialize, Deserializer, Serialize, Serializer};

use crate::generated::global::SkillType;
use crate::value::{AStr, Value, ValueTable};
use crate::Name;

pub type ModRef = Arc<Mod>; // clone = refcount; immutable once inserted (§3.6)

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum ModType {
    BASE,
    INC,
    MORE,
    FLAG,
    OVERRIDE,
    LIST,
    MAX,
    MIN,
    /// `"Flag"` (only `CanNotUseItem`; not the same as `"FLAG"`).
    Flag,
    CHANCE,
    DUMMY,
    /// `type = nil` (a data bug: sup_str.lua:1448 `mod("CorruptingCryStagesFromExerted", nil, …)`);
    /// equals no queried type, as in Lua.
    Nil,
}

impl ModType {
    pub fn parse(s: &str) -> Option<Self> {
        Some(match s {
            "BASE" => Self::BASE,
            "INC" => Self::INC,
            "MORE" => Self::MORE,
            "FLAG" => Self::FLAG,
            "OVERRIDE" => Self::OVERRIDE,
            "LIST" => Self::LIST,
            "MAX" => Self::MAX,
            "MIN" => Self::MIN,
            "Flag" => Self::Flag,
            "CHANCE" => Self::CHANCE,
            "DUMMY" => Self::DUMMY,
            _ => return None,
        })
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::BASE => "BASE",
            Self::INC => "INC",
            Self::MORE => "MORE",
            Self::FLAG => "FLAG",
            Self::OVERRIDE => "OVERRIDE",
            Self::LIST => "LIST",
            Self::MAX => "MAX",
            Self::MIN => "MIN",
            Self::Flag => "Flag",
            Self::CHANCE => "CHANCE",
            Self::DUMMY => "DUMMY",
            Self::Nil => "nil",
        }
    }
}

impl fmt::Display for ModType {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// A ModParser jewel closure as data: which jewelFuncList entry (or
/// specialModList closure) it is, plus the captured upvalues (calc doc §4.3).
/// The table `{ type = …, radius = …, func = function … }` it replaces keeps
/// its other keys here.
#[derive(Debug, Clone, PartialEq)]
pub struct JewelFuncSpec {
    /// Lowercased jewelFuncList key, `"$default"` (ModParser.lua 6552-6595), or
    /// the defining closure's `"<file>:<a>-<b>"` for closures that are not
    /// jewelFuncList entries and for parse-oracle records (MP7 maps these).
    pub key: Name,
    pub kind: JewelFuncKind,
    /// Captures, as passed by the defining entry.
    pub args: smallvec::SmallVec<[Value; 4]>,
    /// `{ type = "Self"/"SelfUnalloc"/"Threshold"/"Other" }`.
    pub r#type: Option<Name>,
    /// `{ radius = "Large" }` (ExtraJewelFunc entries, ModParser.lua:2622/2630).
    pub radius: Option<Name>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum JewelFuncKind {
    Other,
    Self_,
    SelfUnalloc,
    Threshold,
}

impl JewelFuncKind {
    pub fn from_type(t: Option<&str>) -> Self {
        match t {
            Some("Self") => Self::Self_,
            Some("SelfUnalloc") => Self::SelfUnalloc,
            Some("Threshold") => Self::Threshold,
            _ => Self::Other,
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct Mod {
    pub name: Name,
    pub r#type: ModType,
    pub value: Value,
    pub flags: u32,
    pub keywordFlags: u32,
    pub source: Option<AStr>,
    /// Lua `mod[1], mod[2], …` (the part `ipairs` sees).
    pub tags: smallvec::SmallVec<[Tag; 1]>,
    /// Rare non-standard keys (e.g. `sourceSlot`) and the quirks listed in the
    /// module docs; keeps round-trips lossless.
    pub extra: Option<Box<ValueTable>>,
}

impl Mod {
    /// `mod.source:match("[^:]+")`.
    pub fn source_category(&self) -> Option<&str> {
        // "[^:]+": the first run of non-':' characters
        let s = self.source.as_deref()?.trim_start_matches(':');
        if s.is_empty() {
            return None;
        }
        Some(&s[..s.find(':').unwrap_or(s.len())])
    }

    /// The value of an `extra` key, if any.
    pub fn extra_get(&self, k: Name) -> Option<&Value> {
        let v = self.extra.as_ref()?.get(k);
        (!v.is_nil()).then_some(v)
    }
}

/// The tag `type`s. The first 21 are EvalMod's branches; the rest are the other
/// types found by the U0 census (contracts.md#tag-census).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum TagType {
    #[default]
    Condition,
    ActorCondition,
    Multiplier,
    MultiplierThreshold,
    PerStat,
    PercentStat,
    StatThreshold,
    DistanceRamp,
    MeleeProximity,
    Limit,
    ItemCondition,
    SocketedIn,
    SkillName,
    SkillId,
    SkillPart,
    SkillType,
    BaseFlag,
    SlotName,
    ModFlagOr,
    KeywordFlagAnd,
    MonsterTag, // the 21 EvalMod branches
    GlobalEffect,
    InSlot,
    SlotNumber,
    DisablesItem,
    IgnoreCond,
    /// `{ type = "Global" }` (1252 census hits).
    Global,
    /// `{ type = "Skill", skillPartList = … }` (2 census hits).
    Skill,
    /// A tag entry that is not a table (`$seq: [2, 0, {…}]` in skillStatMap
    /// `spell_damage_+%_per_10_int`); the raw value is `extra["$value"]`.
    LuaNonTable,
    Other(Name),
}

impl TagType {
    pub fn parse(s: &str) -> Self {
        match s {
            "Condition" => Self::Condition,
            "ActorCondition" => Self::ActorCondition,
            "Multiplier" => Self::Multiplier,
            "MultiplierThreshold" => Self::MultiplierThreshold,
            "PerStat" => Self::PerStat,
            "PercentStat" => Self::PercentStat,
            "StatThreshold" => Self::StatThreshold,
            "DistanceRamp" => Self::DistanceRamp,
            "MeleeProximity" => Self::MeleeProximity,
            "Limit" => Self::Limit,
            "ItemCondition" => Self::ItemCondition,
            "SocketedIn" => Self::SocketedIn,
            "SkillName" => Self::SkillName,
            "SkillId" => Self::SkillId,
            "SkillPart" => Self::SkillPart,
            "SkillType" => Self::SkillType,
            "BaseFlag" => Self::BaseFlag,
            "SlotName" => Self::SlotName,
            "ModFlagOr" => Self::ModFlagOr,
            "KeywordFlagAnd" => Self::KeywordFlagAnd,
            "MonsterTag" => Self::MonsterTag,
            "GlobalEffect" => Self::GlobalEffect,
            "InSlot" => Self::InSlot,
            "SlotNumber" => Self::SlotNumber,
            "DisablesItem" => Self::DisablesItem,
            "IgnoreCond" => Self::IgnoreCond,
            "Global" => Self::Global,
            "Skill" => Self::Skill,
            other => Self::Other(Name::from(other)),
        }
    }

    pub fn as_str(&self) -> &str {
        match self {
            Self::Condition => "Condition",
            Self::ActorCondition => "ActorCondition",
            Self::Multiplier => "Multiplier",
            Self::MultiplierThreshold => "MultiplierThreshold",
            Self::PerStat => "PerStat",
            Self::PercentStat => "PercentStat",
            Self::StatThreshold => "StatThreshold",
            Self::DistanceRamp => "DistanceRamp",
            Self::MeleeProximity => "MeleeProximity",
            Self::Limit => "Limit",
            Self::ItemCondition => "ItemCondition",
            Self::SocketedIn => "SocketedIn",
            Self::SkillName => "SkillName",
            Self::SkillId => "SkillId",
            Self::SkillPart => "SkillPart",
            Self::SkillType => "SkillType",
            Self::BaseFlag => "BaseFlag",
            Self::SlotName => "SlotName",
            Self::ModFlagOr => "ModFlagOr",
            Self::KeywordFlagAnd => "KeywordFlagAnd",
            Self::MonsterTag => "MonsterTag",
            Self::GlobalEffect => "GlobalEffect",
            Self::InSlot => "InSlot",
            Self::SlotNumber => "SlotNumber",
            Self::DisablesItem => "DisablesItem",
            Self::IgnoreCond => "IgnoreCond",
            Self::Global => "Global",
            Self::Skill => "Skill",
            Self::LuaNonTable => "$nonTable",
            Self::Other(n) => n.as_str(),
        }
    }
}

/// One PoB tag table. Field names are the verbatim Lua keys. `Option`/`false`
/// = the key is absent (nil).
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Tag {
    pub r#type: TagType,
    pub var: Option<Name>,
    pub varList: Option<Arc<[Name]>>,
    pub neg: bool,
    pub actor: Option<Name>,
    pub stat: Option<Name>,
    pub statList: Option<Arc<[Name]>>,
    pub div: Option<f64>,
    pub divVar: Option<Name>,
    pub base: Option<f64>,
    pub noFloor: bool,
    pub invert: bool,
    pub floor: bool,
    pub limit: Option<f64>,
    pub limitVar: Option<Name>,
    pub limitStat: Option<Name>,
    pub limitTotal: bool,
    pub limitNegTotal: bool,
    pub limitActor: Option<Name>,
    pub percent: Option<f64>,
    pub percentVar: Option<Name>,
    pub threshold: Option<f64>,
    pub thresholdVar: Option<Name>,
    pub thresholdStat: Option<Name>,
    pub thresholdActor: Option<Name>,
    pub thresholdPercent: Option<f64>,
    pub thresholdPercentVar: Option<Name>,
    pub upper: bool,
    pub equals: bool,
    pub ramp: Option<Ramp>,
    pub itemSlot: Option<Name>,
    pub allSlots: bool,
    pub bothSlots: bool,
    pub excludeSelf: bool,
    pub searchCond: Option<AStr>,
    pub rarityCond: Option<Name>,
    pub corruptedCond: Option<bool>,
    pub shaperCond: Option<bool>,
    pub elderCond: Option<bool>,
    pub nameCond: Option<AStr>,
    pub slotName: Option<Name>,
    pub slotNameList: Option<Arc<[Name]>>,
    pub keyword: Option<Name>,
    pub socketColor: Option<Name>,
    pub sockets: Option<Sockets>,
    pub skillName: Option<Name>,
    pub skillNameList: Option<Arc<[Name]>>,
    pub includeTransfigured: bool,
    pub summonSkill: bool,
    pub skillId: Option<Name>,
    pub skillPart: Option<f64>,
    pub skillPartList: Option<Arc<[f64]>>,
    pub skillType: Option<SkillType::T>,
    pub skillTypeList: Option<Arc<[SkillType::T]>>,
    pub baseFlag: Option<Name>,
    pub modFlags: u32,
    pub keywordFlags: u32,
    pub monsterTag: Option<Name>,
    pub monsterTagList: Option<Arc<[Name]>>,
    pub globalLimit: Option<f64>,
    pub globalLimitKey: Option<Name>,
    pub num: Option<f64>,
    pub effectType: Option<Name>,
    pub effectName: Option<Name>,
    pub effectCond: Option<Name>,
    pub effectEnemyCond: Option<Name>,
    pub effectStackVar: Option<Name>,
    pub effectStackLimit: Option<f64>,
    pub modCond: Option<Name>,
    pub unscalable: bool,
    pub allowTotemBuff: bool,
    pub applyNotPlayer: bool,
    pub applyMinions: bool,
    /// `{ type = "DisablesItem", excludeItemType = "Tincture" }` (census addition).
    pub excludeItemType: Option<Name>,
    /// Any other key, and the quirks in the module docs.
    pub extra: Option<Box<ValueTable>>,
}

impl Tag {
    pub fn new(t: TagType) -> Self {
        Tag {
            r#type: t,
            ..Default::default()
        }
    }

    pub fn extra_get(&self, k: Name) -> Option<&Value> {
        let v = self.extra.as_ref()?.get(k);
        (!v.is_nil()).then_some(v)
    }
}

/// `tag.ramp`: DistanceRamp's list of `{ distance, multiplier }` points, or
/// MeleeProximity's flat `{ max, min }` pair (census: both shapes occur).
#[derive(Debug, Clone, PartialEq)]
pub enum Ramp {
    Points(Arc<[[f64; 2]]>),
    Flat(Arc<[f64]>),
}

/// `tag.sockets`: `"all"`, a count, or a list of socket indices.
#[derive(Debug, Clone, PartialEq)]
pub enum Sockets {
    All,
    Count(f64),
    List(Arc<[f64]>),
}

/// `tag!({ type = Condition, var = "X", neg = true })`.
#[macro_export]
macro_rules! tag {
    ({ type = $t:ident $(, $k:ident = $v:expr)* $(,)? }) => {{
        #[allow(unused_mut)]
        let mut t = $crate::Tag { r#type: $crate::TagType::$t, ..Default::default() };
        $( t.$k = $crate::modifier::IntoTagField::into_tag_field($v); )*
        t
    }};
}

/// Conversions used by `tag!` (`&str` → `Option<Name>`, `f64` → `Option<f64>`, …).
pub trait IntoTagField<T> {
    fn into_tag_field(self) -> T;
}
impl IntoTagField<Option<Name>> for &str {
    fn into_tag_field(self) -> Option<Name> {
        Some(Name::from(self))
    }
}
impl IntoTagField<Option<Name>> for Name {
    fn into_tag_field(self) -> Option<Name> {
        Some(self)
    }
}
impl IntoTagField<Option<Name>> for Option<Name> {
    fn into_tag_field(self) -> Option<Name> {
        self
    }
}
impl IntoTagField<Option<AStr>> for &str {
    fn into_tag_field(self) -> Option<AStr> {
        Some(AStr::from(self))
    }
}
impl IntoTagField<Option<f64>> for f64 {
    fn into_tag_field(self) -> Option<f64> {
        Some(self)
    }
}
impl IntoTagField<Option<f64>> for i32 {
    fn into_tag_field(self) -> Option<f64> {
        Some(f64::from(self))
    }
}
impl IntoTagField<Option<f64>> for Option<f64> {
    fn into_tag_field(self) -> Option<f64> {
        self
    }
}
impl IntoTagField<bool> for bool {
    fn into_tag_field(self) -> bool {
        self
    }
}
impl IntoTagField<Option<bool>> for bool {
    fn into_tag_field(self) -> Option<bool> {
        Some(self)
    }
}
impl IntoTagField<u32> for u32 {
    fn into_tag_field(self) -> u32 {
        self
    }
}
impl IntoTagField<Option<u8>> for u8 {
    fn into_tag_field(self) -> Option<u8> {
        Some(self)
    }
}
impl IntoTagField<Option<Sockets>> for Sockets {
    fn into_tag_field(self) -> Option<Sockets> {
        Some(self)
    }
}
impl IntoTagField<Option<Arc<[Name]>>> for &[&str] {
    fn into_tag_field(self) -> Option<Arc<[Name]>> {
        Some(self.iter().map(|s| Name::from(s)).collect())
    }
}
impl<const N: usize> IntoTagField<Option<Arc<[Name]>>> for [&str; N] {
    fn into_tag_field(self) -> Option<Arc<[Name]>> {
        Some(self.iter().map(|s| Name::from(s)).collect())
    }
}
impl IntoTagField<Option<Arc<[Name]>>> for &[Name] {
    fn into_tag_field(self) -> Option<Arc<[Name]>> {
        Some(self.into())
    }
}
impl IntoTagField<Option<Arc<[Name]>>> for Vec<Name> {
    fn into_tag_field(self) -> Option<Arc<[Name]>> {
        Some(self.into())
    }
}
impl IntoTagField<Option<Arc<[f64]>>> for &[f64] {
    fn into_tag_field(self) -> Option<Arc<[f64]>> {
        Some(self.into())
    }
}
impl IntoTagField<Option<Arc<[u8]>>> for &[u8] {
    fn into_tag_field(self) -> Option<Arc<[u8]>> {
        Some(self.into())
    }
}
impl IntoTagField<Option<Ramp>> for &[[f64; 2]] {
    fn into_tag_field(self) -> Option<Ramp> {
        Some(Ramp::Points(self.into()))
    }
}
impl IntoTagField<Option<Ramp>> for Ramp {
    fn into_tag_field(self) -> Option<Ramp> {
        Some(self)
    }
}

// ---------------------------------------------------------------------------
// Serde (dump shape)
// ---------------------------------------------------------------------------

/// Generic decoded Lua value used while deserialising mods, tags and values
/// (the dump's JSON shape: `$seq` / `$num` / `$float` members).
#[derive(Debug, Clone)]
pub(crate) enum Gen {
    Nil,
    Bool(bool),
    Num(f64),
    Str(String),
    Seq(Vec<Gen>),
    Map(Vec<(String, Gen)>),
}

impl<'de> Deserialize<'de> for Gen {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        struct V;
        impl<'de> Visitor<'de> for V {
            type Value = Gen;
            fn expecting(&self, f: &mut fmt::Formatter) -> fmt::Result {
                f.write_str("a Lua value")
            }
            fn visit_unit<E>(self) -> Result<Gen, E> {
                Ok(Gen::Nil)
            }
            fn visit_none<E>(self) -> Result<Gen, E> {
                Ok(Gen::Nil)
            }
            fn visit_some<D: Deserializer<'de>>(self, d: D) -> Result<Gen, D::Error> {
                Gen::deserialize(d)
            }
            fn visit_bool<E>(self, b: bool) -> Result<Gen, E> {
                Ok(Gen::Bool(b))
            }
            fn visit_i64<E>(self, n: i64) -> Result<Gen, E> {
                Ok(Gen::Num(n as f64))
            }
            fn visit_u64<E>(self, n: u64) -> Result<Gen, E> {
                Ok(Gen::Num(n as f64))
            }
            fn visit_f64<E>(self, n: f64) -> Result<Gen, E> {
                Ok(Gen::Num(n))
            }
            fn visit_str<E>(self, s: &str) -> Result<Gen, E> {
                Ok(Gen::Str(s.to_owned()))
            }
            fn visit_string<E>(self, s: String) -> Result<Gen, E> {
                Ok(Gen::Str(s))
            }
            fn visit_seq<A: SeqAccess<'de>>(self, mut a: A) -> Result<Gen, A::Error> {
                let mut v = Vec::new();
                while let Some(x) = a.next_element::<Gen>()? {
                    v.push(x);
                }
                Ok(Gen::Seq(v))
            }
            fn visit_map<A: MapAccess<'de>>(self, mut a: A) -> Result<Gen, A::Error> {
                let mut v = Vec::new();
                while let Some(k) = a.next_key::<String>()? {
                    let x = a.next_value::<Gen>()?;
                    v.push((k, x));
                }
                // {"$float": "-0"} etc.
                if v.len() == 1 && v[0].0 == "$float" {
                    if let Gen::Str(s) = &v[0].1 {
                        return Ok(Gen::Num(
                            parse_float_marker(s).ok_or_else(|| de::Error::custom("bad $float"))?,
                        ));
                    }
                }
                Ok(Gen::Map(v))
            }
        }
        d.deserialize_any(V)
    }
}

pub(crate) fn parse_float_marker(s: &str) -> Option<f64> {
    Some(match s {
        "inf" => f64::INFINITY,
        "-inf" => f64::NEG_INFINITY,
        "nan" => f64::NAN,
        "-0" => -0.0,
        _ => return None,
    })
}

fn gen_seq_and_num(map: &[(String, Gen)]) -> (Vec<Gen>, Vec<(f64, Gen)>) {
    let mut seq = Vec::new();
    let mut num = Vec::new();
    for (k, v) in map {
        match (k.as_str(), v) {
            ("$seq", Gen::Seq(s)) => seq.clone_from(s),
            ("$num", Gen::Map(m)) => {
                for (nk, nv) in m {
                    num.push((pob_lua::tonumber(nk).unwrap_or(f64::NAN), nv.clone()));
                }
            }
            _ => {}
        }
    }
    (seq, num)
}

pub(crate) fn looks_like_mod(m: &[(String, Gen)]) -> bool {
    let has = |k: &str| m.iter().any(|(n, _)| n == k);
    has("name")
        && has("flags")
        && has("keywordFlags")
        && m.iter().all(|(n, _)| {
            matches!(
                n.as_str(),
                "name" | "type" | "value" | "flags" | "keywordFlags" | "source" | "$seq" | "$num"
            )
        })
}

pub(crate) fn gen_to_value<E: de::Error>(g: Gen) -> Result<Value, E> {
    Ok(match g {
        Gen::Nil => Value::Nil,
        Gen::Bool(b) => Value::Bool(b),
        Gen::Num(n) => Value::Num(n),
        Gen::Str(s) => Value::Str(AStr::from(s)),
        Gen::Seq(s) => {
            let array = s
                .into_iter()
                .map(gen_to_value)
                .collect::<Result<Vec<_>, E>>()?;
            Value::Table(Arc::new(ValueTable {
                array,
                fields: Vec::new(),
            }))
        }
        Gen::Map(m) => {
            if m.iter().any(|(k, _)| k == "$fn") {
                return Err(E::custom(
                    "a function where a Lua value was expected (only jewel funcs carry closures)",
                ));
            }
            if looks_like_mod(&m) {
                return Ok(Value::Mod(Arc::new(gen_to_mod(m)?)));
            }
            // { type = …, func = function … } → JewelFunc
            if let Some((_, Gen::Map(f))) = m.iter().find(|(k, _)| k == "func") {
                if f.iter().any(|(k, _)| k == "$fn") {
                    return gen_to_jewel_func(&m).map(|j| Value::JewelFunc(Arc::new(j)));
                }
            }
            let (seq, num) = gen_seq_and_num(&m);
            let mut t = ValueTable::new();
            for v in seq {
                t.array.push(gen_to_value(v)?);
            }
            for (k, v) in m {
                if k == "$seq" || k == "$num" {
                    continue;
                }
                if k.starts_with('$') {
                    return Err(E::custom(format!(
                        "unsupported member {k} in a mod value table"
                    )));
                }
                t.fields.push((Name::from(k.as_str()), gen_to_value(v)?));
            }
            for (n, v) in num {
                t.fields.push((
                    Name::from(format!("#{}", pob_lua::tostring_num(n)).as_str()),
                    gen_to_value(v)?,
                ));
            }
            Value::Table(Arc::new(t))
        }
    })
}

fn gen_to_jewel_func<E: de::Error>(m: &[(String, Gen)]) -> Result<JewelFuncSpec, E> {
    let mut r#type = None;
    let mut radius = None;
    let mut key = None;
    for (k, v) in m {
        match (k.as_str(), v) {
            ("type", Gen::Str(s)) => r#type = Some(Name::from(s.as_str())),
            ("radius", Gen::Str(s)) => radius = Some(Name::from(s.as_str())),
            ("func", Gen::Map(f)) => {
                for (fk, fv) in f {
                    if let ("src", Gen::Str(s)) = (fk.as_str(), fv) {
                        key = Some(Name::from(s.as_str()));
                    }
                }
            }
            _ => {
                return Err(E::custom(format!(
                    "unexpected key {k} in a jewel func table"
                )))
            }
        }
    }
    Ok(JewelFuncSpec {
        key: key.ok_or_else(|| E::custom("jewel func without src"))?,
        kind: JewelFuncKind::from_type(r#type.as_ref().map(|n| n.as_str())),
        args: smallvec::SmallVec::new(),
        r#type,
        radius,
    })
}

fn gen_num<E: de::Error>(g: &Gen, what: &str) -> Result<(f64, Option<AStr>), E> {
    match g {
        Gen::Num(n) => Ok((*n, None)),
        Gen::Str(s) => pob_lua::tonumber(s)
            .map(|n| (n, Some(AStr::from(s.as_str()))))
            .ok_or_else(|| E::custom(format!("tag key {what}: string {s:?} is not a number"))),
        _ => Err(E::custom(format!("tag key {what}: expected a number"))),
    }
}

fn gen_name<E: de::Error>(g: &Gen, what: &str) -> Result<Name, E> {
    match g {
        Gen::Str(s) => Ok(Name::from(s.as_str())),
        _ => Err(E::custom(format!(
            "tag key {what}: expected a string, got {g:?}"
        ))),
    }
}

fn gen_astr<E: de::Error>(g: &Gen, what: &str) -> Result<AStr, E> {
    gen_name::<E>(g, what).map(|n| AStr::from(n.as_str()))
}

fn gen_bool<E: de::Error>(g: &Gen, what: &str) -> Result<bool, E> {
    match g {
        Gen::Bool(b) => Ok(*b),
        _ => Err(E::custom(format!(
            "tag key {what}: expected a boolean, got {g:?}"
        ))),
    }
}

fn gen_list<E: de::Error>(g: &Gen, what: &str) -> Result<Vec<Gen>, E> {
    match g {
        Gen::Seq(s) => Ok(s.clone()),
        Gen::Map(m) if m.is_empty() => Ok(Vec::new()),
        _ => Err(E::custom(format!(
            "tag key {what}: expected a list, got {g:?}"
        ))),
    }
}

fn gen_to_tag<E: de::Error>(g: Gen) -> Result<Tag, E> {
    let m = match g {
        Gen::Map(m) => m,
        other => {
            let mut t = Tag::new(TagType::LuaNonTable);
            let mut ex = ValueTable::new();
            ex.fields
                .push((pob_lua::n!("$value"), gen_to_value(other)?));
            t.extra = Some(Box::new(ex));
            return Ok(t);
        }
    };
    let mut t = Tag::default();
    let mut extra = ValueTable::new();
    let mut saw_type = false;
    macro_rules! num {
        ($field:ident, $v:expr, $k:expr) => {{
            let (n, s) = gen_num::<E>($v, $k)?;
            t.$field = Some(n);
            if let Some(s) = s {
                extra.fields.push((Name::from($k), Value::Str(s)));
            }
        }};
    }
    for (k, v) in &m {
        let key = k.as_str();
        match key {
            "type" => {
                saw_type = true;
                t.r#type = TagType::parse(&gen_name::<E>(v, key)?);
            }
            "var" => t.var = Some(gen_name(v, key)?),
            "varList" => {
                t.varList = Some(
                    gen_list::<E>(v, key)?
                        .iter()
                        .map(|x| gen_name(x, key))
                        .collect::<Result<_, E>>()?,
                )
            }
            "neg" => t.neg = gen_bool(v, key)?,
            "actor" => t.actor = Some(gen_name(v, key)?),
            "stat" => t.stat = Some(gen_name(v, key)?),
            "statList" => {
                t.statList = Some(
                    gen_list::<E>(v, key)?
                        .iter()
                        .map(|x| gen_name(x, key))
                        .collect::<Result<_, E>>()?,
                )
            }
            "div" => num!(div, v, key),
            "divVar" => t.divVar = Some(gen_name(v, key)?),
            "base" => num!(base, v, key),
            "noFloor" => t.noFloor = gen_bool(v, key)?,
            "invert" => t.invert = gen_bool(v, key)?,
            "floor" => t.floor = gen_bool(v, key)?,
            "limit" => num!(limit, v, key),
            "limitVar" => t.limitVar = Some(gen_name(v, key)?),
            "limitStat" => t.limitStat = Some(gen_name(v, key)?),
            "limitTotal" => t.limitTotal = gen_bool(v, key)?,
            "limitNegTotal" => t.limitNegTotal = gen_bool(v, key)?,
            "limitActor" => t.limitActor = Some(gen_name(v, key)?),
            "percent" => num!(percent, v, key),
            "percentVar" => t.percentVar = Some(gen_name(v, key)?),
            "threshold" => num!(threshold, v, key),
            "thresholdVar" => t.thresholdVar = Some(gen_name(v, key)?),
            "thresholdStat" => t.thresholdStat = Some(gen_name(v, key)?),
            "thresholdActor" => t.thresholdActor = Some(gen_name(v, key)?),
            "thresholdPercent" => num!(thresholdPercent, v, key),
            "thresholdPercentVar" => t.thresholdPercentVar = Some(gen_name(v, key)?),
            "upper" => t.upper = gen_bool(v, key)?,
            "equals" => t.equals = gen_bool(v, key)?,
            "ramp" => {
                let l = gen_list::<E>(v, key)?;
                if l.iter().all(|x| matches!(x, Gen::Num(_))) {
                    t.ramp = Some(Ramp::Flat(
                        l.iter()
                            .map(|x| gen_num::<E>(x, key).map(|p| p.0))
                            .collect::<Result<_, E>>()?,
                    ));
                } else {
                    let mut r = Vec::new();
                    for p in l {
                        let p = gen_list::<E>(&p, key)?;
                        if p.len() != 2 {
                            return Err(E::custom("ramp points are pairs"));
                        }
                        r.push([gen_num::<E>(&p[0], key)?.0, gen_num::<E>(&p[1], key)?.0]);
                    }
                    t.ramp = Some(Ramp::Points(r.into()));
                }
            }
            "itemSlot" => t.itemSlot = Some(gen_name(v, key)?),
            "allSlots" => t.allSlots = gen_bool(v, key)?,
            "bothSlots" => t.bothSlots = gen_bool(v, key)?,
            "excludeSelf" => t.excludeSelf = gen_bool(v, key)?,
            "searchCond" => t.searchCond = Some(gen_astr(v, key)?),
            "rarityCond" => t.rarityCond = Some(gen_name(v, key)?),
            "corruptedCond" => t.corruptedCond = Some(gen_bool(v, key)?),
            "shaperCond" => t.shaperCond = Some(gen_bool(v, key)?),
            "elderCond" => t.elderCond = Some(gen_bool(v, key)?),
            "nameCond" => t.nameCond = Some(gen_astr(v, key)?),
            "slotName" => t.slotName = Some(gen_name(v, key)?),
            "slotNameList" => {
                t.slotNameList = Some(
                    gen_list::<E>(v, key)?
                        .iter()
                        .map(|x| gen_name(x, key))
                        .collect::<Result<_, E>>()?,
                )
            }
            "keyword" => t.keyword = Some(gen_name(v, key)?),
            "socketColor" => t.socketColor = Some(gen_name(v, key)?),
            "sockets" => {
                t.sockets = Some(match v {
                    Gen::Str(s) if s == "all" => Sockets::All,
                    Gen::Num(n) => Sockets::Count(*n),
                    other => Sockets::List(
                        gen_list::<E>(other, key)?
                            .iter()
                            .map(|x| gen_num::<E>(x, key).map(|p| p.0))
                            .collect::<Result<_, E>>()?,
                    ),
                });
            }
            "skillName" => t.skillName = Some(gen_name(v, key)?),
            "skillNameList" => {
                t.skillNameList = Some(
                    gen_list::<E>(v, key)?
                        .iter()
                        .map(|x| gen_name(x, key))
                        .collect::<Result<_, E>>()?,
                )
            }
            "includeTransfigured" => t.includeTransfigured = gen_bool(v, key)?,
            "summonSkill" => t.summonSkill = gen_bool(v, key)?,
            "skillId" => t.skillId = Some(gen_name(v, key)?),
            "skillPart" => num!(skillPart, v, key),
            "skillPartList" => {
                t.skillPartList = Some(
                    gen_list::<E>(v, key)?
                        .iter()
                        .map(|x| gen_num::<E>(x, key).map(|p| p.0))
                        .collect::<Result<_, E>>()?,
                );
            }
            "skillType" => t.skillType = Some(gen_num::<E>(v, key)?.0 as SkillType::T),
            "skillTypeList" => {
                t.skillTypeList = Some(
                    gen_list::<E>(v, key)?
                        .iter()
                        .map(|x| gen_num::<E>(x, key).map(|p| p.0 as SkillType::T))
                        .collect::<Result<_, E>>()?,
                );
            }
            "baseFlag" => t.baseFlag = Some(gen_name(v, key)?),
            "modFlags" => t.modFlags = gen_num::<E>(v, key)?.0 as u32,
            "keywordFlags" => t.keywordFlags = gen_num::<E>(v, key)?.0 as u32,
            "monsterTag" => t.monsterTag = Some(gen_name(v, key)?),
            "monsterTagList" => {
                t.monsterTagList = Some(
                    gen_list::<E>(v, key)?
                        .iter()
                        .map(|x| gen_name(x, key))
                        .collect::<Result<_, E>>()?,
                )
            }
            "globalLimit" => num!(globalLimit, v, key),
            "globalLimitKey" => t.globalLimitKey = Some(gen_name(v, key)?),
            "num" => num!(num, v, key),
            "effectType" => t.effectType = Some(gen_name(v, key)?),
            "effectName" => t.effectName = Some(gen_name(v, key)?),
            "effectCond" => t.effectCond = Some(gen_name(v, key)?),
            "effectEnemyCond" => t.effectEnemyCond = Some(gen_name(v, key)?),
            "effectStackVar" => t.effectStackVar = Some(gen_name(v, key)?),
            "effectStackLimit" => num!(effectStackLimit, v, key),
            "modCond" => t.modCond = Some(gen_name(v, key)?),
            "unscalable" => t.unscalable = gen_bool(v, key)?,
            "allowTotemBuff" => t.allowTotemBuff = gen_bool(v, key)?,
            "applyNotPlayer" => t.applyNotPlayer = gen_bool(v, key)?,
            "applyMinions" => t.applyMinions = gen_bool(v, key)?,
            "excludeItemType" => t.excludeItemType = Some(gen_name(v, key)?),
            _ => {
                return Err(E::custom(format!(
                    "unknown tag key {key:?} (extend the census, contracts.md#tag-census)"
                )))
            }
        }
    }
    if !saw_type {
        // a tag table without `type` (never in the census): keep it as Other("")
        t.r#type = TagType::Other(Name::from(""));
    }
    if !extra.is_empty() {
        t.extra = Some(Box::new(extra));
    }
    Ok(t)
}

pub(crate) fn gen_to_mod<E: de::Error>(m: Vec<(String, Gen)>) -> Result<Mod, E> {
    let mut name = None;
    let mut ty = None;
    let mut value = Value::Nil;
    let mut flags = 0u32;
    let mut keywordFlags = 0u32;
    let mut source = None;
    let mut tags = smallvec::SmallVec::new();
    let mut extra = ValueTable::new();
    for (k, v) in m {
        match k.as_str() {
            "name" => name = Some(gen_name::<E>(&v, "name")?),
            "type" => {
                let s = gen_name::<E>(&v, "type")?;
                ty = Some(
                    ModType::parse(&s).ok_or_else(|| E::custom(format!("unknown mod type {s}")))?,
                );
            }
            "value" => value = gen_to_value(v)?,
            "flags" => match v {
                Gen::Num(n) => flags = n as u32,
                other => extra
                    .fields
                    .push((pob_lua::n!("flags"), gen_to_value(other)?)),
            },
            "keywordFlags" => keywordFlags = gen_num::<E>(&v, "keywordFlags")?.0 as u32,
            "source" => source = Some(AStr::from(gen_name::<E>(&v, "source")?.as_str())),
            "$seq" => {
                for t in gen_list::<E>(&v, "$seq")? {
                    tags.push(gen_to_tag(t)?);
                }
            }
            "$num" => {
                if let Gen::Map(nm) = v {
                    for (nk, nv) in nm {
                        let tag = gen_to_tag::<E>(nv)?;
                        extra.fields.push((
                            Name::from(format!("#{nk}").as_str()),
                            Value::Table(Arc::new(tag_to_value_table(&tag))),
                        ));
                    }
                }
            }
            other => return Err(E::custom(format!("unknown mod key {other:?}"))),
        }
    }
    Ok(Mod {
        name: name.ok_or_else(|| E::custom("mod without name"))?,
        r#type: ty.unwrap_or(ModType::Nil),
        value,
        flags,
        keywordFlags,
        source,
        tags,
        extra: (!extra.is_empty()).then(|| Box::new(extra)),
    })
}

/// A tag as a plain Lua table value (used for holey tags kept in `extra`).
pub fn tag_to_value_table(t: &Tag) -> ValueTable {
    let mut vt = ValueTable::new();
    for_each_tag_field(t, |k, v| vt.fields.push((Name::from(k), v)));
    vt
}

/// Calls `f(key, value)` for every non-nil key of a tag, `type` first, then in
/// field order, then `extra`.
pub fn for_each_tag_field(t: &Tag, mut f: impl FnMut(&str, Value)) {
    if t.r#type != TagType::LuaNonTable {
        f("type", Value::from(t.r#type.as_str()));
    }
    let names = |l: &Arc<[Name]>| {
        Value::Table(Arc::new(ValueTable {
            array: l.iter().map(|n| Value::from(*n)).collect(),
            fields: Vec::new(),
        }))
    };
    let nums = |l: &[f64]| {
        Value::Table(Arc::new(ValueTable {
            array: l.iter().map(|n| Value::Num(*n)).collect(),
            fields: Vec::new(),
        }))
    };
    macro_rules! o {
        ($k:ident) => {
            if let Some(v) = &t.$k {
                f(stringify!($k), Value::from(*v));
            }
        };
    }
    macro_rules! s {
        ($k:ident) => {
            if let Some(v) = &t.$k {
                f(stringify!($k), Value::Str(v.clone()));
            }
        };
    }
    macro_rules! b {
        ($k:ident) => {
            if t.$k {
                f(stringify!($k), Value::Bool(true));
            }
        };
    }
    macro_rules! ob {
        ($k:ident) => {
            if let Some(v) = t.$k {
                f(stringify!($k), Value::Bool(v));
            }
        };
    }
    macro_rules! l {
        ($k:ident) => {
            if let Some(v) = &t.$k {
                f(stringify!($k), names(v));
            }
        };
    }
    o!(var);
    l!(varList);
    b!(neg);
    o!(actor);
    o!(stat);
    l!(statList);
    o!(div);
    o!(divVar);
    o!(base);
    b!(noFloor);
    b!(invert);
    b!(floor);
    o!(limit);
    o!(limitVar);
    o!(limitStat);
    b!(limitTotal);
    b!(limitNegTotal);
    o!(limitActor);
    o!(percent);
    o!(percentVar);
    o!(threshold);
    o!(thresholdVar);
    o!(thresholdStat);
    o!(thresholdActor);
    o!(thresholdPercent);
    o!(thresholdPercentVar);
    b!(upper);
    b!(equals);
    match &t.ramp {
        Some(Ramp::Points(r)) => f(
            "ramp",
            Value::Table(Arc::new(ValueTable {
                array: r.iter().map(|p| nums(&p[..])).collect(),
                fields: Vec::new(),
            })),
        ),
        Some(Ramp::Flat(r)) => f("ramp", nums(r)),
        None => {}
    }
    o!(itemSlot);
    b!(allSlots);
    b!(bothSlots);
    b!(excludeSelf);
    s!(searchCond);
    o!(rarityCond);
    ob!(corruptedCond);
    ob!(shaperCond);
    ob!(elderCond);
    s!(nameCond);
    o!(slotName);
    l!(slotNameList);
    o!(keyword);
    o!(socketColor);
    match &t.sockets {
        Some(Sockets::All) => f("sockets", Value::from("all")),
        Some(Sockets::Count(n)) => f("sockets", Value::Num(*n)),
        Some(Sockets::List(l)) => f("sockets", nums(l)),
        None => {}
    }
    o!(skillName);
    l!(skillNameList);
    b!(includeTransfigured);
    b!(summonSkill);
    o!(skillId);
    o!(skillPart);
    if let Some(l) = &t.skillPartList {
        f("skillPartList", nums(l));
    }
    if let Some(v) = t.skillType {
        f("skillType", Value::Num(f64::from(v)));
    }
    if let Some(l) = &t.skillTypeList {
        f(
            "skillTypeList",
            nums(&l.iter().map(|x| f64::from(*x)).collect::<Vec<_>>()),
        );
    }
    o!(baseFlag);
    if t.modFlags != 0 {
        f("modFlags", Value::Num(f64::from(t.modFlags)));
    }
    if t.keywordFlags != 0 {
        f("keywordFlags", Value::Num(f64::from(t.keywordFlags)));
    }
    o!(monsterTag);
    l!(monsterTagList);
    o!(globalLimit);
    o!(globalLimitKey);
    o!(num);
    o!(effectType);
    o!(effectName);
    o!(effectCond);
    o!(effectEnemyCond);
    o!(effectStackVar);
    o!(effectStackLimit);
    o!(modCond);
    b!(unscalable);
    b!(allowTotemBuff);
    b!(applyNotPlayer);
    b!(applyMinions);
    o!(excludeItemType);
    if let Some(ex) = &t.extra {
        for (k, v) in &ex.fields {
            f(k.as_str(), v.clone());
        }
    }
}

impl<'de> Deserialize<'de> for Mod {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        match Gen::deserialize(d)? {
            Gen::Map(m) => gen_to_mod(m),
            other => Err(de::Error::custom(format!(
                "expected a mod table, got {other:?}"
            ))),
        }
    }
}

impl<'de> Deserialize<'de> for Tag {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        gen_to_tag(Gen::deserialize(d)?)
    }
}

impl<'de> Deserialize<'de> for Value {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        gen_to_value(Gen::deserialize(d)?)
    }
}

// ---- Serialize (dump shape) ----

struct Num(f64);
impl Serialize for Num {
    fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        let n = self.0;
        if n.is_finite() && !(n == 0.0 && n.is_sign_negative()) {
            if n.fract() == 0.0 && n.abs() < 9.007_199_254_740_992e15 {
                s.serialize_i64(n as i64)
            } else {
                s.serialize_f64(n)
            }
        } else {
            let mut m = s.serialize_map(Some(1))?;
            let t = if n.is_nan() {
                "nan"
            } else if n == 0.0 {
                "-0"
            } else if n > 0.0 {
                "inf"
            } else {
                "-inf"
            };
            m.serialize_entry("$float", t)?;
            m.end()
        }
    }
}

impl Serialize for Value {
    fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        match self {
            Value::Nil => s.serialize_unit(),
            Value::Bool(b) => s.serialize_bool(*b),
            Value::Num(n) => Num(*n).serialize(s),
            Value::Str(x) => s.serialize_str(x),
            Value::Mod(m) => m.serialize(s),
            Value::Table(t) => t.serialize(s),
            Value::JewelFunc(j) => {
                let mut m = s.serialize_map(None)?;
                struct F<'a>(&'a JewelFuncSpec);
                impl Serialize for F<'_> {
                    fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
                        let mut m = s.serialize_map(None)?;
                        m.serialize_entry("$jewelFunc", self.0.key.as_str())?;
                        if !self.0.args.is_empty() {
                            m.serialize_entry("args", &self.0.args[..])?;
                        }
                        m.end()
                    }
                }
                m.serialize_entry("func", &F(j))?;
                if let Some(r) = &j.radius {
                    m.serialize_entry("radius", r.as_str())?;
                }
                if let Some(t) = &j.r#type {
                    m.serialize_entry("type", t.as_str())?;
                }
                m.end()
            }
        }
    }
}

impl Serialize for ValueTable {
    fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        if self.fields.is_empty() && !self.array.is_empty() {
            let mut q = s.serialize_seq(Some(self.array.len()))?;
            for v in &self.array {
                q.serialize_element(v)?;
            }
            return q.end();
        }
        let mut m = s.serialize_map(None)?;
        for (k, v) in &self.fields {
            m.serialize_entry(k.as_str(), v)?;
        }
        if !self.array.is_empty() {
            m.serialize_entry("$seq", &self.array)?;
        }
        m.end()
    }
}

impl Serialize for Tag {
    fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        if self.r#type == TagType::LuaNonTable {
            return self
                .extra_get(pob_lua::n!("$value"))
                .cloned()
                .unwrap_or_default()
                .serialize(s);
        }
        let mut entries = Vec::new();
        for_each_tag_field(self, |k, v| entries.push((k.to_owned(), v)));
        let mut m = s.serialize_map(Some(entries.len()))?;
        for (k, v) in &entries {
            m.serialize_entry(k, v)?;
        }
        m.end()
    }
}

impl Serialize for Mod {
    fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        let mut m = s.serialize_map(None)?;
        m.serialize_entry("name", self.name.as_str())?;
        if self.r#type != ModType::Nil {
            m.serialize_entry("type", self.r#type.as_str())?;
        }
        if !self.value.is_nil() {
            m.serialize_entry("value", &self.value)?;
        }
        match self.extra_get(pob_lua::n!("flags")) {
            Some(raw) => m.serialize_entry("flags", raw)?,
            None => m.serialize_entry("flags", &self.flags)?,
        }
        m.serialize_entry("keywordFlags", &self.keywordFlags)?;
        if let Some(src) = &self.source {
            m.serialize_entry("source", &**src)?;
        }
        if !self.tags.is_empty() {
            m.serialize_entry("$seq", &self.tags[..])?;
        }
        if let Some(ex) = &self.extra {
            // holey tags `#n` go back under `$num`; other keys under their own name
            let holes: Vec<(&str, &Value)> = ex
                .fields
                .iter()
                .filter_map(|(k, v)| k.as_str().strip_prefix('#').map(|n| (n, v)))
                .collect();
            if !holes.is_empty() {
                struct H<'a>(Vec<(&'a str, &'a Value)>);
                impl Serialize for H<'_> {
                    fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
                        let mut m = s.serialize_map(Some(self.0.len()))?;
                        for (k, v) in &self.0 {
                            m.serialize_entry(k, v)?;
                        }
                        m.end()
                    }
                }
                m.serialize_entry("$num", &H(holes))?;
            }
            for (k, v) in &ex.fields {
                if k.as_str() != "flags" && !k.as_str().starts_with('#') {
                    m.serialize_entry(k.as_str(), v)?;
                }
            }
        }
        m.end()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mod_round_trip_dump_shape() {
        let j = r#"{"flags":0,"keywordFlags":0,"name":"Life","source":"Tree:1","type":"INC","value":10,
                   "$seq":[{"type":"Condition","var":"FullLife"},{"div":"10","stat":"Str","type":"PerStat"}]}"#;
        let m: Mod = serde_json::from_str(j).unwrap();
        assert_eq!(m.name.as_str(), "Life");
        assert_eq!(m.r#type, ModType::INC);
        assert_eq!(m.value, Value::Num(10.0));
        assert_eq!(m.tags.len(), 2);
        assert_eq!(m.tags[0].r#type, TagType::Condition);
        assert_eq!(m.tags[1].div, Some(10.0));
        assert_eq!(
            m.tags[1]
                .extra_get(pob_lua::n!("div"))
                .and_then(Value::as_str),
            Some("10")
        );
        let back = serde_json::to_value(&m).unwrap();
        assert_eq!(back["$seq"][0]["var"], "FullLife");
        let t = tag!({ type = Condition, var = "FullLife", neg = true });
        assert_eq!(t.var.unwrap().as_str(), "FullLife");
        assert!(t.neg);
    }

    #[test]
    fn quirks() {
        // holey tags, table flags, non-table tags
        let m: Mod = serde_json::from_str(
            r#"{"flags":0,"keywordFlags":0,"name":"X","type":"BASE","value":1,"$num":{"2":{"type":"Condition","var":"A"}}}"#,
        )
        .unwrap();
        assert!(m.tags.is_empty());
        assert!(m.extra_get(Name::from("#2")).is_some());
        let m: Mod = serde_json::from_str(r#"{"flags":{"type":"Condition","var":"L"},"keywordFlags":0,"name":"X","type":"BASE","value":1}"#).unwrap();
        assert!(m.extra_get(pob_lua::n!("flags")).is_some());
        let m: Mod = serde_json::from_str(
            r#"{"flags":0,"keywordFlags":0,"name":"SkillData","type":"LIST","value":{"key":"Damage"},"$seq":[2,0,{"div":10,"stat":"Int","type":"PerStat"}]}"#,
        )
        .unwrap();
        assert_eq!(m.tags[0].r#type, TagType::LuaNonTable);
        assert_eq!(m.tags[2].r#type, TagType::PerStat);
        assert_eq!(m.value.get(pob_lua::n!("key")).as_str(), Some("Damage"));
        // {"$float":"-0"} values
        let m: Mod = serde_json::from_str(
            r#"{"flags":0,"keywordFlags":0,"name":"X","type":"INC","value":{"$float":"-0"}}"#,
        )
        .unwrap();
        assert!(m.value.num().unwrap().is_sign_negative());
    }

    #[test]
    fn source_category() {
        let m = Mod {
            name: Name::from("X"),
            r#type: ModType::BASE,
            value: Value::Nil,
            flags: 0,
            keywordFlags: 0,
            source: Some(AStr::from("Item:5:Foo")),
            tags: smallvec::SmallVec::new(),
            extra: None,
        };
        assert_eq!(m.source_category(), Some("Item"));
    }
}
