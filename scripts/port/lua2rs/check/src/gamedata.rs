//! Stand-in for `pob-gamedata` (calc doc §4.3, §4.4): Mod, ModType, Value, ValueTable, Tag,
//! TagType, Sockets, IntoTagField, `tag!`, `createMod`, `data()`. Field lists are the doc's.
//! The `From`/`IntoTagField` impls are the conversions lua2rs output relies on (CCR-0006).

use pob_lua::Name;
use std::sync::Arc;

pub use crate::global::{KeywordFlag, ModFlag, SkillType};

pub type ModRef = Arc<Mod>;
pub type AStr = Arc<str>;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ModType { BASE, INC, MORE, FLAG, OVERRIDE, LIST, MAX, MIN, Flag, CHANCE, DUMMY }

#[derive(Debug, Clone, PartialEq, Default)]
pub enum Value {
    #[default]
    Nil,
    Bool(bool),
    Num(f64),
    Str(AStr),
    Mod(ModRef),
    Table(Arc<ValueTable>),
}

#[derive(Debug, Clone, PartialEq, Default)]
pub struct ValueTable { pub array: Vec<Value>, pub fields: Vec<(Name, Value)> }

impl From<f64> for Value { fn from(x: f64) -> Self { Value::Num(x) } }
impl From<Option<f64>> for Value { fn from(x: Option<f64>) -> Self { x.map_or(Value::Nil, Value::Num) } }
impl From<bool> for Value { fn from(b: bool) -> Self { Value::Bool(b) } }
impl From<&str> for Value { fn from(s: &str) -> Self { Value::Str(AStr::from(s)) } }
impl From<Name> for Value { fn from(s: Name) -> Self { Value::Str(AStr::from(s.as_str())) } }
impl From<Option<Name>> for Value { fn from(s: Option<Name>) -> Self { s.map_or(Value::Nil, Value::from) } }
impl From<Mod> for Value { fn from(m: Mod) -> Self { Value::Mod(Arc::new(m)) } }

