//! Port of PathOfBuilding v2.67.2 (16de4b82d) src/Classes/PassiveTreeView.lua:1-1411 (Draw, DrawImageRotated, DrawAsset).
//! Owner: U23. `PassiveTreeView:Draw` (L197-1259): the input half is `input.rs`
//! ([`PassiveTreeView::draw_input`]); this file draws (L545-1258), plus `DrawImageRotated`
//! (L1260-1283) and `DrawAsset` (L1286-1304).
//!
//! @pob-file Classes/PassiveTreeView.lua @16de4b82d
//!
//! ORDER (ui doc §8 rule 6): each loop below draws on its own sub-layer and sets the draw colour
//! before every draw, so a draw's state does not depend on the previous iteration; the U23 test
//! compares the sub-layers these loops fill as multisets (b):
//! * `pairs(tree.groups)` (L650), `pairs(spec.subGraphs)` (L655, L749), the compare spec's
//!   subgraphs (L660, L756): sub-layer 0 (group backgrounds; `tree.groups` is the model's raw
//!   `Table`, whose order is not LuaJIT's);
//! * `pairs(tree.connectors)` (L746) and `pairs(subGraph.connectors)` (L750, L758): sub-layer 20
//!   (the connectors are built in `pairs(tree.nodes)` order, CCR-0024 §5);
//! * `pairs(spec.nodes)` (L810, L816), `pairs(self.compareSpec.nodes)` (L1115) and
//!   `pairs(tree.sockets)` (L1216): sub-layers 15, 25 and 30 (ui doc §8 rule 6 names
//!   PassiveTreeView.lua:828-965 inside `pairs(spec.nodes)`). The hovered node's tooltip
//!   (sub-layers 99/100) is drawn for one node only, so its order is fixed.
//! * `pairs(jewel.jewelData.impossibleEscapeKeystones)` (L1179): keyed rings on sub-layer 25.
//! * L810 fills `searchStrResults` by node id (c).
//!
//! DIVERGENCE: none.

use std::rc::Rc;

use pob_lua::math::{lpow, m_floor, m_max, m_min};
use pob_lua::{n, Name};
use pob_sg::{Align, Font, ImageHandle};
use pob_ui_core::prelude::*;
use pob_ui_core::tooltip::{TtKey, TtObj};

use super::assets::{tree_assets, Asset, AssetSeq, SpriteSet};
use super::compare::{compare_jewels_equal, is_abyss_conquered, DrawColor};
use super::connectors::Connector;
use super::input::{pos, same, Frame};
use super::{hooks, PassiveTreeView};
use crate::app::CxApp;
use crate::model::{ItemRef, Node, NodeRef, Table};

// @pob Classes/PassiveTreeView.lua:17-19 h=0ec78df6
const JEWEL_RADIUS_TINT_NEUTRAL: [f64; 4] = [1.0, 1.0, 1.0, 0.7];
const JEWEL_RADIUS_TINT_PRIMARY_ONLY: [f64; 4] = [1.0, 0.0, 0.0, 0.7];
const JEWEL_RADIUS_TINT_COMPARE_ONLY: [f64; 4] = [0.0, 1.0, 0.0, 0.7];

/// `#node.intuitiveLeapLikesAffecting` (nil raises, as in Lua; PS-B fills it).
pub fn intuitive_leap_likes_affecting_len(node: &Node) -> usize {
    match node.extra.get(n!("intuitiveLeapLikesAffecting")) {
        crate::model::Val::Table(t) => t.len(),
        other => lua_error!(
            "attempt to get length of field 'intuitiveLeapLikesAffecting' (a {} value)",
            other.lua_type()
        ),
    }
}

/// `t.k` of an optional table.
fn opt_str(t: &Table, k: pob_lua::Name) -> Option<String> {
    t.str(k).map(|s| s.to_string())
}

/// `node.overlay[key]` (`node.overlay` is the `nodeOverlay[type]` table PT stored).
fn overlay_key(node: &Node, key: Name) -> Option<String> {
    node.extra
        .get(n!("overlay"))
        .table()
        .and_then(|o| o.str(key).map(|s| s.to_string()))
}

// ---- lookup keys (U53 perf pass) ---------------------------------------------------------------
// The draw builds Lua's `a .. b` keys for every node and connector of every frame. The helpers
// below give the same strings without a `format!` allocation (a reused buffer) and, for the
// `node.overlay[...]` keys, without interning a fresh string (the names are pre-interned).

thread_local! {
    static KEY_BUF: std::cell::Cell<String> = const { std::cell::Cell::new(String::new()) };
}

/// `f(a .. b .. ...)`, the key built in a reused buffer.
fn with_key<R>(parts: &[&str], f: impl FnOnce(&str) -> R) -> R {
    let mut k = KEY_BUF.with(std::cell::Cell::take);
    k.clear();
    for p in parts {
        k.push_str(p);
    }
    let r = f(&k);
    KEY_BUF.with(|b| b.set(k));
    r
}

/// `state .. (alt and "Alt" or "")` (L852, L1124).
fn socket_state_key(state: &str, alt: bool) -> Name {
    match (state, alt) {
        ("alloc", false) => n!("alloc"),
        ("alloc", true) => n!("allocAlt"),
        ("path", false) => n!("path"),
        ("path", true) => n!("pathAlt"),
        ("unalloc", false) => n!("unalloc"),
        ("unalloc", true) => n!("unallocAlt"),
        (s, a) => with_key(&[s, if a { "Alt" } else { "" }], Name::from),
    }
}

/// `state .. (node.ascendancyName and "Ascend" or "") .. (node.isBlighted and "Blighted" or "")`
/// (L888, L1141).
fn node_state_key(state: &str, ascend: bool, blighted: bool) -> Name {
    match (state, ascend, blighted) {
        ("alloc", false, false) => n!("alloc"),
        ("alloc", true, false) => n!("allocAscend"),
        ("alloc", false, true) => n!("allocBlighted"),
        ("alloc", true, true) => n!("allocAscendBlighted"),
        ("path", false, false) => n!("path"),
        ("path", true, false) => n!("pathAscend"),
        ("path", false, true) => n!("pathBlighted"),
        ("path", true, true) => n!("pathAscendBlighted"),
        ("unalloc", false, false) => n!("unalloc"),
        ("unalloc", true, false) => n!("unallocAscend"),
        ("unalloc", false, true) => n!("unallocBlighted"),
        ("unalloc", true, true) => n!("unallocAscendBlighted"),
        (s, a, b) => with_key(
            &[s, if a { "Ascend" } else { "" }, if b { "Blighted" } else { "" }],
            Name::from,
        ),
    }
}

/// `sprites[node.type:lower() .. suffix]` (L887 `"Active"`/`"Inactive"`, L1138 `"Active"`).
fn type_sprite(set: &SpriteSet, ty: &str, suffix: &str) -> Option<Rc<Asset>> {
    let key = match (ty, suffix) {
        ("Normal", "Active") => "normalActive",
        ("Normal", "Inactive") => "normalInactive",
        ("Notable", "Active") => "notableActive",
        ("Notable", "Inactive") => "notableInactive",
        ("Keystone", "Active") => "keystoneActive",
        ("Keystone", "Inactive") => "keystoneInactive",
        _ => {
            let lower = ty.to_ascii_lowercase();
            return with_key(&[&lower, suffix], |k| set.get(k).cloned());
        }
    };
    set.get(key).cloned()
}

/// `sprites[key]`
fn sprite(set: Option<&Rc<super::assets::SpriteSet>>, key: &str) -> Option<Rc<Asset>> {
    set.and_then(|s| s.get(key).cloned())
}

/// `x and x.alloc`
fn alloc_of(n: Option<&NodeRef>) -> bool {
    n.is_some_and(|n| n.alloc.get())
}

impl PassiveTreeView {
    /// lua: PassiveTreeView.lua:197-1259 `PassiveTreeView:Draw(build, viewPort, inputEvents)`
    /// (`build` is `cx.build()`; the input half is [`PassiveTreeView::draw_input`]).
    pub fn draw(&mut self, cx: &mut Cx, view_port: &ViewPort, input_events: &mut InputEvents) {
        let Some(f) = self.draw_input(cx, view_port, input_events) else {
            return;
        };
        self.draw_background_and_groups(cx, view_port, &f);
        self.draw_connectors(cx, &f);
        self.draw_nodes(cx, view_port, &f);
        self.draw_compare_only_nodes(cx, view_port, &f);
        self.draw_jewel_sockets(cx, &f);
    }

