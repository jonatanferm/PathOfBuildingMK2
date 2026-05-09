//! Passive tree widget. Renders edges via egui shapes and nodes via a wgpu
//! custom-paint callback (`tree_renderer::TreeNodeCallback`).
//!
//! Phase 8a moved node rendering off egui's CPU painter. Edges and the
//! tooltip path stay on egui for now; Phase 8b takes edges to wgpu too.

use ahash::{HashMap, HashMapExt};
use eframe::egui::{self, Color32, Pos2, Sense, Vec2};
use eframe::egui_wgpu;
use pob_data::{NodeId, NodeKind, PassiveTree};

use crate::tree_layout::{compute_node_positions, orbit_angles_rad, NodePos};
use crate::tree_renderer::{
    edge_state_bits, kind_to_u32, state_bits, ArcInstance, AscendancyInstance, EdgeInstance,
    FrameInstance, GroupInstance, NodeInstance, TreeArcCallback, TreeAscendancyCallback,
    TreeEdgeCallback, TreeFrameCallback, TreeGroupCallback, TreeNodeCallback,
};

#[derive(Default, Debug, Clone, Copy)]
pub struct TreeInteraction {
    pub hovered: Option<NodeId>,
    pub clicked: Option<NodeId>,
    /// Issue #98 (slice 2): right-click on a node opens the tattoo picker.
    pub right_clicked: Option<NodeId>,
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
    /// Issue #110: ascendancy-start medallion instances. One
    /// `AscendancyMiddle` sprite per `AscendancyStart` node, drawn
    /// from the dedicated `ascendancy.png` atlas.
    ascendancy_instances: Vec<AscendancyInstance>,
    /// Issue #110: class-start portrait gating. Cached so we can
    /// detect a class change in `set_active_class` without re-running
    /// the costly `compute_group_instances` walk every frame.
    active_class_index: Option<u32>,
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
            group_instances: compute_group_instances(tree, sprites, None),
            ascendancy_instances: compute_ascendancy_instances(tree, sprites),
            frame_table: compute_frame_table(sprites),
            search_matches: ahash::HashSet::default(),
            path_overlay: Vec::new(),
            active_class_index: None,
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

    /// Issue #110: count of cached AscendancyStart medallion
    /// instances. Class-independent (the ascendancy medallion is
    /// the same sprite regardless of allocated class) so it isn't
    /// recomputed on `set_active_class`. Exposed for tests.
    pub fn ascendancy_instance_count(&self) -> usize {
        self.ascendancy_instances.len()
    }

    /// Issue #110: gate the class-portrait sprites on the player's
    /// allocated class. Each non-allocated class start gets the
    /// `PSStartNodeBackgroundInactive` background instead of its
    /// dedicated portrait. Pass `None` for "no class selected" to
    /// suppress every portrait. No-op when the index hasn't changed
    /// since the last call.
    pub fn set_active_class(
        &mut self,
        class_name: Option<&str>,
        tree: &PassiveTree,
        sprites: Option<&pob_data::sprites::SpriteSet>,
    ) {
        let next = class_name.and_then(class_name_to_start_index);
        if next == self.active_class_index {
            return;
        }
        self.active_class_index = next;
        self.group_instances = compute_group_instances(tree, sprites, next);
    }

