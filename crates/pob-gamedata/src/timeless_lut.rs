//! @pob-file Modules/DataJewelFileLoader.lua @16de4b82d
//! @pob-file Modules/DataLegionLookUpTableHelper.lua @16de4b82d
//! @pob-file Modules/DataAbyssJewelLookUpTableHelper.lua @16de4b82d
//! Timeless / abyss jewel lookup tables. Owner: LUT.
//!
//! * `data.readLUT` (legion jewels, types 1-6), `data.readAbyssJewelLUT`,
//!   `data.resolveAbyssJewelComponent`, `data.getAbyssJewelComponentRoll` and the
//!   loader they share (`Modules/DataJewelFileLoader.lua`).
//! * `data.timelessJewelLUTs` is the only mutable part of these modules. `Data` is
//!   immutable, so the cache lives here, in one process-global table ([`LUTS`],
//!   keyed by jewel type exactly like the Lua table). It only ever caches file
//!   contents (bytes are immutable once loaded), so sharing it between threads is
//!   unobservable; the dumped `data.timeless.timelessJewelLUTs` (always `{ }` at
//!   dump time) is not used.
//! * Lua strings are byte strings: the jewel files are kept as `Arc<[u8]>`, and the
//!   Glorious Vanity substrings (`raw`, `sizes`, the per-seed strings) are ranges into
//!   the one file buffer instead of copies.
//! * File access (`GetScriptPath`, `io.open`, `NewFileSearch`) goes through the
//!   [`JewelFs`] the host registers with [`set_jewel_fs`] (desktop default:
//!   [`StdJewelFs`] with PoB's `GetScriptPath() == ""` behaviour; wasm default: no
//!   files). `ConPrintf` goes to the sink set with [`set_con_printf`] (default stderr).
//! * The abyss tables are [`AbyssJewelLUT`] structs, and `readAbyssJewelLUT` returns
//!   the integer-keyed `affectedNodes` table as an [`LjIntMap`], so its `pairs()`
//!   order is LuaJIT's (CCR-0020 lists these signature changes).
#![allow(non_snake_case)]

use std::collections::{BTreeMap, HashMap};
use std::sync::{Arc, Mutex, MutexGuard, PoisonError, RwLock};

use pob_lua::math::lmod;
use pob_lua::string::{byte_bytes, sub_range};
use pob_lua::{lua_error, n, tostring_num, LjIntMap};

use crate::data::data;
use crate::schema::item_data::{LocalIdToGlobalId, NodeIdList, NodeIndex};
use crate::value::{DataValue, Value, ValueTable};

// ---------------------------------------------------------------------------
// Host plumbing (not in Lua): file access and the console
// ---------------------------------------------------------------------------

/// One `NewFileSearch` hit: `handle:GetFileName()` and `handle:GetFileModifiedTime()`.
#[derive(Debug, Clone, PartialEq)]
pub struct JewelFileAttr {
    pub fileName: String,
    pub modified: f64,
}

/// What `DataJewelFileLoader.lua` / `repairLUTs` need from PoB's host. Paths are
/// passed exactly as the Lua builds them (`scriptPath .. "/Data/TimelessJewelData/…"`).
pub trait JewelFs: Send + Sync {
    /// `GetScriptPath()`.
    fn script_path(&self) -> String;
    /// `io.open(path, "rb")` succeeds.
    fn exists(&self, path: &str) -> bool;
    /// `io.open(path, "rb")` + `file:read("*a")`; `None` when the open fails.
    fn read(&self, path: &str) -> Option<Vec<u8>>;
    /// `io.open(path, "wb+")` + `file:write(data)` + `file:close()`; `false` when the
    /// open fails (the Lua then skips the write, or `assert`s).
    fn write(&self, path: &str, data: &[u8]) -> bool;
    /// `NewFileSearch(spec)` (a trailing `*` matches any suffix): the first file found
    /// (files sorted by name), or `None` when nothing matches.
    fn search(&self, spec: &str) -> Option<JewelFileAttr>;
}

/// Desktop [`JewelFs`] over `std::fs` (relative paths resolve against the process
/// working directory, as in PoB). `script_path` is what `GetScriptPath()` returns;
/// PoB's own desktop host returns its `src/` folder, and `""` makes the loader probe
/// `./` and `./src` (DataJewelFileLoader.lua:11-22).
#[cfg(not(target_arch = "wasm32"))]
#[derive(Debug, Clone, Default)]
pub struct StdJewelFs {
    pub script_path: String,
    /// `false` turns every write into a failed `io.open(…, "wb+")` (read-only
    /// installs and tests; PoB then simply skips its `.bin` cache).
    pub writable: bool,
}

#[cfg(not(target_arch = "wasm32"))]
impl StdJewelFs {
    /// PoB's desktop behaviour: the given script path, writes allowed.
    pub fn new(script_path: impl Into<String>) -> Self {
        Self {
            script_path: script_path.into(),
            writable: true,
        }
    }
}

#[cfg(not(target_arch = "wasm32"))]
impl JewelFs for StdJewelFs {
    fn script_path(&self) -> String {
        self.script_path.clone()
    }
    fn exists(&self, path: &str) -> bool {
        std::fs::File::open(path).is_ok()
    }
    fn read(&self, path: &str) -> Option<Vec<u8>> {
        std::fs::read(path).ok()
    }
    fn write(&self, path: &str, data: &[u8]) -> bool {
        self.writable && std::fs::write(path, data).is_ok()
    }
    fn search(&self, spec: &str) -> Option<JewelFileAttr> {
        let (pattern, exact) = match spec.strip_suffix('*') {
            Some(p) => (p, false),
            None => (spec, true),
        };
        let path = std::path::Path::new(pattern);
        let (dir, prefix) = if pattern.ends_with('/') {
            (path.to_path_buf(), String::new())
        } else {
            (
                path.parent()
                    .map(std::path::Path::to_path_buf)
                    .unwrap_or_default(),
                path.file_name()
                    .map(|n| n.to_string_lossy().into_owned())
                    .unwrap_or_default(),
            )
        };
        let dir = if dir.as_os_str().is_empty() {
            std::path::PathBuf::from(".")
        } else {
            dir
        };
        let mut files: Vec<JewelFileAttr> = std::fs::read_dir(&dir)
            .ok()?
            .filter_map(Result::ok)
            .filter(|e| e.file_type().is_ok_and(|t| !t.is_dir()))
            .filter_map(|e| {
                let name = e.file_name().to_string_lossy().into_owned();
                let hit = if exact {
                    name == prefix
                } else {
                    name.starts_with(&prefix)
                };
                let modified = e
                    .metadata()
                    .and_then(|m| m.modified())
                    .ok()
                    .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
                    .map_or(0.0, |d| d.as_secs_f64());
                hit.then_some(JewelFileAttr {
                    fileName: name,
                    modified,
                })
            })
            .collect();
        files.sort_by(|a, b| a.fileName.cmp(&b.fileName));
        files.into_iter().next()
    }
}

/// A host without files (the wasm default until the app registers its own).
#[derive(Debug, Clone, Copy, Default)]
pub struct NoJewelFs;

impl JewelFs for NoJewelFs {
    fn script_path(&self) -> String {
        String::new()
    }
    fn exists(&self, _: &str) -> bool {
        false
    }
    fn read(&self, _: &str) -> Option<Vec<u8>> {
        None
    }
    fn write(&self, _: &str, _: &[u8]) -> bool {
        false
    }
    fn search(&self, _: &str) -> Option<JewelFileAttr> {
        None
    }
}

static JEWEL_FS: RwLock<Option<Arc<dyn JewelFs>>> = RwLock::new(None);

/// Register the host's file access (replaces the previous one).
pub fn set_jewel_fs(fs: Arc<dyn JewelFs>) {
    *JEWEL_FS.write().unwrap_or_else(PoisonError::into_inner) = Some(fs);
}

fn jewel_fs() -> Arc<dyn JewelFs> {
    if let Some(fs) = JEWEL_FS
        .read()
        .unwrap_or_else(PoisonError::into_inner)
        .as_ref()
    {
        return fs.clone();
    }
    #[cfg(not(target_arch = "wasm32"))]
    {
        Arc::new(StdJewelFs::new(""))
    }
    #[cfg(target_arch = "wasm32")]
    {
        Arc::new(NoJewelFs)
    }
}

static CON_PRINTF: RwLock<Option<fn(&str)>> = RwLock::new(None);

