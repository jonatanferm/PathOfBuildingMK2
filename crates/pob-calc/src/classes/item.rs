//! @pob-file Classes/Item.lua @16de4b82d
//! @pob-file Modules/ItemSlotHelper.lua @16de4b82d
//! @pob-file Modules/Utils.lua @16de4b82d
//! Classes/Item.lua 1-423, 1646-2191 (Item core), Modules/ItemSlotHelper.lua, Modules/Utils.lua.
//!
//! Owner: IT-B. Stub created by U0: signatures are final (frozen; change through a CCR),
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

// @pob Classes/Item.lua:6-29 h=690d5d30
// (module-level statements: ipairs,t_insert,t_remove,m_min,m_max,m_floor,dmgTypeList,catalystList,catalystDescriptorList,catalys…)

/// @pob Classes/Item.lua:31-62 h=93b4170f
pub fn getCatalystScalar(catalystId: Option<f64>, r#mod: &Table, quality: Option<f64>) -> f64 {
    unported!("Classes/Item.lua:31-62")
}

/// @pob Classes/Item.lua:64-68 h=fef9a806
pub fn normaliseModLine(line: &str) -> String {
    unported!("Classes/Item.lua:64-68")
}

// @pob Classes/Item.lua:70-70 h=1dd79fe5
// (module-level statements: uniqueModStatOrder)

/// @pob Classes/Item.lua:72-87 h=a231bebc
pub fn sortCraftedModLines(modLines: &Shared<Vec<Table>>) {
    unported!("Classes/Item.lua:72-87")
}

// @pob Classes/Item.lua:89-92 h=0eda4292
// (module-level statements: influenceInfo,ItemClass)

/// @pob Classes/Item.lua:94-99 h=d86edc1c
/// `new("Item", raw, rarity, highQuality)`.
pub fn new_Item(raw: Option<&str>, rarity: Option<&str>, highQuality: bool) -> ItemRef {
    unported!("Classes/Item.lua:94-99")
}

/// @pob Classes/Item.lua:102-106 h=cfd1f19c
impl Item {
    pub fn ResetInfluence(&self) {
        unported!("Classes/Item.lua:102-106")
    }
}

// @pob Classes/Item.lua:108-141 h=f99be248
// (module-level statements: influenceItemMap,GenFor,lineFlags,specialModifierFoundList,inverseModifierFoundList)

/// @pob Classes/Item.lua:142-286 h=119add6b
pub fn getTagBasedModifiers(tagName: &str, itemSlotName: &str) -> Vec<Table> {
    unported!("Classes/Item.lua:142-286")
}

/// @pob Classes/Item.lua:289-333 h=767b0498
impl Item {
    pub fn FindModifierSubstring(&self, substring: &str, itemSlotName: &str) -> bool {
        unported!("Classes/Item.lua:289-333")
    }
}

/// @pob Classes/Item.lua:335-338 h=4ca5c800
pub fn specToNumber(s: &str) -> Option<f64> {
    unported!("Classes/Item.lua:335-338")
}

/// @pob Classes/Item.lua:340-344 h=6f595d12
impl Item {
    pub fn IsVariantGroupOptionEligible(&self, groupId: f64, variantId: f64) -> bool {
        unported!("Classes/Item.lua:340-344")
    }
}

/// @pob Classes/Item.lua:346-368 h=f208a3c7
impl Item {
    pub fn GetVariantGroupOptions(&self, groupId: f64, excludeSelected: bool) -> Vec<Table> {
        unported!("Classes/Item.lua:346-368")
    }
}

/// @pob Classes/Item.lua:370-408 h=b690a873
impl Item {
    pub fn NormaliseVariantSelections(&self) {
        unported!("Classes/Item.lua:370-408")
    }
}

/// @pob Classes/Item.lua:1646-1682 h=dd18671b
impl Item {
    pub fn MutateMod(&self, modId: Name, newModId: Name, mutatedValue: Option<f64>) {
        unported!("Classes/Item.lua:1646-1682")
    }
}

/// @pob Classes/Item.lua:1683-1691 h=b3d94dcd
impl Item {
    pub fn NormaliseQuality(&self) {
        unported!("Classes/Item.lua:1683-1691")
    }
}

