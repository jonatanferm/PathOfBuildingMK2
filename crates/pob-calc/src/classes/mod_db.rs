//! @pob-file Classes/ModDB.lua @16de4b82d
//! Classes/ModDB.lua 1-370 (ModDB storage: mods by name).
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

// @pob Classes/ModDB.lua:6-21 h=26072e59
// (module-level statements: ipairs,pairs,select,t_insert,t_remove,m_floor,m_min,m_max,m_modf,band,bor,mod_createMod,ModDBClass)

/// @pob Classes/ModDB.lua:23-27 h=eda70c53
impl ModStore {
    /// `ModDB:ModDB(parent)`: `ModStore::ModDB` (U0) allocates; this runs the constructor body.
    pub(crate) fn init_ModDB(&self, parent: Option<&ModStoreRef>) {
        unported!("Classes/ModDB.lua:23-27")
    }
}

/// @pob Classes/ModDB.lua:29-35 h=fb01ff8f
impl ModStore {
    pub fn AddMod(&self, m: ModRef) {
        unported!("Classes/ModDB.lua:29-35")
    }
}

/// @pob Classes/ModDB.lua:42-71 h=287b0e79
impl ModStore {
    pub(crate) fn ReplaceModInternal_DB(&self, m: ModRef) -> bool {
        unported!("Classes/ModDB.lua:42-71")
    }
}

/// @pob Classes/ModDB.lua:80-110 h=11ad37e7
impl ModStore {
    pub(crate) fn ConvertModInternal_DB(&self, oldName: Name, m: ModRef) -> bool {
        unported!("Classes/ModDB.lua:80-110")
    }
}

/// @pob Classes/ModDB.lua:112-121 h=96951221
impl ModStore {
    pub fn AddList(&self, modList: &[ModRef]) {
        unported!("Classes/ModDB.lua:112-121")
    }
}

/// @pob Classes/ModDB.lua:123-134 h=94daa6cf
impl ModStore {
    pub fn AddDB(&self, modDB: &ModStore) {
        unported!("Classes/ModDB.lua:123-134")
    }
}

/// @pob Classes/ModDB.lua:136-162 h=09eea2a7
impl ModStore {
    pub(crate) fn SumInternal_DB(
        &self,
        context: &ModStore,
        modType: ModType,
        cfg: Option<&ModCfg>,
        flags: u32,
        keywordFlags: u32,
        source: Option<&str>,
        names: &[Name],
    ) -> f64 {
        unported!("Classes/ModDB.lua:136-162")
    }
}

/// @pob Classes/ModDB.lua:164-204 h=c1f2fb20
impl ModStore {
    pub(crate) fn MoreInternal_DB(
        &self,
        context: &ModStore,
        cfg: Option<&ModCfg>,
        flags: u32,
        keywordFlags: u32,
        source: Option<&str>,
        names: &[Name],
    ) -> f64 {
        unported!("Classes/ModDB.lua:164-204")
    }
}

/// @pob Classes/ModDB.lua:206-227 h=c2d7c64d
impl ModStore {
    pub(crate) fn FlagInternal_DB(
        &self,
        context: &ModStore,
        cfg: Option<&ModCfg>,
        flags: u32,
        keywordFlags: u32,
        source: Option<&str>,
        names: &[Name],
    ) -> bool {
        unported!("Classes/ModDB.lua:206-227")
    }
}

/// @pob Classes/ModDB.lua:229-251 h=4ad09d43
impl ModStore {
    pub(crate) fn OverrideInternal_DB(
        &self,
        context: &ModStore,
        cfg: Option<&ModCfg>,
        flags: u32,
        keywordFlags: u32,
        source: Option<&str>,
        names: &[Name],
    ) -> Value {
        unported!("Classes/ModDB.lua:229-251")
    }
}

/// @pob Classes/ModDB.lua:253-276 h=14edd639
impl ModStore {
    pub(crate) fn ListInternal_DB(
        &self,
        context: &ModStore,
        result: &mut Vec<Value>,
        cfg: Option<&ModCfg>,
        flags: u32,
        keywordFlags: u32,
        source: Option<&str>,
        names: &[Name],
    ) {
        unported!("Classes/ModDB.lua:253-276")
    }
}

/// @pob Classes/ModDB.lua:278-306 h=062d225c
impl ModStore {
    pub(crate) fn TabulateInternal_DB(
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
        unported!("Classes/ModDB.lua:278-306")
    }
}

/// @pob Classes/ModDB.lua:315-334 h=bbffab01
impl ModStore {
    pub(crate) fn HasModInternal_DB(
        &self,
        modType: ModType,
        flags: u32,
        keywordFlags: u32,
        source: Option<&str>,
        names: &[Name],
    ) -> bool {
        unported!("Classes/ModDB.lua:315-334")
    }
}

// @pob Classes/ModDB.lua:336-370 h=2097509e
// (module-level statements: ModDBClass.Print)
