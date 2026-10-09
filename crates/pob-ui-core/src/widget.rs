//! Widgets: trait, capabilities, vtable, per-instance overrides, construction (ui doc §6.4, D3,
//! D5, D6). Complete in UI-U0; owned by U4 afterwards.
//!
//! lua: Common.lua:83-230 (`newClass`, `new`: a class is a method table; a subclass inherits by
//! metatable; `new("X"):X(...)` runs the constructor; a trailing table is a mixin applied after),
//! Control.lua (the always-present methods), ControlHost.lua:49-107 (which optional methods the
//! framework checks with `if control.OnKeyDown then`).
#![allow(clippy::self_named_constructors)]

use std::any::{Any, TypeId};
use std::rc::Rc;

use pob_sg::Key;

use crate::cx::Cx;
use crate::ids::{Ctrl, CtrlId, ViewPort};
use crate::layout::{base_is_enabled, base_is_shown, base_pos, base_size, Base};
use crate::prop::Prop;

bitflags::bitflags! {
    /// Which optional Lua methods exist (`if control.OnKeyDown then`). Effective caps = class CAPS | overrides.
    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    pub struct Caps: u32 {
        const DRAW = 1 << 0; const IS_MOUSE_OVER = 1 << 1; const KEY_DOWN = 1 << 2; const KEY_UP = 1 << 3;
        const CHAR = 1 << 4; const HOVER_KEY_UP = 1 << 5; const FOCUS_GAINED = 1 << 6; const FOCUS_LOST = 1 << 7;
        const CLICK = 1 << 8; const CAN_RECEIVE_DRAG = 1 << 9; const RECEIVE_DRAG = 1 << 10;
    }
}

/// The return of OnKeyDown/OnKeyUp/OnChar: `self` => Some(me), nil => None, another control => Some(it).
pub type Focus = Option<CtrlId>;

/// Duck-typed field checks made by the framework (ControlHost.lua:20, :55).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Duck {
    /// selDragActive and dragTargetList
    SelDragActiveWithTargets,
    /// imbuedSelect and dropped
    ImbuedSelectDropped,
}

/// A drag payload (ListControl GetDragValue => type, value).
#[derive(Clone)]
pub struct DragValue(pub Rc<dyn Any>);

