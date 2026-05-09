//! Radius-jewel framework — issue #31.
//!
//! This module owns the generic mechanic that lets a jewel socketed in a passive
//! tree socket modify the passive nodes inside its radius. It does **not** implement
//! the timeless-jewel notable/keystone replacement (issue #30) or the cluster-jewel
//! sub-graph synthesis (issue #21); those will plug into the same handler-dispatch
//! surface in follow-ups.
//!
//! ## What's covered
//!
//! 1. Computing the Cartesian position of any tree node — mirrors PoB's
//!    `Classes/PassiveTree.lua:828-833` (group `(x, y)` + orbit-radius +
//!    orbit-index angle).
//! 2. Enumerating allocated passives within a jewel-socket's radius for a chosen
//!    radius bucket (Small/Medium/Large/Very Large/Massive plus PoB's "Variable"
//!    donut bands).
//! 3. Identifying a socketed item as a "vanilla" radius jewel by its mod text
//!    (lines that mention `Passives in Radius` and an explicit ring size like
//!    `in Small Ring`, `in Medium Ring`, `in Large Ring`).
//! 4. Applying a radius jewel's mods to the env: each in-radius allocated passive
//!    receives one copy of every mod line on the jewel, sourced as
//!    `Source::Passive(<that node's id>)` so the per-node breakdown attributes the
//!    bonus to the in-radius node and a `RadiusJewel:<base>` source label so the
//!    Calcs-tab can find it.
//!
//! ## What's deferred
//!
//! - Timeless jewels (#30): keystone / notable substitution.
//! - Cluster jewel sub-graph synthesis (#21): nodes spawned by a Cluster jewel.
//! - Per-handler closures for named uniques (Watcher's Eye, Healthy Mind, Karui
//!   Heart, Pure Talent, Intuitive Leap, Conqueror's Efficiency, …). The handler
//!   dispatch table is wired up here and a default "Self" handler covers vanilla
//!   passive-multiplying jewels; named uniques attach via [`HandlerKind`].

use ahash::{AHashMap, AHashSet};
use pob_data::{
    radii_for_tree_version, radius_index_for_label, Item, JewelRadiusInfo, NodeId, NodeKind,
    PassiveTree,
};
use serde::{Deserialize, Serialize};

use crate::mod_parser::parse_mod_line;
use crate::modifier::{Mod, Source};

/// Compute the Cartesian position of a tree node, in the same coordinate space the
/// passive tree's `(min_x, min_y)..(max_x, max_y)` rect uses. Mirrors
/// `PassiveTree.lua:828-833`:
///
/// ```text
///   x = group.x + sin(angle) * orbit_radii[orbit]
///   y = group.y - cos(angle) * orbit_radii[orbit]
/// ```
///
/// Returns `None` for nodes that lack a group/orbit/orbit_index (cluster-jewel
/// notable templates that haven't been placed yet, the synthetic root, etc.).
pub fn node_position(tree: &PassiveTree, node_id: NodeId) -> Option<(f64, f64)> {
    let node = tree.nodes.get(&node_id)?;
    let group = tree.groups.get(&node.group?)?;
    let orbit = node.orbit.unwrap_or(0) as usize;
    let orbit_index = node.orbit_index.unwrap_or(0) as usize;
    let radius = *tree.constants.orbit_radii.get(orbit).unwrap_or(&0) as f64;
    let nodes_in_orbit = tree
        .constants
        .skills_per_orbit
        .get(orbit)
        .copied()
        .unwrap_or(1);
    let angle = orbit_angle_rad(nodes_in_orbit, orbit_index);
    let x = f64::from(group.x) + angle.sin() * radius;
    let y = f64::from(group.y) - angle.cos() * radius;
    Some((x, y))
}

/// Per-orbit angle table. Mirrors `PassiveTree.lua:CalcOrbitAngles`. The 16- and
/// 40-slot orbits use bespoke degree tables; everything else is evenly spaced.
fn orbit_angle_rad(nodes_in_orbit: u32, orbit_index: usize) -> f64 {
    const TABLE_16: [f64; 16] = [
        0.0, 30.0, 45.0, 60.0, 90.0, 120.0, 135.0, 150.0, 180.0, 210.0, 225.0, 240.0, 270.0, 300.0,
        315.0, 330.0,
    ];
    const TABLE_40: [f64; 40] = [
        0.0, 10.0, 20.0, 30.0, 40.0, 45.0, 50.0, 60.0, 70.0, 80.0, 90.0, 100.0, 110.0, 120.0,
        130.0, 135.0, 140.0, 150.0, 160.0, 170.0, 180.0, 190.0, 200.0, 210.0, 220.0, 225.0, 230.0,
        240.0, 250.0, 260.0, 270.0, 280.0, 290.0, 300.0, 310.0, 315.0, 320.0, 330.0, 340.0, 350.0,
    ];
    let deg = match nodes_in_orbit {
        16 if orbit_index < 16 => TABLE_16[orbit_index],
        40 if orbit_index < 40 => TABLE_40[orbit_index],
        n if n > 0 => 360.0 * (orbit_index as f64) / f64::from(n),
        _ => 0.0,
    };
    deg.to_radians()
}

