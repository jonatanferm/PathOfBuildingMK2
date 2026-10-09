//! @pob-file Classes/PassiveTree.lua @16de4b82d
//! @pob-file Modules/Data.lua @16de4b82d
//! Classes/PassiveTree.lua 1-908, 1041-1062; `data.setJewelRadiiGlobally` (Modules/Data.lua 583-600).
//!
//! Owner: PT. Stub created by U0: signatures are final (frozen; change through a CCR),
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

// @pob Classes/PassiveTree.lua:8-34 h=b3938374
// (module-level statements: pairs,ipairs,t_insert,t_remove,m_min,m_max,m_pi,m_rad,m_sin,m_cos,m_tan,m_sqrt,classArt,legacySkills…)

/// @pob Classes/PassiveTree.lua:39-50 h=4a9b89ca
pub fn getFile(URL: &str) -> Option<Vec<u8>> {
    unported!("Classes/PassiveTree.lua:39-50")
}

// @pob Classes/PassiveTree.lua:73-73 h=1ec45aa6
// (module-level statements: PassiveTreeClass)

/// @pob Classes/PassiveTree.lua:75-793 h=4485adba
/// `new("PassiveTree", treeVersion)`: loads the tree from `pob_gamedata::tree_bytes(treeVersion)`.
pub fn new_PassiveTree(treeVersion: Name) -> PassiveTreeRef {
    unported!("Classes/PassiveTree.lua:75-793")
}

/// @pob Classes/PassiveTree.lua:795-872 h=cfbf0e0d
impl PassiveTree {
    pub fn ProcessStats(&self, node: &NodeRef, startIndex: Option<f64>) {
        unported!("Classes/PassiveTree.lua:795-872")
    }
}

/// @pob Classes/PassiveTree.lua:875-906 h=0861451c
impl PassiveTree {
    pub fn ProcessNode(&self, node: &NodeRef) {
        unported!("Classes/PassiveTree.lua:875-906")
    }
}

/// @pob Classes/PassiveTree.lua:1041-1062 h=728596d0
impl PassiveTree {
    pub fn CalcOrbitAngles(&self, nodesInOrbit: f64) -> Vec<f64> {
        unported!("Classes/PassiveTree.lua:1041-1062")
    }
}

// @pob Modules/Data.lua:583-600 h=4debba84
// (module-level statements: data.setJewelRadiiGlobally)
