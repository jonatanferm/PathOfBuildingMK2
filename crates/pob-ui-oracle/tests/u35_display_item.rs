//! U35 acceptance (ui doc §12 U35 row: the display item, ItemsTab.lua:1948-2608; "create/edit
//! display item; variants; affix sliders; cluster controls; custom controls"): the full app
//! (Launch, the build shell, the Items tab of U33 with U35's display item functions) on both
//! sides of the UI oracle, in u22's form (the wave-4 template; the harness is u33's). Owner: U35.
//!
//! `u35_scenarios`: every `tests/u35/*.pobscript` at 1280x800 @1.0 and 1920x1080 @1.5 (both 1280
//! virtual px wide), or once at the script's own `screen`. Each script loads a fixture build,
//! switches to the Items tab (Ctrl+3), creates display items (item list, unique DB, rare
//! templates: CreateDisplayItemFromRaw/SetDisplayItem) and edits them through the display item
//! controls (variant dropdowns, affix dropdowns and roll sliders, the modifier sorting, influence,
//! the cluster jewel skill and node count, the custom modifier Remove buttons, the range line
//! dropdown). Per run: zero T3 divergences (draw ops per `(layer, sub)` bucket and side effects,
//! frame by frame), the same window titles, and T4 at every `snapshot` step (u22's
//! normalisation).
//!
//! The Rust side's calc is U15's LuaCalcBridge (as u20-u22, u32, u33).
//!
//! What runs on each side:
//! * PoB's ItemsTab (Lua); U33's constructor with U35's `display_item.rs` behind the hooks
//!   (Rust, the default `ItemsUiHooks`/`ItemsTabHooks` entries);
//! * `ItemsTab:Draw` (U34's, ItemsTab.lua:1419-1572): PoB's on the Lua side; on the Rust side
//!   u33's test-only port [`draw_items`], without the keyboard shortcuts of L1460-1497 (the port
//!   fails the run if one arrives); both sides then draw one status line (`display:
//!   <displayItem.name> undo: <count>`);
//! * `CopyAnointsAndEldritchImplicits` (U34, ItemsTab.lua:1906-1946): PoB's on the Lua side,
//!   U34's `core::CopyAnointsAndEldritchImplicits` on the Rust side (called directly);
//! * stand-ins on both sides, for methods of units that are not ported: `AddItemTooltip` (U38:
//!   the item name, the slot and `dbMode`, then every implicit, enchant, explicit and crucible
//!   mod line's text with its `crafted`/`custom`/`disabled` flags, so each craft is visible),
//!   the socket viewer's `Draw` (U23), the undo count around `AddUndoState` (U38), every other
//!   tab's `Draw` (nothing, as u22: the Rust side masks the Tree arm of `ShellHooks::draw_tab`,
//!   the tab a build opens on) and u22's `utf8.next`.

#![allow(
    non_snake_case,
    clippy::many_single_char_names,
    clippy::doc_lazy_continuation
)]

use std::cell::{Cell, RefCell};
use std::path::{Path, PathBuf};
use std::rc::Rc;

use pob_app::app::CxApp;
use pob_app::build::init;
use pob_app::build::on_frame::{self as of, BuildTab, TabFlag};
use pob_app::model;
use pob_app::tabs::items::ctor;
use pob_app::tabs::items::slot::item_label;
use pob_app::tabs::items::slot_helper::{self as sh};
use pob_app::tabs::items::ItemsTab;
use pob_calc::model::{XmlChild, XmlNode};
use pob_host::{App, PlatformParts};
use pob_lua::n;
use pob_sg::trace::{FrameTrace, SideEffect};
use pob_sg::{Align, Font, Key, Sg, Sys};
use pob_ui_core::controls::scroll_bar::ScrollBarControl;
use pob_ui_core::controls::{DropDownControl, DropItem};
use pob_ui_core::prelude::*;
use pob_ui_core::tooltip::tooltip::Tooltip;
use pob_ui_oracle::boot::{self, Entry, OracleHandles};
use pob_ui_oracle::diff_report;
use pob_ui_oracle::lua_calc::LuaCalcBridge;
use pob_ui_oracle::runner::{self, Driver, LuaApp, RunResult, ScenarioApp, UserDir};

/// One LuaJIT PoB at a time (memory; ui doc §11.3).
static LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

const SCREENS: [(i32, i32, f32); 2] = [(1280, 800, 1.0), (1920, 1080, 1.5)];

fn inputs() -> Option<PathBuf> {
    let root = pob_testpaths::require(pob_testpaths::pob_root(), "PoB checkout (.PathOfBuilding)")?;
    let dump = pob_testpaths::require(pob_testpaths::dump_dir(), "data/dump")?;
    assert!(pob_oracle::testkit::init_data(&dump), "data dump");
    let src = std::fs::canonicalize(root.join("src")).unwrap_or_else(|_| root.join("src"));
    pob_gamedata::timeless_lut::set_jewel_fs(std::sync::Arc::new(
        pob_gamedata::timeless_lut::StdJewelFs {
            script_path: src.to_string_lossy().into_owned(),
            writable: false,
        },
    ));
    Some(root)
}

fn test_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/u35")
}

/// The Lua stand-ins (see the module notes).
const LUA_STUBS: &str = r#"
local lutf8 = require("lua-utf8")
if not lutf8.next then
	local function iscont(s, p)
		local b = s:byte(p + 1) or 0
		return b >= 0x80 and b < 0xC0
	end
	lutf8.next = function(s, i, off)
		local e = #s
		local p = i - 1
		local idx = off
		if idx >= 0 then
			while p < e and idx > 0 do
				while p < e and iscont(s, p + 1) do
					p = p + 1
				end
				p = (p < e) and p + 1 or e
				idx = idx - 1
			end
		else
			while 0 < p and idx < 0 do
				local q = p
				while 0 < q and iscont(s, q - 1) do
					q = q - 1
				end
				p = (0 < q) and q - 1 or 0
				idx = idx + 1
			end
		end
		if idx ~= 0 or p == e then
			return
		end
		return p + 1, s:byte(p + 1)
	end
