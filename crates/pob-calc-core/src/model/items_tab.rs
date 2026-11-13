//! @pob-file Classes/ItemsTab.lua @16de4b82d
//! ItemsTab model (Classes/ItemsTab.lua 1216-1727, 2998-3010).
//!
//! Owner: BM-A. The model halves of the items tab: loading and saving the `<Items>` section,
//! item sets, slot population and `AddItem`. UI statements (controls, undo, tooltips) are left
//! to the UI port (ui doc U33/U34) and marked `// UI:` where they sit in the Lua.
//!
//! CCR-0080 lists the contract gaps this file works around locally:
//! * slots are the `ItemSlotControl`s of the constructor (ItemsTab.lua 140-235). Their model
//!   half is a [`Table`] per slot (`slotName`, `slotNum`, `label`, `nodeId`, `weaponSet`,
//!   `selItemId`, `active`, `inactive`, `parentSlot`, `abyssalSocketList`, `items`,
//!   `controls.activate.state` on flask slots) built by [`ItemsTab::ctor_model`]; the `shown`
//!   closures are the Rust-only key `$shown` interpreted by [`slot_IsShown`].
//!   `ItemSlotControl:SetSelItemId` / `Populate` (ItemSlotControl.lua 72-123),
//!   `ItemsTab:IsItemValidForSlot` (2560-2605), `getAnoint` (2978-2993) and `isAnointable`
//!   (70-74) are ported here because the in-range functions call them;
//! * `ItemsTab` (model/mod.rs, U0) has no field for `showStatDifferences`,
//!   `tradeQuery.statSortSelectionList`, `lastSlot` and `slotOrder`: they live in the build's
//!   side table (`build_xml::build_extra(build)["itemsTab"]`);
//! * `itemOrderList` is the frozen `orderList`; item and item-set ids are `u32` keys (a
//!   non-integral or negative id raises, where Lua would accept any number key).
#![allow(non_snake_case, clippy::all, clippy::pedantic)]

use crate::classes::item::new_Item;
use crate::model::build_xml::build_extra;
use crate::model::{XmlChild, XmlNode};
use crate::prelude::*;

// ---- local helpers (Rust-only plumbing) ---------------------------------------------------

/// @pob Classes/ItemsTab.lua:37-37 h=cce3e07e
const baseSlots: [&str; 18] = [
    "Weapon 1",
    "Weapon 2",
    "Helmet",
    "Body Armour",
    "Gloves",
    "Boots",
    "Amulet",
    "Ring 1",
    "Ring 2",
    "Ring 3",
    "Belt",
    "Graft 1",
    "Graft 2",
    "Flask 1",
    "Flask 2",
    "Flask 3",
    "Flask 4",
    "Flask 5",
];

/// `node.attrib[k]`.
fn attr<'a>(node: &'a XmlNode, k: &str) -> Option<&'a str> {
    node.attrib.get(k).map(|v| &**v)
}

/// `tonumber(s)` (nil → nil).
fn tonum(s: Option<&str>) -> Option<f64> {
    s.and_then(pob_lua::tonumber)
}

/// A table key from a number id: integral ids only (CCR-0080 §5).
pub fn id_key(id: f64) -> Option<u32> {
    (id.fract() == 0.0 && id >= 0.0 && id <= f64::from(u32::MAX)).then_some(id as u32)
}

/// `id_key` that raises where Lua would store under a key the frozen maps cannot hold.
fn id_key_or_raise(id: f64, what: &str) -> u32 {
    id_key(id).unwrap_or_else(|| lua_error!("CCR-0080: non-integer {what} id {}", tostring_num(id)))
}

/// `tostring(v)` of a scalar value.
fn tostring_val(v: &Val) -> String {
    match v {
        Val::Nil => "nil".to_owned(),
        Val::Bool(b) => b.to_string(),
        Val::Num(x) => tostring_num(*x),
        Val::Str(s) => s.to_string(),
        other => lua_error!("ItemsTab: tostring of a {} value", other.lua_type()),
    }
}

fn new_xml(elem: &str) -> XmlNode {
    XmlNode {
        elem: LStr::from(elem),
        attrib: indexmap::IndexMap::new(),
        children: Vec::new(),
    }
}

/// `attrib[k] = v` for a value that may be nil/false (composeNode skips those).
fn set_attr(node: &mut XmlNode, k: &str, v: Option<String>) {
    if let Some(v) = v {
        node.attrib.insert(LStr::from(k), LStr::from(v));
    }
}

/// `t[k]` that raises like Lua when `t[k]` is not a table and is indexed.
fn sub_table(t: &Table, k: impl Into<Key> + Copy + std::fmt::Debug) -> Table {
    t.table(k)
        .unwrap_or_else(|| lua_error!("attempt to index field '{:?}' (a nil value)", k))
}

/// `self.items[id]` for an id value (nil / non-integral → nil).
fn item_at(itemsTab: &ItemsTab, id: &Val) -> Option<ItemRef> {
    let id = id.opt_num()?;
    let k = id_key(id)?;
    itemsTab.items.with(|m| m.get(&k).cloned())
}

/// `self.build.spec`, `nil` while the build has no spec yet (the placeholder of `Build::new`
/// belongs to no build; Lua's `build.spec` is nil until TreeTab's constructor sets it).
pub(crate) fn build_spec(build: &BuildRef) -> Option<PassiveSpecRef> {
    let spec = build.spec.get();
    spec.build
        .upgrade()
        .is_some_and(|b| Rc::ptr_eq(&b, build))
        .then_some(spec)
}

/// The tab's side table (CCR-0080 §2).
fn tab_extra(build: &BuildRef) -> Table {
    build_extra(build).get_or_new_table(n!("itemsTab"))
}

/// `self.activeItemSet` (raises when nil, as an index of it does).
fn activeItemSet(itemsTab: &ItemsTab) -> Table {
    itemsTab
        .activeItemSet
        .get()
        .unwrap_or_else(|| lua_error!("attempt to index field 'activeItemSet' (a nil value)"))
}

// ---- ItemSlotControl (Classes/ItemSlotControl.lua 21-123, model half) --------------------

