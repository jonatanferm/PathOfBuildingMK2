//! The PoB global `data`, post-processed, immutable (calc doc §4.4).
//!
//! Typed roots are listed in [`TYPED_ROOTS`]; every other dump root is kept in
//! dumped form in [`Data::other`] (resolve its references with
//! [`Data::resolve`]). The assembly fails on a dump root that is neither, and
//! on any key a typed struct does not model.
#![allow(non_snake_case)]

use std::collections::{BTreeMap, HashMap};
use std::sync::{Arc, OnceLock};

use serde::Deserialize;

use crate::generated::misc::{characterConstants, gameConstants, misc, monsterConstants};
use crate::loader::{self, DumpTree, LoadError};
use crate::schema::affixes::{AffixList, AffixTable, AffixTables, UniqueMods};
use crate::schema::bases::{Costs, ItemBaseRef, UnarmedWeaponData, WeaponTypeInfo};
use crate::schema::calc_sections::CalcSections;
use crate::schema::config_defs::ConfigOptionDef;
use crate::schema::gems::GemDataRef;
use crate::schema::item_data::{
    BossStats, Enchantments, FoulbornMap, GemsByGameId, ItemTagSpecial, ModScalabilityMap,
    NodeIdList, PowerStatList, RareLikeUniques,
};
use crate::schema::minions::MinionDataRef;
use crate::schema::misc::{
    AilmentData, Boss, BossSkill, ClusterJewelData, JewelRadius, LabelVal, Pantheon,
};
use crate::schema::mod_map_defs::ModMapDefs;
use crate::schema::skills::{GrantedEffectRef, StatMapEntry};
use crate::schema::stat_descriptions::StatDescriptionsLazy;
use crate::schema::tree::GameVersions;
use crate::schema::uniques::UniqueTexts;
use crate::value::{AStr, DataValue};
use crate::{LuaFnRef, ModRef, ModType, Name, NameSet};

/// One `ModCache` entry: `c["<line>"] = { modList, extraText }`.
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ModCacheEntry {
    pub mods: Option<Vec<ModRef>>,
    pub extra: Option<AStr>,
}

/// Data/ModCache.lua (the parse tier of `modLib.parseMod`), line → entry.
pub type ModCacheData = ahash::AHashMap<AStr, ModCacheEntry>;

/// `files["Data/PearlSupports.lua"]`.
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PearlSupport {
    pub baseItemName: AStr,
    pub grantedEffectName: Name,
}

/// The timeless-jewel scalars of `data` (the LUTs are LUT's).
#[derive(Debug, Clone, Default)]
pub struct TimelessMeta {
    pub timelessJewelAdditions: f64,
    pub timelessJewelTypes: DataValue,
    pub timelessJewelSeedMin: DataValue,
    pub timelessJewelSeedMax: DataValue,
    pub timelessJewelTradeIDs: DataValue,
    pub timelessJewelLUTs: DataValue,
    pub legionPassives: DataValue,
}

