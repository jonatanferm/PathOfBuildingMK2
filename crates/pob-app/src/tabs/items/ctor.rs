//! Port of PathOfBuilding v2.67.2 (16de4b82d) src/Classes/ItemsTab.lua:1-1215 (the module
//! locals and `ItemsTabClass:ItemsTab(build)`, the constructor). Owner: U33.
//!
//! @pob-file Classes/ItemsTab.lua @16de4b82d
//!
//! The Items tab (`newClass("ItemsTab", "UndoHandler", "ControlHost", "Control")`) is a free node
//! owned by the build node (`build.itemsTab`, `BuildState::tabs.items`), like the Notes tab (U22,
//! the template). Its widget struct is [`super::ItemsTab`], split over U33-U38 (ui doc §12.1);
//! this unit's part is [`State`]. The fields that the later units' methods write and the
//! constructor's closures read (`displayItem`, `maxY`, `snapHScroll`, the slot and
//! socket controls, the per-control fields `outputTable`/`newVariantVisible`/...) live here, so
//! U34-U38 reach them as `cx.w::<ItemsTab>(me).ctor.<field>`.
//!
//! Split with the model half (markers): calc:BM-A's `ItemsTab::ctor_model` covers ItemsTab.lua 37
//! (`baseSlots`), 70-74 (`isAnointable`), 101-108, 140-235 (the slot tables) and 1196-1215 (the
//! item sets, `PopulateSlots`, `lastSlot`). The UI statements of those lines are here, commented
//! with their line numbers and no marker; the rest of 1-1215 carries this unit's markers.
//!
//! Calls into methods of the other ItemsTab units go through two hook tables, as U32's controls
//! do (CCR-0107 §1): U32's [`ItemsTabHooks`](super::slot_helper::ItemsTabHooks)
//! (`SetActiveItemSet`, `AddUndoState`, `SetDisplayItem`, `AddItemSetTooltip`), and this file's
//! [`ItemsUiHooks`] for the methods the constructor's closures call that U32 never needed
//! (`AddDisplayItem`, `UpdateDisplayItemTooltip`, `CraftItem`, ...; CCR-0120 §1). Each entry names
//! its owner; the owners replace their entry in [`default_items_ui_hooks`] (one line each).
//!
//! Lua semantics:
//! * `self.displayItem` is an [`ItemRef`](model::ItemRef) (the model's Item, shared by identity).
//!   Its untyped fields (`crafted`, `affixLimit`, `rangeLineList`, `customCount`, ...) are the
//!   calc's `item.extra`; an arithmetic or comparison on a nil one raises, as in Lua.
//! * `drop.outputTable`/`drop.outputIndex` (UpdateAffixControl, U35), `control.newVariantVisible`
//!   /`newVariantEnabled`/`variantGroupId` (UpdateDisplayItemVariantControls, U35) and
//!   `label.lineCount` are Lua fields set on the control objects; DropDownControl and
//!   LabelControl have no such fields, so they are [`State::affix_controls`],
//!   [`State::variant_controls`] and [`State::stacked_line_count`].
//! * `self.height < 980` reads the tab's raw `height` (0 until Draw sets it, as Control() with no
//!   rect does); `self.maxY` is nil until Draw sets it, so the DB heights raise before that, as in
//!   Lua.
//! * `t_insert(self.controls, slot)`: the slot controls are `controls[1..n]`, so DrawControls
//!   visits them first, in index order, then the named controls in creation order (D7).
//!
//! ORDER:
//! * L222 `pairs(build.latestTree.nodes)` is calc:BM-A's loop; the result is sorted by id (L227),
//!   so the socket order does not depend on it.
//! * L1198 `pairs(self.slots)` appends the slots to the drag target lists in the model's slot
//!   order (insertion). (c): the lists are only searched for the first shown target under the
//!   mouse (ListControl.lua:179-188) and each target is marked independently (L161-167); visible
//!   slots never overlap, so the order is unobservable.
//! * L904 `pairs(clusterInfo.size)` (the affix tooltip of a cluster jewel notable): the keys are
//!   strings, whose order is per-process random in PoB; the port uses the data dump's (bytewise)
//!   order. Not covered by the U33 scenarios (they open no crafted cluster jewel); open item for
//!   U35's scenarios (resolution (b) on the tooltip lines).
//!
//! DIVERGENCE(mutated-state): the foulborn checkboxes' `shown` functions (L1058-1067,
//! L1123-1132) set `box.state` as a side effect; CheckBoxControl's `state` is a plain field, which
//! a `Prop` closure (`&Cx`) cannot write. The port keeps `shown` pure and copies the same value
//! into `state` at the start of the box's `Draw`, `OnKeyDown` and `OnKeyUp` overrides. In PoB the
//! value is set whenever `shown` is evaluated, which DrawControls/GetMouseOverControl do right
//! before those methods; `state` is read only by those methods, so the result is the same.
//! DIVERGENCE(range-slider-val): likewise the range sliders' `shown` (L1084-1094, L1151-1161) set
//! `slider.val = modLine.range or slider.val`; the port applies it at the start of the slider's
//! `Draw`, `OnKeyDown` and `OnKeyUp`, where PoB's preceding `IsShown` did. A `range` that is not a
//! number raises there (PoB raises at the slider's next arithmetic on it).
//! DIVERGENCE(nil-range): `displayItemRangeLine`'s selFunc (L1032) assigns `rangeLineList[index].
//! range`, which may be nil, to the slider's `val`; SliderControl's `val` is a number, so a nil
//! keeps the previous value. PoB's nil is only read when the slider is shown, and the slider's
//! `shown` (L1089) replaces it first, so nothing observes the difference.
//! DIVERGENCE(ctor-sync-loadouts): the constructor's `self:SetActiveItemSet(1)` (L1209) runs
//! `PopulateSlots` and `self.build:SyncLoadouts()` (L1615-1616). The model half ran in calc:BM-A's
//! `ctor_model`; the UI half here is the slot lists (U32's `populate_slot_lists_of`).
//! `SyncLoadouts` is not called: during the constructor PoB's `build.itemsTab`, `treeTab` and
//! `skillsTab` are still nil, so its list is overwritten by Build.lua:726's call before any frame
//! draws it.
//!
//! Props with side effects (ui doc §7.3): L1171 (`label.lineCount`, written by the stacked range
//! labels' `label` function: [`State::stacked_line_count`], `Cell`s); L1062/L1064, L1127/L1129,
//! L1089 and L1156 (see the DIVERGENCE notes above).
#![allow(non_snake_case, non_upper_case_globals, clippy::too_many_lines)]

use std::cell::{Cell, RefCell};
use std::rc::Rc;
use std::sync::OnceLock;

use pob_lua::math::{m_floor, m_max, m_min, round};
use pob_lua::{n, pattern, tonumber, tostring, Name};
use pob_sg::ImageFlags;
use pob_ui_core::controls::check_box::CheckBoxControl;
use pob_ui_core::controls::drop_down::{DropTipMode, SelFunc};
use pob_ui_core::controls::list::ListControl;
use pob_ui_core::controls::scroll_bar::ScrollDir;
use pob_ui_core::controls::slider::SliderControl;
use pob_ui_core::controls::{
    ButtonControl, DropDownControl, DropItem, EditArgs, EditControl, LabelControl,
    ScrollBarControl,
};
use pob_ui_core::prelude::*;
use pob_ui_core::tooltip::tooltip::{Tooltip, TtKey, TtObj};
use pob_ui_core::wrap::wrap_string;

use super::item_db::{DbType, ItemDBControl};
use super::item_list::ItemListControl;
use super::shared_list::SharedItemListControl;
use super::slot::ItemSlotControl;
use super::slot_helper::{self, items_hooks};
use super::ItemsTab;
use crate::app::CxApp;
use crate::model::{self, gamedata, Table, Val};
// CCR-0120 §3: model.rs (U15) does not re-export ItemTools; `itemLib.influenceInfo` and
// `itemLib.applyRange` are calc:IT-B's
use pob_calc::classes::item_tools::{applyRange, influenceInfo_all, influenceInfo_default};

// @pob Classes/ItemsTab.lua:1-17 h=7c9c492f
// (header comment and the local aliases `pairs, ipairs, next, t_insert, t_remove, s_format,
// m_max, m_min, m_ceil, m_floor, m_modf`: Rust iteration and `pob_lua::math`)

// @pob Classes/ItemsTab.lua:18-20 h=343ae3e4
// (`require("Classes.CompareBuySimilar")` (U51), `require("Modules.AddImplicitPopup")` (U36,
// `add_implicit.rs`), `require("Classes.GemTooltip")` (U29, `controls::gem_tooltip`): reached
// through [`ItemsUiHooks`] and the tooltip unit)

/// A `rarityDropList` entry's `rarity` (the dropdown item's value; U36's CraftItem popup).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RarityValue(pub &'static str);

/// `rarityDropList` (ItemsTab.lua:22-28): the labels read `colorCodes` at module load.
/// @pob Classes/ItemsTab.lua:21-28 h=aa92942c
pub fn rarityDropList(cx: &Cx) -> Vec<DropItem> {
    let cc = &cx.main.cc;
    vec![
        // L23
        DropItem::new(format!("{}Normal", cc.NORMAL), Rc::new(RarityValue("NORMAL"))),
        // L24
        DropItem::new(format!("{}Magic", cc.MAGIC), Rc::new(RarityValue("MAGIC"))),
        // L25
        DropItem::new(format!("{}Rare", cc.RARE), Rc::new(RarityValue("RARE"))),
        // L26
        DropItem::new(format!("{}Unique", cc.UNIQUE), Rc::new(RarityValue("UNIQUE"))),
        // L27
        DropItem::new(format!("{}Relic", cc.RELIC), Rc::new(RarityValue("RELIC"))),
    ]
}

/// A `socketDropList` entry's `color` (the socket dropdowns' value; U35's UpdateSocketControls
/// selects by it).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SocketColorValue(pub &'static str);

/// `socketDropList` (ItemsTab.lua:30-35).
/// @pob Classes/ItemsTab.lua:29-36 h=5267ba4a
pub fn socketDropList(cx: &Cx) -> Vec<DropItem> {
    let cc = &cx.main.cc;
    vec![
        // L31
        DropItem::new(format!("{}R", cc.STRENGTH), Rc::new(SocketColorValue("R"))),
        // L32
        DropItem::new(format!("{}G", cc.DEXTERITY), Rc::new(SocketColorValue("G"))),
        // L33
        DropItem::new(format!("{}B", cc.INTELLIGENCE), Rc::new(SocketColorValue("B"))),
        // L34
        DropItem::new(format!("{}W", cc.SCION), Rc::new(SocketColorValue("W"))),
    ]
}

// L37 `baseSlots`: calc:BM-A's (`model::items_tab`; the model's `orderedSlots` are built from it)

/// `forbiddenJewelCounterpart` (ItemsTab.lua:39-42; U34's AddForbiddenJewelCounterpart).
/// @pob Classes/ItemsTab.lua:38-43 h=7e155e6c
pub fn forbiddenJewelCounterpart(title: &str) -> Option<&'static str> {
    match title {
        // L40
        "Forbidden Flame" => Some("Forbidden Flesh"),
        // L41
        "Forbidden Flesh" => Some("Forbidden Flame"),
        _ => None,
    }
}

// L45 `local influenceInfo = itemLib.influenceInfo.all` (calc:IT-B's `influenceInfo_all`)
// @pob Classes/ItemsTab.lua:44-46 h=0b3a04e3

/// `catalystQualityFormat` (ItemsTab.lua:47-60; U38's tooltips). `%d%%` is `string.format`'s.
/// @pob Classes/ItemsTab.lua:47-61 h=d6298852
pub fn catalystQualityFormat(cx: &Cx) -> [String; 12] {
    let m = &cx.main.cc.MAGIC;
    [
        // L48
        format!("^x7F7F7FQuality (Attack Modifiers): {m}+%d%% (augmented)"),
        // L49
        format!("^x7F7F7FQuality (Speed Modifiers): {m}+%d%% (augmented)"),
        // L50
        format!("^x7F7F7FQuality (Suffix Modifiers): {m}+%d%% (augmented)"),
        // L51
        format!("^x7F7F7FQuality (Life and Mana Modifiers): {m}+%d%% (augmented)"),
        // L52
        format!("^x7F7F7FQuality (Caster Modifiers): {m}+%d%% (augmented)"),
        // L53
        format!("^x7F7F7FQuality (Attribute Modifiers): {m}+%d%% (augmented)"),
        // L54
        format!("^x7F7F7FQuality (Physical and Chaos Modifiers): {m}+%d%% (augmented)"),
        // L55
        format!("^x7F7F7FQuality (Resistance Modifiers): {m}+%d%% (augmented)"),
        // L56
        format!("^x7F7F7FQuality (Prefix Modifiers): {m}+%d%% (augmented)"),
        // L57
        format!("^x7F7F7FQuality (Defence Modifiers): {m}+%d%% (augmented)"),
        // L58
        format!("^x7F7F7FQuality (Elemental Modifiers): {m}+%d%% (augmented)"),
        // L59
        format!("^x7F7F7FQuality (Critical Modifiers): {m}+%d%% (augmented)"),
    ]
}

/// `flavourLookup[name][id] = text` (ItemsTab.lua:62-69; U38's tooltips), built once from
/// `data.flavourText` (a dump root kept untyped, `Data::other`).
/// ORDER: (c) the loop only fills a keyed map; `pairs` order cannot change it except for two
/// entries with the same name and id, which the data does not have (the later one would win in
/// PoB's random order; here the dump's order).
/// @pob Classes/ItemsTab.lua:62-69 h=fff7175d
pub fn flavourLookup() -> &'static indexmap::IndexMap<String, indexmap::IndexMap<String, String>> {
    static LOOKUP: OnceLock<indexmap::IndexMap<String, indexmap::IndexMap<String, String>>> =
        OnceLock::new();
    LOOKUP.get_or_init(|| {
        // L62
        let mut flavourLookup: indexmap::IndexMap<String, indexmap::IndexMap<String, String>> =
            indexmap::IndexMap::new();
        let Some(gamedata::DataValue::Table(t)) = model::data().other.get("data.flavourText")
        else {
            return flavourLookup;
        };
        let text_of = |v: &gamedata::DataValue| match v {
            gamedata::DataValue::Str(s) => Some(s.to_string()),
            gamedata::DataValue::Num(x) => Some(tostring(*x)),
            _ => None,
        };
        // L64 `for _, entry in pairs(data.flavourText)`
        let entries = t.seq.iter().chain(t.fields.iter().map(|(_, v)| v));
        for entry in entries {
            let gamedata::DataValue::Table(entry) = entry else {
                continue;
            };
            // L65
            let (Some(name), Some(id), Some(text)) = (
                text_of(entry.field("name")),
                text_of(entry.field("id")),
                text_of(entry.field("text")),
            ) else {
                continue;
            };
            // L66-67
            flavourLookup.entry(name).or_default().insert(id, text);
        }
        flavourLookup
    })
}