end
local b = main.modes.BUILD
local origInit = b.Init
function b:Init(...)
	origInit(self, ...)
	for _, t in ipairs({ "importTab", "notesTab", "partyTab", "configTab", "treeTab", "skillsTab", "calcsTab", "compareTab" }) do
		if self[t] then self[t].Draw = function() end end
	end
	local it = self.itemsTab
	if not it then return end
	it.u35Undo = 0
	local origUndo = it.AddUndoState
	it.AddUndoState = function(s, ...)
		s.u35Undo = s.u35Undo + 1
		return origUndo(s, ...)
	end
	it.AddItemTooltip = function(s, tooltip, item, slot, dbMode)
		tooltip:AddLine(16, colorCodes[item.rarity] .. item.name)
		tooltip:AddLine(14, "^7slot " .. tostring(slot and slot.slotName) .. " db " .. tostring(dbMode))
		for _, k in ipairs({ "implicitModLines", "enchantModLines", "explicitModLines", "crucibleModLines" }) do
			for _, l in ipairs(item[k] or { }) do
				tooltip:AddLine(14, "^7" .. k:sub(1, 3) .. " " .. l.line .. (l.crafted and " C" or "") .. (l.custom and " U" or "") .. (l.disabled and " D" or ""))
			end
		end
	end
	it.socketViewer.Draw = function(viewer, build, viewPort, inputEvents)
		DrawString(4, 4, "LEFT", 14, "VAR", "^7viewer " .. viewer.zoom .. " " .. viewer.zoomX .. " " .. viewer.zoomY)
	end
	local origDraw = it.Draw
	it.Draw = function(s, viewPort, inputEvents)
		origDraw(s, viewPort, inputEvents)
		DrawString(viewPort.x + 4, viewPort.y + viewPort.height - 40, "LEFT", 16, "VAR", "^7display: " .. tostring(s.displayItem and s.displayItem.name) .. " undo: " .. s.u35Undo)
	end
end
"#;

thread_local! {
    /// The stand-in `spec.modFlag` (UndoHandler.lua:37 via `spec:AddUndoState`, U25).
    static SPEC_MOD_FLAG: Cell<bool> = const { Cell::new(false) };
    /// The stand-in `itemsTab.modFlag` (UndoHandler, U38) and the undo count.
    static ITEMS_MOD_FLAG: Cell<bool> = const { Cell::new(false) };
    static UNDO: Cell<u32> = const { Cell::new(0) };
}

fn tab(cx: &Cx) -> CtrlId {
    ctor::tab(cx).expect("U33's ItemsTab")
}

