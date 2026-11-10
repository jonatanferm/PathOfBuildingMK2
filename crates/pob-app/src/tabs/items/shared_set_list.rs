//! Port of PathOfBuilding v2.67.2 (16de4b82d) src/Classes/SharedItemSetListControl.lua 1-113
//! (`newClass("SharedItemSetListControl", "ListControl")`). Owner: U32.
//!
//! @pob-file Classes/SharedItemSetListControl.lua @16de4b82d
//!
//! The list is `main.sharedItemSetList` itself (L15; U16's `MainState::shared_item_set_list`).
//! Its entries are Lua tables compared by identity (`selValue`, the drag payload), but U16 holds
//! them by value (`Vec<SharedItemSet>`). CCR-0107 §5 asks for `Rc` entries; until then
//! [`SharedSetsSource`] gives every entry an identity token kept in a per-thread vector parallel
//! to the main list (rebuilt fresh when its length differs, i.e. after U16's `LoadSettings`
//! replaced the list; every insert/remove of this file keeps both in step). // CCR-0107
//!
//! ListControl hooks: `GetRowValue`, `AddValueTooltip`, `GetDragValue`, `OnSelDelete`,
//! `OnSelKeyDown`; `CanReceiveDrag`/`ReceiveDrag` are the framework's drag-target methods.
//!
//! LUA-QUIRK(drag-index): ReceiveDrag inserts at `self.dragIndex or #self.list + 1` (L97):
//! `dragIndex` is never set (ListControl sets `selDragIndex`), so a dropped set is always
//! appended. Kept.
//!
//! ORDER: L81 `pairs(self.itemsTab.slots)`: fills the new set's string-keyed `slots`; only the
//! settings file's element order depends on it, which U16's saver writes in insertion order
//! (LUA-ORDER there) (c).
//!
//! Props with side effects (ui doc §7.3): none.
#![allow(non_snake_case)]

use std::cell::RefCell;
use std::rc::Rc;

use pob_lua::{n, pattern};
use pob_ui_core::controls::list::{ListControl, ListData, ListHooks, ListSource, ListValue};
use pob_ui_core::controls::{ButtonControl, EditArgs, EditControl, LabelControl};
use pob_ui_core::popups::{self, PopupOpts};
use pob_ui_core::prelude::*;
use pob_ui_core::tooltip::Tooltip;

use super::set_list::drag_set;
use super::slot::{item_by_id, item_label};
use super::slot_helper::items_hooks;
use crate::app::{app, app_mut, CxApp};
use crate::main_mode::SharedItemSet;
use crate::model::{self, Table, Val};

// @pob-skip Classes/SharedItemSetListControl.lua:1-12 reason="header comment, local aliases and newClass (the struct and Widget impl below)"

/// The identity of one `main.sharedItemSetList` entry (CCR-0107 §5).
pub struct SharedSetToken;

thread_local! {
    // CCR-0107 §5: parallel to `main.sharedItemSetList`.
    static TOKENS: RefCell<Vec<Rc<SharedSetToken>>> = const { RefCell::new(Vec::new()) };
}

/// The tokens, resynchronised with the main list's length.
fn tokens(cx: &Cx) -> Vec<Rc<SharedSetToken>> {
    let n = app(&*cx.ext).main.shared_item_set_list.len();
    TOKENS.with(|t| {
        let mut t = t.borrow_mut();
        if t.len() != n {
            *t = (0..n).map(|_| Rc::new(SharedSetToken)).collect();
        }
        t.clone()
    })
}

/// The 1-based position of a token's entry.
fn position(cx: &Cx, v: &ListValue) -> Option<usize> {
    let tok = v.rc::<SharedSetToken>()?;
    tokens(cx)
        .iter()
        .position(|t| Rc::ptr_eq(t, &tok))
        .map(|i| i + 1)
}

/// The entry a row value names (a copy; writes go through [`with_set_mut`]).
fn set_of(cx: &Cx, v: &ListValue) -> Option<SharedItemSet> {
    let i = position(cx, v)?;
    app(&*cx.ext).main.shared_item_set_list.get(i - 1).cloned()
}