/// Result of an in-radius scan: every node id that falls inside the chosen radius
/// band, keyed by node id, with the squared distance from the socket. The squared
/// distance lets callers further bucketise when a jewel cares about narrower bands
/// (e.g. timeless jewels' inner/outer pair).
pub fn nodes_in_radius(
    tree: &PassiveTree,
    socket_id: NodeId,
    radius: &JewelRadiusInfo,
) -> Vec<(NodeId, f64)> {
    let Some((sx, sy)) = node_position(tree, socket_id) else {
        return Vec::new();
    };
    let outer_sq = radius.outer * radius.outer;
    let inner_sq = radius.inner * radius.inner;
    let mut out: Vec<(NodeId, f64)> = Vec::new();
    for (id, node) in &tree.nodes {
        // PoB skips the socket itself, mastery nodes, and proxy/blighted nodes when
        // building `nodesInRadius`. Mirror that — we don't want a jewel to inject
        // its own mods into itself, and mastery effects come from the player's
        // `mastery_selections` not the jewel.
        if *id == socket_id {
            continue;
        }
        if matches!(
            node.kind,
            NodeKind::Mastery | NodeKind::Root | NodeKind::ClassStart | NodeKind::AscendancyStart
        ) {
            continue;
        }
        let Some((x, y)) = node_position(tree, *id) else {
            continue;
        };
        let dx = x - sx;
        let dy = y - sy;
        let dist_sq = dx * dx + dy * dy;
        if dist_sq >= inner_sq && dist_sq <= outer_sq {
            out.push((*id, dist_sq));
        }
    }
    out
}

/// Filter [`nodes_in_radius`] down to nodes the character has actually allocated.
/// PoB's first-pass radius dispatch (the "Self" handler) only modifies allocated
/// nodes; nearby unallocated nodes feed the second-pass / extra-node list, which
/// vanilla node-modifying jewels don't drive.
pub fn allocated_nodes_in_radius(
    tree: &PassiveTree,
    socket_id: NodeId,
    radius: &JewelRadiusInfo,
    allocated: &AHashSet<NodeId>,
) -> Vec<(NodeId, f64)> {
    nodes_in_radius(tree, socket_id, radius)
        .into_iter()
        .filter(|(id, _)| allocated.contains(id))
        .collect()
}

/// Handler kind. Mirrors PoB's `radiusJewelList[i].type` field. We only implement
/// `SelfAllocated` (PoB's `"Self"`) for vanilla node-modifying jewels in this slice;
/// the other variants exist as named placeholders so timeless / cluster /
/// intuitive-leap / Watcher's-Eye follow-ups can plug in cleanly.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum HandlerKind {
    /// Apply the jewel's mods to every allocated passive in radius, copying each
    /// mod once per in-radius node so per-node breakdowns sum up correctly.
    /// Mirrors PoB's `"Self"` handler.
    SelfAllocated,
    /// Apply to every node in radius regardless of allocation. Used by jewels
    /// like Lethal Pride (which conquers the whole radius, allocated or not).
    /// Reserved for the timeless follow-up.
    All,
    /// Apply once globally per allocated passive matching a threshold (e.g.
    /// "with at least 40 Strength in Radius, X"). The radius pass tallies the
    /// triggering stat and emits the conditional mod once. Reserved for the
    /// threshold-jewel follow-up.
    Threshold,
    /// Apply to nearby unallocated passives — used by conversion jewels that
    /// transform an *unallocated* node's mod set. Reserved for follow-up.
    SelfUnalloc,
    /// Cross-cutting "any node in radius can be reached / treated specially" —
    /// for Intuitive-Leap-like behaviour. Doesn't itself emit mods; flips a
    /// pathfinder bit. Reserved.
    Pathfinder,
}

/// One radius jewel ready to be applied. Owns the parsed mod list and the radius
/// band the jewel claims. Construct via [`identify_radius_jewel`].
#[derive(Debug, Clone)]
pub struct RadiusJewel {
    /// Tree-socket node id the jewel is socketed into.
    pub socket_id: NodeId,
    /// Chosen radius band — typically Small/Medium/Large depending on the jewel's
    /// `Only affects Passives in <Size> Ring` text or PoB's per-base default.
    pub radius: JewelRadiusInfo,
    /// PoB's radius-index for this band (0 = Small, 1 = Medium, …). Useful for the
    /// Calcs-tab breakdown and for follow-ups that need to round-trip with PoB.
    pub radius_index: usize,
    /// Mods parsed off the jewel item that should be replayed per in-radius node.
    /// Each mod is sourceless on this struct; callers re-tag with
    /// `Source::Passive(node_id)` when applying.
    pub mods: Vec<Mod>,
    /// Display label for breakdown attribution (`"RadiusJewel:Crimson Jewel"`).
    pub source_label: String,
    /// Handler kind. Currently always [`HandlerKind::SelfAllocated`] for
    /// framework-level dispatch; named uniques will swap this out.
    pub kind: HandlerKind,
}