/// ItemsTab.lua:1419-1572 `Draw` (U34's; test port, see the module notes).
fn draw_items(cx: &mut Cx, vp: &ViewPort, ev: &mut InputEvents) {
    let me = tab(cx);
    {
        let b = cx.base_mut(me);
        // L1420-1423
        b.x = Prop::Val(vp.x);
        b.y = Prop::Val(vp.y);
        b.width = Prop::Val(vp.width);
        b.height = Prop::Val(vp.height);
    }
    let sbh = cx.child(me, "scrollBarH").expect("scrollBarH");
    let sbv = cx.child(me, "scrollBarV").expect("scrollBarV");
    {
        // L1424-1429
        let b = cx.base_mut(sbh);
        b.width = Prop::Val(vp.width);
        b.x = Prop::Val(vp.x);
        b.y = Prop::Val(vp.y + vp.height - 18.0);
        let b = cx.base_mut(sbv);
        b.height = Prop::Val(vp.height - 18.0);
        b.x = Prop::Val(vp.x + vp.width - 18.0);
        b.y = Prop::Val(vp.y);
    }
    {
        let st = &cx.w::<ItemsTab>(me).ctor;
        let (last_slot, anchor, has_item) = (
            ctor::lastSlot(cx, me).expect("lastSlot"),
            st.anchorDisplayItem,
            st.displayItem.is_some(),
        );
        // L1431
        let mut maxY = cx.pos(last_slot).1 + 24.0;
        // L1432
        let mut maxX = cx.pos(anchor).0 + 462.0;
        // L1433
        if has_item {
            // L1434
            let (x, y) = cx.pos(cx.child(me, "displayItemTooltipAnchor").expect("anchor"));
            // L1435
            let (ttW, ttH) = cx
                .w::<ItemsTab>(me)
                .ctor
                .displayItemTooltip
                .get_dynamic_size(cx, vp);
            // L1436-1437
            maxY = pob_lua::math::m_max(maxY, y + ttH + 4.0);
            maxX = pob_lua::math::m_max(maxX, x + ttW + 80.0);
        }
        // L1439-1440
        let contentHeight = maxY - cx.base(me).y.raw();
        let contentWidth = maxX - cx.base(me).x.raw();
        // L1441-1445
        let mut v = contentHeight > vp.height;
        let h = contentWidth > vp.width - if v { 20.0 } else { 0.0 };
        if h {
            v = contentHeight > vp.height - 20.0;
        }
        // L1446-1447
        ScrollBarControl::set_content_dimension(
            cx,
            sbv,
            contentHeight,
            vp.height - if h { 20.0 } else { 0.0 },
        );
        ScrollBarControl::set_content_dimension(
            cx,
            sbh,
            contentWidth,
            vp.width - if v { 20.0 } else { 0.0 },
        );
        // L1448-1452
        match cx.w::<ItemsTab>(me).ctor.snapHScroll {
            Some("RIGHT") => {
                let m = cx.w::<ScrollBarControl>(sbh).offset_max.expect("offsetMax");
                ScrollBarControl::set_offset(cx, sbh, m);
            }
            Some("LEFT") => ScrollBarControl::set_offset(cx, sbh, 0.0),
            _ => {}
        }
        // L1453
        cx.w_mut::<ItemsTab>(me).ctor.snapHScroll = None;
        // L1454
        let max_y = if h {
            cx.base(sbh).y.raw()
        } else {
            vp.y + vp.height
        };
        cx.w_mut::<ItemsTab>(me).ctor.maxY = Some(max_y);
    }
    // L1456-1457
    let (ox, oy) = (
        cx.w::<ScrollBarControl>(sbh).offset,
        cx.w::<ScrollBarControl>(sbv).offset,
    );
    let (x, y) = (cx.base(me).x.raw() - ox, cx.base(me).y.raw() - oy);
    cx.base_mut(me).x = Prop::Val(x);
    cx.base_mut(me).y = Prop::Val(y);

    // L1459-1497: the shortcuts are not part of the stand-in
    let mut i = 0;
    while let Some(e) = ev.next_ipairs(&mut i) {
        if let EventKind::KeyDown { .. } = e.kind {
            let ctrl = cx.sys.is_key_down(Key::Ctrl);
            let short = match e.key {
                // L1466-1471: only a slot with an item under the mouse does something
                Key::Char(b'e') => cx.get_mouse_over_control(me).is_some_and(|c| {
                    cx.try_w::<pob_app::tabs::items::slot::ItemSlotControl>(c)
                        .is_some_and(|w| w.slot.get(n!("selItemId")).num() != 0.0)
                }),
                Key::Char(b'v' | b'z' | b'y' | b'f' | b'd') => ctrl,
                _ => false,
            };
            assert!(
                !short,
                "U33's Draw stand-in: shortcut {:?} reached the Items tab",
                e.key
            );
        }
    }
    // L1498
    cx.process_controls_input(me, ev, vp);
    // L1499-1515
    let mut i = 0;
    while let Some(e) = ev.next_ipairs(&mut i) {
        if e.kind == EventKind::KeyUp {
            let down = ScrollBarControl::is_scroll_down_key(cx, sbv, e.key);
            let up = !down && ScrollBarControl::is_scroll_up_key(cx, sbv, e.key);
            if down || up {
                let mult = if down { 1.0 } else { -1.0 };
                if cx.is_mouse_over(sbv) || !cx.is_shown(sbh) {
                    ScrollBarControl::scroll(cx, sbv, mult);
                } else {
                    ScrollBarControl::scroll(cx, sbh, mult);
                }
            }
        }
    }

    // L1517
    pob_app::main_mode::background::draw_background(cx, vp);

    // L1519-1527
    let m = cx.build().model.clone();
    let mut new_list = Vec::new();
    let set_select = cx.child(me, "setSelect").expect("setSelect");
    for (index, id) in m.itemsTab.itemSetOrderList.snapshot().iter().enumerate() {
        let set = m
            .itemsTab
            .itemSets
            .with(|s| s.get(id).cloned())
            .expect("itemSet");
        new_list.push(DropItem::str(
            set.str(n!("title"))
                .map_or_else(|| "Default".to_owned(), |t| t.to_string()),
        ));
        if f64::from(*id) == m.itemsTab.activeItemSetId.get() {
            cx.w_mut::<DropDownControl>(set_select).sel_index = index + 1;
        }
    }
    DropDownControl::set_list(cx, set_select, new_list);

    // L1529-1550
    if cx.w::<ItemsTab>(me).ctor.displayItem.is_some() {
        let (x, y) = cx.pos(cx.child(me, "displayItemTooltipAnchor").expect("anchor"));
        cx.lend::<ItemsTab, Tooltip, _>(
            me,
            |w| &mut w.ctor.displayItemTooltip,
            |cx, tt| {
                tt.draw(cx, x, y, None, None, vp);
                // L1535-1549 Toggle mods: the stand-in tooltip has no mod lines
                assert!(tt.lines.iter().all(|l| l.mod_line.is_none()));
            },
        );
    }

    // L1552
    m.itemsTab.UpdateSockets(&m);

    // L1554-1560
    let portrait = cx.app().main.portrait_mode;
    let item_list = cx.child(me, "itemList").expect("itemList");
    let set_manage = cx.child(me, "setManage").expect("setManage");
    let last_slot = ctor::lastSlot(cx, me);
    if portrait {
        cx.set_anchor(
            item_list,
            TopRight,
            last_slot,
            BottomRight,
            Some((Prop::Val(0.0), Prop::Val(40.0))),
            false,
        );
    } else {
        cx.set_anchor(
            item_list,
            TopLeft,
            Some(set_manage),
            TopRight,
            Some((Prop::Val(20.0), Prop::Val(20.0))),
            false,
        );
    }
    let to = if portrait { set_manage } else { item_list };
    let craft = cx.child(me, "craftDisplayItem").expect("craft");
    cx.set_anchor(
        craft,
        TopLeft,
        Some(to),
        TopRight,
        Some((
            Prop::Val(20.0),
            Prop::Val(if portrait { 0.0 } else { -20.0 }),
        )),
        false,
    );
    // L1560 `main.portraitMode and 0` is false (not a number) when not portrait: SetAnchor keeps
    // x/y only when both are given, so the anchor keeps its constructor offsets
    let anchor = cx.w::<ItemsTab>(me).ctor.anchorDisplayItem;
    if portrait {
        cx.set_anchor(
            anchor,
            TopLeft,
            Some(to),
            TopRight,
            Some((Prop::Val(20.0), Prop::Val(0.0))),
            false,
        );
    } else {
        cx.set_anchor(anchor, TopLeft, Some(to), TopRight, None, false);
    }

    // L1562
    cx.draw_controls(me, vp, None);
    // L1563-1568
    if cx.is_shown(sbh) {
        cx.draw(sbh, vp, false);
    }
    if cx.is_shown(sbv) {
        cx.draw(sbv, vp, false);
    }

    // L1570
    let f = init::hooks(cx).tree_get_spec_list;
    let list: Vec<DropItem> = f(cx).into_iter().map(DropItem::str).collect();
    let spec_select = cx.child(me, "specSelect").expect("specSelect");
    DropDownControl::set_list(cx, spec_select, list);

    // the status line
    let name = cx
        .w::<ItemsTab>(me)
        .ctor
        .displayItem
        .as_ref()
        .map_or_else(|| "nil".to_owned(), |i| i.name.get().to_string());
    let line = format!("^7display: {name} undo: {}", UNDO.with(Cell::get));
    cx.sg.draw_string(
        vp.x + 4.0,
        vp.y + vp.height - 40.0,
        Align::Left,
        16.0,
        Font::Var,
        &line,
    );
}