/// Where `ConPrintf` output of these modules goes (default: stderr).
pub fn set_con_printf(f: fn(&str)) {
    *CON_PRINTF.write().unwrap_or_else(PoisonError::into_inner) = Some(f);
}

/// PoB's `ConPrintf(msg)` (the messages here never contain format directives).
fn ConPrintf(msg: &str) {
    let f = *CON_PRINTF.read().unwrap_or_else(PoisonError::into_inner);
    match f {
        Some(f) => f(msg),
        None => eprintln!("{msg}"),
    }
}

// ---------------------------------------------------------------------------
// Lua helpers
// ---------------------------------------------------------------------------

/// `lj_num2int` of the x64 build the port follows (`cvttsd2si`): truncation, and
/// INT32_MIN for NaN or out-of-range values. Used where the string library takes
/// an integer argument (`lj_lib_checkint`).
fn lj_num2int(x: f64) -> i64 {
    let t = x.trunc();
    if x.is_nan() || t < -2_147_483_648.0 || t > 2_147_483_647.0 {
        i64::from(i32::MIN)
    } else {
        t as i64
    }
}

/// `s_byte(s, i)` (first value; `None` = no value).
fn s_byte(s: &[u8], i: f64) -> Option<u8> {
    byte_bytes(s, lj_num2int(i))
}

/// `s:sub(i, j)` as a 0-based half-open range of `s` (empty → `(0, 0)`).
fn s_sub(len: usize, i: f64, j: f64) -> (usize, usize) {
    sub_range(len, lj_num2int(i), Some(lj_num2int(j))).unwrap_or((0, 0))
}

/// The integer a number key normalises to (`t[2.0]` is `t[2]`); `None` for other numbers.
fn int_key(x: f64) -> Option<i64> {
    (x.is_finite() && x.fract() == 0.0 && x.abs() < 9.007_199_254_740_992e15).then_some(x as i64)
}

/// Lua numeric `for _ = 1, limit` trip count check (`limit` nil raises).
fn for_limit(limit: Option<u8>) -> f64 {
    match limit {
        Some(b) => f64::from(b),
        None => lua_error!("'for' limit must be a number"),
    }
}

/// `s:gsub("%s+", "")` (both results).
fn gsub_spaces(s: &str) -> (String, usize) {
    pob_lua::gsub(s, "%s+", "", None)
}

/// `bytesToUShort(b, o)` (Modules/Common.lua:384-387).
fn bytesToUShort(b: &[u8], o: f64) -> f64 {
    f64::from(s_byte(b, o).unwrap_or(0)) + f64::from(s_byte(b, o + 1.0).unwrap_or(0)) * 256.0
}

/// `data.nodeIDList[nodeID]` for a node-id key.
fn nodeIDList_get(nodeID: f64) -> Option<NodeIndex> {
    let k = int_key(nodeID)?;
    data().nodeIDList.nodes.get(&k).copied()
}

/// `data.nodeIDList.localIdToGlobalId[jewelType]`.
fn localIdToGlobalId(jewelType: f64) -> Option<&'static LocalIdToGlobalId> {
    let k = int_key(jewelType)?;
    if k < 1 {
        return None;
    }
    data()
        .nodeIDList
        .localIdToGlobalId
        .get(usize::try_from(k - 1).ok()?)
}

static DNIL: DataValue = DataValue::Nil;

/// `t[k]` on one of the `data.timelessJewel*` tables.
fn timeless_index(t: &'static DataValue, k: f64) -> &'static DataValue {
    t.table().map_or(&DNIL, |t| t.index(k))
}

/// `data.timelessJewelSeedMin[jewelType]` in arithmetic.
fn seedMin(jewelType: f64) -> f64 {
    match timeless_index(&data().timeless.timelessJewelSeedMin, jewelType).num() {
        Some(x) => x,
        None => lua_error!("attempt to perform arithmetic on field '?' (a nil value)"),
    }
}

/// `data.timelessJewelSeedMax[jewelType]` in arithmetic.
fn seedMax(jewelType: f64) -> f64 {
    match timeless_index(&data().timeless.timelessJewelSeedMax, jewelType).num() {
        Some(x) => x,
        None => lua_error!("attempt to perform arithmetic on field '?' (a nil value)"),
    }
}

/// `data.timelessJewelTypes[jewelType]:gsub("%s+", "")`.
fn jewelTypeName_gsub(jewelType: f64) -> (String, usize) {
    match timeless_index(&data().timeless.timelessJewelTypes, jewelType).as_str() {
        Some(s) => gsub_spaces(s),
        None => lua_error!("attempt to index field '?' (a nil value)"),
    }
}

// ---------------------------------------------------------------------------
// data.timelessJewelLUTs
// ---------------------------------------------------------------------------

/// `data.timelessJewelLUTs[1].data[i]` (Glorious Vanity, one passive node): `raw`
/// until the node is split, then `[seedKey]` = a substring per seed.
#[derive(Debug, Default)]
struct GvNode {
    /// `.raw`: byte range in the entry's buffer.
    raw: Option<(usize, usize)>,
    /// `[k]` for k = 1..: start of the substring in the entry's buffer.
    seedStart: Vec<u32>,
    /// `[k]`: its length (`dataLength` is one byte, so it fits).
    seedLen: Vec<u8>,
}

/// `data.timelessJewelLUTs[jewelType].data`.
#[derive(Debug)]
enum LutData {
    /// A string: the whole jewel file (types 2-6; an abyss table's `data`).
    Bytes(Arc<[u8]>),
    /// Glorious Vanity's per-node table `{ [i] = GvNode }`.
    Gv(Vec<GvNode>),
}

/// `data.timelessJewelLUTs[jewelType]`.
#[derive(Debug, Default)]
struct LutEntry {
    /// The Glorious Vanity file that `sizes`, `raw` and the seed strings are cut from.
    buf: Option<Arc<[u8]>>,
    /// `.data`.
    data: Option<LutData>,
    /// `.sizes` (a range of `buf`).
    sizes: Option<(usize, usize)>,
    /// Set when the entry is a `parseAbyssJewel` table (types 7-11).
    abyss: Option<Arc<AbyssJewelLUT>>,
}

impl LutEntry {
    /// `.sizes` as a string (indexing a nil `.sizes` raises).
    fn sizes(&self) -> &[u8] {
        match (self.sizes, &self.buf) {
            (Some((a, b)), Some(buf)) => &buf[a..b],
            _ => lua_error!("attempt to index field 'sizes' (a nil value)"),
        }
    }
}

/// `data.timelessJewelLUTs` (Data.lua:957), keyed like the Lua table (number keys).
static LUTS: Mutex<BTreeMap<u64, LutEntry>> = Mutex::new(BTreeMap::new());

fn luts() -> MutexGuard<'static, BTreeMap<u64, LutEntry>> {
    LUTS.lock().unwrap_or_else(PoisonError::into_inner)
}

fn lut_key(jewelType: f64) -> u64 {
    if jewelType == 0.0 {
        0
    } else {
        jewelType.to_bits()
    }
}

/// Drop cached LUTs (all, or one jewel type). Not in Lua (PoB keeps them for the
/// process lifetime); hosts and tests use it to release the memory. A later read
/// loads the file again, which is what PoB does on its first read.
pub fn clear_timeless_jewel_luts(jewelType: Option<f64>) {
    let mut l = luts();
    match jewelType {
        Some(t) => {
            l.remove(&lut_key(t));
        }
        None => l.clear(),
    }
}

/// `data.timelessJewelLUTs[jewelType].data[k]` of a Glorious Vanity entry (nil for
/// a missing node; a non-table `.data` raises, as indexing would).
fn gv_node_index(nodes: &[GvNode], k: f64) -> Option<usize> {
    let k = int_key(k)?;
    (k >= 1 && (k as usize) <= nodes.len()).then(|| k as usize - 1)
}

