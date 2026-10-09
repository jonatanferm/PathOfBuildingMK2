//! Port of PathOfBuilding v2.67.2 (16de4b82d) src/Classes/TooltipHost.lua 1-27 (ui doc §6.9).
//! Owner: U10. UI-U0 stub: the §6.9 reference body.
//!
//! `A` = extra tooltipFunc args: () Button/Edit/Dragger, bool CheckBox (state), f64 Slider (val),
//! DropTipArgs DropDown.

use std::rc::Rc;

use super::tooltip::Tooltip;
use crate::cx::Cx;
use crate::ids::{CtrlId, ViewPort};
use crate::prop::Prop;
use crate::widget::Widget;

pub type TooltipFn<A> = Rc<dyn Fn(&mut Cx, &mut Tooltip, A)>;

pub struct TooltipHost<A: 'static = ()> {
    pub tooltip: Tooltip,
    /// read through self.Object:GetProperty
    pub tooltip_text: Option<Prop<Option<String>>>,
    pub tooltip_func: Option<TooltipFn<A>>,
}

impl<A: 'static> TooltipHost<A> {
    /// lua: TooltipHost.lua:9-13
    pub fn new(text: Option<Prop<Option<String>>>) -> Self {
        TooltipHost {
            tooltip: Tooltip::default(),
            tooltip_text: text,
            tooltip_func: None,
        }
    }

    /// lua: TooltipHost.lua:15-27. The caller has already done SetDrawLayer(nil, 100).
    pub fn draw_tooltip<W: Widget>(
        cx: &mut Cx,
        me: CtrlId,
        get: fn(&mut W) -> &mut TooltipHost<A>,
        x: f64,
        y: f64,
        w: f64,
        h: f64,
        vp: &ViewPort,
        args: A,
    ) {
        let (func, text) = {
            let th = get(cx.w_mut::<W>(me));
            (th.tooltip_func.clone(), th.tooltip_text.clone())
        };
        let mut tt = std::mem::take(&mut get(cx.w_mut::<W>(me)).tooltip); // lend (6.13)
        if let Some(f) = func {
            f(cx, &mut tt, args);
            tt.draw(cx, x, y, Some(w), Some(h), vp);
        } else if let Some(t) = text.and_then(|p| p.get(cx, me)) {
            // self.Object:GetProperty("tooltipText")
            tt.clear(false);
            tt.add_line(cx, 14.0, &t);
            tt.draw(cx, x, y, Some(w), Some(h), vp);
        }
        get(cx.w_mut::<W>(me)).tooltip = tt; // `me` is still alive: freeing is deferred (D8)
    }
}
