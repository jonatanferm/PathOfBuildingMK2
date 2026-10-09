//! `data.mapMods` (Data/ModMap.lua): map affix definitions; the `apply`
//! closures are [`LuaFnRef`] identities (Rust bodies: the `MODMAP_APPLY`
//! registry, CO-C).
#![allow(non_snake_case)]

use serde::Deserialize;

use crate::value::{AStr, Value};
use crate::{LuaFnRef, Name};

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ModMapAffix {
    #[serde(default)]
    pub r#type: Option<Name>,
    #[serde(default)]
    pub label: Option<AStr>,
    #[serde(default)]
    pub tooltip: Option<AStr>,
    #[serde(default)]
    pub tooltipLines: Vec<AStr>,
    #[serde(default)]
    pub values: Vec<Value>,
    #[serde(default)]
    pub apply: Option<LuaFnRef>,
}

/// One `{ label = …, val = … }` of the Prefix/Suffix dropdown lists.
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ModMapListItem {
    pub label: AStr,
    pub val: Name,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ModMapDefs {
    pub AffixData: indexmap::IndexMap<Name, ModMapAffix>,
    pub Prefix: Vec<ModMapListItem>,
    pub Suffix: Vec<ModMapListItem>,
}
