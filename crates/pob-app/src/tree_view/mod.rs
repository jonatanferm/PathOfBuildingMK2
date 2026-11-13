//! Port of PathOfBuilding v2.67.2 (16de4b82d) src/Classes/PassiveTreeView.lua 1-1411.
//! Owner: U23 (the class, its constructor, Load/Save and the search match here; Draw in
//! `input.rs` + `draw.rs`; the compare helpers in `compare.rs`). U24 owns 1412-1856 in
//! `names.rs`/`tooltip.rs`.
//!
//! @pob-file Classes/PassiveTreeView.lua @16de4b82d
//! @pob-file Modules/Common.lua @16de4b82d
//!
//! The view is an object a TreeTab owns (`self.viewer`, TreeTab.lua:33, U25), lent through
//! `cx.lend` while it draws (ui doc §7.2): its methods take `&mut self` and `&mut Cx`.
//!
//! Calls into units that are not ported yet go through [`TreeViewHooks`] (CCR-0115): U24's
//! `AddNodeTooltip`/`AddCompareNodeTooltip`, U25's `OpenMasteryPopup`/`ModifyNodePopup`, U26's
//! `calcsTab:BuildPower()` call site, and the jewel-socket right click into U32/U34's item slot
//! controls. `spec:AddUndoState()` is U19's `BuildHooks::spec_add_undo_state` (U25 installs it).
//!
//! Lua semantics:
//! * Node identity (`node == hoverNode`, `hoverPath[node]`, `isValueInArray`) is `Rc` pointer
//!   identity.
//! * `self.zoomX ~= nil and ... or 1` (L280-281): `zoomX` is nil only after a Load with a
//!   non-numeric attribute (`tonumber` returns nil), so it is `Option<f64>`.
//! * `self.searchStrResults[nodeId]` is never wiped: entries of nodes that left the spec stay.
//! * `PCall(search, ...)` in DoesNodeMatchSearchParams: a malformed search pattern raises inside
//!   `matchOrPattern`; the PCall turns it into `return false` ([`p_call`]).
//!
//! DIVERGENCE: none beyond the hook seams (CCR-0115). ORDER: see `draw.rs` / `input.rs`;
//! `pairs(orPatterns)` in `matchOrPattern` (Common.lua:1073) only decides whether *some* pattern
//! matches (order-free unless one pattern raises, which only a malformed search does) (c).

pub mod assets;
pub mod compare;
pub mod connectors;
pub mod draw;
pub mod input;
pub mod names;
pub mod tooltip;

use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;

use pob_lua::{n, Shared};
use pob_sg::{ImageFlags, ImageHandle};
use pob_ui_core::prelude::*;
use pob_ui_core::tooltip::Tooltip;

use crate::app::CxApp;
use crate::model::xml::XmlNode;
use crate::model::{self, NodeRef, PassiveSpecRef, Table};

// @pob Classes/PassiveTreeView.lua:1-16 h=e942454c
// @pob Classes/PassiveTreeView.lua:20-22 h=e36030b4
// @pob Classes/PassiveTreeView.lua:27-30 h=f613530b
// (module locals: pairs/ipairs/t_insert/t_remove/m_min/m_max/m_floor/band/b_rshift; the three
// JEWEL_RADIUS_TINT_* tints (draw.rs); `gemTooltip = require("Classes.GemTooltip")` (unused in
// this range); `isAbyssConquered` (compare.rs); `newClass("PassiveTreeView")`)

