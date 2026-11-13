//! Port of PathOfBuilding v2.67.2 (16de4b82d) src/Classes/ItemSlotControl.lua 1-186
//! (`newClass("ItemSlotControl", "DropDownControl")`). Owner: U32.
//!
//! @pob-file Classes/ItemSlotControl.lua @16de4b82d
//!
//! The constructor, `SetSelItemId` and `Populate` (L21-123) interleave model and UI statements.
//! Their model half is calc:BM-A's (`model::items_tab`, CCR-0080 §1): one [`model::Table`] per
//! slot built by `ItemsTab::ctor_model` (`slotName`, `slotNum`, `label`, `nodeId`, `weaponSet`,
//! `selItemId`, `active`, `inactive`, `items`, `abyssalSocketList`, `parentSlot`,
//! `controls.activate.state`). The control wraps that table ([`ItemSlotControl::slot`]): every
//! field the Lua reads on `self` that the model owns is read from it, so the model's writes
//! (`ItemsTab:Load`, `SetActiveItemSet`, `UpdateSockets`' labels) are what the control draws.
//! U33 creates the controls over the tables of `build.itemsTab.slots` (ItemsTab.lua:154-233)
//! and sets the `shown` overrides of ItemsTab.lua:158-208 as mixins.
//!
//! Calls into the ItemsTab UI go through [`ItemsTabHooks`](super::slot_helper::ItemsTabHooks).
//!
//! DIVERGENCE(populate-split): `itemsTab:PopulateSlots()` runs every slot's `Populate` (model
//! and list, slot by slot, in `pairs(self.slots)` order). The port runs calc:BM-A's model
//! `PopulateSlots` (all slots, insertion order) and then [`ItemSlotControl::populate_list`] for
//! every slot control. The list differs only for a slot whose `selItemId` a later slot changes
//! in the same pass: an abyssal socket that its parent turns inactive (L120) and that was
//! populated before the parent. In insertion order every parent comes before its sockets
//! (ItemsTab.lua:154-199), so the lists are the ones the Lua builds in that order; an inactive
//! socket is hidden (`shown`, L34-36) and repopulated before it is shown again.
//!
//! DIVERGENCE(activate-state): the flask checkbox is `self.controls.activate`, which the model
//! writes as the table `slot.controls.activate` (`.state`, ItemsTab.lua:1277/1610). The
//! CheckBoxControl keeps its own `state`; [`ItemSlotControl::sync_activate`] copies the model's
//! value into it before the slot draws and before it handles a key, and its `changeFunc`
//! writes the new state back. A model write between this slot's last draw and a click on the
//! checkbox in the same frame is not seen by that click (no PoB path does both).
//!
//! CCR-0107 §2: `ItemSlot_SetSelItemId` is private in calc:BM-A's file; [`set_sel_item_id`] is
//! a local copy of its body (ItemSlotControl.lua:72-84) until it is public.
//!
//! ORDER: L96 `pairs(self.itemsTab.items)` is calc:BM-A's model loop (insertion order of the
//! item map, LUA-ORDER there); the list is built from its result `slot.items`, in that order.
//!
//! Props with side effects (ui doc §7.3): none.
#![allow(non_snake_case)]

use std::rc::Rc;

use pob_lua::n;
use pob_lua::pattern;
use pob_sg::{Align, Font};
use pob_ui_core::controls::drop_down::{DropDownControl, DropItem, DropTipMode};
use pob_ui_core::controls::list::{other_drag_source, ListControl};
use pob_ui_core::controls::CheckBoxControl;
use pob_ui_core::prelude::*;
use pob_ui_core::tooltip::tooltip::{TtKey, TtObj};

use super::slot_helper::{items_hooks, DrawViewer};
use crate::app::CxApp;
use crate::model::{self, Table, Val};

// @pob-skip Classes/ItemSlotControl.lua:1-20 reason="header comment, local aliases, require of ItemSlotHelper (super::slot_helper) and newClass (the struct and Widget impl below)"

