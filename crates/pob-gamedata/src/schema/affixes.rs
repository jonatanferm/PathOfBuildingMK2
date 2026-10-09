//! Affix tables: `data.itemMods.<category>`, `data.masterMods`,
//! `data.veiledMods`, `data.necropolisMods`, `data.uniqueMods`
//! (Data/Mod*.lua). One strict struct covers every table (the union of their
//! keys, U0 census).
#![allow(non_snake_case)]

use std::sync::Arc;

use serde::Deserialize;

use crate::loader;
use crate::value::AStr;
use crate::{Name, NameSet};

pub type ItemModRef = Arc<ItemMod>;

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ItemMod {
    /// The mod's stat lines (the `1..n` part of the Lua table).
    #[serde(rename = "$seq", default)]
    pub lines: Vec<AStr>,
    #[serde(default)]
    pub r#type: Option<Name>,
    #[serde(default)]
    pub affix: Option<AStr>,
    #[serde(default)]
    pub group: Option<Name>,
    #[serde(default)]
    pub level: Option<f64>,
    #[serde(default)]
    pub modTags: Vec<Name>,
    #[serde(default)]
    pub statOrder: Vec<f64>,
    #[serde(default)]
    pub tags: Vec<Name>,
    /// `tradeHashes[hash] = { lines }`.
    #[serde(default, deserialize_with = "loader::int_map")]
    pub tradeHashes: std::collections::BTreeMap<i64, Vec<AStr>>,
    #[serde(default)]
    pub weightKey: Vec<Name>,
    #[serde(default)]
    pub weightVal: Vec<f64>,
    #[serde(default)]
    pub weightMultiplierKey: Vec<Name>,
    #[serde(default)]
    pub weightMultiplierVal: Vec<f64>,
    /// masterMods: the item types the craft applies to.
    #[serde(default, deserialize_with = "loader::name_set_opt")]
    pub types: Option<NameSet>,
}

/// An affix table: mod id → mod, in dump (bytewise id) order. Entries that are
/// references (`itemMods.ItemExclusive` → `itemMods.Item.*`) share the `Arc`.
#[derive(Debug, Default)]
pub struct AffixTable(pub indexmap::IndexMap<Name, ItemModRef>);

impl<'de> Deserialize<'de> for AffixTable {
    fn deserialize<D: serde::Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        struct E(ItemModRef);
        impl<'de> Deserialize<'de> for E {
            fn deserialize<D: serde::Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
                loader::shared(d).map(E)
            }
        }
        let m: indexmap::IndexMap<Name, E> = indexmap::IndexMap::deserialize(d)?;
        Ok(AffixTable(m.into_iter().map(|(k, v)| (k, v.0)).collect()))
    }
}

/// `data.itemMods`: category (`Item`, `Flask`, `Jewel`, …) → table.
pub type AffixTables = indexmap::IndexMap<Name, AffixTable>;

/// `data.masterMods`: a list (Data/ModMaster.lua).
#[derive(Debug, Default)]
pub struct AffixList(pub Vec<ItemModRef>);

impl<'de> Deserialize<'de> for AffixList {
    fn deserialize<D: serde::Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        loader::shared_vec(d).map(AffixList)
    }
}

/// One `data.uniqueMods[unique][i]`: `{ Id = …, mod = <itemMods entry> }`.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct UniqueModEntry {
    pub Id: Name,
    #[serde(rename = "mod", deserialize_with = "loader::shared")]
    pub r#mod: ItemModRef,
}

/// `data.uniqueMods`: unique name → its selectable mods.
pub type UniqueMods = indexmap::IndexMap<Name, Vec<UniqueModEntry>>;