/// @pob Classes/ItemSlotControl.lua:21-70 h=e51df699
/// `new("ItemSlotControl", anchor, x, y, itemsTab, slotName, slotLabel, nodeId)`: the model
/// half (the dropdown, tooltip and layout are the UI's).
fn new_ItemSlotControl(slotName: &str, slotLabel: Option<&str>, nodeId: Option<f64>) -> Table {
    let slot = Table::new();
    // L22-35 UI: DropDownControl, anchor.collapse, enabled; `shown` is the default kind
    slot.set(n!("$shown"), "default");
    // L37
    slot.set(n!("items"), Table::new());
    // L38
    slot.set(n!("selItemId"), 0.0);
    // L39
    slot.set(n!("slotName"), slotName);
    // L40
    let num = pob_lua::pattern::r#match(slotName, "%d+$", None)
        .or_else(|| pob_lua::pattern::r#match(slotName, "%d+", None))
        .and_then(|c| c[0].num());
    slot.set(n!("slotNum"), num.map_or(Val::Nil, Val::Num));
    let controls = Table::new();
    // L41
    if pob_lua::pattern::find(slotName, "Flask", None, false).is_some() {
        // L42 (the activate checkbox; its callback is the UI's)
        let activate = Table::new();
        activate.set(n!("state"), false);
        controls.set(n!("activate"), activate);
        // L51-52 UI: tooltipText, labelOffset
    }
    slot.set(n!("controls"), controls);
    // L56
    slot.set(n!("abyssalSocketList"), Table::new());
    // L57-66 UI: tooltipFunc
    // L67
    slot.set(n!("label"), slotLabel.unwrap_or(slotName));
    // L68
    slot.set(n!("nodeId"), nodeId.map_or(Val::Nil, Val::Num));
    slot
}

/// `slot:IsShown()` (Control.lua:125-127; `anchor.collapse` is set on every item slot, so it
/// is `slot.shown`): the `shown` closures of ItemsTab.lua 158-208 and ItemSlotControl.lua 33.
/// `pub` for the UI's slots of another build (CCR-0138 §2; FUP6: a comparison entry's ItemsTab).
pub fn slot_IsShown(build: &BuildRef, slot: &Table) -> bool {
    let useSecond = || activeItemSet(&build.itemsTab).truthy(n!("useSecondWeaponSet"));
    match slot.str(n!("$shown")).as_deref() {
        // ItemsTab.lua:159-161
        Some("weapon") => !useSecond(),
        // ItemsTab.lua:165-167
        Some("weaponSwap") => useSecond(),
        // ItemsTab.lua:173-175
        Some("abyssalSwap") => !slot.truthy(n!("inactive")) && useSecond(),
        // ItemsTab.lua:195-197
        Some("abyssalWeapon") => !slot.truthy(n!("inactive")) && !useSecond(),
        // ItemsTab.lua:180-182
        Some("graft") => {
            let spec = build_spec(build)
                .unwrap_or_else(|| lua_error!("attempt to index field 'spec' (a nil value)"));
            pob_lua::pattern::find(&spec.treeVersion.get(), "3_27", None, false).is_some()
        }
        // ItemsTab.lua:184-186
        Some("ring3") => {
            let env =
                build.calcsTab.mainEnv.get().unwrap_or_else(|| {
                    lua_error!("attempt to index field 'mainEnv' (a nil value)")
                });
            env.modDB.get().Flag(None, nm!["AdditionalRingSlot"])
        }
        // ItemSlotControl.lua:33-35
        _ => !slot.truthy(n!("inactive")),
    }
}

/// @pob Classes/ItemSlotControl.lua:72-84 h=65a87401
/// `slot:SetSelItemId(selItemId)`.
fn ItemSlot_SetSelItemId(build: &BuildRef, slot: &Table, selItemId: Val) {
    // L73
    let nodeId = slot.get(n!("nodeId"));
    if let Some(nodeId) = nodeId.opt_num() {
        // L74
        if let Some(spec) = build_spec(build) {
            // L75
            spec.jewels
                .with_mut(|m| m.set(nodeId as i64, selItemId.opt_num()));
            // L76
            if !selItemId.raw_eq(&slot.get(n!("selItemId"))) {
                // L77
                spec.BuildClusterJewelGraphs();
            }
        }
    } else {
        // L81
        let slotName = slot.str(n!("slotName")).unwrap_or_else(|| LStr::from(""));
        sub_table(&activeItemSet(&build.itemsTab), &*slotName)
            .set(n!("selItemId"), selItemId.clone());
    }
    // L83
    slot.set(n!("selItemId"), selItemId);
}

/// @pob Classes/ItemSlotControl.lua:86-123 h=5fb2296e
/// `slot:Populate()` (the model half: `items` and `selItemId`; `list`/`selIndex` are the
/// dropdown's).
fn ItemSlot_Populate(build: &BuildRef, slot: &Table) {
    let itemsTab = &build.itemsTab;
    let slotName = slot.str(n!("slotName")).unwrap_or_else(|| LStr::from(""));
    // L87
    if let Some(nodeId) = slot.get(n!("nodeId")).opt_num() {
        if let Some(spec) = build_spec(build) {
            // L88
            let v = spec.jewels.with(|m| m.get(nodeId as i64)).unwrap_or(0.0);
            slot.set(n!("selItemId"), v);
        }
    }

    // L91
    let items = sub_table(slot, n!("items"));
    items.wipe();
    // L92 UI: wipeTable(self.list)
    // L93
    items.set(1, 0.0);
    // L94-95 UI: self.list[1] = "None"; self.selIndex = 1
    // L96 (integer keys; the port's item map iterates in insertion order — only the dropdown's
    // order depends on it)
    // LUA-ORDER
    let all: Vec<ItemRef> = itemsTab.items.with(|m| m.values().cloned().collect());
    for item in all {
        // L97
        if itemsTab.IsItemValidForSlot(build, &item, &slotName, None) {
            // L98
            items.push(item.id.get().map_or(Val::Nil, Val::Num));
            // L99-102 UI: list entry, selIndex
        }
    }
    // L105
    let selItemId = slot.get(n!("selItemId"));
    let selItem = item_at(itemsTab, &selItemId);
    if !selItemId.truthy()
        || selItem.is_none()
        || !itemsTab.IsItemValidForSlot(build, selItem.as_ref().expect("checked"), &slotName, None)
    {
        // L106
        ItemSlot_SetSelItemId(build, slot, Val::Num(0.0));
    }

    // Update Abyssal Sockets
    // L110
    let mut abyssalSocketCount = 0.0;
    // L111
    let selItemId = slot.get(n!("selItemId"));
    if selItemId.num() > 0.0 {
        // L112
        let selItem = item_at(itemsTab, &selItemId)
            .unwrap_or_else(|| lua_error!("attempt to index local 'selItem' (a nil value)"));
        // L113
        abyssalSocketCount = selItem.abyssalSocketCount.get();
    }
    // L115
    let list = sub_table(slot, n!("abyssalSocketList"));
    for (i, abyssalSocket) in list.ipairs() {
        let Some(abyssalSocket) = abyssalSocket.table() else {
            lua_error!("attempt to index local 'abyssalSocket' (a non-table value)")
        };
        // L116
        let inactive = i as f64 > abyssalSocketCount;
        abyssalSocket.set(n!("inactive"), inactive);
        // L117
        if inactive {
            // this can be inconvenient, but otherwise it is possible to double
            // equip jewels by moving the jewel while the socket is inactive
            // L120
            ItemSlot_SetSelItemId(build, &abyssalSocket, Val::Num(0.0));
        }
    }
}

// ---- ItemsTab helpers outside the unit's ranges --------------------------------------------

/// @pob Classes/ItemsTab.lua:70-74 h=fce26f54
fn isAnointable(item: Option<&ItemRef>) -> bool {
    // L71
    let Some(item) = item else { return false };
    let Some(base) = item.base.get() else {
        return false;
    };
    !base.cannotBeAnointed
        && base.subType.as_deref() != Some("Talisman")
        // L73
        && (item.extra.truthy(n!("canBeAnointed")) || &*base.r#type == "Amulet")
}

/// @pob Classes/ItemsTab.lua:2978-2993 h=c6e8adf0
impl ItemsTab {
    pub fn getAnoint(&self, item: Option<&ItemRef>) -> Vec<String> {
        // L2979
        let mut result = Vec::new();
        // L2980
        if let Some(item) = item {
            // L2981
            for modList in [
                item.enchantModLines.get(),
                item.scourgeModLines.get(),
                item.implicitModLines.get(),
                item.explicitModLines.get(),
                item.crucibleModLines.get(),
            ] {
                // L2982
                for r#mod in modList.snapshot() {
                    // L2983
                    let line = r#mod.str(n!("line")).unwrap_or_else(|| {
                        lua_error!("attempt to index local 'line' (a nil value)")
                    });
                    // L2984
                    let anoint =
                        pob_lua::pattern::find(&line, "Allocates ([a-zA-Z ]+)", None, false);
                    // L2985
                    if let Some((anoint, _, _)) = anoint {
                        // L2986
                        let nodeName =
                            pob_lua::string::sub(&line, (anoint + "Allocates ".len()) as i64, None);
                        // L2987
                        result.push(nodeName.into_owned());
                    }
                }
            }
        }
        // L2992
        result
    }
}

