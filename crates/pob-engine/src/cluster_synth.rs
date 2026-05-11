//! Cluster jewel sub-graph synthesis — issue [#21].
//!
//! When a Cluster Jewel item is socketed into a Large jewel socket on the passive
//! tree, PoB synthesises a small sub-graph of synthetic notable / small / inner-socket
//! nodes around the host socket. This module mirrors that pass.
//!
//! Mirrors `Classes/PassiveSpec.lua:1676-1748` (`BuildClusterJewelGraphs`) and
//! `1838-2224` (`BuildSubgraph`) in upstream PoB. We don't try to be pixel-identical
//! to PoB's coordinate output — the synthesised nodes get a sensible layout near
//! the parent socket so the UI can render them, but the actual orbit math is
//! simplified. What matters for the calc engine is:
//!
//! 1. the synthesised nodes have stable, collision-free `NodeId`s,
//! 2. each synthesised node carries the right `stats` (so `perform.rs` parses them
//!    with `mod_parser::parse_mod_line`), and
//! 3. they're connected into the live graph via their parent socket so the
//!    pathfinder will route through them.
//!
//! ID scheme (mirrors PoB's `BuildSubgraph` line 1850-1864):
//!
//! ```text
//! bit 16  = 1 (signal bit, prevents collision with real tree-node ids)
//! bits 9-10 = medium-jewel index (0..2)
//! bits 6-8  = large-jewel index (0..5)
//! bits 4-5  = cluster size index (0=Small, 1=Medium, 2=Large)
//! bits 0-3  = ring slot (0..11)
//! ```
//!
//! Since MK2 only synthesises depth-1 sub-graphs (no nested cluster recursion in
//! this slice), we use the simpler form `0x10000 | (parent_socket_index << 6) |
//! (size_index << 4) | slot`.
//!
//! [#21]: https://github.com/jonatanferm/PathOfBuildingMK2/issues/21

use ahash::{HashMap, HashMapExt};
use pob_data::{
    cluster_jewel_mods::ClusterModSet, ClusterJewelData, Item, Node, NodeId, NodeKind, PassiveTree,
};
use smallvec::SmallVec;

/// One synthesised sub-graph attached to a parent jewel socket.
#[derive(Debug, Clone)]
pub struct ClusterJewelSpec {
    /// The host jewel socket on the live tree. Always a tree node with
    /// `expansion_jewel_size = Some(2)` (Large) for this slice.
    pub parent_socket: NodeId,
    /// PoB jewel category — `"Small"` / `"Medium"` / `"Large"`. Only `"Large"`
    /// is emitted by this slice (tree only has Large expansion sockets at depth
    /// zero); nested smaller jewels would extend this in a future slice.
    pub jewel_size: String,
    /// Synthesised nodes keyed by their assigned NodeId. Each entry is a fully-
    /// populated `Node` ready to be merged into a `PassiveTree.nodes` map.
    pub nodes: HashMap<NodeId, Node>,
    /// `(a, b)` pairs of bidirectional edges within the sub-graph plus the entry
    /// edge `(parent_socket, entrance_node)`. Caller is responsible for
    /// reflecting these onto the relevant `Node.in_edges` / `out_edges`.
    pub edges: Vec<(NodeId, NodeId)>,
    /// Entrance node id — the synthesised node directly connected to
    /// `parent_socket`. Always at slot 0 of the ring, mirroring PoB.
    pub entrance: NodeId,
    /// Ids of all synthesised notable nodes — handy for UI listings.
    pub notable_ids: SmallVec<[NodeId; 4]>,
    /// Ids of all synthesised small-passive nodes.
    pub small_ids: SmallVec<[NodeId; 12]>,
    /// Ids of all synthesised inner jewel sockets (recursive nesting points).
    pub socket_ids: SmallVec<[NodeId; 3]>,
}

/// Decoded cluster-jewel item metadata. The user pastes a cluster jewel item
/// like any other item, but the calc engine has to crack open its enchant /
/// explicit lines to learn:
///
/// * which `clusterJewelSkill` (small-passive type) to use,
/// * how many `Added Passive Skills` the jewel grants (= `nodeCount`),
/// * which specific notables the jewel rolled (`Added Passive Skill is X`),
/// * how many of those Added passives are jewel sockets,
/// * any `Added Small Passive Skills also grant: …` lines (a bonus mod every
///   small passive on the jewel inherits — used for cluster small-passive
///   stat-stacking builds).
#[derive(Debug, Clone, Default)]
pub struct ParsedClusterJewel {
    /// `"Small"` / `"Medium"` / `"Large"` matching `ClusterJewelData::jewels` keys.
    pub size: String,
    /// `clusterJewelSkill` — the PoB id of the small-passive type
    /// (`affliction_maximum_life`, `affliction_chaos_damage`, …). Empty when
    /// the jewel has no enchant (vendor-rolled blank jewels).
    pub skill_id: String,
    /// Display name of the small-passive skill (`Life`, `Chaos Damage`, …).
    /// Used as the synthesised small-node `name`.
    pub skill_name: String,
    /// Stat lines every synthesised small passive grants. Mirrors PoB's
    /// `clusterJewel.skills[skill_id].stats` plus any `Added Small Passive
    /// Skills also grant: …` extra lines from the jewel's prefixes.
    pub small_passive_stats: Vec<String>,
    /// Notable display names rolled on this jewel — each name resolves through
    /// the cluster notable template to a `Vec<String>` of stat lines. Length
    /// equals `notableCount`.
    pub notables: Vec<String>,
    /// Total number of added passives (`#nodeCount` in PoB). Includes notables
    /// + sockets + smalls.
    pub node_count: u32,
    /// How many of the added passives are jewel sockets (`socketCount`).
    pub socket_count: u32,
    /// User-typed extra mods applied to every small passive (`clusterJewelAddedMods`
    /// in upstream PoB — currently unused, reserved for the future stat-stack
    /// build slice).
    pub added_small_mods: Vec<String>,
    /// Corruption-roll: `Added Small Passive Skills have N% increased Effect`
    /// (`clusterJewelIncEffect` in upstream). When present and non-zero, the
    /// synthesis pass appends a synthetic `N% increased Effect of Small Passive
    /// Skills` line to every small (Normal) node so its mods scale up. PoB
    /// emits a literal `PassiveSkillEffect INC N` mod here; we go through the
    /// stat line so the existing parse_mod_line pipeline does the work.
    pub small_passive_inc_effect: u32,
}

