//! Actors: `env.player`, `env.enemy`, minions, party members (calc doc §5.4).
#![allow(non_snake_case)]

use std::cell::Cell;
use std::rc::{Rc, Weak};

use pob_gamedata::{MinionDataRef, ModRef, NameSet};
use pob_lua::{lua_error, Field, Name, Shared};

use crate::active_skill::ActiveSkillRef;
use crate::breakdown::Breakdown;
use crate::classes::{ItemRef, ModDB, ModStore};
use crate::lua_table::{LStr, Output, Table};

pub type ActorRef = Rc<Actor>;
pub type ActorWeak = Weak<Actor>;

/// `actor.itemList`: slot name → item.
pub type ItemList = Shared<indexmap::IndexMap<Name, ItemRef>>;

/// A Lua table keyed by mod identity (`cache[mod] = true`, CalcPerform.lua:629), in insertion
/// order. Mods are `Arc`s, which `pob_lua::PtrSet` (over `Rc`) cannot hold (CCR-0030 #6).
#[derive(Clone, Default)]
pub struct ModPtrSet(Vec<ModRef>);

impl ModPtrSet {
    pub fn new() -> Self {
        Self::default()
    }
    /// `t[mod] ~= nil`.
    pub fn contains(&self, m: &ModRef) -> bool {
        self.0.iter().any(|x| std::sync::Arc::ptr_eq(x, m))
    }
    /// `t[mod] = true`; returns whether `mod` was absent.
    pub fn insert(&mut self, m: &ModRef) -> bool {
        if self.contains(m) {
            false
        } else {
            self.0.push(m.clone());
            true
        }
    }
    pub fn len(&self) -> usize {
        self.0.len()
    }
    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }
    /// `pairs(t)` keys, in insertion order.
    pub fn keys(&self) -> Vec<ModRef> {
        self.0.clone()
    }
    /// `wipeTable(t)`.
    pub fn wipe(&mut self) {
        self.0.clear();
    }
}

/// An actor table (`env.player = { modDB = …, level = … }`, CalcSetup.lua 519+).
/// Back-pointers are `Weak` (§3.3 rule 4); `Env` owns its actors.
pub struct Actor {
    pub modDB: Field<ModDB>,
    pub level: Cell<f64>,
    /// perform reassigns `env.player.output = {}` (CalcPerform.lua:1305).
    pub output: Field<Output>,
    pub breakdown: Field<Option<Breakdown>>,
    /// slot name → item. `None` = Lua nil: the enemy actor has no `itemList`
    /// (CalcSetup.lua:519-522), a minion gets one at CalcActiveSkill.lua:722 (CCR-0015 §3).
    pub itemList: Field<Option<ItemList>>,
    pub weaponData1: Field<Table>,
    pub weaponData2: Field<Table>,
    pub weaponRange1: Cell<Option<f64>>,
    pub weaponRange2: Cell<Option<f64>>,
    pub mainSkill: Field<Option<ActiveSkillRef>>,
    pub activeSkillList: Field<Shared<Vec<ActiveSkillRef>>>,
    pub enemy: Field<ActorWeak>,
    pub parent: Field<ActorWeak>,
    pub player: Field<ActorWeak>,
    pub minion: Field<Option<ActorRef>>,
    pub partyMembers: Field<ActorWeak>,
    pub minionData: Field<Option<MinionDataRef>>,
    pub damageShiftTable: Field<Option<Table>>,
    pub damageOverTimeShiftTable: Field<Option<Table>>,
    pub allyLifeList: Field<Option<Table>>,
    pub strDmgBonus: Cell<f64>,
    pub hostile: Cell<bool>,
    /// `actor.appliedEnemyModifiers`: the set `cache[mod] = true` of the EnemyModifier mods
    /// already applied (CalcPerform.lua:620-630), keyed by mod identity (CCR-0030 #6).
    pub appliedEnemyModifiers: Field<Option<Shared<ModPtrSet>>>,
    pub infoTrigger: Field<Option<LStr>>,
    pub itemSetId: Cell<Option<f64>>,
    /// `minion.uses = activeGrantedEffect.minionUses` (CalcActiveSkill.lua:723): the data's set
    /// of slot names, shared by identity (every value at the pin is `[slot] = true`; CCR-0030 #2,
    /// CCR-0040 §1).
    pub uses: Field<Option<NameSet>>,
    /// `minion.type`: the minion id (CalcActiveSkill.lua:707; CCR-0040 §1).
    pub r#type: Field<Option<Name>>,
    /// `minion.lifeTable`: one of `data.monsterLifeTable{,2,3}` / `data.monsterAllyLifeTable`,
    /// by identity (CalcActiveSkill.lua:724-727; CCR-0040 §1).
    pub lifeTable: Cell<Option<&'static [f64]>>,
    /// `minion.itemSet`: an item set of `build.itemsTab.itemSets` (CalcActiveSkill.lua:740/745;
    /// CCR-0040 §1).
    pub itemSet: Field<Option<Table>>,
    /// Computed-key fields: `actor["reserved_"..name.."Base"]`, `"<X>Efficiency"`, …
    pub extra: Table,
}

