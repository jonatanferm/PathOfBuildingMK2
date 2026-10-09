//! @pob-file Modules/ModParser.lua @16de4b82d
//! ModParser.lua 6994-7021: `modLib.parseMod` (the two-order pass + memo cache, seeded from
//! `data().modCache`) and `parseModCache`.
//!
//! Owner: MP0. Stub created by U0: signatures are final (frozen; change through a CCR),
//! bodies are `unported!` until the owner ports them (calc doc D9, §10.2).
#![allow(
    non_snake_case,
    unused_variables,
    unused_imports,
    dead_code,
    clippy::all,
    clippy::pedantic
)]

use crate::types::*;
use crate::unported;
use pob_gamedata::{
    JewelFuncSpec, KeywordFlag, Mod, ModFlag, ModType, Name, SkillType, Tag, Value,
};
use pob_lua::n;
use std::sync::LazyLock;

/// @pob Modules/ModParser.lua:6994-7021 h=f5001218
/// `local cache, unsupported, count` and the returned module table (`modLib.parseMod`,
/// ModParser.lua 6994-7021): the two-order pass + memo cache, seeded from `data().modCache`.
/// Returns fresh copies on each call (Lua: `unpack(copyTable(cache[line]))`).
pub fn parseMod(line: &str, isComb: bool) -> (Option<Vec<Mod>>, Option<String>) {
    unported!("Modules/ModParser.lua:6994-7021")
}

/// The ModCache tier bypassed (L1 tests).
pub fn parseModNative(line: &str) -> (Option<Vec<Mod>>, Option<String>) {
    unported!("Modules/ModParser.lua:6994-7021")
}