impl ParsedClusterJewel {
    /// Convenience: derived count of small passives = node_count - notables - sockets.
    pub fn small_count(&self) -> u32 {
        self.node_count
            .saturating_sub(self.notables.len() as u32)
            .saturating_sub(self.socket_count)
    }
}

/// Parse a cluster jewel `Item` into the structured metadata that
/// `synthesise_for_socket` needs. Returns `None` if the item is not a cluster
/// jewel (no `Cluster Jewel` in the base name and no `Added Passive Skills`
/// mod) so callers can speculatively call this without first checking the
/// item type.
///
/// The parser is intentionally permissive — the engine's job is to crank
/// stats out of whatever the user pastes. We accept any of these forms:
///
/// * `Adds N Passive Skills` (plural)
/// * `Adds 1 Passive Skill` (singular)
/// * `Added Small Passive Skills grant: <stat line>`
/// * `Added Small Passive Skills also grant: <stat line>` (alt phrasing)
/// * `1 Added Passive Skill is <Notable>` (each notable mod)
/// * `1 Added Passive Skill is a Jewel Socket` (each inner socket)
///
/// PoB's `Item.lua:ParseRaw` does roughly the same scan over `clusterJewel*`
/// flags; we simplify by walking `mod_lines` directly.
pub fn parse_cluster_jewel(
    item: &Item,
    catalogue: &ClusterJewelData,
) -> Option<ParsedClusterJewel> {
    let base = item.base_name.as_str();
    let size = if base.contains("Large Cluster Jewel") {
        "Large".to_owned()
    } else if base.contains("Medium Cluster Jewel") {
        "Medium".to_owned()
    } else if base.contains("Small Cluster Jewel") {
        "Small".to_owned()
    } else {
        // Tolerate a missing base name when an `Adds N Passive Skills` mod
        // is present anyway. `Large Cluster Jewel` is the only jewel that
        // actually goes into the depth-zero socket the synthesis pass uses,
        // so default to Large.
        if !item
            .mod_lines
            .iter()
            .any(|m| line_is_added_passives_count(&m.line).is_some())
        {
            return None;
        }
        "Large".to_owned()
    };

    let mut parsed = ParsedClusterJewel {
        size: size.clone(),
        ..ParsedClusterJewel::default()
    };

    // Lookup the jewel category to read default node counts. The `min_nodes`
    // value is the fall-back when the user pastes a jewel without an explicit
    // "Adds N Passive Skills" line (e.g. unique cluster jewels without a roll).
    let jewel_def = catalogue.jewels.get(&format!("{} Cluster Jewel", size));

    // Walk mod lines. Order doesn't matter — PoB collects all of them up front.
    for ml in &item.mod_lines {
        let line = ml.line.trim();
        if let Some(n) = line_is_added_passives_count(line) {
            parsed.node_count = parsed.node_count.max(n);
            continue;
        }
        if let Some(notable) = line_is_added_notable(line) {
            parsed.notables.push(notable.to_owned());
            continue;
        }
        if line_is_added_socket(line) {
            parsed.socket_count = parsed.socket_count.saturating_add(1);
            continue;
        }
        // Corruption-roll: `2 Added Passive Skills are Jewel Sockets` (multi-socket
        // alt phrasing; mirrors upstream `(%d+) added passive skills are jewel sockets`).
        if let Some(n) = line_is_added_sockets_count(line) {
            parsed.socket_count = parsed.socket_count.saturating_add(n);
            continue;
        }
        // Corruption-roll: `Adds N Jewel Socket Passive Skills` (override; mirrors
        // upstream `clusterJewelSocketCountOverride`). PoB uses an override here
        // rather than addition; treat it as a direct override of the running
        // socket_count when larger.
        if let Some(n) = line_is_added_sockets_override(line) {
            parsed.socket_count = parsed.socket_count.max(n);
            continue;
        }
        // Corruption-roll: `Added Small Passive Skills have N% increased Effect`.
        // Stored on `small_passive_inc_effect` and surfaced as a synthetic stat
        // line on each small node by the synthesiser.
        if let Some(n) = line_is_small_passive_inc_effect(line) {
            parsed.small_passive_inc_effect = parsed.small_passive_inc_effect.saturating_add(n);
            continue;
        }
        if let Some((skill_name, stat)) = line_is_small_passive_grant(line) {
            // Use the longest such phrasing as the canonical small-passive type
            // name. PoB stores one skill_id per jewel; if there are multiple
            // grant lines they all belong to the same skill (e.g. Axe and
            // Sword Damage emits two lines).
            if parsed.skill_name.is_empty() {
                parsed.skill_name = skill_name.to_owned();
            }
            parsed.small_passive_stats.push(stat.to_owned());
        }
    }

    // Resolve `skill_id` and (if missing) `skill_name` from the catalogue by
    // matching `small_passive_stats` against `ClusterSkill::stats`. The "Added
    // Small Passive Skills grant: …" lines on the item carry only the stat
    // text, not the skill display name, so we have to fingerprint the skill
    // by its stat list. PoB does the same — see `Item.lua:ParseRaw` walking
    // `clusterJewel.skills` looking for an exact stats match.
    if !parsed.small_passive_stats.is_empty() {
        let want = &parsed.small_passive_stats;
        // Prefer the matching jewel's category first (size-specific stats
        // disambiguate "Damage" between Small / Medium / Large jewels).
        let order: Vec<&str> = std::iter::once(size_key_str(&parsed.size))
            .chain([
                "Large Cluster Jewel",
                "Medium Cluster Jewel",
                "Small Cluster Jewel",
            ])
            .collect();
        let mut seen = std::collections::HashSet::new();
        'outer: for key in order {
            if !seen.insert(key) {
                continue;
            }
            let Some(jewel) = catalogue.jewels.get(key) else {
                continue;
            };
            for (id, skill) in &jewel.skills {
                if skill.stats.iter().all(|s| want.contains(s))
                    && want.iter().all(|s| skill.stats.contains(s))
                {
                    parsed.skill_id = id.clone();
                    if parsed.skill_name.is_empty() {
                        parsed.skill_name = skill.name.clone();
                    }
                    break 'outer;
                }
            }
        }
    }

    // Fall back to the per-jewel `min_nodes` when no `Adds N Passive Skills`
    // line was present. This keeps the synthesis pass producing _something_
    // for rolled-blank jewels rather than emitting a zero-node ghost graph.
    if parsed.node_count == 0 {
        if let Some(def) = jewel_def {
            parsed.node_count = u32::from(def.min_nodes);
        }
    }

    // Cluster jewel notable rolls cap at 4 added passives upstream; clamp here
    // so a malformed paste with 5+ "Added Passive Skill is X" lines doesn't
    // overflow the ring slot allocation.
    if parsed.notables.len() > 4 {
        parsed.notables.truncate(4);
    }

    Some(parsed)
}