/// @pob Classes/ItemsTab.lua:2560-2605 h=838a66de
impl ItemsTab {
    /// `ItemsTab:IsItemValidForSlot(item, slotName, itemSet)`.
    pub fn IsItemValidForSlot(
        &self,
        build: &BuildRef,
        item: &ItemRef,
        slotName: &str,
        itemSet: Option<&Table>,
    ) -> bool {
        // L2561
        let itemSet = match itemSet {
            Some(s) => s.clone(),
            None => activeItemSet(self),
        };
        // L2562
        let m = pob_lua::pattern::r#match(slotName, "^([%a ]+) (%d+)$", None);
        let (mut slotType, slotId) = match &m {
            Some(c) => (c[0].to_str().into_owned(), Some(c[1].to_str().into_owned())),
            None => (String::new(), None),
        };
        // L2563
        if m.is_none() {
            // L2564
            slotType = slotName.to_owned();
        }
        let itype = item.r#type.get();
        let base = || {
            item.base
                .get()
                .unwrap_or_else(|| lua_error!("attempt to index field 'base' (a nil value)"))
        };
        // L2566
        if slotType == "Jewel" {
            // Special checks for jewel sockets
            // L2568
            let id = slotId.as_deref().and_then(pob_lua::tonumber);
            let spec = build_spec(build)
                .unwrap_or_else(|| lua_error!("attempt to index field 'spec' (a nil value)"));
            let node = id.and_then(|id| {
                spec.tree
                    .get()
                    .nodes
                    .get(id as i64)
                    .or_else(|| spec.nodes.with(|m| m.get(id as i64)))
            });
            // L2569
            let Some(node) = node.filter(|_| &*itype == "Jewel") else {
                // L2570
                return false;
            };
            let subType = base().subType;
            let charmSocket = node.extra.truthy(n!("charmSocket"));
            if charmSocket || subType.as_deref() == Some("Charm") {
                // L2571 Charm sockets can only have charms, and charms can only be in charm sockets
                // L2573
                if charmSocket && subType.as_deref() == Some("Charm") {
                    // L2574
                    return true;
                }
                // L2576
                return false;
            } else if item.clusterJewel.get().is_some() && node.expansionJewel.get().is_none() {
                // L2577 Don't allow cluster jewels in inner sockets
                // L2579
                return false;
            } else if node
                .expansionJewel
                .get()
                .map_or(true, |e| e.get(n!("size")).raw_eq(&Val::Num(2.0)))
            {
                // L2580 Outer sockets can fit anything
                // L2582
                return true;
            } else {
                // Only allow jewels that fit in this socket
                // L2585
                let Some(cj) = item.clusterJewel.get() else {
                    return true;
                };
                let size = node.expansionJewel.get().expect("checked").num(n!("size"));
                return cj.num(n!("sizeIndex")) <= size;
            }
        } else if *itype == *slotType {
            // L2588
            return true;
        } else if &*itype == "Tincture" && slotType == "Flask" {
            // L2590
            return true;
        } else if &*itype == "Jewel"
            && base().subType.as_deref() == Some("Abyss")
            && pob_lua::pattern::find(slotName, "Abyssal Socket", None, false).is_some()
        {
            // L2592
            return true;
        } else if slotName == "Weapon 1" || slotName == "Weapon 1 Swap" || slotName == "Weapon" {
            // L2594
            return base().weapon.is_some();
        } else if slotName == "Weapon 2" || slotName == "Weapon 2 Swap" {
            // L2596
            let w1 = if slotName == "Weapon 2" {
                "Weapon 1"
            } else {
                "Weapon 1 Swap"
            };
            let weapon1Sel = sub_table(&itemSet, w1).get(n!("selItemId"));
            let weapon1Sel = if weapon1Sel.truthy() {
                weapon1Sel
            } else {
                Val::Num(0.0)
            };
            // L2597
            let weapon1Type: Name = item_at(self, &weapon1Sel)
                .map(|i| {
                    i.base
                        .get()
                        .unwrap_or_else(|| {
                            lua_error!("attempt to index field 'base' (a nil value)")
                        })
                        .r#type
                })
                .unwrap_or_else(|| n!("None"));
            let weaponTypeInfo = &build.data.weaponTypeInfo;
            // L2598
            if &*weapon1Type == "None" {
                // L2599
                return &*itype == "Shield"
                    || weaponTypeInfo.get(&itype).is_some_and(|w| w.oneHand);
            } else if &*weapon1Type == "Bow" {
                // L2601
                return &*itype == "Quiver";
            } else if weaponTypeInfo
                .get(&weapon1Type)
                .unwrap_or_else(|| lua_error!("attempt to index field '?' (a nil value)"))
                .oneHand
            {
                // L2603
                return &*itype == "Shield"
                    || (weaponTypeInfo.get(&itype).is_some_and(|w| w.oneHand)
                        && ((&*weapon1Type == "Wand" && &*itype == "Wand")
                            || (&*weapon1Type != "Wand" && &*itype != "Wand")));
            }
        }
        // L2605 (falls off the end: nil)
        false
    }
}