    /// L545-667: the background artwork, the class background art and the group backgrounds.
    /// @pob Classes/PassiveTreeView.lua:545-667 h=1589838a
    fn draw_background_and_groups(&mut self, cx: &mut Cx, view_port: &ViewPort, f: &Frame) {
        let tree = &f.tree;
        let assets = &f.assets;
        let spec = &f.spec;
        let scale = f.scale;
        // Draw the background artwork
        // L547
        let bg = assets
            .asset("Background2")
            .or_else(|| assets.asset("Background1"))
            .cloned()
            .unwrap_or_else(|| lua_error!("attempt to index local 'bg' (a nil value)"));
        // L548
        if bg.width.get() == 0.0 {
            // L549
            let h = bg.handle();
            let (w, hh) = h
                .as_ref()
                .map_or((0, 0), |h| h.image_size(&mut cx.sg));
            bg.width.set(f64::from(w));
            bg.height.set(f64::from(hh));
        }
        // L551
        if bg.width.get() > 0.0 {
            // L552
            let bg_size = bg.width.get() * scale * 1.33 * 2.5;
            // L553
            cx.sg.set_draw_color(1.0, 1.0, 1.0);
            let (zx, zy) = (self.zoom_x.unwrap(), self.zoom_y.unwrap());
            // L554
            cx.sg.draw_image_tc(
                bg.handle().as_ref(),
                view_port.x,
                view_port.y,
                view_port.width,
                view_port.height,
                (zx + view_port.width / 2.0) / -bg_size,
                (zy + view_port.height / 2.0) / -bg_size,
                (view_port.width / 2.0 - zx) / bg_size,
                (view_port.height / 2.0 - zy) / bg_size,
            );
        }

        // Hack to draw class background art, the position data doesn't seem to be in the tree JSON yet
        // L558-576
        let class_bg = match spec.curClassId.get() {
            1.0 => Some((-2750.0, 1600.0, "BackgroundStr")),
            2.0 => Some((2550.0, 1600.0, "BackgroundDex")),
            3.0 => Some((-250.0, -2200.0, "BackgroundInt")),
            4.0 => Some((-150.0, 2350.0, "BackgroundStrDex")),
            5.0 => Some((-2100.0, -1500.0, "BackgroundStrInt")),
            6.0 => Some((2350.0, -1950.0, "BackgroundDexInt")),
            _ => None,
        };
        if let Some((x, y, name)) = class_bg {
            let (scr_x, scr_y) = f.tree_to_screen(x, y);
            self.draw_asset(cx, assets.asset(name).map(Rc::as_ref), scr_x, scr_y, scale, false);
        }

        // Draw the group backgrounds
        // L650 (ORDER: module notes)
        let groups = tree.extra.get(n!("groups")).table();
        if let Some(groups) = groups {
            for (_, group) in groups.pairs() {
                let Some(group) = group.table() else { continue };
                // L651
                if !group.truthy(n!("isProxy")) {
                    // L652
                    self.render_group(cx, f, &group, false);
                }
            }
        }
        // L655 (ORDER: module notes)
        for (_, sub_graph) in spec.subGraphs.pairs() {
            let Some(group) = sub_graph.get(n!("group")).table() else {
                lua_error!("attempt to index local 'group' (a nil value)")
            };
            // L656
            self.render_group(cx, f, &group, true);
        }
        // Draw group backgrounds for compare-only subgraphs (cluster jewels only in compare build)
        // L659
        if let Some(cs) = self.compare_spec.clone() {
            // L660 (ORDER: module notes)
            for (sub_graph_id, sub_graph) in cs.subGraphs.pairs() {
                // L661
                if !spec.subGraphs.truthy(sub_graph_id) {
                    // L662
                    cx.sg.set_draw_color_a(0.0, 1.0, 0.0, 0.6);
                    let Some(group) = sub_graph.get(n!("group")).table() else {
                        lua_error!("attempt to index local 'group' (a nil value)")
                    };
                    // L663
                    self.render_group(cx, f, &group, true);
                    // L664
                    cx.sg.set_draw_color(1.0, 1.0, 1.0);
                }
            }
        }
    }