/// Map "Large" / "Medium" / "Small" to the catalogue map key.
fn size_key_str(size: &str) -> &'static str {
    match size {
        "Large" => "Large Cluster Jewel",
        "Medium" => "Medium Cluster Jewel",
        "Small" => "Small Cluster Jewel",
        _ => "Large Cluster Jewel",
    }
}

/// Recognise a `1 Added Passive Skill is <Notable Name>` line. Returns the
/// notable's display name. We deliberately reject the special `is a Jewel
/// Socket` variant — that's handled by `line_is_added_socket`.
fn line_is_added_notable(line: &str) -> Option<&str> {
    let rest = line.strip_prefix("1 Added Passive Skill is ")?;
    if rest == "a Jewel Socket" {
        return None;
    }
    Some(rest)
}

fn line_is_added_socket(line: &str) -> bool {
    line == "1 Added Passive Skill is a Jewel Socket"
}

/// Recognise `N Added Passive Skills are Jewel Sockets` (multi-socket
/// corruption-roll phrasing). Returns N.
fn line_is_added_sockets_count(line: &str) -> Option<u32> {
    let rest = line.strip_suffix(" Added Passive Skills are Jewel Sockets")?;
    rest.trim().parse().ok()
}

/// Recognise `Adds N Jewel Socket Passive Skills` (corruption-roll override
/// — `clusterJewelSocketCountOverride` upstream). Returns N.
fn line_is_added_sockets_override(line: &str) -> Option<u32> {
    let rest = line
        .strip_prefix("Adds ")?
        .strip_suffix(" Jewel Socket Passive Skills")?;
    rest.trim().parse().ok()
}

/// Recognise `Added Small Passive Skills have N% increased Effect`
/// (corruption-roll, `clusterJewelIncEffect` upstream). Returns N.
fn line_is_small_passive_inc_effect(line: &str) -> Option<u32> {
    let rest = line
        .strip_prefix("Added Small Passive Skills have ")?
        .strip_suffix("% increased Effect")?;
    rest.trim().parse().ok()
}

/// Recognise `Adds N Passive Skills` / `Adds 1 Passive Skill`. Returns N.
fn line_is_added_passives_count(line: &str) -> Option<u32> {
    let rest = line.strip_prefix("Adds ").and_then(|s| {
        s.strip_suffix(" Passive Skills")
            .or(s.strip_suffix(" Passive Skill"))
    })?;
    // Reject `Adds N Jewel Socket Passive Skills` etc. — only plain count.
    if rest.contains(' ') {
        return None;
    }
    rest.trim().parse().ok()
}

/// Recognise `Added Small Passive Skills grant: <stat>` or the
/// `Added Small Passive Skills also grant: <stat>` alt phrasing.
/// Returns `(skill_name_or_empty, stat_text)`. We don't actually have access
/// to the cluster-jewel skill name from this single line — return `""` and let
/// `parse_cluster_jewel` fill it in by matching against the catalogue afterwards.
fn line_is_small_passive_grant(line: &str) -> Option<(&str, &str)> {
    let stat = line
        .strip_prefix("Added Small Passive Skills grant: ")
        .or_else(|| line.strip_prefix("Added Small Passive Skills also grant: "))?;
    Some(("", stat))
}

/// Look up a cluster-jewel notable by its display name in the live tree.
/// PoB keeps these as orphan (`group = nil`) `notable` nodes so the synthesis
/// pass can copy their `stats` into the synthesised node verbatim.
///
/// Returns the `Node` template if found, otherwise `None` (caller emits a
/// notable with empty `stats` so the alloc / UI surfaces the missing-data
/// gracefully).
pub fn lookup_cluster_notable_template<'a>(
    tree: &'a PassiveTree,
    display_name: &str,
) -> Option<&'a Node> {
    tree.nodes.values().find(|n| {
        matches!(n.kind, NodeKind::Notable)
            && n.group.is_none()
            && n.name.as_deref() == Some(display_name)
    })
}

