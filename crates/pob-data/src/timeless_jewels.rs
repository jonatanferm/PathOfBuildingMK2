//! Timeless jewel keystone-replacement catalogue — slice 1 of
//! [#30](https://github.com/jonatanferm/PathOfBuildingMK2/issues/30).
//!
//! Mirrors PoB's `Data/TimelessJewelData/LegionPassives.lua` for the **keystone**
//! variants only. When a Timeless jewel is socketed into a tree socket, every
//! keystone whose tree position falls inside the jewel's radius is replaced by
//! the conqueror's keystone (e.g. Glorious Vanity / Doryani replaces every
//! allocated keystone in radius with `vaal_keystone_3` → "Corrupted Soul").
//!
//! Notable replacement and small-node replacement also exist in PoB but require
//! per-seed lookup tables (`data.readLUT(conqueror.id, node.id, jewelType)`)
//! that come from compressed binary blobs in
//! `.PathOfBuilding/src/Data/TimelessJewelData/*.zip`. This slice ships only the
//! keystone half of the mechanic, which is plain text and accounts for the
//! highest-impact Timeless plays in practice (Glorious Vanity / Doryani →
//! Corrupted Soul stacking, Militant Faith / Maxarius → Transcendence, etc.).
//!
//! Notable / small-node replacement is reserved for a follow-up slice. The data
//! layout here keeps a `keystones` map keyed by `<conqueror_type>_keystone_<id>`
//! so adding `notables` / `small_nodes` maps later is purely additive.
//!
//! ## Data shape
//!
//! ```text
//! {
//!   "version": 1,
//!   "jewels": {
//!     "Glorious Vanity": {
//!       "conqueror_type": "vaal",
//!       "conquerors": [
//!         { "name": "Doryani", "conqueror_id": "3", "keystone_id": "vaal_keystone_3" },
//!         …
//!       ]
//!     },
//!     …
//!   },
//!   "keystones": {
//!     "vaal_keystone_3": {
//!       "name": "Corrupted Soul",
//!       "stats": ["50% of Non-Chaos Damage taken bypasses Energy Shield", …]
//!     },
//!     …
//!   }
//! }
//! ```

use indexmap::IndexMap;
use serde::{Deserialize, Serialize};

/// Loaded timeless-jewel catalogue. Indexed lookups by jewel base name and by
/// conqueror keystone id are O(1).
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct TimelessJewelData {
    /// Schema version. Currently `1`.
    #[serde(default = "default_version")]
    pub version: u32,
    /// Free-form comment so the on-disk JSON is self-describing for humans.
    #[serde(default, rename = "_comment", skip_serializing_if = "String::is_empty")]
    pub comment: String,
    /// Per-jewel-base configuration. Keyed by the jewel's base name as it
    /// appears on the item (`"Glorious Vanity"`, `"Lethal Pride"`, …).
    pub jewels: IndexMap<String, TimelessJewelConfig>,
    /// Conqueror-keystone mod text, keyed by the canonical PoB keystone id
    /// (`"vaal_keystone_1"` → Divine Flesh, `"vaal_keystone_3"` → Corrupted
    /// Soul, …). Each entry's `stats` lines flow into the player's modDB
    /// when the keystone replaces an allocated in-radius keystone.
    pub keystones: IndexMap<String, ConquerorKeystone>,
}

fn default_version() -> u32 {
    1
}

/// Per-jewel-base config. The jewel's mod text (`"Bathed in the blood of N
/// sacrificed in the name of <conqueror>"`) reveals which conqueror is at play;
/// the parser then looks up the matching `TimelessConqueror` to find the
/// replacement keystone id.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct TimelessJewelConfig {
    /// Conqueror nation name, used to build the keystone-id prefix when joining
    /// `<type>_keystone_<conqueror.id>`. One of `"vaal"` / `"karui"` /
    /// `"maraketh"` / `"templar"` / `"eternal"` / `"kalguur"`. Mirrors
    /// `conqueror.type` in PoB's `Data/ModCache.lua` `JewelData` payloads.
    pub conqueror_type: String,
    /// All conquerors a jewel of this base can be carved by (one variant per
    /// row in PoB's `Variant: …` block on the unique jewel template).
    pub conquerors: Vec<TimelessConqueror>,
}

/// One conqueror variant for a Timeless jewel. The `name` is what appears in
/// the jewel's mod text (`"…in the name of Doryani"` → `name = "Doryani"`).
/// `conqueror_id` matches PoB's `conqueror.id` field (sometimes a string like
/// `"2_v2"` for the post-3.11 v2 keystones, hence string-not-int).
/// `keystone_id` resolves into `TimelessJewelData.keystones[<id>]`.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct TimelessConqueror {
    pub name: String,
    pub conqueror_id: String,
    pub keystone_id: String,
}

/// Mod text for one conqueror keystone. PoB stores per-stat scaling info too
/// (`min` / `max` / `fmt`); for keystones the values are always fixed (PoB's
/// keystones never roll), so we keep just the rendered stat lines that flow
/// into `mod_parser`.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ConquerorKeystone {
    /// Display name (`"Corrupted Soul"`, `"Transcendence"`, …).
    pub name: String,
    /// One stat line per element of the upstream `sd[]` array.
    #[serde(default)]
    pub stats: Vec<String>,
}

/// Decode a `data/timeless_jewels.json` payload.
pub fn load_timeless_jewels(json: &str) -> serde_json::Result<TimelessJewelData> {
    serde_json::from_str(json)
}

/// Per-stat roll metadata for one [`LegionPassive`] stat — PoB's `stats[key] =
/// { index, min, max, fmt }`. When a Glorious Vanity jewel replaces a node, the
/// per-seed LUT returns roll *values* that PoB substitutes into the `(min-max)`
/// ranges in the `sd[]` lines (`replaceHelperFunc` in `PassiveSpec.lua`); this
/// carries the data that substitution needs.
#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq)]
pub struct LegionStat {
    /// Stat key (`"fire_damage_+%"`). Joins an `sd[]` line (via `sorted_stats`)
    /// to its roll slot.
    pub key: String,
    /// 1-based position of this stat's roll value inside the LUT entry
    /// (`statMod.index`; the roll is `entry[index]` 0-based in our decode).
    pub index: u32,
    /// Range minimum as rendered in the `sd[]` string.
    pub min: f64,
    /// Range maximum.
    pub max: f64,
    /// PoB number format: `"d"` (plain integer) or `"g"` (special scaling —
    /// `per_minute` ÷60, `permyriad` ÷100, `_ms` ÷1000).
    pub fmt: String,
}