/// The `AddItemTooltip` stand-in (both sides).
fn add_item_tooltip(
    cx: &Cx,
    tooltip: &mut Tooltip,
    item: &model::ItemRef,
    slot: Option<&model::Table>,
    db: bool,
) {
    let l1 = item_label(cx, item);
    tooltip.add_line(cx, 16.0, &l1);
    let slot_name = slot
        .and_then(|s| s.str(n!("slotName")))
        .map_or_else(|| "nil".to_owned(), |s| s.to_string());
    let l2 = format!("^7slot {slot_name} db {}", if db { "true" } else { "nil" });
    tooltip.add_line(cx, 14.0, &l2);
    let kinds = [
        ("imp", item.implicitModLines.get()),
        ("enc", item.enchantModLines.get()),
        ("exp", item.explicitModLines.get()),
        ("cru", item.crucibleModLines.get()),
    ];
    for (k, lines) in kinds {
        for l in lines.snapshot() {
            let flag = |f: &str, s: &'static str| if l.truthy(f) { s } else { "" };
            let line = format!(
                "^7{k} {}{}{}{}",
                l.str(n!("line")).expect("line"),
                flag("crafted", " C"),
                flag("custom", " U"),
                flag("disabled", " D")
            );
            tooltip.add_line(cx, 14.0, &line);
        }
    }
}

fn rust_stubs() {
    SPEC_MOD_FLAG.with(|f| f.set(false));
    ITEMS_MOD_FLAG.with(|f| f.set(false));
    UNDO.with(|u| u.set(0));
    init::set_hooks(|h| {
        *h = init::default_hooks();
        h.spec_add_undo_state = Rc::new(|_| SPEC_MOD_FLAG.with(|f| f.set(true)));
        // U25 stand-in: TreeTab.lua:492-498 (as u20/u22/u32)
        h.tree_get_spec_list = Rc::new(|cx| {
            let m = &cx.build().model;
            let gv = &m.data.gameVersions;
            m.treeTab
                .specList
                .with(Clone::clone)
                .iter()
                .map(|spec| {
                    let tv = spec.treeVersion.get();
                    let prefix = if tv == gv.latestTreeVersion {
                        String::new()
                    } else {
                        format!("[{}] ", gv.treeVersions[&tv].display)
                    };
                    let title = spec
                        .title
                        .get()
                        .map_or_else(|| "Default".to_owned(), |s| s.to_string());
                    format!("{prefix}{title}")
                })
                .collect()
        });
    });
    sh::set_items_hooks(|h| {
        *h = sh::default_items_hooks();
        h.add_undo_state = Rc::new(|_| {
            UNDO.with(|u| u.set(u.get() + 1));
            ITEMS_MOD_FLAG.with(|f| f.set(true));
        });
        h.set_mod_flag = Rc::new(|_, v| ITEMS_MOD_FLAG.with(|f| f.set(v)));
        h.add_item_tooltip = Rc::new(|cx, _build, tooltip, item, slot, db| {
            add_item_tooltip(cx, tooltip, item, slot, db);
        });
        h.socket_viewer_draw = Rc::new(|cx, a, _vp| {
            let line = format!(
                "^7viewer {} {} {}",
                pob_lua::tostring_num(a.zoom),
                pob_lua::tostring_num(a.zoom_x),
                pob_lua::tostring_num(a.zoom_y)
            );
            cx.sg
                .draw_string(4.0, 4.0, Align::Left, 14.0, Font::Var, &line);
        });
    });
    ctor::set_items_ui_hooks(|h| *h = ctor::default_items_ui_hooks());
    of::set_shell_hooks(|h| {
        *h = of::default_shell_hooks();
        let (draw, flag, set_flag) = (
            h.draw_tab.clone(),
            h.tab_flag.clone(),
            h.set_tab_flag.clone(),
        );
        h.draw_tab = Rc::new(move |cx, t, vp, ev| {
            if t == BuildTab::Items {
                draw_items(cx, vp, ev);
            } else if t != BuildTab::Tree {
                // LUA_STUBS replaces `treeTab.Draw`: the Tree tab (U25) draws nothing here as well
                draw(cx, t, vp, ev);
            }
        });
        h.tab_flag = Rc::new(move |cx, f| match f {
            TabFlag::Spec => SPEC_MOD_FLAG.with(Cell::get),
            TabFlag::Items => ITEMS_MOD_FLAG.with(Cell::get),
            _ => flag(cx, f),
        });
        h.set_tab_flag = Rc::new(move |cx, f, v| match f {
            TabFlag::Spec => SPEC_MOD_FLAG.with(|c| c.set(v)),
            TabFlag::Items => ITEMS_MOD_FLAG.with(|c| c.set(v)),
            _ => set_flag(cx, f, v),
        });
    });
}

/// `#! NAME ARG` directives.
fn directives(text: &str, name: &str) -> Vec<String> {
    let tag = format!("#! {name}");
    text.lines()
        .filter_map(|l| l.trim().strip_prefix(tag.as_str()))
        .map(|s| s.trim().to_owned())
        .collect()
}

// ---- the Lua side ----

struct LuaSide {
    app: LuaApp,
    stubs: String,
}

