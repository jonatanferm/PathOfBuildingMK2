//! Port of PathOfBuilding v2.67.2 (16de4b82d) src/Classes/TimelessJewelListControl.lua 1-352
//! (`newClass("TimelessJewelListControl", "ListControl")`). Owner: U28.
//!
//! @pob-file Classes/TimelessJewelListControl.lua @16de4b82d
//!
//! The result list of the timeless jewel search popup (TreeTab.lua:2217, U27). Its rows are the
//! tables of `build.timelessData.searchResults`, which the control aliases (`self.list`, L19):
//! [`TableSource`] reads and writes that `Table` live, so TreeTab's in-place refills show up
//! without a new list.
//!
//! ListControl hooks: `GetRowValue`, `SetHighlightColor`, `OverrideSelectIndex`,
//! `AddValueTooltip`, `OnSelClick`. Own methods: `Draw` (stores `noTooltip`, then the super Draw
//! with `noTooltip` nil), `ScrollToIndex` and `GetJewelItem` (TreeTab calls both).
//!
//! # The `build` argument (CCR-0112)
//!
//! Lua passes the build object (`self.build = build`, L17) and reads `build.timelessData`,
//! `build.outputRevision`, `build.spec`, `build.buildFlag` and five `build.itemsTab` methods
//! (two of them UI methods of units that are not ported yet: AddItemStatDifferences is U38's,
//! AddUndoState U34's, the socket's SetSelItemId U32's). The Rust control receives that object as
//! an `Rc<dyn` [`TimelessBuild`]`>` whose methods are those reads and calls, one per Lua access.
//! [`AppTimelessBuild`] is the product's: model reads and the model halves of the ItemsTab calls
//! (calc:BM-A), placeholders for the UI-only ones (see each method). Galleries pass stand-ins,
//! which is why `new("Item"):Item(raw)` (L285) is a method too.
//!
//! # Notes
//!
//! - ORDER: L312 `pairs(self.sharedList.desiredNodes or {})` runs over string keys (legion node
//!   ids and `"totalStat"`), so its order is per-process random in PoB; Rust iterates the
//!   `Table`'s insertion order (TreeTab's order of creation). The loop only fills
//!   `sortedNodeLists[desiredIdx]`, so the order is observable only through the layout of that
//!   integer-keyed table, which L323 walks with `pairs`: [`LjIntMap`] reproduces LuaJIT's layout
//!   for the Rust insertion sequence. A contiguous `1..n` set of indices always ends in the array
//!   part (any insertion order), so the lines come out in index order on both sides; holes in
//!   `desiredIdx` can differ (resolution (b): such tooltips are per-process random in PoB).
//! - L133-134 `m_random(2, 17)`: LuaJIT's `math.random` (`lib_math.c`, TW223) is ported here as
//!   [`math_random`] until pob-lua has it (CCR-0033 §3, A8 needs-code item 10). Its state starts
//!   as after `math.randomseed(0)`, which is what the UI oracle's determinism patch 2 sets.
//!   DIVERGENCE(prng-seed): an unpatched PoB on LuaJIT ≥ 2.1.0-beta3+prng starts from
//!   `lj_prng_seed_fixed` instead, so the variant it picks first can differ.
//! - Props with side effects (ui doc §7.3): none.
#![allow(non_snake_case)]

use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;

use pob_lua::format::tostring_num;
use pob_lua::math::{lmod, m_max, m_min};
use pob_ui_core::controls::list::{ListControl, ListData, ListHooks, ListSource, ListValue, ValKey};
use pob_ui_core::controls::scroll_bar::ScrollBarControl;
use pob_ui_core::prelude::*;
use pob_ui_core::tooltip::{TtKey, TtObj, Tooltip};

use crate::app::CxApp;
use crate::model::{self, ItemRef, LjIntMap, NodeRef, Table, Val};
// `x or y` / `x and y` on values (pob_calc::lua_table; model.rs does not re-export them)
use pob_calc::lua_table::{land, lor};

// @pob Classes/TimelessJewelListControl.lua:1-14 h=0fa31a13 (header, locals and newClass: the struct and Widget impl below)

// ---- the `build` argument (Rust plumbing, CCR-0112) ------------------------------------------

/// `jewel.base` (`Item.base`).
pub type ItemBaseRef = model::gamedata::ItemBaseRef;

