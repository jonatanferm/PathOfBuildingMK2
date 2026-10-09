//! `data.itemBases` (Data/Bases/*.lua) and the small item tables the calc
//! reads: weaponTypeInfo, unarmedWeaponData, costs.
#![allow(non_snake_case)]

use std::sync::Arc;

use serde::Deserialize;

use crate::loader;
use crate::value::AStr;
use crate::{Name, NameSet};

pub type ItemBaseRef = Arc<ItemBase>;

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ItemBase {
    pub r#type: Name,
    #[serde(default)]
    pub subType: Option<Name>,
    #[serde(default)]
    pub hidden: bool,
    #[serde(default)]
    pub socketLimit: Option<f64>,
    #[serde(deserialize_with = "loader::name_set")]
    pub tags: NameSet,
    #[serde(default)]
    pub influenceTags: Option<indexmap::IndexMap<Name, Name>>,
    #[serde(default)]
    pub implicit: Option<AStr>,
    #[serde(default)]
    pub implicitIds: Vec<Name>,
    /// One list of mod types per implicit line (`{}` for none).
    #[serde(default)]
    pub implicitModTypes: Vec<Vec<Name>>,
    #[serde(default)]
    pub enchant: Option<AStr>,
    #[serde(default)]
    pub enchantIds: Vec<Name>,
    #[serde(default)]
    pub enchantModTypes: Vec<Vec<Name>>,
    #[serde(default)]
    pub cannotBeAnointed: bool,
    #[serde(default)]
    pub flavourText: Vec<AStr>,
    #[serde(default)]
    pub weapon: Option<WeaponBase>,
    #[serde(default)]
    pub armour: Option<ArmourBase>,
    #[serde(default)]
    pub flask: Option<FlaskBase>,
    #[serde(default)]
    pub tincture: Option<TinctureBase>,
    pub req: BaseReq,
}

#[derive(Debug, Clone, Copy, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WeaponBase {
    pub PhysicalMin: f64,
    pub PhysicalMax: f64,
    pub CritChanceBase: f64,
    pub AttackRateBase: f64,
    pub Range: f64,
}

#[derive(Debug, Clone, Copy, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ArmourBase {
    #[serde(default)]
    pub ArmourBaseMin: Option<f64>,
    #[serde(default)]
    pub ArmourBaseMax: Option<f64>,
    #[serde(default)]
    pub EvasionBaseMin: Option<f64>,
    #[serde(default)]
    pub EvasionBaseMax: Option<f64>,
    #[serde(default)]
    pub EnergyShieldBaseMin: Option<f64>,
    #[serde(default)]
    pub EnergyShieldBaseMax: Option<f64>,
    #[serde(default)]
    pub WardBaseMin: Option<f64>,
    #[serde(default)]
    pub WardBaseMax: Option<f64>,
    #[serde(default)]
    pub BlockChance: Option<f64>,
    #[serde(default)]
    pub MovementPenalty: Option<f64>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FlaskBase {
    #[serde(default)]
    pub life: Option<f64>,
    #[serde(default)]
    pub mana: Option<f64>,
    pub duration: f64,
    pub chargesUsed: f64,
    pub chargesMax: f64,
    #[serde(default)]
    pub buff: Vec<AStr>,
}

#[derive(Debug, Clone, Copy, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TinctureBase {
    pub manaBurn: f64,
    pub cooldown: f64,
}

#[derive(Debug, Clone, Copy, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BaseReq {
    #[serde(default)]
    pub level: Option<f64>,
    #[serde(default)]
    pub str: Option<f64>,
    #[serde(default)]
    pub dex: Option<f64>,
    #[serde(default)]
    pub int: Option<f64>,
}

/// `data.weaponTypeInfo[type]` (Modules/Data.lua).
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WeaponTypeInfo {
    pub oneHand: bool,
    pub melee: bool,
    pub flag: Name,
    #[serde(default)]
    pub label: Option<Name>,
}

/// `data.unarmedWeaponData[classId]` (index = class id + 1; Lua indexes it
/// with `[0..6]`, the dump has the 0 key under `$num`).
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct UnarmedWeaponData {
    pub r#type: Name,
    pub AttackRate: f64,
    pub CritChance: f64,
    pub PhysicalMin: f64,
    pub PhysicalMax: f64,
}

/// One `data.costs` entry (Data/Costs.lua).
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CostInfo {
    pub Resource: Name,
    pub Stat: Name,
    pub ResourceString: AStr,
    pub Divisor: f64,
}

/// `data.costs`: the array plus the `__index` that aliases `costs[Resource]`
/// (Data.lua:722-726).
#[derive(Debug, Default)]
pub struct Costs {
    pub list: Vec<CostInfo>,
}

impl Costs {
    /// `data.costs[resource]` via the `__index` metamethod (Data.lua:722-726).
    pub fn by_resource(&self, r: Name) -> Option<&CostInfo> {
        self.list.iter().find(|c| c.Resource == r)
    }
}

impl<'de> Deserialize<'de> for Costs {
    fn deserialize<D: serde::Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        #[derive(Deserialize)]
        #[serde(deny_unknown_fields)]
        struct Raw {
            #[serde(rename = "$seq")]
            seq: Vec<CostInfo>,
            #[serde(rename = "$meta")]
            #[allow(dead_code)]
            meta: MetaIndex,
        }
        #[derive(Deserialize)]
        #[serde(deny_unknown_fields)]
        struct MetaIndex {
            #[allow(dead_code)]
            __index: crate::LuaFnRef,
        }
        let r = Raw::deserialize(d)?;
        Ok(Costs { list: r.seq })
    }
}
