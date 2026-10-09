//! Actors: `env.player`, `env.enemy`, minions, party members (calc doc §5.4).
#![allow(non_snake_case)]

use std::cell::Cell;
use std::rc::{Rc, Weak};

use pob_gamedata::MinionDataRef;
use pob_lua::{Field, Name, Shared};

use crate::active_skill::ActiveSkillRef;
use crate::breakdown::Breakdown;
use crate::classes::{ItemRef, ModDB, ModStore};
use crate::lua_table::{LStr, Output, Table};

pub type ActorRef = Rc<Actor>;
pub type ActorWeak = Weak<Actor>;

/// An actor table (`env.player = { modDB = …, level = … }`, CalcSetup.lua 519+).
/// Back-pointers are `Weak` (§3.3 rule 4); `Env` owns its actors.
pub struct Actor {
    pub modDB: Field<ModDB>,
    pub level: Cell<f64>,
    /// perform reassigns `env.player.output = {}` (CalcPerform.lua:1305).
    pub output: Field<Output>,
    pub breakdown: Field<Option<Breakdown>>,
    /// slot name → item.
    pub itemList: Shared<indexmap::IndexMap<Name, ItemRef>>,
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
    pub appliedEnemyModifiers: Field<Option<Table>>,
    pub infoTrigger: Field<Option<LStr>>,
    pub itemSetId: Cell<Option<f64>>,
    pub uses: Cell<Option<f64>>,
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
            itemList: Shared::new(indexmap::IndexMap::new()),
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
            uses: Cell::new(None),
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
}