/// Identify whether `item` is a node-modifying radius jewel and, if so, build the
/// [`RadiusJewel`] descriptor that drives application.
///
/// Heuristic for this slice:
///
/// * The item's base name contains `Jewel` (matches Crimson/Viridian/Cobalt/Prismatic
///   Jewels and similarly-named uniques like Searching Eye, Healthy Mind, …).
/// * At least one of the jewel's mod lines mentions `Passives in Radius`,
///   `nearby allocated passives`, or `nodes in Radius` — that's PoB's canonical
///   marker for a radius effect. The chosen radius defaults to Medium (the most
///   common vanilla bucket); explicit `Only affects Passives in <Size> Ring` text,
///   when present, overrides.
/// * Cluster jewels (`subType = Cluster`), Abyss jewels, and timeless jewels
///   (`subType = Timeless`) are intentionally **not** picked up here — they have
///   dedicated dispatch paths (#21, #30) that consume the same radius primitives.
///
/// Returns `None` for items the framework should ignore.
pub fn identify_radius_jewel(socket_id: NodeId, item: &Item) -> Option<RadiusJewel> {
    if !is_jewel_base(&item.base_name) {
        return None;
    }
    if is_special_jewel_subtype(item) {
        return None;
    }
    // Walk mod lines to find a radius marker. We look at all mod sections so a
    // crafted-only or implicit-only radius mod still triggers identification.
    let mut radius_label: Option<&'static str> = None;
    let mut has_radius_text = false;
    for ml in &item.mod_lines {
        let line = ml.line.as_str();
        if mentions_radius(line) {
            has_radius_text = true;
        }
        if let Some(label) = explicit_ring_label(line) {
            radius_label = Some(label);
        }
    }
    if !has_radius_text && radius_label.is_none() {
        return None;
    }
    // Default to Medium when the jewel's text doesn't pin a ring size — that's the
    // canonical vanilla node-modifying-jewel bucket (Viridian / Crimson / Cobalt
    // base default). PoB encodes per-base defaults inside the bases data; once we
    // surface that we'll prefer the per-base value here.
    let label = radius_label.unwrap_or("Medium");
    let idx = radius_index_for_label(label)?;
    let radii = radii_for_tree_version(&item_tree_version(item));
    let radius = *radii.get(idx)?;

    // Parse every line into mods. Strip the trailing "in Radius" / "to allocated
    // passives" / etc. metadata from each line first — those phrases tell the
    // framework *where* to apply the mod, not *what* the mod is. Without
    // stripping, `mod_parser` mints PoB-style suffixed keys
    // (`MaximumLifeToNearbyAllocatedPassives`) that no calc consumer reads.
    let mut mods = Vec::with_capacity(item.mod_lines.len());
    for ml in &item.mod_lines {
        let raw = ml.line.as_str();
        // Skip the metadata-only `Only affects Passives in <Size> Ring` line — it's
        // not a real bonus, just a radius selector. PoB models it as a `JewelData`
        // LIST mod that we don't use yet.
        if explicit_ring_label(raw).is_some() {
            continue;
        }
        // Skip `<n> Added Passive Skills are Jewels` (cluster jewels handled separately).
        if raw.contains("Added Passive Skills are Jewels") {
            continue;
        }
        let stripped = strip_radius_suffix(raw);
        // If stripping leaves nothing parseable (the line was *only* a metadata
        // marker), fall back to parsing the original — `mod_parser` is the source
        // of truth for "is this a real mod".
        let target = stripped.as_deref().unwrap_or(raw);
        if let Some(parsed) = parse_mod_line(target) {
            mods.push(parsed.mod_);
        } else if stripped.is_some() {
            // Stripping changed the text but we couldn't parse the result; try the
            // original line in case the parser handles the long form directly.
            if let Some(parsed) = parse_mod_line(raw) {
                mods.push(parsed.mod_);
            }
        }
    }
    if mods.is_empty() {
        return None;
    }

    let source_label = format!("RadiusJewel:{}", item.base_name);
    Some(RadiusJewel {
        socket_id,
        radius,
        radius_index: idx,
        mods,
        source_label,
        kind: HandlerKind::SelfAllocated,
    })
}

fn is_jewel_base(base: &str) -> bool {
    // Exclude bases that *contain* "Jewel" but aren't actually jewels — defensive
    // against future base names. The current set of jewel bases all literally end
    // with "Jewel" or are eye jewels.
    if base.is_empty() {
        return false;
    }
    base.contains("Jewel")
        || base.ends_with("Eye Jewel")
        || base.contains("Eye Jewel")
        || base == "Crimson Jewel"
        || base == "Viridian Jewel"
        || base == "Cobalt Jewel"
        || base == "Prismatic Jewel"
}