/// The build object the timeless controls receive (`self.build`): one method per Lua access.
/// Node ids are the Lua values (`data.socketId` may be nil or `-1`).
pub trait TimelessBuild {
    /// `build.timelessData`
    fn timeless_data(&self, cx: &Cx) -> Table;
    /// `build.outputRevision`
    fn output_revision(&self, cx: &Cx) -> f64;
    /// `build.spec.allocNodes[nodeId]` as a condition
    fn spec_alloc_node(&self, cx: &Cx, node_id: &Val) -> bool;
    /// `build.spec.nodes[nodeId]` (TimelessJewelSocketControl)
    fn spec_node(&self, cx: &Cx, node_id: &Val) -> Option<NodeRef>;
    /// `build.spec.tree.size` (TimelessJewelSocketControl; an arithmetic operand)
    fn spec_tree_size(&self, cx: &Cx) -> Val;
    /// `build.buildFlag = true`
    fn set_build_flag(&self, cx: &mut Cx);
    /// `new("Item"):Item(raw)` (Classes/Item.lua; `raw` nil when no jewel type matched)
    fn new_item(&self, cx: &Cx, raw: Option<&str>) -> ItemRef;
    /// `build.itemsTab:GetSocketAndJewelForNodeID(nodeId)` (first result: the socket slot)
    fn get_socket_and_jewel_for_node_id(&self, cx: &Cx, node_id: &Val) -> Option<Table>;
    /// `build.itemsTab.sockets[nodeId]`
    fn sockets(&self, cx: &Cx, node_id: &Val) -> Option<Table>;
    /// `build.itemsTab:AddItemStatDifferences(tooltip, item, base, slot)` (`slot` is the Lua
    /// value: nil, `false` for socket -1, or the socket slot)
    fn add_item_stat_differences(
        &self,
        cx: &mut Cx,
        tooltip: &mut Tooltip,
        item: &ItemRef,
        base: Option<ItemBaseRef>,
        slot: &Val,
    );
    /// `build.itemsTab:AddItem(item, noAutoEquip)`
    fn add_item(&self, cx: &mut Cx, item: &ItemRef, no_auto_equip: bool);
    /// `build.itemsTab:IsItemValidForSlot(item, slotName)`
    fn is_item_valid_for_slot(&self, cx: &Cx, item: &ItemRef, slot_name: &Val) -> bool;
    /// `socketControl:SetSelItemId(selItemId)` (ItemSlotControl.lua:72)
    fn set_sel_item_id(&self, cx: &mut Cx, socket: &Table, sel_item_id: Option<f64>);
    /// `build.itemsTab:PopulateSlots()`
    fn populate_slots(&self, cx: &mut Cx);
    /// `build.itemsTab:AddUndoState()`
    fn add_undo_state(&self, cx: &mut Cx);
}

/// `t[k]` for a Lua number key that may be nil or non-integral (`t[nil]` reads nil).
fn int_key(v: &Val) -> Option<i64> {
    v.opt_num()
        .filter(|n| n.fract() == 0.0 && n.abs() < 9.0e15)
        .map(|n| n as i64)
}

#[track_caller]
fn not_ported(what: &str, owner: &str) -> ! {
    lua_error!("{what} is not ported yet ({owner})")
}

/// The product's build: `cx.build()` (the open build; ui doc §10.2).
pub struct AppTimelessBuild;

impl TimelessBuild for AppTimelessBuild {
    fn timeless_data(&self, cx: &Cx) -> Table {
        model::build_xml::timelessData(&cx.build().model)
    }
    fn output_revision(&self, cx: &Cx) -> f64 {
        cx.build().model.outputRevision.get()
    }
    fn spec_alloc_node(&self, cx: &Cx, node_id: &Val) -> bool {
        let spec = cx.build().model.spec.get();
        int_key(node_id).is_some_and(|k| spec.allocNodes.with(|m| m.contains_key(k)))
    }
    fn spec_node(&self, cx: &Cx, node_id: &Val) -> Option<NodeRef> {
        let spec = cx.build().model.spec.get();
        int_key(node_id).and_then(|k| spec.nodes.with(|m| m.get(k)))
    }
    fn spec_tree_size(&self, cx: &Cx) -> Val {
        cx.build().model.spec.get().tree.get().extra.get("size")
    }
    fn set_build_flag(&self, cx: &mut Cx) {
        cx.build().model.buildFlag.set(true);
    }
    fn new_item(&self, _cx: &Cx, raw: Option<&str>) -> ItemRef {
        crate::main_mode::new_item(raw, None, false)
    }
    fn get_socket_and_jewel_for_node_id(&self, cx: &Cx, node_id: &Val) -> Option<Table> {
        let m = cx.build().model.clone();
        let Some(id) = node_id.opt_num() else {
            lua_error!("attempt to index field '?' (a nil value)")
        };
        m.itemsTab.GetSocketAndJewelForNodeID(id).0
    }
    fn sockets(&self, cx: &Cx, node_id: &Val) -> Option<Table> {
        let m = cx.build().model.clone();
        int_key(node_id).and_then(|k| m.itemsTab.sockets.with(|s| s.get(k)))
    }
    /// U38's `ItemsTab:AddItemStatDifferences` (ItemsTab.lua:4664-5019).
    fn add_item_stat_differences(
        &self,
        cx: &mut Cx,
        tooltip: &mut Tooltip,
        item: &ItemRef,
        base: Option<ItemBaseRef>,
        slot: &Val,
    ) {
        crate::tabs::items::tooltip::add_item_stat_differences_hook(cx, tooltip, item, base, slot);
    }
    /// The model half (ItemsTab.lua:1686-1724, calc:BM-A); `noAutoEquip` is always true here.
    fn add_item(&self, cx: &mut Cx, item: &ItemRef, no_auto_equip: bool) {
        let m = cx.build().model.clone();
        m.itemsTab.AddItem(&m, item, no_auto_equip, None);
    }
    fn is_item_valid_for_slot(&self, cx: &Cx, item: &ItemRef, slot_name: &Val) -> bool {
        let m = cx.build().model.clone();
        let Some(slot_name) = slot_name.str() else {
            lua_error!("attempt to index local 'slotName' (a nil value)")
        };
        m.itemsTab.IsItemValidForSlot(&m, item, &slot_name, None)
    }
    fn set_sel_item_id(&self, _cx: &mut Cx, _socket: &Table, _sel_item_id: Option<f64>) {
        not_ported("ItemSlotControl:SetSelItemId", "U32")
    }
    /// The model half (ItemsTab.lua:1652-1656, calc:BM-A).
    fn populate_slots(&self, cx: &mut Cx) {
        let m = cx.build().model.clone();
        m.itemsTab.PopulateSlots(&m);
    }
    fn add_undo_state(&self, _cx: &mut Cx) {
        not_ported("ItemsTab:AddUndoState", "U34")
    }
}

// ---- `self.list` aliasing a Lua table ---------------------------------------------------------

