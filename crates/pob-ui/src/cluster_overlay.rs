//! Visual overlay for synthesised cluster jewel sub-graphs on the Tree tab.
//!
//! Slice B of [#197](https://github.com/jonatanferm/genericpathofbuildingMK2/issues/197).
//! Once the user has socketed a Cluster Jewel into a Large jewel socket
//! (Slice A: `cluster_paste.rs`), the engine's `cluster_synth::synthesise_all`
//! produces a `ClusterJewelSpec` per socket. This module:
//!
//! 1. Computes a deterministic radial layout placing the synthesised nodes in
//!    a ring around the host socket's tree-space position. PoB renders these
//!    on a real expansion-jewel orbit; we use a simpler local layout because
//!    the orbit math depends on tree-version-specific `orbitOffsets` data
//!    that's not exposed to the UI layer. Pixel-identical placement isn't a
//!    hard requirement for Issue #197 — what matters is that the synth nodes
//!    appear *near the host socket* and that clicking them allocates the
//!    underlying synth NodeId.
//!
//! 2. Renders each node as a small filled circle with a kind-aware colour
//!    (notable / small / inner-socket) and an allocated-state ring, plus
//!    edges as thin lines. Done via egui's painter so it runs *on top* of
//!    the wgpu tree pipeline without needing custom shaders for a small
//!    synthetic graph.
//!
//! 3. Exposes a hit-test helper so `lib.rs` can map a screen-space click to
//!    the underlying synth NodeId and forward it to
//!    `Character::allocate` / `Character::unallocate`. We bypass the normal
//!    pathfinder because synth nodes aren't in `PassiveTree.nodes`; we still
//!    gate allocation on the host socket being allocated to mirror the
//!    "must reach the socket first" rule PoB enforces during compute.

use eframe::egui::{Color32, Painter, Pos2, Stroke};
use pob_data::{ClusterJewelData, ClusterModSet, NodeId, NodeKind, PassiveTree};
use pob_engine::cluster_synth::{synthesise_all, ClusterJewelSpec};
use pob_engine::Character;

/// One node placed at a known screen-space position. The renderer + hit-test
/// share the same struct so positions are computed exactly once per frame.
#[derive(Debug, Clone)]
pub struct PlacedSynthNode {
    pub id: NodeId,
    pub host_socket: NodeId,
    pub kind: NodeKind,
    pub screen_pos: Pos2,
    /// Tree-space `(x, y)` — kept on the struct for tooltip / debug surfaces
    /// that want to know where the node was placed in world units.
    #[allow(dead_code)]
    pub world_pos: [f32; 2],
    pub display_name: Option<String>,
    pub stat_lines: Vec<String>,
}

/// Result of `compute_overlay`. Edges are stored as `(a_id, b_id)` so the
/// renderer can look up positions in `nodes` by id.
#[derive(Debug, Clone, Default)]
pub struct ClusterOverlay {
    pub nodes: Vec<PlacedSynthNode>,
    /// Edges between two synth nodes. Drawn as straight lines.
    pub edges: Vec<(NodeId, NodeId)>,
    /// `(host_socket_id, entrance_synth_id)` connections — drawn from the
    /// host socket's screen position to the entrance node so the user can
    /// see the sub-graph hanging off the socket.
    pub socket_links: Vec<(NodeId, NodeId)>,
}

/// Subset of `ClusterOverlayCtx` that doesn't carry the projection closure.
/// Used as the public input to `TreeView::ui_with_overlay` — the `to_screen`
/// closure is built internally by `tree_view` from its viewport / zoom /
/// center, since those aren't otherwise public.
pub struct OverlayInputs<'a> {
    pub character: &'a Character,
    pub cluster_jewels: &'a ClusterJewelData,
    pub cluster_jewel_mods: &'a ClusterModSet,
    pub synth_radius_world: f32,
    pub synth_world_size: f32,
}