/// Cluster / Abyss / Timeless / Charm jewels follow their own dispatch path. We bail
/// out of the generic framework when the base name flags one of those subtypes —
/// for now the heuristic uses the base-name suffix; a future slice will cross-check
/// against `bases.json`'s `sub_type` field.
fn is_special_jewel_subtype(item: &Item) -> bool {
    let n = &item.base_name;
    n.ends_with("Cluster Jewel")
        || n.contains("Abyss")
        || n.contains("Eye Jewel") // Abyss eye-jewel bases (Murderous/Searching/...)
        || matches!(
            n.as_str(),
            "Timeless Jewel"
                | "Lethal Pride"
                | "Brutal Restraint"
                | "Glorious Vanity"
                | "Elegant Hubris"
                | "Militant Faith"
        )
        || n.starts_with("Grand Spectrum")
            && n.contains("Charm")
        || n.contains("Charm")
}

fn mentions_radius(line: &str) -> bool {
    let l = line.to_ascii_lowercase();
    l.contains("passives in radius")
        || l.contains("nearby allocated passives")
        || l.contains("nodes in radius")
        || l.contains("in radius are")
}

fn explicit_ring_label(line: &str) -> Option<&'static str> {
    let l = line.to_ascii_lowercase();
    // PoB canonical text: "Only affects Passives in Small/Medium/Large/Very Large/Massive Ring".
    if !l.contains("only affects passives in") {
        return None;
    }
    if l.contains("massive") {
        Some("Massive")
    } else if l.contains("very large") {
        Some("Very Large")
    } else if l.contains("large") {
        Some("Large")
    } else if l.contains("medium") {
        Some("Medium")
    } else if l.contains("small") {
        Some("Small")
    } else {
        None
    }
}

/// PoB tree version string the radii table should be picked from. For now this is
/// always the modern (3.16+) default — items don't carry their tree version. A
/// future cluster / timeless slice will plumb the active tree version through.
fn item_tree_version(_item: &Item) -> String {
    "3_16".to_string()
}

/// Trim the radius-jewel metadata trailer off a mod line. When a vanilla radius
/// jewel says e.g. `10% increased Maximum Life to nearby allocated passives`, the
/// `to nearby allocated passives` part is a marker for the framework that the mod
/// should be applied per in-radius allocated node — it shouldn't end up in the
/// canonical mod-name. Stripping the trailer lets [`parse_mod_line`] mint the
/// regular `Life` / `MovementSpeed` / etc. keys instead of long-form suffixed
/// aliases that no calc consumer reads.
///
/// Returns `None` when the input has no recognised trailer (i.e. it's already a
/// plain mod line, or the trailer pattern doesn't match).
fn strip_radius_suffix(line: &str) -> Option<String> {
    // Patterns are listed longest-first so a line that contains multiple matches
    // strips the most specific one. Lower-case comparison keeps the match
    // case-insensitive; we splice using the original byte offset so casing in
    // the surviving prefix is preserved.
    const PATTERNS: &[&str] = &[
        " to nearby allocated passives",
        " to all allocated passives in Radius",
        " to allocated Passives in Radius",
        " to Passives in Radius",
        " from Passives in Radius",
        " from allocated Passives in Radius",
        " from nearby allocated passives",
        " for each allocated passive in radius",
        " in Radius",
    ];
    let lower = line.to_ascii_lowercase();
    for pat in PATTERNS {
        let pat_lc = pat.to_ascii_lowercase();
        if let Some(pos) = lower.find(&pat_lc) {
            // Slice the original string at the matched byte offset.
            let head = &line[..pos];
            let tail = &line[pos + pat.len()..];
            // Recombine head + any remaining trailing text (typically nothing,
            // sometimes a trailing comma).
            let mut out = head.trim_end().to_string();
            let trail = tail.trim_start();
            if !trail.is_empty() {
                out.push(' ');
                out.push_str(trail);
            }
            return Some(out);
        }
    }
    None
}

/// Per-character socketed-jewel storage. Maps tree-socket node id → jewel item.
/// Lives next to `Character` rather than on `ItemSet` because the [`Slot`] enum is
/// fixed-arity (Helmet/BodyArmour/…) and the tree exposes 60 jewel sockets, plus
/// timeless / cluster / abyss sockets we'll synthesise later.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct SocketedJewels {
    /// Wire-format-friendly Vec — stable insertion order for round-trip.
    #[serde(default)]
    pub entries: Vec<(NodeId, Item)>,
}

impl SocketedJewels {
    pub fn new() -> Self {
        Self::default()
    }

