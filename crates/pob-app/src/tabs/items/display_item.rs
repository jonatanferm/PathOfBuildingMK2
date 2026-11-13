//! Port of PathOfBuilding v2.67.2 (16de4b82d) src/Classes/ItemsTab.lua:1948-2608 (the display
//! item: CreateDisplayItemFromRaw, the variant controls, SetDisplayItem, the tooltip, the mod line
//! toggle, the socket/cluster/affix/custom/range controls, AddModComparisonTooltip and the
//! equipped-slot lookups). Owner: U35.
//!
//! @pob-file Classes/ItemsTab.lua @16de4b82d
//!
//! The functions are free functions over the ItemsTab node `me` (`self`), named as the Lua
//! methods. They read and write the shared ItemsTab fields of U33's
//! [`ctor::State`](super::ctor::State) (`displayItem`, `snapHScroll`, `displayItemTooltip`, the
//! variant and affix dropdowns' extra fields; CCR-0120 §2). The constructor's closures (U33) and
//! U32's controls reach them through the hook tables: U35's entries of
//! [`default_items_ui_hooks`](super::ctor::default_items_ui_hooks) and
//! [`default_items_hooks`](super::slot_helper::default_items_hooks) are these functions.
//! The methods of other units that these functions call go through the same tables
//! (`AddItemTooltip`, U38); `CopyAnointsAndEldritchImplicits` (U34) is called directly
//! ([`core::CopyAnointsAndEldritchImplicits`](super::core::CopyAnointsAndEldritchImplicits),
//! CCR-0124 §1 as applied).
//!
//! Lua semantics:
//! * `control.list = t` (a direct field store, no CheckDroppedWidth) is `w.list = ..`; the Lua
//!   list of a legacy variant dropdown *is* `item.variantList` (an alias); the port copies it.
//!   Nothing writes either table while both are reachable (the Item re-creates `variantList` on
//!   every parse), so the alias is unobservable.
//! * A dropdown's nil `selIndex` (`item.variant` of an item without variants) is `sel_index = 0`
//!   and a nil `list` is an empty list: SetDisplayItem stores them only into dropdowns that are
//!   hidden for such an item (`variantList and #variantList > 1`, `hasAltVariant*`), and the next
//!   SetDisplayItem of an item that shows them stores real values first. DIVERGENCE(nil-list):
//!   `CheckDroppedWidth(true)` on a nil list does nothing in PoB; on the empty list it sets
//!   `droppedWidth` to the box width, which no hidden dropdown reads, and every later list store
//!   is followed by its own CheckDroppedWidth.
//! * `slider.val = <nil>` (UpdateDisplayItemRangeLines L2489 with a range line that has no
//!   `range`): SliderControl's `val` is a number, so a nil keeps the old value, as U33's
//!   DIVERGENCE(nil-range); the slider's `shown` (L1089) replaces the value before PoB reads it.
//! * DIVERGENCE(pending-calc): `calcFunc(..)` and `GetMiscCalculator()` only return Pending on
//!   wasm (calc facade, ui doc §10.4); the power sort then keeps the affix order and the mod
//!   comparison adds no lines. Never on desktop.
//!
//! ORDER:
//! * L2155 `pairs(item.clusterJewel.skills)`: the list is sorted by label at L2160; the skill
//!   names of one cluster jewel size are distinct (data dump), so the sort is total and the
//!   `pairs` order is unobservable (c).
//! * L2252 `pairs(item.affixes)`: the list is sorted at L2262-2277 with a total order (the last
//!   key is the mod id), so the `pairs` order is unobservable (c).
//! * L1995 `pairsSortByKey(item.variantGroups)`: sorted (numeric group ids).
//!
//! Props with side effects (ui doc §7.3): none (this range stores no property functions; the
//! Remove buttons' `onClick` closures of UpdateCustomControls are callbacks).
#![allow(non_snake_case, clippy::too_many_lines)]

use std::rc::Rc;

use pob_lua::math::{m_max, m_min, round};
use pob_lua::{lfmt, n, tonumber, tostring, Name};
use pob_sg::Font;
use pob_ui_core::controls::check_box::CheckBoxControl;
use pob_ui_core::controls::slider::SliderControl;
use pob_ui_core::controls::{ButtonControl, DropDownControl, DropItem, EditControl, LabelControl};
use pob_ui_core::luastr::{gsub, lmatch};
use pob_ui_core::prelude::*;
use pob_ui_core::tooltip::tooltip::Tooltip;

use super::ctor::{self, AffixSeries, ClusterSkillValue, PowerSortValue, SocketColorValue};
use super::slot_helper::items_hooks;
use super::ItemsTab;
use crate::app::CxApp;
use crate::calc::facade::MiscCalc;
use crate::model::{self, gamedata, Table, Val};
// CCR-0120 §3: `itemLib` is calc:IT-B's `item_tools` (not re-exported by model.rs)
use pob_calc::classes::item_tools::{
    applyRange, formatModLine, influenceInfo_all, influenceInfo_default,
};

/// This unit's part of the split class struct (ui doc §12.1). The display item's state is in
/// U33's `ctor::State` (CCR-0120 §2), so this part is empty.
#[derive(Default)]
pub struct State {}

/// The value of a variant dropdown entry built by UpdateDisplayItemVariantControls (`{ label,
/// variantId }`; the "No available variants" entry has no `variantId`).
#[derive(Clone, Copy, Debug)]
pub struct VariantValue {
    pub variantId: Option<f64>,
}

/// The value of a `displayItemRangeLine` entry (`{ modLine = modLine, label = modLine.line }`,
/// L2484).
#[derive(Clone, Debug)]
pub struct RangeLineValue {
    pub modLine: Table,
}

// ---- helpers (Rust-only plumbing) ------------------------------------------------------------

/// `self.itemsTab` for U32's hooks, which carry no ItemsTab node (nil raises when indexed).
#[track_caller]
pub fn items_tab_or_err(cx: &Cx) -> CtrlId {
    super::slot_helper::items_tab(cx)
        .unwrap_or_else(|| lua_error!("attempt to index field 'itemsTab' (a nil value)"))
}

fn st(cx: &Cx, me: CtrlId) -> &ctor::State {
    &cx.w::<ItemsTab>(me).ctor
}

fn st_mut(cx: &mut Cx, me: CtrlId) -> &mut ctor::State {
    &mut cx.w_mut::<ItemsTab>(me).ctor
}

/// `self.displayItem` that Lua then indexes (nil raises).
#[track_caller]
fn di(cx: &Cx, me: CtrlId) -> model::ItemRef {
    st(cx, me)
        .displayItem
        .clone()
        .unwrap_or_else(|| lua_error!("attempt to index field 'displayItem' (a nil value)"))
}

/// `self.controls[key]` looked up at call time (nil raises when indexed).
#[track_caller]
fn ctl(cx: &Cx, me: CtrlId, key: impl Into<CtrlKey>) -> CtrlId {
    let key = key.into();
    cx.child(me, key.clone())
        .unwrap_or_else(|| lua_error!("attempt to index field '{key:?}' (a nil value)"))
}

/// `item.<k>` truthiness of an untyped Item field.
fn ext(item: &model::ItemRef, k: Name) -> bool {
    item.extra.truthy(k)
}

/// `item.<k>` in arithmetic or a comparison (nil raises).
#[track_caller]
fn exn(item: &model::ItemRef, k: Name) -> f64 {
    match item.extra.get(k) {
        Val::Num(x) => x,
        other => lua_error!(
            "attempt to perform arithmetic on field '{k}' (a {} value)",
            other.lua_type()
        ),
    }
}

/// `item.<k>` that Lua then indexes as a table (nil raises).
#[track_caller]
fn ex_table(item: &model::ItemRef, k: Name) -> Table {
    item.extra
        .get(k)
        .table()
        .unwrap_or_else(|| lua_error!("attempt to index field '{k}' (a nil value)"))
}

/// `t[x]` for a number key (a non-integral or NaN key reads like any other number key).
fn tget(t: &Table, x: f64) -> Val {
    model::Key::try_num(x).map_or(Val::Nil, |k| t.get(k))
}

/// `item.affixes` (pairs/index of a nil table raise).
#[track_caller]
fn affixes(item: &model::ItemRef) -> &'static gamedata::schema::affixes::AffixTable {
    item.affixes_table()
        .unwrap_or_else(|| lua_error!("attempt to index field 'affixes' (a nil value)"))
}

/// `item.affixes[modId]` (a nil id reads nil).
fn affix(
    item: &model::ItemRef,
    modId: Option<Name>,
) -> Option<gamedata::schema::affixes::ItemModRef> {
    let id = modId?;
    affixes(item).0.get(&id).cloned()
}