/// `isAnointable(item)` (ItemsTab.lua:70-74; calc:BM-A's private copy has the marker).
pub fn isAnointable(item: Option<&model::ItemRef>) -> bool {
    // L71
    let Some(item) = item else { return false };
    let Some(base) = item.base.get() else {
        return false;
    };
    !base.cannotBeAnointed
        // L72
        && base.subType.as_deref() != Some("Talisman")
        // L73
        && (item.extra.truthy(n!("canBeAnointed")) || &*base.r#type == "Amulet")
}

/// A `buildModSortList` / `sortingOptions` entry: `{ label = .., stat = .. }` or a
/// `data.powerStatList` entry (`None` is the "Default" entry, whose `stat` is nil).
#[derive(Clone, Copy)]
pub struct PowerSortValue(pub Option<&'static gamedata::schema::item_data::PowerStat>);

impl PowerSortValue {
    /// `entry.stat`
    pub fn stat(&self) -> Option<Name> {
        self.0.and_then(|e| e.stat)
    }
}

/// `buildModSortList()` (ItemsTab.lua:76-87): the list and `sortStats[stat] = entry`.
/// @pob Classes/ItemsTab.lua:75-87 h=11b40ea8
pub fn buildModSortList() -> (
    Vec<DropItem>,
    indexmap::IndexMap<Name, &'static gamedata::schema::item_data::PowerStat>,
) {
    // L77
    let mut sortList = vec![DropItem::new("Default", Rc::new(PowerSortValue(None)))];
    // L78
    let mut sortStats = indexmap::IndexMap::new();
    // L79
    for entry in &model::data().powerStatList.stats {
        // L80
        if let Some(stat) = entry.stat.filter(|_| !entry.ignoreForNodes) {
            // L81
            sortList.push(DropItem::new(
                entry.label.to_string(),
                Rc::new(PowerSortValue(Some(entry))),
            ));
            // L82
            sortStats.insert(stat, entry);
        }
    }
    // L85
    (sortList, sortStats)
}

// @pob Classes/ItemsTab.lua:88-92 h=58f94ce1
// (`newClass("ItemsTab", "UndoHandler", "ControlHost", "Control")`: the `Widget` impl below,
// `.host()` in the constructor; the UndoHandler stacks are U38's part of the struct)

// ---- the state -------------------------------------------------------------------------------

/// `control.newVariantVisible`, `newVariantEnabled`, `variantGroupId` of the six variant
/// dropdowns (written by U35's UpdateDisplayItemVariantControls; nil reads false).
#[derive(Clone, Debug, Default)]
pub struct VariantControl {
    pub newVariantVisible: bool,
    pub newVariantEnabled: bool,
    pub variantGroupId: Option<Val>,
}

/// `drop.outputTable` / `drop.outputIndex` of an affix dropdown (written by U35's
/// UpdateAffixControl; nil until then).
#[derive(Clone, Debug, Default)]
pub struct AffixControl {
    /// `"prefixes"` or `"suffixes"`
    pub outputTable: Option<Name>,
    pub outputIndex: Option<f64>,
}

/// The value of an affix dropdown entry built by U35's UpdateAffixControl (`lastSeries`:
/// `{ label, modList, haveRange, statOrder }`; `modId` for an entry naming one mod). The "None"
/// entry is a string item ([`DropItem::str`]), whose `modId`/`modList` read nil.
#[derive(Clone, Debug, Default)]
pub struct AffixSeries {
    pub modId: Option<Name>,
    pub modList: Vec<Name>,
    pub haveRange: bool,
    pub statOrder: Vec<f64>,
}

/// The value of a cluster jewel skill dropdown entry (`{ label, skillId }`, U35's
/// UpdateClusterJewelControls).
#[derive(Clone, Debug)]
pub struct ClusterSkillValue {
    pub skillId: Name,
}

/// The keys of the six variant dropdowns, in L447-454's order.
pub const VARIANT_KEYS: [&str; 6] = [
    "displayItemVariant",
    "displayItemAltVariant",
    "displayItemAltVariant2",
    "displayItemAltVariant3",
    "displayItemAltVariant4",
    "displayItemAltVariant5",
];

/// `maxModCount` (L731)
pub const maxModCount: usize = 9;

/// The stacked range lines (L1097: `for i = 1, 20`)
pub const STACKED_RANGE_LINES: usize = 20;

/// U33's part of the `ItemsTab` object (ui doc §12.1), plus the fields every ItemsTab unit shares
/// (see the module docs).
#[derive(Default)]
pub struct State {
    /// `self.build` (the build node)
    pub build: CtrlId,
    /// `self.socketViewer` (a PassiveTreeView, U23; lent through `cx.lend`)
    pub socketViewer: crate::tree_view::PassiveTreeView,
    /// `self.slotAnchor`
    pub slotAnchor: CtrlId,
    /// `self.slots[slotName]` (the ItemSlotControl nodes; the model tables are
    /// `build.itemsTab.slots`)
    pub slots: indexmap::IndexMap<Name, CtrlId>,
    /// `self.orderedSlots`
    pub orderedSlots: Vec<CtrlId>,
    /// `self.sockets[nodeId]`
    pub sockets: model::LjIntMap<CtrlId>,
    /// `self.displayItemTooltip` (lent through `cx.lend` to draw it)
    pub displayItemTooltip: Tooltip,
    /// `self.anchorDisplayItem`
    pub anchorDisplayItem: CtrlId,
    /// `self.displayItem` (U35's SetDisplayItem writes it)
    pub displayItem: Option<model::ItemRef>,
    /// `self.maxY` (Draw, U34)
    pub maxY: Option<f64>,
    /// `self.snapHScroll` ("LEFT", "RIGHT" or nil; SetDisplayItem and Draw)
    pub snapHScroll: Option<&'static str>,
    /// the six variant dropdowns' extra fields ([`VARIANT_KEYS`] order)
    pub variant_controls: [VariantControl; 6],
    /// the nine affix dropdowns' extra fields (`displayItemAffix1..9`)
    pub affix_controls: [AffixControl; maxModCount],
    /// `self.controls["displayItemStackedRangeLine"..i].lineCount` (L1171; written by the
    /// label's `label` function, hence `Cell`s)
    pub stacked_line_count: Rc<[Cell<Option<f64>>; STACKED_RANGE_LINES]>,
    /// `foulbornIcon` (L1029-1030)
    pub foulbornIcon: pob_sg::ImageHandle,
}

/// drawn by BuildMode (`self.itemsTab:Draw(tabViewPort, inputEvents)`, Build.lua:1309; U34's)
impl Widget for ItemsTab {
    const NAME: &'static str = "ItemsTab";
    const CAPS: Caps = Caps::empty();
}

// ---- the methods of U34-U38 (and U23-U25, U51) the constructor's closures call -------------

pub type ItemsUiHook = Rc<dyn Fn(&mut Cx, CtrlId)>;

/// The ItemsTab methods (and the methods of other objects) this file's closures call that U32's
/// [`ItemsTabHooks`](super::slot_helper::ItemsTabHooks) does not have (CCR-0120 §1). `CtrlId` is
/// the ItemsTab node. The owners replace their entry in [`default_items_ui_hooks`].
#[derive(Clone)]
pub struct ItemsUiHooks {
    // ---- U34 (ItemsTab.lua:1216-1947) ----
    /// `self:AddDisplayItem(noAutoEquip)` (L1804-1818)
    pub add_display_item: Rc<dyn Fn(&mut Cx, CtrlId, bool)>,
    // ---- U35 (ItemsTab.lua:1948-2608) ----
    /// `self:SelectDisplayItemVariant(index, value, legacyField, control)` (L1960-1974)
    pub select_display_item_variant:
        Rc<dyn Fn(&mut Cx, CtrlId, usize, Option<&DropItem>, &'static str, CtrlId)>,
    /// `self:UpdateDisplayItemVariantControls()` (L1976-2032)
    pub update_display_item_variant_controls: ItemsUiHook,
    /// `self:UpdateDisplayItemTooltip()` (L2117-2121)
    pub update_display_item_tooltip: ItemsUiHook,
    /// `self:UpdateSocketControls()` (L2138-2146)
    pub update_socket_controls: ItemsUiHook,
    /// `self:CraftClusterJewel()` (L2175-2195)
    pub craft_cluster_jewel: ItemsUiHook,
    /// `self:UpdateAffixControls()` (L2197-2216)
    pub update_affix_controls: ItemsUiHook,
    /// `self:UpdateCustomControls()` (L2430-2477)
    pub update_custom_controls: ItemsUiHook,
    /// `self:UpdateDisplayItemRangeLines()` (L2480-2491)
    pub update_display_item_range_lines: ItemsUiHook,
    /// `self:AddModComparisonTooltip(tooltip, mod)` (L2503-2526)
    pub add_mod_comparison_tooltip:
        Rc<dyn Fn(&mut Cx, CtrlId, &mut Tooltip, Option<&gamedata::schema::affixes::ItemModRef>)>,
    /// `self:GetComparisonSlotNameForItem(item)` (L2544-2558)
    pub get_comparison_slot_name_for_item:
        Rc<dyn Fn(&Cx, CtrlId, &model::ItemRef) -> Option<String>>,
    // ---- U36 (ItemsTab.lua:2609-3120, AddImplicitPopup.lua) ----
    /// `self:OpenItemSetManagePopup()` (L2609-2620)
    pub open_item_set_manage_popup: ItemsUiHook,
    /// `self:CraftItem()` (L2622-2707)
    pub craft_item: ItemsUiHook,
    /// `self:EditDisplayItemText(alsoAddItem)` (L2709-2762)
    pub edit_display_item_text: Rc<dyn Fn(&mut Cx, CtrlId, bool)>,
    /// `self:EnchantDisplayItem(enchantSlot)` (L2764-2976)
    pub enchant_display_item: Rc<dyn Fn(&mut Cx, CtrlId, f64)>,
    /// `self:AppendAddedNotableTooltip(tooltip, node)` (L3071-3079)
    pub append_added_notable_tooltip: Rc<dyn Fn(&mut Cx, CtrlId, &mut Tooltip, &model::NodeRef)>,
    /// `self:AnointDisplayItem(enchantSlot)` (L3081-3119)
    pub anoint_display_item: Rc<dyn Fn(&mut Cx, CtrlId, f64)>,
    /// `addImplicit.AddImplicitToDisplayItem(self, self.displayItem)` (AddImplicitPopup.lua)
    pub add_implicit_to_display_item: Rc<dyn Fn(&mut Cx, CtrlId, Option<model::ItemRef>)>,
    // ---- U37 (ItemsTab.lua:3121-3992) ----
    /// `self:CorruptDisplayItem()` (L3121-3514)
    pub corrupt_display_item: ItemsUiHook,
    /// `self:AddCustomModifierToDisplayItem()` (L3516-3868)
    pub add_custom_modifier_to_display_item: ItemsUiHook,
    /// `self:AddCrucibleModifierToDisplayItem()` (L3870-3980)
    pub add_crucible_modifier_to_display_item: ItemsUiHook,
    // ---- U24 (PassiveTreeView.lua:1412-1856) ----
    /// `self.socketViewer:AddNodeName(tooltip, node, self.build)`
    pub socket_viewer_add_node_name: Rc<dyn Fn(&mut Cx, CtrlId, &mut Tooltip, &model::NodeRef)>,
    // ---- U25 (TreeTab) ----
    /// `self.build.treeTab:OpenSpecManagePopup()`
    pub tree_open_spec_manage_popup: ItemsUiHook,
    // ---- U51 (TradeQuery, CompareBuySimilar) ----
    /// `self.tradeQuery:PriceItem()`
    pub trade_query_price_item: ItemsUiHook,
    /// `buySimilar.openPopup(item, itemSlot, self.build)`
    pub buy_similar_open_popup: Rc<dyn Fn(&mut Cx, CtrlId, Option<model::ItemRef>, Option<String>)>,
}

#[track_caller]
fn not_ported(what: &str, owner: &str) -> ! {
    lua_error!("{what} is not ported yet ({owner})")
}

/// The defaults: every entry raises a Lua error naming its owner (PoB's error prompt shows it).
pub fn default_items_ui_hooks() -> ItemsUiHooks {
    ItemsUiHooks {
        add_display_item: Rc::new(|_, _, _| not_ported("ItemsTab:AddDisplayItem", "U34")),
        select_display_item_variant: Rc::new(|_, _, _, _, _, _| {
            not_ported("ItemsTab:SelectDisplayItemVariant", "U35")
        }),
        update_display_item_variant_controls: Rc::new(|_, _| {
            not_ported("ItemsTab:UpdateDisplayItemVariantControls", "U35")
        }),
        update_display_item_tooltip: Rc::new(|_, _| {
            not_ported("ItemsTab:UpdateDisplayItemTooltip", "U35")
        }),
        update_socket_controls: Rc::new(|_, _| not_ported("ItemsTab:UpdateSocketControls", "U35")),
        craft_cluster_jewel: Rc::new(|_, _| not_ported("ItemsTab:CraftClusterJewel", "U35")),
        update_affix_controls: Rc::new(|_, _| not_ported("ItemsTab:UpdateAffixControls", "U35")),
        update_custom_controls: Rc::new(|_, _| {
            not_ported("ItemsTab:UpdateCustomControls", "U35")
        }),
        update_display_item_range_lines: Rc::new(|_, _| {
            not_ported("ItemsTab:UpdateDisplayItemRangeLines", "U35")
        }),
        add_mod_comparison_tooltip: Rc::new(|_, _, _, _| {
            not_ported("ItemsTab:AddModComparisonTooltip", "U35")
        }),
        get_comparison_slot_name_for_item: Rc::new(|_, _, _| {
            not_ported("ItemsTab:GetComparisonSlotNameForItem", "U35")
        }),
        open_item_set_manage_popup: Rc::new(|_, _| {
            not_ported("ItemsTab:OpenItemSetManagePopup", "U36")
        }),
        craft_item: Rc::new(|_, _| not_ported("ItemsTab:CraftItem", "U36")),
        edit_display_item_text: Rc::new(|_, _, _| {
            not_ported("ItemsTab:EditDisplayItemText", "U36")
        }),
        enchant_display_item: Rc::new(|_, _, _| not_ported("ItemsTab:EnchantDisplayItem", "U36")),
        append_added_notable_tooltip: Rc::new(|_, _, _, _| {
            not_ported("ItemsTab:AppendAddedNotableTooltip", "U36")
        }),
        anoint_display_item: Rc::new(|_, _, _| not_ported("ItemsTab:AnointDisplayItem", "U36")),
        add_implicit_to_display_item: Rc::new(|_, _, _| {
            not_ported("addImplicit.AddImplicitToDisplayItem", "U36")
        }),
        corrupt_display_item: Rc::new(|_, _| not_ported("ItemsTab:CorruptDisplayItem", "U37")),
        add_custom_modifier_to_display_item: Rc::new(|_, _| {
            not_ported("ItemsTab:AddCustomModifierToDisplayItem", "U37")
        }),
        add_crucible_modifier_to_display_item: Rc::new(|_, _| {
            not_ported("ItemsTab:AddCrucibleModifierToDisplayItem", "U37")
        }),
        socket_viewer_add_node_name: Rc::new(|_, _, _, _| {
            not_ported("PassiveTreeView:AddNodeName", "U24")
        }),
        tree_open_spec_manage_popup: Rc::new(|_, _| {
            not_ported("TreeTab:OpenSpecManagePopup", "U25")
        }),
        trade_query_price_item: Rc::new(|_, _| not_ported("TradeQuery:PriceItem", "U51")),
        buy_similar_open_popup: Rc::new(|_, _, _, _| {
            not_ported("CompareBuySimilar.openPopup", "U51")
        }),
    }
}

thread_local! {
    static UI_HOOKS: RefCell<Option<ItemsUiHooks>> = const { RefCell::new(None) };
}

/// The installed hooks (a clone; entries are `Rc`s).
pub fn items_ui_hooks() -> ItemsUiHooks {
    UI_HOOKS.with(|h| {
        h.borrow_mut()
            .get_or_insert_with(default_items_ui_hooks)
            .clone()
    })
}

/// Replace hook entries (the owners, and tests).
pub fn set_items_ui_hooks(f: impl FnOnce(&mut ItemsUiHooks)) {
    UI_HOOKS.with(|h| f(h.borrow_mut().get_or_insert_with(default_items_ui_hooks)));
}

// ---- helpers (Rust-only plumbing) ------------------------------------------------------------

fn st(cx: &Cx, me: CtrlId) -> &State {
    &cx.w::<ItemsTab>(me).ctor
}

fn st_mut(cx: &mut Cx, me: CtrlId) -> &mut State {
    &mut cx.w_mut::<ItemsTab>(me).ctor
}

/// `self.displayItem` (nil → `None`).
pub fn display_item(cx: &Cx, me: CtrlId) -> Option<model::ItemRef> {
    st(cx, me).displayItem.clone()
}

/// `self.displayItem` that Lua then indexes (nil raises).
#[track_caller]
fn di(cx: &Cx, me: CtrlId) -> model::ItemRef {
    display_item(cx, me)
        .unwrap_or_else(|| lua_error!("attempt to index field 'displayItem' (a nil value)"))
}

/// `self.controls[key]` looked up at call time (late binding; nil raises when indexed).
#[track_caller]
fn ctl(cx: &Cx, me: CtrlId, key: impl Into<CtrlKey>) -> CtrlId {
    let key = key.into();
    cx.child(me, key.clone())
        .unwrap_or_else(|| lua_error!("attempt to index field '{key:?}' (a nil value)"))
}

/// `item.<k>` of an untyped Item field.
fn ex(item: &model::ItemRef, k: Name) -> Val {
    item.extra.get(k)
}

/// `item.<k>` truthiness.
fn ext(item: &model::ItemRef, k: Name) -> bool {
    item.extra.truthy(k)
}

/// `item.<k>` in arithmetic or a comparison (nil raises).
#[track_caller]
fn exn(item: &model::ItemRef, k: Name) -> f64 {
    match item.extra.get(k) {
        Val::Num(x) => x,
        other => lua_error!(
            "attempt to compare/perform arithmetic on field '{k}' (a {} value)",
            other.lua_type()
        ),
    }
}

/// `item.base.type` (an index of a nil base raises).
#[track_caller]
fn base_type(item: &model::ItemRef) -> Name {
    item.base
        .get()
        .unwrap_or_else(|| lua_error!("attempt to index field 'base' (a nil value)"))
        .r#type
}

/// `#item.sockets`
fn sockets_len(item: &model::ItemRef) -> usize {
    item.sockets.get().len()
}

/// `item.sockets[i]` (1-based; nil outside the list).
fn socket(item: &model::ItemRef, i: usize) -> Option<Table> {
    if i == 0 {
        return None;
    }
    item.sockets.get().with(|s| s.get(i - 1).cloned())
}

/// `item.rangeLineList` (indexing a nil list raises).
#[track_caller]
fn range_line_list(item: &model::ItemRef) -> Table {
    ex(item, n!("rangeLineList"))
        .table()
        .unwrap_or_else(|| lua_error!("attempt to index field 'rangeLineList' (a nil value)"))
}

/// `self.activeItemSet` (indexing nil raises).
#[track_caller]
fn active_item_set(cx: &Cx) -> Table {
    cx.build()
        .model
        .itemsTab
        .activeItemSet
        .get()
        .unwrap_or_else(|| lua_error!("attempt to index field 'activeItemSet' (a nil value)"))
}

/// `self.activeItemSet.useSecondWeaponSet`
fn use_second_weapon_set(cx: &Cx) -> bool {
    active_item_set(cx).truthy(n!("useSecondWeaponSet"))
}

/// `self.displayItem.affixes[modId]` (nil id or unknown id → nil).
fn affix_mod(
    item: &model::ItemRef,
    mod_id: Option<Name>,
) -> Option<gamedata::schema::affixes::ItemModRef> {
    let id = mod_id?;
    let t = item
        .affixes_table()
        .unwrap_or_else(|| lua_error!("attempt to index field 'affixes' (a nil value)"));
    t.0.get(&id).cloned()
}

/// An `affixes[..]` entry that Lua then indexes (nil raises).
#[track_caller]
fn affix_mod_or_err(
    item: &model::ItemRef,
    mod_id: Option<Name>,
) -> gamedata::schema::affixes::ItemModRef {
    affix_mod(item, mod_id).unwrap_or_else(|| lua_error!("attempt to index local 'mod' (a nil value)"))
}

/// `drop.list[drop.selIndex]` of an affix dropdown, as its series (the "None" entry and a
/// missing entry have no `modList`).
fn sel_series(cx: &Cx, drop: CtrlId) -> Option<Rc<AffixSeries>> {
    let w = cx.w::<DropDownControl>(drop);
    let item = w.list.get(w.sel_index.checked_sub(1)?)?;
    item.value.clone().downcast::<AffixSeries>().ok()
}

/// `drop.list[drop.selIndex]` that Lua then indexes (nil raises).
#[track_caller]
fn sel_item_or_err(cx: &Cx, drop: CtrlId) -> DropItem {
    let w = cx.w::<DropDownControl>(drop);
    w.sel_index
        .checked_sub(1)
        .and_then(|i| w.list.get(i))
        .cloned()
        .unwrap_or_else(|| lua_error!("attempt to index field '?' (a nil value)"))
}

/// The series of a dropdown item (`value.modList` / `value.modId`).
fn series_of(item: &DropItem) -> Option<Rc<AffixSeries>> {
    item.value.clone().downcast::<AffixSeries>().ok()
}

/// `self.displayItem[drop.outputTable][drop.outputIndex]` (the table, raising like Lua when an
/// index of a nil value happens).
#[track_caller]
fn output_affix(cx: &Cx, me: CtrlId, i: usize) -> (Table, f64) {
    let item = di(cx, me);
    let a = &st(cx, me).affix_controls[i - 1];
    let tbl = a
        .outputTable
        .and_then(|t| ex(&item, t).table())
        .unwrap_or_else(|| lua_error!("attempt to index a nil value (field '?')"));
    let idx = a
        .outputIndex
        .unwrap_or_else(|| lua_error!("attempt to index a nil value (field '?')"));
    (tbl, idx)
}

/// Force affix selectors to update (L601-605, L671-675, L686-690): `for i = 1,
/// self.displayItem.affixLimit do local drop = ...; drop.selFunc(drop.selIndex,
/// drop.list[drop.selIndex]) end`.
fn force_affix_selectors(cx: &mut Cx, me: CtrlId) {
    let item = di(cx, me);
    let limit = exn(&item, n!("affixLimit"));
    let mut i = 1.0;
    while i <= limit {
        // Force affix selectors to update
        let drop = ctl(cx, me, format!("displayItemAffix{}", tostring(i)));
        let (f, sel, value) = {
            let w = cx.w::<DropDownControl>(drop);
            (
                w.sel_func.clone(),
                w.sel_index,
                w.sel_index.checked_sub(1).and_then(|k| w.list.get(k)).cloned(),
            )
        };
        let f = f.unwrap_or_else(|| lua_error!("attempt to call field 'selFunc' (a nil value)"));
        f(cx, sel, value.as_ref());
        i += 1.0;
    }
}

/// `function(...)` of a DropDownControl selFunc.
fn sel_func(f: impl Fn(&mut Cx, usize, Option<&DropItem>) + 'static) -> Option<SelFunc> {
    Some(Rc::new(f))
}

/// A dropdown over plain strings.
fn str_list(items: &[&str]) -> Vec<DropItem> {
    items.iter().map(|s| DropItem::str(*s)).collect()
}

fn add_undo_state(cx: &mut Cx) {
    let f = items_hooks().add_undo_state;
    f(cx);
}

fn update_display_item_tooltip(cx: &mut Cx, me: CtrlId) {
    let f = items_ui_hooks().update_display_item_tooltip;
    f(cx, me);
}

fn update_custom_controls(cx: &mut Cx, me: CtrlId) {
    let f = items_ui_hooks().update_custom_controls;
    f(cx, me);
}

/// `drop.slider` of affix dropdown `i` (`self.controls["displayItemAffixRange"..i]`).
fn affix_slider(cx: &Cx, me: CtrlId, i: usize) -> CtrlId {
    ctl(cx, me, format!("displayItemAffixRange{i}"))
}

/// `self.lastSlot` (L1212; UpdateSockets, L1673-1676, moves it to the last active socket): the
/// model keeps it as the slot table (`build_extra(build).itemsTab.lastSlot`, calc:BM-A), this is
/// that table's control (`None` while the model has none).
pub fn lastSlot(cx: &Cx, me: CtrlId) -> Option<CtrlId> {
    let m = cx.build().model.clone();
    let slot = model::build_xml::build_extra(&m)
        .table(n!("itemsTab"))?
        .table(n!("lastSlot"))?;
    let name = Name::from(&slot.str(n!("slotName"))?);
    st(cx, me).slots.get(&name).copied()
}

// ---- the constructor ---------------------------------------------------------------------------

impl ItemsTab {
    /// lua: ItemsTab.lua:93-1215 `ItemsTabClass:ItemsTab(build)` (model part: calc:BM-A
    /// `ItemsTab::ctor_model`, run by `build_xml::Init_model` before the UI constructors).
    /// @pob Classes/ItemsTab.lua:93-100 h=2129fa55
    pub fn new(build: CtrlId) -> New<ItemsTab> {
        // L94-96 self:UndoHandler(); self:ControlHost(); self:Control()
        New::new(Base::new(Anchor::NONE, rect(0, 0, 0, 0)), ItemsTab::default())
            .host()
            .init(move |cx, me| {
                let me = me.id();
                // L98
                st_mut(cx, me).build = build;
                // L100 self.socketViewer = new("PassiveTreeView"):PassiveTreeView() (U23: the
                // PassiveTreeView constructor is not ported yet; the field holds its Default)
                // L102-108 (model part: calc:BM-A; `self.tradeQuery` is U51's)
                ctor_sets(cx, me);
                ctor_slots(cx, me, build);
                ctor_lists(cx, me);
                ctor_display_head(cx, me);
                ctor_variants(cx, me);
                ctor_sockets(cx, me);
                ctor_enchant(cx, me);
                ctor_influence(cx, me);
                ctor_quality_catalyst(cx, me);
                ctor_cluster(cx, me);
                ctor_affixes(cx, me);
                ctor_custom(cx, me);
                ctor_range(cx, me);
                ctor_stacked_range(cx, me);
                ctor_tail(cx, me, build);
            })
    }
}

/// L110-138: the set selector and the trade button.
/// @pob Classes/ItemsTab.lua:109-139 h=10994378
fn ctor_sets(cx: &mut Cx, me: CtrlId) {
    // Set selector
    // L111
    let set_select = cx
        .add(
            me,
            "setSelect",
            DropDownControl::new(
                Anchor::new(TopLeft, me, TopLeft),
                rect(96, 8, 216, 20),
                Vec::new(),
                sel_func(|cx, index, _value| {
                    // L112 self:SetActiveItemSet(self.itemSetOrderList[index]) (nil: no set has
                    // that id, `self.itemSets[nil]` is nil: NaN is no id either)
                    let id = cx
                        .build()
                        .model
                        .itemsTab
                        .itemSetOrderList
                        .with(|l| index.checked_sub(1).and_then(|k| l.get(k).copied()));
                    let f = items_hooks().set_active_item_set;
                    f(cx, id.map_or(f64::NAN, f64::from));
                    // L113
                    add_undo_state(cx);
                }),
                None,
                false,
            ),
        )
        .id();
    // L115
    cx.w_mut::<DropDownControl>(set_select).enable_dropped_width = true;
    // L116-118
    cx.base_mut(set_select).enabled =
        Prop::f(|cx, _| cx.build().model.itemsTab.itemSetOrderList.len() > 1);
    // L119-124
    cx.w_mut::<DropDownControl>(set_select).tt.tooltip_func =
        Some(Rc::new(|cx, tooltip, (mode, index, _value)| {
            // L120
            tooltip.clear(false);
            // L121
            if mode == DropTipMode::Hover {
                // L122 self:AddItemSetTooltip(tooltip, self.itemSets[self.itemSetOrderList[index]])
                let m = cx.build().model.clone();
                let set = index
                    .and_then(|i| m.itemsTab.itemSetOrderList.with(|l| l.get(i - 1).copied()))
                    .and_then(|id| m.itemsTab.itemSets.with(|s| s.get(&id).cloned()))
                    .unwrap_or_else(|| lua_error!("attempt to index local 'itemSet' (a nil value)"));
                let f = items_hooks().add_item_set_tooltip;
                f(cx, tooltip, &set);
            }
        }));
    // L125
    cx.add(
        me,
        "setLabel",
        LabelControl::new(
            Anchor::new(Right, set_select, Left),
            rect(-2, 0, 0, 16),
            "^7Item set:",
        ),
    );
    // L126-128
    cx.add(
        me,
        "setManage",
        ButtonControl::new(
            Anchor::new(Left, set_select, Right),
            rect(4, 0, 90, 20),
            "Manage...",
            move |cx| {
                // L127
                let f = items_ui_hooks().open_item_set_manage_popup;
                f(cx, me);
            },
        ),
    );

    // Price Items
    // L131-133
    let price = cx.add(
        me,
        "priceDisplayItem",
        ButtonControl::new(
            Anchor::new(TopLeft, me, TopLeft),
            rect(96, 32, 310, 20),
            "Trade for these items",
            move |cx| {
                // L132
                let f = items_ui_hooks().trade_query_price_item;
                f(cx, me);
            },
        ),
    );
    // L134-138
    cx.get_mut(price).tt.tooltip_func = Some(Rc::new(|cx, tooltip, ()| {
        // L135
        tooltip.clear(false);
        // L136
        tooltip.add_line(
            cx,
            16.0,
            "^7Contains searches from the official trading site to help find",
        );
        // L137
        tooltip.add_line(cx, 16.0, "^7similar or better items for this build");
    }));
}

/// `slot.shown` of ItemsTab.lua:159-197 for a model slot of `$shown` kind `kind` (`None`: the
/// ItemSlotControl's own L34-36).
fn slot_shown(kind: Option<&str>, slot: &Table) -> Option<Prop<bool>> {
    let s = slot.clone();
    Some(match kind? {
        // L159-161
        "weapon" => Prop::f(|cx, _| !use_second_weapon_set(cx)),
        // L165-167
        "weaponSwap" => Prop::f(|cx, _| use_second_weapon_set(cx)),
        // L173-175
        "abyssalSwap" => {
            Prop::f(move |cx, _| !s.truthy(n!("inactive")) && use_second_weapon_set(cx))
        }
        // L195-197
        "abyssalWeapon" => {
            Prop::f(move |cx, _| !s.truthy(n!("inactive")) && !use_second_weapon_set(cx))
        }
        // L179-181 (`self.build.spec.treeVersion:find("3_27")`)
        "graft" => Prop::f(|cx, _| {
            let tv = cx.build().model.spec.get().treeVersion.get();
            let found = pattern::find(&tv, "3_27", None, false).is_some();
            found
        }),
        // L183-185
        "ring3" => Prop::f(|cx, _| {
            let env = cx
                .build()
                .model
                .calcsTab
                .mainEnv
                .get()
                .unwrap_or_else(|| lua_error!("attempt to index field 'mainEnv' (a nil value)"));
            env.modDB.get().Flag(None, pob_lua::nm!["AdditionalRingSlot"])
        }),
        _ => return None,
    })
}

/// L140-276: the item slots, the passive tree selector, the sockets and the weapon swap buttons.
/// L140-235 have calc:BM-A's marker (its `ctor_model` built the slot tables); the UI statements
/// are commented with their lines.
fn ctor_slots(cx: &mut Cx, me: CtrlId, _build: CtrlId) {
    let m = cx.build().model.clone();
    // Item slots
    // L141-143 (model part: calc:BM-A; the control maps are `State::{slots, orderedSlots}`)
    // L144
    let slot_anchor = cx
        .add_free(
            Some(me),
            PlainControl::new(Anchor::new(TopLeft, me, TopLeft), rect(96, 76, 310, 0)),
        )
        .id();
    st_mut(cx, me).slotAnchor = slot_anchor;
    // L145
    let mut prevSlot = slot_anchor;
    // L146-152 addSlot(slot) (the model half: calc:BM-A)
    let mut n_controls: i64 = 0;
    let add_slot = |cx: &mut Cx, prev: &mut CtrlId, n_controls: &mut i64, slot: &Table| {
        let kind = slot.str(n!("$shown")).map(|s| s.to_string());
        // L154 / L162 / L169 / L190 / L231 new("ItemSlotControl"):ItemSlotControl({"TOPLEFT",
        // prevSlot,"BOTTOMLEFT"}, 0, 2, self, ..), stored by L151 t_insert(self.controls, slot)
        *n_controls += 1;
        let id = cx
            .add(
                me,
                CtrlKey::Idx(*n_controls),
                ItemSlotControl::new(
                    Anchor::new(TopLeft, *prev, BottomLeft),
                    Prop::Val(0.0),
                    Prop::Val(2.0),
                    me,
                    slot.clone(),
                ),
            )
            .id();
        // L147
        *prev = id;
        let name = Name::from(&slot.str(n!("slotName")).expect("slotName"));
        let s = st_mut(cx, me);
        // L148
        s.slots.insert(name, id);
        // L149
        s.orderedSlots.push(id);
        // L150 (model part: calc:BM-A)
        // L159 / L165 / L173 / L179 / L183 / L195: the `shown` overrides
        if let Some(p) = slot_shown(kind.as_deref(), slot) {
            cx.base_mut(id).shown = p;
        }
        id
    };
    let ordered = m.itemsTab.orderedSlots.snapshot();
    // L153-202 the base slots with their swap and abyssal slots (calc:BM-A's order)
    let (base_slots, socket_slots): (Vec<Table>, Vec<Table>) = ordered
        .into_iter()
        .partition(|s| s.get(n!("nodeId")).is_nil());
    for slot in &base_slots {
        add_slot(cx, &mut prevSlot, &mut n_controls, slot);
    }

    // Passive tree dropdown controls
    // L205-210
    let spec_select = cx
        .add(
            me,
            "specSelect",
            DropDownControl::new(
                Anchor::new(TopLeft, prevSlot, BottomLeft),
                rect(0, 8, 216, 20),
                Vec::new(),
                sel_func(|cx, index, _value| {
                    // L206
                    let has = cx
                        .build()
                        .model
                        .treeTab
                        .specList
                        .with(|l| index >= 1 && index <= l.len());
                    if has {
                        // L207
                        cx.build_mut().mod_flag = true;
                        // L208
                        let f = crate::build::init::hooks(cx).tree_set_active_spec;
                        f(cx, index as f64);
                    }
                }),
                None,
                false,
            ),
        )
        .id();
    // L211-213
    cx.base_mut(spec_select).enabled =
        Prop::f(|cx, me| cx.w::<DropDownControl>(me).list.len() > 1);
    // L214
    prevSlot = spec_select;
    // L215-217
    cx.add(
        me,
        "specButton",
        ButtonControl::new(
            Anchor::new(Left, prevSlot, Right),
            rect(4, 0, 90, 20),
            "Manage...",
            move |cx| {
                // L216
                let f = items_ui_hooks().tree_open_spec_manage_popup;
                f(cx, me);
            },
        ),
    );
    // L218
    cx.add(
        me,
        "specLabel",
        LabelControl::new(
            Anchor::new(Right, prevSlot, Left),
            rect(-2, 0, 0, 16),
            "^7Passive tree:",
        ),
    );

    // L220-229 (model part: calc:BM-A)
    // L230
    for slot in &socket_slots {
        // L231, L233
        let id = add_slot(cx, &mut prevSlot, &mut n_controls, slot);
        // L232
        let node_id = slot.num(n!("nodeId"));
        st_mut(cx, me).sockets.set(node_id as i64, Some(id));
    }
    // L235
    cx.add(
        me,
        "slotHeader",
        LabelControl::new(
            Anchor::new(BottomLeft, slot_anchor, TopLeft),
            rect(0, -4, 0, 16),
            "^7Equipped items:",
        ),
    );
    ctor_weapon_swap(cx, me, slot_anchor);
}

/// `self.slots[name].weaponSet` (an index of a missing slot raises).
fn slot_weapon_set(cx: &Cx, name: Name) -> Val {
    cx.build()
        .model
        .itemsTab
        .slots
        .with(|s| s.get(&name).cloned())
        .unwrap_or_else(|| lua_error!("attempt to index field '?' (a nil value)"))
        .get(n!("weaponSet"))
}

/// The weapon swap buttons' onClick (L237-250 with `to = 1`, L257-270 with `to = 2`).
fn weapon_swap(cx: &mut Cx, to: f64) {
    let set = active_item_set(cx);
    // L237 / L257
    let from_second = set.truthy(n!("useSecondWeaponSet"));
    if (to == 1.0 && from_second) || (to == 2.0 && !from_second) {
        // L238 / L258
        set.set(n!("useSecondWeaponSet"), to == 2.0);
        // L239 / L259
        add_undo_state(cx);
        // L240 / L260
        cx.build().model.buildFlag.set(true);
        let m = cx.build().model.clone();
        // L241 / L261
        let groups = m.skillsTab.socketGroupList.get().snapshot();
        let msg = m.mainSocketGroup.get();
        let mainSocketGroup = (msg.fract() == 0.0 && msg >= 1.0)
            .then(|| groups.get(msg as usize - 1).cloned())
            .flatten();
        let other = if to == 1.0 { 2.0 } else { 1.0 };
        // L242 / L262
        if let Some(main_group) = mainSocketGroup {
            if let Some(slot) = main_group.slot.get() {
                if slot_weapon_set(cx, slot).raw_eq(&Val::Num(other)) {
                    // L243 / L263
                    for (index, socketGroup) in groups.iter().enumerate() {
                        // L244 / L264
                        if let Some(gs) = socketGroup.slot.get() {
                            if slot_weapon_set(cx, gs).raw_eq(&Val::Num(to)) {
                                // L245 / L265
                                m.mainSocketGroup.set((index + 1) as f64);
                                // L246 / L266
                                break;
                            }
                        }
                    }
                }
            }
        }
    }
}

/// L236-276: the weapon swap buttons.
/// @pob Classes/ItemsTab.lua:236-277 h=75bf435f
fn ctor_weapon_swap(cx: &mut Cx, me: CtrlId, slot_anchor: CtrlId) {
    // L236-251
    let swap1 = cx.add(
        me,
        "weaponSwap1",
        ButtonControl::new(
            Anchor::new(BottomRight, slot_anchor, TopRight),
            rect(-20, -2, 18, 18),
            "I",
            |cx| weapon_swap(cx, 1.0),
        ),
    );
    // L252
    cx.get_mut(swap1).over_size_text = 3.0;
    // L253-255
    cx.get_mut(swap1).locked = Prop::f(|cx, _| !use_second_weapon_set(cx));
    // L256-271
    let swap2 = cx.add(
        me,
        "weaponSwap2",
        ButtonControl::new(
            Anchor::new(BottomRight, slot_anchor, TopRight),
            rect(0, -2, 18, 18),
            "II",
            |cx| weapon_swap(cx, 2.0),
        ),
    );
    // L272
    cx.get_mut(swap2).over_size_text = 3.0;
    // L273-275
    cx.get_mut(swap2).locked = Prop::f(|cx, _| use_second_weapon_set(cx));
    // L276
    cx.add(
        me,
        "weaponSwapLabel",
        LabelControl::new(
            Anchor::new(Right, swap1, Left),
            rect(-4, 0, 0, 14),
            "^7Weapon Set:",
        ),
    );
}

/// The `{"TOPLEFT", main.portraitMode and self.controls.setManage or self.controls.itemList,
/// "TOPRIGHT"}` anchor target and its `y` (L310, L337).
fn display_anchor(cx: &Cx, me: CtrlId) -> (CtrlId, f64) {
    if cx.app().main.portrait_mode {
        (ctl(cx, me, "setManage"), 0.0)
    } else {
        (ctl(cx, me, "itemList"), -20.0)
    }
}

/// `m_min(cap, self.maxY - select(2, c:GetPos()))` (the DB heights, L293, L302).
fn db_height(cx: &Cx, me: CtrlId, c: CtrlId, cap: f64) -> f64 {
    let max_y = st(cx, me)
        .maxY
        .unwrap_or_else(|| lua_error!("attempt to perform arithmetic on field 'maxY' (a nil value)"));
    m_min(cap, max_y - cx.pos(c).1)
}

/// L278-332: the item list, the database selector, the unique and rare databases, the craft
/// buttons, the tip and the shared item list.
/// @pob Classes/ItemsTab.lua:278-333 h=aac7802d
fn ctor_lists(cx: &mut Cx, me: CtrlId) {
    let m = cx.build().model.clone();
    // All items list
    // L279
    let item_list = if cx.app().main.portrait_mode {
        // L280 (`self.lastSlot` is still nil here: an anchor without `other`; Draw re-anchors)
        cx.add(
            me,
            "itemList",
            ItemListControl::new(
                Anchor::nil(TopRight, BottomRight),
                rect(0, 0, 360, 308),
                me,
                m.itemsTab.orderList.clone(),
                true,
            ),
        )
        .id()
    } else {
        // L282
        let set_manage = ctl(cx, me, "setManage");
        cx.add(
            me,
            "itemList",
            ItemListControl::new(
                Anchor::new(TopLeft, set_manage, TopRight),
                rect(20, 20, 360, 308),
                me,
                m.itemsTab.orderList.clone(),
                true,
            ),
        )
        .id()
    };

    // Database selector
    // L286
    let select_db_label = cx
        .add(
            me,
            "selectDBLabel",
            LabelControl::new(
                Anchor::new(TopLeft, item_list, BottomLeft),
                rect(0, 14, 0, 16),
                "^7Import from:",
            ),
        )
        .id();
    // L287-289
    cx.base_mut(select_db_label).shown = Prop::f(move |cx, _| cx.base(me).height.raw() < 980.0);
    // L290
    let select_db = cx
        .add(
            me,
            "selectDB",
            DropDownControl::new(
                Anchor::new(Left, select_db_label, Right),
                rect(4, 0, 150, 18),
                str_list(&["Uniques", "Rare Templates"]),
                None,
                None,
                false,
            ),
        )
        .id();

    // Unique database
    // L293 (`main.uniqueDB`: DbType::Unique)
    let unique_db = cx
        .add(
            me,
            "uniqueDB",
            ItemDBControl::new(
                Anchor::new(TopLeft, item_list, BottomLeft),
                rect(0, 76, 360, Prop::f(move |cx, c| db_height(cx, me, c, 244.0))),
                me,
                DbType::Unique,
            ),
        )
        .id();
    // L294-296
    cx.base_mut(unique_db).y = Prop::f(move |cx, _| {
        if cx.is_shown(select_db_label) {
            118.0
        } else {
            96.0
        }
    });
    // L297-299
    cx.base_mut(unique_db).shown = Prop::f(move |cx, _| {
        !cx.is_shown(select_db_label) || cx.w::<DropDownControl>(select_db).sel_index == 1
    });

    // Rare template database
    // L302
    let rare_db = cx
        .add(
            me,
            "rareDB",
            ItemDBControl::new(
                Anchor::new(TopLeft, item_list, BottomLeft),
                rect(0, 76, 360, Prop::f(move |cx, c| db_height(cx, me, c, 260.0))),
                me,
                DbType::Rare,
            ),
        )
        .id();
    // L303-305
    cx.base_mut(rare_db).y = Prop::f(move |cx, _| {
        if cx.is_shown(select_db_label) {
            78.0
        } else {
            396.0
        }
    });
    // L306-308
    cx.base_mut(rare_db).shown = Prop::f(move |cx, _| {
        !cx.is_shown(select_db_label) || cx.w::<DropDownControl>(select_db).sel_index == 2
    });
    // Create/import item
    // L310-312
    let (anchor_to, anchor_y) = display_anchor(cx, me);
    let craft = cx
        .add(
            me,
            "craftDisplayItem",
            ButtonControl::new(
                Anchor::new(TopLeft, anchor_to, TopRight),
                rect(20, anchor_y, 120, 20),
                "Craft item...",
                move |cx| {
                    // L311
                    let f = items_ui_hooks().craft_item;
                    f(cx, me);
                },
            ),
        )
        .id();
    // L313-315
    cx.base_mut(craft).shown = Prop::f(move |cx, _| st(cx, me).displayItem.is_none());
    // L316-318
    cx.add(
        me,
        "newDisplayItem",
        ButtonControl::new(
            Anchor::new(TopLeft, craft, TopRight),
            rect(8, 0, 120, 20),
            "Create custom...",
            move |cx| {
                // L317 self:EditDisplayItemText() (alsoAddItem nil)
                let f = items_ui_hooks().edit_display_item_text;
                f(cx, me, false);
            },
        ),
    );
    // L319-331
    cx.add(
        me,
        "displayItemTip",
        LabelControl::new(
            Anchor::new(TopLeft, craft, BottomLeft),
            rect(0, 8, 100, 16),
            DISPLAY_ITEM_TIP,
        ),
    );
    // L332
    cx.add(
        me,
        "sharedItemList",
        SharedItemListControl::new(
            Anchor::new(TopLeft, craft, BottomLeft),
            rect(0, 232, 340, 308),
            me,
            true,
        ),
    );
}

/// The tip label (L320-331, a long string).
const DISPLAY_ITEM_TIP: &str = "^7Double-click an item from one of the lists,
or copy and paste an item from in game
(hover over the item and Ctrl+C) to view or edit
the item and add it to your build. You can
also clone an item within Path of Building by
copying and pasting it with Ctrl+C and Ctrl+V.

You can Control + Click an item to equip it, or
drag it onto the slot.  This will also add it to
your build if it's from the unique/template list.
If there's 2 slots an item can go in,
holding Shift will put it in the second.";

/// L334-361: the display item tooltip, its anchor and the add/edit/cancel/buy buttons.
/// @pob Classes/ItemsTab.lua:334-361 h=c7d7fda3
fn ctor_display_head(cx: &mut Cx, me: CtrlId) {
    // Display item
    // L335
    st_mut(cx, me).displayItemTooltip = Tooltip::default();
    // L336
    st_mut(cx, me).displayItemTooltip.max_width = Some(458.0);
    // L337
    let (anchor_to, anchor_y) = display_anchor(cx, me);
    let anchor = cx
        .add_free(
            Some(me),
            PlainControl::new(
                Anchor::new(TopLeft, anchor_to, TopRight),
                rect(20, anchor_y, 0, 0),
            ),
        )
        .id();
    st_mut(cx, me).anchorDisplayItem = anchor;
    // L338-340
    cx.base_mut(anchor).shown = Prop::f(move |cx, _| st(cx, me).displayItem.is_some());
    // L341-343
    let add = cx
        .add(
            me,
            "addDisplayItem",
            ButtonControl::new(
                Anchor::new(TopLeft, anchor, TopLeft),
                rect(0, 0, 100, 20),
                "",
                move |cx| {
                    // L342 self:AddDisplayItem() (noAutoEquip nil)
                    let f = items_ui_hooks().add_display_item;
                    f(cx, me, false);
                },
            ),
        )
        .id();
    // L344-346
    cx.w_mut::<ButtonControl>(add).label = Prop::f(move |cx, _| {
        // L345 `self.items[self.displayItem.id] and "Save" or "Add to build"`
        let id = di(cx, me).id.get();
        let held = id
            .and_then(model::items_tab::id_key)
            .is_some_and(|k| cx.build().model.itemsTab.items.with(|m| m.contains_key(&k)));
        if held { "Save" } else { "Add to build" }.to_owned()
    });
    // L347-349
    let edit = cx
        .add(
            me,
            "editDisplayItem",
            ButtonControl::new(
                Anchor::new(Left, add, Right),
                rect(8, 0, 60, 20),
                "Edit...",
                move |cx| {
                    // L348
                    let f = items_ui_hooks().edit_display_item_text;
                    f(cx, me, false);
                },
            ),
        )
        .id();
    // L350-352
    let remove = cx
        .add(
            me,
            "removeDisplayItem",
            ButtonControl::new(
                Anchor::new(Left, edit, Right),
                rect(8, 0, 60, 20),
                "Cancel",
                |cx| {
                    // L351 self:SetDisplayItem()
                    let f = items_hooks().set_display_item;
                    f(cx, None);
                },
            ),
        )
        .id();

    // L354-358
    let buy = cx
        .add(
            me,
            "displayItemBuySimilar",
            ButtonControl::new(
                Anchor::new(Left, remove, Right).collapse(),
                rect(8, 0, 100, 20),
                "Buy similar",
                move |cx| {
                    // L356
                    let item = display_item(cx, me);
                    let itemSlot = match &item {
                        Some(it) => {
                            let f = items_ui_hooks().get_comparison_slot_name_for_item;
                            f(cx, me, it)
                        }
                        None => lua_error!("attempt to index local 'item' (a nil value)"),
                    };
                    // L357
                    let f = items_ui_hooks().buy_similar_open_popup;
                    f(cx, me, item, itemSlot);
                },
            ),
        )
        .id();
    // L359-361
    cx.base_mut(buy).shown = Prop::f(move |cx, _| st(cx, me).displayItem.is_some());
}

/// `pairsSortByKey(t)`'s keys (Common.lua:657-670: `table.sort` of the keys with `<`).
fn sorted_keys(t: &Table) -> Vec<model::Key> {
    let mut keys: Vec<model::Key> = t.pairs().map(|(k, _)| k).collect();
    let num = |k: &model::Key| {
        k.as_num()
            .unwrap_or_else(|| lua_error!("attempt to compare a non-number table key"))
    };
    pob_lua::sort(&mut keys, |a, b| num(a) < num(b));
    keys
}

/// `#t` of an item field that Lua takes the length of (nil raises).
fn ex_len(item: &model::ItemRef, k: Name) -> usize {
    ex(item, k)
        .table()
        .unwrap_or_else(|| lua_error!("attempt to get length of field '{k}' (a nil value)"))
        .len()
}

/// `self.displayItem.variantList and #self.displayItem.variantList > 1` (L410).
fn variant_list_gt1(item: &model::ItemRef) -> bool {
    item.variantList.get().is_some_and(|l| l.len() > 1)
}

/// L362-463: the variant section and its dropdowns.
/// @pob Classes/ItemsTab.lua:362-464 h=cf53882a
fn ctor_variants(cx: &mut Cx, me: CtrlId) {
    // Section: Variant(s)

    // L364-388
    let add = ctl(cx, me, "addDisplayItem");
    let section = cx
        .add(
            me,
            "displayItemSectionVariant",
            PlainControl::new(
                Anchor::new(TopLeft, add, BottomLeft),
                rect(
                    0,
                    8,
                    0,
                    Prop::f(move |cx, _| {
                        // L365
                        let Some(item) = display_item(cx, me) else {
                            // L366
                            return 0.0;
                        };
                        // L368
                        if ext(&item, n!("usesVariantGroups")) {
                            // L369
                            let mut rows = if ex(&item, n!("versionList")).truthy()
                                && ex_len(&item, n!("versionList")) > 1
                            {
                                1.0
                            } else {
                                0.0
                            };
                            // L370
                            let mut groups = 0.0;
                            // L371
                            let vg = ex(&item, n!("variantGroups")).table().unwrap_or_else(|| {
                                lua_error!("bad argument #1 to 'pairs' (table expected, got nil)")
                            });
                            for groupId in sorted_keys(&vg) {
                                // L372
                                if groups < 6.0
                                    && !item
                                        .GetVariantGroupOptions(
                                            groupId.as_num().unwrap_or(f64::NAN),
                                            false,
                                        )
                                        .is_empty()
                                {
                                    // L373
                                    rows += 1.0;
                                    // L374
                                    groups += 1.0;
                                }
                            }
                            // L377
                            return if rows > 0.0 { rows * 24.0 + 4.0 } else { 0.0 };
                        }
                        // L379
                        if !cx.is_shown(ctl(cx, me, "displayItemVariant")) {
                            // L380
                            return 0.0;
                        }
                        // L382-387
                        28.0 + if ext(&item, n!("hasAltVariant")) { 24.0 } else { 0.0 }
                            + if ext(&item, n!("hasAltVariant2")) { 24.0 } else { 0.0 }
                            + if ext(&item, n!("hasAltVariant3")) { 24.0 } else { 0.0 }
                            + if ext(&item, n!("hasAltVariant4")) { 24.0 } else { 0.0 }
                            + if ext(&item, n!("hasAltVariant5")) { 24.0 } else { 0.0 }
                    }),
                ),
            ),
        )
        .id();
    // L389-396
    let version = cx
        .add(
            me,
            "displayItemVersion",
            DropDownControl::new(
                Anchor::new(TopLeft, section, TopLeft),
                rect(0, 0, 300, 20),
                Vec::new(),
                sel_func(move |cx, index, _value| {
                    let item = di(cx, me);
                    // L390
                    item.extra.set(n!("selectedVersion"), index as f64);
                    // L391
                    item.NormaliseVariantSelections();
                    // L392
                    item.BuildAndParseRaw();
                    let h = items_ui_hooks();
                    // L393
                    (h.update_display_item_variant_controls)(cx, me);
                    // L394
                    (h.update_display_item_tooltip)(cx, me);
                    // L395
                    (h.update_display_item_range_lines)(cx, me);
                }),
                None,
                false,
            ),
        )
        .id();
    // L397
    cx.w_mut::<DropDownControl>(version).max_dropped_width = 1000.0;
    // L398-401
    cx.base_mut(version).shown = Prop::f(move |cx, _| {
        display_item(cx, me).is_some_and(|item| {
            ext(&item, n!("usesVariantGroups"))
                && ex(&item, n!("versionList")).truthy()
                && ex_len(&item, n!("versionList")) > 1
        })
    });
    // L402-446: the six variant dropdowns (legacy field, anchor, `legacyShown`)
    let legacy: [(&'static str, Option<Name>); 6] = [
        // L410
        ("variant", None),
        // L417
        ("variantAlt", Some(n!("hasAltVariant"))),
        // L424
        ("variantAlt2", Some(n!("hasAltVariant2"))),
        // L431
        ("variantAlt3", Some(n!("hasAltVariant3"))),
        // L438
        ("variantAlt4", Some(n!("hasAltVariant4"))),
        // L445
        ("variantAlt5", Some(n!("hasAltVariant5"))),
    ];
    let mut prev = section;
    for (k, (legacy_field, has_alt)) in legacy.into_iter().enumerate() {
        let key = VARIANT_KEYS[k];
        // L402 / L412 / L419 / L426 / L433 / L440
        let (anchor, r) = if k == 0 {
            (Anchor::new(TopLeft, section, TopLeft), rect(0, 0, 300, 20))
        } else {
            (Anchor::new(TopLeft, prev, BottomLeft), rect(0, 4, 300, 20))
        };
        let drop = cx
            .add(
                me,
                key,
                DropDownControl::new(
                    anchor,
                    r,
                    Vec::new(),
                    sel_func(move |cx, index, value| {
                        // L403 / L413 / L420 / L427 / L434 / L441
                        let control = ctl(cx, me, key);
                        let f = items_ui_hooks().select_display_item_variant;
                        f(cx, me, index, value, legacy_field, control);
                    }),
                    None,
                    false,
                ),
            )
            .id();
        if k == 0 {
            // L405-407
            cx.base_mut(drop).y = Prop::f(move |cx, _| {
                if cx.is_shown(version) {
                    24.0
                } else {
                    0.0
                }
            });
        }
        // L408 / L415 / L422 / L429 / L436 / L443
        cx.w_mut::<DropDownControl>(drop).max_dropped_width = 1000.0;
        // L409-411 / L416-418 / ... `legacyShown`; L447-463 wrap it
        let legacyShown = move |cx: &Cx| -> bool {
            let item = di(cx, me);
            match has_alt {
                // L410
                None => variant_list_gt1(&item),
                // L417 ...
                Some(f) => ext(&item, f),
            }
        };
        // L456-459
        cx.base_mut(drop).shown = Prop::f(move |cx, _| {
            let Some(item) = display_item(cx, me) else {
                return false;
            };
            let uses = ext(&item, n!("usesVariantGroups"));
            (uses && st(cx, me).variant_controls[k].newVariantVisible)
                || (!uses && legacyShown(cx))
        });
        // L460-462
        cx.base_mut(drop).enabled = Prop::f(move |cx, _| {
            let Some(item) = display_item(cx, me) else {
                return true;
            };
            !ext(&item, n!("usesVariantGroups")) || st(cx, me).variant_controls[k].newVariantEnabled
        });
        prev = drop;
    }
}

/// L465-514: the socket section, the six socket dropdowns, the five link checkboxes and the add
/// socket button.
/// @pob Classes/ItemsTab.lua:465-515 h=76ecb304
fn ctor_sockets(cx: &mut Cx, me: CtrlId) {
    // Section: Sockets and Links
    // L466-468
    let variant = ctl(cx, me, "displayItemSectionVariant");
    let section = cx
        .add(
            me,
            "displayItemSectionSockets",
            PlainControl::new(
                Anchor::new(TopLeft, variant, BottomLeft),
                rect(
                    0,
                    0,
                    0,
                    Prop::f(move |cx, _| {
                        // L467
                        match display_item(cx, me) {
                            Some(item) if exn(&item, n!("selectableSocketCount")) > 0.0 => 28.0,
                            _ => 0.0,
                        }
                    }),
                ),
            ),
        )
        .id();
    // L469
    for i in 1..=6usize {
        // L470-474
        let drop = cx.add_free(
            Some(me),
            DropDownControl::new(
                Anchor::new(TopLeft, section, TopLeft),
                rect((i as f64 - 1.0) * 64.0, 0, 36, 20),
                socketDropList(cx),
                sel_func(move |cx, _index, value| {
                    let item = di(cx, me);
                    // L471 self.displayItem.sockets[i].color = value.color
                    let color = value
                        .and_then(|v| v.get::<SocketColorValue>())
                        .map(|v| v.0)
                        .unwrap_or_else(|| lua_error!("attempt to index local 'value' (a nil value)"));
                    socket(&item, i)
                        .unwrap_or_else(|| lua_error!("attempt to index field '?' (a nil value)"))
                        .set(n!("color"), color);
                    // L472
                    item.BuildAndParseRaw();
                    // L473
                    update_display_item_tooltip(cx, me);
                }),
                None,
                false,
            ),
        );
        let drop = drop.id();
        // L475-477
        cx.base_mut(drop).shown = Prop::f(move |cx, _| {
            let item = di(cx, me);
            exn(&item, n!("selectableSocketCount")) >= i as f64
                && socket(&item, i).is_some_and(|s| s.str(n!("color")).as_deref() != Some("A"))
        });
        // L478
        cx.put(me, format!("displayItemSocket{i}"), drop);
        // L479
        if i < 6 {
            // L480-492
            let link = cx
                .add_free(
                    Some(me),
                    CheckBoxControl::new(
                        Anchor::new(Left, drop, Right),
                        rect(4, 0, 20, 0),
                        None::<String>,
                        Some(Rc::new(move |cx, state| {
                            let item = di(cx, me);
                            let s = |k: usize| {
                                socket(&item, k).unwrap_or_else(|| {
                                    lua_error!("attempt to index field '?' (a nil value)")
                                })
                            };
                            let n_sockets = sockets_len(&item);
                            // L481
                            if state && !s(i).get(n!("group")).raw_eq(&s(i + 1).get(n!("group"))) {
                                // L482
                                for k in i + 1..=n_sockets {
                                    // L483
                                    let g = s(k).get(n!("group")).num();
                                    s(k).set(n!("group"), g - 1.0);
                                }
                            // L485
                            } else if !state
                                && s(i).get(n!("group")).raw_eq(&s(i + 1).get(n!("group")))
                            {
                                // L486
                                for k in i + 1..=n_sockets {
                                    // L487
                                    let g = s(k).get(n!("group")).num();
                                    s(k).set(n!("group"), g + 1.0);
                                }
                            }
                            // L490
                            item.BuildAndParseRaw();
                            // L491
                            update_display_item_tooltip(cx, me);
                        })),
                        None,
                        false,
                    ),
                )
                .id();
            // L493-495
            cx.base_mut(link).shown = Prop::f(move |cx, _| {
                let item = di(cx, me);
                exn(&item, n!("selectableSocketCount")) > i as f64
                    && socket(&item, i + 1)
                        .is_some_and(|s| s.str(n!("color")).as_deref() != Some("A"))
            });
            // L496
            cx.put(me, format!("displayItemLink{i}"), link);
        }
    }
    // L499-511
    let add_socket = cx
        .add(
            me,
            "displayItemAddSocket",
            ButtonControl::new(
                Anchor::new(TopLeft, section, TopLeft),
                rect(
                    Prop::f(move |cx, _| {
                        let item = di(cx, me);
                        (sockets_len(&item) as f64 - item.abyssalSocketCount.get()) * 64.0 - 12.0
                    }),
                    0,
                    20,
                    20,
                ),
                "+",
                move |cx| {
                    let item = di(cx, me);
                    // L500
                    let insertIndex =
                        sockets_len(&item) as f64 - item.abyssalSocketCount.get() + 1.0;
                    // L501-504
                    let prev_group = socket(&item, (insertIndex - 1.0) as usize)
                        .filter(|_| insertIndex >= 2.0 && insertIndex.fract() == 0.0)
                        .unwrap_or_else(|| lua_error!("attempt to index field '?' (a nil value)"))
                        .get(n!("group"))
                        .num();
                    let new_socket = Table::new();
                    new_socket.set(n!("color"), ex(&item, n!("defaultSocketColor")));
                    new_socket.set(n!("group"), prev_group + 1.0);
                    item.sockets.get().with_mut(|s| {
                        let at = (insertIndex as usize - 1).min(s.len());
                        s.insert(at, new_socket);
                    });
                    // L505
                    let n_sockets = sockets_len(&item);
                    let mut k = insertIndex as usize + 1;
                    while k <= n_sockets {
                        // L506
                        let sk = socket(&item, k).expect("in range");
                        let g = sk.get(n!("group")).num();
                        sk.set(n!("group"), g + 1.0);
                        k += 1;
                    }
                    // L508
                    item.BuildAndParseRaw();
                    let h = items_ui_hooks();
                    // L509
                    (h.update_socket_controls)(cx, me);
                    // L510
                    (h.update_display_item_tooltip)(cx, me);
                },
            ),
        )
        .id();
    // L512-514
    cx.base_mut(add_socket).shown = Prop::f(move |cx, _| {
        let item = di(cx, me);
        (sockets_len(&item) as f64)
            < exn(&item, n!("selectableSocketCount")) + item.abyssalSocketCount.get()
    });
}

/// `#self.displayItem.enchantModLines`
fn enchant_lines(item: &model::ItemRef) -> usize {
    item.enchantModLines.get().len()
}

/// L516-579: the enchant, anoint, corrupt and add-implicit buttons.
/// @pob Classes/ItemsTab.lua:516-580 h=fea8812a
fn ctor_enchant(cx: &mut Cx, me: CtrlId) {
    // Section: Enchant / Anoint / Corrupt
    // L517-519
    let sockets = ctl(cx, me, "displayItemSectionSockets");
    let section = cx
        .add(
            me,
            "displayItemSectionEnchant",
            PlainControl::new(
                Anchor::new(TopLeft, sockets, BottomLeft),
                rect(
                    0,
                    0,
                    0,
                    Prop::f(move |cx, _| {
                        // L518
                        let shown = |k: &'static str| cx.is_shown(ctl(cx, me, k));
                        if shown("displayItemEnchant")
                            || shown("displayItemEnchant2")
                            || shown("displayItemAnoint")
                            || shown("displayItemAnoint2")
                            || shown("displayItemCorrupt")
                        {
                            28.0
                        } else {
                            0.0
                        }
                    }),
                ),
            ),
        )
        .id();
    let button = |cx: &mut Cx,
                  key: &'static str,
                  anchor: Anchor,
                  r: [Prop<f64>; 4],
                  label: &'static str,
                  click: Rc<dyn Fn(&mut Cx)>,
                  shown: Prop<bool>| {
        let id = cx
            .add(me, key, ButtonControl::new(anchor, r, label, move |cx| click(cx)))
            .id();
        cx.base_mut(id).shown = shown;
        id
    };
    let enchant = |slot: f64| -> Rc<dyn Fn(&mut Cx)> {
        Rc::new(move |cx| {
            let f = items_ui_hooks().enchant_display_item;
            f(cx, me, slot);
        })
    };
    let anoint = |slot: f64| -> Rc<dyn Fn(&mut Cx)> {
        Rc::new(move |cx| {
            let f = items_ui_hooks().anoint_display_item;
            f(cx, me, slot);
        })
    };
    // L520-525
    let enchant1 = button(
        cx,
        "displayItemEnchant",
        Anchor::new(TopLeft, section, TopLeft),
        rect(0, 0, 160, 20),
        "Change Enchantment...",
        // L521
        enchant(1.0),
        // L523-525
        Prop::f(move |cx, _| {
            display_item(cx, me).is_some_and(|item| item.enchantments_ref().is_some())
        }),
    );
    // L526-531
    let enchant2 = button(
        cx,
        "displayItemEnchant2",
        Anchor::new(TopLeft, enchant1, TopRight).collapse(),
        rect(8, 0, 160, 20),
        "Change Enchantment 2...",
        // L527
        enchant(2.0),
        // L529-531
        Prop::f(move |cx, _| {
            display_item(cx, me).is_some_and(|item| {
                item.enchantments_ref().is_some()
                    && ext(&item, n!("canHaveTwoEnchants"))
                    && enchant_lines(&item) > 0
            })
        }),
    );
    // L532-537
    let anoint1 = button(
        cx,
        "displayItemAnoint",
        Anchor::new(TopLeft, enchant2, TopRight).collapse(),
        rect(8, 0, 100, 20),
        "Anoint...",
        // L533
        anoint(1.0),
        // L535-537
        Prop::f(move |cx, _| {
            display_item(cx, me).is_some_and(|item| isAnointable(Some(&item)))
        }),
    );
    // L538-546
    let anoint2 = button(
        cx,
        "displayItemAnoint2",
        Anchor::new(TopLeft, anoint1, TopRight).collapse(),
        rect(8, 0, 100, 20),
        "Anoint 2...",
        // L539
        anoint(2.0),
        // L541-546
        Prop::f(move |cx, _| {
            display_item(cx, me).is_some_and(|item| {
                isAnointable(Some(&item))
                    && ext(&item, n!("canHaveTwoEnchants"))
                    && enchant_lines(&item) > 0
            })
        }),
    );
    // L547-555
    let anoint3 = button(
        cx,
        "displayItemAnoint3",
        Anchor::new(TopLeft, anoint2, TopRight).collapse(),
        rect(8, 0, 100, 20),
        "Anoint 3...",
        // L548
        anoint(3.0),
        // L550-555
        Prop::f(move |cx, _| {
            display_item(cx, me).is_some_and(|item| {
                isAnointable(Some(&item))
                    && ext(&item, n!("canHaveThreeEnchants"))
                    && enchant_lines(&item) > 1
            })
        }),
    );
    // L556-564
    let anoint4 = button(
        cx,
        "displayItemAnoint4",
        Anchor::new(TopLeft, anoint3, TopRight).collapse(),
        rect(8, 0, 100, 20),
        "Anoint 4...",
        // L557
        anoint(4.0),
        // L559-564
        Prop::f(move |cx, _| {
            display_item(cx, me).is_some_and(|item| {
                isAnointable(Some(&item))
                    && ext(&item, n!("canHaveFourEnchants"))
                    && enchant_lines(&item) > 2
            })
        }),
    );
    // L565-570
    let corrupt = button(
        cx,
        "displayItemCorrupt",
        Anchor::new(TopLeft, anoint4, TopRight).collapse(),
        rect(8, 10, 100, 20),
        "Corrupt...",
        // L566
        Rc::new(move |cx| {
            let f = items_ui_hooks().corrupt_display_item;
            f(cx, me);
        }),
        // L568-570
        Prop::f(move |cx, _| display_item(cx, me).is_some_and(|item| ext(&item, n!("corruptible")))),
    );
    // L571-579
    button(
        cx,
        "displayItemAddImplicit",
        Anchor::new(TopLeft, corrupt, TopRight).collapse(),
        rect(8, 0, 120, 20),
        "Add Implicit...",
        // L572
        Rc::new(move |cx| {
            let item = display_item(cx, me);
            let f = items_ui_hooks().add_implicit_to_display_item;
            f(cx, me, item);
        }),
        // L574-579
        Prop::f(move |cx, _| {
            display_item(cx, me).is_some_and(|item| {
                let ty = item.r#type.get();
                let rarity = item.rarity.get();
                &*ty != "Tincture"
                    && &*ty != "Graft"
                    && (ext(&item, n!("corruptible"))
                        || ((&*ty != "Flask" && &*ty != "Jewel")
                            && (&*rarity == "NORMAL" || &*rarity == "MAGIC" || &*rarity == "RARE")))
                    && !ext(&item, n!("implicitsCannotBeChanged"))
            })
        }),
    );
}

/// `setDisplayItemInfluence(influenceIndexList)` (L586-609).
fn setDisplayItemInfluence(cx: &mut Cx, me: CtrlId, influenceIndexList: [f64; 2]) {
    let item = di(cx, me);
    // L587
    item.ResetInfluence();
    // L588
    if ext(&item, n!("HasElderShaperAndAllConquerorInfluences")) {
        // L589
        for (i, _curInfluenceInfo) in influenceInfo_default.iter().enumerate() {
            // L590
            item.influences.set(Name::from(influenceInfo_all[i].key), true);
        }
    } else {
        // L593
        for index in influenceIndexList {
            // L594
            if index > 0.0 {
                // L595 `influenceInfo[index].key` (an index past the list raises)
                let info = influenceInfo_all
                    .get(index as usize - 1)
                    .filter(|_| index.fract() == 0.0)
                    .unwrap_or_else(|| lua_error!("attempt to index field '?' (a nil value)"));
                item.influences.set(Name::from(info.key), true);
            }
        }
    }

    // L600
    if ext(&item, n!("crafted")) {
        // L601-605
        force_affix_selectors(cx, me);
    }
    // L607
    item.BuildAndParseRaw();
    // L608
    update_display_item_tooltip(cx, me);
}

/// L581-629: the influence section and dropdowns.
/// @pob Classes/ItemsTab.lua:581-630 h=be1d353b
fn ctor_influence(cx: &mut Cx, me: CtrlId) {
    // Section: Influence dropdowns
    // L582
    let mut influenceDisplayList: Vec<&str> = vec!["Influence"];
    // L583
    for curInfluenceInfo in &influenceInfo_all {
        // L584
        influenceDisplayList.push(curInfluenceInfo.display);
    }
    // L586-609 setDisplayItemInfluence: the fn above
    // L610-612
    let enchant = ctl(cx, me, "displayItemSectionEnchant");
    let section = cx
        .add(
            me,
            "displayItemSectionInfluence",
            PlainControl::new(
                Anchor::new(TopLeft, enchant, BottomLeft),
                rect(
                    0,
                    0,
                    0,
                    Prop::f(move |cx, _| {
                        // L611
                        if display_item(cx, me).is_some_and(|i| ext(&i, n!("canBeInfluenced"))) {
                            28.0
                        } else {
                            0.0
                        }
                    }),
                ),
            ),
        )
        .id();
    // L613
    let influenceTipText = wrap_string(
        cx,
        "Selecting an influence here will also allow the modifier dropdowns to contain influenced mods.",
        16.0,
        140.0,
    )
    .join("\n");
    let shown = move |cx: &Cx, _: CtrlId| {
        display_item(cx, me).is_some_and(|i| ext(&i, n!("canBeInfluenced")))
    };
    // L614-617
    let influence1 = cx
        .add(
            me,
            "displayItemInfluence",
            DropDownControl::new(
                Anchor::new(TopLeft, section, TopRight),
                rect(0, 0, 100, 20),
                str_list(&influenceDisplayList),
                sel_func(move |cx, index, _value| {
                    // L615
                    let other = ctl(cx, me, "displayItemInfluence2");
                    let otherIndex = cx.w::<DropDownControl>(other).sel_index;
                    // L616
                    setDisplayItemInfluence(
                        cx,
                        me,
                        [index as f64 - 1.0, otherIndex as f64 - 1.0],
                    );
                }),
                None,
                false,
            ),
        )
        .id();
    // L618
    cx.w_mut::<DropDownControl>(influence1).tt.tooltip_text =
        Some(Prop::Val(Some(influenceTipText.clone())));
    // L619-621
    cx.base_mut(influence1).shown = Prop::f(shown);
    // L622-625
    let influence2 = cx
        .add(
            me,
            "displayItemInfluence2",
            DropDownControl::new(
                Anchor::new(TopLeft, influence1, TopRight).collapse(),
                rect(8, 0, 100, 20),
                str_list(&influenceDisplayList),
                sel_func(move |cx, index, _value| {
                    // L623
                    let other = ctl(cx, me, "displayItemInfluence");
                    let otherIndex = cx.w::<DropDownControl>(other).sel_index;
                    // L624
                    setDisplayItemInfluence(
                        cx,
                        me,
                        [index as f64 - 1.0, otherIndex as f64 - 1.0],
                    );
                }),
                None,
                false,
            ),
        )
        .id();
    // L626-628
    cx.base_mut(influence2).shown = Prop::f(shown);
    // L629
    cx.w_mut::<DropDownControl>(influence2).tt.tooltip_text =
        Some(Prop::Val(Some(influenceTipText)));
}

/// The quality controls' `shown` (L637, L646): the `or` chain is the Lua's, which is true for
/// every base type (no type equals all five), but it still indexes `base`.
fn quality_shown(cx: &Cx, me: CtrlId) -> bool {
    let Some(item) = display_item(cx, me) else {
        return false;
    };
    if item.quality.get().is_none() {
        return false;
    }
    let bt = base_type(&item);
    #[allow(clippy::nonminimal_bool)]
    let r = &*bt != "Amulet"
        || &*bt != "Belt"
        || &*bt != "Jewel"
        || &*bt != "Quiver"
        || &*bt != "Ring"
        || &*item.r#type.get() != "Graft";
    r
}

/// L631-697: the quality and catalyst sections.
/// @pob Classes/ItemsTab.lua:631-698 h=2919ed90
fn ctor_quality_catalyst(cx: &mut Cx, me: CtrlId) {
    // Section: Item Quality
    // L632-634
    let influence = ctl(cx, me, "displayItemSectionInfluence");
    let section = cx
        .add(
            me,
            "displayItemSectionQuality",
            PlainControl::new(
                Anchor::new(TopLeft, influence, BottomLeft),
                rect(
                    0,
                    0,
                    0,
                    Prop::f(move |cx, _| {
                        // L633
                        if cx.is_shown(ctl(cx, me, "displayItemQuality"))
                            && cx.is_shown(ctl(cx, me, "displayItemQualityEdit"))
                        {
                            28.0
                        } else {
                            0.0
                        }
                    }),
                ),
            ),
        )
        .id();
    // L635
    let quality = cx
        .add(
            me,
            "displayItemQuality",
            LabelControl::new(
                Anchor::new(TopLeft, section, TopRight),
                rect(0, 0, 0, 16),
                "^7Quality:",
            ),
        )
        .id();
    // L636-638
    cx.base_mut(quality).shown = Prop::f(move |cx, _| quality_shown(cx, me));

    // L640-644
    let quality_edit = cx
        .add(
            me,
            "displayItemQualityEdit",
            EditControl::new(
                Anchor::new(Left, quality, Right),
                rect(2, 0, 60, 20),
                EditArgs {
                    filter: Some("%D".into()),
                    limit: Some(2),
                    change_func: Some(Rc::new(move |cx, buf| {
                        let item = di(cx, me);
                        // L641
                        item.quality.set(tonumber(buf));
                        // L642
                        item.BuildAndParseRaw();
                        // L643
                        update_display_item_tooltip(cx, me);
                    })),
                    ..Default::default()
                },
            ),
        )
        .id();
    // L645-647
    cx.base_mut(quality_edit).shown = Prop::f(move |cx, _| quality_shown(cx, me));

    // L649-656
    let mut sortingOptions = vec![DropItem::new("Default", Rc::new(PowerSortValue(None)))];
    // L652
    for option in &model::data().powerStatList.stats {
        // L653
        if !option.ignoreForItems && &*option.label != "Name" {
            // L654
            sortingOptions.push(DropItem::new(
                option.label.to_string(),
                Rc::new(PowerSortValue(Some(option))),
            ));
        }
    }
    SORTING_OPTIONS.with(|s| *s.borrow_mut() = Some(sortingOptions));
    // Section: Catalysts
    // L658-660
    let section_catalyst = cx
        .add(
            me,
            "displayItemSectionCatalyst",
            PlainControl::new(
                Anchor::new(TopLeft, section, BottomLeft),
                rect(
                    0,
                    0,
                    0,
                    Prop::f(move |cx, _| {
                        // L659
                        if cx.is_shown(ctl(cx, me, "displayItemCatalyst"))
                            || cx.is_shown(ctl(cx, me, "displayItemCatalystQualityEdit"))
                        {
                            28.0
                        } else {
                            0.0
                        }
                    }),
                ),
            ),
        )
        .id();
    // L661-679
    let catalyst = cx
        .add(
            me,
            "displayItemCatalyst",
            DropDownControl::new(
                Anchor::new(TopLeft, section_catalyst, TopRight),
                rect(0, 0, 250, 20),
                // L662-663
                str_list(&[
                    "Catalyst",
                    "Abrasive (Attack)",
                    "Accelerating (Speed)",
                    "Dextral (Suffix)",
                    "Fertile (Life & Mana)",
                    "Imbued (Caster)",
                    "Intrinsic (Attribute)",
                    "Noxious (Physical & Chaos Damage)",
                    "Prismatic (Resistance)",
                    "Sinistral (Prefix)",
                    "Tempering (Defense)",
                    "Turbulent (Elemental)",
                    "Unstable (Critical)",
                ]),
                sel_func(move |cx, index, _value| {
                    let item = di(cx, me);
                    // L665
                    item.catalyst.set(Some(index as f64 - 1.0));
                    // L666
                    if item.catalystQuality.get().is_none() {
                        // L667
                        item.catalystQuality.set(Some(20.0));
                        // L668
                        let edit = ctl(cx, me, "displayItemCatalystQualityEdit");
                        EditControl::set_text(cx, edit, &tostring(20.0), false);
                    }
                    // L670
                    if ext(&item, n!("crafted")) {
                        // L671-675
                        force_affix_selectors(cx, me);
                    }
                    // L677
                    item.BuildAndParseRaw();
                    // L678
                    update_display_item_tooltip(cx, me);
                }),
                None,
                false,
            ),
        )
        .id();
    // L680-682
    cx.base_mut(catalyst).shown = Prop::f(move |cx, _| {
        display_item(cx, me).is_some_and(|item| {
            (ext(&item, n!("crafted")) || ext(&item, n!("hasModTags"))) && {
                let bt = base_type(&item);
                &*bt == "Amulet" || &*bt == "Ring" || &*bt == "Belt"
            }
        })
    });
    // L683-694
    let catalyst_edit = cx
        .add(
            me,
            "displayItemCatalystQualityEdit",
            EditControl::new(
                Anchor::new(Left, catalyst, Right),
                rect(2, 0, 60, 20),
                EditArgs {
                    filter: Some("%D".into()),
                    limit: Some(2),
                    change_func: Some(Rc::new(move |cx, buf| {
                        let item = di(cx, me);
                        // L684
                        item.catalystQuality.set(tonumber(buf));
                        // L685
                        if ext(&item, n!("crafted")) {
                            // L686-690
                            force_affix_selectors(cx, me);
                        }
                        // L692
                        item.BuildAndParseRaw();
                        // L693
                        update_display_item_tooltip(cx, me);
                    })),
                    ..Default::default()
                },
            ),
        )
        .id();
    // L695-697
    cx.base_mut(catalyst_edit).shown = Prop::f(move |cx, _| {
        display_item(cx, me).is_some_and(|item| {
            (ext(&item, n!("crafted")) || ext(&item, n!("hasModTags")))
                && item.catalyst.get().is_some_and(|c| c > 0.0)
        })
    });
}

thread_local! {
    /// `sortingOptions` (L649-656) between the quality section, which builds it, and the sorting
    /// dropdown (L726), which takes it.
    static SORTING_OPTIONS: RefCell<Option<Vec<DropItem>>> = const { RefCell::new(None) };
}

/// L699-728: the cluster jewel section and the crafting sort selector.
/// @pob Classes/ItemsTab.lua:699-729 h=514a826b
fn ctor_cluster(cx: &mut Cx, me: CtrlId) {
    // Section: Cluster Jewel
    // L700-702
    let catalyst = ctl(cx, me, "displayItemSectionCatalyst");
    let section = cx
        .add(
            me,
            "displayItemSectionClusterJewel",
            PlainControl::new(
                Anchor::new(TopLeft, catalyst, BottomLeft),
                rect(
                    0,
                    0,
                    0,
                    Prop::f(move |cx, _| {
                        // L701
                        if cx.is_shown(ctl(cx, me, "displayItemClusterJewelSkill")) {
                            52.0
                        } else {
                            0.0
                        }
                    }),
                ),
            ),
        )
        .id();
    // L703-710
    let skill = cx
        .add(
            me,
            "displayItemClusterJewelSkill",
            DropDownControl::new(
                Anchor::new(TopLeft, section, TopLeft),
                rect(0, 0, 300, 20),
                Vec::new(),
                sel_func(move |cx, _index, value| {
                    // L704
                    let skill_id = value
                        .and_then(|v| v.get::<ClusterSkillValue>())
                        .map(|v| v.skillId);
                    di(cx, me).clusterJewelSkill.set(skill_id);
                    // L705
                    let f = items_ui_hooks().craft_cluster_jewel;
                    f(cx, me);
                }),
                None,
                false,
            )
            // L706-710 (the mixin)
            .shown(Prop::f(move |cx, _| {
                display_item(cx, me).is_some_and(|item| {
                    ext(&item, n!("crafted")) && item.clusterJewel.get().is_some()
                })
            })),
        )
        .id();

    // L712
    let count_label = cx
        .add(
            me,
            "displayItemClusterJewelNodeCountLabel",
            LabelControl::new(
                Anchor::new(TopLeft, skill, BottomLeft),
                rect(0, 7, 0, 14),
                "^7Added Passives:",
            ),
        )
        .id();
    // L713-718
    cx.add(
        me,
        "displayItemClusterJewelNodeCount",
        SliderControl::new(
            Anchor::new(Left, count_label, Right),
            rect(2, 0, 150, 20),
            Some(Rc::new(move |cx, val| {
                // L714 (unused)
                let count = ctl(cx, me, "displayItemClusterJewelNodeCount");
                let _divVal = SliderControl::get_div_val(cx, count, None);
                let item = di(cx, me);
                // L715
                let clusterJewel = item.clusterJewel.get().unwrap_or_else(|| {
                    lua_error!("attempt to index local 'clusterJewel' (a nil value)")
                });
                // L716
                let (max, min) = (
                    clusterJewel.num(n!("maxNodes")),
                    clusterJewel.num(n!("minNodes")),
                );
                item.clusterJewelNodeCount
                    .set(Some(round(val * (max - min) + min, None)));
                // L717
                let f = items_ui_hooks().craft_cluster_jewel;
                f(cx, me);
            })),
            None,
        ),
    );

    // L720
    let sorting_label = cx
        .add(
            me,
            "craftingSortingLabel",
            LabelControl::new(
                Anchor::new(TopLeft, section, BottomLeft),
                rect(0, 0, 0, 16),
                "^7Modifier sorting:",
            ),
        )
        .id();
    // L721-725
    cx.base_mut(sorting_label).shown = Prop::f(move |cx, _| {
        display_item(cx, me).is_some_and(|item| {
            ext(&item, n!("crafted"))
                // cluster jewels don't have good comparison support and sorting would be misleading
                && !{
                    let base = item
                        .base
                        .get()
                        .unwrap_or_else(|| lua_error!("attempt to index field 'base' (a nil value)"));
                    &*base.r#type == "Jewel" && base.subType.as_deref() == Some("Cluster")
                }
        })
    });
    // L726-728
    let sortingOptions = SORTING_OPTIONS
        .with(|s| s.borrow_mut().take())
        .unwrap_or_default();
    cx.add(
        me,
        "craftingSorting",
        DropDownControl::new(
            Anchor::new(Left, sorting_label, Right),
            rect(4, 0, 200, 20),
            sortingOptions,
            sel_func(move |cx, _index, _value| {
                // L727
                let f = items_ui_hooks().update_affix_controls;
                f(cx, me);
            }),
            None,
            false,
        ),
    );
}

/// `getMinMax(mod)` (L763-768): the first `(min-max)` range of the mod's lines.
fn getMinMax(r#mod: &gamedata::schema::affixes::ItemMod) -> Option<(Option<f64>, Option<f64>)> {
    // L764
    for line in &r#mod.lines {
        // L765
        if let Some(c) = pattern::r#match(line, "%((%d[%d%.]*)%-(%d[%d%.]*)%)", None) {
            // L766
            return Some((c[0].num(), c[1].num()));
        }
    }
    None
}

/// `flipRange(modA, modB)` (L762-786).
fn flipRange(
    modA: Option<&gamedata::schema::affixes::ItemModRef>,
    modB: Option<&gamedata::schema::affixes::ItemModRef>,
) -> bool {
    let mm = |m: Option<&gamedata::schema::affixes::ItemModRef>| {
        let m = m.unwrap_or_else(|| lua_error!("bad argument #1 to 'ipairs' (table expected, got nil)"));
        getMinMax(m).unwrap_or((None, None))
    };
    // L770
    let (minA, maxA) = mm(modA);
    // L771
    let (minB, maxB) = mm(modB);

    // L773
    let (Some(minA), Some(minB), Some(maxA), Some(maxB)) = (minA, minB, maxA, maxB) else {
        // L774
        return false;
    };

    // L777 if the mod goes in steps that aren't 1, then the code below this doesn't work
    let allInts = minA == m_floor(minA)
        && maxA == m_floor(maxA)
        && minB == m_floor(minB)
        && maxB == m_floor(maxB);
    // L778
    if allInts {
        // L779 ascending
        if minA < minB {
            // L780
            return minA + 1.0 == maxB;
        }
        // L782 descending
        return minA - 1.0 == maxB;
    }
    // L785
    false
}

/// `verifyRange(range, index, drop)` (L759-798): flips range if it will form discontinuous
/// values.
fn verifyRange(cx: &Cx, me: CtrlId, range: f64, index: f64, drop: CtrlId) -> f64 {
    let item = di(cx, me);
    let series = || {
        sel_series(cx, drop).unwrap_or_else(|| lua_error!("attempt to index field 'modList' (a nil value)"))
    };
    let at = |s: &AffixSeries, k: f64| -> Option<Name> {
        (k >= 1.0 && k.fract() == 0.0)
            .then(|| s.modList.get(k as usize - 1).copied())
            .flatten()
    };
    let mut range = range;
    // L760
    let priorMod = if index - 1.0 > 0.0 {
        affix_mod(&item, at(&series(), index - 1.0))
    } else {
        None
    };
    // L761
    let nextMod = if index + 1.0 < series().modList.len() as f64 {
        affix_mod(&item, at(&series(), index + 1.0))
    } else {
        None
    };
    // L762-786 flipRange: the fn above

    // L788
    if priorMod.is_some() {
        // L789
        if flipRange(priorMod.as_ref(), affix_mod(&item, at(&series(), index)).as_ref()) {
            // L790
            range = 1.0 - range;
        }
    // L792
    } else if nextMod.is_some() {
        // L793
        if flipRange(affix_mod(&item, at(&series(), index)).as_ref(), nextMod.as_ref()) {
            // L794
            range = 1.0 - range;
        }
    }
    // L797
    range
}

/// `modList[k]` (1-based; nil outside).
fn mod_at(list: &[Name], k: f64) -> Option<Name> {
    (k >= 1.0 && k.fract() == 0.0)
        .then(|| list.get(k as usize - 1).copied())
        .flatten()
}

/// `table.concat(mod.modTags, ', ')`
fn concat_tags(tags: &[Name]) -> String {
    tags.iter().map(|t| t.to_string()).collect::<Vec<_>>().join(", ")
}

/// L730-755: the affix section.
/// @pob Classes/ItemsTab.lua:730-755 h=65738fda
fn ctor_affixes(cx: &mut Cx, me: CtrlId) {
    // Section: Affix Selection
    // L731 maxModCount (the const)
    // L732-752
    let sorting_label = ctl(cx, me, "craftingSortingLabel");
    let section = cx
        .add(
            me,
            "displayItemSectionAffix",
            PlainControl::new(
                Anchor::new(TopLeft, sorting_label, BottomLeft).collapse(),
                rect(
                    0,
                    Prop::f(move |cx, _| {
                        // L733 `self.controls.craftingSortingLabel.shown()`
                        let label = ctl(cx, me, "craftingSortingLabel");
                        if cx.base(label).shown.get(cx, label) {
                            // L734
                            8.0
                        } else {
                            // L736
                            -16.0
                        }
                    }),
                    0,
                    Prop::f(move |cx, _| {
                        // L739
                        if !display_item(cx, me).is_some_and(|i| ext(&i, n!("crafted"))) {
                            // L740
                            return 0.0;
                        }
                        // L742
                        let mut h = 6.0;
                        // L743
                        for i in 1..=maxModCount {
                            // L744
                            if cx.is_shown(ctl(cx, me, format!("displayItemAffix{i}"))) {
                                // L745
                                h += 24.0;
                                // L746
                                if cx.is_shown(ctl(cx, me, format!("displayItemAffixRange{i}"))) {
                                    // L747
                                    h += 18.0;
                                }
                            }
                        }
                        // L751
                        h
                    }),
                ),
            ),
        )
        .id();
    // L753-755
    cx.base_mut(section).shown =
        Prop::f(move |cx, _| display_item(cx, me).is_some_and(|i| ext(&i, n!("crafted"))));
    // L756
    for i in 1..=maxModCount {
        ctor_affix(cx, me, i, section);
    }
}

/// One affix row (L757-983, the body of `for i = 1, maxModCount`).
/// @pob Classes/ItemsTab.lua:756-985 h=e7644cf2
fn ctor_affix(cx: &mut Cx, me: CtrlId, i: usize, section: CtrlId) {
    // L757
    let prev = if i == 1 {
        section
    } else {
        ctl(cx, me, format!("displayItemAffix{}", i - 1))
    };
    // L758 local drop, slider (`drop` is created first; both are captured by the closures,
    // which are set once both exist)
    // L759-798 verifyRange: the fn above
    // L799-814
    let drop = cx
        .add_free(
            Some(me),
            DropDownControl::new(
                Anchor::new(TopLeft, prev, TopLeft),
                rect(if i == 1 { 40 } else { 0 }, 0, 418, 20),
                Vec::new(),
                None,
                None,
                false,
            ),
        )
        .id();
    // L815-817
    cx.base_mut(drop).y = Prop::f(move |cx, _| {
        if i == 1 {
            0.0
        } else {
            // `prev.slider`
            let prev_slider = affix_slider(cx, me, i - 1);
            24.0 + if cx.is_shown(prev_slider) { 18.0 } else { 0.0 }
        }
    });
    // L818-930
    cx.w_mut::<DropDownControl>(drop).tt.tooltip_func =
        Some(Rc::new(move |cx, tooltip, (mode, _index, value)| {
            affix_tooltip(cx, me, drop, tooltip, mode, value.as_ref());
        }));
    // L931-933
    cx.base_mut(drop).shown = Prop::f(move |cx, _| {
        display_item(cx, me).is_some_and(|item| {
            ext(&item, n!("crafted")) && i as f64 <= exn(&item, n!("affixLimit"))
        })
    });
    // L934-942
    let slider = cx
        .add_free(
            Some(me),
            SliderControl::new(
                Anchor::new(TopLeft, drop, BottomLeft),
                rect(0, 2, 300, 16),
                Some(Rc::new(move |cx, _val| {
                    let slider = affix_slider(cx, me, i);
                    // L935
                    let (tbl, idx) = output_affix(cx, me, i);
                    let affix = tbl.get(idx).table().unwrap_or_else(|| {
                        lua_error!("attempt to index local 'affix' (a nil value)")
                    });
                    // L936
                    let (index, range) = SliderControl::get_div_val(cx, slider, None);
                    // L937
                    let series = sel_series(cx, drop).unwrap_or_else(|| {
                        lua_error!("attempt to index field 'modList' (a nil value)")
                    });
                    affix.set(
                        n!("modId"),
                        mod_at(&series.modList, index).map_or(Val::Nil, Val::from),
                    );

                    // L939
                    let r = verifyRange(cx, me, range, index, drop);
                    affix.set(n!("range"), r);
                    // L940
                    di(cx, me).Craft();
                    // L941
                    update_display_item_tooltip(cx, me);
                })),
                None,
            ),
        )
        .id();
    // L799-814 the dropdown's selFunc (needs `slider`)
    cx.w_mut::<DropDownControl>(drop).sel_func = sel_func(move |cx, _index, value| {
        let item = di(cx, me);
        let (tbl, idx) = output_affix(cx, me, i);
        // L800
        let cur = tbl.get(idx).table().unwrap_or_else(|| {
            lua_error!("attempt to index field '?' (a nil value)")
        });
        let affix = Table::new();
        affix.set(n!("modId"), "None");
        affix.set(n!("fractured"), cur.get(n!("fractured")));
        let series = value.and_then(series_of);
        if value.is_none() {
            lua_error!("attempt to index local 'value' (a nil value)");
        }
        // L801
        if let Some(mod_id) = series.as_ref().and_then(|s| s.modId) {
            // L802
            affix.set(n!("modId"), mod_id);
            // L803
            affix.set(n!("range"), cx.w::<SliderControl>(slider).val);
        // L804 (`value.modList`: a series always has one; the "None" string has none)
        } else if let Some(series) = series {
            // L805
            cx.w_mut::<SliderControl>(slider).div_count = Some(series.modList.len() as f64);
            // L806
            let (index, range) = SliderControl::get_div_val(cx, slider, None);
            // L807
            affix.set(
                n!("modId"),
                mod_at(&series.modList, index).map_or(Val::Nil, Val::from),
            );
            // L808
            let r = verifyRange(cx, me, range, index, drop);
            affix.set(n!("range"), r);
        }
        // L810
        tbl.set(idx, affix);
        // L811
        item.Craft();
        // L812
        update_display_item_tooltip(cx, me);
        // L813
        let f = items_ui_hooks().update_affix_controls;
        f(cx, me);
    });
    // L943-945
    cx.base_mut(slider).width = Prop::f(|cx, me| {
        if cx.w::<SliderControl>(me).div_count.is_some() {
            300.0
        } else {
            100.0
        }
    });
    // L946-971
    cx.w_mut::<SliderControl>(slider).tt.tooltip_func = Some(Rc::new(move |cx, tooltip, val| {
        // L947
        let sel = sel_item_or_err(cx, drop);
        let modList = series_of(&sel).map(|s| s.modList.clone());
        // L948
        let sel_control = cx.host(me).sel_control;
        let Some(modList) = modList.filter(|_| {
            cx.main.popups.is_empty() && !sel_control.is_some_and(|s| s != slider)
        }) else {
            // L949
            tooltip.clear(false);
            return;
        };
        // L950
        if tooltip.check_for_update(cx, &[TtKey::Num(val), TtKey::Obj(TtObj(sel.value.clone()))]) {
            let item = di(cx, me);
            // L951
            let (index, range) = SliderControl::get_div_val(cx, slider, Some(val));
            // L952
            let range = verifyRange(cx, me, range, index, drop);
            // L953
            let modId = mod_at(&modList, index);
            // L954
            let r#mod = affix_mod_or_err(&item, modId);
            // L955
            for line in &r#mod.lines {
                // L956
                let l = applyRange(line, &Val::Num(range), None, None);
                tooltip.add_line(cx, 16.0, &l);
            }
            // L958
            tooltip.add_separator(cx, Some(10.0));
            // L959
            let affix = r#mod.affix.as_deref().unwrap_or_else(|| {
                lua_error!("attempt to concatenate field 'affix' (a nil value)")
            });
            if modList.len() > 1 {
                // L960 `isValueInArray(modList, modId)`
                let pos = modList
                    .iter()
                    .position(|m| Some(*m) == modId)
                    .map(|p| p + 1)
                    .unwrap_or_else(|| {
                        lua_error!("attempt to perform arithmetic on a nil value")
                    });
                let l = format!(
                    "^7Affix: Tier {} ({affix})",
                    tostring(modList.len() as f64 - pos as f64 + 1.0)
                );
                tooltip.add_line(cx, 16.0, &l);
            } else {
                // L962
                tooltip.add_line(cx, 16.0, &format!("^7Affix: {affix}"));
            }
            // L964
            for line in &r#mod.lines {
                // L965
                tooltip.add_line(cx, 14.0, line);
            }
            // L967
            let level = r#mod.level.unwrap_or_else(|| {
                lua_error!("attempt to compare nil with number")
            });
            if level > 1.0 {
                // L968
                tooltip.add_line(cx, 16.0, &format!("Level: {}", tostring(level)));
            }
        }
    }));
    // L972 drop.slider = slider (`self.controls["displayItemAffixRange"..i]`, see affix_slider)
    // L973
    cx.put(me, format!("displayItemAffix{i}"), drop);
    // L974-979
    cx.add(
        me,
        format!("displayItemAffixLabel{i}"),
        LabelControl::new(
            Anchor::new(Right, drop, Left),
            rect(-4, 0, 0, 14),
            Prop::f(move |cx, _| {
                // L975
                let item = di(cx, me);
                let ignoreModType = item.rareLikeUnique_ref().is_some_and(|r| r.ignoreModType);
                // L976-978
                if ignoreModType {
                    "^7Explicit:"
                } else if st(cx, me).affix_controls[i - 1].outputTable == Some(n!("prefixes")) {
                    "^7Prefix:"
                } else {
                    "^7Suffix:"
                }
                .to_owned()
            }),
        ),
    );
    // L980
    cx.put(me, format!("displayItemAffixRange{i}"), slider);
    // L981-983
    cx.add(
        me,
        format!("displayItemAffixRangeLabel{i}"),
        LabelControl::new(
            Anchor::new(Right, slider, Left),
            rect(-4, 0, 0, 14),
            Prop::f(move |cx, _| {
                // L982
                if cx.w::<DropDownControl>(drop).sel_index > 1 {
                    "^7Roll:"
                } else {
                    "^x7F7F7FRoll:"
                }
                .to_owned()
            }),
        ),
    );
}