/// Write to the entry a row value names.
fn with_set_mut(cx: &mut Cx, v: &ListValue, f: impl FnOnce(&mut SharedItemSet)) {
    let Some(i) = position(cx, v) else {
        lua_error!("attempt to index local 'sharedItemSet' (a nil value)")
    };
    f(&mut app_mut(&mut *cx.ext).main.shared_item_set_list[i - 1]);
}

/// The shared item set a `"SharedItemList"` drag payload names (ItemSetListControl.lua:97-110).
pub fn shared_set_of(cx: &Cx, v: &DragValue) -> Option<SharedItemSet> {
    let tok = v.0.clone().downcast::<SharedSetToken>().ok()?;
    let lv = ListValue::of(tok);
    set_of(cx, &lv)
}

/// `self.list = main.sharedItemSetList` (aliased).
pub struct SharedSetsSource;

impl ListSource for SharedSetsSource {
    fn len(&self, cx: &Cx) -> usize {
        app(&*cx.ext).main.shared_item_set_list.len()
    }
    fn get(&self, cx: &Cx, i: usize) -> Option<ListValue> {
        let t = tokens(cx);
        (i >= 1)
            .then(|| t.get(i - 1))
            .flatten()
            .cloned()
            .map(ListValue::of)
    }
    fn insert(&self, cx: &mut Cx, i: usize, v: ListValue) {
        // a reordered entry: its token and its set move together
        let set = set_of(cx, &v);
        let tok = v.rc::<SharedSetToken>().unwrap_or_else(|| {
            lua_error!("SharedSetsSource::insert: a value that is not a shared set")
        });
        let set = set.or_else(|| PENDING.with(|p| p.borrow_mut().take()));
        let Some(set) = set else {
            lua_error!("SharedSetsSource::insert: an unknown shared set")
        };
        insert_entry(cx, i, tok, set);
    }
    fn remove(&self, cx: &mut Cx, i: usize) -> ListValue {
        let n = self.len(cx);
        if i == 0 || i > n {
            lua_error!("t_remove: position out of range");
        }
        let _ = tokens(cx);
        let set = app_mut(&mut *cx.ext)
            .main
            .shared_item_set_list
            .remove(i - 1);
        let tok = TOKENS.with(|t| t.borrow_mut().remove(i - 1));
        // a removed entry that ListControl re-inserts (a reorder) brings its set back
        PENDING.with(|p| *p.borrow_mut() = Some(set));
        ListValue::of(tok)
    }
}

thread_local! {
    /// The set of the entry `remove` took out last (ListControl's reorder is a remove then an
    /// insert of the same value).
    static PENDING: RefCell<Option<SharedItemSet>> = const { RefCell::new(None) };
}

/// `t_insert(main.sharedItemSetList, i, set)` with its token.
fn insert_entry(cx: &mut Cx, i: usize, tok: Rc<SharedSetToken>, set: SharedItemSet) {
    let _ = tokens(cx);
    let l = &mut app_mut(&mut *cx.ext).main.shared_item_set_list;
    if i == 0 || i > l.len() + 1 {
        lua_error!("bad argument #2 to 'insert' (position out of bounds)");
    }
    l.insert(i - 1, set);
    TOKENS.with(|t| t.borrow_mut().insert(i - 1, tok));
}

/// `SharedItemSetListControl`: the item sets shared between builds (the item set manage popup).
pub struct SharedItemSetListControl {
    pub list: ListControl,
    /// `self.itemsTab`
    pub items_tab: CtrlId,
}

static HOOKS: ListHooks = ListHooks {
    get_row_value: SharedItemSetListControl::GetRowValue,
    add_value_tooltip: Some(SharedItemSetListControl::AddValueTooltip),
    get_drag_value: Some(SharedItemSetListControl::GetDragValue),
    on_sel_delete: Some(SharedItemSetListControl::OnSelDelete),
    on_sel_key_down: Some(SharedItemSetListControl::OnSelKeyDown),
    ..ListHooks::BASE
};

