//! @pob-file Modules/ModParser.lua @16de4b82d
//! ModParser.lua 6151-6615: the jewel func tables (`jewelOtherFuncs`, `jewelSelfFuncs`,
//! `jewelSelfUnallocFuncs`, `jewelThresholdFuncs`), `getPerStat`, `getThreshold`, `jewelFuncList` and
//! `clusterJewelSkills`. The parser emits `Value::JewelFunc(JewelFuncSpec)`; the bodies run in
//! `pob_calc::modules::jewel_funcs::callJewelFunc`.
//!
//! Owner: MP7. Stub created by U0: signatures are final (frozen; change through a CCR),
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

/// @pob Modules/ModParser.lua:6151-6366 h=86149ab2
/// `jewelOtherFuncs` (6151-6366): key → (pattern, spec).
pub static jewelOtherFuncs: LazyLock<Vec<(&'static str, JewelFuncSpec)>> =
    LazyLock::new(|| unported!("Modules/ModParser.lua:6151-6366"));

/// @pob Modules/ModParser.lua:6369-6377 h=00a599ce
pub fn getPerStat(
    dst: &str,
    modType: ModType,
    flags: u32,
    stat: &str,
    factor: f64,
) -> JewelFuncSpec {
    let () = unported!("Modules/ModParser.lua:6369-6377");
    JewelFuncSpec {
        key: n!("$getPerStat"),
        kind: pob_gamedata::JewelFuncKind::Other,
        args: Default::default(),
        r#type: None,
        radius: None,
    }
}

/// @pob Modules/ModParser.lua:6378-6452 h=70ab9ca8
/// `jewelSelfFuncs`, `jewelSelfUnallocFuncs` (6378-6452).
pub static jewelSelfFuncs: LazyLock<Vec<(&'static str, JewelFuncSpec)>> =
    LazyLock::new(|| unported!("Modules/ModParser.lua:6378-6452"));

/// @pob Modules/ModParser.lua:6455-6479 h=51ba613c
pub fn getThreshold(
    attrib: &str,
    name: &str,
    modType: ModType,
    value: Value,
    tags: &[Tag],
) -> JewelFuncSpec {
    let () = unported!("Modules/ModParser.lua:6455-6479");
    JewelFuncSpec {
        key: n!("$getThreshold"),
        kind: pob_gamedata::JewelFuncKind::Threshold,
        args: Default::default(),
        r#type: None,
        radius: None,
    }
}

/// @pob Modules/ModParser.lua:6480-6607 h=479cd450
/// `jewelThresholdFuncs`, `jewelFuncList` and `clusterJewelSkills` (6480-6607).
pub static jewelFuncList: LazyLock<Vec<(&'static str, JewelFuncSpec)>> =
    LazyLock::new(|| unported!("Modules/ModParser.lua:6480-6607"));
