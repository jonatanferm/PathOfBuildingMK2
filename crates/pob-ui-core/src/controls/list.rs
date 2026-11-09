//! Port of PathOfBuilding v2.67.2 (16de4b82d) src/Classes/ListControl.lua 1-493
//! (`newClass("ListControl", "Control", "ControlHost")`). Owner: U12.
//!
//! @pob-file Classes/ListControl.lua @16de4b82d
//!
//! # Hook set (frozen by U12, CCR-0078)
//!
//! Grepped over every `ListControl` subclass in `src/` (Export/ is the dev tool, out of scope):
//! BuildList, ComparePowerReportList, ConfigSetList, FolderList, ItemDB, ItemList, ItemSetList,
//! MinionList (+ MinionSearchList), NotableDB, PassiveMastery, PassiveSpecList, PowerReportList,
//! SharedItemList, SharedItemSetList, SkillList, SkillSetList, TimelessJewelList,
//! TradeStatWeightMultiplierList; and every instance assignment (`ConfigModBrowser.lua:235-251`
//! on a plain `new("ListControl")`, `CompareTab.lua:1614-1627`, `BuildListControl.lua:33-36` on
//! its PathControl). The methods ListControl.lua calls on `self` are exactly the [`ListHooks`]
//! fields; each is a class hook (`&'static ListHooks`, built from the subclass's associated
//! functions) plus an instance slot ([`ListOverrides`]); the instance slot wins, and a hook is
//! "present" (`if self.OnSelect then`) when either is set. Signatures follow the Lua call sites:
//!
//! - `OnOrderChange(selIndex, selDragIndex)` (L448) passes both indices; SkillListControl.lua:130
//!   reads them. The ui doc §6.8 stub had no arguments (CCR-0078 A1).
//! - `CanDragToValue(index, value, self.otherDragSource)` (L238) passes `otherDragSource`
//!   unguarded, so it may be nil: `Option<CtrlId>` (CCR-0078 A2).
//! - `OnSelCut` is called (L403) though the header comment does not list it; BuildListControl and
//!   CompareTab define it.
//! - `CanReceiveDrag`/`ReceiveDrag` are called on the drag *targets*, which may be any control
//!   (PathControl, DropDownControl, ItemSlotControl, list subclasses): they are the framework's
//!   `Widget::can_receive_drag`/`receive_drag` with `Caps::CAN_RECEIVE_DRAG` as the presence test
//!   (`not target.CanReceiveDrag or target:CanReceiveDrag(..)`, L164).
//! - `Draw`, `OnHoverKeyUp`, `IsMouseOver` overrides are ordinary `Widget` methods of the subclass
//!   (SkillList, ItemList, ItemDB define OnHoverKeyUp; nine subclasses define Draw and call
//!   [`ListControl::draw`] as `self.ListControl.Draw(self, viewPort)`).
//! - No subclass overrides `GetColumnProperty`, `OnDragSend` or `OnAnyKeyDown`; they stay in the
//!   table because ListControl.lua calls them.
//!
//! A subclass embeds `ListControl` as a field and chains `impl_parts!`; its `Widget` impl
//! delegates `draw`/`is_mouse_over`/`on_key_down`/`on_key_up`/`duck` to the functions here.
//!
//! # Fields other controls carry (`target.otherDragSource`)
//!
//! L165/L184/L186/L460-461 write `otherDragSource`/`otherDragTargeting` on arbitrary target
//! controls, and PathControl, DropDownControl and ItemSlotControl read `self.otherDragSource`.
//! `Base` has no such field (CCR-0078 B proposes one). Until then [`other_drag_source`] /
//! [`set_other_drag_source`] store it in the ListControl or PathControl field when the target is
//! one, and otherwise in a per-thread side table keyed by the target's id. // CCR-0078
//!
//! # Notes
//!
//! - `scroll` is `"HORIZONTAL"|"VERTICAL"|boolean|nil`: [`ListControl::state`] takes the truthiness
//!   and `scroll_h` (`self.scroll == "HORIZONTAL"`) is set by the caller afterwards when needed
//!   (only Export/RowListControl passes "HORIZONTAL").
//! - `column._offset`/`column._width` are written by Draw; OnKeyDown reads them, which is a Lua
//!   arithmetic-on-nil error before the first Draw (kept: `lua_error!`).
//! - DIVERGENCE(nil-arg): where Lua would pass a nil list value to a hook (`GetRowValue(1,
//!   self.selIndex, self.selValue)` at L327 after a failed SelectIndex during an active drag), the
//!   port raises the Lua index error instead of calling the hook; no PoB path reaches it.
//! - DIVERGENCE(nil-drag): a `GetDragValue` that returns nothing gives the drag type `""` and a
//!   unit payload (Lua would pass nil); every PoB GetDragValue returns a value.
#![allow(clippy::many_single_char_names, clippy::manual_range_contains)] // Lua names (x, y, ..) and comparisons

use std::any::Any;
use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;

use pob_lua::math::{lmod, m_floor, m_max, m_min};
use pob_sg::{Align, Font, ImageHandle, Key};

use crate::controls::path::PathControl;
use crate::controls::scroll_bar::{ScrollBarControl, ScrollDir};
use crate::luastr::LBuf;
use crate::prelude::*;
use crate::tooltip::Tooltip;

// @pob-skip Classes/ListControl.lua:1-25 reason="header comment (the hook list, see the module docs)"
// @pob Classes/ListControl.lua:26-35 h=7402fd96 (locals and newClass: the Widget impl below)

/// A list value with Lua identity semantics (`selValue == value`). `data` holds the value itself
/// (usually a model Rc such as SocketGroupRef/ItemRef, or a UI-built Rc<..>), which keeps it alive while it is compared.
#[derive(Clone)]
pub struct ListValue {
    pub key: ValKey,
    pub data: Rc<dyn Any>,
}
impl PartialEq for ListValue {
    fn eq(&self, o: &Self) -> bool {
        self.key == o.key
    }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct ValKey(pub usize);
impl ValKey {
    pub fn of_rc<T: ?Sized>(rc: &Rc<T>) -> ValKey {
        ValKey(Rc::as_ptr(rc) as *const () as usize)
    }
}
impl ListValue {
    pub fn of<T: 'static>(rc: Rc<T>) -> ListValue {
        ListValue {
            key: ValKey::of_rc(&rc),
            data: rc,
        }
    }
    /// The value as `T` (the subclass's element type).
    pub fn get<T: 'static>(&self) -> Option<&T> {
        self.data.downcast_ref::<T>()
    }
    /// The value as `Rc<T>`.
    pub fn rc<T: 'static>(&self) -> Option<Rc<T>> {
        self.data.clone().downcast::<T>().ok()
    }
}
impl std::fmt::Debug for ListValue {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "ListValue({:#x})", self.key.0)
    }
}

/// Adapter for model lists (`Shared<Vec<T>>`, calc doc §3) aliased by a list control: reorder/delete write through.
pub struct SharedVecSource<T: Clone + 'static>(pub pob_lua::Shared<Vec<Rc<T>>>);

/// `self.list = build.skillsTab.socketGroupList` aliasing: the list lives in the model; reorder/delete write through.
pub trait ListSource {
    fn len(&self, cx: &Cx) -> usize;
    /// 1-based
    fn get(&self, cx: &Cx, i: usize) -> Option<ListValue>;
    fn insert(&self, cx: &mut Cx, i: usize, v: ListValue);
    fn remove(&self, cx: &mut Cx, i: usize) -> ListValue;
}

impl<T: Clone + 'static> ListSource for SharedVecSource<T> {
    /// `#self.list` (hole-free)
    fn len(&self, _cx: &Cx) -> usize {
        self.0.len()
    }
    /// `self.list[i]`
    fn get(&self, _cx: &Cx, i: usize) -> Option<ListValue> {
        self.0.get(i).map(ListValue::of)
    }
    /// `t_insert(self.list, i, v)`: `v` must hold an `Rc<T>` (it came out of this list).
    fn insert(&self, _cx: &mut Cx, i: usize, v: ListValue) {
        let Ok(rc) = v.data.downcast::<T>() else {
            crate::lua_error!("SharedVecSource::insert: value of another type")
        };
        self.0.with_mut(|l| {
            // lj_tab insert: 1 <= i <= #t + 1
            if i == 0 || i > l.len() + 1 {
                crate::lua_error!("bad argument #2 to 'insert' (position out of bounds)");
            }
            l.insert(i - 1, rc);
        });
    }
    /// `t_remove(self.list, i)` (1 <= i <= #t)
    fn remove(&self, _cx: &mut Cx, i: usize) -> ListValue {
        let rc = self.0.with_mut(|l| {
            if i == 0 || i > l.len() {
                crate::lua_error!("t_remove: position out of range");
            }
            l.remove(i - 1)
        });
        ListValue::of(rc)
    }
}

/// `self.list`: a list the control owns, or one aliased from the model.
pub enum ListData {
    Local(Vec<ListValue>),
    Source(Rc<dyn ListSource>),
}

impl Default for ListData {
    /// `list or { }`
    fn default() -> Self {
        ListData::Local(Vec::new())
    }
}

/// One `colList[i]` entry (`{ width, sortable, label, font }`; ListControl.lua:52, :145, :218,
/// :295, :306). Fields that may be nil are `Option`; `width` may be a function of the control.
#[derive(Clone, Default)]
pub struct ListColumn {
    /// `width` (number, function, or nil: Draw then uses the remaining width)
    pub width: Option<Prop<f64>>,
    pub label: Option<Prop<String>>,
    pub sortable: Option<Prop<bool>>,
    /// `font` (Export/RowListControl only; nil => "VAR")
    pub font: Option<Font>,
    /// `column._offset` (written by Draw)
    pub offset: Option<f64>,
    /// `column._width` (written by Draw)
    pub computed_width: Option<f64>,
}