/// @pob Classes/Item.lua:1693-1752 h=2ec74720
impl Item {
    pub fn GetModSpawnWeight(
        &self,
        r#mod: &pob_gamedata::schema::affixes::ItemMod,
        includeTags: Option<&Table>,
        excludeTags: Option<&Table>,
        baseTags: Option<&Table>,
    ) -> f64 {
        unported!("Classes/Item.lua:1693-1752")
    }
}

/// @pob Classes/Item.lua:1754-1765 h=99b93939
impl Item {
    pub fn GetNecropolisModSpawnWeight(
        &self,
        r#mod: &pob_gamedata::schema::affixes::ItemMod,
    ) -> f64 {
        unported!("Classes/Item.lua:1754-1765")
    }
}

/// @pob Classes/Item.lua:1767-1769 h=f6c5dd40
impl Item {
    pub fn CheckIfModIsDelve(&self, r#mod: &pob_gamedata::schema::affixes::ItemMod) -> bool {
        unported!("Classes/Item.lua:1767-1769")
    }
}

/// @pob Classes/Item.lua:1772-2047 h=ec27f0b6
impl Item {
    pub fn BuildRaw(&self) -> String {
        unported!("Classes/Item.lua:1772-2047")
    }
}

/// @pob Classes/Item.lua:2049-2052 h=4d709cd8
impl Item {
    pub fn BuildAndParseRaw(&self) {
        unported!("Classes/Item.lua:2049-2052")
    }
}

/// @pob Classes/Item.lua:2055-2118 h=cef182f1
impl Item {
    pub fn Craft(&self) {
        unported!("Classes/Item.lua:2055-2118")
    }
}

/// @pob Classes/Item.lua:2120-2146 h=c1722bca
impl Item {
    pub fn CheckModLineVariant(&self, modLine: &Table) -> bool {
        unported!("Classes/Item.lua:2120-2146")
    }
}

/// @pob Classes/Item.lua:2148-2164 h=35fea79e
impl Item {
    pub fn GetModLineVariantCount(&self, modLine: &Table) -> f64 {
        unported!("Classes/Item.lua:2148-2164")
    }
}

/// @pob Classes/Item.lua:2166-2182 h=45e1b16b
impl Item {
    pub fn GetPrimarySlot(&self) -> Name {
        unported!("Classes/Item.lua:2166-2182")
    }
}

// @pob Modules/ItemSlotHelper.lua:1-1 h=5ce3829c
// (module-level statements: M)

/// @pob Modules/ItemSlotHelper.lua:10-38 h=112b6e05
/// UI drawing helper (ItemSlotHelper.lua `M.DrawViewer`); kept for provenance, the UI port owns drawing.
pub fn DrawViewer(nodeId: f64, x: f64, y: f64, w: f64, h: f64) {
    unported!("Modules/ItemSlotHelper.lua:10-38")
}

// @pob Modules/ItemSlotHelper.lua:40-40 h=85c34a63
// (module-level statements: Return)

// @pob Modules/Utils.lua:1-5 h=8afe2e80
// (module-level statements: t_insert,M,indentCache)

/// @pob Modules/Utils.lua:6-14 h=233c3a4e
pub fn indentFor(k: &Val) -> String {
    unported!("Modules/Utils.lua:6-14")
}

/// @pob Modules/Utils.lua:15-17 h=0b041ef6
pub fn strSort(a: &Val, b: &Val) -> bool {
    unported!("Modules/Utils.lua:15-17")
}

/// @pob Modules/Utils.lua:19-62 h=6bc36f7a
pub fn writeStringify(
    buf: &mut String,
    value: &Val,
    allowNewlines: bool,
    tabs: f64,
    singleLine: bool,
) {
    unported!("Modules/Utils.lua:19-62")
}

/// @pob Modules/Utils.lua:69-73 h=8c4f2ac1
pub fn stringify(value: &Val, allowNewlines: bool, tabs: Option<f64>) -> String {
    unported!("Modules/Utils.lua:69-73")
}

/// @pob Modules/Utils.lua:79-83 h=37e4525d
pub fn stringifyInline(value: &Val) -> String {
    unported!("Modules/Utils.lua:79-83")
}

/// @pob Modules/Utils.lua:90-123 h=7297461d
pub fn saveTableToFile(
    fileName: &str,
    table: &Val,
    description: Option<&str>,
    allowMultiLine: bool,
) -> bool {
    unported!("Modules/Utils.lua:90-123")
}

// @pob Modules/Utils.lua:125-125 h=85c34a63
// (module-level statements: Return)