/// A string-valued Lua field as a `Name` (nil and non-strings are `None`).
fn name_of(v: &Val) -> Option<Name> {
    v.str().map(|s| Name::from(&s))
}

/// The `modTags` list of a mod line built from an affix (`modTags = mod.modTags`; Lua shares the
/// data table, a copy is equivalent: nothing mutates it in place, see calc:IT-B Item.lua:1051).
fn mod_tags(r#mod: &gamedata::schema::affixes::ItemMod) -> Table {
    let t = Table::new();
    for tag in &r#mod.modTags {
        t.push(*tag);
    }
    t
}

/// `self.build.calcsTab:GetMiscCalculator()` (the calc facade; `None` = Pending, wasm only).
fn misc_calc(cx: &mut Cx) -> Option<Rc<dyn MiscCalc>> {
    let m = cx.build().model.clone();
    match cx.build_mut().calc.misc_calculator(&m) {
        std::task::Poll::Ready((f, _base)) => Some(f),
        // DIVERGENCE(pending-calc)
        std::task::Poll::Pending => None,
    }
}

/// `calcFunc({ repSlotName = slotName, repItem = item })` (`None` = Pending, wasm only).
fn calc_with(
    calcFunc: &Rc<dyn MiscCalc>,
    slotName: Name,
    item: &model::ItemRef,
) -> Option<model::Output> {
    let ov = model::Override::default();
    ov.repSlotName.set(Some(slotName));
    ov.repItem.set(Some(item.clone()));
    match calcFunc.call(Some(Rc::new(ov)), None) {
        std::task::Poll::Ready(o) => Some(o),
        // DIVERGENCE(pending-calc)
        std::task::Poll::Pending => None,
    }
}

/// `drop.selFunc(drop.selIndex, drop.list[drop.selIndex])` (L2191-2194).
fn call_sel_func(cx: &mut Cx, drop: CtrlId) {
    let (f, sel, value) = {
        let w = cx.w::<DropDownControl>(drop);
        (
            w.sel_func.clone(),
            w.sel_index,
            w.sel_index
                .checked_sub(1)
                .and_then(|k| w.list.get(k))
                .cloned(),
        )
    };
    let f = f.unwrap_or_else(|| lua_error!("attempt to call field 'selFunc' (a nil value)"));
    f(cx, sel, value.as_ref());
}

/// The 1-based index of an affix dropdown (`self.controls["displayItemAffix"..i]`), for its
/// `slider` (`displayItemAffixRange<i>`, L972) and its `outputTable`/`outputIndex` fields.
#[track_caller]
fn affix_index(cx: &Cx, me: CtrlId, control: CtrlId) -> usize {
    (1..=ctor::maxModCount)
        .find(|i| cx.child(me, format!("displayItemAffix{i}")) == Some(control))
        .unwrap_or_else(|| lua_error!("attempt to index local 'control' (not an affix dropdown)"))
}

// ---- the methods -------------------------------------------------------------------------------

/// lua: ItemsTab.lua:1948-1958 `ItemsTabClass:CreateDisplayItemFromRaw(itemRaw, normalise)`:
/// attempt to create a new item from the given item raw text and set it as the new display item.
/// @pob Classes/ItemsTab.lua:1948-1958 h=fa46e51a
pub fn CreateDisplayItemFromRaw(cx: &mut Cx, me: CtrlId, itemRaw: &str, normalise: bool) {
    // L1949
    let newItem = crate::main_mode::new_item(Some(itemRaw), None, false);
    // L1950
    if newItem.base.get().is_some() {
        // L1951
        if normalise {
            // L1952
            let migrate = cx.app().main.migrate_eldritch_implicits;
            super::core::CopyAnointsAndEldritchImplicits(cx, &newItem, migrate, false, None);
            // L1953
            newItem.NormaliseQuality();
            // L1954
            newItem.BuildModList();
        }
        // L1956
        SetDisplayItem(cx, me, Some(newItem));
    }
}

/// `self.displayItem[legacyField]` of a legacy variant dropdown (L403-441).
fn legacy_variant_field<'a>(
    item: &'a model::ItemRef,
    legacyField: &str,
) -> &'a std::cell::Cell<Option<f64>> {
    match legacyField {
        "variant" => &item.variant,
        "variantAlt" => &item.variantAlt,
        "variantAlt2" => &item.variantAlt2,
        "variantAlt3" => &item.variantAlt3,
        "variantAlt4" => &item.variantAlt4,
        "variantAlt5" => &item.variantAlt5,
        other => unreachable!("no legacy variant field {other}"),
    }
}

/// lua: ItemsTab.lua:1960-1974 `ItemsTabClass:SelectDisplayItemVariant(index, value,
/// legacyField, control)`.
/// @pob Classes/ItemsTab.lua:1960-1974 h=7b69cf78
pub fn SelectDisplayItemVariant(
    cx: &mut Cx,
    me: CtrlId,
    index: usize,
    value: Option<&DropItem>,
    legacyField: &str,
    control: CtrlId,
) {
    let item = di(cx, me);
    // L1961
    if ext(&item, n!("usesVariantGroups")) {
        // L1962 (`value.variantId`; a string entry has none)
        let variantId = value
            .and_then(|v| v.get::<VariantValue>())
            .and_then(|v| v.variantId);
        let Some(variantId) = variantId else {
            // L1963
            return;
        };
        // L1965 `control.variantGroupId`
        let k = ctor::VARIANT_KEYS
            .iter()
            .position(|key| cx.child(me, *key) == Some(control))
            .unwrap_or_else(|| lua_error!("attempt to index local 'control' (a nil value)"));
        let groupId = st(cx, me).variant_controls[k].variantGroupId.clone();
        let key = match groupId {
            Some(Val::Num(x)) => {
                model::Key::try_num(x).unwrap_or_else(|| lua_error!("table index is NaN"))
            }
            Some(Val::Str(s)) => model::Key::from(Name::from(&s)),
            _ => lua_error!("table index is nil"),
        };
        ex_table(&item, n!("variantGroupSelections")).set(key, variantId);
        // L1966
        item.NormaliseVariantSelections();
    } else {
        // L1968
        legacy_variant_field(&item, legacyField).set(Some(index as f64));
    }
    // L1970
    item.BuildAndParseRaw();
    // L1971
    UpdateDisplayItemVariantControls(cx, me);
    // L1972
    UpdateDisplayItemTooltip(cx, me);
    // L1973
    UpdateDisplayItemRangeLines(cx, me);
}

/// `pairsSortByKey(t)` (Common.lua:657-670): the keys sorted with Lua's `<` (the variant group
/// ids are numbers; comparing a string key with a number raises).
fn pairs_sort_by_key(t: &Table) -> Vec<model::Key> {
    let mut keys: Vec<model::Key> = t.pairs().map(|(k, _)| k).collect();
    let num = |k: &model::Key| {
        k.as_num()
            .unwrap_or_else(|| lua_error!("attempt to compare number with string"))
    };
    pob_lua::sort(&mut keys, |a, b| num(a) < num(b));
    keys
}