impl ListColumn {
    /// `{ width = w, label = l, sortable = s }`
    pub fn new(width: Option<Prop<f64>>, label: Option<&str>, sortable: bool) -> ListColumn {
        ListColumn {
            width,
            label: label.map(|l| Prop::Val(l.to_owned())),
            sortable: sortable.then_some(Prop::Val(true)),
            ..ListColumn::default()
        }
    }
}

/// What GetColumnProperty returns (ListControl.lua:111-117: a column field, maybe a function).
#[derive(Clone, Debug, PartialEq)]
pub enum ColumnProp {
    Nil,
    Num(f64),
    Str(String),
    Bool(bool),
}

impl ColumnProp {
    /// Lua truthiness
    pub fn truthy(&self) -> bool {
        !matches!(self, ColumnProp::Nil | ColumnProp::Bool(false))
    }
    pub fn num(&self) -> Option<f64> {
        match self {
            ColumnProp::Num(n) => Some(*n),
            _ => None,
        }
    }
    pub fn str(&self) -> Option<&str> {
        match self {
            ColumnProp::Str(s) => Some(s),
            _ => None,
        }
    }
}

pub type GetRowValueFn = dyn Fn(&Cx, CtrlId, usize, usize, &ListValue) -> String;
pub type GetRowIconFn = dyn Fn(&Cx, CtrlId, usize, usize, &ListValue) -> Option<ImageHandle>;
pub type AddValueTooltipFn = dyn Fn(&mut Cx, CtrlId, &mut Tooltip, usize, &ListValue);
pub type GetDragValueFn = dyn Fn(&Cx, CtrlId, usize, &ListValue) -> Option<(Rc<str>, DragValue)>;
pub type OnDragSendFn = dyn Fn(&mut Cx, CtrlId, usize, &ListValue, CtrlId);
pub type OnOrderChangeFn = dyn Fn(&mut Cx, CtrlId, usize, usize);
pub type IndexValueFn = dyn Fn(&mut Cx, CtrlId, usize, &ListValue);
pub type OnSelClickFn = dyn Fn(&mut Cx, CtrlId, usize, &ListValue, bool) -> Option<bool>;
pub type OnSelKeyDownFn = dyn Fn(&mut Cx, CtrlId, usize, &ListValue, Key);
pub type OnAnyKeyDownFn = dyn Fn(&mut Cx, CtrlId, Key);
pub type OverrideSelectIndexFn = dyn Fn(&mut Cx, CtrlId, usize) -> bool;
pub type SetHighlightColorFn = dyn Fn(&mut Cx, CtrlId, usize, &ListValue) -> bool;
pub type CanDragToValueFn = dyn Fn(&Cx, CtrlId, usize, &ListValue, Option<CtrlId>) -> bool;
pub type ReSortFn = dyn Fn(&mut Cx, CtrlId, usize);
pub type GetColumnPropertyFn = dyn Fn(&Cx, CtrlId, usize, &str) -> ColumnProp;

/// ListControl.lua:6-24 + every `self.X and self:X(..)` call site in ListControl.lua (frozen,
/// see the module docs). `None` = the method is absent.
pub struct ListHooks {
    /// `GetRowValue(column, index, value)` (required)
    pub get_row_value:
        fn(&Cx, CtrlId, usize /*column*/, usize /*index*/, &ListValue) -> String,
    pub get_row_icon: Option<fn(&Cx, CtrlId, usize, usize, &ListValue) -> Option<ImageHandle>>,
    /// `AddValueTooltip(tooltip, index, value)`
    pub add_value_tooltip: Option<fn(&mut Cx, CtrlId, &mut Tooltip, usize, &ListValue)>,
    /// `GetDragValue(index, value)` => `type, value`
    pub get_drag_value: Option<fn(&Cx, CtrlId, usize, &ListValue) -> Option<(Rc<str>, DragValue)>>,
    /// `OnDragSend(index, value, target)`
    pub on_drag_send: Option<fn(&mut Cx, CtrlId, usize, &ListValue, CtrlId)>,
    /// `OnOrderChange(selIndex, selDragIndex)` (CCR-0078 A1)
    pub on_order_change: Option<fn(&mut Cx, CtrlId, usize, usize)>,
    pub on_select: Option<fn(&mut Cx, CtrlId, usize, &ListValue)>,
    /// `OnSelClick(index, value, doubleClick)`; Some(false) releases focus
    pub on_sel_click: Option<fn(&mut Cx, CtrlId, usize, &ListValue, bool) -> Option<bool>>,
    pub on_sel_copy: Option<fn(&mut Cx, CtrlId, usize, &ListValue)>,
    pub on_sel_cut: Option<fn(&mut Cx, CtrlId, usize, &ListValue)>,
    pub on_sel_delete: Option<fn(&mut Cx, CtrlId, usize, &ListValue)>,
    pub on_sel_key_down: Option<fn(&mut Cx, CtrlId, usize, &ListValue, Key)>,
    pub on_any_key_down: Option<fn(&mut Cx, CtrlId, Key)>,
    /// `OverrideSelectIndex(index)`: true prevents the default action
    pub override_select_index: Option<fn(&mut Cx, CtrlId, usize) -> bool>,
    /// `SetHighlightColor(index, value)`: true when it set the colour itself
    pub set_highlight_color: Option<fn(&mut Cx, CtrlId, usize, &ListValue) -> bool>,
    /// `CanDragToValue(index, value, otherDragSource)` (CCR-0078 A2)
    pub can_drag_to_value: Option<fn(&Cx, CtrlId, usize, &ListValue, Option<CtrlId>) -> bool>,
    pub re_sort: Option<fn(&mut Cx, CtrlId, usize)>,
    /// default = ListControl's own
    pub get_column_property: fn(&Cx, CtrlId, usize, &str) -> ColumnProp,
}

impl ListHooks {
    /// The base class's table: every optional method absent, `GetRowValue` missing (a plain
    /// `new("ListControl")` gets it per instance, ConfigModBrowser.lua:235). Subclasses write
    /// `ListHooks { get_row_value: .., on_select: Some(..), ..ListHooks::BASE }`.
    pub const BASE: ListHooks = ListHooks {
        get_row_value: nil_get_row_value,
        get_row_icon: None,
        add_value_tooltip: None,
        get_drag_value: None,
        on_drag_send: None,
        on_order_change: None,
        on_select: None,
        on_sel_click: None,
        on_sel_copy: None,
        on_sel_cut: None,
        on_sel_delete: None,
        on_sel_key_down: None,
        on_any_key_down: None,
        override_select_index: None,
        set_highlight_color: None,
        can_drag_to_value: None,
        re_sort: None,
        get_column_property: ListControl::get_column_property,
    };
}

/// `ListControl` itself (`new("ListControl")`).
pub static LIST_CONTROL_HOOKS: ListHooks = ListHooks::BASE;

fn nil_get_row_value(_cx: &Cx, me: CtrlId, _c: usize, _i: usize, _v: &ListValue) -> String {
    nil_method::<ListControl>(me, "GetRowValue")
}

/// Same slots as Rc closures, for per-instance overrides (CompareTab.lua:1614-1627). Instance wins.
#[derive(Default, Clone)]
pub struct ListOverrides {
    pub get_row_value: Option<Rc<GetRowValueFn>>,
    pub get_row_icon: Option<Rc<GetRowIconFn>>,
    pub add_value_tooltip: Option<Rc<AddValueTooltipFn>>,
    pub get_drag_value: Option<Rc<GetDragValueFn>>,
    pub on_drag_send: Option<Rc<OnDragSendFn>>,
    pub on_order_change: Option<Rc<OnOrderChangeFn>>,
    pub on_select: Option<Rc<IndexValueFn>>,
    pub on_sel_click: Option<Rc<OnSelClickFn>>,
    pub on_sel_copy: Option<Rc<IndexValueFn>>,
    pub on_sel_cut: Option<Rc<IndexValueFn>>,
    pub on_sel_delete: Option<Rc<IndexValueFn>>,
    pub on_sel_key_down: Option<Rc<OnSelKeyDownFn>>,
    pub on_any_key_down: Option<Rc<OnAnyKeyDownFn>>,
    pub override_select_index: Option<Rc<OverrideSelectIndexFn>>,
    pub set_highlight_color: Option<Rc<SetHighlightColorFn>>,
    pub can_drag_to_value: Option<Rc<CanDragToValueFn>>,
    pub re_sort: Option<Rc<ReSortFn>>,
    pub get_column_property: Option<Rc<GetColumnPropertyFn>>,
}

/// "instance override, else class hook" for an optional hook.
macro_rules! resolver {
    ($fname:ident, $field:ident, $ty:ty) => {
        fn $fname(cx: &Cx, me: CtrlId) -> Option<Rc<$ty>> {
            let w = cx.w::<ListControl>(me);
            if let Some(f) = &w.ov.$field {
                return Some(f.clone());
            }
            w.hooks.$field.map(|f| Rc::new(f) as Rc<$ty>)
        }
    };
}
resolver!(r_get_row_icon, get_row_icon, GetRowIconFn);
resolver!(r_add_value_tooltip, add_value_tooltip, AddValueTooltipFn);
resolver!(r_get_drag_value, get_drag_value, GetDragValueFn);
resolver!(r_on_drag_send, on_drag_send, OnDragSendFn);
resolver!(r_on_order_change, on_order_change, OnOrderChangeFn);
resolver!(r_on_select, on_select, IndexValueFn);
resolver!(r_on_sel_click, on_sel_click, OnSelClickFn);
resolver!(r_on_sel_copy, on_sel_copy, IndexValueFn);
resolver!(r_on_sel_cut, on_sel_cut, IndexValueFn);
resolver!(r_on_sel_delete, on_sel_delete, IndexValueFn);
resolver!(r_on_sel_key_down, on_sel_key_down, OnSelKeyDownFn);
resolver!(r_on_any_key_down, on_any_key_down, OnAnyKeyDownFn);
resolver!(
    r_override_select_index,
    override_select_index,
    OverrideSelectIndexFn
);
resolver!(
    r_set_highlight_color,
    set_highlight_color,
    SetHighlightColorFn
);
resolver!(r_can_drag_to_value, can_drag_to_value, CanDragToValueFn);
resolver!(r_re_sort, re_sort, ReSortFn);

