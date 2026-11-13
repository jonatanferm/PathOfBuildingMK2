//! Port of PathOfBuilding v2.67.2 (16de4b82d) src/Classes/ItemsTab.lua:2609-3120 (the item set
//! manager, CraftItem, EditDisplayItemText, EnchantDisplayItem, the anoint helpers and
//! AnointDisplayItem). Owner: U36.
//!
//! @pob-file Classes/ItemsTab.lua @16de4b82d
//!
//! The functions are free functions over the ItemsTab node `me` (`self`), named as the Lua
//! methods; the constructor's closures (U33) reach them through U36's entries of
//! [`default_items_ui_hooks`](super::ctor::default_items_ui_hooks). The fields they write on the
//! ItemsTab (`lastCraftRaritySel`, `lastCraftTypeSel`, `lastCraftBaseSel`, `enchantSlot`,
//! `anointEnchantSlot`) are this unit's part of the split struct, [`State`].
//!
//! Calls into the other ItemsTab units: `SetDisplayItem` and `CreateDisplayItemFromRaw` (U35,
//! ported) are called directly (CCR-0124 §2's rule: a ported method needs no seam);
//! `AddItemTooltip` (U38) goes through U32's `ItemsTabHooks::add_item_tooltip` and
//! `AddDisplayItem` (U34) through `ItemsUiHooks::add_display_item`, the entries the tests
//! replace. `getAnoint` (L2978-2993) and `getMissingAnointCount` (L2998-3005) are calc:BM-A's
//! model methods (`model::ItemsTab::getAnoint`, `getMissingAnointCount`; their markers are in
//! pob-calc); the anoint helpers below call the model's `getAnoint`.
//!
//! Lua semantics:
//! * A popup's dropdown `list` *is* the Lua table the popup's closures mutate in place
//!   (`skillList`, `enchantmentSourceList`, `enchantmentList`): the Rust dropdown owns a
//!   `Vec<DropItem>`, so every function that rebuilds or sorts one of those tables copies it into
//!   its dropdown right after the mutation (`sync_*`), before anything reads the dropdown. A
//!   direct `control.list = t` store is the same copy (no CheckDroppedWidth, as in Lua).
//! * The dropdown entries of EnchantDisplayItem are `{ label, line }` tables that the sort
//!   decorates (`defaultSortOrder`, `sortValue`, `sortValues`) and compares by identity
//!   (`entry == selected`): [`EnchantEntry`] behind an `Rc`, with `Cell` fields.
//! * `t_insert(t, pos, v)` on the hole-free mod line lists: every call site inserts at most one
//!   past the end (the "Enchantment 2" / "Anoint N" buttons are shown only when the item already
//!   has N-1 enchant lines, ItemsTab.lua:527-558, and the re-parsed copy keeps them), so
//!   `Shared::insert`'s hole error is never reached.
//! * DIVERGENCE(pending-calc): `GetMiscCalculator()` and `calcFunc(..)` only return Pending on
//!   wasm (calc facade, ui doc §10.4); the enchant power sort then sorts by 0, the anoint and
//!   notable comparisons add no lines. Never on desktop.
//! * `self.build.spec.tree.nodes` (the anoint popup's NotableDB `db`) is passed as a copy of the
//!   node map: the map is not mutated while the popup is open (the builder writes
//!   `node.measuredPower` on the shared node objects, which the copy shares).
//!
//! ORDER:
//! * L2793 `pairs(enchantments)`: the skill names are sorted at L2798 (`table.sort` of distinct
//!   strings, a total order), so the `pairs` order is unobservable (c).
//!
//! Props with side effects (ui doc §7.3): none in this range (the anoint button's `x`, `width`
//! and `label` functions only read).
#![allow(non_snake_case, clippy::too_many_lines)]

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use pob_lua::math::m_min;
use pob_lua::Name;
use pob_sg::Font;
use pob_ui_core::controls::check_box::CheckBoxControl;
use pob_ui_core::controls::drop_down::{DropTipMode, SelFunc};
use pob_ui_core::controls::list::ListControl;
use pob_ui_core::controls::search_host::SearchHost;
use pob_ui_core::controls::{
    ButtonControl, DropDownControl, DropItem, EditArgs, EditControl, LabelControl,
};
use pob_ui_core::popups::{self, PopupOpts};
use pob_ui_core::prelude::*;
use pob_ui_core::tooltip::tooltip::Tooltip;

use super::ctor::{self, PowerSortValue, RarityValue};
use super::set_list::ItemSetListControl;
use super::shared_set_list::SharedItemSetListControl;
use super::slot_helper::items_hooks;
use super::ItemsTab;
use crate::app::CxApp;
use crate::calc::facade::MiscCalc;
use crate::model::{self, gamedata, Table, Val};
use crate::tabs::tree::notable_db::{NotableDBControl, NotableItemsTab};

/// This unit's part of the split class struct (ui doc §12.1): the ItemsTab fields this range
/// writes (nil until first written).
#[derive(Default)]
pub struct State {
    /// `self.lastCraftRaritySel` (L2698)
    pub lastCraftRaritySel: Option<usize>,
    /// `self.lastCraftTypeSel` (L2699)
    pub lastCraftTypeSel: Option<usize>,
    /// `self.lastCraftBaseSel` (L2700)
    pub lastCraftBaseSel: Option<usize>,
    /// `self.enchantSlot` (L2765)
    pub enchantSlot: Option<f64>,
    /// `self.anointEnchantSlot` (L3012, L3082)
    pub anointEnchantSlot: Option<f64>,
}

// ---- helpers (Rust-only plumbing) ------------------------------------------------------------

fn st(cx: &Cx, me: CtrlId) -> &State {
    &cx.w::<ItemsTab>(me).craft_a
}

fn st_mut(cx: &mut Cx, me: CtrlId) -> &mut State {
    &mut cx.w_mut::<ItemsTab>(me).craft_a
}

/// `self.displayItem` (nil → `None`).
fn display_item(cx: &Cx, me: CtrlId) -> Option<model::ItemRef> {
    ctor::display_item(cx, me)
}

/// `self.displayItem` that Lua then indexes (nil raises).
#[track_caller]
fn di(cx: &Cx, me: CtrlId) -> model::ItemRef {
    display_item(cx, me)
        .unwrap_or_else(|| lua_error!("attempt to index field 'displayItem' (a nil value)"))
}

/// `controls.<key>` of a popup, looked up when the closure runs (nil raises when indexed).
#[track_caller]
fn pc(cx: &Cx, p: CtrlId, key: &'static str) -> CtrlId {
    cx.child(p, key)
        .unwrap_or_else(|| lua_error!("attempt to index field '{key}' (a nil value)"))
}

/// `controls.<key>` of a popup, nil while it does not exist yet.
fn pc_opt(cx: &Cx, p: CtrlId, key: &'static str) -> Option<CtrlId> {
    cx.child(p, key)
}

/// `drop.selIndex`
fn sel_index(cx: &Cx, drop: CtrlId) -> usize {
    cx.w::<DropDownControl>(drop).sel_index
}

/// The SearchHost of a dropdown (`drop:UpdateSearch()`).
fn dd_search(cx: &mut Cx, me: CtrlId) -> &mut SearchHost {
    &mut cx.w_mut::<DropDownControl>(me).search
}

/// `drop:UpdateSearch()`
fn update_search(cx: &mut Cx, drop: CtrlId) {
    SearchHost::update_search(cx, drop, dd_search);
}

/// `item.<k>` truthiness of an untyped Item field.
fn ext(item: &model::ItemRef, k: &str) -> bool {
    item.extra.truthy(Name::from(k))
}

/// `{ crafted = true, line = line }`
fn crafted_line(line: &str) -> Table {
    let t = Table::new();
    t.set(Name::from("crafted"), true);
    t.set(Name::from("line"), line);
    t
}

/// `self.build.calcsTab:GetMiscCalculator()`: `calcFunc, calcBase` (`None` = Pending, wasm only).
pub(crate) fn misc_calc(cx: &mut Cx) -> Option<(Rc<dyn MiscCalc>, model::Output)> {
    let m = cx.build().model.clone();
    match cx.build_mut().calc.misc_calculator(&m) {
        std::task::Poll::Ready(r) => Some(r),
        // DIVERGENCE(pending-calc)
        std::task::Poll::Pending => None,
    }
}

