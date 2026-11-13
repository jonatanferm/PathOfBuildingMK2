//! Port of PathOfBuilding v2.67.2 (16de4b82d) src/Classes/ItemsTab.lua:3993-5055 (the header
//! influence, FormatItemSource, the jewel comparison spec, GetSocketDescriptionLine,
//! AddItemTooltip, AddItemStatDifferences, CreateUndoState / RestoreUndoState). Owner: U38.
//!
//! @pob-file Classes/ItemsTab.lua @16de4b82d
//!
//! This unit's part of the split `ItemsTab` struct (ui doc §12.1) is [`State`]: the UndoHandler
//! stack (`self.undo`, `self.redo`, `self.modFlag`, UndoHandler.lua:17-21) with this file's
//! CreateUndoState / RestoreUndoState as [`UNDO_OPS`] (ui doc §10.3: the restore tail,
//! `PopulateSlots`, is U34's and is reached through U32's `ItemsTabHooks::populate_slots`).
//!
//! `self` is the ItemsTab node (`me`, [`slot_helper::items_tab`]); `self.slots` are U33's slot
//! controls (`ctor::State::slots`) whose model half is calc:BM-A's slot table
//! (`ItemSlotControl::slot`); `self.items`, `self.itemSets`, ... are calc:BM-A's
//! `build.itemsTab` (`model::ItemsTab`). A Lua `slot` argument is a [`Val`]: nil, `false`
//! (TimelessJewelListControl's socket -1), the model slot table, or a slot name (a string:
//! CalcPerform.lua:2139 `reqSource.sourceSlot`, CalcBreakdownControl's `row.source`); indexing
//! a string reads nil (`("x").slotNum`, the string library has no such member).
//!
//! The ItemsTab of another build (FUP6, closing CCR-0122 §2 and CCR-0143 §4): a comparison
//! entry has its own ItemsTab (CompareEntry.lua:113), reached as
//! `compareSpec.build.itemsTab` (PassiveTreeView.lua:1492/1783) and `hoverItemsTab`
//! (CompareTab.lua:3988). [`AddItemTooltip`] and [`AddItemStatDifferences`] take `self` as an
//! [`ItemsTabSelf`]: for an entry they run with the entry swapped in for the open build
//! (`compare::entry::with_entry_as_build`: `self.build.X` is then `cx.build().X`), `self.slots`
//! are the entry model's slot tables (their `shown()` is calc:BM-A's `slot_IsShown`, the same
//! closures over the model table) and `self.build:AddStatComparesToTooltip` /
//! `AddRequirementsToTooltip` are CompareEntry's (U47). U32's hook passes the model build
//! (`ItemsTabSelf::of_build`: an entry's model selects that entry, any other the open build).
//!
//! The entries of the other units' hook tables that name these methods are installed by one
//! line each in their defaults (CCR-0107 §1, CCR-0116 §4, CCR-0106 §2;
//! CCR-0125 §4 lists them): [`add_item_tooltip_hook`], [`add_undo_state_hook`],
//! [`set_mod_flag_hook`], [`AddItemStatDifferences`], [`mod_flag`] / [`set_mod_flag`].
//!
//! Lua semantics:
//! * Untyped Item fields (`foulborn`, `veiled`, `vestigial`, `memoryStrands`, `intangibility`,
//!   `foilType`, `namePrefix`, `league`, `source`, `versionList`, ...) are `item.extra`; the
//!   influence flags (`item.shaper`, `item.cleansing`, ...) are `item.influences`. A comparison
//!   or arithmetic on a nil field raises, as in Lua; `~=` does not.
//! * `text:gsub(..)` inside `..` keeps the first result ([`luastr::gsub`]);
//!   `FormatItemSource`'s callers concatenate it, so it returns the string.
//! * `#aParams` of the sort parameter lists (L4996-4998) is 7: the constructor sizes the array
//!   part for 7 values and the 7th (`slotName`) is never nil, so LuaJIT's border is 7 whatever
//!   holes the output values leave.
//! * `item.base.flavourText` is nil or a list in PoB; the data schema keeps it as a list that is
//!   empty when the base has none (no base has an empty list), so "non-empty" is its truthiness.
//!
//! ORDER:
//! * L4939 `pairs(self.slots)` (the compare slots): the list is sorted by `sortFunc` (L4994-5012),
//!   a total order (its last keys are `label` and `slotName`, unique per slot), so the shown order
//!   does not depend on it, except for the limited-unique branch (L4966-4973), which adds the
//!   compare blocks in `pairs` order: per-process random in PoB (string keys); the port uses the
//!   slot creation order. Resolution (b): those blocks compare as a multiset (u38 test).
//! * L4813 `pairs(self.slots)` counts empty flask slots: (c) a count.
//! * L4406 / L4428 `pairs(item.buffModLines)`: an array (no holes; the parser appends), whose
//!   `pairs` order is index order: (c).
//! * L4582 `pairs(flavourTable)` takes the first entry: string keys, per-process random in PoB
//!   when a unique has more than one flavour id; the port takes the data dump's first. Resolution
//!   (b): the u38 test reports and skips such tooltips (no unique of the corpus has two).
//! * L4105, L4112 (field copies), L5025, L5030 (keyed state tables), L5041, L5050 (array copies):
//!   (c) the result is a keyed table, whatever the visit order.
//! * L4110 / L4124 / L4133 `pairs(spec.nodes)` / `pairs(spec.allocNodes)`: the comparison spec's
//!   maps are filled in the source maps' `LjIntMap` order, as Lua does; `spec.nodes`' own order
//!   is per-process random in PoB (CCR-0028 §9), so (b) as every `spec.nodes` site.
//! * L5044 `pairs(state.slotSelItemId)`: each `SetSelItemId` writes its own slot; the cluster
//!   graph rebuilds (ItemSlotControl.lua:77) read the whole `spec.jewels`, so the last rebuild
//!   sees every write unless a socket whose id did not change carries a `spec.jewels` value
//!   different from its `selItemId` (SetSelItemId keeps the two equal): (c).
//!
//! DIVERGENCE(item-copy): `copyTableSafe(item, true, true)` (L5026) is [`copy_item`]: the
//! scalar fields and the item's raw keys (`extra`, `influences`) are copied, the sub-tables are
//! shared (as Lua's shallow copy shares them). The one Lua sub-table `slotModList` is three
//! typed Rust fields (`slotModList[1]`..`[3]`), so a later write of one slot's mod list through
//! the copy would not reach the original (PoB writes it only in `BuildModList`, which assigns a
//! new `slotModList` table there too). CCR-0125 §3.
//! DIVERGENCE(temp-item-id): `buildSpecForJewelComparison` stores the replacement jewel under
//! the first free id counting down from -1 (L4166-4170); calc:BM-A's item map has `u32` keys, so
//! the port counts down from `u32::MAX`. The id is visible only to the comparison spec's
//! `BuildAllDependsAndPaths` (which finds the item by it) and is removed right after, as in Lua.
//! CCR-0125 §2.
//! DIVERGENCE(xpcall-traceback): a Lua error inside that `BuildAllDependsAndPaths` re-raises
//! with PoB's `debug.traceback` text appended (`xpcall(.., debug.traceback)`, `error(err, 0)`);
//! the port re-raises the message alone (a drop guard removes the temporary item first).
//! DIVERGENCE(pending-calc): `GetMiscCalculator` / `calcFunc` returning `Pending` (wasm only)
//! ends AddItemStatDifferences without stat lines.
//! `flavourLookup` (ItemsTab.lua:62-69) is [`flavour_lookup`] here, with `text` the list of lines
//! (CCR-0125 §1; FUP7 removed U33's unused copy, which read `text` as a string and so dropped
//! every entry, and moved its `@pob` marker here).
//!
//! Props with side effects (ui doc §7.3): none (no property closures in this range).
#![allow(non_snake_case, non_upper_case_globals, clippy::too_many_lines)]

use std::rc::Rc;
use std::sync::OnceLock;

use pob_lua::math::{m_floor, m_min};
use pob_lua::{lfmt, n, Name};
use pob_sg::{Font, Key as SgKey};
use pob_ui_core::luastr;
use pob_ui_core::prelude::*;
use pob_ui_core::tooltip::tooltip::{ModLineRef, Tooltip, TtBackground, TtColor, TtModLine};
use pob_ui_core::undo::{UndoOps, UndoStack};

use super::ctor;
use super::slot::{item_by_id, set_sel_item_id, ItemSlotControl};
use super::slot_helper::{self, items_hooks};
use super::ItemsTab;
use crate::app::CxApp;
use crate::build::stats as build_stats;
use crate::calc::MiscCalc;
use crate::controls::gem_tooltip::{self, GemTooltipOptions, TooltipGemData, TooltipGemInstance};
use crate::main_mode::folders::stat_color;
use crate::model::{self, gamedata, Key, LjIntMap, Output, Shared, Table, Val};
use crate::tabs::compare::init::CompareEntryRef;
// CCR-0120 §3 / CCR-0125 §5: model.rs (U15) re-exports neither ItemTools nor ModCfg
use pob_calc::cfg::ModCfg;
use pob_calc::classes::item_tools::{formatModLine, influenceInfo_all};
use pob_calc::modules::common::copyTableSafe;

// ---- the state -------------------------------------------------------------------------------

/// `CreateUndoState`'s table (L5022-5035). `items` and `itemSets` are the copies Lua makes;
/// `slotSelItemId[slotName]` is the slot's `selItemId` value.
pub struct UndoState {
    pub activeItemSetId: f64,
    pub items: indexmap::IndexMap<u32, model::ItemRef>,
    pub itemOrderList: Vec<u32>,
    pub slotSelItemId: indexmap::IndexMap<Name, Val>,
    pub itemSets: indexmap::IndexMap<u32, Table>,
    pub itemSetOrderList: Vec<u32>,
}

/// U38's part of the `ItemsTab` object (ui doc §12.1): the UndoHandler state.
#[derive(Default)]
pub struct State {
    /// `self.undo`, `self.redo`, `self.modFlag` (UndoHandler.lua:17-21)
    pub undo: UndoStack<UndoState>,
}

fn undo_stack(cx: &mut Cx, me: CtrlId) -> &mut UndoStack<UndoState> {
    &mut cx.w_mut::<ItemsTab>(me).tooltip.undo
}

/// The ItemsTab's UndoHandler "class" (ui doc §10.3).
pub const UNDO_OPS: UndoOps<UndoState> = UndoOps {
    stack: undo_stack,
    create: CreateUndoState,
    restore: RestoreUndoState,
};

// ---- Rust-only plumbing ----------------------------------------------------------------------

/// `self` for a method called from outside the tab (`build.itemsTab`): the ItemsTab node.
#[track_caller]
fn tab(cx: &Cx) -> CtrlId {
    slot_helper::items_tab(cx)
        .unwrap_or_else(|| lua_error!("attempt to index field 'itemsTab' (a nil value)"))
}

/// `self` of this file's ItemsTab methods: the ItemsTab whose `self.build` and `self.slots` a
/// call means (FUP6; module notes "The ItemsTab of another build").
#[derive(Clone)]
pub enum ItemsTabSelf {
    /// the open build's ItemsTab node (`self.slots` its ItemSlotControls)
    Open(CtrlId),
    /// `compareEntry.itemsTab` (CompareEntry.lua:113): `self.build` is the entry, `self.slots`
    /// the entry model's slot tables (calc:BM-A's model half of the ItemSlotControls; the entry
    /// has no slot controls)
    Entry(CompareEntryRef),
}

impl From<CtrlId> for ItemsTabSelf {
    fn from(id: CtrlId) -> ItemsTabSelf {
        ItemsTabSelf::Open(id)
    }
}

impl ItemsTabSelf {
    /// `build.itemsTab` for the model build `build`: a comparison entry's ItemsTab when `build`
    /// is one of the Compare tab's entries, else the open build's.
    #[track_caller]
    pub fn of_build(cx: &Cx, build: &model::BuildRef) -> ItemsTabSelf {
        let entry = crate::tabs::compare::init::tab(cx).and_then(|ct| {
            crate::tabs::compare::init::st(cx, ct)
                .compareEntries
                .iter()
                .find(|e| Rc::ptr_eq(&e.model, build))
                .cloned()
        });
        match entry {
            Some(e) => ItemsTabSelf::Entry(e),
            None => ItemsTabSelf::Open(tab(cx)),
        }
    }

    /// `f` with `self.build` as `cx.build()`: an entry's methods run with the entry swapped in
    /// for the open build (`compare::entry::with_entry_as_build`, which does nothing when the
    /// entry is swapped in already).
    fn run<R>(&self, cx: &mut Cx, f: impl FnOnce(&mut Cx) -> R) -> R {
        match self {
            ItemsTabSelf::Open(_) => f(cx),
            ItemsTabSelf::Entry(e) => crate::tabs::compare::entry::with_entry_as_build(cx, e, f),
        }
    }

    /// `pairs(self.slots)`: `(slotName, slot)` with the slot's model table, in slot creation
    /// order (the module docs' ORDER notes).
    fn slots(&self, cx: &Cx) -> Vec<(Name, Table, Option<CtrlId>)> {
        match self {
            ItemsTabSelf::Open(me) => cx
                .w::<ItemsTab>(*me)
                .ctor
                .slots
                .iter()
                .map(|(k, id)| (*k, cx.w::<ItemSlotControl>(*id).slot.clone(), Some(*id)))
                .collect(),
            ItemsTabSelf::Entry(e) => e
                .model
                .itemsTab
                .slots
                .with(|s| s.iter().map(|(k, t)| (*k, t.clone(), None)).collect()),
        }
    }

    /// `slot.shown()` of a slot of [`Self::slots`]: the control's `shown`, or for an entry slot
    /// the same closures over the model table (calc:BM-A `slot_IsShown`).
    fn slot_shown(&self, cx: &Cx, slot: &Table, id: Option<CtrlId>) -> bool {
        match (self, id) {
            (ItemsTabSelf::Open(_), Some(id)) => cx.base(id).shown.get(cx, id),
            (ItemsTabSelf::Entry(e), _) => model::items_tab::slot_IsShown(&e.model, slot),
            (ItemsTabSelf::Open(_), None) => unreachable!("an open-build slot without its control"),
        }
    }

    /// `self.slots[slotName]` (the slot's model table)
    fn slot_named(&self, cx: &Cx, slot_name: &str) -> Option<Table> {
        match self {
            ItemsTabSelf::Open(me) => {
                slot_control(cx, *me, slot_name).map(|id| cx.w::<ItemSlotControl>(id).slot.clone())
            }
            ItemsTabSelf::Entry(e) => e
                .model
                .itemsTab
                .slots
                .with(|s| s.get(&Name::from(slot_name)).cloned()),
        }
    }

    /// `self.build:AddStatComparesToTooltip(tooltip, calcBase, output, header)`: BuildMode's
    /// (Build.lua:2168-2180) or CompareEntry's (CompareEntry.lua:427-442).
    fn add_stat_compares(
        &self,
        cx: &Cx,
        tooltip: &mut Tooltip,
        base: &Output,
        output: &Output,
        header: &str,
    ) -> f64 {
        match self {
            ItemsTabSelf::Open(_) => {
                build_stats::add_stat_compares_to_tooltip(cx, tooltip, base, output, header, None)
            }
            ItemsTabSelf::Entry(e) => {
                crate::tabs::compare::entry::CompareEntry::AddStatComparesToTooltip(
                    cx, e, tooltip, base, output, header, None,
                )
            }
        }
    }