fn r_get_row_value(cx: &Cx, me: CtrlId) -> Rc<GetRowValueFn> {
    let w = cx.w::<ListControl>(me);
    if let Some(f) = &w.ov.get_row_value {
        return f.clone();
    }
    Rc::new(w.hooks.get_row_value)
}

fn r_get_column_property(cx: &Cx, me: CtrlId) -> Rc<GetColumnPropertyFn> {
    let w = cx.w::<ListControl>(me);
    if let Some(f) = &w.ov.get_column_property {
        return f.clone();
    }
    Rc::new(w.hooks.get_column_property)
}

/// `self:GetColumnProperty(column, property)` through the hook table.
fn col_prop(cx: &Cx, me: CtrlId, column: usize, property: &str) -> ColumnProp {
    let f = r_get_column_property(cx, me);
    f(cx, me, column, property)
}

#[track_caller]
fn num(v: Option<f64>) -> f64 {
    v.unwrap_or_else(|| crate::lua_error!("attempt to perform arithmetic on a nil value"))
}

/// `otherDragSource`/`otherDragTargeting` of a control that is neither a ListControl nor a
/// PathControl (CCR-0078 B). // CCR-0078
#[derive(Clone, Copy, Default)]
struct DragTargetFields {
    other_drag_source: Option<CtrlId>,
    other_drag_targeting: bool,
}

thread_local! {
    // CCR-0078: until `Base` carries the two fields.
    static DRAG_TARGET_FIELDS: RefCell<HashMap<CtrlId, DragTargetFields>> = RefCell::new(HashMap::new());
}

/// `control.otherDragSource` of any control (nil/false => None). // CCR-0078
pub fn other_drag_source(cx: &Cx, id: CtrlId) -> Option<CtrlId> {
    if let Some(l) = cx.ui.try_w::<ListControl>(id) {
        return l.other_drag_source;
    }
    if let Some(p) = cx.ui.try_w::<PathControl>(id) {
        return p.other_drag_source;
    }
    DRAG_TARGET_FIELDS.with(|m| m.borrow().get(&id).and_then(|f| f.other_drag_source))
}

/// `control.otherDragSource = v`. // CCR-0078
pub fn set_other_drag_source(cx: &mut Cx, id: CtrlId, v: Option<CtrlId>) {
    if cx.is::<ListControl>(id) {
        cx.w_mut::<ListControl>(id).other_drag_source = v;
    } else if cx.is::<PathControl>(id) {
        cx.w_mut::<PathControl>(id).other_drag_source = v;
    } else {
        DRAG_TARGET_FIELDS.with(|m| {
            let mut m = m.borrow_mut();
            let e = m.entry(id).or_default();
            e.other_drag_source = v;
            if e.other_drag_source.is_none() && !e.other_drag_targeting {
                m.remove(&id);
            }
        });
    }
}

/// `control.otherDragTargeting` of any control. // CCR-0078
pub fn other_drag_targeting(cx: &Cx, id: CtrlId) -> bool {
    if let Some(l) = cx.ui.try_w::<ListControl>(id) {
        return l.other_drag_targeting;
    }
    if let Some(p) = cx.ui.try_w::<PathControl>(id) {
        return p.other_drag_targeting;
    }
    DRAG_TARGET_FIELDS.with(|m| m.borrow().get(&id).is_some_and(|f| f.other_drag_targeting))
}

/// `control.otherDragTargeting = v`. // CCR-0078
pub fn set_other_drag_targeting(cx: &mut Cx, id: CtrlId, v: bool) {
    if cx.is::<ListControl>(id) {
        cx.w_mut::<ListControl>(id).other_drag_targeting = v;
    } else if cx.is::<PathControl>(id) {
        cx.w_mut::<PathControl>(id).other_drag_targeting = v;
    } else {
        DRAG_TARGET_FIELDS.with(|m| {
            let mut m = m.borrow_mut();
            let e = m.entry(id).or_default();
            e.other_drag_targeting = v;
            if e.other_drag_source.is_none() && !e.other_drag_targeting {
                m.remove(&id);
            }
        });
    }
}

/// `ListControl:GetRowRegion()` (L126-134)
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct RowRegion {
    pub x: f64,
    pub y: f64,
    pub width: f64,
    pub height: f64,
}

pub struct ListControl {
    pub hooks: &'static ListHooks,
    pub ov: ListOverrides,
    pub list: ListData,
    pub row_height: f64,
    /// `self.scroll` truthiness
    pub scroll: bool,
    pub is_mutable: bool,
    pub force_tooltip: bool,
    pub sel_index: Option<usize>,
    pub sel_value: Option<ListValue>,
    pub sel_dragging: bool,
    pub sel_drag_active: bool,
    pub drag_target_list: Option<Vec<CtrlId>>,
    pub other_drag_source: Option<CtrlId>,
    pub drag_type: Option<Rc<str>>,
    pub drag_value: Option<DragValue>,
    pub col_list: Vec<ListColumn>,
    pub col_labels: bool,
    pub label: Option<Prop<String>>,
    pub show_row_separators: bool,
    pub tooltip: Tooltip,
    /// `self.font` ("VAR")
    pub font: Font,
    /// `self.scrollH` (`self.scroll == "HORIZONTAL"`; nil when scroll is falsy)
    pub scroll_h: bool,
    /// `self.labelPositionOffset`
    pub label_position_offset: [f64; 2],
    /// `self.defaultText`
    pub default_text: Option<String>,
    /// `self.selCX`, `self.selCY`
    pub sel_cx: Option<f64>,
    pub sel_cy: Option<f64>,
    /// `self.selDragIndex`
    pub sel_drag_index: Option<usize>,
    /// `self.dragTarget`
    pub drag_target: Option<CtrlId>,
    /// `self.otherDragTargeting`
    pub other_drag_targeting: bool,
    /// `self.hoverIndex`, `self.hoverValue` (written by Draw)
    pub hover_index: Option<usize>,
    pub hover_value: Option<ListValue>,
}

impl ListControl {
    /// ListControl.lua:44 ListControl(anchor, rect, rowHeight, scroll, isMutable, list, forceTooltip):
    /// the fields (L47-L54, L84). The caller runs [`ListControl::init`] for the scroll bars, and
    /// before constructing the first ListControl, `tooltip::tooltip::module_load` (L53 is the
    /// first `new("Tooltip")` when no TooltipHost came earlier).
    /// @pob Classes/ListControl.lua:44-86 h=0a0c92ad
    pub fn state(
        hooks: &'static ListHooks,
        row_height: f64,
        scroll: bool,
        is_mutable: bool,
        list: ListData,
        force_tooltip: bool,
    ) -> ListControl {
        ListControl {
            hooks,
            ov: ListOverrides::default(),
            // L50
            list,
            // L47
            row_height,
            // L48
            scroll,
            // L49
            is_mutable,
            // L51
            force_tooltip,
            sel_index: None,
            sel_value: None,
            sel_dragging: false,
            sel_drag_active: false,
            drag_target_list: None,
            other_drag_source: None,
            drag_type: None,
            drag_value: None,
            // L52
            col_list: vec![ListColumn::default()],
            col_labels: false,
            label: None,
            show_row_separators: false,
            // L53
            tooltip: Tooltip::default(),
            // L54
            font: Font::Var,
            // L55-L61 (`scroll == "HORIZONTAL"`: set `scroll_h` after `state`)
            scroll_h: false,
            // L84
            label_position_offset: [0.0, 0.0],
            default_text: None,
            sel_cx: None,
            sel_cy: None,
            sel_drag_index: None,
            drag_target: None,
            other_drag_targeting: false,
            hover_index: None,
            hover_value: None,
        }
    }

    /// `new("ListControl"):ListControl(anchor, rect, ...)` with instance hooks set later
    /// (ConfigModBrowser.lua:232).
    pub fn new(
        anchor: Anchor,
        rect: [Prop<f64>; 4],
        row_height: f64,
        scroll: bool,
        is_mutable: bool,
        list: ListData,
        force_tooltip: bool,
    ) -> New<ListControl> {
        // L45-L46
        New::new(
            Base::new(anchor, rect),
            ListControl::state(
                &LIST_CONTROL_HOOKS,
                row_height,
                scroll,
                is_mutable,
                list,
                force_tooltip,
            ),
        )
        .host()
        .init(|cx, me| ListControl::init(cx, me.0))
    }