impl App for LuaSide {
    fn sg(&mut self) -> &mut Sg {
        self.app.sg()
    }
    fn sys(&mut self) -> &mut Sys {
        self.app.sys()
    }
    fn on_init(&mut self) {
        self.app.on_init();
        self.app
            .with_vm(|pob| pob.eval::<()>(self.stubs.as_str()))
            .unwrap_or_else(|e| panic!("Lua stubs: {e}"));
    }
    fn on_frame(&mut self) {
        self.app.on_frame();
    }
    fn on_key_down(&mut self, key: Key, double_click: bool) {
        self.app.on_key_down(key, double_click);
    }
    fn on_key_up(&mut self, key: Key) {
        self.app.on_key_up(key);
    }
    fn on_char(&mut self, ch: Key) {
        self.app.on_char(ch);
    }
    fn on_lua_error(&mut self, msg: String) {
        self.app.on_lua_error(msg);
    }
}

impl ScenarioApp for LuaSide {
    fn load_build(&mut self, name: &str, xml: &str) -> anyhow::Result<()> {
        self.app.load_build(name, xml)
    }
    fn snapshot(&mut self) -> anyhow::Result<Option<String>> {
        self.app.snapshot()
    }
    fn after_frame(&mut self) -> anyhow::Result<()> {
        self.app.after_frame()
    }
    fn take_error(&mut self) -> Option<String> {
        self.app.take_error()
    }
}

// ---- the Rust side ----

/// Records the platform side effects the Lua side's bridge records (the oracle's Rust platform
/// does not, CCR-0028 item 2); as in u16-u21.
struct RecordingPlatform {
    inner: Box<dyn pob_sg::Platform>,
    effects: Rc<RefCell<Vec<SideEffect>>>,
}

impl pob_sg::Platform for RecordingPlatform {
    fn time_ms(&self) -> i64 {
        self.inner.time_ms()
    }
    fn copy(&mut self, text: &str) {
        self.effects
            .borrow_mut()
            .push(SideEffect::Copy(text.to_owned()));
        self.inner.copy(text);
    }
    fn paste(&mut self) -> Option<String> {
        self.inner.paste()
    }
    fn open_url(&mut self, url: &str) {
        self.effects
            .borrow_mut()
            .push(SideEffect::OpenUrl(url.to_owned()));
        self.inner.open_url(url);
    }
    fn set_window_title(&mut self, title: &str) {
        self.effects
            .borrow_mut()
            .push(SideEffect::WindowTitle(title.to_owned()));
        self.inner.set_window_title(title);
    }
    fn take_screenshot(&mut self) {
        self.effects.borrow_mut().push(SideEffect::Screenshot);
        self.inner.take_screenshot();
    }
    fn restart(&mut self) {
        self.inner.restart();
    }
    fn exit(&mut self, msg: Option<&str>) {
        self.effects.borrow_mut().push(SideEffect::Exit);
        self.inner.exit(msg);
    }
    fn script_path(&self) -> String {
        self.inner.script_path()
    }
    fn runtime_path(&self) -> String {
        self.inner.runtime_path()
    }
    fn user_path(&self) -> Result<String, (String, String)> {
        self.inner.user_path()
    }
    fn con_print(&mut self, s: &str) {
        self.inner.con_print(s);
    }
}

struct NullPlatform;
impl pob_sg::Platform for NullPlatform {
    fn time_ms(&self) -> i64 {
        0
    }
    fn copy(&mut self, _: &str) {}
    fn paste(&mut self) -> Option<String> {
        None
    }
    fn open_url(&mut self, _: &str) {}
    fn set_window_title(&mut self, _: &str) {}
    fn take_screenshot(&mut self) {}
    fn restart(&mut self) {}
    fn exit(&mut self, _: Option<&str>) {}
    fn script_path(&self) -> String {
        String::new()
    }
    fn runtime_path(&self) -> String {
        String::new()
    }
    fn user_path(&self) -> Result<String, (String, String)> {
        Ok(String::new())
    }
    fn con_print(&mut self, _: &str) {}
}

struct RustSide {
    app: pob_app::PobApp,
    effects: Rc<RefCell<Vec<SideEffect>>>,
}

impl RustSide {
    fn new(mut parts: PlatformParts, src: PathBuf) -> RustSide {
        let effects = Rc::new(RefCell::new(Vec::new()));
        let inner = std::mem::replace(&mut parts.sys.platform, Box::new(NullPlatform));
        parts.sys.platform = Box::new(RecordingPlatform {
            inner,
            effects: effects.clone(),
        });
        let mut app = pob_app::PobApp::with_calc(
            parts,
            Rc::new(move || Box::new(LuaCalcBridge::new(Some(src.clone())))),
        );
        effects.borrow_mut().clear(); // Launch.lua:11 runs at script load, before tracing
                                      // pob-ui-oracle patch 4's equivalent (CCR-0091)
        app.cx.app_mut().launch.force_release_install = true;
        RustSide { app, effects }
    }
    fn flush(&mut self) {
        let e: Vec<_> = self.effects.borrow_mut().drain(..).collect();
        if let Some(t) = self.app.cx.sg.trace.as_mut() {
            for e in e {
                t.effect(e);
            }
        }
    }
}

impl App for RustSide {
    fn sg(&mut self) -> &mut Sg {
        self.app.sg()
    }
    fn sys(&mut self) -> &mut Sys {
        self.app.sys()
    }
    fn on_init(&mut self) {
        rust_stubs();
        self.app.on_init();
        self.flush();
    }
    fn on_frame(&mut self) {
        self.app.on_frame();
        self.flush();
    }
    fn on_key_down(&mut self, key: Key, double_click: bool) {
        self.app.on_key_down(key, double_click);
        self.flush();
    }
    fn on_key_up(&mut self, key: Key) {
        self.app.on_key_up(key);
        self.flush();
    }
    fn on_char(&mut self, ch: Key) {
        self.app.on_char(ch);
        self.flush();
    }
    fn on_lua_error(&mut self, msg: String) {
        self.app.on_lua_error(msg);
    }
}

