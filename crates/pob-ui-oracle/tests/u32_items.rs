//! U32 acceptance (ui doc §12 U32 row: item slots, the item list, the item DBs, the item-set
//! lists): the full app (Launch, the build shell, the Items tab's U32 controls) on both sides of
//! the UI oracle, in u22's form (the wave-4 template). Owner: U32.
//!
//! `u32_scenarios`: every `tests/u32/*.pobscript` at 1280x800 @1.0 and 1920x1080 @1.5 (both 1280
//! virtual px wide). Each script loads a fixture build, switches to the Items tab (Ctrl+3) and
//! drives the U32 controls. Per run: zero T3 divergences (draw ops per `(layer, sub)` bucket and
//! side effects, frame by frame), the same window titles, and T4: every `snapshot` step's build
//! XML identical between the sides after u22's normalisation (attributes as sets, the top-level
//! sections and the `Calcs`/`ConfigSet`/`ItemSet`/`Sockets`/`Overrides` children as multisets,
//! the pending UI-saver sections reported, not failed).
//!
//! The Rust side's calc is U15's LuaCalcBridge (as u20-u22), so the ItemDB stat sort's
//! `calcFunc` calls give PoB's numbers.
//!
//! The ItemsTab UI (U33-U38) is not ported, so both sides run the same **stand-in ItemsTab**:
//! * the controls are the real ones on the Lua side (built by ItemsTab.lua's constructor) and
//!   U32's on the Rust side: the slot chain exactly as ItemsTab.lua:144-233 builds it (the
//!   `slotAnchor`, the base slots with their swap/abyssal slots and `shown` overrides, the
//!   `specSelect` position the sockets hang from, the sockets), `itemList`, `sharedItemList`,
//!   `uniqueDB` and `rareDB` re-anchored to fixed positions in the tab (`#! u32-layout a|b`
//!   shows the unique or the rare DB), and a stand-in "Manage sets" button that runs
//!   ItemsTab:OpenItemSetManagePopup (L2609-2620, U36's, ported below) for the set lists;
//! * the drag-target lists of ItemsTab.lua:1188-1202;
//! * `Draw` is the stand-in below (both sides): the tab's rect, `ProcessControlsInput` and
//!   `DrawControls` over those controls only, `main:DrawBackground`, `UpdateSockets`
//!   (calc:BM-A's), and one status line (`display: <SetDisplayItem's item> undo: <count>`);
//! * Lua stand-ins on both sides: `AddItemTooltip` (U38: two lines naming the item, the slot and
//!   `dbMode`), `SetDisplayItem` / `CreateDisplayItemFromRaw` (U35: record the item's name),
//!   the socket viewer's `Draw` (U23: one line with its zoom fields);
//! * Lua real, Rust ported here (faithful copies of U34-U37's Lua, test-only):
//!   `GetEquippedSlotForItem`, `SortItemList`, `DeleteItem`, `AddItemSetTooltip`,
//!   `OpenItemSetManagePopup`; `AddForbiddenJewelCounterpart` returns at its first line for
//!   every item the scripts use (no Forbidden Flame/Flesh); `AddUndoState` is the real
//!   UndoHandler on the Lua side and sets `modFlag` (and counts) on the Rust side.
//! * every other tab's `Draw` draws nothing (as u22), and u22's `utf8.next` stand-in.

use std::cell::{Cell, RefCell};
use std::path::{Path, PathBuf};
use std::rc::Rc;

use pob_app::app::CxApp;
use pob_app::build::init;
use pob_app::build::on_frame::{self as of, BuildTab, TabFlag};
use pob_app::model::{self, Table, Val};
use pob_app::tabs::items::item_db::{DbType, ItemDBControl};
use pob_app::tabs::items::item_list::ItemListControl;
use pob_app::tabs::items::set_list::ItemSetListControl;
use pob_app::tabs::items::shared_list::SharedItemListControl;
use pob_app::tabs::items::shared_set_list::SharedItemSetListControl;
use pob_app::tabs::items::slot::{item_label, set_sel_item_id, ItemSlotControl};
use pob_app::tabs::items::slot_helper::{self as sh, items_hooks};
use pob_calc::model::{XmlChild, XmlNode};
use pob_host::{App, PlatformParts};
use pob_lua::n;
use pob_sg::trace::{FrameTrace, SideEffect};
use pob_sg::{Align, Font, Key, Sg, Sys};
use pob_ui_core::controls::list::ListControl;
use pob_ui_core::controls::ButtonControl;
use pob_ui_core::popups;
use pob_ui_core::prelude::*;
use pob_ui_oracle::boot::{self, Entry, OracleHandles};
use pob_ui_oracle::diff_report;
use pob_ui_oracle::lua_calc::LuaCalcBridge;
use pob_ui_oracle::oracle_cache::{self, LuaRun};
use pob_ui_oracle::runner::{self, Driver, LuaApp, RunResult, ScenarioApp, UserDir};

