//! Passive tree widget. Renders edges via egui shapes and nodes via a wgpu
//! custom-paint callback (`tree_renderer::TreeNodeCallback`).
//!
//! Phase 8a moved node rendering off egui's CPU painter. Edges and the
//! tooltip path stay on egui for now; Phase 8b takes edges to wgpu too.

use ahash::{HashMap, HashMapExt};
use eframe::egui::{self, Color32, Pos2, Sense, Vec2};
use eframe::egui_wgpu;
use pob_data::{NodeId, NodeKind, PassiveTree};

use crate::tree_layout::{compute_node_positions, NodePos};
use crate::tree_renderer::{
    edge_state_bits, kind_to_u32, state_bits, EdgeInstance, FrameInstance, GroupInstance,
    NodeInstance, TreeEdgeCallback, TreeFrameCallback, TreeGroupCallback, TreeNodeCallback,
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
    /// Per-node icon UV rect into the skills atlas: `[u, v, du, dv]` in
    /// [0, 1]. Empty rect means "no icon — render flat colour".
    icon_uvs: HashMap<NodeId, [f32; 4]>,
    /// Pre-computed `GroupInstance`s for the cluster halos.
    group_instances: Vec<GroupInstance>,
    /// Frame UV rects per kind × state. Indexed [kind][state]: kind is one
    /// of Normal/Notable/Keystone/JewelSocket; state is 0=Unallocated,
    /// 1=CanAllocate, 2=Allocated. Each entry carries the atlas-relative UV
    /// rect plus the frame's native pixel size used as `world_size`.
    frame_table: FrameTable,
    /// Nodes matching the current search filter — drawn with a highlight ring.
    pub search_matches: ahash::HashSet<NodeId>,
    /// Path overlay (set externally, drawn on top of edges).
    pub path_overlay: Vec<NodeId>,
}

#[derive(Default, Clone)]
struct FrameTable {
    /// `[normal, notable, keystone, jewel]`, each Option<[u, v, du, dv, w_px, h_px]>
    /// per state Unallocated/CanAllocate/Allocated. Missing entries skip rendering.
    entries: [[Option<[f32; 6]>; 3]; 4],
}