/// `ItemSlotControl`: a DropDownControl over the items valid for one slot.
pub struct ItemSlotControl {
    pub dd: DropDownControl,
    /// `self.itemsTab` (the ItemsTab node)
    pub items_tab: CtrlId,
    /// The model half of `self` (calc:BM-A's slot table, see the module docs).
    pub slot: Table,
    /// `self.labelOffset`
    pub label_offset: f64,
}

/// `itemsTab.items[id]` (a nil or non-integral id is nil).
pub fn item_by_id(cx: &Cx, id: &Val) -> Option<model::ItemRef> {
    let id = id.opt_num()?;
    let k = model::items_tab::id_key(id)?;
    cx.build().model.itemsTab.items.with(|m| m.get(&k).cloned())
}

/// The model `ItemRef` of a drag payload (`"Item", item`).
pub fn drag_item(v: &DragValue) -> model::ItemRef {
    v.0.clone()
        .downcast::<model::Item>()
        .unwrap_or_else(|_| lua_error!("ItemSlotControl: a drag value that is not an item"))
}

/// `colorCodes[item.rarity] .. item.name` (a missing colour is Lua's concatenation error).
pub fn item_label(cx: &Cx, item: &model::Item) -> String {
    let rarity = item.rarity.get();
    let Some(cc) = cx.main.cc.get(&rarity) else {
        lua_error!("attempt to concatenate field '?' (a nil value)")
    };
    format!("{cc}{}", item.name.get())
}

/// lua: ItemSlotControl.lua:72-84 `ItemSlotClass:SetSelItemId(selItemId)` on the slot's model
/// table (model part: calc:BM-A `ItemSlot_SetSelItemId`, private; CCR-0107 §2 local copy).
pub fn set_sel_item_id(build: &model::BuildRef, slot: &Table, sel_item_id: f64) {
    // L73
    let node_id = slot.get(n!("nodeId"));
    if let Some(node_id) = node_id.opt_num() {
        // L74 (`self.itemsTab.build.spec` exists once the TreeTab is constructed, before any
        // ItemsTab UI)
        {
            let spec = build.spec.get();
            // L75
            spec.jewels
                .with_mut(|m| m.set(node_id as i64, Some(sel_item_id)));
            // L76
            if !Val::Num(sel_item_id).raw_eq(&slot.get(n!("selItemId"))) {
                // L77
                spec.BuildClusterJewelGraphs();
            }
        }
    } else {
        // L81
        let slot_name = slot.str(n!("slotName")).unwrap_or_else(|| "".into());
        let set =
            build.itemsTab.activeItemSet.get().unwrap_or_else(|| {
                lua_error!("attempt to index field 'activeItemSet' (a nil value)")
            });
        let entry = set
            .table(&*slot_name)
            .unwrap_or_else(|| lua_error!("attempt to index field '?' (a nil value)"));
        entry.set(n!("selItemId"), sel_item_id);
    }
    // L83
    slot.set(n!("selItemId"), sel_item_id);
}

/// `itemLib.wiki.openItem(item)` (ItemTools.lua:399-410: `openItem` and `open`; calc:IT-B
/// skips them as UI and no UI unit row names them; CCR-0107 §4).
pub fn wiki_open_item(cx: &mut Cx, item: &model::Item) {
    // ItemTools.lua:400
    let rarity = item.rarity.get();
    let name: Option<String> = if &*rarity == "UNIQUE" || &*rarity == "RELIC" {
        item.title.get().map(|s| s.to_string())
    } else {
        None
    }
    .or_else(|| item.baseName.get().map(|s| s.to_string()));
    let Some(name) = name else {
        lua_error!("bad argument #1 to 'gsub' (string expected, got nil)")
    };
    // ItemTools.lua:406
    let route = pattern::gsub(&name, " ", "_", None).0;
    // ItemTools.lua:407
    cx.sys
        .platform
        .open_url(&format!("https://www.poewiki.net/wiki/{route}"));
    // ItemTools.lua:408
    cx.app_mut().main.item_lib_wiki_triggered = true;
}