/// The PassiveTreeView object (a widget-owned object lent through `cx.lend`, ui doc §7.2).
#[derive(Default)]
pub struct PassiveTreeView {
    /// `self.ring` .. `self.kalguur2` (the ring and jewel-radius images)
    pub ring: ImageHandle,
    pub highlight_ring: ImageHandle,
    pub jewel_shaded_outer_ring: ImageHandle,
    pub jewel_shaded_outer_ring_flipped: ImageHandle,
    pub jewel_shaded_inner_ring: ImageHandle,
    pub jewel_shaded_inner_ring_flipped: ImageHandle,
    pub eternal1: ImageHandle,
    pub eternal2: ImageHandle,
    pub karui1: ImageHandle,
    pub karui2: ImageHandle,
    pub maraketh1: ImageHandle,
    pub maraketh2: ImageHandle,
    pub templar1: ImageHandle,
    pub templar2: ImageHandle,
    pub vaal1: ImageHandle,
    pub vaal2: ImageHandle,
    pub kalguur1: ImageHandle,
    pub kalguur2: ImageHandle,
    /// `self.tooltip` / `self.skillTooltip`
    pub tooltip: Tooltip,
    pub skill_tooltip: Tooltip,
    /// `self.zoomLevel`, `self.zoom`
    pub zoom_level: f64,
    pub zoom: f64,
    /// `self.zoomX` / `self.zoomY` (nil after a Load of a non-numeric attribute)
    pub zoom_x: Option<f64>,
    pub zoom_y: Option<f64>,
    /// `self.searchStr`, `self.searchStrSaved`, `self.searchStrCached`
    pub search_str: String,
    pub search_str_saved: String,
    pub search_str_cached: String,
    /// `self.searchStrResults[nodeId]` (truthiness of the stored value)
    pub search_str_results: HashMap<i64, bool>,
    /// `self.searchParams` (nil until the first search update)
    pub search_params: Option<Vec<String>>,
    /// `self.showStatDifferences`
    pub show_stat_differences: bool,
    /// `self.hoverNode`
    pub hover_node: Option<NodeRef>,
    /// `self.showHeatMap` (nil and false are both false)
    pub show_heat_map: bool,
    /// `self.heatMapStat` (`build.calcsTab.powerStat`)
    pub heat_map_stat: Option<Table>,
    /// `self.dragX` / `self.dragY` (nil when no drag is pending)
    pub drag_x: Option<f64>,
    pub drag_y: Option<f64>,
    /// `self.dragging` (nil and false are both false)
    pub dragging: bool,
    /// `self.traceMode` / `self.tracePath`
    pub trace_mode: bool,
    pub trace_path: Option<Rc<Shared<Vec<NodeRef>>>>,
    /// `self.compareSpec` (set by the TreeTab before each Draw)
    pub compare_spec: Option<PassiveSpecRef>,
}

/// The calls into later units (module notes; CCR-0115).
#[derive(Clone)]
pub struct TreeViewHooks {
    /// `self:AddNodeTooltip(tooltip, node, build)` (U24, PassiveTreeView.lua:1534-1856)
    pub add_node_tooltip: Rc<dyn Fn(&mut Cx, &mut PassiveTreeView, &mut Tooltip, &NodeRef)>,
    /// `self:AddCompareNodeTooltip(tooltip, node, build)` (U24)
    pub add_compare_node_tooltip: Rc<dyn Fn(&mut Cx, &mut PassiveTreeView, &mut Tooltip, &NodeRef)>,
    /// `build.treeTab:OpenMasteryPopup(node, viewPort)` (U25, TreeTab.lua)
    pub open_mastery_popup: Rc<dyn Fn(&mut Cx, &NodeRef, &ViewPort)>,
    /// `build.treeTab:ModifyNodePopup(node, viewPort)` (U25, TreeTab.lua)
    pub modify_node_popup: Rc<dyn Fn(&mut Cx, &NodeRef, &ViewPort)>,
    /// `build.calcsTab:BuildPower()` (the call site at PassiveTreeView.lua:785 that U26 steps)
    pub build_power: Rc<dyn Fn(&mut Cx)>,
    /// PassiveTreeView.lua:503-510: `local slot = build.itemsTab.sockets[hoverNode.id]; if
    /// slot:IsEnabled() then slot.dropped = true; build.itemsTab:SelectControl(slot);
    /// build.viewMode = "ITEMS" end` (the ItemSlotControl is U32's, ItemsTab U33/U34)
    pub socket_right_click: Rc<dyn Fn(&mut Cx, f64)>,
}

/// The owners' ports (the tooltips are U24's, the mastery/modify popups U25's, the jewel socket's
/// right click U32/U34's), and `BuildPower`, which runs
/// calc:PWR's `CalcsTab:BuildPower` once per frame, as PoB resumes it.
pub fn default_hooks() -> TreeViewHooks {
    TreeViewHooks {
        // CCR-0115 A: U24's ports (tooltip.rs)
        add_node_tooltip: Rc::new(|cx, view, tt, node| view.add_node_tooltip(cx, tt, node, false)),
        add_compare_node_tooltip: Rc::new(|cx, view, tt, node| {
            view.add_compare_node_tooltip(cx, tt, node)
        }),
        open_mastery_popup: Rc::new(crate::tabs::tree::core::open_mastery_popup_hook),
        modify_node_popup: Rc::new(crate::tabs::tree::core::modify_node_popup_hook),
        build_power: Rc::new(crate::tabs::tree::power::build_power), // U26
        // CCR-0145 §5 (U53 cut-over): U32's slot control and U34's ItemsTab
        socket_right_click: Rc::new(socket_right_click),
    }
}