    /// The constructor's children (L62-L83): the scroll bars, anchored to `me`.
    pub fn init(cx: &mut Cx, me: CtrlId) {
        let (row_height, scroll) = {
            let w = cx.w::<ListControl>(me);
            (w.row_height, w.scroll)
        };
        // L62
        cx.add(
            me,
            "scrollBarH",
            ScrollBarControl::new(
                Anchor::new(Bottom, me, Bottom),
                rect(-8, -1, 0, if scroll { 16 } else { 0 }),
                Some(row_height * 2.0),
                Some(ScrollDir::Horizontal),
                false,
            )
            // L63
            .shown(Prop::f(move |cx, _| {
                // L64
                cx.w::<ListControl>(me).scroll_h
            }))
            // L66
            .width(Prop::f(move |cx, _| {
                // L67
                let (width, _height) = cx.size(me);
                // L68
                width - 18.0
            })),
        );
        // L71
        cx.add(
            me,
            "scrollBarV",
            ScrollBarControl::new(
                Anchor::new(Right, me, Right),
                rect(-1, 0, if scroll { 16 } else { 0 }, 0),
                Some(row_height * 2.0),
                Some(ScrollDir::Vertical),
                false,
            )
            // L72
            .y(Prop::f(move |cx, _| {
                // L73
                if cx.w::<ListControl>(me).scroll_h {
                    -8.0
                } else {
                    0.0
                }
            }))
            // L75
            .height(Prop::f(move |cx, _| {
                // L76
                let (_width, height) = cx.size(me);
                // L77
                height
                    - 2.0
                    - if cx.w::<ListControl>(me).scroll_h {
                        16.0
                    } else {
                        0.0
                    }
            })),
        );
        // L80
        if !scroll {
            // L81
            let h = cx.child_as::<ScrollBarControl>(me, "scrollBarH");
            cx.base_mut(h).shown = Prop::Val(false);
            // L82
            let v = cx.child_as::<ScrollBarControl>(me, "scrollBarV");
            cx.base_mut(v).shown = Prop::Val(false);
        }
    }

    fn sb_h(cx: &Cx, me: CtrlId) -> CtrlId {
        cx.child_as::<ScrollBarControl>(me, "scrollBarH").id()
    }
    fn sb_v(cx: &Cx, me: CtrlId) -> CtrlId {
        cx.child_as::<ScrollBarControl>(me, "scrollBarV").id()
    }

    /// `#self.list`
    pub fn list_len(cx: &Cx, me: CtrlId) -> usize {
        match &cx.w::<ListControl>(me).list {
            ListData::Local(v) => v.len(),
            ListData::Source(s) => s.len(cx),
        }
    }

    /// `self.list[i]` (1-based; 0 and out-of-range are nil)
    pub fn list_get(cx: &Cx, me: CtrlId, i: usize) -> Option<ListValue> {
        match &cx.w::<ListControl>(me).list {
            ListData::Local(v) => {
                if i >= 1 {
                    v.get(i - 1).cloned()
                } else {
                    None
                }
            }
            ListData::Source(s) => s.get(cx, i),
        }
    }

    /// `self.list[i]` for a Lua number index (non-integers and indices < 1 are nil).
    fn list_get_num(cx: &Cx, me: CtrlId, i: f64) -> Option<ListValue> {
        if i >= 1.0 && i.fract() == 0.0 {
            Self::list_get(cx, me, i as usize)
        } else {
            None
        }
    }

    /// `t_remove(self.list, i)`
    pub fn list_remove(cx: &mut Cx, me: CtrlId, i: usize) -> Option<ListValue> {
        let src = match &mut cx.w_mut::<ListControl>(me).list {
            ListData::Local(v) => {
                return (i >= 1 && i <= v.len()).then(|| v.remove(i - 1));
            }
            ListData::Source(s) => s.clone(),
        };
        if i >= 1 && i <= src.len(cx) {
            Some(src.remove(cx, i))
        } else {
            None
        }
    }

    /// `t_insert(self.list, i, v)`
    pub fn list_insert(cx: &mut Cx, me: CtrlId, i: usize, v: ListValue) {
        let src = match &mut cx.w_mut::<ListControl>(me).list {
            ListData::Local(l) => {
                // DIVERGENCE(hole): LuaJIT's insert has no bounds check; a position past #t + 1
                // would leave a hole, which a Vec cannot hold. ListControl only inserts at
                // selDragIndex <= #list + 1.
                if i == 0 || i > l.len() + 1 {
                    crate::lua_error!("t_insert: position {} would leave a hole", i);
                }
                l.insert(i - 1, v);
                return;
            }
            ListData::Source(s) => s.clone(),
        };
        src.insert(cx, i, v);
    }

    /// lua: ListControl.lua:89-109 SelectIndex(index); `index` 0 is Lua's nil/out-of-range.
    /// @pob Classes/ListControl.lua:89-109 h=a9042cec
    pub fn select_index(cx: &mut Cx, me: CtrlId, index: usize) -> bool {
        // L90
        let v = Self::list_get(cx, me, index);
        cx.w_mut::<ListControl>(me).sel_value = v.clone();
        // L91
        let Some(sel_value) = v else {
            // L92
            return false;
        };
        // L95
        if let Some(f) = r_override_select_index(cx, me) {
            if f(cx, me, index) {
                // L96
                return false;
            }
        }
        // L98
        cx.w_mut::<ListControl>(me).sel_index = Some(index);
        // L99
        let (_width, height) = cx.size(me);
        // L100
        if cx.w::<ListControl>(me).scroll {
            let sbv = Self::sb_v(cx, me);
            let (n, row_height) = (Self::list_len(cx, me), cx.w::<ListControl>(me).row_height);
            // L101
            ScrollBarControl::set_content_dimension(cx, sbv, n as f64 * row_height, height - 4.0);
            // L102
            ScrollBarControl::scroll_into_view(
                cx,
                sbv,
                (index as f64 - 2.0) * row_height,
                row_height * 3.0,
            );
        }
        // L104
        if let Some(f) = r_on_select(cx, me) {
            // L105 (self.selIndex, self.selValue: `index` and the value just stored)
            f(cx, me, index, &sel_value);
        }
        // L108
        true
    }

    /// lua: ListControl.lua:111-117 GetColumnProperty(column, property): the class method.
    /// @pob Classes/ListControl.lua:111-117 h=f34e5e9e
    pub fn get_column_property(cx: &Cx, me: CtrlId, column: usize, property: &str) -> ColumnProp {
        let col = cx
            .w::<ListControl>(me)
            .col_list
            .get(column.wrapping_sub(1))
            .cloned()
            .unwrap_or_else(|| crate::lua_error!("attempt to index local 'column' (a nil value)"));
        // L112 `type(column[property]) == "function"` => L113 call it, else L115 the value
        match property {
            "width" => col
                .width
                .map_or(ColumnProp::Nil, |p| ColumnProp::Num(p.get(cx, me))),
            "label" => col
                .label
                .map_or(ColumnProp::Nil, |p| ColumnProp::Str(p.get(cx, me))),
            "sortable" => col
                .sortable
                .map_or(ColumnProp::Nil, |p| ColumnProp::Bool(p.get(cx, me))),
            "font" => col.font.map_or(ColumnProp::Nil, |f| {
                ColumnProp::Str(font_name(f).to_owned())
            }),
            _ => ColumnProp::Nil,
        }
    }

    /// lua: ListControl.lua:119-124
    /// @pob Classes/ListControl.lua:119-124 h=fd5c5c03
    pub fn is_mouse_over(cx: &Cx, me: CtrlId) -> bool {
        // L120
        if !cx.is_shown(me) {
            // L121
            return false;
        }
        // L123
        cx.is_mouse_in_bounds(me) || cx.get_mouse_over_control(me).is_some()
    }

    /// lua: ListControl.lua:126-134 GetRowRegion
    /// @pob Classes/ListControl.lua:126-134 h=92018a48
    pub fn get_row_region(cx: &Cx, me: CtrlId) -> RowRegion {
        // L127
        let (width, height) = cx.size(me);
        let w = cx.w::<ListControl>(me);
        // L128
        RowRegion {
            // L129
            x: 2.0,
            // L130
            y: if w.col_labels { 20.0 } else { 2.0 },
            // L131
            width: if w.scroll { width - 20.0 } else { width },
            // L132
            height: height
                - 4.0
                - if w.scroll && w.scroll_h { 16.0 } else { 0.0 }
                - if w.col_labels { 18.0 } else { 0.0 },
        }
    }