// TP2: every (script, screen) runs as an independent case on `oracle_cache::par_map` (at most
// `POB_TEST_PAR` LuaJIT PoBs / Rust apps at once in the whole process, which replaces the old
// one-PoB-at-a-time test lock; the bridge still boots its own PoB per operation under the
// machine-wide oracle lock, or reads it from the Lua-side cache), each case on a fresh thread
// (the Rust side's thread-local state and hooks stay per case), and the Lua side of each case
// goes through the Lua-side cache (`lua_run`). Results are collected in case order, so output
// and failures do not depend on the schedule.

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
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/u32")
}

// ---- the stand-in layout (tab-relative; see the module notes) ----

const ITEM_LIST: [f64; 4] = [430.0, 30.0, 360.0, 160.0];
const SHARED_LIST: [f64; 4] = [430.0, 240.0, 340.0, 100.0];
const DB: [f64; 4] = [430.0, 440.0, 360.0, 190.0];
const MANAGE: [f64; 4] = [800.0, 30.0, 120.0, 20.0];

/// The Lua stand-ins (see the module notes); `U32_LAYOUT` is prepended by the test.
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
	local c = it.controls
	local function place(ctl, r)
		ctl:SetAnchor("TOPLEFT", it, "TOPLEFT", r[1], r[2])
		ctl.width = r[3]
		ctl.height = r[4]
	end
	place(c.itemList, { 430, 30, 360, 160 })
	place(c.sharedItemList, { 430, 240, 340, 100 })
	place(c.uniqueDB, { 430, 440, 360, 190 })
	place(c.rareDB, { 430, 440, 360, 190 })
	c.uniqueDB.shown = function() return U32_LAYOUT == "a" end
	c.rareDB.shown = function() return U32_LAYOUT == "b" end
	local manage = new("ButtonControl"):ButtonControl({"TOPLEFT", it, "TOPLEFT"}, { 800, 30, 120, 20 }, "Manage sets", function()
		it:OpenItemSetManagePopup()
	end)
	local filtered = { itemList = c.itemList, uniqueDB = c.uniqueDB, rareDB = c.rareDB, sharedItemList = c.sharedItemList, u32Manage = manage }
	for i, slot in ipairs(c) do
		filtered[i] = slot
	end
	it.u32Controls = filtered
	it.u32Undo = 0
	local origUndo = it.AddUndoState
	it.AddUndoState = function(s, ...)
		s.u32Undo = s.u32Undo + 1
		return origUndo(s, ...)
	end
	it.AddItemTooltip = function(s, tooltip, item, slot, dbMode)
		tooltip:AddLine(16, colorCodes[item.rarity] .. item.name)
		tooltip:AddLine(14, "^7slot " .. tostring(slot and slot.slotName) .. " db " .. tostring(dbMode))
	end
	it.SetDisplayItem = function(s, item)
		s.u32Display = item and item.name
	end
	it.CreateDisplayItemFromRaw = function(s, raw, normalise)
		local item = new("Item"):Item(raw)
		s.u32Display = "raw " .. tostring(item.name) .. " " .. tostring(normalise)
	end
	it.socketViewer.Draw = function(viewer, build, viewPort, inputEvents)
		DrawString(4, 4, "LEFT", 14, "VAR", "^7viewer " .. viewer.zoom .. " " .. viewer.zoomX .. " " .. viewer.zoomY)
	end
	it.Draw = function(s, viewPort, inputEvents)
		s.x = viewPort.x
		s.y = viewPort.y
		s.width = viewPort.width
		s.height = viewPort.height
		s.maxY = viewPort.y + viewPort.height
		local saved = s.controls
		s.controls = s.u32Controls
		s:ProcessControlsInput(inputEvents, viewPort)
		main:DrawBackground(viewPort)
		s:UpdateSockets()
		s:DrawControls(viewPort)
		s.controls = saved
		DrawString(viewPort.x + 4, viewPort.y + viewPort.height - 20, "LEFT", 16, "VAR", "^7display: " .. tostring(s.u32Display) .. " undo: " .. s.u32Undo)
	end
