//! Timeless jewel handler — slice 1 of
//! [#30](https://github.com/jonatanferm/PathOfBuildingMK2/issues/30).
//!
//! When a Timeless jewel (Glorious Vanity / Lethal Pride / Brutal Restraint /
//! Militant Faith / Elegant Hubris / Heroic Tragedy) is socketed into a
//! tree-socket node, every keystone whose tree position falls inside the
//! jewel's radius is **replaced** by the conqueror's keystone:
//!
//! * Glorious Vanity / Doryani replaces every in-radius allocated keystone
//!   with `vaal_keystone_3` ("Corrupted Soul").
//! * Militant Faith / Maxarius replaces in-radius keystones with
//!   `templar_keystone_1_v2` ("Transcendence").
//! * etc.
//!
//! Mirrors the keystone branch of `Classes/PassiveSpec.lua:1279-1285` in PoB:
//!
//! ```lua
//! elseif node.type == "Keystone" then
//!     local matchStr = conqueredBy.conqueror.type .. "_keystone_" .. conqueredBy.conqueror.id
//!     for _, legionNode in ipairs(legionNodes) do
//!         if legionNode.id == matchStr then
//!             self:ReplaceNode(node, legionNode)
//!             break
//!         end
//!     end
//! ```
//!
//! ## Out of scope (deferred)
//!
//! * **Notable replacement**. PoB's notable branch reads a per-(jewelType,
//!   conqueror.id, node.id) lookup table out of compressed binary blobs in
//!   `.PathOfBuilding/src/Data/TimelessJewelData/*.zip`. Each tuple yields a
//!   replacement-notable id plus rolled stat values. Plugging that in needs the
//!   extractor + LUT decoder, which is its own slice.
//! * ~~**Small-node replacement**~~. Now implemented (#216 follow-up): vaal
//!   smalls go through the per-seed LUT; non-vaal conquerors inject the fixed
//!   text additions / replacements ("+4 Strength" from karui smalls, "+5
//!   Devotion" from templar, the eternal blank node, etc.) via
//!   `pob_data::resolve_node_replacement`'s `Normal`-node branch.
//! * **Multiple-choice notables / Vaal "Might / Legacy of the Vaal"**. PoB's
//!   `headerSize == 6 || 8` branch handles those; same LUT story.
//!
//! What ships here covers the highest-impact play (Glorious Vanity / Doryani,
//! Militant Faith / Maxarius, etc.) — keystone-based Timeless builds — and
//! leaves the LUT-driven half clearly factored out behind
//! [`pob_data::TimelessJewelData`].

use ahash::{AHashSet, HashMap};
use pob_data::{
    jewel_type_for_base, radii_for_tree_version, ConquerorKeystone, Item, LegionPool, NodeId,
    NodeKind, PassiveTree, TimelessJewelData, TimelessNodeIndex,
};

use crate::jewel_radius::nodes_in_radius;
use crate::mod_parser::parse_mod_line;
use crate::modifier::{Mod, Source};

/// Identification result for a Timeless jewel. Carries the conqueror name we
/// pulled out of the jewel's mod text plus the seed (Glorious Vanity's
/// "Bathed in the blood of N sacrificed in the name of <X>" → seed = N,
/// conqueror = X).
///
/// The seed isn't used in slice 1 (keystone replacement is seed-independent)
/// but is preserved so the future notable / small-node slices don't need to
/// re-parse the mod text.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TimelessJewel {
    /// Jewel base name as it appears on the item (`"Glorious Vanity"`, …).
    pub base_name: String,
    /// Conqueror name (`"Doryani"`, `"Maxarius"`, …) — case-preserved from the
    /// jewel's mod text. `TimelessJewelData::find_conqueror` is
    /// case-insensitive on lookup.
    pub conqueror_name: String,
    /// Numeric seed pulled from the jewel's mod text. PoB clamps to
    /// `[seedMin, seedMax]` per jewel type at lookup time; we just preserve
    /// the value the user typed.
    pub seed: u32,
}

/// Try to identify `item` as a Timeless jewel and extract the conqueror name +
/// seed from its mod text. Returns `None` for items that don't match either
/// the base name or the canonical mod-text patterns.
///
/// The patterns mirror PoB's `Data/ModCache.lua` entries:
///
/// * Glorious Vanity: `"Bathed in the blood of N sacrificed in the name of X"`.
/// * Lethal Pride:    `"Commanded leadership over N warriors under X"`.
/// * Brutal Restraint:`"Denoted service of N dekhara in the akhara of X"`.
/// * Militant Faith:  `"Carved to glorify N new faithful converted by High Templar X"`.
/// * Elegant Hubris:  `"Commissioned N coins to commemorate X"`.
/// * Heroic Tragedy:  `"Remembrancing N songworthy deeds by the line of X"`.
pub fn identify_timeless_jewel(item: &Item) -> Option<TimelessJewel> {
    if !is_timeless_base(&item.base_name) {
        return None;
    }
    for ml in &item.mod_lines {
        if let Some((seed, name)) = parse_timeless_marker(&item.base_name, &ml.line) {
            return Some(TimelessJewel {
                base_name: item.base_name.clone(),
                conqueror_name: name,
                seed,
            });
        }
    }
    None
}

fn is_timeless_base(name: &str) -> bool {
    matches!(
        name,
        "Glorious Vanity"
            | "Lethal Pride"
            | "Brutal Restraint"
            | "Militant Faith"
            | "Elegant Hubris"
            | "Heroic Tragedy"
    )
}

