//! Passive tree widget. Renders edges + nodes via egui's `Painter`, supports pan/zoom,
//! click-to-allocate, and hover tooltips.
//!
//! Phase 4a uses egui shapes (lines + circles) — fast enough for the ~3000-node tree at
//! interactive zooms. A wgpu custom paint callback is queued for Phase 6 polish if
//! profiling shows we need it.

use ahash::HashMap;
use eframe::egui::{self, Color32, Pos2, Rect, Sense, Stroke, Vec2};
use pob_data::{NodeId, NodeKind, PassiveTree};

use crate::tree_layout::{compute_node_positions, NodePos};

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

        // Edges first (so nodes draw on top).
        // Use a HashSet of (min, max) pairs to draw each edge once.
        let mut drawn_edges: ahash::HashSet<(NodeId, NodeId)> = ahash::HashSet::default();
        for (id, node) in &tree.nodes {
            let Some(p_a) = self.positions.get(id).copied() else {
                continue;
            };
            for nb_id in node.out_edges.iter().chain(node.in_edges.iter()) {
                if id == nb_id {
                    continue;
                }
                let pair = if id < nb_id { (*id, *nb_id) } else { (*nb_id, *id) };
                if !drawn_edges.insert(pair) {
                    continue;
                }
                let Some(p_b) = self.positions.get(nb_id).copied() else {
                    continue;
                };
                let a = to_screen(p_a);
                let b = to_screen(p_b);
                if !rect_contains_segment(viewport, a, b) {
                    continue;
                }
                let both_alloc = allocated.contains(id) && allocated.contains(nb_id);
                let stroke = if both_alloc {
                    Stroke::new(1.5, Color32::from_rgb(180, 220, 255))
                } else {
                    Stroke::new(0.6, Color32::from_rgb(70, 70, 80))
                };
                painter.line_segment([a, b], stroke);
            }
        }

        // Path overlay (drawn over edges, under nodes).
        if !self.path_overlay.is_empty() {
            for win in self.path_overlay.windows(2) {
                let (a, b) = (win[0], win[1]);
                let (Some(pa), Some(pb)) = (self.positions.get(&a), self.positions.get(&b)) else {
                    continue;
                };
                painter.line_segment(
                    [to_screen(*pa), to_screen(*pb)],
                    Stroke::new(2.5, Color32::from_rgb(255, 200, 80)),
                );
            }
        }

        // Nodes.
        let mut hovered: Option<NodeId> = None;
        let mut clicked: Option<NodeId> = None;
        let pointer = response.hover_pos();

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
            let alloc = allocated.contains(id);
            let (fill, ring) = node_colors(node, alloc);
            painter.circle(s, radius, fill, Stroke::new(1.0, ring));

            // Search-match highlight ring.
            if self.search_matches.contains(id) {
                painter.circle_stroke(
                    s,
                    radius + 3.0,
                    Stroke::new(2.0, Color32::from_rgb(255, 240, 80)),
                );
            }

            if let Some(pp) = pointer {
                if (pp - s).length() < radius + 2.0 {
                    hovered = Some(*id);
                }
            }
        }

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

fn node_radius(node: &pob_data::Node, zoom: f32) -> f32 {
    let world = match node.kind {
        NodeKind::Keystone => 90.0,
        NodeKind::Notable => 70.0,
        NodeKind::Mastery => 80.0,
        NodeKind::JewelSocket => 75.0,
        NodeKind::AscendancyStart | NodeKind::ClassStart => 110.0,
        _ => 35.0,
    };
    (world * zoom).max(2.0)
}

fn node_colors(node: &pob_data::Node, allocated: bool) -> (Color32, Color32) {
    use Color32 as C;
    if allocated {
        let fill = match node.kind {
            NodeKind::Keystone => C::from_rgb(220, 180, 60),
            NodeKind::Notable => C::from_rgb(220, 100, 100),
            NodeKind::Mastery => C::from_rgb(180, 80, 220),
            NodeKind::JewelSocket => C::from_rgb(60, 220, 220),
            _ => C::from_rgb(100, 200, 255),
        };
        return (fill, C::WHITE);
    }
    let fill = match node.kind {
        NodeKind::Keystone => C::from_rgb(120, 100, 50),
        NodeKind::Notable => C::from_rgb(140, 70, 70),
        NodeKind::Mastery => C::from_rgb(100, 60, 130),
        NodeKind::JewelSocket => C::from_rgb(40, 110, 110),
        NodeKind::AscendancyStart | NodeKind::ClassStart => C::from_rgb(80, 80, 110),
        _ => C::from_rgb(70, 70, 90),
    };
    (fill, C::from_rgb(120, 120, 130))
}

fn rect_contains_segment(rect: Rect, a: Pos2, b: Pos2) -> bool {
    rect.expand(50.0).contains(a) || rect.expand(50.0).contains(b)
}