end
"#;

thread_local! {
    /// The stand-in `spec.modFlag` (UndoHandler.lua:37 via `spec:AddUndoState`, U25).
    static SPEC_MOD_FLAG: Cell<bool> = const { Cell::new(false) };
    /// `#! u32-layout`
    static LAYOUT: RefCell<String> = RefCell::new("a".to_owned());
}

/// The stand-in ItemsTab node (`self.itemsTab`): `modFlag`, the recorded display item and
/// the undo count.
#[derive(Default)]
struct U32ItemsTab {
    mod_flag: bool,
    display: Option<String>,
    undo: u32,
}

impl Widget for U32ItemsTab {
    const NAME: &'static str = "ItemsTab";
    const CAPS: Caps = Caps::empty();
}

fn tab(cx: &Cx) -> CtrlId {
    sh::items_tab(cx).expect("the stand-in ItemsTab")
}

fn layout_is(l: &str) -> bool {
    LAYOUT.with(|x| *x.borrow() == l)
}

/// `self.activeItemSet.useSecondWeaponSet`
fn use_second(cx: &Cx) -> bool {
    cx.build()
        .model
        .itemsTab
        .activeItemSet
        .get()
        .expect("activeItemSet")
        .truthy(n!("useSecondWeaponSet"))
}

/// ItemsTab.lua:144-233 (the slot chain), the stand-in list positions, the manage button and
/// the drag targets of L1188-1202.
fn ctor(cx: &mut Cx, build: CtrlId) -> CtrlId {
    let me = cx
        .add_free(
            Some(build),
            New::new(
                Base::new(Anchor::NONE, rect(0, 0, 0, 0)),
                U32ItemsTab::default(),
            )
            .host(),
        )
        .id();
    let m = cx.build().model.clone();
    // L144
    let slot_anchor = cx
        .add_free(
            Some(me),
            PlainControl::new(Anchor::new(TopLeft, me, TopLeft), rect(96, 76, 310, 0)),
        )
        .id();
    let mut prev = slot_anchor;
    let mut n_slots = 0i64;
    let ordered = m.itemsTab.orderedSlots.snapshot();
    let mut slot_ids: Vec<CtrlId> = Vec::new();
    for slot in ordered {
        // the first socket hangs from specSelect (L204, L231)
        if !slot.get(n!("nodeId")).is_nil() && !slot_ids.is_empty() {
            let first_socket = slot_ids
                .iter()
                .all(|id| cx.w::<ItemSlotControl>(*id).slot.get(n!("nodeId")).is_nil());
            if first_socket {
                // L204 specSelect {0, 8, 216, 20}
                prev = cx
                    .add_free(
                        Some(me),
                        PlainControl::new(
                            Anchor::new(TopLeft, prev, BottomLeft),
                            rect(0, 8, 216, 20),
                        ),
                    )
                    .id();
            }
        }
        let kind = slot.str(n!("$shown")).map(|s| s.to_string());
        let id = cx
            .add(
                me,
                CtrlKey::Idx(n_slots + 1),
                ItemSlotControl::new(
                    Anchor::new(TopLeft, prev, BottomLeft),
                    Prop::Val(0.0),
                    Prop::Val(2.0),
                    me,
                    slot.clone(),
                ),
            )
            .id();
        n_slots += 1;
        // the `shown` overrides of L159-197
        let s = slot.clone();
        let shown: Option<Prop<bool>> = match kind.as_deref() {
            Some("weapon") => Some(Prop::f(|cx, _| !use_second(cx))),
            Some("weaponSwap") => Some(Prop::f(|cx, _| use_second(cx))),
            Some("abyssalSwap") => Some(Prop::f(move |cx, _| {
                !s.truthy(n!("inactive")) && use_second(cx)
            })),
            Some("abyssalWeapon") => Some(Prop::f(move |cx, _| {
                !s.truthy(n!("inactive")) && !use_second(cx)
            })),
            Some("graft") => Some(Prop::f(|cx, _| {
                cx.build()
                    .model
                    .spec
                    .get()
                    .treeVersion
                    .get()
                    .contains("3_27")
            })),
            Some("ring3") => Some(Prop::f(|cx, _| {
                let env = cx.build().model.calcsTab.mainEnv.get().expect("mainEnv");
                env.modDB
                    .get()
                    .Flag(None, pob_lua::nm!["AdditionalRingSlot"])
            })),
            _ => None,
        };
        if let Some(p) = shown {
            cx.base_mut(id).shown = p;
        }
        prev = id;
        slot_ids.push(id);
    }
    let place = |r: [f64; 4]| rect(r[0], r[1], r[2], r[3]);
    let item_list = cx
        .add(
            me,
            "itemList",
            ItemListControl::new(
                Anchor::new(TopLeft, me, TopLeft),
                place(ITEM_LIST),
                me,
                m.itemsTab.orderList.clone(),
                true,
            ),
        )
        .id();
    let unique_db = cx
        .add(
            me,
            "uniqueDB",
            ItemDBControl::new(
                Anchor::new(TopLeft, me, TopLeft),
                place(DB),
                me,
                DbType::Unique,
            )
            .shown(Prop::f(|_, _| layout_is("a"))),
        )
        .id();
    let rare_db = cx
        .add(
            me,
            "rareDB",
            ItemDBControl::new(
                Anchor::new(TopLeft, me, TopLeft),
                place(DB),
                me,
                DbType::Rare,
            )
            .shown(Prop::f(|_, _| layout_is("b"))),
        )
        .id();
    let shared = cx
        .add(
            me,
            "sharedItemList",
            SharedItemListControl::new(
                Anchor::new(TopLeft, me, TopLeft),
                place(SHARED_LIST),
                me,
                true,
            ),
        )
        .id();
    cx.add(
        me,
        "u32Manage",
        ButtonControl::new(
            Anchor::new(TopLeft, me, TopLeft),
            place(MANAGE),
            "Manage sets",
            |cx| {
                open_item_set_manage_popup(cx);
            },
        ),
    );
    // L1188-1202
    let minion = cx.child(build, "mainSkillMinion").expect("mainSkillMinion");
    let push = |cx: &mut Cx, list: CtrlId, t: CtrlId| {
        cx.w_mut::<ListControl>(list)
            .drag_target_list
            .get_or_insert_with(Vec::new)
            .push(t);
    };
    push(cx, item_list, shared);
    push(cx, item_list, minion);
    push(cx, unique_db, item_list);
    push(cx, unique_db, shared);
    push(cx, unique_db, minion);
    push(cx, rare_db, item_list);
    push(cx, rare_db, shared);
    push(cx, rare_db, minion);
    push(cx, shared, item_list);
    push(cx, shared, minion);
    for id in &slot_ids {
        push(cx, item_list, *id);
        push(cx, unique_db, *id);
        push(cx, rare_db, *id);
        push(cx, shared, *id);
    }
    // L1211 PopulateSlots (the model half ran in calc:BM-A's ctor_model and Load)
    sh::populate_slot_lists_of(cx, me);
    me
}