impl TreeView {
    pub fn new(tree: &PassiveTree, sprites: Option<&pob_data::sprites::SpriteSet>) -> Self {
        Self {
            center: Vec2::ZERO,
            zoom: 0.06,
            positions: compute_node_positions(tree),
            icon_uvs: compute_icon_uvs(tree, sprites),
            group_instances: compute_group_instances(tree, sprites),
            frame_table: compute_frame_table(sprites),
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
        // Sprite metadata stays the same across tree-version swaps; we only
        // re-key the icon-uv table by NodeId. New tree versions would need a
        // matching sprite set — until then keep the existing mapping (icons
        // for missing nodes simply render as flat colours).
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

        // Zoom: scroll wheel. Anchor the zoom at the pointer so the world point
        // under the cursor stays put — matches PoB / poeplanner.
        if response.hovered() {
            let pointer = response.hover_pos();
            ui.input(|i| {
                let scroll = i.smooth_scroll_delta.y;
                if scroll.abs() > 0.0 {
                    let factor = (scroll * 0.005).exp();
                    let new_zoom = (self.zoom * factor).clamp(0.005, 0.5);
                    if let Some(p) = pointer {
                        let vc = available.center();
                        let cursor_world = Vec2::new(
                            (p.x - vc.x) / self.zoom + self.center.x,
                            (p.y - vc.y) / self.zoom + self.center.y,
                        );
                        self.zoom = new_zoom;
                        let cursor_world_after = Vec2::new(
                            (p.x - vc.x) / self.zoom + self.center.x,
                            (p.y - vc.y) / self.zoom + self.center.y,
                        );
                        self.center += cursor_world - cursor_world_after;
                    } else {
                        self.zoom = new_zoom;
                    }
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
                // Don't draw the edges that bridge an ascendancy cluster to the
                // main tree (or a different ascendancy). PoB / poeplanner render
                // ascendancies as floating sub-graphs; we mirror that visually.
                // Pathfinding still walks these edges so click-to-allocate
                // works through them.
                let neighbour = tree.nodes.get(nb_id);
                let asc_a = node.ascendancy_name.as_deref();
                let asc_b = neighbour.and_then(|n| n.ascendancy_name.as_deref());
                if asc_a != asc_b {
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
        let mut frames: Vec<FrameInstance> = Vec::with_capacity(tree.nodes.len());
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
            // Frame ring: per-kind sprite, per-state variant. Pixel size from
            // atlas; we scale to roughly match the node's circle (frame
            // sprites have built-in margin around the node).
            let kind_idx = match node.kind {
                NodeKind::Normal => Some(0usize),
                NodeKind::Notable => Some(1),
                NodeKind::Keystone => Some(2),
                NodeKind::JewelSocket => Some(3),
                _ => None,
            };
            if let Some(ki) = kind_idx {
                let alloc = (state & state_bits::ALLOCATED) != 0;
                let on_path = (state & state_bits::PATH) != 0;
                let si = if alloc {
                    2
                } else if on_path {
                    1
                } else {
                    0
                };
                if let Some(entry) = self.frame_table.entries[ki][si] {
                    let world_size = world_radius_for(node) * 2.0 * FRAME_SCALE;
                    frames.push(FrameInstance {
                        world_pos: [p.x, p.y],
                        world_size: [world_size, world_size],
                        uv_rect: [entry[0], entry[1], entry[2], entry[3]],
                    });
                }
            }
            let icon_uv = self.icon_uvs.get(id).copied().unwrap_or([0.0, 0.0, 0.0, 0.0]);
            instances.push(NodeInstance {
                world_pos: [p.x, p.y],
                world_radius: world_radius_for(node),
                kind: kind_to_u32(node.kind),
                state,
                icon_uv,
            });
        }

        // Hand the per-frame state to the wgpu pipelines. Order matches
        // PoB's `Draw`: group backgrounds first (under everything), then
        // edges (the SDFs cover endpoints), then nodes on top.
        let pixels_per_point = ui.ctx().pixels_per_point();
        let viewport_size_px = [
            viewport.width() * pixels_per_point,
            viewport.height() * pixels_per_point,
        ];
        let viewport_center_world = [self.center.x, self.center.y];
        let zoom_px = self.zoom * pixels_per_point;
        painter.add(egui_wgpu::Callback::new_paint_callback(
            viewport,
            TreeGroupCallback {
                groups: self.group_instances.clone(),
                viewport_center: viewport_center_world,
                zoom: zoom_px,
                viewport_size: viewport_size_px,
                pixels_per_point,
            },
        ));
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
        painter.add(egui_wgpu::Callback::new_paint_callback(
            viewport,
            TreeFrameCallback {
                frames,
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

/// Build a `GroupInstance` per non-proxy group: pick the right
/// `PSGroupBackground{1,2,3}` sprite based on the group's largest orbit
/// (mirrors PoB's `if group.oo[3] then PSGroupBackground3 ...` ladder), and
/// render a quad of native sprite size centered at the group's tree-space
/// position.
fn compute_group_instances(
    tree: &PassiveTree,
    sprites: Option<&pob_data::sprites::SpriteSet>,
) -> Vec<GroupInstance> {
    let mut out = Vec::new();
    let Some(sprites) = sprites else { return out };
    let Some(cat) = sprites.get("groupBackground") else { return out };
    // Sprite-size mapping: per PoB, the native pixel size at scale=1. We
    // scale by 2.5 in the shader-equivalent (matching PoB's apparent ~2.5x
    // factor on background draws — the raw atlas slice is much smaller than
    // the cluster of nodes it backs).
    const BG_SCALE: f32 = 2.5;
    for group in tree.groups.values() {
        if group.is_proxy {
            continue;
        }
        let key = if group.orbits.contains(&3) {
            "PSGroupBackground3"
        } else if group.orbits.contains(&2) {
            "PSGroupBackground2"
        } else if group.orbits.contains(&1) {
            "PSGroupBackground1"
        } else {
            continue;
        };
        let Some(rect) = cat.coords.get(key) else { continue };
        let uv = rect.uv(cat.w as f32, cat.h as f32);
        // PSGroupBackground3 is a 'half image' in PoB — drawn twice, once
        // mirrored — to bridge wide clusters. We approximate as a single
        // wider quad (height stays the sprite height, width doubles).
        let (w, h) = if key == "PSGroupBackground3" {
            (rect.w() * 2.0 * BG_SCALE, rect.h() * BG_SCALE)
        } else {
            (rect.w() * BG_SCALE, rect.h() * BG_SCALE)
        };
        out.push(GroupInstance {
            world_pos: [group.x, group.y],
            world_size: [w, h],
            uv_rect: uv,
        });
    }
    out
}

/// Look up the per-kind/per-state frame sprite from the `frame` category.
/// We index 0=Normal, 1=Notable, 2=Keystone, 3=JewelSocket and
/// 0=Unallocated, 1=CanAllocate, 2=Allocated. `PSSkillFrame` is the only
/// shared normal-node frame — its three "states" are aliases of the same
/// sprite + the `Active`/`Highlighted` variants.
fn compute_frame_table(sprites: Option<&pob_data::sprites::SpriteSet>) -> FrameTable {
    let mut table = FrameTable::default();
    let Some(sprites) = sprites else { return table };
    let Some(cat) = sprites.get("frame") else { return table };
    let aw = cat.w as f32;
    let ah = cat.h as f32;
    let lookup = |key: &str| -> Option<[f32; 6]> {
        let r = cat.coords.get(key)?;
        let uv = r.uv(aw, ah);
        Some([uv[0], uv[1], uv[2], uv[3], r.w(), r.h()])
    };
    // Normal nodes: PSSkillFrame[Active]. No "CanAllocate" art; reuse
    // base for all three states.
    let psf = lookup("PSSkillFrame");
    let psf_active = lookup("PSSkillFrameActive");
    let psf_high = lookup("PSSkillFrameHighlighted");
    table.entries[0][0] = psf;
    table.entries[0][1] = psf_high.or(psf);
    table.entries[0][2] = psf_active.or(psf);
    // Notable
    table.entries[1][0] = lookup("NotableFrameUnallocated");
    table.entries[1][1] = lookup("NotableFrameCanAllocate");
    table.entries[1][2] = lookup("NotableFrameAllocated");
    // Keystone
    table.entries[2][0] = lookup("KeystoneFrameUnallocated");
    table.entries[2][1] = lookup("KeystoneFrameCanAllocate");
    table.entries[2][2] = lookup("KeystoneFrameAllocated");
    // JewelSocket — uses the JewelSocketAlt variants.
    table.entries[3][0] = lookup("JewelSocketAltNormal");
    table.entries[3][1] = lookup("JewelSocketAltCanAllocate");
    table.entries[3][2] = lookup("JewelSocketAltActive");
    table
}

/// Look up each node's atlas-relative icon rect. Picks the sprite category
/// by the node's `kind` (normal → normalActive, notable → notableActive,
/// etc.) so the same icon path can have different rects per category in the
/// atlas — PoB does the same in `node.sprites[node.type:lower()..Active]`.
fn compute_icon_uvs(
    tree: &PassiveTree,
    sprites: Option<&pob_data::sprites::SpriteSet>,
) -> HashMap<NodeId, [f32; 4]> {
    use pob_data::NodeKind;
    let mut out = HashMap::new();
    let Some(sprites) = sprites else {
        return out;
    };
    for (id, node) in &tree.nodes {
        let Some(icon) = node.icon.as_deref() else {
            continue;
        };
        let category = match node.kind {
            NodeKind::Normal => "normalActive",
            NodeKind::Notable => "notableActive",
            NodeKind::Keystone => "keystoneActive",
            // Mastery / JewelSocket / Root / ClassStart / AscendancyStart /
            // Tattoo / Blighted use atlases we don't yet bundle (masteryInactive
            // is in mastery-disabled-3.png; jewelSocket in jewel-3.png). Until
            // we wire those in, those nodes fall back to flat colored circles.
            _ => continue,
        };
        let Some(c) = sprites.get(category) else { continue };
        if let Some(rect) = c.coords.get(icon) {
            out.insert(*id, rect.uv(c.w as f32, c.h as f32));
        }
    }
    out
}

/// Frame ring is drawn slightly larger than the node's SDF circle so it
/// surrounds the icon without clipping it. Empirically ~1.3 looks like the
/// PoE-style ring; the frame sprite has internal padding so this is a
/// world-units scaler against `node_radius * 2`.
const FRAME_SCALE: f32 = 1.3;

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
