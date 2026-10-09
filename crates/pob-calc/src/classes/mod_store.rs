//! @pob-file Classes/ModStore.lua @16de4b82d
//! Classes/ModStore.lua 1-362: the shared ModStore methods (calc doc §5.3). The struct and the
//! constructors are U0's (`classes/mod.rs`).
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

// @pob Classes/ModStore.lua:6-38 h=6234244b
// (module-level statements: ipairs,pairs,select,t_insert,m_floor,m_ceil,m_min,m_max,m_modf,band,bor,mod_createMod,multiplierName…)

/// @pob Classes/ModStore.lua:40-46 h=abc6532b
impl ModStore {
    /// `ModStore:ModStore(parent)`: the field initialisation is `ModStore::ModDB`/`ModList` (U0).
    pub(crate) fn init_ModStore(&self, parent: Option<&ModStoreRef>) {
        unported!("Classes/ModStore.lua:40-46")
    }
}

/// @pob Classes/ModStore.lua:56-91 h=664c3a90
impl ModStore {
    pub fn ScaleAddMod(&self, m: &ModRef, scale: f64, replace: bool) {
        unported!("Classes/ModStore.lua:56-91")
    }
}

/// @pob Classes/ModStore.lua:93-97 h=d852294b
impl ModStore {
    pub fn CopyList(&self, modList: &[ModRef]) {
        unported!("Classes/ModStore.lua:93-97")
    }
}

/// @pob Classes/ModStore.lua:99-107 h=ffb52d14
impl ModStore {
    pub fn ScaleAddList(&self, modList: &[ModRef], scale: f64, replace: bool) {
        unported!("Classes/ModStore.lua:99-107")
    }
}

/// @pob Classes/ModStore.lua:109-111 h=04a2b779
impl ModStore {
    pub fn NewMod(
        &self,
        name: Name,
        ty: ModType,
        value: impl Into<Value>,
        source: &str,
        flags: u32,
        keywordFlags: u32,
        tags: &[Tag],
    ) {
        unported!("Classes/ModStore.lua:109-111")
    }
}

/// @pob Classes/ModStore.lua:125-130 h=2fc96789
impl ModStore {
    pub fn ReplaceMod(
        &self,
        name: Name,
        ty: ModType,
        value: impl Into<Value>,
        source: &str,
        flags: u32,
        keywordFlags: u32,
        tags: &[Tag],
    ) {
        unported!("Classes/ModStore.lua:125-130")
    }
}

/// @pob Classes/ModStore.lua:138-143 h=60847b01
impl ModStore {
    pub fn ConvertMod(
        &self,
        oldName: Name,
        name: Name,
        ty: ModType,
        value: impl Into<Value>,
        source: &str,
        flags: u32,
        keywordFlags: u32,
        tags: &[Tag],
    ) {
        unported!("Classes/ModStore.lua:138-143")
    }
}

/// @pob Classes/ModStore.lua:145-159 h=d72a3c2d
impl ModStore {
    pub fn Combine(&self, modType: ModType, cfg: Option<&ModCfg>, names: &[Name]) -> Val {
        unported!("Classes/ModStore.lua:145-159")
    }
}

/// @pob Classes/ModStore.lua:165-174 h=3b896425
impl ModStore {
    pub fn Sum(&self, modType: ModType, cfg: Option<&ModCfg>, names: &[Name]) -> f64 {
        unported!("Classes/ModStore.lua:165-174")
    }
}

/// @pob Classes/ModStore.lua:179-188 h=ea04c772
impl ModStore {
    pub fn More(&self, cfg: Option<&ModCfg>, names: &[Name]) -> f64 {
        unported!("Classes/ModStore.lua:179-188")
    }
}

/// @pob Classes/ModStore.lua:190-199 h=51887969
impl ModStore {
    pub fn Flag(&self, cfg: Option<&ModCfg>, names: &[Name]) -> bool {
        unported!("Classes/ModStore.lua:190-199")
    }
}

/// @pob Classes/ModStore.lua:204-213 h=b70d870c
impl ModStore {
    /// `Value::Nil` = nil.
    pub fn Override(&self, cfg: Option<&ModCfg>, names: &[Name]) -> Value {
        unported!("Classes/ModStore.lua:204-213")
    }
}

/// @pob Classes/ModStore.lua:218-229 h=19bfdfa0
impl ModStore {
    pub fn List(&self, cfg: Option<&ModCfg>, names: &[Name]) -> Vec<Value> {
        unported!("Classes/ModStore.lua:218-229")
    }
}

/// @pob Classes/ModStore.lua:235-246 h=91103769
impl ModStore {
    pub fn Tabulate(
        &self,
        modType: Option<ModType>,
        cfg: Option<&ModCfg>,
        names: &[Name],
    ) -> Vec<TabRow> {
        unported!("Classes/ModStore.lua:235-246")
    }
}

/// @pob Classes/ModStore.lua:248-257 h=49b95c53
impl ModStore {
    /// nil unless some value > 0 (ModStore.lua:248-257, §3.5).
    pub fn Max(&self, cfg: Option<&ModCfg>, names: &[Name]) -> Option<f64> {
        unported!("Classes/ModStore.lua:248-257")
    }
}

/// @pob Classes/ModStore.lua:259-268 h=62eb650f
impl ModStore {
    pub fn Min(&self, cfg: Option<&ModCfg>, names: &[Name]) -> Option<f64> {
        unported!("Classes/ModStore.lua:259-268")
    }
}

/// @pob Classes/ModStore.lua:278-287 h=c61b0ba6
impl ModStore {
    /// ModDB only: on a ModList Lua errors (no HasModInternal), so this raises LuaError.
    pub fn HasMod(&self, modType: ModType, cfg: Option<&ModCfg>, names: &[Name]) -> bool {
        unported!("Classes/ModStore.lua:278-287")
    }
}

/// @pob Classes/ModStore.lua:293-295 h=1f193e4f
impl ModStore {
    pub fn GetCondition(&self, var: Name, cfg: Option<&ModCfg>, noMod: bool) -> bool {
        unported!("Classes/ModStore.lua:293-295")
    }
}

/// @pob Classes/ModStore.lua:301-303 h=bd51854d
impl ModStore {
    pub fn GetMultiplier(&self, var: Name, cfg: Option<&ModCfg>, noMod: bool) -> f64 {
        unported!("Classes/ModStore.lua:301-303")
    }
}

/// @pob Classes/ModStore.lua:308-357 h=62a1c1f8
impl ModStore {
    pub fn GetStat(&self, stat: Name, cfg: Option<&ModCfg>) -> f64 {
        unported!("Classes/ModStore.lua:308-357")
    }
}

// ---- Rust-only accessors (oracle, injection, UI) ----

impl ModStore {
    /// The mods stored under `name` (ModDB) / with that name (ModList), in store order.
    pub fn mods_named(&self, name: Name) -> Vec<ModRef> {
        unported!("Classes/ModStore.lua:1-362 (mods_named)")
    }

    /// Every mod, in store order (ModDB: name order is insertion order of names).
    pub fn all_mods(&self) -> Vec<ModRef> {
        unported!("Classes/ModStore.lua:1-362 (all_mods)")
    }
}