/// The stand-in Draw (see the module notes).
fn draw_items(cx: &mut Cx, vp: &ViewPort, ev: &mut InputEvents) {
    let me = tab(cx);
    {
        let b = cx.base_mut(me);
        b.x = Prop::Val(vp.x);
        b.y = Prop::Val(vp.y);
        b.width = Prop::Val(vp.width);
        b.height = Prop::Val(vp.height);
    }
    cx.process_controls_input(me, ev, vp);
    pob_app::main_mode::background::draw_background(cx, vp);
    let m = cx.build().model.clone();
    m.itemsTab.UpdateSockets(&m);
    cx.draw_controls(me, vp, None);
    let w = cx.w::<U32ItemsTab>(me);
    let line = format!(
        "^7display: {} undo: {}",
        w.display.as_deref().unwrap_or("nil"),
        w.undo
    );
    cx.sg.draw_string(
        vp.x + 4.0,
        vp.y + vp.height - 20.0,
        Align::Left,
        16.0,
        Font::Var,
        &line,
    );
}

/// ItemsTab.lua:2528-2542 `GetEquippedSlotForItem(item)` (U35's; test port).
fn get_equipped_slot_for_item(cx: &Cx, item: &model::ItemRef) -> (Option<Table>, Option<Table>) {
    let m = &cx.build().model;
    let id = item.id.get().map_or(Val::Nil, Val::Num);
    for slot in m.itemsTab.orderedSlots.snapshot() {
        if !slot.truthy(n!("inactive")) {
            if slot.get(n!("selItemId")).raw_eq(&id) {
                return (Some(slot), None);
            }
            let slot_name = slot.str(n!("slotName")).expect("slotName");
            for set_id in m.itemsTab.itemSetOrderList.snapshot() {
                let set = m
                    .itemsTab
                    .itemSets
                    .with(|s| s.get(&set_id).cloned())
                    .expect("itemSet");
                if f64::from(set_id) != m.itemsTab.activeItemSetId.get() {
                    if let Some(e) = set.table(&*slot_name) {
                        if e.get(n!("selItemId")).raw_eq(&id) {
                            return (Some(slot), Some(set));
                        }
                    }
                }
            }
        }
    }
    (None, None)
}

