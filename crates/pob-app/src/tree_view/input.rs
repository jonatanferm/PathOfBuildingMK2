//! Port of PathOfBuilding v2.67.2 (16de4b82d) src/Classes/PassiveTreeView.lua:1-1411 (Zoom, Focus, input handling).
//! Owner: U23. The first half of `PassiveTreeView:Draw` (L197-544: input events, dragging,
//! the zoom clamp, the coordinate transforms, path tracing, the hovered node and the clicks),
//! `Zoom` (L1307-1320) and `Focus` (L1323-1332); `draw.rs` continues Draw with [`Frame`].
//!
//! @pob-file Classes/PassiveTreeView.lua @16de4b82d
//! @pob-file Modules/ItemTools.lua @16de4b82d
//!
//! Draw's locals that cross into the drawing half are the fields of [`Frame`]. `return` inside
//! Draw (L501, after the class-change confirm popup) ends the whole Draw: [`Frame`] is `None`.
//!
//! ORDER:
//! * `pairs(spec.nodes)` (L307) and `pairs(self.compareSpec.nodes)` (L320) stop at the first node
//!   whose hit circle (`node.rsq`) holds the cursor. The circles of drawn nodes do not overlap
//!   (node frames never overlap on the tree), so the order does not matter (c).
//! * `pairs(self.tracePath)` / `pairs(hoverNode.path)` / `pairs(hoverNode.depends)` (L371, L378,
//!   L383) fill sets (c). `pairs(spec.tree.alternate_ascendancies)` (L413),
//!   `pairs(spec.curClass.classes)` (L434) and `pairs(spec.tree.classes)` (L449-450) look for the
//!   one entry whose `id` is the node's ascendancy (ids are unique) (c).
//!
//! DIVERGENCE: none beyond the hooks (mod.rs, CCR-0115).

use std::collections::HashSet;
use std::rc::Rc;

use pob_lua::math::{lpow, m_max, m_min};
use pob_lua::{n, Shared};
use pob_ui_core::prelude::*;

use super::assets::{tree_assets, TreeAssets};
use super::{hooks, PassiveTreeView};
use crate::app::CxApp;
use crate::model::{BuildRef, Node, NodeRef, PassiveSpecRef, PassiveTreeRef, Table};

/// A set of nodes by identity (`hoverPath[node] = true`).
#[derive(Default)]
pub struct NodeSet(HashSet<usize>);

impl NodeSet {
    fn key(n: &Node) -> usize {
        std::ptr::from_ref(n) as usize
    }
    pub fn insert(&mut self, n: &Node) {
        self.0.insert(Self::key(n));
    }
    /// `set[node]`
    pub fn has(&self, n: &Node) -> bool {
        self.0.contains(&Self::key(n))
    }
}

/// Draw's locals shared by its two halves.
pub struct Frame {
    /// `build` (the model) and `build.spec`, `spec.tree`
    pub build: BuildRef,
    pub spec: PassiveSpecRef,
    pub tree: PassiveTreeRef,
    /// `tree.assets`, `tree.spriteMap`, `tree.connectors`, ... (the UI half of the tree)
    pub assets: Rc<TreeAssets>,
    /// `cursorX, cursorY`
    pub cursor_x: f64,
    pub cursor_y: f64,
    /// `scale`, `offsetX`, `offsetY` (treeToScreen)
    pub scale: f64,
    pub offset_x: f64,
    pub offset_y: f64,
    /// `hoverNode` (after path tracing), `hoverCompareNode`
    pub hover_node: Option<NodeRef>,
    pub hover_compare_node: Option<NodeRef>,
    /// `hoverPath`, `hoverDep`
    pub hover_path: Option<NodeSet>,
    pub hover_dep: Option<NodeSet>,
    /// `splitPersonalityPath` (`spec.splitPersonalityPath or { }`)
    pub split_personality_path: Option<Table>,
}

impl Frame {
    /// lua: PassiveTreeView.lua:288-291 `treeToScreen(x, y)`
    pub fn tree_to_screen(&self, x: f64, y: f64) -> (f64, f64) {
        (x * self.scale + self.offset_x, y * self.scale + self.offset_y)
    }