/// The affix dropdowns' tooltipFunc (L818-930).
fn affix_tooltip(
    cx: &mut Cx,
    me: CtrlId,
    drop: CtrlId,
    tooltip: &mut Tooltip,
    mode: DropTipMode,
    value: Option<&DropItem>,
) {
    // L819
    let value = value.unwrap_or_else(|| lua_error!("attempt to index local 'value' (a nil value)"));
    let series = series_of(value);
    let modList = series.as_ref().map(|s| s.modList.clone());
    // L820
    let sel_control = cx.host(me).sel_control;
    let Some(modList) = modList.filter(|_| {
        cx.main.popups.is_empty()
            && mode != DropTipMode::Out
            && !sel_control.is_some_and(|s| s != drop)
    }) else {
        // L821
        tooltip.clear(false);
        return;
    };
    // L822
    if !tooltip.check_for_update(cx, &[TtKey::Obj(TtObj(value.value.clone()))]) {
        return;
    }
    let item = di(cx, me);
    let modId = series.as_ref().and_then(|s| s.modId);
    // L823
    if modId.is_some() || modList.len() == 1 {
        // L824
        let r#mod = affix_mod_or_err(&item, modId.or_else(|| modList.first().copied()));
        // L825
        let affix = r#mod
            .affix
            .as_deref()
            .unwrap_or_else(|| lua_error!("attempt to concatenate field 'affix' (a nil value)"));
        tooltip.add_line(cx, 16.0, &format!("^7Affix: {affix}"));
        // L826
        for line in &r#mod.lines {
            // L827
            tooltip.add_line(cx, 14.0, &format!("^7{line}"));
        }
        // L829
        let level = r#mod
            .level
            .unwrap_or_else(|| lua_error!("attempt to compare nil with number"));
        if level > 1.0 {
            // L830
            tooltip.add_line(cx, 16.0, &format!("Level: {}", tostring(level)));
        }
        // L832
        if !r#mod.modTags.is_empty() {
            // L833
            tooltip.add_line(cx, 16.0, &format!("Tags: {}", concat_tags(&r#mod.modTags)));
        }
    } else {
        // L836
        tooltip.add_line(cx, 16.0, &format!("^7{} Tiers", modList.len()));
        // L837
        let minMod = affix_mod_or_err(&item, modList.first().copied());
        // L838
        let maxMod = affix_mod_or_err(&item, modList.last().copied());
        // L839
        for (l, line) in minMod.lines.iter().enumerate() {
            // L840
            let minLine = pattern::gsub(line, "%((%d[%d%.]*)%-(%d[%d%.]*)%)", "%1", None).0;
            // L841
            let max_l = maxMod
                .lines
                .get(l)
                .unwrap_or_else(|| lua_error!("attempt to index field '?' (a nil value)"));
            let maxLine = pattern::gsub(max_l, "%((%d[%d%.]*)%-(%d[%d%.]*)%)", "%2", None).0;
            // L842
            if maxLine == **max_l {
                // L843
                tooltip.add_line(cx, 14.0, &maxLine);
            } else {
                // L845
                let mut start: i64 = 1;
                // L846-854
                let text = pattern::gsub_fn(
                    &minLine,
                    "%d[%d%.]*",
                    |c| {
                        let min = c[0].to_str().into_owned();
                        // L847
                        let (_s, e, caps) = pattern::find(&maxLine, "(%d[%d%.]*)", Some(start), false)
                            .unwrap_or_else(|| {
                                lua_error!("attempt to perform arithmetic on local 'e' (a nil value)")
                            });
                        let max = caps[0].to_str().into_owned();
                        // L848
                        start = e as i64 + 1;
                        // L849
                        if min == max {
                            // L850
                            Some(min)
                        } else {
                            // L852
                            Some(format!("({min}-{max})"))
                        }
                    },
                    None,
                )
                .0;
                tooltip.add_line(cx, 14.0, &text);
            }
        }
        // L857
        let lvl = |m: &gamedata::schema::affixes::ItemMod| {
            tostring(
                m.level
                    .unwrap_or_else(|| lua_error!("attempt to concatenate field 'level' (a nil value)")),
            )
        };
        tooltip.add_line(cx, 16.0, &format!("Level: {} to {}", lvl(&minMod), lvl(&maxMod)));
        // Assuming that all mods have the same tags
        // L859
        if !maxMod.modTags.is_empty() {
            // L860
            tooltip.add_line(cx, 16.0, &format!("Tags: {}", concat_tags(&maxMod.modTags)));
        }
    }
    // L863
    let r#mod = affix_mod_or_err(&item, modId.or_else(|| modList.first().copied()));
    // L864
    let notableName = r#mod.lines.first().and_then(|l| {
        pattern::r#match(l, "1 Added Passive Skill is (.*)", None).map(|c| c[0].to_str().into_owned())
    });
    // L865
    let node = notableName.as_ref().and_then(|name| {
        cx.build()
            .model
            .spec
            .get()
            .tree
            .get()
            .clusterNodeMap
            .get(name.as_str())
            .obj::<model::Node>()
    });
    // L866
    if let (Some(node), Some(notableName)) = (node, notableName) {
        // L867
        tooltip.add_separator(cx, Some(14.0));

        // Node name
        // L870
        let f = items_ui_hooks().socket_viewer_add_node_name;
        f(cx, me, tooltip, &node);

        // Node description
        // L873
        let sd = node.sd.get().snapshot();
        if !sd.is_empty() {
            // L874
            tooltip.add_line(cx, 16.0, "");
            let mods = node.mods.get().snapshot();
            // L875
            for (i, line) in sd.iter().enumerate() {
                // L876
                let m = mods
                    .get(i)
                    .cloned()
                    .unwrap_or_else(|| lua_error!("attempt to index field '?' (a nil value)"));
                if &**line != " " && (m.truthy(n!("extra")) || !m.truthy(n!("list"))) {
                    // L877
                    let mut line = format!("{}{line}", cx.main.cc.UNSUPPORTED);
                    // L878
                    if cx.main.not_supported_mod_tooltips {
                        line = format!("{line}{}", cx.app().main.not_supported_tooltip_text);
                    }
                    // L879
                    tooltip.add_line(cx, 16.0, &line);
                } else {
                    // L881
                    let l = format!("{}{line}", cx.main.cc.MAGIC);
                    tooltip.add_line(cx, 16.0, &l);
                }
            }
        }

        // Reminder text
        // L887
        if let Some(reminder) = node.extra.get(n!("reminderText")).table() {
            // L888
            tooltip.add_separator(cx, Some(14.0));
            // L889
            for (_, line) in reminder.ipairs() {
                // L890
                let l = format!("^xA0A080{}", line.str().unwrap_or_default());
                tooltip.add_line(cx, 14.0, &l);
            }
        }

        // Comparison
        // L895
        tooltip.add_separator(cx, Some(14.0));
        // L896
        let f = items_ui_hooks().append_added_notable_tooltip;
        f(cx, me, tooltip, &node);

        // Information of for this notable appears
        // L899
        let clusterInfo = cluster_info_for_notable(&notableName);
        // L900
        if let Some(clusterInfo) = clusterInfo {
            // L901
            tooltip.add_separator(cx, Some(14.0));
            // L902
            tooltip.add_line(cx, 20.0, &format!("^7{notableName} can appear on:"));
            // L903
            let mut isFirstSize = true;
            // L904 ORDER: see the module docs (the dump's key order)
            let sizes: Vec<String> = match clusterInfo.field("size") {
                gamedata::DataValue::Table(t) => t
                    .fields
                    .iter()
                    .filter(|(_, v)| v.truthy())
                    .map(|(k, _)| k.to_string())
                    .collect(),
                _ => lua_error!("bad argument #1 to 'pairs' (table expected, got nil)"),
            };
            let jewelTypes: Vec<String> = match clusterInfo.field("jewelTypes") {
                gamedata::DataValue::Table(t) => t
                    .seq
                    .iter()
                    .map_while(|v| match v {
                        gamedata::DataValue::Str(s) => Some(s.to_string()),
                        _ => None,
                    })
                    .collect(),
                _ => lua_error!("bad argument #1 to 'ipairs' (table expected, got nil)"),
            };
            for size in sizes {
                // L905
                let l = format!("{}{size}:", cx.main.cc.MAGIC);
                tooltip.add_line(cx, 18.0, &l);
                // L906
                let sizeSkills = &model::data()
                    .clusterJewels
                    .jewels
                    .get(&Name::from(size.as_str()))
                    .unwrap_or_else(|| lua_error!("attempt to index field '?' (a nil value)"))
                    .skills;
                // L907
                for ty in &jewelTypes {
                    // L908
                    if let Some(s) = sizeSkills.get(&Name::from(ty.as_str())) {
                        // L909
                        tooltip.add_line(cx, 14.0, &format!("^7    {}", s.name));
                    }
                }
                // L912
                if !isFirstSize {
                    // L913
                    tooltip.add_line(cx, 10.0, "");
                }
                // L915
                isFirstSize = false;
            }
        }
    } else {
        // L919-924
        let r#mod = if modId.is_some() || modList.len() == 1 {
            // L921
            affix_mod(&item, modId.or_else(|| modList.first().copied()))
        } else {
            // L923
            let q = cx.app().main.default_item_affix_quality.unwrap_or(0.5);
            let k = 1.0 + round((modList.len() as f64 - 1.0) * q, None);
            affix_mod(&item, mod_at(&modList, k))
        };

        // Adding Mod
        // L927
        let f = items_ui_hooks().add_mod_comparison_tooltip;
        f(cx, me, tooltip, r#mod.as_ref());
    }
}