impl SharedItemSetListControl {
    /// lua: SharedItemSetListControl.lua:14-31 `SharedItemSetListClass:SharedItemSetListControl(
    /// anchor, rect, itemsTab)`
    /// @pob Classes/SharedItemSetListControl.lua:14-31 h=c0c06865
    pub fn new(
        anchor: Anchor,
        r: [Prop<f64>; 4],
        items_tab: CtrlId,
    ) -> New<SharedItemSetListControl> {
        // L15
        let mut l = ListControl::state(
            &HOOKS,
            16.0,
            true,
            true,
            ListData::Source(Rc::new(SharedSetsSource)),
            false,
        );
        // L17
        l.default_text = Some("^x7F7F7FThis is a list of item sets that will be shared\nbetween all of your builds.\nYou can add sets to this list by dragging them\nfrom the build's set list.".to_owned());
        New::new(
            Base::new(anchor, r),
            SharedItemSetListControl {
                list: l,
                // L16
                items_tab,
            },
        )
        .host()
        .init(|cx, me| ListControl::init(cx, me.0))
        .init(|cx, me| {
            let me = me.0;
            let sel = move |cx: &Cx| cx.w::<ListControl>(me).sel_value.clone();
            // L18-20
            let delete = cx.add(
                me,
                "delete",
                ButtonControl::new(
                    Anchor::new(BottomLeft, me, Top),
                    rect(2, -4, 60, 18),
                    "Delete",
                    move |cx| {
                        // L19
                        let index = cx.w::<ListControl>(me).sel_index;
                        let value = sel(cx).unwrap_or_else(|| {
                            lua_error!("attempt to index local 'sharedItemSet' (a nil value)")
                        });
                        SharedItemSetListControl::OnSelDelete(cx, me, index.unwrap_or(0), &value);
                    },
                ),
            );
            // L21-23
            cx.base_mut(delete).enabled = Prop::f(move |cx, _| {
                // L22
                sel(cx).is_some()
            });
            // L24-26
            let rename = cx.add(
                me,
                "rename",
                ButtonControl::new(
                    Anchor::new(BottomRight, me, Top),
                    rect(-2, -4, 60, 18),
                    "Rename",
                    move |cx| {
                        // L25
                        let value = sel(cx).unwrap_or_else(|| {
                            lua_error!("attempt to index local 'sharedItemSet' (a nil value)")
                        });
                        SharedItemSetListControl::RenameSet(cx, me, &value);
                    },
                ),
            );
            // L27-29
            cx.base_mut(rename).enabled = Prop::f(move |cx, _| {
                // L28
                sel(cx).is_some()
            });
            // L30 return self
        })
    }

    /// lua: SharedItemSetListControl.lua:33-49 `SharedItemSetListClass:RenameSet(sharedItemSet)`
    /// @pob Classes/SharedItemSetListControl.lua:33-49 h=63e87ab4
    pub fn RenameSet(cx: &mut Cx, _me: CtrlId, value: &ListValue) {
        let title = set_of(cx, value)
            .unwrap_or_else(|| lua_error!("attempt to index local 'sharedItemSet' (a nil value)"))
            .title;
        // L48 (the arguments are evaluated after the constructors; these draw nothing)
        let popup_title = if title.is_some() {
            "Rename"
        } else {
            "Set Name"
        };
        let value = value.clone();
        popups::open_popup(
            cx,
            370.0,
            100.0,
            popup_title,
            move |cx, p| {
                let p = p.id();
                // L34 local controls = { }
                // L35
                cx.add(
                    p,
                    "label",
                    LabelControl::new(
                        Anchor::NONE,
                        rect(0, 20, 0, 16),
                        "^7Enter name for this item set:",
                    ),
                );
                // L36-38
                cx.add(
                    p,
                    "edit",
                    EditControl::new(
                        Anchor::NONE,
                        rect(0, 40, 350, 20),
                        EditArgs {
                            init: title,
                            limit: Some(100),
                            change_func: Some(Rc::new(move |cx, buf| {
                                // L37
                                let save = cx.child_as::<ButtonControl>(p, "save");
                                let ok = pattern::find(buf, "%S", None, false).is_some();
                                cx.base_mut(save).enabled = Prop::Val(ok);
                            })),
                            ..EditArgs::default()
                        },
                    ),
                );
                // L39-43
                let save = cx.add(
                    p,
                    "save",
                    ButtonControl::new(Anchor::NONE, rect(-45, 70, 80, 20), "Save", move |cx| {
                        let edit = cx.child_as::<EditControl>(p, "edit");
                        // L40
                        let buf = cx.get(edit).buf.clone();
                        with_set_mut(cx, &value, |s| s.title = Some(buf));
                        // L41
                        let f = items_hooks().set_mod_flag;
                        f(cx, true);
                        // L42
                        popups::close_popup(cx);
                    }),
                );
                // L44
                cx.base_mut(save).enabled = Prop::Val(false);
                // L45-47
                cx.add(
                    p,
                    "cancel",
                    ButtonControl::new(Anchor::NONE, rect(45, 70, 80, 20), "Cancel", |cx| {
                        // L46
                        popups::close_popup(cx);
                    }),
                );
            },
            // L48
            PopupOpts {
                enter: Some("save".into()),
                default: Some("edit".into()),
                ..PopupOpts::default()
            },
        );
    }