/// `calcFunc({ repSlotName = slotName, repItem = item }, useFullDPS)` (`None` = Pending).
pub(crate) fn calc_rep(
    calcFunc: &Rc<dyn MiscCalc>,
    slotName: Name,
    item: &model::ItemRef,
    useFullDPS: Option<bool>,
) -> Option<model::Output> {
    let ov = model::Override::default();
    ov.repSlotName.set(Some(slotName));
    ov.repItem.set(Some(item.clone()));
    match calcFunc.call(Some(Rc::new(ov)), useFullDPS) {
        std::task::Poll::Ready(o) => Some(o),
        // DIVERGENCE(pending-calc)
        std::task::Poll::Pending => None,
    }
}

/// `self:AddItemTooltip(tooltip, item, slot, dbMode)` (U38, through U32's hook table).
fn add_item_tooltip(cx: &mut Cx, tooltip: &mut Tooltip, item: &model::ItemRef, dbMode: bool) {
    let f = items_hooks().add_item_tooltip;
    let b = cx.build().model.clone();
    f(cx, &b, tooltip, item, None, dbMode);
}

/// `data.<root>` of an untyped dump root (`Data::other`).
fn data_root(root: &str) -> &'static gamedata::DataValue {
    static NIL: gamedata::DataValue = gamedata::DataValue::Nil;
    model::data().other.get(root).unwrap_or(&NIL)
}

/// A string field of a data table (`nil` and other kinds are `None`).
fn data_str(t: &gamedata::DataTable, k: &str) -> Option<String> {
    match model::data().resolve(t.field(k)) {
        gamedata::DataValue::Str(s) => Some(s.to_string()),
        _ => None,
    }
}