/// `self.list = <a Lua sequence>`: the rows are the table's entries (tables), read live.
/// Each row's [`ListValue`] keeps one `Rc` per table, so a row compares (ListControl's
/// `selValue == value`) and memoises (`tooltip:CheckForUpdate(.., data)`) by table identity.
pub struct TableSource {
    pub t: Table,
    rows: RefCell<HashMap<usize, Rc<Table>>>,
}

impl TableSource {
    pub fn new(t: Table) -> TableSource {
        TableSource {
            t,
            rows: RefCell::new(HashMap::new()),
        }
    }
    fn value(&self, t: Table) -> ListValue {
        let key = t.as_ptr() as usize;
        let mut rows = self.rows.borrow_mut();
        // Forget rows that left the list (the Rc kept their address unique until now).
        if rows.len() > 2 * self.t.len() + 16 {
            let live: std::collections::HashSet<usize> = (1..=self.t.len())
                .filter_map(|i| self.t.get(i as i64).table())
                .map(|r| r.as_ptr() as usize)
                .collect();
            rows.retain(|k, _| live.contains(k));
        }
        let rc = rows.entry(key).or_insert_with(|| Rc::new(t)).clone();
        ListValue {
            key: ValKey(key),
            data: rc,
        }
    }
}

impl ListSource for TableSource {
    /// `#self.list`
    fn len(&self, _cx: &Cx) -> usize {
        self.t.len()
    }
    /// `self.list[i]` (a non-table entry is not a row)
    fn get(&self, _cx: &Cx, i: usize) -> Option<ListValue> {
        self.t.get(i as i64).table().map(|t| self.value(t))
    }
    /// `t_insert(self.list, i, v)`
    fn insert(&self, _cx: &mut Cx, i: usize, v: ListValue) {
        let t = row_of(&v);
        self.t.insert(i as i64, t);
    }
    /// `t_remove(self.list, i)`
    fn remove(&self, _cx: &mut Cx, i: usize) -> ListValue {
        match self.t.remove(Some(i as i64)).table() {
            Some(t) => self.value(t),
            None => lua_error!("t_remove: no row at {i}"),
        }
    }
}

/// The row table of a list value.
pub fn row_of(v: &ListValue) -> Table {
    v.get::<Table>()
        .cloned()
        .unwrap_or_else(|| lua_error!("TimelessJewelListControl: a row that is not a table"))
}

// ---- LuaJIT math.random (lib_math.c; CCR-0033 §3) --------------------------------------------

thread_local! {
    // CCR-0033 §3: until pob-lua ports `math.random`. State after `math.randomseed(0)`.
    static PRNG: RefCell<Option<[u64; 4]>> = const { RefCell::new(None) };
}

/// lj_prng.c TW223_STEP: one step of the Tausworthe generator, the running xor of its states.
fn tw223_step(u: &mut [u64; 4]) -> u64 {
    let mut r = 0u64;
    for (i, (k, q, s)) in [(63u32, 31u32, 18u32), (58, 19, 28), (55, 24, 7), (47, 21, 8)]
        .into_iter()
        .enumerate()
    {
        let mut z = u[i];
        z = (((z << q) ^ z) >> (k - s)) ^ ((z & (u64::MAX << (64 - k))) << s);
        r ^= z;
        u[i] = z;
    }
    r
}

/// lib_math.c random_seed(rs, d)
fn random_seed_state(mut d: f64) -> [u64; 4] {
    let mut u = [0u64; 4];
    // 64-k[i] as four 8 bit constants.
    let mut r: u32 = 0x1109_0601;
    for slot in &mut u {
        let m = 1u64 << (r & 255);
        r >>= 8;
        // 3.14159265358979323846, 2.7182818284590452354 (the same doubles)
        d = d * std::f64::consts::PI + std::f64::consts::E;
        let mut bits = d.to_bits();
        // Ensure k[i] MSB of u[i] are non-zero.
        if bits < m {
            bits += m;
        }
        *slot = bits;
    }
    for _ in 0..10 {
        tw223_step(&mut u);
    }
    u
}

/// `math.randomseed(d)` for [`math_random`]'s state.
pub fn math_randomseed(d: f64) {
    PRNG.with(|p| *p.borrow_mut() = Some(random_seed_state(d)));
}

/// `math.random(r1, r2)` (lib_math.c math_random, two-argument form): an integer in `[r1, r2]`.
pub fn math_random(r1: f64, r2: f64) -> f64 {
    PRNG.with(|p| {
        let mut p = p.borrow_mut();
        let u = p.get_or_insert_with(|| random_seed_state(0.0));
        // lj_prng_u64d: a double bit pattern in the range 1.0 <= d < 2.0
        let r = tw223_step(u);
        let d = f64::from_bits((r & 0x000f_ffff_ffff_ffff) | 0x3ff0_0000_0000_0000) - 1.0;
        // d is an int in range [r1, r2]
        (d * (r2 - r1 + 1.0)).floor() + r1
    })
}

// ---- the control ------------------------------------------------------------------------------

/// Lua `a .. b` operand: a string, or a number (`%.14g`); anything else raises.
#[track_caller]
fn cat(v: &Val) -> String {
    match v {
        Val::Str(s) => s.to_string(),
        Val::Num(n) => tostring_num(*n),
        other => lua_error!("attempt to concatenate a {} value", other.lua_type()),
    }
}

/// `table.concat(t, sep)` over `1..#t` (strings and numbers).
fn t_concat(t: &Table, sep: &str) -> String {
    let n = t.len();
    let mut out = String::new();
    for i in 1..=n {
        match t.get(i as i64) {
            Val::Str(s) => out.push_str(&s),
            Val::Num(x) => out.push_str(&tostring_num(x)),
            other => lua_error!(
                "invalid value (at index {i}) in table for 'concat' (a {} value)",
                other.lua_type()
            ),
        }
        if i < n {
            out.push_str(sep);
        }
    }
    out
}