    /// lua: SharedItemSetListControl.lua:51-55 `SharedItemSetListClass:GetRowValue(column, index,
    /// sharedItemSet)`
    /// @pob Classes/SharedItemSetListControl.lua:51-55 h=dde5a2b7
    fn GetRowValue(cx: &Cx, _me: CtrlId, column: usize, _index: usize, v: &ListValue) -> String {
        // L52
        if column == 1 {
            let set = set_of(cx, v).unwrap_or_else(|| {
                lua_error!("attempt to index local 'sharedItemSet' (a nil value)")
            });
            // L53
            return set.title.unwrap_or_else(|| "Default".to_owned());
        }
        String::new()
    }

    /// lua: SharedItemSetListControl.lua:57-68 `SharedItemSetListClass:AddValueTooltip(tooltip,
    /// index, sharedItemSet)`
    /// @pob Classes/SharedItemSetListControl.lua:57-68 h=6bb3cfab
    fn AddValueTooltip(
        cx: &mut Cx,
        _me: CtrlId,
        tooltip: &mut Tooltip,
        _index: usize,
        v: &ListValue,
    ) {
        // L58
        tooltip.clear(false);
        let set = set_of(cx, v)
            .unwrap_or_else(|| lua_error!("attempt to index local 'sharedItemSet' (a nil value)"));
        let m = cx.build().model.clone();
        // L59
        for slot in m.itemsTab.orderedSlots.snapshot() {
            // L60
            if slot.get(n!("nodeId")).is_nil() {
                // L61
                let slot_name = slot.str(n!("slotName")).unwrap_or_else(|| "".into());
                // L62
                // L63
                if let Some(item) = set.slots.get(&*slot_name) {
                    // L64
                    let label = m
                        .itemsTab
                        .slots
                        .with(|s| s.get(&model::Name::from(&slot_name)).cloned())
                        .unwrap_or_else(|| lua_error!("attempt to index field '?' (a nil value)"))
                        .get(n!("label"));
                    let Some(label) = label.str() else {
                        lua_error!(
                            "bad argument #2 to 'format' (string expected, got {})",
                            label.lua_type()
                        )
                    };
                    let line = format!("^7{label}: {}", item_label(cx, item));
                    tooltip.add_line(cx, 16.0, &line);
                }
            }
        }
    }

    /// lua: SharedItemSetListControl.lua:70-72 `SharedItemSetListClass:GetDragValue(index, value)`
    /// @pob Classes/SharedItemSetListControl.lua:70-72 h=cc841331
    fn GetDragValue(
        _cx: &Cx,
        _me: CtrlId,
        _index: usize,
        v: &ListValue,
    ) -> Option<(Rc<str>, DragValue)> {
        // L71
        Some((Rc::from("SharedItemList"), DragValue(v.data.clone())))
    }

    /// lua: SharedItemSetListControl.lua:101-107 `SharedItemSetListClass:OnSelDelete(index,
    /// sharedItemSet)`
    /// @pob Classes/SharedItemSetListControl.lua:101-107 h=d8330db0
    fn OnSelDelete(cx: &mut Cx, me: CtrlId, index: usize, v: &ListValue) {
        let title = set_of(cx, v)
            .unwrap_or_else(|| lua_error!("attempt to index local 'sharedItemSet' (a nil value)"))
            .title
            .unwrap_or_else(|| "Default".to_owned());
        // L102-106
        popups::open_confirm_popup(
            cx,
            "Delete Item Set",
            &format!("Are you sure you want to delete '{title}' from the shared item set list?"),
            "Delete",
            move |cx| {
                // L103
                ListControl::list_remove(cx, me, index);
                PENDING.with(|p| p.borrow_mut().take());
                // L104-105
                let w = cx.w_mut::<ListControl>(me);
                w.sel_index = None;
                w.sel_value = None;
            },
            None,
        );
    }