/// @pob Classes/ItemsTab.lua:101-108 h=98f6ff72
/// @pob Classes/ItemsTab.lua:140-235 h=e7cab513
/// @pob Classes/ItemsTab.lua:1196-1215 h=ae3e60a9
impl ItemsTab {
    /// The constructor's model half (idempotent; CCR-0080 §1): `items`, `itemOrderList`,
    /// `showStatDifferences`, the slots (base slots with their swap and abyssal slots, then a
    /// socket slot per `Socket` node of `build.latestTree`, by id), the first item set, and
    /// `lastSlot`. Needs `build.latestTree` (Build.lua:605).
    pub fn ctor_model(&self, build: &BuildRef) {
        let extra = tab_extra(build);
        if extra.truthy(n!("$ctor")) {
            return;
        }
        extra.set(n!("$ctor"), true);
        // L102-103 self.items = { }; self.itemOrderList = { } (the frozen handles)
        // L105
        extra.set(n!("showStatDifferences"), true);
        // L108 self.tradeQuery = new("TradeQuery"): statSortSelectionList starts nil
        extra.set(n!("tradeQuery"), Table::new());

        // Item slots
        // L141-143
        self.slots.with_mut(|m| m.clear());
        self.orderedSlots.wipe();
        let slotOrder = Table::new();
        extra.set(n!("slotOrder"), slotOrder.clone());
        // L146
        let addSlot = |slot: &Table| {
            let name = Name::from(&*slot.str(n!("slotName")).expect("slotName"));
            // L148
            self.slots.with_mut(|m| m.insert(name, slot.clone()));
            // L149
            self.orderedSlots.push(slot.clone());
            // L150
            slotOrder.set(name, self.orderedSlots.len() as f64);
            // L151 UI: t_insert(self.controls, slot)
        };
        // L153
        for slotName in baseSlots {
            // L154
            let slot = new_ItemSlotControl(slotName, None, None);
            // L155
            addSlot(&slot);
            // L156
            if pob_lua::pattern::find(slotName, "Weapon", None, false).is_some() {
                // Add alternate weapon slot
                // L158
                slot.set(n!("weaponSet"), 1.0);
                // L159
                slot.set(n!("$shown"), "weapon");
                // L162
                let swapName = format!("{slotName} Swap");
                let swapSlot = new_ItemSlotControl(&swapName, Some(slotName), None);
                // L163
                addSlot(&swapSlot);
                // L164
                swapSlot.set(n!("weaponSet"), 2.0);
                // L165
                swapSlot.set(n!("$shown"), "weaponSwap");
                // L168
                for i in 1..=6 {
                    // L169
                    let abyssal = new_ItemSlotControl(
                        &format!("{slotName} Swap Abyssal Socket {i}"),
                        Some(&format!("Abyssal #{i}")),
                        None,
                    );
                    // L170
                    addSlot(&abyssal);
                    // L171
                    abyssal.set(n!("parentSlot"), swapSlot.clone());
                    // L172
                    abyssal.set(n!("weaponSet"), 2.0);
                    // L173
                    abyssal.set(n!("$shown"), "abyssalSwap");
                    // L176
                    sub_table(&swapSlot, n!("abyssalSocketList")).set(i, abyssal);
                }
            } else if slotName == "Graft 1" || slotName == "Graft 2" {
                // L179
                slot.set(n!("$shown"), "graft");
            } else if slotName == "Ring 3" {
                // L183
                slot.set(n!("$shown"), "ring3");
            }
            // L187
            if matches!(
                slotName,
                "Weapon 1" | "Weapon 2" | "Helmet" | "Gloves" | "Body Armour" | "Boots" | "Belt"
            ) {
                // Add Abyssal Socket slots
                // L189
                for i in 1..=6 {
                    // L190
                    let abyssal = new_ItemSlotControl(
                        &format!("{slotName} Abyssal Socket {i}"),
                        Some(&format!("Abyssal #{i}")),
                        None,
                    );
                    // L191
                    addSlot(&abyssal);
                    // L192
                    abyssal.set(n!("parentSlot"), slot.clone());
                    // L193
                    if pob_lua::pattern::find(slotName, "Weapon", None, false).is_some() {
                        // L194
                        abyssal.set(n!("weaponSet"), 1.0);
                        // L195
                        abyssal.set(n!("$shown"), "abyssalWeapon");
                    }
                    // L199
                    sub_table(&slot, n!("abyssalSocketList")).set(i, abyssal);
                }
            }
        }

        // L203-218 UI: passive tree dropdown controls

        // L220
        self.sockets.with_mut(|m| m.wipe());
        // L221
        let mut socketOrder: Vec<NodeRef> = Vec::new();
        let latestTree = build
            .latestTree
            .get()
            .unwrap_or_else(|| lua_error!("attempt to index field 'latestTree' (a nil value)"));
        // L222
        for (_, node) in latestTree.nodes.pairs() {
            // L223
            if &*node.r#type.get() == "Socket" {
                // L224
                socketOrder.push(node);
            }
        }
        // L227 (ids are unique: a stable sort gives the same order)
        socketOrder.sort_by(|a, b| a.id.partial_cmp(&b.id).unwrap_or(std::cmp::Ordering::Equal));
        // L230
        for node in &socketOrder {
            // L231
            let socketControl = new_ItemSlotControl(
                &format!("Jewel {}", tostring_num(node.id)),
                Some("Socket"),
                Some(node.id),
            );
            // L232
            self.sockets
                .with_mut(|m| m.set(node.id as i64, Some(socketControl.clone())));
            // L233
            addSlot(&socketControl);
        }
        // L235-1203 UI: slot header, drag targets

        // Initialise item sets
        // L1206
        self.itemSets.with_mut(|m| m.clear());
        // L1207
        self.itemSetOrderList.wipe();
        self.itemSetOrderList.push(1);
        // L1208
        self.NewItemSet(Some(1.0));
        // L1209
        self.SetActiveItemSet(build, 1.0);

        // L1211
        self.PopulateSlots(build);
        // L1212
        extra.set(
            n!("lastSlot"),
            self.slots
                .with(|m| m.get(&n!("Flask 5")).cloned())
                .map_or(Val::Nil, Val::Table),
        );
    }

    /// `self.showStatDifferences`.
    pub fn showStatDifferences(&self, build: &BuildRef) -> bool {
        tab_extra(build).truthy(n!("showStatDifferences"))
    }

    /// `self.tradeQuery.statSortSelectionList` (nil until Load).
    pub fn statSortSelectionList(&self, build: &BuildRef) -> Option<Table> {
        tab_extra(build)
            .table(n!("tradeQuery"))
            .and_then(|t| t.table(n!("statSortSelectionList")))
    }
}

/// Breaks the slot reference cycles (`slot.abyssalSocketList[i].parentSlot == slot`; calc doc
/// §3.3 rule 4), which Lua's GC collects.
impl Drop for ItemsTab {
    fn drop(&mut self) {
        let slots: Vec<Table> = self.slots.with(|m| m.values().cloned().collect());
        for slot in slots {
            slot.set(n!("abyssalSocketList"), Val::Nil);
            slot.set(n!("parentSlot"), Val::Nil);
        }
    }
}