/// The split of one Glorious Vanity node (DataLegionLookUpTableHelper.lua:28-36 and
/// 72-79, which differ only in their local names): every `seedKey` in
/// `1 .. seedSize + 1` gets `raw:sub(off + 1, off + sizes:byte(nodeIndex * seedSize + seedKey))`,
/// then `raw` is dropped.
fn gv_split(e: &mut LutEntry, k: usize, nodeIndex: f64, seedSize: f64) {
    let buf = e.buf.clone().unwrap_or_else(|| Arc::from(&[][..]));
    let Some((sa, sb)) = e.sizes.filter(|_| e.buf.is_some()) else {
        lua_error!("attempt to index field 'sizes' (a nil value)")
    };
    let sizes = &buf[sa..sb];
    let Some(LutData::Gv(nodes)) = &mut e.data else {
        lua_error!("attempt to index field 'data' (a nil value)")
    };
    let node = &mut nodes[k];
    let (rs, re) = node.raw.unwrap_or((0, 0));
    let raw = &buf[rs..re];
    node.seedStart.clear();
    node.seedLen.clear();
    let mut count = 0.0;
    let mut seedOffset = 1.0;
    while seedOffset <= seedSize + 1.0 {
        let Some(dataLength) = s_byte(sizes, nodeIndex * seedSize + seedOffset) else {
            lua_error!("attempt to perform arithmetic on local 'dataLength' (a nil value)")
        };
        let dataLength = f64::from(dataLength);
        let (a, b) = s_sub(raw.len(), count + 1.0, count + dataLength);
        node.seedStart.push((rs + a) as u32);
        node.seedLen.push((b - a) as u8);
        count += dataLength;
        seedOffset += 1.0;
    }
    node.raw = None;
}

// ---- Modules/DataJewelFileLoader.lua ----

/// @pob Modules/DataJewelFileLoader.lua:1-7 h=1861155c
/// Module header (`local t_concat = table.concat`); no Rust counterpart.
const _LOADER_HEADER: () = ();

/// @pob Modules/DataJewelFileLoader.lua:8-95 h=03e9e516
/// `loadJewelFile(jewelTypeName, cacheUncompressed)`: the decompressed LUT bytes.
/// The Lua's `cacheUncompressed = cacheUncompressed ~= false` makes nil (and the
/// `gsub` count the legion helper passes, LUA-QUIRK) `true`; callers pass the
/// resulting bool.
pub fn loadJewelFile(jewelTypeName: &str, cacheUncompressed: bool) -> Option<Vec<u8>> {
    let fs = jewel_fs();
    // L9
    let jewelPath = format!("/Data/TimelessJewelData/{jewelTypeName}");
    // L10
    let mut scriptPath = fs.script_path();
    // L11
    if scriptPath.is_empty() {
        // The desktop app supplies its script folder. Headless tests may start in
        // either the repository root or the src folder, so check both locations.
        // L14
        let relativePath = format!(".{jewelPath}");
        // L15
        let file = fs.exists(&format!("{relativePath}.zip"))
            || fs.exists(&format!("{relativePath}.zip.part0"))
            || fs.exists(&format!("{relativePath}.bin"));
        // L16
        if file {
            // L18
            scriptPath = ".".to_owned();
        } else {
            // L20
            scriptPath = "./src".to_owned();
        }
    }
    // L23: cacheUncompressed = cacheUncompressed ~= false (already a bool)

    // L25
    let mut uncompressedFileAttr: Option<JewelFileAttr> = None;
    // L26
    if cacheUncompressed {
        // L27
        let fileHandle = fs.search(&format!("{scriptPath}{jewelPath}.bin"));
        // L28
        if let Some(h) = fileHandle {
            // L29-30
            uncompressedFileAttr = Some(h);
        }
    }

    // L34
    let mut compressedModified: Option<f64> = None;
    // L35
    let fileHandle = fs.search(&format!("{scriptPath}{jewelPath}.zip"));
    // L36
    if let Some(h) = fileHandle {
        // L37
        compressedModified = Some(h.modified);
    }
    // L39
    let fileHandle = fs.search(&format!("{scriptPath}{jewelPath}.zip.part*"));
    // L40
    if let Some(h) = fileHandle {
        // L41
        compressedModified = Some(h.modified);
    }

    // L44
    if let Some(u) = &uncompressedFileAttr {
        if u.modified > compressedModified.unwrap_or(0.0) {
            // L45
            ConPrintf(&format!(
                "Uncompressed jewel data is up-to-date, loading {}",
                u.fileName
            ));
            // L46
            let uncompressedFile = fs.read(&format!("{scriptPath}{jewelPath}.bin"));
            // L47
            if let Some(jewelData) = uncompressedFile {
                // L50-51: read("*a") always yields a string
                return Some(jewelData);
            }
        }
    }

    // L56
    if cacheUncompressed {
        // L57
        ConPrintf(&format!(
            "Failed to load {scriptPath}{jewelPath}.bin, or data is out of date, falling back to compressed file"
        ));
    }
    // L59
    let compressedFile = fs.read(&format!("{scriptPath}{jewelPath}.zip"));
    // L60
    let compressedData: Vec<u8>;
    // L61
    if let Some(d) = compressedFile {
        // L62
        compressedData = d;
    } else {
        // L65
        let mut splitFile: Vec<Vec<u8>> = Vec::new();
        // L66
        let mut part = 0u32;
        // L67
        loop {
            // L68
            let file = fs.read(&format!("{scriptPath}{jewelPath}.zip.part{part}"));
            // L69
            let Some(d) = file else {
                break;
            };
            // L72
            splitFile.push(d);
            // L74
            part += 1;
        }
        // L76
        compressedData = splitFile.concat();
    }

    // L79
    if compressedData.is_empty() {
        // L80
        ConPrintf(&format!("Failed to load jewel data: {jewelTypeName}"));
        // L81
        return None;
    }

    // L84
    // (`local jewelData = Inflate(...)` keeps the first result: nil on failure)
    let jewelData = crate::sg_zlib::Inflate(&compressedData).ok();
    // L85
    if cacheUncompressed {
        // L86-90: io.open(…, "wb+") truncates; `write(nil)` then raises
        let path = format!("{scriptPath}{jewelPath}.bin");
        match &jewelData {
            Some(d) => {
                let _ = fs.write(&path, d);
            }
            None => {
                if fs.write(&path, b"") {
                    lua_error!("bad argument #1 to 'write' (string expected, got nil)");
                }
            }
        }
    }
    // L92
    jewelData
}

// ---- Modules/DataLegionLookUpTableHelper.lua ----

/// @pob Modules/DataLegionLookUpTableHelper.lua:1-10 h=14c08cd2
/// Module header: `local loadJewelFile = require("Modules.DataJewelFileLoader")`.
// lazy load a specific timeless jewel type
// valid values: "Glorious Vanity", "Lethal Pride", "Brutal Restraint", "Militant Faith", "Elegant Hubris"
// nodeID is needed for "Glorious Vanity"
const _LEGION_HEADER: () = ();

