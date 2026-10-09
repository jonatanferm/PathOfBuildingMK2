//! @pob-file Modules/ModParser.lua @16de4b82d
//! ModParser.lua 1-71 and 2011-2129: module header, `getSimpleConv`, `conquerorList` and the mod
//! constructor helpers (`flag`, `dealNoNonDamageType`, `grantedExtraSkill`, `triggerExtraSkill`,
//! `extraSupport`, `explodeFunc`, `dmgTypes`). `firstToUpper` (13-15) is `pob_lua::string::firstToUpper`.
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

// @pob Modules/ModParser.lua:6-12 h=6fa8e6d6
// (module-level statements: pairs,ipairs,t_insert,band,bor,bnot,m_huge)

/// @pob Modules/ModParser.lua:18-39 h=503b072a
/// Returns the jewel func `function(node, out, data)` as data (MP7 runs it).
pub fn getSimpleConv(
    srcList: &[&str],
    dst: &str,
    r#type: ModType,
    remove: bool,
    factor: Option<f64>,
    srcType: Option<ModType>,
) -> JewelFuncSpec {
    let () = unported!("Modules/ModParser.lua:18-39");
    JewelFuncSpec {
        key: n!("$getSimpleConv"),
        kind: pob_gamedata::JewelFuncKind::Other,
        args: Default::default(),
        r#type: None,
        radius: None,
    }
}

/// @pob Modules/ModParser.lua:41-70 h=e4b94ab4
/// `conquerorList` (ModParser.lua 41-70).
pub static conquerorList: LazyLock<Vec<(Name, Value)>> =
    LazyLock::new(|| unported!("Modules/ModParser.lua:41-70"));

/// @pob Modules/ModParser.lua:2011-2011 h=505e172d
/// `local mod = modLib.createMod` (CCR-0006 §6: generated lists call it with createMod's
/// argument dispatch already resolved).
pub use pob_gamedata::mod_tools::createMod as r#mod;

/// @pob Modules/ModParser.lua:2012-2014 h=880a0d5d
/// `flag(name, ...)` = `mod(name, "FLAG", true, ...)`.
pub fn flag(name: Name, source: Option<&str>, flags: u32, keywordFlags: u32, tags: &[Tag]) -> Mod {
    let () = unported!("Modules/ModParser.lua:2012-2014");
    pob_gamedata::mod_tools::createMod(name, ModType::FLAG, true, source, flags, keywordFlags, tags)
}

// @pob Modules/ModParser.lua:2015-2015 h=fae2d0b6
// (module-level statements: damageTypeList)

/// @pob Modules/ModParser.lua:2018-2031 h=3de6b0b1
pub fn dealNoNonDamageType(dmgType: &str, forMinion: bool) -> Option<Vec<Mod>> {
    unported!("Modules/ModParser.lua:2018-2031")
}

/// @pob Modules/ModParser.lua:2033-2046 h=8e717654
/// The `gemIdLookup` table (built from `data.skills` at load): lowercased skill name → skill id.
pub fn gemIdLookup(name: &str) -> Option<Name> {
    unported!("Modules/ModParser.lua:2033-2046")
}

/// @pob Modules/ModParser.lua:2047-2054 h=3a2daa78
/// `level` is already `tonumber(level)` (CCR-0006 §6).
pub fn grantedExtraSkill(
    name: &str,
    level: Option<f64>,
    noSupports: Option<bool>,
) -> Option<Vec<Mod>> {
    unported!("Modules/ModParser.lua:2047-2054")
}

/// `triggerExtraSkill`'s `options` table (ModParser.lua:2055-2072; CCR-0006 §6).
#[derive(Default, Clone, Debug)]
pub struct TriggerExtraSkillOptions {
    pub noSupports: Option<bool>,
    pub sourceSkill: Option<String>,
    pub triggerChance: Option<f64>,
    pub ignoreHexproof: bool,
    pub onCrit: bool,
}

/// @pob Modules/ModParser.lua:2055-2072 h=9f9268e5
pub fn triggerExtraSkill(
    name: &str,
    level: Option<f64>,
    options: Option<TriggerExtraSkillOptions>,
) -> Vec<Mod> {
    unported!("Modules/ModParser.lua:2055-2072")
}

/// @pob Modules/ModParser.lua:2073-2106 h=b72fdb0b
pub fn extraSupport(
    name: &str,
    level: Option<f64>,
    slot: Option<&str>,
    appliesToGrantedSkills: Option<bool>,
) -> Option<Vec<Mod>> {
    unported!("Modules/ModParser.lua:2073-2106")
}

/// A helper argument the Lua body uses both as a number and as a string (`explodeFunc`'s
/// `amount`; CCR-0006 §6).
#[derive(Clone, Copy, Debug)]
pub enum LuaArg<'a> {
    Nil,
    Num(f64),
    Str(&'a str),
}

/// @pob Modules/ModParser.lua:2108-2119 h=095f070d
/// `explodeFunc(chance, amount, type, ...)` (extra tags as varargs).
pub fn explodeFunc(
    chance: Option<f64>,
    amount: LuaArg,
    r#type: &str,
    tags: &[Tag],
) -> Option<Vec<Mod>> {
    unported!("Modules/ModParser.lua:2108-2119")
}

/// @pob Modules/ModParser.lua:2121-2127 h=53fc767e
/// The `dmgTypes` table: damage type word → mod name stem.
pub fn dmgTypes(name: &str) -> Option<Name> {
    unported!("Modules/ModParser.lua:2121-2127")
}