/// @pob Classes/ItemsTab.lua:1216-1327 h=00865e77
impl ItemsTab {
    pub fn Load(&self, build: &BuildRef, xml: &crate::model::XmlNode, dbFileName: &str) {
        let _ = dbFileName;
        self.ctor_model(build);
        let extra = tab_extra(build);
        // L1217
        self.activeItemSetId.set(0.0);
        // L1218
        self.itemSets.with_mut(|m| m.clear());
        // L1219
        self.itemSetOrderList.wipe();
        // L1220
        let statSortSelectionList = Table::new();
        sub_table(&extra, n!("tradeQuery"))
            .set(n!("statSortSelectionList"), statSortSelectionList.clone());
        // L1221
        for node in &xml.children {
            // a text child has no `elem` (nil): no branch applies
            let XmlChild::Node(node) = node else { continue };
            // L1222
            if &*node.elem == "Item" {
                // L1223
                let item = new_Item(Some(""), None, false);
                // L1224
                item.id.set(tonum(attr(node, "id")));
                // L1225
                item.variant.set(tonum(attr(node, "variant")));
                // L1226
                if attr(node, "variantAlt").is_some() {
                    // L1227
                    item.extra.set(n!("hasAltVariant"), true);
                    // L1228
                    item.variantAlt.set(tonum(attr(node, "variantAlt")));
                }
                // L1230
                if attr(node, "variantAlt2").is_some() {
                    // L1231
                    item.extra.set(n!("hasAltVariant2"), true);
                    // L1232
                    item.variantAlt2.set(tonum(attr(node, "variantAlt2")));
                }
                // L1234
                if attr(node, "variantAlt3").is_some() {
                    // L1235
                    item.extra.set(n!("hasAltVariant3"), true);
                    // L1236
                    item.variantAlt3.set(tonum(attr(node, "variantAlt3")));
                }
                // L1238
                if attr(node, "variantAlt4").is_some() {
                    // L1239
                    item.extra.set(n!("hasAltVariant4"), true);
                    // L1240
                    item.variantAlt4.set(tonum(attr(node, "variantAlt4")));
                }
                // L1242
                if attr(node, "variantAlt5").is_some() {
                    // L1243
                    item.extra.set(n!("hasAltVariant5"), true);
                    // L1244
                    item.variantAlt5.set(tonum(attr(node, "variantAlt5")));
                }
                // L1246
                for child in &node.children {
                    match child {
                        // L1247
                        XmlChild::Text(child) => {
                            // L1248
                            item.ParseRaw(child, None, false);
                        }
                        // L1249
                        XmlChild::Node(child) if &*child.elem == "ModRange" => {
                            // L1250
                            let mut id = tonum(attr(child, "id")).unwrap_or(0.0);
                            // L1251
                            let range = tonum(attr(child, "range")).unwrap_or(1.0);
                            // This is garbage, but needed due to change to separate mod line lists
                            // 'ModRange' elements are legacy though, so is this actually needed? :<
                            // Maybe it is? Maybe it isn't? Maybe up is down? Maybe good is bad? AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA
                            // Sorry, cluster jewels are making me crazy(-ier)
                            // L1256
                            for list in [
                                item.buffModLines.get(),
                                item.enchantModLines.get(),
                                item.scourgeModLines.get(),
                                item.implicitModLines.get(),
                                item.explicitModLines.get(),
                                item.crucibleModLines.get(),
                            ] {
                                // L1257
                                if id <= list.len() as f64 {
                                    // L1258
                                    let line = if id >= 1.0 && id.fract() == 0.0 {
                                        list.get(id as usize)
                                    } else {
                                        None
                                    };
                                    let Some(line) = line else {
                                        lua_error!("attempt to index field '?' (a nil value)")
                                    };
                                    line.set(n!("range"), range);
                                    // L1259
                                    break;
                                }
                                // L1261
                                id -= list.len() as f64;
                            }
                        }
                        XmlChild::Node(_) => {}
                    }
                }
                // L1265
                if item.base.get().is_some() {
                    // L1266
                    item.BuildModList();
                    // L1267
                    let Some(id) = item.id.get() else {
                        lua_error!("table index is nil")
                    };
                    let k = id_key_or_raise(id, "item");
                    self.items.with_mut(|m| m.insert(k, item.clone()));
                    // L1268
                    self.orderList.push(k);
                }
            // Below is OBE and left for legacy compatibility (all Slots are part of ItemSets now)
            } else if &*node.elem == "Slot" {
                // L1272
                let name = Name::from(attr(node, "name").unwrap_or(""));
                let slot = self.slots.with(|m| m.get(&name).cloned());
                // L1273
                if let Some(slot) = slot {
                    // L1274
                    slot.set(
                        n!("selItemId"),
                        tonum(attr(node, "itemId")).map_or(Val::Nil, Val::Num),
                    );
                    // L1275
                    if let Some(activate) = sub_table(&slot, n!("controls")).table(n!("activate")) {
                        // L1276
                        let active = attr(node, "active") == Some("true");
                        slot.set(n!("active"), active);
                        // L1277
                        activate.set(n!("state"), active);
                    }
                }
            } else if &*node.elem == "ItemSet" {
                // L1281
                let itemSet = self.NewItemSet(tonum(attr(node, "id")));
                // L1282
                itemSet.set(n!("title"), attr(node, "title").map_or(Val::Nil, Val::from));
                // L1283
                itemSet.set(
                    n!("useSecondWeaponSet"),
                    attr(node, "useSecondWeaponSet") == Some("true"),
                );
                // L1284
                for child in &node.children {
                    let XmlChild::Node(child) = child else {
                        continue;
                    };
                    // L1285
                    if &*child.elem == "Slot" {
                        // L1286
                        let slotName = attr(child, "name").unwrap_or("");
                        // L1287
                        let v = itemSet.get(slotName);
                        if v.truthy() {
                            // L1288: a non-table truthy value (a Slot named `title`, `id`,
                            // `useSecondWeaponSet`, ...) cannot take a field store
                            let Some(s) = v.table() else {
                                lua_error!("attempt to index field '?' (a {} value)", v.lua_type())
                            };
                            s.set(
                                n!("selItemId"),
                                tonum(attr(child, "itemId")).map_or(Val::Nil, Val::Num),
                            );
                            // L1289
                            s.set(n!("active"), attr(child, "active") == Some("true"));
                            // L1290
                            s.set(n!("pbURL"), attr(child, "itemPbURL").unwrap_or(""));
                        }
                    } else if &*child.elem == "SocketIdURL" {
                        // L1293
                        let Some(id) = tonum(attr(child, "nodeId")) else {
                            lua_error!("table index is nil")
                        };
                        // L1294
                        let t = Table::new();
                        t.set(n!("pbURL"), attr(child, "itemPbURL").unwrap_or(""));
                        itemSet.set(id, t);
                    }
                }
                // L1297
                let id = itemSet.num(n!("id"));
                self.itemSetOrderList.push(id_key_or_raise(id, "item set"));
            } else if &*node.elem == "TradeSearchWeights" {
                // L1299
                for child in &node.children {
                    let XmlChild::Node(child) = child else {
                        lua_error!("attempt to index field 'attrib' (a nil value)")
                    };
                    // L1300
                    let statSort = Table::new();
                    // L1301
                    statSort.set(
                        n!("label"),
                        attr(child, "label").map_or(Val::Nil, Val::from),
                    );
                    // L1302
                    statSort.set(n!("stat"), attr(child, "stat").map_or(Val::Nil, Val::from));
                    // L1303
                    statSort.set(
                        n!("weightMult"),
                        tonum(attr(child, "weightMult")).map_or(Val::Nil, Val::Num),
                    );
                    // L1305
                    for statEntry in &build.data.powerStatList.stats {
                        // L1306
                        if statSort.str(n!("stat")).as_deref() == statEntry.stat.as_deref() {
                            // update information which can be out of data or missing in the xml
                            // L1308
                            statSort.set(n!("label"), &*statEntry.label);
                            // L1309
                            statSort.set(
                                n!("transform"),
                                statEntry.transform.clone().map_or(Val::Nil, |f| {
                                    Val::Obj(Rc::new(f) as Rc<dyn std::any::Any>)
                                }),
                            );
                            // L1310
                            statSortSelectionList.push(statSort.clone());
                            // L1311
                            break;
                        }
                    }
                }
            }
        }
        // L1317
        if self.itemSetOrderList.get(1).is_none() {
            // L1318
            let s = self.NewItemSet(Some(1.0));
            self.activeItemSet.set(Some(s.clone()));
            // L1319
            s.set(
                n!("useSecondWeaponSet"),
                attr(xml, "useSecondWeaponSet") == Some("true"),
            );
            // L1320
            self.itemSetOrderList.set(1, 1);
        }
        // L1322
        self.SetActiveItemSet(build, tonum(attr(xml, "activeItemSet")).unwrap_or(1.0));
        // L1323
        if let Some(v) = attr(xml, "showStatDifferences") {
            // L1324
            extra.set(n!("showStatDifferences"), v == "true");
        }
        // L1326 UI: self:ResetUndo()
    }
}