/// lua: ItemsTab.lua:1976-2032 `ItemsTabClass:UpdateDisplayItemVariantControls()`.
/// @pob Classes/ItemsTab.lua:1976-2032 h=f0861826
pub fn UpdateDisplayItemVariantControls(cx: &mut Cx, me: CtrlId) {
    // L1977
    let item = st(cx, me).displayItem.clone();
    // L1978-1985
    let controls: Vec<CtrlId> = ctor::VARIANT_KEYS.iter().map(|k| ctl(cx, me, *k)).collect();
    // L1986-1988
    for vc in &mut st_mut(cx, me).variant_controls {
        vc.newVariantVisible = false;
    }
    // L1989
    let Some(item) = item.filter(|i| ext(i, n!("usesVariantGroups"))) else {
        // L1990
        return;
    };

    // L1993
    let version = ctl(cx, me, "displayItemVersion");
    let versionList: Vec<DropItem> = match item.extra.get(n!("versionList")) {
        Val::Table(t) => t
            .ipairs()
            .into_iter()
            .map(|(_, v)| DropItem::str(v.str().map_or_else(String::new, |s| s.to_string())))
            .collect(),
        _ => Vec::new(),
    };
    cx.w_mut::<DropDownControl>(version).list = versionList;
    // L1994
    let selectedVersion = item
        .extra
        .get(n!("selectedVersion"))
        .opt_num()
        .unwrap_or(1.0);
    cx.w_mut::<DropDownControl>(version).sel_index = selectedVersion as usize;
    // L1995
    DropDownControl::check_dropped_width(cx, version, true);
    // L1996
    let mut controlIndex = 1usize;
    // L1997
    let groups = item
        .extra
        .get(n!("variantGroups"))
        .table()
        .unwrap_or_else(|| lua_error!("bad argument #1 to 'pairs' (table expected, got nil)"));
    let selections = item.extra.get(n!("variantGroupSelections"));
    for groupKey in pairs_sort_by_key(&groups) {
        let groupId = groupKey.as_num().unwrap_or(f64::NAN);
        // L1998
        let eligibleOptions = item.GetVariantGroupOptions(groupId, false);
        // L1999
        if !eligibleOptions.is_empty() {
            // L2000
            let Some(&control) = controls.get(controlIndex - 1) else {
                // L2002
                let name = item.name.get();
                cx.sys
                    .platform
                    .con_print(&lfmt!("Item '%s' has more than 6 variant groups", &name));
                // L2003
                break;
            };
            // L2005
            let availableOptions = item.GetVariantGroupOptions(groupId, true);
            // L2006
            let mut list: Vec<DropItem> = Vec::new();
            // L2007
            let mut selectedIndex: Option<usize> = None;
            let variantList = item.variantList.get();
            // L2008
            for &variantId in &availableOptions {
                // L2009-2012
                let label = variantList
                    .as_ref()
                    .and_then(|l| l.get(variantId as usize))
                    .map_or_else(String::new, |s| s.to_string());
                list.push(DropItem::new(
                    label,
                    Rc::new(VariantValue {
                        variantId: Some(variantId),
                    }),
                ));
                // L2013
                let sel = match &selections {
                    Val::Table(t) => t.get(groupKey).opt_num(),
                    _ => {
                        lua_error!("attempt to index field 'variantGroupSelections' (a nil value)")
                    }
                };
                if sel == Some(variantId) {
                    // L2014
                    selectedIndex = Some(list.len());
                }
            }
            // L2017
            if list.is_empty() {
                // L2018-2020
                list.push(DropItem::new(
                    "No available variants",
                    Rc::new(VariantValue { variantId: None }),
                ));
            }
            // L2022-2023
            {
                let w = cx.w_mut::<DropDownControl>(control);
                w.list = list;
                w.sel_index = selectedIndex.unwrap_or(1);
            }
            // L2024-2026
            let vc = &mut st_mut(cx, me).variant_controls[controlIndex - 1];
            vc.variantGroupId = Some(groupKey.to_val());
            vc.newVariantVisible = true;
            vc.newVariantEnabled = availableOptions.len() > 1;
            // L2027
            DropDownControl::check_dropped_width(cx, control, true);
            // L2028
            controlIndex += 1;
        }
    }
}

/// `control.list = item.variantList; control.selIndex = <field>; control:CheckDroppedWidth(true)`
/// (L2044-2046, L2049-2051, ...; see the module notes on nil lists).
fn set_legacy_variant(cx: &mut Cx, control: CtrlId, item: &model::ItemRef, sel: Option<f64>) {
    let list: Vec<DropItem> = item.variantList.get().map_or_else(Vec::new, |l| {
        l.snapshot()
            .iter()
            .map(|s| DropItem::str(s.to_string()))
            .collect()
    });
    let w = cx.w_mut::<DropDownControl>(control);
    w.list = list;
    w.sel_index = sel.map_or(0, |s| s as usize);
    DropDownControl::check_dropped_width(cx, control, true);
}

/// lua: ItemsTab.lua:2033-2115 `ItemsTabClass:SetDisplayItem(item)`: sets the display item to
/// the given item.
/// @pob Classes/ItemsTab.lua:2033-2115 h=cdf77dd3
pub fn SetDisplayItem(cx: &mut Cx, me: CtrlId, item: Option<model::ItemRef>) {
    // L2035
    st_mut(cx, me).displayItem = item.clone();
    // L2036
    let Some(item) = item else {
        // L2113
        st_mut(cx, me).snapHScroll = Some("LEFT");
        return;
    };
    // Update the display item controls
    // L2038
    UpdateDisplayItemTooltip(cx, me);
    // L2039
    st_mut(cx, me).snapHScroll = Some("RIGHT");

    let uses = ext(&item, n!("usesVariantGroups"));
    // L2041
    if uses {
        // L2042
        UpdateDisplayItemVariantControls(cx, me);
    } else {
        // L2044-2046
        let c = ctl(cx, me, "displayItemVariant");
        set_legacy_variant(cx, c, &item, item.variant.get());
    }
    // L2048-2077
    let alts: [(&str, Name, Option<f64>); 5] = [
        (
            "displayItemAltVariant",
            n!("hasAltVariant"),
            item.variantAlt.get(),
        ),
        (
            "displayItemAltVariant2",
            n!("hasAltVariant2"),
            item.variantAlt2.get(),
        ),
        (
            "displayItemAltVariant3",
            n!("hasAltVariant3"),
            item.variantAlt3.get(),
        ),
        (
            "displayItemAltVariant4",
            n!("hasAltVariant4"),
            item.variantAlt4.get(),
        ),
        (
            "displayItemAltVariant5",
            n!("hasAltVariant5"),
            item.variantAlt5.get(),
        ),
    ];
    for (key, has, sel) in alts {
        if !uses && ext(&item, has) {
            let c = ctl(cx, me, key);
            set_legacy_variant(cx, c, &item, sel);
        }
    }
    // L2078
    UpdateSocketControls(cx, me);
    // L2079
    if ext(&item, n!("crafted")) {
        // L2080
        UpdateAffixControls(cx, me);
    }

    // Set both influence dropdowns
    // L2084-2085
    let mut influence1 = 1.0;
    let mut influence2 = 1.0;
    // L2086
    let mut influenceDisplayList = vec![DropItem::str("Influence")];
    // L2087
    let t = item.r#type.get();
    let all = ext(&item, n!("canHaveEldritchInfluence"))
        || matches!(&*t, "Helmet" | "Body Armour" | "Gloves" | "Boots");
    let infos: &[pob_calc::classes::item_tools::InfluenceInfo] = if all {
        &influenceInfo_all
    } else {
        &influenceInfo_default
    };
    for curInfluenceInfo in infos {
        // L2088
        influenceDisplayList.push(DropItem::str(curInfluenceInfo.display));
    }
    // L2090-2091
    let inf1 = ctl(cx, me, "displayItemInfluence");
    let inf2 = ctl(cx, me, "displayItemInfluence2");
    cx.w_mut::<DropDownControl>(inf1).list = influenceDisplayList.clone();
    cx.w_mut::<DropDownControl>(inf2).list = influenceDisplayList;
    // L2092 `ipairs(influenceInfo)` (the module's `itemLib.influenceInfo.all`)
    for (i, curInfluenceInfo) in influenceInfo_all.iter().enumerate() {
        // L2093
        if item.influences.truthy(Name::from(curInfluenceInfo.key)) {
            // L2094
            if influence1 == 1.0 {
                // L2095
                influence1 = (i + 1) as f64 + 1.0;
            // L2096
            } else if influence2 == 1.0 {
                // L2097
                influence2 = (i + 1) as f64 + 1.0;
                // L2098
                break;
            }
        }
    }
    // Initialising these controls must not re-craft the parsed item.
    // L2103-2104
    DropDownControl::set_sel_num(cx, inf1, influence1, true);
    DropDownControl::set_sel_num(cx, inf2, influence2, true);
    // L2105
    let quality = ctl(cx, me, "displayItemQualityEdit");
    let q = item
        .quality
        .get()
        .map_or_else(|| "nil".to_owned(), tostring);
    EditControl::set_text(cx, quality, &q, false);
    // L2106
    let catalyst = ctl(cx, me, "displayItemCatalyst");
    DropDownControl::set_sel_num(cx, catalyst, item.catalyst.get().unwrap_or(0.0) + 1.0, true);
    // L2107-2111
    let cq = ctl(cx, me, "displayItemCatalystQualityEdit");
    let text = match item.catalystQuality.get() {
        // L2108
        Some(v) => tostring(m_max(v, 0.0)),
        // L2110
        None => tostring(0.0),
    };
    EditControl::set_text(cx, cq, &text, false);
    // L2112
    UpdateCustomControls(cx, me);
    // L2113
    UpdateDisplayItemRangeLines(cx, me);
    // L2114
    if item.clusterJewel.get().is_some() && ext(&item, n!("crafted")) {
        // L2115
        UpdateClusterJewelControls(cx, me);
    }
}