impl Default for Actor {
    fn default() -> Self {
        Actor {
            modDB: Field::new(Rc::new(ModStore::default())),
            level: Cell::new(0.0),
            output: Field::new(Table::new()),
            breakdown: Field::new(None),
            itemList: Field::new(Some(Shared::new(indexmap::IndexMap::new()))),
            weaponData1: Field::new(Table::new()),
            weaponData2: Field::new(Table::new()),
            weaponRange1: Cell::new(None),
            weaponRange2: Cell::new(None),
            mainSkill: Field::new(None),
            activeSkillList: Field::new(Shared::new(Vec::new())),
            enemy: Field::new(Weak::new()),
            parent: Field::new(Weak::new()),
            player: Field::new(Weak::new()),
            minion: Field::new(None),
            partyMembers: Field::new(Weak::new()),
            minionData: Field::new(None),
            damageShiftTable: Field::new(None),
            damageOverTimeShiftTable: Field::new(None),
            allyLifeList: Field::new(None),
            strDmgBonus: Cell::new(0.0),
            hostile: Cell::new(false),
            appliedEnemyModifiers: Field::new(None),
            infoTrigger: Field::new(None),
            itemSetId: Cell::new(None),
            uses: Field::new(None),
            r#type: Field::new(None),
            lifeTable: Cell::new(None),
            itemSet: Field::new(None),
            extra: Table::new(),
        }
    }
}

impl Actor {
    pub fn new() -> ActorRef {
        Rc::new(Actor::default())
    }

    /// `{ modDB = modDB, level = level }` (the literal CalcSetup builds).
    pub fn with(modDB: ModDB, level: f64) -> ActorRef {
        let a = Actor::default();
        a.modDB.set(modDB);
        a.level.set(level);
        Rc::new(a)
    }

    /// `actor.enemy` (nil → None).
    pub fn enemy(&self) -> Option<ActorRef> {
        self.enemy.get().upgrade()
    }

    /// `actor.parent`.
    pub fn parent(&self) -> Option<ActorRef> {
        self.parent.get().upgrade()
    }

    /// `actor.player`.
    pub fn player(&self) -> Option<ActorRef> {
        self.player.get().upgrade()
    }

    /// `actor.itemList`, raising Lua's index-of-nil error when the actor has none (the enemy).
    pub fn itemList(&self) -> ItemList {
        self.itemList
            .get()
            .unwrap_or_else(|| lua_error!("attempt to index field 'itemList' (a nil value)"))
    }

    /// `actor.itemList[slotName]` (raises when `itemList` is nil, as Lua's index does).
    pub fn item_in(&self, slotName: Name) -> Option<ItemRef> {
        self.itemList().with(|l| l.get(&slotName).cloned())
    }
}