/// @pob Modules/DataLegionLookUpTableHelper.lua:11-88 h=e18c3d34
/// `loadTimelessJewel(jewelType, nodeID)`: fills `data.timelessJewelLUTs[jewelType]`.
pub fn loadTimelessJewel(jewelType: f64, nodeID: f64) {
    let d = data();
    // L12
    let mut nodeIndex: Option<f64> = None;
    // L13
    if let Some(n) = nodeIDList_get(nodeID) {
        // L14
        nodeIndex = Some(n.index);
    }
    // for GV, if nodeIndex is invalid, return
    // L17
    if jewelType == 1.0 && nodeIndex.is_none() {
        // L18
        return;
    }
    let key = lut_key(jewelType);
    let mut l = luts();
    // if LUT is already loaded, and this either isn't GV, or GV has already emptied it's raw data out, return
    // L21
    if let Some(e) = l.get(&key) {
        if let Some(lutData) = &e.data {
            if jewelType != 1.0 {
                // L22
                return;
            }
            let LutData::Gv(nodes) = lutData else {
                lua_error!("attempt to index a nil value")
            };
            let Some(k) = gv_node_index(nodes, nodeIndex.unwrap_or(f64::NAN) + 1.0) else {
                lua_error!("attempt to index a nil value")
            };
            if nodes[k].raw.is_none() {
                // L22
                return;
            }
        }
    }

    // L25
    if jewelType == 1.0 {
        let nodeIndex = nodeIndex.unwrap_or(f64::NAN);
        // if data is already loaded but table for specific node is not created, just make table and return
        // L27
        if let Some(e) = l.get_mut(&key) {
            let k = match &e.data {
                None => lua_error!("attempt to index field 'data' (a nil value)"),
                Some(LutData::Gv(nodes)) => {
                    gv_node_index(nodes, nodeIndex + 1.0).filter(|&k| nodes[k].raw.is_some())
                }
                // a string indexed with a number is nil
                Some(LutData::Bytes(_)) => None,
            };
            if let Some(k) = k {
                // L28-29
                let seedSize = seedMax(1.0) - seedMin(1.0) + 1.0;
                // L30-36
                gv_split(e, k, nodeIndex, seedSize);
                // L37
                return;
            }
        }
        // L39
        l.insert(
            key,
            LutEntry {
                data: Some(LutData::Gv(Vec::new())),
                ..LutEntry::default()
            },
        );
    } else {
        // L41
        l.insert(key, LutEntry::default());
    }

    // L44
    ConPrintf("LOADING");

    // L46: LUA-QUIRK: gsub's second result (the match count) is passed as
    // cacheUncompressed, which `~= false` turns into true.
    let (name, _count) = jewelTypeName_gsub(jewelType);
    let jewelData = loadJewelFile(&name, true);

    // L48
    if let Some(jewelData) = jewelData {
        let jewelData: Arc<[u8]> = Arc::from(jewelData);
        let Some(e) = l.get_mut(&key) else {
            unreachable!("set at L39/L41")
        };
        // L49
        if jewelType == 1.0 {
            // "Glorious Vanity"
            let nodeIndex = nodeIndex.unwrap_or(f64::NAN);
            // L50
            let GV_nodecount = d.nodeIDList.size;
            // L51
            let seedSize = seedMax(1.0) - seedMin(1.0) + 1.0;
            // L52
            let mut sizeOffset = GV_nodecount * seedSize;
            // L53
            e.buf = Some(jewelData.clone());
            e.sizes = Some(s_sub(jewelData.len(), 1.0, sizeOffset + 1.0));

            // Loop through nodes in order as if we were reading from a file
            // L56
            let mut i = 1.0;
            while i <= GV_nodecount {
                // Find the node this corresponds to
                // L58
                let mut nodeID: Option<i64> = None;
                // L59: LUA-ORDER: pairs(data.nodeIDList); the indices are unique, so the
                // first match is the only one
                for (k, v) in &d.nodeIDList.nodes {
                    // L60
                    if v.index == (i - 1.0) {
                        // L61
                        nodeID = Some(*k);
                        // L62
                        break;
                    }
                }
                // Preliminary initialization
                // L66
                let Some(seedDataLength) = nodeID
                    .and_then(|k| d.nodeIDList.nodes.get(&k))
                    .map(|n| n.size)
                else {
                    lua_error!("attempt to index field '?' (a nil value)")
                };
                // L67
                let Some(LutData::Gv(nodes)) = &mut e.data else {
                    unreachable!("set at L39")
                };
                nodes.push(GvNode::default());
                // L68
                let (a, b) = s_sub(
                    jewelData.len(),
                    sizeOffset + 1.0,
                    sizeOffset + seedDataLength,
                );
                if let Some(node) = nodes.last_mut() {
                    node.raw = Some((a, b));
                }
                // L69
                sizeOffset += seedDataLength;
                // L70
                if i == (nodeIndex + 1.0) {
                    // Final initialization for this seed
                    // L72-79
                    let k = nodes.len() - 1;
                    gv_split(e, k, nodeIndex, seedSize);
                }
                i += 1.0;
            }
            // L82
            ConPrintf(&format!(
                "Glorious Vanity Lookup Table Loaded! Read {} bytes",
                tostring_num(sizeOffset)
            ));
            // L83
            return;
        }
        // L85
        e.data = Some(LutData::Bytes(jewelData));
    }
}

/// @pob Modules/DataLegionLookUpTableHelper.lua:89-96 h=81ee299f
pub fn convertLocalIdToGlobalId(jewelType: f64, localId: f64) -> f64 {
    // L91
    let mappingByJewelType = localIdToGlobalId(jewelType);
    // L92
    if let Some(m) = mappingByJewelType {
        if let Some(v) = int_key(localId).and_then(|k| m.map.get(&k)) {
            // L93
            return *v;
        }
    }
    // L95
    localId
}

/// @pob Modules/DataLegionLookUpTableHelper.lua:97-119 h=b2168677
// --[[
// -- the generation functions needs to be ported to LUA
// local generateNode(jewelType, seed, nodeID)
// 	...
// end
//
// local function generateLUT(jewelType)
// 	...
// end
// --]]
//
//  -- this doesn't rebuilt the list with the correct sizes, likely an issue with lua indexing from 1 instead of 0, but cbf debugging so just generated the index mapping in c#
//  -- is disabled on the data side atm, needs to be setup correctly if we ever setup generation on the LUA side
const _GENERATORS_COMMENT: () = ();

/// @pob Modules/DataLegionLookUpTableHelper.lua:120-219 h=8ae93e11
/// `repairLUTs()`: rebuilds `NodeIndexMapping.lua` from `node_indices.csv` (PoB runs it
/// only in dev mode when that file lacks `size`, Data.lua:961-965). Returns the
/// rebuilt `nodeIDList` (nil → `None`; CCR-0020: the frozen stub returned nothing).
pub fn repairLUTs() -> Option<NodeIdList> {
    let fs = jewel_fs();
    // L121
    ConPrintf("Error NodeIndexMapping file empty");
    // L122
    let mut nodeIDList = NodeIdList {
        size: 0.0,
        sizeNotable: 0.0,
        localIdToGlobalId: Vec::new(),
        nodes: BTreeMap::new(),
    };
    // L123
    let _ = fs.script_path();
    // L124
    for jewelType in [2.0, 3.0, 4.0, 5.0, 6.0] {
        // L125
        loadTimelessJewel(jewelType, 1.0);
        // L126
        let (jewelTypeName, _) = jewelTypeName_gsub(jewelType);
        // L127
        let jewelData = loadJewelFile(&jewelTypeName, true);
        // L128
        if jewelData.is_none() {
            // L129
            ConPrintf("looking for base LUT to rebuild");
            // L130-135: commented-out search loop
            // L136 (and L144): LUA-GLOBAL-NIL: `scriptPath` is an undeclared global here (the local
            // is declared at L158), so the concatenation raises (CCR-0004). L137-150
            // (read the raw file, Deflate it, write the .zip, "GV needs to be split
            // manually") are unreachable.
            lua_error!(
                "Modules/DataLegionLookUpTableHelper.lua:136: attempt to concatenate global 'scriptPath' (a nil value)"
            );
        }
    }
    // L153: assigns the global `jewelData` (LUA-QUIRK: no `local`); gsub's count → true
    let (gvName, _count) = jewelTypeName_gsub(1.0);
    let jewelData = loadJewelFile(&gvName, true);
    // L154
    let Some(jewelData) = jewelData else {
        // L155
        ConPrintf("missing GV file to rebuild NodeIndexMapping");
        return None;
    };
    // L157
    ConPrintf("attempting to rebuild NodeIndexMapping");
    // L158
    let scriptPath = fs.script_path();
    // L159
    let compressedFile = fs.read(&format!(
        "{scriptPath}/Data/TimelessJewelData/node_indices.csv"
    ));
    // L160
    let Some(nodeData) = compressedFile else {
        // L216
        ConPrintf("csv missing, cannot rebuild NodeIndexMapping");
        return None;
    };
    // L161
    ConPrintf("csv found");

    // L165: the global `tempIndList` (LUA-QUIRK: no `local`)
    let mut tempIndList: BTreeMap<i64, f64> = BTreeMap::new();
    // L166-167
    nodeIDList.size = 0.0;
    nodeIDList.sizeNotable = 0.0;
    // L168
    for caps in pob_lua::pattern::gmatch_bytes(&nodeData, b"([^\n]*)\n?") {
        let line = caps.first().map_or(&b""[..], |c| c.as_bytes());
        // L169
        nodeIDList.size += 1.0;
        // L170
        if nodeIDList.size != 1.0 {
            // L171
            if let Some(split) = pob_lua::pattern::gmatch_bytes(line, b"([^,]*),?").next() {
                let split = split.first().map_or(&b""[..], |c| c.as_bytes());
                // L172
                if let Some(v) = pob_lua::format::tonumber_bytes(split) {
                    // L173
                    let k = nodeIDList.size as i64 - 1;
                    tempIndList.insert(k, v);
                    // L174
                    if nodeIDList.size != 2.0 {
                        let Some(prev) = tempIndList.get(&(k - 1)).copied() else {
                            lua_error!("attempt to compare nil with number")
                        };
                        if v < prev {
                            // L175
                            nodeIDList.sizeNotable = nodeIDList.size - 2.0;
                        }
                    }
                }
                // L178: break after the first field
            }
        }
    }
    // L182
    nodeIDList.size -= 2.0;
    // L183-184
    ConPrintf(&tostring_num(nodeIDList.sizeNotable));
    ConPrintf(&tostring_num(nodeIDList.size));

    // L187
    let seedSize = seedMax(1.0) - seedMin(1.0) + 1.0;
    // L188
    let sizeOffset = nodeIDList.size * seedSize;
    // L189-190
    let jewelData: Arc<[u8]> = Arc::from(jewelData);
    let sizes = s_sub(jewelData.len(), 1.0, sizeOffset + 1.0);
    let sizesBytes = jewelData[sizes.0..sizes.1].to_vec();
    luts().insert(
        lut_key(1.0),
        LutEntry {
            buf: Some(jewelData),
            sizes: Some(sizes),
            ..LutEntry::default()
        },
    );
    // L191: ipairs(tempIndList)
    let ipairs: Vec<f64> = (1..).map_while(|i| tempIndList.get(&i).copied()).collect();
    for (i0, &nodeID) in ipairs.iter().enumerate() {
        let i = i0 as f64 + 1.0;
        // L192
        let nodeIndex = i - 1.0;
        // L193
        let mut count = 0.0;
        // L194
        if i > nodeIDList.sizeNotable {
            // L195
            count = seedSize * 2.0;
        } else {
            // L197
            let mut seedOffset = 1.0;
            while seedOffset <= seedSize + 1.0 {
                // L198
                let Some(dataLength) = s_byte(&sizesBytes, nodeIndex * seedSize + seedOffset)
                else {
                    lua_error!("attempt to perform arithmetic on local 'dataLength' (a nil value)")
                };
                // L199
                count += f64::from(dataLength);
                seedOffset += 1.0;
            }
        }
        // L202
        if let Some(k) = int_key(nodeID) {
            nodeIDList.nodes.insert(
                k,
                NodeIndex {
                    index: nodeIndex,
                    size: count,
                },
            );
        }
    }

    // L205-213
    let mut file = String::new();
    file.push_str("nodeIDList = { }\n");
    file.push_str("nodeIDList[\"size\"] = ");
    file.push_str(&tostring_num(nodeIDList.size));
    file.push('\n');
    file.push_str("nodeIDList[\"sizeNotable\"] = ");
    file.push_str(&tostring_num(nodeIDList.sizeNotable));
    file.push('\n');
    for &nodeID in &ipairs {
        let n = int_key(nodeID).and_then(|k| nodeIDList.nodes.get(&k));
        let Some(n) = n else {
            lua_error!("attempt to index field '?' (a nil value)")
        };
        for part in [
            "nodeIDList[",
            &tostring_num(nodeID),
            "] = { index = ",
            &tostring_num(n.index),
            ", size = ",
            &tostring_num(n.size),
            " }\n",
        ] {
            file.push_str(part);
        }
    }
    file.push_str("return nodeIDList");
    let path = "Data/TimelessJewelData/NodeIndexMapping.lua";
    if !fs.write(path, file.as_bytes()) {
        lua_error!("{path}: No such file or directory");
    }
    // L214
    Some(nodeIDList)
}