impl ScenarioApp for RustSide {
    fn load_build(&mut self, name: &str, xml: &str) -> anyhow::Result<()> {
        self.app.load_build(name, xml);
        Ok(())
    }
    fn snapshot(&mut self) -> anyhow::Result<Option<String>> {
        Ok(self.app.snapshot())
    }
    fn after_frame(&mut self) -> anyhow::Result<()> {
        // the Lua side fails the run on PoB's error prompt; so does this one
        if let Some(p) = self
            .app
            .cx
            .app()
            .launch
            .prompt_msg
            .clone()
            .filter(|p| p.starts_with("^1Error"))
        {
            anyhow::bail!("the error prompt (launch:ShowErrMsg): {p}");
        }
        Ok(())
    }
}

// ---- T4 (the rules are u20's; see its module notes) ----

fn unordered_children(elem: &str) -> bool {
    matches!(
        elem,
        "Calcs" | "ConfigSet" | "ItemSet" | "Sockets" | "Overrides"
    )
}

/// The UI-only savers not ported yet: compared separately, a difference is reported.
const PENDING_SAVERS: [&str; 2] = ["Import", "TreeView"];

#[derive(Default)]
struct T4Notes {
    pending_sections: Vec<String>,
    gem_nil_bools: usize,
    ambiguous_variants: usize,
}

fn ambiguous_variant(node: &XmlNode) -> bool {
    let (Some(game_id), Some(skill_id)) = (node.attrib.get("gemId"), node.attrib.get("skillId"))
    else {
        return false;
    };
    let n = pob_gamedata::data()
        .gems
        .values()
        .filter(|g| *g.gameId == **game_id && *g.grantedEffectId == **skill_id)
        .count();
    n > 1
}

fn normalise(node: &XmlNode, top: bool, lua: bool, notes: &mut T4Notes) -> String {
    let mut attrs: Vec<(String, String)> = Vec::new();
    for (k, v) in &node.attrib {
        let rust_default = match (&*node.elem, &**k) {
            ("Gem", "enableGlobal1" | "enableGlobal2") | ("Skill", "includeInFullDPS") => {
                Some("false")
            }
            ("Skill", "mainActiveSkill" | "mainActiveSkillCalcs") => Some("0"),
            // `count`: an item-granted gem of a group UpdateSocketGroups re-creates after an
            // equip change has `count = nil` in PoB and 1 in calc:BM-B's port (the calc reads
            // `count or 1`, so both mean 1); both sides' "nil" read as "1"
            ("Gem", "count") => Some("1"),
            _ => None,
        };
        if let Some(d) = rust_default.filter(|_| &**v == "nil") {
            notes.gem_nil_bools += 1;
            attrs.push((k.to_string(), d.to_owned()));
        } else if &*node.elem == "Gem"
            && matches!(&**k, "variantId" | "nameSpec")
            && ambiguous_variant(node)
        {
            if k.as_ref() == "variantId" {
                notes.ambiguous_variants += 1;
            }
            attrs.push((k.to_string(), "<pairs-chosen variant>".to_owned()));
        } else {
            attrs.push((k.to_string(), v.to_string()));
        }
    }
    attrs.sort();
    let mut kids: Vec<String> = Vec::new();
    for c in &node.children {
        match c {
            XmlChild::Node(n) => {
                if top && notes.pending_sections.iter().any(|p| **p == *n.elem) {
                    continue;
                }
                kids.push(normalise(n, false, lua, notes));
            }
            XmlChild::Text(t) => kids.push(format!("{t:?}")),
        }
    }
    if top || unordered_children(&node.elem) {
        kids.sort();
    }
    format!("<{} {:?}>[{}]", node.elem, attrs, kids.join(","))
}

fn root_element(text: &str) -> Result<XmlNode, String> {
    let root = pob_calc::model::xml::ParseXML_root(text)?;
    root.children
        .into_iter()
        .find_map(|c| match c {
            XmlChild::Node(n) => Some(n),
            XmlChild::Text(_) => None,
        })
        .ok_or_else(|| "no root element".to_owned())
}

fn first_diff(a: &str, b: &str) -> String {
    let (a, b): (Vec<&str>, Vec<&str>) = (a.split('<').collect(), b.split('<').collect());
    for (i, (x, y)) in a.iter().zip(b.iter()).enumerate() {
        if x != y {
            return format!("element {i}: lua <{x} | rust <{y}");
        }
    }
    format!("lengths {} vs {}", a.len(), b.len())
}

/// Compare one T4 pair; `Ok(byte_identical)`.
fn compare_t4(what: &str, lua: &str, rust: &str, notes: &mut T4Notes) -> Result<bool, String> {
    let lr = root_element(lua).map_err(|e| format!("{what}: Lua XML: {e}"))?;
    let rr = root_element(rust).map_err(|e| format!("{what}: Rust XML: {e}"))?;
    let mut local = T4Notes::default();
    let section = |root: &XmlNode, elem: &str, lua: bool| {
        root.children.iter().find_map(|c| match c {
            XmlChild::Node(n) if *n.elem == *elem => {
                Some(normalise(n, false, lua, &mut T4Notes::default()))
            }
            _ => None,
        })
    };
    for elem in PENDING_SAVERS {
        if section(&lr, elem, true) != section(&rr, elem, false) {
            local.pending_sections.push(elem.to_owned());
        }
    }
    let ln = normalise(&lr, true, true, &mut local);
    let rn = normalise(&rr, true, false, &mut local);
    notes.gem_nil_bools += local.gem_nil_bools;
    notes.ambiguous_variants += local.ambiguous_variants;
    notes.pending_sections.extend(local.pending_sections);
    if ln != rn {
        return Err(format!("{what}: T4 differs: {}", first_diff(&ln, &rn)));
    }
    Ok(lua == rust)
}

// ---- runs ----

fn drop_image_loads(frames: &mut [FrameTrace]) {
    for f in frames {
        f.effects.retain(|e| !matches!(e, SideEffect::ImageLoad(_)));
    }
}

