//! @pob-file Modules/ModParser.lua @16de4b82d
//! specialModList entries, ModParser.lua 4037-4988 (calc doc §8; provenance per block of about 50
//! entries once ported). `special_a`..`special_d` concatenate, in source order, into `specialModList`.
//!
//! Owner: MP5. Stub created by U0: signatures are final (frozen; change through a CCR),
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

/// @pob Modules/ModParser.lua:4037-4988 h=56820d88
/// The entries of ModParser.lua 4037-4988, in source order.
pub fn entries() -> Vec<PatternEntry<SpecialVal>> {
    unported!("Modules/ModParser.lua:4037-4988")
}
