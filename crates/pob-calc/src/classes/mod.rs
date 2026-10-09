//! PoB's classes as Rust types (calc doc §5.3, §5.9). The struct definitions
//! (fields) are U0's frozen contract; the method bodies live in the unit
//! files declared below.
//!
//! Field lists follow the census of `self.<field> =` assignments in each
//! class file (docs/port/contracts.md#class-census): fields the calc reads or
//! writes are typed, the long tail lives in `extra` until a unit needs it typed
//! (CCR).
#![allow(non_snake_case)]

use std::cell::{Cell, RefCell};
use std::rc::{Rc, Weak};

use pob_gamedata::{ItemBaseRef, ModRef, Name, Value};
use pob_lua::{Field, LjIntMap, Shared};

use crate::actor::ActorWeak;
use crate::lua_table::{LStr, Table};

// ---- ModStore / ModDB / ModList ---------------------------------------------------------

pub mod eval_mod;
pub mod mod_db;
pub mod mod_list;
pub mod mod_store;

pub mod item;
pub mod item_mods;
pub mod item_parse;
pub mod item_tools;
pub mod passive_spec;
pub mod passive_spec_cluster;
pub mod passive_spec_paths;
pub mod passive_tree;

pub type ModStoreRef = Rc<ModStore>;
/// Names mirror PoB's classes; both are a [`ModStore`] with a [`StoreKind`].
pub type ModDB = ModStoreRef;
pub type ModList = ModStoreRef;

/// `new("ModDB")` / `new("ModList")`.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum StoreKind {
    #[default]
    ModDB,
    ModList,
}

/// The mods of a store. Only the `*Internal` functions (MS-A) touch it.
#[allow(dead_code)]
pub(crate) enum StoreMods {
    /// ModDB: `self.mods[name] = { mod, … }`.
    DB(ahash::AHashMap<Name, Vec<ModRef>>),
    /// ModList: flat list in insertion order, plus a name → positions index.
    /// The index is order-safe because SumInternal and friends iterate names
    /// outside and mods inside (ModList.lua:101-121).
    List {
        list: Vec<ModRef>,
        index: ahash::AHashMap<Name, smallvec::SmallVec<[u32; 2]>>,
    },
}

/// Classes/ModStore.lua + ModDB.lua / ModList.lua.
pub struct ModStore {
    pub kind: StoreKind,
    #[allow(dead_code)]
    pub(crate) mods: RefCell<StoreMods>,
    /// `self.parent` (false → None). A `Field`: CalcSetup.lua:689-692 reassigns
    /// `env.modDB.parent = cachedPlayerDB` (contracts.md#modstore).
    pub parent: Field<Option<ModStoreRef>>,
    /// ModStore.lua:42: `parent.actor` at construction time, else `{}` (a
    /// dangling Weak). Reassigned by `modDB.actor = actor`.
    pub actor: Field<ActorWeak>,
    pub conditions: Table,
    pub multipliers: Table,
}

impl Default for ModStore {
    fn default() -> Self {
        ModStore {
            kind: StoreKind::ModDB,
            mods: RefCell::new(StoreMods::DB(ahash::AHashMap::new())),
            parent: Field::new(None),
            actor: Field::new(Weak::new()),
            conditions: Table::new(),
            multipliers: Table::new(),
        }
    }
}

/// One `Tabulate` row: `{ value = …, mod = … }`.
#[derive(Clone, Debug)]
pub struct TabRow {
    pub value: Value,
    pub r#mod: ModRef,
}

/// `globalLimits` (ModDB levels only; allocated lazily).
pub type GlobalLimits = ahash::AHashMap<Name, f64>;

impl ModStore {
    /// `new("ModDB"):ModDB(parent)` (ModDB.lua:23-27 → ModStore.lua:40-46).
    /// The constructor is U0's (allocation); `ModStore:ModStore` statement
    /// bodies are MS-A's and run here in Lua order.
    pub fn ModDB(parent: Option<ModStoreRef>) -> ModStoreRef {
        let actor = parent.as_ref().map_or_else(Weak::new, |p| p.actor.get());
        Rc::new(ModStore {
            kind: StoreKind::ModDB,
            mods: RefCell::new(StoreMods::DB(ahash::AHashMap::new())),
            parent: Field::new(parent),
            actor: Field::new(actor),
            conditions: Table::new(),
            multipliers: Table::new(),
        })
    }

    /// `new("ModList"):ModList(parent)` (ModList.lua:22-25).
    pub fn ModList(parent: Option<ModStoreRef>) -> ModStoreRef {
        let actor = parent.as_ref().map_or_else(Weak::new, |p| p.actor.get());
        Rc::new(ModStore {
            kind: StoreKind::ModList,
            mods: RefCell::new(StoreMods::List {
                list: Vec::new(),
                index: ahash::AHashMap::new(),
            }),
            parent: Field::new(parent),
            actor: Field::new(actor),
            conditions: Table::new(),
            multipliers: Table::new(),
        })
    }
}