/// Build the synthesised sub-graph for a single Large jewel socket holding a
/// parsed cluster jewel. Returns `None` when:
///
/// * the catalogue doesn't define the jewel size (corrupt data file), or
/// * `parsed.node_count` is zero after parsing _and_ catalogue fallback
///   (nothing to synthesise — e.g. a Magic-rarity blank jewel).
///
/// Synthesis rules (slice 1):
/// * Sockets are placed at the slots in `ClusterJewelType.socket_indicies` up
///   to `parsed.socket_count`.
/// * Notables are placed at the next free slots of
///   `ClusterJewelType.notable_indicies`.
/// * Small passives fill remaining `ClusterJewelType.small_indicies`.
/// * The entrance is whichever node ended up at slot 0; if slot 0 was claimed
///   by a non-small node we still pick slot 0 so the graph stays connected.
/// * Edges form a chain following the slot order around the ring; the first
///   and last nodes also connect on non-Small jewels (closing the loop).
pub fn synthesise_for_socket(
    parent_socket: NodeId,
    parent_socket_idx: u32,
    parsed: &ParsedClusterJewel,
    catalogue: &ClusterJewelData,
    tree: &PassiveTree,
) -> Option<ClusterJewelSpec> {
    if parsed.node_count == 0 {
        return None;
    }
    let size_key = format!("{} Cluster Jewel", parsed.size);
    let jewel_def = catalogue.jewels.get(&size_key)?;
    let size_index = jewel_def.size_index;

    let socket_count = parsed
        .socket_count
        .min(jewel_def.socket_indicies.len() as u32);
    let notable_count = (parsed.notables.len() as u32).min(jewel_def.notable_indicies.len() as u32);
    let want_smalls = parsed
        .node_count
        .saturating_sub(socket_count)
        .saturating_sub(notable_count);

    let mut by_slot: HashMap<u8, SynthRole> = HashMap::new();

    // Pass 1: sockets — Large jewels with a single inner socket pin it at slot 6
    // per PoB. Otherwise consume from `socket_indicies`.
    if parsed.size == "Large" && socket_count == 1 {
        by_slot.insert(6, SynthRole::Socket);
    } else {
        for (i, &slot) in jewel_def.socket_indicies.iter().enumerate() {
            if i as u32 >= socket_count {
                break;
            }
            by_slot.insert(slot, SynthRole::Socket);
        }
    }

    // Pass 2: notables. PoB has a few special-case Medium re-mappings that we
    // ignore here — they only ever shift two slots and the result is
    // observationally equivalent for a calc-only pipeline.
    let mut notable_iter = parsed.notables.iter();
    for &slot in &jewel_def.notable_indicies {
        if let std::collections::hash_map::Entry::Vacant(e) = by_slot.entry(slot) {
            if let Some(name) = notable_iter.next() {
                e.insert(SynthRole::Notable(name.clone()));
            }
        }
    }
    // If there are more parsed notables than slots in `notable_indicies`,
    // place the overflow in any remaining small slot. PoB's "silently fail"
    // path covers this; we follow.
    for name in notable_iter {
        if let Some(&slot) = jewel_def
            .small_indicies
            .iter()
            .find(|s| !by_slot.contains_key(s))
        {
            by_slot.insert(slot, SynthRole::Notable(name.clone()));
        }
    }

    // Pass 3: smalls.
    let mut placed_smalls = 0u32;
    for &slot in &jewel_def.small_indicies {
        if placed_smalls >= want_smalls {
            break;
        }
        if let std::collections::hash_map::Entry::Vacant(e) = by_slot.entry(slot) {
            e.insert(SynthRole::Small);
            placed_smalls += 1;
        }
    }

    // Build the actual Nodes.
    let mut nodes: HashMap<NodeId, Node> = HashMap::new();
    let mut notable_ids: SmallVec<[NodeId; 4]> = SmallVec::new();
    let mut small_ids: SmallVec<[NodeId; 12]> = SmallVec::new();
    let mut socket_ids: SmallVec<[NodeId; 3]> = SmallVec::new();

    // Sort by slot so the result is deterministic.
    let mut slots: Vec<(u8, SynthRole)> = by_slot.into_iter().collect();
    slots.sort_by_key(|(slot, _)| *slot);

    let mut slot_to_id: HashMap<u8, NodeId> = HashMap::new();
    for (slot, role) in &slots {
        let id = make_synth_id(parent_socket_idx, size_index, *slot);
        let node = match role {
            SynthRole::Notable(name) => {
                let stats = lookup_cluster_notable_template(tree, name)
                    .map(|t| t.stats.clone())
                    .unwrap_or_default();
                let icon = lookup_cluster_notable_template(tree, name).and_then(|t| t.icon.clone());
                notable_ids.push(id);
                Node {
                    id,
                    name: Some(name.clone()),
                    icon,
                    ascendancy_name: None,
                    stats,
                    reminder_text: Vec::new(),
                    kind: NodeKind::Notable,
                    class_start_index: None,
                    group: None,
                    orbit: None,
                    orbit_index: None,
                    out_edges: SmallVec::new(),
                    in_edges: SmallVec::new(),
                    mastery_effects: Vec::new(),
                    expansion_jewel_size: None,
                    jewel_radius: None,
                }
            }
            SynthRole::Small => {
                let mut stats: Vec<String> = parsed
                    .small_passive_stats
                    .iter()
                    .map(|s| scale_stat_line(s, parsed.small_passive_inc_effect))
                    .collect();
                // `clusterJewelAddedMods` lines also scale with `clusterJewelIncEffect`
                // upstream — they share the small node's mod list so the
                // `PassiveSkillEffect INC N` mod scales them all uniformly.
                stats.extend(
                    parsed
                        .added_small_mods
                        .iter()
                        .map(|s| scale_stat_line(s, parsed.small_passive_inc_effect)),
                );
                small_ids.push(id);
                Node {
                    id,
                    name: Some(parsed.skill_name.clone()),
                    icon: None,
                    ascendancy_name: None,
                    stats,
                    reminder_text: Vec::new(),
                    kind: NodeKind::Normal,
                    class_start_index: None,
                    group: None,
                    orbit: None,
                    orbit_index: None,
                    out_edges: SmallVec::new(),
                    in_edges: SmallVec::new(),
                    mastery_effects: Vec::new(),
                    expansion_jewel_size: None,
                    jewel_radius: None,
                }
            }
            SynthRole::Socket => {
                socket_ids.push(id);
                Node {
                    id,
                    name: Some("Jewel Socket".into()),
                    icon: None,
                    ascendancy_name: None,
                    stats: Vec::new(),
                    reminder_text: Vec::new(),
                    kind: NodeKind::JewelSocket,
                    class_start_index: None,
                    group: None,
                    orbit: None,
                    orbit_index: None,
                    out_edges: SmallVec::new(),
                    in_edges: SmallVec::new(),
                    mastery_effects: Vec::new(),
                    expansion_jewel_size: Some(size_index.saturating_sub(1)),
                    jewel_radius: None,
                }
            }
        };
        slot_to_id.insert(*slot, id);
        nodes.insert(id, node);
    }

    // Build edges. Walk slot order around the ring (0..total_indicies) and
    // chain consecutive present nodes. Close the loop on non-Small jewels.
    let mut edges: Vec<(NodeId, NodeId)> = Vec::new();
    let total = u32::from(jewel_def.total_indicies);
    let mut prev_id: Option<NodeId> = None;
    let mut first_id: Option<NodeId> = None;
    let mut last_id: Option<NodeId> = None;
    for slot in 0..total as u8 {
        if let Some(&id) = slot_to_id.get(&slot) {
            if let Some(p) = prev_id {
                edges.push((p, id));
            }
            if first_id.is_none() {
                first_id = Some(id);
            }
            prev_id = Some(id);
            last_id = Some(id);
        }
    }
    if parsed.size != "Small" {
        if let (Some(f), Some(l)) = (first_id, last_id) {
            if f != l {
                edges.push((f, l));
            }
        }
    }

    // Entrance: prefer slot 0 if present, else the lowest-slot node we placed.
    let entrance = slot_to_id
        .get(&0u8)
        .copied()
        .or_else(|| slots.first().and_then(|(s, _)| slot_to_id.get(s).copied()))?;
    edges.push((parent_socket, entrance));

    // Reflect each edge onto the affected node's `in_edges`/`out_edges` so
    // pathfinding works without a second pass. Edges within the sub-graph
    // are bidirectional in PoB's tree (every edge is in both `in` and `out`
    // on each endpoint); we mirror that.
    for (a, b) in &edges {
        if let Some(node) = nodes.get_mut(a) {
            if !node.out_edges.contains(b) {
                node.out_edges.push(*b);
            }
            if !node.in_edges.contains(b) {
                node.in_edges.push(*b);
            }
        }
        if let Some(node) = nodes.get_mut(b) {
            if !node.out_edges.contains(a) {
                node.out_edges.push(*a);
            }
            if !node.in_edges.contains(a) {
                node.in_edges.push(*a);
            }
        }
    }

    Some(ClusterJewelSpec {
        parent_socket,
        jewel_size: parsed.size.clone(),
        nodes,
        edges,
        entrance,
        notable_ids,
        small_ids,
        socket_ids,
    })
}