/// The `1..n` part of a data table.
fn data_seq(v: &'static gamedata::DataValue) -> Vec<&'static gamedata::DataValue> {
    match model::data().resolve(v) {
        gamedata::DataValue::Table(t) => t.seq.iter().map(|x| model::data().resolve(x)).collect(),
        _ => Vec::new(),
    }
}

// ---- L2609-2619 OpenItemSetManagePopup ----------------------------------------------------------

/// lua: ItemsTab.lua:2609-2619 `ItemsTabClass:OpenItemSetManagePopup()`: opens the item set
/// manager.
/// @pob Classes/ItemsTab.lua:2608-2619 h=a72a2341
pub fn OpenItemSetManagePopup(cx: &mut Cx, me: CtrlId) {
    // L2610 local controls = { }
    let order = cx.build().model.itemsTab.itemSetOrderList.clone();
    // L2618
    popups::open_popup(
        cx,
        630.0,
        290.0,
        "Manage Item Sets",
        move |cx, p| {
            let p = p.id();
            // L2611
            let setList = cx
                .add(
                    p,
                    "setList",
                    ItemSetListControl::new(Anchor::NONE, rect(-155, 50, 300, 200), me, order),
                )
                .id();
            // L2612
            let sharedList = cx
                .add(
                    p,
                    "sharedList",
                    SharedItemSetListControl::new(Anchor::NONE, rect(155, 50, 300, 200), me),
                )
                .id();
            // L2613
            cx.w_mut::<ListControl>(setList).drag_target_list = Some(vec![sharedList]);
            // L2614
            cx.w_mut::<ListControl>(sharedList).drag_target_list = Some(vec![setList]);
            // L2615
            cx.add(
                p,
                "close",
                ButtonControl::new(Anchor::NONE, rect(0, 260, 90, 20), "Done", |cx| {
                    // L2616
                    popups::close_popup(cx);
                }),
            );
        },
        PopupOpts::default(),
    );
}

// ---- L2622-2706 CraftItem -----------------------------------------------------------------------

/// A `data.itemBaseLists[type][i]` entry (`{ label, name, base }`, Data.lua:1259): the value of
/// the CraftItem popup's base dropdown entries.
#[derive(Clone, Debug)]
pub struct CraftBase {
    /// `base.name` (the `data.itemBases` key)
    pub name: String,
    /// `base.base`
    pub base: gamedata::schema::bases::ItemBaseRef,
}

/// `self.build.data.itemBaseTypeList` (Data.lua:1262-1281).
fn itemBaseTypeList() -> Vec<String> {
    data_seq(data_root("data.itemBaseTypeList"))
        .into_iter()
        .filter_map(|v| match v {
            gamedata::DataValue::Str(s) => Some(s.to_string()),
            _ => None,
        })
        .collect()
}

/// `self.build.data.itemBaseLists[type]` as dropdown entries (nil → empty list: the dropdown's
/// SetList(list or { })).
fn itemBaseList(ty: Option<&str>) -> Vec<DropItem> {
    let Some(ty) = ty else { return Vec::new() };
    let gamedata::DataValue::Table(lists) = model::data().resolve(data_root("data.itemBaseLists"))
    else {
        return Vec::new();
    };
    let list = lists.field(ty);
    data_seq_owned(list)
        .into_iter()
        .filter_map(|e| {
            let gamedata::DataValue::Table(e) = e else {
                return None;
            };
            let name = data_str(e, "name")?;
            let label = data_str(e, "label").unwrap_or_default();
            // `base` is `data.itemBases[name]` (Data.lua:1259)
            let base = model::data().itemBases.get(name.as_str())?.clone();
            Some(DropItem::new(label, Rc::new(CraftBase { name, base })))
        })
        .collect()
}

fn data_seq_owned(v: &gamedata::DataValue) -> Vec<&gamedata::DataValue> {
    match model::data().resolve(v) {
        gamedata::DataValue::Table(t) => t.seq.iter().map(|x| model::data().resolve(x)).collect(),
        _ => Vec::new(),
    }
}

/// `makeItem(base)` (L2624-2670), a closure over the popup's `controls`.
fn makeItem(cx: &Cx, p: CtrlId, base: Option<&DropItem>) -> model::ItemRef {
    let base = base
        .and_then(|b| b.get::<CraftBase>())
        .unwrap_or_else(|| lua_error!("attempt to index local 'base' (a nil value)"));
    let bb = &base.base;
    // L2625
    let item = crate::main_mode::new_item(None, None, false);
    // L2626
    item.name.set(model::LStr::from(base.name.as_str()));
    // L2627
    item.base.set(Some(bb.clone()));
    // L2628
    item.baseName.set(Some(Name::from(base.name.as_str())));
    // L2629-2635
    item.buffModLines.set(model::Shared::new(Vec::new()));
    item.enchantModLines.set(model::Shared::new(Vec::new()));
    item.classRequirementModLines
        .set(model::Shared::new(Vec::new()));
    item.scourgeModLines.set(model::Shared::new(Vec::new()));
    item.implicitModLines.set(model::Shared::new(Vec::new()));
    item.explicitModLines.set(model::Shared::new(Vec::new()));
    item.crucibleModLines.set(model::Shared::new(Vec::new()));
    // L2636
    let ty = &*bb.r#type;
    if matches!(
        ty,
        "Amulet" | "Belt" | "Jewel" | "Quiver" | "Ring" | "Graft"
    ) {
        // L2637
        item.quality.set(None);
    } else {
        // L2639
        item.quality.set(Some(0.0));
    }
    // L2641
    let rarity = pc(cx, p, "rarity");
    let mut raritySel = sel_index(cx, rarity);
    // L2642-2645
    if bb.flask.is_some()
        || (ty == "Jewel" && bb.subType.as_deref() == Some("Charm"))
        || ty == "Tincture"
    {
        // L2646
        if raritySel == 3 {
            // L2647
            raritySel = 2;
        }
    }
    // L2650
    if raritySel == 2 || raritySel == 3 {
        // L2651
        item.extra.set(Name::from("crafted"), true);
    }
    // L2653
    let r = cx
        .w::<DropDownControl>(rarity)
        .list
        .get(raritySel.wrapping_sub(1))
        .and_then(|v| v.get::<RarityValue>().copied())
        .unwrap_or_else(|| lua_error!("attempt to index field '?' (a nil value)"));
    item.rarity.set(Name::from(r.0));
    // L2654
    if raritySel >= 3 {
        // L2655
        let buf = cx.w::<EditControl>(pc(cx, p, "title")).buf.clone();
        let title = if pob_ui_core::luastr::lmatch(&buf, "%S").is_some() {
            buf
        } else {
            "New Item".to_owned()
        };
        item.title.set(Some(model::LStr::from(title.as_str())));
    }
    // L2657
    if let Some(implicit) = &bb.implicit {
        // L2658
        let mut implicitIndex = 1usize;
        // L2659
        for line in pob_lua::gmatch(implicit, "[^\n]+").map(|c| c[0].to_str().into_owned()) {
            // L2660
            let (modList, extra) = pob_modparser::parseMod(&line, false);
            // L2661
            let modLine = Table::new();
            modLine.set(Name::from("line"), line.as_str());
            if let Some(extra) = extra {
                modLine.set(Name::from("extra"), extra.as_str());
            }
            let ml = Table::new();
            for m in modList.unwrap_or_default() {
                ml.push(Val::Mod(std::sync::Arc::new(m)));
            }
            modLine.set(Name::from("modList"), ml);
            let tags = Table::new();
            if let Some(types) = bb.implicitModTypes.get(implicitIndex - 1) {
                for tag in types {
                    tags.push(*tag);
                }
            }
            modLine.set(Name::from("modTags"), tags);
            item.implicitModLines.get().push(modLine);
            // L2662
            implicitIndex += 1;
        }
    }
    // L2665
    item.NormaliseQuality();
    // L2666
    item.BuildAndParseRaw();
    // L2667
    item
}

/// lua: ItemsTab.lua:2622-2706 `ItemsTabClass:CraftItem()`: opens the item crafting popup.
/// @pob Classes/ItemsTab.lua:2621-2706 h=b38548ea
pub fn CraftItem(cx: &mut Cx, me: CtrlId) {
    // L2623 local controls = { }; L2624-2670 makeItem: [`makeItem`]
    let typeList = itemBaseTypeList();
    let (lastRarity, lastType, lastBase) = {
        let s = st(cx, me);
        (s.lastCraftRaritySel, s.lastCraftTypeSel, s.lastCraftBaseSel)
    };
    let rarityList = ctor::rarityDropList(cx);
    // L2705
    popups::open_popup(
        cx,
        370.0,
        130.0,
        "Craft Item",
        move |cx, p| {
            let p = p.id();
            // L2669
            cx.add(
                p,
                "rarityLabel",
                LabelControl::new(
                    Anchor::nil(TopRight, TopLeft),
                    rect(50, 20, 0, 16),
                    "Rarity:",
                ),
            );
            // L2670
            let rarity = cx
                .add(
                    p,
                    "rarity",
                    DropDownControl::new(
                        Anchor::NONE,
                        rect(-80, 20, 100, 18),
                        rarityList,
                        None,
                        None,
                        false,
                    ),
                )
                .id();
            // L2671
            cx.w_mut::<DropDownControl>(rarity).sel_index = lastRarity.unwrap_or(3);
            // L2672
            cx.add(
                p,
                "title",
                EditControl::new(
                    Anchor::NONE,
                    rect(70, 20, 190, 18),
                    EditArgs {
                        init: Some(String::new()),
                        prompt: Some("Name".to_owned()),
                        ..EditArgs::default()
                    },
                )
                // L2673
                .shown(Prop::f(move |cx, _| {
                    // L2674
                    sel_index(cx, pc(cx, p, "rarity")) >= 3
                })),
            );
            // L2676
            cx.add(
                p,
                "typeLabel",
                LabelControl::new(Anchor::nil(TopRight, TopLeft), rect(50, 45, 0, 16), "Type:"),
            );
            // L2677
            let typeList2 = typeList.clone();
            let typeSel: SelFunc = Rc::new(move |cx, index, _value| {
                // L2678
                let base = pc(cx, p, "base");
                cx.w_mut::<DropDownControl>(base).list =
                    itemBaseList(typeList2.get(index.wrapping_sub(1)).map(String::as_str));
                // L2679
                cx.w_mut::<DropDownControl>(base).sel_index = 1;
            });
            let ty = cx
                .add(
                    p,
                    "type",
                    DropDownControl::new(
                        Anchor::nil(TopLeft, TopLeft),
                        rect(55, 45, 295, 18),
                        typeList.iter().map(|s| DropItem::str(s.as_str())).collect(),
                        Some(typeSel),
                        None,
                        false,
                    ),
                )
                .id();
            // L2681
            cx.w_mut::<DropDownControl>(ty).sel_index = lastType.unwrap_or(1);
            // L2682
            cx.add(
                p,
                "baseLabel",
                LabelControl::new(Anchor::nil(TopRight, TopLeft), rect(50, 70, 0, 16), "Base:"),
            );
            // L2683
            let typeSelIndex = sel_index(cx, ty);
            let baseList = itemBaseList(
                typeList
                    .get(typeSelIndex.wrapping_sub(1))
                    .map(String::as_str),
            );
            let base = cx
                .add(
                    p,
                    "base",
                    DropDownControl::new(
                        Anchor::nil(TopLeft, TopLeft),
                        rect(55, 70, 200, 18),
                        baseList,
                        None,
                        None,
                        false,
                    ),
                )
                .id();
            // L2684
            cx.w_mut::<DropDownControl>(base).sel_index = lastBase.unwrap_or(1);
            // L2685
            cx.w_mut::<DropDownControl>(base).tt.tooltip_func =
                Some(Rc::new(move |cx, tooltip, (mode, _index, value)| {
                    // L2686
                    tooltip.clear(false);
                    // L2687
                    if mode != DropTipMode::Out {
                        // L2688
                        let item = makeItem(cx, p, value.as_ref());
                        add_item_tooltip(cx, tooltip, &item, true);
                    }
                }));
            // L2691
            cx.add(
                p,
                "save",
                ButtonControl::new(Anchor::NONE, rect(-45, 100, 80, 20), "Create", move |cx| {
                    // L2692
                    popups::close_popup(cx);
                    // L2693
                    let base = pc(cx, p, "base");
                    let value = {
                        let w = cx.w::<DropDownControl>(base);
                        w.sel_index
                            .checked_sub(1)
                            .and_then(|i| w.list.get(i))
                            .cloned()
                    };
                    let item = makeItem(cx, p, value.as_ref());
                    // L2694
                    super::display_item::SetDisplayItem(cx, me, Some(item.clone()));
                    // L2695
                    if !ext(&item, "crafted") && &*item.rarity.get() != "NORMAL" {
                        // L2696
                        EditDisplayItemText(cx, me, false);
                    }
                    // L2698-2700
                    let r = sel_index(cx, pc(cx, p, "rarity"));
                    let t = sel_index(cx, pc(cx, p, "type"));
                    let b = sel_index(cx, pc(cx, p, "base"));
                    let s = st_mut(cx, me);
                    s.lastCraftRaritySel = Some(r);
                    s.lastCraftTypeSel = Some(t);
                    s.lastCraftBaseSel = Some(b);
                }),
            );
            // L2702
            cx.add(
                p,
                "cancel",
                ButtonControl::new(Anchor::NONE, rect(45, 100, 80, 20), "Cancel", |cx| {
                    // L2703
                    popups::close_popup(cx);
                }),
            );
        },
        PopupOpts::default(),
    );
}

// ---- L2709-2761 EditDisplayItemText ---------------------------------------------------------------

/// `buildRaw()` (L2711-2718), a closure over the popup's `controls`.
fn buildRaw(cx: &Cx, p: CtrlId) -> String {
    // L2712
    let editBuf = cx.w::<EditControl>(pc(cx, p, "edit")).buf.clone();
    // L2713
    if pob_lua::lmatch(&editBuf, "^Item Class: .*\nRarity: ", None).is_some()
        || pob_lua::lmatch(&editBuf, "^Rarity: ", None).is_some()
    {
        // L2714
        editBuf
    } else {
        // L2716
        let rarity = pc(cx, p, "rarity");
        let r = DropDownControl::get_sel_value(cx, rarity)
            .and_then(|v| v.get::<RarityValue>().copied())
            .unwrap_or_else(|| lua_error!("attempt to index field '?' (a nil value)"));
        format!("Rarity: {}\n{editBuf}", r.0)
    }
}

/// lua: ItemsTab.lua:2709-2761 `ItemsTabClass:EditDisplayItemText(alsoAddItem)`: opens the item
/// text editor popup.
/// @pob Classes/ItemsTab.lua:2708-2761 h=1552ade3
pub fn EditDisplayItemText(cx: &mut Cx, me: CtrlId, alsoAddItem: bool) {
    // L2710 local controls = { }; L2711-2718 buildRaw: [`buildRaw`]
    let rarityList = ctor::rarityDropList(cx);
    let has_item = display_item(cx, me).is_some();
    // L2760
    let title = if has_item {
        "Edit Item Text"
    } else {
        "Create Custom Item from Text"
    };
    popups::open_popup(
        cx,
        500.0,
        500.0,
        title,
        move |cx, p| {
            let p = p.id();
            // L2719
            let rarity = cx
                .add(
                    p,
                    "rarity",
                    DropDownControl::new(
                        Anchor::NONE,
                        rect(-190, 10, 100, 18),
                        rarityList,
                        None,
                        None,
                        false,
                    ),
                )
                .id();
            // L2720
            let edit = cx
                .add(
                    p,
                    "edit",
                    EditControl::new(
                        Anchor::NONE,
                        rect(0, 40, 480, 420),
                        EditArgs {
                            init: Some(String::new()),
                            filter: Some("^%C\t\n".to_owned()),
                            line_height: Some(14.0),
                            ..EditArgs::default()
                        },
                    ),
                )
                .id();
            // L2721
            if let Some(item) = display_item(cx, me) {
                // L2722 (`SetText(s:gsub(..))` also passes gsub's count, a truthy `notify`;
                // the control has no changeFunc)
                let text = pob_ui_core::luastr::gsub(&item.BuildRaw(), "Rarity: %w+\n", "");
                EditControl::set_text(cx, edit, &text, true);
                // L2723
                let r = item.rarity.get();
                DropDownControl::sel_by_value(cx, rarity, &|v| {
                    v.get::<RarityValue>().is_some_and(|x| *x.0 == *r)
                });
            } else {
                // L2725
                cx.w_mut::<DropDownControl>(rarity).sel_index = 3;
            }
            // L2727
            cx.w_mut::<EditControl>(edit).font = Font::Fixed;
            // L2728
            cx.w_mut::<EditControl>(edit).paste_filter =
                Some(Rc::new(|_cx: &mut Cx, text: &str| {
                    pob_lua::string::sanitiseText(text)
                }));
            // L2729
            let save = cx
                .add(
                    p,
                    "save",
                    ButtonControl::new(
                        Anchor::NONE,
                        rect(-45, 470, 80, 20),
                        if has_item { "Save" } else { "Create" },
                        move |cx| {
                            // L2730
                            let id = display_item(cx, me).and_then(|i| i.id.get());
                            // L2731
                            let raw = buildRaw(cx, p);
                            let normalise = display_item(cx, me).is_none();
                            super::display_item::CreateDisplayItemFromRaw(cx, me, &raw, normalise);
                            // L2732
                            di(cx, me).id.set(id);
                            // L2733
                            if alsoAddItem {
                                // L2734
                                let f = ctor::items_ui_hooks().add_display_item;
                                f(cx, me, false);
                            }
                            // L2736
                            popups::close_popup(cx);
                        },
                    )
                    .force_tooltip(true)
                    // L2738
                    .enabled(Prop::f(move |cx, _| {
                        // L2739
                        let item = crate::main_mode::new_item(Some(&buildRaw(cx, p)), None, false);
                        // L2740
                        item.base.get().is_some()
                    })),
                )
                .id();
            // L2742
            cx.w_mut::<ButtonControl>(save).tt.tooltip_func = Some(Rc::new(
                move |cx, tooltip, ()| {
                    // L2743
                    tooltip.clear(false);
                    // L2744
                    let item = crate::main_mode::new_item(Some(&buildRaw(cx, p)), None, false);
                    // L2745
                    if item.base.get().is_some() {
                        // L2746
                        add_item_tooltip(cx, tooltip, &item, true);
                    } else {
                        // L2748
                        tooltip.add_line(cx, 14.0, "The item is invalid.");
                        // L2749
                        tooltip.add_line(
                            cx,
                            14.0,
                            "Check that the item's title and base name are in the correct format.",
                        );
                        // L2750
                        tooltip.add_line(
                        cx,
                        14.0,
                        "For Rare and Unique items, the first 2 lines must be the title and base name. E.g.:",
                    );
                        // L2751
                        tooltip.add_line(cx, 14.0, "Abberath's Horn");
                        // L2752
                        tooltip.add_line(cx, 14.0, "Goat's Horn");
                        // L2753
                        tooltip.add_line(
                        cx,
                        14.0,
                        "For Normal and Magic items, the base name must be somewhere in the first line. E.g.:",
                    );
                        // L2754
                        tooltip.add_line(cx, 14.0, "Scholar's Platinum Kris of Joy");
                    }
                },
            ));
            // L2757
            cx.add(
                p,
                "cancel",
                ButtonControl::new(Anchor::NONE, rect(45, 470, 80, 20), "Cancel", |cx| {
                    // L2758
                    popups::close_popup(cx);
                }),
            );
        },
        PopupOpts {
            default: Some("edit".into()),
            ..PopupOpts::default()
        },
    );
}

// ---- L2764-2973 EnchantDisplayItem ----------------------------------------------------------------

/// An `enchantmentList` entry (`{ label = enchantment, line = enchantment }`, L2823) with the
/// fields the sort adds.
#[derive(Debug)]
pub struct EnchantEntry {
    pub label: String,
    pub line: String,
    /// `entry.defaultSortOrder`
    pub defaultSortOrder: Cell<Option<f64>>,
    /// `entry.sortValue`
    pub sortValue: Cell<Option<f64>>,
    /// `entry.sortValues[stat]` (`None`: the table is nil)
    pub sortValues: RefCell<Option<Vec<(Name, f64)>>>,
}

/// A `data.enchantmentSource` entry (`{ label, name }`; Data.lua).
#[derive(Clone, Debug)]
pub struct EnchantSource {
    pub label: String,
    pub name: Name,
}

/// `self.build.data.enchantmentSource`
fn enchantmentSource() -> Vec<EnchantSource> {
    data_seq(data_root("data.enchantmentSource"))
        .into_iter()
        .filter_map(|v| match v {
            gamedata::DataValue::Table(t) => Some(EnchantSource {
                label: data_str(t, "label").unwrap_or_default(),
                name: Name::from(data_str(t, "name")?.as_str()),
            }),
            _ => None,
        })
        .collect()
}

/// The table `list` of L2803/L2821: `enchantments[skill]` (a source → lines table) or
/// `enchantments` itself (keys are sources or skills).
enum EnchLookup<'a> {
    Slot(&'a gamedata::schema::item_data::EnchantmentSlot),
    Skill(&'a indexmap::IndexMap<Name, Vec<gamedata::AStr>>),
}

impl EnchLookup<'_> {
    /// `list[name]`: `None` is nil; a table value that is not a list of lines (a skill's source
    /// table, read through `ipairs`) yields no lines.
    fn get(&self, name: Name) -> Option<Vec<String>> {
        use gamedata::schema::item_data::EnchantmentList;
        match self {
            EnchLookup::Slot(s) => s.get(&name).map(|l| match l {
                EnchantmentList::Lines(v) => v.iter().map(|s| s.to_string()).collect(),
                EnchantmentList::BySource(_) => Vec::new(),
            }),
            EnchLookup::Skill(m) => m
                .get(&name)
                .map(|v| v.iter().map(|s| s.to_string()).collect()),
        }
    }
}

/// The locals of one EnchantDisplayItem popup that its closures share.
struct EnchState {
    me: CtrlId,
    /// `enchantments` (L2768)
    enchantments: gamedata::schema::item_data::EnchantmentSlotRef,
    /// `haveSkills` (L2769)
    haveSkills: bool,
    /// `skillList` (L2776)
    skillList: Vec<Name>,
    /// `skillsUsed` (L2777; a set)
    skillsUsed: Vec<Name>,
    /// `enchantmentSourceList` (L2800)
    enchantmentSourceList: Vec<EnchantSource>,
    /// `enchantmentList` (L2810)
    enchantmentList: Vec<Rc<EnchantEntry>>,
    /// `sortStats` (L2811)
    sortStats: indexmap::IndexMap<Name, &'static gamedata::schema::item_data::PowerStat>,
}

type Ench = Rc<RefCell<EnchState>>;

/// `list` of L2803/L2821: `haveSkills and enchantments[skillList[..]] or enchantments`.
fn ench_list(s: &EnchState, skillSel: usize) -> EnchLookup<'_> {
    use gamedata::schema::item_data::EnchantmentList;
    if s.haveSkills {
        if let Some(skill) = s.skillList.get(skillSel.wrapping_sub(1)) {
            match s.enchantments.get(skill) {
                Some(EnchantmentList::BySource(m)) => return EnchLookup::Skill(m),
                // a skill entry that is a list of lines: its `[source]` reads nil
                Some(EnchantmentList::Lines(_)) => {
                    static EMPTY: std::sync::OnceLock<
                        indexmap::IndexMap<Name, Vec<gamedata::AStr>>,
                    > = std::sync::OnceLock::new();
                    return EnchLookup::Skill(EMPTY.get_or_init(indexmap::IndexMap::new));
                }
                // `enchantments[nil-or-missing]` is nil: `or enchantments`
                None => {}
            }
        }
    }
    EnchLookup::Slot(&s.enchantments)
}