/// One entry from a PoB timeless replacement-passive pool
/// (`Data/TimelessJewelData/LegionPassives.lua`). Two pools exist: `additions`
/// (small-node stat injections, 96 entries) and `nodes` (the replacement-notable
/// pool, `legionNodes`). The per-seed LUT (#227) yields a *global id* that
/// selects between them (`id < 96` → additions, `id >= 96` → nodes); see
/// [`LegionPool`]. Keystone replacements use the separate [`ConquerorKeystone`]
/// path.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct LegionPassive {
    /// Display name (`"Fire Damage"`, `"Doryani, Royal Architect"`, …).
    pub name: String,
    /// Stable identifier (`"vaal_small_fire_damage"`, …).
    pub id: String,
    /// One stat line per element of the upstream `sd[]` array. Roll ranges
    /// (`"(7-12)% increased Fire Damage"`) are preserved verbatim as PoB
    /// renders them; Glorious Vanity substitutes a rolled value at apply time.
    #[serde(default)]
    pub stats: Vec<String>,
    /// `sortedStats[]` — the stat key for each `sd[]` line, parallel to
    /// `stats`. Used to look up the matching [`LegionStat`] roll metadata.
    #[serde(default)]
    pub sorted_stats: Vec<String>,
    /// `stats{}` flattened — roll metadata keyed by stat key.
    #[serde(default)]
    pub rolls: Vec<LegionStat>,
}

impl LegionPassive {
    /// Roll metadata for `key`, if present.
    #[must_use]
    pub fn roll(&self, key: &str) -> Option<&LegionStat> {
        self.rolls.iter().find(|r| r.key == key)
    }
}

/// PoB's two timeless replacement pools (`legionAdditions` + `legionNodes`),
/// loaded from `legion_passives.json` (additions) and `legion_nodes.json`
/// (nodes). The decode yields a global id; [`LegionPool::resolve`] dispatches it
/// to the right pool exactly as PoB does (split at `additions.len()` ==
/// `data.timelessJewelAdditions` == 96).
#[derive(Debug, Clone, Default)]
pub struct LegionPool {
    /// `legionAdditions` — global id `g` (`g < split`) resolves to
    /// `additions[g]` (PoB's `legionAdditions[g + 1]`).
    pub additions: Vec<LegionPassive>,
    /// `legionNodes` — the replacement-notable pool. Global id `g`
    /// (`g >= split`) resolves to `nodes[g - split]` (PoB's
    /// `legionNodes[g + 1 - timelessJewelAdditions]`).
    pub nodes: Vec<LegionPassive>,
}

/// What `add < split` vs `id >= split` means: either a notable *replacement*
/// (wipe the node, use the new node's stats) or an *addition* (stack the stat
/// lines on top of the node's own).
#[derive(Debug, Clone, Copy)]
pub enum LegionResolved<'a> {
    /// `id >= split`: replace the node with this notable.
    Replace(&'a LegionPassive),
    /// `id < split`: add this addition's stat lines on top.
    Add(&'a LegionPassive),
}

impl LegionPool {
    /// The additions/nodes split point — PoB's `data.timelessJewelAdditions`
    /// (`#legionAdditions`, 96). Global ids below it are additions, at/above are
    /// node replacements.
    #[must_use]
    pub fn split(&self) -> u32 {
        self.additions.len() as u32
    }

    /// Resolve a global id to its pool entry, mirroring PoB's `jewelData >=
    /// data.timelessJewelAdditions` branch in `PassiveSpec.lua`.
    #[must_use]
    pub fn resolve(&self, global_id: u32) -> Option<LegionResolved<'_>> {
        let split = self.split();
        if global_id >= split {
            self.nodes
                .get((global_id - split) as usize)
                .map(LegionResolved::Replace)
        } else {
            self.additions
                .get(global_id as usize)
                .map(LegionResolved::Add)
        }
    }

    /// A node from the replacement pool by **1-based** `legionNodes` index
    /// (PoB hard-codes `legionNodes[77]` = Might of the Vaal, `[78]` = Legacy
    /// of the Vaal for the Glorious Vanity multiple-choice branch).
    #[must_use]
    pub fn node_1based(&self, idx: usize) -> Option<&LegionPassive> {
        idx.checked_sub(1).and_then(|i| self.nodes.get(i))
    }
}

/// Decode a `data/legion_passives.json` payload — the additions pool, indexed
/// by `global_id`.
pub fn load_legion_passives(json: &str) -> serde_json::Result<Vec<LegionPassive>> {
    serde_json::from_str(json)
}

/// Decode a `data/legion_nodes.json` payload — the `legionNodes` replacement
/// pool (1-based in PoB; element `i` here is `legionNodes[i + 1]`).
pub fn load_legion_nodes(json: &str) -> serde_json::Result<Vec<LegionPassive>> {
    serde_json::from_str(json)
}

/// One node's slot in the timeless LUT (PoB's `NodeIndexMapping.lua`
/// `nodeIDList[nodeID] = { index, size }`). `index` is the node's 0-based
/// position in the per-jewel binary blob; `size` is the byte length of that
/// node's seed data (used to walk the packed Glorious Vanity blob).
#[derive(Debug, Clone, Copy, Serialize, Deserialize, Default)]
pub struct TimelessNodeSlot {
    pub node_id: u32,
    pub index: u32,
    pub size: u32,
}

/// PoB's `NodeIndexMapping.lua`: the node→slot table plus the notable count
/// (the LUT splits notables `index <= notable_count` from smalls) and the
/// per-jewel-type local→global id remap (`localIdToGlobalId`) the decode
/// applies to every replacement id. Drives the #227 `readLUT` port.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct TimelessNodeIndex {
    /// `nodeIDList["size"]` — total node count.
    pub node_count: u32,
    /// `nodeIDList["sizeNotable"]` — notable cutoff index.
    pub notable_count: u32,
    /// Per-node slots, in ascending `index` order.
    pub nodes: Vec<TimelessNodeSlot>,
    /// `localIdToGlobalId[jewelType][localId] = globalId`, outer indexed by
    /// `jewel_type - 1` (jewel types are 1-based in PoB), inner by `localId`.
    #[serde(default)]
    pub local_to_global: Vec<Vec<u32>>,
}

impl TimelessNodeIndex {
    /// The slot for `node_id`, if the node participates in timeless replacement.
    #[must_use]
    pub fn slot(&self, node_id: u32) -> Option<&TimelessNodeSlot> {
        self.nodes.iter().find(|s| s.node_id == node_id)
    }

    /// Map a per-jewel `local_id` to its global `LegionPassive` id, falling
    /// back to the local id when no remap entry exists (mirrors PoB's
    /// `convertLocalIdToGlobalId`). `jewel_type` is 1-based.
    #[must_use]
    pub fn to_global_id(&self, jewel_type: u32, local_id: u32) -> u32 {
        self.local_to_global
            .get(jewel_type.wrapping_sub(1) as usize)
            .and_then(|m| m.get(local_id as usize).copied())
            .unwrap_or(local_id)
    }
}

/// Decode a `data/timeless_node_index.json` payload.
pub fn load_timeless_node_index(json: &str) -> serde_json::Result<TimelessNodeIndex> {
    serde_json::from_str(json)
}