/// `#v` of a table value (nil and other kinds raise).
#[track_caller]
fn len_of(v: &Val) -> usize {
    match v {
        Val::Table(t) => t.len(),
        Val::Data(model::gamedata::Value::Table(t)) => t.len(),
        other => lua_error!("attempt to get length of a {} value", other.lua_type()),
    }
}

/// `label:match("B2B2B2") == nil` of a row (the row's `label` must be a string).
fn not_added(row: &Table) -> bool {
    match row.get("label") {
        Val::Str(s) => pob_lua::pattern::find(&s, "B2B2B2", None, false).is_none(),
        other => lua_error!("attempt to index field 'label' (a {} value)", other.lua_type()),
    }
}

pub struct TimelessJewelListControl {
    pub list: ListControl,
    /// `self.build`
    pub build: Rc<dyn TimelessBuild>,
    /// `self.sharedList` (`build.timelessData.sharedResults or { }`, aliased)
    pub shared_list: Table,
    /// `self.noTooltip` (written by Draw)
    pub no_tooltip: bool,
    /// `self.highlightIndex` (TreeTab.lua:2224-2249 reads and writes it)
    pub highlight_index: Option<usize>,
}

impl TimelessJewelListControl {
    /// lua: TimelessJewelListControl.lua:16-23 TimelessJewelListControl(anchor, rect, build)
    /// @pob Classes/TimelessJewelListControl.lua:15-23 h=8d78e06a
    pub fn new(
        cx: &Cx,
        anchor: Anchor,
        rect: [Prop<f64>; 4],
        build: Rc<dyn TimelessBuild>,
    ) -> New<TimelessJewelListControl> {
        // L17 self.build = build
        let timeless_data = build.timeless_data(cx);
        // L18
        let shared_list = timeless_data.table("sharedResults").unwrap_or_default();
        // L19
        let list = timeless_data.table("searchResults").unwrap_or_default();
        // L20
        let l = ListControl::state(
            &HOOKS,
            16.0,
            true,
            false,
            ListData::Source(Rc::new(TableSource::new(list))),
            false,
        );
        New::new(
            Base::new(anchor, rect),
            TimelessJewelListControl {
                list: l,
                build,
                shared_list,
                no_tooltip: false,
                highlight_index: None,
            },
        )
        .host()
        .init(|cx, me| {
            // L20 (the scroll bars)
            ListControl::init(cx, me.id());
            // L21
            cx.w_mut::<ListControl>(me.id()).sel_index = None;
        })
    }

    /// lua: TimelessJewelListControl.lua:49-53 ScrollToIndex(index)
    /// @pob Classes/TimelessJewelListControl.lua:49-53 h=90f64e08
    pub fn scroll_to_index(cx: &mut Cx, me: CtrlId, index: f64) {
        let (scroll, row_height) = {
            let l = cx.w::<ListControl>(me);
            (l.scroll, l.row_height)
        };
        // L50
        if scroll {
            // L51
            let sb = cx.child_as::<ScrollBarControl>(me, "scrollBarV");
            ScrollBarControl::set_offset(cx, sb.id(), (index - 1.0) * row_height);
        }
    }

    /// lua: TimelessJewelListControl.lua:73-286 GetJewelItem(data)
    /// @pob Classes/TimelessJewelListControl.lua:72-286 h=79a5f791
    pub fn get_jewel_item(cx: &Cx, me: CtrlId, data: &Table) -> ItemRef {
        let raw = Self::jewel_item_raw(cx, me, data);
        // L285
        let build = cx.w::<Self>(me).build.clone();
        build.new_item(cx, raw.as_deref())
    }