/// @pob Modules/DataLegionLookUpTableHelper.lua:220-260 h=16172d2f
/// `readLUT(seed, nodeID, jewelType)` → `{ ids… }`.
pub fn readLUT(seed: f64, nodeID: f64, jewelType: f64) -> Vec<f64> {
    let d = data();
    // L222
    loadTimelessJewel(jewelType, nodeID);
    let key = lut_key(jewelType);
    let l = luts();
    // L223
    if jewelType == 1.0 {
        // L224
        match l.get(&key).map(|e| &e.data) {
            None => lua_error!("attempt to index field '?' (a nil value)"),
            Some(None) => lua_error!("bad argument #1 to 'next' (table expected, got nil)"),
            Some(Some(LutData::Bytes(_))) => {
                lua_error!("bad argument #1 to 'next' (table expected, got string)")
            }
            Some(Some(LutData::Gv(nodes))) => {
                if nodes.is_empty() {
                    lua_error!("Error occurred loading Glorious Vanity data");
                }
            }
        }
    } else {
        // L226
        match l.get(&key) {
            None => lua_error!("attempt to index field '?' (a nil value)"),
            Some(e) => {
                if e.data.is_none() {
                    lua_error!("Error occurred loading Timeless Jewel data");
                }
            }
        }
    }
    let Some(e) = l.get(&key) else {
        unreachable!("checked at L224/L226")
    };
    let mut seed = seed;
    // "Elegant Hubris"
    // L229
    if jewelType == 5.0 {
        // L230
        seed /= 20.0;
    }
    // L232
    let seedOffset = seed - seedMin(jewelType);
    // L233
    let seedSize = (seedMax(jewelType) - seedMin(jewelType)) + 1.0;
    // L234
    let index = nodeIDList_get(nodeID).map(|n| n.index);
    // L235
    if let Some(index) = index {
        // "Glorious Vanity"
        // L237
        if jewelType == 1.0 {
            // L238
            let mut result: Vec<f64> = Vec::new();
            // L239
            let Some(dataLength) = s_byte(e.sizes(), index * seedSize + seedOffset + 1.0) else {
                lua_error!("'for' limit must be a number")
            };
            let dataLength = f64::from(dataLength);
            // L240
            let mut i = 1.0;
            while i <= dataLength {
                // L241
                let Some(LutData::Gv(nodes)) = &e.data else {
                    unreachable!("checked at L224")
                };
                let Some(node) = gv_node_index(nodes, index + 1.0).map(|k| &nodes[k]) else {
                    lua_error!("attempt to index field '?' (a nil value)")
                };
                let Some(s) = int_key(seedOffset + 1.0)
                    .filter(|&k| k >= 1 && (k as usize) <= node.seedStart.len())
                    .map(|k| k as usize - 1)
                else {
                    lua_error!("attempt to index field '?' (a nil value)")
                };
                let buf = e.buf.as_deref().unwrap_or(&[]);
                let start = node.seedStart[s] as usize;
                let str_ = &buf[start..start + usize::from(node.seedLen[s])];
                // LUA-QUIRK: past the end of a (short) seed string `byte` gives no value
                // and Lua stores nil; every later index is past the end too, so the
                // table is exactly the bytes that exist.
                if let Some(b) = s_byte(str_, i) {
                    result.push(f64::from(b));
                }
                i += 1.0;
            }
            // replacement id in first byte, or addition ids in first half for legacy/might variants
            // L244
            if dataLength == 2.0 || dataLength == 3.0 {
                // L245
                if let Some(r) = result.first_mut() {
                    *r = convertLocalIdToGlobalId(jewelType, *r);
                }
            } else if dataLength == 6.0 || dataLength == 8.0 {
                // L247
                let mut i = 1.0;
                while i <= (dataLength / 2.0) {
                    // L248
                    if let Some(r) = result.get_mut(i as usize - 1) {
                        *r = convertLocalIdToGlobalId(jewelType, *r);
                    }
                    i += 1.0;
                }
            }
            // L251
            return result;
        } else if index <= d.nodeIDList.sizeNotable {
            // L253
            let bytes: &[u8] = match &e.data {
                Some(LutData::Bytes(b)) => b,
                _ => lua_error!("attempt to call method 'byte' (a nil value)"),
            };
            let localId = s_byte(bytes, index * seedSize + seedOffset + 1.0);
            // L254: `{ convertLocalIdToGlobalId(jewelType, nil) }` is `{ }`
            return match localId {
                Some(localId) => vec![convertLocalIdToGlobalId(jewelType, f64::from(localId))],
                None => Vec::new(),
            };
        }
    } else {
        drop(l);
        // L257
        ConPrintf(&format!(
            "ERROR: Missing Index lookup for nodeID: {}",
            tostring_num(nodeID)
        ));
    }
    // L259
    Vec::new()
}

/// @pob Modules/DataLegionLookUpTableHelper.lua:261-262 h=f5ad65e4
/// `return { readLUT = readLUT, repairLUTs = repairLUTs }`: the pub fns above.
const _LEGION_EXPORTS: () = ();

// ---- Modules/DataAbyssJewelLookUpTableHelper.lua ----

/// @pob Modules/DataAbyssJewelLookUpTableHelper.lua:1-9 h=a8455220
/// Module header (`s_byte`, `s_sub`, `loadJewelFile` locals).
const _ABYSS_HEADER: () = ();

/// @pob Modules/DataAbyssJewelLookUpTableHelper.lua:10-12 h=dfa01c64
/// `readUInt16(jewelData, offset)` → (value, next offset). Offsets are Lua's (1-based).
pub fn readUInt16(jewelData: &[u8], offset: usize) -> (f64, usize) {
    // L11
    (bytesToUShort(jewelData, offset as f64), offset + 2)
}