    /// lua: PassiveTreeView.lua:292-295 `screenToTree(x, y)`
    pub fn screen_to_tree(&self, x: f64, y: f64) -> (f64, f64) {
        ((x - self.offset_x) / self.scale, (y - self.offset_y) / self.scale)
    }

    /// `splitPersonalityPath[id]`
    pub fn split_personality(&self, id: f64) -> bool {
        self.split_personality_path
            .as_ref()
            .is_some_and(|t| t.truthy(id))
    }
}

/// `isValueInArray(list, node)` by identity: the 1-based index.
fn index_in(list: &[NodeRef], node: &NodeRef) -> Option<usize> {
    list.iter().position(|n| Rc::ptr_eq(n, node)).map(|i| i + 1)
}

/// `a == b` for optional nodes.
pub fn same(a: Option<&NodeRef>, b: Option<&NodeRef>) -> bool {
    match (a, b) {
        (Some(a), Some(b)) => Rc::ptr_eq(a, b),
        (None, None) => true,
        _ => false,
    }
}

/// A position field Lua does arithmetic on (`NaN` is the model's nil, CCR-0024 §2).
#[track_caller]
pub fn pos(x: f64, what: &str) -> f64 {
    if x.is_nan() {
        lua_error!("attempt to perform arithmetic on field '{what}' (a nil value)");
    }
    x
}

/// lua: ItemTools.lua:404-409 `itemLib.wiki.open(name)`: the item tools' UI half. No `@pob`
/// marker: calc:IT-C's `@pob-skip` of 384-413 (`item_tools.rs`) covers the block; no UI unit owns
/// ItemTools.lua, so its UI callers (here, `controls/gem_select.rs`, `tabs/items/slot.rs`) each
/// carry an unmarked copy (CCR-0107 §4; FS1: one marker per Lua line).
fn wiki_open(cx: &mut Cx, name: &str) {
    // L405
    let route = pob_lua::gsub(name, " ", "_", None).0;
    // L407
    cx.sys
        .platform
        .open_url(&format!("https://www.poewiki.net/wiki/{route}"));
    // L408
    cx.app_mut().main.item_lib_wiki_triggered = true;
}

/// `spec:AddUndoState()` (U25 installs it in U19's hooks).
fn add_undo_state(cx: &mut Cx) {
    let f = crate::build::init::hooks(cx).spec_add_undo_state.clone();
    f(cx);
}

/// `spec:SetWindowTitleWithBuildClass()`
fn set_window_title(cx: &mut Cx) {
    crate::build::init::set_window_title_with_build_class(cx);
}

/// `x.type`
fn ty(n: &Node) -> String {
    n.r#type.get().to_string()
}