/// A Lua control class. All methods are ASSOCIATED FUNCTIONS: `me` is Lua's `self`.
/// Methods not in CAPS are never called by the framework; their defaults raise the Lua
/// "attempt to call a nil value" error, which is what the Lua would do.
pub trait Widget: Any + Sized {
    const NAME: &'static str;
    const CAPS: Caps;
    fn draw(cx: &mut Cx, me: CtrlId, vp: &ViewPort, no_tooltip: bool) {
        let _ = (cx, vp, no_tooltip);
        nil_method::<Self>(me, "Draw")
    }
    fn is_mouse_over(cx: &Cx, me: CtrlId) -> bool {
        let _ = cx;
        nil_method::<Self>(me, "IsMouseOver")
    }
    fn on_key_down(cx: &mut Cx, me: CtrlId, key: Key, double_click: bool) -> Focus {
        let _ = (cx, key, double_click);
        nil_method::<Self>(me, "OnKeyDown")
    }
    fn on_key_up(cx: &mut Cx, me: CtrlId, key: Key) -> Focus {
        let _ = (cx, key);
        nil_method::<Self>(me, "OnKeyUp")
    }
    fn on_char(cx: &mut Cx, me: CtrlId, ch: Key) -> Focus {
        let _ = (cx, ch);
        nil_method::<Self>(me, "OnChar")
    }
    fn on_hover_key_up(cx: &mut Cx, me: CtrlId, key: Key) {
        let _ = (cx, key);
        nil_method::<Self>(me, "OnHoverKeyUp")
    }
    fn on_focus_gained(cx: &mut Cx, me: CtrlId) {
        let _ = cx;
        nil_method::<Self>(me, "OnFocusGained")
    }
    fn on_focus_lost(cx: &mut Cx, me: CtrlId) {
        let _ = cx;
        nil_method::<Self>(me, "OnFocusLost")
    }
    fn click(cx: &mut Cx, me: CtrlId) {
        let _ = cx;
        nil_method::<Self>(me, "Click")
    }
    fn can_receive_drag(cx: &Cx, me: CtrlId, ty: &str, v: &DragValue) -> bool {
        let _ = (cx, ty, v);
        nil_method::<Self>(me, "CanReceiveDrag")
    }
    fn receive_drag(cx: &mut Cx, me: CtrlId, ty: &str, v: &DragValue, source: CtrlId) {
        let _ = (cx, ty, v, source);
        nil_method::<Self>(me, "ReceiveDrag")
    }
    // Always-present Control methods (overridable by subclasses):
    fn get_pos(cx: &Cx, me: CtrlId) -> (f64, f64) {
        base_pos(cx, me)
    }
    fn get_size(cx: &Cx, me: CtrlId) -> (f64, f64) {
        base_size(cx, me)
    }
    fn is_shown(cx: &Cx, me: CtrlId) -> bool {
        base_is_shown(cx, me)
    }
    fn is_enabled(cx: &Cx, me: CtrlId) -> bool {
        base_is_enabled(cx, me)
    }
    fn duck(cx: &Cx, me: CtrlId, q: Duck) -> bool {
        let _ = (cx, me, q);
        false
    }
    /// Embedding upcast (D5). Default: only Self. Subclasses chain to their embedded base (see impl_parts!).
    fn part(w: &Self, ty: TypeId) -> Option<&dyn Any> {
        (ty == TypeId::of::<Self>()).then_some(w as &dyn Any)
    }
    fn part_mut(w: &mut Self, ty: TypeId) -> Option<&mut dyn Any> {
        (ty == TypeId::of::<Self>()).then_some(w as &mut dyn Any)
    }
}

#[track_caller]
pub fn nil_method<W: Widget>(me: CtrlId, m: &str) -> ! {
    crate::lua_error!(
        "attempt to call method '{}' (a nil value) on {} {:?}",
        m,
        W::NAME,
        me
    )
}

/// `impl_parts!(GemSelectControl: edit => EditControl)` implements part/part_mut chaining to the
/// embedded base. It expands to the two `Widget` methods, so it is invoked INSIDE the
/// `impl Widget for GemSelectControl { .. }` block (CCR-0013: ui doc §9.3 shows it at item level).
#[macro_export]
macro_rules! impl_parts {
    ($outer:ty : $field:ident => $inner:ty) => {
        fn part(w: &Self, ty: ::std::any::TypeId) -> ::core::option::Option<&dyn ::std::any::Any> {
            if ty == ::std::any::TypeId::of::<Self>() {
                return ::core::option::Option::Some(w as &dyn ::std::any::Any);
            }
            <$inner as $crate::widget::Widget>::part(&w.$field, ty)
        }
        fn part_mut(
            w: &mut Self,
            ty: ::std::any::TypeId,
        ) -> ::core::option::Option<&mut dyn ::std::any::Any> {
            if ty == ::std::any::TypeId::of::<Self>() {
                return ::core::option::Option::Some(w as &mut dyn ::std::any::Any);
            }
            <$inner as $crate::widget::Widget>::part_mut(&mut w.$field, ty)
        }
    };
}

