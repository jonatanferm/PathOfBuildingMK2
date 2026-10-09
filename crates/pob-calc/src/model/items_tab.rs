//! @pob-file Classes/ItemsTab.lua @16de4b82d
//! ItemsTab model (Classes/ItemsTab.lua 1216-1727, 2998-3010).
//!
//! Owner: BM-A. Stub created by U0: signatures are final (frozen; change through a CCR),
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

/// @pob Classes/ItemsTab.lua:1216-1327 h=00865e77
impl ItemsTab {
    pub fn Load(&self, build: &BuildRef, xml: &crate::model::XmlNode, dbFileName: &str) {
        unported!("Classes/ItemsTab.lua:1216-1327")
    }
}

/// @pob Classes/ItemsTab.lua:1329-1417 h=8d12a9b2
impl ItemsTab {
    pub fn Save(&self, build: &BuildRef, xml: &mut crate::model::XmlNode) {
        unported!("Classes/ItemsTab.lua:1329-1417")
    }
}

/// @pob Classes/ItemsTab.lua:1419-1570 h=5d67df2f
impl ItemsTab {
    /// UI drawing (kept for provenance; the UI port owns it).
    pub fn Draw(&self, build: &BuildRef) {
        unported!("Classes/ItemsTab.lua:1419-1570")
    }
}

/// @pob Classes/ItemsTab.lua:1573-1588 h=e9334233
impl ItemsTab {
    pub fn NewItemSet(&self, itemSetId: Option<f64>) -> Table {
        unported!("Classes/ItemsTab.lua:1573-1588")
    }
}

/// @pob Classes/ItemsTab.lua:1591-1617 h=3a9e0e62
impl ItemsTab {
    pub fn SetActiveItemSet(&self, build: &BuildRef, itemSetId: f64) {
        unported!("Classes/ItemsTab.lua:1591-1617")
    }
}

/// @pob Classes/ItemsTab.lua:1620-1649 h=8b04062e
impl ItemsTab {
    pub fn EquipItemInSet(&self, build: &BuildRef, item: &ItemRef, itemSetId: f64) {
        unported!("Classes/ItemsTab.lua:1620-1649")
    }
}

/// @pob Classes/ItemsTab.lua:1652-1656 h=b3fa8787
impl ItemsTab {
    pub fn PopulateSlots(&self, build: &BuildRef) {
        unported!("Classes/ItemsTab.lua:1652-1656")
    }
}

/// @pob Classes/ItemsTab.lua:1659-1678 h=9311599f
impl ItemsTab {
    pub fn UpdateSockets(&self, build: &BuildRef) {
        unported!("Classes/ItemsTab.lua:1659-1678")
    }
}

/// @pob Classes/ItemsTab.lua:1681-1683 h=5d7fe975
impl ItemsTab {
    pub fn GetSocketAndJewelForNodeID(&self, nodeId: f64) -> (Option<Table>, Option<ItemRef>) {
        unported!("Classes/ItemsTab.lua:1681-1683")
    }
}

/// @pob Classes/ItemsTab.lua:1686-1724 h=d04c87d8
impl ItemsTab {
    pub fn AddItem(
        &self,
        build: &BuildRef,
        item: &ItemRef,
        noAutoEquip: bool,
        index: Option<usize>,
    ) {
        unported!("Classes/ItemsTab.lua:1686-1724")
    }
}

/// @pob Classes/ItemsTab.lua:2998-3005 h=28941651
impl ItemsTab {
    pub fn getMissingAnointCount(&self, item: &ItemRef) -> f64 {
        unported!("Classes/ItemsTab.lua:2998-3005")
    }
}
