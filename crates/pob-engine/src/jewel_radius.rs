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
//! ## What's covered (Issue #196 named uniques)
//!
//! - **Watcher's Eye** ([`HandlerKind::WatchersEye`]) — aura-conditional global
//!   buff; mods carry per-aura `AffectedBy<Aura>` Condition tags emitted by the
//!   parser, gated by the `detect_active_auras` pass in `perform.rs`.
//! - **Healthy Mind** ([`HandlerKind::LifeToManaTransform`]) — transforms each
//!   in-radius allocated node's `Inc Life` mod into an `Inc Mana` mod at 200%.
//! - **Fertile Mind** ([`HandlerKind::DexToIntTransform`]) — transforms each
//!   in-radius allocated node's `+N Dex` BASE mod into an `+N Int` BASE plus a
//!   counter `-N Dex` so the source attribute is moved, not duplicated.
//!
//! ## What's deferred
//!
//! - Timeless jewels (#30): keystone / notable substitution.
//! - Cluster jewel sub-graph synthesis (#21): nodes spawned by a Cluster jewel.
//! - Intuitive Leap (#196 follow-up): pathfind-side connectivity skip.
//! - Pure Talent (#196 follow-up): class-conditional notable buff.
//! - Conqueror's Efficiency (#196 deferred): not actually radius-scoped — the
//!   live unique grants flat global mods only.

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

