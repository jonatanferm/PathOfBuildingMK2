//! Passive tree widget. Renders edges via egui shapes and nodes via a wgpu
//! custom-paint callback (`tree_renderer::TreeNodeCallback`).
//!
//! Phase 8a moved node rendering off egui's CPU painter. Edges and the
//! tooltip path stay on egui for now; Phase 8b takes edges to wgpu too.

use ahash::HashMap;
use eframe::egui::{self, Color32, Pos2, Sense, Vec2};
use eframe::egui_wgpu;
use pob_data::{NodeId, NodeKind, PassiveTree};

use crate::tree_layout::{compute_node_positions, NodePos};
use crate::tree_renderer::{
    edge_state_bits, kind_to_u32, state_bits, EdgeInstance, NodeInstance, TreeEdgeCallback,
    TreeNodeCallback,
};

#[derive(Default, Debug, Clone, Copy)]
pub struct TreeInteraction {
    pub hovered: Option<NodeId>,
    pub clicked: Option<NodeId>,
}

pub struct TreeView {
    /// Tree-space origin currently shown at the centre of the viewport.
    center: Vec2,
    /// World-units → screen-pixels.
    zoom: f32,
    /// Cached layout (recomputed on tree change).
    positions: HashMap<NodeId, NodePos>,
    /// Nodes matching the current search filter — drawn with a highlight ring.
    pub search_matches: ahash::HashSet<NodeId>,
    /// Path overlay (set externally, drawn on top of edges).
    pub path_overlay: Vec<NodeId>,
}

impl TreeView {
    pub fn new(tree: &PassiveTree) -> Self {
        Self {
            center: Vec2::ZERO,
            zoom: 0.04,
            positions: compute_node_positions(tree),
            search_matches: ahash::HashSet::default(),
            path_overlay: Vec::new(),
        }
    }

    /// Tree-space position of a node, if known.
    pub fn position_of(&self, id: NodeId) -> Option<NodePos> {
        self.positions.get(&id).copied()
    }

    /// Centre the viewport on a tree-space point.
    pub fn focus(&mut self, x: f32, y: f32) {
        self.center = Vec2::new(x, y);
    }

    pub fn rebind(&mut self, tree: &PassiveTree) {
        self.positions = compute_node_positions(tree);
    }