    /// `self.build:AddRequirementsToTooltip(tooltip, level, str, dex, int, strBase, dexBase,
    /// intBase)`: BuildMode's (Build.lua:2185-2218) or CompareEntry's (CompareEntry.lua:514-550).
    #[allow(clippy::too_many_arguments)]
    fn add_requirements(
        &self,
        cx: &Cx,
        tooltip: &mut Tooltip,
        level: Option<f64>,
        str: Option<f64>,
        dex: Option<f64>,
        int: Option<f64>,
        strBase: Option<f64>,
        dexBase: Option<f64>,
        intBase: Option<f64>,
    ) {
        match self {
            ItemsTabSelf::Open(_) => build_stats::add_requirements_to_tooltip(
                cx, tooltip, level, str, dex, int, strBase, dexBase, intBase,
            ),
            ItemsTabSelf::Entry(e) => {
                crate::tabs::compare::entry::CompareEntry::AddRequirementsToTooltip(
                    cx, e, tooltip, level, str, dex, int, strBase, dexBase, intBase,
                );
            }
        }
    }
}

/// `item.<k>` of an untyped Item field.
fn ex(item: &model::Item, k: Name) -> Val {
    item.extra.get(k)
}

/// Lua `..` on a value (strings and numbers; anything else raises).
#[track_caller]
fn cs(v: &Val) -> String {
    match v {
        Val::Str(s) => s.to_string(),
        Val::Num(x) => pob_lua::tostring_num(*x),
        other => lua_error!("attempt to concatenate a {} value", other.lua_type()),
    }
}

/// A number field used in arithmetic, a comparison or a `%d`/`%f` format (nil raises).
#[track_caller]
fn tnum(t: &Table, k: &str) -> f64 {
    match t.get(Name::from(k)) {
        Val::Num(x) => x,
        other => lua_error!(
            "attempt to perform arithmetic on field '{k}' (a {} value)",
            other.lua_type()
        ),
    }
}

/// `t.<k> ~= 0` (no error on nil).
fn ne0(t: &Table, k: &str) -> bool {
    !t.get(Name::from(k)).raw_eq(&Val::Num(0.0))
}

/// An optional number used in a comparison (`x > 0` with nil raises).
#[track_caller]
fn onum(v: Option<f64>, what: &str) -> f64 {
    v.unwrap_or_else(|| lua_error!("attempt to compare nil with number ({what})"))
}

/// `slot.<k>` for a Lua `slot` value (a string has no fields; nil/false are not indexed by
/// the callers, which test them first).
fn slot_get(slot: &Val, k: Name) -> Val {
    match slot {
        Val::Table(t) => t.get(k),
        _ => Val::Nil,
    }
}

/// `t[v]` for a Lua key value (a nil or NaN key reads nil).
fn tget(t: &Table, k: &Val) -> Val {
    match k {
        Val::Num(x) => Key::try_num(*x).map_or(Val::Nil, |k| t.get(k)),
        Val::Str(s) => t.get(Name::from(&**s as &str)),
        Val::Bool(b) => t.get(Key::B(*b)),
        _ => Val::Nil,
    }
}

/// The slot control of `self.slots[slotName]`.
fn slot_control(cx: &Cx, me: CtrlId, slot_name: &str) -> Option<CtrlId> {
    cx.w::<ItemsTab>(me)
        .ctor
        .slots
        .get(&Name::from(slot_name))
        .copied()
}

/// `self.build.spec`
fn spec(cx: &Cx) -> model::PassiveSpecRef {
    cx.build().model.spec.get()
}

/// `self.build.calcsTab.mainEnv`
#[track_caller]
fn main_env(cx: &Cx) -> model::EnvRef {
    cx.build()
        .model
        .calcsTab
        .mainEnv
        .get()
        .unwrap_or_else(|| lua_error!("attempt to index field 'mainEnv' (a nil value)"))
}

/// `{ actor = "player" }`
fn cfg_player() -> ModCfg {
    let c = ModCfg::default();
    c.actor.set(Some(n!("player")));
    c
}

/// `copyTableSafe(item, true, true)` on an Item: a new Item object with the same field values
/// (see DIVERGENCE(item-copy)).
pub fn copy_item(src: &model::Item) -> model::Item {
    model::Item {
        id: src.id.clone(),
        raw: src.raw.clone(),
        name: src.name.clone(),
        title: src.title.clone(),
        baseName: src.baseName.clone(),
        base: src.base.clone(),
        r#type: src.r#type.clone(),
        rarity: src.rarity.clone(),
        uniqueID: src.uniqueID.clone(),
        corrupted: src.corrupted.clone(),
        mirrored: src.mirrored.clone(),
        fractured: src.fractured.clone(),
        split: src.split.clone(),
        synthesised: src.synthesised.clone(),
        // raw keys of the item (`item.shaper = true`): copied
        influences: src.influences.copyTable(true),
        quality: src.quality.clone(),
        itemLevel: src.itemLevel.clone(),
        sockets: src.sockets.clone(),
        abyssalSocketCount: src.abyssalSocketCount.clone(),
        variant: src.variant.clone(),
        variantAlt: src.variantAlt.clone(),
        variantAlt2: src.variantAlt2.clone(),
        variantAlt3: src.variantAlt3.clone(),
        variantAlt4: src.variantAlt4.clone(),
        variantAlt5: src.variantAlt5.clone(),
        variantList: src.variantList.clone(),
        explicitModLines: src.explicitModLines.clone(),
        implicitModLines: src.implicitModLines.clone(),
        enchantModLines: src.enchantModLines.clone(),
        scourgeModLines: src.scourgeModLines.clone(),
        crucibleModLines: src.crucibleModLines.clone(),
        buffModLines: src.buffModLines.clone(),
        classRequirementModLines: src.classRequirementModLines.clone(),
        baseModList: src.baseModList.clone(),
        modList: src.modList.clone(),
        slotModList: [
            src.slotModList[0].clone(),
            src.slotModList[1].clone(),
            src.slotModList[2].clone(),
        ],
        buffModList: src.buffModList.clone(),
        weaponData: src.weaponData.clone(),
        armourData: src.armourData.clone(),
        flaskData: src.flaskData.clone(),
        tinctureData: src.tinctureData.clone(),
        jewelData: src.jewelData.clone(),
        jewelRadiusIndex: src.jewelRadiusIndex.clone(),
        jewelRadiusLabel: src.jewelRadiusLabel.clone(),
        jewelRadiusData: src.jewelRadiusData.clone(),
        clusterJewel: src.clusterJewel.clone(),
        clusterJewelSkill: src.clusterJewelSkill.clone(),
        clusterJewelNodeCount: src.clusterJewelNodeCount.clone(),
        catalyst: src.catalyst.clone(),
        catalystQuality: src.catalystQuality.clone(),
        talismanTier: src.talismanTier.clone(),
        // `item.requirements` is a sub-table: shared
        requirements: src.requirements.clone(),
        grantedSkills: src.grantedSkills.clone(),
        classRestriction: src.classRestriction.clone(),
        limit: src.limit.clone(),
        modSource: src.modSource.clone(),
        // the other raw keys of the item: copied (their table values shared)
        extra: src.extra.copyTable(true),
    }
}

/// `flavourLookup[name][id] = text` (ItemsTab.lua:62-69; U38's tooltips) with `text` the list of
/// lines (CCR-0125 §1), built once from `data.flavourText` (a dump root kept untyped,
/// `Data::other`).
/// ORDER: (c) the loop only fills a keyed map; `pairs` order cannot change it except for two
/// entries with the same name and id, which the data does not have (the later one would win in
/// PoB's random order; here the dump's order).
/// @pob Classes/ItemsTab.lua:62-69 h=fff7175d
pub fn flavour_lookup(
) -> &'static indexmap::IndexMap<String, indexmap::IndexMap<String, Vec<String>>> {
    type Lookup = indexmap::IndexMap<String, indexmap::IndexMap<String, Vec<String>>>;
    static LOOKUP: OnceLock<Lookup> = OnceLock::new();
    LOOKUP.get_or_init(|| {
        use gamedata::DataValue;
        // L62
        let mut flavourLookup: Lookup = indexmap::IndexMap::new();
        let Some(DataValue::Table(t)) = model::data().other.get("data.flavourText") else {
            return flavourLookup;
        };
        let text_of = |v: &DataValue| match v {
            DataValue::Str(s) => Some(s.to_string()),
            DataValue::Num(x) => Some(pob_lua::tostring_num(*x)),
            _ => None,
        };
        // L64 `for _, entry in pairs(data.flavourText)` (ORDER: above)
        for entry in t.seq.iter().chain(t.fields.iter().map(|(_, v)| v)) {
            let DataValue::Table(entry) = entry else {
                continue;
            };
            // L65
            let (Some(name), Some(id), DataValue::Table(text)) = (
                text_of(entry.field("name")),
                text_of(entry.field("id")),
                entry.field("text"),
            ) else {
                continue;
            };
            // `ipairs(flavour)` (L4643) stops at the first non-string
            let lines: Vec<String> = text.seq.iter().map_while(text_of).collect();
            // L66-67
            flavourLookup.entry(name).or_default().insert(id, lines);
        }
        flavourLookup
    })
}

// ---- SetTooltipHeaderInfluence, FormatItemSource ---------------------------------------------

/// lua: ItemsTab.lua:3993-4062 `ItemsTabClass:SetTooltipHeaderInfluence(tooltip, item)`
/// @pob Classes/ItemsTab.lua:3993-4063 h=139d0cb1
pub fn SetTooltipHeaderInfluence(tooltip: &mut Tooltip, item: &model::Item) {
    // L3994
    tooltip.influence_header1 = None;
    // L3995
    tooltip.influence_header2 = None;

    // L3997
    let addInfluence = |tooltip: &mut Tooltip, name: &str| {
        // L3998
        if tooltip.influence_header1.is_none() {
            // L3999
            tooltip.influence_header1 = Some(name.to_owned());
        } else if tooltip.influence_header2.is_none() {
            // L4001
            tooltip.influence_header2 = Some(name.to_owned());
        }
    };
    let inf = |k: &str| item.influences.truthy(Name::from(k));

    // Eater and Exarch combo takes priority over fractured icon.
    // L4006
    if inf("cleansing") && inf("tangle") {
        // L4007
        addInfluence(tooltip, "Exarch");
        // L4008
        addInfluence(tooltip, "Eater");
    } else {
        // Dual influence with fracture will show fractured icon and highest priority influence.
        // L4011
        if item.fractured.get() {
            // L4012
            addInfluence(tooltip, "Fractured");
        }
        // Replica Eternity Shroud has Experimented icon and Shaper icon on the right.
        // L4015
        if item
            .title
            .get()
            .is_some_and(|t| pob_lua::find(&t, "Replica", None, false).is_some())
        {
            // L4016
            addInfluence(tooltip, "Experimented");
        }
        // L4018
        if ex(item, n!("foulborn")).truthy() {
            addInfluence(tooltip, "Foulborn");
        }
        // L4021
        if ex(item, n!("veiled")).truthy() {
            addInfluence(tooltip, "Veiled");
        }
        // L4024
        if inf("cleansing") {
            addInfluence(tooltip, "Exarch");
        }
        // L4027
        if inf("tangle") {
            addInfluence(tooltip, "Eater");
        }
        // L4030
        if inf("shaper") {
            addInfluence(tooltip, "Shaper");
        }
        // L4033
        if inf("elder") {
            addInfluence(tooltip, "Elder");
        }
        // L4036
        if inf("crusader") {
            addInfluence(tooltip, "Crusader");
        }
        // L4039
        if inf("eyrie") {
            addInfluence(tooltip, "Redeemer");
        }
        // L4042
        if inf("basilisk") {
            addInfluence(tooltip, "Hunter");
        }
        // L4045
        if inf("adjudicator") {
            addInfluence(tooltip, "Warlord");
        }
        // L4048
        if ex(item, n!("vestigial")).truthy() {
            addInfluence(tooltip, "Vestigial");
        }
        // L4051
        if item.synthesised.get() && tooltip.influence_header1.is_none() {
            addInfluence(tooltip, "Synthesis");
        }
        // L4054
        if ex(item, n!("memoryStrands")).truthy() && tooltip.influence_header1.is_none() {
            addInfluence(tooltip, "Memory");
        }
    }

    // L4059
    if tooltip.influence_header1.is_some() && tooltip.influence_header2.is_none() {
        // L4060
        tooltip.influence_header2 = tooltip.influence_header1.clone();
    }
}

/// lua: ItemsTab.lua:4064-4069 `ItemsTabClass:FormatItemSource(text)` (the callers
/// concatenate the result: the string only)
/// @pob Classes/ItemsTab.lua:4064-4070 h=04a6d089
pub fn FormatItemSource(cx: &Cx, text: &str) -> String {
    let cc = &cx.main.cc;
    // L4065
    let text = luastr::gsub(
        text,
        "unique{([^}]+)}",
        &format!("{}%1{}", cc.UNIQUE, cc.SOURCE),
    );
    // L4066
    let text = luastr::gsub(
        &text,
        "normal{([^}]+)}",
        &format!("{}%1{}", cc.NORMAL, cc.SOURCE),
    );
    // L4067
    let text = luastr::gsub(
        &text,
        "currency{([^}]+)}",
        &format!("{}%1{}", cc.CURRENCY, cc.SOURCE),
    );
    // L4068
    luastr::gsub(
        &text,
        "prophecy{([^}]+)}",
        &format!("{}%1{}", cc.PROPHECY, cc.SOURCE),
    )
}

// ---- the jewel comparison spec ---------------------------------------------------------------

/// lua: ItemsTab.lua:4071-4075 `local function itemChangesPassiveTree(item)`
/// @pob Classes/ItemsTab.lua:4071-4076 h=a7cb4540
fn itemChangesPassiveTree(item: Option<&model::ItemRef>) -> bool {
    // L4072
    let Some(item) = item else { return false };
    if &*item.r#type.get() != "Jewel" {
        return false;
    }
    let Some(jewelData) = item.jewelData.get() else {
        return false;
    };
    // L4073-4074
    jewelData.truthy(n!("conqueredBy"))
        || (item.jewelRadiusIndex.get().is_some()
            && (jewelData.truthy(n!("intuitiveLeapLike"))
                || jewelData.truthy(n!("impossibleEscapeKeystone"))))
}

// These jewels can replace passive nodes or disconnect allocated passives, so
// rebuild the passive tree before comparing their stats.
// Keep this list in sync with PassiveSpec's constructor, Init, and Select*
// methods; omitted fields fail safe as nil on the comparison spec.
/// `sharedSpecKeysForJewelComparison` (L4081-4098): the keys `cloneSpecForJewelComparison`
/// copies. The typed `PassiveSpec` fields among them are assigned by name in that function;
/// these are the ones the calc keeps in `spec.extra`.
/// @pob Classes/ItemsTab.lua:4077-4099 h=14825bd4
const sharedSpecKeysForJewelComparison_extra: [&str; 5] = [
    // L4086
    "ignoreAllocatingSubgraph",
    // L4087
    "clusterHashFormatVersion",
    // L4089
    "curClass",
    // L4092
    "curAscendClass",
    // L4096
    "curSecondaryAscendClass",
];