/// The PoB global `data` (post-processed). Immutable, `Send + Sync`.
pub struct Data {
    pub skills: indexmap::IndexMap<Name, GrantedEffectRef>,
    pub gems: indexmap::IndexMap<Name, GemDataRef>,
    /// `data.gemForSkill[grantedEffect]` = gem id (keyed by the granted effect's id).
    pub gemForSkill: ahash::AHashMap<Name, Name>,
    /// `data.gemForBaseName[lowercase name]` = gem id.
    pub gemForBaseName: ahash::AHashMap<AStr, Name>,
    pub skillStatMap: ahash::AHashMap<Name, Arc<StatMapEntry>>,
    pub itemBases: indexmap::IndexMap<AStr, ItemBaseRef>,
    pub minions: indexmap::IndexMap<Name, MinionDataRef>,
    pub spectres: indexmap::IndexMap<Name, MinionDataRef>,
    pub highPrecisionMods: ahash::AHashMap<Name, ahash::AHashMap<ModType, i32>>,
    pub defaultHighPrecision: i32,
    /// verbatim keys: `data().misc.EnemyMaxResist`
    pub misc: misc,
    pub characterConstants: characterConstants,
    pub monsterConstants: monsterConstants,
    pub gameConstants: gameConstants,
    pub monsterLifeTable: Vec<f64>,
    pub monsterLifeTable2: Vec<f64>,
    pub monsterLifeTable3: Vec<f64>,
    pub monsterDamageTable: Vec<f64>,
    pub monsterAccuracyTable: Vec<f64>,
    pub monsterArmourTable: Vec<f64>,
    pub monsterEvasionTable: Vec<f64>,
    pub monsterAilmentThresholdTable: Vec<f64>,
    pub monsterPhysConversionMultiTable: Vec<f64>,
    pub monsterAllyLifeTable: Vec<f64>,
    pub monsterAllyDamageTable: Vec<f64>,
    pub monsterVarietyLifeMult: indexmap::IndexMap<AStr, f64>,
    pub mapLevelLifeMult: BTreeMap<i64, f64>,
    pub mapLevelBossLifeMult: Vec<f64>,
    pub mapLevelBossAilmentMult: Vec<f64>,
    pub totemLifeMult: BTreeMap<i64, f64>,
    pub goldRespecPrices: Vec<f64>,
    pub maxJewelRadius: f64,
    pub enemyIsBossTooltip: AStr,
    /// The function members of `data` (describeStats, readLUT, setJewelRadiiGlobally, …), by name.
    pub fns: indexmap::IndexMap<Name, LuaFnRef>,
    pub weaponTypeInfo: ahash::AHashMap<Name, WeaponTypeInfo>,
    /// `data.unarmedWeaponData[classId]` (keys 0..6).
    pub unarmedWeaponData: BTreeMap<i64, UnarmedWeaponData>,
    pub nonDamagingAilment: indexmap::IndexMap<Name, AilmentData>,
    pub ailmentTypeList: Vec<Name>,
    pub elementalAilmentTypeList: Vec<Name>,
    pub nonDamagingAilmentTypeList: Vec<Name>,
    pub nonElementalAilmentTypeList: Vec<Name>,
    /// `"3_15"`, `"3_16"`.
    pub jewelRadii: ahash::AHashMap<Name, Vec<JewelRadius>>,
    pub clusterJewels: ClusterJewelData,
    pub keystones: Vec<Name>,
    pub uniques: UniqueTexts,
    pub rares: Vec<AStr>,
    pub itemMods: AffixTables,
    pub masterMods: AffixList,
    pub veiledMods: AffixTable,
    pub necropolisMods: AffixTable,
    pub uniqueMods: UniqueMods,
    /// `data.enchantments[slot][key]` (Item.lua:1043-1048).
    pub enchantments: Enchantments,
    /// `data.foulbornMap[title]` (Item.lua:1028).
    pub foulbornMap: FoulbornMap,
    /// `data.rareLikeUniques[lowercase title]` (Item.lua:1036).
    pub rareLikeUniques: RareLikeUniques,
    /// Item.lua:164-324.
    pub itemTagSpecial: ItemTagSpecial,
    pub itemTagSpecialExclusionPattern: ItemTagSpecial,
    /// Unique titles (Item.lua:2713/2716).
    pub casterTagCrucibleUniques: NameSet,
    pub minionTagCrucibleUniques: NameSet,
    /// stat id → display name (PassiveTree.lua:728).
    pub abyssNotableNames: ahash::AHashMap<Name, AStr>,
    /// ItemTools.lua:146-191.
    pub modScalability: ModScalabilityMap,
    /// `data.monsterExperienceLevelMap[level]` (Build.lua:236).
    pub monsterExperienceLevelMap: Vec<f64>,
    /// ConfigOptions.lua:2185-2220.
    pub bossStats: BossStats,
    /// Vaal gem id → base gem id (CalcTools.lua:287).
    pub gemVaalGemIdForBaseGemId: ahash::AHashMap<Name, Name>,
    /// SkillsTab.lua:466.
    pub gemsByGameId: GemsByGameId,
    /// CalcsTab.lua:743-744.
    pub powerStatList: PowerStatList,
    /// DataLegionLookUpTableHelper.lua.
    pub nodeIDList: NodeIdList,
    pub pantheons: indexmap::IndexMap<Name, Pantheon>,
    pub bosses: indexmap::IndexMap<Name, Boss>,
    pub bossSkills: indexmap::IndexMap<Name, BossSkill>,
    pub bossSkillsList: Vec<LabelVal>,
    pub costs: Costs,
    pub cursePriority: indexmap::IndexMap<Name, f64>,
    pub configOptions: Vec<ConfigOptionDef>,
    pub mapMods: ModMapDefs,
    pub calcSections: CalcSections,
    pub statDescriptions: StatDescriptionsLazy,
    pub pearlSupports: Vec<PearlSupport>,
    pub gameVersions: GameVersions,
    /// Data/ModCache.lua (parse tier).
    pub modCache: ModCacheData,
    pub timeless: TimelessMeta,
    /// Every other dump root, in dumped form (contracts.md#schema lists them).
    pub other: indexmap::IndexMap<AStr, DataValue>,
    /// Reference targets reachable from `other` (see [`Data::resolve`]).
    pub refs: HashMap<AStr, DataValue>,
}