/// Map a timeless jewel's base name to its 1-based PoB jewel type
/// (`data.timelessJewelTypes` / the `conqueror.type` → jewelType branch in
/// `PassiveSpec.lua`). `None` for non-timeless bases.
#[must_use]
pub fn jewel_type_for_base(base_name: &str) -> Option<u32> {
    Some(match base_name {
        "Glorious Vanity" => 1,
        "Lethal Pride" => 2,
        "Brutal Restraint" => 3,
        "Militant Faith" => 4,
        "Elegant Hubris" => 5,
        "Heroic Tragedy" => 6,
        _ => return None,
    })
}

/// Inclusive `(seedMin, seedMax)` for a 1-based jewel type, from PoB's
/// `data.timelessJewelSeedMin/Max` (`Modules/Data.lua`). Elegant Hubris (5)
/// is pre-divided by 20 — the *stored* item seed is divided by 20 before
/// lookup (handled in [`flat_notable_replacement`]). `None` for unknown types.
#[must_use]
pub fn seed_range(jewel_type: u32) -> Option<(u32, u32)> {
    Some(match jewel_type {
        1 => (100, 8000),    // Glorious Vanity
        2 => (10000, 18000), // Lethal Pride
        3 => (500, 8000),    // Brutal Restraint
        4 => (2000, 10000),  // Militant Faith
        5 => (100, 8000),    // Elegant Hubris (2000/20 .. 160000/20)
        6 => (100, 8000),    // Heroic Tragedy
        _ => return None,
    })
}

/// Inclusive `(min, max, step)` for the seed value **as stored in the item's
/// mod text** — the value the user edits in the socket picker, not the internal
/// LUT seed. For every jewel except Elegant Hubris this equals
/// [`seed_range`] with `step == 1`. Elegant Hubris stores the seed ×20 (the
/// in-game "coins" count), so its bounds are the internal range scaled by 20
/// and valid seeds advance in steps of 20 — the engine divides back down at
/// lookup time. `None` for unknown jewel types.
#[must_use]
pub fn stored_seed_bounds(jewel_type: u32) -> Option<(u32, u32, u32)> {
    let (min, max) = seed_range(jewel_type)?;
    Some(if jewel_type == 5 {
        (min * 20, max * 20, 20)
    } else {
        (min, max, 1)
    })
}

/// Clamp `seed` into the stored bounds for `jewel_type` and snap it onto the
/// step grid (only Elegant Hubris has a step > 1). Returns `seed` unchanged for
/// unknown jewel types. Used by the socket picker so manual entry / rotation
/// always yields a seed the LUT can resolve.
#[must_use]
pub fn snap_stored_seed(jewel_type: u32, seed: u32) -> u32 {
    let Some((min, max, step)) = stored_seed_bounds(jewel_type) else {
        return seed;
    };
    let clamped = seed.clamp(min, max);
    if step <= 1 {
        return clamped;
    }
    // Round to the nearest multiple of `step` measured from `min`, then re-clamp
    // (rounding up near `max` could overshoot).
    let offset = clamped - min;
    let snapped = min + ((offset + step / 2) / step) * step;
    snapped.clamp(min, max)
}

impl TimelessNodeIndex {
    /// Decode the replacement `LegionPassive` global id for a **notable** node
    /// under a non-Glorious-Vanity jewel, from the inflated flat LUT `data`.
    ///
    /// Mirrors PoB's `readLUT` non-GV notable branch: `seedOffset = seed -
    /// seedMin` (Elegant Hubris divides the seed by 20 first), one byte per
    /// `(node index, seed)` at `index * seedSize + seedOffset`, then
    /// local→global remap. Returns `None` for Glorious Vanity (type 1, which
    /// uses the variable-length path), non-notable nodes, unknown nodes,
    /// out-of-range seeds, or a short `data` buffer.
    #[must_use]
    pub fn flat_notable_replacement(
        &self,
        data: &[u8],
        jewel_type: u32,
        node_id: u32,
        seed: u32,
    ) -> Option<u32> {
        if jewel_type == 1 {
            return None; // Glorious Vanity → variable-length path
        }
        let slot = self.slot(node_id)?;
        if slot.index > self.notable_count {
            return None; // smalls aren't replaced on the flat notable path
        }
        let (min, max) = seed_range(jewel_type)?;
        let seed = if jewel_type == 5 { seed / 20 } else { seed };
        if seed < min || seed > max {
            return None;
        }
        let seed_size = (max - min + 1) as usize;
        let offset = slot.index as usize * seed_size + (seed - min) as usize;
        let local_id = u32::from(*data.get(offset)?);
        Some(self.to_global_id(jewel_type, local_id))
    }

    /// Decode the full **Glorious Vanity** (jewel type 1) LUT entry for
    /// `(node_id, seed)` from the inflated GV LUT `lut`, mirroring PoB's
    /// `readLUT` type-1 branch exactly.
    ///
    /// GV's blob is variable-length: a `node_count * seedSize` *sizes* block
    /// (one byte = the byte-length of each `(node, seed)` entry) followed by
    /// each node's packed seed data (node `i`'s block is `slot.size` bytes).
    /// To reach `(node, seed)` we skip the sizes block + every earlier node's
    /// data, then walk this node's per-seed lengths up to `seed`.
    ///
    /// The returned vector is the raw entry (length = `dataLength`) with the
    /// **id positions** remapped local→global, exactly as `readLUT` leaves it:
    /// * length 2/3 → `[global_id, roll, (roll)]` (single replacement + rolls).
    /// * length 6/8 → `[g1, …, roll1, …]` (first half = converted ids, second
    ///   half = rolls; the multiple-choice Might/Legacy branch).
    ///
    /// The consumer ([`TimelessNodeIndex::resolve_node_replacement`]) interprets
    /// these. Returns an empty vec on any failure (OOR seed, unknown node, short
    /// buffer, unhandled length).
    #[must_use]
    pub fn gv_lut_entry(&self, lut: &[u8], node_id: u32, seed: u32) -> Vec<u32> {
        let Some((min, max)) = seed_range(1) else {
            return Vec::new();
        };
        if seed < min || seed > max {
            return Vec::new();
        }
        let Some(slot) = self.slot(node_id) else {
            return Vec::new();
        };
        let seed_size = (max - min + 1) as usize;
        let seed_offset = (seed - min) as usize;
        let size_block_len = self.node_count as usize * seed_size;

        // Start of this node's packed data = sizes block + sum of earlier
        // nodes' data lengths (their `slot.size`).
        let mut node_data_start = size_block_len;
        for s in &self.nodes {
            if s.index < slot.index {
                node_data_start += s.size as usize;
            }
        }

        // Walk per-seed lengths within this node up to `seed_offset`.
        let sizes_base = slot.index as usize * seed_size;
        let mut entry_off = 0usize;
        for k in 0..seed_offset {
            entry_off += *lut.get(sizes_base + k).unwrap_or(&0) as usize;
        }
        let Some(&data_length) = lut.get(sizes_base + seed_offset) else {
            return Vec::new();
        };
        let data_length = data_length as usize;

        let start = node_data_start + entry_off;
        let Some(entry) = lut.get(start..start + data_length) else {
            return Vec::new();
        };
        let mut result: Vec<u32> = entry.iter().map(|b| u32::from(*b)).collect();

        // Remap id positions local→global (PoB readLUT). Rolls stay raw.
        match data_length {
            2 | 3 => {
                if let Some(first) = result.first_mut() {
                    *first = self.to_global_id(1, *first);
                }
            }
            6 | 8 => {
                for v in result.iter_mut().take(data_length / 2) {
                    *v = self.to_global_id(1, *v);
                }
            }
            _ => return Vec::new(),
        }
        result
    }
}

