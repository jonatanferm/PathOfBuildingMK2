//! Port of PathOfBuilding v2.67.2 (16de4b82d) src/Classes/RectangleOutlineControl.lua 1-23. Owner: U9.
//!
//! @pob-file Classes/RectangleOutlineControl.lua @16de4b82d
//!
//! `Draw` reads `self.width`/`self.height` RAW (no GetProperty): a function-valued width is a Lua
//! arithmetic error there, which `Prop::raw` reproduces. No PoB file constructs this class
//! (grep at 16de4b82d), so the `colors` table is kept to the 3-number shape of its default.

use crate::prelude::*;

pub struct RectangleOutlineControl {
    /// `self.colors` (`SetDrawColor(unpack(self.colors))`)
    pub colors: [f64; 3],
    pub stroke: f64,
}

impl RectangleOutlineControl {
    /// lua: RectangleOutlineControl.lua:9-14 RectangleOutlineControl(anchor, rect, colors, stroke)
    /// @pob Classes/RectangleOutlineControl.lua:9-14 h=afe09698
    pub fn new(
        anchor: Anchor,
        rect: [Prop<f64>; 4],
        colors: Option<[f64; 3]>,
        stroke: Option<f64>,
    ) -> New<RectangleOutlineControl> {
        // L10
        let base = Base::new(anchor, rect);
        New::new(
            base,
            RectangleOutlineControl {
                // L12
                colors: colors.unwrap_or([1.0, 1.0, 1.0]),
                // L11
                stroke: stroke.unwrap_or(1.0),
            },
        )
    }
}

impl Widget for RectangleOutlineControl {
    const NAME: &'static str = "RectangleOutlineControl";
    const CAPS: Caps = Caps::DRAW;
    /// lua: RectangleOutlineControl.lua:16-23
    /// @pob Classes/RectangleOutlineControl.lua:16-23 h=3755500c
    fn draw(cx: &mut Cx, me: CtrlId, _vp: &ViewPort, _no_tooltip: bool) {
        // L17
        let (x, y) = cx.pos(me);
        let (colors, stroke) = {
            let w = cx.w::<Self>(me);
            (w.colors, w.stroke)
        };
        // L18
        cx.sg.set_draw_color(colors[0], colors[1], colors[2]);
        // L19
        let width = cx.base(me).width.raw();
        cx.sg.draw_image(None, x, y, width + stroke, stroke);
        // L20
        let height = cx.base(me).height.raw();
        cx.sg
            .draw_image(None, x, y + height, width + stroke, stroke);
        // L21
        cx.sg.draw_image(None, x, y, stroke, height + stroke);
        // L22
        cx.sg
            .draw_image(None, x + width, y, stroke, height + stroke);
    }
}