/// ItemsTab.lua:1820-1855 `SortItemList()` (U34's; test port).
fn sort_item_list(cx: &mut Cx) {
    let m = cx.build().model.clone();
    let ordered = m.itemsTab.orderedSlots.snapshot();
    let slot_order = |name: &str| -> Option<usize> {
        ordered
            .iter()
            .position(|s| s.str(n!("slotName")).as_deref() == Some(name))
            .map(|i| i + 1)
    };
    let set_pos = |id: f64| -> usize {
        m.itemsTab
            .itemSetOrderList
            .snapshot()
            .iter()
            .position(|x| f64::from(*x) == id)
            .map(|i| i + 1)
            .expect("isValueInArray")
    };
    let mut list = m.itemsTab.orderList.snapshot();
    pob_lua::sort(&mut list, |a, b| {
        let item_a = m.itemsTab.items.with(|i| i.get(a).cloned()).expect("item");
        let item_b = m.itemsTab.items.with(|i| i.get(b).cloned()).expect("item");
        let prim_a = item_a.GetPrimarySlot();
        let prim_b = item_b.GetPrimarySlot();
        if prim_a != prim_b {
            let (Some(oa), ob) = (slot_order(&prim_a), slot_order(&prim_b)) else {
                return false;
            };
            let Some(ob) = ob else { return true };
            return oa < ob;
        }
        let (sa, seta) = get_equipped_slot_for_item(cx, &item_a);
        let (sb, setb) = get_equipped_slot_for_item(cx, &item_b);
        match (&sa, &sb) {
            (Some(sa), Some(sb)) => {
                if !sa.ptr_eq(sb) {
                    let na = slot_order(&sa.str(n!("slotName")).expect("slotName")).expect("order");
                    let nb = slot_order(&sb.str(n!("slotName")).expect("slotName")).expect("order");
                    return na < nb;
                } else if seta.is_some() && setb.is_none() {
                    return false;
                } else if seta.is_none() && setb.is_some() {
                    return true;
                } else if let (Some(x), Some(y)) = (&seta, &setb) {
                    return set_pos(x.num(n!("id"))) < set_pos(y.num(n!("id")));
                }
            }
            (Some(_), None) => return true,
            (None, Some(_)) => return false,
            (None, None) => {}
        }
        item_a.name.get().as_bytes() < item_b.name.get().as_bytes()
    });
    m.itemsTab.orderList.with_mut(|l| *l = list);
    let f = items_hooks().add_undo_state;
    f(cx);
}

/// ItemsTab.lua:1857-1904 `DeleteItem(item, deferUndoState)` (U34's; test port).
fn delete_item(cx: &mut Cx, item: &model::ItemRef, defer: bool) {
    let m = cx.build().model.clone();
    let id = item.id.get().expect("item.id");
    let slots: Vec<(model::Name, Table)> = m
        .itemsTab
        .slots
        .with(|s| s.iter().map(|(k, v)| (*k, v.clone())).collect());
    for (slot_name, slot) in slots {
        if slot.get(n!("selItemId")).raw_eq(&Val::Num(id)) {
            set_sel_item_id(&m, &slot, 0.0);
            m.buildFlag.set(true);
        }
        if slot.get(n!("nodeId")).is_nil() {
            let sets: Vec<Table> = m.itemsTab.itemSets.with(|s| s.values().cloned().collect());
            for set in sets {
                let e = set.table(slot_name).expect("itemSet[slotName]");
                if e.get(n!("selItemId")).raw_eq(&Val::Num(id)) {
                    e.set(n!("selItemId"), 0.0);
                    m.buildFlag.set(true);
                }
            }
        }
    }
    let k = id as u32;
    m.itemsTab.orderList.with_mut(|l| {
        if let Some(i) = l.iter().position(|x| *x == k) {
            l.remove(i);
        }
    });
    for spec in m.treeTab.specList.snapshot() {
        let mut rebuild = false;
        for (node_id, item_id) in spec.jewels.with(|j| j.pairs()) {
            if item_id == id {
                spec.jewels.with_mut(|j| j.set(node_id, Some(0.0)));
                rebuild = true;
                // Deallocate all nodes that required this jewel (`ipairs` index: the Lua clears
                // allocNodes[1..#depends], as PoB does)
                if let Some(node) = spec.nodes.with(|n| n.get(node_id)) {
                    for (dep_i, dep) in node.depends.get().snapshot().iter().enumerate() {
                        dep.alloc.set(false);
                        spec.allocNodes.with_mut(|a| a.set(dep_i as i64 + 1, None));
                    }
                    node.alloc.set(false);
                    spec.allocNodes.with_mut(|a| a.set(node_id, None));
                }
            }
        }
        if rebuild && !defer {
            spec.BuildClusterJewelGraphs();
        }
    }
    m.itemsTab.items.with_mut(|i| i.shift_remove(&k));
    if !defer {
        let h = items_hooks();
        (h.populate_slots)(cx);
        (h.add_undo_state)(cx);
    }
}

