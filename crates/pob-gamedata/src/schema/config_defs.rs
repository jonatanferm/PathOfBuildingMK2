//! `ConfigOptions` (Modules/ConfigOptions.lua): the option definitions, with
//! the `apply` / `tooltipFunc` closures as [`LuaFnRef`] identities (their Rust
//! bodies are the `CONFIG_APPLY` registry, CO-A/CO-B).
#![allow(non_snake_case)]

use std::fmt;

use serde::de::{self, Deserializer, Visitor};
use serde::Deserialize;

use crate::value::{AStr, Value};
use crate::{LuaFnRef, Name};

/// A key that holds either one string or a list of strings (`ifSkill`,
/// `ifCond`, `ifMod`, `ifStat`); UI code distinguishes the two
/// (`type(x) == "table"`).
#[derive(Debug, Clone, PartialEq)]
pub enum StrOrList {
    Str(Name),
    List(Vec<Name>),
}

impl<'de> Deserialize<'de> for StrOrList {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        struct V;
        impl<'de> Visitor<'de> for V {
            type Value = StrOrList;
            fn expecting(&self, f: &mut fmt::Formatter) -> fmt::Result {
                f.write_str("a string or a list of strings")
            }
            fn visit_str<E: de::Error>(self, s: &str) -> Result<StrOrList, E> {
                Ok(StrOrList::Str(Name::from(s)))
            }
            fn visit_seq<A: de::SeqAccess<'de>>(self, mut a: A) -> Result<StrOrList, A::Error> {
                let mut v = Vec::new();
                while let Some(x) = a.next_element::<Name>()? {
                    v.push(x);
                }
                Ok(StrOrList::List(v))
            }
        }
        d.deserialize_any(V)
    }
}

/// `tooltip`: a string, or (2 options) a function.
#[derive(Debug, Clone, PartialEq)]
pub enum Tooltip {
    Str(AStr),
    Fn(LuaFnRef),
}

impl<'de> Deserialize<'de> for Tooltip {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        struct V;
        impl<'de> Visitor<'de> for V {
            type Value = Tooltip;
            fn expecting(&self, f: &mut fmt::Formatter) -> fmt::Result {
                f.write_str("a tooltip string or function")
            }
            fn visit_str<E: de::Error>(self, s: &str) -> Result<Tooltip, E> {
                Ok(Tooltip::Str(AStr::from(s)))
            }
            fn visit_map<A: de::MapAccess<'de>>(self, a: A) -> Result<Tooltip, A::Error> {
                LuaFnRef::deserialize(de::value::MapAccessDeserializer::new(a)).map(Tooltip::Fn)
            }
        }
        d.deserialize_any(V)
    }
}

/// One `{ label = …, val = … }` of a list option.
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ConfigListItem {
    pub label: AStr,
    pub val: Value,
}

/// One entry of `ConfigOptions` (a section header has only `section`/`col`).
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ConfigOptionDef {
    #[serde(default)]
    pub var: Option<Name>,
    #[serde(default)]
    pub r#type: Option<Name>,
    #[serde(default)]
    pub label: Option<AStr>,
    #[serde(default)]
    pub section: Option<AStr>,
    #[serde(default)]
    pub col: Option<f64>,
    #[serde(default)]
    pub tooltip: Option<Tooltip>,
    #[serde(default)]
    pub tooltipFunc: Option<LuaFnRef>,
    #[serde(default)]
    pub apply: Option<LuaFnRef>,
    #[serde(default)]
    pub list: Option<Vec<ConfigListItem>>,
    #[serde(default)]
    pub defaultIndex: Option<f64>,
    #[serde(default)]
    pub defaultState: Option<Value>,
    #[serde(default)]
    pub defaultPlaceholderState: Option<Value>,
    #[serde(default)]
    pub hideIfInvalid: bool,
    #[serde(default)]
    pub legacy: bool,
    #[serde(default)]
    pub includeTransfigured: bool,
    #[serde(default)]
    pub ifCond: Option<StrOrList>,
    #[serde(default)]
    pub ifCondTrue: Option<Name>,
    #[serde(default)]
    pub ifEnemyCond: Option<Name>,
    #[serde(default)]
    pub ifEnemyMult: Option<Name>,
    #[serde(default)]
    pub ifFlag: Option<Name>,
    #[serde(default)]
    pub ifMinionCond: Option<Name>,
    #[serde(default)]
    pub ifMod: Option<StrOrList>,
    #[serde(default)]
    pub ifMult: Option<Name>,
    #[serde(default)]
    pub ifOption: Option<Name>,
    #[serde(default)]
    pub ifSkill: Option<StrOrList>,
    #[serde(default)]
    pub ifSkillData: Option<Name>,
    #[serde(default)]
    pub ifSkillFlag: Option<Name>,
    #[serde(default)]
    pub ifStat: Option<StrOrList>,
    #[serde(default)]
    pub ifTagType: Option<Name>,
    #[serde(default)]
    pub implyCond: Option<Name>,
    #[serde(default)]
    pub implyCondList: Vec<Name>,
    /// Positional sub-tables (`{ ifCond = …, ifFlag = … }`, 2 options).
    #[serde(rename = "$seq", default)]
    pub positional: Vec<Value>,
}