/// `self.build.data.clusterJewelInfoForNotable[notableName]` (a dump root kept untyped).
fn cluster_info_for_notable(name: &str) -> Option<&'static gamedata::DataTable> {
    match model::data().other.get("data.clusterJewelInfoForNotable")? {
        gamedata::DataValue::Table(t) => match t.field(name) {
            gamedata::DataValue::Table(info) => Some(info),
            _ => None,
        },
        _ => None,
    }
}

/// L986-1010: the custom and crucible modifier buttons.
/// @pob Classes/ItemsTab.lua:986-1011 h=fe3cf8b8
fn ctor_custom(cx: &mut Cx, me: CtrlId) {
    // Section: Custom modifiers
    // if either Custom or Crucible mod buttons are shown, create the control for the list of mods
    // L988-990
    let affix = ctl(cx, me, "displayItemSectionAffix");
    let section = cx
        .add(
            me,
            "displayItemSectionCustom",
            PlainControl::new(
                Anchor::new(TopLeft, affix, BottomLeft).collapse(),
                rect(
                    0,
                    0,
                    0,
                    Prop::f(move |cx, _| {
                        // L989
                        if cx.is_shown(ctl(cx, me, "displayItemAddCustom"))
                            || cx.is_shown(ctl(cx, me, "displayItemAddCrucible"))
                        {
                            28.0 + exn(&di(cx, me), n!("customCount")) * 22.0
                        } else {
                            0.0
                        }
                    }),
                ),
            ),
        )
        .id();
    // L991-993
    cx.base_mut(section).shown = Prop::f(move |cx, _| st(cx, me).displayItem.is_some());
    // L994-996
    let add_custom = cx
        .add(
            me,
            "displayItemAddCustom",
            ButtonControl::new(
                Anchor::new(TopLeft, section, TopLeft),
                rect(0, 0, 120, 20),
                "Add modifier...",
                move |cx| {
                    // L995
                    let f = items_ui_hooks().add_custom_modifier_to_display_item;
                    f(cx, me);
                },
            ),
        )
        .id();
    // L997-999
    cx.base_mut(add_custom).shown = Prop::f(move |cx, _| {
        display_item(cx, me).is_some_and(|item| {
            let rarity = item.rarity.get();
            &*rarity == "MAGIC"
                || &*rarity == "RARE"
                || item
                    .rareLikeUnique_ref()
                    .is_some_and(|r| r.supportsCustomModifiers.is_some())
        })
    });

    // Section: Crucible modifiers
    // if the Add modifier button is not shown, take its place, otherwise move it to the right of it
    // L1003-1007
    let add_crucible = cx
        .add(
            me,
            "displayItemAddCrucible",
            ButtonControl::new(
                Anchor::new(TopLeft, section, TopLeft),
                rect(
                    Prop::f(move |cx, _| {
                        // L1004
                        if cx.is_shown(ctl(cx, me, "displayItemAddCustom")) {
                            128.0
                        } else {
                            0.0
                        }
                    }),
                    0,
                    150,
                    20,
                ),
                "Add Crucible mod...",
                move |cx| {
                    // L1006
                    let f = items_ui_hooks().add_crucible_modifier_to_display_item;
                    f(cx, me);
                },
            ),
        )
        .id();
    // L1008-1010
    cx.base_mut(add_crucible).shown = Prop::f(move |cx, _| {
        display_item(cx, me).is_some_and(|item| {
            &*item.GetPrimarySlot() == "Weapon 1"
                || &*item.r#type.get() == "Shield"
                || ext(&item, n!("canHaveShieldCrucibleTree"))
        })
    });
}