    /// The item text of GetJewelItem (L74-L284); `None` when no jewel type matched (`itemData`
    /// stays nil).
    pub fn jewel_item_raw(cx: &Cx, me: CtrlId, data: &Table) -> Option<String> {
        let shared = Val::Table(cx.w::<Self>(me).shared_list.clone());
        let datav = Val::Table(data.clone());
        // L74
        let socket_info = lor(data.get("socketLabel"), || {
            lor(
                {
                    let socket = shared.get("socket");
                    land(socket.clone(), || socket.get("keystone"))
                },
                || Val::from("Unknown"),
            )
        });
        // L75
        let label = format!(
            "[{}; {}; {}]\n",
            cat(&datav.get("seed")),
            cat(&datav.get("total")),
            cat(&socket_info)
        );
        // L76
        let conqueror_id = shared.get("conqueror").get("id");
        let variant = if conqueror_id.raw_eq(&Val::Num(1.0)) {
            "1".to_owned()
        } else {
            format!("{}\n", tostring_num(conqueror_id.num() - 1.0))
        };
        // L77
        let seed = cat(&datav.get("seed"));
        let type_id = shared.get("type").get("id");
        let is = |n: f64| type_id.raw_eq(&Val::Num(n));
        // L78
        let item_data = if is(1.0) {
            // L79-L95
            format!(
                "Glorious Vanity {label}Timeless Jewel\n\
League: Legion\n\
Limited to: 1 Historic\n\
Variant: Doryani (Corrupted Soul)\n\
Variant: Xibaqua (Divine Flesh)\n\
Variant: Ahuana (Immortal Ambition)\n\
Selected Variant: {variant}\n\
Radius: Large\n\
Implicits: 0\n\
{{variant:1}}Bathed in the blood of {seed} sacrificed in the name of Doryani\n\
{{variant:2}}Bathed in the blood of {seed} sacrificed in the name of Xibaqua\n\
{{variant:3}}Bathed in the blood of {seed} sacrificed in the name of Ahuana\n\
Passives in radius are Conquered by the Vaal\n\
Historic\n"
            )
        } else if is(2.0) {
            // L96-L113
            format!(
                "Lethal Pride {label}Timeless Jewel\n\
League: Legion\n\
Limited to: 1 Historic\n\
Variant: Kaom (Strength of Blood)\n\
Variant: Rakiata (Tempered by War)\n\
Variant: Akoya (Chainbreaker)\n\
Selected Variant: {variant}\n\
Radius: Large\n\
Implicits: 0\n\
{{variant:1}}Commanded leadership over {seed} warriors under Kaom\n\
{{variant:2}}Commanded leadership over {seed} warriors under Rakiata\n\
{{variant:3}}Commanded leadership over {seed} warriors under Akoya\n\
Passives in radius are Conquered by the Karui\n\
Historic\n"
            )
        } else if is(3.0) {
            // L114-L131
            format!(
                "Brutal Restraint {label}Timeless Jewel\n\
League: Legion\n\
Limited to: 1 Historic\n\
Variant: Asenath (Dance with Death)\n\
Variant: Nasima (Second Sight)\n\
Variant: Balbala (The Traitor)\n\
Selected Variant: {variant}\n\
Radius: Large\n\
Implicits: 0\n\
{{variant:1}}Denoted service of {seed} dekhara in the akhara of Asenath\n\
{{variant:2}}Denoted service of {seed} dekhara in the akhara of Nasima\n\
{{variant:3}}Denoted service of {seed} dekhara in the akhara of Balbala\n\
Passives in radius are Conquered by the Maraketh\n\
Historic\n"
            )
        } else if is(4.0) {
            // L133
            let dv1 = shared.get("devotionVariant1").get("id");
            let mut alt_variant = if !dv1.raw_eq(&Val::Num(1.0)) && dv1.truthy() {
                dv1
            } else {
                lor(data.get("altVariant"), || Val::Num(math_random(2.0, 17.0)))
            };
            // L134
            let dv2 = shared.get("devotionVariant2").get("id");
            let alt_variant2 = if !dv2.raw_eq(&Val::Num(1.0)) && dv2.truthy() {
                dv2
            } else {
                lor(data.get("altVariant2"), || Val::Num(math_random(2.0, 17.0)))
            };
            // L135
            if alt_variant.raw_eq(&alt_variant2) {
                // L136
                alt_variant = Val::Num(lmod(alt_variant.num(), 16.0) + 2.0);
            }
            // L138
            data.set("altVariant", alt_variant.clone());
            // L139
            data.set("altVariant2", alt_variant2.clone());
            let alt = tostring_num(alt_variant.num() + 2.0);
            let alt2 = tostring_num(alt_variant2.num() + 2.0);
            // L140-L192
            format!(
                "Militant Faith {label}Timeless Jewel\n\
League: Legion\n\
Limited to: 1 Historic\n\
Has Alt Variant: true\n\
Has Alt Variant Two: true\n\
Variant: Avarius (Power of Purpose)\n\
Variant: Dominus (Inner Conviction)\n\
Variant: Maxarius (Transcendence)\n\
Variant: Totem Damage\n\
Variant: Brand Damage\n\
Variant: Channelling Damage\n\
Variant: Area Damage\n\
Variant: Elemental Damage\n\
Variant: Elemental Resistances\n\
Variant: Effect of non-Damaging Ailments\n\
Variant: Elemental Ailment Duration\n\
Variant: Duration of Curses\n\
Variant: Minion Attack and Cast Speed\n\
Variant: Minions Accuracy Rating\n\
Variant: Mana Regen\n\
Variant: Mana Cost (legacy)\n\
Variant: Non-Curse Aura Effect\n\
Variant: Defences from Shield\n\
Variant: Mana Cost Efficiency\n\
Selected Variant: {variant}\n\
Selected Alt Variant: {alt}\n\
Selected Alt Variant Two: {alt2}\n\
Radius: Large\n\
Implicits: 0\n\
{{variant:1}}Carved to glorify {seed} new faithful converted by High Templar Avarius\n\
{{variant:2}}Carved to glorify {seed} new faithful converted by High Templar Dominus\n\
{{variant:3}}Carved to glorify {seed} new faithful converted by High Templar Maxarius\n\
{{variant:4}}4% increased Totem Damage per 10 Devotion\n\
{{variant:5}}4% increased Brand Damage per 10 Devotion\n\
{{variant:6}}Channelling Skills deal 4% increased Damage per 10 Devotion\n\
{{variant:7}}4% increased Area Damage per 10 Devotion\n\
{{variant:8}}4% increased Elemental Damage per 10 Devotion\n\
{{variant:9}}+2% to all Elemental Resistances per 10 Devotion\n\
{{variant:10}}3% increased Effect of non-Damaging Ailments on Enemies per 10 Devotion\n\
{{variant:11}}4% reduced Elemental Ailment Duration on you per 10 Devotion\n\
{{variant:12}}4% reduced Duration of Curses on you per 10 Devotion\n\
{{variant:13}}1% increased Minion Attack and Cast Speed per 10 Devotion\n\
{{variant:14}}Minions have +60 to Accuracy Rating per 10 Devotion\n\
{{variant:15}}Regenerate 0.6 Mana per Second per 10 Devotion\n\
{{variant:16}}1% reduced Mana Cost of Skills per 10 Devotion\n\
{{variant:17}}1% increased effect of Non-Curse Auras per 10 Devotion\n\
{{variant:18}}3% increased Defences from Equipped Shield per 10 Devotion\n\
{{variant:19}}3% increased Mana Cost Efficiency per 10 Devotion\n\
Passives in radius are Conquered by the Templars\n\
Historic\n"
            )
        } else if is(5.0) {
            // L193-L210
            format!(
                "Elegant Hubris {label}Timeless Jewel\n\
League: Legion\n\
Limited to: 1 Historic\n\
Variant: Cadiro (Supreme Decadence)\n\
Variant: Victario (Supreme Grandstanding)\n\
Variant: Caspiro (Supreme Ostentation)\n\
Selected Variant: {variant}\n\
Radius: Large\n\
Implicits: 0\n\
{{variant:1}}Commissioned {seed} coins to commemorate Cadiro\n\
{{variant:2}}Commissioned {seed} coins to commemorate Victario\n\
{{variant:3}}Commissioned {seed} coins to commemorate Caspiro\n\
Passives in radius are Conquered by the Eternal Empire\n\
Historic\n"
            )
        } else if is(6.0) {
            // L211-L228
            format!(
                "Heroic Tragedy {label}Timeless Jewel\n\
League: Legion\n\
Limited to: 1 Historic\n\
Variant: Vorana (Black Scythe Training)\n\
Variant: Uhtred (Celestial Mathematics)\n\
Variant: Medved (The Unbreaking Circle)\n\
Selected Variant: {variant}\n\
Radius: Large\n\
Implicits: 0\n\
{{variant:1}}Remembrancing {seed} songworthy deeds by the line of Vorana\n\
{{variant:2}}Remembrancing {seed} songworthy deeds by the line of Uhtred\n\
{{variant:3}}Remembrancing {seed} songworthy deeds by the line of Medved\n\
Passives in radius are Conquered by the Kalguur\n\
Historic\n"
            )
        } else if is(7.0) {
            // L229-L239
            format!(
                "Festering Vengeance {label}Murderous Eye Jewel\n\
League: Allflame\n\
Limited to: 1 Historic\n\
Implicits: 0\n\
Subjugating {seed} souls in the thrall of Tecrod\n\
Passives affected are Conquered by the Abyssal\n\
Historic\n"
            )
        } else if is(8.0) {
            // L240-L250
            format!(
                "Extinguishing Grasp {label}Searching Eye Jewel\n\
League: Allflame\n\
Limited to: 1 Historic\n\
Implicits: 0\n\
Subjugating {seed} souls in the thrall of Ulaman\n\
Passives affected are Conquered by the Abyssal\n\
Historic\n"
            )
        } else if is(9.0) {
            // L251-L261
            format!(
                "Baleful Dominion {label}Hypnotic Eye Jewel\n\
League: Allflame\n\
Limited to: 1 Historic\n\
Implicits: 0\n\
Subjugating {seed} souls in the thrall of Kurgal\n\
Passives affected are Conquered by the Abyssal\n\
Historic\n"
            )
        } else if is(10.0) {
            // L262-L272
            format!(
                "Destructive Aspiration {label}Ghastly Eye Jewel\n\
League: Allflame\n\
Limited to: 1 Historic\n\
Implicits: 0\n\
Subjugating {seed} souls in the thrall of Amanamu\n\
Passives affected are Conquered by the Abyssal\n\
Historic\n"
            )
        } else if is(11.0) {
            // L273-L283
            format!(
                "Reclaimed Malevolence {label}Assembled Eye Jewel\n\
League: Allflame\n\
Limited to: 1 Historic\n\
Implicits: 0\n\
Binding {seed} souls to phylacteries to sustain Zorath\n\
Passives affected are Conquered by the Abyssal\n\
Historic\n"
            )
        } else {
            return None;
        };
        Some(item_data)
    }
}

