//! The only door from pob-app to the calc plan's model and data (ui doc §10.2, D15). Owner: U15
//! (complete re-export surface delivered by UI-U0).
//!
//! `pob-app` names model and calc types only through this module and `calc/`. Model structs that
//! share names with UI tab widgets (`model::ItemsTab`, `model::SkillsTab`, ...) are always
//! path-qualified as `model::X` in UI code. No UI unit invents model types: gaps go through the
//! calc doc's change process (calc doc §10.3, "CCR").

pub use pob_calc::model::*;

pub use pob_calc::calcs_facade::calcs::{self, MiscCalculator};
pub use pob_calc::classes::{
    Item, ItemRef, ModDB, ModList, Node, NodeRef, PassiveSpec, PassiveSpecRef, PassiveTree,
    PassiveTreeRef,
};
pub use pob_calc::env::{Env, EnvRef, Mode, Override};
pub use pob_calc::lua_table::{Key, LStr, Output, Table, Val};
pub use pob_gamedata::{self as gamedata, Data};
pub use pob_lua::{Field, LjIntMap, Name, Shared};

/// runtime/lua/xml.lua (calc:BM-A): the reader/writer functions and the node types.
pub mod xml {
    pub use pob_calc::model::xml::*;
    pub use pob_calc::model::{XmlChild, XmlNode};
}

/// The process-global game data (`data`, calc doc §4).
pub fn data() -> &'static Data {
    pob_gamedata::data()
}