/// lua: ItemsTab.lua:4100-4151 `local function cloneSpecForJewelComparison(spec)`
/// @pob Classes/ItemsTab.lua:4100-4152 h=223c13e0
fn cloneSpecForJewelComparison(spec: &model::PassiveSpec) -> model::PassiveSpec {
    use pob_calc::classes::passive_spec::copy_node;
    // L4101 `setmetatable({ }, getmetatable(spec))`
    // Share only immutable/scalar spec state. Tables that BuildAllDependsAndPaths
    // may mutate must be owned by the comparison spec.
    // L4104-4106 (`build`, `treeVersion`, `tree`, `title`, the class fields; the rest of the
    // spec reads nil, which the typed fields hold as their defaults)
    let extra = Table::new();
    for key in sharedSpecKeysForJewelComparison_extra {
        extra.set(Name::from(key), spec.extra.get(Name::from(key)));
    }

    // L4108
    let mut nodes: LjIntMap<model::NodeRef> = LjIntMap::new();
    // L4109
    for (id, node) in spec.nodes.with(LjIntMap::pairs) {
        // L4110 `setmetatable({ }, getmetatable(node))`; L4111-4116 every field except
        // linked, depends, intuitiveLeapLikesAffecting, path and power
        let nodeCopy = copy_node(&node);
        nodeCopy.path.set(None);
        // L4117
        nodeCopy.alloc.set(false);
        // L4118
        nodeCopy.linked.set(Shared::new(Vec::new()));
        // L4119
        nodeCopy.depends.set(Shared::new(Vec::new()));
        // L4120
        nodeCopy
            .extra
            .set(n!("intuitiveLeapLikesAffecting"), Table::new());
        // L4121
        nodeCopy.extra.set(n!("power"), Table::new());
        // L4122
        nodes.set(id, Some(Rc::new(nodeCopy)));
    }
    // L4124
    for (id, nodeCopy) in nodes.pairs() {
        // L4125
        let src = spec.nodes.with(|m| m.get(id));
        for linkedNode in src.map(|n| n.linked.get().snapshot()).unwrap_or_default() {
            // L4126
            let linkedCopy = if linkedNode.id.fract() == 0.0 {
                nodes.get(linkedNode.id as i64)
            } else {
                None
            };
            // L4127
            if let Some(linkedCopy) = linkedCopy {
                // L4128
                nodeCopy.linked.get().push(linkedCopy);
            }
        }
    }

    // L4133
    let mut allocNodes: LjIntMap<model::NodeRef> = LjIntMap::new();
    // L4134
    for id in spec.allocNodes.with(LjIntMap::keys) {
        // L4135
        // L4136
        if let Some(nodeCopy) = nodes.get(id) {
            // L4137
            nodeCopy.alloc.set(true);
            // L4138
            allocNodes.set(id, Some(nodeCopy));
        }
    }
    // (PassiveSpec has a Drop impl, so the copy is a default spec whose fields are assigned)
    let mut specCopy = model::PassiveSpec::default();
    // L4104-4106 (sharedSpecKeysForJewelComparison: build, treeVersion, tree, title, curClassId,
    // curClassName, curAscendClassId, curAscendClassName, curAscendClassBaseName,
    // curSecondaryAscendClassId, curSecondaryAscendClassName, and `extra` above)
    specCopy.build = spec.build.clone();
    specCopy.title = spec.title.clone();
    specCopy.tree = spec.tree.clone();
    specCopy.treeVersion = spec.treeVersion.clone();
    specCopy.curClassId = spec.curClassId.clone();
    specCopy.curClassName = spec.curClassName.clone();
    specCopy.curAscendClassId = spec.curAscendClassId.clone();
    specCopy.curAscendClassName = spec.curAscendClassName.clone();
    specCopy.curAscendClassBaseName = spec.curAscendClassBaseName.clone();
    specCopy.curSecondaryAscendClassId = spec.curSecondaryAscendClassId.clone();
    specCopy.curSecondaryAscendClassName = spec.curSecondaryAscendClassName.clone();
    specCopy.extra = extra;
    // L4108
    specCopy.nodes = Shared::new(nodes);
    // L4133
    specCopy.allocNodes = Shared::new(allocNodes);
    // L4141
    specCopy.jewels = Shared::new(spec.jewels.with(LjIntMap::copyTable));
    // L4142
    specCopy.masterySelections = Shared::new(spec.masterySelections.with(LjIntMap::copyTable));
    // L4143
    specCopy.hashOverrides = Shared::new(spec.hashOverrides.with(LjIntMap::copyTable));
    // L4144
    specCopy.ignoredNodes = spec.ignoredNodes.copyTable(true);
    // L4145
    specCopy.splitPersonalityPath = pob_lua::Field::new(Some(Table::new()));
    // L4146
    specCopy.allocSubgraphNodes = Shared::new(Vec::new());
    // L4147
    specCopy.allocExtendedNodes = Shared::new(Vec::new());
    // L4148
    specCopy.subGraphs = Table::new();
    // L4150
    specCopy
}

/// Removes the temporary item of `buildSpecForJewelComparison` when dropped (L4177-4179; also
/// on a Lua error, which `xpcall` catches before re-raising it).
struct TempItemGuard {
    build: model::BuildRef,
    id: Option<u32>,
}

impl Drop for TempItemGuard {
    fn drop(&mut self) {
        // L4177
        if let Some(id) = self.id {
            // L4178
            self.build.itemsTab.items.with_mut(|m| m.shift_remove(&id));
        }
    }
}

/// lua: ItemsTab.lua:4156-4183 `local function buildSpecForJewelComparison(itemsTab,
/// compareSlot, replacementItem)` (`compareSlot` is the slot's model table)
/// @pob Classes/ItemsTab.lua:4153-4184 h=e1136d49
fn buildSpecForJewelComparison(
    cx: &Cx,
    compareSlot: &Table,
    replacementItem: Option<&model::ItemRef>,
) -> model::PassiveSpecRef {
    let build = cx.build().model.clone();
    // L4157
    let mut tempItemId: Option<u32> = None;
    // L4158
    let spec = cloneSpecForJewelComparison(&build.spec.get());
    let nodeId = compareSlot.get(n!("nodeId"));
    let nodeKey = nodeId
        .opt_num()
        .filter(|x| x.fract() == 0.0)
        .map(|x| x as i64)
        .unwrap_or_else(|| lua_error!("table index is nil"));
    // L4159
    if let Some(replacementItem) = replacementItem {
        // L4160
        let live = replacementItem
            .id
            .get()
            .and_then(model::items_tab::id_key)
            .filter(|k| {
                build
                    .itemsTab
                    .items
                    .with(|m| m.get(k).is_some_and(|l| Rc::ptr_eq(l, replacementItem)))
            });
        if let Some(k) = live {
            // L4161
            spec.jewels.with_mut(|m| m.set(nodeKey, Some(f64::from(k))));
        } else {
            // L4163 DIVERGENCE(temp-item-id): counting down from u32::MAX, not -1
            let mut id = u32::MAX;
            // L4164
            while build.itemsTab.items.with(|m| m.contains_key(&id)) {
                // L4165
                id -= 1;
            }
            // L4167
            build
                .itemsTab
                .items
                .with_mut(|m| m.insert(id, replacementItem.clone()));
            tempItemId = Some(id);
            // L4168
            spec.jewels
                .with_mut(|m| m.set(nodeKey, Some(f64::from(id))));
        }
    } else {
        // L4171
        spec.jewels.with_mut(|m| m.set(nodeKey, None));
    }
    // L4173-4176 xpcall(spec:BuildAllDependsAndPaths, debug.traceback); L4177-4182: the guard
    // removes the temporary item, then a Lua error keeps unwinding (`error(err, 0)`)
    let guard = TempItemGuard {
        build: build.clone(),
        id: tempItemId,
    };
    let spec = Rc::new(spec);
    spec.BuildAllDependsAndPaths();
    drop(guard);
    // L4183
    spec
}

// ---- GetSocketDescriptionLine ----------------------------------------------------------------

/// lua: ItemsTab.lua:4185-4212 `ItemsTabClass:GetSocketDescriptionLine(item)`
/// @pob Classes/ItemsTab.lua:4185-4213 h=294a7d2d
pub fn GetSocketDescriptionLine(cx: &Cx, item: &model::Item) -> String {
    let cc = &cx.main.cc;
    // Sockets/links
    // L4187
    let mut group = Val::Num(0.0);
    // L4188
    let mut line = String::new();
    // L4189
    for (i0, socket) in item.sockets.get().snapshot().iter().enumerate() {
        // L4190
        if i0 + 1 > 1 {
            // L4191
            if socket.get(n!("group")).raw_eq(&group) {
                // L4192
                line += "^7=";
            } else {
                // L4194
                line += "  ";
            }
            // L4196
            group = socket.get(n!("group"));
        }
        // L4198-4209
        let color = socket.str(n!("color"));
        let code: Option<&str> = match color.as_deref() {
            Some("R") => Some(&cc.STRENGTH),
            Some("G") => Some(&cc.DEXTERITY),
            Some("B") => Some(&cc.INTELLIGENCE),
            Some("W") => Some(&cc.SCION),
            Some("A") => Some("^xB0B0B0"),
            _ => None,
        };
        // L4210
        let Some(code) = code else {
            lua_error!("attempt to concatenate local 'code' (a nil value)")
        };
        line += code;
        line += &cs(&socket.get(n!("color")));
    }
    // L4212
    line
}

// ---- AddItemTooltip --------------------------------------------------------------------------

/// The `modLine` argument of `tooltip:AddLine` (CCR-0073 B): `disabled` reads the model table
/// live; `line` is the mod line table (U34's Draw reads it back).
fn mod_line_ref(modLine: &Table) -> ModLineRef {
    let t = modLine.clone();
    Rc::new(TtModLine {
        disabled: Rc::new(move || t.truthy(n!("disabled"))),
        line: Rc::new(modLine.clone()),
    })
}

/// lua: ItemsTab.lua:4214-4659 `ItemsTabClass:AddItemTooltip(tooltip, item, slot, dbMode,
/// maxWidth)` (`me` is `self`, the open build's ItemsTab node or a comparison entry's
/// [`ItemsTabSelf`]; `slot` see the module docs)
/// @pob Classes/ItemsTab.lua:4214-4660 h=bcd759f6
#[allow(clippy::too_many_arguments)]
pub fn AddItemTooltip(
    cx: &mut Cx,
    me: impl Into<ItemsTabSelf>,
    tooltip: &mut Tooltip,
    item: &model::ItemRef,
    slot: &Val,
    dbMode: bool,
    maxWidth: Option<f64>,
) {
    let me: ItemsTabSelf = me.into();
    // `self.build` is the entry for an entry's ItemsTab (FUP6)
    me.clone().run(cx, |cx| {
        AddItemTooltip_as(cx, &me, tooltip, item, slot, dbMode, maxWidth);
    });
}