    /// lua: ListControl.lua:136-338
    /// @pob Classes/ListControl.lua:136-338 h=c668385e
    pub fn draw(cx: &mut Cx, me: CtrlId, vp: &ViewPort, no_tooltip: bool) {
        // L137
        let (x, y) = cx.pos(me);
        // L138
        let (width, height) = cx.size(me);
        // L139
        let row_height = cx.w::<ListControl>(me).row_height;
        // L140 local list = self.list (read through list_len/list_get)
        let scroll = cx.w::<ListControl>(me).scroll;

        // L142
        let mut col_offset = 0.0;
        // L143
        let n_cols = cx.w::<ListControl>(me).col_list.len();
        for index in 1..=n_cols {
            // L144
            cx.w_mut::<ListControl>(me).col_list[index - 1].offset = Some(col_offset);
            // L145
            let w =
                col_prop(cx, me, index, "width")
                    .num()
                    .unwrap_or(if index == n_cols && scroll {
                        width - 20.0
                    } else {
                        width - col_offset
                    });
            cx.w_mut::<ListControl>(me).col_list[index - 1].computed_width = Some(w);
            // L146
            col_offset += w;
        }

        // L149
        let scroll_bar_v = Self::sb_v(cx, me);
        // L150
        let row_region = Self::get_row_region(cx, me);
        // L151
        let n = Self::list_len(cx, me);
        ScrollBarControl::set_content_dimension(
            cx,
            scroll_bar_v,
            n as f64 * row_height,
            row_region.height,
        );
        // L152
        let scroll_offset_v = cx.w::<ScrollBarControl>(scroll_bar_v).offset;
        // L153
        let scroll_bar_h = Self::sb_h(cx, me);
        // L154
        let (last_off, last_w) = {
            let l = cx.w::<ListControl>(me).col_list.last().unwrap_or_else(|| {
                crate::lua_error!("attempt to index local 'lastCol' (a nil value)")
            });
            (num(l.offset), num(l.computed_width))
        };
        // L155
        ScrollBarControl::set_content_dimension(
            cx,
            scroll_bar_h,
            last_off + last_w,
            row_region.width,
        );
        // L156
        let scroll_offset_h = cx.w::<ScrollBarControl>(scroll_bar_h).offset;

        // L158
        let (cursor_x, cursor_y) = cx.sys.cursor_pos();
        // L159
        let start_drag = {
            let w = cx.w::<ListControl>(me);
            w.sel_value.is_some() && w.sel_dragging && !w.sel_drag_active && {
                let (scx, scy) = (num(w.sel_cx), num(w.sel_cy));
                (cursor_x - scx) * (cursor_x - scx) + (cursor_y - scy) * (cursor_y - scy) > 100.0
            }
        };
        if start_drag {
            // L160
            cx.w_mut::<ListControl>(me).sel_drag_active = true;
            // L161
            if let Some(targets) = cx.w::<ListControl>(me).drag_target_list.clone() {
                // L162
                let (si, sv) = {
                    let w = cx.w::<ListControl>(me);
                    (w.sel_index, w.sel_value.clone())
                };
                let f = r_get_drag_value(cx, me)
                    .unwrap_or_else(|| nil_method::<ListControl>(me, "GetDragValue"));
                let (si, sv) = (
                    si.unwrap_or_else(|| crate::lua_error!("selIndex is nil")),
                    sv.expect("selValue checked above"),
                );
                let (ty, val) = match f(cx, me, si, &sv) {
                    Some((t, v)) => (Some(t), Some(v)),
                    None => (None, None),
                };
                {
                    let w = cx.w_mut::<ListControl>(me);
                    w.drag_type = ty.clone();
                    w.drag_value = val.clone();
                }
                // L163
                for target in targets {
                    // L164
                    let accepts = !cx.has(target, Caps::CAN_RECEIVE_DRAG) || {
                        let (t, v) = drag_args(&ty, &val);
                        cx.can_receive_drag(target, &t, &v)
                    };
                    if accepts {
                        // L165
                        set_other_drag_source(cx, target, Some(me));
                    }
                }
            }
        }
        // L170
        cx.w_mut::<ListControl>(me).sel_drag_index = None;
        // L171
        let (sel_drag_active, other_src, is_mutable) = {
            let w = cx.w::<ListControl>(me);
            (w.sel_drag_active, w.other_drag_source, w.is_mutable)
        };
        if (sel_drag_active || other_src.is_some()) && is_mutable {
            // L172
            if cursor_x >= x + 2.0
                && cursor_y >= y + 2.0
                && cursor_x < x + width - 20.0
                && cursor_y < y + height - 2.0
            {
                // L173
                let index =
                    m_floor((cursor_y - y - 2.0 + scroll_offset_v) / row_height + 0.5) + 1.0;
                // L174
                let sel_index = cx.w::<ListControl>(me).sel_index;
                if sel_index.map_or(true, |s| index < s as f64 || index > s as f64 + 1.0) {
                    // L175
                    cx.w_mut::<ListControl>(me).sel_drag_index =
                        Some(m_min(index, n as f64 + 1.0) as usize);
                }
            }
        }
        // L179
        let targets = cx.w::<ListControl>(me).drag_target_list.clone();
        if sel_drag_active {
            if let Some(targets) = &targets {
                // L180
                cx.w_mut::<ListControl>(me).drag_target = None;
                // L181
                for &target in targets {
                    // L182
                    if cx.w::<ListControl>(me).drag_target.is_none()
                        && other_drag_source(cx, target) == Some(me)
                        && cx.is_mouse_over(target)
                    {
                        // L183
                        cx.w_mut::<ListControl>(me).drag_target = Some(target);
                        // L184
                        set_other_drag_targeting(cx, target, true);
                    } else {
                        // L186
                        set_other_drag_targeting(cx, target, false);
                    }
                }
            }
        }

        // L191
        let label = cx.w::<ListControl>(me).label.clone().map(|p| p.get(cx, me));
        // L192
        if let Some(label) = label {
            let (off, font) = {
                let w = cx.w::<ListControl>(me);
                (w.label_position_offset, w.font)
            };
            // L193
            cx.sg.draw_string(
                x + off[0],
                y - 20.0 + off[1],
                Align::Left,
                16.0,
                font,
                &label,
            );
        }
        let has_can_drag = r_can_drag_to_value(cx, me);
        let other_src = cx.w::<ListControl>(me).other_drag_source;
        let has_focus = cx.base(me).has_focus == Some(true);
        // L195
        if other_src.is_some() && has_can_drag.is_none() {
            cx.sg.set_draw_color(0.2, 0.6, 0.2);
        } else if has_focus {
            // L197
            cx.sg.set_draw_color(1.0, 1.0, 1.0);
        } else {
            // L200
            cx.sg.set_draw_color(0.5, 0.5, 0.5);
        }
        // L202
        cx.sg.draw_image(None, x, y, width, height);
        // L203
        if other_src.is_some() && has_can_drag.is_none() {
            cx.sg.set_draw_color(0.0, 0.05, 0.0);
        } else {
            // L206
            cx.sg.set_draw_color(0.0, 0.0, 0.0);
        }
        // L208
        cx.sg
            .draw_image(None, x + 1.0, y + 1.0, width - 2.0, height - 2.0);
        // L209
        let force_tooltip = cx.w::<ListControl>(me).force_tooltip;
        cx.draw_controls(me, vp, (no_tooltip && !force_tooltip).then_some(me));

        // `self.hasFocus` again: the row loop reads it after DrawControls
        let has_focus = cx.base(me).has_focus == Some(true);
        let (scroll_h, show_row_separators, col_labels) = {
            let w = cx.w::<ListControl>(me);
            (w.scroll_h, w.show_row_separators, w.col_labels)
        };
        // L211
        cx.sg.set_viewport(
            x + 2.0,
            y + 2.0,
            if scroll { width - 20.0 } else { width },
            height - 4.0 - if scroll && scroll_h { 16.0 } else { 0.0 },
        );
        // L212
        let text_offset_y = if show_row_separators { 2.0 } else { 0.0 };
        // L213
        let text_height = row_height - text_offset_y * 2.0;
        // L214
        let mut tt: Option<(usize, ListValue, f64, f64, f64)> = None;
        // L215
        let min_index = m_floor(scroll_offset_v / row_height + 1.0);
        // L216
        let max_index = m_min(
            m_floor((scroll_offset_v + height) / row_height + 1.0),
            n as f64,
        );
        let get_row_value = r_get_row_value(cx, me);
        let get_row_icon = r_get_row_icon(cx, me);
        let set_highlight_color = r_set_highlight_color(cx, me);
        let lab_y = if col_labels { 18.0 } else { 0.0 };
        // L217
        for col_index in 1..=n_cols {
            // L218
            let col_font = col_prop(cx, me, col_index, "font")
                .str()
                .and_then(Font::from_lua)
                .unwrap_or(Font::Var);
            // L219
            let clip_width = cx.sg.draw_string_width(text_height, col_font, "...");
            let (c_off, c_w) = {
                let c = &cx.w::<ListControl>(me).col_list[col_index - 1];
                (num(c.offset), num(c.computed_width))
            };
            // L220
            col_offset = c_off - scroll_offset_h;
            // L221
            let col_width = c_w;
            // L222
            let rel_x = cursor_x - (x + 2.0);
            // L223
            let rel_y = cursor_y - (y + 2.0);
            // L224
            let mut index_f = min_index;
            while index_f <= max_index {
                let index = index_f as usize;
                // L225
                let line_y = row_height * (index_f - 1.0) - scroll_offset_v + lab_y;
                // L226
                let value = Self::list_get(cx, me, index).unwrap_or_else(|| {
                    crate::lua_error!("attempt to index a nil value (list[{}])", index)
                });
                // L227
                let mut text = get_row_value(cx, me, col_index, index, &value);
                // L228
                let mut icon = None;
                // L229
                if let Some(f) = &get_row_icon {
                    // L230
                    icon = f(cx, me, col_index, index, &value);
                }
                // L232
                let mut text_width = cx.sg.draw_string_width(text_height, col_font, &text);
                // L233
                if text_width > col_width - 2.0 {
                    // L234
                    let clip_index = cx.sg.draw_string_cursor_index(
                        text_height,
                        col_font,
                        &text,
                        col_width - clip_width - 2.0,
                        0.0,
                    );
                    // L235
                    let mut b = LBuf::new();
                    b.push_sub(&text, 1.0, Some(clip_index as f64 - 1.0))
                        .push_str("...");
                    text = b.into_string();
                    // L236
                    text_width = cx.sg.draw_string_width(text_height, col_font, &text);
                }
                // L238
                let (sel_drag_active, other_src) = {
                    let w = cx.w::<ListControl>(me);
                    (w.sel_drag_active, w.other_drag_source)
                };
                let sb_dragging = cx.w::<ScrollBarControl>(scroll_bar_v).dragging;
                if !sb_dragging
                    && (!sel_drag_active
                        || has_can_drag
                            .as_ref()
                            .is_some_and(|f| f(cx, me, index, &value, other_src)))
                {
                    // L239
                    if rel_x >= col_offset
                        && rel_x < if scroll { width - 20.0 } else { width }
                        && rel_y >= 0.0
                        && rel_y >= line_y
                        && rel_y < height - 2.0 - if scroll && scroll_h { 18.0 } else { 0.0 }
                        && rel_y < line_y + row_height
                    {
                        // L240-L244
                        tt = Some((
                            index,
                            value.clone(),
                            x + 2.0 + col_offset,
                            line_y + y + 2.0,
                            m_max(text_width + 8.0, rel_x - col_offset),
                        ));
                    }
                }
                let sel_value = cx.w::<ListControl>(me).sel_value.clone();
                let is_sel = sel_value.as_ref() == Some(&value);
                let is_tt = tt.as_ref().map(|t| &t.1) == Some(&value);
                let row_w = if !scroll { col_width - 4.0 } else { col_width };
                // L247
                if show_row_separators {
                    // L248
                    if has_focus && is_sel {
                        cx.sg.set_draw_color(1.0, 1.0, 1.0);
                    } else if is_tt {
                        // L251
                        cx.sg.set_draw_color(0.8, 0.8, 0.8);
                    } else {
                        // L253
                        cx.sg.set_draw_color(0.5, 0.5, 0.5);
                    }
                    // L255
                    cx.sg
                        .draw_image(None, col_offset, line_y, row_w, row_height);
                    // L256
                    if is_sel || is_tt {
                        // L257
                        cx.sg.set_draw_color(0.33, 0.33, 0.33);
                    } else if other_src.is_some()
                        && has_can_drag
                            .as_ref()
                            .is_some_and(|f| f(cx, me, index, &value, other_src))
                    {
                        // L259
                        cx.sg.set_draw_color(0.0, 0.2, 0.0);
                    } else if lmod(index_f, 2.0) == 0.0 {
                        // L261
                        cx.sg.set_draw_color(0.05, 0.05, 0.05);
                    } else {
                        // L263
                        cx.sg.set_draw_color(0.0, 0.0, 0.0);
                    }
                    // L265
                    cx.sg
                        .draw_image(None, col_offset, line_y + 1.0, row_w, row_height - 2.0);
                } else if is_sel || is_tt {
                    // L267
                    if has_focus && is_sel {
                        cx.sg.set_draw_color(1.0, 1.0, 1.0);
                    } else if is_tt {
                        // L270
                        cx.sg.set_draw_color(0.8, 0.8, 0.8);
                    } else {
                        // L272
                        cx.sg.set_draw_color(0.5, 0.5, 0.5);
                    }
                    // L274
                    cx.sg
                        .draw_image(None, col_offset, line_y, row_w, row_height);
                    // L275
                    if other_src.is_some()
                        && has_can_drag
                            .as_ref()
                            .is_some_and(|f| f(cx, me, index, &value, other_src))
                    {
                        // L276
                        cx.sg.set_draw_color(0.0, 0.2, 0.0);
                    } else {
                        // L278
                        cx.sg.set_draw_color(0.15, 0.15, 0.15);
                    }
                    // L280
                    cx.sg
                        .draw_image(None, col_offset, line_y + 1.0, row_w, row_height - 2.0);
                }
                // L282
                if !set_highlight_color
                    .as_ref()
                    .is_some_and(|f| f(cx, me, index, &value))
                {
                    // L283
                    cx.sg.set_draw_color(1.0, 1.0, 1.0);
                }
                // L285 TODO: handle icon size properly, for now assume they are 16x16
                // L286
                if let Some(icon) = icon {
                    // L289
                    cx.sg
                        .draw_image(Some(&icon), col_offset, line_y, 16.0, 16.0);
                    // L290
                    cx.sg.draw_string(
                        col_offset + 16.0 + 2.0,
                        line_y + text_offset_y,
                        Align::Left,
                        text_height,
                        col_font,
                        &text,
                    );
                } else {
                    // L287
                    cx.sg.draw_string(
                        col_offset,
                        line_y + text_offset_y,
                        Align::Left,
                        text_height,
                        col_font,
                        &text,
                    );
                }
                index_f += 1.0;
            }
            // L293
            if col_labels {
                // L294
                let m_over = rel_x >= col_offset
                    && rel_x <= col_offset + col_width
                    && rel_y >= 0.0
                    && rel_y <= 18.0;
                // L295
                if m_over && col_prop(cx, me, col_index, "sortable").truthy() {
                    // L296
                    cx.sg.set_draw_color(1.0, 1.0, 1.0);
                    // L297
                    cx.sg.draw_image(None, col_offset, 1.0, col_width, 18.0);
                    // L298
                    cx.sg.set_draw_color(0.33, 0.33, 0.33);
                    // L299
                    cx.sg
                        .draw_image(None, col_offset + 1.0, 2.0, col_width - 2.0, 16.0);
                } else {
                    // L301
                    cx.sg.set_draw_color(0.5, 0.5, 0.5);
                    // L302
                    cx.sg.draw_image(None, col_offset, 1.0, col_width, 18.0);
                    // L303
                    cx.sg.set_draw_color(0.15, 0.15, 0.15);
                    // L304
                    cx.sg
                        .draw_image(None, col_offset + 1.0, 2.0, col_width - 2.0, 16.0);
                }
                // L306
                let label = col_prop(cx, me, col_index, "label");
                // L307
                if let Some(label) = label.str().filter(|l| !l.is_empty()) {
                    // L308
                    cx.sg.set_draw_color(1.0, 1.0, 1.0);
                    // L309
                    cx.sg.draw_string(
                        col_offset + col_width / 2.0,
                        4.0,
                        Align::CenterX,
                        12.0,
                        Font::Var,
                        label,
                    );
                }
            }
        }
        // L313
        let default_text = cx.w::<ListControl>(me).default_text.clone();
        if Self::list_len(cx, me) == 0 {
            if let Some(default_text) = default_text {
                // L314
                cx.sg.set_draw_color(1.0, 1.0, 1.0);
                // L315
                let font = cx.w::<ListControl>(me).font;
                cx.sg
                    .draw_string(2.0, 2.0, Align::Left, 14.0, font, &default_text);
            }
        }
        // L317
        if let Some(sel_drag_index) = cx.w::<ListControl>(me).sel_drag_index {
            // L318
            let line_y = row_height * (sel_drag_index as f64 - 1.0) - scroll_offset_v;
            // L319
            cx.sg.set_draw_color(1.0, 1.0, 1.0);
            // L320
            cx.sg.draw_image(None, 0.0, line_y - 1.0, width - 20.0, 3.0);
            // L321
            cx.sg.set_draw_color(0.0, 0.0, 0.0);
            // L322
            cx.sg.draw_image(None, 0.0, line_y, width - 20.0, 1.0);
        }
        // L324
        cx.sg.reset_viewport();

        // L326
        let (sel_drag_active, has_targets, is_mutable) = {
            let w = cx.w::<ListControl>(me);
            (
                w.sel_drag_active,
                w.drag_target_list.is_some(),
                w.is_mutable,
            )
        };
        if sel_drag_active && has_targets && (!is_mutable || !cx.is_mouse_over(me)) {
            // L327
            let (si, sv) = {
                let w = cx.w::<ListControl>(me);
                (w.sel_index, w.sel_value.clone())
            };
            let (Some(si), Some(sv)) = (si, sv) else {
                crate::lua_error!("attempt to index a nil value (GetRowValue of selValue)")
            };
            let text = get_row_value(cx, me, 1, si, &sv);
            cx.main.show_drag_text = Some(text);
        }

        // L330-L331
        {
            let w = cx.w_mut::<ListControl>(me);
            w.hover_index = tt.as_ref().map(|t| t.0);
            w.hover_value = tt.as_ref().map(|t| t.1.clone());
        }
        // L332
        if let Some((tt_index, tt_value, tt_x, tt_y, tt_width)) = tt {
            if let Some(add) = r_add_value_tooltip(cx, me) {
                if !no_tooltip || force_tooltip {
                    // L333
                    cx.sg.set_draw_layer(None, Some(100));
                    cx.lend::<ListControl, Tooltip, ()>(
                        me,
                        |w| &mut w.tooltip,
                        |cx, tooltip| {
                            // L334
                            add(cx, me, tooltip, tt_index, &tt_value);
                            // L335
                            tooltip.draw(cx, tt_x, tt_y, Some(tt_width), Some(row_height), vp);
                        },
                    );
                    // L336
                    cx.sg.set_draw_layer(None, Some(0));
                }
            }
        }
    }

