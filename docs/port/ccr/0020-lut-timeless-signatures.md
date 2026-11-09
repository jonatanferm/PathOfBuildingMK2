# CCR-0020 (LUT): timeless/abyss LUT signatures, the LUT cache, file access and the `path` table

**Status: proposed** (worked around in `crates/pob-gamedata/src/timeless_lut.rs`, marked `CCR-0020`).

- **Raised by:** LUT
- **Affects:** `crates/pob-gamedata/src/timeless_lut.rs` (U0 stub signatures), `crates/pob-gamedata/Cargo.toml`
  (new dependency), calc doc §2.1/§4.4 (LUT notes), PS-A/PS-B (`GetShortestPathToClassStart`,
  `self.tree.legion`)
- **Pin:** 16de4b82d

## Problem

U0's stub typed the abyss LUT table (`parseAbyssJewel`'s result, `data.timelessJewelLUTs[7..11]`) as an
immutable `Value`. That cannot work:

1. The table's `data` member is the whole jewel file, a binary Lua string
   (DataAbyssJewelLookUpTableHelper.lua:103 `data = jewelData`). `Value::Str` holds `Arc<str>` (UTF-8);
   the files are not UTF-8.
2. The table is mutated after construction: `getRecordOffsets` memoises
   `jewelLUT.seedOffsets[blockKey] = offsets` (DataAbyssJewelLookUpTableHelper.lua:191-200), and
   `loadAbyssJewel` caches the table in `data.timelessJewelLUTs` (165-178). `Value` is immutable and
   `Data` is immutable.
3. `readAbyssJewelLUT` returns `affectedNodes`, an integer-keyed table (`affectedNodes[nodeId] = …`,
   :233/:244/:266) whose `pairs()` order the caller observes (PassiveSpec.lua:1118;
   `docs/port/pairs_inventory.md` row 287: **int**, by hand). `ValueTable` has no integer hash keys
   (only `"#<n>"` fields), so the order could not be reproduced.
4. `getRecordOffsets` returning `Option<Vec<usize>>` copies a 7 901-entry vector for every read
   (Zorath reads every path node for every seed).
5. `repairLUTs()` returns the rebuilt `nodeIDList` (DataLegionLookUpTableHelper.lua:214; Data.lua
   assigns it to `data.nodeIDList`); the stub returned nothing.
6. `loadJewelFile` needs SimpleGraphic's `Inflate` (zlib), `GetScriptPath`, `io.open` and
   `NewFileSearch`. pob-gamedata had no zlib dependency and no way for a host (desktop, wasm) to
   provide files.