#[derive(Debug, Clone)]
enum SynthRole {
    Notable(String),
    Small,
    Socket,
}

/// Scale every leading numeric value in `line` by `(1 + inc_pct/100)` and
/// re-emit the line. `inc_pct == 0` returns the line unchanged. Mirrors PoB's
/// `clusterJewelIncEffect` semantics — the `PassiveSkillEffect INC N` mod gets
/// applied to the small node's mod list, which uniformly scales every value
/// the node produces. Since MK2 doesn't yet have the
/// `PassiveSkillEffect`-INC-scales-co-located-mods machinery, we fold the
/// scalar in numerically at synthesis time.
///
/// Handles both integer and decimal forms (`5`, `5.5`) and signed values
/// (`+12`, `-3`). Returns the original string when no leading numeric is
/// recognised — e.g. flag-style lines like `"Cannot be Frozen"`.
fn scale_stat_line(line: &str, inc_pct: u32) -> String {
    if inc_pct == 0 {
        return line.to_string();
    }
    let scale = 1.0 + (f64::from(inc_pct) / 100.0);
    let bytes = line.as_bytes();
    // Find the first numeric run, optionally preceded by `+` / `-`.
    let mut i = 0;
    while i < bytes.len() && !bytes[i].is_ascii_digit() && bytes[i] != b'+' && bytes[i] != b'-' {
        i += 1;
    }
    if i == bytes.len() {
        return line.to_string();
    }
    let sign_start = i;
    if bytes[i] == b'+' || bytes[i] == b'-' {
        i += 1;
    }
    let num_start = i;
    while i < bytes.len() && (bytes[i].is_ascii_digit() || bytes[i] == b'.') {
        i += 1;
    }
    if num_start == i {
        return line.to_string();
    }
    let num_end = i;
    let raw: f64 = match line[num_start..num_end].parse() {
        Ok(v) => v,
        Err(_) => return line.to_string(),
    };
    let leading_sign = bytes[sign_start];
    let was_signed = leading_sign == b'+' || leading_sign == b'-';
    let signed_value = if leading_sign == b'-' { -raw } else { raw };
    let scaled = signed_value * scale;
    // Round half-up to one decimal place; emit integer when exact.
    let rounded = (scaled * 10.0).round() / 10.0;
    let abs_value = rounded.abs();
    let formatted = if (abs_value.fract()).abs() < 1e-9 {
        format!("{}", abs_value as i64)
    } else {
        format!("{abs_value}")
    };
    let mut out = String::with_capacity(line.len() + 4);
    out.push_str(&line[..sign_start]);
    if was_signed {
        if rounded < 0.0 {
            out.push('-');
        } else if leading_sign == b'+' {
            out.push('+');
        }
    } else if rounded < 0.0 {
        out.push('-');
    }
    out.push_str(&formatted);
    out.push_str(&line[num_end..]);
    out
}