    /// lua: ListControl.lua:340-418
    /// @pob Classes/ListControl.lua:340-418 h=11a6f891
    pub fn on_key_down(cx: &mut Cx, me: CtrlId, key: Key, dbl: bool) -> Focus {
        // L341
        if !cx.is_shown(me) || !cx.is_enabled(me) {
            // L342
            return None;
        }
        // L344
        let m_over_control = cx.get_mouse_over_control(me);
        // L345
        if let Some(m) = m_over_control {
            if cx.has(m, Caps::KEY_DOWN) {
                // L346 (OnKeyDown(key): doubleClick nil)
                return cx.on_key_down(m, key, false);
            }
        }
        // L348
        if !cx.is_mouse_over(me) && key.name().contains("BUTTON") {
            // L349
            return None;
        }
        // L351
        if key == Key::LeftButton {
            // L352
            let mut new_select: usize = 0;
            // L353
            let (x, y) = cx.pos(me);
            // L354
            let (cursor_x, cursor_y) = cx.sys.cursor_pos();
            // L355
            let row_region = Self::get_row_region(cx, me);
            // L356
            if cursor_x >= x + row_region.x
                && cursor_y >= y + row_region.y
                && cursor_x < x + row_region.x + row_region.width
                && cursor_y < y + row_region.y + row_region.height
            {
                // L357
                let offset = cx.w::<ScrollBarControl>(Self::sb_v(cx, me)).offset;
                let row_height = cx.w::<ListControl>(me).row_height;
                let index = m_floor((cursor_y - y - row_region.y + offset) / row_height) + 1.0;
                // L358
                if Self::list_get_num(cx, me, index).is_some() {
                    // L359
                    new_select = index as usize;
                }
            } else {
                // L362
                let scroll_offset_h = cx.w::<ScrollBarControl>(Self::sb_h(cx, me)).offset;
                // L363
                let n_cols = cx.w::<ListControl>(me).col_list.len();
                for col_index in 1..=n_cols {
                    // L364
                    let rel_x = cursor_x - (x + 2.0);
                    // L365
                    let rel_y = cursor_y - (y + 2.0);
                    // L366
                    let adjusted_rel_x = rel_x + scroll_offset_h;
                    // L367
                    let (c_off, c_w) = {
                        let c = &cx.w::<ListControl>(me).col_list[col_index - 1];
                        (c.offset, c.computed_width)
                    };
                    let m_over = adjusted_rel_x >= num(c_off)
                        && adjusted_rel_x <= num(c_off) + num(c_w)
                        && rel_y >= 0.0
                        && rel_y <= 18.0;
                    // L368
                    if col_prop(cx, me, col_index, "sortable").truthy() && m_over {
                        if let Some(f) = r_re_sort(cx, me) {
                            // L369
                            f(cx, me, col_index);
                        }
                    }
                }
            }

            // L374
            if Self::select_index(cx, me, new_select) {
                // L375
                let (is_mutable, has_targets) = {
                    let w = cx.w::<ListControl>(me);
                    (w.is_mutable, w.drag_target_list.is_some())
                };
                if (is_mutable || has_targets) && cx.is_shown(me) {
                    let w = cx.w_mut::<ListControl>(me);
                    // L376
                    w.sel_cx = Some(cursor_x);
                    // L377
                    w.sel_cy = Some(cursor_y);
                    // L378
                    w.sel_dragging = true;
                    // L379
                    w.sel_drag_active = false;
                }
                // L381
                if let Some(f) = r_on_sel_click(cx, me) {
                    let (si, sv) = Self::sel(cx, me);
                    // L382
                    if f(cx, me, si, &sv, dbl) == Some(false) {
                        // L383
                        return None;
                    }
                }
            }
        } else if Self::list_len(cx, me) > 0 && !cx.w::<ListControl>(me).sel_drag_active {
            // L387
            let n = Self::list_len(cx, me) as f64;
            let sel_index = cx.w::<ListControl>(me).sel_index.map(|i| i as f64);
            // L388
            if key == Key::Up {
                // L389
                let i = lmod(sel_index.unwrap_or(1.0) - 2.0, n) + 1.0;
                Self::select_index(cx, me, i as usize);
            } else if key == Key::Down {
                // L391
                let i = lmod(sel_index.unwrap_or(n), n) + 1.0;
                Self::select_index(cx, me, i as usize);
            } else if key == Key::Home {
                // L393
                Self::select_index(cx, me, 1);
            } else if key == Key::End {
                // L395
                Self::select_index(cx, me, n as usize);
            } else if cx.w::<ListControl>(me).sel_value.is_some() {
                // L396
                let ctrl = cx.sys.is_key_down(Key::Ctrl);
                // L397
                if key == Key::Char(b'c') && ctrl {
                    // L398
                    if let Some(f) = r_on_sel_copy(cx, me) {
                        let (si, sv) = Self::sel(cx, me);
                        // L399
                        f(cx, me, si, &sv);
                    }
                } else if key == Key::Char(b'x') && ctrl {
                    // L402
                    if let Some(f) = r_on_sel_cut(cx, me) {
                        let (si, sv) = Self::sel(cx, me);
                        // L403
                        f(cx, me, si, &sv);
                    }
                } else if key == Key::Back || key == Key::Delete {
                    // L406
                    if let Some(f) = r_on_sel_delete(cx, me) {
                        let (si, sv) = Self::sel(cx, me);
                        // L407
                        f(cx, me, si, &sv);
                    }
                } else if let Some(f) = r_on_sel_key_down(cx, me) {
                    // L409
                    let (si, sv) = Self::sel(cx, me);
                    // L410
                    f(cx, me, si, &sv, key);
                }
            }
            // L413
            if let Some(f) = r_on_any_key_down(cx, me) {
                // L414
                f(cx, me, key);
            }
        }
        // L417
        Some(me)
    }