/// Roots deserialised into typed fields (everything else goes to `other`).
pub const TYPED_ROOTS: &[&str] = &[
    "data.skills",
    "data.gems",
    "data.gemForSkill",
    "data.gemForBaseName",
    "data.skillStatMap",
    "data.skillStatMapMeta",
    "data.itemBases",
    "data.minions",
    "data.spectres",
    "data.highPrecisionMods",
    "data.defaultHighPrecision",
    "data.misc",
    "data.characterConstants",
    "data.monsterConstants",
    "data.gameConstants",
    "data.monsterLifeTable",
    "data.monsterLifeTable2",
    "data.monsterLifeTable3",
    "data.monsterDamageTable",
    "data.monsterAccuracyTable",
    "data.monsterArmourTable",
    "data.monsterEvasionTable",
    "data.monsterAilmentThresholdTable",
    "data.monsterPhysConversionMultiTable",
    "data.monsterAllyLifeTable",
    "data.monsterAllyDamageTable",
    "data.monsterVarietyLifeMult",
    "data.mapLevelLifeMult",
    "data.mapLevelBossLifeMult",
    "data.mapLevelBossAilmentMult",
    "data.totemLifeMult",
    "data.goldRespecPrices",
    "data.maxJewelRadius",
    "data.enemyIsBossTooltip",
    "data.describeStats",
    "data.getAbyssJewelComponentRoll",
    "data.printMissingMinionSkills",
    "data.readAbyssJewelLUT",
    "data.readLUT",
    "data.repairLUTs",
    "data.resolveAbyssJewelComponent",
    "data.setJewelRadiiGlobally",
    "data.weaponTypeInfo",
    "data.unarmedWeaponData",
    "data.nonDamagingAilment",
    "data.ailmentTypeList",
    "data.elementalAilmentTypeList",
    "data.nonDamagingAilmentTypeList",
    "data.nonElementalAilmentTypeList",
    "data.jewelRadii",
    "data.clusterJewels",
    "data.keystones",
    "data.uniques",
    "data.rares",
    "data.itemMods",
    "data.masterMods",
    "data.veiledMods",
    "data.necropolisMods",
    "data.uniqueMods",
    "data.enchantments",
    "data.foulbornMap",
    "data.rareLikeUniques",
    "data.itemTagSpecial",
    "data.itemTagSpecialExclusionPattern",
    "data.casterTagCrucibleUniques",
    "data.minionTagCrucibleUniques",
    "data.abyssNotableNames",
    "data.modScalability",
    "data.monsterExperienceLevelMap",
    "data.bossStats",
    "data.gemVaalGemIdForBaseGemId",
    "data.gemsByGameId",
    "data.powerStatList",
    "data.nodeIDList",
    "data.pantheons",
    "data.bosses",
    "data.bossSkills",
    "data.bossSkillsList",
    "data.costs",
    "data.cursePriority",
    "ConfigOptions",
    "data.mapMods",
    "CalcSections",
    "files[\"Data/PearlSupports.lua\"]",
    "GameVersions",
    "ModCache",
    "data.timelessJewelAdditions",
    "data.timelessJewelTypes",
    "data.timelessJewelSeedMin",
    "data.timelessJewelSeedMax",
    "data.timelessJewelTradeIDs",
    "data.timelessJewelLUTs",
    "files[\"Data/TimelessJewelData/LegionPassives.lua\"]",
];

/// The function members of `data` kept in [`Data::fns`].
const FN_ROOTS: &[&str] = &[
    "describeStats",
    "getAbyssJewelComponentRoll",
    "printMissingMinionSkills",
    "readAbyssJewelLUT",
    "readLUT",
    "repairLUTs",
    "resolveAbyssJewelComponent",
    "setJewelRadiiGlobally",
];

struct Sh<T>(Arc<T>);
impl<'de, T: Deserialize<'de> + Send + Sync + 'static> Deserialize<'de> for Sh<T> {
    fn deserialize<D: serde::Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        loader::shared(d).map(Sh)
    }
}