/// PassiveTreeView.lua:523-529 (a right click on an allocated jewel socket): `local slot =
/// build.itemsTab.sockets[hoverNode.id]; if slot:IsEnabled() then slot.dropped = true;
/// build.itemsTab:SelectControl(slot); build.viewMode = "ITEMS" end`.
fn socket_right_click(cx: &mut Cx, node_id: f64) {
    use crate::tabs::items::{slot::ItemSlotControl, slot_helper, ItemsTab};
    // L523
    let tab = slot_helper::items_tab(cx)
        .unwrap_or_else(|| lua_error!("attempt to index field 'itemsTab' (a nil value)"));
    let slot = if node_id.fract() == 0.0 {
        cx.w::<ItemsTab>(tab).ctor.sockets.get(node_id as i64)
    } else {
        None
    };
    let Some(slot) = slot else {
        lua_error!("attempt to index local 'slot' (a nil value)")
    };
    // L524
    if cx.is_enabled(slot) {
        // L526
        cx.w_mut::<ItemSlotControl>(slot).dd.dropped = true;
        // L527
        cx.select_control(tab, Some(slot));
        // L528
        cx.build()
            .model
            .viewMode
            .set(crate::model::Name::from("ITEMS"));
    }
}

thread_local! {
    static HOOKS: RefCell<Option<TreeViewHooks>> = const { RefCell::new(None) };
}

/// The installed hooks (a clone; entries are `Rc`s).
pub fn hooks() -> TreeViewHooks {
    HOOKS.with(|h| h.borrow_mut().get_or_insert_with(default_hooks).clone())
}

/// Replace hook entries (owners at run time, tests).
pub fn set_hooks(f: impl FnOnce(&mut TreeViewHooks)) {
    HOOKS.with(|h| f(h.borrow_mut().get_or_insert_with(default_hooks)));
}

/// `NewImageHandle()` + `:Load(path, "CLAMP")`.
fn clamp_image(cx: &mut Cx, path: &str) -> ImageHandle {
    let h = cx.sg.new_image_handle();
    h.load(&mut cx.sg, path, ImageFlags::CLAMP);
    h
}

/// `PCall(f, ...)`: `Err` when `f` raised a Lua error.
fn p_call<R>(f: impl FnOnce() -> R) -> Result<R, String> {
    std::panic::catch_unwind(std::panic::AssertUnwindSafe(f))
        .map_err(|p| pob_host::panic_message(&*p))
}

impl PassiveTreeView {
    /// lua: PassiveTreeView.lua:31-85 `new("PassiveTreeView"):PassiveTreeView()`
    /// @pob Classes/PassiveTreeView.lua:31-85 h=42cfdaf5
    pub fn new(cx: &mut Cx) -> PassiveTreeView {
        // L32-43
        let ring = clamp_image(cx, "Assets/ring.png");
        let highlight_ring = clamp_image(cx, "Assets/small_ring.png");
        let jewel_shaded_outer_ring = clamp_image(cx, "Assets/ShadedOuterRing.png");
        let jewel_shaded_outer_ring_flipped = clamp_image(cx, "Assets/ShadedOuterRingFlipped.png");
        let jewel_shaded_inner_ring = clamp_image(cx, "Assets/ShadedInnerRing.png");
        let jewel_shaded_inner_ring_flipped = clamp_image(cx, "Assets/ShadedInnerRingFlipped.png");
        // L45-68
        let eternal1 = clamp_image(
            cx,
            "TreeData/PassiveSkillScreenEternalEmpireJewelCircle1.png",
        );
        let eternal2 = clamp_image(
            cx,
            "TreeData/PassiveSkillScreenEternalEmpireJewelCircle2.png",
        );
        let karui1 = clamp_image(cx, "TreeData/PassiveSkillScreenKaruiJewelCircle1.png");
        let karui2 = clamp_image(cx, "TreeData/PassiveSkillScreenKaruiJewelCircle2.png");
        let maraketh1 = clamp_image(cx, "TreeData/PassiveSkillScreenMarakethJewelCircle1.png");
        let maraketh2 = clamp_image(cx, "TreeData/PassiveSkillScreenMarakethJewelCircle2.png");
        let templar1 = clamp_image(cx, "TreeData/PassiveSkillScreenTemplarJewelCircle1.png");
        let templar2 = clamp_image(cx, "TreeData/PassiveSkillScreenTemplarJewelCircle2.png");
        let vaal1 = clamp_image(cx, "TreeData/PassiveSkillScreenVaalJewelCircle1.png");
        let vaal2 = clamp_image(cx, "TreeData/PassiveSkillScreenVaalJewelCircle2.png");
        let kalguur1 = clamp_image(cx, "TreeData/PassiveSkillScreenKalguuranJewelCircle1.png");
        let kalguur2 = clamp_image(cx, "TreeData/PassiveSkillScreenKalguuranJewelCircle2.png");
        // L70-71
        let tooltip = Tooltip::default();
        let skill_tooltip = Tooltip::default();
        // L73-76
        let zoom_level = 3.0;
        let zoom = pob_lua::math::lpow(1.2, zoom_level);
        PassiveTreeView {
            ring,
            highlight_ring,
            jewel_shaded_outer_ring,
            jewel_shaded_outer_ring_flipped,
            jewel_shaded_inner_ring,
            jewel_shaded_inner_ring_flipped,
            eternal1,
            eternal2,
            karui1,
            karui2,
            maraketh1,
            maraketh2,
            templar1,
            templar2,
            vaal1,
            vaal2,
            kalguur1,
            kalguur2,
            tooltip,
            skill_tooltip,
            zoom_level,
            zoom,
            zoom_x: Some(0.0),
            zoom_y: Some(0.0),
            // L78-81
            search_str: String::new(),
            search_str_saved: String::new(),
            search_str_cached: String::new(),
            search_str_results: HashMap::new(),
            search_params: None,
            // L82
            show_stat_differences: true,
            // L83
            hover_node: None,
            show_heat_map: false,
            heat_map_stat: None,
            drag_x: None,
            drag_y: None,
            dragging: false,
            trace_mode: false,
            trace_path: None,
            compare_spec: None,
        }
    }

