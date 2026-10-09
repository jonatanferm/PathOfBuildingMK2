//! Port of PathOfBuilding v2.67.2 (16de4b82d) src/Classes/DropDownControl.lua 1-573
//! (`newClass("DropDownControl", "Control", "ControlHost", "TooltipHost", "SearchHost")`). Owner: U13.
//! UI-U0 stub: the ui doc §6.8 surface (DropItem, DropDownHooks, sel_func, the tooltip argument);
//! the hook set is frozen by U13 at its wave-2 gate.

use std::any::Any;
use std::rc::Rc;

use pob_sg::Key;

use crate::controls::search_host::SearchHost;
use crate::prelude::*;
use crate::tooltip::TooltipHost;

/// One `list[i]` entry. A Lua string entry is a DropItem whose `value` is the `String`.
#[derive(Clone)]
pub struct DropItem {
    pub label: String,
    pub search_filter: Option<String>,
    pub value: Rc<dyn Any>,
}

/// The `mode` argument of tooltipFunc (DropDownControl.lua Draw: "BODY" | "OUT" | "HOVER").
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DropTipMode {
    Body,
    Out,
    Hover,
}

/// tooltipFunc(tooltip, mode, index, value)
pub type DropTipArgs = (DropTipMode, Option<usize>, Option<DropItem>);

/// The overridable methods ItemSlot and TimelessJewelSocket use (frozen by U13 at its gate).
#[derive(Default)]
pub struct DropDownHooks {
    pub can_receive_drag: Option<fn(&Cx, CtrlId, &str, &DragValue) -> bool>,
    pub receive_drag: Option<fn(&mut Cx, CtrlId, &str, &DragValue, CtrlId)>,
}

pub struct DropDownControl {
    pub tt: TooltipHost<DropTipArgs>,
    pub search: SearchHost,
    pub hooks: &'static DropDownHooks,
    pub list: Vec<DropItem>,
    pub sel_index: usize,
    pub sel_func: Option<Rc<dyn Fn(&mut Cx, usize, &DropItem)>>,
    pub dropped: bool,
    pub drop_up: bool,
    pub ignore_search_order: bool,
}

impl DropDownControl {
    /// lua: DropDownControl.lua:14-58 DropDownControl(anchor, rect, list, selFunc, tooltipText, ignoreSearchOrder)
    pub fn new(
        anchor: Anchor,
        rect: [Prop<f64>; 4],
        list: Vec<DropItem>,
        sel_func: Option<Rc<dyn Fn(&mut Cx, usize, &DropItem)>>,
        tooltip_text: Option<Prop<Option<String>>>,
        ignore_search_order: bool,
    ) -> New<DropDownControl> {
        let _ = (
            anchor,
            rect,
            list,
            sel_func,
            tooltip_text,
            ignore_search_order,
        );
        todo!("U13: DropDownControl.lua:14-58")
    }
    /// lua: DropDownControl.lua:136-150 SelByValue(value, key)
    pub fn sel_by_value(cx: &mut Cx, me: CtrlId, value: &dyn Fn(&DropItem) -> bool) {
        let _ = (cx, me, value);
        todo!("U13: DropDownControl:SelByValue")
    }
    /// lua: DropDownControl.lua:156-158
    pub fn get_sel_value(cx: &Cx, me: CtrlId) -> Option<DropItem> {
        let _ = (cx, me);
        todo!("U13: DropDownControl:GetSelValue")
    }
    /// lua: DropDownControl.lua:160-169 SetSel(newSel, noCallSelFunc)
    pub fn set_sel(cx: &mut Cx, me: CtrlId, new_sel: usize, no_call_sel_func: bool) {
        let _ = (cx, me, new_sel, no_call_sel_func);
        todo!("U13: DropDownControl:SetSel")
    }
    /// lua: DropDownControl.lua:528-535 SetList(textList)
    pub fn set_list(cx: &mut Cx, me: CtrlId, list: Vec<DropItem>) {
        let _ = (cx, me, list);
        todo!("U13: DropDownControl:SetList")
    }
    /// lua: DropDownControl.lua:537-573 CheckDroppedWidth(enable)
    pub fn check_dropped_width(cx: &mut Cx, me: CtrlId, enable: bool) {
        let _ = (cx, me, enable);
        todo!("U13: DropDownControl:CheckDroppedWidth")
    }
}

impl Widget for DropDownControl {
    const NAME: &'static str = "DropDownControl";
    const CAPS: Caps = Caps::DRAW
        .union(Caps::IS_MOUSE_OVER)
        .union(Caps::KEY_DOWN)
        .union(Caps::KEY_UP)
        .union(Caps::CHAR);

    /// lua: DropDownControl.lua:178-210
    fn is_mouse_over(cx: &Cx, me: CtrlId) -> bool {
        let _ = (cx, me);
        todo!("U13: DropDownControl:IsMouseOver")
    }
    /// lua: DropDownControl.lua:212-425
    fn draw(cx: &mut Cx, me: CtrlId, vp: &ViewPort, no_tooltip: bool) {
        let _ = (cx, me, vp, no_tooltip);
        todo!("U13: DropDownControl:Draw")
    }
    /// lua: DropDownControl.lua:427-432
    fn on_char(cx: &mut Cx, me: CtrlId, ch: Key) -> Focus {
        let _ = (cx, me, ch);
        todo!("U13: DropDownControl:OnChar")
    }
    /// lua: DropDownControl.lua:434-464
    fn on_key_down(cx: &mut Cx, me: CtrlId, key: Key, double_click: bool) -> Focus {
        let _ = (cx, me, key, double_click);
        todo!("U13: DropDownControl:OnKeyDown")
    }
    /// lua: DropDownControl.lua:466-522
    fn on_key_up(cx: &mut Cx, me: CtrlId, key: Key) -> Focus {
        let _ = (cx, me, key);
        todo!("U13: DropDownControl:OnKeyUp")
    }
}