#[derive(Debug, Clone, PartialEq)]
pub struct Mod {
    pub name: Name,
    pub r#type: ModType,
    pub value: Value,
    pub flags: u32,
    pub keywordFlags: u32,
    pub source: Option<AStr>,
    pub tags: Vec<Tag>,
    pub extra: Option<Box<ValueTable>>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum TagType {
    #[default]
    Condition, ActorCondition, Multiplier, MultiplierThreshold, PerStat, PercentStat,
    StatThreshold, DistanceRamp, MeleeProximity, Limit, ItemCondition, SocketedIn, SkillName, SkillId, SkillPart,
    SkillType, BaseFlag, SlotName, ModFlagOr, KeywordFlagAnd, MonsterTag,
    GlobalEffect, InSlot, SlotNumber, DisablesItem, IgnoreCond, Other(Name),
    Global, // CCR-0006
}

#[derive(Debug, Clone, PartialEq, Default)]
pub struct Tag {
    pub r#type: TagType,
    pub var: Option<Name>, pub varList: Option<Arc<[Name]>>, pub neg: bool, pub actor: Option<Name>,
    pub stat: Option<Name>, pub statList: Option<Arc<[Name]>>, pub div: Option<f64>, pub divVar: Option<Name>,
    pub base: Option<f64>, pub noFloor: bool, pub invert: bool, pub floor: bool,
    pub limit: Option<f64>, pub limitVar: Option<Name>, pub limitStat: Option<Name>, pub limitTotal: bool,
    pub limitNegTotal: bool, pub limitActor: Option<Name>, pub percent: Option<f64>, pub percentVar: Option<Name>,
    pub threshold: Option<f64>, pub thresholdVar: Option<Name>, pub thresholdStat: Option<Name>,
    pub thresholdActor: Option<Name>, pub thresholdPercent: Option<f64>, pub thresholdPercentVar: Option<Name>,
    pub upper: bool, pub equals: bool, pub ramp: Option<Arc<[[f64; 2]]>>,
    pub itemSlot: Option<Name>, pub allSlots: bool, pub bothSlots: bool, pub excludeSelf: bool,
    pub searchCond: Option<AStr>, pub rarityCond: Option<Name>, pub corruptedCond: Option<bool>,
    pub shaperCond: Option<bool>, pub elderCond: Option<bool>, pub nameCond: Option<AStr>,
    pub slotName: Option<Name>, pub slotNameList: Option<Arc<[Name]>>, pub keyword: Option<Name>,
    pub socketColor: Option<Name>, pub sockets: Option<Sockets>,
    pub skillName: Option<Name>, pub skillNameList: Option<Arc<[Name]>>, pub includeTransfigured: bool,
    pub summonSkill: bool, pub skillId: Option<Name>, pub skillPart: Option<f64>, pub skillPartList: Option<Arc<[f64]>>,
    pub skillType: Option<SkillType::T>, pub skillTypeList: Option<Arc<[SkillType::T]>>, pub baseFlag: Option<Name>,
    pub modFlags: u32, pub keywordFlags: u32, pub monsterTag: Option<Name>, pub monsterTagList: Option<Arc<[Name]>>,
    pub globalLimit: Option<f64>, pub globalLimitKey: Option<Name>, pub num: Option<f64>,
    pub effectType: Option<Name>, pub effectName: Option<Name>, pub effectCond: Option<Name>,
    pub effectEnemyCond: Option<Name>, pub effectStackVar: Option<Name>, pub effectStackLimit: Option<f64>,
    pub modCond: Option<Name>, pub unscalable: bool, pub allowTotemBuff: bool, pub applyNotPlayer: bool,
    pub applyMinions: bool,
    pub excludeItemType: Option<Name>, // CCR-0006
    pub extra: Option<Box<ValueTable>>,
}

#[derive(Debug, Clone, PartialEq)]
pub enum Sockets { All, Count(f64), List(Arc<[f64]>) }

pub trait IntoTagField<T> { fn into_tag_field(self) -> T; }
impl IntoTagField<Option<Name>> for &str { fn into_tag_field(self) -> Option<Name> { Some(Name::from(self)) } }
impl IntoTagField<Option<Name>> for Name { fn into_tag_field(self) -> Option<Name> { Some(self) } }
impl IntoTagField<Option<Name>> for Option<Name> { fn into_tag_field(self) -> Option<Name> { self } }
impl IntoTagField<Option<AStr>> for &str { fn into_tag_field(self) -> Option<AStr> { Some(AStr::from(self)) } }
impl IntoTagField<Option<f64>> for f64 { fn into_tag_field(self) -> Option<f64> { Some(self) } }
impl IntoTagField<Option<f64>> for Option<f64> { fn into_tag_field(self) -> Option<f64> { self } }
impl IntoTagField<bool> for bool { fn into_tag_field(self) -> bool { self } }
impl IntoTagField<Option<bool>> for bool { fn into_tag_field(self) -> Option<bool> { Some(self) } }
impl IntoTagField<u32> for u32 { fn into_tag_field(self) -> u32 { self } }
impl IntoTagField<Option<SkillType::T>> for SkillType::T { fn into_tag_field(self) -> Option<SkillType::T> { Some(self) } }
impl IntoTagField<Option<Arc<[Name]>>> for &[Name] { fn into_tag_field(self) -> Option<Arc<[Name]>> { Some(Arc::from(self)) } }
impl IntoTagField<Option<Arc<[f64]>>> for &[f64] { fn into_tag_field(self) -> Option<Arc<[f64]>> { Some(Arc::from(self)) } }
impl IntoTagField<Option<Arc<[SkillType::T]>>> for &[SkillType::T] { fn into_tag_field(self) -> Option<Arc<[SkillType::T]>> { Some(Arc::from(self)) } }
impl IntoTagField<Option<Arc<[[f64; 2]]>>> for &[[f64; 2]] { fn into_tag_field(self) -> Option<Arc<[[f64; 2]]>> { Some(Arc::from(self)) } }
impl IntoTagField<Option<Sockets>> for Sockets { fn into_tag_field(self) -> Option<Sockets> { Some(self) } }

/// calc doc §4.3, verbatim shape.
#[macro_export]
macro_rules! tag { ({ type = $t:ident $(, $k:ident = $v:expr)* $(,)? }) => {{
    let mut t = $crate::gamedata::Tag { r#type: $crate::gamedata::TagType::$t, ..Default::default() };
    $( t.$k = $crate::gamedata::IntoTagField::into_tag_field($v); )* t }}; }

/// ModTools.lua `modLib.createMod` with the argument dispatch resolved by the caller (§4.3).
pub fn createMod(modName: Name, modType: ModType, modVal: impl Into<Value>, source: Option<&str>,
                 flags: u32, keywordFlags: u32, tags: &[Tag]) -> Mod {
    Mod { name: modName, r#type: modType, value: modVal.into(), flags, keywordFlags,
          source: source.map(AStr::from), tags: tags.to_vec(), extra: None }
}

pub struct Data {
    pub misc: crate::misc::misc,
    pub characterConstants: crate::misc::characterConstants,
    pub monsterConstants: crate::misc::monsterConstants,
    pub gameConstants: crate::misc::gameConstants,
    pub nonDamagingAilment: std::collections::HashMap<Name, AilmentData>,
}
/// data.nonDamagingAilment[k] (doc §4.4 `AilmentData`; the `default` field is CCR-0006).
pub struct AilmentData { pub default: Option<f64> }
pub fn data() -> &'static Data { unimplemented!("lua2rs compile check only") }
