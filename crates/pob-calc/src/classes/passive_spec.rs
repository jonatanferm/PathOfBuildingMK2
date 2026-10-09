//! @pob-file Classes/PassiveSpec.lua @16de4b82d
//! Classes/PassiveSpec.lua 1-1091 (spec core: load/save, URLs, allocation).
//!
//! Owner: PS-A. Stub created by U0: signatures are final (frozen; change through a CCR),
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

// @pob Classes/PassiveSpec.lua:7-22 h=2bd6e633
// (module-level statements: pairs,ipairs,t_insert,t_remove,m_min,m_max,m_floor,b_lshift,b_rshift,band,bor,PassiveSpecClass)

/// @pob Classes/PassiveSpec.lua:27-37 h=78cda35b
/// `new("PassiveSpec", build, treeVersion, convert)`.
pub fn new_PassiveSpec(build: &BuildRef, treeVersion: Name, convert: bool) -> PassiveSpecRef {
    unported!("Classes/PassiveSpec.lua:27-37")
}

/// @pob Classes/PassiveSpec.lua:39-101 h=633a551e
impl PassiveSpec {
    pub fn Init(&self, treeVersion: Name, convert: bool) {
        unported!("Classes/PassiveSpec.lua:39-101")
    }
}

/// @pob Classes/PassiveSpec.lua:103-195 h=7ca95d98
impl PassiveSpec {
    pub fn Load(&self, xml: &crate::model::XmlNode, dbFileName: &str) {
        unported!("Classes/PassiveSpec.lua:103-195")
    }
}

/// @pob Classes/PassiveSpec.lua:197-249 h=28e40f84
impl PassiveSpec {
    pub fn Save(&self, xml: &mut crate::model::XmlNode) {
        unported!("Classes/PassiveSpec.lua:197-249")
    }
}

/// @pob Classes/PassiveSpec.lua:251-253 h=35cc6604
impl PassiveSpec {
    pub fn PostLoad(&self) {
        unported!("Classes/PassiveSpec.lua:251-253")
    }
}

/// @pob Classes/PassiveSpec.lua:256-312 h=836ed7ce
impl PassiveSpec {
    pub fn ImportFromNodeList(
        &self,
        className: Option<Name>,
        classId: f64,
        ascendClassId: f64,
        secondaryAscendClassId: f64,
        hashList: &[f64],
        hashOverrides: &Table,
        masteryEffects: &Table,
        treeVersion: Option<Name>,
    ) {
        unported!("Classes/PassiveSpec.lua:256-312")
    }
}

/// @pob Classes/PassiveSpec.lua:314-331 h=226da948
impl PassiveSpec {
    pub fn AllocateDecodedNodes(&self, nodes: &[f64], isCluster: bool, endian: &str) {
        unported!("Classes/PassiveSpec.lua:314-331")
    }
}

/// @pob Classes/PassiveSpec.lua:333-374 h=641deb19
impl PassiveSpec {
    pub fn AllocateMasteryEffects(&self, masteryEffects: &[f64], endian: &str) {
        unported!("Classes/PassiveSpec.lua:333-374")
    }
}

/// @pob Classes/PassiveSpec.lua:377-477 h=7d56c8dc
impl PassiveSpec {
    /// → (error message, tree version only).
    pub fn DecodePoePlannerURL(&self, url: &str, return_tree_version_only: bool) -> Option<String> {
        unported!("Classes/PassiveSpec.lua:377-477")
    }
}

/// @pob Classes/PassiveSpec.lua:480-523 h=3dcb536a
impl PassiveSpec {
    /// → error message (nil on success).
    pub fn DecodeURL(&self, url: &str) -> Option<String> {
        unported!("Classes/PassiveSpec.lua:480-523")
    }
}

/// @pob Classes/PassiveSpec.lua:527-570 h=8fec1270
impl PassiveSpec {
    pub fn EncodeURL(&self, prefix: Option<&str>) -> String {
        unported!("Classes/PassiveSpec.lua:527-570")
    }
}

/// @pob Classes/PassiveSpec.lua:573-596 h=98421186
impl PassiveSpec {
    pub fn SelectClass(&self, classId: f64) {
        unported!("Classes/PassiveSpec.lua:573-596")
    }
}

/// @pob Classes/PassiveSpec.lua:598-608 h=e5ac1323
impl PassiveSpec {
    pub fn ResetAscendClass(&self) {
        unported!("Classes/PassiveSpec.lua:598-608")
    }
}

/// @pob Classes/PassiveSpec.lua:610-628 h=1ba91446
impl PassiveSpec {
    pub fn SelectAscendClass(&self, ascendClassId: f64) {
        unported!("Classes/PassiveSpec.lua:610-628")
    }
}