/// `itemLib.wiki.matchesKey(key)` (ItemTools.lua:411-413: `key == itemLib.wiki.key`, "F1").
pub fn wiki_matches_key(key: Key) -> bool {
    key == Key::F(1)
}

impl ItemSlotControl {
    /// lua: ItemSlotControl.lua:21-70 `ItemSlotClass:ItemSlotControl(anchor, x, y, itemsTab,
    /// slotName, slotLabel, nodeId)` (model part: calc:BM-A `new_ItemSlotControl`, which built
    /// `slot` from `slotName`, `slotLabel` and `nodeId`).
    pub fn new(
        anchor: Anchor,
        x: Prop<f64>,
        y: Prop<f64>,
        items_tab: CtrlId,
        slot: Table,
    ) -> New<ItemSlotControl> {
        let slot_name = slot
            .str(n!("slotName"))
            .unwrap_or_else(|| lua_error!("ItemSlotControl: a slot table without slotName"));
        // L42
        let is_flask = pattern::find(&slot_name, "Flask", None, false).is_some();
        // L22 self:DropDownControl(anchor, { x, y, 310, 20 }, {}, function(index, value) .. end)
        let mut base = Base::new(anchor, [x, y, Prop::Val(310.0), Prop::Val(20.0)]);
        // L30
        base.anchor.collapse = true;
        let slot_t = slot.clone();
        // L31-33
        base.enabled = Prop::f(move |_cx, _| {
            // L32
            slot_t.table(n!("items")).map_or(0, |t| t.len()) > 1
        });
        let slot_t = slot.clone();
        // L34-36
        base.shown = Prop::f(move |_cx, _| {
            // L35
            !slot_t.truthy(n!("inactive"))
        });
        New::new(
            base,
            ItemSlotControl {
                dd: DropDownControl::state(Vec::new(), None, None, false),
                // L37 (model part: calc:BM-A)
                items_tab,
                slot,
                // L53 / L55
                label_offset: if is_flask { -24.0 } else { -2.0 },
            },
        )
        .host()
        .init(|cx, me| DropDownControl::init(cx, me.0))
        .init(move |cx, me| {
            let me = me.0;
            // L22-29 the selFunc
            cx.w_mut::<DropDownControl>(me).sel_func = Some(Rc::new(move |cx, index, _value| {
                let slot = cx.w::<ItemSlotControl>(me).slot.clone();
                let id = slot
                    .table(n!("items"))
                    .map_or(Val::Nil, |t| t.get(index as i64));
                // L23
                if !id.raw_eq(&slot.get(n!("selItemId"))) {
                    // L24
                    let m = cx.build().model.clone();
                    set_sel_item_id(&m, &slot, id.num());
                    let h = items_hooks();
                    // L25
                    (h.populate_slots)(cx);
                    // L26
                    (h.add_undo_state)(cx);
                    // L27
                    cx.build().model.buildFlag.set(true);
                }
            }));
            // L37-41 (model part: calc:BM-A: itemsTab, items, selItemId, slotName, slotNum)
            // L42
            if is_flask {
                // L43-48
                let activate = cx.add(
                    me,
                    "activate",
                    CheckBoxControl::new(
                        Anchor::new(Right, me, Left),
                        rect(-2, 0, 20, 0),
                        None,
                        Some(Rc::new(move |cx, state| {
                            let slot = cx.w::<ItemSlotControl>(me).slot.clone();
                            // DIVERGENCE(activate-state): the model's copy of the checkbox
                            if let Some(a) = slot
                                .table(n!("controls"))
                                .and_then(|c| c.table(n!("activate")))
                            {
                                a.set(n!("state"), state);
                            }
                            // L44
                            slot.set(n!("active"), state);
                            // L45
                            let slot_name = slot.str(n!("slotName")).unwrap_or_else(|| "".into());
                            let set = cx
                                .build()
                                .model
                                .itemsTab
                                .activeItemSet
                                .get()
                                .unwrap_or_else(|| {
                                    lua_error!(
                                        "attempt to index field 'activeItemSet' (a nil value)"
                                    )
                                });
                            set.table(&*slot_name)
                                .unwrap_or_else(|| {
                                    lua_error!("attempt to index field '?' (a nil value)")
                                })
                                .set(n!("active"), state);
                            // L46
                            let f = items_hooks().add_undo_state;
                            f(cx);
                            // L47
                            cx.build().model.buildFlag.set(true);
                        })),
                        None,
                        false,
                    ),
                );
                // L49-51
                let slot = cx.w::<ItemSlotControl>(me).slot.clone();
                cx.base_mut(activate).enabled = Prop::f(move |_cx, _| {
                    // L50
                    !slot.get(n!("selItemId")).raw_eq(&Val::Num(0.0))
                });
                // L52
                cx.get_mut(activate).tt.tooltip_text =
                    Some(Prop::Val(Some("Activate this flask.".to_owned())));
                // L53 (labelOffset: state)
            }
            // L57 (model part: calc:BM-A)
            // L58-66
            cx.w_mut::<DropDownControl>(me).tt.tooltip_func =
                Some(Rc::new(move |cx, tooltip, (mode, index, _value)| {
                    let (items_tab, slot) = {
                        let w = cx.w::<ItemSlotControl>(me);
                        (w.items_tab, w.slot.clone())
                    };
                    // L59
                    let item = index
                        .and_then(|i| slot.table(n!("items")).map(|t| t.get(i as i64)))
                        .and_then(|id| item_by_id(cx, &id));
                    // not selControl.ListControl allows hover when All Items or Unique/Rare DB Sections are in focus
                    // L61
                    let sel = cx.host(items_tab).sel_control;
                    let activate = cx.child(me, "activate");
                    let blocked = !cx.w::<DropDownControl>(me).dropped
                        && sel.is_some_and(|s| Some(s) != activate && !cx.is::<ListControl>(s));
                    match item {
                        Some(item)
                            if cx.main.popups.is_empty()
                                && mode != DropTipMode::Out
                                && !blocked =>
                        {
                            // L63
                            let rev = cx.build().model.outputRevision.get();
                            let shift = cx.sys.is_key_down(Key::Shift);
                            let alt = cx.app().launch.dev_mode_alt;
                            if tooltip.check_for_update(
                                cx,
                                &[
                                    TtKey::Obj(TtObj(item.clone())),
                                    TtKey::Bool(alt),
                                    TtKey::Num(rev),
                                    TtKey::Bool(shift),
                                ],
                            ) {
                                // L64
                                let f = items_hooks().add_item_tooltip;
                                let b = cx.build().model.clone();
                                f(cx, &b, tooltip, &item, Some(&slot), false);
                            }
                        }
                        // L62
                        _ => tooltip.clear(true),
                    }
                }));
            // L67-68 (model part: calc:BM-A: label, nodeId)
            // L69 return self
        })
    }