/// lua: TimelessJewelListControl.lua:30-47 SetHighlightColor(index, value)
/// @pob Classes/TimelessJewelListControl.lua:30-47 h=ef779943
fn set_highlight_color(cx: &mut Cx, me: CtrlId, index: usize, _value: &ListValue) -> bool {
    let (highlight_index, sel_index) = (
        cx.w::<TimelessJewelListControl>(me).highlight_index,
        cx.w::<ListControl>(me).sel_index,
    );
    // L31
    let (Some(highlight_index), Some(sel_index)) = (highlight_index, sel_index) else {
        // L32
        return false;
    };
    let (h, s, i) = (highlight_index as f64, sel_index as f64, index as f64);
    // L34
    let is_highlighted = m_min(s, h) <= i && m_max(s, h) >= i;

    // L36
    if is_highlighted {
        // L37
        if sel_index == index || highlight_index == index {
            // L38
            cx.sg.set_draw_color(1.0, 0.5, 0.0);
        } else {
            // L40
            cx.sg.set_draw_color(1.0, 1.0, 0.0);
        }

        // L43
        return true;
    }

    // L46
    false
}

/// lua: TimelessJewelListControl.lua:55-64 OverrideSelectIndex(index)
/// @pob Classes/TimelessJewelListControl.lua:55-64 h=534b55c6
fn override_select_index(cx: &mut Cx, me: CtrlId, index: usize) -> bool {
    // L56
    if cx.sys.is_key_down(Key::Shift) && cx.w::<ListControl>(me).sel_index.is_some() {
        // L57
        cx.w_mut::<TimelessJewelListControl>(me).highlight_index = Some(index);
        // L58
        return true;
    }
    // L60
    cx.w_mut::<TimelessJewelListControl>(me).highlight_index = None;

    // L63
    false
}

