//! @pob-file Modules/ItemTools.lua @16de4b82d
//! `itemLib` (Modules/ItemTools.lua 1-413).
//!
//! Owner: IT-C. Stub created by U0: signatures are final (frozen; change through a CCR),
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

// @pob Modules/ItemTools.lua:6-34 h=734fcb5c
// (module-level statements: t_insert,t_remove,m_min,m_max,m_floor,itemLib,itemLib.influenceInfo)

/// @pob Modules/ItemTools.lua:37-57 h=42d468ce
pub fn applyValueScalar(
    line: &str,
    valueScalar: Option<f64>,
    baseValueScalar: Option<f64>,
    numbers: Option<f64>,
    precision: Option<f64>,
) -> String {
    unported!("Modules/ItemTools.lua:37-57")
}

/// @pob Modules/ItemTools.lua:61-75 h=c3aa4f2e
pub fn formatValue(
    value: f64,
    baseValueScalar: Option<f64>,
    valueScalar: Option<f64>,
    precision: Option<f64>,
    displayPrecision: Option<f64>,
    ifRequired: bool,
) -> String {
    unported!("Modules/ItemTools.lua:61-75")
}

// @pob Modules/ItemTools.lua:76-81 h=3d97ddd5
// (module-level statements: antonyms)

/// @pob Modules/ItemTools.lua:83-86 h=e7105ba1
pub fn antonymFunc(num: &str, word: &str) -> String {
    unported!("Modules/ItemTools.lua:83-86")
}

/// @pob Modules/ItemTools.lua:88-90 h=40feee29
pub fn isZeroValueLine(line: &str) -> bool {
    unported!("Modules/ItemTools.lua:88-90")
}

/// `range` is a Lua value (CCR-0032 §4): a number, or a list of numbers used per `(x-y)` range
/// (`ranges[rangeIndex] or 0.5`, ItemTools.lua:99/104), as stored by ParseRaw for
/// `{range:a,b}` (Item.lua:803-809); callers pass `modLine.range`, which may be nil.
/// @pob Modules/ItemTools.lua:96-355 h=46779a84
pub fn applyRange(
    line: &str,
    range: &Val, // CCR-0032 §4
    valueScalar: Option<f64>,
    baseValueScalar: Option<f64>,
) -> String {
    unported!("Modules/ItemTools.lua:96-355")
}

/// `range` is passed to `applyRange` unchanged (CCR-0032 §4).
/// @pob Modules/ItemTools.lua:357-359 h=49fbc3b8
pub fn isModLineScalable(line: &str, range: &Val, valueScalar: Option<f64>) -> bool {
    unported!("Modules/ItemTools.lua:357-359")
}

/// @pob Modules/ItemTools.lua:361-382 h=57921347
pub fn formatModLine(modLine: &Table, dbMode: bool) -> Option<String> {
    unported!("Modules/ItemTools.lua:361-382")
}

// @pob Modules/ItemTools.lua:384-413 h=a9b20e36
// (module-level statements: itemLib.wiki)