/// @pob Classes/ItemsTab.lua:1329-1417 h=8d12a9b2
impl ItemsTab {
    pub fn Save(&self, build: &BuildRef, xml: &mut crate::model::XmlNode) {
        let extra = tab_extra(build);
        // L1330
        xml.attrib = indexmap::IndexMap::new();
        set_attr(
            xml,
            "activeItemSet",
            Some(tostring_num(self.activeItemSetId.get())),
        );
        set_attr(
            xml,
            "useSecondWeaponSet",
            Some(tostring_val(
                &activeItemSet(self).get(n!("useSecondWeaponSet")),
            )),
        );
        set_attr(
            xml,
            "showStatDifferences",
            Some(tostring_val(&extra.get(n!("showStatDifferences")))),
        );
        // L1335
        for id in self.orderList.snapshot() {
            // L1336
            let item = self
                .items
                .with(|m| m.get(&id).cloned())
                .unwrap_or_else(|| lua_error!("attempt to index local 'item' (a nil value)"));
            // L1337
            let mut child = new_xml("Item");
            // L1340
            set_attr(&mut child, "id", Some(tostring_num(f64::from(id))));
            set_attr(&mut child, "variant", item.variant.get().map(tostring_num));
            set_attr(
                &mut child,
                "variantAlt",
                item.variantAlt.get().map(tostring_num),
            );
            set_attr(
                &mut child,
                "variantAlt2",
                item.variantAlt2.get().map(tostring_num),
            );
            set_attr(
                &mut child,
                "variantAlt3",
                item.variantAlt3.get().map(tostring_num),
            );
            set_attr(
                &mut child,
                "variantAlt4",
                item.variantAlt4.get().map(tostring_num),
            );
            set_attr(
                &mut child,
                "variantAlt5",
                item.variantAlt5.get().map(tostring_num),
            );
            // L1350
            item.BuildAndParseRaw();
            // L1351
            child.children.push(XmlChild::Text(item.raw.get()));
            // L1352
            let mut id = item.buffModLines.get().len() as f64 + 1.0;
            // L1353-1382
            for lines in [
                item.enchantModLines.get(),
                item.scourgeModLines.get(),
                item.implicitModLines.get(),
                item.explicitModLines.get(),
                item.crucibleModLines.get(),
            ] {
                for modLine in lines.snapshot() {
                    let range = modLine.get(n!("range"));
                    if range.truthy() {
                        let mut mr = new_xml("ModRange");
                        set_attr(&mut mr, "id", Some(tostring_num(id)));
                        set_attr(&mut mr, "range", Some(tostring_val(&range)));
                        child.children.push(XmlChild::Node(mr));
                    }
                    id += 1.0;
                }
            }
            // L1383
            xml.children.push(XmlChild::Node(child));
        }
        // L1385
        let spec = build_spec(build);
        for itemSetId in self.itemSetOrderList.snapshot() {
            // L1386
            let itemSet = self
                .itemSets
                .with(|m| m.get(&itemSetId).cloned())
                .unwrap_or_else(|| lua_error!("attempt to index local 'itemSet' (a nil value)"));
            // L1387
            let mut child = new_xml("ItemSet");
            set_attr(&mut child, "id", Some(tostring_num(f64::from(itemSetId))));
            set_attr(
                &mut child,
                "title",
                itemSet.str(n!("title")).map(|s| s.to_string()),
            );
            set_attr(
                &mut child,
                "useSecondWeaponSet",
                Some(tostring_val(&itemSet.get(n!("useSecondWeaponSet")))),
            );
            // L1388 (string keys iterate in insertion order; PoB's `pairs` order is random)
            // LUA-ORDER
            let slots: Vec<(Name, Table)> = self
                .slots
                .with(|m| m.iter().map(|(k, v)| (*k, v.clone())).collect());
            for (slotName, slot) in slots {
                // L1389
                let nodeId = slot.get(n!("nodeId"));
                if nodeId.is_nil() {
                    // L1390
                    let s = sub_table(&itemSet, slotName);
                    let mut c = new_xml("Slot");
                    set_attr(&mut c, "name", Some(slotName.to_string()));
                    set_attr(
                        &mut c,
                        "itemId",
                        Some(tostring_val(&s.get(n!("selItemId")))),
                    );
                    let pb = s.get(n!("pbURL"));
                    set_attr(
                        &mut c,
                        "itemPbURL",
                        Some(if pb.truthy() {
                            tostring_val(&pb)
                        } else {
                            String::new()
                        }),
                    );
                    set_attr(
                        &mut c,
                        "active",
                        s.truthy(n!("active")).then(|| "true".to_owned()),
                    );
                    child.children.push(XmlChild::Node(c));
                } else {
                    let nodeId = nodeId.num();
                    // L1392
                    let spec = spec.as_ref().unwrap_or_else(|| {
                        lua_error!("attempt to index field 'spec' (a nil value)")
                    });
                    if spec.allocNodes.with(|m| m.contains_key(nodeId as i64)) {
                        // L1393
                        let mut c = new_xml("SocketIdURL");
                        set_attr(&mut c, "name", Some(slotName.to_string()));
                        set_attr(&mut c, "nodeId", Some(tostring_num(nodeId)));
                        let pb = itemSet
                            .table(nodeId)
                            .map(|t| t.get(n!("pbURL")))
                            .filter(Val::truthy);
                        set_attr(
                            &mut c,
                            "itemPbURL",
                            Some(pb.map_or_else(String::new, |p| tostring_val(&p))),
                        );
                        child.children.push(XmlChild::Node(c));
                    }
                }
            }
            // L1397
            xml.children.push(XmlChild::Node(child));
        }
        // L1399
        if let Some(list) = self.statSortSelectionList(build) {
            // L1400
            let mut parent = new_xml("TradeSearchWeights");
            // L1403
            for (_, statSort) in list.ipairs() {
                let Some(statSort) = statSort.table() else {
                    continue;
                };
                // L1404
                let wm = statSort.get(n!("weightMult"));
                if wm.truthy() && wm.num() > 0.0 {
                    // L1405
                    let mut c = new_xml("Stat");
                    set_attr(
                        &mut c,
                        "label",
                        statSort.str(n!("label")).map(|s| s.to_string()),
                    );
                    set_attr(
                        &mut c,
                        "stat",
                        statSort.str(n!("stat")).map(|s| s.to_string()),
                    );
                    // `s_format("%.2f", tostring(x))`: %f converts the string back
                    let x = pob_lua::tonumber(&tostring_val(&wm)).unwrap_or(f64::NAN);
                    set_attr(&mut c, "weightMult", Some(lfmt!("%.2f", x).to_string()));
                    // L1413
                    parent.children.push(XmlChild::Node(c));
                }
            }
            // L1416
            xml.children.push(XmlChild::Node(parent));
        }
    }
}