    /// `self.selIndex, self.selValue` as hook arguments (both set whenever selValue is).
    #[track_caller]
    fn sel(cx: &Cx, me: CtrlId) -> (usize, ListValue) {
        let w = cx.w::<ListControl>(me);
        match (w.sel_index, w.sel_value.clone()) {
            (Some(i), Some(v)) => (i, v),
            _ => crate::lua_error!("attempt to index a nil value (selIndex/selValue)"),
        }
    }

    /// lua: ListControl.lua:420-471
    /// @pob Classes/ListControl.lua:420-471 h=86296b78
    pub fn on_key_up(cx: &mut Cx, me: CtrlId, key: Key) -> Focus {
        // L421
        if !cx.is_shown(me) || !cx.is_enabled(me) {
            // L422
            return None;
        }
        let sbv = Self::sb_v(cx, me);
        let (scroll, scroll_h) = {
            let w = cx.w::<ListControl>(me);
            (w.scroll, w.scroll_h)
        };
        // L424
        if ScrollBarControl::is_scroll_down_key(cx, sbv, key) {
            // L425
            if scroll && scroll_h && cx.sys.is_key_down(Key::Shift) {
                // L426
                let sbh = Self::sb_h(cx, me);
                ScrollBarControl::scroll(cx, sbh, 1.0);
            } else {
                // L428
                ScrollBarControl::scroll(cx, sbv, 1.0);
            }
        } else if ScrollBarControl::is_scroll_up_key(cx, sbv, key) {
            // L431
            if scroll && scroll_h && cx.sys.is_key_down(Key::Shift) {
                // L432
                let sbh = Self::sb_h(cx, me);
                ScrollBarControl::scroll(cx, sbh, -1.0);
            } else {
                // L434
                ScrollBarControl::scroll(cx, sbv, -1.0);
            }
        } else if cx.w::<ListControl>(me).sel_value.is_some() {
            // L437
            if key == Key::LeftButton {
                // L438
                cx.w_mut::<ListControl>(me).sel_dragging = false;
                // L439
                if cx.w::<ListControl>(me).sel_drag_active {
                    // L440
                    cx.w_mut::<ListControl>(me).sel_drag_active = false;
                    let (sel_drag_index, sel_index, drag_target) = {
                        let w = cx.w::<ListControl>(me);
                        (w.sel_drag_index, w.sel_index, w.drag_target)
                    };
                    // L441
                    if let Some(sdi) = sel_drag_index.filter(|&d| Some(d) != sel_index) {
                        let si = sel_index
                            .unwrap_or_else(|| crate::lua_error!("bad argument #2 to 'remove'"));
                        // L442
                        Self::list_remove(cx, me, si);
                        // L443
                        let sdi = if sdi > si {
                            // L444
                            sdi - 1
                        } else {
                            sdi
                        };
                        cx.w_mut::<ListControl>(me).sel_drag_index = Some(sdi);
                        // L446
                        let sv = cx.w::<ListControl>(me).sel_value.clone().expect("checked");
                        Self::list_insert(cx, me, sdi, sv);
                        // L447
                        if let Some(f) = r_on_order_change(cx, me) {
                            // L448
                            f(cx, me, si, sdi);
                        }
                        // L450
                        cx.w_mut::<ListControl>(me).sel_value = None;
                    } else if let Some(target) = drag_target {
                        // L452
                        let (ty, val) = {
                            let w = cx.w::<ListControl>(me);
                            drag_args(&w.drag_type, &w.drag_value)
                        };
                        cx.receive_drag(target, &ty, &val, me);
                        // L453
                        if let Some(f) = r_on_drag_send(cx, me) {
                            // L454
                            let (si, sv) = Self::sel(cx, me);
                            f(cx, me, si, &sv, target);
                        }
                        // L456
                        cx.w_mut::<ListControl>(me).sel_value = None;
                    }
                    // L458
                    if let Some(targets) = cx.w::<ListControl>(me).drag_target_list.clone() {
                        // L459
                        for target in targets {
                            // L460
                            set_other_drag_source(cx, target, None);
                            // L461
                            set_other_drag_targeting(cx, target, false);
                        }
                    }
                    let w = cx.w_mut::<ListControl>(me);
                    // L464
                    w.drag_type = None;
                    // L465
                    w.drag_value = None;
                    // L466
                    w.drag_target = None;
                }
            }
        }
        // L470
        Some(me)
    }

    /// lua: ListControl.lua:473-483
    /// @pob Classes/ListControl.lua:473-483 h=153ab89a
    pub fn get_hover_index(cx: &Cx, me: CtrlId) -> Option<usize> {
        // L474
        let (x, y) = cx.pos(me);
        // L475
        let (cursor_x, cursor_y) = cx.sys.cursor_pos();
        // L476
        let row_region = Self::get_row_region(cx, me);
        // L477
        if cursor_x >= x + row_region.x
            && cursor_y >= y + row_region.y
            && cursor_x < x + row_region.x + row_region.width
            && cursor_y < y + row_region.y + row_region.height
        {
            // L478
            let offset = cx.w::<ScrollBarControl>(Self::sb_v(cx, me)).offset;
            let index = m_floor(
                (cursor_y - y - row_region.y + offset) / cx.w::<ListControl>(me).row_height,
            ) + 1.0;
            // L479
            if Self::list_get_num(cx, me, index).is_some() {
                // L480
                return Some(index as usize);
            }
        }
        None
    }

    /// lua: ListControl.lua:485-493 GetHoverValue(key) (the argument is unused)
    /// @pob Classes/ListControl.lua:485-493 h=d41c6769
    pub fn get_hover_value(cx: &Cx, me: CtrlId) -> Option<ListValue> {
        // L486
        let index = Self::get_hover_index(cx, me)?;
        // L488
        let value = Self::list_get(cx, me, index)?;
        // L490
        Some(value)
    }

    /// ControlHost.lua:20's duck check `selDragActive and dragTargetList`.
    pub fn duck(cx: &Cx, me: CtrlId, q: Duck) -> bool {
        match q {
            Duck::SelDragActiveWithTargets => {
                let w = cx.w::<ListControl>(me);
                w.sel_drag_active && w.drag_target_list.is_some()
            }
            Duck::ImbuedSelectDropped => false,
        }
    }
}