    /// lua: SharedItemSetListControl.lua:109-113 `SharedItemSetListClass:OnSelKeyDown(index,
    /// sharedItemSet, key)`
    /// @pob Classes/SharedItemSetListControl.lua:109-113 h=68846b73
    fn OnSelKeyDown(cx: &mut Cx, me: CtrlId, _index: usize, v: &ListValue, key: Key) {
        // L110
        if key == Key::F(2) {
            // L111
            Self::RenameSet(cx, me, v);
        }
    }
}

impl Widget for SharedItemSetListControl {
    const NAME: &'static str = "SharedItemSetListControl";
    const CAPS: Caps = <ListControl as Widget>::CAPS
        .union(Caps::CAN_RECEIVE_DRAG)
        .union(Caps::RECEIVE_DRAG);

    fn draw(cx: &mut Cx, me: CtrlId, vp: &ViewPort, no_tooltip: bool) {
        ListControl::draw(cx, me, vp, no_tooltip);
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

    /// lua: SharedItemSetListControl.lua:74-76 `SharedItemSetListClass:CanReceiveDrag(type, value)`
    /// @pob Classes/SharedItemSetListControl.lua:74-76 h=9f655d92
    fn can_receive_drag(_cx: &Cx, _me: CtrlId, ty: &str, _v: &DragValue) -> bool {
        // L75
        ty == "ItemList"
    }

    /// lua: SharedItemSetListControl.lua:78-99 `SharedItemSetListClass:ReceiveDrag(type, value,
    /// source)`
    /// @pob Classes/SharedItemSetListControl.lua:78-99 h=5d9f99ce
    fn receive_drag(cx: &mut Cx, me: CtrlId, ty: &str, v: &DragValue, _source: CtrlId) {
        // L79
        if ty == "ItemList" {
            let value: Table = drag_set(v);
            let m = cx.build().model.clone();
            // L80
            let mut shared = SharedItemSet {
                title: value.str(n!("title")).map(|s| s.to_string()),
                ..SharedItemSet::default()
            };
            let active = m.itemsTab.activeItemSet.get();
            // L81 (ORDER, module docs)
            let slots: Vec<(model::Name, Table)> = m
                .itemsTab
                .slots
                .with(|s| s.iter().map(|(k, v)| (*k, v.clone())).collect());
            for (slot_name, slot) in slots {
                // L82
                if slot.get(n!("nodeId")).is_nil() {
                    let mut slot = slot;
                    // L83
                    if !active.as_ref().is_some_and(|a| a.ptr_eq(&value)) {
                        // L84
                        slot = value.table(slot_name).unwrap_or_else(|| {
                            lua_error!("attempt to index local 'slot' (a nil value)")
                        });
                    }
                    // L86
                    let sel = slot.get(n!("selItemId"));
                    if !sel.raw_eq(&Val::Num(0.0)) {
                        // L87
                        let item = item_by_id(cx, &sel).unwrap_or_else(|| {
                            lua_error!("attempt to index local 'item' (a nil value)")
                        });
                        // L88
                        let raw = item.BuildRaw();
                        // L89
                        let new_item = crate::main_mode::new_item(Some(&raw), None, false);
                        // L90
                        if value.get(n!("id")).is_nil() {
                            // L91
                            new_item.NormaliseQuality();
                        }
                        // L93
                        shared.slots.insert(slot_name.to_string(), new_item);
                    }
                }
            }
            // L97 LUA-QUIRK(drag-index): `self.dragIndex` is nil
            let pos = ListControl::list_len(cx, me) + 1;
            insert_entry(cx, pos, Rc::new(SharedSetToken), shared);
        }
    }

    pob_ui_core::impl_parts!(SharedItemSetListControl: list => ListControl);
}