/// @pob Modules/DataAbyssJewelLookUpTableHelper.lua:13-24 h=95f85e10
// A modification starts with its number of parts. Each part then stores whether
// it replaces or adds stats, its stat ID, its roll count, and its two-byte rolls.
pub fn skipModification(jewelData: &[u8], offset: usize) -> usize {
    let mut offset = offset;
    // L17
    let componentCount = s_byte(jewelData, offset as f64);
    // L18
    offset += 1;
    // L19
    let mut c = 1.0;
    let limit = for_limit(componentCount);
    while c <= limit {
        // L20
        let Some(statCount) = s_byte(jewelData, (offset + 2) as f64) else {
            lua_error!("attempt to perform arithmetic on local 'statCount' (a nil value)")
        };
        // L21
        offset = offset + 3 + usize::from(statCount) * 2;
        c += 1.0;
    }
    // L23
    offset
}

/// @pob Modules/DataAbyssJewelLookUpTableHelper.lua:25-47 h=ecd7a333
/// → (modification table, next offset). A modification is `{ [i] = { type = …,
/// id = …, rolls = { … } } }`.
pub fn readModification(jewelData: &[u8], offset: usize, jewelType: f64) -> (Value, usize) {
    let mut offset = offset;
    // L27
    let componentCount = s_byte(jewelData, offset as f64);
    // L28
    let mut modification = ValueTable::new();
    // L29
    offset += 1;
    // L30
    let mut componentIndex = 1.0;
    let limit = for_limit(componentCount);
    while componentIndex <= limit {
        // L31
        let componentType = s_byte(jewelData, offset as f64);
        let localId = s_byte(jewelData, (offset + 1) as f64);
        let statCount = s_byte(jewelData, (offset + 2) as f64);
        // L32
        offset += 3;
        // L33
        let mapping = localIdToGlobalId(jewelType);
        // L34-38
        let id = localId.map(|localId| {
            mapping
                .and_then(|m| m.map.get(&i64::from(localId)).copied())
                .unwrap_or(f64::from(localId))
        });
        let mut component = ValueTable::new();
        component.set(n!("type"), componentType.map(f64::from).into());
        component.set(n!("id"), id.into());
        let mut rolls = ValueTable::new();
        // L39
        let mut statIndex = 1.0;
        let slimit = for_limit(statCount);
        while statIndex <= slimit {
            // L40
            let roll;
            // L41
            (roll, offset) = readUInt16(jewelData, offset);
            // L42
            rolls.array.push(Value::Num(if roll >= 32768.0 {
                roll - 65536.0
            } else {
                roll
            }));
            statIndex += 1.0;
        }
        component.set(n!("rolls"), rolls.into());
        // L44
        modification.array.push(component.into());
        componentIndex += 1.0;
    }
    // L46
    (modification.into(), offset)
}

/// `t[k]` for a number key on a table value (indexing a non-table raises).
fn value_index_num(t: &Value, k: f64, what: &str) -> Value {
    match t {
        Value::Table(t) => int_key(k)
            .filter(|&k| k >= 1)
            .map_or(Value::Nil, |k| t.index(k as usize).clone()),
        _ => lua_error!("attempt to index field '{what}' (a nil value)"),
    }
}

/// `component.id` in arithmetic.
fn component_id(component: &Value) -> f64 {
    match component.get(n!("id")).num() {
        Some(x) => x,
        None => lua_error!("attempt to perform arithmetic on field 'id' (a nil value)"),
    }
}

/// @pob Modules/DataAbyssJewelLookUpTableHelper.lua:48-57 h=d5b32a4a
// The file uses type 1 for a full node replacement and type 2 for stats added
// to the original node. Return the matching existing Legion node data.
/// → (legion node or addition, isNode); `(Nil, None)` for any other type (no values).
/// `legion` is the LegionPassives table (`{ nodes = { … }, additions = { … } }`).
pub fn resolveAbyssJewelComponent(component: &Value, legion: &Value) -> (Value, Option<bool>) {
    if !matches!(component, Value::Table(_)) {
        lua_error!("attempt to index local 'component' (a nil value)");
    }
    let ty = component.get(n!("type"));
    // L52
    if ty.num() == Some(1.0) {
        // L53
        let node = value_index_num(
            legion.get(n!("nodes")),
            component_id(component) + 1.0 - data().timeless.timelessJewelAdditions,
            "nodes",
        );
        (node, Some(true))
    } else if ty.num() == Some(2.0) {
        // L55
        let addition = value_index_num(
            legion.get(n!("additions")),
            component_id(component) + 1.0,
            "additions",
        );
        (addition, Some(false))
    } else {
        (Value::Nil, None)
    }
}

/// @pob Modules/DataAbyssJewelLookUpTableHelper.lua:58-73 h=895d7a06
/// → (statKey, statMod, roll).
pub fn getAbyssJewelComponentRoll(
    component: &Value,
    node: &Value,
    statIndex: f64,
) -> (Value, Value, Option<f64>) {
    // L60
    let mut statKey = Value::Nil;
    let mut statMod = Value::Nil;
    // L61: LUA-ORDER: pairs(node.stats), string keys (stat ids) in dump order; the
    // stat indices of a legion node are unique, so at most one entry matches
    let Value::Table(stats) = node.get(n!("stats")) else {
        lua_error!("bad argument #1 to 'pairs' (table expected, got nil)")
    };
    let entries = stats
        .array
        .iter()
        .enumerate()
        .map(|(i, v)| (Value::Num(i as f64 + 1.0), v))
        .chain(stats.fields.iter().map(|(k, v)| (Value::from(*k), v)));
    for (key, m) in entries {
        // L62
        if m.get(n!("index")).num() == Some(statIndex) {
            // L63
            statKey = key;
            statMod = m.clone();
            // L64
            break;
        }
    }
    // L67
    let mut roll = if statMod.truthy() {
        let rolls = component.get(n!("rolls"));
        let idx = statMod.get(n!("index")).num().unwrap_or(f64::NAN);
        value_index_num(rolls, idx, "rolls").num()
    } else {
        None
    };
    // A negative stored value is shown as a positive number in "reduced" descriptions.
    // L69
    if let Some(r) = roll {
        if r < 0.0 {
            let Some(min) = statMod.get(n!("min")).num() else {
                lua_error!("attempt to compare nil with number")
            };
            if min >= 0.0 {
                // L70
                roll = Some(-r);
            }
        }
    }
    // L72
    (statKey, statMod, roll)
}

/// @pob Modules/DataAbyssJewelLookUpTableHelper.lua:74-82 h=2747e296
pub fn skipSocketRecord(jewelData: &[u8], offset: usize) -> usize {
    let mut offset = offset;
    // L76
    let affectedNodeCount = s_byte(jewelData, offset as f64);
    // L77
    offset += 1;
    // L78
    let mut c = 1.0;
    let limit = for_limit(affectedNodeCount);
    while c <= limit {
        // L79
        offset = skipModification(jewelData, offset + 2);
        c += 1.0;
    }
    // L81
    offset
}

/// @pob Modules/DataAbyssJewelLookUpTableHelper.lua:83-86 h=f3d82384
pub fn skipAscendancyRecord(jewelData: &[u8], offset: usize) -> usize {
    // L85
    let Some(count) = s_byte(jewelData, offset as f64) else {
        lua_error!("attempt to perform arithmetic on a nil value")
    };
    offset + 1 + usize::from(count) * 2
}

/// A `seedOffsets` key: a socket / node id or `"ascendancy:" .. name`.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum BlockKey {
    Num(u64),
    Str(Vec<u8>),
}

impl BlockKey {
    /// A number key (`-0` and `0` are the same key, as in Lua).
    pub fn num(x: f64) -> Self {
        BlockKey::Num(if x == 0.0 { 0 } else { x.to_bits() })
    }
}

/// The table `parseAbyssJewel` returns (`data.timelessJewelLUTs[7..11]`).
#[derive(Debug)]
pub struct AbyssJewelLUT {
    /// `data`: the whole file.
    pub data: Arc<[u8]>,
    /// `format`: the first four bytes (`"ABYS"` / `"ABYN"`).
    pub format: Vec<u8>,
    pub seedMinimum: f64,
    pub seedMaximum: f64,
    pub seedIncrement: f64,
    pub seedCount: f64,
    /// `seedOffsets[blockKey]`: the start of every seed's record, filled on first use.
    pub seedOffsets: Mutex<HashMap<BlockKey, Arc<Vec<usize>>>>,
    /// `blockOffsets[socketId | nodeId]`.
    pub blockOffsets: BTreeMap<i64, usize>,
    /// `abyssSize` (ABYS only).
    pub abyssSize: Option<f64>,
    /// `ascendancyOffsets[name]` (ABYN only), in file order.
    pub ascendancyOffsets: Option<Vec<(Vec<u8>, usize)>>,
}