/// `main.showAllItemAffixes`
fn show_all(cx: &Cx) -> bool {
    cx.app().main.show_all_item_affixes
}

/// `self.displayItem.rarity == "UNIQUE"`
fn is_unique(item: &model::ItemRef) -> bool {
    &*item.rarity.get() == "UNIQUE"
}

/// `getSelectedModLine()` (L1037-1039).
fn getSelectedModLine(cx: &Cx, me: CtrlId) -> Option<Table> {
    // L1038
    let item = display_item(cx, me)?;
    let sel = cx
        .w::<DropDownControl>(ctl(cx, me, "displayItemRangeLine"))
        .sel_index;
    range_line_list(&item).get(sel as f64).table()
}

/// `line and line.modId and line.newModId` (L1043, L1066, L1104, L1131).
fn mutable_line(line: Option<&Table>) -> bool {
    line.is_some_and(|l| l.truthy(n!("modId")) && l.truthy(n!("newModId")))
}

/// `self.displayItem:MutateMod(line.modId, line.newModId, not line.mutated)`
fn mutate(item: &model::ItemRef, line: &Table) {
    let name = |k: Name| {
        Name::from(
            &line
                .str(k)
                .unwrap_or_else(|| lua_error!("MutateMod: '{k}' is not a string")),
        )
    };
    item.MutateMod(
        name(n!("modId")),
        name(n!("newModId")),
        !line.truthy(n!("mutated")),
    );
}