    /// Socket `item` into `node_id`, replacing any existing jewel.
    pub fn socket(&mut self, node_id: NodeId, item: Item) {
        if let Some(slot) = self.entries.iter_mut().find(|(id, _)| *id == node_id) {
            slot.1 = item;
        } else {
            self.entries.push((node_id, item));
        }
    }

    /// Remove the jewel at `node_id`. Returns the unsocketed item.
    pub fn unsocket(&mut self, node_id: NodeId) -> Option<Item> {
        if let Some(pos) = self.entries.iter().position(|(id, _)| *id == node_id) {
            Some(self.entries.remove(pos).1)
        } else {
            None
        }
    }

    pub fn get(&self, node_id: NodeId) -> Option<&Item> {
        self.entries
            .iter()
            .find(|(id, _)| *id == node_id)
            .map(|(_, it)| it)
    }

    pub fn iter(&self) -> impl Iterator<Item = (&NodeId, &Item)> {
        self.entries.iter().map(|(id, it)| (id, it))
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }
}

/// Apply every socketed radius jewel's mods to the env. Each in-radius allocated
/// node receives one copy of every parsed mod line on the jewel; copies are sourced
/// as `Source::Passive(<node_id>)` so per-node breakdown attribution treats the
/// bonus as if it lived on that passive itself, mirroring PoB's
/// `buildModListForNode` "Self" pass.
///
/// Returns the number of (jewel, node) emissions performed, suitable for tests and
/// for callers that want a quick diagnostic ("X mods applied across Y radius
/// jewels"). Skips invalid socket node ids and silently drops jewels that don't
/// identify as radius jewels via [`identify_radius_jewel`].
pub fn apply_radius_jewels(
    tree: &PassiveTree,
    allocated: &AHashSet<NodeId>,
    socketed: &SocketedJewels,
    db: &mut crate::ModDB,
) -> RadiusJewelReport {
    let mut report = RadiusJewelReport::default();
    for (socket_id, item) in socketed.iter() {
        let Some(jewel) = identify_radius_jewel(*socket_id, item) else {
            report.skipped += 1;
            continue;
        };
        report.applied_jewels += 1;
        let in_radius = allocated_nodes_in_radius(tree, *socket_id, &jewel.radius, allocated);
        for (target_node, _) in in_radius {
            for m in &jewel.mods {
                let mut clone = m.clone();
                clone.source = Some(Source::Passive(target_node));
                db.add(clone);
                report.mod_emissions += 1;
            }
        }
    }
    report
}

/// Diagnostic summary returned by [`apply_radius_jewels`].
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct RadiusJewelReport {
    /// Number of socketed jewels successfully identified as radius jewels.
    pub applied_jewels: usize,
    /// Number of socketed jewels skipped because they didn't identify as a radius
    /// jewel (cluster / abyss / timeless / non-radius rare jewel).
    pub skipped: usize,
    /// Total per-(jewel, node) mod copies emitted into the modDB.
    pub mod_emissions: usize,
}

/// Issue #196: apply mods from socketed jewels that don't identify as radius
/// jewels and aren't a special subtype (cluster / abyss / timeless / charm).
/// Mirrors PoB's behaviour where any socketed jewel still contributes its
/// item-level stats to the global modDB — only the *radius-conditional* piece
/// is per-allocated-node. Without this, modern unique jewels whose entire
/// effect is global (Conqueror's Efficiency / Conqueror's Potency / Conqueror's
/// Longevity, plus most rare-rolled Crimson / Viridian / Cobalt / Prismatic
/// jewels with no radius modifiers) silently drop everything they grant.
///
/// Subtype gates:
/// - Cluster jewels feed `cluster_synth` and shouldn't apply their item-level
///   "Adds N Passive Skills" lines globally — those are synthesis metadata.
/// - Abyss / Eye / Timeless / Charm jewels follow their own dispatch paths
///   (#30 timeless, future abyss / charm follow-ups). We bail here so this
///   helper doesn't double-apply mods that the dedicated path will own.
/// - Cluster jewels are also caught by [`is_special_jewel_subtype`] above.
///
/// Mods are sourced as `Source::Other("SocketedJewel:<base>:<socket_id>")` so
/// the Calcs-tab breakdown can attribute each mod back to the socketed item
/// that contributed it. Returns the number of mods successfully parsed and
/// added so callers can spot unparseable mod text in tests.
pub fn apply_non_radius_socketed_jewels(socketed: &SocketedJewels, db: &mut crate::ModDB) -> usize {
    let mut emitted = 0usize;
    for (socket_id, item) in socketed.iter() {
        if !is_jewel_base(&item.base_name) {
            continue;
        }
        if is_special_jewel_subtype(item) {
            continue;
        }
        // Identifiable as a radius jewel → already handled by
        // `apply_radius_jewels`. We only fill the gap for jewels whose
        // *entire* mod set is non-radius.
        if identify_radius_jewel(*socket_id, item).is_some() {
            continue;
        }
        let source = Source::Other(format!("SocketedJewel:{}:{}", item.base_name, socket_id));
        for ml in &item.mod_lines {
            // Defensive: skip the metadata-only ring-size selectors that some
            // hand-crafted jewels still ship (rare, but cheap to filter).
            if explicit_ring_label(&ml.line).is_some() {
                continue;
            }
            if let Some(parsed) = parse_mod_line(&ml.line) {
                let m = parsed.mod_.with_source(source.clone());
                db.add(m);
                emitted += 1;
            }
        }
    }
    emitted
}

