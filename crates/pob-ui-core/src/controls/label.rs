//! Port of PathOfBuilding v2.67.2 (16de4b82d) src/Classes/LabelControl.lua 1-24. Owner: U9.
//!
//! @pob-file Classes/LabelControl.lua @16de4b82d
//!
//! Props with side effects (ui doc §7.3): none (the `width` function only reads).

use pob_sg::{Align, Font};

use crate::prelude::*;

pub struct LabelControl {
    pub label: Prop<String>,
}

impl LabelControl {
    /// lua: LabelControl.lua:12-19 LabelControl(anchor, rect, label)
    /// @pob Classes/LabelControl.lua:12-19 h=daaab412
    pub fn new(
        anchor: Anchor,
        rect: [Prop<f64>; 4],
        label: impl Into<Prop<String>>,
    ) -> New<LabelControl> {
        // L13
        let mut base = Base::new(anchor, rect);
        // L15: self.width = function() ... end (before any mixin, which may replace it)
        base.width = Prop::f(|cx, me| {
            // L16
            let height = cx.height(me);
            let label = cx.prop::<LabelControl, _>(me, |w| &w.label);
            cx.sg.draw_string_width(height, Font::Var, &label)
        });
        // L14
        New::new(
            base,
            LabelControl {
                label: label.into(),
            },
        )
    }
}

impl Widget for LabelControl {
    const NAME: &'static str = "LabelControl";
    const CAPS: Caps = Caps::DRAW;
    /// lua: LabelControl.lua:21-24
    /// @pob Classes/LabelControl.lua:21-24 h=a538e3f1
    fn draw(cx: &mut Cx, me: CtrlId, _vp: &ViewPort, _no_tooltip: bool) {
        // L22
        let (x, y) = cx.pos(me);
        // L23
        let height = cx.height(me);
        let label = cx.prop::<Self, _>(me, |w| &w.label);
        cx.sg
            .draw_string(x, y, Align::Left, height, Font::Var, &label);
    }
}