/// Snapshot of the inputs `compute_overlay` needs. Bundled so callers don't
/// have to thread five separate borrows through every frame.
pub struct ClusterOverlayCtx<'a> {
    pub character: &'a Character,
    pub tree: &'a PassiveTree,
    pub cluster_jewels: &'a ClusterJewelData,
    pub cluster_jewel_mods: &'a ClusterModSet,
    /// `(world_x, world_y)` → screen `Pos2` projection.
    pub to_screen: &'a dyn Fn([f32; 2]) -> Pos2,
    /// Per-jewel layout radius in world units. The host socket is a
    /// JewelSocket node with `world_radius_for(JewelSocket) = 75.0` (see
    /// `tree_view::world_radius_for`); we place synth nodes at ~3x that
    /// distance so they don't overlap the host visually.
    pub synth_radius_world: f32,
    /// World-space radius of each synth node. Defaults to 35 (Normal radius)
    /// or 70 (Notable radius) depending on the role; expressed here as the
    /// hit-test radius the caller wants.
    pub synth_world_size: f32,
}

/// Compute the overlay for the current frame. Empty when no cluster jewels
/// are socketed.
pub fn compute_overlay(ctx: &ClusterOverlayCtx<'_>) -> ClusterOverlay {
    if ctx.character.jewels.is_empty() {
        return ClusterOverlay::default();
    }
    let specs = synthesise_all(
        ctx.tree,
        &ctx.character.jewels,
        ctx.cluster_jewels,
        ctx.cluster_jewel_mods,
    );
    let mut overlay = ClusterOverlay::default();
    for spec in &specs {
        let Some(host_world) = host_world_pos(ctx.tree, spec.parent_socket) else {
            continue;
        };
        place_spec(spec, host_world, ctx, &mut overlay);
    }
    overlay
}

/// Hit-test the overlay at `screen_pos`. Returns the closest synth NodeId
/// within `hit_radius_screen` pixels, if any.
pub fn hit_test(
    overlay: &ClusterOverlay,
    screen_pos: Pos2,
    hit_radius_screen: f32,
) -> Option<NodeId> {
    let mut best: Option<(NodeId, f32)> = None;
    for n in &overlay.nodes {
        let d = (n.screen_pos - screen_pos).length();
        if d <= hit_radius_screen {
            if best.map(|(_, bd)| d < bd).unwrap_or(true) {
                best = Some((n.id, d));
            }
        }
    }
    best.map(|(id, _)| id)
}

/// Render the overlay over the passed painter. `host_pos_lookup` maps a host
/// socket id to its current screen position (so the renderer can draw the
/// `socket → entrance` connector at the same position the rest of the tree
/// view uses).
pub fn render(
    painter: &Painter,
    overlay: &ClusterOverlay,
    character: &Character,
    host_pos_lookup: &dyn Fn(NodeId) -> Option<Pos2>,
) {
    // Edges first so node circles draw on top.
    let edge_color = Color32::from_rgb(96, 96, 96);
    let edge_color_alloc = Color32::from_rgb(220, 200, 80);
    for &(a, b) in &overlay.edges {
        let Some(pa) = overlay
            .nodes
            .iter()
            .find(|n| n.id == a)
            .map(|n| n.screen_pos)
        else {
            continue;
        };
        let Some(pb) = overlay
            .nodes
            .iter()
            .find(|n| n.id == b)
            .map(|n| n.screen_pos)
        else {
            continue;
        };
        let alloc = character.allocated.contains(&a) && character.allocated.contains(&b);
        painter.line_segment(
            [pa, pb],
            Stroke::new(
                if alloc { 2.0 } else { 1.5 },
                if alloc { edge_color_alloc } else { edge_color },
            ),
        );
    }
    for &(host_id, entrance_id) in &overlay.socket_links {
        let Some(host_pos) = host_pos_lookup(host_id) else {
            continue;
        };
        let Some(entrance_pos) = overlay
            .nodes
            .iter()
            .find(|n| n.id == entrance_id)
            .map(|n| n.screen_pos)
        else {
            continue;
        };
        let alloc =
            character.allocated.contains(&host_id) && character.allocated.contains(&entrance_id);
        painter.line_segment(
            [host_pos, entrance_pos],
            Stroke::new(
                if alloc { 2.0 } else { 1.5 },
                if alloc { edge_color_alloc } else { edge_color },
            ),
        );
    }

    for node in &overlay.nodes {
        let allocated = character.allocated.contains(&node.id);
        let host_alloc = character.allocated.contains(&node.host_socket);
        let (fill, ring, radius_px) = match node.kind {
            NodeKind::Notable => (
                Color32::from_rgb(180, 100, 50),
                Color32::from_rgb(255, 200, 80),
                10.0,
            ),
            NodeKind::JewelSocket => (
                Color32::from_rgb(50, 80, 140),
                Color32::from_rgb(120, 180, 255),
                9.0,
            ),
            // Normal / small.
            _ => (
                Color32::from_rgb(110, 110, 110),
                Color32::from_rgb(220, 220, 220),
                6.0,
            ),
        };
        // Dim the fill when the host socket isn't allocated — the synth nodes
        // are unreachable in that state, mirroring PoB's grey-out behaviour.
        let fill_actual = if host_alloc { fill } else { dim(fill, 0.45) };
        painter.circle_filled(node.screen_pos, radius_px, fill_actual);
        let stroke = if allocated {
            Stroke::new(2.5, ring)
        } else {
            Stroke::new(1.5, dim(ring, 0.6))
        };
        painter.circle_stroke(node.screen_pos, radius_px, stroke);
    }
}

