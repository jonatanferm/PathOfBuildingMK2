//! @pob-file Classes/Item.lua @16de4b82d
//! Item mod lists (Classes/Item.lua 2192-2736).
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

/// @pob Classes/Item.lua:2192-2219 h=1dfbd9ce
/// `calcLocal(modList, name, type, flags)` (local to Item.lua).
pub fn calcLocal(modList: &ModList, name: Name, r#type: ModType, flags: u32) -> Val {
    unported!("Classes/Item.lua:2192-2219")
}

/// @pob Classes/Item.lua:2222-2520 h=169596ce
impl Item {
    pub fn BuildModListForSlotNum(&self, baseList: &ModList, slotNum: Option<f64>) -> ModList {
        unported!("Classes/Item.lua:2222-2520")
    }
}

/// @pob Classes/Item.lua:2522-2532 h=29c57056
pub fn getRangedModList(item: &Item, modLine: &Table) -> Option<Vec<Mod>> {
    unported!("Classes/Item.lua:2522-2532")
}

/// @pob Classes/Item.lua:2534-2704 h=cd449c3b
impl Item {
    pub fn BuildModList(&self) {
        unported!("Classes/Item.lua:2534-2704")
    }
}

/// @pob Classes/Item.lua:2706-2736 h=3cbbe245
impl Item {
    pub fn CanHaveMod(
        &self,
        r#mod: &pob_gamedata::schema::affixes::ItemMod,
        includeTags: Option<&Table>,
    ) -> bool {
        unported!("Classes/Item.lua:2706-2736")
    }
}