/// `slider.val = modLine.range or slider.val` (L1089, L1156; see DIVERGENCE(range-slider-val)).
fn sync_range_val(cx: &mut Cx, slider: CtrlId, modLine: Option<Table>) {
    let Some(modLine) = modLine else { return };
    match modLine.get(n!("range")) {
        Val::Nil | Val::Bool(false) => {}
        Val::Num(v) => cx.w_mut::<SliderControl>(slider).val = v,
        other => lua_error!(
            "attempt to perform arithmetic on field 'val' (a {} value)",
            other.lua_type()
        ),
    }
}

/// The foulborn icon `Draw` override (L1051-1056, L1134-1139: `dy` is -1 and 0) plus the
/// DIVERGENCE(mutated-state) sync, and the key overrides that sync before the class methods.
fn foulborn_box_overrides(
    cx: &mut Cx,
    me: CtrlId,
    boxid: CtrlId,
    dy: f64,
    state: Rc<dyn Fn(&Cx) -> bool>,
) {
    let icon = st(cx, me).foulbornIcon.clone();
    let s1 = state.clone();
    let s2 = state.clone();
    let s3 = state;
    cx.set_ov(boxid, move |o| {
        o.draw = Some(Rc::new(move |cx, b, vp, no_tooltip| {
            let v = s1(cx);
            cx.w_mut::<CheckBoxControl>(b).state = v;
            // L1052 / L1135
            let (x, y) = cx.pos(b);
            // L1053 / L1136
            cx.sg.set_draw_color(1.0, 1.0, 1.0);
            // L1054 / L1137
            cx.sg.draw_image(Some(&icon), x - 24.0, y + dy, 20.0, 20.0);
            // L1055 / L1138
            <CheckBoxControl as Widget>::draw(cx, b, vp, no_tooltip);
        }));
        o.on_key_down = Some(Rc::new(move |cx, b, key, dc| {
            let v = s2(cx);
            cx.w_mut::<CheckBoxControl>(b).state = v;
            <CheckBoxControl as Widget>::on_key_down(cx, b, key, dc)
        }));
        o.on_key_up = Some(Rc::new(move |cx, b, key| {
            let v = s3(cx);
            cx.w_mut::<CheckBoxControl>(b).state = v;
            <CheckBoxControl as Widget>::on_key_up(cx, b, key)
        }));
    });
}

