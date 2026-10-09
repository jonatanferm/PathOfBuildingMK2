//! @pob-file Classes/PartyTab.lua @16de4b82d
//! PartyTab model (Classes/PartyTab.lua 516-669, 718-1042).
//!
//! Owner: BM-B. Stub created by U0: signatures are final (frozen; change through a CCR),
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

/// @pob Classes/PartyTab.lua:516-573 h=df6a5f6f
impl PartyTab {
    pub fn Load(&self, build: &BuildRef, xml: &crate::model::XmlNode, fileName: &str) {
        unported!("Classes/PartyTab.lua:516-573")
    }
}

/// @pob Classes/PartyTab.lua:575-668 h=9b8d985e
impl PartyTab {
    pub fn Save(&self, xml: &mut crate::model::XmlNode) {
        unported!("Classes/PartyTab.lua:575-668")
    }
}

/// @pob Classes/PartyTab.lua:718-982 h=9638cce9
impl PartyTab {
    /// `aura` at 919 is an undeclared global (CCR-0004): LUA-GLOBAL-NIL.
    pub fn ParseBuffs(
        &self,
        build: &BuildRef,
        list: &Table,
        buf: &str,
        buffType: Name,
        label: Option<&str>,
    ) {
        unported!("Classes/PartyTab.lua:718-982")
    }
}

/// @pob Classes/PartyTab.lua:984-990 h=80f3f6ea
impl PartyTab {
    pub fn setBuffExports(&self, buffExports: &Table) {
        unported!("Classes/PartyTab.lua:984-990")
    }
}

/// @pob Classes/PartyTab.lua:992-1043 h=3b532484
impl PartyTab {
    pub fn exportBuffs(&self, buffType: Name) -> String {
        unported!("Classes/PartyTab.lua:992-1043")
    }
}
