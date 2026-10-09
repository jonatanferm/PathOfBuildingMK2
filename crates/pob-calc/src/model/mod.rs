//! The build model: calc halves of `Build.lua` and the tabs (calc doc §5.8).
//!
//! Field paths are PoB's (`build.itemsTab.items`, `build.skillsTab.socketGroupList`,
//! `build.configTab.input`). The UI's tab controls wrap these models. Calc
//! code mutates the model through `Field`/`Shared` exactly where Lua does
//! (D13). The census of `build.<x>` / `<tab>.<x>` reads in the calc modules
//! is in docs/port/contracts.md#model-census.
#![allow(non_snake_case)]

use std::cell::Cell;
use std::rc::Rc;

use pob_gamedata::{Data, GemDataRef, GrantedEffectRef};
use pob_lua::{Field, LjIntMap, Name, Shared};

use crate::active_skill::{ActiveSkillRef, SkillEffectRef};
use crate::actor::{Actor, ActorRef};
use crate::classes::{ItemRef, ModList, ModStore, PassiveSpec, PassiveSpecRef, PassiveTreeRef};
use crate::env::EnvRef;
use crate::lua_table::{LStr, Output, Table};
use crate::modules::calcs::MiscCalculator;

pub mod build_xml;
pub mod calcs_tab;
pub mod config_build_mod_list;
pub mod config_tab;
pub mod items_tab;
pub mod party_tab;
pub mod skills_tab;
pub mod tree_tab;
pub mod xml;

pub type BuildRef = Rc<Build>;

/// The calc half of `Modules/Build.lua`'s build object.
pub struct Build {
    pub characterLevel: Cell<f64>,
    pub targetVersion: Field<Name>,
    pub viewMode: Field<Name>,
    pub mainSocketGroup: Cell<f64>,
    pub spectreList: Field<Shared<Vec<Name>>>,
    pub bandit: Field<Name>,
    pub pantheonMajorGod: Field<Name>,
    pub pantheonMinorGod: Field<Name>,
    pub buildFlag: Cell<bool>,
    pub spec: Field<PassiveSpecRef>,
    pub latestTree: Field<Option<PassiveTreeRef>>,
    pub treeTab: TreeTab,
    pub itemsTab: ItemsTab,
    pub skillsTab: SkillsTab,
    pub configTab: ConfigTab,
    pub calcsTab: CalcsTab,
    pub partyTab: PartyTab,
    pub data: &'static Data,
    pub outputRevision: Cell<f64>,
}

impl Build {
    /// An empty build (no tree, items or skills): the object `Build:Init`
    /// creates before `Load` fills it. BM-A's loaders fill it from XML.
    pub fn new(data: &'static Data) -> BuildRef {
        Rc::new(Build {
            characterLevel: Cell::new(1.0),
            targetVersion: Field::new(pob_lua::n!("3_0")),
            viewMode: Field::new(pob_lua::n!("TREE")),
            mainSocketGroup: Cell::new(1.0),
            spectreList: Field::new(Shared::new(Vec::new())),
            bandit: Field::new(pob_lua::n!("None")),
            pantheonMajorGod: Field::new(pob_lua::n!("None")),
            pantheonMinorGod: Field::new(pob_lua::n!("None")),
            buildFlag: Cell::new(false),
            spec: Field::new(Rc::new(PassiveSpec::default())),
            latestTree: Field::new(None),
            treeTab: TreeTab::default(),
            itemsTab: ItemsTab::default(),
            skillsTab: SkillsTab::default(),
            configTab: ConfigTab::default(),
            calcsTab: CalcsTab::default(),
            partyTab: PartyTab::default(),
            data,
            outputRevision: Cell::new(1.0),
        })
    }
}

/// `build.itemsTab` (Classes/ItemsTab.lua, model half).
#[derive(Default)]
pub struct ItemsTab {
    pub items: Shared<indexmap::IndexMap<u32, ItemRef>>,
    /// slot name → slot table (`{ selItemId = …, … }`).
    pub slots: Shared<indexmap::IndexMap<Name, Table>>,
    pub orderedSlots: Shared<Vec<Table>>,
    pub orderList: Shared<Vec<u32>>,
    pub itemSets: Shared<indexmap::IndexMap<u32, Table>>,
    pub itemSetOrderList: Shared<Vec<u32>>,
    pub activeItemSetId: Cell<f64>,
    pub activeItemSet: Field<Option<Table>>,
    pub sockets: Shared<LjIntMap<Table>>,
}

/// `build.skillsTab` (Classes/SkillsTab.lua, model half).
#[derive(Default)]
pub struct SkillsTab {
    pub socketGroupList: Field<Shared<Vec<SocketGroupRef>>>,
    pub skillSets: Shared<indexmap::IndexMap<u32, Table>>,
    pub skillSetOrderList: Shared<Vec<u32>>,
    pub activeSkillSetId: Cell<f64>,
    pub defaultGemLevel: Field<Option<Name>>,
    pub defaultGemQuality: Cell<Option<f64>>,
    pub displayGroup: Field<Option<SocketGroupRef>>,
    pub imbuedSupportBySlot: Table,
}

pub type SocketGroupRef = Rc<SocketGroup>;