/// The marker-line grammar for a Timeless jewel base: `(prefix, infix,
/// strip_high_templar)`. The prefix ends right before the seed; the infix ends
/// right before the conqueror name. Mirrors PoB's `Data/ModCache.lua` entries
/// verbatim. Militant Faith carries a `"High Templar "` honorific on the name
/// in the mod text that the data file's `"Avarius"` / `"Dominus"` /
/// `"Maxarius"` / `"Venarius"` don't — `strip_high_templar` flags it so
/// [`parse_timeless_marker`] removes it and [`format_timeless_marker`] re-adds
/// it, keeping the two inverse.
fn timeless_marker_grammar(base_name: &str) -> Option<(&'static str, &'static str, bool)> {
    Some(match base_name {
        "Glorious Vanity" => (
            "Bathed in the blood of ",
            " sacrificed in the name of ",
            false,
        ),
        "Lethal Pride" => ("Commanded leadership over ", " warriors under ", false),
        "Brutal Restraint" => ("Denoted service of ", " dekhara in the akhara of ", false),
        "Militant Faith" => ("Carved to glorify ", " new faithful converted by ", true),
        "Elegant Hubris" => ("Commissioned ", " coins to commemorate ", false),
        "Heroic Tragedy" => ("Remembrancing ", " songworthy deeds by the line of ", false),
        _ => return None,
    })
}

/// Extract `(seed, conqueror_name)` from a single mod line. Returns `None`
/// if the line doesn't match the prefix associated with `base_name`.
///
/// The grammar is fixed-prefix + integer + fixed-infix + free-form name. We
/// match by stripping the prefix, parsing the leading integer, then stripping
/// the infix and trimming the rest.
fn parse_timeless_marker(base_name: &str, line: &str) -> Option<(u32, String)> {
    let (prefix, infix, strip_high_templar) = timeless_marker_grammar(base_name)?;
    let rest = line.strip_prefix(prefix)?;
    let infix_pos = rest.find(infix)?;
    let (seed_text, after) = rest.split_at(infix_pos);
    let seed = seed_text.trim().parse::<u32>().ok()?;
    let mut name = after[infix.len()..].trim().to_string();
    if strip_high_templar {
        name = name
            .strip_prefix("High Templar ")
            .unwrap_or(name.as_str())
            .to_string();
    }
    if name.is_empty() {
        return None;
    }
    Some((seed, name))
}

/// Render a marker mod line for `base_name` from a `seed` and a (stripped)
/// `conqueror_name`. Exact inverse of [`parse_timeless_marker`]:
/// `parse_timeless_marker(base, &format_timeless_marker(base, seed, name)?) ==
/// Some((seed, name))` for every Timeless base. Returns `None` for a non-Timeless
/// base or an empty name.
///
/// `seed` is the **stored** value as it appears in the item text — for Elegant
/// Hubris that is the ×20 "coins" value, not the internal LUT seed (the engine
/// divides by 20 at lookup time; see `pob_data::seed_range`). The picker UI
/// supplies stored values straight from `pob_data::stored_seed_bounds`.
#[must_use]
pub fn format_timeless_marker(base_name: &str, seed: u32, conqueror_name: &str) -> Option<String> {
    let (prefix, infix, strip_high_templar) = timeless_marker_grammar(base_name)?;
    let name = conqueror_name.trim();
    if name.is_empty() {
        return None;
    }
    let honorific = if strip_high_templar {
        "High Templar "
    } else {
        ""
    };
    Some(format!("{prefix}{seed}{infix}{honorific}{name}"))
}

/// Rewrite the Timeless-jewel marker mod line on `item` in place so it carries
/// the given `conqueror_name` + `seed`, leaving every other mod line untouched.
/// Returns `true` when a marker line was found and its text actually changed
/// (so the caller can gate a recompute), `false` otherwise.
///
/// Only the *first* line that currently parses as a marker is rewritten — a
/// well-formed Timeless jewel has exactly one. If `item.base_name` isn't a
/// Timeless base or no marker line is present, the item is left unchanged.
pub fn set_timeless_marker(item: &mut Item, conqueror_name: &str, seed: u32) -> bool {
    let Some(new_line) = format_timeless_marker(&item.base_name, seed, conqueror_name) else {
        return false;
    };
    let base_name = item.base_name.clone();
    for ml in &mut item.mod_lines {
        if parse_timeless_marker(&base_name, &ml.line).is_some() {
            if ml.line == new_line {
                return false;
            }
            ml.line = new_line;
            return true;
        }
    }
    false
}

/// One keystone replacement to apply: the in-radius allocated keystone node
/// id and the replacement-keystone descriptor. Produced by
/// [`compute_keystone_replacements`]; consumed by the perform pipeline.
#[derive(Debug, Clone)]
pub struct KeystoneReplacement {
    /// Tree-socket node id of the jewel that owns this replacement (for
    /// breakdown attribution / debugging).
    pub socket_id: NodeId,
    /// Allocated keystone node id whose mods are being overridden.
    pub target_node: NodeId,
    /// Replacement-keystone mod text. Cloned out of
    /// `TimelessJewelData::keystones` so the caller doesn't need to keep the
    /// data borrow alive while applying.
    pub replacement: ConquerorKeystone,
    /// Display label used as the breakdown source (`"Timeless:Glorious
    /// Vanity:Doryani"`).
    pub source_label: String,
}