    /// lua: PassiveTreeView.lua:87-103 `PassiveTreeView:Load(xml, fileName)`
    /// @pob Classes/PassiveTreeView.lua:87-103 h=14ceaaf6
    pub fn load(&mut self, xml: &XmlNode, _file_name: Option<&str>) {
        let attr = |k: &str| xml.attrib.get(k).map(|v| v.to_string());
        // L88
        if let Some(zl) = attr("zoomLevel") {
            // L89-90 (`1.2 ^ nil` raises after tonumber fails)
            let Some(zl) = pob_lua::tonumber(&zl) else {
                lua_error!("attempt to perform arithmetic on field 'zoomLevel' (a nil value)")
            };
            self.zoom_level = zl;
            self.zoom = pob_lua::math::lpow(1.2, self.zoom_level);
        }
        // L92
        if let (Some(zx), Some(zy)) = (attr("zoomX"), attr("zoomY")) {
            // L93-94
            self.zoom_x = pob_lua::tonumber(&zx);
            self.zoom_y = pob_lua::tonumber(&zy);
        }
        // L96
        if let Some(s) = attr("searchStr") {
            // L97-98
            self.search_str = s.clone();
            self.search_str_saved = s;
        }
        // L100
        if let Some(s) = attr("showStatDifferences") {
            // L101
            self.show_stat_differences = s == "true";
        }
    }

    /// lua: PassiveTreeView.lua:105-114 `PassiveTreeView:Save(xml)`
    /// @pob Classes/PassiveTreeView.lua:105-114 h=0b0fa7ef
    pub fn save(&mut self, xml: &mut XmlNode) {
        // L106
        self.search_str_saved = self.search_str.clone();
        let opt = |v: Option<f64>| v.map_or_else(|| "nil".to_owned(), pob_lua::tostring);
        // L107-113 (a string-keyed table: the writer's attribute order is `pairs` order)
        let mut attrib = indexmap::IndexMap::new();
        attrib.insert(
            "zoomLevel".into(),
            pob_lua::tostring(self.zoom_level).into(),
        );
        attrib.insert("zoomX".into(), opt(self.zoom_x).into());
        attrib.insert("zoomY".into(), opt(self.zoom_y).into());
        attrib.insert("searchStr".into(), self.search_str.as_str().into());
        attrib.insert(
            "showStatDifferences".into(),
            if self.show_stat_differences {
                "true"
            } else {
                "false"
            }
            .into(),
        );
        xml.attrib = attrib;
    }

