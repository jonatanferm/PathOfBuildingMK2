//! @pob-file Modules/ModParser.lua @16de4b82d
//! ModParser.lua 6616-6992: `scan` and the local `parseMod(line, order)`.
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

/// @pob Modules/ModParser.lua:6616-6638 h=7a700992
/// ModParser.lua:6616 `scan()`: lowest start, then longest end, then longest pattern; full ties →
/// lowest `lua_line` (§3.4). Returns (value, the line with the match removed, captures).
pub fn scan<'l, T>(
    line: &'l str,
    list: &PatternList<T>,
    plain: bool,
) -> Option<(&'static T, String, Caps<'l>)>
where
    T: Sync + Send + 'static,
{
    unported!("Modules/ModParser.lua:6616-6638")
}

/// @pob Modules/ModParser.lua:6640-6992 h=c2f6fd73
/// The local `parseMod(line, order)` (ModParser.lua:6640): one pass. → (mods, unparsed remainder).
pub fn parseModPass(line: &str, order: u8) -> (Option<Vec<Mod>>, Option<String>) {
    unported!("Modules/ModParser.lua:6640-6992")
}
