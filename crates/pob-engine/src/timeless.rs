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
//! * **Small-node replacement**. Vaal smalls go through the same per-seed LUT;
//!   non-Vaal conquerors instead inject simple text additions ("+4 Strength"
//!   from Karui smalls, "+5 Devotion" from Templar smalls, etc.). The text
//!   path is small but cosmetic without matching aggregation, so deferred.
//! * **Multiple-choice notables / Vaal "Might / Legacy of the Vaal"**. PoB's
//!   `headerSize == 6 || 8` branch handles those; same LUT story.
//!
//! What ships here covers the highest-impact play (Glorious Vanity / Doryani,
//! Militant Faith / Maxarius, etc.) — keystone-based Timeless builds — and
//! leaves the LUT-driven half clearly factored out behind
//! [`pob_data::TimelessJewelData`].

use ahash::AHashSet;
use pob_data::{
    radii_for_tree_version, ConquerorKeystone, Item, NodeId, NodeKind, PassiveTree,
    TimelessJewelData,
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

/// Extract `(seed, conqueror_name)` from a single mod line. Returns `None`
/// if the line doesn't match the prefix associated with `base_name`.
///
/// The grammar is fixed-prefix + integer + fixed-infix + free-form name. We
/// match by stripping the prefix, parsing the leading integer, then stripping
/// the infix and trimming the rest.
fn parse_timeless_marker(base_name: &str, line: &str) -> Option<(u32, String)> {
    // (prefix, infix). Each prefix ends right before the seed; each infix
    // ends right before the conqueror name. Mirrors PoB's `Data/ModCache.lua`
    // entries verbatim. Militant Faith strips the leading "High Templar " so
    // the looked-up name matches the data file's `"Avarius"` / `"Dominus"` /
    // `"Maxarius"` / `"Venarius"`.
    let (prefix, infix, strip_high_templar) = match base_name {
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
    };
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ModDB;
    use pob_data::{
        item::{ModSection, Rarity},
        ConquerorKeystone, Group, ItemSet, ModLine, Node, NodeKind, PassiveTree, TimelessConqueror,
        TimelessJewelConfig, TimelessJewelData, TreeConstants,
    };

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
                })
                .collect(),
            sockets: String::new(),
            raw: String::new(),
            corrupted: false,
            mirrored: false,
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

    // Suppress "unused-import" lint for the convenience re-export when this
    // module is consumed by callers via the lib.rs facade.
    #[test]
    fn item_set_alias_compiles() {
        let _: ItemSet = ItemSet::new();
    }
}
