//! The small calc-relevant tables of `data` (Modules/Data.lua, Data/Misc.lua,
//! Data/Bosses.lua, Data/Pantheons.lua, Data/ClusterJewels.lua): strict
//! records. The verbatim-key constant records (`data.misc`,
//! `characterConstants`, …) are P1's generated structs (`generated::misc`).
#![allow(non_snake_case)]

use serde::Deserialize;

use crate::loader;
use crate::value::{AStr, Value};
use crate::Name;

/// `data.nonDamagingAilment[name]` (Modules/Data.lua).
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AilmentData {
    pub associatedType: Name,
    pub alt: bool,
    #[serde(default)]
    pub default: Option<f64>,
    pub min: f64,
    pub max: f64,
    pub precision: f64,
    #[serde(default)]
    pub duration: Option<f64>,
}

/// One `data.jewelRadii[ver][i]`.
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct JewelRadius {
    pub inner: f64,
    pub outer: f64,
    #[serde(default)]
    pub innerSquared: Option<f64>,
    #[serde(default)]
    pub outerSquared: Option<f64>,
    pub col: Name,
    pub label: Name,
}

/// `data.bosses[name]`.
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Boss {
    pub armourMult: f64,
    pub evasionMult: f64,
    pub isUber: bool,
}

/// `data.bossSkills[name]`.
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BossSkill {
    pub DamageType: Name,
    pub DamageMultipliers: indexmap::IndexMap<Name, Vec<f64>>,
    #[serde(default)]
    pub DamagePenetrations: Option<indexmap::IndexMap<Name, Value>>,
    #[serde(default)]
    pub UberDamageMultiplier: Option<f64>,
    #[serde(default)]
    pub UberDamagePenetrations: Option<indexmap::IndexMap<Name, Value>>,
    #[serde(default)]
    pub UberSpeed: Option<f64>,
    pub speed: f64,
    #[serde(default)]
    pub critChance: Option<f64>,
    #[serde(default)]
    pub earlierUber: bool,
    #[serde(default)]
    pub additionalStats: Option<Value>,
    pub tooltip: AStr,
}

/// One `{ label, val }` of `data.bossSkillsList`.
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LabelVal {
    pub label: AStr,
    pub val: Name,
}

/// `data.pantheons[god]`.
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Pantheon {
    pub isMajorGod: bool,
    pub souls: Vec<PantheonSoul>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PantheonSoul {
    pub name: AStr,
    pub mods: Vec<PantheonMod>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PantheonMod {
    pub line: AStr,
    pub value: Vec<f64>,
}

/// `data.clusterJewels`.
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ClusterJewelData {
    pub jewels: indexmap::IndexMap<Name, ClusterJewelSize>,
    pub keystones: Vec<Name>,
    pub notableSortOrder: indexmap::IndexMap<Name, f64>,
    /// `orbitOffsets[nodeId][index] = offset`.
    #[serde(deserialize_with = "orbit_offsets")]
    pub orbitOffsets: std::collections::BTreeMap<i64, std::collections::BTreeMap<i64, f64>>,
}

fn orbit_offsets<'de, D: serde::Deserializer<'de>>(
    d: D,
) -> Result<std::collections::BTreeMap<i64, std::collections::BTreeMap<i64, f64>>, D::Error> {
    struct Inner(std::collections::BTreeMap<i64, f64>);
    impl<'de> Deserialize<'de> for Inner {
        fn deserialize<D: serde::Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
            loader::int_map(d).map(Inner)
        }
    }
    let m: std::collections::BTreeMap<i64, Inner> = loader::int_map(d)?;
    Ok(m.into_iter().map(|(k, v)| (k, v.0)).collect())
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ClusterJewelSize {
    pub size: Name,
    pub sizeIndex: f64,
    pub minNodes: f64,
    pub maxNodes: f64,
    pub smallIndicies: Vec<f64>,
    pub notableIndicies: Vec<f64>,
    pub socketIndicies: Vec<f64>,
    pub totalIndicies: f64,
    pub skills: indexmap::IndexMap<Name, ClusterJewelSkill>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ClusterJewelSkill {
    pub name: AStr,
    pub icon: AStr,
    #[serde(default)]
    pub masteryIcon: Option<AStr>,
    pub tag: Name,
    pub stats: Vec<AStr>,
    pub enchant: Vec<AStr>,
}
