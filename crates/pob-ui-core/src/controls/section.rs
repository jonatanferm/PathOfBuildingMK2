//! Port of PathOfBuilding v2.67.2 (16de4b82d) src/Classes/SectionControl.lua 1-33. Owner: U9.
//!
//! @pob-file Classes/SectionControl.lua @16de4b82d

use pob_sg::{Align, Font};

use crate::prelude::*;

pub struct SectionControl {
    pub label: Prop<String>,
}

impl SectionControl {
    /// lua: SectionControl.lua:10-14 SectionControl(anchor, rect, label)
    /// @pob Classes/SectionControl.lua:10-14 h=954fd590
    pub fn new(
        anchor: Anchor,
        rect: [Prop<f64>; 4],
        label: impl Into<Prop<String>>,
    ) -> New<SectionControl> {
        // L11
        let base = Base::new(anchor, rect);
        // L12
        New::new(
            base,
            SectionControl {
                label: label.into(),
            },
        )
    }
}

impl Widget for SectionControl {
    const NAME: &'static str = "SectionControl";
    const CAPS: Caps = Caps::DRAW;
    /// lua: SectionControl.lua:16-33
    /// @pob Classes/SectionControl.lua:16-33 h=3412f68e
    fn draw(cx: &mut Cx, me: CtrlId, _vp: &ViewPort, _no_tooltip: bool) {
        // L17
        let (x, y) = cx.pos(me);
        // L18
        let (width, height) = cx.size(me);
        // L19
        cx.sg.set_draw_layer(None, Some(-10));
        // L20
        cx.sg.set_draw_color(0.66, 0.66, 0.66);
        // L21
        cx.sg.draw_image(None, x, y, width, height);
        // L22
        cx.sg.set_draw_color(0.1, 0.1, 0.1);
        // L23
        cx.sg
            .draw_image(None, x + 2.0, y + 2.0, width - 4.0, height - 4.0);
        // L24
        cx.sg.set_draw_layer(None, Some(0));
        // L25
        let label = cx.prop::<Self, _>(me, |w| &w.label);
        // L26
        let label_width = cx.sg.draw_string_width(14.0, Font::Var, &label);
        // L27
        cx.sg.set_draw_color(0.66, 0.66, 0.66);
        // L28
        cx.sg
            .draw_image(None, x + 6.0, y - 8.0, label_width + 6.0, 18.0);
        // L29
        cx.sg.set_draw_color(0.0, 0.0, 0.0);
        // L30
        cx.sg
            .draw_image(None, x + 7.0, y - 7.0, label_width + 4.0, 16.0);
        // L31
        cx.sg.set_draw_color(1.0, 1.0, 1.0);
        // L32
        cx.sg
            .draw_string(x + 9.0, y - 6.0, Align::Left, 14.0, Font::Var, &label);
    }
}