/// [`AddItemTooltip`]'s body, run with `self.build` as `cx.build()`.
#[allow(clippy::too_many_arguments)]
fn AddItemTooltip_as(
    cx: &mut Cx,
    me: &ItemsTabSelf,
    tooltip: &mut Tooltip,
    item: &model::ItemRef,
    slot: &Val,
    dbMode: bool,
    maxWidth: Option<f64>,
) {
    let fs = Some(Font::FontinSc);
    let cc = cx.main.cc.clone();
    let showFlavourText = cx.main.show_flavour_text;
    // L4215
    let fontSizeSmall = if showFlavourText { 16.0 } else { 14.0 };
    let _ = fontSizeSmall;
    // L4216
    let fontSizeBig = if showFlavourText { 18.0 } else { 16.0 };
    // L4217
    let fontSizeTitle = if showFlavourText { 22.0 } else { 20.0 };
    // L4218
    let rarity = item.rarity.get();
    let rarityCode: Option<String> = cc.get(&rarity).cloned();
    let rc = || {
        rarityCode.clone().unwrap_or_else(|| {
            lua_error!("attempt to concatenate local 'rarityCode' (a nil value)")
        })
    };
    // L4219 Cap very long lines. Can use a narrower width for small viewports
    tooltip.max_width = Some(m_min(maxWidth.unwrap_or(600.0), 600.0));
    // L4220
    tooltip.tooltip_header = Some(rarity.to_string());
    // L4221
    tooltip.foil_type = ex(item, n!("foilType")).str().map(|s| s.to_string());
    // L4222
    tooltip.center = true;
    // L4223
    tooltip.color = rarityCode.clone().map(TtColor::Esc);
    // Shared items can use old base names that no longer exist. Add a tooltip so they can be copied or removed without causing a crash.
    // L4225
    if item.base.get().is_none() || item.baseName.get().is_none() {
        // L4226
        let title = item
            .title
            .get()
            .map(|t| t.to_string())
            .or_else(|| Some(item.name.get().to_string()))
            .unwrap_or_else(|| "Unknown Item".to_owned());
        tooltip.add_line_ex(
            cx,
            fontSizeTitle,
            &format!("{}{title}", rc()),
            fs,
            None,
            None,
        );
        // L4227
        tooltip.add_separator(cx, Some(30.0));
        // L4228
        tooltip.add_line_ex(
            cx,
            fontSizeTitle,
            &format!(
                "{}Item base is not supported by the current version.",
                cc.NEGATIVE
            ),
            fs,
            None,
            None,
        );
        // L4229
        return;
    }
    // L4231
    SetTooltipHeaderInfluence(tooltip, item);
    // Item name
    let baseName = item.baseName.get().expect("checked at L4225");
    // L4233
    if let Some(title) = item.title.get() {
        // L4234
        tooltip.add_line_ex(
            cx,
            fontSizeTitle,
            &format!("{}{title}", rc()),
            fs,
            None,
            None,
        );
        // L4235
        let bn = luastr::gsub(&baseName, " %(.+%)", "");
        tooltip.add_line_ex(cx, fontSizeTitle, &format!("{}{bn}", rc()), fs, None, None);
    } else {
        // L4237
        let bn = luastr::gsub(&baseName, " %(.+%)", "");
        let text = format!(
            "{}{}{bn}{}",
            rc(),
            cs(&ex(item, n!("namePrefix"))),
            cs(&ex(item, n!("nameSuffix")))
        );
        tooltip.add_line_ex(cx, fontSizeTitle, &text, fs, None, None);
    }
    // L4239
    for curInfluenceInfo in &influenceInfo_all {
        // L4240
        if item.influences.truthy(Name::from(curInfluenceInfo.key)) && !showFlavourText {
            // L4241
            let text = format!(
                "{}{} Item",
                curInfluenceInfo.color(),
                curInfluenceInfo.display
            );
            tooltip.add_line_ex(cx, fontSizeBig, &text, fs, None, None);
        }
    }
    // L4244
    if item.fractured.get() && !showFlavourText {
        // L4245
        tooltip.add_line(cx, fontSizeBig, &format!("{}Fractured Item", cc.FRACTURED));
    }
    // L4247
    if item.synthesised.get() && !showFlavourText {
        // L4248
        tooltip.add_line(cx, fontSizeBig, &format!("{}Synthesised Item", cc.CRAFTED));
    }
    // L4250
    tooltip.add_separator(cx, Some(10.0));

    // Special fields for database items
    // L4253
    if dbMode {
        // L4254
        if ex(item, n!("usesVariantGroups")).truthy() {
            // L4255
            let versionList = ex(item, n!("versionList"));
            let selectedVersion = ex(item, n!("selectedVersion"));
            if versionList.truthy() && selectedVersion.truthy() {
                // L4256
                let Val::Table(vl) = &versionList else {
                    lua_error!(
                        "attempt to index field 'versionList' (a {} value)",
                        versionList.lua_type()
                    )
                };
                let text = format!("^xFFFF30Version: {}", cs(&tget(vl, &selectedVersion)));
                tooltip.add_line_ex(cx, fontSizeBig, &text, fs, None, None);
            }
            // L4258
            let mut selectedVariants: Vec<String> = Vec::new();
            // L4259
            let variantGroups = ex(item, n!("variantGroups"));
            let Val::Table(variantGroups) = variantGroups else {
                lua_error!(
                    "bad argument #1 to 'pairs' (table expected, got {})",
                    variantGroups.lua_type()
                )
            };
            for groupId in pairs_sort_by_key(&variantGroups) {
                // L4260
                let selections = ex(item, n!("variantGroupSelections"));
                let variantId = match &selections {
                    Val::Table(t) => t.get(groupId),
                    other => lua_error!(
                        "attempt to index field 'variantGroupSelections' (a {} value)",
                        other.lua_type()
                    ),
                };
                // L4261
                if let Val::Num(vid) = variantId {
                    let gid = groupId.as_num().unwrap_or(f64::NAN);
                    if item.IsVariantGroupOptionEligible(gid, vid) {
                        // L4262 (t_insert of a nil appends nothing)
                        let v = variant_at(item, vid);
                        if let Some(v) = v {
                            selectedVariants.push(v);
                        }
                    }
                }
            }
            // L4265
            if !selectedVariants.is_empty() {
                // L4266
                let text = format!("^xFFFF30Variants: {}", selectedVariants.join(", "));
                tooltip.add_line_ex(cx, fontSizeBig, &text, fs, None, None);
            }
        } else if let Some(variantList) = item.variantList.get() {
            // L4269
            if variantList.len() == 1 {
                // L4270
                let text = format!(
                    "^xFFFF30Variant: {}",
                    variantList
                        .get(1)
                        .map(|s| s.to_string())
                        .unwrap_or_default()
                );
                tooltip.add_line_ex(cx, fontSizeBig, &text, fs, None, None);
            } else {
                // L4272
                let v = item
                    .variant
                    .get()
                    .and_then(|v| variant_at(item, v))
                    .unwrap_or_else(|| {
                        lua_error!("attempt to concatenate field '?' (a nil value)")
                    });
                let text = format!("^xFFFF30Variant: {v} ({} variants)", variantList.len());
                tooltip.add_line_ex(cx, fontSizeBig, &text, fs, None, None);
            }
        }
        // L4275
        let league = ex(item, n!("league"));
        if league.truthy() {
            // L4276
            let text = format!("^xFF5555Exclusive to: {}", cs(&league));
            tooltip.add_line_ex(cx, fontSizeBig, &text, fs, None, None);
        }
        // L4278
        if ex(item, n!("unreleased")).truthy() {
            // L4279
            let text = format!("{}Not yet available", cc.NEGATIVE);
            tooltip.add_line_ex(cx, fontSizeBig, &text, fs, None, None);
        }
        // L4281
        let source = ex(item, n!("source"));
        if source.truthy() {
            // L4282
            let text = format!(
                "{}Source: {}",
                cc.SOURCE,
                FormatItemSource(cx, &cs(&source))
            );
            tooltip.add_line_ex(cx, fontSizeBig, &text, fs, None, None);
        }
        // L4284
        let upgradePaths = ex(item, n!("upgradePaths"));
        if let Val::Table(upgradePaths) = &upgradePaths {
            // L4285
            for (_, path) in upgradePaths.ipairs() {
                // L4286
                let text = format!("{}{}", cc.SOURCE, FormatItemSource(cx, &cs(&path)));
                tooltip.add_line_ex(cx, fontSizeBig, &text, fs, None, None);
            }
        } else if upgradePaths.truthy() {
            lua_error!(
                "bad argument #1 to 'ipairs' (table expected, got {})",
                upgradePaths.lua_type()
            );
        }
        // L4289
        tooltip.add_separator(cx, Some(10.0));
    }

    // L4292
    let base = item.base.get().expect("checked at L4225");
    // L4293
    let slotNum: Val = if slot.truthy() {
        lor_val(slot_get(slot, n!("slotNum")), || {
            Val::Num(if cx.sys.is_key_down(SgKey::Shift) {
                2.0
            } else {
                1.0
            })
        })
    } else if cx.sys.is_key_down(SgKey::Shift) {
        Val::Num(2.0)
    } else {
        Val::Num(1.0)
    };
    // L4294
    let modList: Option<model::ModList> = item.modList.get().or_else(|| {
        match slotNum {
            Val::Num(1.0) => item.slotModList[0].get(),
            Val::Num(2.0) => item.slotModList[1].get(),
            // `slotModList[3]` ("Ring 3", CCR-0061; a typed slot since A10 E1)
            Val::Num(3.0) => item.slotModList[2].get(),
            _ => None,
        }
    });
    let modList_or_err = || {
        modList
            .clone()
            .unwrap_or_else(|| lua_error!("bad argument #1 to 'ipairs' (table expected, got nil)"))
    };
    let qualityGt0 = |what: &str| onum(item.quality.get(), what) > 0.0;
    // L4295
    if let Some(weapon) = &base.weapon {
        // Weapon-specific info
        // L4297
        let weaponData = item
            .weaponData
            .get()
            .unwrap_or_else(|| lua_error!("attempt to index field 'weaponData' (a nil value)"));
        let weaponData = match tget(&weaponData, &slotNum) {
            Val::Table(t) => t,
            other => lua_error!(
                "attempt to index local 'weaponData' (a {} value)",
                other.lua_type()
            ),
        };
        // L4298
        let info = cx
            .data()
            .weaponTypeInfo
            .get(&base.r#type)
            .unwrap_or_else(|| lua_error!("attempt to index field '?' (a nil value)"));
        let label = info.label.or(base.subType).unwrap_or(base.r#type);
        tooltip.add_line_ex(cx, fontSizeBig, &lfmt!("^x7F7F7F%s", label), fs, None, None);
        // L4299
        if qualityGt0("quality") {
            // L4300
            let f = format!("^x7F7F7FQuality: {}+%d%%", cc.MAGIC);
            let text = lfmt!(&f, item.quality.get().unwrap_or_default());
            tooltip.add_line_ex(cx, fontSizeBig, &text, fs, None, None);
        }
        // L4302
        let mut totalDamageTypes = 0.0;
        // L4303
        if weaponData.truthy(n!("PhysicalDPS")) {
            // L4304
            let f = format!("^x7F7F7FPhysical Damage: {}%d-%d (%.1f DPS)", cc.MAGIC);
            let text = lfmt!(
                &f,
                tnum(&weaponData, "PhysicalMin"),
                tnum(&weaponData, "PhysicalMax"),
                tnum(&weaponData, "PhysicalDPS")
            );
            tooltip.add_line_ex(cx, fontSizeBig, &text, fs, None, None);
            // L4305
            totalDamageTypes += 1.0;
        }
        // L4307
        if weaponData.truthy(n!("ElementalDPS")) {
            // L4308
            let mut elemLine: Option<String> = None;
            // L4309
            for var in ["Fire", "Cold", "Lightning"] {
                // L4310
                if weaponData.truthy(Name::from(format!("{var}DPS").as_str())) {
                    // L4311
                    let mut l = match elemLine {
                        Some(l) => l + "^x7F7F7F, ",
                        None => "^x7F7F7FElemental Damage: ".to_owned(),
                    };
                    // L4312
                    let code = cc
                        .get(&var.to_ascii_uppercase())
                        .cloned()
                        .unwrap_or_default();
                    l += &lfmt!(
                        "%s%d-%d",
                        code.as_str(),
                        tnum(&weaponData, &format!("{var}Min")),
                        tnum(&weaponData, &format!("{var}Max"))
                    );
                    elemLine = Some(l);
                }
            }
            // L4315 (AddLine with a nil text adds nothing)
            if let Some(elemLine) = &elemLine {
                tooltip.add_line_ex(cx, fontSizeBig, elemLine, fs, None, None);
            }
            // L4316
            let f = format!("^x7F7F7FElemental DPS: {}%.1f", cc.MAGIC);
            let text = lfmt!(&f, tnum(&weaponData, "ElementalDPS"));
            tooltip.add_line_ex(cx, fontSizeBig, &text, fs, None, None);
            // L4317
            totalDamageTypes += 1.0;
        }
        // L4319
        if weaponData.truthy(n!("ChaosDPS")) {
            // L4320
            let f = format!(
                "^x7F7F7FChaos Damage: {}%d-%d {}(%.1f DPS)",
                cc.CHAOS, cc.MAGIC
            );
            let text = lfmt!(
                &f,
                tnum(&weaponData, "ChaosMin"),
                tnum(&weaponData, "ChaosMax"),
                tnum(&weaponData, "ChaosDPS")
            );
            tooltip.add_line_ex(cx, fontSizeBig, &text, fs, None, None);
            // L4321
            totalDamageTypes += 1.0;
        }
        // L4323
        if totalDamageTypes > 1.0 {
            // L4324
            let f = format!("^x7F7F7FTotal DPS: {}%.1f", cc.MAGIC);
            let text = lfmt!(&f, tnum(&weaponData, "TotalDPS"));
            tooltip.add_line_ex(cx, fontSizeBig, &text, fs, None, None);
        }
        // L4326
        let crit = tnum(&weaponData, "CritChance");
        let text = lfmt!(
            "^x7F7F7FCritical Strike Chance: %s%.2f%%",
            stat_color(cx, crit, Some(weapon.CritChanceBase), None).as_str(),
            crit
        );
        tooltip.add_line_ex(cx, fontSizeBig, &text, fs, None, None);
        // L4327
        let rate = tnum(&weaponData, "AttackRate");
        let text = lfmt!(
            "^x7F7F7FAttacks per Second: %s%.2f",
            stat_color(cx, rate, Some(weapon.AttackRateBase), None).as_str(),
            rate
        );
        tooltip.add_line_ex(cx, fontSizeBig, &text, fs, None, None);
        // L4328
        let range = tnum(&weaponData, "range");
        if range < 120.0 {
            // L4329
            let text = lfmt!(
                "^x7F7F7FWeapon Range: %s%.1f ^x7F7F7Fmetres",
                stat_color(cx, range, Some(weapon.Range), None).as_str(),
                range / 10.0
            );
            tooltip.add_line_ex(cx, fontSizeBig, &text, fs, None, None);
        }
    } else if let Some(armour) = &base.armour {
        // Armour-specific info
        // L4333
        let armourData = item
            .armourData
            .get()
            .unwrap_or_else(|| lua_error!("attempt to index local 'armourData' (a nil value)"));
        // L4334
        if qualityGt0("quality") {
            // L4335
            let f = format!("^x7F7F7FQuality: {}+%d%%", cc.MAGIC);
            let text = lfmt!(&f, item.quality.get().unwrap_or_default());
            tooltip.add_line_ex(cx, fontSizeBig, &text, fs, None, None);
        }
        // L4337
        if armour.BlockChance.is_some() && tnum(&armourData, "BlockChance") > 0.0 {
            // L4338
            let v = tnum(&armourData, "BlockChance");
            let text = lfmt!(
                "^x7F7F7FChance to Block: %s%d%%",
                stat_color(cx, v, armour.BlockChance, None).as_str(),
                v
            );
            tooltip.add_line_ex(cx, fontSizeBig, &text, fs, None, None);
        }
        // L4340-4351
        for (key, base_key, label) in [
            ("Armour", "ArmourBase", "Armour"),
            ("Evasion", "EvasionBase", "Evasion Rating"),
            ("EnergyShield", "EnergyShieldBase", "Energy Shield"),
            ("Ward", "WardBase", "Ward"),
        ] {
            let v = tnum(&armourData, key);
            if v > 0.0 {
                let b = armourData.opt_num(Name::from(base_key));
                let text = lfmt!(
                    &format!("^x7F7F7F{label}: %s%d"),
                    stat_color(cx, v, b, None).as_str(),
                    v
                );
                tooltip.add_line_ex(cx, fontSizeBig, &text, fs, None, None);
            }
        }
    } else if let Some(flask) = &base.flask {
        // Flask-specific info
        // L4355
        let flaskData = item
            .flaskData
            .get()
            .unwrap_or_else(|| lua_error!("attempt to index local 'flaskData' (a nil value)"));
        // L4356
        if qualityGt0("quality") {
            // L4357
            let f = format!("^x7F7F7FQuality: {}+%d%%", cc.MAGIC);
            let text = lfmt!(&f, item.quality.get().unwrap_or_default());
            tooltip.add_line_ex(cx, fontSizeBig, &text, fs, None, None);
        }
        // L4359-4382 (life, then mana: the same statements)
        for (total, gradual, instant, base_amount, what) in [
            (
                "lifeTotal",
                "lifeGradual",
                "lifeInstant",
                flask.life,
                "Life",
            ),
            (
                "manaTotal",
                "manaGradual",
                "manaInstant",
                flask.mana,
                "Mana",
            ),
        ] {
            // L4359 / L4371
            if flaskData.truthy(Name::from(total)) {
                // L4360 / L4372
                if ne0(&flaskData, gradual) {
                    // L4361-4364 / L4373-4376
                    let t = tnum(&flaskData, total);
                    let d = tnum(&flaskData, "duration");
                    let text = lfmt!(
                        &format!(
                            "^x7F7F7FRecovers %s%d ^x7F7F7F{what} over %s%.1f0 ^x7F7F7FSeconds"
                        ),
                        stat_color(cx, t, base_amount, None).as_str(),
                        tnum(&flaskData, gradual),
                        stat_color(cx, d, Some(flask.duration), None).as_str(),
                        d
                    );
                    tooltip.add_line_ex(cx, fontSizeBig, &text, fs, None, None);
                }
                // L4366 / L4378
                if ne0(&flaskData, instant) {
                    // L4367 / L4379
                    let t = tnum(&flaskData, total);
                    let text = lfmt!(
                        &format!("^x7F7F7FRecovers %s%d ^x7F7F7F{what} instantly"),
                        stat_color(cx, t, base_amount, None).as_str(),
                        tnum(&flaskData, instant)
                    );
                    tooltip.add_line_ex(cx, fontSizeBig, &text, fs, None, None);
                }
            }
        }
        // L4383
        if !flaskData.truthy(n!("lifeTotal")) && !flaskData.truthy(n!("manaTotal")) {
            // L4384
            let d = tnum(&flaskData, "duration");
            let text = lfmt!(
                "^x7F7F7FLasts %s%.2f ^x7F7F7FSeconds",
                stat_color(cx, d, Some(flask.duration), None).as_str(),
                d
            );
            tooltip.add_line_ex(cx, fontSizeBig, &text, fs, None, None);
        }
        // L4386-4389
        let used = tnum(&flaskData, "chargesUsed");
        let max = tnum(&flaskData, "chargesMax");
        let text = lfmt!(
            "^x7F7F7FConsumes %s%d ^x7F7F7Fof %s%d ^x7F7F7FCharges on use",
            stat_color(cx, used, Some(flask.chargesUsed), None).as_str(),
            used,
            stat_color(cx, max, Some(flask.chargesMax), None).as_str(),
            max
        );
        tooltip.add_line_ex(cx, fontSizeBig, &text, fs, None, None);
        // L4390
        add_buff_mod_lines(cx, tooltip, item, fontSizeBig);
    } else if let Some(tincture) = &base.tincture {
        // Tincture-specific info
        // L4401
        let tinctureData = item
            .tinctureData
            .get()
            .unwrap_or_else(|| lua_error!("attempt to index local 'tinctureData' (a nil value)"));

        // L4403
        if item.quality.get().is_some() && qualityGt0("quality") {
            // L4404
            let f = format!("^x7F7F7FQuality: {}+%d%%", cc.MAGIC);
            let text = lfmt!(&f, item.quality.get().unwrap_or_default());
            tooltip.add_line_ex(cx, fontSizeBig, &text, fs, None, None);
        }

        // L4407
        let mb = tnum(&tinctureData, "manaBurn");
        let text = lfmt!(
            "^x7F7F7FInflicts Mana Burn every %s%.2f ^x7F7F7FSeconds",
            stat_color(cx, mb, Some(tincture.manaBurn), None).as_str(),
            mb
        );
        tooltip.add_line_ex(cx, fontSizeBig, &text, fs, None, None);
        // L4408
        let cd = tnum(&tinctureData, "cooldown");
        let text = lfmt!(
            "^x7F7F7F%s%.2f ^x7F7F7FSecond Cooldown When Deactivated",
            stat_color(cx, cd, Some(tincture.cooldown), None).as_str(),
            cd
        );
        tooltip.add_line_ex(cx, fontSizeBig, &text, fs, None, None);
        // L4409
        add_buff_mod_lines(cx, tooltip, item, fontSizeBig);
    } else if &*item.r#type.get() == "Jewel" {
        // Jewel-specific info
        // L4420
        if let Some(limit) = item.limit.get() {
            // L4421
            let text = format!("^x7F7F7FLimited to: ^7{}", pob_lua::tostring_num(limit));
            tooltip.add_line_ex(cx, fontSizeBig, &text, fs, None, None);
        }
        // L4423
        if let Some(classRestriction) = item.classRestriction.get() {
            // L4424
            let ok = spec(cx).curClassName.get() == Some(classRestriction);
            let text = format!(
                "^x7F7F7FRequires Class {}{classRestriction}",
                if ok { &cc.POSITIVE } else { &cc.NEGATIVE }
            );
            tooltip.add_line_ex(cx, fontSizeBig, &text, fs, None, None);
        }
        // L4426
        if let Some(label) = item.jewelRadiusLabel.get() {
            // L4427
            let text = format!("^x7F7F7FRadius: ^7{label}");
            tooltip.add_line_ex(cx, fontSizeBig, &text, fs, None, None);
        }
        // L4429
        let radiusData = item
            .jewelRadiusData
            .get()
            .filter(|_| slot.truthy())
            .map(|t| tget(&t, &slot_get(slot, n!("nodeId"))));
        if let Some(Val::Table(radiusData)) = radiusData {
            // L4430
            // L4431
            let mut line: Option<String> = None;
            // L4432
            let codes = [&cc.STRENGTH, &cc.DEXTERITY, &cc.INTELLIGENCE];
            // L4433
            for (i0, stat) in ["Str", "Dex", "Int"].into_iter().enumerate() {
                // L4434
                let v = radiusData.get(Name::from(stat));
                if v.truthy() && !v.raw_eq(&Val::Num(0.0)) {
                    // L4435
                    let s = lfmt!("%s%d %s^7", codes[i0].as_str(), v.num(), stat);
                    line = Some(match line {
                        Some(l) => l + ", " + &s,
                        None => s,
                    });
                }
            }
            // L4438
            if let Some(line) = line {
                // L4439
                let text = format!("^x7F7F7FAttributes in Radius: {line}");
                tooltip.add_line_ex(cx, fontSizeBig, &text, fs, None, None);
            }
        } else if radiusData.as_ref().is_some_and(Val::truthy) {
            lua_error!("attempt to index local 'radiusData' (a non-table value)");
        }
    }
    // L4444
    let catalystQualityFormat = ctor::catalystQualityFormat(cx);
    if let (Some(catalyst), Some(catalystQuality)) =
        (item.catalyst.get(), item.catalystQuality.get())
    {
        if catalyst > 0.0 && catalyst <= catalystQualityFormat.len() as f64 && catalystQuality > 0.0
        {
            // L4445
            let fmt = if catalyst.fract() == 0.0 {
                &catalystQualityFormat[catalyst as usize - 1]
            } else {
                lua_error!("bad argument #1 to 'format' (string expected, got nil)")
            };
            let text = lfmt!(fmt.as_str(), catalystQuality);
            tooltip.add_line_ex(cx, fontSizeBig, &text, fs, None, None);
            // L4446
            tooltip.add_separator(cx, Some(10.0));
        }
    }

    // L4449
    if !item.sockets.get().is_empty() {
        // L4450
        let line = GetSocketDescriptionLine(cx, item);
        // L4451
        tooltip.add_line_ex(
            cx,
            fontSizeBig,
            &format!("^x7F7F7FSockets: {line}"),
            fs,
            None,
            None,
        );
    }

    // L4454
    if let Some(talismanTier) = item.talismanTier.get() {
        // L4455
        let text = format!(
            "^x7F7F7FTalisman Tier ^xFFFFFF{}",
            pob_lua::tostring_num(talismanTier)
        );
        tooltip.add_line_ex(cx, fontSizeBig, &text, fs, None, None);
        // L4456
        tooltip.add_separator(cx, Some(10.0));
    }
    // L4458
    let memoryStrands = ex(item, n!("memoryStrands"));
    if memoryStrands.truthy() {
        // L4459
        if showFlavourText {
            tooltip.add_line(cx, 5.0, "");
        }
        // L4460
        let text = format!(
            "{}{}",
            cc.MEMORY,
            lfmt!("Memory Strands: ^7%d", memoryStrands.num())
        );
        tooltip.add_line_ex(
            cx,
            fontSizeBig,
            &text,
            fs,
            None,
            Some(TtBackground::Image("Assets/memorybg.png".to_owned())),
        );
        // L4461
        if showFlavourText {
            tooltip.add_line(cx, 5.0, "");
        }
    }
    // L4463
    let intangibility = ex(item, n!("intangibility"));
    if intangibility.truthy() {
        // L4464
        if showFlavourText {
            tooltip.add_line(cx, 5.0, "");
        }
        // L4465
        let text = format!(
            "{}{}",
            cc.INTANGIBILITY,
            lfmt!("Intangibility: ^7%d%%", intangibility.num())
        );
        tooltip.add_line_ex(
            cx,
            fontSizeBig,
            &text,
            fs,
            None,
            Some(TtBackground::Image("Assets/intangibilitybg.png".to_owned())),
        );
        // L4466
        if showFlavourText {
            tooltip.add_line(cx, 5.0, "");
        }
    }
    // L4468
    tooltip.add_separator(cx, Some(10.0));
    // Requirements
    // L4470-4472
    let req = &item.requirements;
    me.add_requirements(
        cx,
        tooltip,
        req.opt_num(n!("level")),
        req.opt_num(n!("strMod")),
        req.opt_num(n!("dexMod")),
        req.opt_num(n!("intMod")),
        Some(req.opt_num(n!("str")).unwrap_or(0.0)),
        Some(req.opt_num(n!("dex")).unwrap_or(0.0)),
        Some(req.opt_num(n!("int")).unwrap_or(0.0)),
    );

    // Modifiers
    // L4475
    for modList in [
        item.enchantModLines.get(),
        item.scourgeModLines.get(),
        item.implicitModLines.get(),
        item.explicitModLines.get(),
        item.crucibleModLines.get(),
    ] {
        // L4476
        if !modList.is_empty() {
            // L4477
            for modLine in modList.snapshot() {
                // L4478
                let variantCount = item.GetModLineVariantCount(&modLine);
                // L4479
                if variantCount > 0.0 {
                    // L4480
                    let formattedModLine = formatModLine(&modLine, dbMode);
                    // L4481
                    if let Some(formattedModLine) = formattedModLine {
                        // L4482
                        let mut i = 1.0;
                        while i <= variantCount {
                            // L4483
                            tooltip.add_line_ex(
                                cx,
                                fontSizeBig,
                                &formattedModLine,
                                fs,
                                Some(mod_line_ref(&modLine)),
                                None,
                            );
                            i += 1.0;
                        }
                    }
                }
            }
            // L4488
            tooltip.add_separator(cx, Some(10.0));
        }
    }

    // Cluster jewel notables/keystone
    // L4493
    if item.clusterJewel.get().is_some() {
        // L4494
        tooltip.add_separator(cx, Some(10.0));
        let jewelData = item
            .jewelData
            .get()
            .unwrap_or_else(|| lua_error!("attempt to index field 'jewelData' (a nil value)"));
        let tree = spec(cx).tree.get();
        let add_node = |cx: &mut Cx, tooltip: &mut Tooltip, node: &model::NodeRef| {
            // L4499 / L4507
            tooltip.add_line_ex(
                cx,
                fontSizeBig,
                &format!("{}{}", cc.MAGIC, node.dn.get()),
                fs,
                None,
                None,
            );
            // L4500 / L4508
            for stat in node.sd.get().snapshot() {
                // L4501 / L4509
                tooltip.add_line_ex(cx, fontSizeBig, &format!("^x7F7F7F{stat}"), fs, None, None);
            }
        };
        // L4495
        let notables = match jewelData.get(n!("clusterJewelNotables")) {
            Val::Table(t) => t,
            other => lua_error!(
                "attempt to get length of field 'clusterJewelNotables' (a {} value)",
                other.lua_type()
            ),
        };
        if !notables.is_empty() {
            // L4496
            for (_, name) in notables.ipairs() {
                // L4497
                let node = tget(&tree.clusterNodeMap, &name).obj::<model::Node>();
                // L4498
                if let Some(node) = node {
                    add_node(cx, tooltip, &node);
                }
            }
        } else {
            let keystone = jewelData.get(n!("clusterJewelKeystone"));
            // L4504
            if keystone.truthy() {
                // L4505
                let node = tget(&tree.clusterNodeMap, &keystone).obj::<model::Node>();
                // L4506
                if let Some(node) = node {
                    add_node(cx, tooltip, &node);
                }
            }
        }
        // L4513
        tooltip.add_separator(cx, Some(10.0));
    }

    // Corrupted item label
    // L4517
    if item.corrupted.get() || item.split.get() || item.mirrored.get() {
        // L4518
        if item.explicitModLines.get().is_empty() {
            // L4519
            tooltip.add_separator(cx, Some(10.0));
        }
        // L4521
        if item.split.get() {
            // L4522
            tooltip.add_line_ex(
                cx,
                fontSizeBig,
                &format!("{}Split", cc.NEGATIVE),
                fs,
                None,
                None,
            );
        }
        // L4524
        if item.mirrored.get() {
            // L4525
            let text = format!("{}Mirrored", cc.NEGATIVE);
            tooltip.add_line_ex(cx, fontSizeBig, &text, fs, None, None);
        }
        // L4527
        if item.corrupted.get() {
            // L4528
            let text = format!("{}Corrupted", cc.NEGATIVE);
            tooltip.add_line_ex(cx, fontSizeBig, &text, fs, None, None);
        }
        // L4530
        tooltip.add_separator(cx, Some(14.0));
    }

    // Show flavour text:
    // L4534
    if (&*rarity == "UNIQUE" || &*rarity == "RELIC" || !base.flavourText.is_empty())
        && showFlavourText
    {
        // L4535
        let mut flavour: Option<Vec<String>> = None;
        // L4536
        let mut flavourTable: Option<&indexmap::IndexMap<String, Vec<String>>> = None;

        // L4538
        if !base.flavourText.is_empty() {
            // L4539
            flavour = Some(base.flavourText.iter().map(|s| s.to_string()).collect());
        } else {
            // L4541
            let title = item
                .title
                .get()
                .unwrap_or_else(|| lua_error!("attempt to index field 'title' (a nil value)"));
            let key = luastr::gsub(&title, "^Foulborn%s+", "");
            flavourTable = flavour_lookup().get(&key);
        }

        // L4544
        if let Some(flavourTable) = flavourTable {
            // L4545
            if item
                .title
                .get()
                .is_some_and(|t| pob_lua::find(&t, "Grand Spectrum", None, false).is_some())
            {
                // L4546
                let mut selectedFlavourId: Option<&str> = None;
                // L4547
                for lineEntry in &tooltip.lines {
                    // L4548
                    let lineText = lineEntry.text.as_deref().unwrap_or("");
                    let find = |p: &str| pob_lua::find(lineText, p, None, false).is_some();
                    // L4549-4576
                    let id = if find("Power") {
                        Some("UniqueJewel170")
                    } else if find("Endurance") {
                        Some("UniqueJewel168")
                    } else if find("Frenzy") {
                        Some("UniqueJewel169")
                    } else if find("Elemental Damage") {
                        Some("UniqueJewel168")
                    } else if find("Elemental Ailments") {
                        Some("UniqueJewel166")
                    } else if find("Elemental Resistances") || find("Armour") {
                        Some("UniqueJewel76")
                    } else if find("Maximum Life") {
                        Some("UniqueJewel165")
                    } else if find("Critical Strike Multiplier") {
                        Some("UniqueJewel167")
                    } else if find("Critical Strike Chance") || find("Mana") {
                        Some("UniqueJewel75")
                    } else {
                        None
                    };
                    if id.is_some() {
                        selectedFlavourId = id;
                        break;
                    }
                }
                // L4578
                if let Some(text) = selectedFlavourId.and_then(|id| flavourTable.get(id)) {
                    // L4579
                    flavour = Some(text.clone());
                }
            } else {
                // L4582 ORDER: the first entry (module docs)
                if let Some((_, text)) = flavourTable.iter().next() {
                    // L4583
                    flavour = Some(text.clone());
                }
            }
        }

        // L4589
        if let Some(flavour) = flavour {
            // L4590
            for line in &flavour {
                // L4591
                tooltip.add_line_ex(
                    cx,
                    fontSizeBig,
                    &format!("{}{line}", cc.UNIQUE),
                    Some(Font::FontinScItalic),
                    None,
                    None,
                );
            }
            // L4593
            tooltip.add_separator(cx, Some(14.0));
        }
    }

    // Skill tooltip. We add child tooltips, which will be rendered to the right of the main
    // tooltip, growing downwards
    // L4599
    if tooltip.child_tooltips.is_none() {
        // L4600
        tooltip.child_tooltips = Some(Vec::new());
    }
    // L4602
    for tt in tooltip.child_tooltips.as_mut().expect("set above") {
        // L4603
        tt.clear(false);
    }
    // L4605
    let mut itemSkills: Vec<Val> = item
        .grantedSkills
        .get()
        .snapshot()
        .into_iter()
        .map(Val::Table)
        .collect();
    // append "Supported by #" to active skills
    // L4607
    for m in modList_or_err().all_mods() {
        // L4608
        if &*m.name == "ExtraSupport" {
            // L4609
            itemSkills.push(Val::from_value(&m.value));
        }
    }
    // L4612
    if !itemSkills.is_empty() {
        // L4613
        tooltip.add_separator(cx, Some(14.0));
        // L4614-4617
        let text = format!(
            "{}Tip: Hold Shift to display a tooltip for the granted skill{}.",
            cc.TIP,
            if itemSkills.len() > 1 { "s" } else { "" }
        );
        tooltip.add_line(cx, 14.0, &text);
        let model = cx.build().model.clone();
        // L4618
        for (i0, itemSkill) in itemSkills.iter().enumerate() {
            // L4619
            let children = tooltip.child_tooltips.as_mut().expect("set above");
            if children.get(i0).is_none() {
                // L4620
                children.push(Tooltip::default());
            }
            // find gem since the item data only contains the skill id
            // L4623
            let skill = match itemSkill.get(n!("skillId")) {
                Val::Str(id) => cx.data().skills.get(&Name::from(&id)).cloned(),
                _ => None,
            };
            // L4624
            if let Some(skill) = skill.filter(|_| cx.sys.is_key_down(SgKey::Shift)) {
                // L4625-4626 `data.gems[data.gemForSkill[skill] or ""]`
                let gem = cx.data().gem_for_skill(skill.id).cloned();
                // if the skill has no matching gem, make up one. it will lack some information, but should still display somewhat correctly
                // L4630
                let mut options = GemTooltipOptions::default();
                // L4631
                let gem_data = if let Some(gem) = gem {
                    TooltipGemData::Gem(gem)
                } else {
                    // L4633
                    options.skip_requirements = true;
                    // L4632
                    TooltipGemData::MadeUp(skill.clone())
                };
                // L4635-4640
                let gemInst = TooltipGemInstance {
                    gem_data,
                    level: itemSkill.get(n!("level")).opt_num().unwrap_or(1.0),
                    quality: 0.0,
                    display_effect: None,
                };
                // L4641
                let child = &mut tooltip.child_tooltips.as_mut().expect("set above")[i0];
                gem_tooltip::add_gem_tooltip(cx, child, &model, &gemInst, Some(options));
            }
        }
    }
    // Stat differences
    // L4646
    let itemTabHint = if &*cx.build().model.viewMode.get() == "ITEMS" {
        ""
    } else {
        " in the Items tab"
    };
    let showStatDifferences = {
        let m = cx.build().model.clone();
        m.itemsTab.showStatDifferences(&m)
    };
    // L4647
    if !showStatDifferences {
        // L4648
        tooltip.add_separator(cx, Some(14.0));
        // L4649
        let text = format!(
            "{}Tip: Press Ctrl+D{itemTabHint} to enable the display of stat differences.",
            cc.TIP
        );
        tooltip.add_line(cx, 14.0, &text);
        // L4650
        return;
    } else if base.flask.is_none() && base.tincture.is_none() {
        // L4652
        let text = format!(
            "{}Tip: Press Ctrl+D{itemTabHint} to disable the display of stat differences.",
            cc.TIP
        );
        tooltip.add_line(cx, 14.0, &text);
    }
    // L4654
    AddItemStatDifferences(cx, me.clone(), tooltip, item, Some(&base), slot);

    // L4656
    if cx.app().launch.dev_mode_alt {
        // Modifier debugging info
        // L4658
        tooltip.add_separator(cx, Some(10.0));
        // L4659
        for m in modList_or_err().all_mods() {
            // L4660
            let text = format!("^7{}", gamedata::mod_tools::formatMod(&m));
            tooltip.add_line(cx, 14.0, &text);
        }
    }
}

/// L4390-4398 / L4409-4417: `for _, modLine in pairs(item.buffModLines)` (flasks and tinctures).
fn add_buff_mod_lines(cx: &Cx, tooltip: &mut Tooltip, item: &model::Item, fontSizeBig: f64) {
    let fs = Some(Font::FontinSc);
    for modLine in item.buffModLines.get().snapshot() {
        // L4391
        if modLine.truthy(n!("extra")) {
            // L4392
            let mut line = format!("{}{}", cx.main.cc.UNSUPPORTED, cs(&modLine.get(n!("line"))));
            // L4393
            if cx.main.not_supported_mod_tooltips {
                line += pob_calc::classes::item_tools::main_notSupportedTooltipText;
            }
            // L4394
            tooltip.add_line_ex(cx, fontSizeBig, &line, fs, None, None);
        } else {
            // L4396
            let text = format!("{}{}", cx.main.cc.MAGIC, cs(&modLine.get(n!("line"))));
            tooltip.add_line_ex(cx, fontSizeBig, &text, fs, None, None);
        }
    }
}

/// `item.variantList[id]` (nil when out of range or not integral).
fn variant_at(item: &model::Item, id: f64) -> Option<String> {
    let list = item.variantList.get()?;
    if id.fract() != 0.0 || id < 1.0 {
        return None;
    }
    list.get(id as usize).map(|s| s.to_string())
}

/// `a or b` on values.
fn lor_val(a: Val, b: impl FnOnce() -> Val) -> Val {
    if a.truthy() {
        a
    } else {
        b()
    }
}

/// The keys `pairsSortByKey(t)` (Modules/Common.lua) visits: the `pairs` keys sorted with
/// `table.sort` and Lua's default `<` (numbers numerically, strings bytewise, mixed raises).
fn pairs_sort_by_key(t: &Table) -> Vec<Key> {
    let mut keys: Vec<Key> = t.pairs().map(|(k, _)| k).collect();
    pob_lua::sort(&mut keys, |a, b| match (a.as_num(), b.as_num()) {
        (Some(x), Some(y)) => x < y,
        (None, None) => match (a.name(), b.name()) {
            (Some(x), Some(y)) => x.as_str().as_bytes() < y.as_str().as_bytes(),
            _ => lua_error!("attempt to compare two boolean values"),
        },
        _ => lua_error!("attempt to compare number with string"),
    });
    keys
}

// ---- AddItemStatDifferences ------------------------------------------------------------------

/// `calcFunc(override)` (DIVERGENCE(pending-calc): `None` on wasm while pending).
fn call_calc(calcFunc: &Rc<dyn MiscCalc>, ov: model::Override) -> Option<Output> {
    match calcFunc.call(Some(Rc::new(ov)), None) {
        std::task::Poll::Ready(o) => Some(o),
        std::task::Poll::Pending => None,
    }
}

/// One `slots` entry of the compare (L4954-4955).
struct SlotEntry {
    selItem: Option<model::ItemRef>,
    output: Output,
    compareSlot: Table,
    isSameUnique: bool,
}

/// A sort parameter (L4996-4998): a number, a string or nil.
#[derive(Clone)]
enum Param {
    Nil,
    Num(f64),
    Str(String),
}

impl Param {
    fn of(v: Val) -> Param {
        match v {
            Val::Nil => Param::Nil,
            Val::Num(x) => Param::Num(x),
            Val::Str(s) => Param::Str(s.to_string()),
            other => lua_error!("attempt to compare two {} values", other.lua_type()),
        }
    }
    /// Lua `a > b` (strings bytewise; mixed raises)
    fn gt(&self, o: &Param) -> bool {
        match (self, o) {
            (Param::Num(a), Param::Num(b)) => a > b,
            (Param::Str(a), Param::Str(b)) => a.as_bytes() > b.as_bytes(),
            _ => lua_error!("attempt to compare number with string"),
        }
    }
    /// Lua `a < b`
    fn lt(&self, o: &Param) -> bool {
        o.gt(self)
    }
}

/// lua: ItemsTab.lua:4664-5019 `ItemsTabClass:AddItemStatDifferences(tooltip, item, base, slot)`
/// (`base` is `item.base`; `slot` see the module docs)
/// @pob Classes/ItemsTab.lua:4661-5020 h=4fe1ff00
pub fn AddItemStatDifferences(
    cx: &mut Cx,
    me: impl Into<ItemsTabSelf>,
    tooltip: &mut Tooltip,
    item: &model::ItemRef,
    base: Option<&gamedata::ItemBaseRef>,
    slot: &Val,
) {
    let me: ItemsTabSelf = me.into();
    // `self.build` is the entry for an entry's ItemsTab (FUP6)
    me.clone().run(cx, |cx| {
        AddItemStatDifferences_as(cx, &me, tooltip, item, base, slot);
    });
}

/// [`AddItemStatDifferences`]'s body, run with `self.build` as `cx.build()`.
fn AddItemStatDifferences_as(
    cx: &mut Cx,
    me: &ItemsTabSelf,
    tooltip: &mut Tooltip,
    item: &model::ItemRef,
    base: Option<&gamedata::ItemBaseRef>,
    slot: &Val,
) {
    let model = cx.build().model.clone();
    // L4665
    let (calcFunc, calcBase) = match cx.build_mut().calc.misc_calculator(&model) {
        std::task::Poll::Ready(r) => r,
        // DIVERGENCE(pending-calc)
        std::task::Poll::Pending => return,
    };
    let base = base.unwrap_or_else(|| lua_error!("attempt to index local 'base' (a nil value)"));
    let item_base = || {
        item.base
            .get()
            .unwrap_or_else(|| lua_error!("attempt to index field 'base' (a nil value)"))
    };
    // L4666
    if base.flask.is_some() {
        // Special handling for flasks
        // L4668
        let mut stats: Vec<String> = Vec::new();
        // L4669
        let flaskData = item
            .flaskData
            .get()
            .unwrap_or_else(|| lua_error!("attempt to index local 'flaskData' (a nil value)"));
        let env = main_env(cx);
        // L4670
        let modDB = env.modDB.get();
        // L4671
        let output = model
            .calcsTab
            .mainOutput
            .get()
            .unwrap_or_else(|| lua_error!("attempt to index local 'output' (a nil value)"));
        let player = cfg_player();
        let sum = |t: gamedata::ModType, cfg: Option<&ModCfg>, name: &str| {
            modDB.Sum(t, cfg, &[Name::from(name)])
        };
        let flag = |name: &str| modDB.Flag(None, &[Name::from(name)]);
        let more = |name: &str| modDB.More(None, &[Name::from(name)]);
        use gamedata::ModType::{BASE, INC};
        let ib_flask = item_base()
            .flask
            .clone()
            .unwrap_or_else(|| lua_error!("attempt to index field 'flask' (a nil value)"));
        // L4672
        let durInc = sum(INC, None, "FlaskDuration");
        // L4673
        let mut effectInc = sum(INC, Some(&player), "FlaskEffect");
        // L4674
        let mut lifeDur = 0.0;
        // L4675
        let mut manaDur = 0.0;

        // L4677
        if &*item.rarity.get() == "MAGIC" && ib_flask.life.is_none() && ib_flask.mana.is_none() {
            // L4678
            effectInc += sum(INC, Some(&player), "MagicUtilityFlaskEffect");
        }

        // L4681
        if ib_flask.life.is_some() || ib_flask.mana.is_some() {
            // L4682
            let rateInc = sum(INC, None, "FlaskRecoveryRate");
            // L4683
            if ib_flask.life.is_some() {
                // L4684
                let lifeInc = sum(INC, None, "FlaskLifeRecovery");
                // L4685
                let lifeMore = more("FlaskLifeRecovery");
                // L4686
                let lifeRateInc = sum(INC, None, "FlaskLifeRecoveryRate");
                // L4687
                let instantPerc =
                    tnum(&flaskData, "instantPerc") + sum(BASE, None, "LifeFlaskInstantRecovery");

                // More life recovery while on low life is not affected by flask effect (verified ingame).
                // Since this will be multiplied by the flask effect value below we have to counteract this by removing the flask effect from the value beforehand.
                // This is also the reason why this value needs a separate multiplier and cannot just be calculated into FlaskLifeRecovery.
                // L4692
                let lifeMoreOnLowLife = more("FlaskLifeRecoveryLowLife");
                // L4693
                let lowLifeMulti = if lifeMoreOnLowLife > 1.0 {
                    ((lifeMoreOnLowLife - 1.0) / (1.0 + effectInc / 100.0)) + 1.0
                } else {
                    1.0
                };

                let lifeBase = tnum(&flaskData, "lifeBase");
                // L4695
                let inst = lifeBase * instantPerc / 100.0
                    * (1.0 + lifeInc / 100.0)
                    * lifeMore
                    * (1.0 + effectInc / 100.0)
                    * lowLifeMulti;
                // L4696
                let base = lifeBase
                    * (1.0 - instantPerc / 100.0)
                    * (1.0 + lifeInc / 100.0)
                    * lifeMore
                    * (1.0 + effectInc / 100.0)
                    * (1.0 + durInc / 100.0)
                    * lowLifeMulti;
                // L4697
                let grad = base * tnum(&output, "LifeRecoveryRateMod");
                // L4698
                let esGrad = base * tnum(&output, "EnergyShieldRecoveryRateMod");
                // L4699
                lifeDur = tnum(&flaskData, "duration") * (1.0 + durInc / 100.0)
                    / (1.0 + rateInc / 100.0)
                    / (1.0 + lifeRateInc / 100.0);

                // LocalLifeFlaskAdditionalLifeRecovery flask mods
                // L4702
                if tnum(&flaskData, "lifeAdditional") > 0.0
                    && !model.configTab.input.truthy(n!("conditionFullLife"))
                {
                    // L4703
                    let totalAdditionalAmount = (tnum(&flaskData, "lifeAdditional") / 100.0)
                        * tnum(&flaskData, "lifeTotal")
                        * tnum(&output, "LifeRecoveryRateMod");
                    // L4704
                    let additionalGrad = (lifeDur / 10.0) * totalAdditionalAmount;
                    // L4705
                    let leftoverDur = 10.0 - lifeDur;
                    // L4706
                    let leftoverAmount = totalAdditionalAmount - additionalGrad;

                    // L4708
                    if inst > 0.0 {
                        // L4709
                        if grad > 0.0 {
                            // L4710-4711
                            stats.push(lfmt!(
                                "^8Life recovered: ^7%d ^8(^7%d^8 instantly, plus ^7%d ^8over^7 %.2fs^8, and an additional ^7%d ^8over subsequent ^7%.2fs^8)",
                                inst + grad + totalAdditionalAmount,
                                inst,
                                grad + additionalGrad,
                                lifeDur,
                                leftoverAmount,
                                leftoverDur
                            ));
                        } else {
                            // L4713
                            lifeDur = 0.0;
                            // L4714-4715
                            stats.push(lfmt!(
                                "^8Life recovered: ^7%d ^8(^7%d^8 instantly, and an additional ^7%d ^8over ^7%.2fs^8)",
                                inst + totalAdditionalAmount,
                                inst,
                                totalAdditionalAmount,
                                10.0
                            ));
                        }
                    } else {
                        // L4718-4719
                        stats.push(lfmt!(
                            "^8Life recovered: ^7%d ^8(^7%d ^8over ^7%.2fs^8, and an additional ^7%d ^8over subsequent ^7%.2fs^8)",
                            grad + totalAdditionalAmount,
                            grad + additionalGrad,
                            lifeDur,
                            leftoverAmount,
                            leftoverDur
                        ));
                    }
                } else {
                    // L4722
                    if inst > 0.0 && grad > 0.0 {
                        // L4723
                        stats.push(lfmt!(
                            "^8Life recovered: ^7%d ^8(^7%d^8 instantly, plus ^7%d ^8over^7 %.2fs^8)",
                            inst + grad,
                            inst,
                            grad,
                            lifeDur
                        ));
                        // modifiers to recovery amount or duration
                    } else if !Val::Num(inst + grad).raw_eq(&flaskData.get(n!("lifeTotal")))
                        || (inst == 0.0
                            && !Val::Num(lifeDur).raw_eq(&flaskData.get(n!("duration"))))
                    {
                        // L4726
                        if inst > 0.0 {
                            // L4727
                            lifeDur = 0.0;
                            // L4728
                            stats.push(lfmt!("^8Life recovered: ^7%d ^8instantly", inst));
                        } else if grad > 0.0 {
                            // L4730
                            stats.push(lfmt!(
                                "^8Life recovered: ^7%d ^8over ^7%.2fs",
                                grad,
                                lifeDur
                            ));
                        }
                    }
                }
                // L4734
                if flag("LifeFlaskAppliesToEnergyShield") {
                    // L4735
                    if inst > 0.0 && esGrad > 0.0 {
                        // L4736
                        stats.push(lfmt!(
                            "^8Energy Shield recovered: ^7%d ^8(^7%d^8 instantly, plus ^7%d ^8over^7 %.2fs^8)",
                            inst + esGrad,
                            inst,
                            esGrad,
                            lifeDur
                        ));
                    } else if inst > 0.0 && esGrad == 0.0 {
                        // L4738
                        stats.push(lfmt!("^8Energy Shield recovered: ^7%d ^8instantly", inst));
                    } else if inst == 0.0 && esGrad > 0.0 {
                        // L4740
                        stats.push(lfmt!(
                            "^8Energy Shield recovered: ^7%d ^8over ^7%.2fs",
                            esGrad,
                            lifeDur
                        ));
                    }
                }
            }
            // L4744
            if ib_flask.mana.is_some() {
                // L4745
                let manaInc = sum(INC, None, "FlaskManaRecovery");
                // L4746
                let manaRateInc = sum(INC, None, "FlaskManaRecoveryRate");
                // L4747
                let instantPerc =
                    tnum(&flaskData, "instantPerc") + sum(BASE, None, "ManaFlaskInstantRecovery");
                let manaBase = tnum(&flaskData, "manaBase");
                // L4748
                let inst = manaBase * instantPerc / 100.0
                    * (1.0 + manaInc / 100.0)
                    * (1.0 + effectInc / 100.0);
                // L4749
                let base = manaBase
                    * (1.0 - instantPerc / 100.0)
                    * (1.0 + manaInc / 100.0)
                    * (1.0 + effectInc / 100.0)
                    * (1.0 + durInc / 100.0);
                // L4750
                let grad = base * tnum(&output, "ManaRecoveryRateMod");
                // L4751
                let lifeGrad = base * tnum(&output, "LifeRecoveryRateMod");
                // L4752
                manaDur = tnum(&flaskData, "duration") * (1.0 + durInc / 100.0)
                    / (1.0 + rateInc / 100.0)
                    / (1.0 + manaRateInc / 100.0);

                // L4754
                if inst > 0.0 && grad > 0.0 {
                    // L4755
                    stats.push(lfmt!(
                        "^8Mana recovered: ^7%d ^8(^7%d^8 instantly, plus ^7%d ^8over^7 %.2fs^8)",
                        inst + grad,
                        inst,
                        grad,
                        manaDur
                    ));
                } else if !Val::Num(inst + grad).raw_eq(&flaskData.get(n!("manaTotal")))
                    || (inst == 0.0 && !Val::Num(manaDur).raw_eq(&flaskData.get(n!("duration"))))
                {
                    // L4757
                    if inst > 0.0 {
                        // L4758
                        manaDur = 0.0;
                        // L4759
                        stats.push(lfmt!("^8Mana recovered: ^7%d ^8instantly", inst));
                    } else if grad > 0.0 {
                        // L4761
                        stats.push(lfmt!(
                            "^8Mana recovered: ^7%d ^8over ^7%.2fs",
                            grad,
                            manaDur
                        ));
                    }
                }
                // L4764
                if flag("ManaFlaskAppliesToLife") {
                    // L4765
                    if lifeGrad > 0.0 {
                        // L4766
                        stats.push(lfmt!(
                            "^8Life recovered: ^7%d ^8over ^7%.2fs",
                            lifeGrad,
                            manaDur
                        ));
                    }
                }
            }
        } else {
            // L4771
            if durInc != 0.0 {
                // L4772
                stats.push(lfmt!(
                    "^8Flask effect duration: ^7%.1f0s",
                    tnum(&flaskData, "duration") * (1.0 + durInc / 100.0)
                ));
            }
        }
        // L4775
        let effectMod = 1.0 + (tnum(&flaskData, "effectInc") + effectInc) / 100.0;
        // L4776
        if effectMod != 1.0 {
            // L4777
            stats.push(lfmt!(
                "^8Flask effect modifier: ^7%+d%%",
                effectMod * 100.0 - 100.0
            ));
        }
        // L4779
        let usedInc = sum(INC, None, "FlaskChargesUsed");
        // L4780
        if usedInc != 0.0 {
            // L4781
            let used = m_floor(tnum(&flaskData, "chargesUsed") * (1.0 + usedInc / 100.0));
            // L4782
            stats.push(lfmt!(
                "^8Charges used: ^7%d ^8of ^7%d ^8(^7%d ^8uses)",
                used,
                tnum(&flaskData, "chargesMax"),
                m_floor(tnum(&flaskData, "chargesMax") / used)
            ));
        }
        // L4784
        let gainMod =
            tnum(&flaskData, "gainMod") * (1.0 + sum(INC, None, "FlaskChargesGained") / 100.0);
        // L4785
        if gainMod != 1.0 {
            // L4786
            stats.push(lfmt!(
                "^8Charge gain modifier: ^7%+d%%",
                gainMod * 100.0 - 100.0
            ));
        }

        // charge generation
        // L4790
        let mut chargesGenerated = sum(BASE, None, "FlaskChargesGenerated");
        // L4791
        if ib_flask.life.is_some() {
            // L4792
            chargesGenerated += sum(BASE, None, "LifeFlaskChargesGenerated");
        }
        // L4794
        if ib_flask.mana.is_some() {
            // L4795
            chargesGenerated += sum(BASE, None, "ManaFlaskChargesGenerated");
        }
        // L4797
        if ib_flask.mana.is_none() && ib_flask.life.is_none() {
            // L4798
            chargesGenerated += sum(BASE, None, "UtilityFlaskChargesGenerated");
        }
        // L4800
        let mut chargesGeneratedOnWardBreak = 0.0;
        // L4801
        if item.baseName.get().as_deref() == Some("Iron Flask") {
            // L4802
            chargesGeneratedOnWardBreak += sum(BASE, None, "IronFlaskChargesGeneratedOnWardBreak");
        }

        // L4805
        let mut chargesGeneratedPerFlask = sum(BASE, None, "FlaskChargesGeneratedPerEmptyFlask");
        // L4806
        let mut emptyFlaskSlots = 0.0;
        // L4807 ORDER: (c) a count
        let slots = me.slots(cx);
        for (slotName, slot, _) in &slots {
            // L4808
            let sel = slot.get(n!("selItemId"));
            if pob_lua::find(slotName, "^Flask", None, false).is_some()
                && sel.raw_eq(&Val::Num(0.0))
            {
                // L4809
                emptyFlaskSlots += 1.0;
            }
        }
        // L4812
        chargesGeneratedPerFlask *= emptyFlaskSlots;
        // L4813
        chargesGenerated *= gainMod;
        // L4814
        chargesGeneratedOnWardBreak *= gainMod;
        // L4815
        chargesGeneratedPerFlask *= gainMod;

        // L4817
        let totalChargesGenerated = chargesGenerated + chargesGeneratedPerFlask;
        // L4818
        if totalChargesGenerated > 0.0 {
            // L4819
            stats.push(lfmt!(
                "^8Charges generated: ^7%.2f^8 per second",
                totalChargesGenerated
            ));
        }
        // L4821
        if chargesGeneratedOnWardBreak > 0.0 {
            // L4822
            stats.push(lfmt!(
                "^8Charges generated on Ward Break: ^7%.2f",
                chargesGeneratedOnWardBreak
            ));
        }

        // L4825
        let chanceToNotConsumeCharges =
            m_min(sum(BASE, None, "FlaskChanceNotConsumeCharges"), 100.0);
        // L4826
        if chanceToNotConsumeCharges != 0.0 {
            // L4827
            stats.push(lfmt!(
                "^8Chance to not consume charges: ^7%d%%",
                chanceToNotConsumeCharges
            ));
        }

        // flask uptime
        // L4831
        let mut hasUptime = ib_flask.life.is_none() && ib_flask.mana.is_none();
        // L4832
        let mut flaskDuration = tnum(&flaskData, "duration") * (1.0 + durInc / 100.0);

        // L4834
        if ib_flask.life.is_some()
            && (flaskData.truthy(n!("lifeEffectNotRemoved")) || flag("LifeFlaskEffectNotRemoved"))
        {
            // L4835
            hasUptime = true;
            // L4836
            flaskDuration = lifeDur;
        } else if ib_flask.mana.is_some()
            && (flaskData.truthy(n!("manaEffectNotRemoved")) || flag("ManaFlaskEffectNotRemoved"))
        {
            // L4838
            hasUptime = true;
            // L4839
            flaskDuration = manaDur;
        }

        // L4842
        if hasUptime {
            // L4843
            let flaskChargesUsed = tnum(&flaskData, "chargesUsed") * (1.0 + usedInc / 100.0);
            // L4844
            if flaskChargesUsed > 0.0 && flaskDuration > 0.0 {
                // L4845
                let per3Duration = flaskDuration - pob_lua::fmod_lua(flaskDuration, 3.0);
                // L4846
                let per5Duration = flaskDuration - pob_lua::fmod_lua(flaskDuration, 5.0);
                // L4847
                let minimumChargesGenerated =
                    per3Duration * chargesGenerated + per5Duration * chargesGeneratedPerFlask;
                // L4848
                let percentageMin =
                    m_min(minimumChargesGenerated / flaskChargesUsed * 100.0, 100.0);
                // L4849
                if percentageMin < 100.0 && chanceToNotConsumeCharges < 100.0 {
                    // L4850
                    let averageChargesGenerated =
                        (chargesGenerated + chargesGeneratedPerFlask) * flaskDuration;
                    // L4851
                    let averageChargesUsed =
                        flaskChargesUsed * (100.0 - chanceToNotConsumeCharges) / 100.0;
                    // L4852
                    let percentageAvg =
                        m_min(averageChargesGenerated / averageChargesUsed * 100.0, 100.0);
                    // L4853
                    stats.push(lfmt!(
                        "^8Flask uptime: ^7%d%%^8 average, ^7%d%%^8 minimum",
                        percentageAvg,
                        percentageMin
                    ));
                } else {
                    // L4855
                    stats.push(lfmt!("^8Flask uptime: ^7100%%^8"));
                }
            }
        }

        // L4860
        if !stats.is_empty() {
            // L4861
            tooltip.add_line(cx, 14.0, "^7Effective flask stats:");
            // L4862
            for stat in &stats {
                // L4863
                tooltip.add_line(cx, 14.0, stat);
            }
        }
        // L4866
        let ov = model::Override::default();
        ov.toggleFlask.set(Some(item.clone()));
        let Some(output) = call_calc(&calcFunc, ov) else {
            return;
        };
        // L4867-4872
        let active = env.flasks.get().with(|m| m.get(item)).unwrap_or(false);
        let header = if active {
            "^7Deactivating this flask will give you:"
        } else {
            "^7Activating this flask will give you:"
        };
        // L4873
        me.add_stat_compares(cx, tooltip, &calcBase, &output, header);
    } else if base.tincture.is_some() {
        // Special handling for tinctures
        // L4876
        let mut stats: Vec<String> = Vec::new();
        // L4877
        let tinctureData = item
            .tinctureData
            .get()
            .unwrap_or_else(|| lua_error!("attempt to index local 'tinctureData' (a nil value)"));
        let env = main_env(cx);
        // L4878
        let modDB = env.modDB.get();
        // L4879 (`output` is never read)
        let player = cfg_player();
        use gamedata::ModType::{BASE, INC, MORE};
        let sum = |t: gamedata::ModType, cfg: Option<&ModCfg>, name: &str| {
            modDB.Sum(t, cfg, &[Name::from(name)])
        };
        // L4880
        let mut effectInc = sum(INC, Some(&player), "TinctureEffect");

        // L4882
        if &*item.rarity.get() == "MAGIC" {
            // L4883
            effectInc += sum(INC, Some(&player), "MagicTinctureEffect");
        }
        // L4885
        let effectMod = (1.0 + (tnum(&tinctureData, "effectInc") + effectInc) / 100.0)
            * (1.0 + item.quality.get().unwrap_or(0.0) / 100.0);
        // L4886
        if effectMod != 1.0 {
            // L4887
            stats.push(lfmt!(
                "^8Tincture effect modifier: ^7%+d%%",
                effectMod * 100.0 - 100.0
            ));
        }
        // L4889
        stats.push(lfmt!(
            "^8Mana Burn Inflicted Every Second: ^7%.2f",
            1.0 / (tnum(&tinctureData, "manaBurn")
                / (1.0 + sum(INC, Some(&player), "TinctureManaBurnRate") / 100.0)
                / (1.0 + sum(MORE, Some(&player), "TinctureManaBurnRate") / 100.0))
        ));
        // L4890
        let TincturesNotInflictManaBurn =
            m_min(sum(BASE, None, "TincturesNotInflictManaBurn"), 100.0);
        // L4891
        if TincturesNotInflictManaBurn != 0.0 {
            // L4892
            stats.push(lfmt!(
                "^8Chance to not inflict Mana Burn: ^7%d%%",
                TincturesNotInflictManaBurn
            ));
        }
        // L4894
        let tincture = base.tincture.expect("checked");
        stats.push(lfmt!(
            "^8Tincture Cooldown when deactivated: ^7%.2f^8 seconds",
            tincture.cooldown
                / (1.0
                    + (sum(INC, Some(&player), "TinctureCooldownRecovery")
                        + tnum(&tinctureData, "cooldownInc"))
                        / 100.0)
        ));

        // L4896
        if !stats.is_empty() {
            // L4897
            tooltip.add_line(cx, 14.0, "^7Effective tincture stats:");
            // L4898
            for stat in &stats {
                // L4899
                tooltip.add_line(cx, 14.0, stat);
            }
        }
        // L4902
        let ov = model::Override::default();
        ov.toggleTincture.set(Some(item.clone()));
        let Some(output) = call_calc(&calcFunc, ov) else {
            return;
        };
        // L4903-4908
        let active = env.tinctures.get().with(|m| m.get(item)).unwrap_or(false);
        let header = if active {
            "^7Deactivating this tincture will give you:"
        } else {
            "^7Activating this tincture will give you:"
        };
        // L4909
        me.add_stat_compares(cx, tooltip, &calcBase, &output, header);
    } else {
        // L4911
        model.itemsTab.UpdateSockets(&model);
        // Build sorted list of slots to compare with
        // L4913
        let mut compareSlots: Vec<Table> = Vec::new();
        let useSecond = model
            .itemsTab
            .activeItemSet
            .get()
            .unwrap_or_else(|| lua_error!("attempt to index field 'activeItemSet' (a nil value)"))
            .truthy(n!("useSecondWeaponSet"));
        // L4914 ORDER: the module docs
        let slots = me.slots(cx);
        for (slotName, s, slot_id) in slots {
            // L4915
            let weaponSet = s.get(n!("weaponSet"));
            if model
                .itemsTab
                .IsItemValidForSlot(&model, item, &slotName, None)
                && !s.truthy(n!("inactive"))
                && (!weaponSet.truthy()
                    || weaponSet.raw_eq(&Val::Num(if useSecond { 2.0 } else { 1.0 })))
                && me.slot_shown(cx, &s, slot_id)
            {
                // L4916
                compareSlots.push(s);
            }
        }

        // L4920-4928
        let getReplacedItemAndOutput = |cx: &mut Cx,
                                        compareSlot: &Table|
         -> Option<(Option<model::ItemRef>, Output)> {
            // L4921
            let selItem = item_by_id(cx, &compareSlot.get(n!("selItemId")));
            // L4922
            let ov = model::Override::default();
            ov.repSlotName
                .set(compareSlot.str(n!("slotName")).map(|s| Name::from(&s)));
            let repItem =
                (!selItem.as_ref().is_some_and(|s| Rc::ptr_eq(s, item))).then(|| item.clone());
            ov.repItem.set(repItem.clone());
            // L4923
            if compareSlot.truthy(n!("nodeId"))
                && (itemChangesPassiveTree(selItem.as_ref()) || itemChangesPassiveTree(Some(item)))
            {
                // L4924
                ov.spec.set(Some(buildSpecForJewelComparison(
                    cx,
                    compareSlot,
                    repItem.as_ref(),
                )));
            }
            // L4926
            let output = call_calc(&calcFunc, ov)?;
            // L4927
            Some((selItem, output))
        };
        // L4929-4941
        let addCompareForSlot = |cx: &mut Cx,
                                 tooltip: &mut Tooltip,
                                 compareSlot: &Table,
                                 pre: Option<(Option<model::ItemRef>, Output)>|
         -> Option<()> {
            // L4930-4932
            let (selItem, output) = match pre {
                Some(p) => p,
                None => getReplacedItemAndOutput(cx, compareSlot)?,
            };
            // L4934
            let header = if selItem.as_ref().is_some_and(|s| Rc::ptr_eq(s, item)) {
                // L4935
                format!(
                    "^7Removing this item from {} will give you:",
                    cs(&compareSlot.get(n!("label")))
                )
            } else {
                // L4937
                let label = lor_val(compareSlot.get(n!("label")), || {
                    compareSlot.get(n!("slotName"))
                });
                let replacing = match &selItem {
                    Some(s) => {
                        let rc =
                            cx.main.cc.get(&s.rarity.get()).cloned().unwrap_or_else(|| {
                                lua_error!("attempt to concatenate a nil value")
                            });
                        format!("\n(replacing {rc}{}^7)", s.name.get())
                    }
                    None => String::new(),
                };
                lfmt!(
                    "^7Equipping this item in %s will give you:%s",
                    cs(&label).as_str(),
                    replacing.as_str()
                )
            };
            // L4939
            me.add_stat_compares(cx, tooltip, &calcBase, &output, &header);
            Some(())
        };

        // if we have a specific slot to compare to, and the user has "Show
        // tooltips only for affected slots" checked, we can just compare that
        // one slot
        // L4946
        if cx.app().main.slot_only_tooltips && slot.truthy() {
            // L4947
            let slot: Option<Table> = match slot {
                Val::Str(name) => me.slot_named(cx, name),
                Val::Table(t) => Some(t.clone()),
                _ => None,
            };
            // L4948
            if let Some(slot) = slot {
                let _ = addCompareForSlot(cx, tooltip, &slot, None);
            }
            // L4949
            return;
        }

        // L4953
        let mut slots: Vec<Rc<SlotEntry>> = Vec::new();
        // L4954
        let rarity = item.rarity.get();
        let isUnique = &*rarity == "UNIQUE" || &*rarity == "RELIC";
        // L4955
        let mut currentSameUniqueCount = 0.0;
        // L4956
        for compareSlot in &compareSlots {
            // L4957
            let Some((selItem, output)) = getReplacedItemAndOutput(cx, compareSlot) else {
                return;
            };
            // L4958
            let isSameUnique = isUnique
                && selItem
                    .as_ref()
                    .is_some_and(|s| item.name.get() == s.name.get());
            // L4959
            if isUnique && isSameUnique && item.limit.get().is_some() {
                // L4960
                currentSameUniqueCount += 1.0;
            }
            // L4962-4963
            slots.push(Rc::new(SlotEntry {
                selItem,
                output,
                compareSlot: compareSlot.clone(),
                isSameUnique,
            }));
        }

        // limited uniques: only compare to slots with the same item if more don't fit
        // L4967
        if item.limit.get() == Some(currentSameUniqueCount) {
            // L4968
            for slotEntry in &slots {
                // L4969
                if slotEntry.isSameUnique {
                    // L4970
                    let _ = addCompareForSlot(
                        cx,
                        tooltip,
                        &slotEntry.compareSlot,
                        Some((slotEntry.selItem.clone(), slotEntry.output.clone())),
                    );
                }
            }
            // L4973
            return;
        }

        // either the same unique or same base type
        // L4978-4992
        let similar = |compareItem: Option<&model::ItemRef>, sameUnique: bool| -> f64 {
            // empty slot
            // L4980
            let Some(compareItem) = compareItem else {
                return 0.0;
            };

            // L4982-4985
            let cr = compareItem.rarity.get();
            let sameBaseType = !isUnique && &*cr != "UNIQUE" && &*cr != "RELIC" && {
                let a = item_base();
                let b = compareItem
                    .base
                    .get()
                    .unwrap_or_else(|| lua_error!("attempt to index field 'base' (a nil value)"));
                a.r#type == b.r#type && a.subType == b.subType
            };
            // L4986
            if sameBaseType || sameUnique {
                // L4987
                1.0
            } else {
                // L4989
                0.0
            }
        };
        // sort by:
        // 1. empty sockets
        // 2. same base group jewel or unique
        // 3. DPS
        // 4. EHP
        let params = |e: &SlotEntry| -> [Param; 7] {
            [
                Param::Num(
                    if e.compareSlot.get(n!("selItemId")).raw_eq(&Val::Num(0.0)) {
                        1.0
                    } else {
                        0.0
                    },
                ),
                Param::Num(similar(e.selItem.as_ref(), e.isSameUnique)),
                Param::of(e.output.get(n!("FullDPS"))),
                Param::of(e.output.get(n!("CombinedDPS"))),
                Param::of(e.output.get(n!("TotalEHP"))),
                Param::of(e.compareSlot.get(n!("label"))),
                Param::of(e.compareSlot.get(n!("slotName"))),
            ]
        };
        // L4994-5012
        let sortFunc = |a: &Rc<SlotEntry>, b: &Rc<SlotEntry>| -> bool {
            // L4995
            if Rc::ptr_eq(a, b) {
                return false;
            }

            // L4997-4999
            let aParams = params(a);
            let bParams = params(b);
            // L5000 (`#aParams` is 7: the module docs)
            for i in 0..7 {
                let (x, y) = (&aParams[i], &bParams[i]);
                // L5001
                if matches!(x, Param::Nil) || matches!(y, Param::Nil) {
                    // continue
                } else if x.gt(y) {
                    // L5004
                    return true;
                } else if x.lt(y) {
                    // L5006
                    return false;
                }
            }
            // L5009
            false
        };
        // L5013
        pob_lua::sort(&mut slots, sortFunc);

        // L5015
        for slotEntry in &slots {
            // L5016
            let _ = addCompareForSlot(
                cx,
                tooltip,
                &slotEntry.compareSlot,
                Some((slotEntry.selItem.clone(), slotEntry.output.clone())),
            );
        }
    }
}

// ---- CreateUndoState / RestoreUndoState ------------------------------------------------------

/// lua: ItemsTab.lua:5021-5036 `ItemsTabClass:CreateUndoState()`
/// @pob Classes/ItemsTab.lua:5021-5037 h=dad1113c
pub fn CreateUndoState(cx: &mut Cx, me: CtrlId) -> UndoState {
    let m = cx.build().model.clone();
    let tab = &m.itemsTab;
    // L5022-5023
    let activeItemSetId = tab.activeItemSetId.get();
    // L5024
    let mut items = indexmap::IndexMap::new();
    // L5025 ORDER: (c)
    for (k, v) in tab
        .items
        .with(|i| i.iter().map(|(k, v)| (*k, v.clone())).collect::<Vec<_>>())
    {
        // L5026
        items.insert(k, Rc::new(copy_item(&v)));
    }
    // L5028
    let itemOrderList = tab.orderList.with(Clone::clone);
    // L5029
    let mut slotSelItemId = indexmap::IndexMap::new();
    // L5030 ORDER: (c)
    let slots: Vec<(Name, CtrlId)> = cx
        .w::<ItemsTab>(me)
        .ctor
        .slots
        .iter()
        .map(|(k, v)| (*k, *v))
        .collect();
    for (slotName, slot_id) in slots {
        // L5031
        let sel = cx.w::<ItemSlotControl>(slot_id).slot.get(n!("selItemId"));
        slotSelItemId.insert(slotName, sel);
    }
    // L5033 `copyTableSafe(self.itemSets)`: one deep copy of the whole map (shared sub-tables
    // map to one copy across the sets)
    let wrapper = Table::new();
    let ids: Vec<u32> = tab.itemSets.with(|s| s.keys().copied().collect());
    for (id, set) in tab
        .itemSets
        .with(|s| s.iter().map(|(k, v)| (*k, v.clone())).collect::<Vec<_>>())
    {
        wrapper.set(f64::from(id), set);
    }
    let copy = copyTableSafe(&wrapper, false, false);
    let mut itemSets = indexmap::IndexMap::new();
    for id in ids {
        if let Some(t) = copy.table(f64::from(id)) {
            itemSets.insert(id, t);
        }
    }
    // L5034
    let itemSetOrderList = tab.itemSetOrderList.with(Clone::clone);
    // L5035
    UndoState {
        activeItemSetId,
        items,
        itemOrderList,
        slotSelItemId,
        itemSets,
        itemSetOrderList,
    }
}

/// lua: ItemsTab.lua:5038-5055 `ItemsTabClass:RestoreUndoState(state)`
/// @pob Classes/ItemsTab.lua:5038-5055 h=3a058253
pub fn RestoreUndoState(cx: &mut Cx, me: CtrlId, state: UndoState) {
    let m = cx.build().model.clone();
    let tab = &m.itemsTab;
    // L5039
    tab.items.with_mut(|i| *i = state.items);
    // L5040-5043
    tab.orderList.with_mut(|l| *l = state.itemOrderList);
    // L5044 ORDER: (c) (module docs)
    for (slotName, selItemId) in &state.slotSelItemId {
        // L5045
        let id = slot_control(cx, me, slotName)
            .unwrap_or_else(|| lua_error!("attempt to index field '?' (a nil value)"));
        let s = cx.w::<ItemSlotControl>(id).slot.clone();
        let sel = selItemId
            .opt_num()
            .unwrap_or_else(|| lua_error!("ItemSlotControl:SetSelItemId: a non-number id"));
        set_sel_item_id(&m, &s, sel);
    }
    // L5047
    tab.itemSets.with_mut(|s| *s = state.itemSets);
    // L5048-5051
    tab.itemSetOrderList
        .with_mut(|l| *l = state.itemSetOrderList);
    // L5052
    tab.activeItemSetId.set(state.activeItemSetId);
    // L5053
    let active = model::items_tab::id_key(state.activeItemSetId)
        .and_then(|k| tab.itemSets.with(|s| s.get(&k).cloned()));
    tab.activeItemSet.set(active);
    // L5054 self:PopulateSlots() (U34's)
    let f = items_hooks().populate_slots;
    f(cx);
}

// ---- the entries other units call (CCR-0125 §4) ----------------------------------------------

/// `build.itemsTab:AddItemTooltip(tooltip, item, slot, dbMode)` as U32's `ItemsTabHooks` calls
/// it (`build` the model build whose ItemsTab is meant, `slot` the slot's model table).
pub fn add_item_tooltip_hook(
    cx: &mut Cx,
    build: &model::BuildRef,
    tooltip: &mut Tooltip,
    item: &model::ItemRef,
    slot: Option<&Table>,
    dbMode: bool,
) {
    let me = ItemsTabSelf::of_build(cx, build);
    let slot = slot.map_or(Val::Nil, |t| Val::Table(t.clone()));
    AddItemTooltip(cx, me, tooltip, item, &slot, dbMode, None);
}

/// `build.itemsTab:AddItemTooltip(tooltip, item, slot)` with a slot name (CalcBreakdownControl,
/// CalcPerform.lua:2139 `reqSource.sourceSlot`).
pub fn add_item_tooltip_named(
    cx: &mut Cx,
    tooltip: &mut Tooltip,
    item: &model::ItemRef,
    slot: Option<&str>,
) {
    let me = tab(cx);
    let slot = slot.map_or(Val::Nil, Val::from);
    AddItemTooltip(cx, me, tooltip, item, &slot, false, None);
}

/// `build.itemsTab:AddItemStatDifferences(tooltip, item, base, slot)` from outside the tab.
pub fn add_item_stat_differences_hook(
    cx: &mut Cx,
    tooltip: &mut Tooltip,
    item: &model::ItemRef,
    base: Option<gamedata::ItemBaseRef>,
    slot: &Val,
) {
    let me = tab(cx);
    AddItemStatDifferences(cx, me, tooltip, item, base.as_ref(), slot);
}

/// `itemsTab:AddUndoState()` (UndoHandler.lua:34-41 with this file's CreateUndoState).
pub fn add_undo_state_hook(cx: &mut Cx) {
    let me = tab(cx);
    UNDO_OPS.add_undo_state(cx, me, false);
}

/// `itemsTab:ResetUndo()` (UndoHandler.lua:25-29; ItemsTab:Load's tail, U34).
pub fn reset_undo(cx: &mut Cx, me: CtrlId) {
    UNDO_OPS.reset_undo(cx, me);
}

/// `itemsTab:Undo()` (UndoHandler.lua:44-50; Ctrl+Z in Draw, U34).
pub fn undo(cx: &mut Cx, me: CtrlId) {
    UNDO_OPS.undo(cx, me);
}

/// `itemsTab:Redo()` (UndoHandler.lua:53-58; Ctrl+Y in Draw, U34).
pub fn redo(cx: &mut Cx, me: CtrlId) {
    UNDO_OPS.redo(cx, me);
}

/// `itemsTab.modFlag = v` (U32's `ItemsTabHooks::set_mod_flag`).
pub fn set_mod_flag_hook(cx: &mut Cx, v: bool) {
    set_mod_flag(cx, v);
}

/// `ShellHooks::tab_flag` for `TabFlag::Items`: `self.itemsTab.modFlag` (Build.lua:1327).
pub fn mod_flag(cx: &Cx) -> bool {
    slot_helper::items_tab(cx).is_some_and(|me| cx.w::<ItemsTab>(me).tooltip.undo.mod_flag)
}

/// `ShellHooks::set_tab_flag` for `TabFlag::Items`: `self.itemsTab.modFlag = v`
/// (ResetModFlags, Build.lua:1100-1111).
pub fn set_mod_flag(cx: &mut Cx, v: bool) {
    if let Some(me) = slot_helper::items_tab(cx) {
        cx.w_mut::<ItemsTab>(me).tooltip.undo.mod_flag = v;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn params_compare_like_lua() {
        assert!(Param::Num(2.0).gt(&Param::Num(1.0)));
        assert!(!Param::Num(f64::NAN).gt(&Param::Num(1.0)));
        assert!(!Param::Num(f64::NAN).lt(&Param::Num(1.0)));
        assert!(Param::Str("b".into()).gt(&Param::Str("a".into())));
        assert!(Param::Str("B".into()).lt(&Param::Str("a".into())));
    }

    #[test]
    fn pairs_sort_by_key_numbers() {
        let t = Table::new();
        for k in [3.0, 1.0, 2.0, 10.0] {
            t.set(k, true);
        }
        let keys: Vec<f64> = pairs_sort_by_key(&t)
            .into_iter()
            .filter_map(Key::as_num)
            .collect();
        assert_eq!(keys, vec![1.0, 2.0, 3.0, 10.0]);
    }
}
