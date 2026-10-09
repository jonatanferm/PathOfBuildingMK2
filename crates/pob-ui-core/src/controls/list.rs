//! Port of PathOfBuilding v2.67.2 (16de4b82d) src/Classes/ListControl.lua 1-493
//! (`newClass("ListControl", "Control", "ControlHost")`). Owner: U12.
//! UI-U0 stub: the ui doc §6.8 frozen surface (ListValue, ListSource, ListHooks, ListOverrides,
//! ListControl). The hook set is re-checked by U12 against every subclass before the wave-2 gate.

use std::any::Any;
use std::rc::Rc;

use pob_sg::Key;

use crate::prelude::*;
use crate::tooltip::Tooltip;

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
    fn len(&self, cx: &Cx) -> usize {
        let _ = cx;
        todo!("U12: SharedVecSource::len")
    }
    fn get(&self, cx: &Cx, i: usize) -> Option<ListValue> {
        let _ = (cx, i);
        todo!("U12: SharedVecSource::get")
    }
    fn insert(&self, cx: &mut Cx, i: usize, v: ListValue) {
        let _ = (cx, i, v);
        todo!("U12: SharedVecSource::insert")
    }
    fn remove(&self, cx: &mut Cx, i: usize) -> ListValue {
        let _ = (cx, i);
        todo!("U12: SharedVecSource::remove")
    }
}

pub enum ListData {
    Local(Vec<ListValue>),
    Source(Rc<dyn ListSource>),
}

/// One `colList[i]` entry (`{ width, sortable, label }`; ListControl.lua:44-87).
#[derive(Clone)]
pub struct ListColumn {
    pub width: Prop<f64>,
    pub label: Option<String>,
    pub sortable: bool,
}

/// What GetColumnProperty returns (ListControl.lua:111-117: a column field, maybe a function).
#[derive(Clone, Debug, PartialEq)]
pub enum ColumnProp {
    Nil,
    Num(f64),
    Str(String),
    Bool(bool),
}

/// ListControl.lua:6-24 + every `self.X and self:X(..)` call site in ListControl.lua
pub struct ListHooks {
    pub get_row_value:
        fn(&Cx, CtrlId, usize /*column*/, usize /*index*/, &ListValue) -> String,
    pub get_row_icon:
        Option<fn(&Cx, CtrlId, usize, usize, &ListValue) -> Option<pob_sg::ImageHandle>>,
    pub add_value_tooltip: Option<fn(&mut Cx, CtrlId, &mut Tooltip, usize, &ListValue)>,
    pub get_drag_value: Option<fn(&Cx, CtrlId, usize, &ListValue) -> Option<(Rc<str>, DragValue)>>,
    pub on_drag_send: Option<fn(&mut Cx, CtrlId, usize, &ListValue, CtrlId)>,
    pub on_order_change: Option<fn(&mut Cx, CtrlId)>,
    pub on_select: Option<fn(&mut Cx, CtrlId, usize, &ListValue)>,
    /// Some(false) releases focus
    pub on_sel_click: Option<fn(&mut Cx, CtrlId, usize, &ListValue, bool) -> Option<bool>>,
    pub on_sel_copy: Option<fn(&mut Cx, CtrlId, usize, &ListValue)>,
    pub on_sel_cut: Option<fn(&mut Cx, CtrlId, usize, &ListValue)>,
    pub on_sel_delete: Option<fn(&mut Cx, CtrlId, usize, &ListValue)>,
    pub on_sel_key_down: Option<fn(&mut Cx, CtrlId, usize, &ListValue, Key)>,
    pub on_any_key_down: Option<fn(&mut Cx, CtrlId, Key)>,
    pub override_select_index: Option<fn(&mut Cx, CtrlId, usize) -> bool>,
    pub set_highlight_color: Option<fn(&mut Cx, CtrlId, usize, &ListValue) -> bool>,
    pub can_drag_to_value: Option<fn(&Cx, CtrlId, usize, &ListValue, CtrlId) -> bool>,
    pub re_sort: Option<fn(&mut Cx, CtrlId, usize)>,
    /// default = ListControl's own
    pub get_column_property: fn(&Cx, CtrlId, usize, &str) -> ColumnProp,
}