/// The per-class dispatch table (copied out of the node, then called: §6.5).
pub struct VTable {
    pub name: &'static str,
    pub caps: Caps,
    pub draw: fn(&mut Cx, CtrlId, &ViewPort, bool),
    pub is_mouse_over: fn(&Cx, CtrlId) -> bool,
    pub on_key_down: fn(&mut Cx, CtrlId, Key, bool) -> Focus,
    pub on_key_up: fn(&mut Cx, CtrlId, Key) -> Focus,
    pub on_char: fn(&mut Cx, CtrlId, Key) -> Focus,
    pub on_hover_key_up: fn(&mut Cx, CtrlId, Key),
    pub on_focus_gained: fn(&mut Cx, CtrlId),
    pub on_focus_lost: fn(&mut Cx, CtrlId),
    pub click: fn(&mut Cx, CtrlId),
    pub can_receive_drag: fn(&Cx, CtrlId, &str, &DragValue) -> bool,
    pub receive_drag: fn(&mut Cx, CtrlId, &str, &DragValue, CtrlId),
    pub get_pos: fn(&Cx, CtrlId) -> (f64, f64),
    pub get_size: fn(&Cx, CtrlId) -> (f64, f64),
    pub is_shown: fn(&Cx, CtrlId) -> bool,
    pub is_enabled: fn(&Cx, CtrlId) -> bool,
    pub duck: fn(&Cx, CtrlId, Duck) -> bool,
    pub part: fn(&dyn Any, TypeId) -> Option<&dyn Any>,
    pub part_mut: fn(&mut dyn Any, TypeId) -> Option<&mut dyn Any>,
}

fn part_shim<W: Widget>(s: &dyn Any, ty: TypeId) -> Option<&dyn Any> {
    W::part(s.downcast_ref::<W>().expect("vtable/state mismatch"), ty)
}
fn part_mut_shim<W: Widget>(s: &mut dyn Any, ty: TypeId) -> Option<&mut dyn Any> {
    W::part_mut(s.downcast_mut::<W>().expect("vtable/state mismatch"), ty)
}

pub fn vtable_of<W: Widget>() -> &'static VTable {
    const {
        &VTable {
            name: W::NAME,
            caps: W::CAPS,
            draw: W::draw,
            is_mouse_over: W::is_mouse_over,
            on_key_down: W::on_key_down,
            on_key_up: W::on_key_up,
            on_char: W::on_char,
            on_hover_key_up: W::on_hover_key_up,
            on_focus_gained: W::on_focus_gained,
            on_focus_lost: W::on_focus_lost,
            click: W::click,
            can_receive_drag: W::can_receive_drag,
            receive_drag: W::receive_drag,
            get_pos: W::get_pos,
            get_size: W::get_size,
            is_shown: W::is_shown,
            is_enabled: W::is_enabled,
            duck: W::duck,
            part: part_shim::<W>,
            part_mut: part_mut_shim::<W>,
        }
    }
}

/// Per-instance method assignment (`ctrl.Draw = function(control) … end`, `function ctrl:OnFocusLost() … end`).
/// Dispatch checks these first. A Some(..) also turns the matching Caps bit on.
#[derive(Default, Clone)]
pub struct Overrides {
    pub draw: Option<Rc<dyn Fn(&mut Cx, CtrlId, &ViewPort, bool)>>,
    pub is_mouse_over: Option<Rc<dyn Fn(&Cx, CtrlId) -> bool>>,
    pub on_key_down: Option<Rc<dyn Fn(&mut Cx, CtrlId, Key, bool) -> Focus>>,
    pub on_key_up: Option<Rc<dyn Fn(&mut Cx, CtrlId, Key) -> Focus>>,
    pub on_char: Option<Rc<dyn Fn(&mut Cx, CtrlId, Key) -> Focus>>,
    pub on_hover_key_up: Option<Rc<dyn Fn(&mut Cx, CtrlId, Key)>>,
    pub on_focus_gained: Option<Rc<dyn Fn(&mut Cx, CtrlId)>>,
    pub on_focus_lost: Option<Rc<dyn Fn(&mut Cx, CtrlId)>>,
    pub can_receive_drag: Option<Rc<dyn Fn(&Cx, CtrlId, &str, &DragValue) -> bool>>,
    pub receive_drag: Option<Rc<dyn Fn(&mut Cx, CtrlId, &str, &DragValue, CtrlId)>>,
    pub get_size: Option<Rc<dyn Fn(&Cx, CtrlId) -> (f64, f64)>>,
    /// ControlHost methods overridden on an instance (Main.lua:1297, :1307)
    pub draw_controls: Option<Rc<dyn Fn(&mut Cx, CtrlId, &ViewPort, Option<CtrlId>)>>,
    pub get_mouse_over_control: Option<Rc<dyn Fn(&Cx, CtrlId) -> Option<CtrlId>>>,
}