/// One socket group (`skillsTab.socketGroupList[i]`).
#[derive(Default)]
pub struct SocketGroup {
    pub label: Field<LStr>,
    pub enabled: Cell<bool>,
    pub slotEnabled: Cell<bool>,
    pub slot: Field<Option<Name>>,
    pub source: Field<Option<LStr>>,
    pub includeInFullDPS: Cell<bool>,
    pub groupCount: Cell<Option<f64>>,
    pub gemList: Field<Shared<Vec<GemInstanceRef>>>,
    pub mainActiveSkill: Cell<f64>,
    pub mainActiveSkillCalcs: Cell<f64>,
    pub noSupports: Cell<bool>,
    pub displaySkillList: Field<Option<Shared<Vec<ActiveSkillRef>>>>,
    pub displaySkillListCalcs: Field<Option<Shared<Vec<ActiveSkillRef>>>>,
    pub displayLabel: Field<Option<LStr>>,
    pub explodeSources: Field<Option<Table>>,
    pub extra: Table,
}

pub type GemInstanceRef = Rc<GemInstance>;

/// One gem of a socket group (`socketGroup.gemList[i]`).
#[derive(Default)]
pub struct GemInstance {
    pub nameSpec: Field<LStr>,
    pub skillId: Field<Option<Name>>,
    pub gemId: Field<Option<Name>>,
    pub level: Cell<f64>,
    pub quality: Cell<f64>,
    pub enabled: Cell<bool>,
    pub enableGlobal1: Cell<bool>,
    pub enableGlobal2: Cell<bool>,
    pub count: Cell<Option<f64>>,
    pub gemData: Field<Option<GemDataRef>>,
    pub grantedEffect: Field<Option<GrantedEffectRef>>,
    pub skillPart: Cell<Option<f64>>,
    pub skillPartCalcs: Cell<Option<f64>>,
    pub skillStageCount: Cell<Option<f64>>,
    pub skillStageCountCalcs: Cell<Option<f64>>,
    pub skillMineCount: Cell<Option<f64>>,
    pub skillMineCountCalcs: Cell<Option<f64>>,
    pub skillMinion: Field<Option<Name>>,
    pub skillMinionCalcs: Field<Option<Name>>,
    pub skillMinionItemSet: Cell<Option<f64>>,
    pub skillMinionItemSetCalcs: Cell<Option<f64>>,
    pub skillMinionSkill: Cell<Option<f64>>,
    pub skillMinionSkillCalcs: Cell<Option<f64>>,
    pub fromItem: Cell<bool>,
    pub noSupports: Cell<bool>,
    pub triggered: Cell<bool>,
    pub displayEffect: Field<Option<SkillEffectRef>>,
    pub supportEffect: Field<Option<SkillEffectRef>>,
    pub reqLevel: Cell<Option<f64>>,
    pub extra: Table,
}

/// `build.configTab` (Classes/ConfigTab.lua, model half).
pub struct ConfigTab {
    pub input: Table,
    pub placeholder: Table,
    /// `varControls[var]`: the UI control state apply functions read and
    /// write (`selIndex`, `list`, `enabled`, `placeholder`).
    pub varControls: Table,
    pub modList: Field<ModList>,
    pub enemyModList: Field<ModList>,
    pub enemyLevel: Cell<Option<f64>>,
    pub configSets: Shared<indexmap::IndexMap<u32, Table>>,
    pub configSetOrderList: Shared<Vec<u32>>,
    pub activeConfigSetId: Cell<f64>,
}

impl Default for ConfigTab {
    fn default() -> Self {
        ConfigTab {
            input: Table::new(),
            placeholder: Table::new(),
            varControls: Table::new(),
            modList: Field::new(ModStore::ModList(None)),
            enemyModList: Field::new(ModStore::ModList(None)),
            enemyLevel: Cell::new(None),
            configSets: Shared::new(indexmap::IndexMap::new()),
            configSetOrderList: Shared::new(Vec::new()),
            activeConfigSetId: Cell::new(1.0),
        }
    }
}

/// `build.calcsTab` (Classes/CalcsTab.lua, model half).
#[derive(Default)]
pub struct CalcsTab {
    pub input: Table,
    pub mainEnv: Field<Option<EnvRef>>,
    pub calcsEnv: Field<Option<EnvRef>>,
    pub mainOutput: Field<Option<Output>>,
    pub calcsOutput: Field<Option<Output>>,
    pub miscCalculator: Field<Option<Rc<MiscCalculator>>>,
    pub powerBuildFlag: Cell<bool>,
}

/// `build.partyTab` (Classes/PartyTab.lua, model half).
pub struct PartyTab {
    pub actor: ActorRef,
    pub enableExportBuffs: Cell<bool>,
    pub enemyModList: Field<ModList>,
    pub buffExports: Field<Option<Table>>,
    pub extra: Table,
}

impl Default for PartyTab {
    fn default() -> Self {
        PartyTab {
            actor: Actor::new(),
            enableExportBuffs: Cell::new(false),
            enemyModList: Field::new(ModStore::ModList(None)),
            buffExports: Field::new(None),
            extra: Table::new(),
        }
    }
}

/// A node of PoB's XML tree (runtime/lua/xml.lua): `{ elem = …, attrib = { … }, [1] = child, [2] = "text", … }`.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct XmlNode {
    pub elem: LStr,
    /// Attributes in document order.
    pub attrib: indexmap::IndexMap<LStr, LStr>,
    pub children: Vec<XmlChild>,
}

/// One child of an [`XmlNode`]: an element or a text run.
#[derive(Clone, Debug, PartialEq)]
pub enum XmlChild {
    Node(XmlNode),
    Text(LStr),
}

/// `build.treeTab` (Classes/TreeTab.lua, model half).
#[derive(Default)]
pub struct TreeTab {
    pub specList: Shared<Vec<PassiveSpecRef>>,
    pub activeSpec: Cell<f64>,
}