impl PassiveTreeView {
    /// The input half of `PassiveTreeView:Draw(build, viewPort, inputEvents)` (module notes).
    /// lua: PassiveTreeView.lua:197-544
    /// @pob Classes/PassiveTreeView.lua:197-544 h=7f2243d9
    pub fn draw_input(
        &mut self,
        cx: &mut Cx,
        view_port: &ViewPort,
        input_events: &mut InputEvents,
    ) -> Option<Frame> {
        let build = cx.build().model.clone();
        // L198
        let spec = build.spec.get();
        // L199
        let tree = spec.tree.get();
        let assets = tree_assets(cx, &tree);

        // L201
        let (cursor_x, cursor_y) = cx.sys.cursor_pos();
        // L202
        let m_over = cursor_x >= view_port.x
            && cursor_x < view_port.x + view_port.width
            && cursor_y >= view_port.y
            && cursor_y < view_port.y + view_port.height;

        // Process input events
        // L205
        let mut tree_click: Option<&'static str> = None;
        // L206
        let mut id = 0;
        while let Some(event) = input_events.next_ipairs(&mut id) {
            match event.kind {
                // L207
                EventKind::KeyDown { .. } => {
                    let key = event.key;
                    // L208
                    if key == Key::LeftButton {
                        // L209
                        if m_over {
                            // Record starting coords of mouse drag
                            // Dragging won't actually commence unless the cursor moves far enough
                            // L212
                            self.drag_x = Some(cursor_x);
                            self.drag_y = Some(cursor_y);
                        }
                    } else if key == Key::Char(b'p') {
                        // L215
                        self.show_heat_map = !self.show_heat_map;
                    } else if key == Key::Char(b'd') && cx.sys.is_key_down(Key::Ctrl) {
                        // L217
                        self.show_stat_differences = !self.show_stat_differences;
                    } else if key == Key::Char(b'c')
                        && cx.sys.is_key_down(Key::Ctrl)
                        && self
                            .hover_node
                            .as_ref()
                            .is_some_and(|h| &*h.r#type.get() != "Socket")
                    {
                        let hover = self.hover_node.clone().unwrap();
                        // L219
                        let mut result = format!("# {}\n", hover.dn.get());
                        // L220
                        for line in hover.sd.get().snapshot() {
                            // L221
                            result.push_str(&line);
                            result.push('\n');
                        }
                        // L223
                        cx.sys.platform.copy(&result);
                    } else if key == Key::PageUp {
                        // L225
                        let l = if cx.sys.is_key_down(Key::Shift) { 3.0 } else { 1.0 };
                        self.zoom(cx, l, view_port);
                    } else if key == Key::PageDown {
                        // L227
                        let l = if cx.sys.is_key_down(Key::Shift) { -3.0 } else { -1.0 };
                        self.zoom(cx, l, view_port);
                    } else if key == Key::F(1) && self.hover_node.is_some() {
                        // L228 itemLib.wiki.matchesKey(event.key) (wiki.key = "F1")
                        let hover = self.hover_node.clone().unwrap();
                        let name = hover
                            .extra
                            .str(n!("name"))
                            .map_or_else(|| hover.dn.get().to_string(), |s| s.to_string());
                        // L229
                        wiki_open(cx, &name);
                    }
                }
                // L231
                EventKind::KeyUp => {
                    let key = event.key;
                    // L232
                    if key == Key::LeftButton {
                        // L233
                        if self.drag_x.is_some() && !self.dragging {
                            // Mouse button went down, but didn't move far enough to trigger drag, so register a normal click
                            // L235
                            tree_click = Some("LEFT");
                        }
                    } else if m_over {
                        // L238
                        if key == Key::RightButton {
                            // L239
                            tree_click = Some("RIGHT");
                        } else if key == Key::WheelUp {
                            // L241
                            let l = if cx.sys.is_key_down(Key::Shift) { 3.0 } else { 1.0 };
                            self.zoom(cx, l, view_port);
                        } else if key == Key::WheelDown {
                            // L243
                            let l = if cx.sys.is_key_down(Key::Shift) { -3.0 } else { -1.0 };
                            self.zoom(cx, l, view_port);
                        }
                    }
                }
                EventKind::Char => {}
            }
        }

        // L249
        if !cx.sys.is_key_down(Key::LeftButton) {
            // Left mouse button isn't down, stop dragging if dragging was in progress
            // L251-252
            self.dragging = false;
            self.drag_x = None;
            self.drag_y = None;
        }
        // L254
        if let (Some(dx), Some(dy)) = (self.drag_x, self.drag_y) {
            // Left mouse is down
            // L256
            if !self.dragging {
                // Check if mouse has moved more than a few pixels, and if so, initiate dragging
                // L258
                if (cursor_x - dx).abs() > 5.0 || (cursor_y - dy).abs() > 5.0 {
                    // L259
                    self.dragging = true;
                }
            }
            // L262
            if self.dragging {
                // L263-265
                self.zoom_x = Some(pos_opt(self.zoom_x, "zoomX") + cursor_x - dx);
                self.zoom_y = Some(pos_opt(self.zoom_y, "zoomY") + cursor_y - dy);
                self.drag_x = Some(cursor_x);
                self.drag_y = Some(cursor_y);
            }
        }

        // Ctrl-click to zoom
        // L270
        if tree_click.is_some() && cx.sys.is_key_down(Key::Ctrl) {
            // L271
            let l = if tree_click == Some("RIGHT") { -2.0 } else { 2.0 };
            self.zoom(cx, l, view_port);
            // L272
            tree_click = None;
        }

        // Clamp zoom offset
        // L276
        let clamp_factor = self.zoom * 2.0 / 3.0;
        // L277-278
        self.zoom_x = Some(self.zoom_x.map_or(1.0, |zx| {
            m_min(
                m_max(zx, -view_port.width * clamp_factor),
                view_port.width * clamp_factor,
            )
        }));
        self.zoom_y = Some(self.zoom_y.map_or(1.0, |zy| {
            m_min(
                m_max(zy, -view_port.height * clamp_factor),
                view_port.height * clamp_factor,
            )
        }));

        // Create functions that will convert coordinates between the screen and tree coordinate spaces
        // L281
        let scale = m_min(view_port.width, view_port.height) / tree.extra.num(n!("size")) * self.zoom;
        // L282-283
        let offset_x = self.zoom_x.unwrap() + view_port.x + view_port.width / 2.0;
        let offset_y = self.zoom_y.unwrap() + view_port.y + view_port.height / 2.0;
        let mut f = Frame {
            build: build.clone(),
            spec: spec.clone(),
            tree: tree.clone(),
            assets,
            cursor_x,
            cursor_y,
            scale,
            offset_x,
            offset_y,
            hover_node: None,
            hover_compare_node: None,
            hover_path: None,
            hover_dep: None,
            split_personality_path: None,
        };

        // L293
        if cx.sys.is_key_down(Key::Shift) {
            // Enable path tracing mode
            // L295-296
            self.trace_mode = true;
            if self.trace_path.is_none() {
                self.trace_path = Some(Rc::new(Shared::new(Vec::new())));
            }
        } else {
            // L298-299
            self.trace_mode = false;
            self.trace_path = None;
        }

        // L302-303
        let mut hover_node: Option<NodeRef> = None;
        let mut hover_compare_node: Option<NodeRef> = None; // Track compare-only node hover separately
                                                            // L304
        if m_over {
            // Cursor is over the tree, check if it is over a node
            // L306
            let (cur_tree_x, cur_tree_y) = f.screen_to_tree(cursor_x, cursor_y);
            // L307 (ORDER: module notes)
            for (_, node) in spec.nodes.with(pob_lua::LjIntMap::pairs) {
                // L308
                let rsq = node.extra.get(n!("rsq"));
                if !rsq.truthy() {
                    continue;
                }
                // U53 perf pass: L312's distance test first, as a filter (most nodes are far
                // from the cursor; it saves the two `isProxy` lookups). The reads have no side
                // effects, and a node it lets through takes Lua's path below (a nil position
                // goes there too, so the arithmetic error is raised where Lua raises it).
                let (nx, ny) = (node.x.get(), node.y.get());
                let near = nx.is_nan() || ny.is_nan() || {
                    let (v_x, v_y) = (cur_tree_x - nx, cur_tree_y - ny);
                    v_x * v_x + v_y * v_y <= rsq.num()
                };
                if !near {
                    continue;
                }
                let Some(group) = node.group.get() else { continue };
                if !node.extra.truthy(n!("isProxy")) && !group.truthy(n!("isProxy")) {
                    // Node has a defined size (i.e. has artwork)
                    // L310-311
                    let v_x = cur_tree_x - pos(nx, "x");
                    let v_y = cur_tree_y - pos(ny, "y");
                    // L312
                    if v_x * v_x + v_y * v_y <= rsq.num() {
                        // L313
                        hover_node = Some(node);
                        // L314
                        break;
                    }
                }
            }
            // If not hovering a primary node, check compare-only nodes (e.g. cluster jewel subgraph nodes)
            // L319
            if hover_node.is_none() {
                if let Some(cs) = self.compare_spec.clone() {
                    // L320 (ORDER: module notes)
                    for (node_id, c_node) in cs.nodes.with(pob_lua::LjIntMap::pairs) {
                        let rsq = c_node.extra.get(n!("rsq"));
                        let cty = ty(&c_node);
                        // L321-322
                        if !spec.nodes.with(|m| m.contains_key(node_id))
                            && c_node.alloc.get()
                            && rsq.truthy()
                            && !c_node.x.get().is_nan()
                            && !c_node.y.get().is_nan()
                            && cty != "ClassStart"
                            && cty != "AscendClassStart"
                        {
                            // L323-324
                            let v_x = cur_tree_x - c_node.x.get();
                            let v_y = cur_tree_y - c_node.y.get();
                            // L325
                            if v_x * v_x + v_y * v_y <= rsq.num() {
                                // L326
                                hover_compare_node = Some(c_node);
                                // L327
                                break;
                            }
                        }
                    }
                }
            }
        }

        // L334
        self.hover_node = hover_node.clone();
        // If hovering over a node, find the path to it (if unallocated) or the list of dependent nodes (if allocated)
        // L336
        let mut hover_path: Option<NodeSet> = None;
        let mut hover_dep: Option<NodeSet> = None;
        // L337
        if self.trace_mode {
            let trace_path = self.trace_path.clone().expect("tracePath");
            // Path tracing mode is enabled
            // L339
            if let Some(hn) = hover_node.clone() {
                let hn_path = hn.path.get();
                // L340
                if hn_path.is_none() {
                    // Don't highlight the node if it can't be pathed to
                    // L342
                    hover_node = None;
                } else if let Some(hn_path) = hn_path.filter(|_| trace_path.is_empty()) {
                    // Initialise the trace path using this node's path
                    // L345
                    for path_node in hn_path.snapshot() {
                        // L346
                        trace_path.insert(1, path_node);
                    }
                } else {
                    // L349
                    let last_path_node = trace_path.get(trace_path.len()).expect("tracePath");
                    // L350
                    if !Rc::ptr_eq(&hn, &last_path_node) {
                        // If node is directly linked to the last node in the path, add it
                        // L352
                        if index_in(&hn.linked.get().snapshot(), &last_path_node).is_some() {
                            // L353
                            let index = index_in(&trace_path.snapshot(), &hn);
                            // L354
                            if let Some(index) = index {
                                // Node is already in the trace path, remove it first
                                // L356-357
                                trace_path.remove(Some(index));
                                trace_path.push(hn.clone());
                            } else if &*last_path_node.r#type.get() == "Mastery" {
                                // L359
                                hover_node = None;
                            } else {
                                // L361
                                trace_path.push(hn.clone());
                            }
                        } else {
                            // L364
                            hover_node = None;
                        }
                    }
                }
            }
            // Use the trace path as the path
            // L370
            let mut hp = NodeSet::default();
            // L371 (ORDER: module notes)
            for path_node in trace_path.snapshot() {
                // L372
                hp.insert(&path_node);
            }
            hover_path = Some(hp);
        } else if let Some(hn) = hover_node.clone().filter(|h| h.path.get().is_some()) {
            // Use the node's own path and dependence list
            // L376
            let mut hp = NodeSet::default();
            // L377
            if super::draw::intuitive_leap_likes_affecting_len(&hn) == 0 {
                // L378 (ORDER: module notes)
                for path_node in hn.path.get().unwrap().snapshot() {
                    // L379
                    hp.insert(&path_node);
                }
            }
            hover_path = Some(hp);
            // L382
            let mut hd = NodeSet::default();
            // L383 (ORDER: module notes)
            for dep_node in hn.depends.get().snapshot() {
                // L384
                hd.insert(&dep_node);
            }
            hover_dep = Some(hd);
        }

        // Split Personality highlight
        // L389
        f.split_personality_path = spec.splitPersonalityPath.get();

        // L391
        if tree_click == Some("LEFT") {
            // L392
            if let Some(hn) = hover_node.clone() {
                if !self.left_click(cx, &build, &spec, &hn, view_port) {
                    // L501 `return` inside Draw
                    return None;
                }
            }
        } else if tree_click == Some("RIGHT") {
            if let Some(hn) = hover_node.clone() {
                Self::right_click(cx, &build, &hn, view_port);
            }
        }

        f.hover_node = hover_node;
        f.hover_compare_node = hover_compare_node;
        f.hover_path = hover_path;
        f.hover_dep = hover_dep;
        Some(f)
    }