/// The outcome of decoding one in-radius node against a socketed timeless
/// jewel — the stat lines to apply plus whether they replace the node's own
/// stats (PoB `ReplaceNode`) or stack on top (`NodeAdditionOrReplacementFromString`).
#[derive(Debug, Clone, PartialEq)]
pub struct TimelessReplacement {
    /// Final stat lines, roll-substituted, ready for `mod_parser`.
    pub lines: Vec<String>,
    /// `true` → wipe the node's original stats first; `false` → add on top.
    pub wipe: bool,
    /// Replacement-node display name, if any (for breakdown labels).
    pub name: Option<String>,
}

/// PoB's `replaceHelperFunc` — substitute a rolled `value` into one `sd[]`
/// `line` using its [`LegionStat`] range metadata. `"g"`-format stats apply the
/// `per_minute` / `permyriad` / `_ms` scaling before substitution.
fn apply_roll(line: &str, st: &LegionStat, value: f64) -> String {
    let mut value = value;
    if st.fmt == "g" {
        if st.key.contains("per_minute") {
            value = (value / 60.0 * 10.0).round() / 10.0;
        } else if st.key.contains("permyriad") {
            value /= 100.0;
        } else if st.key.contains("_ms") {
            value /= 1000.0;
        }
    }
    if (st.min - st.max).abs() > f64::EPSILON {
        let pat = format!("({}-{})", fmt_num(st.min), fmt_num(st.max));
        line.replace(&pat, &fmt_num(value))
    } else if (st.min - value).abs() > f64::EPSILON {
        // Might / Legacy of the Vaal combine: the range is a point but the
        // rolled value differs — substitute the literal min.
        line.replace(&fmt_num(st.min), &fmt_num(value))
    } else {
        line.to_string()
    }
}

/// Render a roll number the way PoB's Lua `tostring` does: integers without a
/// decimal point, fractions trimmed of trailing zeros.
fn fmt_num(v: f64) -> String {
    if v.fract().abs() < 1e-9 {
        return (v.round() as i64).to_string();
    }
    let mut s = format!("{v:.4}");
    while s.ends_with('0') {
        s.pop();
    }
    if s.ends_with('.') {
        s.pop();
    }
    s
}

/// Fixed small-node (`Normal`) effect for a non-Glorious-Vanity timeless jewel,
/// mirroring PoB's `PassiveSpec.lua` `node.type == "Normal"` conqueror-nation
/// branches. Attribute / tattoo nodes (`is_attr`) take the smaller addition (or
/// a replacement for templar); every other small node takes the larger one.
///
/// - **karui** (Lethal Pride, type 2): `+2/4 to Strength` (addition).
/// - **maraketh** (Brutal Restraint, type 3): `+2/4 to Dexterity` (addition).
/// - **templar** (Militant Faith, type 4): attribute nodes are *replaced* by the
///   Devotion node (`legionNodes[91]`, `+10 to Devotion`); others gain
///   `+5 to Devotion` (addition).
/// - **eternal** (Elegant Hubris, type 5): every small node is *replaced* by the
///   blank node (`legionNodes[110]`, "Price of Glory", no stats).
/// - **kalguur** (Heroic Tragedy, type 6): `1/2% increased Ward` (addition).
fn small_node_addition(
    pool: &LegionPool,
    jewel_type: u32,
    is_attr: bool,
) -> Option<TimelessReplacement> {
    let addition = |line: String| TimelessReplacement {
        lines: vec![line],
        wipe: false,
        name: None,
    };
    let replace_with = |node: &LegionPassive| TimelessReplacement {
        lines: node.stats.clone(),
        wipe: true,
        name: Some(node.name.clone()),
    };
    Some(match jewel_type {
        2 => addition(format!("+{} to Strength", if is_attr { 2 } else { 4 })),
        3 => addition(format!("+{} to Dexterity", if is_attr { 2 } else { 4 })),
        4 => {
            if is_attr {
                // legionNodes[91] — templar_devotion_node ("+10 to Devotion").
                replace_with(pool.node_1based(91)?)
            } else {
                addition("+5 to Devotion".to_string())
            }
        }
        // legionNodes[110] — eternal_small_blank ("Price of Glory", no stats).
        5 => replace_with(pool.node_1based(110)?),
        6 => addition(format!("{}% increased Ward", if is_attr { 1 } else { 2 })),
        _ => return None,
    })
}

impl TimelessNodeIndex {
    /// Decode and resolve a single in-radius node against a socketed timeless
    /// jewel, mirroring PoB's `PassiveSpec.lua` notable / vaal-small branches.
    /// `lut` is the inflated per-jewel LUT for `jewel_type`; `is_notable`
    /// distinguishes notables from small (`Normal`) nodes.
    ///
    /// `is_attribute_node` marks a small (`Normal`) node whose display name is a
    /// pure attribute (`Strength` / `Dexterity` / `Intelligence`) or a tattoo —
    /// PoB's `isValueInArray(attributes, node.dn) or node.isTattoo` test, which
    /// halves the karui/maraketh/kalguur stat addition and routes templar
    /// attribute nodes to the Devotion-node replacement. Ignored for notables
    /// and for Glorious Vanity (LUT-driven).
    ///
    /// Returns `None` when the node isn't replaced (out-of-radius handling is
    /// the caller's job; here it's: OOR seed, unknown node, an unknown jewel
    /// type, or a malformed LUT entry).
    #[must_use]
    pub fn resolve_node_replacement(
        &self,
        pool: &LegionPool,
        lut: &[u8],
        jewel_type: u32,
        node_id: u32,
        seed: u32,
        is_notable: bool,
        is_attribute_node: bool,
    ) -> Option<TimelessReplacement> {
        if jewel_type == 1 {
            // Glorious Vanity — variable-length entry; notables and vaal smalls
            // both route through the LUT.
            let entry = self.gv_lut_entry(lut, node_id, seed);
            match entry.len() {
                2 | 3 => {
                    // Single replacement notable / vaal small.
                    let LegionResolved::Replace(node) = pool.resolve(entry[0])? else {
                        return None;
                    };
                    Some(TimelessReplacement {
                        lines: roll_substitute(node, &entry),
                        wipe: true,
                        name: Some(node.name.clone()),
                    })
                }
                6 | 8 => gv_multiple_choice(pool, &entry),
                _ => None,
            }
        } else {
            // Lethal Pride / Brutal Restraint / Militant Faith / Elegant
            // Hubris / Heroic Tragedy. Notables route through the flat LUT;
            // small (`Normal`) nodes get a fixed text addition / replacement
            // per conqueror nation (PoB `PassiveSpec.lua` `node.type ==
            // "Normal"` branch), with no LUT lookup.
            if !is_notable {
                return small_node_addition(pool, jewel_type, is_attribute_node);
            }
            let global = self.flat_notable_replacement(lut, jewel_type, node_id, seed)?;
            match pool.resolve(global)? {
                LegionResolved::Replace(node) => Some(TimelessReplacement {
                    lines: node.stats.clone(),
                    wipe: true,
                    name: Some(node.name.clone()),
                }),
                LegionResolved::Add(add) => Some(TimelessReplacement {
                    lines: add.stats.clone(),
                    wipe: false,
                    name: None,
                }),
            }
        }
    }
}