    /// lua: PassiveTreeView.lua:578-647 `local function renderGroup(group, isExpansion)`
    fn render_group(&mut self, cx: &mut Cx, f: &Frame, group: &Table, is_expansion: bool) {
        let tree = &f.tree;
        let assets = &f.assets;
        let spec = &f.spec;
        let scale = f.scale;
        // L579
        let (scr_x, scr_y) = f.tree_to_screen(group.num(n!("x")), group.num(n!("y")));
        let oo = group.get(n!("oo"));
        // L580
        if let Some(asc) = opt_str(group, n!("ascendancyName")) {
            // L581
            if group.truthy(n!("isAscendancyStart")) {
                // L582
                let cur_secondary = spec.extra.table(n!("curSecondaryAscendClass"));
                let is_selected_ascendancy = spec
                    .curAscendClassBaseName
                    .get()
                    .is_some_and(|b| *b == *asc)
                    || cur_secondary
                        .as_ref()
                        .is_some_and(|c| c.str(n!("id")).as_deref() == Some(asc.as_str()));
                // L583
                if !is_selected_ascendancy {
                    // L584
                    cx.sg.set_draw_color_a(1.0, 1.0, 1.0, 0.50);
                }
                // L586
                self.draw_asset(
                    cx,
                    assets.asset(&format!("Classes{asc}")).map(Rc::as_ref),
                    scr_x,
                    scr_y,
                    scale,
                    false,
                );

                // L588
                let class1 = tree.classes.get(1.0).table();
                if class1.as_ref().is_some_and(|c| c.truthy(n!("ascendancies"))) {
                    // L589-590
                    let mut ascendancy_data: Option<Table> = None;
                    let mut is_alternate_ascendancy = false;
                    // Search normal ascendancies
                    // L592
                    'outer: for (_, class) in tree.classes.ipairs() {
                        let Some(asc_list) = class.get(n!("ascendancies")).table() else {
                            lua_error!(
                                "bad argument #1 to 'ipairs' (table expected, got nil)"
                            )
                        };
                        // L593
                        for (_, ascendancy) in asc_list.ipairs() {
                            // L594
                            if ascendancy.get(n!("id")).str().as_deref() == Some(asc.as_str()) {
                                // L595-596
                                ascendancy_data = ascendancy.table();
                                break;
                            }
                        }
                        // L599
                        if ascendancy_data.is_some() {
                            break 'outer;
                        }
                    }

                    // Search alternate ascendancies if not found
                    // L603
                    if ascendancy_data.is_none() {
                        if let Some(alt) = tree.alternate_ascendancies.get() {
                            // L604 (ORDER: unique ids, input.rs notes)
                            for (_, ascendancy) in alt.pairs() {
                                // L605
                                if ascendancy.get(n!("id")).str().as_deref() == Some(asc.as_str()) {
                                    // L606-608
                                    ascendancy_data = ascendancy.table();
                                    is_alternate_ascendancy = true;
                                    break;
                                }
                            }
                        }
                    }
                    // L612
                    if let Some(ad) = ascendancy_data.filter(|a| a.truthy(n!("flavourTextRect"))) {
                        // L613
                        let rect = ad.table(n!("flavourTextRect")).unwrap();

                        // Normal ascendancy images are 1300x1300, bloodline appears to be 1488x1412
                        // L616-617
                        let offset_x = rect.num(n!("x"))
                            - if is_alternate_ascendancy { 744.0 } else { 650.0 };
                        let offset_y = rect.num(n!("y"))
                            - if is_alternate_ascendancy { 706.0 } else { 650.0 };

                        // L619
                        let (text_x, text_y) = f.tree_to_screen(
                            group.num(n!("x")) + offset_x,
                            group.num(n!("y")) + offset_y,
                        );

                        // L621-622
                        let flavour_text_base_font_size = 52.0;
                        let flavour_text_min_zoom = 2.5;
                        // L623
                        if self.zoom >= flavour_text_min_zoom {
                            let colour = ad.str(n!("flavourTextColour")).unwrap_or_else(|| {
                                lua_error!(
                                    "attempt to concatenate field 'flavourTextColour' (a nil value)"
                                )
                            });
                            // L624
                            let mut text_color = format!("^x{colour}");
                            // L625
                            if !is_selected_ascendancy {
                                // L627-630
                                let half = |a: f64, b: f64| -> i64 {
                                    let s = pob_ui_core::luastr::sub(&colour, a, Some(b));
                                    let v = pob_lua::tonumber_base(&s, 16).unwrap_or_else(|| {
                                        lua_error!("attempt to perform arithmetic on a nil value")
                                    });
                                    m_floor(v * 0.5) as i64
                                };
                                text_color = format!(
                                    "^x{:02X}{:02X}{:02X}",
                                    half(1.0, 2.0),
                                    half(3.0, 4.0),
                                    half(5.0, 6.0)
                                );
                            }
                            let text = ad.str(n!("flavourText")).unwrap_or_else(|| {
                                lua_error!(
                                    "attempt to concatenate field 'flavourText' (a nil value)"
                                )
                            });
                            // L632
                            cx.sg.draw_string(
                                text_x,
                                text_y,
                                Align::Left,
                                flavour_text_base_font_size * scale,
                                Font::FontinItalic,
                                &format!("{text_color}{text}"),
                            );
                        }
                    }
                } else {
                    // L636 ConPrintTable(tree.classes)
                    cx.sys.platform.con_print("ConPrintTable(tree.classes)");
                }
                // L638
                cx.sg.set_draw_color(1.0, 1.0, 1.0);
            }
        } else if oo.get(3.0).truthy() {
            // L641
            let name = if is_expansion {
                "GroupBackgroundLargeHalfAlt"
            } else {
                "PSGroupBackground3"
            };
            self.draw_asset(cx, assets.asset(name).map(Rc::as_ref), scr_x, scr_y, scale, true);
        } else if oo.get(2.0).truthy() {
            // L643
            let name = if is_expansion {
                "GroupBackgroundMediumAlt"
            } else {
                "PSGroupBackground2"
            };
            self.draw_asset(cx, assets.asset(name).map(Rc::as_ref), scr_x, scr_y, scale, false);
        } else if oo.get(1.0).truthy() {
            // L645
            let name = if is_expansion {
                "GroupBackgroundSmallAlt"
            } else {
                "PSGroupBackground1"
            };
            self.draw_asset(cx, assets.asset(name).map(Rc::as_ref), scr_x, scr_y, scale, false);
        }
    }

    /// L669-781: the connectors (`setConnectorColor`, `getState`, `renderConnector`, the tree's,
    /// the subgraphs' and the compare-only subgraphs').
    /// @pob Classes/PassiveTreeView.lua:669-781 h=6fce22a5
    fn draw_connectors(&mut self, cx: &mut Cx, f: &Frame) {
        let spec = &f.spec;
        let assets = &f.assets;
        // Draw the connecting lines between nodes
        // L745
        cx.sg.set_draw_layer(None, Some(20));
        // L746 (ORDER: module notes)
        for connector in &assets.connectors {
            // L747
            self.render_connector(cx, f, connector);
        }
        // L749 (ORDER: module notes)
        for (_, sub_graph) in spec.subGraphs.pairs() {
            let Some(sg) = sub_graph.table() else { continue };
            // L750
            for connector in assets.subgraph_connectors(&sg).iter() {
                // L751
                self.render_connector(cx, f, connector);
            }
        }
        // Draw connectors for compare-only subgraphs (cluster jewels only in compare build)
        // L755
        if let Some(cs) = self.compare_spec.clone() {
            let c_assets = tree_assets(cx, &cs.tree.get());
            // L756 (ORDER: module notes)
            for (sub_graph_id, sub_graph) in cs.subGraphs.pairs() {
                // L757
                if spec.subGraphs.truthy(sub_graph_id) {
                    continue;
                }
                let Some(sg) = sub_graph.table() else { continue };
                // L758
                for connector in c_assets.subgraph_connectors(&sg).iter() {
                    // L759-760
                    let c_node1 = cs.nodes.with(|m| m.get(connector.node_id1 as i64));
                    let c_node2 = cs.nodes.with(|m| m.get(connector.node_id2 as i64));
                    // L761
                    if alloc_of(c_node1.as_ref()) && alloc_of(c_node2.as_ref()) {
                        // L762
                        let state = "Active";
                        // L763
                        let vert = *connector.vert(state);
                        // L765-769
                        {
                            let mut c = connector.c.borrow_mut();
                            for i in 0..4 {
                                let (sx, sy) = f.tree_to_screen(vert[2 * i], vert[2 * i + 1]);
                                c[2 * i] = sx;
                                c[2 * i + 1] = sy;
                            }
                        }
                        // L770
                        cx.sg.set_draw_color(0.0, 1.0, 0.0);
                        // L771
                        let asset = with_key(&[&connector.r#type, state], |k| {
                            f.assets.asset(k).cloned()
                        })
                        .or_else(|| {
                            with_key(&[&connector.r#type, "Normal"], |k| f.assets.asset(k).cloned())
                        });
                        // L772
                        if let Some(asset) = asset {
                            let c = *connector.c.borrow();
                            let (p, tc): ([f64; 8], [f64; 8]) = (
                                c[..8].try_into().unwrap(),
                                c[8..].try_into().unwrap(),
                            );
                            // L773
                            cx.sg.draw_image_quad_tc(asset.handle().as_ref(), p, tc);
                        }
                    }
                }
            }
            // L780
            cx.sg.set_draw_color(1.0, 1.0, 1.0);
        }
    }

    /// lua: PassiveTreeView.lua:673-684 `local function getState(n1, n2)`
    fn get_state(f: &Frame, n1: &NodeRef, n2: &NodeRef) -> &'static str {
        // Determine the connector state
        // L675
        let mut state = "Normal";
        // L676
        if n1.alloc.get() && n2.alloc.get() {
            // L677
            state = "Active";
        } else if let Some(hp) = &f.hover_path {
            // L679
            let on = |n: &NodeRef| {
                n.alloc.get() || f.hover_node.as_ref().is_some_and(|h| Rc::ptr_eq(h, n)) || hp.has(n)
            };
            if on(n1) && on(n2) {
                // L680
                state = "Intermediate";
            }
        }
        // L683
        state
    }

    /// lua: PassiveTreeView.lua:685-742 `local function renderConnector(connector)`
    fn render_connector(&mut self, cx: &mut Cx, f: &Frame, connector: &Connector) {
        let spec = &f.spec;
        // L686
        let node1 = spec.nodes.with(|m| m.get(connector.node_id1 as i64));
        let node2 = spec.nodes.with(|m| m.get(connector.node_id2 as i64));
        let (Some(node1), Some(node2)) = (node1, node2) else {
            lua_error!("attempt to index local 'node1' (a nil value)")
        };
        // L687
        let mut connector_default_color = DrawColor::Esc("^xFFFFFF".to_owned());

        // L689
        if f.split_personality(node1.id) && f.split_personality(node2.id) {
            // L690
            connector_default_color = DrawColor::Esc(cx.main.cc.SPLITPERSONALITY.clone());
        }

        // L669 local connectorColor = { 1, 1, 1 }; L670-672 setConnectorColor
        // L693
        let mut connector_color = connector_default_color;
        // L694
        let mut state = Self::get_state(f, &node1, &node2);
        // L695
        let mut base_state = state;
        // L696
        if let Some(cs) = &self.compare_spec {
            // L697
            let c_node1 = cs.nodes.with(|m| m.get(connector.node_id1 as i64));
            let c_node2 = cs.nodes.with(|m| m.get(connector.node_id2 as i64));
            // L698
            if let (Some(c1), Some(c2)) = (c_node1, c_node2) {
                // L699
                base_state = Self::get_state(f, &c1, &c2);
            }
        }

        // L703
        if base_state == "Active" && state != "Active" {
            // L704-705
            state = "Active";
            connector_color = DrawColor::Rgb(0.0, 1.0, 0.0);
        }
        // L707
        if base_state != "Active" && state == "Active" {
            // L708
            connector_color = DrawColor::Rgb(1.0, 0.0, 0.0);
        }

        // Convert vertex coordinates to screen-space and add them to the coordinate array
        // L712
        let vert = *connector.vert(state);
        // L713-716
        {
            let mut c = connector.c.borrow_mut();
            for i in 0..4 {
                let (sx, sy) = f.tree_to_screen(vert[2 * i], vert[2 * i + 1]);
                c[2 * i] = sx;
                c[2 * i + 1] = sy;
            }
        }

        // L718
        if f.hover_dep.as_ref().is_some_and(|d| d.has(&node1) && d.has(&node2)) {
            // Both nodes depend on the node currently being hovered over, so color the line red
            // L720
            connector_color = DrawColor::Rgb(1.0, 0.0, 0.0);
        } else if connector
            .ascendancy_name
            .as_ref()
            .is_some_and(|a| Some(a) != spec.curAscendClassBaseName.get().as_ref())
        {
            // Fade out lines in ascendancy classes other than the current one
            // L723
            connector_color = DrawColor::Rgb(0.75, 0.75, 0.75);
        }
        // L725
        connector_color.set(cx);
        // L726 (asset_name = connector.type .. state)
        // The game uses Abyss connector art only when both connected nodes are conquered.
        // L728
        let abyss = is_abyss_conquered(Some(&node1)) && is_abyss_conquered(Some(&node2));
        // L729 (asset_name = "Abyss" .. asset_name), L731
        let asset = with_key(
            &[if abyss { "Abyss" } else { "" }, &connector.r#type, state],
            |k| f.assets.asset(k).cloned(),
        )
        .or_else(|| with_key(&[&connector.r#type, state], |k| f.assets.asset(k).cloned()))
        .unwrap_or_else(|| lua_error!("attempt to index local 'asset' (a nil value)"));
        // Atlas assets provide bounds that map the usual connector coordinates into one sprite.
        // L733-734
        let left = asset.index_num(1).unwrap_or(0.0);
        let top = asset.index_num(2).unwrap_or(0.0);
        let width = asset.index_num(3).unwrap_or(1.0) - left;
        let height = asset.index_num(4).unwrap_or(1.0) - top;
        // L735
        let c = *connector.c.borrow();
        // L736-741
        cx.sg.draw_image_quad_tc(
            asset.handle().as_ref(),
            [c[0], c[1], c[2], c[3], c[4], c[5], c[6], c[7]],
            [
                left + c[8] * width,
                top + c[9] * height,
                left + c[10] * width,
                top + c[11] * height,
                left + c[12] * width,
                top + c[13] * height,
                left + c[14] * width,
                top + c[15] * height,
            ],
        );
    }

    /// L783-1110: the heat map power, the search results and the nodes (with the hovered node's
    /// tooltip).
    /// @pob Classes/PassiveTreeView.lua:783-1110 h=f5185883
    fn draw_nodes(&mut self, cx: &mut Cx, view_port: &ViewPort, f: &Frame) {
        let spec = &f.spec;
        let assets = &f.assets;
        let build = &f.build;
        let scale = f.scale;
        // L783
        if self.show_heat_map {
            // Build the power numbers if needed
            // L785
            let bp = hooks().build_power;
            bp(cx);
            // L786
            self.heat_map_stat = pob_calc::modules::power::powerStat(build); // CCR-0115
        }

        // Update cached node data
        // L790
        if self.search_str_cached != self.search_str {
            // L791
            self.search_str_cached = self.search_str.clone();
            // L808
            self.search_params = Some(prep_search(&self.search_str));

            // L810 (ORDER: module notes)
            for (node_id, node) in spec.nodes.with(pob_lua::LjIntMap::pairs) {
                // L811
                let m = !self.search_params.as_ref().unwrap().is_empty()
                    && self.does_node_match_search_params(cx, &node) == Some(true);
                self.search_str_results.insert(node_id, m);
            }
        }

        let main_env = build.calcsTab.mainEnv.get();
        let granted = main_env.as_ref().map(|e| e.grantedPassives.get());
        let devmode_alt = cx.app().launch.dev_mode_alt;
        let node_power_theme = cx.app().main.node_power_theme.clone();
        let edge_search_highlight = cx.app().main.edge_search_highlight;

        // Draw the nodes
        // L816 (ORDER: module notes)
        for (node_id, node) in spec.nodes.with(pob_lua::LjIntMap::pairs) {
            // Determine the base and overlay images for this node based on type and state
            // L818
            let compare_node = self
                .compare_spec
                .as_ref()
                .and_then(|cs| cs.nodes.with(|m| m.get(node_id)));

            // L820
            let mut base: Option<Rc<Asset>> = None;
            let mut overlay: Option<String> = None;
            let mut effect: Option<Rc<Asset>> = None;
            // L821
            let granted_here = match &granted {
                Some(g) => g.with(|m| m.get(node_id)).unwrap_or(false),
                None => lua_error!("attempt to index field 'mainEnv' (a nil value)"),
            };
            let is_alloc = node.alloc.get() || granted_here || alloc_of(compare_node.as_ref());
            // L822
            let mut node_default_color = DrawColor::Esc("^xFFFFFF".to_owned());

            // L824
            if f.split_personality(node.id) {
                // L825
                node_default_color = DrawColor::Esc(cx.main.cc.SPLITPERSONALITY.clone());
            }

            let ty_name = node.r#type.get();
            let ty: &str = ty_name.as_str();
            // L828
            cx.sg.set_draw_layer(None, Some(25));
            // L829
            if ty == "ClassStart" {
                // L830
                overlay = Some(if is_alloc {
                    node.extra.str(n!("startArt")).map_or_else(
                        || "PSStartNodeBackgroundInactive".to_owned(),
                        |s| s.to_string(),
                    )
                } else {
                    "PSStartNodeBackgroundInactive".to_owned()
                });
                // `isAlloc and node.startArt or "..."`: a nil startArt falls through to the
                // inactive art, as above
            } else if ty == "AscendClassStart" {
                // L832
                let mut o = if assets.version_num >= 3.10 {
                    "AscendancyMiddle".to_owned()
                } else {
                    "PassiveSkillScreenAscendancyMiddle".to_owned()
                };
                // L833
                if let Some(asc) = node.ascendancyName.get() {
                    // L834
                    let prefix = assets.bloodline_sprite_prefixes.get(&*asc);
                    // L835
                    if let Some(prefix) = prefix {
                        // L836
                        o = format!("{prefix}{o}");
                    }
                }
                overlay = Some(o);
            } else {
                // L840-849
                let hover_is_node = f.hover_node.as_ref().is_some_and(|h| Rc::ptr_eq(h, &node));
                let trace_last = self.trace_mode
                    && self
                        .trace_path
                        .as_ref()
                        .and_then(|tp| tp.get(tp.len()))
                        .is_some_and(|last| Rc::ptr_eq(&last, &node));
                let state = if self.show_heat_map || is_alloc || hover_is_node || trace_last {
                    // Show node as allocated if it is being hovered over
                    // Also if the heat map is turned on (makes the nodes more visible)
                    // L844
                    "alloc"
                } else if f.hover_path.as_ref().is_some_and(|hp| hp.has(&node)) {
                    // L846
                    "path"
                } else {
                    // L848
                    "unalloc"
                };
                // L850
                if ty == "Socket" {
                    // Node is a jewel socket, retrieve the socketed jewel (if present) so we can display the correct art
                    // L852
                    let key = socket_state_key(state, node.expansionJewel.get().is_some());
                    let o = overlay_key(&node, key).unwrap_or_else(|| {
                        lua_error!("attempt to concatenate field '?' (a nil value)")
                    });
                    let prefix = if node.extra.str(n!("name")).as_deref() == Some("Charm Socket") {
                        "Azmeri"
                    } else {
                        ""
                    };
                    base = with_key(&[prefix, &o], |k| assets.asset(k).cloned());
                    // L853
                    let (_socket, jewel) = build.itemsTab.GetSocketAndJewelForNodeID(node.id);
                    // L854
                    if is_alloc {
                        if let Some(jewel) = jewel {
                            // L855
                            overlay = Self::get_jewel_socket_overlay(
                                &jewel,
                                node.expansionJewel.get().is_some(),
                            )
                            .map(str::to_owned);
                        }
                    }
                } else if ty == "Mastery" {
                    // L858-859
                    let ovr = spec.hashOverrides.with(|m| m.get(node.id as i64));
                    let has_override = ovr.is_some();
                    // L861-862
                    let sprites = ovr
                        .as_ref()
                        .and_then(|o| assets.sprites(o.extra.str(n!("icon")).as_deref()));
                    let effect_sprites = ovr.as_ref().and_then(|o| {
                        assets.sprites(o.extra.str(n!("activeEffectImage")).as_deref())
                    });
                    let _ = has_override;
                    let ms = assets.node_mastery_sprites(&node);

                    // L864
                    if node.masteryEffects.get().is_some() {
                        // L865
                        if is_alloc {
                            // L866-867
                            base = sprite(sprites.as_ref(), "masteryActiveSelected").or_else(|| {
                                sprite(
                                    ms.as_ref().and_then(|m| m.active_icon.as_ref()),
                                    "masteryActiveSelected",
                                )
                            });
                            // L868-869
                            effect = sprite(effect_sprites.as_ref(), "masteryActiveEffect")
                                .or_else(|| {
                                    sprite(
                                        ms.as_ref().and_then(|m| m.active_effect_image.as_ref()),
                                        "masteryActiveEffect",
                                    )
                                });
                        } else if hover_is_node {
                            // L871-872
                            base = sprite(sprites.as_ref(), "masteryConnected").or_else(|| {
                                sprite(
                                    ms.as_ref().and_then(|m| m.inactive_icon.as_ref()),
                                    "masteryConnected",
                                )
                            });
                        } else {
                            // L874-875
                            base = sprite(sprites.as_ref(), "masteryInactive").or_else(|| {
                                sprite(
                                    ms.as_ref().and_then(|m| m.inactive_icon.as_ref()),
                                    "masteryInactive",
                                )
                            });
                        }
                    } else {
                        // L878-879
                        base = sprite(sprites.as_ref(), "mastery")
                            .or_else(|| sprite(assets.node_sprites(&node).as_ref(), "mastery"));
                    }
                    // L881
                    cx.sg.set_draw_layer(None, Some(15));
                } else {
                    // Normal node (includes keystones and notables)
                    // L884
                    if node.isTattoo.get() {
                        if let Some(es) = assets.node_effect_sprites(&node) {
                            // L885
                            effect = es.get("tattooActiveEffect").cloned();
                        }
                    }
                    // L887
                    let Some(node_sprites) = assets.node_sprites(&node) else {
                        lua_error!("attempt to index field 'sprites' (a nil value)")
                    };
                    base = type_sprite(
                        &node_sprites,
                        ty,
                        if is_alloc { "Active" } else { "Inactive" },
                    );
                    // L888
                    let overlay_key_s = node_state_key(
                        state,
                        node.ascendancyName.get().is_some(),
                        node.extra.truthy(n!("isBlighted")),
                    );
                    // L889
                    let mut overlay_name = overlay_key(&node, overlay_key_s);
                    // L890
                    if is_abyss_conquered(Some(&node)) && (ty == "Notable" || ty == "Keystone") {
                        // AlternateTreeArt gives Abyss replacements their own notable,
                        // keystone, and ascendancy-notable frames.
                        // L893
                        let frame_type = if node.ascendancyName.get().is_some() {
                            "Ascendancy".to_owned()
                        } else {
                            ty.to_owned()
                        };
                        // L894
                        let frame_state = match state {
                            "alloc" => "Allocated",
                            "path" => "CanAllocate",
                            _ => "Unallocated",
                        };
                        // L895
                        let abyss_overlay = format!("Abyss{frame_type}Frame{frame_state}");
                        // L896
                        if assets.asset(&abyss_overlay).is_some() {
                            // L897
                            overlay_name = Some(abyss_overlay);
                        }
                    }
                    // L900
                    if let Some(asc) = node.ascendancyName.get() {
                        // L901
                        let prefix = node
                            .extra
                            .str(n!("bloodlineOverlayPrefix"))
                            .map(|s| s.to_string())
                            .or_else(|| assets.bloodline_sprite_prefixes.get(&*asc).cloned());
                        // L902
                        overlay = match (prefix, overlay_name) {
                            // L903
                            (Some(p), Some(o)) => Some(format!("{p}{o}")),
                            // L905
                            (_, o) => o,
                        };
                    } else {
                        // L908
                        overlay = overlay_name;
                    }
                }
            }

            // Convert node position to screen-space
            // L914
            let (scr_x, scr_y) = f.tree_to_screen(pos(node.x.get(), "x"), pos(node.y.get(), "y"));

            // Determine color for the base artwork
            // L917
            if self.show_heat_map {
                // L918
                if !is_alloc && ty != "ClassStart" && ty != "AscendClassStart" {
                    let power = node.extra.get(n!("power"));
                    let power_max = pob_calc::modules::power::powerMax(build); // CCR-0115
                    let pm = |k: pob_lua::Name| -> f64 {
                        power_max
                            .as_ref()
                            .unwrap_or_else(|| {
                                lua_error!("attempt to index field 'powerMax' (a nil value)")
                            })
                            .num(k)
                    };
                    // L919
                    if self
                        .heat_map_stat
                        .as_ref()
                        .is_some_and(|s| s.truthy(n!("stat")))
                    {
                        // Calculate color based on a single stat
                        // L921
                        let stat = m_max(power.get(n!("singleStat")).num_or(0.0), 0.0);
                        // L922
                        let stat_col = lpow(stat / pm(n!("singleStat")) * 1.5, 0.5);
                        // L923-929
                        match node_power_theme.as_str() {
                            "RED/BLUE" => cx.sg.set_draw_color(stat_col, 0.0, 0.0),
                            "RED/GREEN" => cx.sg.set_draw_color(0.0, stat_col, 0.0),
                            "GREEN/BLUE" => cx.sg.set_draw_color(0.0, 0.0, stat_col),
                            _ => {}
                        }
                    } else {
                        // Calculate color based on DPS and defensive powers
                        // L932-933
                        let offence = m_max(power.get(n!("offence")).num_or(0.0), 0.0);
                        let defence = m_max(power.get(n!("defence")).num_or(0.0), 0.0);
                        // L934-935
                        let dps_col = lpow(offence / pm(n!("offence")) * 1.5, 0.5);
                        let def_col = lpow(defence / pm(n!("defence")) * 1.5, 0.5);
                        // L936
                        let mix_col =
                            (m_max(dps_col - 0.5, 0.0) + m_max(def_col - 0.5, 0.0)) / 2.0;
                        // L937-943
                        match node_power_theme.as_str() {
                            "RED/BLUE" => cx.sg.set_draw_color(dps_col, mix_col, def_col),
                            "RED/GREEN" => cx.sg.set_draw_color(dps_col, def_col, mix_col),
                            "GREEN/BLUE" => cx.sg.set_draw_color(mix_col, dps_col, def_col),
                            _ => {}
                        }
                    }
                } else {
                    // L946
                    self.get_compare_node_color(cx, &node, compare_node.as_deref(), spec, &node_default_color)
                        .set(cx);
                }
            } else if devmode_alt {
                // Debug display
                // L950 (`node.extra` is the model's `node.extra.extra`, CCR-0024 §4)
                if node.extra.truthy(n!("extra")) {
                    // L951
                    cx.sg.set_draw_color(1.0, 0.0, 0.0);
                } else if node.extra.truthy(n!("unknown")) {
                    // L953
                    cx.sg.set_draw_color(0.0, 1.0, 1.0);
                } else {
                    // L955
                    cx.sg.set_draw_color(0.0, 0.0, 0.0);
                }
            } else {
                // L958
                self.get_compare_node_color(cx, &node, compare_node.as_deref(), spec, &node_default_color)
                    .set(cx);
            }

            // Draw mastery/tattoo effect artwork
            // L962
            if let Some(effect) = &effect {
                // L963-965
                cx.sg.set_draw_layer(None, Some(15));
                self.draw_asset(cx, Some(effect), scr_x, scr_y, scale, false);
                cx.sg.set_draw_layer(None, Some(25));
            }

            // Draw base artwork
            // L969
            if let Some(base) = &base {
                // L970
                if ty == "Socket" && f.hover_dep.as_ref().is_some_and(|d| d.has(&node)) {
                    // L971-973
                    cx.sg.set_draw_color(1.0, 0.0, 0.0);
                    self.draw_asset(cx, Some(base), scr_x, scr_y, scale, false);
                    cx.sg.set_draw_color(1.0, 1.0, 1.0);
                } else {
                    // L975
                    self.draw_asset(cx, Some(base), scr_x, scr_y, scale, false);
                }
            }

            // L979
            if let Some(overlay) = &overlay {
                // Draw overlay
                // L981
                if ty != "ClassStart" && ty != "AscendClassStart" {
                    // L982
                    if let Some(hover) = f.hover_node.as_ref().filter(|h| !Rc::ptr_eq(h, &node)) {
                        // Mouse is hovering over a different node
                        // L984
                        if f.hover_dep.as_ref().is_some_and(|d| d.has(&node)) {
                            // This node depends on the hover node, turn it red
                            // L986
                            cx.sg.set_draw_color(1.0, 0.0, 0.0);
                        } else if &*hover.r#type.get() == "Socket" && hover.nodesInRadius.get().is_some() {
                            // Hover node is a socket, check if this node falls within its radius and color it accordingly
                            // L989
                            let (_socket, jewel) = build.itemsTab.GetSocketAndJewelForNodeID(hover.id);
                            // L990
                            let is_thread_of_hope = jewel
                                .as_ref()
                                .is_some_and(|j| j.jewelRadiusLabel.get().as_deref() == Some("Variable"));
                            let radii = pob_gamedata::jewelRadius();
                            // L991
                            for (i0, data) in radii.jewelRadius.iter().enumerate() {
                                // L994 / L1005
                                let in_radius = pob_calc::classes::passive_tree::nodesInRadiusAt( // CCR-0115
                                    hover,
                                    (i0 + 1) as f64,
                                )
                                .unwrap_or_else(|| {
                                    lua_error!("attempt to index field '?' (a nil value)")
                                })
                                .contains_key(node.id as i64);
                                if in_radius {
                                    if is_thread_of_hope {
                                        // Draw Thread of Hope's annuli
                                        // L996
                                        if data.inner != 0.0 {
                                            // L997-998
                                            cx.sg.set_draw_color_esc(&data.col);
                                            break;
                                        }
                                    } else if data.inner == 0.0 {
                                        // Draw normal jewel radii
                                        // L1008-1009
                                        cx.sg.set_draw_color_esc(&data.col);
                                        break;
                                    }
                                }
                            }
                        }
                    }
                }
                // L1017
                self.draw_asset(cx, assets.asset(overlay).map(Rc::as_ref), scr_x, scr_y, scale, false);
                // L1018
                cx.sg.set_draw_color(1.0, 1.0, 1.0);
            }
            // L1020
            if self.search_str_results.get(&node_id).copied().unwrap_or(false) {
                // Node matches the search string, show the highlight circle
                // L1022
                cx.sg.set_draw_layer(None, Some(30));
                // L1023-1024 (the global `rgbColor` is nil)
                cx.sg.set_draw_color(1.0, 0.0, 0.0);
                // L1025
                let mut size = 175.0 * scale / lpow(self.zoom, 0.4);

                // L1027
                if edge_search_highlight {
                    // Snap node matches to the edge of the viewPort
                    // L1029-1030
                    let peekaboo_ratio = 1.15;
                    let scaled_down_ratio = 0.6667;
                    // L1031-1032
                    let wide_cull = [
                        view_port.x - size / peekaboo_ratio,
                        view_port.x + view_port.width - size * peekaboo_ratio,
                    ];
                    let high_cull = [
                        view_port.y - size / peekaboo_ratio,
                        view_port.y + view_port.height - size * peekaboo_ratio,
                    ];
                    // L1033-1034
                    let mut new_x = m_min(m_max(scr_x - size, wide_cull[0]), wide_cull[1]);
                    let mut new_y = m_min(m_max(scr_y - size, high_cull[0]), high_cull[1]);

                    // L1036
                    if new_x != scr_x - size || new_y != scr_y - size {
                        // L1037-1039
                        size *= scaled_down_ratio;
                        new_x += size / 2.0;
                        new_y += size / 2.0;
                    }
                    // L1041
                    cx.sg
                        .draw_image(Some(&self.highlight_ring), new_x, new_y, size * 2.0, size * 2.0);
                } else {
                    // L1043
                    cx.sg.draw_image(
                        Some(&self.highlight_ring),
                        scr_x - size,
                        scr_y - size,
                        size * 2.0,
                        size * 2.0,
                    );
                }
            }
            // L1047
            let hover_is_node = f.hover_node.as_ref().is_some_and(|h| Rc::ptr_eq(h, &node));
            if hover_is_node
                && (ty != "Socket" || !cx.sys.is_key_down(Key::Shift))
                && (ty != "Mastery" || node.masteryEffects.get().is_some())
                && !cx.sys.is_key_down(Key::Ctrl)
                && cx.main.popups.is_empty()
            {
                self.draw_node_tooltip(cx, view_port, f, &node, scr_x, scr_y);
            }
        }
    }

    /// L1047-1109: the hovered node's tooltip.
    fn draw_node_tooltip(
        &mut self,
        cx: &mut Cx,
        view_port: &ViewPort,
        f: &Frame,
        node: &NodeRef,
        scr_x: f64,
        scr_y: f64,
    ) {
        let scale = f.scale;
        // Draw tooltip
        // L1049
        cx.sg.set_draw_layer(None, Some(100));
        // L1050
        let size = m_floor(node.extra.num(n!("size")) * scale);
        // L1051
        let params = [
            TtKey::Obj(TtObj(node.clone() as Rc<dyn std::any::Any>)),
            TtKey::Bool(self.show_stat_differences),
            self.trace_path
                .as_ref()
                .map_or(TtKey::Nil, |tp| TtKey::Obj(TtObj(tp.clone() as Rc<dyn std::any::Any>))),
            TtKey::Bool(cx.app().launch.dev_mode_alt),
            TtKey::Num(f.build.outputRevision.get()),
            // `build.spec.allocMode`: nothing in PoB sets it
            TtKey::Nil,
        ];
        if self.tooltip.check_for_update(cx, &params) {
            // L1052
            let mut tt = std::mem::take(&mut self.tooltip);
            let h = hooks().add_node_tooltip;
            h(cx, self, &mut tt, node);
            self.tooltip = tt;
        }
        // L1054
        self.tooltip.center = true;
        // L1055-1056
        let (tt_width, tt_height) = self.tooltip.get_dynamic_size(cx, view_port);
        let (skill_width, skill_height) = self.skill_tooltip.get_dynamic_size(cx, view_port);

        // L1058
        let fat_skill = skill_width > skill_height * 1.5;

        // L1060-1067
        let skill_lines = !self.skill_tooltip.lines.is_empty();
        let (total_width, total_height) = if fat_skill {
            (
                m_max(tt_width, if skill_lines { skill_width } else { 0.0 }),
                tt_height + if skill_lines { skill_height } else { 0.0 },
            )
        } else {
            (
                tt_width + if skill_lines { skill_width } else { 0.0 },
                m_max(tt_height, if skill_lines { skill_height } else { 0.0 }),
            )
        };

        // main tooltip is anchored from top left to the node
        // L1070-1073
        let node_x = m_floor(scr_x + size);
        let mut tt_x = m_floor(scr_x + size);
        let node_y = m_floor(scr_y - size);
        let mut tt_y = m_floor(scr_y - size);

        // if the right side goes outside the viewport, we adjust by moving to the left
        // L1077-1078
        let r_edge_x = tt_x + total_width - view_port.x;
        let r_over_by = r_edge_x - view_port.width;
        // L1079
        if r_over_by > 0.0 {
            // L1080
            tt_x -= r_over_by;
        }

        // same for bottom edge
        // L1084-1085
        let btm_edge_y = tt_y + total_height - view_port.y;
        let btm_over_by = btm_edge_y - view_port.height;
        // L1086
        if btm_over_by > 0.0 {
            // L1087
            tt_y -= btm_over_by;
        }

        // L1090
        cx.sg.set_draw_layer(None, Some(100));
        // main tooltip is attached to node, unless it is pushed
        // L1092
        if fat_skill {
            // L1093
            self.tooltip.draw(
                cx,
                m_min(node_x, view_port.width - tt_width + view_port.x),
                m_max(tt_y, view_port.y),
                None,
                None,
                view_port,
            );
        } else {
            // L1096
            self.tooltip.draw(
                cx,
                m_max(tt_x, view_port.x),
                m_min(node_y, view_port.height - tt_height + view_port.y),
                None,
                None,
                view_port,
            );
        }
        // L1099
        cx.sg.set_draw_layer(None, Some(99));
        // draw below main tooltip
        // L1101
        if fat_skill {
            // L1102
            self.skill_tooltip
                .draw(cx, tt_x, tt_y + tt_height, None, None, view_port);
            // draw to the right of main tooltip
        } else {
            // L1106
            self.skill_tooltip
                .draw(cx, tt_x + tt_width + 5.0, tt_y, None, None, view_port);
        }
    }

    /// L1112-1165: compare-only nodes (nodes in compareSpec but not in primary spec, e.g.
    /// cluster jewel subgraphs).
    /// @pob Classes/PassiveTreeView.lua:1112-1165 h=aa73e205
    fn draw_compare_only_nodes(&mut self, cx: &mut Cx, view_port: &ViewPort, f: &Frame) {
        let Some(cs) = self.compare_spec.clone() else {
            return;
        };
        let spec = &f.spec;
        let scale = f.scale;
        let c_assets = tree_assets(cx, &cs.tree.get());
        // L1114
        cx.sg.set_draw_layer(None, Some(25));
        // L1115 (ORDER: module notes)
        for (node_id, compare_node) in cs.nodes.with(pob_lua::LjIntMap::pairs) {
            let cty = compare_node.r#type.get().to_string();
            // L1116-1117
            if spec.nodes.with(|m| m.contains_key(node_id))
                || !compare_node.alloc.get()
                || compare_node.x.get().is_nan()
                || compare_node.y.get().is_nan()
                || cty == "ClassStart"
                || cty == "AscendClassStart"
            {
                continue;
            }
            // L1118
            let (scr_x, scr_y) = f.tree_to_screen(compare_node.x.get(), compare_node.y.get());
            // Draw base artwork with green coloring (compare-only = "added" nodes)
            // L1120
            cx.sg.set_draw_color(0.0, 1.0, 0.0);
            // L1121
            let state = "alloc";
            // L1122
            let mut base: Option<Rc<Asset>> = None;
            let mut overlay: Option<String> = None;
            // L1123
            if cty == "Socket" {
                // L1124
                let key = socket_state_key(state, compare_node.expansionJewel.get().is_some());
                let name = if compare_node.extra.get(n!("overlay")).truthy() {
                    overlay_key(&compare_node, key)
                } else {
                    None
                }
                .unwrap_or_else(|| "JewelSocketActiveBlue".to_owned());
                base = f.assets.asset(&name).cloned();
                // Look up jewel from compare build to show correct colored socket overlay
                // L1126
                let c_jewel = self.get_compare_jewel(node_id as f64);
                // L1127
                if let Some(c_jewel) = c_jewel {
                    // L1128
                    overlay = Self::get_jewel_socket_overlay(
                        &c_jewel,
                        compare_node.expansionJewel.get().is_some(),
                    )
                    .map(str::to_owned);
                }
            } else if cty == "Mastery" {
                let ms = c_assets.node_mastery_sprites(&compare_node);
                // L1131
                if let Some(ai) = ms.as_ref().and_then(|m| m.active_icon.as_ref()) {
                    // L1132
                    base = ai.get("masteryActiveSelected").cloned();
                } else if let Some(s) = c_assets.node_sprites(&compare_node) {
                    // L1134
                    base = s.get("mastery").cloned();
                }
            } else {
                // L1137
                if let Some(s) = c_assets.node_sprites(&compare_node) {
                    // L1138
                    base = type_sprite(&s, &cty, "Active");
                }
                // L1140
                if compare_node.extra.get(n!("overlay")).truthy() {
                    // L1141
                    let key = node_state_key(
                        state,
                        compare_node.ascendancyName.get().is_some(),
                        compare_node.extra.truthy(n!("isBlighted")),
                    );
                    // L1142
                    overlay = overlay_key(&compare_node, key);
                }
            }
            // L1145
            if let Some(base) = &base {
                // L1146
                self.draw_asset(cx, Some(base), scr_x, scr_y, scale, false);
            }
            // L1148
            if let Some(overlay) = &overlay {
                // L1149
                self.draw_asset(cx, f.assets.asset(overlay).map(Rc::as_ref), scr_x, scr_y, scale, false);
            }
            // L1151
            cx.sg.set_draw_color(1.0, 1.0, 1.0);
            // Draw tooltip for hovered compare-only node
            // L1153
            if same(Some(&compare_node), f.hover_compare_node.as_ref())
                && (cty != "Mastery" || compare_node.masteryEffects.get().is_some())
                && !cx.sys.is_key_down(Key::Ctrl)
                && cx.main.popups.is_empty()
            {
                // L1154
                cx.sg.set_draw_layer(None, Some(100));
                // L1155
                let size = m_floor(compare_node.extra.num(n!("size")) * scale);
                // L1156
                let params = [
                    TtKey::Obj(TtObj(compare_node.clone() as Rc<dyn std::any::Any>)),
                    TtKey::Bool(false),
                    TtKey::Nil,
                    TtKey::Bool(cx.app().launch.dev_mode_alt),
                    TtKey::Num(f.build.outputRevision.get()),
                ];
                if self.tooltip.check_for_update(cx, &params) {
                    // L1157
                    let mut tt = std::mem::take(&mut self.tooltip);
                    let h = hooks().add_compare_node_tooltip;
                    h(cx, self, &mut tt, &compare_node);
                    self.tooltip = tt;
                }
                // L1159
                self.tooltip.center = true;
                // L1160
                self.tooltip.draw(
                    cx,
                    m_floor(scr_x - size),
                    m_floor(scr_y - size),
                    Some(size * 2.0),
                    Some(size * 2.0),
                    view_port,
                );
                // L1161
                cx.sg.set_draw_layer(None, Some(25));
            }
        }
    }

    /// lua: PassiveTreeView.lua:1168-1214 `local function drawJewelRadius(jewel, scrX, scrY, tint)`
    fn draw_jewel_radius(&mut self, cx: &mut Cx, f: &Frame, jewel: &ItemRef, scr_x: f64, scr_y: f64, tint: [f64; 4]) {
        let scale = f.scale;
        let jewel_data = jewel.jewelData.get();
        // Abyss jewels do not show radius art in game.
        // L1170
        if is_abyss_conquered_tbl(jewel_data.as_ref()) {
            // L1171
            return;
        }
        // L1173
        let radii = pob_gamedata::jewelRadius();
        let idx = jewel
            .jewelRadiusIndex
            .get()
            .unwrap_or_else(|| lua_error!("attempt to index local 'radData' (a nil value)"));
        let rad_data = radii
            .jewelRadius
            .get((idx as usize).wrapping_sub(1))
            .cloned()
            .unwrap_or_else(|| lua_error!("attempt to index local 'radData' (a nil value)"));
        // L1174-1175
        let outer_size = rad_data.outer * scale;
        let mut inner_size = rad_data.inner * scale * 1.06;
        // L1176
        cx.sg.set_draw_color_a(tint[0], tint[1], tint[2], tint[3]);
        let title = jewel
            .title
            .get()
            .map(|s| s.to_string())
            .unwrap_or_else(|| lua_error!("attempt to index field 'title' (a nil value)"));
        let m = |p: &str| pob_lua::lmatch(&title, p, None).is_some();
        let rot = |cx: &mut Cx, h: &ImageHandle, x: f64, y: f64, sz: f64, a: f64| {
            draw_image_rotated(cx, h, x, y, sz * 2.0, sz * 2.0, a);
        };
        // L1177
        if m("Impossible Escape") {
            // Impossible Escape ring shows on the allocated Keystone
            // L1179 (ORDER: module notes)
            let keystones = jewel_data
                .as_ref()
                .and_then(|d| d.table(n!("impossibleEscapeKeystones")))
                .unwrap_or_else(|| {
                    lua_error!("bad argument #1 to 'for iterator' (table expected, got nil)")
                });
            for (keystone_name, _) in keystones.pairs() {
                // L1180
                let keystone = f
                    .spec
                    .tree
                    .get()
                    .keystoneMap
                    .get(keystone_name)
                    .obj::<Node>();
                // L1181
                if let Some(k) = keystone.filter(|k| !k.x.get().is_nan() && !k.y.get().is_nan()) {
                    // L1182
                    inner_size = 150.0 * scale;
                    // L1183
                    let (key_x, key_y) = f.tree_to_screen(k.x.get(), k.y.get());
                    // L1184-1187
                    let (o1, o2, i1, i2) = (
                        self.jewel_shaded_outer_ring.clone(),
                        self.jewel_shaded_outer_ring_flipped.clone(),
                        self.jewel_shaded_inner_ring.clone(),
                        self.jewel_shaded_inner_ring_flipped.clone(),
                    );
                    rot(cx, &o1, key_x, key_y, outer_size, -0.8);
                    rot(cx, &o2, key_x, key_y, outer_size, 1.0);
                    rot(cx, &i1, key_x, key_y, inner_size, -1.2);
                    rot(cx, &i2, key_x, key_y, inner_size, 1.0);
                }
            }
        } else {
            let pair = if m("^Brutal Restraint") {
                // L1191-1192
                Some((self.maraketh1.clone(), self.maraketh2.clone()))
            } else if m("^Elegant Hubris") {
                // L1194-1195
                Some((self.eternal1.clone(), self.eternal2.clone()))
            } else if m("^Glorious Vanity") {
                // L1197-1198
                Some((self.vaal1.clone(), self.vaal2.clone()))
            } else if m("^Lethal Pride") {
                // L1200-1201
                Some((self.karui1.clone(), self.karui2.clone()))
            } else if m("^Militant Faith") {
                // L1203-1204
                Some((self.templar1.clone(), self.templar2.clone()))
            } else if m("^Heroic Tragedy") {
                // L1206-1207
                Some((self.kalguur1.clone(), self.kalguur2.clone()))
            } else {
                None
            };
            if let Some((a, b)) = pair {
                rot(cx, &a, scr_x, scr_y, outer_size, -0.7);
                rot(cx, &b, scr_x, scr_y, outer_size, 0.7);
            } else {
                // L1209-1212
                let (o1, o2, i1, i2) = (
                    self.jewel_shaded_outer_ring.clone(),
                    self.jewel_shaded_outer_ring_flipped.clone(),
                    self.jewel_shaded_inner_ring.clone(),
                    self.jewel_shaded_inner_ring_flipped.clone(),
                );
                rot(cx, &o1, scr_x, scr_y, outer_size, -0.7);
                rot(cx, &o2, scr_x, scr_y, outer_size, 0.7);
                rot(cx, &i1, scr_x, scr_y, inner_size, -0.7);
                rot(cx, &i2, scr_x, scr_y, inner_size, 0.7);
            }
        }
    }

    /// L1167-1258: ring overlays for jewel sockets.
    /// @pob Classes/PassiveTreeView.lua:1167-1258 h=1e947498
    fn draw_jewel_sockets(&mut self, cx: &mut Cx, f: &Frame) {
        let spec = &f.spec;
        let build = &f.build;
        let scale = f.scale;
        // L1215
        cx.sg.set_draw_layer(None, Some(25));
        // L1216 (ORDER: module notes)
        for node_id in f.tree.sockets.keys() {
            // L1217
            let Some(node) = spec.nodes.with(|m| m.get(node_id)) else { continue };
            // L1218
            if node.extra.str(n!("name")).as_deref() == Some("Charm Socket") {
                continue;
            }
            if let Some(ej) = node.expansionJewel.get() {
                if ej.get(n!("size")).opt_num() != Some(2.0) {
                    continue;
                }
            }
            // L1219
            let (scr_x, scr_y) = f.tree_to_screen(pos(node.x.get(), "x"), pos(node.y.get(), "y"));
            // L1220
            let (_socket, jewel) = build.itemsTab.GetSocketAndJewelForNodeID(node_id as f64);
            // L1221
            let compare_node = self
                .compare_spec
                .as_ref()
                .and_then(|cs| cs.nodes.with(|m| m.get(node_id)));
            // L1222
            let c_jewel = if self.compare_spec.is_some() {
                self.get_compare_jewel(node_id as f64)
            } else {
                None
            };
            // L1223
            if f.hover_node.as_ref().is_some_and(|h| Rc::ptr_eq(h, &node)) {
                // L1224
                let effective_jewel = jewel.clone().or_else(|| c_jewel.clone());
                // L1225
                let is_thread_of_hope = effective_jewel
                    .as_ref()
                    .is_some_and(|j| j.jewelRadiusLabel.get().as_deref() == Some("Variable"));
                let radii = pob_gamedata::jewelRadius();
                // L1226
                for rad_data in &radii.jewelRadius {
                    // L1227-1228
                    let outer_size = rad_data.outer * scale;
                    let inner_size = rad_data.inner * scale;
                    // L1229
                    if is_thread_of_hope {
                        // Thread of Hope-like: draw the annulus (only radii with a non-zero inner)
                        // L1231
                        if inner_size != 0.0 {
                            // L1232-1234
                            cx.sg.set_draw_color_esc(&rad_data.col);
                            cx.sg.draw_image(
                                Some(&self.ring),
                                scr_x - outer_size,
                                scr_y - outer_size,
                                outer_size * 2.0,
                                outer_size * 2.0,
                            );
                            cx.sg.draw_image(
                                Some(&self.ring),
                                scr_x - inner_size,
                                scr_y - inner_size,
                                inner_size * 2.0,
                                inner_size * 2.0,
                            );
                        }
                    } else {
                        // Standard jewel: draw the full-disc radii (inner == 0)
                        // L1238
                        if inner_size == 0.0 {
                            // L1239-1240
                            cx.sg.set_draw_color_esc(&rad_data.col);
                            cx.sg.draw_image(
                                Some(&self.ring),
                                scr_x - outer_size,
                                scr_y - outer_size,
                                outer_size * 2.0,
                                outer_size * 2.0,
                            );
                        }
                    }
                }
            }
            // L1245
            if node.alloc.get() || alloc_of(compare_node.as_ref()) {
                // L1246-1248
                let p_has_radius = jewel.as_ref().is_some_and(|j| j.jewelRadiusIndex.get().is_some());
                let c_has_radius = c_jewel.as_ref().is_some_and(|j| j.jewelRadiusIndex.get().is_some());
                let same_jewel = compare_jewels_equal(jewel.as_ref(), c_jewel.as_ref());
                // L1249
                if p_has_radius {
                    // L1250
                    let tint = if self.compare_spec.is_none() || same_jewel {
                        JEWEL_RADIUS_TINT_NEUTRAL
                    } else {
                        JEWEL_RADIUS_TINT_PRIMARY_ONLY
                    };
                    // L1251
                    self.draw_jewel_radius(cx, f, jewel.as_ref().unwrap(), scr_x, scr_y, tint);
                }
                // L1253
                if c_has_radius && !same_jewel {
                    // L1254
                    self.draw_jewel_radius(
                        cx,
                        f,
                        c_jewel.as_ref().unwrap(),
                        scr_x,
                        scr_y,
                        JEWEL_RADIUS_TINT_COMPARE_ONLY,
                    );
                }
            }
        }
    }

    /// Draws the given asset at the given position
    /// lua: PassiveTreeView.lua:1286-1304 `PassiveTreeView:DrawAsset(data, x, y, scale, isHalf)`
    /// @pob Classes/PassiveTreeView.lua:1286-1304 h=62b7167c
    pub fn draw_asset(
        &self,
        cx: &mut Cx,
        data: Option<&Asset>,
        x: f64,
        y: f64,
        scale: f64,
        is_half: bool,
    ) {
        // L1287
        let Some(data) = data else {
            // L1288
            return;
        };
        // L1290
        if data.width.get() == 0.0 {
            // L1291
            let (w, h) = data
                .handle()
                .as_ref()
                .map_or((0, 0), |h| h.image_size(&mut cx.sg));
            data.width.set(f64::from(w));
            data.height.set(f64::from(h));
            // L1292
            if data.width.get() == 0.0 {
                // L1293
                return;
            }
        }
        // L1296-1297
        let width = pos(data.width.get(), "width") * scale * 1.33;
        let height = pos(data.height.get(), "height") * scale * 1.33;
        let handle = data.handle();
        // L1298
        if is_half {
            // L1299-1300
            cx.sg.draw_image(
                handle.as_ref(),
                x - width,
                y - height * 2.0,
                width * 2.0,
                height * 2.0,
            );
            cx.sg.draw_image_tc(
                handle.as_ref(),
                x - width,
                y,
                width * 2.0,
                height * 2.0,
                0.0,
                1.0,
                1.0,
                0.0,
            );
        } else {
            // L1302 DrawImage(data.handle, x - width, y - height, width * 2, height * 2, unpack(data))
            match &data.seq {
                AssetSeq::Empty => {
                    cx.sg
                        .draw_image(handle.as_ref(), x - width, y - height, width * 2.0, height * 2.0);
                }
                AssetSeq::Tc(tc) => {
                    cx.sg.draw_image_tc(
                        handle.as_ref(),
                        x - width,
                        y - height,
                        width * 2.0,
                        height * 2.0,
                        tc[0],
                        tc[1],
                        tc[2],
                        tc[3],
                    );
                }
                AssetSeq::Other(_) => {
                    lua_error!("DrawImage() argument 6: expected number, got string")
                }
            }
        }
    }
}

/// `isAbyssConquered(jewel.jewelData)` (L1170): the same test on a jewel's data table.
fn is_abyss_conquered_tbl(t: Option<&Table>) -> bool {
    let Some(cb) = t.and_then(|t| t.table(n!("conqueredBy"))) else {
        return false;
    };
    let conqueror = cb.get(n!("conqueror"));
    conqueror.truthy()
        && conqueror
            .get(n!("type"))
            .str()
            .is_some_and(|ty| ty.starts_with("abyss_"))
}

/// lua: PassiveTreeView.lua:1260-1283 `PassiveTreeView:DrawImageRotated(handle, x, y, width, height, angle, ...)`
/// (every caller passes no extra arguments)
/// @pob Classes/PassiveTreeView.lua:1260-1283 h=d61eb003
pub fn draw_image_rotated(
    cx: &mut Cx,
    handle: &ImageHandle,
    x: f64,
    y: f64,
    width: f64,
    height: f64,
    angle: f64,
) {
    // L1261 `main.showAnimations == false`
    if !cx.app().main.show_animations {
        // Skip rotation and animation
        // L1263
        cx.sg
            .draw_image(Some(handle), x - width / 2.0, y - height / 2.0, width, height);
        // L1264
        return;
    }

    // L1267
    let t = cx.sys.time_ms() as f64 * 0.00003;
    // L1268
    let rot = angle * t;

    // L1270-1271
    let (hw, hh) = (width / 2.0, height / 2.0);
    let (cos_a, sin_a) = (rot.cos(), rot.sin());

    // L1273-1280
    let x1 = x - hw * cos_a + hh * sin_a;
    let y1 = y - hw * sin_a - hh * cos_a;
    let x2 = x + hw * cos_a + hh * sin_a;
    let y2 = y + hw * sin_a - hh * cos_a;
    let x3 = x + hw * cos_a - hh * sin_a;
    let y3 = y + hw * sin_a + hh * cos_a;
    let x4 = x - hw * cos_a - hh * sin_a;
    let y4 = y - hw * sin_a + hh * cos_a;

    // L1282
    cx.sg
        .draw_image_quad(Some(handle), [x1, y1, x2, y2, x3, y3, x4, y4]);
}

/// lua: PassiveTreeView.lua:793-807 `local function prepSearch(search)`
fn prep_search(search: &str) -> Vec<String> {
    // L794
    let mut search = search.to_ascii_lowercase();
    //gsub("([%[%]%%])", "%%%1")
    // L796
    let mut search_words: Vec<String> = Vec::new();
    // L797 (gmatch over the string as it was when the loop started)
    let quoted: Vec<String> = pob_lua::gmatch(&search, "\"([^\"]*)\"")
        .map(|c| c[0].to_str().into_owned())
        .collect();
    for matchstring in quoted {
        // L798
        search_words.push(matchstring.clone());
        // L799
        let escaped = pob_lua::gsub(&matchstring, "([%(%)])", "%%%1", None).0;
        search = pob_lua::gsub(&search, &format!("\"{escaped}\""), "", None).0;
    }
    // L801
    let words: Vec<String> = pob_lua::gmatch(&search, "(%S*)")
        .map(|c| c[0].to_str().into_owned())
        .collect();
    for matchstring in words {
        // L802
        if pob_lua::lmatch(&matchstring, "%S", None).is_some() {
            // L803
            search_words.push(matchstring);
        }
    }
    // L806
    search_words
}
