//! The `data` roots that Modules/Data.lua builds for the Item class, ItemTools,
//! PassiveTree, CalcTools, ConfigOptions, SkillsTab, CalcsTab and the legion
//! LUT helper (CCR-0011 §7: calc-read roots are typed, not `Data::other`).
//! Strict: every struct is `deny_unknown_fields`, every map value typed.
#![allow(non_snake_case)]

use std::collections::BTreeMap;
use std::fmt;
use std::sync::Arc;

use serde::de::{self, MapAccess, SeqAccess, Visitor};
use serde::Deserialize;

use super::affixes::AffixTable;
use super::bases::ItemBaseRef;
use super::gems::GemDataRef;
use crate::loader;
use crate::value::AStr;
use crate::{LuaFnRef, Name};

// ---------------------------------------------------------------------------
// data.enchantments (Data.lua:675-709; Item.lua:1043-1048, IT-A)
// ---------------------------------------------------------------------------

/// One `data.enchantments[slot][key]`.
#[derive(Debug)]
pub enum EnchantmentList {
    /// `{ "line", … }` (Belt, Boots, Gloves, Body Armour, Flask, weapons:
    /// key = source such as `HARVEST`; weapon lines are `/`-joined).
    Lines(Vec<AStr>),
    /// Helmet: key = skill name, value = source (`ENDGAME`, `MERCILESS`, …) → lines.
    BySource(indexmap::IndexMap<Name, Vec<AStr>>),
}

impl<'de> Deserialize<'de> for EnchantmentList {
    fn deserialize<D: de::Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        struct V;
        impl<'de> Visitor<'de> for V {
            type Value = EnchantmentList;
            fn expecting(&self, f: &mut fmt::Formatter) -> fmt::Result {
                f.write_str("a list of enchantment lines or a source → lines table")
            }
            fn visit_seq<A: SeqAccess<'de>>(self, mut a: A) -> Result<Self::Value, A::Error> {
                let mut v = Vec::new();
                while let Some(s) = a.next_element::<AStr>()? {
                    v.push(s);
                }
                Ok(EnchantmentList::Lines(v))
            }
            fn visit_map<A: MapAccess<'de>>(self, mut a: A) -> Result<Self::Value, A::Error> {
                let mut m = indexmap::IndexMap::new();
                while let Some(k) = a.next_key::<String>()? {
                    if k.starts_with('$') {
                        return Err(de::Error::custom(format!(
                            "unexpected {k} in an enchantment table"
                        )));
                    }
                    m.insert(Name::from(k.as_str()), a.next_value::<Vec<AStr>>()?);
                }
                Ok(EnchantmentList::BySource(m))
            }
        }
        d.deserialize_any(V)
    }
}

/// `data.enchantments[slot]` (slot = `Helmet`, `Boots`, …, `Flask`,
/// `UtilityFlask`, `Weapon` and every `data.weaponTypeInfo` key).
pub type EnchantmentSlot = indexmap::IndexMap<Name, EnchantmentList>;
pub type EnchantmentSlotRef = Arc<EnchantmentSlot>;

/// `data.enchantments`. Aliased slots (`UtilityFlask` = `Flask`) share the `Arc`.
#[derive(Debug, Default)]
pub struct Enchantments(pub indexmap::IndexMap<Name, EnchantmentSlotRef>);

impl<'de> Deserialize<'de> for Enchantments {
    fn deserialize<D: de::Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        struct E(EnchantmentSlotRef);
        impl<'de> Deserialize<'de> for E {
            fn deserialize<D: de::Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
                loader::shared(d).map(E)
            }
        }
        let m: indexmap::IndexMap<Name, E> = indexmap::IndexMap::deserialize(d)?;
        Ok(Enchantments(m.into_iter().map(|(k, v)| (k, v.0)).collect()))
    }
}

impl Enchantments {
    /// `data.enchantments[slot]`.
    pub fn get(&self, slot: &str) -> Option<&EnchantmentSlotRef> {
        self.0.get(&Name::from(slot))
    }
}

// ---------------------------------------------------------------------------
// data.foulbornMap (Data.lua:1479-1481, dkjson; Item.lua:1028/1553, IT-A)
// ---------------------------------------------------------------------------

/// The dkjson object metatable `{ __jsontype = "object" }` every decoded
/// object carries (checked, then dropped).
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct DkjsonMeta {
    #[allow(dead_code)]
    __jsontype: Name,
}

