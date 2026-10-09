//! @pob-file Modules/CalcSetup.lua @16de4b82d
//! CalcSetup.lua 121-304: node mod lists and jewel functions.
//!
//! Owner: CS0. Stub created by U0: signatures are final (frozen; change through a CCR),
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

/// @pob Modules/CalcSetup.lua:123-256 h=610f22bb
pub fn buildModListForNode(env: &EnvRef, node: &NodeRef, reuse: bool) -> ModList {
    unported!("Modules/CalcSetup.lua:123-256")
}

/// @pob Modules/CalcSetup.lua:259-304 h=6c6f8915
/// `nodeList` is a node-id map (LuaJIT order, D8); `finishJewels` runs the jewel funcs' "finish" call (node = nil).
pub fn buildModListForNodeList(
    env: &EnvRef,
    nodeList: &Shared<LjIntMap<NodeRef>>,
    finishJewels: bool,
) -> ModList {
    unported!("Modules/CalcSetup.lua:259-304")
}