/// ItemsTab.lua:3982-3991 `AddItemSetTooltip(tooltip, itemSet)` (U37's; test port).
fn add_item_set_tooltip(cx: &mut Cx, tooltip: &mut pob_ui_core::tooltip::Tooltip, set: &Table) {
    let m = cx.build().model.clone();
    for slot in m.itemsTab.orderedSlots.snapshot() {
        if slot.get(n!("nodeId")).is_nil() {
            let slot_name = slot.str(n!("slotName")).expect("slotName");
            let sel = set
                .table(&*slot_name)
                .expect("itemSet[slotName]")
                .get(n!("selItemId"));
            if let Some(item) = pob_app::tabs::items::slot::item_by_id(cx, &sel) {
                let label = slot.str(n!("label")).expect("label");
                let line = format!("^7{label}: {}", item_label(cx, &item));
                tooltip.add_line(cx, 16.0, &line);
            }
        }
    }
}

/// ItemsTab.lua:2609-2620 `OpenItemSetManagePopup()` (U36's; test port).
fn open_item_set_manage_popup(cx: &mut Cx) {
    let me = tab(cx);
    let order = cx.build().model.itemsTab.itemSetOrderList.clone();
    popups::open_popup(
        cx,
        630.0,
        290.0,
        "Manage Item Sets",
        move |cx, p| {
            let p = p.id();
            let set_list = cx
                .add(
                    p,
                    "setList",
                    ItemSetListControl::new(Anchor::NONE, rect(-155, 50, 300, 200), me, order),
                )
                .id();
            let shared = cx
                .add(
                    p,
                    "sharedList",
                    SharedItemSetListControl::new(Anchor::NONE, rect(155, 50, 300, 200), me),
                )
                .id();
            cx.w_mut::<ListControl>(set_list).drag_target_list = Some(vec![shared]);
            cx.w_mut::<ListControl>(shared).drag_target_list = Some(vec![set_list]);
            cx.add(
                p,
                "close",
                ButtonControl::new(Anchor::NONE, rect(0, 260, 90, 20), "Done", |cx| {
                    popups::close_popup(cx);
                }),
            );
        },
        popups::PopupOpts::default(),
    );
}

