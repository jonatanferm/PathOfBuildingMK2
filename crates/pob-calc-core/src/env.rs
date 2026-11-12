//! The calc environment (`env`), `override` and `specEnv` (calc doc §5.4).
#![allow(non_snake_case)]

use std::cell::Cell;
use std::rc::Rc;

use pob_gamedata::Data;
use pob_lua::{Field, LjIntMap, Name, PtrMap, Shared};

use crate::active_skill::ActiveSkillRef;
use crate::actor::{Actor, ActorRef};
use crate::classes::{Item, ItemRef, ModDB, ModList, ModStore, NodeRef, PassiveSpecRef};
use crate::lua_table::Table;
use crate::model::BuildRef;

/// `env.mode`.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub enum Mode {
    MAIN,
    CALCS,
    CALCULATOR,
}

impl Mode {
    pub fn as_str(self) -> &'static str {
        match self {
            Mode::MAIN => "MAIN",
            Mode::CALCS => "CALCS",
            Mode::CALCULATOR => "CALCULATOR",
        }
    }

    /// Index into `GlobalCache.cachedData` (§5.6).
    pub fn index(self) -> usize {
        match self {
            Mode::MAIN => 0,
            Mode::CALCS => 1,
            Mode::CALCULATOR => 2,
        }
    }
}

pub type EnvRef = Rc<Env>;

/// One entry of `env.explodeSources`: `buildModListForNodeList` returns the allocated nodes
/// with a `CanExplode` mod (CalcSetup.lua:252-275), CalcSetup.lua:838 adds items (CCR-0030 #3).
#[derive(Clone)]
pub enum ExplodeSource {
    Node(NodeRef),
    Item(ItemRef),
}

impl ExplodeSource {
    /// The entry as a Lua value (`Val::Obj` of the node or item).
    pub fn to_val(&self) -> crate::lua_table::Val {
        match self {
            ExplodeSource::Node(n) => crate::lua_table::Val::Obj(n.clone()),
            ExplodeSource::Item(i) => crate::lua_table::Val::Obj(i.clone()),
        }
    }

    /// A node or item value as an entry (`None` for anything else).
    pub fn from_val(v: &crate::lua_table::Val) -> Option<Self> {
        if let Some(n) = v.obj::<crate::classes::Node>() {
            return Some(ExplodeSource::Node(n));
        }
        v.obj::<Item>().map(ExplodeSource::Item)
    }

    /// Identity (`rawequal`).
    pub fn ptr_eq(&self, o: &ExplodeSource) -> bool {
        match (self, o) {
            (ExplodeSource::Node(a), ExplodeSource::Node(b)) => Rc::ptr_eq(a, b),
            (ExplodeSource::Item(a), ExplodeSource::Item(b)) => Rc::ptr_eq(a, b),
            _ => false,
        }
    }
}

