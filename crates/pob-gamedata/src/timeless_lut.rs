//! @pob-file Modules/DataLegionLookUpTableHelper.lua @16de4b82d
//! @pob-file Modules/DataAbyssJewelLookUpTableHelper.lua @16de4b82d
//! @pob-file Modules/DataJewelFileLoader.lua @16de4b82d
//! Timeless / abyss jewel lookup tables. Owner: LUT. Signatures frozen by U0;
//! results are the Lua tables as immutable [`Value`]s.
#![allow(non_snake_case, unused_variables)]

use crate::unported;
use crate::value::Value;

// ---- Modules/DataJewelFileLoader.lua ----

/// @pob Modules/DataJewelFileLoader.lua:1-7 h=1861155c
/// Module header (no Rust counterpart).
const _LOADER_HEADER: () = ();

/// @pob Modules/DataJewelFileLoader.lua:8-95 h=03e9e516
/// `loadJewelFile(jewelTypeName, cacheUncompressed)`: the decompressed LUT
/// bytes (desktop reads `Data/TimelessJewelData/<name>.zip|.bin`).
pub fn loadJewelFile(jewelTypeName: &str, cacheUncompressed: bool) -> Option<Vec<u8>> {
    unported!("Modules/DataJewelFileLoader.lua:8-95")
}

// ---- Modules/DataLegionLookUpTableHelper.lua ----

/// @pob Modules/DataLegionLookUpTableHelper.lua:1-10 h=14c08cd2
const _LEGION_HEADER: () = ();

/// @pob Modules/DataLegionLookUpTableHelper.lua:11-88 h=e18c3d34
/// `loadTimelessJewel(jewelType, nodeID)`: fills the per-type LUT cache.
pub fn loadTimelessJewel(jewelType: f64, nodeID: f64) {
    unported!("Modules/DataLegionLookUpTableHelper.lua:11-88")
}

/// @pob Modules/DataLegionLookUpTableHelper.lua:89-96 h=81ee299f
pub fn convertLocalIdToGlobalId(jewelType: f64, localId: f64) -> f64 {
    unported!("Modules/DataLegionLookUpTableHelper.lua:89-96")
}

/// @pob Modules/DataLegionLookUpTableHelper.lua:97-219 h=e975e911
/// The commented-out generators and `repairLUTs()` (concatenates the
/// undeclared global `scriptPath`: raises a LuaError if reached, CCR-0004).
pub fn repairLUTs() {
    unported!("Modules/DataLegionLookUpTableHelper.lua:97-219")
}

/// @pob Modules/DataLegionLookUpTableHelper.lua:220-262 h=74d25bfc
/// `readLUT(seed, nodeID, jewelType)` → `{ ids… }`.
pub fn readLUT(seed: f64, nodeID: f64, jewelType: f64) -> Vec<f64> {
    unported!("Modules/DataLegionLookUpTableHelper.lua:220-262")
}

// ---- Modules/DataAbyssJewelLookUpTableHelper.lua ----

/// @pob Modules/DataAbyssJewelLookUpTableHelper.lua:1-12 h=4e63e43b
/// Module header and `readUInt16(jewelData, offset)` → (value, next offset).
pub fn readUInt16(jewelData: &[u8], offset: usize) -> (f64, usize) {
    unported!("Modules/DataAbyssJewelLookUpTableHelper.lua:1-12")
}

/// @pob Modules/DataAbyssJewelLookUpTableHelper.lua:13-24 h=95f85e10
pub fn skipModification(jewelData: &[u8], offset: usize) -> usize {
    unported!("Modules/DataAbyssJewelLookUpTableHelper.lua:13-24")
}

/// @pob Modules/DataAbyssJewelLookUpTableHelper.lua:25-47 h=ecd7a333
/// → (modification table, next offset).
pub fn readModification(jewelData: &[u8], offset: usize, jewelType: f64) -> (Value, usize) {
    unported!("Modules/DataAbyssJewelLookUpTableHelper.lua:25-47")
}

/// @pob Modules/DataAbyssJewelLookUpTableHelper.lua:48-57 h=d5b32a4a
/// → (legion node or addition, isNode).
pub fn resolveAbyssJewelComponent(component: &Value, legion: &Value) -> (Value, Option<bool>) {
    unported!("Modules/DataAbyssJewelLookUpTableHelper.lua:48-57")
}

/// @pob Modules/DataAbyssJewelLookUpTableHelper.lua:58-73 h=895d7a06
/// → (statKey, statMod, roll).
pub fn getAbyssJewelComponentRoll(
    component: &Value,
    node: &Value,
    statIndex: f64,
) -> (Value, Value, Option<f64>) {
    unported!("Modules/DataAbyssJewelLookUpTableHelper.lua:58-73")
}

/// @pob Modules/DataAbyssJewelLookUpTableHelper.lua:74-82 h=2747e296
pub fn skipSocketRecord(jewelData: &[u8], offset: usize) -> usize {
    unported!("Modules/DataAbyssJewelLookUpTableHelper.lua:74-82")
}

/// @pob Modules/DataAbyssJewelLookUpTableHelper.lua:83-86 h=f3d82384
pub fn skipAscendancyRecord(jewelData: &[u8], offset: usize) -> usize {
    unported!("Modules/DataAbyssJewelLookUpTableHelper.lua:83-86")
}

/// @pob Modules/DataAbyssJewelLookUpTableHelper.lua:87-164 h=5df677b7
pub fn parseAbyssJewel(jewelType: f64, jewelData: &[u8]) -> Value {
    unported!("Modules/DataAbyssJewelLookUpTableHelper.lua:87-164")
}

/// @pob Modules/DataAbyssJewelLookUpTableHelper.lua:165-178 h=4ff767b2
pub fn loadAbyssJewel(jewelType: f64) -> Value {
    unported!("Modules/DataAbyssJewelLookUpTableHelper.lua:165-178")
}

/// @pob Modules/DataAbyssJewelLookUpTableHelper.lua:179-186 h=6f5113eb
pub fn getSeedIndex(jewelLUT: &Value, seed: f64) -> Option<f64> {
    unported!("Modules/DataAbyssJewelLookUpTableHelper.lua:179-186")
}

/// @pob Modules/DataAbyssJewelLookUpTableHelper.lua:187-203 h=d68c25be
pub fn getRecordOffsets(
    jewelLUT: &Value,
    blockKey: &Value,
    blockOffset: Option<f64>,
    skipRecord: fn(&[u8], usize) -> usize,
) -> Option<Vec<usize>> {
    unported!("Modules/DataAbyssJewelLookUpTableHelper.lua:187-203")
}

/// @pob Modules/DataAbyssJewelLookUpTableHelper.lua:204-213 h=e2f461a6
pub fn readNode(seed: f64, nodeId: f64, jewelType: f64) -> Value {
    unported!("Modules/DataAbyssJewelLookUpTableHelper.lua:204-213")
}

/// @pob Modules/DataAbyssJewelLookUpTableHelper.lua:214-275 h=82766499
/// `readAbyssJewelLUT(seed, socketId, jewelType, path, ascendancyName)` →
/// `{ [nodeId] = modification }` (`path` keys are node ids).
pub fn readAbyssJewelLUT(
    seed: f64,
    socketId: f64,
    jewelType: f64,
    path: Option<&[f64]>,
    ascendancyName: Option<&str>,
) -> Value {
    unported!("Modules/DataAbyssJewelLookUpTableHelper.lua:214-275")
}
