//! @pob-file Modules/ModParser.lua @16de4b82d
//! ModParser.lua 1102-1319: preFlagList (7 closures).
//!
//! Owner: MP2. Stub created by U0: signatures are final (frozen; change through a CCR),
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

/// @pob Modules/ModParser.lua:1102-1317 h=7fc3a4c3
/// Built once in source order (calc doc §6).
pub static preFlagList: LazyLock<PatternList<TagVal>> =
    LazyLock::new(|| unported!("Modules/ModParser.lua:1102-1317"));
