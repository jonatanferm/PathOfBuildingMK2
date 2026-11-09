//! Port of PathOfBuilding v2.67.2 (16de4b82d) src/Classes/ResizableEditControl.lua 1-51
//! (`newClass("ResizableEditControl", "EditControl")`). Owner: U11.
//!
//! @pob-file Classes/ResizableEditControl.lua @16de4b82d
//!
//! Surface notes (owner's amendments to the UI-U0 stub, CCR-0078 item 2):
//! - The constructor takes the rect's 5th..8th entries in Lua's order (`minWidth, minHeight,
//!   maxWidth, maxHeight`, L14) as `Option<f64>`: nil means the rect's `width`/`height` (L15-L18),
//!   which are read raw (PoB always passes numbers there: ConfigTab.lua:79, :334).
//! - Every method ResizableEditControl inherits from EditControl is delegated to EditControl's
//!   (the class only overrides Draw), so the vtable is EditControl's plus that Draw.
//! - The dragger's right click reads the raw `self.width`/`self.height` fields (L21); a function
//!   stored there compares unequal to a number (`~=`), as in Lua.
//! - `SetWidth`/`SetHeight` assign the raw fields (`Base::width/height = Prop::Val(..)`).

use std::rc::Rc;

use pob_lua::math::{m_max, m_min};
use pob_sg::Key;

use crate::controls::dragger::DraggerControl;
use crate::controls::edit::{EditArgs, EditControl};
use crate::prelude::*;

// @pob-skip Classes/ResizableEditControl.lua:1-11 reason="file header, math locals and the class header (newClass)"
pub struct ResizableEditControl {
    pub edit: EditControl,
    pub min_width: f64,
    pub max_width: f64,
    pub min_height: f64,
    pub max_height: f64,
}

/// `raw ~= v` for a raw `self.width`/`self.height` field.
fn raw_ne(p: &Prop<f64>, v: f64) -> bool {
    match p {
        Prop::Val(x) => *x != v,
        Prop::Fn(_) => true,
    }
}

impl ResizableEditControl {
    /// lua: ResizableEditControl.lua:12-31 ResizableEditControl(anchor, rect, init, prompt, filter, limit, changeFunc, lineHeight, allowZoom, clearable)
    /// `rect` is the first four entries; `min_width, min_height, max_width, max_height` are the
    /// 5th..8th (nil = the rect's width/height).
    /// @pob Classes/ResizableEditControl.lua:12-31 h=1e7ced6d
    pub fn new(
        anchor: Anchor,
        rect: [Prop<f64>; 4],
        args: EditArgs,
        min_width: Option<f64>,
        min_height: Option<f64>,
        max_width: Option<f64>,
        max_height: Option<f64>,
    ) -> New<ResizableEditControl> {
        // L14 local x, y, width, height, minWidth, minHeight, maxWidth, maxHeight = unpack(rect)
        let width = || rect[2].raw();
        let height = || rect[3].raw();
        // L15
        let min_height = min_height.unwrap_or_else(height);
        // L16
        let max_height = max_height.unwrap_or_else(height);
        // L17
        let min_width = min_width.unwrap_or_else(width);
        // L18
        let max_width = max_width.unwrap_or_else(width);
        New::new(
            Base::new(anchor, rect.clone()),
            ResizableEditControl {
                // L13 self:EditControl(anchor, rect, init, prompt, filter, limit, changeFunc, lineHeight, allowZoom, clearable)
                edit: EditControl::state(args),
                min_width,
                max_width,
                min_height,
                max_height,
            },
        )
        .host()
        .init(|cx, me| EditControl::init(cx, me.id()))
        .init(|cx, me| {
            let me = me.id();
            // L19-L28
            cx.add(
                me,
                "draggerHeight",
                DraggerControl::new(
                    Anchor::new(BottomRight, me, BottomRight),
                    crate::layout::rect(7, 7, 14, 14),
                    "//",
                    None,
                    None,
                    Some(Rc::new(move |cx: &mut Cx, _position: (f64, f64)| {
                        // L20 onRightClick
                        let (w, b) = (cx.w::<Self>(me), cx.base(me));
                        let (min_w, min_h, max_w, max_h) =
                            (w.min_width, w.min_height, w.max_width, w.max_height);
                        // L21
                        if raw_ne(&b.height, min_h) || raw_ne(&b.width, min_w) {
                            // L22
                            Self::set_width(cx, me, min_w);
                            // L23
                            Self::set_height(cx, me, min_h);
                        } else {
                            // L25
                            Self::set_width(cx, me, max_w);
                            // L26
                            Self::set_height(cx, me, max_h);
                        }
                    })),
                    None,
                    false,
                ),
            );
            // L29
            cx.w_mut::<EditControl>(me).protected = false;
        })
    }