    /// DIVERGENCE(activate-state): `self.controls.activate.state` from the model's copy.
    pub fn sync_activate(cx: &mut Cx, me: CtrlId) {
        let Some(activate) = cx.child(me, "activate") else {
            return;
        };
        let state = cx
            .w::<ItemSlotControl>(me)
            .slot
            .table(n!("controls"))
            .and_then(|c| c.table(n!("activate")))
            .is_some_and(|a| a.truthy(n!("state")));
        cx.w_mut::<CheckBoxControl>(activate).state = state;
    }

    /// lua: ItemSlotControl.lua:86-123 `ItemSlotClass:Populate()`, the UI part (L92-104: the
    /// dropdown list and `selIndex`); the model part (L87-91, L93, L96-98, L105-122) is
    /// calc:BM-A's `PopulateSlots`, which has run (DIVERGENCE(populate-split)).
    pub fn populate_list(cx: &mut Cx, me: CtrlId) {
        let slot = cx.w::<ItemSlotControl>(me).slot.clone();
        let sel_item_id = slot.get(n!("selItemId"));
        let ids: Vec<Val> = slot.table(n!("items")).map_or_else(Vec::new, |t| {
            t.ipairs().into_iter().map(|(_, v)| v).collect()
        });
        // L92 wipeTable(self.list)
        let mut list: Vec<DropItem> = Vec::new();
        // L94
        list.push(DropItem::str("None"));
        // L95
        let mut sel_index = 1;
        // L96-104 (the ids after `items[1] = 0`, L93)
        for id in ids.iter().skip(1) {
            let item = item_by_id(cx, id)
                .unwrap_or_else(|| lua_error!("attempt to index local 'item' (a nil value)"));
            // L99
            list.push(DropItem::str(item_label(cx, &item)));
            // L100
            if item
                .id
                .get()
                .map_or(Val::Nil, Val::Num)
                .raw_eq(&sel_item_id)
            {
                // L101
                sel_index = list.len();
            }
        }
        let w = cx.w_mut::<DropDownControl>(me);
        w.list = list;
        w.sel_index = sel_index;
    }