// ---- Item ---------------------------------------------------------------------------------

pub type ItemRef = Rc<Item>;

/// Classes/Item.lua. Typed fields are the ones the calc and the oracle read
/// and write; every other `self.<x>` of ParseRaw/BuildModList is in `extra`.
#[derive(Default)]
pub struct Item {
    pub id: Cell<Option<f64>>,
    pub raw: Field<LStr>,
    pub name: Field<LStr>,
    pub title: Field<Option<LStr>>,
    pub baseName: Field<Option<Name>>,
    pub base: Field<Option<ItemBaseRef>>,
    pub r#type: Field<Name>,
    pub rarity: Field<Name>,
    pub uniqueID: Field<Option<LStr>>,
    pub corrupted: Cell<bool>,
    pub mirrored: Cell<bool>,
    pub fractured: Cell<bool>,
    pub split: Cell<bool>,
    pub synthesised: Cell<bool>,
    /// shaper, elder, crusader, …
    pub influences: Table,
    pub quality: Cell<Option<f64>>,
    pub itemLevel: Cell<Option<f64>>,
    pub sockets: Field<Shared<Vec<Table>>>,
    pub abyssalSocketCount: Cell<f64>,
    pub variant: Cell<Option<f64>>,
    pub variantAlt: Cell<Option<f64>>,
    pub variantAlt2: Cell<Option<f64>>,
    pub variantAlt3: Cell<Option<f64>>,
    pub variantAlt4: Cell<Option<f64>>,
    pub variantAlt5: Cell<Option<f64>>,
    pub variantList: Field<Option<Shared<Vec<LStr>>>>,
    pub explicitModLines: Field<Shared<Vec<Table>>>,
    pub implicitModLines: Field<Shared<Vec<Table>>>,
    pub enchantModLines: Field<Shared<Vec<Table>>>,
    pub scourgeModLines: Field<Shared<Vec<Table>>>,
    pub crucibleModLines: Field<Shared<Vec<Table>>>,
    pub buffModLines: Field<Shared<Vec<Table>>>,
    pub classRequirementModLines: Field<Shared<Vec<Table>>>,
    pub baseModList: Field<Option<ModList>>,
    pub modList: Field<Option<ModList>>,
    /// Lua `slotModList[1]`, `[2]`.
    pub slotModList: [Field<Option<ModList>>; 2],
    pub buffModList: Field<Option<ModList>>,
    /// Lua `weaponData[1]`, `[2]` (keyed by slot number).
    pub weaponData: Field<Option<Table>>,
    pub armourData: Field<Option<Table>>,
    pub flaskData: Field<Option<Table>>,
    pub tinctureData: Field<Option<Table>>,
    /// funcList, radiusIndex, conqueredBy, limit, limitDisabled, …
    pub jewelData: Field<Option<Table>>,
    pub jewelRadiusIndex: Cell<Option<f64>>,
    pub jewelRadiusLabel: Field<Option<LStr>>,
    pub jewelRadiusData: Field<Option<Table>>,
    pub clusterJewel: Field<Option<Table>>,
    pub clusterJewelSkill: Field<Option<Name>>,
    pub clusterJewelNodeCount: Cell<Option<f64>>,
    pub catalyst: Cell<Option<f64>>,
    pub catalystQuality: Cell<Option<f64>>,
    pub talismanTier: Cell<Option<f64>>,
    pub requirements: Table,
    pub grantedSkills: Field<Shared<Vec<Table>>>,
    pub classRestriction: Field<Option<Name>>,
    pub limit: Cell<Option<f64>>,
    pub modSource: Field<Option<LStr>>,
    /// Every other ParseRaw / BuildModList field (contracts.md#class-census).
    pub extra: Table,
}

// ---- PassiveTree / Node / PassiveSpec -------------------------------------------------------

pub type NodeRef = Rc<Node>;

