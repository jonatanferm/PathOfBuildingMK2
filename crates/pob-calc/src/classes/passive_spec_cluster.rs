//! @pob-file Classes/PassiveSpec.lua @16de4b82d
//! Cluster jewel subgraphs and legacy remaps (Classes/PassiveSpec.lua 1697-2494).
//!
//! Owner: PS-C. Stub created by U0: signatures are final (frozen; change through a CCR),
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

/// @pob Classes/PassiveSpec.lua:1697-1718 h=75755a02
impl PassiveSpec {
    pub fn ReplaceNode(&self, old: &NodeRef, newNode: &NodeRef) -> bool {
        unported!("Classes/PassiveSpec.lua:1697-1718")
    }
}

/// @pob Classes/PassiveSpec.lua:1722-1730 h=63ec140a
impl PassiveSpec {
    pub fn ReconnectNodeToClassStart(&self, node: &NodeRef) {
        unported!("Classes/PassiveSpec.lua:1722-1730")
    }
}

/// @pob Classes/PassiveSpec.lua:1734-1739 h=3b29a79d
impl PassiveSpec {
    pub fn BeginLegacyClusterHashConversion(&self) {
        unported!("Classes/PassiveSpec.lua:1734-1739")
    }
}

/// @pob Classes/PassiveSpec.lua:1743-1751 h=ecad0e93
impl PassiveSpec {
    pub fn RegisterLegacyClusterNodeMap(&self, legacyNodeId: f64, currentNodeId: f64) {
        unported!("Classes/PassiveSpec.lua:1743-1751")
    }
}

/// @pob Classes/PassiveSpec.lua:1754-1760 h=57acfb06
impl PassiveSpec {
    pub fn GetMappedClusterNodeId(&self, nodeId: f64) -> f64 {
        unported!("Classes/PassiveSpec.lua:1754-1760")
    }
}

/// @pob Classes/PassiveSpec.lua:1763-1797 h=67976b4c
impl PassiveSpec {
    pub fn ApplyLegacyClusterNodeRemap(&self) {
        unported!("Classes/PassiveSpec.lua:1763-1797")
    }
}

/// @pob Classes/PassiveSpec.lua:1800-1804 h=e335efee
impl PassiveSpec {
    pub fn EndLegacyClusterHashConversion(&self) {
        unported!("Classes/PassiveSpec.lua:1800-1804")
    }
}

/// @pob Classes/PassiveSpec.lua:1806-1880 h=309d9367
impl PassiveSpec {
    pub fn BuildClusterJewelGraphs(&self) {
        unported!("Classes/PassiveSpec.lua:1806-1880")
    }
}

/// @pob Classes/PassiveSpec.lua:1883-1890 h=3accf14a
impl PassiveSpec {
    pub fn FindClusterSocket(&self, group: &Table, index: f64) -> Option<NodeRef> {
        unported!("Classes/PassiveSpec.lua:1883-1890")
    }
}

/// @pob Classes/PassiveSpec.lua:1894-1912 h=1d6b0614
impl PassiveSpec {
    pub fn BuildLegacyProxyGroup(
        &self,
        proxyGroup: &Table,
        expansionJewelSize: f64,
        clusterSizeIndex: f64,
    ) -> Table {
        unported!("Classes/PassiveSpec.lua:1894-1912")
    }
}

/// @pob Classes/PassiveSpec.lua:1916-1933 h=635f363d
impl PassiveSpec {
    pub fn TranslateClusterOrbitIndex(
        &self,
        srcOidx: f64,
        srcNodesPerOrbit: f64,
        destNodesPerOrbit: f64,
    ) -> f64 {
        unported!("Classes/PassiveSpec.lua:1916-1933")
    }
}

/// @pob Classes/PassiveSpec.lua:1936-1941 h=a694ce1f
impl PassiveSpec {
    pub fn ApplyClusterOrbitIndexAdjustment(
        &self,
        indicies: &Table,
        startOidx: f64,
        clusterTotalIndicies: f64,
        skillsPerOrbit: f64,
    ) -> Table {
        unported!("Classes/PassiveSpec.lua:1936-1941")
    }
}

/// @pob Classes/PassiveSpec.lua:1944-1968 h=61d1b75c
impl PassiveSpec {
    pub fn BuildLegacyClusterOrbitMappings(
        &self,
        indicies: &Table,
        proxyNode: &NodeRef,
        clusterTotalIndicies: f64,
        skillsPerOrbit: f64,
    ) -> Table {
        unported!("Classes/PassiveSpec.lua:1944-1968")
    }
}

/// @pob Classes/PassiveSpec.lua:1970-2359 h=3c5172e7
impl PassiveSpec {
    pub fn BuildSubgraph(
        &self,
        jewel: &ItemRef,
        parentSocket: &NodeRef,
        id: Option<f64>,
        upSize: Option<f64>,
        importedNodes: Option<&Table>,
        importedGroups: Option<&Table>,
    ) {
        unported!("Classes/PassiveSpec.lua:1970-2359")
    }
}

/// @pob Classes/PassiveSpec.lua:2361-2383 h=11f62e74
impl PassiveSpec {
    pub fn CreateUndoState(&self) -> Table {
        unported!("Classes/PassiveSpec.lua:2361-2383")
    }
}

/// @pob Classes/PassiveSpec.lua:2385-2388 h=9d8437a6
impl PassiveSpec {
    pub fn RestoreUndoState(&self, state: &Table, treeVersion: Option<Name>) {
        unported!("Classes/PassiveSpec.lua:2385-2388")
    }
}

/// @pob Classes/PassiveSpec.lua:2390-2396 h=4d7d27cb
impl PassiveSpec {
    /// UI only (window title).
    pub fn SetWindowTitleWithBuildClass(&self) {
        unported!("Classes/PassiveSpec.lua:2390-2396")
    }
}

/// @pob Classes/PassiveSpec.lua:2402-2483 h=5a99512a
impl PassiveSpec {
    pub fn NodeAdditionOrReplacementFromString(&self, node: &NodeRef, sd: &str, replacement: bool) {
        unported!("Classes/PassiveSpec.lua:2402-2483")
    }
}

/// @pob Classes/PassiveSpec.lua:2485-2494 h=ecbda051
impl PassiveSpec {
    pub fn NodeInKeystoneRadius(
        &self,
        keystoneNames: &Table,
        nodeId: f64,
        radiusIndex: f64,
    ) -> bool {
        unported!("Classes/PassiveSpec.lua:2485-2494")
    }
}