/// Same slots as Rc closures, for per-instance overrides (CompareTab.lua:1614-1627). Instance wins.
#[derive(Default, Clone)]
pub struct ListOverrides {
    pub get_row_value: Option<Rc<dyn Fn(&Cx, CtrlId, usize, usize, &ListValue) -> String>>,
    pub get_row_icon:
        Option<Rc<dyn Fn(&Cx, CtrlId, usize, usize, &ListValue) -> Option<pob_sg::ImageHandle>>>,
    pub add_value_tooltip: Option<Rc<dyn Fn(&mut Cx, CtrlId, &mut Tooltip, usize, &ListValue)>>,
    pub get_drag_value:
        Option<Rc<dyn Fn(&Cx, CtrlId, usize, &ListValue) -> Option<(Rc<str>, DragValue)>>>,
    pub on_drag_send: Option<Rc<dyn Fn(&mut Cx, CtrlId, usize, &ListValue, CtrlId)>>,
    pub on_order_change: Option<Rc<dyn Fn(&mut Cx, CtrlId)>>,
    pub on_select: Option<Rc<dyn Fn(&mut Cx, CtrlId, usize, &ListValue)>>,
    pub on_sel_click: Option<Rc<dyn Fn(&mut Cx, CtrlId, usize, &ListValue, bool) -> Option<bool>>>,
    pub on_sel_copy: Option<Rc<dyn Fn(&mut Cx, CtrlId, usize, &ListValue)>>,
    pub on_sel_cut: Option<Rc<dyn Fn(&mut Cx, CtrlId, usize, &ListValue)>>,
    pub on_sel_delete: Option<Rc<dyn Fn(&mut Cx, CtrlId, usize, &ListValue)>>,
    pub on_sel_key_down: Option<Rc<dyn Fn(&mut Cx, CtrlId, usize, &ListValue, Key)>>,
    pub on_any_key_down: Option<Rc<dyn Fn(&mut Cx, CtrlId, Key)>>,
    pub override_select_index: Option<Rc<dyn Fn(&mut Cx, CtrlId, usize) -> bool>>,
    pub set_highlight_color: Option<Rc<dyn Fn(&mut Cx, CtrlId, usize, &ListValue) -> bool>>,
    pub can_drag_to_value: Option<Rc<dyn Fn(&Cx, CtrlId, usize, &ListValue, CtrlId) -> bool>>,
    pub re_sort: Option<Rc<dyn Fn(&mut Cx, CtrlId, usize)>>,
    pub get_column_property: Option<Rc<dyn Fn(&Cx, CtrlId, usize, &str) -> ColumnProp>>,
}

pub struct ListControl {
    pub hooks: &'static ListHooks,
    pub ov: ListOverrides,
    pub list: ListData,
    pub row_height: f64,
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
    // … the remaining ListControl.lua fields, same names in snake_case (U12) …
}

impl ListControl {
    /// ListControl.lua:44 ListControl(anchor, rect, rowHeight, scroll, isMutable, list, forceTooltip)
    pub fn state(
        hooks: &'static ListHooks,
        row_height: f64,
        scroll: bool,
        is_mutable: bool,
        list: ListData,
        force_tooltip: bool,
    ) -> ListControl {
        let _ = (hooks, row_height, scroll, is_mutable, list, force_tooltip);
        todo!("U12: ListControl.lua:44-87 (fields)")
    }
    /// scrollbars (anchored to `me`)
    pub fn init(cx: &mut Cx, me: CtrlId) {
        let _ = (cx, me);
        todo!("U12: ListControl.lua:44-87 (children)")
    }
    /// lua: ListControl.lua:136-338
    pub fn draw(cx: &mut Cx, me: CtrlId, vp: &ViewPort, no_tooltip: bool) {
        let _ = (cx, me, vp, no_tooltip);
        todo!("U12: ListControl:Draw")
    }
    /// lua: ListControl.lua:340-418
    pub fn on_key_down(cx: &mut Cx, me: CtrlId, key: Key, dbl: bool) -> Focus {
        let _ = (cx, me, key, dbl);
        todo!("U12: ListControl:OnKeyDown")
    }
    /// lua: ListControl.lua:420-471
    pub fn on_key_up(cx: &mut Cx, me: CtrlId, key: Key) -> Focus {
        let _ = (cx, me, key);
        todo!("U12: ListControl:OnKeyUp")
    }
    /// lua: ListControl.lua:119-124
    pub fn is_mouse_over(cx: &Cx, me: CtrlId) -> bool {
        let _ = (cx, me);
        todo!("U12: ListControl:IsMouseOver")
    }
    /// lua: ListControl.lua:89-109
    pub fn select_index(cx: &mut Cx, me: CtrlId, index: usize) {
        let _ = (cx, me, index);
        todo!("U12: ListControl:SelectIndex")
    }
    /// lua: ListControl.lua:473-483
    pub fn get_hover_index(cx: &Cx, me: CtrlId) -> Option<usize> {
        let _ = (cx, me);
        todo!("U12: ListControl:GetHoverIndex")
    }
    /// lua: ListControl.lua:485-493
    pub fn get_hover_value(cx: &Cx, me: CtrlId) -> Option<ListValue> {
        let _ = (cx, me);
        todo!("U12: ListControl:GetHoverValue")
    }
    /// `#self.list`
    pub fn list_len(cx: &Cx, me: CtrlId) -> usize {
        let _ = (cx, me);
        todo!("U12: #self.list")
    }
    /// `self.list[i]` (1-based)
    pub fn list_get(cx: &Cx, me: CtrlId, i: usize) -> Option<ListValue> {
        let _ = (cx, me, i);
        todo!("U12: self.list[i]")
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
}
