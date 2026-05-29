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

/// One entry from PoB's timeless replacement-passive pool
/// (`Data/TimelessJewelData/LegionPassives.lua` `additions`). The per-seed LUT
/// (#227) yields a *global id* that indexes this list (id `N` → element
/// `N - 1`); the entry supplies the notable/small's name and the stat lines
/// (`sd[]`) that flow into `mod_parser` when the jewel replaces an in-radius
/// node. Keystone replacements use the separate [`ConquerorKeystone`] path.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct LegionPassive {
    /// Display name (`"Fire Damage"`, `"Doryani, Royal Architect"`, …).
    pub name: String,
    /// Stable identifier (`"vaal_small_fire_damage"`, …).
    pub id: String,
    /// One stat line per element of the upstream `sd[]` array. Roll ranges
    /// (`"(7-12)% increased Fire Damage"`) are preserved verbatim as PoB
    /// renders them.
    #[serde(default)]
    pub stats: Vec<String>,
}

/// Decode a `data/legion_passives.json` payload — the flat additions pool,
/// indexed by `global_id - 1`.
pub fn load_legion_passives(json: &str) -> serde_json::Result<Vec<LegionPassive>> {
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