/// lua: TimelessJewelListControl.lua:66-70 GetRowValue(column, index, data)
/// @pob Classes/TimelessJewelListControl.lua:66-70 h=15b964f7
fn get_row_value(_cx: &Cx, _me: CtrlId, column: usize, _index: usize, v: &ListValue) -> String {
    // L67
    if column == 1 {
        // L68
        return cat(&row_of(v).get("label"));
    }
    // (nil: the list has one column)
    String::new()
}

/// lua: TimelessJewelListControl.lua:291-333 AddValueTooltip(tooltip, index, data)
/// @pob Classes/TimelessJewelListControl.lua:288-333 h=5a874c3e
fn add_value_tooltip(cx: &mut Cx, me: CtrlId, tooltip: &mut Tooltip, index: usize, v: &ListValue) {
    let data = row_of(v);
    let (build, shared_list) = {
        let w = cx.w::<TimelessJewelListControl>(me);
        (w.build.clone(), w.shared_list.clone())
    };
    let shared = Val::Table(shared_list.clone());
    // L292
    let socket_id = lor(data.get("socketId"), || shared.get("socket").get("id"));
    // L293
    // (`socketId and socketId ~= -1 and ...`: nil, false or the socket's result)
    let socket = if !socket_id.truthy() {
        socket_id.clone()
    } else if socket_id.raw_eq(&Val::Num(-1.0)) {
        Val::Bool(false)
    } else {
        build
            .get_socket_and_jewel_for_node_id(cx, &socket_id)
            .map_or(Val::Nil, Val::Table)
    };
    // L294
    let rev = build.output_revision(cx);
    if !tooltip.check_for_update(cx, &[TtKey::Num(rev), TtKey::Obj(TtObj(v.data.clone()))]) {
        // L295
        return;
    }
    // L297
    tooltip.clear(false);
    // L298
    if !cx.w::<TimelessJewelListControl>(me).no_tooltip {
        let row = ListControl::list_get(cx, me, index)
            .map(|r| row_of(&r))
            .unwrap_or_else(|| lua_error!("attempt to index field '?' (a nil value)"));
        // L299
        if not_added(&row) {
            // L300
            tooltip.add_line(cx, 16.0, "^7Double click to add this jewel to your build.");
            // L301
            if build.timeless_data(cx).truthy("socketAllocate") {
                // L302
                if socket.truthy() && build.spec_alloc_node(cx, &socket_id) {
                    // L303
                    let where_ = lor(data.get("socketLabel"), || {
                        lor(shared.get("socket").get("label"), || socket_id.clone())
                    });
                    tooltip.add_line(
                        cx,
                        16.0,
                        &format!("^7It will be socketed into {}.", cat(&where_)),
                    );
                } else {
                    // L305
                    let text = format!(
                        "{}That jewel socket is not allocated, so the jewel will only be added to your items.",
                        cx.main.cc.WARNING
                    );
                    tooltip.add_line(cx, 16.0, &text);
                }
            }
        } else {
            // L309
            let text = format!(
                "^7{} {} was successfully added to your build.",
                cat(&shared.get("type").get("label")),
                cat(&data.get("seed"))
            );
            tooltip.add_line(cx, 16.0, &text);
        }
        // L311
        let mut sorted_node_lists: LjIntMap<String> = LjIntMap::new();
        // L312 (ORDER: string keys, see the module docs)
        let desired_nodes = lor(shared.get("desiredNodes"), || Val::Table(Table::new()));
        let Some(desired_nodes) = desired_nodes.table() else {
            lua_error!("bad argument #1 to 'pairs' (table expected)")
        };
        for (legion_id, desired_node) in desired_nodes.pairs() {
            let entry = row.get(legion_id);
            // L313
            if entry.truthy() {
                let desired_idx = desired_node.get("desiredIdx");
                let Some(k) = int_key(&desired_idx) else {
                    lua_error!("sortedNodeLists: desiredIdx {desired_idx:?} is not an integer key")
                };
                let names = entry.get("targetNodeNames");
                // L314
                let text = if names.truthy() && len_of(&names) > 0 {
                    // L315
                    format!(
                        "^7        {}:\n^8                {}",
                        cat(&desired_node.get("displayName")),
                        t_concat(&names.table().expect("a table"), "\n                ")
                    )
                } else {
                    // L317
                    format!(
                        "^7        {}:\n^8                None",
                        cat(&desired_node.get("displayName"))
                    )
                };
                sorted_node_lists.set(k, Some(text));
            }
        }
        // L321
        if !sorted_node_lists.is_empty() {
            // L322
            tooltip.add_line(cx, 16.0, "^7Node List:");
            // L323
            for (_, sorted_node_list) in sorted_node_lists.pairs() {
                // L324
                tooltip.add_line(cx, 16.0, &sorted_node_list);
            }
        }
        // L327
        let total = data.get("total");
        let Val::Num(total_n) = total else {
            lua_error!("attempt to compare number with {}", total.lua_type())
        };
        if total_n > 0.0 {
            // L328
            tooltip.add_line(cx, 16.0, &format!("^7Combined Node Weight: {}", cat(&total)));
        }
        // L330
        let jewel = TimelessJewelListControl::get_jewel_item(cx, me, &data);
        // L331
        let base = jewel.base.get();
        build.add_item_stat_differences(cx, tooltip, &jewel, base, &socket);
    }
}

