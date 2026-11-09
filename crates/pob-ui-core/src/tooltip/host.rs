//! Port of PathOfBuilding v2.67.2 (16de4b82d) src/Classes/TooltipHost.lua 1-27 (ui doc §6.9).
//! Owner: U10. The §6.9 reference body.
//!
//! @pob-file Classes/TooltipHost.lua @16de4b82d
//!
//! `A` = extra tooltipFunc args: () Button/Edit/Dragger, bool CheckBox (state), f64 Slider (val),
//! DropTipArgs DropDown.
//!
//! The class's first construction also loads Classes/Tooltip.lua, whose module-level code
//! preloads the recipe images: Rust callers run `tooltip::tooltip::module_load` at that point
//! (see tooltip.rs).

use std::rc::Rc;

use super::tooltip::Tooltip;
use crate::cx::Cx;
use crate::ids::{CtrlId, ViewPort};
use crate::prop::Prop;
use crate::widget::Widget;

pub type TooltipFn<A> = Rc<dyn Fn(&mut Cx, &mut Tooltip, A)>;

// @pob-skip Classes/TooltipHost.lua:1-8 reason="class header (newClass)"
pub struct TooltipHost<A: 'static = ()> {
    pub tooltip: Tooltip,
    /// read through self.Object:GetProperty
    pub tooltip_text: Option<Prop<Option<String>>>,
    pub tooltip_func: Option<TooltipFn<A>>,
}

impl<A: 'static> TooltipHost<A> {
    /// lua: TooltipHost.lua:9-13
    /// @pob Classes/TooltipHost.lua:9-13 h=048772ce
    pub fn new(text: Option<Prop<Option<String>>>) -> Self {
        TooltipHost {
            // L10
            tooltip: Tooltip::default(),
            // L11
            tooltip_text: text,
            tooltip_func: None,
        }
    }

    /// lua: TooltipHost.lua:15-27. The caller has already done SetDrawLayer(nil, 100).
    /// @pob Classes/TooltipHost.lua:15-27 h=9cdd0bd6
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
        // L16
        if let Some(f) = func {
            // L17
            f(cx, &mut tt, args);
            // L18
            tt.draw(cx, x, y, Some(w), Some(h), vp);
        } else if let Some(t) = text.and_then(|p| p.get(cx, me)) {
            // L20 self.Object:GetProperty("tooltipText"); L21
            // L22
            tt.clear(false);
            // L23
            tt.add_line(cx, 14.0, &t);
            // L24
            tt.draw(cx, x, y, Some(w), Some(h), vp);
        }
        get(cx.w_mut::<W>(me)).tooltip = tt; // `me` is still alive: freeing is deferred (D8)
    }
}