    /// Render the tree. Returns `(hovered, clicked)` node ids.
    pub fn ui(
        &mut self,
        ui: &mut egui::Ui,
        tree: &PassiveTree,
        allocated: &std::collections::HashSet<NodeId>,
        tattooed: &std::collections::HashSet<NodeId>,
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
        painter.rect_filled(available, 0.0, Color32::BLACK);

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
            .map(|w| {
                if w[0] < w[1] {
                    (w[0], w[1])
                } else {
                    (w[1], w[0])
                }
            })
            .collect();

        let mut edge_state: ahash::HashMap<(NodeId, NodeId), u32> = ahash::HashMap::default();
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
                let pair = if id < nb_id {
                    (*id, *nb_id)
                } else {
                    (*nb_id, *id)
                };
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

        // Precompute per-orbit angle tables once for arc classification.
        // `orbit_angles_rad(skills_per_orbit[orbit])[orbit_index]` is the
        // angle a node sits at within its group — same source of truth as
        // `compute_node_positions`. Empty when constants are missing.
        let angle_tables: Vec<Vec<f32>> = tree
            .constants
            .skills_per_orbit
            .iter()
            .map(|&n| orbit_angles_rad(n))
            .collect();

        let mut edges: Vec<EdgeInstance> = Vec::with_capacity(edge_state.len());
        let mut arcs: Vec<ArcInstance> = Vec::new();
        for ((a_id, b_id), state) in edge_state {
            let (Some(pa), Some(pb)) = (
                self.positions.get(&a_id).copied(),
                self.positions.get(&b_id).copied(),
            ) else {
                continue;
            };
            // Try to lift this edge to a curved arc when both endpoints
            // sit on the same orbit of the same group. PoB renders these
            // with cropped orbit-line sprites; we tessellate analytically
            // in the arc shader instead, which keeps the curve crisp at
            // any zoom.
            if let Some(arc) = try_classify_arc(tree, &angle_tables, a_id, b_id, state) {
                arcs.push(arc);
            } else {
                edges.push(EdgeInstance {
                    a: [pa.x, pa.y],
                    b: [pb.x, pb.y],
                    state,
                    _pad: 0,
                });
            }
        }

        // Nodes — hit-test in CPU and emit a single wgpu draw callback for the
        // whole tree. The shader's SDF circle handles fill/ring/search/hover
        // visuals based on the per-instance state byte.
        let mut hovered: Option<NodeId> = None;
        let mut clicked: Option<NodeId> = None;
        let mut right_clicked: Option<NodeId> = None;
        let pointer = response.hover_pos();

        // Path-overlay set for fast lookup when building the state byte.
        let path_set: ahash::HashSet<NodeId> = self.path_overlay.iter().copied().collect();

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
            let icon_uv = self
                .icon_uvs
                .get(id)
                .copied()
                .unwrap_or([0.0, 0.0, 0.0, 0.0]);
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
        // Issue #110: ascendancy medallions are drawn alongside the
        // group halos (under edges + nodes). Same pipeline as group
        // backgrounds, different atlas — so it's a separate callback
        // rather than appending to `group_instances`.
        painter.add(egui_wgpu::Callback::new_paint_callback(
            viewport,
            TreeAscendancyCallback {
                ascendancies: self.ascendancy_instances.clone(),
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
            TreeArcCallback {
                arcs,
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

        // Issue #98 (slice 3): tattoo badge overlay. PoB renders a small T-style badge
        // on each tattooed node so the user can spot at a glance which nodes are
        // overridden. We paint a filled gold ring on top of the wgpu node draw — the
        // egui painter runs after the wgpu callbacks within the same layer, so the
        // badge always sits on top of the regular ring.
        if !tattooed.is_empty() {
            let badge_color = Color32::from_rgb(255, 198, 41); // PoB's tattoo gold
            for &id in tattooed {
                let Some(node) = tree.nodes.get(&id) else {
                    continue;
                };
                let Some(p) = self.positions.get(&id).copied() else {
                    continue;
                };
                let s = to_screen(p);
                if !viewport.expand(20.0).contains(s) {
                    continue;
                }
                let radius = node_radius(node, self.zoom);
                // Outer accent ring sits just outside the node's existing ring.
                painter.circle_stroke(s, radius + 2.5, egui::Stroke::new(2.0, badge_color));
                // Centre dot picks the same accent so the marker reads against any
                // background (active group bg, search highlight, etc.).
                painter.circle_filled(s, (radius * 0.18).max(2.0), badge_color);
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
            if response.secondary_clicked() {
                right_clicked = Some(id);
            }
        }

        TreeInteraction {
            hovered,
            clicked,
            right_clicked,
        }
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

/// Map a class name (e.g. "Witch", "Marauder") to its canonical
/// `class_start_index` per the PoE convention used in tree node data.
/// Indices are stable across tree versions: 0=Scion, 1=Marauder,
/// 2=Ranger, 3=Witch, 4=Duelist, 5=Templar, 6=Shadow.
fn class_name_to_start_index(name: &str) -> Option<u32> {
    Some(match name {
        "Scion" => 0,
        "Marauder" => 1,
        "Ranger" => 2,
        "Witch" => 3,
        "Duelist" => 4,
        "Templar" => 5,
        "Shadow" => 6,
        _ => return None,
    })
}

/// Build a `GroupInstance` per non-proxy group: pick the right
/// `PSGroupBackground{1,2,3}` sprite based on the group's largest orbit
/// (mirrors PoB's `if group.oo[3] then PSGroupBackground3 ...` ladder), and
/// render a quad of native sprite size centered at the group's tree-space
/// position.
fn compute_group_instances(
    tree: &PassiveTree,
    sprites: Option<&pob_data::sprites::SpriteSet>,
    active_class_index: Option<u32>,
) -> Vec<GroupInstance> {
    let mut out = Vec::new();
    let Some(sprites) = sprites else { return out };
    let Some(cat) = sprites.get("groupBackground") else {
        return out;
    };
    // PoB blits these sprites at native pixel size (sprite pixels == tree
    // units). PSGroupBackground1=138², 2=178² match their target orbit
    // radii (82, 162) closely enough to look right.
    //
    // Class-start medallions live in the same atlas (we packed them in
    // alongside the PSGroupBackground sprites). They're drawn after the
    // halo so the portrait sits on top of it — PoB does the same with a
    // higher draw layer for the start-art overlay.
    const CLASS_CENTER_KEYS: [&str; 7] = [
        "centerscion",
        "centermarauder",
        "centerranger",
        "centerwitch",
        "centerduelist",
        "centertemplar",
        "centershadow",
    ];

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
        let Some(rect) = cat.coords.get(key) else {
            continue;
        };
        let uv = rect.uv(cat.w as f32, cat.h as f32);
        let (w, h) = (rect.w(), rect.h());
        if key == "PSGroupBackground3" {
            // PoB draws BG3 as two halves vertically — top sprite at
            // (y - h/2), bottom sprite at (y + h/2) with V flipped, so the
            // full halo spans 2*h. The bottom instance gets a UV rect with
            // negated `dv` so the shader's local_uv.y * dv mapping flips
            // the sample direction.
            out.push(GroupInstance {
                world_pos: [group.x, group.y - h * 0.5],
                world_size: [w, h],
                uv_rect: uv,
            });
            out.push(GroupInstance {
                world_pos: [group.x, group.y + h * 0.5],
                world_size: [w, h],
                uv_rect: [uv[0], uv[1] + uv[3], uv[2], -uv[3]],
            });
        } else {
            out.push(GroupInstance {
                world_pos: [group.x, group.y],
                world_size: [w, h],
                uv_rect: uv,
            });
        }
    }

    // Class-start medallions: a `centerXxx` portrait sprite per class. PoB
    // draws the per-class portrait (centerwitch.png, centermarauder.png,
    // …) at the ClassStart node's position. We treat it as another group
    // sprite so it shares the same pipeline.
    for node in tree.nodes.values() {
        if !matches!(node.kind, pob_data::NodeKind::ClassStart) {
            continue;
        }
        let Some(idx) = node.class_start_index else {
            continue;
        };
        // Issue #110: only the allocated class shows its dedicated
        // portrait. The other six start nodes get the inactive
        // background sprite — matching upstream PoB. Passing `None`
        // for `active_class_index` falls back to the inactive
        // background for every class (initial scratch build state).
        let key = match active_class_index {
            Some(active) if active == idx => CLASS_CENTER_KEYS
                .get(idx as usize)
                .copied()
                .unwrap_or("PSStartNodeBackgroundInactive"),
            _ => "PSStartNodeBackgroundInactive",
        };
        let Some(rect) = cat.coords.get(key) else {
            continue;
        };
        let group = match node.group.and_then(|gid| tree.groups.get(&gid)) {
            Some(g) => g,
            None => continue,
        };
        let uv = rect.uv(cat.w as f32, cat.h as f32);
        out.push(GroupInstance {
            world_pos: [group.x, group.y],
            world_size: [rect.w(), rect.h()],
            uv_rect: uv,
        });
    }

    // Issue #110 part 2: AscendancyStart medallions are emitted by
    // `compute_ascendancy_instances` against a different atlas
    // (`ascendancy.png`); they ride a dedicated callback so the
    // shader can sample from that texture. See below.

    out
}

/// Issue #110: build one instance per `AscendancyStart` node sampling
/// the `AscendancyMiddle` sprite from the dedicated `ascendancy`
/// atlas. The medallion is small (~31x31 in the atlas) and we render
/// it at native pixel size. Returns an empty vec when the atlas or
/// rect lookup fails — the caller's pipeline tolerates an empty buffer.
fn compute_ascendancy_instances(
    tree: &PassiveTree,
    sprites: Option<&pob_data::sprites::SpriteSet>,
) -> Vec<AscendancyInstance> {
    let mut out = Vec::new();
    let Some(sprites) = sprites else { return out };
    let Some(cat) = sprites.get("ascendancy") else {
        return out;
    };
    let Some(rect) = cat.coords.get("AscendancyMiddle") else {
        return out;
    };
    let uv = rect.uv(cat.w as f32, cat.h as f32);
    // PoB blits AscendancyMiddle at 2x its native pixel size in
    // `Draw` (sprite is 31x31, drawn at ~62px). Mirror that so the
    // medallion reads at the same scale relative to the ascendancy
    // sub-tree's halo.
    const ASCENDANCY_MIDDLE_SCALE: f32 = 2.0;
    let world_w = rect.w() * ASCENDANCY_MIDDLE_SCALE;
    let world_h = rect.h() * ASCENDANCY_MIDDLE_SCALE;
    for node in tree.nodes.values() {
        if !matches!(node.kind, pob_data::NodeKind::AscendancyStart) {
            continue;
        }
        let Some(group) = node.group.and_then(|gid| tree.groups.get(&gid)) else {
            continue;
        };
        out.push(AscendancyInstance {
            world_pos: [group.x, group.y],
            world_size: [world_w, world_h],
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
    let Some(cat) = sprites.get("frame") else {
        return table;
    };
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
            // Mastery node icons (`MasteryGroupTwoHands.png` etc.) live in
            // the `mastery` sprite category — different atlas (`mastery-3.png`)
            // than normal/notable/keystone. The shader picks atlas by kind.
            NodeKind::Mastery => "mastery",
            // JewelSocket / Root / ClassStart / AscendancyStart / Tattoo /
            // Blighted: jewel sockets need a separate sprite-name lookup
            // (the tree's `icon` field is "MasteryBlank.png" for all of them
            // — the actual variant comes from elsewhere). Fall back to flat
            // colors for now.
            _ => continue,
        };
        let Some(c) = sprites.get(category) else {
            continue;
        };
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

/// Detect orbital connectors. An edge is "arc-able" when both endpoints
/// belong to the same group and the same orbit; PoB's tree only ever
/// connects nodes along an orbit (curved) or along a radial spoke
/// (straight), so this single check is enough.
///
/// Returns `None` for cross-group, cross-orbit, missing-group, or
/// orbit-0 (group centre) edges — those fall back to straight lines.
/// On success, normalises the angle delta into `[-π, π]` so the shader's
/// linear interpolation walks the short way around. Single-segment short
/// arcs (delta < ~3°) also fall back to straight lines: the radial cost
/// of tessellating them isn't worth a curve indistinguishable from a line.
fn try_classify_arc(
    tree: &PassiveTree,
    angle_tables: &[Vec<f32>],
    a_id: NodeId,
    b_id: NodeId,
    state: u32,
) -> Option<ArcInstance> {
    let na = tree.nodes.get(&a_id)?;
    let nb = tree.nodes.get(&b_id)?;
    let group_id = na.group?;
    if Some(group_id) != nb.group {
        return None;
    }
    let orbit = na.orbit?;
    if Some(orbit) != nb.orbit {
        return None;
    }
    if orbit == 0 {
        // Orbit 0 is the group centre — a single node at radius 0; an
        // edge there can't curve. (Defensive — shouldn't appear in real
        // trees but we guard against zero radius below regardless.)
        return None;
    }
    let group = tree.groups.get(&group_id)?;
    let radius = *tree.constants.orbit_radii.get(orbit as usize)? as f32;
    if radius <= 0.0 {
        return None;
    }
    let table = angle_tables.get(orbit as usize)?;
    let ai = na.orbit_index? as usize;
    let bi = nb.orbit_index? as usize;
    let angle_a = *table.get(ai)?;
    let angle_b_raw = *table.get(bi)?;

    // Normalise (b - a) into [-π, π] so the shader lerps along the
    // shorter arc, then express b as `a + delta`.
    let two_pi = std::f32::consts::TAU;
    let mut delta = angle_b_raw - angle_a;
    while delta > std::f32::consts::PI {
        delta -= two_pi;
    }
    while delta < -std::f32::consts::PI {
        delta += two_pi;
    }
    if delta.abs() < 0.06 {
        // <~3° — chord ≈ arc; not worth a curve.
        return None;
    }
    Some(ArcInstance {
        center: [group.x, group.y],
        radius,
        angle_a,
        angle_b: angle_a + delta,
        state,
        _pad: [0; 2],
    })
}

#[cfg(test)]
mod tests {
    use super::class_name_to_start_index;

    #[test]
    fn class_name_maps_to_pob_canonical_index() {
        // Indices are stable PoE convention: 0..6 in this exact order.
        assert_eq!(class_name_to_start_index("Scion"), Some(0));
        assert_eq!(class_name_to_start_index("Marauder"), Some(1));
        assert_eq!(class_name_to_start_index("Ranger"), Some(2));
        assert_eq!(class_name_to_start_index("Witch"), Some(3));
        assert_eq!(class_name_to_start_index("Duelist"), Some(4));
        assert_eq!(class_name_to_start_index("Templar"), Some(5));
        assert_eq!(class_name_to_start_index("Shadow"), Some(6));
        // Unknown / lowercase / empty all collapse to None so the
        // tree view falls back to the inactive background sprite for
        // every class start.
        assert_eq!(class_name_to_start_index(""), None);
        assert_eq!(class_name_to_start_index("witch"), None);
        assert_eq!(class_name_to_start_index("Necromancer"), None);
    }
}