/// A dkjson-decoded object: its string members (the `$meta` is checked).
fn dkjson_object<'de, D, T>(d: D) -> Result<indexmap::IndexMap<AStr, T>, D::Error>
where
    D: de::Deserializer<'de>,
    T: Deserialize<'de>,
{
    struct V<T>(std::marker::PhantomData<T>);
    impl<'de, T: Deserialize<'de>> Visitor<'de> for V<T> {
        type Value = indexmap::IndexMap<AStr, T>;
        fn expecting(&self, f: &mut fmt::Formatter) -> fmt::Result {
            f.write_str("a dkjson object")
        }
        fn visit_map<A: MapAccess<'de>>(self, mut a: A) -> Result<Self::Value, A::Error> {
            let mut m = indexmap::IndexMap::new();
            while let Some(k) = a.next_key::<String>()? {
                match k.as_str() {
                    "$meta" => {
                        let _: DkjsonMeta = a.next_value()?;
                    }
                    k if k.starts_with('$') => {
                        return Err(de::Error::custom(format!(
                            "unexpected {k} in a dkjson object"
                        )))
                    }
                    _ => {
                        m.insert(AStr::from(k.as_str()), a.next_value::<T>()?);
                    }
                }
            }
            Ok(m)
        }
    }
    d.deserialize_map(V(std::marker::PhantomData))
}

/// `data.foulbornMap[title]`: original mod id → foulborn (mutated) mod id.
#[derive(Debug, Default)]
pub struct FoulbornLines(pub indexmap::IndexMap<AStr, Name>);

impl<'de> Deserialize<'de> for FoulbornLines {
    fn deserialize<D: de::Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        let m: indexmap::IndexMap<AStr, Name> = dkjson_object(d)?;
        Ok(FoulbornLines(m))
    }
}

/// `data.foulbornMap`: unique title (without "Foulborn ") → its mutated lines.
#[derive(Debug, Default)]
pub struct FoulbornMap(pub indexmap::IndexMap<AStr, Arc<FoulbornLines>>);

impl<'de> Deserialize<'de> for FoulbornMap {
    fn deserialize<D: de::Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        struct E(Arc<FoulbornLines>);
        impl<'de> Deserialize<'de> for E {
            fn deserialize<D: de::Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
                loader::shared(d).map(E)
            }
        }
        let m: indexmap::IndexMap<AStr, E> = dkjson_object(d)?;
        Ok(FoulbornMap(m.into_iter().map(|(k, v)| (k, v.0)).collect()))
    }
}

// ---------------------------------------------------------------------------
// data.rareLikeUniques (Data.lua:1398-1457; Item.lua:1036-1038/1493/2059/2719, IT-A/IT-C)
// ---------------------------------------------------------------------------

/// One `validBases[i]`: `{ base }` (synthesised bases) or a
/// `data.itemBaseLists[type][i]` entry `{ label, name, base }`.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RareLikeItemBase {
    #[serde(deserialize_with = "loader::shared")]
    pub base: ItemBaseRef,
    #[serde(default)]
    pub name: Option<AStr>,
    #[serde(default)]
    pub label: Option<AStr>,
}

/// `data.rareLikeUniques[lowercase title]` (`RareLikeUniqueDescription`).
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RareLikeUnique {
    /// A mod id → mod table (mods shared with `data.itemMods`/`veiledMods`).
    pub affixes: AffixTable,
    pub prefixLimit: f64,
    pub suffixLimit: f64,
    #[serde(default)]
    pub validBases: Option<Vec<RareLikeItemBase>>,
    #[serde(default)]
    pub ignoreModType: bool,
    #[serde(default)]
    pub allowDuplicateGroups: bool,
    #[serde(default)]
    pub supportsCustomModifiers: Option<indexmap::IndexMap<Name, bool>>,
}

pub type RareLikeUniqueRef = Arc<RareLikeUnique>;

/// `data.rareLikeUniques`.
#[derive(Debug, Default)]
pub struct RareLikeUniques(pub indexmap::IndexMap<AStr, RareLikeUniqueRef>);

impl<'de> Deserialize<'de> for RareLikeUniques {
    fn deserialize<D: de::Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        struct E(RareLikeUniqueRef);
        impl<'de> Deserialize<'de> for E {
            fn deserialize<D: de::Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
                loader::shared(d).map(E)
            }
        }
        let m: indexmap::IndexMap<AStr, E> = indexmap::IndexMap::deserialize(d)?;
        Ok(RareLikeUniques(
            m.into_iter().map(|(k, v)| (k, v.0)).collect(),
        ))
    }
}

// ---------------------------------------------------------------------------
// data.itemTagSpecial / itemTagSpecialExclusionPattern (Data.lua:731-790; Item.lua:164-324, IT-A)
// ---------------------------------------------------------------------------

/// `data.itemTagSpecial[tag][slotName]` = list of line patterns (both tables).
pub type ItemTagSpecial = indexmap::IndexMap<Name, indexmap::IndexMap<Name, Vec<AStr>>>;

// ---------------------------------------------------------------------------
// data.modScalability (Data/ModScalability.lua; ItemTools.lua:146-191, IT-C)
// ---------------------------------------------------------------------------

/// One `data.modScalability[line][i]`.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ModScalability {
    pub isScalable: bool,
    #[serde(default)]
    pub formats: Option<Vec<Name>>,
}