/// Glorious Vanity `headerSize == 6 || 8` branch: a bias over the first-half
/// ids picks Might of the Vaal (`legionNodes[77]`) or Legacy of the Vaal
/// (`legionNodes[78]`) as the base node, then each first-half id contributes
/// a rolled addition (second half = the roll values, summed per id).
fn gv_multiple_choice(pool: &LegionPool, entry: &[u32]) -> Option<TimelessReplacement> {
    let half = entry.len() / 2;
    let bias: i32 = entry[..half]
        .iter()
        .map(|&v| if v <= 21 { 1 } else { -1 })
        .sum();
    let base = pool.node_1based(if bias >= 0 { 77 } else { 78 })?;

    // Accumulate rolls per addition id (PoB sums duplicates).
    let mut acc: Vec<(u32, f64)> = Vec::new();
    for i in 0..half {
        let id = entry[i];
        let roll = f64::from(entry[i + half]);
        match acc.iter_mut().find(|(v, _)| *v == id) {
            Some(slot) => slot.1 += roll,
            None => acc.push((id, roll)),
        }
    }

    let mut lines = base.stats.clone();
    for (add_id, roll) in acc {
        let Some(add) = pool.additions.get(add_id as usize) else {
            continue;
        };
        for (i, sd) in add.stats.iter().enumerate() {
            let st = add
                .sorted_stats
                .get(i)
                .and_then(|k| add.roll(k))
                .or_else(|| add.rolls.first());
            lines.push(match st {
                Some(st) => apply_roll(sd, st, roll),
                None => sd.clone(),
            });
        }
    }
    Some(TimelessReplacement {
        lines,
        wipe: true,
        name: Some(base.name.clone()),
    })
}

/// Roll-substitute every `sd[]` line of a single-replacement node using the
/// rolls packed after the id in a Glorious Vanity LUT `entry`
/// (`entry[stat.index]`, 0-based — PoB's `jewelDataTbl[statMod.index + 1]`).
fn roll_substitute(node: &LegionPassive, entry: &[u32]) -> Vec<String> {
    node.stats
        .iter()
        .enumerate()
        .map(|(i, sd)| {
            if let Some(st) = node.sorted_stats.get(i).and_then(|k| node.roll(k)) {
                if let Some(&val) = entry.get(st.index as usize) {
                    return apply_roll(sd, st, f64::from(val));
                }
            }
            sd.clone()
        })
        .collect()
}

impl TimelessJewelData {
    /// Find the conqueror entry on `jewel_base` whose textual name matches
    /// `conqueror_name` (case-insensitive). Returns `None` if either the jewel
    /// base or the conqueror name is unknown.
    pub fn find_conqueror(
        &self,
        jewel_base: &str,
        conqueror_name: &str,
    ) -> Option<(&TimelessJewelConfig, &TimelessConqueror)> {
        let cfg = self.jewels.get(jewel_base)?;
        let want = conqueror_name.trim().to_ascii_lowercase();
        let conq = cfg
            .conquerors
            .iter()
            .find(|c| c.name.eq_ignore_ascii_case(&want))?;
        Some((cfg, conq))
    }

    /// Look up the replacement keystone's stat lines for `(jewel_base,
    /// conqueror_name)`. Returns `None` for unknown entries.
    pub fn replacement_for(
        &self,
        jewel_base: &str,
        conqueror_name: &str,
    ) -> Option<&ConquerorKeystone> {
        let (_, conq) = self.find_conqueror(jewel_base, conqueror_name)?;
        self.keystones.get(&conq.keystone_id)
    }