    /// lua: PassiveTreeView.lua:1334-1407 `PassiveTreeView:DoesNodeMatchSearchParams(node)`:
    /// `None` is Lua's `nil` (the bare `return`s and the fall-through), `Some(b)` a boolean.
    /// @pob Classes/PassiveTreeView.lua:1334-1407 h=41abf60d
    pub fn does_node_match_search_params(&self, cx: &Cx, node: &model::Node) -> Option<bool> {
        let ty = node.r#type.get();
        // L1335
        if &*ty == "ClassStart" || (&*ty == "Mastery" && node.masteryEffects.get().is_none()) {
            // L1336
            return None;
        }

        // L1339
        let mut need_matches: Vec<String> = self.search_params.clone().unwrap_or_default();
        // L1338 local err

        // L1342-1349 local function search(haystack, need)
        let search = |haystack: &str, need: &mut Vec<String>| {
            // L1343
            let mut i = need.len();
            while i >= 1 {
                // L1344
                if pob_lua::string::matchOrPattern(haystack, &need[i - 1]) {
                    // L1345
                    need.remove(i - 1);
                }
                i -= 1;
            }
        };
        // `err, needMatches = PCall(search, ..., needMatches)`: `search` removes in place and
        // returns the same table; a raise leaves it half-filtered and Lua returns false.
        let run = |haystack: &str, need: &mut Vec<String>| -> bool {
            p_call(|| search(haystack, need)).is_err()
        };

        // Check recipes
        // L1352
        if need_matches.first().map(String::as_str) == Some("oil:") {
            // L1353
            if let Some(recipe) = node.extra.get(n!("recipe")).table() {
                // L1354
                for (_, recipe_name) in recipe.ipairs() {
                    let recipe_name = recipe_name.str().map(|s| s.to_string()).unwrap_or_default();
                    // L1355
                    let hay = pob_lua::gsub(&recipe_name, "Oil", "", None)
                        .0
                        .to_ascii_lowercase();
                    if run(&hay, &mut need_matches) {
                        // L1356
                        return Some(false);
                    }
                    // L1357
                    if need_matches.len() == 1 && need_matches[0] == "oil:" {
                        // L1358
                        return Some(true);
                    }
                }
            }
            // L1362
            return Some(false);
        }

        // Check node name
        // L1366
        if run(&node.dn.get().to_ascii_lowercase(), &mut need_matches) {
            // L1367
            return Some(false);
        }
        // L1368
        if need_matches.is_empty() {
            // L1369
            return Some(true);
        }

        // Check node description
        // L1373
        let sd = node.sd.get().snapshot();
        let mods = node.mods.get().snapshot();
        for (index, line) in sd.iter().enumerate() {
            // Check display text first
            // L1375
            if run(&line.to_ascii_lowercase(), &mut need_matches) {
                // L1376
                return Some(false);
            }
            // L1377
            if need_matches.is_empty() {
                // L1378
                return Some(true);
            }
            // L1380 if #needMatches > 0 and node.mods[index].list then
            let list = mods
                .get(index)
                .unwrap_or_else(|| lua_error!("attempt to index field '?' (a nil value)"))
                .get(n!("list"));
            if !need_matches.is_empty() && list.truthy() {
                // Then check modifiers
                // L1382
                let list = list.table().map(|t| t.ipairs()).unwrap_or_default();
                for (_, m) in list {
                    let name = match &m {
                        Val::Mod(m) => m.name.to_string(),
                        other => other
                            .get(n!("name"))
                            .str()
                            .map(|s| s.to_string())
                            .unwrap_or_default(),
                    };
                    // L1383
                    if run(&name, &mut need_matches) {
                        // L1384
                        return Some(false);
                    }
                    // L1385
                    if need_matches.is_empty() {
                        // L1386
                        return Some(true);
                    }
                }
            }
        }

        // Check node type
        // L1393
        if run(&ty.to_ascii_lowercase(), &mut need_matches) {
            // L1394
            return Some(false);
        }
        // L1395
        if need_matches.is_empty() {
            // L1396
            return Some(true);
        }

        // Check node id for devs
        // L1400
        if cx.app().launch.dev_mode {
            // L1401
            let id = match pob_calc::classes::passive_tree::node_id_val(node) {
                // CCR-0115
                Val::Num(x) => pob_lua::tostring(x),
                Val::Str(s) => s.to_string(),
                _ => "nil".to_owned(),
            };
            if run(&id, &mut need_matches) {
                // L1402
                return Some(false);
            }
            // L1403
            if need_matches.is_empty() {
                // L1404
                return Some(true);
            }
        }
        None
    }
}

use crate::model::Val;