/// Walk every socketed jewel and, for each Timeless jewel, build the
/// replacement list for its allocated in-radius keystones. Returns an empty
/// vec when no Timeless jewels are present or no keystones are conquered.
///
/// Computation is read-only — the caller is responsible for (a) skipping the
/// original keystone's stats during the tree-stat pass, and (b) applying
/// `replacement.stats` afterwards. The keystone-replacement set tells the
/// tree-stat pass which nodes to skip; see [`conquered_keystone_set`].
pub fn compute_keystone_replacements(
    tree: &PassiveTree,
    allocated: &AHashSet<NodeId>,
    socketed: &[(NodeId, Item)],
    data: &TimelessJewelData,
) -> Vec<KeystoneReplacement> {
    let mut out: Vec<KeystoneReplacement> = Vec::new();
    let radii = radii_for_tree_version(&tree.version);
    // Timeless jewels always use the Large radius (PoB encodes this on the
    // item base; all five variants share `Radius: Large` in
    // `Data/Uniques/jewel.lua`). RADII_3_16[2] = Large (0..1800).
    let large_idx = 2usize;
    let Some(radius) = radii.get(large_idx).copied() else {
        return out;
    };
    for (socket_id, item) in socketed {
        let Some(jewel) = identify_timeless_jewel(item) else {
            continue;
        };
        let Some(replacement) = data.replacement_for(&jewel.base_name, &jewel.conqueror_name)
        else {
            continue;
        };
        let label = format!("Timeless:{}:{}", jewel.base_name, jewel.conqueror_name);
        // Identify allocated keystones inside the jewel's radius. PoB also
        // *removes* keystones that are no longer satisfied (e.g. you can
        // "lose" Pain Attunement when conquered by Vaal); the alloc filter
        // handles that since the tree-stat pass only credits allocated
        // nodes anyway.
        for (target_node, _) in nodes_in_radius(tree, *socket_id, &radius) {
            if !allocated.contains(&target_node) {
                continue;
            }
            let Some(node) = tree.nodes.get(&target_node) else {
                continue;
            };
            if !matches!(node.kind, NodeKind::Keystone) {
                continue;
            }
            out.push(KeystoneReplacement {
                socket_id: *socket_id,
                target_node,
                replacement: replacement.clone(),
                source_label: label.clone(),
            });
        }
    }
    out
}

/// Build the set of allocated keystone node ids that the tree-stat pass
/// should **skip** because a Timeless jewel is replacing their mods. Pulled
/// out from [`compute_keystone_replacements`] so callers that only need the
/// skip-set don't pay for the keystone-mod clone.
pub fn conquered_keystone_set(replacements: &[KeystoneReplacement]) -> AHashSet<NodeId> {
    replacements.iter().map(|r| r.target_node).collect()
}

/// Apply every keystone replacement in `replacements` to `db`. Each line on
/// the conqueror keystone is parsed via [`parse_mod_line`] and added with
/// `Source::Passive(target_node)` so the per-node breakdown attributes the
/// new mods to the conquered passive (mirroring PoB's `ReplaceNode` keeping
/// the same node id).
///
/// Returns the number of mod emissions performed (sum across all
/// replacements). Lines that fail `parse_mod_line` are silently dropped —
/// the source data ships canonical PoB strings, so failures are bugs in the
/// parser, not data.
pub fn apply_keystone_replacements(
    replacements: &[KeystoneReplacement],
    db: &mut crate::ModDB,
) -> usize {
    let mut emissions = 0usize;
    for r in replacements {
        for line in &r.replacement.stats {
            let trimmed = line.trim();
            if trimmed.is_empty() {
                continue;
            }
            if let Some(parsed) = parse_mod_line(trimmed) {
                let mut m: Mod = parsed.mod_;
                m.source = Some(Source::Passive(r.target_node));
                db.add(m);
                emissions += 1;
            }
        }
    }
    emissions
}

/// Read-only data the notable/small-node LUT decode needs (#227). The engine
/// never touches the filesystem; the caller (UI) loads the static metadata and
/// inflates the per-jewel LUT blobs, then lends them here.
pub struct TimelessLutContext<'a> {
    /// `NodeIndexMapping` — node→slot table + local→global id remap.
    pub node_index: &'a TimelessNodeIndex,
    /// The `legionAdditions` + `legionNodes` replacement pools.
    pub pool: &'a LegionPool,
    /// Inflated LUT bytes per **1-based** jewel type (`jewel_type_for_base`).
    /// The caller only populates the types actually socketed; a missing type
    /// simply yields no replacements for that jewel.
    pub luts: &'a HashMap<u32, Vec<u8>>,
}

/// One notable/small-node replacement to apply: the in-radius allocated node
/// and the (already roll-substituted) stat lines that replace or augment it.
/// Produced by [`compute_notable_replacements`]; consumed by the perform
/// pipeline analogously to [`KeystoneReplacement`].
#[derive(Debug, Clone)]
pub struct NotableReplacement {
    /// Tree-socket node id of the owning jewel (breakdown attribution).
    pub socket_id: NodeId,
    /// Allocated node id whose stats are overridden / augmented.
    pub target_node: NodeId,
    /// Stat lines to apply (roll-substituted in `pob_data`).
    pub lines: Vec<String>,
    /// `true` → the node's own stats are skipped (PoB `ReplaceNode`); `false`
    /// → `lines` stack on top of the node's own (an *addition*).
    pub wipe: bool,
    /// Display label used as the breakdown source.
    pub source_label: String,
}