/// lua: ItemsTab.lua:2117-2121 `ItemsTabClass:UpdateDisplayItemTooltip()`.
/// @pob Classes/ItemsTab.lua:2117-2121 h=b45af611
pub fn UpdateDisplayItemTooltip(cx: &mut Cx, me: CtrlId) {
    let item = st(cx, me).displayItem.clone();
    let add = items_hooks().add_item_tooltip;
    cx.lend::<ItemsTab, Tooltip, _>(
        me,
        |w| &mut w.ctor.displayItemTooltip,
        |cx, tt| {
            // L2118
            tt.clear(false);
            // L2119 (AddItemTooltip indexes its nil item)
            let item =
                item.unwrap_or_else(|| lua_error!("attempt to index local 'item' (a nil value)"));
            let b = cx.build().model.clone();
            add(cx, &b, tt, &item, None, false);
            // L2120
            tt.center = true;
        },
    );
}

/// lua: ItemsTab.lua:2123-2136 `ItemsTabClass:ToggleDisplayItemModLine(modLine)` (called by
/// Draw's tooltip mod-line click, U34).
/// @pob Classes/ItemsTab.lua:2123-2136 h=751645f9
pub fn ToggleDisplayItemModLine(cx: &mut Cx, me: CtrlId, modLine: Option<&Table>) {
    // L2124
    let (Some(item), Some(modLine)) = (st(cx, me).displayItem.clone(), modLine) else {
        // L2125
        return;
    };
    // L2127
    let disabled = modLine.truthy(n!("disabled"));
    modLine.set(n!("disabled"), !disabled);
    // L2128
    item.BuildAndParseRaw();
    // L2129
    UpdateDisplayItemTooltip(cx, me);
    // L2130
    UpdateDisplayItemRangeLines(cx, me);
    // L2131
    UpdateCustomControls(cx, me);
    // L2132
    if ext(&di(cx, me), n!("crafted")) {
        // L2133
        UpdateAffixControls(cx, me);
    }
    // L2135
    cx.build().model.buildFlag.set(true);
}

/// lua: ItemsTab.lua:2138-2146 `ItemsTabClass:UpdateSocketControls()`.
/// @pob Classes/ItemsTab.lua:2138-2146 h=e5fda89b
pub fn UpdateSocketControls(cx: &mut Cx, me: CtrlId) {
    let item = di(cx, me);
    // L2139
    let sockets = item.sockets.get().snapshot();
    // L2140
    let n = sockets.len() as f64 - item.abyssalSocketCount.get();
    let mut i = 1.0;
    while i <= n {
        let iu = i as usize;
        // L2141
        let drop = ctl(cx, me, format!("displayItemSocket{}", tostring(i)));
        let color = sockets[iu - 1].str(n!("color"));
        DropDownControl::sel_by_value(cx, drop, &|d: &DropItem| {
            d.get::<SocketColorValue>().map(|v| v.0) == color.as_deref()
        });
        // L2142
        if i > 1.0 {
            // L2143
            let link = ctl(cx, me, format!("displayItemLink{}", tostring(i - 1.0)));
            let same = sockets[iu - 1]
                .get(n!("group"))
                .raw_eq(&sockets[iu - 2].get(n!("group")));
            cx.w_mut::<CheckBoxControl>(link).state = same;
        }
        i += 1.0;
    }
}

/// lua: ItemsTab.lua:2148-2173 `ItemsTabClass:UpdateClusterJewelControls()`.
/// @pob Classes/ItemsTab.lua:2148-2173 h=a039d4eb
pub fn UpdateClusterJewelControls(cx: &mut Cx, me: CtrlId) {
    // L2149
    let item = di(cx, me);

    // L2151
    let unavailableSkills = [
        "affliction_strength",
        "affliction_dexterity",
        "affliction_intelligence",
    ];

    // Update list of skills
    // L2154 (wipeTable: the list is replaced below)
    let clusterJewel = item
        .clusterJewel
        .get()
        .unwrap_or_else(|| lua_error!("attempt to index field 'clusterJewel' (a nil value)"));
    let skills = clusterJewel
        .table(n!("skills"))
        .unwrap_or_else(|| lua_error!("bad argument #1 to 'pairs' (table expected, got nil)"));
    let mut skillList: Vec<(String, Name)> = Vec::new();
    // L2155 (ORDER (c): sorted below)
    for (skillId, skill) in skills.pairs() {
        let Some(skillId) = skillId.name() else {
            continue;
        };
        // L2156
        if !unavailableSkills.contains(&&*skillId) {
            // L2157
            let label = skill
                .get(n!("name"))
                .str()
                .map_or_else(String::new, |s| s.to_string());
            skillList.push((label, skillId));
        }
    }
    // L2160
    pob_lua::sort(&mut skillList, |a, b| a.0.as_bytes() < b.0.as_bytes());
    let skill_drop = ctl(cx, me, "displayItemClusterJewelSkill");
    cx.w_mut::<DropDownControl>(skill_drop).list = skillList
        .iter()
        .map(|(label, skillId)| {
            DropItem::new(
                label.clone(),
                Rc::new(ClusterSkillValue { skillId: *skillId }),
            )
        })
        .collect();
    // L2161
    let cur = item.clusterJewelSkill.get();
    if !cur.is_some_and(|s| skills.truthy(s)) {
        // L2162
        let first = skillList
            .first()
            .unwrap_or_else(|| lua_error!("attempt to index field '?' (a nil value)"));
        item.clusterJewelSkill.set(Some(first.1));
    }
    // L2164
    let want = item.clusterJewelSkill.get();
    DropDownControl::sel_by_value(cx, skill_drop, &|d: &DropItem| {
        d.get::<ClusterSkillValue>().map(|v| v.skillId) == want
    });

    // Update added node count slider
    // L2167
    let countControl = ctl(cx, me, "displayItemClusterJewelNodeCount");
    // L2168
    let minNodes = clusterJewel.num(n!("minNodes"));
    let maxNodes = clusterJewel.num(n!("maxNodes"));
    let count = m_min(
        m_max(
            item.clusterJewelNodeCount.get().unwrap_or(minNodes),
            minNodes,
        ),
        maxNodes,
    );
    item.clusterJewelNodeCount.set(Some(count));
    {
        let w = cx.w_mut::<SliderControl>(countControl);
        // L2169
        w.div_count = Some(maxNodes - minNodes);
        // L2170
        w.val = (count - minNodes) / (maxNodes - minNodes);
    }

    // L2172
    CraftClusterJewel(cx, me);
}

/// lua: ItemsTab.lua:2175-2195 `ItemsTabClass:CraftClusterJewel()`.
/// @pob Classes/ItemsTab.lua:2175-2195 h=ccff5011
pub fn CraftClusterJewel(cx: &mut Cx, me: CtrlId) {
    // L2176
    let item = di(cx, me);
    let clusterJewel = item
        .clusterJewel
        .get()
        .unwrap_or_else(|| lua_error!("attempt to index field 'clusterJewel' (a nil value)"));
    let enchantModLines = item.enchantModLines.get();
    // L2177
    enchantModLines.wipe();
    let line = |s: String| {
        let t = Table::new();
        t.set(n!("line"), s);
        t.set(n!("crafted"), true);
        t
    };
    // L2178
    let count = item
        .clusterJewelNodeCount
        .get()
        .unwrap_or_else(|| clusterJewel.num(n!("minNodes")));
    enchantModLines.push(line(format!("Adds {} Passive Skills", tostring(count))));
    // L2179
    let size = clusterJewel.str(n!("size"));
    if size.as_deref() == Some("Large") {
        // L2180
        enchantModLines.push(line("2 Added Passive Skills are Jewel Sockets".to_owned()));
    // L2181
    } else if size.as_deref() == Some("Medium") {
        // L2182
        enchantModLines.push(line("1 Added Passive Skill is a Jewel Socket".to_owned()));
    }
    // L2184
    let skill = item
        .clusterJewelSkill
        .get()
        .and_then(|s| clusterJewel.table(n!("skills")).and_then(|t| t.table(s)));
    let skill = skill.unwrap_or_else(|| lua_error!("attempt to index local 'skill' (a nil value)"));
    // L2185
    let enchant: Vec<String> = skill
        .table(n!("enchant"))
        .unwrap_or_else(|| lua_error!("bad argument #1 to 'concat' (table expected, got nil)"))
        .ipairs()
        .into_iter()
        .map(|(_, v)| v.str().map_or_else(String::new, |s| s.to_string()))
        .collect();
    enchantModLines.push(line(enchant.join("\n")));
    // L2186
    item.BuildAndParseRaw();

    // Update affixes manually to force out affixes that may now be invalid
    // L2189
    UpdateAffixControls(cx, me);
    // L2190
    let limit = exn(&item, n!("affixLimit"));
    let mut i = 1.0;
    while i <= limit {
        // L2191
        let drop = ctl(cx, me, format!("displayItemAffix{}", tostring(i)));
        // L2192
        call_sel_func(cx, drop);
        i += 1.0;
    }
}