    /// The node id of a jewel socket slot (`self.nodeId`).
    fn node_id(cx: &Cx, me: CtrlId) -> Option<f64> {
        cx.w::<ItemSlotControl>(me).slot.get(n!("nodeId")).opt_num()
    }
}

impl Widget for ItemSlotControl {
    const NAME: &'static str = "ItemSlotControl";
    const CAPS: Caps = <DropDownControl as Widget>::CAPS
        .union(Caps::CAN_RECEIVE_DRAG)
        .union(Caps::RECEIVE_DRAG)
        .union(Caps::HOVER_KEY_UP);

    fn is_mouse_over(cx: &Cx, me: CtrlId) -> bool {
        DropDownControl::is_mouse_over_comp(cx, me).0
    }

    /// lua: ItemSlotControl.lua:125-127 `ItemSlotClass:CanReceiveDrag(type, value)`
    /// @pob Classes/ItemSlotControl.lua:125-127 h=951acc24
    fn can_receive_drag(cx: &Cx, me: CtrlId, ty: &str, v: &DragValue) -> bool {
        // L126
        if ty != "Item" {
            return false;
        }
        let slot_name = cx
            .w::<ItemSlotControl>(me)
            .slot
            .str(n!("slotName"))
            .unwrap_or_else(|| "".into());
        let m = cx.build().model.clone();
        m.itemsTab
            .IsItemValidForSlot(&m, &drag_item(v), &slot_name, None)
    }

    /// lua: ItemSlotControl.lua:129-142 `ItemSlotClass:ReceiveDrag(type, value, source)`
    /// @pob Classes/ItemSlotControl.lua:129-142 h=ea4934aa
    fn receive_drag(cx: &mut Cx, me: CtrlId, _ty: &str, v: &DragValue, _source: CtrlId) {
        let value = drag_item(v);
        let slot = cx.w::<ItemSlotControl>(me).slot.clone();
        let m = cx.build().model.clone();
        let h = items_hooks();
        // L130
        let id = value.id.get();
        if let Some(id) = id.filter(|id| item_by_id(cx, &Val::Num(*id)).is_some()) {
            // L131
            set_sel_item_id(&m, &slot, id);
        } else {
            // L133
            let new_item = crate::main_mode::new_item(Some(&value.raw.get()), None, false);
            // L134
            new_item.NormaliseQuality();
            // L135
            (h.add_item)(cx, &new_item, true, None);
            // L136
            let new_id = new_item.id.get().unwrap_or_else(|| {
                lua_error!("ItemSlotControl: AddItem left the item without an id")
            });
            set_sel_item_id(&m, &slot, new_id);
            // L137
            (h.add_forbidden_jewel_counterpart)(cx, &new_item);
        }
        // L139
        (h.populate_slots)(cx);
        // L140
        (h.add_undo_state)(cx);
        // L141
        m.buildFlag.set(true);
    }