impl AbyssJewelLUT {
    /// `jewelLUT.blockOffsets[id]`.
    pub fn blockOffset(&self, id: f64) -> Option<usize> {
        int_key(id).and_then(|k| self.blockOffsets.get(&k).copied())
    }
}

/// @pob Modules/DataAbyssJewelLookUpTableHelper.lua:87-164 h=5df677b7
/// CCR-0020: takes the file as `Arc<[u8]>` and returns the struct (a Lua string
/// cannot live in an immutable `Value`).
pub fn parseAbyssJewel(jewelType: f64, jewelData: Arc<[u8]>) -> AbyssJewelLUT {
    // The header stores the four-letter format, version, jewel type, first and
    // last seed, and the amount between seeds.
    // L91
    let (fa, fb) = s_sub(jewelData.len(), 1.0, 4.0);
    let format = jewelData[fa..fb].to_vec();
    // L92
    let formatVersion = s_byte(&jewelData, 5.0);
    // L93
    let storedJewelType = s_byte(&jewelData, 6.0);
    // L94
    let (seedMinimum, mut offset) = readUInt16(&jewelData, 7);
    // L95-96
    let seedMaximum;
    (seedMaximum, offset) = readUInt16(&jewelData, offset);
    // L97-98
    let seedIncrement;
    (seedIncrement, offset) = readUInt16(&jewelData, offset);
    // L99
    if formatVersion != Some(1)
        || storedJewelType.map(f64::from) != Some(jewelType)
        || seedIncrement == 0.0
    {
        // L100
        lua_error!(
            "Modules/DataAbyssJewelLookUpTableHelper.lua:100: Invalid Abyss timeless jewel header for jewel type {}",
            tostring_num(jewelType)
        );
    }

    // L103-112
    let mut jewelLUT = AbyssJewelLUT {
        data: jewelData.clone(),
        format,
        seedMinimum,
        seedMaximum,
        seedIncrement,
        seedCount: (seedMaximum - seedMinimum) / seedIncrement + 1.0,
        seedOffsets: Mutex::new(HashMap::new()),
        blockOffsets: BTreeMap::new(),
        abyssSize: None,
        ascendancyOffsets: None,
    };
    let jewelData = &*jewelData;

    // L114
    if jewelLUT.format == b"ABYS" {
        // Each socket has one entry for every seed.
        // L116
        let socketCount = s_byte(jewelData, offset as f64);
        // L117
        jewelLUT.abyssSize = s_byte(jewelData, (offset + 1) as f64).map(f64::from);
        // L118
        offset += 2;
        // L119
        let mut socketIds: Vec<f64> = Vec::new();
        // L120
        let mut socketIndex = 1.0;
        let limit = for_limit(socketCount);
        while socketIndex <= limit {
            // L121
            let id;
            (id, offset) = readUInt16(jewelData, offset);
            socketIds.push(id);
            socketIndex += 1.0;
        }
        // L123
        for &socketId in &socketIds {
            // L124
            if let Some(k) = int_key(socketId) {
                jewelLUT.blockOffsets.insert(k, offset);
            }
            // L125
            let mut s = 1.0;
            while s <= jewelLUT.seedCount {
                // L126
                offset = skipSocketRecord(jewelData, offset);
                s += 1.0;
            }
        }
    } else if jewelLUT.format == b"ABYN" {
        // Zorath has one block per passive node, so only allocated path nodes are read later.
        // L131-132
        let nodeCount;
        (nodeCount, offset) = readUInt16(jewelData, offset);
        // L133
        let mut nodeIds: Vec<f64> = Vec::new();
        // L134
        let mut nodeIndex = 1.0;
        while nodeIndex <= nodeCount {
            // L135
            let id;
            (id, offset) = readUInt16(jewelData, offset);
            nodeIds.push(id);
            nodeIndex += 1.0;
        }
        // L137
        for &nodeId in &nodeIds {
            // L138
            if let Some(k) = int_key(nodeId) {
                jewelLUT.blockOffsets.insert(k, offset);
            }
            // L139
            let mut s = 1.0;
            while s <= jewelLUT.seedCount {
                // L140
                offset = skipModification(jewelData, offset);
                s += 1.0;
            }
        }

        // L144
        let (a, b) = s_sub(jewelData.len(), offset as f64, (offset + 3) as f64);
        if &jewelData[a..b] != b"ASCS" {
            // L145
            lua_error!(
                "Modules/DataAbyssJewelLookUpTableHelper.lua:145: Missing Abyss ascendancy section for jewel type {}",
                tostring_num(jewelType)
            );
        }
        // L147
        offset += 4;
        // L148-149
        let ascendancyCount;
        (ascendancyCount, offset) = readUInt16(jewelData, offset);
        // L150
        let mut ascendancyOffsets: Vec<(Vec<u8>, usize)> = Vec::new();
        // L151
        let mut c = 1.0;
        while c <= ascendancyCount {
            // L152
            let Some(nameLength) = s_byte(jewelData, offset as f64) else {
                lua_error!("attempt to perform arithmetic on local 'nameLength' (a nil value)")
            };
            let nameLength = usize::from(nameLength);
            // L153
            let (a, b) = s_sub(
                jewelData.len(),
                (offset + 1) as f64,
                (offset + nameLength) as f64,
            );
            let ascendancyName = jewelData[a..b].to_vec();
            // L154
            offset = offset + nameLength + 1;
            // L155: a repeated name overwrites its value in place
            match ascendancyOffsets
                .iter_mut()
                .find(|(n, _)| *n == ascendancyName)
            {
                Some(e) => e.1 = offset,
                None => ascendancyOffsets.push((ascendancyName, offset)),
            }
            // L156
            let mut s = 1.0;
            while s <= jewelLUT.seedCount {
                // L157
                offset = skipAscendancyRecord(jewelData, offset);
                s += 1.0;
            }
            c += 1.0;
        }
        jewelLUT.ascendancyOffsets = Some(ascendancyOffsets);
    } else {
        // L161
        lua_error!(
            "Modules/DataAbyssJewelLookUpTableHelper.lua:161: Unknown Abyss timeless jewel format {}",
            String::from_utf8_lossy(&jewelLUT.format)
        );
    }
    // L163
    jewelLUT
}

/// @pob Modules/DataAbyssJewelLookUpTableHelper.lua:165-178 h=4ff767b2
/// CCR-0020: returns the cached [`AbyssJewelLUT`] (`None` = nil).
pub fn loadAbyssJewel(jewelType: f64) -> Option<Arc<AbyssJewelLUT>> {
    let key = lut_key(jewelType);
    // L167
    if let Some(e) = luts().get(&key) {
        if e.data.is_some() {
            // L168
            return match &e.abyss {
                Some(a) => Some(a.clone()),
                // LUA-QUIRK: a legion entry (readLUT on an abyss type) is returned as is,
                // and the caller's `seed - jewelLUT.seedMinimum` raises at once.
                None => {
                    lua_error!("attempt to perform arithmetic on field 'seedMinimum' (a nil value)")
                }
            };
        }
    }

    // L171
    let (jewelTypeName, _) = jewelTypeName_gsub(jewelType);
    // L172
    let jewelData = loadJewelFile(&jewelTypeName, false);
    // L173
    let jewelData = jewelData?;
    // L176
    let lut = Arc::new(parseAbyssJewel(jewelType, Arc::from(jewelData)));
    luts().insert(
        key,
        LutEntry {
            data: Some(LutData::Bytes(lut.data.clone())),
            abyss: Some(lut.clone()),
            ..LutEntry::default()
        },
    );
    // L177
    Some(lut)
}

/// @pob Modules/DataAbyssJewelLookUpTableHelper.lua:179-186 h=6f5113eb
/// CCR-0020: takes the struct.
pub fn getSeedIndex(jewelLUT: &AbyssJewelLUT, seed: f64) -> Option<f64> {
    // L181
    let seedOffset = seed - jewelLUT.seedMinimum;
    // L182
    if seedOffset < 0.0
        || seed > jewelLUT.seedMaximum
        || lmod(seedOffset, jewelLUT.seedIncrement) != 0.0
    {
        // L183
        return None;
    }
    // L185
    Some(seedOffset / jewelLUT.seedIncrement + 1.0)
}