/// lua: ItemsTab.lua:2196-2216 `ItemsTabClass:UpdateAffixControls()`: update affix selection
/// controls.
/// @pob Classes/ItemsTab.lua:2196-2216 h=8e6f9a42
pub fn UpdateAffixControls(cx: &mut Cx, me: CtrlId) {
    // L2198
    let item = di(cx, me);
    // L2199
    let affixLimit = exn(&item, n!("affixLimit"));
    let prefixLimit = ex_table(&item, n!("prefixes"))
        .opt_num(n!("limit"))
        .unwrap_or(affixLimit / 2.0);
    // L2200
    let ignoreModType = item.rareLikeUnique_ref().is_some_and(|r| r.ignoreModType);
    // L2201
    let mut powerCache: PowerCache = Vec::new();
    // L2202
    let mut i = 1.0;
    while i <= affixLimit {
        let control = ctl(cx, me, format!("displayItemAffix{}", tostring(i)));
        // L2203
        if i <= prefixLimit {
            // L2204-2207
            let modType = if ignoreModType {
                None
            } else {
                Some(n!("Prefix"))
            };
            // L2208
            UpdateAffixControl(
                cx,
                me,
                control,
                &item,
                modType,
                n!("prefixes"),
                i,
                &mut powerCache,
            );
        } else {
            // L2210
            UpdateAffixControl(
                cx,
                me,
                control,
                &item,
                Some(n!("Suffix")),
                n!("suffixes"),
                i - prefixLimit,
                &mut powerCache,
            );
        }
        i += 1.0;
    }
    // The custom affixes may have had their indexes changed, so the custom control UI is also
    // rebuilt so that it will reference the correct affix index.
    // L2215
    UpdateCustomControls(cx, me);
}

/// `powerCache` (`modId → power`; a Lua table keyed by mod id).
pub type PowerCache = Vec<(Name, f64)>;

fn cache_get(c: &PowerCache, id: Name) -> Option<f64> {
    c.iter().find(|(k, _)| *k == id).map(|(_, v)| *v)
}

/// One `lastSeries` (L2286-2291) while the list is built.
struct Series {
    label: String,
    modList: Vec<Name>,
    haveRange: bool,
    statOrder: Vec<f64>,
}

/// `control.list` entries during UpdateAffixControl: the "None" string or a series.
#[derive(Clone, Copy)]
enum Entry {
    None,
    Series(usize),
}

/// The state of the power sort's `getPower` (L2318-2369): the (re-created) `testSubject` and
/// the cache it fills.
struct PowerState<'a> {
    testSubject: model::ItemRef,
    cache: &'a mut PowerCache,
    calcFunc: Rc<dyn MiscCalc>,
    slotName: Name,
    statTable: Table,
    quality: f64,
}

/// `getPower(modId)` (L2318-2369).
fn get_power(ps: &mut PowerState<'_>, modId: Name) -> f64 {
    // L2319
    if let Some(p) = cache_get(ps.cache, modId) {
        // L2320
        return p;
    }
    // L2322
    let r#mod = affix(&ps.testSubject, Some(modId))
        .unwrap_or_else(|| lua_error!("attempt to get length of local 'mod' (a nil value)"));

    // L2324
    let modCount = r#mod.lines.len();
    // magnitude scaling happens during item parsing, which means we can't use the faster path
    // where items don't need to be re-parsed. note that this wouldn't be correct if we were
    // adding a mod magnitude mod here, but currently all mod magnitude mods are custom modifiers
    // and so this works
    // L2330
    let power;
    let magnitude = ps
        .testSubject
        .extra
        .get(n!("modMagnitudeMods"))
        .table()
        .unwrap_or_else(|| {
            lua_error!("attempt to get length of field 'modMagnitudeMods' (a nil value)")
        })
        .len();
    // L2331
    if magnitude > 0 || ps.testSubject.catalyst.get().is_some_and(|c| c > 0.0) {
        // L2332
        let originalItem = ps.testSubject.BuildRaw();
        // L2333
        for subMod in &r#mod.lines {
            // L2334
            let modLine = Table::new();
            modLine.set(n!("line"), &**subMod);
            modLine.set(n!("modTags"), mod_tags(&r#mod));
            // L2335
            if let Some(t) = r#mod.r#type {
                // L2336
                modLine.set(t, true);
            }
            // L2338
            ps.testSubject.explicitModLines.get().push(modLine);
        }
        // L2340
        ps.testSubject.BuildAndParseRaw();
        // L2341-2344
        power = calc_with(&ps.calcFunc, ps.slotName, &ps.testSubject).map_or(0.0, |o| {
            pob_calc::modules::power::GetFromOutput(&o, &ps.statTable, false)
        });
        // L2345
        ps.testSubject = crate::main_mode::new_item(Some(&originalItem), None, false);
    } else {
        // L2347
        for line in &r#mod.lines {
            // L2348
            let rangedLine = applyRange(line, &Val::Num(ps.quality), Some(1.0), Some(1.0));
            // L2349
            let (modList, extra) = pob_modparser::parseMod(&rangedLine, false);
            // L2350
            let modLine = Table::new();
            modLine.set(n!("line"), &**line);
            if let Some(modList) = modList {
                let t = Table::new();
                for m in modList {
                    t.push(Val::Mod(std::sync::Arc::new(m)));
                }
                modLine.set(n!("modList"), t);
            }
            if let Some(extra) = extra {
                modLine.set(n!("extra"), extra);
            }
            modLine.set(n!("modTags"), mod_tags(&r#mod));
            // L2351
            if let Some(t) = r#mod.r#type {
                // L2352
                modLine.set(t, true);
            }
            // L2354
            ps.testSubject.explicitModLines.get().push(modLine);
        }

        // L2357
        ps.testSubject.BuildModList();
        // L2358-2361
        power = calc_with(&ps.calcFunc, ps.slotName, &ps.testSubject).map_or(0.0, |o| {
            pob_calc::modules::power::GetFromOutput(&o, &ps.statTable, false)
        });
        // L2362
        for _ in 0..modCount {
            // L2363
            let lines = ps.testSubject.explicitModLines.get();
            let len = lines.len();
            lines.remove(Some(len));
        }
    }
    // L2366
    ps.cache.push((modId, power));
    // L2367
    power
}

/// `affixA.statOrder[i]` in a comparison (nil raises when compared with a number).
fn stat_order_lt(a: Option<f64>, b: Option<f64>) -> bool {
    match (a, b) {
        (Some(x), Some(y)) => x < y,
        (None, _) => lua_error!("attempt to compare nil with number"),
        (_, None) => lua_error!("attempt to compare number with nil"),
    }
}