fn shared_map<K, T>(tree: &DumpTree, root: &str) -> Result<indexmap::IndexMap<K, Arc<T>>, LoadError>
where
    K: for<'de> Deserialize<'de> + std::hash::Hash + Eq,
    T: for<'de> Deserialize<'de> + Send + Sync + 'static,
{
    let m: indexmap::IndexMap<K, Sh<T>> = tree.get(root)?;
    Ok(m.into_iter().map(|(k, v)| (k, v.0)).collect())
}

struct GemForSkill(Vec<(Name, Name)>);
impl<'de> Deserialize<'de> for GemForSkill {
    fn deserialize<D: serde::Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        #[derive(Deserialize)]
        #[serde(deny_unknown_fields)]
        struct Raw {
            #[serde(rename = "$keys")]
            keys: Vec<(SkillId, Name)>,
        }
        /// A granted effect used as a key: only its id is needed.
        struct SkillId(Name);
        impl<'de> Deserialize<'de> for SkillId {
            fn deserialize<D: serde::Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
                #[derive(Deserialize)]
                struct OnlyId {
                    id: Name,
                }
                OnlyId::deserialize(d).map(|o| SkillId(o.id))
            }
        }
        let r = Raw::deserialize(d)?;
        Ok(GemForSkill(
            r.keys.into_iter().map(|(k, v)| (k.0, v)).collect(),
        ))
    }
}

impl Data {
    /// Build `Data` from a dump tree (index built). Fails on the first
    /// unknown key (with its Lua path) and on a dump root that is neither
    /// typed nor known-dynamic.
    pub fn from_tree(tree: &DumpTree) -> Result<Data, LoadError> {
        loader::reset_shared_cache();
        let r = Self::from_tree_inner(tree);
        loader::reset_shared_cache();
        r
    }

