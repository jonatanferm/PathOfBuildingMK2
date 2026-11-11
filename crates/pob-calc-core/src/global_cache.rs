//! `GlobalCache` (Data/Global.lua:356), filled by `cacheData`
//! (Common.lua:924-942) and wiped by `wipeGlobalCache` (944-948). Entries hold
//! LIVE handles (D11): `.Env` aliases an env that may later be re-performed
//! (calcFullDPS). The fill/wipe functions are CT's (`modules/common.rs`).
#![allow(non_snake_case)]

use std::cell::RefCell;
use std::rc::Rc;

use pob_lua::Name;

use crate::active_skill::ActiveSkillRef;
use crate::env::{EnvRef, Mode};
use crate::lua_table::Val;

/// One `GlobalCache.cachedData[mode][uuid]` entry (Common.lua:924-942).
pub struct CachedData {
    pub Name: Name,
    pub Speed: Val,
    pub HitSpeed: Val,
    pub ManaCost: Val,
    pub LifeCost: Val,
    pub ESCost: Val,
    pub RageCost: Val,
    pub HitChance: Val,
    pub AccuracyHitChance: Val,
    pub PreEffectiveCritChance: Val,
    pub CritChance: Val,
    pub TotalDPS: Val,
    pub ActiveSkill: ActiveSkillRef,
    pub Env: EnvRef,
}

/// `GlobalCache.cachedData`, indexed by `Mode::index()`.
#[derive(Default)]
pub struct GlobalCacheT {
    pub cachedData: [indexmap::IndexMap<Name, Rc<CachedData>>; 3],
}

thread_local! {
    pub static GlobalCache: RefCell<GlobalCacheT> = RefCell::default();
}

/// Read helper: `GlobalCache.cachedData[mode][uuid]` (no borrow escapes).
pub fn cachedData(mode: Mode, uuid: Name) -> Option<Rc<CachedData>> {
    GlobalCache.with(|g| g.borrow().cachedData[mode.index()].get(&uuid).cloned())
}

/// Write helper used by `cacheData` (CT).
pub fn set_cachedData(mode: Mode, uuid: Name, entry: Option<Rc<CachedData>>) {
    GlobalCache.with(|g| {
        let mut g = g.borrow_mut();
        match entry {
            Some(e) => {
                g.cachedData[mode.index()].insert(uuid, e);
            }
            None => {
                g.cachedData[mode.index()].shift_remove(&uuid);
            }
        }
    });
}

/// Clear every mode (used by `wipeGlobalCache`, CT).
pub fn clear_all() {
    GlobalCache.with(|g| *g.borrow_mut() = GlobalCacheT::default());
}