fn sync_skill(cx: &mut Cx, p: CtrlId, e: &Ench) {
    if let Some(c) = pc_opt(cx, p, "skill") {
        let list = e
            .borrow()
            .skillList
            .iter()
            .map(|s| DropItem::str(&**s))
            .collect();
        cx.w_mut::<DropDownControl>(c).list = list;
    }
}

fn source_items(s: &EnchState) -> Vec<DropItem> {
    s.enchantmentSourceList
        .iter()
        .map(|x| DropItem::new(x.label.clone(), Rc::new(x.clone())))
        .collect()
}

fn sync_source(cx: &mut Cx, p: CtrlId, e: &Ench) {
    if let Some(c) = pc_opt(cx, p, "enchantmentSource") {
        let list = source_items(&e.borrow());
        cx.w_mut::<DropDownControl>(c).list = list;
    }
}

fn enchantment_items(s: &EnchState) -> Vec<DropItem> {
    s.enchantmentList
        .iter()
        .map(|x| DropItem::new(x.label.clone(), x.clone()))
        .collect()
}

fn sync_enchantment(cx: &mut Cx, p: CtrlId, e: &Ench) {
    if let Some(c) = pc_opt(cx, p, "enchantment") {
        let list = enchantment_items(&e.borrow());
        cx.w_mut::<DropDownControl>(c).list = list;
    }
}

