//! Port of PathOfBuilding v2.67.2 (16de4b82d) src/Classes/ButtonControl.lua 1-130. Owner: U9.
//! UI-U0 stub: the ui doc §9.1 surface (struct, constructors, builder methods, Widget caps);
//! U9 writes the bodies (§9.1 is the reference port).

use std::rc::Rc;

use pob_sg::{ImageHandle, Key};

use crate::prelude::*;
use crate::tooltip::TooltipHost;

pub type ClickFn = Rc<dyn Fn(&mut Cx) -> Focus>;

pub struct ButtonControl {
    pub tt: TooltipHost<()>,
    pub label: Prop<String>,
    pub on_click: ClickFn,
    pub on_hover: Option<Rc<dyn Fn(&mut Cx)>>,
    pub force_tooltip: bool,
    /// GetProperty("locked"); nil => false
    pub locked: Prop<bool>,
    pub enter_func: Option<Rc<dyn Fn(&mut Cx)>>,
    pub image: Option<ImageHandle>,
    /// `self.overSizeText or 0`
    pub over_size_text: f64,
    pub clicked: bool,
}

impl ButtonControl {
    /// lua: ButtonControl.lua:9-17 ButtonControl(anchor, rect, label, onClick, onHover, forceTooltip)
    pub fn new(
        anchor: Anchor,
        rect: [Prop<f64>; 4],
        label: impl Into<Prop<String>>,
        on_click: impl Fn(&mut Cx) + 'static,
    ) -> New<ButtonControl> {
        Self::new_ret(anchor, rect, label, move |cx| {
            on_click(cx);
            None
        })
    }
    /// For onClick functions whose return value Lua feeds into SelectControl (`return self.onClick()` in OnKeyUp).
    pub fn new_ret(
        anchor: Anchor,
        rect: [Prop<f64>; 4],
        label: impl Into<Prop<String>>,
        on_click: impl Fn(&mut Cx) -> Focus + 'static,
    ) -> New<ButtonControl> {
        New::new(
            Base::new(anchor, rect),
            ButtonControl {
                tt: TooltipHost::new(None),
                label: label.into(),
                on_click: Rc::new(on_click),
                on_hover: None,
                force_tooltip: false,
                locked: Prop::Val(false),
                enter_func: None,
                image: None,
                over_size_text: 0.0,
                clicked: false,
            },
        )
    }
    /// lua: ButtonControl.lua:25-32
    pub fn set_image(cx: &mut Cx, me: CtrlId, path: Option<&str>) {
        let _ = (cx, me, path);
        todo!("U9: ButtonControl:SetImage")
    }
}

impl New<ButtonControl> {
    /// 5th and 6th constructor arguments.
    pub fn on_hover(mut self, f: impl Fn(&mut Cx) + 'static) -> Self {
        self.w.on_hover = Some(Rc::new(f));
        self
    }
    pub fn force_tooltip(mut self, v: bool) -> Self {
        self.w.force_tooltip = v;
        self
    }
}

impl Widget for ButtonControl {
    const NAME: &'static str = "ButtonControl";
    const CAPS: Caps = Caps::DRAW
        .union(Caps::IS_MOUSE_OVER)
        .union(Caps::KEY_DOWN)
        .union(Caps::KEY_UP)
        .union(Caps::CLICK);

    /// lua: ButtonControl.lua:19-23
    fn click(cx: &mut Cx, me: CtrlId) {
        let _ = (cx, me);
        todo!("U9: ButtonControl:Click")
    }
    /// lua: ButtonControl.lua:34-39
    fn is_mouse_over(cx: &Cx, me: CtrlId) -> bool {
        let _ = (cx, me);
        todo!("U9: ButtonControl:IsMouseOver")
    }
    /// lua: ButtonControl.lua:41-105
    fn draw(cx: &mut Cx, me: CtrlId, vp: &ViewPort, no_tooltip: bool) {
        let _ = (cx, me, vp, no_tooltip);
        todo!("U9: ButtonControl:Draw")
    }
    /// lua: ButtonControl.lua:107-117
    fn on_key_down(cx: &mut Cx, me: CtrlId, key: Key, double_click: bool) -> Focus {
        let _ = (cx, me, key, double_click);
        todo!("U9: ButtonControl:OnKeyDown")
    }
    /// lua: ButtonControl.lua:119-130
    fn on_key_up(cx: &mut Cx, me: CtrlId, key: Key) -> Focus {
        let _ = (cx, me, key);
        todo!("U9: ButtonControl:OnKeyUp")
    }
}
