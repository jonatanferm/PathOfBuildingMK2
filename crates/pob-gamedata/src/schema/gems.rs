//! `data.gems` (Data/Gems.lua after Modules/Data.lua, incl. the 49 Vaal
//! AltX/AltY gems Data.lua synthesises; 871 entries).
#![allow(non_snake_case)]

use std::sync::Arc;

use serde::Deserialize;

use super::skills::GrantedEffectRef;
use crate::loader;
use crate::value::AStr;
use crate::{Name, NameSet};

pub type GemDataRef = Arc<GemData>;

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct GemData {
    pub id: Name,
    pub name: Name,
    pub gameId: Name,
    pub variantId: Name,
    #[serde(default)]
    pub baseTypeName: Option<Name>,
    pub grantedEffectId: Name,
    #[serde(deserialize_with = "loader::shared")]
    pub grantedEffect: GrantedEffectRef,
    #[serde(default)]
    pub secondaryGrantedEffectId: Option<Name>,
    #[serde(default, deserialize_with = "loader::shared_opt")]
    pub secondaryGrantedEffect: Option<GrantedEffectRef>,
    #[serde(default)]
    pub secondaryEffectName: Option<Name>,
    #[serde(deserialize_with = "loader::shared_vec")]
    pub grantedEffectList: Vec<GrantedEffectRef>,
    #[serde(default)]
    pub vaalGem: bool,
    #[serde(deserialize_with = "loader::name_set")]
    pub tags: NameSet,
    pub tagString: AStr,
    pub reqStr: f64,
    pub reqDex: f64,
    pub reqInt: f64,
    pub naturalMaxLevel: f64,
}