/// The `env` table (CalcSetup.lua 490+).
pub struct Env {
    pub build: BuildRef,
    pub data: &'static Data,
    pub mode: Mode,
    pub mode_buffs: Cell<bool>,
    pub mode_combat: Cell<bool>,
    pub mode_effective: Cell<bool>,
    pub buildBreakdown: Cell<bool>,
    pub configInput: Table,
    pub configPlaceholder: Table,
    pub calcsInput: Table,
    pub spec: Field<PassiveSpecRef>,
    pub r#override: Rc<Override>,
    pub classId: Cell<f64>,
    pub modDB: Field<ModDB>,
    pub enemyDB: Field<ModDB>,
    pub itemModDB: Field<ModDB>,
    pub initialNodeModDB: Field<Option<ModDB>>,
    pub aegisModList: Field<Option<ModList>>,
    pub weaponModList1: Field<Option<ModList>>,
    pub theIronMass: Field<Option<ModList>>,
    pub enemyLevel: Cell<f64>,
    pub player: ActorRef,
    pub enemy: ActorRef,
    pub minion: Field<Option<ActorRef>>,
    pub partyMembers: Field<Option<ActorRef>>,
    pub allocNodes: Field<Shared<LjIntMap<NodeRef>>>,
    pub grantedPassives: Field<Shared<LjIntMap<bool>>>,
    pub grantedSkills: Field<Shared<Vec<Table>>>,
    pub grantedSkillsNodes: Field<Shared<Vec<Table>>>,
    pub grantedSkillsItems: Field<Shared<Vec<Table>>>,
    pub radiusJewelList: Field<Shared<Vec<Table>>>,
    pub extraRadiusNodeList: Field<Shared<LjIntMap<NodeRef>>>,
    pub flasks: Field<Shared<PtrMap<Item, bool>>>,
    pub tinctures: Field<Shared<PtrMap<Item, bool>>>,
    pub flaskSlotOccupied: Field<Table>,
    /// `env.flaskSlotMap[item] = flaskNum` (CalcSetup.lua:1023/1035): keyed by item identity
    /// (CCR-0030 #5).
    pub flaskSlotMap: Field<Shared<PtrMap<Item, f64>>>,
    pub itemWarnings: Field<Table>,
    pub requirementsTable: Field<Table>,
    pub requirementsTableItems: Field<Table>,
    pub requirementsTableGems: Field<Table>,
    pub auxSkillList: Field<Shared<Vec<ActiveSkillRef>>>,
    pub limitedSkills: Field<Option<Table>>,
    pub mainSocketGroup: Cell<Option<f64>>,
    pub crossLinkedSupportGroups: Field<Table>,
    pub keystonesAdded: Field<Table>,
    /// Passive tree nodes and items with an explode mod (CalcSetup.lua 540, 838, 1421-1423;
    /// CCR-0030 #3). `socketGroup.explodeSources` aliases it (1522).
    pub explodeSources: Field<Shared<Vec<ExplodeSource>>>,
    pub aegisItem: Field<Option<ItemRef>>,
    pub buffs: Field<Table>,
    pub minionBuffs: Field<Table>,
    pub debuffs: Field<Table>,
    pub curseSlots: Field<Table>,
    // MAIN/CALCS used-tracking (Calcs.lua 437-872)
    pub skillsUsed: Field<Option<Table>>,
    pub conditionsUsed: Field<Option<Table>>,
    pub enemyConditionsUsed: Field<Option<Table>>,
    pub minionConditionsUsed: Field<Option<Table>>,
    pub multipliersUsed: Field<Option<Table>>,
    pub enemyMultipliersUsed: Field<Option<Table>>,
    pub perStatsUsed: Field<Option<Table>>,
    pub enemyPerStatsUsed: Field<Option<Table>>,
    pub tagTypesUsed: Field<Option<Table>>,
    pub modsUsed: Field<Option<Table>>,
    pub extra: Table,
}