/// The range sliders' DIVERGENCE(range-slider-val) overrides: `modLine` is the line their `shown`
/// reads.
fn range_slider_overrides(cx: &mut Cx, slider: CtrlId, modLine: Rc<dyn Fn(&Cx) -> Option<Table>>) {
    let m1 = modLine.clone();
    let m2 = modLine.clone();
    let m3 = modLine;
    cx.set_ov(slider, move |o| {
        o.draw = Some(Rc::new(move |cx, s, vp, no_tooltip| {
            let line = m1(cx);
            sync_range_val(cx, s, line);
            <SliderControl as Widget>::draw(cx, s, vp, no_tooltip);
        }));
        o.on_key_down = Some(Rc::new(move |cx, s, key, dc| {
            let line = m2(cx);
            sync_range_val(cx, s, line);
            <SliderControl as Widget>::on_key_down(cx, s, key, dc)
        }));
        o.on_key_up = Some(Rc::new(move |cx, s, key| {
            let line = m3(cx);
            sync_range_val(cx, s, line);
            <SliderControl as Widget>::on_key_up(cx, s, key)
        }));
    });
}

/// L1012-1095: the range section, the range line dropdown, its foulborn checkbox and slider.
/// @pob Classes/ItemsTab.lua:1012-1096 h=6eb1f70e
fn ctor_range(cx: &mut Cx, me: CtrlId) {
    // Section: Modifier Range
    // L1013
    let labelFontSize = 14.0;
    // L1014-1028
    let custom = ctl(cx, me, "displayItemSectionCustom");
    let section = cx
        .add(
            me,
            "displayItemSectionRange",
            PlainControl::new(
                Anchor::new(TopLeft, custom, BottomLeft),
                rect(
                    0,
                    0,
                    0,
                    Prop::f(move |cx, _| {
                        // L1015
                        let Some(item) = display_item(cx, me) else {
                            // L1016
                            return 0.0;
                        };
                        let list = range_line_list(&item);
                        if !list.truthy(1) {
                            // L1016
                            return 0.0;
                        }
                        // L1018
                        if show_all(cx) && is_unique(&item) {
                            // L1019
                            let mut height = 0.0;
                            // L1020
                            for i in 1..=list.len() {
                                // L1021
                                if i > STACKED_RANGE_LINES {
                                    lua_error!("attempt to index local 'label' (a nil value)");
                                }
                                // L1022
                                let lc = st(cx, me).stacked_line_count[i - 1].get();
                                height += m_max(22.0, lc.unwrap_or(0.0) * labelFontSize);
                            }
                            // L1024
                            height + 4.0
                        } else {
                            // L1026
                            28.0
                        }
                    }),
                ),
            ),
        )
        .id();
    // L1029-1030
    let icon = cx.sg.new_image_handle();
    icon.load(&mut cx.sg, "Assets/breachicon.png", ImageFlags::empty());
    st_mut(cx, me).foulbornIcon = icon;
    // L1031-1033
    let range_line = cx
        .add(
            me,
            "displayItemRangeLine",
            DropDownControl::new(
                Anchor::new(TopLeft, section, TopLeft),
                rect(0, 0, 350, 18),
                Vec::new(),
                sel_func(move |cx, index, _value| {
                    // L1032 (DIVERGENCE(nil-range): a nil `range` keeps `val`)
                    let item = di(cx, me);
                    let line = range_line_list(&item)
                        .get(index as f64)
                        .table()
                        .unwrap_or_else(|| lua_error!("attempt to index field '?' (a nil value)"));
                    let slider = ctl(cx, me, "displayItemRangeSlider");
                    if let Some(v) = line.get(n!("range")).opt_num() {
                        cx.w_mut::<SliderControl>(slider).val = v;
                    }
                }),
                None,
                false,
            ),
        )
        .id();
    // L1034-1036
    cx.base_mut(range_line).shown = Prop::f(move |cx, _| {
        display_item(cx, me).is_some_and(|item| {
            range_line_list(&item).truthy(1) && !(show_all(cx) && is_unique(&item))
        })
    });
    // L1037-1039 getSelectedModLine: the fn above
    // L1040
    let boxid = cx
        .add_free(
            Some(me),
            CheckBoxControl::new(
                Anchor::new(Left, range_line, Right).collapse(),
                rect(0, 0, 18, 0),
                None::<String>,
                None,
                None,
                false,
            ),
        )
        .id();
    // L1041-1049
    cx.w_mut::<CheckBoxControl>(boxid).change_func = Some(Rc::new(move |cx, _val| {
        // L1042
        let line = getSelectedModLine(cx, me);
        // L1043
        if mutable_line(line.as_ref()) {
            // L1044
            mutate(&di(cx, me), line.as_ref().expect("checked"));
            let h = items_ui_hooks();
            // L1045
            (h.update_display_item_tooltip)(cx, me);
            // L1046
            (h.update_custom_controls)(cx, me);
            // L1047
            (h.update_display_item_range_lines)(cx, me);
        }
    }));
    // L1050-1056 box.RealDraw = box.Draw; function box:Draw(...) (and the state sync)
    foulborn_box_overrides(
        cx,
        me,
        boxid,
        -1.0,
        Rc::new(move |cx| {
            // L1059-1065
            getSelectedModLine(cx, me).is_some_and(|l| l.truthy(n!("mutated")))
        }),
    );

    // L1058-1067 (the state write: DIVERGENCE(mutated-state))
    cx.base_mut(boxid).shown = Prop::f(move |cx, _| {
        // L1059
        let line = getSelectedModLine(cx, me);
        // L1066
        !show_all(cx) && mutable_line(line.as_ref())
    });
    // fix spacing
    // L1069-1075
    cx.base_mut(boxid).x = Prop::f(|cx, b| if cx.is_shown(b) { 25.0 } else { 8.0 });
    // L1076
    cx.put(me, "displayItemMutatedCheckbox", boxid);
    // L1077-1083
    let slider = cx
        .add_free(
            Some(me),
            SliderControl::new(
                Anchor::new(Left, boxid, Right).collapse(),
                rect(4, 0, 100, 18),
                Some(Rc::new(move |cx, val| {
                    // L1078
                    let line = getSelectedModLine(cx, me)
                        .unwrap_or_else(|| lua_error!("attempt to index local 'line' (a nil value)"));
                    // L1079
                    line.set(n!("range"), val);
                    // L1080
                    di(cx, me).BuildAndParseRaw();
                    // L1081
                    update_display_item_tooltip(cx, me);
                    // L1082
                    update_custom_controls(cx, me);
                })),
                None,
            ),
        )
        .id();
    // L1085 `modLine` of the slider's `shown`
    let modLine = move |cx: &Cx| -> Option<Table> {
        let item = display_item(cx, me)?;
        if show_all(cx) && is_unique(&item) {
            return None;
        }
        let sel = cx
            .w::<DropDownControl>(ctl(cx, me, "displayItemRangeLine"))
            .sel_index;
        range_line_list(&item).get(sel as f64).table()
    };
    // L1084-1094
    cx.base_mut(slider).shown = Prop::f(move |cx, _| {
        // L1085
        match modLine(cx) {
            // L1086-1090 (the `val` write: DIVERGENCE(range-slider-val))
            Some(l) => l.truthy(n!("showSlider")),
            // L1092
            None => false,
        }
    });
    range_slider_overrides(cx, slider, Rc::new(modLine));
    // L1095
    cx.put(me, "displayItemRangeSlider", slider);
}