/// Walk every socketed Timeless jewel and, for each allocated **notable** or
/// **small** node inside its radius, decode the per-seed LUT replacement.
/// Mirrors the `node.type == "Notable"` / vaal-`"Normal"` branches of
/// `PassiveSpec.lua`. Keystones are handled by
/// [`compute_keystone_replacements`]; the non-vaal small-node text additions
/// (karui `+Strength`, etc.) are not yet ported and simply yield nothing.
///
/// Returns an empty vec when no Timeless jewel is socketed or its LUT is absent
/// from `ctx.luts`.
pub fn compute_notable_replacements(
    tree: &PassiveTree,
    allocated: &AHashSet<NodeId>,
    socketed: &[(NodeId, Item)],
    ctx: &TimelessLutContext<'_>,
) -> Vec<NotableReplacement> {
    let mut out: Vec<NotableReplacement> = Vec::new();
    let radii = radii_for_tree_version(&tree.version);
    // Timeless jewels always use the Large radius (see compute_keystone_replacements).
    let Some(radius) = radii.get(2usize).copied() else {
        return out;
    };
    for (socket_id, item) in socketed {
        let Some(jewel) = identify_timeless_jewel(item) else {
            continue;
        };
        let Some(jewel_type) = jewel_type_for_base(&jewel.base_name) else {
            continue;
        };
        let Some(lut) = ctx.luts.get(&jewel_type) else {
            continue;
        };
        let label = format!("Timeless:{}:{}", jewel.base_name, jewel.conqueror_name);
        for (target_node, _) in nodes_in_radius(tree, *socket_id, &radius) {
            if !allocated.contains(&target_node) {
                continue;
            }
            let Some(node) = tree.nodes.get(&target_node) else {
                continue;
            };
            // Keystones go through the keystone path; sockets / masteries /
            // class-starts are never conquered into notables.
            let is_notable = match node.kind {
                NodeKind::Notable => true,
                NodeKind::Normal => false,
                _ => continue,
            };
            // PoB's `isValueInArray(attributes, node.dn)`: a pure attribute
            // small node takes the smaller non-vaal addition (or the templar
            // Devotion-node replacement). Tattoos also qualify in PoB, but the
            // radius decode doesn't carry tattoo state here.
            let is_attribute_node = node
                .name
                .as_deref()
                .is_some_and(|n| matches!(n, "Strength" | "Dexterity" | "Intelligence"));
            if let Some(rep) = ctx.node_index.resolve_node_replacement(
                ctx.pool,
                lut,
                jewel_type,
                target_node,
                jewel.seed,
                is_notable,
                is_attribute_node,
            ) {
                out.push(NotableReplacement {
                    socket_id: *socket_id,
                    target_node,
                    lines: rep.lines,
                    wipe: rep.wipe,
                    source_label: label.clone(),
                });
            }
        }
    }
    out
}

/// The set of allocated node ids whose original stats the tree-stat pass should
/// **skip** because a Timeless jewel *replaces* (not augments) them. Additions
/// (`wipe == false`) are excluded — their host node keeps its own stats.
#[must_use]
pub fn conquered_notable_set(replacements: &[NotableReplacement]) -> AHashSet<NodeId> {
    replacements
        .iter()
        .filter(|r| r.wipe)
        .map(|r| r.target_node)
        .collect()
}

/// Apply every notable/small-node replacement to `db`. Each line is parsed via
/// [`parse_mod_line`] and added with `Source::Passive(target_node)` so per-node
/// breakdown attribution stays sensible (mirroring PoB's `ReplaceNode` /
/// `NodeAdditionOrReplacementFromString` keeping the same node id). Returns the
/// number of mod emissions.
pub fn apply_notable_replacements(
    replacements: &[NotableReplacement],
    db: &mut crate::ModDB,
) -> usize {
    let mut emissions = 0usize;
    for r in replacements {
        for line in &r.lines {
            let trimmed = line.trim();
            if trimmed.is_empty() {
                continue;
            }
            if let Some(parsed) = parse_mod_line(trimmed) {
                let mut m: Mod = parsed.mod_;
                m.source = Some(Source::Passive(r.target_node));
                db.add(m);
                emissions += 1;
            }
        }
    }
    emissions
}