fn rust_stubs() {
    SPEC_MOD_FLAG.with(|f| f.set(false));
    init::set_hooks(|h| {
        *h = init::default_hooks();
        h.spec_add_undo_state = Rc::new(|_| SPEC_MOD_FLAG.with(|f| f.set(true)));
        // U25 stand-in: TreeTab.lua:492-498 (the buildLoadouts selFunc, reached from
        // SyncLoadouts' SetSel), as in u20/u22
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
        h.items_tab = Rc::new(ctor);
        h.items_add_item_set_tooltip = Rc::new(add_item_set_tooltip);
    });
    sh::set_items_hooks(|h| {
        *h = sh::default_items_hooks();
        h.add_undo_state = Rc::new(|cx| {
            let me = tab(cx);
            let w = cx.w_mut::<U32ItemsTab>(me);
            w.undo += 1;
            w.mod_flag = true;
        });
        h.set_mod_flag = Rc::new(|cx, v| {
            let me = tab(cx);
            cx.w_mut::<U32ItemsTab>(me).mod_flag = v;
        });
        h.add_item_tooltip = Rc::new(|cx, tooltip, item, slot, db| {
            let l1 = item_label(cx, item);
            tooltip.add_line(cx, 16.0, &l1);
            let slot_name = slot
                .and_then(|s| s.str(n!("slotName")))
                .map_or_else(|| "nil".to_owned(), |s| s.to_string());
            let l2 = format!("^7slot {slot_name} db {}", if db { "true" } else { "nil" });
            tooltip.add_line(cx, 14.0, &l2);
        });
        h.set_display_item = Rc::new(|cx, item| {
            let me = tab(cx);
            cx.w_mut::<U32ItemsTab>(me).display = item.map(|i| i.name.get().to_string());
        });
        h.create_display_item_from_raw = Rc::new(|cx, raw, normalise| {
            let item = pob_app::main_mode::new_item(Some(raw), None, false);
            let me = tab(cx);
            cx.w_mut::<U32ItemsTab>(me).display =
                Some(format!("raw {} {normalise}", item.name.get()));
        });
        h.get_equipped_slot_for_item = Rc::new(get_equipped_slot_for_item);
        h.sort_item_list = Rc::new(sort_item_list);
        h.delete_item = Rc::new(delete_item);
        h.add_item_set_tooltip = Rc::new(add_item_set_tooltip);
        h.add_forbidden_jewel_counterpart = Rc::new(|_, item| {
            let title = item.title.get();
            assert!(
                !matches!(
                    title.as_deref(),
                    Some("Forbidden Flame" | "Forbidden Flesh")
                ),
                "U32 stand-in AddForbiddenJewelCounterpart: Forbidden jewels are not covered"
            );
            None
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
    of::set_shell_hooks(|h| {
        *h = of::default_shell_hooks();
        // LUA_STUBS replaces `treeTab.Draw`: the Tree tab (U25) draws nothing here as well
        let all_tabs = h.draw_tab.clone();
        h.draw_tab = Rc::new(move |cx, tab, vp, ev| {
            if tab != of::BuildTab::Tree {
                all_tabs(cx, tab, vp, ev);
            }
        });
        let (draw, flag, set_flag) = (
            h.draw_tab.clone(),
            h.tab_flag.clone(),
            h.set_tab_flag.clone(),
        );
        h.draw_tab = Rc::new(move |cx, t, vp, ev| {
            if t == BuildTab::Items {
                draw_items(cx, vp, ev);
            } else {
                draw(cx, t, vp, ev);
            }
        });
        h.tab_flag = Rc::new(move |cx, f| match f {
            TabFlag::Spec => SPEC_MOD_FLAG.with(Cell::get),
            TabFlag::Items => sh::items_tab(cx).is_some_and(|me| cx.w::<U32ItemsTab>(me).mod_flag),
            _ => flag(cx, f),
        });
        h.set_tab_flag = Rc::new(move |cx, f, v| match f {
            TabFlag::Spec => SPEC_MOD_FLAG.with(|c| c.set(v)),
            TabFlag::Items => {
                if let Some(me) = sh::items_tab(cx) {
                    cx.w_mut::<U32ItemsTab>(me).mod_flag = v;
                }
            }
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

/// The Lua side of one run through the Lua-side cache (TP2): boot, `script`, the snapshots and
/// the `saved` files of the user dir afterwards (`extra[0]`, JSON), frames without image loads.
/// The key covers this file (stubs and Lua-side wrapper), the Lua-side code, PoB, the fixture user
/// dir, this test's scripts and fixtures, the script, the screen, the layout and `saved`.
fn lua_run(
    root: &Path,
    script: &pob_host::script::Script,
    user: &UserDir,
    fixture: Option<&Path>,
    screen: pob_sg::ScreenInfo,
    saved: &[String],
) -> anyhow::Result<LuaRun> {
    let stubs = format!(
        "U32_LAYOUT = \"{}\"\n{LUA_STUBS}",
        LAYOUT.with(|l| l.borrow().clone())
    );
    let scripts = test_dir();
    let mut inputs: Vec<&Path> = vec![&scripts];
    inputs.extend(fixture);
    let key = oracle_cache::ui_key(
        "ui-u32",
        file!(),
        root,
        &inputs,
        &[
            &format!("{script:?}"),
            &format!("{screen:?}"),
            &stubs,
            &saved.join("\n"),
        ],
    );
    oracle_cache::cached_lua_run(key, || {
        let handles = OracleHandles::default();
        let lua_app = runner::lua_side(root, user, &handles, Entry::Launch, false)?;
        let mut lua_side = LuaSide {
            app: lua_app,
            stubs: stubs.clone(),
        };
        let mut d = Driver::new(screen, handles, user.path.clone());
        let (mut frames, r) = runner::drive(&mut lua_side, script, &mut d);
        drop(lua_side);
        drop_image_loads(&mut frames);
        Ok(LuaRun {
            frames,
            error: r.err().map(|e| e.to_string()),
            snapshots: std::mem::take(&mut d.snapshots),
            extra: vec![serde_json::to_string(&read_saved(user, saved))?],
        })
    })
}

/// Run `script` on both sides with `fixture` as the user dir (fresh copies per side). The Lua
/// VM is dropped before the Rust side starts (memory).
fn run_sides(
    root: &Path,
    script: &pob_host::script::Script,
    fixture: Option<&Path>,
    screen: pob_sg::ScreenInfo,
    saved: &[String],
) -> anyhow::Result<(Side, Side)> {
    let user = UserDir::new(fixture)?;

    // Lua (cached)
    let run = lua_run(root, script, &user, fixture, screen, saved)?;
    let lua = Side {
        frames: run.frames,
        error: run.error,
        snapshots: run.snapshots,
        saved: serde_json::from_str(run.extra.first().map_or("[]", String::as_str))?,
    };

    // Rust
    user.reset()?;
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

/// `#! u32-expect TEXT`: some Lua frame draws a string containing TEXT (guards a script whose
/// clicks silently stopped reaching their target, which both sides would agree on).
fn check_expectations(text: &str, lua: &Side) -> Vec<String> {
    use pob_sg::trace::ApiCall;
    let mut errs = Vec::new();
    for want in directives(text, "u32-expect") {
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

/// One case's outcome (collected in case order).
#[derive(Default)]
struct CaseOut {
    line: String,
    failure: Option<String>,
    ran: usize,
    t4_pairs: usize,
    t4_bytes: usize,
}

#[test]
fn u32_scenarios() {
    let Some(root) = inputs() else { return };
    let mut scripts: Vec<PathBuf> = std::fs::read_dir(test_dir())
        .expect("tests/u32")
        .filter_map(Result::ok)
        .map(|e| e.path())
        .filter(|p| p.extension().is_some_and(|x| x == "pobscript"))
        .collect();
    scripts.sort();
    assert!(!scripts.is_empty(), "no tests/u32/*.pobscript");
    let filter = std::env::var("U32_FILTER").ok();
    let only_screen = std::env::var("U32_SCREEN").ok();
    let mut cases = Vec::new();
    for path in scripts {
        let stem = path.file_stem().unwrap().to_string_lossy().into_owned();
        if filter.as_ref().is_some_and(|f| !stem.contains(f.as_str())) {
            continue;
        }
        let (script, _) = runner::load_script(&path).unwrap();
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
            cases.push((path.clone(), stem.clone(), (w, h, s)));
        }
    }
    let results = oracle_cache::par_map(cases, |(path, stem, (w, h, s))| {
        let mut out = CaseOut::default();
        let text = std::fs::read_to_string(&path).unwrap();
        let (script, base_dir) = runner::load_script(&path).unwrap();
        let fx = runner::fixtures(&script, &base_dir).unwrap();
        let layout = directives(&text, "u32-layout")
            .into_iter()
            .next()
            .unwrap_or_else(|| "a".to_owned());
        LAYOUT.with(|l| *l.borrow_mut() = layout);
        {
            let tag = format!("u32_{stem}@{w}x{h}@{s}");
            let (lua, rust) = match run_sides(
                &root,
                &script,
                fx.user_dir.as_deref(),
                runner::screen(w, h, s),
                &[],
            ) {
                Ok(s) => s,
                Err(e) => {
                    out.failure = Some(format!("{tag}: {e:#}"));
                    return out;
                }
            };
            out.ran = 1;
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
            let n = diff_report::write_report(&out_dir(), &tag, &run, false).unwrap_or(usize::MAX);
            let ops = draw_ops(&run.lua);
            let t4_line = match &t4r {
                Ok((p, b)) => {
                    out.t4_pairs += p;
                    out.t4_bytes += b;
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
            out.line = format!(
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
                out.failure = Some(format!(
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
        out
    });
    let mut failures = Vec::new();
    let (mut ran, mut t4_pairs, mut t4_bytes) = (0, 0, 0);
    for case in results {
        if !case.line.is_empty() {
            println!("{}", case.line);
        }
        failures.extend(case.failure);
        ran += case.ran;
        t4_pairs += case.t4_pairs;
        t4_bytes += case.t4_bytes;
    }
    println!("u32: {ran} runs, T4 {t4_pairs} pairs ({t4_bytes} byte-identical)");
    println!("{}", oracle_cache::summary());
    assert!(ran > 0 || filter.is_some(), "no run");
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}