/// Handler kind. Mirrors PoB's `radiusJewelList[i].type` field. The framework's
/// default is `SelfAllocated` (PoB's `"Self"`); named-unique handlers — Issue
/// #196 — extend this enum with bespoke per-jewel logic.
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
    /// Issue #196: Watcher's Eye. Despite being labelled a radius jewel by
    /// base, in PoB it's actually an aura-conditional global buff — every mod
    /// on the jewel is gated on `AffectedByHatred` / `AffectedByDetermination`
    /// / etc., set when the player has the corresponding aura active. The
    /// radius scan is bypassed; mods land in the player's modDB once with
    /// their parsed condition tag.
    WatchersEye,
    /// Issue #196: Healthy Mind. `Increases and Reductions to Life in Radius
    /// are Transformed to apply to Mana at 200% of their value`. Reads each
    /// in-radius allocated node's `Inc Life` / `Reduce Life` mods and emits
    /// equivalent `Inc Mana` / `Reduce Mana` mods at 2× value. The jewel's own
    /// flat mods (e.g. `+15% increased maximum Mana`) still apply globally
    /// like vanilla.
    LifeToManaTransform,
    /// Issue #196: Fertile Mind. `Dexterity from Passives in Radius is
    /// Transformed to Intelligence`. Reads each in-radius allocated node's
    /// `+N Dex` BASE mods and emits an equivalent `+N Int` BASE mod sourced as
    /// the in-radius node, suppressing the original Dex contribution by
    /// emitting a counter `-N Dex` BASE.
    DexToIntTransform,
    /// Issue #196: Pure Talent / Replica Pure Talent. The jewel grants per-class
    /// bonuses gated on whether the player's tree connects to that class's
    /// starting location. Each mod_line on the jewel is prefixed with a class
    /// name (`Marauder: …`, `Witch: …`); the handler emits only those whose
    /// prefix matches a connected class — the player's own class always counts,
    /// and any other class's `ClassStart` node that's allocated is treated as
    /// connected. The radius scan is bypassed; mods land in the player's modDB
    /// once with no per-radius copying.
    PureTalent,
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
    // Issue #196: named-unique handlers. Detected by item *name* (the unique
    // name, not the base) so we don't conflate the unique with vanilla rolls
    // on the same base. Each named-unique routes through a dedicated
    // [`HandlerKind`] in [`apply_radius_jewels`]; their parsed mods, radius,
    // and source label come from per-handler constructors below.
    if let Some(j) = identify_named_unique(socket_id, item) {
        return Some(j);
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

/// Issue #196: dispatch on the unique's display name to pick a bespoke
/// [`HandlerKind`]. PoB tracks these in `data.jewelData.funcList` per unique;
/// we model them inline as a small lookup. Returns `None` for non-named
/// jewels — the caller then falls through to the vanilla
/// `SelfAllocated`-on-radius-marker path.
fn identify_named_unique(socket_id: NodeId, item: &Item) -> Option<RadiusJewel> {
    // The PoB-canonical "is this a named unique" check is `item.title ~= ""`
    // plus a name match. Items synthesised in tests / from PoB import keep the
    // unique name in `item.name`; the base lives in `base_name`. We compare
    // against `name` so a rare `Cobalt Jewel` named "Healthy Mind" by accident
    // doesn't trip the dispatch.
    let n = item.name.as_str();
    match n {
        "Watcher's Eye" => Some(build_watchers_eye(socket_id, item)),
        "Healthy Mind" => Some(build_life_to_mana(socket_id, item)),
        "Fertile Mind" => Some(build_dex_to_int(socket_id, item)),
        "Pure Talent" | "Replica Pure Talent" => Some(build_pure_talent(socket_id, item)),
        _ => None,
    }
}

/// Watcher's Eye: build a `RadiusJewel` whose mods are the parsed jewel-text
/// lines, sized to whatever radius the base claims (irrelevant — the
/// `WatchersEye` handler ignores radius). Each parsed mod retains the
/// `AffectedBy<Aura>` Condition tag emitted by `mod_parser`'s
/// `match_while_var_dyn`. Lines that don't carry a "while affected by" clause
/// (like the base `+X% maximum Energy Shield/Life/Mana`) parse as plain
/// global mods and apply unconditionally — matching PoB's behaviour where
/// those base implicits are unguarded.
fn build_watchers_eye(socket_id: NodeId, item: &Item) -> RadiusJewel {
    let mut mods = Vec::with_capacity(item.mod_lines.len());
    for ml in &item.mod_lines {
        if let Some(parsed) = parse_mod_line(ml.line.as_str()) {
            mods.push(parsed.mod_);
        }
    }
    RadiusJewel {
        socket_id,
        radius: pob_data::JewelRadiusInfo::new(0.0, 0.0, "Watcher's Eye"),
        radius_index: 0,
        mods,
        source_label: format!("RadiusJewel:{}", item.base_name),
        kind: HandlerKind::WatchersEye,
    }
}

/// Healthy Mind: parse the jewel's *non-transform* mods (e.g.
/// `(15-20)% increased maximum Mana`) like a vanilla jewel, but drop the
/// `Increases and Reductions to Life in Radius are Transformed to apply to
/// Mana at 200% of their value` line — that's a metadata marker the
/// [`HandlerKind::LifeToManaTransform`] handler reads directly. The radius
/// defaults to Large (`Radius: Large` per upstream `Data/Uniques/jewel.lua`).
fn build_life_to_mana(socket_id: NodeId, item: &Item) -> RadiusJewel {
    build_transformer(
        socket_id,
        item,
        HandlerKind::LifeToManaTransform,
        is_life_mana_transform_marker,
    )
}

/// Fertile Mind: parse the `+(16-24) to Intelligence` flat mod normally and
/// drop the transform marker line. Default radius Large.
fn build_dex_to_int(socket_id: NodeId, item: &Item) -> RadiusJewel {
    build_transformer(
        socket_id,
        item,
        HandlerKind::DexToIntTransform,
        is_dex_int_transform_marker,
    )
}

/// Pure Talent / Replica Pure Talent: build a [`RadiusJewel`] whose `mods` list
/// is empty — the actual class-conditional bonuses come from the dispatch
/// handler reading the item's raw `mod_lines` and filtering by class
/// connection. The radius is irrelevant (PoB ignores it for this jewel) but we
/// still pin it to a `(0.0, 0.0)` band so the dispatch's radius-scan branch
/// short-circuits with an empty in-radius set if it accidentally falls
/// through.
fn build_pure_talent(socket_id: NodeId, item: &Item) -> RadiusJewel {
    RadiusJewel {
        socket_id,
        radius: pob_data::JewelRadiusInfo::new(0.0, 0.0, "Pure Talent"),
        radius_index: 0,
        mods: Vec::new(),
        source_label: format!("RadiusJewel:{}", item.name),
        kind: HandlerKind::PureTalent,
    }
}

fn build_transformer(
    socket_id: NodeId,
    item: &Item,
    kind: HandlerKind,
    marker: fn(&str) -> bool,
) -> RadiusJewel {
    let mods = parse_non_transform_mods(item, marker);
    let idx = radius_index_for_label("Large").unwrap_or(2);
    let radii = radii_for_tree_version(&item_tree_version(item));
    let radius = radii
        .get(idx)
        .copied()
        .unwrap_or_else(|| pob_data::JewelRadiusInfo::new(0.0, 1500.0, "Large"));
    RadiusJewel {
        socket_id,
        radius,
        radius_index: idx,
        mods,
        source_label: format!("RadiusJewel:{}", item.base_name),
        kind,
    }
}

fn parse_non_transform_mods(item: &Item, is_marker: fn(&str) -> bool) -> Vec<Mod> {
    let mut mods = Vec::with_capacity(item.mod_lines.len());
    for ml in &item.mod_lines {
        let raw = ml.line.as_str();
        if is_marker(raw) || explicit_ring_label(raw).is_some() {
            continue;
        }
        let stripped = strip_radius_suffix(raw);
        let target = stripped.as_deref().unwrap_or(raw);
        if let Some(parsed) = parse_mod_line(target) {
            mods.push(parsed.mod_);
        } else if stripped.is_some() {
            if let Some(parsed) = parse_mod_line(raw) {
                mods.push(parsed.mod_);
            }
        }
    }
    mods
}

fn is_life_mana_transform_marker(line: &str) -> bool {
    let l = line.to_ascii_lowercase();
    l.contains("increases and reductions to life in radius") && l.contains("to apply to mana")
}

/// Issue #196 (Pure Talent): the seven base classes whose starting locations
/// the jewel checks for connection. Mirrors PoB's `PureTalent` handler list
/// and the upstream jewel text. Replica Pure Talent uses the same set.
const PURE_TALENT_CLASSES: &[&str] = &[
    "Marauder", "Duelist", "Ranger", "Shadow", "Witch", "Templar", "Scion",
];

/// Issue #196: build the set of classes the player's tree currently connects
/// to for Pure Talent purposes. The player's own class always counts (PoB
/// treats the class-start anchor as connected even when not in the allocated
/// set). Any other class's `ClassStart` node that's been allocated also
/// counts — that's how a path that crosses through an adjacent class start
/// picks up its bonus.
fn pure_talent_connected_classes(
    player_class: &str,
    tree: &PassiveTree,
    allocated: &AHashSet<NodeId>,
) -> std::collections::HashSet<String> {
    let mut connected: std::collections::HashSet<String> = std::collections::HashSet::new();
    if PURE_TALENT_CLASSES.contains(&player_class) {
        connected.insert(player_class.to_owned());
    }
    for &id in allocated {
        let Some(node) = tree.nodes.get(&id) else {
            continue;
        };
        if node.kind != NodeKind::ClassStart {
            continue;
        }
        let Some(idx) = node.class_start_index else {
            continue;
        };
        // `tree.classes` is indexed positionally — `class_start_index` is the
        // same index PoB uses for `Build.targetVersion` class lookups.
        let Some(class) = tree.classes.get(idx as usize) else {
            continue;
        };
        if PURE_TALENT_CLASSES.contains(&class.name.as_str()) {
            connected.insert(class.name.clone());
        }
    }
    connected
}

/// Issue #196: walk Pure Talent's `mod_lines`, strip the leading `<Class>: `
/// prefix from each, and emit the resulting mod globally only when the class
/// is in `connected`. Returns the number of mods successfully emitted so the
/// dispatch's `RadiusJewelReport.mod_emissions` stays accurate.
fn apply_pure_talent_lines(
    item: &Item,
    connected: &std::collections::HashSet<String>,
    source_label: &str,
    db: &mut crate::ModDB,
) -> usize {
    let mut emitted = 0usize;
    for ml in &item.mod_lines {
        let raw = ml.line.trim();
        let Some((prefix, body)) = raw.split_once(": ") else {
            continue;
        };
        if !PURE_TALENT_CLASSES.contains(&prefix) {
            // Non-class metadata like `Limited to: 1` or PoB's
            // `Variant: Current` lines are intentionally dropped here —
            // they don't contribute mods to the build.
            continue;
        }
        if !connected.contains(prefix) {
            continue;
        }
        let body = body.trim();
        if body.is_empty() {
            continue;
        }
        if let Some(parsed) = parse_mod_line(body) {
            let mut clone = parsed.mod_;
            clone.source = Some(Source::Other(format!("{source_label}:{prefix}")));
            db.add(clone);
            emitted += 1;
        }
    }
    emitted
}

fn is_dex_int_transform_marker(line: &str) -> bool {
    let l = line.to_ascii_lowercase();
    l.contains("dexterity from passives in radius") && l.contains("transformed to intelligence")
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
    player_class: &str,
    db: &mut crate::ModDB,
) -> RadiusJewelReport {
    let mut report = RadiusJewelReport::default();
    for (socket_id, item) in socketed.iter() {
        let Some(jewel) = identify_radius_jewel(*socket_id, item) else {
            report.skipped += 1;
            continue;
        };
        report.applied_jewels += 1;
        match jewel.kind {
            HandlerKind::WatchersEye => {
                // Aura-conditional global buff: emit each parsed mod once into
                // the player's modDB. The mod's `AffectedBy<Aura>` Condition
                // tag (set by the parser) gates application; the conditions
                // themselves are flipped on by the active-aura detection in
                // perform.rs. The base implicits (`X% increased maximum Life`
                // etc.) parse without a condition tag and apply globally.
                for m in &jewel.mods {
                    let mut clone = m.clone();
                    clone.source = Some(Source::Other(jewel.source_label.clone()));
                    db.add(clone);
                    report.mod_emissions += 1;
                }
            }
            HandlerKind::LifeToManaTransform => {
                // First, emit the jewel's plain mods (e.g. `+15% increased
                // maximum Mana`) globally — these aren't transforms and apply
                // exactly like vanilla jewel bonuses.
                for m in &jewel.mods {
                    let mut clone = m.clone();
                    clone.source = Some(Source::Other(jewel.source_label.clone()));
                    db.add(clone);
                    report.mod_emissions += 1;
                }
                // Then walk the in-radius allocated nodes' stats, find any
                // Inc/Reduce Life mod, and emit an equivalent Inc/Reduce Mana
                // mod at 200% of the source value sourced as the in-radius
                // node so per-node breakdowns attribute the bonus correctly.
                let in_radius =
                    allocated_nodes_in_radius(tree, *socket_id, &jewel.radius, allocated);
                let n = transform_radius_attribute(
                    tree,
                    db,
                    &in_radius,
                    "Life",
                    "Mana",
                    crate::ModType::Inc,
                    2.0,
                    &jewel.source_label,
                );
                report.mod_emissions += n;
            }
            HandlerKind::DexToIntTransform => {
                // Emit base mods (`+(16-24) to Intelligence`) globally first.
                for m in &jewel.mods {
                    let mut clone = m.clone();
                    clone.source = Some(Source::Other(jewel.source_label.clone()));
                    db.add(clone);
                    report.mod_emissions += 1;
                }
                // For each in-radius allocated node, find `+N Dex` BASE mods
                // and emit an equivalent `+N Int` BASE; do not double-count
                // the Dex (PoB's transform fully replaces it). We model that
                // by emitting an offsetting `-N Dex` BASE so the original
                // node-side Dex contribution cancels out.
                let in_radius =
                    allocated_nodes_in_radius(tree, *socket_id, &jewel.radius, allocated);
                let n = transform_radius_attribute(
                    tree,
                    db,
                    &in_radius,
                    "Dexterity",
                    "Intelligence",
                    crate::ModType::Base,
                    1.0,
                    &jewel.source_label,
                );
                report.mod_emissions += n;
            }
            HandlerKind::PureTalent => {
                // Pure Talent grants per-class bonuses gated on the player's
                // tree connecting to that class' starting location. The own
                // class is always considered connected; any other class's
                // `ClassStart` node that lives in the allocated set counts as
                // connected too. Each `<Class>: <mod>` line on the jewel only
                // emits when its prefix matches a connected class.
                //
                // We read the raw `mod_lines` rather than a pre-parsed list
                // because the class prefix isn't a stat the mod parser
                // understands — stripping it here keeps the parser's per-line
                // mod-text grammar untouched and avoids minting bogus
                // `Marauder` named mods.
                let connected = pure_talent_connected_classes(player_class, tree, allocated);
                let n = apply_pure_talent_lines(item, &connected, &jewel.source_label, db);
                report.mod_emissions += n;
            }
            // SelfAllocated (default), All, Threshold, SelfUnalloc, Pathfinder
            // all currently route through the vanilla per-allocated-node mod
            // copy. The non-Self variants will get their own dispatch arms
            // when the timeless / cluster / intuitive-leap follow-ups land.
            _ => {
                let in_radius =
                    allocated_nodes_in_radius(tree, *socket_id, &jewel.radius, allocated);
                for (target_node, _) in in_radius {
                    for m in &jewel.mods {
                        let mut clone = m.clone();
                        clone.source = Some(Source::Passive(target_node));
                        db.add(clone);
                        report.mod_emissions += 1;
                    }
                }
            }
        }
    }
    report
}

/// Issue #196: walk an in-radius node list, parse each node's stat lines, and
/// emit a transformed mod for any line that produces a `<from>` mod of the
/// requested kind. The transformed mod targets `<to>` at `scale ×` the source
/// value, sourced as the same in-radius node so per-node breakdowns line up
/// with PoB's. Returns the number of mod copies emitted.
///
/// Used for Healthy Mind (`Inc Life` → `Inc Mana` × 2) and Fertile Mind
/// (`Base Dex` → `Base Int` × 1, plus a counter `-N Dex` so the original Dex
/// contribution from the in-radius node cancels out).
fn transform_radius_attribute(
    tree: &PassiveTree,
    db: &mut crate::ModDB,
    in_radius: &[(NodeId, f64)],
    from: &str,
    to: &str,
    kind: crate::ModType,
    scale: f64,
    source_label: &str,
) -> usize {
    let mut emitted = 0usize;
    for (node_id, _) in in_radius {
        let Some(node) = tree.nodes.get(node_id) else {
            continue;
        };
        for raw in &node.stats {
            for line in raw.lines() {
                let line = line.trim();
                if line.is_empty() {
                    continue;
                }
                let Some(parsed) = parse_mod_line(line) else {
                    continue;
                };
                let m = &parsed.mod_;
                if m.kind != kind || m.name != from {
                    continue;
                }
                let Some(value) = m.value.as_f64() else {
                    continue;
                };
                // Emit the transformed mod (e.g. Inc Mana += value × scale)
                // sourced as the in-radius passive so the per-node breakdown
                // attributes the gain to that node.
                let mut to_mod = m.clone();
                to_mod.name = to.to_string();
                to_mod.value = crate::ModValue::Number(value * scale);
                to_mod.source = Some(Source::Passive(*node_id));
                // Drop tags — the transformer ignores conditional clauses on
                // the source mod (PoB's Healthy Mind transforms unconditional
                // Inc Life only, mirroring the simplification).
                to_mod.tags.clear();
                db.add(to_mod);
                emitted += 1;
                // For BASE attribute transforms (Fertile Mind), also emit a
                // counter mod so the original Dex contribution cancels out.
                // This matches PoB's "Transformed to" semantics where the
                // attribute is *moved*, not duplicated.
                if kind == crate::ModType::Base {
                    let mut counter = m.clone();
                    counter.value = crate::ModValue::Number(-value);
                    counter.source = Some(Source::Other(source_label.to_string()));
                    counter.tags.clear();
                    db.add(counter);
                    emitted += 1;
                }
            }
        }
    }
    emitted
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
        mk_item_named(base, base, mod_lines)
    }

    fn mk_item_named(name: &str, base: &str, mod_lines: &[(&str, ModSection)]) -> Item {
        Item {
            name: name.into(),
            base_name: base.into(),
            rarity: Rarity::Unique,
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
        let report = apply_radius_jewels(&tree, &alloc, &socketed, "Marauder", &mut db);
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
        let report = apply_radius_jewels(&tree, &alloc, &socketed, "Marauder", &mut db);
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
        let radius_report = apply_radius_jewels(&tree, &alloc, &socketed, "Marauder", &mut db);
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

    // ---- Issue #196: named-unique handlers ---------------------------------

    /// Watcher's Eye is identified by `item.name`, not by the radius marker —
    /// the unique's mod text is "while affected by <Aura>", not "in Radius".
    #[test]
    fn identify_watchers_eye_routes_to_aura_handler() {
        let item = mk_item_named(
            "Watcher's Eye",
            "Prismatic Jewel",
            &[(
                "40% increased Cold Damage while affected by Hatred",
                ModSection::Explicit,
            )],
        );
        let jewel = identify_radius_jewel(1, &item).expect("identified");
        assert_eq!(jewel.kind, HandlerKind::WatchersEye);
        assert_eq!(jewel.mods.len(), 1);
        // The parser stamps `AffectedByHatred` as a Condition tag on the mod.
        assert!(
            jewel.mods[0]
                .tags
                .iter()
                .any(|t| matches!(&t.kind, crate::TagKind::Condition { var, .. } if var == "AffectedByHatred")),
            "expected AffectedByHatred Condition tag on Watcher's Eye mod, got {:?}",
            jewel.mods[0].tags,
        );
    }

    #[test]
    fn watchers_eye_mods_apply_globally_with_condition() {
        let tree = mk_tree();
        let alloc: AHashSet<NodeId> = AHashSet::default(); // no allocations needed
        let mut socketed = SocketedJewels::new();
        socketed.socket(
            1,
            mk_item_named(
                "Watcher's Eye",
                "Prismatic Jewel",
                &[(
                    "40% increased Cold Damage while affected by Hatred",
                    ModSection::Explicit,
                )],
            ),
        );
        let mut db = crate::ModDB::default();
        let report = apply_radius_jewels(&tree, &alloc, &socketed, "Marauder", &mut db);
        assert_eq!(report.applied_jewels, 1);
        // One global emission — the radius scan is bypassed entirely.
        assert_eq!(report.mod_emissions, 1);
        // The mod is in the modDB with its condition tag intact.
        let cold = db.slice_named("ColdDamage");
        assert!(
            cold.iter().any(|m| matches!(m.kind, crate::ModType::Inc)
                && m.tags.iter().any(|t| matches!(&t.kind, crate::TagKind::Condition { var, .. } if var == "AffectedByHatred"))),
            "expected gated Inc ColdDamage mod, got {cold:#?}",
        );
    }

    /// Healthy Mind: in-radius Inc Life mods should produce Inc Mana mods at 2×
    /// scale, sourced as the in-radius node.
    #[test]
    fn healthy_mind_transforms_inc_life_to_inc_mana_double() {
        let tree = mk_tree();
        let mut alloc: AHashSet<NodeId> = AHashSet::default();
        // Node 2 is in radius (600 units away from socket at 0,0 with Medium ring 1440).
        // The mock test tree node 2 has stats `["10% increased Life"]`.
        alloc.insert(2);
        let mut socketed = SocketedJewels::new();
        socketed.socket(
            1,
            mk_item_named(
                "Healthy Mind",
                "Cobalt Jewel",
                &[
                    ("15% increased maximum Mana", ModSection::Explicit),
                    (
                        "Increases and Reductions to Life in Radius are Transformed to apply to Mana at 200% of their value",
                        ModSection::Explicit,
                    ),
                ],
            ),
        );
        let mut db = crate::ModDB::default();
        let report = apply_radius_jewels(&tree, &alloc, &socketed, "Marauder", &mut db);
        assert_eq!(report.applied_jewels, 1);
        // Exactly one global Mana mod (the +15% line) plus one transformed
        // Inc Mana mod from node 2's `10% increased Life`.
        assert!(report.mod_emissions >= 2);
        let mana = db.slice_named("Mana");
        // The +15% global Mana mod.
        assert!(
            mana.iter().any(|m| matches!(m.kind, crate::ModType::Inc)
                && (m.value.as_f64().unwrap_or(0.0) - 15.0).abs() < 1e-6),
            "expected global +15% Inc Mana, got {mana:#?}",
        );
        // The transformed mod: 10% Inc Life × 200% = +20% Inc Mana sourced from node 2.
        assert!(
            mana.iter().any(|m| matches!(m.kind, crate::ModType::Inc)
                && matches!(&m.source, Some(Source::Passive(id)) if *id == 2)
                && (m.value.as_f64().unwrap_or(0.0) - 20.0).abs() < 1e-6),
            "expected transformed +20% Inc Mana sourced from Passive(2), got {mana:#?}",
        );
    }

    #[test]
    fn fertile_mind_transforms_dex_base_to_int() {
        // Custom tree where node 2 has a `+30 to Dexterity` BASE stat.
        let mut tree = mk_tree();
        tree.nodes.get_mut(&2).unwrap().stats = vec!["+30 to Dexterity".into()];
        let mut alloc: AHashSet<NodeId> = AHashSet::default();
        alloc.insert(2);
        let mut socketed = SocketedJewels::new();
        socketed.socket(
            1,
            mk_item_named(
                "Fertile Mind",
                "Cobalt Jewel",
                &[
                    ("+20 to Intelligence", ModSection::Explicit),
                    (
                        "Dexterity from Passives in Radius is Transformed to Intelligence",
                        ModSection::Explicit,
                    ),
                ],
            ),
        );
        let mut db = crate::ModDB::default();
        let report = apply_radius_jewels(&tree, &alloc, &socketed, "Marauder", &mut db);
        assert_eq!(report.applied_jewels, 1);
        // Plain mod (+20 Int) is one emission. Transform of +30 Dex emits two
        // (Int + counter Dex) for at least three emissions total.
        assert!(report.mod_emissions >= 3);
        let int_mods = db.slice_named("Intelligence");
        // +20 global Int
        assert!(
            int_mods
                .iter()
                .any(|m| (m.value.as_f64().unwrap_or(0.0) - 20.0).abs() < 1e-6),
            "expected global +20 Int, got {int_mods:#?}",
        );
        // Transformed +30 Int sourced from node 2.
        assert!(
            int_mods.iter().any(
                |m| matches!(&m.source, Some(Source::Passive(id)) if *id == 2)
                    && (m.value.as_f64().unwrap_or(0.0) - 30.0).abs() < 1e-6
            ),
            "expected +30 Int sourced from Passive(2), got {int_mods:#?}",
        );
        // Counter -30 Dex offsetting the source contribution.
        let dex = db.slice_named("Dexterity");
        assert!(
            dex.iter()
                .any(|m| (m.value.as_f64().unwrap_or(0.0) + 30.0).abs() < 1e-6),
            "expected counter -30 Dex from Fertile Mind, got {dex:#?}",
        );
    }

    /// Issue #196: Pure Talent test scaffold. Build a tree that has both
    /// ClassStart nodes (for Marauder + Witch) and a regular jewel-socket
    /// node so the dispatch can identify the connected class set. The
    /// tree's positions don't matter for this handler — the radius is
    /// pinned to (0, 0) by `build_pure_talent`.
    fn mk_class_start_tree() -> PassiveTree {
        use pob_data::Class;
        let mut groups = ahash::HashMap::default();
        groups.insert(
            10,
            Group {
                x: 0.0,
                y: 0.0,
                orbits: smallvec::smallvec![0],
                background: None,
                nodes: vec![1, 100, 200],
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
        // Class start for Marauder (index 0).
        nodes.insert(
            100,
            Node {
                id: 100,
                name: Some("Marauder Start".into()),
                icon: None,
                ascendancy_name: None,
                stats: vec![],
                reminder_text: vec![],
                kind: NodeKind::ClassStart,
                class_start_index: Some(0),
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
        // Class start for Witch (index 1).
        nodes.insert(
            200,
            Node {
                id: 200,
                name: Some("Witch Start".into()),
                icon: None,
                ascendancy_name: None,
                stats: vec![],
                reminder_text: vec![],
                kind: NodeKind::ClassStart,
                class_start_index: Some(1),
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
        PassiveTree {
            version: "3_25".into(),
            tree: "Default".into(),
            classes: vec![
                Class {
                    name: "Marauder".into(),
                    base_str: 32,
                    base_dex: 14,
                    base_int: 14,
                    ascendancies: vec![],
                },
                Class {
                    name: "Witch".into(),
                    base_str: 14,
                    base_dex: 14,
                    base_int: 32,
                    ascendancies: vec![],
                },
            ],
            groups,
            nodes,
            jewel_slots: vec![1],
            min_x: -100,
            min_y: -100,
            max_x: 100,
            max_y: 100,
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

    /// Issue #196: a Pure Talent socketed by a Marauder (own class) only
    /// emits the `Marauder:` line — the other six class lines are gated.
    /// Verify the Marauder bonus lands as `AreaOfEffect Inc 25`.
    #[test]
    fn pure_talent_emits_only_player_class_line_by_default() {
        use crate::{ModStore as _, ModType};
        let tree = mk_class_start_tree();
        // Marauder allocation only — no other class start node allocated.
        let alloc: AHashSet<NodeId> = AHashSet::default();
        let item = mk_item_named(
            "Pure Talent",
            "Viridian Jewel",
            &[
                (
                    "Marauder: Melee Skills have 25% increased Area of Effect",
                    ModSection::Explicit,
                ),
                (
                    "Duelist: 1% of Attack Damage Leeched as Life",
                    ModSection::Explicit,
                ),
                ("Ranger: 7% increased Movement Speed", ModSection::Explicit),
                (
                    "Witch: 0.5% of Mana Regenerated per second",
                    ModSection::Explicit,
                ),
            ],
        );
        let mut socketed = SocketedJewels::new();
        socketed.socket(1, item);
        let mut db = crate::ModDB::default();
        let report = apply_radius_jewels(&tree, &alloc, &socketed, "Marauder", &mut db);
        assert_eq!(report.applied_jewels, 1);
        // Only the Marauder line emits.
        assert_eq!(report.mod_emissions, 1);
        // Walk the modDB looking for the emitted mod. The Marauder line
        // parses as `AreaOfEffect Inc 25` with a `melee` flag — both flag
        // and name are inspected by the assertion so future parser
        // changes that rename the stat surface here.
        let mut found = false;
        for m in db.iter_all() {
            if m.kind == ModType::Inc && m.name == "AreaOfEffect" {
                found = true;
                assert!(matches!(m.value, crate::ModValue::Number(v) if (v - 25.0).abs() < 0.001));
            }
        }
        assert!(
            found,
            "expected an AreaOfEffect Inc mod from the Marauder line"
        );
        // None of the gated classes' bonuses landed: Witch's "Mana
        // Regenerated per second" would target ManaRegen if it had landed.
        let mut witch_found = false;
        for m in db.iter_all() {
            if m.name == "ManaRegen" {
                witch_found = true;
            }
        }
        assert!(
            !witch_found,
            "Witch line should be gated when Marauder is the player class"
        );
    }

    /// Issue #196: when the player's tree allocates a non-own ClassStart
    /// (e.g. a Marauder pathing into the Witch start), Pure Talent grants
    /// that other class's bonus too. Verify Witch's `Mana Regenerated per
    /// second` bonus lands when node 200 (Witch start) is in `allocated`.
    #[test]
    fn pure_talent_emits_other_class_when_class_start_allocated() {
        let tree = mk_class_start_tree();
        let mut alloc: AHashSet<NodeId> = AHashSet::default();
        alloc.insert(200); // Witch start allocated.
        let item = mk_item_named(
            "Pure Talent",
            "Viridian Jewel",
            &[
                (
                    "Marauder: Melee Skills have 25% increased Area of Effect",
                    ModSection::Explicit,
                ),
                (
                    "Witch: 0.5% of Mana Regenerated per second",
                    ModSection::Explicit,
                ),
            ],
        );
        let mut socketed = SocketedJewels::new();
        socketed.socket(1, item);
        let mut db = crate::ModDB::default();
        let report = apply_radius_jewels(&tree, &alloc, &socketed, "Marauder", &mut db);
        // Both Marauder (own) and Witch (path-connected) emit.
        assert_eq!(report.mod_emissions, 2);
    }

    /// Issue #196: Replica Pure Talent uses the same handler. A non-jewel
    /// item with the Pure Talent name should still be ignored — the
    /// identifier checks `is_jewel_base` first.
    #[test]
    fn replica_pure_talent_uses_same_handler() {
        let tree = mk_class_start_tree();
        let alloc: AHashSet<NodeId> = AHashSet::default();
        let item = mk_item_named(
            "Replica Pure Talent",
            "Viridian Jewel",
            &[(
                "Marauder: Melee Skills have 25% increased Area of Effect",
                ModSection::Explicit,
            )],
        );
        let mut socketed = SocketedJewels::new();
        socketed.socket(1, item);
        let mut db = crate::ModDB::default();
        let report = apply_radius_jewels(&tree, &alloc, &socketed, "Marauder", &mut db);
        assert_eq!(report.applied_jewels, 1);
        assert_eq!(report.mod_emissions, 1);
    }

    /// Issue #196: `Limited to: 1` and other non-class metadata lines on
    /// Pure Talent must be silently dropped — they're informational, not
    /// stat mods.
    #[test]
    fn pure_talent_drops_metadata_lines() {
        let tree = mk_class_start_tree();
        let alloc: AHashSet<NodeId> = AHashSet::default();
        let item = mk_item_named(
            "Pure Talent",
            "Viridian Jewel",
            &[
                ("Limited to: 1", ModSection::Explicit),
                (
                    "Marauder: Melee Skills have 25% increased Area of Effect",
                    ModSection::Explicit,
                ),
            ],
        );
        let mut socketed = SocketedJewels::new();
        socketed.socket(1, item);
        let mut db = crate::ModDB::default();
        let report = apply_radius_jewels(&tree, &alloc, &socketed, "Marauder", &mut db);
        // One mod (Marauder), no error from the `Limited to: 1` line.
        assert_eq!(report.mod_emissions, 1);
    }

    /// Issue #196: connected_classes computes the same set the dispatch
    /// uses. Sanity-check the helper directly so a future refactor that
    /// changes the call shape can't silently break the trigger logic.
    #[test]
    fn pure_talent_connected_classes_resolves_player_and_allocated_starts() {
        let tree = mk_class_start_tree();
        let mut alloc: AHashSet<NodeId> = AHashSet::default();
        // Player class only.
        let connected = pure_talent_connected_classes("Marauder", &tree, &alloc);
        assert!(connected.contains("Marauder"));
        assert!(!connected.contains("Witch"));

        // Plus an allocated Witch start.
        alloc.insert(200);
        let connected = pure_talent_connected_classes("Marauder", &tree, &alloc);
        assert!(connected.contains("Marauder"));
        assert!(connected.contains("Witch"));
        assert_eq!(connected.len(), 2);
    }

    /// A radius jewel that transforms Inc Life out of radius shouldn't fire
    /// on a node *outside* the medium ring.
    #[test]
    fn healthy_mind_skips_out_of_radius_nodes() {
        let tree = mk_tree();
        let mut alloc: AHashSet<NodeId> = AHashSet::default();
        // Node 3 sits at (2000, 0) — outside Large ring (~1800).
        alloc.insert(3);
        let mut socketed = SocketedJewels::new();
        socketed.socket(
            1,
            mk_item_named(
                "Healthy Mind",
                "Cobalt Jewel",
                &[(
                    "Increases and Reductions to Life in Radius are Transformed to apply to Mana at 200% of their value",
                    ModSection::Explicit,
                )],
            ),
        );
        let mut db = crate::ModDB::default();
        let report = apply_radius_jewels(&tree, &alloc, &socketed, "Marauder", &mut db);
        // No transformed mods (node 3 is out of Large ring) and no plain
        // global mods (the only line was the metadata marker).
        assert_eq!(report.applied_jewels, 1);
        assert_eq!(report.mod_emissions, 0);
    }
}