    /// lua: ItemSlotControl.lua:144-161 `ItemSlotClass:Draw(viewPort)`
    /// @pob Classes/ItemSlotControl.lua:144-161 h=bfee3763
    fn draw(cx: &mut Cx, me: CtrlId, vp: &ViewPort, _no_tooltip: bool) {
        // L145
        let (x, y) = cx.pos(me);
        // L146
        let (_width, height) = cx.size(me);
        // L147
        let (label_offset, label) = {
            let w = cx.w::<ItemSlotControl>(me);
            let label = w.slot.get(n!("label"));
            let Some(label) = label.str() else {
                lua_error!(
                    "attempt to concatenate field 'label' (a {} value)",
                    label.lua_type()
                )
            };
            (w.label_offset, label)
        };
        cx.sg.draw_string(
            x + label_offset,
            y + 2.0,
            Align::RightX,
            height - 4.0,
            Font::Var,
            &format!("^7{label}:"),
        );
        // L148
        DropDownControl::draw(cx, me, vp, false);
        // L149
        Self::sync_activate(cx, me);
        cx.draw_controls(me, vp, None);
        // L150
        let node_id = Self::node_id(cx, me);
        if cx.main.popups.is_empty() {
            if let Some(node_id) = node_id {
                let dropped = cx.w::<DropDownControl>(me).dropped;
                let show = dropped || {
                    cx.is_mouse_over(me) && {
                        let items_tab = cx.w::<ItemSlotControl>(me).items_tab;
                        other_drag_source(cx, me).is_some()
                            || cx.host(items_tab).sel_control.is_none()
                    }
                };
                if show {
                    // L151
                    let width = 308.0;
                    // L152
                    let height = 280.0;
                    // L153
                    let (drop_up, dropped) = {
                        let w = cx.w::<DropDownControl>(me);
                        (w.drop_up, w.dropped)
                    };
                    // L154
                    let viewer_y = if drop_up && dropped {
                        // L155
                        y + 20.0
                    } else {
                        // L157
                        pob_lua::math::m_min(y - height - 4.0, vp.y + vp.height - height)
                    };
                    // L159
                    DrawViewer(cx, node_id, x, viewer_y, width, height);
                }
            }
        }
    }

    /// lua: ItemSlotControl.lua:163-172 `ItemSlotClass:OnKeyDown(key)`
    /// @pob Classes/ItemSlotControl.lua:163-172 h=9a2dc2ed
    fn on_key_down(cx: &mut Cx, me: CtrlId, key: Key, _double_click: bool) -> Focus {
        // L164
        if !cx.is_shown(me) || !cx.is_enabled(me) {
            // L165
            return None;
        }
        // L167
        let m_over_control = cx.get_mouse_over_control(me);
        // L168
        let activate = cx.child(me, "activate");
        if let Some(c) = m_over_control.filter(|c| Some(*c) == activate) {
            // L169 (OnKeyDown(key): no doubleClick argument)
            Self::sync_activate(cx, me);
            return cx.on_key_down(c, key, false);
        }
        // L171
        DropDownControl::on_key_down(cx, me, key)
    }

    fn on_key_up(cx: &mut Cx, me: CtrlId, key: Key) -> Focus {
        DropDownControl::on_key_up(cx, me, key)
    }

    fn on_char(cx: &mut Cx, me: CtrlId, ch: Key) -> Focus {
        DropDownControl::on_char(cx, me, ch)
    }

    /// lua: ItemSlotControl.lua:174-186 `ItemSlotClass:OnHoverKeyUp(key)`
    /// @pob Classes/ItemSlotControl.lua:174-186 h=1703cb10
    fn on_hover_key_up(cx: &mut Cx, me: CtrlId, key: Key) {
        // L175
        if wiki_matches_key(key) {
            // L176
            let index = DropDownControl::get_hover_index(cx, me);
            // L177 (always a number: `self.hoverSel or self.selIndex`)
            // L178
            let item_index = cx
                .w::<ItemSlotControl>(me)
                .slot
                .table(n!("items"))
                .map_or(Val::Nil, |t| t.get(index as i64));
            // L179
            let item = item_by_id(cx, &item_index);
            // L181
            if let Some(item) = item {
                // L182
                wiki_open_item(cx, &item);
            }
        }
    }

    pob_ui_core::impl_parts!(ItemSlotControl: dd => DropDownControl);
}