Two related caller-side problems (not LUT's files):

7. `PassiveSpec::GetShortestPathToClassStart` (passive_spec.rs:216, PS-A) is typed
   `Option<Shared<Vec<NodeRef>>>`, but the Lua returns the set `{ [nodeId] = true }`
   (PassiveSpec.lua:989-995), whose `pairs()` order `readAbyssJewelLUT` iterates (:241). It
   should be an `LjIntMap<bool>` built in Lua order (constructor `{ [root.id] = true, [other.id] = true }`,
   then `path[cur.id] = true` per step); PS-B passes `path.keys()` to `readAbyssJewelLUT`.
8. `resolveAbyssJewelComponent` / `getAbyssJewelComponentRoll` take the legion table as `&Value`
   (frozen), but `PassiveTree.legion` is a pob-calc `Table` (classes/mod.rs:282) that PassiveTree.lua
   726-760 mutates. The LUT port keeps the frozen `&Value` signatures (they are verified against PoB
   with `LegionPassives` converted to `Value`); PS-B either converts the legion node/addition it
   needs, or ports these two 10-line functions against `Table` in passive_spec_paths.rs.

## Proposed diff (as implemented)

```rust
// timeless_lut.rs: new pub items (host plumbing, not in Lua)
pub trait JewelFs: Send + Sync {                   // GetScriptPath / io.open / NewFileSearch
    fn script_path(&self) -> String;
    fn exists(&self, path: &str) -> bool;
    fn read(&self, path: &str) -> Option<Vec<u8>>;
    fn write(&self, path: &str, data: &[u8]) -> bool;
    fn search(&self, spec: &str) -> Option<JewelFileAttr>;
}
pub struct JewelFileAttr { pub fileName: String, pub modified: f64 }
pub struct StdJewelFs { pub script_path: String, pub writable: bool }   // cfg(not(wasm32)); desktop default ("" script path)
pub struct NoJewelFs;                                                   // wasm default
pub fn set_jewel_fs(fs: Arc<dyn JewelFs>);
pub fn set_con_printf(f: fn(&str));                                     // ConPrintf sink (default stderr)
pub fn clear_timeless_jewel_luts(jewelType: Option<f64>);               // release cached files

pub struct AbyssJewelLUT {                                              // parseAbyssJewel's table
    pub data: Arc<[u8]>, pub format: Vec<u8>,
    pub seedMinimum: f64, pub seedMaximum: f64, pub seedIncrement: f64, pub seedCount: f64,
    pub seedOffsets: Mutex<HashMap<BlockKey, Arc<Vec<usize>>>>,
    pub blockOffsets: BTreeMap<i64, usize>,
    pub abyssSize: Option<f64>, pub ascendancyOffsets: Option<Vec<(Vec<u8>, usize)>>,
}
pub enum BlockKey { Num(u64), Str(Vec<u8>) }

// changed signatures
-pub fn repairLUTs()
+pub fn repairLUTs() -> Option<NodeIdList>
-pub fn parseAbyssJewel(jewelType: f64, jewelData: &[u8]) -> Value
+pub fn parseAbyssJewel(jewelType: f64, jewelData: Arc<[u8]>) -> AbyssJewelLUT
-pub fn loadAbyssJewel(jewelType: f64) -> Value
+pub fn loadAbyssJewel(jewelType: f64) -> Option<Arc<AbyssJewelLUT>>
-pub fn getSeedIndex(jewelLUT: &Value, seed: f64) -> Option<f64>
+pub fn getSeedIndex(jewelLUT: &AbyssJewelLUT, seed: f64) -> Option<f64>
-pub fn getRecordOffsets(jewelLUT: &Value, blockKey: &Value, blockOffset: Option<f64>,
-                        skipRecord: fn(&[u8], usize) -> usize) -> Option<Vec<usize>>
+pub fn getRecordOffsets(jewelLUT: &AbyssJewelLUT, blockKey: &BlockKey, blockOffset: Option<usize>,
+                        skipRecord: fn(&[u8], usize) -> usize) -> Option<Arc<Vec<usize>>>
 pub fn readAbyssJewelLUT(seed: f64, socketId: f64, jewelType: f64,
-                         path: Option<&[f64]>, ascendancyName: Option<&str>) -> Value
+                         path: Option<&[f64]>, ascendancyName: Option<&str>) -> LjIntMap<Value>
```

`path` stays `Option<&[f64]>`: the keys of the Lua `path` table in its `pairs()` order
(`LjIntMap::keys()` once item 7 lands). Unchanged: `loadJewelFile`, `loadTimelessJewel`,
`convertLocalIdToGlobalId`, `readLUT`, `readUInt16`, `skipModification`, `readModification`,
`resolveAbyssJewelComponent`, `getAbyssJewelComponentRoll`, `skipSocketRecord`,
`skipAscendancyRecord`, `readNode`.

`crates/pob-gamedata/Cargo.toml`: `miniz_oxide = "0.8"` (already in Cargo.lock via flate2; pure Rust,
wasm32-safe) for `Inflate`.

`data.timelessJewelLUTs` is a process-global `Mutex` cache in timeless_lut.rs (it only caches file
bytes, so sharing it across threads is unobservable); `Data.timeless.timelessJewelLUTs` (the dumped
`{ }`) is not read.

Calc doc §4.4: add after the `TimelessMeta` comment: "`data.timelessJewelLUTs` and the file access the
LUTs need live in `timeless_lut.rs` (CCR-0020)."