struct Side {
    frames: Vec<FrameTrace>,
    error: Option<String>,
    snapshots: Vec<(String, String)>,
    saved: Vec<(String, Option<String>)>,
}

fn read_saved(user: &UserDir, names: &[String]) -> Vec<(String, Option<String>)> {
    names
        .iter()
        .map(|n| {
            let p = user.path.join("Path of Building").join(n);
            (n.clone(), std::fs::read_to_string(p).ok())
        })
        .collect()
}

/// Run `script` on both sides with `fixture` as the user dir (fresh copies per side). The Lua
/// VM is dropped before the Rust side starts (memory).
/// `#! u35-settings FROM => TO`: a text replacement in the fixture's `Settings.xml` copy (both
/// sides read it at launch), e.g. `showAllItemAffixes="true" => showAllItemAffixes="false"`.
fn patch_settings(user: &UserDir, settings: &[(String, String)]) -> anyhow::Result<()> {
    if settings.is_empty() {
        return Ok(());
    }
    let p = user.path.join("Path of Building").join("Settings.xml");
    let mut text = std::fs::read_to_string(&p)?;
    for (from, to) in settings {
        anyhow::ensure!(text.contains(from.as_str()), "Settings.xml has no {from:?}");
        text = text.replace(from.as_str(), to);
    }
    std::fs::write(&p, text)?;
    Ok(())
}

fn run_sides(
    root: &Path,
    script: &pob_host::script::Script,
    fixture: Option<&Path>,
    screen: pob_sg::ScreenInfo,
    saved: &[String],
    settings: &[(String, String)],
) -> anyhow::Result<(Side, Side)> {
    let user = UserDir::new(fixture)?;
    patch_settings(&user, settings)?;

    // Lua
    let handles = OracleHandles::default();
    let lua_app = runner::lua_side(root, &user, &handles, Entry::Launch, false)?;
    let stubs = LUA_STUBS.to_owned();
    let mut lua_side = LuaSide {
        app: lua_app,
        stubs,
    };
    let mut d = Driver::new(screen, handles, user.path.clone());
    let (mut frames, r) = runner::drive(&mut lua_side, script, &mut d);
    drop(lua_side);
    drop_image_loads(&mut frames);
    let lua = Side {
        frames,
        error: r.err().map(|e| e.to_string()),
        snapshots: std::mem::take(&mut d.snapshots),
        saved: read_saved(&user, saved),
    };

    // Rust
    user.reset()?;
    patch_settings(&user, settings)?;
    let handles = OracleHandles::default();
    let (sg, sys) = boot::oracle_parts(root, &user.path, &handles)?;
    let src = std::fs::canonicalize(root.join("src")).unwrap_or_else(|_| root.join("src"));
    let mut rust_side = RustSide::new(PlatformParts { sg, sys }, src);
    let mut d = Driver::new(screen, handles, user.path.clone());
    let res = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        runner::drive(&mut rust_side, script, &mut d)
    }));
    let (mut frames, error) = match res {
        Ok((frames, r)) => (frames, r.err().map(|e| e.to_string())),
        Err(p) => (
            Vec::new(),
            Some(format!("panicked: {}", pob_host::panic_message(&*p))),
        ),
    };
    drop(rust_side);
    drop_image_loads(&mut frames);
    let rust = Side {
        frames,
        error,
        snapshots: std::mem::take(&mut d.snapshots),
        saved: read_saved(&user, saved),
    };
    Ok((lua, rust))
}

fn out_dir() -> PathBuf {
    std::env::var_os("CARGO_TARGET_DIR")
        .map_or_else(
            || PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../target"),
            PathBuf::from,
        )
        .join("ui-parity")
}

fn draw_ops(frames: &[FrameTrace]) -> usize {
    use pob_sg::trace::ApiCall;
    frames
        .iter()
        .flat_map(|f| &f.calls)
        .filter(|c| {
            matches!(
                c.call,
                ApiCall::DrawImage { .. }
                    | ApiCall::DrawImageQuad { .. }
                    | ApiCall::DrawString { .. }
            )
        })
        .count()
}

fn window_titles(frames: &[FrameTrace]) -> Vec<String> {
    frames
        .iter()
        .flat_map(|f| &f.effects)
        .filter_map(|e| match e {
            SideEffect::WindowTitle(t) => Some(t.clone()),
            _ => None,
        })
        .collect()
}

fn distinct_titles(t: &[String]) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for s in t {
        if out.last() != Some(s) {
            out.push(s.clone());
        }
    }
    out
}

/// Every T4 pair of a run; `Ok(number of pairs, byte-identical pairs)`.
fn t4(lua: &Side, rust: &Side, notes: &mut T4Notes) -> Result<(usize, usize), Vec<String>> {
    let mut errs = Vec::new();
    let (mut n, mut same) = (0, 0);
    if lua.snapshots.len() != rust.snapshots.len() {
        errs.push(format!(
            "snapshots: lua {} rust {}",
            lua.snapshots.len(),
            rust.snapshots.len()
        ));
    }
    for ((ln, lx), (_, rx)) in lua.snapshots.iter().zip(&rust.snapshots) {
        n += 1;
        match compare_t4(&format!("snapshot {ln}"), lx, rx, notes) {
            Ok(b) => same += usize::from(b),
            Err(e) => errs.push(e),
        }
    }
    for ((name, l), (_, r)) in lua.saved.iter().zip(&rust.saved) {
        match (l, r) {
            (Some(l), Some(r)) => {
                n += 1;
                match compare_t4(&format!("saved {name}"), l, r, notes) {
                    Ok(b) => same += usize::from(b),
                    Err(e) => errs.push(e),
                }
            }
            (None, None) => errs.push(format!("saved {name}: written by neither side")),
            (l, r) => errs.push(format!(
                "saved {name}: lua {} rust {}",
                if l.is_some() { "wrote it" } else { "did not" },
                if r.is_some() { "wrote it" } else { "did not" }
            )),
        }
    }
    if errs.is_empty() {
        Ok((n, same))
    } else {
        Err(errs)
    }
}