    /// L392-520 (the left click on a node); `false` for Lua's `return` from Draw.
    fn left_click(
        &mut self,
        cx: &mut Cx,
        build: &BuildRef,
        spec: &PassiveSpecRef,
        hover_node: &NodeRef,
        view_port: &ViewPort,
    ) -> bool {
        // User left-clicked on a node
        // L394
        if hover_node.alloc.get() {
            // Node is allocated, so deallocate it
            // L396-398
            spec.DeallocNode(hover_node);
            add_undo_state(cx);
            build.buildFlag.set(true);
            return true;
        }
        // Check if the node belongs to a different ascendancy
        // L401
        if let Some(hn_asc) = hover_node.ascendancyName.get() {
            // L402-405
            let mut is_different_ascendancy = false;
            let mut target_ascend_class_id: Option<f64> = None;
            let mut target_base_class_id: Option<f64> = None;
            let mut target_base_class: Option<Table> = None;

            let cur_secondary = spec.extra.table(n!("curSecondaryAscendClass"));
            let alt = spec.tree.get().alternate_ascendancies.get();
            // Check if this is a bloodline (secondary ascendancy) node
            // L408
            if let Some(alt) = alt.filter(|_| hover_node.extra.truthy(n!("isBloodline"))) {
                // L409
                let is_different_bloodline = !cur_secondary
                    .as_ref()
                    .is_some_and(|c| c.str(n!("id")).as_deref() == Some(&*hn_asc));
                // L411
                if is_different_bloodline {
                    // Find the bloodline in alternate_ascendancies
                    // L413 (ORDER: module notes)
                    for (bloodline_id, bloodline_data) in alt.pairs() {
                        // L414
                        if bloodline_data.get(n!("id")).str().as_deref() == Some(&*hn_asc) {
                            // L415-418
                            spec.SelectSecondaryAscendClass(bloodline_id.to_val().num());
                            add_undo_state(cx);
                            set_window_title(cx);
                            build.buildFlag.set(true);
                            // L419
                            break;
                        }
                    }
                }
            } else {
                // Regular ascendancy node (not bloodline)
                // Check if it's different from current primary or secondary ascendancy
                // L426
                if spec.curAscendClassId.get() == 0.0
                    || Some(&hn_asc) != spec.curAscendClassBaseName.get().as_ref()
                {
                    // L427
                    if !cur_secondary
                        .as_ref()
                        .is_some_and(|c| c.str(n!("id")).as_deref() == Some(&*hn_asc))
                    {
                        // L428
                        is_different_ascendancy = true;
                    }
                }

                // L432
                if is_different_ascendancy {
                    // First, check if it's in the current class (same-class switching)
                    // L434 (ORDER: module notes)
                    let cur_class = spec.extra.table(n!("curClass")).unwrap_or_else(|| {
                        lua_error!("attempt to index field 'curClass' (a nil value)")
                    });
                    let classes = cur_class.table(n!("classes")).unwrap_or_else(|| {
                        lua_error!("bad argument #1 to 'for iterator' (table expected, got nil)")
                    });
                    for (ascend_class_id, ascend_class) in classes.pairs() {
                        // L435
                        if ascend_class.get(n!("id")).str().as_deref() == Some(&*hn_asc) {
                            // L436-437
                            target_ascend_class_id = Some(ascend_class_id.to_val().num());
                            break;
                        }
                    }

                    // L441
                    if let Some(target) = target_ascend_class_id {
                        // Same-class switching - always allowed
                        // L443-446
                        spec.SelectAscendClass(target);
                        add_undo_state(cx);
                        set_window_title(cx);
                        build.buildFlag.set(true);
                    } else {
                        // Cross-class switching - search all classes
                        // L449 (ORDER: module notes)
                        'outer: for (class_id, class_data) in spec.tree.get().classes.pairs() {
                            // L450
                            let Some(cd) = class_data.table() else { continue };
                            let Some(cl) = cd.table(n!("classes")) else {
                                lua_error!(
                                    "bad argument #1 to 'for iterator' (table expected, got nil)"
                                )
                            };
                            for (ascend_class_id, ascend_class) in cl.pairs() {
                                // L451
                                if ascend_class.get(n!("id")).str().as_deref() == Some(&*hn_asc) {
                                    // L452-455
                                    target_base_class_id = Some(class_id.to_val().num());
                                    target_base_class = Some(cd.clone());
                                    target_ascend_class_id = Some(ascend_class_id.to_val().num());
                                    break;
                                }
                            }
                            // L458
                            if target_base_class_id.is_some() {
                                break 'outer;
                            }
                        }

                        // L461
                        if let Some(target_base_class_id) = target_base_class_id {
                            let target_ascend_class_id = target_ascend_class_id.unwrap();
                            // L462
                            let (used, ..) = spec.CountAllocNodes();
                            // L463
                            let clicked_ascend_node_id = hover_node.id;
                            // L464-472 local function allocateClickedAscendancy()
                            let allocate_clicked_ascendancy = {
                                let spec = spec.clone();
                                move || {
                                    // L468
                                    let target_node = spec
                                        .nodes
                                        .with(|m| m.get(clicked_ascend_node_id as i64));
                                    // L469
                                    if let Some(t) = target_node.filter(|t| !t.alloc.get()) {
                                        // L470
                                        spec.AllocNode(&t, None);
                                    }
                                }
                            };

                            // Allow cross-class switching if: no regular points allocated OR tree is connected to target class
                            // L475
                            if used == 0.0 || spec.IsClassConnected(target_base_class_id) {
                                // L476-481
                                spec.SelectClass(target_base_class_id);
                                spec.SelectAscendClass(target_ascend_class_id);
                                allocate_clicked_ascendancy();
                                add_undo_state(cx);
                                set_window_title(cx);
                                build.buildFlag.set(true);
                            } else {
                                // Tree has points but isn't connected to target class
                                let name = target_base_class
                                    .as_ref()
                                    .and_then(|c| c.str(n!("name")))
                                    .map(|s| s.to_string())
                                    .unwrap_or_else(|| {
                                        lua_error!("attempt to concatenate field 'name' (a nil value)")
                                    });
                                let (s1, b1) = (spec.clone(), build.clone());
                                let a1 = allocate_clicked_ascendancy.clone();
                                let (s2, b2) = (spec.clone(), build.clone());
                                let a2 = allocate_clicked_ascendancy;
                                // L484-500
                                pob_ui_core::popups::open_confirm_popup(
                                    cx,
                                    "Class Change",
                                    &format!("Changing class to {name} will reset your passive tree.\nThis can be avoided by connecting one of the {name} starting nodes to your tree."),
                                    "Continue",
                                    move |cx| {
                                        // L485-490
                                        s1.SelectClass(target_base_class_id);
                                        s1.SelectAscendClass(target_ascend_class_id);
                                        a1();
                                        add_undo_state(cx);
                                        set_window_title(cx);
                                        b1.buildFlag.set(true);
                                    },
                                    Some((
                                        "Connect Path",
                                        Rc::new(move |cx: &mut Cx| {
                                            // L492
                                            if s2.ConnectToClass_ret(target_base_class_id) {
                                                // L493-498
                                                s2.SelectClass(target_base_class_id);
                                                s2.SelectAscendClass(target_ascend_class_id);
                                                a2();
                                                add_undo_state(cx);
                                                set_window_title(cx);
                                                b2.buildFlag.set(true);
                                            }
                                        }),
                                    )),
                                );
                                // L501
                                return false;
                            }
                        }
                    }
                }
            }
        }