/// Build a synthetic NodeId. Mirrors PoB's `BuildSubgraph` id scheme — see the
/// module-level comment for the bit layout. We use bits 6-8 for the parent
/// large-socket index because nested cluster recursion isn't supported in
/// this slice; the "medium-jewel index" bits (9-10) stay zero.
pub fn make_synth_id(parent_socket_idx: u32, size_index: u8, slot: u8) -> NodeId {
    let mut id = 0x0001_0000u32;
    id |= (parent_socket_idx & 0b111) << 6;
    id |= (u32::from(size_index) & 0b11) << 4;
    id |= u32::from(slot) & 0b1111;
    id
}

/// Synthesise sub-graphs for every Large jewel socket of `tree` that has a
/// cluster jewel mapped in `jewels`. Returns one `ClusterJewelSpec` per
/// socketed jewel.
///
/// `cluster_data` and `_mods` are the loaded `data/cluster_jewels.json` /
/// `data/cluster_jewel_mods.json` payloads (shared by all builds — passed
/// through here so wasm and tests can supply their own without I/O). The mods
/// payload is currently unused for synthesis (we read notable stats from the
/// tree's orphan-notable templates) but is retained on the API for a future
/// slice that handles corrupt-implicit (`Corrupted`-affix) mods which aren't
/// already on the parent item's mod_lines.
///
/// `parent_socket_idx` for each Large socket is determined by enumerating
/// `tree.nodes` filtered to `expansion_jewel_size == 2` in NodeId order — a
/// stable scheme that matches PoB's behaviour for our scope.
pub fn synthesise_all(
    tree: &PassiveTree,
    jewels: &HashMap<NodeId, Item>,
    cluster_data: &ClusterJewelData,
    _mods: &ClusterModSet,
) -> Vec<ClusterJewelSpec> {
    // Stable enumeration of the Large sockets so synthetic ids are
    // deterministic across runs.
    let mut large_sockets: Vec<NodeId> = tree
        .nodes
        .iter()
        .filter_map(|(id, n)| {
            if matches!(n.kind, NodeKind::JewelSocket) && n.expansion_jewel_size == Some(2) {
                Some(*id)
            } else {
                None
            }
        })
        .collect();
    large_sockets.sort_unstable();

    let mut out = Vec::new();
    for (idx, socket_id) in large_sockets.iter().enumerate() {
        let Some(item) = jewels.get(socket_id) else {
            continue;
        };
        let Some(parsed) = parse_cluster_jewel(item, cluster_data) else {
            continue;
        };
        if let Some(spec) =
            synthesise_for_socket(*socket_id, idx as u32, &parsed, cluster_data, tree)
        {
            out.push(spec);
        }
    }
    out
}

