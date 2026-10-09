//! `ModCfg`: the `cfg` tables passed to ModStore queries (`skillCfg`,
//! `weapon1Cfg`, ad-hoc `{ flags = …, keywordFlags = … }`; calc doc §5.2).
//!
//! Every field is mutable in place because Lua aliases cfg tables
//! (`copyTable(cfg, true)` is shallow, and segments write `cfg.flags` on a
//! shared table). The census of keys any cfg literal or write sets is in
//! docs/port/contracts.md#modcfg; other keys go to `extra`.
#![allow(non_snake_case)]

use std::cell::Cell;
use std::rc::Rc;

use pob_gamedata::{GemDataRef, GrantedEffectRef, SkillTypeSet};
use pob_lua::{Field, Name, Shared};

use crate::classes::ItemRef;
use crate::lua_table::{Output, Table};

pub type Cfg = Rc<ModCfg>;

/// `activeSkill.skillTypes` is shared by identity with `cfg.skillTypes`
/// (CalcActiveSkill.lua ~467).
pub type SkillTypes = Shared<SkillTypeSet>;

#[derive(Default)]
pub struct ModCfg {
    pub flags: Cell<u32>,
    pub keywordFlags: Cell<u32>,
    pub source: Cell<Option<Name>>,
    pub actor: Cell<Option<Name>>,
    pub skillName: Cell<Option<Name>>,
    pub summonSkillName: Cell<Option<Name>>,
    pub skillGem: Field<Option<GemDataRef>>,
    pub skillGrantedEffect: Field<Option<GrantedEffectRef>>,
    pub skillPart: Cell<Option<f64>>,
    pub skillTypes: Field<Option<SkillTypes>>,
    /// Layered via `IndexMeta` (CalcActiveSkill.lua:482/487, CalcOffence.lua:4422).
    pub skillCond: Field<Option<Table>>,
    pub skillDist: Cell<Option<f64>>,
    /// `weapon1Cfg.skillStats = output.MainHand` (CalcOffence.lua:2008/2025).
    pub skillStats: Field<Option<Output>>,
    pub slotName: Cell<Option<Name>>,
    pub socketColor: Cell<Option<Name>>,
    pub socketNum: Cell<Option<f64>>,
    pub strengthGems: Cell<Option<f64>>,
    pub dexterityGems: Cell<Option<f64>>,
    pub intelligenceGems: Cell<Option<f64>>,
    pub item: Field<Option<ItemRef>>,
    /// `cfg.conversionTable` (CalcOffence.lua:77/145 read it; offence pass cfgs set it).
    pub conversionTable: Field<Option<Table>>,
    /// `cfg.baseFlags` (read by EvalMod's BaseFlag tag, ModStore.lua:882; no cfg sets it at the pin).
    pub baseFlags: Field<Option<pob_gamedata::NameSet>>,
    /// Any other key a cfg literal sets.
    pub extra: Table,
}

thread_local! {
    static MODCFG_STRING: &'static ModCfg = Box::leak(Box::new(ModCfg::default()));
}

impl ModCfg {
    pub fn new() -> Cfg {
        Rc::new(ModCfg::default())
    }

    /// `{ flags = f, keywordFlags = k }` (the common ad-hoc literal).
    pub fn with_flags(flags: u32, keywordFlags: u32) -> Cfg {
        let c = ModCfg::default();
        c.flags.set(flags);
        c.keywordFlags.set(keywordFlags);
        Rc::new(c)
    }

    /// Common.lua `copyTable(cfg, noRecurse)`. Shallow (`true`, the only form
    /// PoB uses on cfgs) shares the `skillCond` / `skillTypes` / `skillStats`
    /// / `item` handles; deep copies `skillCond`, `skillTypes`, `skillStats`
    /// and `extra` (items are never deep-copied by PoB).
    pub fn copyTable(&self, noRecurse: bool) -> Cfg {
        let c = ModCfg {
            flags: Cell::new(self.flags.get()),
            keywordFlags: Cell::new(self.keywordFlags.get()),
            source: Cell::new(self.source.get()),
            actor: Cell::new(self.actor.get()),
            skillName: Cell::new(self.skillName.get()),
            summonSkillName: Cell::new(self.summonSkillName.get()),
            skillGem: Field::new(self.skillGem.get()),
            skillGrantedEffect: Field::new(self.skillGrantedEffect.get()),
            skillPart: Cell::new(self.skillPart.get()),
            skillTypes: Field::new(self.skillTypes.get()),
            skillCond: Field::new(self.skillCond.get()),
            skillDist: Cell::new(self.skillDist.get()),
            skillStats: Field::new(self.skillStats.get()),
            slotName: Cell::new(self.slotName.get()),
            socketColor: Cell::new(self.socketColor.get()),
            socketNum: Cell::new(self.socketNum.get()),
            strengthGems: Cell::new(self.strengthGems.get()),
            dexterityGems: Cell::new(self.dexterityGems.get()),
            intelligenceGems: Cell::new(self.intelligenceGems.get()),
            item: Field::new(self.item.get()),
            conversionTable: Field::new(self.conversionTable.get()),
            baseFlags: Field::new(self.baseFlags.get()),
            extra: self.extra.copyTable(noRecurse),
        };
        if !noRecurse {
            c.skillCond
                .set(self.skillCond.get().map(|t| t.copyTable(false)));
            c.skillTypes
                .set(self.skillTypes.get().map(|s| Shared::new(s.with(|x| *x))));
            c.skillStats
                .set(self.skillStats.get().map(|t| t.copyTable(false)));
            c.conversionTable
                .set(self.conversionTable.get().map(|t| t.copyTable(false)));
        }
        Rc::new(c)
    }

    /// The `"MORE"` string used as a cfg (CalcOffence.lua:2194-2195): every
    /// field nil (`LUA-QUIRK`).
    pub fn STRING_CFG() -> &'static ModCfg {
        MODCFG_STRING.with(|c| *c)
    }
}