    /// True if `jewel_base` is a known Timeless jewel base. Used by the
    /// engine's identification step to short-circuit on names like
    /// `"Crimson Jewel"`.
    pub fn is_timeless_base(&self, jewel_base: &str) -> bool {
        self.jewels.contains_key(jewel_base)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn seed_range_known_and_unknown() {
        assert_eq!(seed_range(2), Some((10000, 18000)));
        assert_eq!(seed_range(1), Some((100, 8000)));
        assert_eq!(seed_range(99), None);
    }

    #[test]
    fn stored_seed_bounds_scales_elegant_hubris() {
        // Non-EH: stored == internal, step 1.
        assert_eq!(stored_seed_bounds(1), Some((100, 8000, 1)));
        assert_eq!(stored_seed_bounds(2), Some((10000, 18000, 1)));
        // Elegant Hubris (5): ×20 bounds, step 20.
        assert_eq!(stored_seed_bounds(5), Some((2000, 160000, 20)));
        assert_eq!(stored_seed_bounds(99), None);
    }

    #[test]
    fn snap_stored_seed_clamps_and_steps() {
        // Non-EH: plain clamp.
        assert_eq!(snap_stored_seed(1, 50), 100); // below min
        assert_eq!(snap_stored_seed(1, 9000), 8000); // above max
        assert_eq!(snap_stored_seed(1, 5000), 5000); // in range, untouched
                                                     // Elegant Hubris: snap to the ×20 grid and clamp.
        assert_eq!(snap_stored_seed(5, 2009), 2000); // rounds down to nearest 20
        assert_eq!(snap_stored_seed(5, 2010), 2020); // rounds up at the half-step
        assert_eq!(snap_stored_seed(5, 200000), 160000); // clamps to max
        assert_eq!(snap_stored_seed(5, 1000), 2000); // clamps to min
                                                     // Unknown type: identity.
        assert_eq!(snap_stored_seed(99, 1234), 1234);
    }

    #[test]
    fn flat_notable_decode_reads_byte_and_remaps() {
        let mut remap_t2 = vec![0u32; 6];
        remap_t2[5] = 42;
        let ni = TimelessNodeIndex {
            node_count: 2,
            notable_count: 0, // only index 0 is a notable (index <= 0)
            nodes: vec![
                TimelessNodeSlot {
                    node_id: 529,
                    index: 0,
                    size: 0,
                },
                TimelessNodeSlot {
                    node_id: 600,
                    index: 1,
                    size: 0,
                },
            ],
            local_to_global: vec![Vec::new(), remap_t2], // type 1 empty, type 2 remaps 5→42
        };
        // Lethal Pride (type 2), seedMin 10000 → seed 10000 = offset 0, index 0
        // → data[0] = 5 → remapped to global 42.
        let data = vec![5u8];
        assert_eq!(ni.flat_notable_replacement(&data, 2, 529, 10000), Some(42));
        // Glorious Vanity routes through the variable-length path → None here.
        assert_eq!(ni.flat_notable_replacement(&data, 1, 529, 10000), None);
        // A small node (index 1 > notable_count 0) is not on the flat path.
        assert_eq!(ni.flat_notable_replacement(&data, 2, 600, 10000), None);
        // Out-of-range seed, unknown node, and short buffer all → None.
        assert_eq!(ni.flat_notable_replacement(&data, 2, 529, 99999), None);
        assert_eq!(ni.flat_notable_replacement(&data, 2, 999, 10000), None);
        assert_eq!(ni.flat_notable_replacement(&[], 2, 529, 10000), None);
    }

    #[test]
    fn gv_variable_length_decode_single_and_multiple_choice() {
        // GV seedSize = 8000 - 100 + 1 = 7901. One node (index 0), so the
        // sizes block is 7901 bytes; node 0's packed data follows.
        let seed_size = 7901usize;
        let mut remap = vec![0u32; 60];
        remap[5] = 42;
        remap[6] = 43;
        remap[7] = 44;
        remap[8] = 45;
        // Two seeds carry entries: seed offset 0 → a 2-byte single (local 5);
        // seed offset 1 → a 6-byte multiple-choice (locals 6,7,8 in first half).
        let mut lut = vec![0u8; seed_size];
        lut[0] = 2; // length of seed-offset-0 entry
        lut[1] = 6; // length of seed-offset-1 entry
        let node_size = 2 + 6; // total packed bytes for this node
        lut.extend_from_slice(&[5, 0]); // seed 0 entry: [local 5, extra]
        lut.extend_from_slice(&[6, 7, 8, 0, 0, 0]); // seed 1 entry: first half = 6,7,8

        let ni = TimelessNodeIndex {
            node_count: 1,
            notable_count: 1,
            nodes: vec![TimelessNodeSlot {
                node_id: 6,
                index: 0,
                size: node_size as u32,
            }],
            local_to_global: vec![remap], // type 1 remap
        };

        // seed 100 → offset 0 → 2-byte single → [remap(5)=42, roll 0].
        assert_eq!(ni.gv_lut_entry(&lut, 6, 100), vec![42, 0]);
        // seed 101 → offset 1 → 6-byte multiple-choice → first 3 remapped, rolls raw.
        assert_eq!(ni.gv_lut_entry(&lut, 6, 101), vec![43, 44, 45, 0, 0, 0]);
        // Out-of-range seed and unknown node → empty.
        assert!(ni.gv_lut_entry(&lut, 6, 99999).is_empty());
        assert!(ni.gv_lut_entry(&lut, 999, 100).is_empty());
    }

    #[test]
    fn elegant_hubris_divides_stored_seed_by_20() {
        let ni = TimelessNodeIndex {
            node_count: 1,
            notable_count: 0,
            nodes: vec![TimelessNodeSlot {
                node_id: 7,
                index: 0,
                size: 0,
            }],
            local_to_global: vec![Vec::new(); 5], // type 5 → empty → identity remap
        };
        // Elegant Hubris (type 5) seedMin 100; stored seed 2000 / 20 = 100 →
        // offset 0 → data[0] = 9, identity remap → 9.
        assert_eq!(ni.flat_notable_replacement(&[9u8], 5, 7, 2000), Some(9));
    }

    /// The committed data files load through the public loaders and compose a
    /// coherent `LegionPool` (additions split = 96, Might/Legacy present, node
    /// index dimensions match PoB). Self-skips when `data/` is absent.
    #[test]
    fn real_data_files_load_into_a_coherent_pool() {
        let dir = std::path::Path::new("../../data");
        let (Ok(add_json), Ok(node_json), Ok(idx_json)) = (
            std::fs::read_to_string(dir.join("legion_passives.json")),
            std::fs::read_to_string(dir.join("legion_nodes.json")),
            std::fs::read_to_string(dir.join("timeless_node_index.json")),
        ) else {
            return;
        };
        let additions = load_legion_passives(&add_json).expect("additions");
        let nodes = load_legion_nodes(&node_json).expect("nodes");
        let index = load_timeless_node_index(&idx_json).expect("index");
        let pool = LegionPool { additions, nodes };
        // `data.timelessJewelAdditions`: 96 in 3.25-era data, 337 since the
        // Foulborn / Vestigial additions.
        assert!(
            pool.split() == 96 || pool.split() == 337,
            "additions = data.timelessJewelAdditions, got {}",
            pool.split()
        );
        assert!(pool
            .node_1based(77)
            .is_some_and(|n| n.name.contains("Might")));
        assert!(pool
            .node_1based(78)
            .is_some_and(|n| n.name.contains("Legacy")));
        // Fire Damage (global id 0) carries its roll range.
        assert_eq!(pool.additions[0].rolls[0].min, 7.0);
        // 1931 / 452 in 3.25-era data; grows as the tree gains nodes.
        assert!(index.node_count >= 1931);
        assert!(index.notable_count >= 452);
        assert_eq!(index.nodes.len() as u32, index.node_count);
        // Six classic timeless jewels; current PoB adds five Abyss types (7-11).
        assert!(index.local_to_global.len() >= 6, "jewel types");
    }

    #[test]
    fn jewel_type_for_base_matches_pob_ordering() {
        assert_eq!(jewel_type_for_base("Glorious Vanity"), Some(1));
        assert_eq!(jewel_type_for_base("Lethal Pride"), Some(2));
        assert_eq!(jewel_type_for_base("Brutal Restraint"), Some(3));
        assert_eq!(jewel_type_for_base("Militant Faith"), Some(4));
        assert_eq!(jewel_type_for_base("Elegant Hubris"), Some(5));
        assert_eq!(jewel_type_for_base("Heroic Tragedy"), Some(6));
        assert_eq!(jewel_type_for_base("Crimson Jewel"), None);
    }

    #[test]
    fn fmt_num_renders_like_pob_tostring() {
        assert_eq!(fmt_num(7.0), "7");
        assert_eq!(fmt_num(12.0), "12");
        assert_eq!(fmt_num(1.5), "1.5");
        assert_eq!(fmt_num(0.05), "0.05");
    }

    #[test]
    fn apply_roll_substitutes_range_and_point() {
        // "d" range: "(7-12)% increased Fire Damage" with roll 9 → "9% …".
        let st = LegionStat {
            key: "fire_damage_+%".into(),
            index: 1,
            min: 7.0,
            max: 12.0,
            fmt: "d".into(),
        };
        assert_eq!(
            apply_roll("(7-12)% increased Fire Damage", &st, 9.0),
            "9% increased Fire Damage"
        );
        // Point range (min == max) but a differing value (Might/Legacy combine).
        let pt = LegionStat {
            key: "x".into(),
            index: 1,
            min: 20.0,
            max: 20.0,
            fmt: "d".into(),
        };
        assert_eq!(apply_roll("+20 to Strength", &pt, 40.0), "+40 to Strength");
        // "g" per_minute scaling: value/60 rounded to 1 dp.
        let g = LegionStat {
            key: "life_regen_per_minute".into(),
            index: 1,
            min: 0.0,
            max: 100.0,
            fmt: "g".into(),
        };
        assert_eq!(
            apply_roll("Regenerate (0-100) Life per second", &g, 90.0),
            "Regenerate 1.5 Life per second"
        );
    }

    /// Build a `LegionPool`: 2 additions (ids 0,1) and a `nodes` pool whose
    /// 1-based index 77/78 are Might/Legacy of the Vaal. With `split = 2`,
    /// global id `g >= 2` is a node replacement (`nodes[g - 2]`).
    fn mk_pool() -> LegionPool {
        let mut nodes = vec![
            LegionPassive {
                name: "filler".into(),
                ..Default::default()
            };
            78
        ];
        nodes[76] = LegionPassive {
            name: "Might of the Vaal".into(),
            id: "might".into(),
            stats: vec!["Might base line".into()],
            ..Default::default()
        };
        nodes[77] = LegionPassive {
            name: "Legacy of the Vaal".into(),
            id: "legacy".into(),
            stats: vec!["Legacy base line".into()],
            ..Default::default()
        };
        LegionPool {
            additions: vec![
                LegionPassive {
                    name: "Fire Damage".into(),
                    id: "vaal_small_fire_damage".into(),
                    stats: vec!["(7-12)% increased Fire Damage".into()],
                    sorted_stats: vec!["fire_damage_+%".into()],
                    rolls: vec![LegionStat {
                        key: "fire_damage_+%".into(),
                        index: 1,
                        min: 7.0,
                        max: 12.0,
                        fmt: "d".into(),
                    }],
                },
                LegionPassive {
                    name: "Cold Damage".into(),
                    id: "vaal_small_cold_damage".into(),
                    stats: vec!["(7-12)% increased Cold Damage".into()],
                    sorted_stats: vec!["cold_damage_+%".into()],
                    rolls: vec![LegionStat {
                        key: "cold_damage_+%".into(),
                        index: 1,
                        min: 7.0,
                        max: 12.0,
                        fmt: "d".into(),
                    }],
                },
            ],
            nodes,
        }
    }

    #[test]
    fn pool_resolve_splits_additions_and_nodes() {
        let pool = mk_pool();
        assert_eq!(pool.split(), 2);
        // g < 2 → addition.
        assert!(matches!(pool.resolve(0), Some(LegionResolved::Add(_))));
        // g >= 2 → node[g-2]. node_1based(77) == nodes[76] == Might.
        assert_eq!(
            pool.node_1based(77).map(|n| n.name.as_str()),
            Some("Might of the Vaal")
        );
        // global id for nodes[76] = split + 76 = 78.
        assert!(matches!(pool.resolve(78), Some(LegionResolved::Replace(_))));
    }

    /// GV single replacement (headerSize 2): node replaced with a notable whose
    /// `(min-max)` stat is roll-substituted from `entry[index]`.
    #[test]
    fn resolve_gv_single_replacement_rolls_stat() {
        let pool = mk_pool();
        // One GV node, index 0. seedSize 7901; sizes block then packed data.
        let seed_size = 7901usize;
        let mut lut = vec![0u8; seed_size];
        lut[0] = 2; // entry length for seed offset 0
                    // entry: [local id, roll]. local 0 maps (identity) to global 2 via remap.
                    // We want global id 2 (= nodes[0]... but nodes[0] is filler). Instead
                    // target an addition? No — single replacement requires a node (>= split).
                    // Use remap local 0 → global 78 (Might) and roll 9.
        lut.extend_from_slice(&[0, 9]);
        let mut remap = vec![0u32; 1];
        remap[0] = 78; // local 0 → global 78 → nodes[76] (Might)
        let ni = TimelessNodeIndex {
            node_count: 1,
            notable_count: 1,
            nodes: vec![TimelessNodeSlot {
                node_id: 6,
                index: 0,
                size: 2,
            }],
            local_to_global: vec![remap],
        };
        let rep = ni
            .resolve_node_replacement(&pool, &lut, 1, 6, 100, true, false)
            .expect("resolved");
        assert!(rep.wipe);
        assert_eq!(rep.name.as_deref(), Some("Might of the Vaal"));
        // Might's base sd line has no range → passes through unchanged.
        assert_eq!(rep.lines, vec!["Might base line".to_string()]);
    }

    /// GV multiple-choice (headerSize 6): bias picks Might (all first-half ids
    /// <= 21) and each addition id contributes a rolled line.
    #[test]
    fn resolve_gv_multiple_choice_might_with_additions() {
        let pool = mk_pool();
        let seed_size = 7901usize;
        let mut lut = vec![0u8; seed_size];
        lut[0] = 6;
        // first half = addition global ids [0,1,0] (all <= 21 → bias +3 → Might),
        // second half = rolls [9, 11, 3]. id 0 accumulates 9+3=12, id 1 → 11.
        lut.extend_from_slice(&[0, 1, 0, 9, 11, 3]);
        let remap = vec![0u32, 1, 2]; // identity for locals 0,1,2
        let ni = TimelessNodeIndex {
            node_count: 1,
            notable_count: 1,
            nodes: vec![TimelessNodeSlot {
                node_id: 6,
                index: 0,
                size: 6,
            }],
            local_to_global: vec![remap],
        };
        let rep = ni
            .resolve_node_replacement(&pool, &lut, 1, 6, 100, true, false)
            .expect("resolved");
        assert_eq!(rep.name.as_deref(), Some("Might of the Vaal"));
        // base line + addition 0 (Fire, roll 12) + addition 1 (Cold, roll 11).
        assert!(rep.lines.contains(&"Might base line".to_string()));
        assert!(rep.lines.contains(&"12% increased Fire Damage".to_string()));
        assert!(rep.lines.contains(&"11% increased Cold Damage".to_string()));
    }

    /// Non-GV notable: a flat-LUT global id >= split → verbatim replacement (no
    /// roll substitution), < split → additive.
    #[test]
    fn resolve_flat_replace_and_add() {
        let pool = mk_pool();
        let ni = TimelessNodeIndex {
            node_count: 2,
            notable_count: 1, // index 0 and 1 are both notables (index <= 1)
            nodes: vec![
                TimelessNodeSlot {
                    node_id: 10,
                    index: 0,
                    size: 0,
                },
                TimelessNodeSlot {
                    node_id: 11,
                    index: 1,
                    size: 0,
                },
            ],
            // Lethal Pride (type 2): local 0 → global 78 (replace), local 1 → global 1 (add).
            local_to_global: vec![Vec::new(), vec![78, 1]],
        };
        let seed_size = (18000 - 10000 + 1) as usize;
        // data[index*seedSize + offset]; offset 0. node 10 (index 0) → byte 0,
        // node 11 (index 1) → byte seed_size.
        let mut data = vec![0u8; seed_size * 2];
        data[0] = 0; // node 10 → local 0 → global 78 → Replace
        data[seed_size] = 1; // node 11 → local 1 → global 1 → Add (addition[1] Cold)
        let rep_replace = ni
            .resolve_node_replacement(&pool, &data, 2, 10, 10000, true, false)
            .expect("replace");
        assert!(rep_replace.wipe);
        assert_eq!(rep_replace.lines, vec!["Might base line".to_string()]);
        let rep_add = ni
            .resolve_node_replacement(&pool, &data, 2, 11, 10000, true, false)
            .expect("add");
        assert!(!rep_add.wipe);
        assert_eq!(rep_add.name, None);
        // Verbatim (non-GV never rolls): the range string is preserved.
        assert_eq!(
            rep_add.lines,
            vec!["(7-12)% increased Cold Damage".to_string()]
        );
    }

    /// Build a pool large enough to expose the templar Devotion node
    /// (`legionNodes[91]`) and the eternal blank node (`legionNodes[110]`).
    fn mk_small_pool() -> LegionPool {
        let mut nodes = vec![
            LegionPassive {
                name: "filler".into(),
                ..Default::default()
            };
            110
        ];
        nodes[90] = LegionPassive {
            name: "Devotion".into(),
            id: "templar_devotion_node".into(),
            stats: vec!["+10 to Devotion".into()],
            ..Default::default()
        };
        nodes[109] = LegionPassive {
            name: "Price of Glory".into(),
            id: "eternal_small_blank".into(),
            stats: vec![],
            ..Default::default()
        };
        LegionPool {
            additions: Vec::new(),
            nodes,
        }
    }

    /// Non-vaal small (`Normal`) nodes get a fixed text addition / replacement
    /// per conqueror nation — PoB `PassiveSpec.lua` `node.type == "Normal"`.
    /// `node_id` / `seed` / `lut` are unused on this path (no LUT lookup), so we
    /// pass dummies and toggle `is_notable=false`, exercising both the
    /// attribute (smaller) and non-attribute (larger) magnitudes.
    #[test]
    fn small_node_additions_per_nation() {
        let pool = mk_small_pool();
        let ni = TimelessNodeIndex::default();
        let resolve = |jewel_type: u32, is_attr: bool| {
            ni.resolve_node_replacement(&pool, &[], jewel_type, 0, 0, false, is_attr)
        };

        // Karui (Lethal Pride, 2) → +Strength, 4 normally / 2 on attribute nodes.
        let r = resolve(2, false).unwrap();
        assert!(!r.wipe);
        assert_eq!(r.lines, vec!["+4 to Strength".to_string()]);
        assert_eq!(resolve(2, true).unwrap().lines, vec!["+2 to Strength"]);

        // Maraketh (Brutal Restraint, 3) → +Dexterity.
        assert_eq!(resolve(3, false).unwrap().lines, vec!["+4 to Dexterity"]);
        assert_eq!(resolve(3, true).unwrap().lines, vec!["+2 to Dexterity"]);

        // Templar (Militant Faith, 4): non-attr adds +5 Devotion; attribute
        // nodes are *replaced* by the +10 Devotion node (wipe).
        let r = resolve(4, false).unwrap();
        assert!(!r.wipe);
        assert_eq!(r.lines, vec!["+5 to Devotion".to_string()]);
        let r = resolve(4, true).unwrap();
        assert!(r.wipe);
        assert_eq!(r.name.as_deref(), Some("Devotion"));
        assert_eq!(r.lines, vec!["+10 to Devotion".to_string()]);

        // Eternal (Elegant Hubris, 5): every small node replaced by the blank
        // node (no stats), regardless of attribute-ness.
        for is_attr in [false, true] {
            let r = resolve(5, is_attr).unwrap();
            assert!(r.wipe);
            assert_eq!(r.name.as_deref(), Some("Price of Glory"));
            assert!(r.lines.is_empty());
        }

        // Kalguur (Heroic Tragedy, 6) → % increased Ward, 2 normally / 1 on attr.
        assert_eq!(resolve(6, false).unwrap().lines, vec!["2% increased Ward"]);
        assert_eq!(resolve(6, true).unwrap().lines, vec!["1% increased Ward"]);
    }

    fn sample() -> TimelessJewelData {
        let json = r#"{
          "version": 1,
          "jewels": {
            "Glorious Vanity": {
              "conqueror_type": "vaal",
              "conquerors": [
                {"name": "Doryani",  "conqueror_id": "3",    "keystone_id": "vaal_keystone_3"},
                {"name": "Xibaqua",  "conqueror_id": "1",    "keystone_id": "vaal_keystone_1"},
                {"name": "Ahuana",   "conqueror_id": "2_v2", "keystone_id": "vaal_keystone_2_v2"}
              ]
            }
          },
          "keystones": {
            "vaal_keystone_1": {"name": "Divine Flesh",
              "stats": [
                "All Damage taken bypasses Energy Shield",
                "50% of Elemental Damage taken as Chaos Damage",
                "+5% to maximum Chaos Resistance"
              ]
            },
            "vaal_keystone_3": {"name": "Corrupted Soul",
              "stats": [
                "50% of Non-Chaos Damage taken bypasses Energy Shield",
                "Gain 15% of Maximum Life as Extra Maximum Energy Shield"
              ]
            },
            "vaal_keystone_2_v2": {"name": "Immortal Ambition",
              "stats": ["Energy Shield starts at zero"]
            }
          }
        }"#;
        load_timeless_jewels(json).expect("decode")
    }