/// `self.dragType, self.dragValue` as call arguments (DIVERGENCE(nil-drag), module docs).
fn drag_args(ty: &Option<Rc<str>>, v: &Option<DragValue>) -> (Rc<str>, DragValue) {
    (
        ty.clone().unwrap_or_else(|| Rc::from("")),
        v.clone().unwrap_or_else(|| DragValue(Rc::new(()))),
    )
}

/// The SimpleGraphic font name (`"VAR"`, ...), the inverse of `Font::from_lua`.
fn font_name(f: Font) -> &'static str {
    match f {
        Font::Fixed => "FIXED",
        Font::Var => "VAR",
        Font::VarBold => "VAR BOLD",
        Font::FontinSc => "FONTIN SC",
        Font::FontinScItalic => "FONTIN SC ITALIC",
        Font::Fontin => "FONTIN",
        Font::FontinItalic => "FONTIN ITALIC",
    }
}

impl Widget for ListControl {
    const NAME: &'static str = "ListControl";
    const CAPS: Caps = Caps::DRAW
        .union(Caps::IS_MOUSE_OVER)
        .union(Caps::KEY_DOWN)
        .union(Caps::KEY_UP);

    fn draw(cx: &mut Cx, me: CtrlId, vp: &ViewPort, no_tooltip: bool) {
        ListControl::draw(cx, me, vp, no_tooltip)
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
}

#[cfg(test)]
mod tests {
    //! Unit checks of the parts the galleries do not observe through draw calls: model aliasing
    //! through `ListSource`, SelectIndex's nil and override paths, the drag-reorder write-through,
    //! hook resolution (instance wins), and the drag-target field storage.
    use std::cell::Cell;

    use super::*;

    struct Host;
    impl Widget for Host {
        const NAME: &'static str = "ControlHost";
        const CAPS: Caps = Caps::empty();
    }

    fn label(v: &ListValue) -> String {
        v.get::<String>().expect("String row").clone()
    }

    fn grv(_cx: &Cx, _me: CtrlId, _c: usize, _i: usize, v: &ListValue) -> String {
        label(v)
    }

    thread_local! {
        static ORDER: Cell<(usize, usize)> = const { Cell::new((0, 0)) };
        static SELECTED: Cell<usize> = const { Cell::new(0) };
    }

    fn on_order_change(_cx: &mut Cx, _me: CtrlId, a: usize, b: usize) {
        ORDER.with(|o| o.set((a, b)));
    }
    fn on_select(_cx: &mut Cx, _me: CtrlId, i: usize, _v: &ListValue) {
        SELECTED.with(|s| s.set(i));
    }

    static HOOKS: ListHooks = ListHooks {
        get_row_value: grv,
        on_order_change: Some(on_order_change),
        on_select: Some(on_select),
        ..ListHooks::BASE
    };

    fn setup(list: ListData) -> (Cx, CtrlId) {
        let mut cx = Cx::headless();
        let host = cx
            .add_free(
                None,
                New::new(Base::new(Anchor::NONE, rect(0, 0, 0, 0)), Host).host(),
            )
            .id();
        let n = New::new(
            Base::new(Anchor::NONE, rect(0, 0, 200, 100)),
            ListControl::state(&HOOKS, 16.0, true, true, list, false),
        )
        .host()
        .init(|cx, me| ListControl::init(cx, me.0));
        let me = cx.add(host, "list", n).id();
        (cx, me)
    }

    fn shared(labels: &[&str]) -> pob_lua::Shared<Vec<Rc<String>>> {
        pob_lua::Shared::new(labels.iter().map(|s| Rc::new((*s).to_owned())).collect())
    }

    fn model_labels(model: &pob_lua::Shared<Vec<Rc<String>>>) -> Vec<String> {
        model.with(|l| l.iter().map(|s| (**s).clone()).collect())
    }

    #[test]
    fn shared_source_aliases_the_model() {
        let model = shared(&["a", "b", "c"]);
        let (mut cx, me) = setup(ListData::Source(Rc::new(SharedVecSource(model.clone()))));
        assert_eq!(ListControl::list_len(&cx, me), 3);
        assert_eq!(label(&ListControl::list_get(&cx, me, 2).unwrap()), "b");
        assert!(ListControl::list_get(&cx, me, 0).is_none());
        assert!(ListControl::list_get(&cx, me, 4).is_none());
        // identity: the same Rc gives the same ValKey
        let b1 = ListControl::list_get(&cx, me, 2).unwrap();
        assert_eq!(b1, ListControl::list_get(&cx, me, 2).unwrap());
        // remove + insert write through to the model (OnKeyUp's reorder)
        let v = ListControl::list_remove(&mut cx, me, 1).unwrap();
        ListControl::list_insert(&mut cx, me, 3, v);
        assert_eq!(model_labels(&model), ["b", "c", "a"]);
        // t_remove out of range is a no-op
        assert!(ListControl::list_remove(&mut cx, me, 9).is_none());
    }

    #[test]
    fn select_index_nil_clears_sel_value() {
        let model = shared(&["a", "b"]);
        let (mut cx, me) = setup(ListData::Source(Rc::new(SharedVecSource(model))));
        assert!(ListControl::select_index(&mut cx, me, 2));
        assert_eq!(SELECTED.with(Cell::get), 2);
        assert_eq!(cx.w::<ListControl>(me).sel_index, Some(2));
        // SelectIndex(nil): selValue = nil, selIndex kept, false
        assert!(!ListControl::select_index(&mut cx, me, 0));
        assert!(cx.w::<ListControl>(me).sel_value.is_none());
        assert_eq!(cx.w::<ListControl>(me).sel_index, Some(2));
    }

    #[test]
    fn override_select_index_keeps_index_but_sets_value() {
        let model = shared(&["a", "b", "c"]);
        let (mut cx, me) = setup(ListData::Source(Rc::new(SharedVecSource(model))));
        assert!(ListControl::select_index(&mut cx, me, 1));
        cx.w_mut::<ListControl>(me).ov.override_select_index = Some(Rc::new(|_, _, _| true));
        assert!(!ListControl::select_index(&mut cx, me, 3));
        let w = cx.w::<ListControl>(me);
        // L90 runs before the override: selValue is row 3 while selIndex stays 1
        assert_eq!(w.sel_index, Some(1));
        assert_eq!(label(w.sel_value.as_ref().unwrap()), "c");
    }

    #[test]
    fn drag_reorder_on_key_up() {
        let model = shared(&["a", "b", "c", "d"]);
        let (mut cx, me) = setup(ListData::Source(Rc::new(SharedVecSource(model.clone()))));
        assert!(ListControl::select_index(&mut cx, me, 1));
        {
            let w = cx.w_mut::<ListControl>(me);
            w.sel_dragging = true;
            w.sel_drag_active = true;
            w.sel_drag_index = Some(4);
        }
        assert_eq!(
            ListControl::on_key_up(&mut cx, me, Key::LeftButton),
            Some(me)
        );
        assert_eq!(model_labels(&model), ["b", "c", "a", "d"]);
        assert_eq!(ORDER.with(Cell::get), (1, 3));
        let w = cx.w::<ListControl>(me);
        assert!(w.sel_value.is_none() && !w.sel_drag_active && !w.sel_dragging);
    }

    #[test]
    fn instance_hook_wins_and_presence() {
        let (mut cx, me) = setup(ListData::Local(vec![ListValue::of(Rc::new(
            "x".to_owned(),
        ))]));
        assert!(r_on_sel_copy(&cx, me).is_none());
        assert!(r_on_select(&cx, me).is_some());
        cx.w_mut::<ListControl>(me).ov.get_row_value =
            Some(Rc::new(|_, _, _, _, _| "instance".to_owned()));
        let v = ListControl::list_get(&cx, me, 1).unwrap();
        assert_eq!(r_get_row_value(&cx, me)(&cx, me, 1, 1, &v), "instance");
    }

    #[test]
    fn drag_target_fields_for_any_control() {
        let (mut cx, me) = setup(ListData::default());
        let other = cx
            .add_free(
                None,
                New::new(Base::new(Anchor::NONE, rect(0, 0, 0, 0)), Host),
            )
            .id();
        assert_eq!(other_drag_source(&cx, other), None);
        set_other_drag_source(&mut cx, other, Some(me));
        set_other_drag_targeting(&mut cx, other, true);
        assert_eq!(other_drag_source(&cx, other), Some(me));
        assert!(other_drag_targeting(&cx, other));
        set_other_drag_source(&mut cx, other, None);
        set_other_drag_targeting(&mut cx, other, false);
        assert_eq!(other_drag_source(&cx, other), None);
        assert!(DRAG_TARGET_FIELDS.with(|m| m.borrow().is_empty()));
        // a ListControl target uses its own field
        set_other_drag_source(&mut cx, me, Some(other));
        assert_eq!(cx.w::<ListControl>(me).other_drag_source, Some(other));
    }

    #[test]
    fn column_properties() {
        let (mut cx, me) = setup(ListData::default());
        cx.w_mut::<ListControl>(me).col_list = vec![
            ListColumn::new(
                Some(Prop::f(|cx, me| cx.width(me) - 172.0)),
                Some("A"),
                true,
            ),
            ListColumn::default(),
        ];
        assert_eq!(
            ListControl::get_column_property(&cx, me, 1, "width"),
            ColumnProp::Num(28.0)
        );
        assert_eq!(
            ListControl::get_column_property(&cx, me, 1, "label"),
            ColumnProp::Str("A".into())
        );
        assert!(ListControl::get_column_property(&cx, me, 1, "sortable").truthy());
        assert_eq!(
            ListControl::get_column_property(&cx, me, 2, "width"),
            ColumnProp::Nil
        );
        assert!(!ListControl::get_column_property(&cx, me, 2, "sortable").truthy());
    }
}
