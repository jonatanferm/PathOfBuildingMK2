//! `data.minions` (Data/Minions.lua + Data/Spectres.lua) and `data.spectres`
//! (references into `data.minions`).
#![allow(non_snake_case)]

use std::sync::Arc;

use serde::Deserialize;

use crate::{ModRef, Name};

pub type MinionDataRef = Arc<MinionData>;

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MinionData {
    pub name: Name,
    /// `None` = Lua nil (`data.minions.GuardianRelicAll` has no `monsterTags`; CCR-0015 §3).
    #[serde(default)]
    pub monsterTags: Option<Vec<Name>>,
    pub life: f64,
    #[serde(default)]
    pub lifeScaling: Option<Name>,
    #[serde(default)]
    pub energyShield: Option<f64>,
    #[serde(default)]
    pub armour: Option<f64>,
    #[serde(default)]
    pub evasion: Option<f64>,
    pub fireResist: f64,
    pub coldResist: f64,
    pub lightningResist: f64,
    pub chaosResist: f64,
    pub damage: f64,
    pub damageSpread: f64,
    #[serde(default)]
    pub damageFixup: Option<f64>,
    pub attackTime: f64,
    pub attackRange: f64,
    pub accuracy: f64,
    #[serde(default)]
    pub baseDamageIgnoresAttackSpeed: bool,
    #[serde(default)]
    pub hostile: bool,
    #[serde(default)]
    pub limit: Option<Name>,
    #[serde(default)]
    pub weaponType1: Option<Name>,
    #[serde(default)]
    pub weaponType2: Option<Name>,
    pub skillList: Vec<Name>,
    #[serde(default)]
    pub modList: Vec<ModRef>,
}