/// `buildSkillList(onlyUsedSkills)` (L2791-2799).
fn buildSkillList(cx: &mut Cx, p: CtrlId, e: &Ench, onlyUsedSkills: bool) {
    {
        let mut s = e.borrow_mut();
        // L2792
        s.skillList.clear();
        // L2793 (ORDER: sorted below)
        let names: Vec<Name> = s.enchantments.keys().copied().collect();
        for skillName in names {
            // L2794
            if !onlyUsedSkills || s.skillsUsed.is_empty() || s.skillsUsed.contains(&skillName) {
                // L2795
                s.skillList.push(skillName);
            }
        }
        // L2798
        pob_lua::sort(&mut s.skillList, |a, b| a.as_bytes() < b.as_bytes());
    }
    sync_skill(cx, p, e);
}

/// `buildEnchantmentSourceList()` (L2801-2809).
fn buildEnchantmentSourceList(cx: &mut Cx, p: CtrlId, e: &Ench) {
    let skillSel = pc_opt(cx, p, "skill").map_or(1, |c| sel_index(cx, c));
    let sources = enchantmentSource();
    {
        let mut s = e.borrow_mut();
        // L2802
        s.enchantmentSourceList.clear();
        // L2803
        let found: Vec<EnchantSource> = {
            let list = ench_list(&s, skillSel);
            // L2804
            sources
                .into_iter()
                // L2805
                .filter(|source| list.get(source.name).is_some())
                .collect()
        };
        // L2806
        s.enchantmentSourceList = found;
    }
    sync_source(cx, p, e);
}

/// `setDefaultSortOrder()` (L2812-2818).
fn setDefaultSortOrder(s: &EnchState) {
    // L2813
    for (index, entry) in s.enchantmentList.iter().enumerate() {
        // L2814
        entry.defaultSortOrder.set(Some((index + 1) as f64));
        // L2815
        entry.sortValue.set(None);
        // L2816
        *entry.sortValues.borrow_mut() = None;
    }
}

/// `buildEnchantmentList()` (L2819-2826).
fn buildEnchantmentList(cx: &mut Cx, p: CtrlId, e: &Ench) {
    let skillSel = pc_opt(cx, p, "skill").map_or(1, |c| sel_index(cx, c));
    let sourceSel = pc_opt(cx, p, "enchantmentSource").map_or(1, |c| sel_index(cx, c));
    {
        let mut s = e.borrow_mut();
        // L2820
        s.enchantmentList.clear();
        // L2821-2822
        let sourceName = s
            .enchantmentSourceList
            .get(sourceSel.wrapping_sub(1))
            .map(|x| x.name)
            .unwrap_or_else(|| lua_error!("attempt to index field '?' (a nil value)"));
        let lines = ench_list(&s, skillSel)
            .get(sourceName)
            .unwrap_or_else(|| lua_error!("bad argument #1 to 'ipairs' (table expected, got nil)"));
        for enchantment in lines {
            // L2823
            s.enchantmentList.push(Rc::new(EnchantEntry {
                label: enchantment.clone(),
                line: enchantment,
                defaultSortOrder: Cell::new(None),
                sortValue: Cell::new(None),
                sortValues: RefCell::new(None),
            }));
        }
        // L2825
        setDefaultSortOrder(&s);
    }
    sync_enchantment(cx, p, e);
}

/// The enchant step shared by `enchantItem` (L2840-2852) and `getSortValue` (L2864-2876):
/// replaces enchant slot `enchantSlot` of `item` with `line`.
fn apply_enchant_line(item: &model::ItemRef, line: &str, enchantSlot: f64) {
    // L2841 / L2865
    let caps = pob_lua::lmatch(line, "([^/]+)/([^/]+)", None);
    // L2842 / L2866
    if let Some(caps) = caps {
        let first = caps[0].to_str().into_owned();
        let second = caps[1].to_str().into_owned();
        // L2843 / L2867
        item.enchantModLines.set(model::Shared::new(vec![
            crafted_line(&first),
            crafted_line(&second),
        ]));
    } else {
        // L2845 / L2869
        if !ext(item, "canHaveTwoEnchants") && item.enchantModLines.get().len() > 1 {
            // L2846 / L2870
            let first = item.enchantModLines.get().get(1);
            item.enchantModLines
                .set(model::Shared::new(first.into_iter().collect()));
        }
        // L2848 / L2872
        let lines = item.enchantModLines.get();
        if lines.len() as f64 >= enchantSlot {
            // L2849 / L2873
            lines.remove(Some(enchantSlot as usize));
        }
        // L2851 / L2875
        item.enchantModLines
            .get()
            .insert(enchantSlot as usize, crafted_line(line));
    }
}

/// `self.enchantSlot` (a number once EnchantDisplayItem ran; nil raises in arithmetic).
#[track_caller]
fn enchant_slot(cx: &Cx, me: CtrlId) -> f64 {
    st(cx, me)
        .enchantSlot
        .unwrap_or_else(|| lua_error!("attempt to compare nil with number"))
}

/// `enchantItem(idx, remove)` (L2832-2856).
fn enchantItem(cx: &Cx, p: CtrlId, e: &Ench, idx: Option<usize>, remove: bool) -> model::ItemRef {
    let me = e.borrow().me;
    let displayItem = di(cx, me);
    // L2833
    let item = crate::main_mode::new_item(Some(&displayItem.BuildRaw()), None, false);
    // L2834
    let index = idx.unwrap_or_else(|| sel_index(cx, pc(cx, p, "enchantment")));
    // L2835
    item.id.set(displayItem.id.get());
    // L2836
    let entry = e
        .borrow()
        .enchantmentList
        .get(index.wrapping_sub(1))
        .cloned();
    let enchantSlot = enchant_slot(cx, me);
    // L2837
    if remove {
        // L2838
        item.enchantModLines
            .get()
            .remove(Some(enchantSlot as usize));
    } else if let Some(entry) = entry {
        // L2839-2853
        apply_enchant_line(&item, &entry.line, enchantSlot);
    }
    // L2854
    item.BuildAndParseRaw();
    // L2855
    item
}

/// `getSortValue(entry, stat, calcFunc, slotName, useFullDPS)` (L2857-2882).
fn getSortValue(
    cx: &Cx,
    e: &Ench,
    entry: &EnchantEntry,
    stat: Name,
    calcFunc: Option<&Rc<dyn MiscCalc>>,
    slotName: Name,
    useFullDPS: bool,
) -> f64 {
    // L2858
    if entry.sortValues.borrow().is_none() {
        *entry.sortValues.borrow_mut() = Some(Vec::new());
    }
    // L2859
    let cached = entry
        .sortValues
        .borrow()
        .as_ref()
        .and_then(|v| v.iter().find(|(k, _)| *k == stat).map(|(_, x)| *x));
    if let Some(v) = cached {
        // L2860
        return v;
    }
    let me = e.borrow().me;
    let displayItem = di(cx, me);
    // L2862
    let item = crate::main_mode::new_item(Some(&displayItem.BuildRaw()), None, false);
    // L2863
    item.id.set(displayItem.id.get());
    // L2864-2876
    apply_enchant_line(&item, &entry.line, enchant_slot(cx, me));
    // L2877
    item.BuildAndParseRaw();
    // L2878
    let output = calcFunc.and_then(|f| calc_rep(f, slotName, &item, Some(useFullDPS)));
    // L2879
    let statEntry = e.borrow().sortStats.get(&stat).copied();
    let value = match (output, statEntry) {
        (Some(o), Some(s)) => pob_calc::modules::power::GetFromOutput(
            &o,
            &pob_calc::modules::power::powerStat_table(s),
            false,
        ),
        (Some(_), None) => lua_error!("attempt to index local 'statTable' (a nil value)"),
        // DIVERGENCE(pending-calc)
        (None, _) => 0.0,
    };
    // L2880
    entry
        .sortValues
        .borrow_mut()
        .get_or_insert_with(Vec::new)
        .push((stat, value));
    // L2881
    value
}