/// @pob Classes/PassiveSpec.lua:630-666 h=f74d7e40
impl PassiveSpec {
    pub fn SelectSecondaryAscendClass(&self, ascendClassId: f64) {
        unported!("Classes/PassiveSpec.lua:630-666")
    }
}

/// @pob Classes/PassiveSpec.lua:670-691 h=9451597a
impl PassiveSpec {
    pub fn IsClassConnected(&self, classId: f64) -> bool {
        unported!("Classes/PassiveSpec.lua:670-691")
    }
}

/// @pob Classes/PassiveSpec.lua:694-765 h=540a823c
impl PassiveSpec {
    pub fn ConnectToClass(&self, classId: f64) {
        unported!("Classes/PassiveSpec.lua:694-765")
    }
}

/// @pob Classes/PassiveSpec.lua:768-776 h=e76c08e6
impl PassiveSpec {
    pub fn ResetNodes(&self) {
        unported!("Classes/PassiveSpec.lua:768-776")
    }
}

/// @pob Classes/PassiveSpec.lua:781-818 h=961561ce
impl PassiveSpec {
    pub fn AllocNode(&self, node: &NodeRef, altPath: Option<&Shared<Vec<NodeRef>>>) {
        unported!("Classes/PassiveSpec.lua:781-818")
    }
}

/// @pob Classes/PassiveSpec.lua:820-827 h=2bef35bc
impl PassiveSpec {
    pub fn DeallocSingleNode(&self, node: &NodeRef) {
        unported!("Classes/PassiveSpec.lua:820-827")
    }
}

/// @pob Classes/PassiveSpec.lua:830-843 h=50673e99
impl PassiveSpec {
    pub fn DeallocNode(&self, node: &NodeRef) {
        unported!("Classes/PassiveSpec.lua:830-843")
    }
}

/// @pob Classes/PassiveSpec.lua:846-866 h=d2ab6728
impl PassiveSpec {
    /// → (used, ascUsed, secondaryAscUsed, sockets, weaponSet1Used, weaponSet2Used).
    pub fn CountAllocNodes(&self) -> (f64, f64, f64, f64, f64, f64) {
        unported!("Classes/PassiveSpec.lua:846-866")
    }
}

/// @pob Classes/PassiveSpec.lua:870-899 h=d3187816
impl PassiveSpec {
    pub fn FindStartFromNode(&self, node: &NodeRef, visited: &Table, noAscend: bool) -> bool {
        unported!("Classes/PassiveSpec.lua:870-899")
    }
}

/// @pob Classes/PassiveSpec.lua:901-910 h=4df6fc8b
impl PassiveSpec {
    pub fn GetJewel(&self, itemId: Option<f64>) -> Option<ItemRef> {
        unported!("Classes/PassiveSpec.lua:901-910")
    }
}

/// @pob Classes/PassiveSpec.lua:912-921 h=63afb853
impl PassiveSpec {
    pub fn GetSocketedJewel(&self, nodeId: f64) -> (Option<ItemRef>, Option<Table>) {
        unported!("Classes/PassiveSpec.lua:912-921")
    }
}

/// @pob Classes/PassiveSpec.lua:925-964 h=0b0f9e48
impl PassiveSpec {
    pub fn SetNodeDistanceToClassStart(&self, root: &NodeRef) {
        unported!("Classes/PassiveSpec.lua:925-964")
    }
}

/// @pob Classes/PassiveSpec.lua:968-1009 h=b51b3a8c
impl PassiveSpec {
    pub fn GetShortestPathToClassStart(&self, rootId: f64) -> Option<Shared<Vec<NodeRef>>> {
        unported!("Classes/PassiveSpec.lua:968-1009")
    }
}

/// @pob Classes/PassiveSpec.lua:1011-1025 h=47124b86
impl PassiveSpec {
    pub fn BuildSplitPersonalityPath(&self) {
        unported!("Classes/PassiveSpec.lua:1011-1025")
    }
}

/// @pob Classes/PassiveSpec.lua:1029-1060 h=4c8cbe9d
impl PassiveSpec {
    pub fn AddMasteryEffectOptionsToNode(&self, node: &NodeRef) {
        unported!("Classes/PassiveSpec.lua:1029-1060")
    }
}

/// @pob Classes/PassiveSpec.lua:1061-1089 h=97a87698
impl PassiveSpec {
    pub fn NodesInIntuitiveLeapLikeRadius(&self, node: &NodeRef) -> Vec<NodeRef> {
        unported!("Classes/PassiveSpec.lua:1061-1089")
    }
}