// L1419-1570 `ItemsTab:Draw` is UI drawing: U34 ports and marks it in pob-app
// `tabs/items/core.rs` (ui doc U34; FS1: one marker per Lua line).
impl ItemsTab {
    /// UI drawing: the UI port owns it (ui doc U34); nothing in the model.
    pub fn Draw(&self, build: &BuildRef) {
        let _ = build;
    }
}

/// @pob Classes/ItemsTab.lua:1573-1588 h=e9334233
impl ItemsTab {
    pub fn NewItemSet(&self, itemSetId: Option<f64>) -> Table {
        // L1574
        let itemSet = Table::new();
        itemSet.set(n!("id"), itemSetId.map_or(Val::Nil, Val::Num));
        // L1575
        if itemSetId.is_none() {
            // L1576
            let mut id = 1.0;
            // L1577
            while self
                .itemSets
                .with(|m| m.contains_key(&id_key_or_raise(id, "item set")))
            {
                // L1578
                id += 1.0;
            }
            itemSet.set(n!("id"), id);
        }
        // L1581 (string keys: insertion order)
        // LUA-ORDER
        let slots: Vec<(Name, Table)> = self
            .slots
            .with(|m| m.iter().map(|(k, v)| (*k, v.clone())).collect());
        for (slotName, slot) in slots {
            // L1582
            if slot.get(n!("nodeId")).is_nil() {
                // L1583
                let s = Table::new();
                s.set(n!("selItemId"), 0.0);
                itemSet.set(slotName, s);
            }
        }
        // L1586
        let id = itemSet.num(n!("id"));
        self.itemSets
            .with_mut(|m| m.insert(id_key_or_raise(id, "item set"), itemSet.clone()));
        // L1587
        itemSet
    }
}

/// @pob Classes/ItemsTab.lua:1591-1617 h=3a9e0e62
impl ItemsTab {
    pub fn SetActiveItemSet(&self, build: &BuildRef, itemSetId: f64) {
        let mut itemSetId = itemSetId;
        // L1592
        let prevSet = self.activeItemSet.get();
        // L1593
        let has = id_key(itemSetId).is_some_and(|k| self.itemSets.with(|m| m.contains_key(&k)));
        if !has {
            // L1594
            itemSetId = match self.itemSetOrderList.get(1) {
                Some(id) => f64::from(id),
                None => lua_error!("CCR-0080: itemSetOrderList[1] is nil"),
            };
        }
        // L1596
        self.activeItemSetId.set(itemSetId);
        // L1597
        let curSet = id_key(itemSetId).and_then(|k| self.itemSets.with(|m| m.get(&k).cloned()));
        self.activeItemSet.set(curSet.clone());
        // L1598
        let curSet =
            curSet.unwrap_or_else(|| lua_error!("attempt to index local 'curSet' (a nil value)"));
        // L1599 (string keys: insertion order)
        // LUA-ORDER
        let slots: Vec<(Name, Table)> = self
            .slots
            .with(|m| m.iter().map(|(k, v)| (*k, v.clone())).collect());
        for (slotName, slot) in slots {
            // L1600
            if slot.get(n!("nodeId")).is_nil() {
                // L1601
                if let Some(prevSet) = &prevSet {
                    // Update the previous set
                    let p = sub_table(prevSet, slotName);
                    // L1603
                    p.set(n!("selItemId"), slot.get(n!("selItemId")));
                    // L1604
                    p.set(n!("active"), slot.get(n!("active")));
                }
                // Equip the incoming set's item
                let c = sub_table(&curSet, slotName);
                // L1607
                slot.set(n!("selItemId"), c.get(n!("selItemId")));
                // L1608
                slot.set(n!("active"), c.get(n!("active")));
                // L1609
                if let Some(activate) = sub_table(&slot, n!("controls")).table(n!("activate")) {
                    // L1610
                    activate.set(n!("state"), slot.get(n!("active")));
                }
            }
        }
        // L1614
        build.buildFlag.set(true);
        // L1615
        self.PopulateSlots(build);
        // L1616 UI: self.build:SyncLoadouts()
    }
}

/// @pob Classes/ItemsTab.lua:1620-1649 h=8b04062e
impl ItemsTab {
    pub fn EquipItemInSet(&self, build: &BuildRef, item: &ItemRef, itemSetId: f64) {
        let mut item = item.clone();
        // L1621
        let itemSet = id_key(itemSetId)
            .and_then(|k| self.itemSets.with(|m| m.get(&k).cloned()))
            .unwrap_or_else(|| lua_error!("attempt to index local 'itemSet' (a nil value)"));
        // L1622
        let mut slotName = item.GetPrimarySlot().to_string();
        // L1623
        let slot = self
            .slots
            .with(|m| m.get(&Name::from(&*slotName)).cloned())
            .unwrap_or_else(|| lua_error!("attempt to index field '?' (a nil value)"));
        if slot.get(n!("weaponSet")).raw_eq(&Val::Num(1.0))
            && itemSet.truthy(n!("useSecondWeaponSet"))
        {
            // Redirect to second weapon set
            // L1625
            slotName = format!("{slotName} Swap");
        }
        // L1627
        let known = item
            .id
            .get()
            .and_then(id_key)
            .is_some_and(|k| self.items.with(|m| m.contains_key(&k)));
        if !known {
            // L1628
            item = new_Item(Some(&item.raw.get()), None, false);
            // L1629
            self.AddItem(build, &item, true, None);
        }
        // L1631
        let altSlot = pob_lua::pattern::gsub(&slotName, "1", "2", None).0;
        // L1632 UI: IsKeyDown("SHIFT") is false without a keyboard
        // L1638
        let isActive = self.activeItemSet.get().is_some_and(|s| s.ptr_eq(&itemSet));
        let itemId = item.id.get().map_or(Val::Nil, Val::Num);
        if isActive {
            // L1639
            let slot = self
                .slots
                .with(|m| m.get(&Name::from(&*slotName)).cloned())
                .unwrap_or_else(|| lua_error!("attempt to index field '?' (a nil value)"));
            ItemSlot_SetSelItemId(build, &slot, itemId);
        } else {
            // L1641
            sub_table(&itemSet, &*slotName).set(n!("selItemId"), itemId);
            // L1642
            let alt = sub_table(&itemSet, &*altSlot);
            let altSel = alt.get(n!("selItemId"));
            if !altSel.raw_eq(&Val::Num(0.0)) {
                let altItem = item_at(self, &altSel)
                    .unwrap_or_else(|| lua_error!("attempt to index local 'item' (a nil value)"));
                if !self.IsItemValidForSlot(build, &altItem, &altSlot, Some(&itemSet)) {
                    // L1643
                    alt.set(n!("selItemId"), 0.0);
                }
            }
        }
        // L1646
        self.PopulateSlots(build);
        // L1647 UI: self:AddUndoState()
        // L1648
        build.buildFlag.set(true);
    }
}