/// L1097-1179: the stacked range lines (`main.showAllItemAffixes` on a unique).
/// @pob Classes/ItemsTab.lua:1097-1179 h=76c3e394
fn ctor_stacked_range(cx: &mut Cx, me: CtrlId) {
    let labelFontSize = 14.0;
    // L1097
    for i in 1..=STACKED_RANGE_LINES {
        // L1098
        let baseControl = ctl(cx, me, "displayItemSectionRange");

        // layout: [box] <- [slider] <- text

        // `(self.displayItem and self.displayItem.rangeLineList[i]) or nil`
        let line_at = move |cx: &Cx| -> Option<Table> {
            let item = display_item(cx, me)?;
            range_line_list(&item).get(i as f64).table()
        };
        // L1102-1109
        let boxid = cx
            .add_free(
                Some(me),
                CheckBoxControl::new(
                    Anchor::new(TopLeft, baseControl, TopLeft).collapse(),
                    rect(0, 0, 18, 0),
                    None::<String>,
                    Some(Rc::new(move |cx, _val| {
                        // L1103
                        let line = line_at(cx);
                        // L1104
                        if mutable_line(line.as_ref()) {
                            // L1105
                            mutate(&di(cx, me), line.as_ref().expect("checked"));
                            // L1106
                            update_display_item_tooltip(cx, me);
                            // L1107
                            update_custom_controls(cx, me);
                        }
                    })),
                    None,
                    false,
                ),
            )
            .id();
        // fix spacing
        // L1111-1117
        cx.base_mut(boxid).x = Prop::f(|cx, b| if cx.is_shown(b) { 24.0 } else { 0.0 });
        // L1118-1122
        cx.base_mut(boxid).y = Prop::f(move |cx, _| {
            if i == 1 {
                return 2.0;
            }
            // L1119
            let prevBox = ctl(cx, me, format!("displayItemStackedMutatedCheckbox{}", i - 1));
            // L1120 (`prevLabel.lineCount`)
            let lc = st(cx, me).stacked_line_count[i - 2].get();
            // L1121
            cx.base(prevBox).y.get(cx, prevBox) + m_max(22.0, lc.unwrap_or(0.0) * 14.0)
        });
        // L1123-1132 (the state write: DIVERGENCE(mutated-state))
        cx.base_mut(boxid).shown = Prop::f(move |cx, _| {
            // L1124
            let line = line_at(cx);
            // L1131
            show_all(cx) && mutable_line(line.as_ref())
        });
        // L1133-1139
        foulborn_box_overrides(
            cx,
            me,
            boxid,
            0.0,
            Rc::new(move |cx| {
                // L1126-1130
                line_at(cx).is_some_and(|l| l.truthy(n!("mutated")))
            }),
        );

        // L1141
        cx.put(me, format!("displayItemStackedMutatedCheckbox{i}"), boxid);

        // L1143-1150
        let slider = cx
            .add_free(
                Some(me),
                SliderControl::new(
                    Anchor::new(Left, boxid, Right).collapse(),
                    rect(4, 0, 100, 18),
                    Some(Rc::new(move |cx, val| {
                        // L1144
                        if let Some(line) = line_at(cx) {
                            // L1145
                            line.set(n!("range"), val);
                            // L1146
                            di(cx, me).BuildAndParseRaw();
                            // L1147
                            update_display_item_tooltip(cx, me);
                            // L1148
                            update_custom_controls(cx, me);
                        }
                    })),
                    None,
                ),
            )
            .id();
        // L1152 `modLine` of the slider's `shown`
        let modLine = move |cx: &Cx| -> Option<Table> {
            if !show_all(cx) {
                return None;
            }
            let item = display_item(cx, me)?;
            if !is_unique(&item) {
                return None;
            }
            range_line_list(&item).get(i as f64).table()
        };
        // L1151-1161
        cx.base_mut(slider).shown = Prop::f(move |cx, _| match modLine(cx) {
            // L1153-1157 (the `val` write: DIVERGENCE(range-slider-val))
            Some(l) => l.truthy(n!("showSlider")),
            // L1159
            None => false,
        });
        range_slider_overrides(cx, slider, Rc::new(modLine));

        // L1163
        cx.put(me, format!("displayItemStackedRangeSlider{i}"), slider);

        // L1165-1175
        let label = cx
            .add(
                me,
                format!("displayItemStackedRangeLine{i}"),
                LabelControl::new(
                    Anchor::new(Left, slider, Right).collapse(),
                    rect(4, -2, 350, labelFontSize),
                    Prop::f(move |cx, _| {
                        // L1166
                        let item = di(cx, me);
                        let modLine = range_line_list(&item).get(i as f64).table();
                        // L1167
                        if let Some(modLine) = modLine {
                            // L1168
                            let colour = if modLine.truthy(n!("mutated")) {
                                cx.main.cc.MUTATED.clone()
                            } else {
                                "^7".to_owned()
                            };
                            // L1169
                            let line = modLine.str(n!("line")).unwrap_or_else(|| {
                                lua_error!("bad argument #1 to 'WrapString' (string expected)")
                            });
                            let text = wrap_string(cx, &line, labelFontSize, 370.0).join("\n");
                            // L1170
                            let lineCount = text.bytes().filter(|&b| b == b'\n').count();
                            // L1171
                            st(cx, me).stacked_line_count[i - 1].set(Some(lineCount as f64 + 1.0));
                            // L1172
                            return format!("{colour}{text}");
                        }
                        // L1174
                        String::new()
                    }),
                ),
            )
            .id();
        // L1176-1178
        cx.base_mut(label).shown =
            Prop::f(move |cx, _| cx.is_shown(slider) || cx.is_shown(boxid));
    }
}

/// L1180-1215: the tooltip anchor, the scroll bars, the drag targets and the item sets (UI part).
/// @pob Classes/ItemsTab.lua:1180-1195 h=5d255ab4
fn ctor_tail(cx: &mut Cx, me: CtrlId, build: CtrlId) {
    // Tooltip anchor
    // L1181
    let range = ctl(cx, me, "displayItemSectionRange");
    cx.add(
        me,
        "displayItemTooltipAnchor",
        PlainControl::new(Anchor::new(TopLeft, range, BottomLeft), rect(0, 0, 0, 0)),
    );

    // Scroll bars
    // L1184
    cx.add(
        me,
        "scrollBarH",
        ScrollBarControl::new(
            Anchor::NONE,
            rect(0, 0, 0, 18),
            Some(100.0),
            Some(ScrollDir::Horizontal),
            true,
        ),
    );
    // L1185
    cx.add(
        me,
        "scrollBarV",
        ScrollBarControl::new(
            Anchor::NONE,
            rect(0, 0, 18, 0),
            Some(100.0),
            Some(ScrollDir::Vertical),
            true,
        ),
    );

    // Initialise drag target lists
    let item_list = ctl(cx, me, "itemList");
    let shared = ctl(cx, me, "sharedItemList");
    let unique_db = ctl(cx, me, "uniqueDB");
    let rare_db = ctl(cx, me, "rareDB");
    let minion = cx
        .child(build, "mainSkillMinion")
        .unwrap_or_else(|| lua_error!("attempt to index field 'mainSkillMinion' (a nil value)"));
    let push = |cx: &mut Cx, list: CtrlId, t: CtrlId| {
        cx.w_mut::<ListControl>(list)
            .drag_target_list
            .get_or_insert_with(Vec::new)
            .push(t);
    };
    // L1188
    push(cx, item_list, shared);
    // L1189
    push(cx, item_list, minion);
    // L1190
    push(cx, unique_db, item_list);
    // L1191
    push(cx, unique_db, shared);
    // L1192
    push(cx, unique_db, minion);
    // L1193
    push(cx, rare_db, item_list);
    // L1194
    push(cx, rare_db, shared);
    // L1195
    push(cx, rare_db, minion);
    // L1196
    push(cx, shared, item_list);
    // L1197
    push(cx, shared, minion);
    // L1198 ORDER: (c), see the module docs
    let slots: Vec<CtrlId> = st(cx, me).slots.values().copied().collect();
    for slot in slots {
        // L1199
        push(cx, item_list, slot);
        // L1200
        push(cx, unique_db, slot);
        // L1201
        push(cx, rare_db, slot);
        // L1202
        push(cx, shared, slot);
    }

    // Initialise item sets
    // L1206-1209 (model part: calc:BM-A; DIVERGENCE(ctor-sync-loadouts) for L1209's UI tail)

    // L1211 self:PopulateSlots() (model part: calc:BM-A): the slot lists
    slot_helper::populate_slot_lists_of(cx, me);
    // L1212 (model part: calc:BM-A; see [`lastSlot`])
    // L1213 return self
}

// ---- the BuildMode seams (CCR-0098 §3, CCR-0106 §2) ------------------------------------------

/// `self.itemsTab` of the open build, when the tab is this unit's ItemsTab.
pub fn tab(cx: &Cx) -> Option<CtrlId> {
    let id = cx.app().build.as_ref()?.tabs.items.id();
    cx.ui.try_w::<ItemsTab>(id).map(|_| id)
}

/// `BuildHooks::items_tab`: `self.itemsTab = new("ItemsTab"):ItemsTab(self)` (Build.lua:612).
pub fn ctor_hook(cx: &mut Cx, build: CtrlId) -> CtrlId {
    cx.add_free(Some(build), ItemsTab::new(build)).id()
}
