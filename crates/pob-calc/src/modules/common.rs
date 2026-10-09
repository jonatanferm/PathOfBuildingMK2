//! @pob-file Modules/Common.lua @16de4b82d
//! Modules/Common.lua 477-566 and 897-962: table helpers, the skill UUID and the GlobalCache.
//! `copyTable` / `wipeTable` are `Table::copyTable` / `Table::wipe` (U0, lua_table.rs).
//!
//! Owner: CT. Stub created by U0: signatures are final (frozen; change through a CCR),
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

/// @pob Modules/Common.lua:488-510 h=22168b4d
/// `copyTableSafe(tbl, noRecurse, preserveMeta, isSubTable)` (inside a `do` block with its
/// `subTableMap` memo: cycles map to the same copy).
pub fn copyTableSafe(tbl: &Table, noRecurse: bool, preserveMeta: bool) -> Table {
    unported!("Modules/Common.lua:488-510")
}

/// @pob Modules/Common.lua:512-522 h=bf7c004f
pub fn mergeDB(srcDB: &ModDB, modDB: Option<&ModDB>) {
    unported!("Modules/Common.lua:512-522")
}

/// @pob Modules/Common.lua:524-543 h=222b93ee
/// → (modDB, enemyDB, minionDB). Lua reassigns the fresh stores' `conditions`/`multipliers` to
/// copies; the port fills the fresh stores' (unaliased) tables, which is equivalent.
pub fn specCopy(env: &EnvRef) -> (ModDB, ModDB, Option<ModDB>) {
    unported!("Modules/Common.lua:524-543")
}

/// @pob Modules/Common.lua:560-566 h=3d9ff30f
pub fn isValueInTable(tbl: &Table, val: &Val) -> Option<Key> {
    unported!("Modules/Common.lua:560-566")
}

/// @pob Modules/Common.lua:897-921 h=e90b67af
pub fn cacheSkillUUID(skill: &ActiveSkillRef, env: &EnvRef) -> Name {
    unported!("Modules/Common.lua:897-921")
}

/// @pob Modules/Common.lua:924-941 h=6ebae9ec
pub fn cacheData(uuid: Name, env: &EnvRef) {
    unported!("Modules/Common.lua:924-941")
}

/// @pob Modules/Common.lua:944-948 h=56bb634a
pub fn wipeGlobalCache() {
    unported!("Modules/Common.lua:944-948")
}

/// @pob Modules/Common.lua:951-958 h=f2a0525b
pub fn supportEnabled(skillName: Name, activeSkill: &ActiveSkillRef) -> bool {
    unported!("Modules/Common.lua:951-958")
}