/// `(a.defaultSortOrder or 0) < (b.defaultSortOrder or 0)`
fn by_default(a: &EnchantEntry, b: &EnchantEntry) -> bool {
    a.defaultSortOrder.get().unwrap_or(0.0) < b.defaultSortOrder.get().unwrap_or(0.0)
}

/// `a.sortValue` in a comparison (nil raises).
#[track_caller]
fn sort_value(v: Option<f64>) -> f64 {
    v.unwrap_or_else(|| lua_error!("attempt to compare nil with number"))
}

/// `applySort(stat, selectFirst)` (L2883-2917).
fn applySort(cx: &mut Cx, p: CtrlId, e: &Ench, stat: Option<Name>, selectFirst: bool) {
    // L2884
    let Some(enchantment) = pc_opt(cx, p, "enchantment").filter(|c| cx.is_shown(*c)) else {
        // L2885
        return;
    };
    // L2887
    let selected = if selectFirst {
        None
    } else {
        e.borrow()
            .enchantmentList
            .get(sel_index(cx, enchantment).wrapping_sub(1))
            .cloned()
    };
    // L2888
    if let Some(stat) = stat {
        let me = e.borrow().me;
        // L2889
        let slotName = di(cx, me).GetPrimarySlot();
        // L2890
        let calcFunc = misc_calc(cx).map(|(f, _)| f);
        // L2891
        let useFullDPS = &*stat == "FullDPS";
        // L2892
        let list = e.borrow().enchantmentList.clone();
        for entry in &list {
            // L2893
            let v = getSortValue(cx, e, entry, stat, calcFunc.as_ref(), slotName, useFullDPS);
            entry.sortValue.set(Some(v));
        }
        // L2895
        pob_lua::sort(&mut e.borrow_mut().enchantmentList, |a, b| {
            // L2896
            if a.sortValue.get() != b.sortValue.get() {
                // L2897
                return sort_value(a.sortValue.get()) > sort_value(b.sortValue.get());
            }
            // L2899
            by_default(a, b)
        });
    } else {
        // L2902
        pob_lua::sort(&mut e.borrow_mut().enchantmentList, |a, b| by_default(a, b));
    }
    sync_enchantment(cx, p, e);
    // L2906
    update_search(cx, enchantment);
    // L2907
    if let Some(selected) = selected {
        // L2908
        let found = e
            .borrow()
            .enchantmentList
            .iter()
            .position(|x| Rc::ptr_eq(x, &selected));
        if let Some(index) = found {
            // L2910
            cx.w_mut::<DropDownControl>(enchantment).sel_index = index + 1;
        }
    } else {
        // L2915
        DropDownControl::set_sel(cx, enchantment, 1, true);
    }
}

/// `controls.sort.list[controls.sort.selIndex].stat` (the sort dropdown of a popup).
fn sort_stat(cx: &Cx, sort: CtrlId) -> Option<Name> {
    DropDownControl::get_sel_value(cx, sort)
        .unwrap_or_else(|| lua_error!("attempt to index field '?' (a nil value)"))
        .get::<PowerSortValue>()
        .and_then(PowerSortValue::stat)
}

/// lua: ItemsTab.lua:2764-2973 `ItemsTabClass:EnchantDisplayItem(enchantSlot)`: opens the item
/// enchanting popup.
/// @pob Classes/ItemsTab.lua:2763-2973 h=7e697902
pub fn EnchantDisplayItem(cx: &mut Cx, me: CtrlId, enchantSlot: Option<f64>) {
    // L2765
    st_mut(cx, me).enchantSlot = Some(enchantSlot.unwrap_or(1.0));

    // L2767 local controls = { }
    // L2768
    let enchantments = di(cx, me)
        .enchantments_ref()
        .unwrap_or_else(|| lua_error!("attempt to index local 'enchantments' (a nil value)"));
    // L2769
    let mut haveSkills = true;
    // L2770
    for source in enchantmentSource() {
        // L2771
        if enchantments.contains_key(&source.name) {
            // L2772
            haveSkills = false;
            // L2773
            break;
        }
    }
    // L2776-2777
    let mut skillsUsed: Vec<Name> = Vec::new();
    // L2778
    if haveSkills {
        let groups = cx.build().model.skillsTab.socketGroupList.get().snapshot();
        // L2779
        for socketGroup in groups {
            // L2780
            for gemInstance in socketGroup.gemList.get().snapshot() {
                // L2781
                if let Some(gemData) = gemInstance.gemData.get() {
                    // L2782
                    for grantedEffect in &gemData.grantedEffectList {
                        // L2783
                        if !grantedEffect.support && enchantments.contains_key(&grantedEffect.name)
                        {
                            // L2784
                            if !skillsUsed.contains(&grantedEffect.name) {
                                skillsUsed.push(grantedEffect.name);
                            }
                        }
                    }
                }
            }
        }
    }
    // L2811
    let (sortList, sortStats) = ctor::buildModSortList();
    let e: Ench = Rc::new(RefCell::new(EnchState {
        me,
        enchantments,
        haveSkills,
        skillList: Vec::new(),
        skillsUsed,
        enchantmentSourceList: Vec::new(),
        enchantmentList: Vec::new(),
        sortStats,
    }));

    // L2972
    popups::open_popup(
        cx,
        605.0,
        130.0,
        "Enchant Item",
        move |cx, p| {
            let p = p.id();
            // L2827 (the controls do not exist yet: `controls.skill` etc. read nil)
            if haveSkills {
                // L2828
                buildSkillList(cx, p, &e, true);
            }
            // L2830
            buildEnchantmentSourceList(cx, p, &e);
            // L2831
            buildEnchantmentList(cx, p, &e);
            // L2918
            if haveSkills {
                // L2919
                cx.add(
                    p,
                    "skillLabel",
                    LabelControl::new(
                        Anchor::nil(TopRight, TopLeft),
                        rect(95, 20, 0, 16),
                        "^7Skill:",
                    ),
                );
                // L2920
                let e2 = e.clone();
                let skillSel: SelFunc = Rc::new(move |cx, _index, _value| {
                    // L2921
                    buildEnchantmentSourceList(cx, p, &e2);
                    // L2922
                    buildEnchantmentList(cx, p, &e2);
                    // L2923
                    DropDownControl::set_sel(cx, pc(cx, p, "enchantment"), 1, false);
                    // L2924
                    if let Some(sort) = pc_opt(cx, p, "sort") {
                        // L2925
                        let stat = sort_stat(cx, sort);
                        applySort(cx, p, &e2, stat, true);
                    }
                });
                let list = e
                    .borrow()
                    .skillList
                    .iter()
                    .map(|s| DropItem::str(&**s))
                    .collect();
                cx.add(
                    p,
                    "skill",
                    DropDownControl::new(
                        Anchor::nil(TopLeft, TopLeft),
                        rect(100, 20, 180, 18),
                        list,
                        Some(skillSel),
                        None,
                        false,
                    ),
                );
                // L2928
                let e2 = e.clone();
                let allSkills = cx
                    .add(
                        p,
                        "allSkills",
                        CheckBoxControl::new(
                            Anchor::nil(TopLeft, TopLeft),
                            rect(350, 20, 18, 18),
                            "All skills:",
                            Some(Rc::new(move |cx: &mut Cx, state: bool| {
                                // L2929
                                buildSkillList(cx, p, &e2, !state);
                                // L2930
                                DropDownControl::set_sel(cx, pc(cx, p, "skill"), 1, false);
                                // L2931
                                buildEnchantmentList(cx, p, &e2);
                                // L2932
                                DropDownControl::set_sel(cx, pc(cx, p, "enchantment"), 1, false);
                                // L2933
                                if let Some(sort) = pc_opt(cx, p, "sort") {
                                    // L2934
                                    let stat = sort_stat(cx, sort);
                                    applySort(cx, p, &e2, stat, true);
                                }
                            })),
                            None,
                            false,
                        ),
                    )
                    .id();
                // L2937
                cx.w_mut::<CheckBoxControl>(allSkills).tt.tooltip_text = Some(Prop::Val(Some(
                    "Show all skills, not just those used by this build.".to_owned(),
                )));
                // L2938
                if e.borrow().skillsUsed.is_empty() {
                    // L2939
                    cx.w_mut::<CheckBoxControl>(allSkills).state = true;
                    // L2940
                    cx.base_mut(allSkills).enabled = Prop::Val(false);
                }
            }
            // L2943
            cx.add(
                p,
                "enchantmentSourceLabel",
                LabelControl::new(
                    Anchor::nil(TopRight, TopLeft),
                    rect(95, 45, 0, 16),
                    "^7Source:",
                ),
            );
            // L2944
            let e2 = e.clone();
            let sourceSel: SelFunc = Rc::new(move |cx, _index, _value| {
                // L2945
                buildEnchantmentList(cx, p, &e2);
                // L2946
                let enchantment = pc(cx, p, "enchantment");
                let n = e2.borrow().enchantmentList.len() as f64;
                let sel = m_min(sel_index(cx, enchantment) as f64, n);
                DropDownControl::set_sel_num(cx, enchantment, sel, false);
                // L2947
                if let Some(sort) = pc_opt(cx, p, "sort") {
                    // L2948
                    let stat = sort_stat(cx, sort);
                    applySort(cx, p, &e2, stat, true);
                }
            });
            let list = source_items(&e.borrow());
            cx.add(
                p,
                "enchantmentSource",
                DropDownControl::new(
                    Anchor::nil(TopLeft, TopLeft),
                    rect(100, 45, 180, 18),
                    list,
                    Some(sourceSel),
                    None,
                    false,
                ),
            );
            // L2951
            cx.add(
                p,
                "sortLabel",
                LabelControl::new(
                    Anchor::nil(TopRight, TopLeft),
                    rect(350, 45, 0, 16),
                    "^7Sort by:",
                ),
            );
            // L2952
            let e2 = e.clone();
            let sortSel: SelFunc = Rc::new(move |cx, _index, value| {
                // L2953
                let stat = value
                    .unwrap_or_else(|| lua_error!("attempt to index local 'value' (a nil value)"))
                    .get::<PowerSortValue>()
                    .and_then(PowerSortValue::stat);
                applySort(cx, p, &e2, stat, true);
            });
            cx.add(
                p,
                "sort",
                DropDownControl::new(
                    Anchor::nil(TopLeft, TopLeft),
                    rect(355, 45, 240, 18),
                    sortList,
                    Some(sortSel),
                    None,
                    false,
                ),
            );
            // L2955
            cx.add(
                p,
                "enchantmentLabel",
                LabelControl::new(
                    Anchor::nil(TopRight, TopLeft),
                    rect(95, 70, 0, 16),
                    "^7Enchantment:",
                ),
            );
            // L2956
            let list = enchantment_items(&e.borrow());
            let enchantment = cx
                .add(
                    p,
                    "enchantment",
                    DropDownControl::new(
                        Anchor::nil(TopLeft, TopLeft),
                        rect(100, 70, 495, 18),
                        list,
                        None,
                        None,
                        false,
                    ),
                )
                .id();
            // L2957
            let e2 = e.clone();
            cx.w_mut::<DropDownControl>(enchantment).tt.tooltip_func =
                Some(Rc::new(move |cx, tooltip, (_mode, index, _value)| {
                    // L2958
                    tooltip.clear(false);
                    // L2959
                    let item = enchantItem(cx, p, &e2, index, false);
                    add_item_tooltip(cx, tooltip, &item, true);
                }));
            // L2961
            let e2 = e.clone();
            cx.add(
                p,
                "save",
                ButtonControl::new(Anchor::NONE, rect(-88, 100, 80, 20), "Enchant", move |cx| {
                    // L2962
                    let item = enchantItem(cx, p, &e2, None, false);
                    super::display_item::SetDisplayItem(cx, me, Some(item));
                    // L2963
                    popups::close_popup(cx);
                }),
            );
            // L2965
            let e2 = e.clone();
            cx.add(
                p,
                "remove",
                ButtonControl::new(Anchor::NONE, rect(0, 100, 80, 20), "Remove", move |cx| {
                    // L2966
                    let item = enchantItem(cx, p, &e2, None, true);
                    super::display_item::SetDisplayItem(cx, me, Some(item));
                    // L2967
                    popups::close_popup(cx);
                }),
            );
            // L2969
            cx.add(
                p,
                "close",
                ButtonControl::new(Anchor::NONE, rect(88, 100, 80, 20), "Cancel", |cx| {
                    // L2970
                    popups::close_popup(cx);
                }),
            );
        },
        PopupOpts::default(),
    );
}