    /// Render the tree. Returns `(hovered, clicked)` node ids.
    pub fn ui(
        &mut self,
        ui: &mut egui::Ui,
        tree: &PassiveTree,
        allocated: &std::collections::HashSet<NodeId>,
    ) -> TreeInteraction {
        let available = ui.available_rect_before_wrap();
        let response = ui.allocate_rect(available, Sense::click_and_drag());

        // Pan: drag with primary mouse.
        if response.dragged() {
            let delta = response.drag_delta();
            self.center -= Vec2::new(delta.x / self.zoom, delta.y / self.zoom);
        }

        // Zoom: scroll wheel.
        if response.hovered() {
            ui.input(|i| {
                let scroll = i.smooth_scroll_delta.y;
                if scroll.abs() > 0.0 {
                    let factor = (scroll * 0.005).exp();
                    self.zoom = (self.zoom * factor).clamp(0.005, 0.5);
                }
            });
        }

        let painter = ui.painter_at(available);
        painter.rect_filled(available, 0.0, Color32::from_rgb(20, 20, 28));

        let viewport = available;
        let to_screen = |p: NodePos| -> Pos2 {
            Pos2::new(
                viewport.center().x + (p.x - self.center.x) * self.zoom,
                viewport.center().y + (p.y - self.center.y) * self.zoom,
            )
        };

        // Edges (and the path overlay, which rides the same pipeline via bit 1
        // of the state byte). Build a (lo,hi) → state map so duplicate
        // out_edge/in_edge entries collapse into a single instance.
        let path_pairs: ahash::HashSet<(NodeId, NodeId)> = self
            .path_overlay
            .windows(2)
            .map(|w| if w[0] < w[1] { (w[0], w[1]) } else { (w[1], w[0]) })
            .collect();

        let mut edge_state: ahash::HashMap<(NodeId, NodeId), u32> =
            ahash::HashMap::default();
        for (id, node) in &tree.nodes {
            for nb_id in node.out_edges.iter().chain(node.in_edges.iter()) {
                if id == nb_id {
                    continue;
                }
                let pair = if id < nb_id { (*id, *nb_id) } else { (*nb_id, *id) };
                let mut state = 0u32;
                if allocated.contains(&pair.0) && allocated.contains(&pair.1) {
                    state |= edge_state_bits::ALLOCATED;
                }
                if path_pairs.contains(&pair) {
                    state |= edge_state_bits::PATH;
                }
                edge_state.insert(pair, state);
            }
        }

        let mut edges: Vec<EdgeInstance> = Vec::with_capacity(edge_state.len());
        for ((a_id, b_id), state) in edge_state {
            let (Some(pa), Some(pb)) =
                (self.positions.get(&a_id).copied(), self.positions.get(&b_id).copied())
            else {
                continue;
            };
            edges.push(EdgeInstance {
                a: [pa.x, pa.y],
                b: [pb.x, pb.y],
                state,
                _pad: 0,
            });
        }

        // Nodes — hit-test in CPU and emit a single wgpu draw callback for the
        // whole tree. The shader's SDF circle handles fill/ring/search/hover
        // visuals based on the per-instance state byte.
        let mut hovered: Option<NodeId> = None;
        let mut clicked: Option<NodeId> = None;
        let pointer = response.hover_pos();

        // Path-overlay set for fast lookup when building the state byte.
        let path_set: ahash::HashSet<NodeId> =
            self.path_overlay.iter().copied().collect();

        // First pass: hit-test (so `hovered` is known when we build the state
        // byte for the same draw call).
        for (id, node) in &tree.nodes {
            if matches!(node.kind, NodeKind::Root) {
                continue;
            }
            let Some(p) = self.positions.get(id).copied() else {
                continue;
            };
            let s = to_screen(p);
            if !viewport.expand(20.0).contains(s) {
                continue;
            }
            let radius = node_radius(node, self.zoom);
            if let Some(pp) = pointer {
                if (pp - s).length() < radius + 2.0 {
                    hovered = Some(*id);
                }
            }
        }

        // Second pass: build the instance buffer for the wgpu draw call.
        let mut instances: Vec<NodeInstance> = Vec::with_capacity(tree.nodes.len());
        for (id, node) in &tree.nodes {
            if matches!(node.kind, NodeKind::Root) {
                continue;
            }
            let Some(p) = self.positions.get(id).copied() else {
                continue;
            };
            let mut state = 0u32;
            if allocated.contains(id) {
                state |= state_bits::ALLOCATED;
            }
            if self.search_matches.contains(id) {
                state |= state_bits::SEARCH;
            }
            if hovered == Some(*id) {
                state |= state_bits::HOVERED;
            }
            if path_set.contains(id) {
                state |= state_bits::PATH;
            }
            instances.push(NodeInstance {
                world_pos: [p.x, p.y],
                world_radius: world_radius_for(node),
                kind: kind_to_u32(node.kind),
                state,
                _pad: 0,
            });
        }

        // Hand the per-frame state to the wgpu pipelines. Edges first (so node
        // SDFs occlude their endpoints), nodes second.
        let pixels_per_point = ui.ctx().pixels_per_point();
        let viewport_size_px = [
            viewport.width() * pixels_per_point,
            viewport.height() * pixels_per_point,
        ];
        let viewport_center_world = [self.center.x, self.center.y];
        let zoom_px = self.zoom * pixels_per_point;
        painter.add(egui_wgpu::Callback::new_paint_callback(
            viewport,
            TreeEdgeCallback {
                edges,
                viewport_center: viewport_center_world,
                zoom: zoom_px,
                viewport_size: viewport_size_px,
                pixels_per_point,
            },
        ));
        painter.add(egui_wgpu::Callback::new_paint_callback(
            viewport,
            TreeNodeCallback {
                instances,
                viewport_center: viewport_center_world,
                zoom: zoom_px,
                viewport_size: viewport_size_px,
                pixels_per_point,
            },
        ));

        if let Some(id) = hovered {
            if let Some(node) = tree.nodes.get(&id) {
                let label = if let Some(name) = &node.name {
                    name.clone()
                } else {
                    format!("#{id}")
                };
                let mut text = label;
                for s in &node.stats {
                    text.push('\n');
                    text.push_str(s);
                }
                egui::show_tooltip_at_pointer(
                    ui.ctx(),
                    egui::LayerId::new(egui::Order::Tooltip, ui.id()),
                    egui::Id::new(("tree-tooltip", id)),
                    |ui| {
                        ui.label(text);
                    },
                );
            }
            if response.clicked() {
                clicked = Some(id);
            }
        }

        TreeInteraction { hovered, clicked }
    }

    pub fn center(&self) -> Vec2 {
        self.center
    }
    pub fn zoom(&self) -> f32 {
        self.zoom
    }
    pub fn set_view(&mut self, center: Vec2, zoom: f32) {
        self.center = center;
        self.zoom = zoom;
    }
}

/// World-space (pre-zoom) radius for a node. The wgpu pipeline applies zoom
/// inside the shader; the CPU-side hit-test uses `node_radius` below to
/// compute the pixel radius.
fn world_radius_for(node: &pob_data::Node) -> f32 {
    match node.kind {
        NodeKind::Keystone => 90.0,
        NodeKind::Notable => 70.0,
        NodeKind::Mastery => 80.0,
        NodeKind::JewelSocket => 75.0,
        NodeKind::AscendancyStart | NodeKind::ClassStart => 110.0,
        _ => 35.0,
    }
}

fn node_radius(node: &pob_data::Node, zoom: f32) -> f32 {
    (world_radius_for(node) * zoom).max(2.0)
}