    #[test]
    fn loads_minimal_payload() {
        let d = sample();
        assert_eq!(d.version, 1);
        assert!(d.is_timeless_base("Glorious Vanity"));
        assert!(!d.is_timeless_base("Crimson Jewel"));
        assert_eq!(d.jewels.len(), 1);
        assert_eq!(d.keystones.len(), 3);
    }

    #[test]
    fn lookup_resolves_conqueror_to_keystone() {
        let d = sample();
        let ks = d
            .replacement_for("Glorious Vanity", "Doryani")
            .expect("found");
        assert_eq!(ks.name, "Corrupted Soul");
        assert_eq!(ks.stats.len(), 2);
    }

    #[test]
    fn lookup_is_case_insensitive_on_conqueror() {
        let d = sample();
        assert!(d.replacement_for("Glorious Vanity", "doryani").is_some());
        assert!(d.replacement_for("Glorious Vanity", "DORYANI").is_some());
    }

    #[test]
    fn lookup_handles_v2_conqueror_id() {
        let d = sample();
        let ks = d
            .replacement_for("Glorious Vanity", "Ahuana")
            .expect("v2 keystone resolves");
        assert_eq!(ks.name, "Immortal Ambition");
    }

    #[test]
    fn unknown_jewel_or_conqueror_returns_none() {
        let d = sample();
        assert!(d.replacement_for("Crimson Jewel", "Doryani").is_none());
        assert!(d.replacement_for("Glorious Vanity", "NotAName").is_none());
    }

    #[test]
    fn empty_optional_fields_round_trip() {
        let d = sample();
        let json = serde_json::to_string(&d).expect("encode");
        let round: TimelessJewelData = serde_json::from_str(&json).expect("decode");
        assert_eq!(round.jewels.len(), d.jewels.len());
        assert_eq!(round.keystones.len(), d.keystones.len());
    }
}