    fn from_tree_inner(t: &DumpTree) -> Result<Data, LoadError> {
        // skills first, so gems share their Arcs
        let skills: indexmap::IndexMap<Name, GrantedEffectRef> = shared_map(t, "data.skills")?;
        let gems: indexmap::IndexMap<Name, GemDataRef> = shared_map(t, "data.gems")?;
        let minions: indexmap::IndexMap<Name, MinionDataRef> = shared_map(t, "data.minions")?;
        let spectres: indexmap::IndexMap<Name, MinionDataRef> = shared_map(t, "data.spectres")?;
        let itemBases: indexmap::IndexMap<AStr, ItemBaseRef> = shared_map(t, "data.itemBases")?;
        let itemMods: AffixTables = t.get("data.itemMods")?;
        let gemForSkill: GemForSkill = t.get("data.gemForSkill")?;
        let skillStatMap: indexmap::IndexMap<Name, StatMapEntry> = t.get("data.skillStatMap")?;
        #[derive(Deserialize)]
        #[serde(deny_unknown_fields)]
        struct StatMapMeta {
            #[allow(dead_code)]
            __index: LuaFnRef,
        }
        let _: StatMapMeta = t.get("data.skillStatMapMeta")?;
        let highPrecisionMods: indexmap::IndexMap<Name, indexmap::IndexMap<ModType, i32>> =
            t.get("data.highPrecisionMods")?;
        let mut fns = indexmap::IndexMap::new();
        for f in FN_ROOTS {
            fns.insert(Name::from(f), t.get::<LuaFnRef>(&format!("data.{f}"))?);
        }
        let mut statDescriptions = StatDescriptionsLazy::default();
        let mut other = indexmap::IndexMap::new();
        for (root, v) in &t.roots {
            if TYPED_ROOTS.contains(&&**root) {
                continue;
            }
            if let Some(scope) = root.strip_prefix("StatDescriptions.") {
                statDescriptions.0.insert(Name::from(scope), v.clone());
                continue;
            }
            if DYNAMIC_ROOTS.contains(&&**root) {
                other.insert(root.clone(), v.clone());
                continue;
            }
            return Err(LoadError::new(format!(
                "dump root {root} is neither typed nor a known dynamic root (data.rs TYPED_ROOTS / DYNAMIC_ROOTS)"
            )));
        }
        // reference targets reachable from the dynamic roots and stat descriptions
        let mut refs = HashMap::new();
        let mut stack: Vec<DataValue> = other.values().cloned().collect();
        stack.extend(statDescriptions.0.values().cloned());
        while let Some(v) = stack.pop() {
            match &v {
                DataValue::Ref(p) => {
                    if !refs.contains_key(p) {
                        let target = t.resolve(p)?.clone();
                        refs.insert(p.clone(), target.clone());
                        stack.push(target);
                    }
                }
                DataValue::Table(tb) => {
                    stack.extend(tb.fields.iter().map(|(_, x)| x.clone()));
                    stack.extend(tb.seq.iter().cloned());
                    stack.extend(tb.num.iter().map(|(_, x)| x.clone()));
                    for (k, x) in &tb.keys {
                        stack.push(k.clone());
                        stack.push(x.clone());
                    }
                    if let Some(m) = &tb.meta {
                        stack.push(m.clone());
                    }
                }
                _ => {}
            }
        }
        let raw = |root: &str| -> Result<DataValue, LoadError> {
            t.roots
                .get(root)
                .cloned()
                .ok_or_else(|| LoadError::new(format!("missing root {root}")))
        };
        Ok(Data {
            gemForSkill: gemForSkill.0.into_iter().collect(),
            gemForBaseName: t
                .get::<indexmap::IndexMap<AStr, Name>>("data.gemForBaseName")?
                .into_iter()
                .collect(),
            skillStatMap: skillStatMap
                .into_iter()
                .map(|(k, v)| (k, Arc::new(v)))
                .collect(),
            highPrecisionMods: highPrecisionMods
                .into_iter()
                .map(|(k, v)| (k, v.into_iter().collect()))
                .collect(),
            defaultHighPrecision: t.get("data.defaultHighPrecision")?,
            misc: t.get("data.misc")?,
            characterConstants: t.get("data.characterConstants")?,
            monsterConstants: t.get("data.monsterConstants")?,
            gameConstants: t.get("data.gameConstants")?,
            monsterLifeTable: t.get("data.monsterLifeTable")?,
            monsterLifeTable2: t.get("data.monsterLifeTable2")?,
            monsterLifeTable3: t.get("data.monsterLifeTable3")?,
            monsterDamageTable: t.get("data.monsterDamageTable")?,
            monsterAccuracyTable: t.get("data.monsterAccuracyTable")?,
            monsterArmourTable: t.get("data.monsterArmourTable")?,
            monsterEvasionTable: t.get("data.monsterEvasionTable")?,
            monsterAilmentThresholdTable: t.get("data.monsterAilmentThresholdTable")?,
            monsterPhysConversionMultiTable: t.get("data.monsterPhysConversionMultiTable")?,
            monsterAllyLifeTable: t.get("data.monsterAllyLifeTable")?,
            monsterAllyDamageTable: t.get("data.monsterAllyDamageTable")?,
            monsterVarietyLifeMult: t.get("data.monsterVarietyLifeMult")?,
            mapLevelLifeMult: IntMap::get(t, "data.mapLevelLifeMult")?,
            mapLevelBossLifeMult: t.get("data.mapLevelBossLifeMult")?,
            mapLevelBossAilmentMult: t.get("data.mapLevelBossAilmentMult")?,
            totemLifeMult: IntMap::get(t, "data.totemLifeMult")?,
            goldRespecPrices: t.get("data.goldRespecPrices")?,
            maxJewelRadius: t.get("data.maxJewelRadius")?,
            enemyIsBossTooltip: AStr::from(t.get::<String>("data.enemyIsBossTooltip")?),
            fns,
            weaponTypeInfo: t
                .get::<indexmap::IndexMap<Name, WeaponTypeInfo>>("data.weaponTypeInfo")?
                .into_iter()
                .collect(),
            unarmedWeaponData: IntMap::get(t, "data.unarmedWeaponData")?,
            nonDamagingAilment: t.get("data.nonDamagingAilment")?,
            ailmentTypeList: t.get("data.ailmentTypeList")?,
            elementalAilmentTypeList: t.get("data.elementalAilmentTypeList")?,
            nonDamagingAilmentTypeList: t.get("data.nonDamagingAilmentTypeList")?,
            nonElementalAilmentTypeList: t.get("data.nonElementalAilmentTypeList")?,
            jewelRadii: t
                .get::<indexmap::IndexMap<Name, Vec<JewelRadius>>>("data.jewelRadii")?
                .into_iter()
                .collect(),
            clusterJewels: t.get("data.clusterJewels")?,
            keystones: t.get("data.keystones")?,
            uniques: t.get("data.uniques")?,
            rares: t.get("data.rares")?,
            itemMods,
            masterMods: t.get("data.masterMods")?,
            veiledMods: t.get("data.veiledMods")?,
            necropolisMods: t.get("data.necropolisMods")?,
            uniqueMods: t.get("data.uniqueMods")?,
            enchantments: t.get("data.enchantments")?,
            foulbornMap: t.get("data.foulbornMap")?,
            rareLikeUniques: t.get("data.rareLikeUniques")?,
            itemTagSpecial: t.get("data.itemTagSpecial")?,
            itemTagSpecialExclusionPattern: t.get("data.itemTagSpecialExclusionPattern")?,
            casterTagCrucibleUniques: NameSetRoot::get(t, "data.casterTagCrucibleUniques")?,
            minionTagCrucibleUniques: NameSetRoot::get(t, "data.minionTagCrucibleUniques")?,
            abyssNotableNames: t
                .get::<indexmap::IndexMap<Name, AStr>>("data.abyssNotableNames")?
                .into_iter()
                .collect(),
            modScalability: t
                .get::<indexmap::IndexMap<AStr, Vec<crate::schema::item_data::ModScalability>>>(
                    "data.modScalability",
                )?
                .into_iter()
                .collect(),
            monsterExperienceLevelMap: t.get("data.monsterExperienceLevelMap")?,
            bossStats: t.get("data.bossStats")?,
            gemVaalGemIdForBaseGemId: t
                .get::<indexmap::IndexMap<Name, Name>>("data.gemVaalGemIdForBaseGemId")?
                .into_iter()
                .collect(),
            gemsByGameId: t.get("data.gemsByGameId")?,
            powerStatList: t.get("data.powerStatList")?,
            nodeIDList: t.get("data.nodeIDList")?,
            pantheons: t.get("data.pantheons")?,
            bosses: t.get("data.bosses")?,
            bossSkills: t.get("data.bossSkills")?,
            bossSkillsList: t.get("data.bossSkillsList")?,
            costs: t.get("data.costs")?,
            cursePriority: t.get("data.cursePriority")?,
            configOptions: t.get("ConfigOptions")?,
            mapMods: t.get("data.mapMods")?,
            calcSections: t.get("CalcSections")?,
            statDescriptions,
            pearlSupports: t.get("files[\"Data/PearlSupports.lua\"]")?,
            gameVersions: t.get("GameVersions")?,
            modCache: t
                .get::<indexmap::IndexMap<AStr, ModCacheEntry>>("ModCache")?
                .into_iter()
                .collect(),
            timeless: TimelessMeta {
                timelessJewelAdditions: t.get("data.timelessJewelAdditions")?,
                timelessJewelTypes: raw("data.timelessJewelTypes")?,
                timelessJewelSeedMin: raw("data.timelessJewelSeedMin")?,
                timelessJewelSeedMax: raw("data.timelessJewelSeedMax")?,
                timelessJewelTradeIDs: raw("data.timelessJewelTradeIDs")?,
                timelessJewelLUTs: raw("data.timelessJewelLUTs")?,
                legionPassives: raw("files[\"Data/TimelessJewelData/LegionPassives.lua\"]")?,
            },
            skills,
            gems,
            itemBases,
            minions,
            spectres,
            other,
            refs,
        })
    }

