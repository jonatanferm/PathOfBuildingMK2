//! The jewel func bodies (ModParser.lua 6151-6615 closures): `func(node, out, data)`; node None = the
//! "finish" call (CalcSetup.lua 259-305). Provenance per func is MP7's (the table statements are
//! marked in pob-modparser/src/jewel_func_list.rs).
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

use crate::prelude::*;

/// `spec.func(node, out, data)`.
pub fn callJewelFunc(
    spec: &pob_gamedata::JewelFuncSpec,
    node: Option<&NodeRef>,
    out: &ModList,
    data: &Table,
) {
    unported!("Modules/ModParser.lua:6151-6615 (jewel funcs)")
}