/// A tree node (PassiveTree.lua). Spec nodes overlay tree nodes via
/// `setmetatable({...}, treeNode)` (PassiveSpec.lua:56): an overlay `Node`
/// holds its own fields and falls back to `tree` for anything unset.
#[derive(Default)]
pub struct Node {
    pub id: f64,
    pub dn: Field<LStr>,
    pub sd: Field<Shared<Vec<LStr>>>,
    pub r#type: Field<Name>,
    pub isKeystone: Cell<bool>,
    pub isNotable: Cell<bool>,
    pub isMastery: Cell<bool>,
    pub isJewelSocket: Cell<bool>,
    pub isTattoo: Cell<bool>,
    pub isMultipleChoiceOption: Cell<bool>,
    pub ascendancyName: Field<Option<Name>>,
    pub classStartIndex: Cell<Option<f64>>,
    pub passivePointsGranted: Cell<Option<f64>>,
    pub modList: Field<ModList>,
    pub modKey: Field<LStr>,
    pub mods: Field<Shared<Vec<Table>>>,
    pub keystoneMod: Field<Option<ModRef>>,
    /// Written by CalcSetup (278/287) and CalcsTab:BuildOutput (452-455).
    pub finalModList: Field<Option<ModList>>,
    pub grantedSkills: Field<Option<Shared<Vec<Table>>>>,
    pub x: Cell<f64>,
    pub y: Cell<f64>,
    pub group: Field<Option<Table>>,
    pub o: Cell<f64>,
    pub oidx: Cell<f64>,
    pub angle: Cell<f64>,
    pub linked: Field<Shared<Vec<NodeRef>>>,
    pub linkedId: Field<Shared<Vec<f64>>>,
    pub depends: Field<Shared<Vec<NodeRef>>>,
    pub path: Field<Option<Shared<Vec<NodeRef>>>>,
    pub pathDist: Cell<Option<f64>>,
    pub distanceToClassStart: Cell<Option<f64>>,
    pub connectedToStart: Cell<bool>,
    pub nodesInRadius: Field<Option<Table>>,
    pub attributesInRadius: Field<Option<Table>>,
    pub conqueredBy: Field<Option<Table>>,
    pub alloc: Cell<bool>,
    pub expansionJewel: Field<Option<Table>>,
    pub masteryEffects: Field<Option<Table>>,
    pub allMasteryOptions: Cell<bool>,
    pub overrideType: Field<Option<Name>>,
    /// Some for spec overlay nodes (the `__index` parent).
    pub tree: Option<NodeRef>,
    pub extra: Table,
}

pub type PassiveTreeRef = Rc<PassiveTree>;

/// Classes/PassiveTree.lua (one per tree version).
#[derive(Default)]
pub struct PassiveTree {
    pub treeVersion: Name,
    pub nodes: LjIntMap<NodeRef>,
    pub classes: Table,
    pub classNameMap: Table,
    pub ascendNameMap: Table,
    pub internalAscendNameMap: Table,
    pub secondaryAscendNameMap: Table,
    pub keystoneMap: Table,
    pub notableMap: Table,
    pub ascendancyMap: Table,
    pub clusterNodeMap: Table,
    pub sockets: LjIntMap<NodeRef>,
    pub masteryEffects: Table,
    pub orbitRadii: Vec<f64>,
    pub skillsPerOrbit: Vec<f64>,
    pub orbitAnglesByOrbit: Vec<Vec<f64>>,
    pub legion: Field<Option<Table>>,
    pub tattoo: Field<Option<Table>>,
    pub alternate_ascendancies: Field<Option<Table>>,
    pub extra: Table,
}

pub type PassiveSpecRef = Rc<PassiveSpec>;

/// Classes/PassiveSpec.lua.
#[derive(Default)]
pub struct PassiveSpec {
    pub build: Weak<crate::model::Build>,
    pub title: Field<Option<LStr>>,
    pub tree: Field<PassiveTreeRef>,
    pub treeVersion: Field<Name>,
    pub nodes: Shared<LjIntMap<NodeRef>>,
    pub allocNodes: Shared<LjIntMap<NodeRef>>,
    pub allocSubgraphNodes: Shared<Vec<f64>>,
    pub allocExtendedNodes: Shared<Vec<f64>>,
    pub curClassId: Cell<f64>,
    pub curClassName: Field<Option<Name>>,
    pub curAscendClassId: Cell<f64>,
    pub curAscendClassName: Field<Option<Name>>,
    pub curAscendClassBaseName: Field<Option<Name>>,
    pub curSecondaryAscendClassId: Cell<f64>,
    pub curSecondaryAscendClassName: Field<Option<Name>>,
    pub masterySelections: Shared<LjIntMap<f64>>,
    pub jewels: Shared<LjIntMap<f64>>,
    pub subGraphs: Table,
    pub hashOverrides: Shared<LjIntMap<NodeRef>>,
    pub allocatedNotableCount: Cell<f64>,
    pub allocatedKeystoneCount: Cell<f64>,
    pub allocatedMasteryCount: Cell<f64>,
    pub allocatedMasteryTypeCount: Cell<f64>,
    pub allocatedMasteryTypes: Table,
    pub allocatedTattooTypes: Table,
    pub ignoredNodes: Table,
    pub splitPersonalityPath: Field<Option<Table>>,
    pub extra: Table,
}
