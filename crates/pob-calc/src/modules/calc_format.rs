//! @pob-file Modules/CalcFormat.lua @16de4b82d
//! Modules/CalcFormat.lua: calc section cell formatting.
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

// @pob Modules/CalcFormat.lua:10-10 h=04510cb8
// (module-level statements: t_insert)

/// @pob Modules/CalcFormat.lua:12-14 h=a194b6fe
pub fn formatCalcVal(val: f64, p: Option<f64>) -> String {
    unported!("Modules/CalcFormat.lua:12-14")
}

/// @pob Modules/CalcFormat.lua:16-74 h=2ccb71c0
/// `colData` is the calc section column table (UI data, `CalcSections`).
pub fn formatCalcStr(str: &str, actor: Option<&ActorRef>, colData: &Val) -> String {
    unported!("Modules/CalcFormat.lua:16-74")
}