    /// Follow a reference found inside [`Data::other`] / stat descriptions.
    pub fn resolve<'a>(&'a self, v: &'a DataValue) -> &'a DataValue {
        let mut v = v;
        while let DataValue::Ref(p) = v {
            match self.refs.get(p) {
                Some(t) => v = t,
                None => break,
            }
        }
        v
    }

    /// `data.gems[data.gemForSkill[grantedEffect]]`.
    pub fn gem_for_skill(&self, skillId: Name) -> Option<&GemDataRef> {
        self.gemForSkill
            .get(&skillId)
            .and_then(|g| self.gems.get(g))
    }
}

struct NameSetRoot;
impl NameSetRoot {
    fn get(t: &DumpTree, root: &str) -> Result<NameSet, LoadError> {
        struct W(NameSet);
        impl<'de> Deserialize<'de> for W {
            fn deserialize<D: serde::Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
                loader::name_set(d).map(W)
            }
        }
        Ok(t.get::<W>(root)?.0)
    }
}

struct IntMap;
impl IntMap {
    fn get<T: for<'de> Deserialize<'de>>(
        t: &DumpTree,
        root: &str,
    ) -> Result<BTreeMap<i64, T>, LoadError> {
        struct W<T>(BTreeMap<i64, T>);
        impl<'de, T: Deserialize<'de>> Deserialize<'de> for W<T> {
            fn deserialize<D: serde::Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
                loader::int_map(d).map(W)
            }
        }
        Ok(t.get::<W<T>>(root)?.0)
    }
}