    /// lua: ResizableEditControl.lua:37-44 SetBoundedDrag
    /// @pob Classes/ResizableEditControl.lua:37-44 h=4c4883e2
    pub fn set_bounded_drag(cx: &mut Cx, me: CtrlId) {
        // L38
        let dragger = cx.child_as::<DraggerControl>(me, "draggerHeight");
        if cx.get(dragger).dragging {
            // L39
            let (cursor_x, cursor_y) = cx.sys.cursor_pos();
            // L40
            let (x, y) = cx.pos(me);
            // L41
            Self::set_height(cx, me, cursor_y - y);
            // L42
            Self::set_width(cx, me, cursor_x - x);
        }
    }

    /// lua: ResizableEditControl.lua:46-48 SetWidth
    /// @pob Classes/ResizableEditControl.lua:46-48 h=8989e477
    pub fn set_width(cx: &mut Cx, me: CtrlId, width: f64) {
        let w = cx.w::<Self>(me);
        // L47
        let v = m_max(m_min(width, w.max_width), w.min_width);
        cx.base_mut(me).width = Prop::Val(v);
    }

    /// lua: ResizableEditControl.lua:49-51 SetHeight
    /// @pob Classes/ResizableEditControl.lua:49-51 h=7143acca
    pub fn set_height(cx: &mut Cx, me: CtrlId, height: f64) {
        let w = cx.w::<Self>(me);
        // L50
        let v = m_max(m_min(height, w.max_height), w.min_height);
        cx.base_mut(me).height = Prop::Val(v);
    }
}

impl Widget for ResizableEditControl {
    const NAME: &'static str = "ResizableEditControl";
    const CAPS: Caps = EditControl::CAPS;
    crate::impl_parts!(ResizableEditControl: edit => EditControl);

    /// lua: ResizableEditControl.lua:33-36
    /// @pob Classes/ResizableEditControl.lua:33-36 h=b480d28b
    fn draw(cx: &mut Cx, me: CtrlId, vp: &ViewPort, no_tooltip: bool) {
        // L34
        Self::set_bounded_drag(cx, me);
        // L35
        <EditControl as Widget>::draw(cx, me, vp, no_tooltip);
    }
    /// inherited: EditControl.lua:125-130
    fn is_mouse_over(cx: &Cx, me: CtrlId) -> bool {
        <EditControl as Widget>::is_mouse_over(cx, me)
    }
    /// inherited: EditControl.lua:436-441
    fn on_focus_gained(cx: &mut Cx, me: CtrlId) {
        <EditControl as Widget>::on_focus_gained(cx, me);
    }
    /// inherited: EditControl.lua:443-666
    fn on_key_down(cx: &mut Cx, me: CtrlId, key: Key, double_click: bool) -> Focus {
        <EditControl as Widget>::on_key_down(cx, me, key, double_click)
    }
    /// inherited: EditControl.lua:668-727
    fn on_key_up(cx: &mut Cx, me: CtrlId, key: Key) -> Focus {
        <EditControl as Widget>::on_key_up(cx, me, key)
    }
    /// inherited: EditControl.lua:729-741
    fn on_char(cx: &mut Cx, me: CtrlId, ch: Key) -> Focus {
        <EditControl as Widget>::on_char(cx, me, ch)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bounds_default_to_the_rect_and_clamp() {
        let mut cx = Cx::headless();
        let r = cx
            .add_free(
                None,
                ResizableEditControl::new(
                    Anchor::NONE,
                    crate::layout::rect(8, 0, 344, 118),
                    EditArgs {
                        line_height: Some(16.0),
                        ..Default::default()
                    },
                    None,
                    None,
                    None,
                    Some(758.0),
                ),
            )
            .id();
        let w = cx.w::<ResizableEditControl>(r);
        assert_eq!(
            (w.min_width, w.min_height, w.max_width, w.max_height),
            (344.0, 118.0, 344.0, 758.0)
        );
        assert!(cx.child(r, "draggerHeight").is_some());
        ResizableEditControl::set_height(&mut cx, r, 1000.0);
        assert_eq!(cx.base(r).height.raw(), 758.0);
        ResizableEditControl::set_height(&mut cx, r, 10.0);
        assert_eq!(cx.base(r).height.raw(), 118.0);
        ResizableEditControl::set_width(&mut cx, r, 10.0);
        assert_eq!(cx.base(r).width.raw(), 344.0);
        // the embedded EditControl is reachable through the node
        assert_eq!(cx.w::<EditControl>(r).line_height, Some(16.0));
    }
}