// ---- L2978-3005 getAnoint / getMissingAnointCount: calc:BM-A's (model::ItemsTab) --------------

/// `self:getAnoint(item)` (calc:BM-A's `model::ItemsTab::getAnoint`, ItemsTab.lua:2978-2993).
fn getAnoint(cx: &Cx, item: Option<&model::ItemRef>) -> Vec<String> {
    cx.build().model.itemsTab.getAnoint(item)
}

// ---- L3011-3118 the anoint helpers and AnointDisplayItem ------------------------------------------

/// lua: ItemsTab.lua:3011-3023 `ItemsTabClass:anointItem(node)`: a copy of the display item,
/// anointed with `node` (nil: the existing anoint of the slot removed).
/// Removes any existing enchantments before anointing. (Anoints are considered enchantments)
/// @pob Classes/ItemsTab.lua:3007-3023 h=2c320b2d
pub fn anointItem(cx: &mut Cx, me: CtrlId, node: Option<&model::NodeRef>) -> model::ItemRef {
    // L3012
    let slot = st(cx, me).anointEnchantSlot.unwrap_or(1.0);
    st_mut(cx, me).anointEnchantSlot = Some(slot);
    let displayItem = di(cx, me);
    // L3013
    let item = crate::main_mode::new_item(Some(&displayItem.BuildRaw()), None, false);
    // L3014
    item.id.set(displayItem.id.get());
    // L3015
    if item.enchantModLines.get().len() as f64 >= slot {
        // L3016
        item.enchantModLines.get().remove(Some(slot as usize));
    }
    // L3018
    if let Some(node) = node {
        // L3019
        item.enchantModLines.get().insert(
            slot as usize,
            crafted_line(&format!("Allocates {}", node.dn.get())),
        );
    }
    // L3021
    item.BuildAndParseRaw();
    // L3022
    item
}