/// Dump roots kept untyped in [`Data::other`]. None of them is read by the
/// in-scope calc Lua (docs/port/scope.toml); each is read only by UI/trade code
/// outside the port's calc scope (CCR-0011 §7 lists the reader of each).
/// `global` is the exception: its calc members (`SkillType`, `ModFlag`,
/// `KeywordFlag`, `colorCodes`, `MatchKeywordFlags`, …) are P1's generated code
/// (crates/pob-dump/generated/global.rs), so the root itself stays untyped.
/// Calc-read roots are typed (`TYPED_ROOTS`); adding a root here is a schema
/// decision that needs a CCR.
pub const DYNAMIC_ROOTS: &[&str] = &[
    "global",
    "data.beastCraft",
    "data.clusterJewelInfoForNotable",
    "data.crucible",
    "data.enchantmentSource",
    "data.essences",
    "data.flavourText",
    "data.gemGrantedEffectIdForVaalGemId",
    "data.itemBaseLists",
    "data.itemBaseTypeList",
    "data.skillColorMap",
    "data.vestigialModMappings",
    "data.vestigialUniqueBaseTypes",
    "files[\"Data/CurrencyNames.lua\"]",
    "files[\"Data/DelveDropOnly.lua\"]",
    "files[\"Data/IncursionDropOnly.lua\"]",
    "files[\"Data/ModFoulbornMap.lua\"]",
    "files[\"Data/QueryMods.lua\"]",
    "files[\"Data/TattooPassives.lua\"]",
    "files[\"Data/TradeSiteStats.lua\"]",
];

// ---------------------------------------------------------------------------
// Process global
// ---------------------------------------------------------------------------

static DATA: OnceLock<Data> = OnceLock::new();

/// The process-global `data`. Panics if [`init`] was not called.
pub fn data() -> &'static Data {
    DATA.get().expect("pob_gamedata::init not called")
}

/// Whether [`init`] has run.
pub fn is_initialised() -> bool {
    DATA.get().is_some()
}

/// Serialises the decode in [`init`] / [`init_from_dir`] (CCR-0089): `OnceLock::get_or_try_init`
/// is unstable, so concurrent first calls (parallel `#[test]`s of one binary) wait here instead
/// of each decoding the ~40 MB `data.bin`.
static INIT: std::sync::Mutex<()> = std::sync::Mutex::new(());

/// Initialise from `data.bin` bytes (postcard; desktop reads the file, wasm
/// fetches it). A second call is a no-op returning `Ok`. Thread-safe: concurrent
/// first calls decode once (CCR-0089).
pub fn init(bytes: &[u8]) -> Result<(), LoadError> {
    if DATA.get().is_some() {
        return Ok(());
    }
    let _once = INIT
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    init_locked(bytes)
}

/// [`init`] with [`INIT`] held.
fn init_locked(bytes: &[u8]) -> Result<(), LoadError> {
    if DATA.get().is_some() {
        return Ok(());
    }
    let tree = loader::decode_bin(bytes)?;
    let d = Data::from_tree(&tree)?;
    let _ = DATA.set(d);
    Ok(())
}

/// Initialise from an already built [`Data`] (tests, tools).
pub fn init_with(d: Data) {
    let _ = DATA.set(d);
}

type TreeProvider = fn(&str) -> Option<Vec<u8>>;
static TREE_PROVIDER: OnceLock<TreeProvider> = OnceLock::new();

/// Register where per-version tree files come from (desktop: the
/// `data/dump/trees/<ver>.bin` files `pob-dump compile` writes).
pub fn set_tree_provider(f: TreeProvider) {
    let _ = TREE_PROVIDER.set(f);
}

/// Lazy per tree version: the postcard bytes of `trees["<ver>"]`
/// ([`loader::decode_bin`] decodes them into a [`DumpTree`] whose root is
/// `trees["<ver>"]`).
pub fn tree_bytes(version: &str) -> Option<std::borrow::Cow<'static, [u8]>> {
    let f = TREE_PROVIDER.get()?;
    f(version).map(std::borrow::Cow::Owned)
}

// ---------------------------------------------------------------------------
// DataSel: the mutable parts of `data` (thread-local, like the Lua VM)
// ---------------------------------------------------------------------------

/// The only mutable parts of PoB's `data` (Data.lua:583-600, TreeTab.lua:553).
#[derive(Debug, Clone, Default)]
pub struct DataSel {
    pub jewelRadius: Vec<JewelRadius>,
    pub maxJewelRadius: f64,
}