fn dim(c: Color32, scale: f32) -> Color32 {
    let r = (c.r() as f32 * scale).clamp(0.0, 255.0) as u8;
    let g = (c.g() as f32 * scale).clamp(0.0, 255.0) as u8;
    let b = (c.b() as f32 * scale).clamp(0.0, 255.0) as u8;
    Color32::from_rgba_premultiplied(r, g, b, c.a())
}

fn host_world_pos(tree: &PassiveTree, socket_id: NodeId) -> Option<[f32; 2]> {
    let node = tree.nodes.get(&socket_id)?;
    let group = tree.groups.get(&node.group?)?;
    // Node world position = group origin + orbit offset. Notable / jewel
    // sockets generally sit at orbit_index 0 (group centre), so falling back
    // to the group origin is good enough for the host position. The actual
    // synth nodes use a local radial layout around the host that doesn't
    // care about the host's orbit index.
    Some([group.x, group.y])
}

fn place_spec(
    spec: &ClusterJewelSpec,
    host_world: [f32; 2],
    ctx: &ClusterOverlayCtx<'_>,
    overlay: &mut ClusterOverlay,
) {
    // Layout: walk synth nodes in id order and fan them around the host on
    // a ring of radius `synth_radius_world`. Using id order is stable across
    // frames (the synthetic-id scheme is deterministic). PoB places nodes by
    // their assigned slot index so the ring ordering matches the source
    // template; we mimic that by sorting on the id's low 4 bits (ring slot)
    // before fanning, which preserves the "edge connects neighbours on the
    // ring" relationship visually.
    let mut placed: Vec<(NodeId, &pob_data::Node)> =
        spec.nodes.iter().map(|(id, n)| (*id, n)).collect();
    placed.sort_by_key(|(id, _)| (id & 0xF, *id));
    let n = placed.len() as f32;
    if n < 1.0 {
        return;
    }
    for (i, (id, node)) in placed.iter().enumerate() {
        let theta = (i as f32 / n) * std::f32::consts::TAU - std::f32::consts::FRAC_PI_2;
        let world = [
            host_world[0] + theta.cos() * ctx.synth_radius_world,
            host_world[1] + theta.sin() * ctx.synth_radius_world,
        ];
        let screen_pos = (ctx.to_screen)(world);
        overlay.nodes.push(PlacedSynthNode {
            id: *id,
            host_socket: spec.parent_socket,
            kind: node.kind,
            screen_pos,
            world_pos: world,
            display_name: node.name.clone(),
            stat_lines: node.stats.clone(),
        });
    }
    for &(a, b) in &spec.edges {
        if a == spec.parent_socket || b == spec.parent_socket {
            // Host connection — recorded separately so it can use the host's
            // real screen position rather than a synth-overlay one.
            let other = if a == spec.parent_socket { b } else { a };
            overlay.socket_links.push((spec.parent_socket, other));
        } else {
            overlay.edges.push((a, b));
        }
    }
    let _ = ctx.synth_world_size; // currently unused; reserved for hit-test sizing.
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fake_to_screen() -> impl Fn([f32; 2]) -> Pos2 {
        |w| Pos2::new(w[0], w[1])
    }

    fn fake_tree_with_socket() -> PassiveTree {
        // Round-trip a minimal JSON tree fixture so we don't need to pull
        // `smallvec` / `ahash` / `pob_data::TreeConstants` constructors into
        // the UI test scope. The payload mirrors the synthetic tree used in
        // `cluster_synth::tests`.
        let json = r#"{
          "version":"test",
          "tree":"Default",
          "classes":[],
          "groups":{"7":{"x":100.0,"y":200.0,"orbits":[],"background":null,"nodes":[1000],"is_proxy":false}},
          "nodes":{
            "1000":{
              "id":1000,
              "name":"Large Jewel Socket",
              "kind":"jewel_socket",
              "stats":[],
              "reminder_text":[],
              "out_edges":[],
              "in_edges":[],
              "mastery_effects":[],
              "group":7,
              "orbit":0,
              "orbit_index":0,
              "expansion_jewel_size":2
            }
          },
          "jewel_slots":[1000],
          "min_x":0,"min_y":0,"max_x":0,"max_y":0,
          "constants":{
            "skills_per_orbit":[1,6,16,16,40,72,72],
            "orbit_radii":[0,82,162,335,493,662,846],
            "classes":{},
            "character_attributes":{},
            "pss_centre_inner_radius":null
          },
          "points":{}
        }"#;
        pob_data::load_passive_tree(json).expect("decode minimal tree")
    }

    fn empty_cluster_data() -> ClusterJewelData {
        // Round-trip a minimal JSON payload to dodge the lack of `Default`
        // on `ClusterJewelData` without taking an `indexmap` test-dep.
        pob_data::load_cluster_jewels(r#"{"jewels":{}}"#).expect("decode empty payload")
    }

    #[test]
    fn empty_overlay_when_no_jewels_socketed() {
        let tree = fake_tree_with_socket();
        let cj = empty_cluster_data();
        let cm = ClusterModSet::default();
        let character = Character::new(pob_engine::character::ClassRef::marauder(), 90);
        let to_screen = fake_to_screen();
        let ctx = ClusterOverlayCtx {
            character: &character,
            tree: &tree,
            cluster_jewels: &cj,
            cluster_jewel_mods: &cm,
            to_screen: &to_screen,
            synth_radius_world: 200.0,
            synth_world_size: 35.0,
        };
        let overlay = compute_overlay(&ctx);
        assert!(overlay.nodes.is_empty());
        assert!(overlay.edges.is_empty());
        assert!(overlay.socket_links.is_empty());
    }

    #[test]
    fn hit_test_finds_closest_node_within_radius() {
        let nodes = vec![
            PlacedSynthNode {
                id: 0x10000,
                host_socket: 1000,
                kind: NodeKind::Normal,
                screen_pos: Pos2::new(100.0, 100.0),
                world_pos: [0.0, 0.0],
                display_name: None,
                stat_lines: vec![],
            },
            PlacedSynthNode {
                id: 0x10001,
                host_socket: 1000,
                kind: NodeKind::Notable,
                screen_pos: Pos2::new(200.0, 100.0),
                world_pos: [0.0, 0.0],
                display_name: None,
                stat_lines: vec![],
            },
        ];
        let overlay = ClusterOverlay {
            nodes,
            edges: vec![],
            socket_links: vec![],
        };
        // Direct hit on node 0.
        assert_eq!(
            hit_test(&overlay, Pos2::new(100.0, 100.0), 8.0),
            Some(0x10000)
        );
        // Out of radius on both — None.
        assert!(hit_test(&overlay, Pos2::new(150.0, 100.0), 8.0).is_none());
        // Closer to node 1.
        assert_eq!(
            hit_test(&overlay, Pos2::new(198.0, 100.0), 8.0),
            Some(0x10001)
        );
    }

    #[test]
    fn dim_scales_rgb_uniformly() {
        let c = Color32::from_rgba_premultiplied(200, 100, 50, 255);
        let d = dim(c, 0.5);
        assert_eq!(d.r(), 100);
        assert_eq!(d.g(), 50);
        assert_eq!(d.b(), 25);
        assert_eq!(d.a(), 255);
    }
}