impl Env {
    /// Allocate an `env` (the `env = { }` of CalcSetup.lua:490): every field a
    /// fresh, empty handle; `player`/`enemy` are fresh actors. initEnv's
    /// segments (CS1-CS3) then assign the fields in Lua order.
    pub fn new(
        build: &BuildRef,
        mode: Mode,
        r#override: Rc<Override>,
        spec: PassiveSpecRef,
    ) -> EnvRef {
        Rc::new(Env {
            build: build.clone(),
            data: build.data,
            mode,
            mode_buffs: Cell::new(false),
            mode_combat: Cell::new(false),
            mode_effective: Cell::new(false),
            buildBreakdown: Cell::new(false),
            configInput: build.configTab.input.clone(),
            configPlaceholder: build.configTab.placeholder.clone(),
            calcsInput: build.calcsTab.input.clone(),
            spec: Field::new(spec),
            r#override,
            classId: Cell::new(0.0),
            modDB: Field::new(Rc::new(ModStore::default())),
            enemyDB: Field::new(Rc::new(ModStore::default())),
            itemModDB: Field::new(Rc::new(ModStore::default())),
            initialNodeModDB: Field::new(None),
            aegisModList: Field::new(None),
            weaponModList1: Field::new(None),
            theIronMass: Field::new(None),
            enemyLevel: Cell::new(0.0),
            player: Actor::new(),
            enemy: Actor::new(),
            minion: Field::new(None),
            partyMembers: Field::new(None),
            allocNodes: Field::new(Shared::new(LjIntMap::new())),
            grantedPassives: Field::new(Shared::new(LjIntMap::new())),
            grantedSkills: Field::new(Shared::new(Vec::new())),
            grantedSkillsNodes: Field::new(Shared::new(Vec::new())),
            grantedSkillsItems: Field::new(Shared::new(Vec::new())),
            radiusJewelList: Field::new(Shared::new(Vec::new())),
            extraRadiusNodeList: Field::new(Shared::new(LjIntMap::new())),
            flasks: Field::new(Shared::new(PtrMap::new())),
            tinctures: Field::new(Shared::new(PtrMap::new())),
            flaskSlotOccupied: Field::new(Table::new()),
            flaskSlotMap: Field::new(Shared::new(PtrMap::new())),
            itemWarnings: Field::new(Table::new()),
            requirementsTable: Field::new(Table::new()),
            requirementsTableItems: Field::new(Table::new()),
            requirementsTableGems: Field::new(Table::new()),
            auxSkillList: Field::new(Shared::new(Vec::new())),
            limitedSkills: Field::new(None),
            mainSocketGroup: Cell::new(None),
            crossLinkedSupportGroups: Field::new(Table::new()),
            keystonesAdded: Field::new(Table::new()),
            explodeSources: Field::new(Shared::new(Vec::new())),
            aegisItem: Field::new(None),
            buffs: Field::new(Table::new()),
            minionBuffs: Field::new(Table::new()),
            debuffs: Field::new(Table::new()),
            curseSlots: Field::new(Table::new()),
            skillsUsed: Field::new(None),
            conditionsUsed: Field::new(None),
            enemyConditionsUsed: Field::new(None),
            minionConditionsUsed: Field::new(None),
            multipliersUsed: Field::new(None),
            enemyMultipliersUsed: Field::new(None),
            perStatsUsed: Field::new(None),
            enemyPerStatsUsed: Field::new(None),
            tagTypesUsed: Field::new(None),
            modsUsed: Field::new(None),
            extra: Table::new(),
        })
    }
}

/// The PoB `override` table. Mutable and shared by identity: initEnv sets
/// `extraJewelFuncs` (CalcSetup.lua:1424-1430), appends `conditions`
/// (1921-1922), restarts with the same object, and
/// buildActiveSkill/copyActiveSkill reuse `env.override`.
#[derive(Default)]
pub struct Override {
    pub spec: Field<Option<PassiveSpecRef>>,
    pub addNodes: Field<Option<Shared<LjIntMap<NodeRef>>>>,
    pub removeNodes: Field<Option<Shared<LjIntMap<NodeRef>>>>,
    pub repSlotName: Field<Option<Name>>,
    pub repItem: Field<Option<ItemRef>>,
    pub toggleFlask: Field<Option<ItemRef>>,
    pub toggleTincture: Field<Option<ItemRef>>,
    pub conditions: Field<Option<Shared<Vec<Name>>>>,
    pub extraJewelFuncs: Field<Option<ModList>>,
    pub extra: Table,
}

/// The `specEnv` argument of initEnv (Calcs.lua:93/192, CalcSetup.lua:478-482).
#[derive(Default, Clone)]
pub struct SpecEnv {
    pub cachedPlayerDB: Option<ModDB>,
    pub cachedEnemyDB: Option<ModDB>,
    pub cachedMinionDB: Option<ModDB>,
    pub env: Option<EnvRef>,
    pub accelerate: Accelerate,
}

/// `specEnv.accelerate`.
#[derive(Default, Clone, Copy, Debug)]
pub struct Accelerate {
    pub nodeAlloc: bool,
    pub requirementsItems: bool,
    pub requirementsGems: bool,
    pub skills: bool,
    pub everything: bool,
}