/// `data.modScalability[line]`. An entry with an empty list is present (and
/// truthy in Lua): the key `""` maps to `{}`.
pub type ModScalabilityMap = ahash::AHashMap<AStr, Vec<ModScalability>>;

// ---------------------------------------------------------------------------
// data.bossStats (Data.lua:988-993; ConfigOptions.lua:2185-2220, CO)
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BossStats {
    pub PinnacleArmourMean: f64,
    pub PinnacleEvasionMean: f64,
    pub UberArmourMean: f64,
    pub UberEvasionMean: f64,
}

// ---------------------------------------------------------------------------
// data.gemsByGameId (Data.lua:1087-1096; SkillsTab.lua:466, BM-B)
// ---------------------------------------------------------------------------

/// `data.gemsByGameId[gameId][variantId]` = gem (the `data.gems` `Arc`).
#[derive(Debug, Default)]
pub struct GemsByGameId(pub ahash::AHashMap<Name, indexmap::IndexMap<Name, GemDataRef>>);

impl<'de> Deserialize<'de> for GemsByGameId {
    fn deserialize<D: de::Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        struct E(GemDataRef);
        impl<'de> Deserialize<'de> for E {
            fn deserialize<D: de::Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
                loader::shared(d).map(E)
            }
        }
        let m: indexmap::IndexMap<Name, indexmap::IndexMap<Name, E>> =
            indexmap::IndexMap::deserialize(d)?;
        Ok(GemsByGameId(
            m.into_iter()
                .map(|(k, v)| (k, v.into_iter().map(|(vk, e)| (vk, e.0)).collect()))
                .collect(),
        ))
    }
}

// ---------------------------------------------------------------------------
// data.powerStatList (Data.lua:129-235; CalcsTab.lua:743-744, PWR)
// ---------------------------------------------------------------------------

/// One `data.powerStatList[i]` (`PowerStat`).
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PowerStat {
    #[serde(default)]
    pub stat: Option<Name>,
    pub label: AStr,
    #[serde(default)]
    pub transform: Option<LuaFnRef>,
    #[serde(default)]
    pub combinedOffDef: bool,
    #[serde(default)]
    pub ignoreForItems: bool,
    #[serde(default)]
    pub ignoreForNodes: bool,
    #[serde(default)]
    pub reverseSort: bool,
    #[serde(default)]
    pub itemField: Option<Name>,
}

/// `data.powerStatList`: the list plus its `GetFromOutput` member
/// (Data.lua:186-212).
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PowerStatList {
    #[serde(rename = "$seq")]
    pub stats: Vec<PowerStat>,
    pub GetFromOutput: LuaFnRef,
}

// ---------------------------------------------------------------------------
// data.nodeIDList (Data/TimelessJewelData/NodeIndexMapping.lua; DataLegionLookUpTableHelper.lua, LUT)
// ---------------------------------------------------------------------------

/// `data.nodeIDList[nodeId]`.
#[derive(Debug, Clone, Copy, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct NodeIndex {
    pub index: f64,
    pub size: f64,
}

/// `data.nodeIDList.localIdToGlobalId[jewelType]`: `size` plus the
/// integer-keyed mapping (`$seq` and `$num` merged).
#[derive(Debug, Default)]
pub struct LocalIdToGlobalId {
    pub size: f64,
    pub map: BTreeMap<i64, f64>,
}

impl<'de> Deserialize<'de> for LocalIdToGlobalId {
    fn deserialize<D: de::Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        #[derive(Deserialize)]
        #[serde(deny_unknown_fields)]
        struct Raw {
            size: f64,
            #[serde(rename = "$seq", default)]
            seq: Vec<f64>,
            #[serde(rename = "$num", default, deserialize_with = "num_map")]
            num: BTreeMap<i64, f64>,
        }
        let r = Raw::deserialize(d)?;
        let mut map = r.num;
        for (i, v) in r.seq.into_iter().enumerate() {
            map.insert(i as i64 + 1, v);
        }
        Ok(LocalIdToGlobalId { size: r.size, map })
    }
}

/// `data.nodeIDList`.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct NodeIdList {
    pub size: f64,
    pub sizeNotable: f64,
    pub localIdToGlobalId: Vec<LocalIdToGlobalId>,
    /// node id → `{ index, size }`.
    #[serde(rename = "$num", deserialize_with = "num_map")]
    pub nodes: BTreeMap<i64, NodeIndex>,
}

/// A `$num` pseudo member → integer-keyed map.
fn num_map<'de, D, T>(d: D) -> Result<BTreeMap<i64, T>, D::Error>
where
    D: de::Deserializer<'de>,
    T: Deserialize<'de>,
{
    let m: indexmap::IndexMap<String, T> = indexmap::IndexMap::deserialize(d)?;
    m.into_iter()
        .map(|(k, v)| {
            k.parse::<i64>()
                .map(|i| (i, v))
                .map_err(|_| de::Error::custom(format!("non-integer key {k}")))
        })
        .collect()
}