/// lua: TimelessJewelListControl.lua:335-352 OnSelClick(index, data, doubleClick)
/// @pob Classes/TimelessJewelListControl.lua:335-352 h=ef01aa26
fn on_sel_click(
    cx: &mut Cx,
    me: CtrlId,
    index: usize,
    v: &ListValue,
    double_click: bool,
) -> Option<bool> {
    // `self.list[index]`
    fn row(cx: &Cx, me: CtrlId, index: usize) -> Table {
        ListControl::list_get(cx, me, index)
            .map(|r| row_of(&r))
            .unwrap_or_else(|| lua_error!("attempt to index field '?' (a nil value)"))
    }
    // L336
    if double_click && not_added(&row(cx, me, index)) {
        let data = row_of(v);
        let (build, shared_list) = {
            let w = cx.w::<TimelessJewelListControl>(me);
            (w.build.clone(), w.shared_list.clone())
        };
        // L337
        let item = TimelessJewelListControl::get_jewel_item(cx, me, &data);
        // L338 local itemsTab = self.build.itemsTab
        // L339
        build.add_item(cx, &item, true);
        // L340
        if build.timeless_data(cx).truthy("socketAllocate") {
            // L341
            let socket_id = lor(data.get("socketId"), || {
                Val::Table(shared_list.clone()).get("socket").get("id")
            });
            // L342
            let socket_control = if socket_id.raw_eq(&Val::Num(-1.0)) {
                None
            } else {
                build.sockets(cx, &socket_id)
            };
            // L343
            if let Some(socket_control) = socket_control {
                if build.spec_alloc_node(cx, &socket_id)
                    && build.is_item_valid_for_slot(cx, &item, &socket_control.get("slotName"))
                {
                    // L344
                    build.set_sel_item_id(cx, &socket_control, item.id.get());
                    // L345
                    build.set_build_flag(cx);
                }
            }
        }
        // L348
        build.populate_slots(cx);
        // L349
        build.add_undo_state(cx);
        // L350
        let row = row(cx, me, index);
        let label = format!("^xB2B2B2{}", cat(&row.get("label")));
        row.set("label", label);
    }
    None
}

static HOOKS: ListHooks = ListHooks {
    get_row_value,
    add_value_tooltip: Some(add_value_tooltip),
    on_sel_click: Some(on_sel_click),
    override_select_index: Some(override_select_index),
    set_highlight_color: Some(set_highlight_color),
    ..ListHooks::BASE
};

impl Widget for TimelessJewelListControl {
    const NAME: &'static str = "TimelessJewelListControl";
    const CAPS: Caps = <ListControl as Widget>::CAPS;

    /// lua: TimelessJewelListControl.lua:25-28 Draw(viewPort, noTooltip)
    /// @pob Classes/TimelessJewelListControl.lua:25-28 h=3b6f23ad
    fn draw(cx: &mut Cx, me: CtrlId, vp: &ViewPort, no_tooltip: bool) {
        // L26
        cx.w_mut::<Self>(me).no_tooltip = no_tooltip;
        // L27 (the super Draw gets noTooltip nil)
        ListControl::draw(cx, me, vp, false);
    }
    fn is_mouse_over(cx: &Cx, me: CtrlId) -> bool {
        ListControl::is_mouse_over(cx, me)
    }
    fn on_key_down(cx: &mut Cx, me: CtrlId, key: Key, double_click: bool) -> Focus {
        ListControl::on_key_down(cx, me, key, double_click)
    }
    fn on_key_up(cx: &mut Cx, me: CtrlId, key: Key) -> Focus {
        ListControl::on_key_up(cx, me, key)
    }
    fn duck(cx: &Cx, me: CtrlId, q: Duck) -> bool {
        ListControl::duck(cx, me, q)
    }

    pob_ui_core::impl_parts!(TimelessJewelListControl: list => ListControl);
}

#[cfg(test)]
mod tests {
    use super::*;

    /// `math.randomseed(s)` then `math.random(2, 17)` under LuaJIT 2.1.1788856981 (/usr/bin/luajit):
    /// `luajit -e 'math.randomseed(0) for i=1,8 do io.write(math.random(2,17)," ") end'` prints
    /// `14 13 11 14 3 12 7 13`; seed 12.5 gives `9 13 2 7`.
    #[test]
    fn prng_matches_luajit() {
        math_randomseed(0.0);
        let got: Vec<f64> = (0..8).map(|_| math_random(2.0, 17.0)).collect();
        assert_eq!(got, vec![14.0, 13.0, 11.0, 14.0, 3.0, 12.0, 7.0, 13.0]);
        math_randomseed(12.5);
        let got: Vec<f64> = (0..4).map(|_| math_random(2.0, 17.0)).collect();
        assert_eq!(got, vec![9.0, 13.0, 2.0, 7.0]);
        // the state starts as after randomseed(0)
        PRNG.with(|p| *p.borrow_mut() = None);
        assert_eq!(math_random(2.0, 17.0), 14.0);
    }

    #[test]
    fn t_concat_and_cat() {
        let t = Table::new();
        t.push("a");
        t.push(2.5);
        t.push("c");
        assert_eq!(t_concat(&t, ", "), "a, 2.5, c");
        assert_eq!(cat(&Val::Num(12.0)), "12");
        assert!(pob_lua::error::catch(|| cat(&Val::Nil)).is_err());
    }

    #[test]
    fn sorted_node_lists_contiguous_indices_iterate_in_order() {
        // L311-L325: whatever order `pairs(desiredNodes)` inserts 1..n in, the integer keys end
        // in the array part and iterate 1..n (module docs, ORDER).
        let perms: [[i64; 4]; 4] = [[1, 2, 3, 4], [4, 3, 2, 1], [3, 1, 4, 2], [2, 4, 1, 3]];
        for p in perms {
            let mut m: LjIntMap<String> = LjIntMap::new();
            for k in p {
                m.set(k, Some(k.to_string()));
            }
            let keys: Vec<i64> = m.pairs().into_iter().map(|(k, _)| k).collect();
            assert_eq!(keys, vec![1, 2, 3, 4], "insertion order {p:?}");
        }
    }
}