/// lua: ItemsTab.lua:3028-3066 `ItemsTabClass:AppendAnointTooltip(tooltip, node, actionText)`:
/// appends the stat changes of anointing `node` (nil: removing the current anoint) onto the
/// display item.
/// @pob Classes/ItemsTab.lua:3025-3066 h=66321769
pub fn AppendAnointTooltip(
    cx: &mut Cx,
    me: CtrlId,
    tooltip: &mut Tooltip,
    node: Option<&model::NodeRef>,
    actionText: Option<&str>,
) {
    // L3029
    let Some(displayItem) = display_item(cx, me) else {
        // L3030
        return;
    };

    // L3033-3035
    let actionText = actionText.unwrap_or("Anointing");

    // L3037
    let header;
    // L3038
    if let Some(node) = node {
        let dn = node.dn.get();
        // L3039
        let spec = cx.build().model.spec.get();
        if spec.allocNodes.with(|m| m.contains_key(node.id as i64)) {
            // L3040
            tooltip.add_line(
                cx,
                14.0,
                &format!("^7{actionText} {dn} changes nothing because this node is already allocated on the tree."),
            );
            // L3041
            return;
        }

        // L3044
        let curAnoints = getAnoint(cx, Some(&displayItem));
        // L3045
        if !curAnoints.is_empty() {
            // L3046
            for curAnoint in &curAnoints {
                // L3047
                if **curAnoint == *dn {
                    // L3048
                    tooltip.add_line(
                        cx,
                        14.0,
                        &format!("^7{actionText} {dn} changes nothing because this node is already anointed."),
                    );
                    // L3049
                    return;
                }
            }
        }

        // L3054
        header = format!("^7{actionText} {dn} will give you: ");
    } else {
        // L3056
        header = format!("^7{actionText} nothing will give you: ");
    }
    // L3058 DIVERGENCE(pending-calc)
    let Some((calcFunc, _)) = misc_calc(cx) else {
        return;
    };
    // L3059
    let repSlotName = displayItem
        .base
        .get()
        .map_or_else(|| Name::from("Amulet"), |b| b.r#type);
    // L3060
    let outputBase = calc_rep(&calcFunc, repSlotName, &displayItem, None);
    // L3061
    let newItem = anointItem(cx, me, node);
    let outputNew = calc_rep(&calcFunc, repSlotName, &newItem, None);
    let (Some(outputBase), Some(outputNew)) = (outputBase, outputNew) else {
        return;
    };
    // L3062
    let numChanges = crate::build::stats::add_stat_compares_to_tooltip(
        cx,
        tooltip,
        &outputBase,
        &outputNew,
        &header,
        None,
    );
    // L3063
    if let Some(node) = node.filter(|_| numChanges == 0.0) {
        // L3064
        tooltip.add_line(
            cx,
            14.0,
            &format!("^7{actionText} {} changes nothing.", node.dn.get()),
        );
    }
}

/// lua: ItemsTab.lua:3071-3078 `ItemsTabClass:AppendAddedNotableTooltip(tooltip, node)`:
/// appends the stat changes of allocating `node`.
/// @pob Classes/ItemsTab.lua:3068-3078 h=a7ddb089
pub fn AppendAddedNotableTooltip(
    cx: &mut Cx,
    _me: CtrlId,
    tooltip: &mut Tooltip,
    node: &model::NodeRef,
) {
    // L3072 DIVERGENCE(pending-calc)
    let Some((calcFunc, calcBase)) = misc_calc(cx) else {
        return;
    };
    // L3073 `{ addNodes = { [node] = true } }`
    let set = model::Shared::new(model::LjIntMap::new());
    set.with_mut(|m| m.set(node.id as i64, Some(node.clone())));
    let ov = model::Override::default();
    ov.addNodes.set(Some(set));
    let outputNew = match calcFunc.call(Some(Rc::new(ov)), None) {
        std::task::Poll::Ready(o) => o,
        // DIVERGENCE(pending-calc)
        std::task::Poll::Pending => return,
    };
    let dn = node.dn.get();
    // L3074
    let numChanges = crate::build::stats::add_stat_compares_to_tooltip(
        cx,
        tooltip,
        &calcBase,
        &outputNew,
        &format!("^7Allocating {dn} will give you: "),
        None,
    );
    // L3075
    if numChanges == 0.0 {
        // L3076
        tooltip.add_line(cx, 14.0, &format!("^7Allocating {dn} changes nothing."));
    }
}

/// A `calcFunc` that answers Pending (the facade had no calculator yet; wasm only).
struct PendingCalc;

impl MiscCalc for PendingCalc {
    fn call(
        &self,
        _ov: Option<Rc<model::Override>>,
        _use_full_dps: Option<bool>,
    ) -> std::task::Poll<model::Output> {
        // DIVERGENCE(pending-calc)
        std::task::Poll::Pending
    }
}

/// The ItemsTab as the anoint popup's NotableDBControl sees it (`self.itemsTab`, CCR-0112).
struct AnointItemsTab {
    me: CtrlId,
}

impl NotableItemsTab for AnointItemsTab {
    fn build_output_revision(&self, cx: &Cx) -> f64 {
        cx.build().model.outputRevision.get()
    }
    fn get_misc_calculator(&self, cx: &mut Cx) -> Rc<dyn MiscCalc> {
        misc_calc(cx).map_or_else(|| Rc::new(PendingCalc) as Rc<dyn MiscCalc>, |(f, _)| f)
    }
    fn display_item(&self, cx: &Cx) -> Option<model::ItemRef> {
        display_item(cx, self.me)
    }
    fn anoint_item(&self, cx: &mut Cx, node: Option<&model::NodeRef>) -> model::ItemRef {
        anointItem(cx, self.me, node)
    }
    fn add_node_name(&self, cx: &mut Cx, tooltip: &mut Tooltip, node: &model::NodeRef) {
        // `itemsTab.socketViewer:AddNodeName(tooltip, node, itemsTab.build)` (U24, ported:
        // called directly)
        cx.lend::<ItemsTab, _, _>(
            self.me,
            |w| &mut w.ctor.socketViewer,
            |cx, viewer| viewer.add_node_name(cx, tooltip, node),
        );
    }
    fn append_anoint_tooltip(&self, cx: &mut Cx, tooltip: &mut Tooltip, node: &model::NodeRef) {
        AppendAnointTooltip(cx, self.me, tooltip, Some(node), None);
    }
}

/// `controls.notableDB.selValue` (a node, or nil).
fn notable_sel(cx: &Cx, db: CtrlId) -> Option<model::NodeRef> {
    cx.w::<ListControl>(db)
        .sel_value
        .as_ref()
        .and_then(|v| v.rc::<model::Node>())
}

/// `saveLabel()` (L3087-3097).
fn saveLabel(cx: &Cx, me: CtrlId, p: CtrlId) -> String {
    // L3088
    let node = notable_sel(cx, pc(cx, p, "notableDB"));
    // L3089
    if let Some(node) = node {
        // L3090
        return format!("Anoint {}", node.dn.get());
    }
    // L3092
    let curAnoints = getAnoint(cx, display_item(cx, me).as_ref());
    // L3093
    let slot = st(cx, me)
        .anointEnchantSlot
        .unwrap_or_else(|| lua_error!("attempt to compare number with nil"));
    if curAnoints.len() as f64 >= slot {
        // L3094
        let i = slot as usize;
        return format!(
            "Remove {}",
            curAnoints
                .get(i.wrapping_sub(1))
                .unwrap_or_else(|| lua_error!("attempt to concatenate a nil value"))
        );
    }
    // L3096
    "No Anoint".to_owned()
}

/// `saveLabelWidth()` (L3098-3101).
fn saveLabelWidth(cx: &Cx, me: CtrlId, p: CtrlId) -> f64 {
    // L3099
    let label = saveLabel(cx, me, p);
    // L3100
    cx.sg.draw_string_width(16.0, Font::Var, &label) + 10.0
}

/// `saveLabelX()` (L3102-3105).
fn saveLabelX(cx: &Cx, me: CtrlId, p: CtrlId) -> f64 {
    // L3103
    let width = saveLabelWidth(cx, me, p);
    // L3104
    -(width + 90.0) / 2.0
}

/// lua: ItemsTab.lua:3081-3118 `ItemsTabClass:AnointDisplayItem(enchantSlot)`: opens the item
/// anointing popup.
/// @pob Classes/ItemsTab.lua:3080-3118 h=b48f9609
pub fn AnointDisplayItem(cx: &mut Cx, me: CtrlId, enchantSlot: Option<f64>) {
    // L3082
    st_mut(cx, me).anointEnchantSlot = Some(enchantSlot.unwrap_or(1.0));

    // L3084 local controls = { }
    let nodes = cx.build().model.spec.get().tree.get().nodes.clone();
    // L3117
    popups::open_popup(
        cx,
        380.0,
        448.0,
        "Anoint Item",
        move |cx, p| {
            let p = p.id();
            // L3085
            cx.add(
                p,
                "notableDB",
                NotableDBControl::new(
                    Anchor::nil(TopLeft, TopLeft),
                    rect(10, 60, 360, 360),
                    Rc::new(AnointItemsTab { me }),
                    nodes,
                    Some("ANOINT"),
                ),
            );
            // L3087-3105 saveLabel, saveLabelWidth, saveLabelX: [`saveLabel`] ...
            // L3106
            let save = cx
                .add(
                    p,
                    "save",
                    ButtonControl::new(
                        Anchor::nil(BottomLeft, Bottom),
                        [
                            Prop::f(move |cx, _| saveLabelX(cx, me, p)),
                            Prop::Val(-4.0),
                            Prop::f(move |cx, _| saveLabelWidth(cx, me, p)),
                            Prop::Val(20.0),
                        ],
                        Prop::f(move |cx, _| saveLabel(cx, me, p)),
                        move |cx| {
                            // L3107
                            let node = notable_sel(cx, pc(cx, p, "notableDB"));
                            let item = anointItem(cx, me, node.as_ref());
                            super::display_item::SetDisplayItem(cx, me, Some(item));
                            // L3108
                            popups::close_popup(cx);
                        },
                    ),
                )
                .id();
            // L3110
            cx.w_mut::<ButtonControl>(save).tt.tooltip_func =
                Some(Rc::new(move |cx, tooltip, ()| {
                    // L3111
                    tooltip.clear(false);
                    // L3112
                    let node = notable_sel(cx, pc(cx, p, "notableDB"));
                    AppendAnointTooltip(cx, me, tooltip, node.as_ref(), None);
                }));
            // L3114
            cx.add(
                p,
                "close",
                ButtonControl::new(
                    Anchor::new(TopLeft, save, TopRight),
                    rect(10, 0, 80, 20),
                    "Cancel",
                    |cx| {
                        // L3115
                        popups::close_popup(cx);
                    },
                ),
            );
        },
        PopupOpts::default(),
    );
}