/// Convenience wrapper: collect node positions for every node in `tree`. Useful
/// for UI / debug; the radius scan computes positions on demand.
pub fn all_node_positions(tree: &PassiveTree) -> AHashMap<NodeId, (f64, f64)> {
    let mut out: AHashMap<NodeId, (f64, f64)> = AHashMap::default();
    for id in tree.nodes.keys() {
        if let Some(p) = node_position(tree, *id) {
            out.insert(*id, p);
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use ahash::AHashSet;
    use pob_data::{
        item::{ModSection, Rarity},
        Group, ItemSet, ModLine, Node, NodeKind, PassiveTree, TreeConstants,
    };

    fn mk_tree() -> PassiveTree {
        // Two-node toy tree: a jewel socket at group (0, 0) orbit-0, and a normal
        // passive sitting 600 units to the right (orbit-2 of a 16-orbit group at
        // x=600, orbit_index=4 → angle = 90°, sin=1, cos=0 → x = group.x + 162).
        // Easier: place both nodes at orbit-0 of their own groups so the math is
        // group.x / group.y verbatim.
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
                x: 2000.0,
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
                name: Some("Near Notable".into()),
                icon: None,
                ascendancy_name: None,
                stats: vec!["10% increased Life".into()],
                reminder_text: vec![],
                kind: NodeKind::Notable,
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
                name: Some("Far Notable".into()),
                icon: None,
                ascendancy_name: None,
                stats: vec!["10% increased Life".into()],
                reminder_text: vec![],
                kind: NodeKind::Notable,
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
            min_x: -1000,
            min_y: -1000,
            max_x: 3000,
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

    fn mk_item(base: &str, mod_lines: &[(&str, ModSection)]) -> Item {
        Item {
            name: base.into(),
            base_name: base.into(),
            rarity: Rarity::Magic,
            item_level: 84,
            quality: 0,
            tags: ahash::HashSet::default(),
            mod_lines: mod_lines
                .iter()
                .map(|(l, s)| ModLine {
                    line: (*l).to_string(),
                    section: *s,
                })
                .collect(),
            sockets: String::new(),
            raw: String::new(),
            corrupted: false,
            mirrored: false,
        }
    }

    #[test]
    fn node_positions_compute() {
        let tree = mk_tree();
        let p1 = node_position(&tree, 1).unwrap();
        let p2 = node_position(&tree, 2).unwrap();
        // Group orbit-0 → node sits at the group origin.
        assert!((p1.0 - 0.0).abs() < 1e-6 && (p1.1 - 0.0).abs() < 1e-6);
        assert!((p2.0 - 600.0).abs() < 1e-6 && (p2.1 - 0.0).abs() < 1e-6);
    }

    #[test]
    fn medium_radius_includes_close_node() {
        let tree = mk_tree();
        let radius = pob_data::RADII_3_16[1]; // Medium 0..1440
        let near = nodes_in_radius(&tree, 1, &radius);
        let ids: Vec<NodeId> = near.iter().map(|(id, _)| *id).collect();
        assert!(ids.contains(&2));
        assert!(!ids.contains(&3)); // Far at 2000 is outside Medium 1440.
    }

    #[test]
    fn allocated_filter() {
        let tree = mk_tree();
        let radius = pob_data::RADII_3_16[1];
        let mut alloc: AHashSet<NodeId> = AHashSet::default();
        alloc.insert(2);
        let res = allocated_nodes_in_radius(&tree, 1, &radius, &alloc);
        assert_eq!(res.len(), 1);
        assert_eq!(res[0].0, 2);
    }

    #[test]
    fn identify_basic_radius_jewel() {
        let item = mk_item(
            "Crimson Jewel",
            &[(
                "10% increased Strength to all allocated Passives in Radius",
                ModSection::Explicit,
            )],
        );
        let jewel = identify_radius_jewel(1, &item).expect("identified");
        assert_eq!(jewel.radius_index, 1); // Medium default
        assert_eq!(jewel.kind, HandlerKind::SelfAllocated);
        assert_eq!(jewel.mods.len(), 1);
    }

    #[test]
    fn identify_explicit_large_ring() {
        let item = mk_item(
            "Cobalt Jewel",
            &[
                (
                    "10% increased Cold Damage to nearby allocated passives",
                    ModSection::Explicit,
                ),
                ("Only affects Passives in Large Ring", ModSection::Explicit),
            ],
        );
        let jewel = identify_radius_jewel(1, &item).expect("identified");
        assert_eq!(jewel.radius_index, 2);
        // Header line is excluded from the mod list.
        assert_eq!(jewel.mods.len(), 1);
    }

    #[test]
    fn cluster_jewel_skipped() {
        let item = mk_item(
            "Small Cluster Jewel",
            &[("10% increased Damage", ModSection::Explicit)],
        );
        assert!(identify_radius_jewel(1, &item).is_none());
    }

    #[test]
    fn timeless_jewel_skipped() {
        let item = mk_item(
            "Lethal Pride",
            &[("Passives in Radius gain something", ModSection::Explicit)],
        );
        assert!(identify_radius_jewel(1, &item).is_none());
    }

    #[test]
    fn non_jewel_item_ignored() {
        let item = mk_item(
            "Driftwood Wand",
            &[("10% increased Spell Damage", ModSection::Explicit)],
        );
        assert!(identify_radius_jewel(1, &item).is_none());
    }

    #[test]
    fn rare_jewel_without_radius_text_skipped() {
        let item = mk_item(
            "Cobalt Jewel",
            &[("+20 to Maximum Life", ModSection::Explicit)],
        );
        assert!(identify_radius_jewel(1, &item).is_none());
    }

    #[test]
    fn apply_emits_one_mod_per_in_radius_node() {
        let tree = mk_tree();
        let mut alloc: AHashSet<NodeId> = AHashSet::default();
        alloc.insert(2);
        // Pretend node 3 is also allocated even though it's outside radius — must
        // still be filtered out.
        alloc.insert(3);
        let mut socketed = SocketedJewels::new();
        socketed.socket(
            1,
            mk_item(
                "Crimson Jewel",
                &[(
                    "10% increased Maximum Life to nearby allocated passives",
                    ModSection::Explicit,
                )],
            ),
        );
        let mut db = crate::ModDB::default();
        let report = apply_radius_jewels(&tree, &alloc, &socketed, &mut db);
        assert_eq!(report.applied_jewels, 1);
        assert_eq!(report.skipped, 0);
        // One in-radius allocated node × one mod = one emission.
        assert_eq!(report.mod_emissions, 1);
    }

    #[test]
    fn socketed_jewels_round_trip_storage() {
        let mut s = SocketedJewels::new();
        s.socket(
            42,
            mk_item(
                "Crimson Jewel",
                &[("+1 to all Attributes", ModSection::Explicit)],
            ),
        );
        assert_eq!(s.len(), 1);
        let pulled = s.unsocket(42).expect("removed");
        assert_eq!(pulled.base_name, "Crimson Jewel");
        assert!(s.is_empty());
    }

    #[test]
    fn strip_suffix_removes_to_nearby_allocated_passives() {
        let stripped =
            strip_radius_suffix("10% increased Maximum Life to nearby allocated passives");
        assert_eq!(stripped.as_deref(), Some("10% increased Maximum Life"));
    }

    #[test]
    fn strip_suffix_removes_from_passives_in_radius() {
        let stripped = strip_radius_suffix("+5 to all Attributes from Passives in Radius");
        assert_eq!(stripped.as_deref(), Some("+5 to all Attributes"));
    }

    #[test]
    fn strip_suffix_returns_none_when_line_has_no_marker() {
        assert!(strip_radius_suffix("+20 to maximum Life").is_none());
        assert!(strip_radius_suffix("10% increased Damage").is_none());
    }

    #[test]
    fn parsed_mods_use_canonical_names() {
        let item = mk_item(
            "Crimson Jewel",
            &[(
                "10% increased Maximum Life to nearby allocated passives",
                ModSection::Explicit,
            )],
        );
        let jewel = identify_radius_jewel(1, &item).expect("identified");
        assert_eq!(jewel.mods.len(), 1);
        // Stripping the suffix lets the parser mint the canonical `Life` key,
        // not the long-form `MaximumLifeToNearbyAllocatedPassives`.
        assert_eq!(jewel.mods[0].name, "Life");
        assert_eq!(jewel.mods[0].kind, crate::ModType::Inc);
    }

    #[test]
    fn empty_socket_set_is_no_op() {
        let tree = mk_tree();
        let alloc: AHashSet<NodeId> = AHashSet::default();
        let socketed = SocketedJewels::new();
        let mut db = crate::ModDB::default();
        let report = apply_radius_jewels(&tree, &alloc, &socketed, &mut db);
        assert_eq!(report, RadiusJewelReport::default());
    }

    /// Issue #196: a non-radius unique jewel like Conqueror's Efficiency
    /// (Crimson Jewel base) carries plain global mods — no "to Passives in
    /// Radius" markers. `apply_radius_jewels` correctly skips it. The
    /// fallback `apply_non_radius_socketed_jewels` must pick it up so the
    /// global stats actually land in the modDB.
    #[test]
    fn non_radius_unique_jewel_applies_mods_globally() {
        use crate::mod_db::{EvalState, QueryCfg};
        use crate::{ModStore, ModType};
        // Conqueror's Efficiency mod text — three plain global mods, none
        // mention radius. The Crimson Jewel base is a vanilla jewel base so
        // it survives the subtype gate.
        let item = mk_item(
            "Crimson Jewel",
            &[
                ("4% increased Skill Effect Duration", ModSection::Explicit),
                (
                    "4% increased Mana Reservation Efficiency of Skills",
                    ModSection::Explicit,
                ),
            ],
        );
        let mut socketed = SocketedJewels::new();
        socketed.socket(42, item);
        let mut db = crate::ModDB::default();

        // The radius pass skips this item — applied_jewels stays at 0.
        let tree = mk_tree();
        let alloc: AHashSet<NodeId> = AHashSet::default();
        let radius_report = apply_radius_jewels(&tree, &alloc, &socketed, &mut db);
        assert_eq!(radius_report.applied_jewels, 0);
        assert_eq!(radius_report.skipped, 1);

        // The fallback picks it up and emits both mods globally.
        let emitted = apply_non_radius_socketed_jewels(&socketed, &mut db);
        assert_eq!(emitted, 2);
        let cfg = QueryCfg::default();
        let st = EvalState::default();
        assert_eq!(db.sum(ModType::Inc, &cfg, &st, "SkillEffectDuration"), 4.0);
        assert_eq!(
            db.sum(ModType::Inc, &cfg, &st, "ManaReservationEfficiency"),
            4.0
        );
    }

    /// Issue #196: special-subtype jewels (Cluster, Abyss, Eye, Timeless,
    /// Charm) follow their own dispatch paths. The fallback must NOT
    /// double-apply their item-level mods globally — that would be a
    /// regression for cluster sub-graph synthesis (#21) and the timeless
    /// override path (#30).
    #[test]
    fn non_radius_fallback_skips_special_subtypes() {
        // Cluster jewel with a stat line that, if applied globally, would
        // pollute Damage. Verify the fallback leaves it alone.
        let cluster = mk_item(
            "Large Cluster Jewel",
            &[("Adds 8 Passive Skills", ModSection::Implicit)],
        );
        let mut socketed = SocketedJewels::new();
        socketed.socket(1, cluster);
        let mut db = crate::ModDB::default();
        let emitted = apply_non_radius_socketed_jewels(&socketed, &mut db);
        // Cluster jewel skipped — zero mods emitted.
        assert_eq!(emitted, 0);

        // Same for an abyss eye-jewel base.
        let abyss = mk_item(
            "Searching Eye Jewel",
            &[("+50 to maximum Life", ModSection::Explicit)],
        );
        let mut socketed2 = SocketedJewels::new();
        socketed2.socket(2, abyss);
        let mut db2 = crate::ModDB::default();
        let emitted2 = apply_non_radius_socketed_jewels(&socketed2, &mut db2);
        assert_eq!(emitted2, 0);

        // And for a timeless jewel.
        let timeless = mk_item(
            "Lethal Pride",
            &[(
                "Commanded leadership over 10000 warriors",
                ModSection::Explicit,
            )],
        );
        let mut socketed3 = SocketedJewels::new();
        socketed3.socket(3, timeless);
        let mut db3 = crate::ModDB::default();
        let emitted3 = apply_non_radius_socketed_jewels(&socketed3, &mut db3);
        assert_eq!(emitted3, 0);
    }

    /// Issue #196: a vanilla radius jewel must also be skipped by the
    /// fallback — `apply_radius_jewels` already owns it. Otherwise the
    /// jewel's mods would land twice (once per allocated passive in
    /// radius and once globally), badly inflating the stat.
    #[test]
    fn non_radius_fallback_skips_radius_jewels() {
        use crate::mod_db::{EvalState, QueryCfg};
        use crate::{ModStore, ModType};
        let item = mk_item(
            "Crimson Jewel",
            &[(
                "10% increased Maximum Life to nearby allocated passives",
                ModSection::Explicit,
            )],
        );
        let mut socketed = SocketedJewels::new();
        socketed.socket(1, item);
        let mut db = crate::ModDB::default();
        let emitted = apply_non_radius_socketed_jewels(&socketed, &mut db);
        // identify_radius_jewel returns Some for this item → fallback skips.
        assert_eq!(emitted, 0);
        let cfg = QueryCfg::default();
        let st = EvalState::default();
        assert_eq!(db.sum(ModType::Inc, &cfg, &st, "Life"), 0.0);
    }

    // Suppress "unused-import" lint for the convenience re-export when this
    // module is consumed by callers via the lib.rs facade.
    #[test]
    fn item_set_alias_compiles() {
        let _: ItemSet = ItemSet::new();
    }
}