impl Overrides {
    /// The optional-method bits these overrides add.
    pub fn caps(&self) -> Caps {
        let mut c = Caps::empty();
        c.set(Caps::DRAW, self.draw.is_some());
        c.set(Caps::IS_MOUSE_OVER, self.is_mouse_over.is_some());
        c.set(Caps::KEY_DOWN, self.on_key_down.is_some());
        c.set(Caps::KEY_UP, self.on_key_up.is_some());
        c.set(Caps::CHAR, self.on_char.is_some());
        c.set(Caps::HOVER_KEY_UP, self.on_hover_key_up.is_some());
        c.set(Caps::FOCUS_GAINED, self.on_focus_gained.is_some());
        c.set(Caps::FOCUS_LOST, self.on_focus_lost.is_some());
        c.set(Caps::CAN_RECEIVE_DRAG, self.can_receive_drag.is_some());
        c.set(Caps::RECEIVE_DRAG, self.receive_drag.is_some());
        c
    }
}

/// A not-yet-inserted control: what `new("X"):X(...)` evaluates to.
pub struct New<W: Widget> {
    pub base: Base,
    pub w: W,
    pub(crate) is_host: bool,
    pub(crate) inits: Vec<Box<dyn FnOnce(&mut Cx, Ctrl<W>)>>,
    pub(crate) mixins: Vec<Box<dyn FnOnce(&mut Cx, Ctrl<W>)>>,
}

impl<W: Widget> New<W> {
    pub fn new(base: Base, w: W) -> Self {
        New {
            base,
            w,
            is_host: false,
            inits: Vec::new(),
            mixins: Vec::new(),
        }
    }
    /// The class is (also) a ControlHost: the node gets a `controls` table.
    pub fn host(mut self) -> Self {
        self.is_host = true;
        self
    }
    /// Constructor tail that needs the id (children, closures over `self`). Runs right after insertion, in order.
    /// A subclass constructor chains the base's init first: `.init(|cx, me| EditControl::init(cx, me.0, ..))`.
    pub fn init(mut self, f: impl FnOnce(&mut Cx, Ctrl<W>) + 'static) -> Self {
        self.inits.push(Box::new(f));
        self
    }
    /// Lua mixin call `new("X"):X(...) { k = v }`: applied AFTER every init, in call order.
    pub fn mixin(mut self, f: impl FnOnce(&mut Cx, Ctrl<W>) + 'static) -> Self {
        self.mixins.push(Box::new(f));
        self
    }
    pub fn shown(self, p: Prop<bool>) -> Self {
        self.mixin(move |cx, c| cx.base_mut(c).shown = p)
    }
    pub fn enabled(self, p: Prop<bool>) -> Self {
        self.mixin(move |cx, c| cx.base_mut(c).enabled = p)
    }
    pub fn x(self, p: Prop<f64>) -> Self {
        self.mixin(move |cx, c| cx.base_mut(c).x = p)
    }
    pub fn y(self, p: Prop<f64>) -> Self {
        self.mixin(move |cx, c| cx.base_mut(c).y = p)
    }
    pub fn width(self, p: Prop<f64>) -> Self {
        self.mixin(move |cx, c| cx.base_mut(c).width = p)
    }
    pub fn height(self, p: Prop<f64>) -> Self {
        self.mixin(move |cx, c| cx.base_mut(c).height = p)
    }
}

/// `new("Control"):Control(anchor, rect)`: a pure geometry/anchor node (pointDisplay, anchorTopBarRight, sectionAnchor).
pub struct PlainControl;
impl Widget for PlainControl {
    const NAME: &'static str = "Control";
    const CAPS: Caps = Caps::empty();
}
impl PlainControl {
    pub fn new(anchor: crate::layout::Anchor, rect: [Prop<f64>; 4]) -> New<PlainControl> {
        New::new(Base::new(anchor, rect), PlainControl)
    }
}