/// @pob Classes/ItemsTab.lua:1652-1656 h=b3fa8787
impl ItemsTab {
    pub fn PopulateSlots(&self, build: &BuildRef) {
        // L1653 (string keys: insertion order; PoB's order is random per process)
        // LUA-ORDER
        let slots: Vec<Table> = self.slots.with(|m| m.values().cloned().collect());
        for slot in slots {
            // L1654
            ItemSlot_Populate(build, &slot);
        }
    }
}

/// @pob Classes/ItemsTab.lua:1659-1678 h=9311599f
impl ItemsTab {
    pub fn UpdateSockets(&self, build: &BuildRef) {
        // Build a list of active sockets
        // L1661
        let mut activeSocketList: Vec<f64> = Vec::new();
        let spec = build_spec(build)
            .unwrap_or_else(|| lua_error!("attempt to index field 'spec' (a nil value)"));
        // L1662
        for (nodeId, slot) in self.sockets.with(|m| m.pairs()) {
            // L1663
            if spec.allocNodes.with(|m| m.contains_key(nodeId)) {
                // L1664
                activeSocketList.push(nodeId as f64);
                // L1665
                slot.set(n!("inactive"), false);
            } else {
                // L1667
                slot.set(n!("inactive"), true);
            }
        }
        // L1670 (distinct node ids)
        activeSocketList.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));

        // Update the state of the active socket controls
        let extra = tab_extra(build);
        // L1673
        extra.set(
            n!("lastSlot"),
            self.slots
                .with(|m| m.get(&Name::from(baseSlots[baseSlots.len() - 1])).cloned())
                .map_or(Val::Nil, Val::Table),
        );
        // L1674
        for (index, nodeId) in activeSocketList.iter().enumerate() {
            let socket = self
                .sockets
                .with(|m| m.get(*nodeId as i64))
                .expect("listed above");
            // L1675
            socket.set(n!("label"), format!("Socket #{}", index + 1));
            // L1676
            extra.set(n!("lastSlot"), socket);
        }
    }
}

/// @pob Classes/ItemsTab.lua:1681-1683 h=5d7fe975
impl ItemsTab {
    pub fn GetSocketAndJewelForNodeID(&self, nodeId: f64) -> (Option<Table>, Option<ItemRef>) {
        // L1682
        let socket = if nodeId.fract() == 0.0 {
            self.sockets.with(|m| m.get(nodeId as i64))
        } else {
            None
        };
        let Some(socket) = socket else {
            lua_error!("attempt to index field '?' (a nil value)")
        };
        let item = item_at(self, &socket.get(n!("selItemId")));
        (Some(socket), item)
    }
}

/// @pob Classes/ItemsTab.lua:1686-1724 h=d04c87d8
impl ItemsTab {
    pub fn AddItem(
        &self,
        build: &BuildRef,
        item: &ItemRef,
        noAutoEquip: bool,
        index: Option<usize>,
    ) {
        // L1687
        if item.id.get().is_none() {
            // Find an unused item ID
            // L1689
            let mut id = 1.0;
            // L1690
            while self
                .items
                .with(|m| m.contains_key(&id_key_or_raise(id, "item")))
            {
                // L1691
                id += 1.0;
            }
            item.id.set(Some(id));
            let k = id_key_or_raise(id, "item");

            // L1694
            if let Some(index) = index {
                // L1695
                self.orderList.insert(index, k);
            } else {
                // Add it to the end of the display order list
                // L1698
                self.orderList.push(k);
            }

            // L1701
            if !noAutoEquip {
                // Autoequip it
                // L1703
                for slot in self.orderedSlots.snapshot() {
                    let slotName = slot.str(n!("slotName")).unwrap_or_else(|| LStr::from(""));
                    // L1704
                    if slot.get(n!("nodeId")).is_nil()
                        && slot.get(n!("selItemId")).raw_eq(&Val::Num(0.0))
                        && slot_IsShown(build, &slot)
                        && self.IsItemValidForSlot(build, item, &slotName, None)
                    {
                        // L1705
                        ItemSlot_SetSelItemId(build, &slot, Val::Num(id));
                        // L1706
                        break;
                    }
                }
            }
        }

        // Add it to the list
        let id = item.id.get().expect("set above");
        let k = id_key_or_raise(id, "item");
        // L1713
        let replacing = self.items.with(|m| m.get(&k).cloned());
        // L1714
        self.items.with_mut(|m| m.insert(k, item.clone()));
        // L1715
        item.BuildModList();

        // L1717
        if let Some(replacing) = replacing {
            if replacing.clusterJewel.get().is_some()
                || item.clusterJewel.get().is_some()
                || replacing.baseName.get().as_deref() == Some("Timeless Jewel")
            {
                // We're replacing an existing item, and either the new or old one is a cluster jewel
                let spec = build_spec(build)
                    .unwrap_or_else(|| lua_error!("attempt to index field 'spec' (a nil value)"));
                // L1719
                if spec
                    .jewels
                    .with(|m| m.pairs().iter().any(|(_, v)| *v == id))
                {
                    // Item is currently equipped, so we need to rebuild the graphs
                    // L1721
                    spec.BuildClusterJewelGraphs();
                }
            }
        }
    }
}

/// @pob Classes/ItemsTab.lua:2998-3005 h=28941651
impl ItemsTab {
    pub fn getMissingAnointCount(&self, item: &ItemRef) -> f64 {
        // L2999
        if !isAnointable(Some(item)) {
            // L3000
            return 0.0;
        }
        // L3002
        let maxAnoints = if item.extra.truthy(n!("canHaveFourEnchants")) {
            4.0
        } else if item.extra.truthy(n!("canHaveThreeEnchants")) {
            3.0
        } else if item.extra.truthy(n!("canHaveTwoEnchants")) {
            2.0
        } else {
            1.0
        };
        // L3003
        let anointCount = self.getAnoint(Some(item)).len() as f64;
        // L3004
        m_max(0.0, maxAnoints - m_min(anointCount, maxAnoints))
    }
}