/// Helper used by `perform`: given the synthesised sub-graphs, return the
/// concrete set of synthesised nodes the engine should treat as
/// "available for allocation" / mod-bearing. The set is just the union of
/// every spec's `nodes`.
pub fn merged_synth_nodes(specs: &[ClusterJewelSpec]) -> HashMap<NodeId, &Node> {
    let mut out: HashMap<NodeId, &Node> = HashMap::new();
    for spec in specs {
        for (id, node) in &spec.nodes {
            out.insert(*id, node);
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use indexmap::IndexMap;
    use pob_data::{
        cluster_jewels::{ClusterJewelType, ClusterSkill},
        ClusterJewelData, Group, ModLine, ModSection, Node, NodeKind, PassiveTree, Rarity,
        TreeConstants,
    };

    fn small_jewel_data() -> ClusterJewelData {
        let mut skills = IndexMap::new();
        skills.insert(
            "affliction_maximum_life".into(),
            ClusterSkill {
                name: "Life".into(),
                icon: "Art/Life.png".into(),
                tag: "affliction_maximum_life".into(),
                stats: vec!["4% increased maximum Life".into()],
                enchant: vec!["Added Small Passive Skills grant: 4% increased maximum Life".into()],
            },
        );
        let mut jewels = IndexMap::new();
        jewels.insert(
            "Small Cluster Jewel".into(),
            ClusterJewelType {
                size: "Small".into(),
                size_index: 0,
                min_nodes: 2,
                max_nodes: 3,
                small_indicies: vec![0, 4, 2],
                notable_indicies: vec![4],
                socket_indicies: vec![4],
                total_indicies: 6,
                skills,
            },
        );
        // Fill in a Large entry so synthesis can find it.
        let mut large_skills = IndexMap::new();
        large_skills.insert(
            "affliction_chaos_damage".into(),
            ClusterSkill {
                name: "Chaos Damage".into(),
                icon: "Art/Chaos.png".into(),
                tag: "affliction_chaos_damage".into(),
                stats: vec!["12% increased Chaos Damage".into()],
                enchant: vec![
                    "Added Small Passive Skills grant: 12% increased Chaos Damage".into(),
                ],
            },
        );
        jewels.insert(
            "Large Cluster Jewel".into(),
            ClusterJewelType {
                size: "Large".into(),
                size_index: 2,
                min_nodes: 8,
                max_nodes: 12,
                small_indicies: vec![0, 4, 6, 8, 10, 2, 7, 5, 9, 3, 11, 1],
                notable_indicies: vec![6, 4, 8, 10, 2],
                socket_indicies: vec![4, 8, 6],
                total_indicies: 12,
                skills: large_skills,
            },
        );
        ClusterJewelData {
            jewels,
            notable_sort_order: IndexMap::new(),
            keystones: vec![],
            orbit_offsets: IndexMap::new(),
        }
    }

    fn make_tree() -> PassiveTree {
        // A minimal tree: one Large jewel socket node + an orphan notable
        // template the synthesis will look up.
        let mut nodes = ahash::HashMap::default();
        nodes.insert(
            1000,
            Node {
                id: 1000,
                name: Some("Large Jewel Socket".into()),
                icon: None,
                ascendancy_name: None,
                stats: vec![],
                reminder_text: vec![],
                kind: NodeKind::JewelSocket,
                class_start_index: None,
                group: Some(7),
                orbit: Some(0),
                orbit_index: Some(0),
                out_edges: SmallVec::new(),
                in_edges: SmallVec::new(),
                mastery_effects: vec![],
                expansion_jewel_size: Some(2),
                jewel_radius: None,
            },
        );
        nodes.insert(
            44237,
            Node {
                id: 44237,
                name: Some("Prodigious Defence".into()),
                icon: Some("Art/Prodigious.png".into()),
                ascendancy_name: None,
                stats: vec![
                    "30% increased Attack Damage while holding a Shield".into(),
                    "+4% Chance to Block Attack Damage".into(),
                ],
                reminder_text: vec![],
                kind: NodeKind::Notable,
                class_start_index: None,
                group: None, // orphan = cluster template
                orbit: None,
                orbit_index: None,
                out_edges: SmallVec::new(),
                in_edges: SmallVec::new(),
                mastery_effects: vec![],
                expansion_jewel_size: None,
                jewel_radius: None,
            },
        );
        let mut groups = ahash::HashMap::default();
        groups.insert(
            7,
            Group {
                x: 0.0,
                y: 0.0,
                orbits: SmallVec::new(),
                background: None,
                nodes: vec![1000],
                is_proxy: false,
            },
        );
        PassiveTree {
            version: "test".into(),
            tree: "Default".into(),
            classes: vec![],
            groups,
            nodes,
            jewel_slots: vec![1000],
            min_x: 0,
            min_y: 0,
            max_x: 0,
            max_y: 0,
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

    fn cluster_jewel_item(notable: &str, smalls: u32, sockets: u32) -> Item {
        let mut mod_lines = vec![ModLine {
            line: format!("Adds {smalls} Passive Skills"),
            section: ModSection::Enchant,
            variant_list: None,
        }];
        for _ in 0..sockets {
            mod_lines.push(ModLine {
                line: "1 Added Passive Skill is a Jewel Socket".into(),
                section: ModSection::Enchant,
                variant_list: None,
            });
        }
        if !notable.is_empty() {
            mod_lines.push(ModLine {
                line: format!("1 Added Passive Skill is {notable}"),
                section: ModSection::Enchant,
                variant_list: None,
            });
        }
        mod_lines.push(ModLine {
            line: "Added Small Passive Skills grant: 12% increased Chaos Damage".into(),
            section: ModSection::Enchant,
            variant_list: None,
        });
        Item {
            name: String::new(),
            base_name: "Large Cluster Jewel".into(),
            rarity: Rarity::Magic,
            item_level: 84,
            quality: 0,
            tags: ahash::HashSet::default(),
            mod_lines,
            sockets: String::new(),
            raw: String::new(),
            corrupted: false,
            mirrored: false,
            variants: Vec::new(),
            variant: None,
        }
    }

    #[test]
    fn parses_cluster_jewel_metadata() {
        let item = cluster_jewel_item("Prodigious Defence", 8, 1);
        let data = small_jewel_data();
        let parsed = parse_cluster_jewel(&item, &data).expect("parses");
        assert_eq!(parsed.size, "Large");
        assert_eq!(parsed.notables, vec!["Prodigious Defence".to_owned()]);
        assert_eq!(parsed.socket_count, 1);
        assert_eq!(parsed.node_count, 8);
        // 8 - 1 notable - 1 socket = 6 smalls
        assert_eq!(parsed.small_count(), 6);
        assert_eq!(parsed.skill_name, "Chaos Damage");
        assert_eq!(parsed.small_passive_stats.len(), 1);
        assert_eq!(parsed.small_passive_stats[0], "12% increased Chaos Damage");
    }

    #[test]
    fn synthesises_large_jewel_sub_graph() {
        let tree = make_tree();
        let item = cluster_jewel_item("Prodigious Defence", 8, 1);
        let data = small_jewel_data();
        let parsed = parse_cluster_jewel(&item, &data).expect("parses");
        let spec = synthesise_for_socket(1000, 0, &parsed, &data, &tree).expect("synthesised");
        assert_eq!(spec.parent_socket, 1000);
        // 8 nodes total: 1 notable, 1 socket, 6 smalls
        assert_eq!(spec.nodes.len(), 8);
        assert_eq!(spec.notable_ids.len(), 1);
        assert_eq!(spec.socket_ids.len(), 1);
        assert_eq!(spec.small_ids.len(), 6);

        // The notable should carry the template's stats.
        let notable = &spec.nodes[&spec.notable_ids[0]];
        assert_eq!(notable.name.as_deref(), Some("Prodigious Defence"));
        assert!(notable
            .stats
            .iter()
            .any(|s| s.contains("Attack Damage while holding a Shield")));

        // Every small should carry the small-passive grant text.
        for &id in &spec.small_ids {
            let node = &spec.nodes[&id];
            assert!(node
                .stats
                .iter()
                .any(|s| s.contains("12% increased Chaos Damage")));
        }

        // Edge from parent socket to the entrance.
        assert!(spec
            .edges
            .iter()
            .any(|(a, b)| *a == 1000 && *b == spec.entrance));
    }

    #[test]
    fn synthesises_minimum_node_count_when_adds_line_missing() {
        // No `Adds N Passive Skills` line at all — fall back to the catalogue's
        // `min_nodes`. Not a realistic in-game item but a useful resilience
        // check for partial pastes.
        let mut item = cluster_jewel_item("", 0, 0);
        item.mod_lines.retain(|m| !m.line.starts_with("Adds "));
        let data = small_jewel_data();
        let parsed = parse_cluster_jewel(&item, &data).expect("parses");
        // Large.min_nodes = 8 in the test catalogue.
        assert_eq!(parsed.node_count, 8);
    }

    #[test]
    fn synthetic_ids_dont_collide_with_tree_nodes() {
        let tree = make_tree();
        let id = make_synth_id(0, 2, 0);
        assert!(!tree.nodes.contains_key(&id));
        let id = make_synth_id(5, 2, 11);
        assert!(!tree.nodes.contains_key(&id));
    }

    #[test]
    fn synthesise_all_skips_sockets_without_jewels() {
        let tree = make_tree();
        let data = small_jewel_data();
        let mods = pob_data::cluster_jewel_mods::ClusterModSet::default();
        let jewels: HashMap<NodeId, Item> = HashMap::new();
        let specs = synthesise_all(&tree, &jewels, &data, &mods);
        assert!(specs.is_empty());

        let mut jewels = HashMap::new();
        jewels.insert(1000u32, cluster_jewel_item("Prodigious Defence", 8, 1));
        let specs = synthesise_all(&tree, &jewels, &data, &mods);
        assert_eq!(specs.len(), 1);
        assert_eq!(specs[0].parent_socket, 1000);
    }

    // Tests for the engine integration live in `perform.rs` test module.

    /// Issue #197 (slice C): corruption-roll `+1 socket` should bump the
    /// synthesised socket count by one without affecting the count of smalls
    /// the parser otherwise produces. This is the canonical acceptance
    /// criterion from the issue body — pasting a corrupted cluster jewel
    /// that rolled `1 Added Passive Skill is a Jewel Socket` (the corruption
    /// implicit form) must spawn one extra socket in the synth sub-graph.
    #[test]
    fn corruption_extra_socket_bumps_synth_socket_count() {
        let tree = make_tree();
        // Base jewel: 8 nodes, 1 notable, no rolled sockets. The corruption
        // implicit then adds a socket on top.
        let mut item = cluster_jewel_item("Prodigious Defence", 8, 0);
        item.corrupted = true;
        item.mod_lines.push(pob_data::ModLine {
            line: "1 Added Passive Skill is a Jewel Socket".into(),
            section: pob_data::ModSection::Corrupted,
            variant_list: None,
        });
        let data = small_jewel_data();
        let parsed = parse_cluster_jewel(&item, &data).expect("parses");
        assert_eq!(parsed.socket_count, 1, "corruption rolled +1 socket");
        let spec = synthesise_for_socket(1000, 0, &parsed, &data, &tree).expect("synthesised");
        assert_eq!(spec.socket_ids.len(), 1, "+1 socket → one synth socket");
    }

    /// Issue #197 (slice C): corruption-roll `2 Added Passive Skills are
    /// Jewel Sockets` (multi-socket alt phrasing) and `Adds N Jewel Socket
    /// Passive Skills` (override phrasing). Both should bump socket_count.
    #[test]
    fn corruption_multi_socket_phrasings_recognised() {
        let mut item1 = cluster_jewel_item("", 8, 0);
        item1.mod_lines.push(pob_data::ModLine {
            line: "2 Added Passive Skills are Jewel Sockets".into(),
            section: pob_data::ModSection::Corrupted,
            variant_list: None,
        });
        let data = small_jewel_data();
        let parsed1 = parse_cluster_jewel(&item1, &data).expect("parses");
        assert_eq!(parsed1.socket_count, 2);

        let mut item2 = cluster_jewel_item("", 8, 0);
        item2.mod_lines.push(pob_data::ModLine {
            line: "Adds 3 Jewel Socket Passive Skills".into(),
            section: pob_data::ModSection::Corrupted,
            variant_list: None,
        });
        let parsed2 = parse_cluster_jewel(&item2, &data).expect("parses");
        assert_eq!(parsed2.socket_count, 3);
    }

    /// Issue #197 (slice C): corruption-roll `Added Small Passive Skills
    /// have N% increased Effect`. Stored on `small_passive_inc_effect` and
    /// scales every numeric value in the small's stat lines uniformly.
    #[test]
    fn corruption_inc_effect_scales_small_stat_lines() {
        let tree = make_tree();
        let mut item = cluster_jewel_item("", 8, 0);
        item.mod_lines.push(pob_data::ModLine {
            line: "Added Small Passive Skills have 50% increased Effect".into(),
            section: pob_data::ModSection::Corrupted,
            variant_list: None,
        });
        let data = small_jewel_data();
        let parsed = parse_cluster_jewel(&item, &data).expect("parses");
        assert_eq!(parsed.small_passive_inc_effect, 50);
        let spec = synthesise_for_socket(1000, 0, &parsed, &data, &tree).expect("synthesised");
        // Each small originally grants `12% increased Chaos Damage`; with
        // +50% effect we expect `18% increased Chaos Damage`.
        let small_id = spec.small_ids[0];
        let small = &spec.nodes[&small_id];
        let line = small
            .stats
            .iter()
            .find(|s| s.contains("Chaos Damage"))
            .expect("small has chaos damage line");
        assert_eq!(line, "18% increased Chaos Damage");
    }

    #[test]
    fn scale_stat_line_handles_signed_and_decimal() {
        // No-op when inc=0.
        assert_eq!(
            scale_stat_line("12% increased Life", 0),
            "12% increased Life"
        );
        // Plain integer.
        assert_eq!(
            scale_stat_line("12% increased Life", 50),
            "18% increased Life"
        );
        // Signed positive.
        assert_eq!(scale_stat_line("+10 to Strength", 50), "+15 to Strength");
        // Signed negative — corruption could stack with negative-effect mods.
        assert_eq!(scale_stat_line("-10 to Strength", 50), "-15 to Strength");
        // Decimal that stays decimal after scale.
        assert_eq!(scale_stat_line("0.5% Life Regen", 50), "0.8% Life Regen");
        // Non-numeric leading content (flag mod): unchanged.
        assert_eq!(scale_stat_line("Cannot be Frozen", 50), "Cannot be Frozen");
    }
}