thread_local! {
    pub(crate) static DATA_SEL: std::cell::RefCell<std::rc::Rc<DataSel>> = std::cell::RefCell::new(std::rc::Rc::new(DataSel::default()));
}

/// Read as `data.jewelRadius` / `data.maxJewelRadius`.
pub fn jewelRadius() -> std::rc::Rc<DataSel> {
    DATA_SEL.with(|s| s.borrow().clone())
}

/// Replace the selection (used by `setJewelRadiiGlobally`, PT).
pub fn set_jewel_radius_sel(sel: DataSel) {
    DATA_SEL.with(|s| *s.borrow_mut() = std::rc::Rc::new(sel));
}

// ---------------------------------------------------------------------------
// Desktop loaders
// ---------------------------------------------------------------------------

/// Parse a dump directory (`data/dump`, crates/pob-dump/FORMAT.md) into a
/// tree: every manifest file except `closures.json`, the parse oracle and the
/// tree files (`trees/*`, loaded lazily). The index is built.
#[cfg(not(target_arch = "wasm32"))]
pub fn load_dump_dir(dir: &std::path::Path) -> Result<DumpTree, LoadError> {
    let manifest = std::fs::read_to_string(dir.join("manifest.json"))
        .map_err(|e| LoadError::new(format!("manifest.json: {e}")))?;
    let m: serde_json::Value =
        serde_json::from_str(&manifest).map_err(|e| LoadError::new(e.to_string()))?;
    let files = m["files"]
        .as_array()
        .ok_or_else(|| LoadError::new("manifest without files"))?;
    let mut tree = DumpTree::default();
    for f in files {
        let file = f["file"].as_str().unwrap_or_default();
        let roots: Vec<String> = f["roots"]
            .as_array()
            .map(|r| {
                r.iter()
                    .filter_map(|x| x.as_str().map(str::to_owned))
                    .collect()
            })
            .unwrap_or_default();
        if roots.is_empty() || file.starts_with("trees/") {
            continue;
        }
        let text = std::fs::read_to_string(dir.join(file))
            .map_err(|e| LoadError::new(format!("{file}: {e}")))?;
        let v = loader::parse_json(&text).map_err(|e| LoadError::new(format!("{file}: {e}")))?;
        tree.add_file(&roots, v)?;
    }
    tree.build_index()?;
    Ok(tree)
}

/// The tree files of a dump directory: `(version, roots, path)` for every
/// `trees/<ver>.json` (not sprites/assets).
#[cfg(not(target_arch = "wasm32"))]
pub fn dump_tree_files(
    dir: &std::path::Path,
) -> Result<Vec<(String, String, std::path::PathBuf)>, LoadError> {
    let manifest = std::fs::read_to_string(dir.join("manifest.json"))
        .map_err(|e| LoadError::new(format!("manifest.json: {e}")))?;
    let m: serde_json::Value =
        serde_json::from_str(&manifest).map_err(|e| LoadError::new(e.to_string()))?;
    let mut out = Vec::new();
    for f in m["files"].as_array().into_iter().flatten() {
        let file = f["file"].as_str().unwrap_or_default();
        let root = f["roots"][0].as_str().unwrap_or_default();
        if let Some(ver) = root
            .strip_prefix("trees[\"")
            .and_then(|r| r.strip_suffix("\"]"))
        {
            out.push((ver.to_owned(), root.to_owned(), dir.join(file)));
        }
    }
    Ok(out)
}

/// Desktop convenience: initialise from `<dir>/data.bin` and serve trees from
/// `<dir>/trees/<ver>.bin`.
#[cfg(not(target_arch = "wasm32"))]
pub fn init_from_dir(dir: &std::path::Path) -> Result<(), LoadError> {
    static DIR: OnceLock<std::path::PathBuf> = OnceLock::new();
    let _ = DIR.set(dir.to_owned());
    set_tree_provider(|ver| {
        std::fs::read(DIR.get()?.join("trees").join(format!("{ver}.bin"))).ok()
    });
    if DATA.get().is_some() {
        return Ok(());
    }
    // CCR-0089: one read and one decode per process, however many threads get here first
    let _once = INIT
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    if DATA.get().is_some() {
        return Ok(());
    }
    let bytes = std::fs::read(dir.join("data.bin"))
        .map_err(|e| LoadError::new(format!("{}: {e}", dir.join("data.bin").display())))?;
    init_locked(&bytes)
}
