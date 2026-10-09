//! @pob-file Modules/ModParser.lua @16de4b82d
//! ModParser.lua 5943-6150: the generated specialModList entries (keystones, anchoring), the suffix,
//! penetration, resource, regen/degen/cost and flag type tables, and the skill name lists.
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

// @pob Modules/ModParser.lua:5943-6034 h=bd141394
// (module-level statements: GenFor,GenFor,oldList,specialModList,GenFor,unsupportedModList,suffixTypes,penTypes,resourceTypes,Do)

/// @pob Modules/ModParser.lua:6035-6048 h=37d44b85
pub fn appendMod(inputTable: &[(Name, Value)], string: &str) -> Vec<(Name, Value)> {
    unported!("Modules/ModParser.lua:6035-6048")
}

// @pob Modules/ModParser.lua:6049-6149 h=e4dc5684
// (module-level statements: regenTypes,degenTypes,costTypes,baseCostTypes,flagTypes,skillNameList,preSkillNameList,GenFor)