        // Normal node allocation (non-ascendancy or same ascendancy)
        // L510
        if hover_node.path.get().is_some() && !hover_node.alloc.get() {
            // L511
            if &*hover_node.r#type.get() == "Mastery" && hover_node.masteryEffects.get().is_some() {
                // L512
                let f = hooks().open_mastery_popup;
                f(cx, hover_node, view_port);
            } else {
                // L514 self.tracePath and hoverNode == self.tracePath[#self.tracePath] and self.tracePath
                let alt_path = self.trace_path.clone().filter(|tp| {
                    tp.get(tp.len())
                        .is_some_and(|last| Rc::ptr_eq(&last, hover_node))
                });
                spec.AllocNode(hover_node, alt_path.as_deref());
                // L515-516
                add_undo_state(cx);
                build.buildFlag.set(true);
            }
        }
        true
    }

    /// L521-543 (the right click on a node).
    fn right_click(cx: &mut Cx, build: &BuildRef, hover_node: &NodeRef, view_port: &ViewPort) {
        let t = ty(hover_node);
        let sd = hover_node.sd.get().snapshot();
        let dn = hover_node.dn.get();
        // L522
        if hover_node.alloc.get() && t == "Socket" {
            // L523-529 (U32/U34's slot control, CCR-0115)
            let f = hooks().socket_right_click;
            f(cx, hover_node.id);
        } else if (hover_node.isTattoo.get()
            || (t == "Normal"
                && (&*dn == "Strength" || &*dn == "Dexterity" || &*dn == "Intelligence"))
            || (t == "Notable"
                && !sd.is_empty()
                && (pob_lua::lmatch(&sd[0], "+30 to Dexterity", None).is_some()
                    || pob_lua::lmatch(&sd[0], "+30 to Strength", None).is_some()
                    || pob_lua::lmatch(&sd[0], "+30 to Intelligence", None).is_some()))
            || t == "Keystone")
            && !hover_node.extra.truthy(n!("expansionSkill"))
        {
            // L535-536
            let f = hooks().modify_node_popup;
            f(cx, hover_node, view_port);
            build.buildFlag.set(true);
        } else if hover_node.alloc.get()
            && t == "Mastery"
            && hover_node.masteryEffects.get().is_some()
        {
            // L538-539
            let f = hooks().open_mastery_popup;
            f(cx, hover_node, view_port);
            build.buildFlag.set(true);
        } else if !hover_node.alloc.get()
            && t == "Mastery"
            && hover_node.masteryEffects.get().is_some()
        {
            // L541-542
            let f = hooks().modify_node_popup;
            f(cx, hover_node, view_port);
            build.buildFlag.set(true);
        }
    }

    /// Zoom the tree in or out
    /// lua: PassiveTreeView.lua:1307-1320 `PassiveTreeView:Zoom(level, viewPort)`
    /// @pob Classes/PassiveTreeView.lua:1307-1320 h=b8c1ac73
    pub fn zoom(&mut self, cx: &Cx, level: f64, view_port: &ViewPort) {
        // Calculate new zoom level and zoom factor
        // L1309
        self.zoom_level = m_max(0.0, m_min(12.0, self.zoom_level + level));
        // L1310
        let old_zoom = self.zoom;
        // L1311
        self.zoom = lpow(1.2, self.zoom_level);

        // Adjust zoom center position so that the point on the tree that is currently under the mouse will remain under it
        // L1314
        let factor = self.zoom / old_zoom;
        // L1315
        let (cursor_x, cursor_y) = cx.sys.cursor_pos();
        // L1316-1317
        let rel_x = cursor_x - view_port.x - view_port.width / 2.0;
        let rel_y = cursor_y - view_port.y - view_port.height / 2.0;
        // L1318-1319
        self.zoom_x = Some(rel_x + (pos_opt(self.zoom_x, "zoomX") - rel_x) * factor);
        self.zoom_y = Some(rel_y + (pos_opt(self.zoom_y, "zoomY") - rel_y) * factor);
    }

    /// lua: PassiveTreeView.lua:1323-1332 `PassiveTreeView:Focus(x, y, viewPort, build)`
    /// @pob Classes/PassiveTreeView.lua:1323-1332 h=2105e116
    pub fn focus(&mut self, cx: &Cx, x: f64, y: f64, view_port: &ViewPort) {
        // L1324-1325
        self.zoom_level = 12.0;
        self.zoom = lpow(1.2, self.zoom_level);

        // L1327
        let tree = cx.build().model.spec.get().tree.get();
        // L1328
        let scale = m_min(view_port.width, view_port.height) / tree.extra.num(n!("size")) * self.zoom;

        // L1330-1331
        self.zoom_x = Some(-x * scale);
        self.zoom_y = Some(-y * scale);
    }
}

/// `self.zoomX` in arithmetic (nil raises).
#[track_caller]
fn pos_opt(v: Option<f64>, what: &str) -> f64 {
    v.unwrap_or_else(|| lua_error!("attempt to perform arithmetic on field '{what}' (a nil value)"))
}