/// lua: ItemsTab.lua:2218-2428 `ItemsTabClass:UpdateAffixControl(control, item, affixType,
/// outputTable, outputIndex, powerCache)`.
/// @pob Classes/ItemsTab.lua:2218-2428 h=996b8b8c
#[allow(clippy::too_many_arguments)]
pub fn UpdateAffixControl(
    cx: &mut Cx,
    me: CtrlId,
    control: CtrlId,
    item: &model::ItemRef,
    affixType: Option<Name>,
    outputTable: Name,
    outputIndex: f64,
    powerCache: &mut PowerCache,
) {
    let ci = affix_index(cx, me, control);
    let slider = ctl(cx, me, format!("displayItemAffixRange{ci}"));
    // L2219-2220
    let extraTags = Table::new();
    let mut excludeGroups: Vec<Name> = Vec::new();
    // L2221
    let allowDuplicateGroups = item
        .rareLikeUnique_ref()
        .is_some_and(|r| r.allowDuplicateGroups);
    let affixLimit = || exn(item, n!("affixLimit"));
    // L2222
    for table in [n!("prefixes"), n!("suffixes")] {
        let tbl = ex_table(item, table);
        // L2223
        let limit = tbl
            .opt_num(n!("limit"))
            .unwrap_or_else(|| affixLimit() / 2.0);
        let mut index = 1.0;
        while index <= limit {
            // L2224
            if index != outputIndex || table != outputTable {
                // L2225
                let entry = tget(&tbl, index);
                let modId = entry.table().and_then(|e| name_of(&e.get(n!("modId"))));
                // L2226
                if let Some(r#mod) = affix(item, modId) {
                    // L2227
                    if let Some(g) = r#mod.group.filter(|_| !allowDuplicateGroups) {
                        // L2228
                        excludeGroups.push(g);
                    }
                    // L2230-2234
                    for tag in &r#mod.tags {
                        extraTags.set(*tag, true);
                    }
                }
            }
            index += 1.0;
        }
    }
    // L2239
    if let (Some(cj), Some(skillId)) = (item.clusterJewel.get(), item.clusterJewelSkill.get()) {
        // L2240
        let skill = cj.table(n!("skills")).and_then(|s| s.table(skillId));
        // L2241
        if let Some(skill) = skill {
            // L2242
            match skill.get(n!("tag")) {
                Val::Str(t) => extraTags.set(Name::from(&t), true),
                _ => lua_error!("table index is nil"),
            }
        }
    }
    // L2245
    let outTbl = ex_table(item, outputTable);
    let selAffix = tget(&outTbl, outputIndex)
        .table()
        .and_then(|e| name_of(&e.get(n!("modId"))));
    // L2246-2247
    let mut affixList: Vec<Name> = Vec::new();
    let mut retainedAffixes: Vec<Name> = Vec::new();
    // L2248 (ORDER (c), see the module notes)
    for (modId, r#mod) in &affixes(item).0 {
        // L2249
        if (affixType.is_none() || r#mod.r#type == affixType)
            && !r#mod.group.is_some_and(|g| excludeGroups.contains(&g))
            && !item.CheckIfModIsDelve(r#mod)
        {
            // L2250
            if item.CanHaveMod(r#mod, Some(&extraTags)) {
                // L2251
                affixList.push(*modId);
            // L2252
            } else if Some(*modId) == selAffix {
                // L2253
                affixList.push(*modId);
                // L2254
                retainedAffixes.push(*modId);
            }
        }
    }
    // L2258-2276
    let table = affixes(item);
    pob_lua::sort(&mut affixList, |a, b| {
        // L2259-2260
        let modA = &table.0[a];
        let modB = &table.0[b];
        // L2261
        for i in 0..m_max(modA.lines.len() as f64, modB.lines.len() as f64) as usize {
            // L2262
            if modA.lines.get(i).is_none() {
                // L2263
                return true;
            // L2264
            } else if modB.lines.get(i).is_none() {
                // L2265
                return false;
            // L2266
            } else if modA.statOrder.get(i) != modB.statOrder.get(i) {
                // L2267
                return stat_order_lt(
                    modA.statOrder.get(i).copied(),
                    modB.statOrder.get(i).copied(),
                );
            }
        }
        // L2270
        if modA.level != modB.level {
            // L2271
            return stat_order_lt(modA.level, modB.level);
        }
        // L2273
        a.as_str().as_bytes() < b.as_str().as_bytes()
    });
    let quality = cx.app().main.default_item_affix_quality;
    // L2275-2276 (the list is stored below; nothing reads it in between)
    {
        let w = cx.w_mut::<DropDownControl>(control);
        w.sel_index = 1;
        w.list = vec![DropItem::str("None")];
    }
    // L2277-2278
    {
        let a = &mut st_mut(cx, me).affix_controls[ci - 1];
        a.outputTable = Some(outputTable);
        a.outputIndex = Some(outputIndex);
    }
    // L2279
    cx.base_mut(slider).shown = Prop::Val(false);
    // L2280
    cx.w_mut::<SliderControl>(slider).val = quality.unwrap_or(0.5);
    // L2281
    let selAffix = tget(&outTbl, outputIndex)
        .table()
        .unwrap_or_else(|| lua_error!("attempt to index field '?' (a nil value)"))
        .get(n!("modId"));
    let selAffix = name_of(&selAffix);
    // L2282
    let mut series: Vec<Series> = Vec::new();
    let mut list: Vec<Entry> = vec![Entry::None];
    // combine runs of modifiers to one group, which will only take up one row in the list
    // L2285
    for modId in &affixList {
        // L2286
        let r#mod = &table.0[modId];
        // L2287
        if !series
            .last()
            .is_some_and(|s| s.statOrder == r#mod.statOrder)
        {
            // L2288
            let modString: String = r#mod
                .lines
                .iter()
                .map(|l| &**l)
                .collect::<Vec<&str>>()
                .join("/");
            // L2289-2294
            series.push(Series {
                haveRange: lmatch(&modString, "%(%-?[%d%.]+%-%-?[%d%.]+%)").is_some(),
                label: modString,
                modList: Vec::new(),
                statOrder: r#mod.statOrder.clone(),
            });
            // L2295
            list.push(Entry::Series(series.len() - 1));
        }
        let lastSeries = series.last_mut().expect("pushed above");
        // cluster jewel mods retained after changing the cluster type
        // L2298
        if retainedAffixes.contains(modId) && item.clusterJewel.get().is_some() {
            // L2299
            lastSeries.label = format!("^8[Retained] {}", lastSeries.label);
        }
        // L2301
        lastSeries.modList.push(*modId);
        // L2302
        if lastSeries.modList.len() == 2 {
            // L2303
            lastSeries.label = gsub(
                &gsub(&lastSeries.label, "%(%-?[%d%.]+%-%-?[%d%.]+%)", "#"),
                "%-?%d+%.?%d*",
                "#",
            );
            // L2304
            lastSeries.haveRange = true;
        }
    }

    // L2308
    let sorting = ctl(cx, me, "craftingSorting");
    let sortOption = DropDownControl::get_sel_value(cx, sorting)
        .unwrap_or_else(|| lua_error!("attempt to index local 'sortOption' (a nil value)"));
    let sortEntry = sortOption.get::<PowerSortValue>().and_then(|v| v.0);
    // sort modifier groups by power
    // L2310 `self.controls.craftingSortingLabel.shown()`
    let label = ctl(cx, me, "craftingSortingLabel");
    if let Some(entry) = sortEntry.filter(|e| e.stat.is_some()) {
        if cx.base(label).shown.get(cx, label) {
            // L2311
            let calcFunc = misc_calc(cx);
            let displayItem = di(cx, me);
            // L2312
            let slotName = displayItem.GetPrimarySlot();
            // L2313
            let testSubject =
                crate::main_mode::new_item(Some(&displayItem.BuildRaw()), None, false);
            // L2314
            let mut freshCache: PowerCache = Vec::new();
            let mut usesFresh = false;
            // L2315
            if selAffix.is_some_and(|s| &*s != "None") {
                // L2316
                let none = Table::new();
                none.set(n!("modId"), "None");
                ex_table(&testSubject, outputTable).set(
                    model::Key::try_num(outputIndex)
                        .unwrap_or_else(|| lua_error!("table index is NaN")),
                    none,
                );
                // L2317
                testSubject.Craft();
                // L2318
                usesFresh = true;
            }
            // L2320-2327 pickModifierFromList
            let pickModifierFromList = |modList: &[Name]| -> Name {
                // pick mid tier modifier from a group
                if modList.len() == 1 {
                    modList[0]
                } else {
                    let q = quality.unwrap_or_else(|| {
                        lua_error!("attempt to perform arithmetic on field 'defaultItemAffixQuality' (a nil value)")
                    });
                    let k = 1.0 + round((modList.len() as f64 - 1.0) * q, None);
                    *modList
                        .get(k as usize - 1)
                        .unwrap_or_else(|| lua_error!("attempt to index a nil value"))
                }
            };
            // DIVERGENCE(pending-calc): no calculator (wasm only), no sort
            if let Some(calcFunc) = calcFunc {
                let mut ps = PowerState {
                    testSubject,
                    cache: if usesFresh {
                        &mut freshCache
                    } else {
                        &mut *powerCache
                    },
                    calcFunc,
                    slotName,
                    statTable: pob_calc::modules::power::powerStat_table(entry),
                    quality: quality.unwrap_or(0.5),
                };
                // L2370-2381
                pob_lua::sort(&mut list, |a, b| {
                    // keep "None" as the first option
                    // L2372
                    let Entry::Series(a) = *a else {
                        // L2373
                        return true;
                    };
                    // L2374
                    let Entry::Series(b) = *b else {
                        // L2375
                        return false;
                    };
                    // L2378-2379
                    let modIdA = pickModifierFromList(&series[a].modList);
                    let modIdB = pickModifierFromList(&series[b].modList);
                    // L2381
                    get_power(&mut ps, modIdA) > get_power(&mut ps, modIdB)
                });
            }
        }
    }
    // L2384-2398 findSelectedIdx
    let findSelectedIdx = || -> usize {
        // L2385
        for (i, entry) in list.iter().enumerate() {
            match entry {
                // L2386-2391
                Entry::Series(s) => {
                    for modId in &series[*s].modList {
                        if selAffix == Some(*modId) {
                            return i + 1;
                        }
                    }
                }
                // L2392-2395 (`selAffix == entry`: the "None" string)
                Entry::None => {
                    if selAffix.as_deref() == Some("None") {
                        return i + 1;
                    }
                }
            }
        }
        // L2397
        1
    };
    // L2399
    let selIndex = findSelectedIdx();
    let items: Vec<DropItem> = list
        .iter()
        .map(|e| match e {
            Entry::None => DropItem::str("None"),
            Entry::Series(s) => {
                let s = &series[*s];
                DropItem::new(
                    s.label.clone(),
                    Rc::new(AffixSeries {
                        modId: None,
                        modList: s.modList.clone(),
                        haveRange: s.haveRange,
                        statOrder: s.statOrder.clone(),
                    }),
                )
            }
        })
        .collect();
    {
        let w = cx.w_mut::<DropDownControl>(control);
        w.list = items;
        w.sel_index = selIndex;
    }
    // L2400
    let sel = match list[selIndex - 1] {
        Entry::Series(s) => Some(&series[s]),
        Entry::None => None,
    };
    if let Some(sel) = sel.filter(|s| s.haveRange) {
        // L2401
        let divCount = sel.modList.len() as f64;
        // L2402
        let index = sel
            .modList
            .iter()
            .position(|m| Some(*m) == selAffix)
            .map(|p| (p + 1) as f64)
            .unwrap_or_else(|| lua_error!("attempt to compare nil with number"));
        // Imported legacy rolls can sit outside the current 0-1 affix range.
        // Keep that value on the affix, but show the nearest slider endpoint.
        // L2405
        let affixRange = tget(&outTbl, outputIndex)
            .table()
            .unwrap_or_else(|| lua_error!("attempt to index field '?' (a nil value)"))
            .get(n!("range"));
        // L2406
        let raw = match &affixRange {
            Val::Table(t) => match t.get(1) {
                Val::Nil | Val::Bool(false) => affixRange.clone(),
                v => v,
            },
            Val::Nil | Val::Bool(false) => Val::Num(0.5),
            v => v.clone(),
        };
        let raw = match raw {
            Val::Num(x) => x,
            Val::Str(s) => tonumber(&s).unwrap_or_else(|| {
                lua_error!("bad argument #2 to 'max' (number expected, got string)")
            }),
            other => lua_error!(
                "bad argument #2 to 'max' (number expected, got {})",
                other.lua_type()
            ),
        };
        let mut range = m_min(1.0, m_max(0.0, raw));
        // Avoid exact integer boundary that slider:GetDivVal's ceil would assign to the previous
        // segment
        // L2408
        if range == 0.0 && index > 1.0 {
            // L2409
            range = 1e-4;
        }
        let w = cx.w_mut::<SliderControl>(slider);
        // L2401
        w.div_count = Some(divCount);
        // L2411
        w.val = (index - 1.0 + range) / divCount;
        // L2412
        if divCount == 1.0 {
            // L2413
            w.div_count = None;
        }
        // L2415
        cx.base_mut(slider).shown = Prop::Val(true);
    }
}

/// lua: ItemsTab.lua:2429-2477 `ItemsTabClass:UpdateCustomControls()`: create/update custom
/// modifier controls.
/// @pob Classes/ItemsTab.lua:2429-2477 h=3260778d
pub fn UpdateCustomControls(cx: &mut Cx, me: CtrlId) {
    // L2431
    let item = di(cx, me);
    // L2432
    let mut i = 1usize;
    // L2433 (`copyTable`: the loop only reads the copies, so the lines themselves are used)
    let explicit = item.explicitModLines.get();
    let mut modLines: Vec<Table> = explicit.snapshot();
    // L2434
    let crucible = item.crucibleModLines.get();
    if !crucible.is_empty() {
        // L2435
        for line in crucible.snapshot() {
            // L2436
            modLines.push(line);
        }
    }
    // L2439
    let rarity = item.rarity.get();
    if item.rareLikeUnique_ref().is_some()
        || &*rarity == "MAGIC"
        || &*rarity == "RARE"
        || !crucible.is_empty()
    {
        // L2440
        for (index0, modLine) in modLines.iter().enumerate() {
            let index = index0 + 1;
            // L2441
            if modLine.truthy(n!("custom"))
                || modLine.truthy(n!("crafted"))
                || modLine.truthy(n!("crucible"))
            {
                // L2442
                let line = formatModLine(modLine, false);
                // L2443
                if line.is_some() {
                    let removeKey = format!("displayItemCustomModifierRemove{i}");
                    // L2444
                    if cx.child(me, removeKey.clone()).is_none() {
                        let section = ctl(cx, me, "displayItemSectionCustom");
                        // L2445
                        let remove = cx
                            .add(
                                me,
                                removeKey.clone(),
                                ButtonControl::new(
                                    Anchor::new(TopLeft, section, TopLeft),
                                    rect(0.0, i as f64 * 22.0 + 4.0, 70.0, 20.0),
                                    "^7Remove",
                                    |_| {},
                                ),
                            )
                            .id();
                        // L2446
                        cx.add(
                            me,
                            format!("displayItemCustomModifier{i}"),
                            LabelControl::new(
                                Anchor::new(Left, remove, Right),
                                rect(65, 0, 0, 16),
                                String::new(),
                            ),
                        );
                        // L2447
                        cx.add(
                            me,
                            format!("displayItemCustomModifierLabel{i}"),
                            LabelControl::new(
                                Anchor::new(Left, remove, Right),
                                rect(5, 0, 0, 16),
                                String::new(),
                            ),
                        );
                    }
                    let remove = ctl(cx, me, removeKey.clone());
                    // L2449
                    cx.base_mut(remove).shown = Prop::Val(true);
                    // L2450
                    let mut label = formatModLine(modLine, false).unwrap_or_default();
                    // L2451
                    if cx
                        .sg
                        .draw_string_cursor_index(16.0, Font::Var, &label, 330.0, 10.0)
                        < label.len()
                    {
                        // L2452
                        let k =
                            cx.sg
                                .draw_string_cursor_index(16.0, Font::Var, &label, 310.0, 10.0);
                        label = format!(
                            "{}...",
                            pob_lua::string::sub_lossy(&label, 1, Some(k as i64))
                        );
                    }
                    // L2454
                    let m = ctl(cx, me, format!("displayItemCustomModifier{i}"));
                    cx.w_mut::<LabelControl>(m).label = Prop::Val(label);
                    // L2455
                    let l = ctl(cx, me, format!("displayItemCustomModifierLabel{i}"));
                    let kind = if modLine.truthy(n!("crafted")) {
                        " ^7Crafted:"
                    } else if modLine.truthy(n!("crucible")) {
                        "^7Crucible:"
                    } else {
                        " ^7Custom:"
                    };
                    cx.w_mut::<LabelControl>(l).label = Prop::Val(kind.to_owned());
                    // L2456-2467
                    let item = item.clone();
                    cx.w_mut::<ButtonControl>(remove).on_click = Rc::new(move |cx| {
                        let explicit = item.explicitModLines.get();
                        // L2457
                        if index > explicit.len() {
                            // L2458
                            item.crucibleModLines
                                .get()
                                .remove(Some(index - explicit.len()));
                        } else {
                            // L2460
                            explicit.remove(Some(index));
                        }
                        // L2462
                        item.BuildAndParseRaw();
                        // L2463
                        let id = item.id.get();
                        // L2464
                        CreateDisplayItemFromRaw(cx, me, &item.BuildRaw(), false);
                        // L2465
                        di(cx, me).id.set(id);
                        None
                    });
                    // L2467
                    i += 1;
                }
            }
        }
    }
    // L2472
    item.extra.set(n!("customCount"), (i - 1) as f64);
    // L2473
    while let Some(remove) = cx.child(me, format!("displayItemCustomModifierRemove{i}")) {
        // L2474
        cx.base_mut(remove).shown = Prop::Val(false);
        // L2475
        i += 1;
    }
}

/// lua: ItemsTab.lua:2479-2491 `ItemsTabClass:UpdateDisplayItemRangeLines()`: updates the range
/// line dropdown and range slider for the current display item.
/// @pob Classes/ItemsTab.lua:2479-2491 h=9ff02dcf
pub fn UpdateDisplayItemRangeLines(cx: &mut Cx, me: CtrlId) {
    // L2481
    let Some(item) = st(cx, me).displayItem.clone() else {
        return;
    };
    let rangeLineList = ex_table(&item, n!("rangeLineList"));
    if !rangeLineList.truthy(1) {
        return;
    }
    // L2482 (wipeTable: replaced below)
    let mut list: Vec<DropItem> = Vec::new();
    // L2483
    for (_, modLine) in rangeLineList.ipairs() {
        let Val::Table(modLine) = modLine else {
            lua_error!(
                "attempt to index local 'modLine' (a {} value)",
                modLine.lua_type()
            )
        };
        // L2484
        if (modLine.truthy(n!("modId")) && modLine.truthy(n!("newModId")))
            || modLine.truthy(n!("range"))
        {
            // L2485
            let label = modLine
                .get(n!("line"))
                .str()
                .map_or_else(String::new, |s| s.to_string());
            list.push(DropItem::new(label, Rc::new(RangeLineValue { modLine })));
        }
    }
    let drop = ctl(cx, me, "displayItemRangeLine");
    {
        let w = cx.w_mut::<DropDownControl>(drop);
        w.list = list;
        // L2488
        w.sel_index = 1;
    }
    // L2489 (a nil `range` keeps the value; see the module notes)
    let slider = ctl(cx, me, "displayItemRangeSlider");
    let first = rangeLineList.table(1).expect("checked at L2481");
    match first.get(n!("range")) {
        Val::Num(v) => cx.w_mut::<SliderControl>(slider).val = v,
        Val::Nil => {}
        other => lua_error!(
            "attempt to perform arithmetic on field 'val' (a {} value)",
            other.lua_type()
        ),
    }
}

/// lua: ItemsTab.lua:2493-2501 `checkLineForAllocates(line, nodes)` (a module local).
/// @pob Classes/ItemsTab.lua:2493-2501 h=df97821d
fn checkLineForAllocates(line: &str, nodes: Option<&model::LjIntMap<model::NodeRef>>) -> String {
    // L2494
    if let Some(nodes) = nodes.filter(|_| lmatch(line, "Allocates").is_some()) {
        // L2495
        let nodeId = lmatch(line, "%d+").and_then(|s| tonumber(&s));
        // L2496
        let node = nodeId
            .filter(|x| x.fract() == 0.0)
            .and_then(|x| nodes.get(x as i64));
        if let Some(node) = node {
            // L2497 (`node.name`: PassiveTree.lua:534 copies it into `dn`)
            return format!("Allocates {}", node.dn.get());
        }
    }
    // L2500
    line.to_owned()
}

/// lua: ItemsTab.lua:2503-2526 `ItemsTabClass:AddModComparisonTooltip(tooltip, mod,
/// replaceImplicits)`. // CCR-0124 §2: U33's hook entry has no `replaceImplicits` (it passes
/// false, the L927 nil); U36/U37 call this fn with `true`.
/// @pob Classes/ItemsTab.lua:2503-2526 h=3f7f8072
pub fn AddModComparisonTooltip(
    cx: &mut Cx,
    me: CtrlId,
    tooltip: &mut Tooltip,
    r#mod: Option<&gamedata::schema::affixes::ItemMod>,
    replaceImplicits: bool,
) {
    let displayItem = di(cx, me);
    // L2504
    let slotName = displayItem.GetPrimarySlot();
    // L2505
    let newItem = crate::main_mode::new_item(Some(&displayItem.BuildRaw()), None, false);
    // L2506
    let targetLines = if replaceImplicits {
        newItem.implicitModLines.get()
    } else {
        newItem.explicitModLines.get()
    };
    // L2507
    if replaceImplicits {
        // L2508
        targetLines.wipe();
    }

    // L2511
    let r#mod = r#mod
        .unwrap_or_else(|| lua_error!("bad argument #1 to 'ipairs' (table expected, got nil)"));
    let spec = cx.build().model.spec.get();
    for subMod in &r#mod.lines {
        // L2512
        let modLine = Table::new();
        let line = spec
            .nodes
            .with(|nodes| checkLineForAllocates(subMod, Some(nodes)));
        modLine.set(n!("line"), line);
        modLine.set(n!("modTags"), mod_tags(r#mod));
        // L2513
        if let Some(t) = r#mod.r#type {
            // L2514
            modLine.set(t, true);
        }
        // L2516
        targetLines.push(modLine);
    }

    // L2519
    newItem.BuildAndParseRaw();

    // L2521 DIVERGENCE(pending-calc)
    let Some(calcFunc) = misc_calc(cx) else {
        return;
    };
    // L2522
    let Some(outputBase) = calc_with(&calcFunc, slotName, &displayItem) else {
        return;
    };
    // L2523
    let Some(outputNew) = calc_with(&calcFunc, slotName, &newItem) else {
        return;
    };
    // L2524
    crate::build::stats::add_stat_compares_to_tooltip(
        cx,
        tooltip,
        &outputBase,
        &outputNew,
        "\nAdding this mod will give: ",
        None,
    );
}

/// lua: ItemsTab.lua:2527-2542 `ItemsTabClass:GetEquippedSlotForItem(item)`: returns the first
/// slot in which the given item is equipped (the model's slot tables, CCR-0107 §1).
/// @pob Classes/ItemsTab.lua:2527-2542 h=50c00ec5
pub fn GetEquippedSlotForItem(cx: &Cx, item: &model::ItemRef) -> (Option<Table>, Option<Table>) {
    let m = &cx.build().model;
    let id = item.id.get().map_or(Val::Nil, Val::Num);
    // L2529
    for slot in m.itemsTab.orderedSlots.snapshot() {
        // L2530
        if !slot.truthy(n!("inactive")) {
            // L2531
            if slot.get(n!("selItemId")).raw_eq(&id) {
                // L2532
                return (Some(slot), None);
            }
            // (`itemSet[nil]` is a legal read: a nil slotName matches nothing)
            let slotName = slot.str(n!("slotName"));
            // L2534
            for itemSetId in m.itemsTab.itemSetOrderList.snapshot() {
                // L2536 (`and` short-circuits: the set is indexed only when it is not the
                // active one)
                if f64::from(itemSetId) != m.itemsTab.activeItemSetId.get() {
                    // L2535
                    let itemSet = m
                        .itemsTab
                        .itemSets
                        .with(|s| s.get(&itemSetId).cloned())
                        .unwrap_or_else(|| {
                            lua_error!("attempt to index local 'itemSet' (a nil value)")
                        });
                    let e = slotName
                        .as_ref()
                        .and_then(|slotName| itemSet.table(Name::from(slotName)));
                    if let Some(e) = e {
                        if e.get(n!("selItemId")).raw_eq(&id) {
                            // L2537
                            return (Some(slot), Some(itemSet));
                        }
                    }
                }
            }
        }
    }
    (None, None)
}

/// lua: ItemsTab.lua:2544-2558 `ItemsTabClass:GetComparisonSlotNameForItem(item)`.
/// @pob Classes/ItemsTab.lua:2544-2558 h=5b4742e9
pub fn GetComparisonSlotNameForItem(cx: &Cx, me: CtrlId, item: &model::ItemRef) -> String {
    // L2545
    let (equippedSlot, _) = GetEquippedSlotForItem(cx, item);
    // L2546
    if let Some(slot) = equippedSlot {
        // L2547
        return slot
            .str(n!("slotName"))
            .map_or_else(String::new, |s| s.to_string());
    }
    // L2549
    if &*item.r#type.get() == "Jewel" {
        let m = cx.build().model.clone();
        // L2550
        for slot in m.itemsTab.orderedSlots.snapshot() {
            let slotName = slot.str(n!("slotName")).unwrap_or_default();
            let name = Name::from(&slotName);
            // L2551 (`slot:IsShown()`: the slot's control)
            let shown = || {
                st(cx, me)
                    .slots
                    .get(&name)
                    .is_some_and(|id| cx.is_shown(*id))
            };
            if !slot.truthy(n!("inactive"))
                && slot.get(n!("selItemId")).raw_eq(&Val::Num(0.0))
                && shown()
                && m.itemsTab.IsItemValidForSlot(&m, item, &slotName, None)
            {
                // L2552
                return slotName.to_string();
            }
        }
    }
    // L2557
    item.GetPrimarySlot().to_string()
}

// ItemsTab.lua:2559-2605 (`IsItemValidForSlot`) is calc:BM-A's (`model::ItemsTab::
// IsItemValidForSlot`, marker there, 2560-2605); its closing `end` (2606) has no statement:
// @pob-skip Classes/ItemsTab.lua:2606-2606 reason="the closing end of IsItemValidForSlot (calc:BM-A ports the body)"
// 2607-2608 are blank and the next function's comment.