/// `#! u35-expect TEXT`: some Lua frame draws a string containing TEXT (guards a script whose
/// clicks silently stopped reaching their target, which both sides would agree on).
fn check_expectations(text: &str, lua: &Side) -> Vec<String> {
    use pob_sg::trace::ApiCall;
    let mut errs = Vec::new();
    for want in directives(text, "u35-expect") {
        let found = lua
            .frames
            .iter()
            .flat_map(|f| &f.calls)
            .any(|c| match &c.call {
                ApiCall::DrawString { text, .. } => text.contains(want.as_str()),
                _ => false,
            });
        if !found {
            errs.push(format!("no Lua DrawString contains {want:?}"));
        }
    }
    errs
}

#[test]
fn u35_scenarios() {
    let _g = LOCK
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let Some(root) = inputs() else { return };
    let mut scripts: Vec<PathBuf> = std::fs::read_dir(test_dir())
        .expect("tests/u35")
        .filter_map(Result::ok)
        .map(|e| e.path())
        .filter(|p| p.extension().is_some_and(|x| x == "pobscript"))
        .collect();
    scripts.sort();
    assert!(!scripts.is_empty(), "no tests/u35/*.pobscript");
    let filter = std::env::var("U35_FILTER").ok();
    let only_screen = std::env::var("U35_SCREEN").ok();
    let mut failures = Vec::new();
    let mut ran = 0;
    let (mut t4_pairs, mut t4_bytes) = (0, 0);
    for path in scripts {
        let stem = path.file_stem().unwrap().to_string_lossy().into_owned();
        if filter.as_ref().is_some_and(|f| !stem.contains(f.as_str())) {
            continue;
        }
        let text = std::fs::read_to_string(&path).unwrap();
        let (script, base_dir) = runner::load_script(&path).unwrap();
        let fx = runner::fixtures(&script, &base_dir).unwrap();
        let settings: Vec<(String, String)> = directives(&text, "u35-settings")
            .iter()
            .map(|d| {
                let (a, b) = d.split_once(" => ").expect("u35-settings FROM => TO");
                (a.to_owned(), b.to_owned())
            })
            .collect();
        // a script that sets its own `screen` (the popups are centred, so their coordinates
        // depend on the virtual height) runs once, at that screen
        let own_screen = match script.steps.first() {
            Some(pob_host::script::Step::Screen { w, h, dpi }) => Some((*w, *h, *dpi)),
            _ => None,
        };
        let screens: Vec<(i32, i32, f32)> =
            own_screen.map_or_else(|| SCREENS.to_vec(), |s| vec![s]);
        for (w, h, s) in screens {
            if only_screen.as_ref().is_some_and(|o| *o != w.to_string()) {
                continue;
            }
            let tag = format!("u35_{stem}@{w}x{h}@{s}");
            let (lua, rust) = match run_sides(
                &root,
                &script,
                fx.user_dir.as_deref(),
                runner::screen(w, h, s),
                &[],
                &settings,
            ) {
                Ok(s) => s,
                Err(e) => {
                    failures.push(format!("{tag}: {e:#}"));
                    continue;
                }
            };
            ran += 1;
            let titles = (window_titles(&lua.frames), window_titles(&rust.frames));
            let mut notes = T4Notes::default();
            let t4r = t4(&lua, &rust, &mut notes);
            let expect = check_expectations(&text, &lua);
            let run = RunResult {
                lua: lua.frames,
                rust: rust.frames,
                rust_error: rust.error.clone(),
                lua_snapshots: lua.snapshots.clone(),
                rust_snapshots: rust.snapshots.clone(),
                audit: Vec::new(),
                compared: true,
            };
            let n = diff_report::write_report(
                &out_dir(),
                &tag,
                &run,
                std::env::var_os("U35_PNG").is_some(),
            )
            .unwrap_or(usize::MAX);
            let ops = draw_ops(&run.lua);
            let t4_line = match &t4r {
                Ok((p, b)) => {
                    t4_pairs += p;
                    t4_bytes += b;
                    let mut pending = notes.pending_sections.clone();
                    pending.sort();
                    pending.dedup();
                    format!(
                        "T4 {p} identical ({b} byte-identical; set aside: {} Lua nil gem/group fields, {} pairs-chosen gem variants, pending UI-saver sections differing {:?})",
                        notes.gem_nil_bools, notes.ambiguous_variants, pending
                    )
                }
                Err(e) => format!("T4 FAILED: {}", e.join("; ")),
            };
            println!(
                "{tag}: {} frames, {ops} Lua draw ops, {n} divergences, {t4_line}, titles {:?}{}{}{}",
                run.lua.len(),
                distinct_titles(&titles.0),
                lua.error
                    .as_ref()
                    .map_or_else(String::new, |e| format!(", Lua error: {e}")),
                rust.error
                    .as_ref()
                    .map_or_else(String::new, |e| format!(", Rust error: {e}")),
                if expect.is_empty() {
                    String::new()
                } else {
                    format!(", expectations: {}", expect.join("; "))
                }
            );
            if n != 0
                || rust.error.is_some()
                || lua.error.is_some()
                || run.lua.len() != run.rust.len()
                || ops == 0
                || titles.0 != titles.1
                || t4r.is_err()
                || !expect.is_empty()
            {
                failures.push(format!(
                    "{tag}: {n} divergences, frames lua {} rust {}, {ops} ops, {t4_line}, titles lua {:?} rust {:?}, errors lua {:?} rust {:?}, expectations {expect:?} (see {})",
                    run.lua.len(),
                    run.rust.len(),
                    distinct_titles(&titles.0),
                    distinct_titles(&titles.1),
                    lua.error,
                    rust.error,
                    out_dir().join(&tag).join("report.txt").display()
                ));
            }
        }
    }
    println!("u35: {ran} runs, T4 {t4_pairs} pairs ({t4_bytes} byte-identical)");
    assert!(ran > 0 || filter.is_some(), "no run");
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}
