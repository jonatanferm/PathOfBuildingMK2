//! @pob-file Classes/ModList.lua @16de4b82d
//! Classes/ModList.lua 1-249 (ModList storage: a flat list). The public methods dispatch on
//! `ModStore::kind`; the ModList bodies are the `*_List` functions here.
//!
//! Owner: MS-A. Stub created by U0: signatures are final (frozen; change through a CCR),
//! bodies are `unported!` until the owner ports them (calc doc D9, §10.2).
#![allow(
    non_snake_case,
    unused_variables,
    unused_imports,
    dead_code,
    clippy::all,
    clippy::pedantic
)]

use crate::prelude::*;

// @pob Classes/ModList.lua:6-20 h=27e6b307
// (module-level statements: ipairs,pairs,select,t_insert,m_floor,m_min,m_max,m_modf,band,bor,mod_createMod,ModListClass)

/// @pob Classes/ModList.lua:22-25 h=835d6b1e
impl ModStore {
    /// `ModList:ModList(parent)`: `ModStore::ModList` (U0) allocates; this runs the constructor body.
    pub(crate) fn init_ModList(&self, parent: Option<&ModStoreRef>) {
        unported!("Classes/ModList.lua:22-25")
    }
}

/// @pob Classes/ModList.lua:27-29 h=67b4b724
impl ModStore {
    pub(crate) fn AddMod_List(&self, m: ModRef) {
        unported!("Classes/ModList.lua:27-29")
    }
}

/// @pob Classes/ModList.lua:36-50 h=e02d7c3c
impl ModStore {
    pub(crate) fn ReplaceModInternal_List(&self, m: ModRef) -> bool {
        unported!("Classes/ModList.lua:36-50")
    }
}

/// @pob Classes/ModList.lua:58-71 h=f3adb218
impl ModStore {
    pub(crate) fn ConvertModInternal_List(&self, oldName: Name, m: ModRef) -> bool {
        unported!("Classes/ModList.lua:58-71")
    }
}

/// @pob Classes/ModList.lua:73-86 h=21dc2f7c
impl ModStore {
    /// ModList only (ModList.lua:73).
    pub fn MergeMod(&self, m: ModRef, skipNonAdditive: bool) {
        unported!("Classes/ModList.lua:73-86")
    }
}

/// @pob Classes/ModList.lua:88-94 h=210fa5c6
impl ModStore {
    pub(crate) fn AddList_List(&self, modList: &[ModRef]) {
        unported!("Classes/ModList.lua:88-94")
    }
}

/// @pob Classes/ModList.lua:96-98 h=a23cd7cc
impl ModStore {
    /// ModList only (ModList.lua:96).
    pub fn MergeNewMod(&self, m: Mod) {
        unported!("Classes/ModList.lua:96-98")
    }
}

/// @pob Classes/ModList.lua:101-120 h=6fa85692
impl ModStore {
    pub(crate) fn SumInternal_List(
        &self,
        context: &ModStore,
        modType: ModType,
        cfg: Option<&ModCfg>,
        flags: u32,
        keywordFlags: u32,
        source: Option<&str>,
        names: &[Name],
    ) -> f64 {
        unported!("Classes/ModList.lua:101-120")
    }
}

/// @pob Classes/ModList.lua:122-154 h=0b941555
impl ModStore {
    pub(crate) fn MoreInternal_List(
        &self,
        context: &ModStore,
        cfg: Option<&ModCfg>,
        flags: u32,
        keywordFlags: u32,
        source: Option<&str>,
        names: &[Name],
    ) -> f64 {
        unported!("Classes/ModList.lua:122-154")
    }
}

/// @pob Classes/ModList.lua:156-175 h=57b1b000
impl ModStore {
    pub(crate) fn FlagInternal_List(
        &self,
        context: &ModStore,
        cfg: Option<&ModCfg>,
        flags: u32,
        keywordFlags: u32,
        source: Option<&str>,
        names: &[Name],
    ) -> bool {
        unported!("Classes/ModList.lua:156-175")
    }
}

/// @pob Classes/ModList.lua:177-197 h=f54918f6
impl ModStore {
    pub(crate) fn OverrideInternal_List(
        &self,
        context: &ModStore,
        cfg: Option<&ModCfg>,
        flags: u32,
        keywordFlags: u32,
        source: Option<&str>,
        names: &[Name],
    ) -> Value {
        unported!("Classes/ModList.lua:177-197")
    }
}

/// @pob Classes/ModList.lua:199-220 h=749601ee
impl ModStore {
    pub(crate) fn ListInternal_List(
        &self,
        context: &ModStore,
        result: &mut Vec<Value>,
        cfg: Option<&ModCfg>,
        flags: u32,
        keywordFlags: u32,
        source: Option<&str>,
        names: &[Name],
    ) {
        unported!("Classes/ModList.lua:199-220")
    }
}

/// @pob Classes/ModList.lua:222-243 h=3e42aaaa
impl ModStore {
    pub(crate) fn TabulateInternal_List(
        &self,
        context: &ModStore,
        result: &mut Vec<TabRow>,
        modType: Option<ModType>,
        cfg: Option<&ModCfg>,
        flags: u32,
        keywordFlags: u32,
        source: Option<&str>,
        names: &[Name],
    ) {
        unported!("Classes/ModList.lua:222-243")
    }
}

/// @pob Classes/ModList.lua:245-249 h=94a08b7a
impl ModStore {
    pub(crate) fn Print_List(&self) {
        unported!("Classes/ModList.lua:245-249")
    }
}

impl ModStore {
    /// `#modList` (ModList).
    pub fn len(&self) -> usize {
        unported!("Classes/ModList.lua:1-249 (len)")
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// `for _, mod in ipairs(modList)` (ModList): `Arc` clones in list order.
    pub fn ipairs(&self) -> Vec<ModRef> {
        unported!("Classes/ModList.lua:1-249 (ipairs)")
    }

    /// `wipeTable(modDB.mods)` etc.
    pub fn wipe(&self) {
        unported!("Classes/ModList.lua:1-249 (wipe)")
    }
}