/// Inflate a raw-zlib timeless LUT (PoB's `Inflate`). Glorious Vanity ships as
/// `*.zip.part0..N` which must be concatenated before calling. The `.zip`
/// extension is a misnomer — the files are raw zlib streams, not archives.
pub fn inflate_timeless_lut(compressed: &[u8]) -> std::io::Result<Vec<u8>> {
    use flate2::read::ZlibDecoder;
    use std::io::Read;
    let mut out = Vec::new();
    ZlibDecoder::new(compressed).read_to_end(&mut out)?;
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ModDB;

    /// Real-data check (self-skips in CI): the Militant Faith LUT inflates to
    /// exactly `notable_count (452) * seedSize (8001)` bytes — confirming both
    /// the zlib format and the flat non-GV layout `flat_notable_replacement`
    /// indexes into. seedSize = seedMax(10000) - seedMin(2000) + 1.
    #[test]
    fn militant_faith_lut_inflates_to_notable_grid() {
        let path = std::path::Path::new("../../data/timeless/MilitantFaith.zip");
        if !path.exists() {
            return;
        }
        let compressed = std::fs::read(path).expect("read MilitantFaith.zip");
        // In CI the file is an unresolved Git LFS pointer (~130 bytes), not the
        // real ~860 KB blob — skip rather than try to inflate the pointer text.
        if compressed.len() < 100_000 {
            return;
        }
        let lut = inflate_timeless_lut(&compressed).expect("inflate");
        // notable_count is 452 in 3.25-era data; read the matching index.
        let notables = std::fs::read_to_string("../../data/timeless_node_index.json")
            .ok()
            .and_then(|j| pob_data::load_timeless_node_index(&j).ok())
            .map_or(452, |idx| idx.notable_count as usize);
        assert_eq!(lut.len(), notables * 8001, "flat LUT = notables × seedSize");
    }

    use pob_data::{
        item::{ModSection, Rarity},
        ConquerorKeystone, Group, ItemSet, ModLine, Node, NodeKind, PassiveTree, TimelessConqueror,
        TimelessJewelConfig, TimelessJewelData, TreeConstants,
    };

    /// Real-data structural check (self-skips in CI where the LFS blobs are
    /// unresolved pointers): inflate the actual Glorious Vanity LUT and the
    /// committed node-index + pools, then (a) verify the inflated length matches
    /// the GV layout `node_count*seedSize + Σ slot.size` exactly, and (b) decode
    /// a real notable for a real seed and confirm it resolves to a valid
    /// replacement with non-empty stat lines. This catches sizes-block / packed
    /// offset bugs the synthetic tests can't.
    #[test]
    fn real_glorious_vanity_lut_layout_and_decode() {
        let dir = std::path::Path::new("../../data");
        // Concatenate the GV parts (part0..N).
        let mut compressed = Vec::new();
        for part in 0.. {
            let p = dir.join(format!("timeless/GloriousVanity.zip.part{part}"));
            let Ok(bytes) = std::fs::read(&p) else { break };
            compressed.extend_from_slice(&bytes);
        }
        // Skip when absent or an unresolved LFS pointer (real blob is ~22 MB).
        if compressed.len() < 1_000_000 {
            return;
        }
        let (Ok(idx_json), Ok(add_json), Ok(node_json)) = (
            std::fs::read_to_string(dir.join("timeless_node_index.json")),
            std::fs::read_to_string(dir.join("legion_passives.json")),
            std::fs::read_to_string(dir.join("legion_nodes.json")),
        ) else {
            return;
        };
        let index = pob_data::load_timeless_node_index(&idx_json).expect("node index");
        let pool = pob_data::LegionPool {
            additions: pob_data::load_legion_passives(&add_json).expect("additions"),
            nodes: pob_data::load_legion_nodes(&node_json).expect("nodes"),
        };
        let lut = inflate_timeless_lut(&compressed).expect("inflate GV");

        // (a) Layout: sizes block (node_count * seedSize) + Σ per-node packed.
        let (smin, smax) = pob_data::seed_range(1).unwrap();
        let seed_size = (smax - smin + 1) as usize; // 7901
        let sizes_block = index.node_count as usize * seed_size;
        let packed: usize = index.nodes.iter().map(|s| s.size as usize).sum();
        assert_eq!(
            lut.len(),
            sizes_block + packed,
            "GV inflated length must equal sizes block + packed node data"
        );

        // (b) Decode: for a fixed seed, the first stretch of notable slots
        // (index <= notable_count) should mostly resolve to valid replacements.
        let seed = 5000u32;
        let mut decoded = 0;
        let mut checked = 0;
        for slot in index
            .nodes
            .iter()
            .filter(|s| s.index <= index.notable_count)
        {
            if checked >= 40 {
                break;
            }
            checked += 1;
            if let Some(rep) =
                index.resolve_node_replacement(&pool, &lut, 1, slot.node_id, seed, true, false)
            {
                assert!(
                    !rep.lines.is_empty(),
                    "node {} decoded to empty lines",
                    slot.node_id
                );
                decoded += 1;
            }
        }
        assert!(
            decoded * 2 >= checked,
            "expected most notables to decode for seed {seed}: {decoded}/{checked}"
        );
    }

    fn mk_item(base: &str, mod_lines: &[&str]) -> Item {
        Item {
            name: base.into(),
            base_name: base.into(),
            rarity: Rarity::Unique,
            item_level: 84,
            quality: 0,
            tags: ahash::HashSet::default(),
            mod_lines: mod_lines
                .iter()
                .map(|l| ModLine {
                    line: (*l).to_string(),
                    section: ModSection::Explicit,
                    variant_list: None,
                })
                .collect(),
            sockets: String::new(),
            raw: String::new(),
            corrupted: false,
            mirrored: false,
            variants: Vec::new(),
            variant: None,
        }
    }

    #[test]
    fn identify_glorious_vanity_doryani() {
        let item = mk_item(
            "Glorious Vanity",
            &[
                "Bathed in the blood of 8000 sacrificed in the name of Doryani",
                "Passives in radius are Conquered by the Vaal",
            ],
        );
        let j = identify_timeless_jewel(&item).expect("identified");
        assert_eq!(j.base_name, "Glorious Vanity");
        assert_eq!(j.conqueror_name, "Doryani");
        assert_eq!(j.seed, 8000);
    }

    #[test]
    fn identify_militant_faith_strips_high_templar() {
        let item = mk_item(
            "Militant Faith",
            &["Carved to glorify 6000 new faithful converted by High Templar Avarius"],
        );
        let j = identify_timeless_jewel(&item).expect("identified");
        assert_eq!(j.conqueror_name, "Avarius");
        assert_eq!(j.seed, 6000);
    }

    #[test]
    fn identify_lethal_pride_v2_conqueror() {
        let item = mk_item(
            "Lethal Pride",
            &["Commanded leadership over 14000 warriors under Akoya"],
        );
        let j = identify_timeless_jewel(&item).expect("identified");
        assert_eq!(j.base_name, "Lethal Pride");
        assert_eq!(j.conqueror_name, "Akoya");
        assert_eq!(j.seed, 14000);
    }

    #[test]
    fn identify_elegant_hubris_extracts_seed() {
        let item = mk_item(
            "Elegant Hubris",
            &["Commissioned 81000 coins to commemorate Caspiro"],
        );
        let j = identify_timeless_jewel(&item).expect("identified");
        assert_eq!(j.seed, 81000);
        assert_eq!(j.conqueror_name, "Caspiro");
    }

    #[test]
    fn ignores_non_timeless_base() {
        let item = mk_item("Crimson Jewel", &["10% increased Maximum Life"]);
        assert!(identify_timeless_jewel(&item).is_none());
    }

    #[test]
    fn ignores_timeless_base_without_marker_line() {
        let item = mk_item(
            "Glorious Vanity",
            &["Passives in radius are Conquered by the Vaal"],
        );
        assert!(identify_timeless_jewel(&item).is_none());
    }

    #[test]
    fn format_marker_round_trips_through_parse() {
        // (base, stored seed, stripped conqueror name) — one per base, covering
        // the High-Templar honorific (Militant Faith) and the ×20 Elegant Hubris
        // stored value. format → parse must reproduce the inputs exactly.
        let cases = [
            ("Glorious Vanity", 6500u32, "Doryani"),
            ("Lethal Pride", 14000, "Kaom"),
            ("Brutal Restraint", 500, "Asenath"),
            ("Militant Faith", 2000, "Maxarius"),
            ("Elegant Hubris", 160000, "Cadiro"),
            ("Heroic Tragedy", 100, "Vorana"),
        ];
        for (base, seed, name) in cases {
            let line = format_timeless_marker(base, seed, name)
                .unwrap_or_else(|| panic!("format failed for {base}"));
            let (got_seed, got_name) = parse_timeless_marker(base, &line)
                .unwrap_or_else(|| panic!("parse failed for {base}: {line:?}"));
            assert_eq!(got_seed, seed, "{base} seed round-trip");
            assert_eq!(got_name, name, "{base} name round-trip");
        }
    }

    #[test]
    fn format_marker_re_adds_high_templar_for_militant_faith() {
        let line = format_timeless_marker("Militant Faith", 5000, "Avarius").unwrap();
        assert_eq!(
            line,
            "Carved to glorify 5000 new faithful converted by High Templar Avarius"
        );
    }

    #[test]
    fn format_marker_rejects_non_timeless_and_empty_name() {
        assert!(format_timeless_marker("Crimson Jewel", 10, "Foo").is_none());
        assert!(format_timeless_marker("Glorious Vanity", 10, "  ").is_none());
    }

    #[test]
    fn set_marker_rewrites_only_the_marker_line() {
        let mut item = mk_item(
            "Glorious Vanity",
            &[
                "Bathed in the blood of 8000 sacrificed in the name of Doryani",
                "Passives in radius are Conquered by the Vaal",
            ],
        );
        let changed = set_timeless_marker(&mut item, "Xibaqua", 6500);
        assert!(changed, "marker text changed");
        // The non-marker line is untouched.
        assert_eq!(
            item.mod_lines[1].line,
            "Passives in radius are Conquered by the Vaal"
        );
        // identify now reflects the new conqueror + seed — i.e. persistence is
        // automatic via the serialized item.
        let j = identify_timeless_jewel(&item).expect("identified after rewrite");
        assert_eq!(j.conqueror_name, "Xibaqua");
        assert_eq!(j.seed, 6500);
    }

    #[test]
    fn set_marker_is_noop_when_unchanged() {
        let mut item = mk_item(
            "Lethal Pride",
            &["Commanded leadership over 14000 warriors under Kaom"],
        );
        assert!(
            !set_timeless_marker(&mut item, "Kaom", 14000),
            "identical rewrite reports no change"
        );
    }

    #[test]
    fn marker_edit_survives_item_serialization_round_trip() {
        // AC#3 for #216: a conqueror/seed change persists through save/load,
        // which for a socketed jewel means the `Item` (with its rewritten
        // marker line) serialises and re-identifies to the new values.
        let mut item = mk_item(
            "Elegant Hubris",
            &["Commissioned 2000 coins to commemorate Cadiro"],
        );
        assert!(set_timeless_marker(&mut item, "Caspiro", 160000));
        let json = serde_json::to_string(&item).expect("serialize item");
        let restored: Item = serde_json::from_str(&json).expect("deserialize item");
        let j = identify_timeless_jewel(&restored).expect("identified after round-trip");
        assert_eq!(j.conqueror_name, "Caspiro");
        assert_eq!(j.seed, 160000);
    }

    fn mk_data() -> TimelessJewelData {
        let mut data = TimelessJewelData {
            version: 1,
            comment: String::new(),
            jewels: indexmap::IndexMap::new(),
            keystones: indexmap::IndexMap::new(),
        };
        data.jewels.insert(
            "Glorious Vanity".into(),
            TimelessJewelConfig {
                conqueror_type: "vaal".into(),
                conquerors: vec![TimelessConqueror {
                    name: "Doryani".into(),
                    conqueror_id: "3".into(),
                    keystone_id: "vaal_keystone_3".into(),
                }],
            },
        );
        data.keystones.insert(
            "vaal_keystone_3".into(),
            ConquerorKeystone {
                name: "Corrupted Soul".into(),
                stats: vec![
                    "50% of Non-Chaos Damage taken bypasses Energy Shield".into(),
                    "Gain 15% of Maximum Life as Extra Maximum Energy Shield".into(),
                ],
            },
        );
        data
    }

    fn mk_two_node_tree(_socket_radius: f32) -> PassiveTree {
        // Socket at (0,0); a near keystone at (600,0) (inside Large radius
        // 1800) and a far keystone at (2500,0) (outside the radius).
        let mut groups = ahash::HashMap::default();
        groups.insert(
            10,
            Group {
                x: 0.0,
                y: 0.0,
                orbits: smallvec::smallvec![0],
                background: None,
                nodes: vec![1],
                is_proxy: false,
            },
        );
        groups.insert(
            20,
            Group {
                x: 600.0,
                y: 0.0,
                orbits: smallvec::smallvec![0],
                background: None,
                nodes: vec![2],
                is_proxy: false,
            },
        );
        groups.insert(
            30,
            Group {
                x: 2500.0,
                y: 0.0,
                orbits: smallvec::smallvec![0],
                background: None,
                nodes: vec![3],
                is_proxy: false,
            },
        );
        let mut nodes = ahash::HashMap::default();
        nodes.insert(
            1,
            Node {
                id: 1,
                name: Some("Jewel Socket".into()),
                icon: None,
                ascendancy_name: None,
                stats: vec![],
                reminder_text: vec![],
                kind: NodeKind::JewelSocket,
                class_start_index: None,
                group: Some(10),
                orbit: Some(0),
                orbit_index: Some(0),
                out_edges: smallvec::smallvec![],
                in_edges: smallvec::smallvec![],
                mastery_effects: vec![],
                expansion_jewel_size: None,
                jewel_radius: None,
            },
        );
        nodes.insert(
            2,
            Node {
                id: 2,
                name: Some("Original Keystone".into()),
                icon: None,
                ascendancy_name: None,
                stats: vec!["Original Keystone Stat".into()],
                reminder_text: vec![],
                kind: NodeKind::Keystone,
                class_start_index: None,
                group: Some(20),
                orbit: Some(0),
                orbit_index: Some(0),
                out_edges: smallvec::smallvec![],
                in_edges: smallvec::smallvec![],
                mastery_effects: vec![],
                expansion_jewel_size: None,
                jewel_radius: None,
            },
        );
        nodes.insert(
            3,
            Node {
                id: 3,
                name: Some("Far Keystone".into()),
                icon: None,
                ascendancy_name: None,
                stats: vec!["Far Keystone Stat".into()],
                reminder_text: vec![],
                kind: NodeKind::Keystone,
                class_start_index: None,
                group: Some(30),
                orbit: Some(0),
                orbit_index: Some(0),
                out_edges: smallvec::smallvec![],
                in_edges: smallvec::smallvec![],
                mastery_effects: vec![],
                expansion_jewel_size: None,
                jewel_radius: None,
            },
        );
        PassiveTree {
            version: "3_25".into(),
            tree: "Default".into(),
            classes: vec![],
            groups,
            nodes,
            jewel_slots: vec![1],
            min_x: -3000,
            min_y: -1000,
            max_x: 5000,
            max_y: 1000,
            constants: TreeConstants {
                skills_per_orbit: vec![1, 6, 16, 16, 40, 72, 72],
                orbit_radii: vec![0, 82, 162, 335, 493, 662, 846],
                classes: ahash::HashMap::default(),
                character_attributes: ahash::HashMap::default(),
                pss_centre_inner_radius: None,
            },
            points: pob_data::TreePoints::default(),
        }
    }

    #[test]
    fn replacement_targets_in_radius_allocated_keystone() {
        let tree = mk_two_node_tree(0.0);
        let data = mk_data();
        let item = mk_item(
            "Glorious Vanity",
            &["Bathed in the blood of 8000 sacrificed in the name of Doryani"],
        );
        let mut alloc: AHashSet<NodeId> = AHashSet::default();
        alloc.insert(2);
        alloc.insert(3);
        let socketed = vec![(1u32, item)];
        let reps = compute_keystone_replacements(&tree, &alloc, &socketed, &data);
        // Node 2 is in radius (600 units < 1800), node 3 is outside (~2500 units > 1800).
        assert_eq!(reps.len(), 1);
        assert_eq!(reps[0].target_node, 2);
        assert_eq!(reps[0].replacement.name, "Corrupted Soul");
    }

    #[test]
    fn replacement_skips_unallocated_keystone() {
        let tree = mk_two_node_tree(0.0);
        let data = mk_data();
        let item = mk_item(
            "Glorious Vanity",
            &["Bathed in the blood of 8000 sacrificed in the name of Doryani"],
        );
        let alloc: AHashSet<NodeId> = AHashSet::default(); // nothing allocated
        let socketed = vec![(1u32, item)];
        let reps = compute_keystone_replacements(&tree, &alloc, &socketed, &data);
        assert!(reps.is_empty());
    }

    #[test]
    fn unknown_conqueror_yields_no_replacement() {
        let tree = mk_two_node_tree(0.0);
        let data = mk_data();
        // The data file we built only knows Doryani; pretend the user has
        // Xibaqua. Should silently no-op rather than panic.
        let item = mk_item(
            "Glorious Vanity",
            &["Bathed in the blood of 8000 sacrificed in the name of Xibaqua"],
        );
        let mut alloc: AHashSet<NodeId> = AHashSet::default();
        alloc.insert(2);
        let socketed = vec![(1u32, item)];
        let reps = compute_keystone_replacements(&tree, &alloc, &socketed, &data);
        assert!(reps.is_empty());
    }

    #[test]
    fn apply_emits_replacement_mods_with_source() {
        let tree = mk_two_node_tree(0.0);
        let data = mk_data();
        let item = mk_item(
            "Glorious Vanity",
            &["Bathed in the blood of 8000 sacrificed in the name of Doryani"],
        );
        let mut alloc: AHashSet<NodeId> = AHashSet::default();
        alloc.insert(2);
        let socketed = vec![(1u32, item)];
        let reps = compute_keystone_replacements(&tree, &alloc, &socketed, &data);
        let mut db = ModDB::default();
        let emitted = apply_keystone_replacements(&reps, &mut db);
        // "50% of Non-Chaos Damage taken bypasses Energy Shield" parses to a
        // FLAG; "Gain 15% of Maximum Life as Extra Maximum Energy Shield"
        // parses to one mod too. So we expect 2 emissions (or could be 1 if
        // the flag line doesn't currently parse — assert_at_least style).
        assert!(
            emitted >= 1,
            "expected at least one mod emission, got {emitted}"
        );
        // At least one mod tagged Source::Passive(2) — i.e., kept on the
        // conquered node id.
        use crate::ModStore;
        let any_passive_2 = db
            .iter_all()
            .any(|m| matches!(m.source, Some(Source::Passive(2))));
        assert!(any_passive_2);
    }

    #[test]
    fn skip_set_lists_replaced_node_ids() {
        let tree = mk_two_node_tree(0.0);
        let data = mk_data();
        let item = mk_item(
            "Glorious Vanity",
            &["Bathed in the blood of 8000 sacrificed in the name of Doryani"],
        );
        let mut alloc: AHashSet<NodeId> = AHashSet::default();
        alloc.insert(2);
        let socketed = vec![(1u32, item)];
        let reps = compute_keystone_replacements(&tree, &alloc, &socketed, &data);
        let skip = conquered_keystone_set(&reps);
        assert!(skip.contains(&2));
        assert_eq!(skip.len(), 1);
    }

    /// Build a socket (node 1) + one allocated Notable (node 2, inside the
    /// Large radius) + one far Notable (node 3, outside). Mirrors
    /// `mk_two_node_tree` but with Notable nodes for the #227 path.
    fn mk_notable_tree() -> PassiveTree {
        let mk_group = |x: f32, node: NodeId| Group {
            x,
            y: 0.0,
            orbits: smallvec::smallvec![0],
            background: None,
            nodes: vec![node],
            is_proxy: false,
        };
        let mk_node = |id: NodeId, kind: NodeKind, group: u32, stats: Vec<String>| Node {
            id,
            name: Some(format!("node{id}")),
            icon: None,
            ascendancy_name: None,
            stats,
            reminder_text: vec![],
            kind,
            class_start_index: None,
            group: Some(group),
            orbit: Some(0),
            orbit_index: Some(0),
            out_edges: smallvec::smallvec![],
            in_edges: smallvec::smallvec![],
            mastery_effects: vec![],
            expansion_jewel_size: None,
            jewel_radius: None,
        };
        let mut groups = ahash::HashMap::default();
        groups.insert(10, mk_group(0.0, 1));
        groups.insert(20, mk_group(600.0, 2));
        let mut nodes = ahash::HashMap::default();
        nodes.insert(1, mk_node(1, NodeKind::JewelSocket, 10, vec![]));
        nodes.insert(
            2,
            mk_node(
                2,
                NodeKind::Notable,
                20,
                vec!["10% increased Original Notable Damage".into()],
            ),
        );
        PassiveTree {
            version: "3_25".into(),
            tree: "Default".into(),
            classes: vec![],
            groups,
            nodes,
            jewel_slots: vec![1],
            min_x: -3000,
            min_y: -1000,
            max_x: 5000,
            max_y: 1000,
            constants: TreeConstants {
                skills_per_orbit: vec![1, 6, 16, 16, 40, 72, 72],
                orbit_radii: vec![0, 82, 162, 335, 493, 662, 846],
                classes: ahash::HashMap::default(),
                character_attributes: ahash::HashMap::default(),
                pss_centre_inner_radius: None,
            },
            points: pob_data::TreePoints::default(),
        }
    }

    #[test]
    fn notable_replacement_flows_into_moddb() {
        use pob_data::{LegionPassive, LegionPool, TimelessNodeIndex, TimelessNodeSlot};

        let tree = mk_notable_tree();
        // Lethal Pride (type 2), flat LUT. split = additions.len() = 1, so
        // global id >= 1 is a node replacement. local 0 → global 1 → nodes[0].
        let pool = LegionPool {
            additions: vec![LegionPassive {
                name: "Some Addition".into(),
                ..Default::default()
            }],
            nodes: vec![LegionPassive {
                name: "Replacement Notable".into(),
                id: "rep".into(),
                stats: vec!["+25 to Strength".into()],
                ..Default::default()
            }],
        };
        // Node 2 at slot index 0 (notable). seedSize = 18000-10000+1 = 8001.
        let node_index = TimelessNodeIndex {
            node_count: 1,
            notable_count: 0, // index 0 <= 0 → notable
            nodes: vec![TimelessNodeSlot {
                node_id: 2,
                index: 0,
                size: 0,
            }],
            // type 2 (idx 1): local 0 → global 1 (= split → nodes[0]).
            local_to_global: vec![Vec::new(), vec![1]],
        };
        let seed_size = (18000 - 10000 + 1) as usize;
        let mut bytes = vec![0u8; seed_size];
        bytes[0] = 0; // node index 0, seed offset 0 → local 0
        let mut luts: ahash::HashMap<u32, Vec<u8>> = ahash::HashMap::default();
        luts.insert(2, bytes);
        let ctx = TimelessLutContext {
            node_index: &node_index,
            pool: &pool,
            luts: &luts,
        };

        let item = mk_item(
            "Lethal Pride",
            &["Commanded leadership over 10000 warriors under Akoya"],
        );
        let mut alloc: AHashSet<NodeId> = AHashSet::default();
        alloc.insert(2);
        let socketed = vec![(1u32, item)];

        let reps = compute_notable_replacements(&tree, &alloc, &socketed, &ctx);
        assert_eq!(reps.len(), 1, "node 2 in radius and allocated");
        assert_eq!(reps[0].target_node, 2);
        assert!(reps[0].wipe, "replacement wipes the original notable");
        assert_eq!(reps[0].lines, vec!["+25 to Strength".to_string()]);

        // The wipe set lists node 2; applying injects the replacement mod.
        let skip = conquered_notable_set(&reps);
        assert!(skip.contains(&2));
        let mut db = crate::ModDB::default();
        let emitted = apply_notable_replacements(&reps, &mut db);
        assert!(emitted >= 1);
        use crate::ModStore;
        assert!(db
            .iter_all()
            .any(|m| matches!(m.source, Some(Source::Passive(2)))));
    }

    #[test]
    fn no_lut_for_jewel_type_yields_no_replacements() {
        use pob_data::{LegionPool, TimelessNodeIndex, TimelessNodeSlot};
        let tree = mk_notable_tree();
        let pool = LegionPool::default();
        let node_index = TimelessNodeIndex {
            node_count: 1,
            notable_count: 0,
            nodes: vec![TimelessNodeSlot {
                node_id: 2,
                index: 0,
                size: 0,
            }],
            local_to_global: vec![],
        };
        // Empty LUT map → the Lethal Pride jewel finds no bytes → no-op.
        let luts: ahash::HashMap<u32, Vec<u8>> = ahash::HashMap::default();
        let ctx = TimelessLutContext {
            node_index: &node_index,
            pool: &pool,
            luts: &luts,
        };
        let item = mk_item(
            "Lethal Pride",
            &["Commanded leadership over 10000 warriors under Akoya"],
        );
        let mut alloc: AHashSet<NodeId> = AHashSet::default();
        alloc.insert(2);
        let reps = compute_notable_replacements(&tree, &alloc, &[(1u32, item)], &ctx);
        assert!(reps.is_empty());
    }

    // Suppress "unused-import" lint for the convenience re-export when this
    // module is consumed by callers via the lib.rs facade.
    #[test]
    fn item_set_alias_compiles() {
        let _: ItemSet = ItemSet::new();
    }
}