/// @pob Modules/DataAbyssJewelLookUpTableHelper.lua:187-203 h=d68c25be
/// CCR-0020: struct, [`BlockKey`], `usize` offsets and a shared result (the memo is
/// read for every seed).
pub fn getRecordOffsets(
    jewelLUT: &AbyssJewelLUT,
    blockKey: &BlockKey,
    blockOffset: Option<usize>,
    skipRecord: fn(&[u8], usize) -> usize,
) -> Option<Arc<Vec<usize>>> {
    // L189
    let blockOffset = blockOffset?;
    let mut seedOffsets = jewelLUT
        .seedOffsets
        .lock()
        .unwrap_or_else(PoisonError::into_inner);
    // L192
    if !seedOffsets.contains_key(blockKey) {
        // Entry sizes vary, so remember where each seed starts after the first scan.
        // L194
        let mut offsets: Vec<usize> = Vec::new();
        // L195
        let mut offset = blockOffset;
        // L196
        let mut seedIndex = 1.0;
        while seedIndex <= jewelLUT.seedCount {
            // L197
            offsets.push(offset);
            // L198
            offset = skipRecord(&jewelLUT.data, offset);
            seedIndex += 1.0;
        }
        // L200
        seedOffsets.insert(blockKey.clone(), Arc::new(offsets));
    }
    // L202
    seedOffsets.get(blockKey).cloned()
}

/// `offsets[seedIndex]` (nil → the following `s_byte(data, nil)` raises).
fn offset_at(offsets: &[usize], seedIndex: f64) -> usize {
    match int_key(seedIndex)
        .filter(|&k| k >= 1 && (k as usize) <= offsets.len())
        .map(|k| offsets[k as usize - 1])
    {
        Some(o) => o,
        None => lua_error!("bad argument #2 to 'byte' (number expected, got nil)"),
    }
}

fn empty_table() -> Value {
    ValueTable::new().into()
}

/// @pob Modules/DataAbyssJewelLookUpTableHelper.lua:204-213 h=e2f461a6
pub fn readNode(seed: f64, nodeId: f64, jewelType: f64) -> Value {
    // L206
    let jewelLUT = loadAbyssJewel(jewelType);
    // L207
    let seedIndex = jewelLUT.as_ref().and_then(|l| getSeedIndex(l, seed));
    // L208
    let (Some(jewelLUT), Some(seedIndex)) = (jewelLUT, seedIndex) else {
        return empty_table();
    };
    if jewelLUT.format != b"ABYN" {
        // L209
        return empty_table();
    }
    // L211
    let offsets = getRecordOffsets(
        &jewelLUT,
        &BlockKey::num(nodeId),
        jewelLUT.blockOffset(nodeId),
        skipModification,
    );
    // L212
    match offsets {
        Some(offsets) => {
            readModification(&jewelLUT.data, offset_at(&offsets, seedIndex), jewelType).0
        }
        None => empty_table(),
    }
}

/// `t[k] = v` on the integer-keyed `affectedNodes`.
fn set_node(t: &mut LjIntMap<Value>, nodeId: f64, v: Value) {
    match int_key(nodeId) {
        Some(k) => t.set(k, Some(v)),
        None => lua_error!("LjIntMap: non-integer node id {}", tostring_num(nodeId)),
    }
}

/// @pob Modules/DataAbyssJewelLookUpTableHelper.lua:214-273 h=371a2ce9
/// `readAbyssJewelLUT(seed, socketId, jewelType, path, ascendancyName)` →
/// `{ [nodeId] = modification }`. `path` is the Lua table `{ [nodeId] = true }`
/// given as its keys in `pairs()` order (`LjIntMap::keys`), `None` for nil.
/// CCR-0020: the result is the integer-keyed table as an [`LjIntMap`].
pub fn readAbyssJewelLUT(
    seed: f64,
    socketId: f64,
    jewelType: f64,
    path: Option<&[f64]>,
    ascendancyName: Option<&str>,
) -> LjIntMap<Value> {
    // L216
    if (7.0..=10.0).contains(&jewelType) {
        // L217
        let jewelLUT = loadAbyssJewel(jewelType);
        // L218
        let seedIndex = jewelLUT.as_ref().and_then(|l| getSeedIndex(l, seed));
        // L219
        let (Some(jewelLUT), Some(seedIndex)) = (jewelLUT, seedIndex) else {
            return LjIntMap::new();
        };
        if jewelLUT.format != b"ABYS" {
            // L220
            return LjIntMap::new();
        }
        // L222
        let offsets = getRecordOffsets(
            &jewelLUT,
            &BlockKey::num(socketId),
            jewelLUT.blockOffset(socketId),
            skipSocketRecord,
        );
        // L223
        let Some(offsets) = offsets else {
            // L224
            return LjIntMap::new();
        };
        let data = &*jewelLUT.data;

        // L227
        let mut offset = offset_at(&offsets, seedIndex);
        // L228
        let affectedNodeCount = s_byte(data, offset as f64);
        // L229
        let mut affectedNodes = LjIntMap::new();
        // L230
        offset += 1;
        // L231
        let mut c = 1.0;
        let limit = for_limit(affectedNodeCount);
        while c <= limit {
            // L232-233
            let nodeId;
            (nodeId, offset) = readUInt16(data, offset);
            // L234
            let modification;
            (modification, offset) = readModification(data, offset, jewelType);
            set_node(&mut affectedNodes, nodeId, modification);
            c += 1.0;
        }
        // L236
        return affectedNodes;
    } else if jewelType != 11.0 || path.is_none() {
        // L238
        return LjIntMap::new();
    }

    // L241
    let mut affectedNodes = LjIntMap::new();
    // L242: pairs(path) (int keys; the caller passes LuaJIT's order)
    for &nodeId in path.unwrap_or(&[]) {
        // L243
        let modification = readNode(seed, nodeId, jewelType);
        // L244: next(modification)
        if modification.as_table().is_some_and(|t| !t.is_empty()) {
            // L245
            set_node(&mut affectedNodes, nodeId, modification);
        }
    }
    // L248
    let jewelLUT = loadAbyssJewel(jewelType);
    // L249
    let seedIndex = jewelLUT.as_ref().and_then(|l| getSeedIndex(l, seed));
    // L250
    if let (Some(jewelLUT), Some(seedIndex)) = (jewelLUT, seedIndex) {
        if jewelLUT.format == b"ABYN" {
            // A Zorath seed selects one node in every ascendancy. A name limits the
            // result to one ascendancy when a caller only needs that section.
            // L253
            let all = jewelLUT.ascendancyOffsets.clone().unwrap_or_default();
            let mut ascendancyOffsets = all.clone();
            // L254
            if let Some(name) = ascendancyName {
                // L255
                ascendancyOffsets = all
                    .into_iter()
                    .filter(|(n, _)| n.as_slice() == name.as_bytes())
                    .collect();
            }
            // L257: LUA-ORDER: pairs over string keys (random in PoB); file order here
            for (name, blockOffset) in &ascendancyOffsets {
                // L258
                let mut blockKey = b"ascendancy:".to_vec();
                blockKey.extend_from_slice(name);
                let blockKey = BlockKey::Str(blockKey);
                // L259
                let offsets = getRecordOffsets(
                    &jewelLUT,
                    &blockKey,
                    Some(*blockOffset),
                    skipAscendancyRecord,
                );
                // L260
                if let Some(offsets) = offsets {
                    let data = &*jewelLUT.data;
                    // L261
                    let mut offset = offset_at(&offsets, seedIndex);
                    // L262
                    let selectedCount = s_byte(data, offset as f64);
                    // L263
                    offset += 1;
                    // L264
                    let mut c = 1.0;
                    let limit = for_limit(selectedCount);
                    while c <= limit {
                        // L265-266
                        let nodeId;
                        (nodeId, offset) = readUInt16(data, offset);
                        // L267
                        let modification = readNode(seed, nodeId, jewelType);
                        set_node(&mut affectedNodes, nodeId, modification);
                        c += 1.0;
                    }
                }
            }
        }
    }
    // L272
    affectedNodes
}

/// @pob Modules/DataAbyssJewelLookUpTableHelper.lua:274-275 h=9dff1a62
/// `return readAbyssJewelLUT, resolveAbyssJewelComponent, getAbyssJewelComponentRoll,
/// parseAbyssJewel`: the pub fns above.
const _ABYSS_EXPORTS: () = ();
