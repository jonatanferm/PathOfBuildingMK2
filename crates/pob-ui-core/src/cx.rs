//! `Cx`: the single context every UI function receives (ui doc §6.5, D3). Owner: U4.
//! UI-U0 stub: final signatures; the §6.5 reference bodies (accessors, `prop`, the dispatch
//! shape) are in, the arena operations are U4's.
//!
//! lua: Control.lua (GetPos/GetSize/GetProperty/IsShown/IsEnabled/IsMouseInBounds/SetAnchor
//! dispatch through the object's metatable chain), Common.lua:158-230 (`new`).

use pob_sg::Key;
use smallvec::SmallVec;

use crate::arena::{HostData, Ui};
use crate::ids::{Ctrl, CtrlId, CtrlKey, ViewPort};
use crate::layout::{AnchorPoint, Base};
use crate::main_core::MainCore;
use crate::prop::Prop;
use crate::widget::{Caps, DragValue, Duck, Focus, New, Overrides, Widget};

/// The single context every UI function receives: Lua's globals plus the object arena.
pub struct Cx {
    pub ui: Ui,
    pub sg: pob_sg::Sg,
    pub sys: pob_sg::Sys,
    /// the part of `main` generic controls use (§6.11)
    pub main: MainCore,
    /// pob-app's AppState; typed access through pob_app::CxApp
    pub ext: Box<dyn std::any::Any>,
}

impl Cx {
    // ---- arena (one call per Lua assignment) ----

    /// `host.controls[key] = new(..)`. Assigns the creation seq, runs inits, then mixins. If `key` already held a
    /// control, that control is buried (D8); use `detach_child` first when the Lua keeps referencing it.
    pub fn add<W: Widget>(
        &mut self,
        host: impl Into<CtrlId>,
        key: impl Into<CtrlKey>,
        n: New<W>,
    ) -> Ctrl<W> {
        let _ = (host.into(), key.into(), n);
        todo!("U4: Cx::add")
    }
    /// `local c = new(..)` not stored in a controls table (anchor helpers, tabs, popups, mode objects).
    pub fn add_free<W: Widget>(&mut self, owner: Option<CtrlId>, n: New<W>) -> Ctrl<W> {
        let _ = (owner, n);
        todo!("U4: Cx::add_free")
    }
    /// `host.controls[key] = existing` (an existing object; ownership unchanged).
    pub fn put(&mut self, host: impl Into<CtrlId>, key: impl Into<CtrlKey>, id: CtrlId) {
        let _ = (host.into(), key.into(), id);
        todo!("U4: Cx::put")
    }
    /// `host.controls[key] = nil` where nothing else keeps the object: detach + bury (deferred).
    pub fn remove_child(&mut self, host: impl Into<CtrlId>, key: &CtrlKey) {
        let _ = (host.into(), key);
        todo!("U4: Cx::remove_child")
    }
    /// `host.controls[key] = nil` where the object is still referenced elsewhere.
    pub fn detach_child(&mut self, host: impl Into<CtrlId>, key: &CtrlKey) -> Option<CtrlId> {
        let _ = (host.into(), key);
        todo!("U4: Cx::detach_child")
    }
    /// wipeTable(self.controls)
    pub fn clear_children(&mut self, host: impl Into<CtrlId>) {
        let _ = host.into();
        todo!("U4: Cx::clear_children")
    }
    /// Free `id` and everything it owns at end of frame.
    pub fn bury(&mut self, id: impl Into<CtrlId>) {
        let _ = id.into();
        todo!("U4: Cx::bury")
    }
    /// Host calls this after on_frame. Drops buried nodes (their Rc closures drop here, outside any borrow).
    pub fn end_frame_gc(&mut self) {
        todo!("U4: Cx::end_frame_gc")
    }

    #[track_caller]
    pub fn base(&self, id: impl Into<CtrlId>) -> &Base {
        self.ui.base(id)
    }
    #[track_caller]
    pub fn base_mut(&mut self, id: impl Into<CtrlId>) -> &mut Base {
        self.ui.base_mut(id)
    }
    #[track_caller]
    pub fn w<W: Widget>(&self, id: impl Into<CtrlId>) -> &W {
        self.ui.w(id)
    }
    #[track_caller]
    pub fn w_mut<W: Widget>(&mut self, id: impl Into<CtrlId>) -> &mut W {
        self.ui.w_mut(id)
    }
    #[track_caller]
    pub fn get<W: Widget>(&self, c: Ctrl<W>) -> &W {
        self.ui.w(c.0)
    }
    #[track_caller]
    pub fn get_mut<W: Widget>(&mut self, c: Ctrl<W>) -> &mut W {
        self.ui.w_mut(c.0)
    }
    pub fn try_w<W: Widget>(&self, id: CtrlId) -> Option<&W> {
        self.ui.try_w(id)
    }
    pub fn is<W: Widget>(&self, id: CtrlId) -> bool {
        self.ui.is::<W>(id)
    }
    /// GetProperty on a class-specific Prop field: clones the Prop out, then evaluates it with the whole &Cx.
    pub fn prop<W: Widget, T: Clone + 'static>(
        &self,
        me: CtrlId,
        f: impl FnOnce(&W) -> &Prop<T>,
    ) -> T {
        let p = f(self.w::<W>(me)).clone();
        p.get(self, me)
    }
    pub fn child(&self, host: impl Into<CtrlId>, key: impl Into<CtrlKey>) -> Option<CtrlId> {
        let _ = (host.into(), key.into());
        todo!("U4: Cx::child")
    }
    /// Typed `self.controls.key`; a missing key panics like a Lua nil index.
    #[track_caller]
    pub fn child_as<W: Widget>(&self, host: impl Into<CtrlId>, key: &'static str) -> Ctrl<W> {
        let _ = (host.into(), key);
        todo!("U4: Cx::child_as")
    }
    /// Canonical iteration order (D7): Idx keys ascending, then Name keys by control creation seq. Snapshot.
    pub fn children(&self, host: impl Into<CtrlId>) -> SmallVec<[CtrlId; 32]> {
        let _ = host.into();
        todo!("U4: Cx::children")
    }
    pub fn host(&self, id: impl Into<CtrlId>) -> &HostData {
        let _ = id.into();
        todo!("U4: Cx::host")
    }
    pub fn host_mut(&mut self, id: impl Into<CtrlId>) -> &mut HostData {
        let _ = id.into();
        todo!("U4: Cx::host_mut")
    }
    /// Still a member of `host` (a control removed earlier in the same iteration is skipped, as with Lua `pairs`).
    pub fn is_child_of(&self, host: CtrlId, id: CtrlId) -> bool {
        let _ = (host, id);
        todo!("U4: Cx::is_child_of")
    }
    /// Control-path markers for the trace (no-op unless tracing): "<HostClass>.<key>".
    pub fn trace_push(&mut self, host: CtrlId, id: CtrlId) {
        let _ = (host, id);
        todo!("U4: Cx::trace_push")
    }
    pub fn trace_pop(&mut self) {
        todo!("U4: Cx::trace_pop")
    }
    /// Lend a widget-owned object that needs &mut Cx for its methods (Tooltip, PassiveTreeView, Coroutine):
    /// it is taken out (Default left behind), `f` runs, then it is put back. Only for objects that nothing else
    /// reads during the call (§7.2).
    pub fn lend<W: Widget, T: Default, R>(
        &mut self,
        me: CtrlId,
        field: fn(&mut W) -> &mut T,
        f: impl FnOnce(&mut Cx, &mut T) -> R,
    ) -> R {
        let mut t = std::mem::take(field(self.w_mut::<W>(me)));
        let r = f(self, &mut t);
        *field(self.w_mut::<W>(me)) = t;
        r
    }

    // ---- Control.lua (dynamic dispatch through vtable + overrides) ----

    /// GetPos
    pub fn pos(&self, id: impl Into<CtrlId>) -> (f64, f64) {
        let id = id.into();
        let f = self.ui.node(id).vt.get_pos;
        f(self, id)
    }
    /// GetSize (ov.get_size first)
    pub fn size(&self, id: impl Into<CtrlId>) -> (f64, f64) {
        let id = id.into();
        let n = self.ui.node(id);
        if let Some(f) = n.ov.get_size.clone() {
            return f(self, id);
        }
        let f = n.vt.get_size;
        f(self, id)
    }
    /// GetProperty("width")
    pub fn width(&self, id: impl Into<CtrlId>) -> f64 {
        let id = id.into();
        self.base(id).width.get(self, id)
    }
    /// GetProperty("height")
    pub fn height(&self, id: impl Into<CtrlId>) -> f64 {
        let id = id.into();
        self.base(id).height.get(self, id)
    }
    pub fn is_shown(&self, id: impl Into<CtrlId>) -> bool {
        let id = id.into();
        let f = self.ui.node(id).vt.is_shown;
        f(self, id)
    }
    pub fn is_enabled(&self, id: impl Into<CtrlId>) -> bool {
        let id = id.into();
        let f = self.ui.node(id).vt.is_enabled;
        f(self, id)
    }
    pub fn is_mouse_in_bounds(&self, id: impl Into<CtrlId>) -> bool {
        crate::layout::base_is_mouse_in_bounds(self, id.into())
    }
    /// Control.lua:81-90: x/y are replaced only when BOTH are Some; collapse is always overwritten.
    pub fn set_anchor(
        &mut self,
        id: impl Into<CtrlId>,
        point: AnchorPoint,
        other: Option<CtrlId>,
        other_point: AnchorPoint,
        xy: Option<(Prop<f64>, Prop<f64>)>,
        collapse: bool,
    ) {
        let _ = (id.into(), point, other, other_point, xy, collapse);
        todo!("U4: Cx::set_anchor")
    }

    // ---- optional-method dispatch (ov first, then vtable) ----

    /// (vt.caps | ov.caps()).contains(c)
    pub fn has(&self, id: CtrlId, c: Caps) -> bool {
        let n = self.ui.node(id);
        (n.vt.caps | n.ov.caps()).contains(c)
    }
    pub fn draw(&mut self, id: CtrlId, vp: &ViewPort, no_tooltip: bool) {
        let n = self.ui.node(id);
        if let Some(f) = n.ov.draw.clone() {
            return f(self, id, vp, no_tooltip);
        }
        let f = n.vt.draw;
        f(self, id, vp, no_tooltip)
    }
    pub fn is_mouse_over(&self, id: CtrlId) -> bool {
        let n = self.ui.node(id);
        if let Some(f) = n.ov.is_mouse_over.clone() {
            return f(self, id);
        }
        let f = n.vt.is_mouse_over;
        f(self, id)
    }
    pub fn on_key_down(&mut self, id: CtrlId, key: Key, double_click: bool) -> Focus {
        let n = self.ui.node(id);
        if let Some(f) = n.ov.on_key_down.clone() {
            return f(self, id, key, double_click);
        }
        let f = n.vt.on_key_down;
        f(self, id, key, double_click)
    }
    pub fn on_key_up(&mut self, id: CtrlId, key: Key) -> Focus {
        let n = self.ui.node(id);
        if let Some(f) = n.ov.on_key_up.clone() {
            return f(self, id, key);
        }
        let f = n.vt.on_key_up;
        f(self, id, key)
    }
    pub fn on_char(&mut self, id: CtrlId, ch: Key) -> Focus {
        let n = self.ui.node(id);
        if let Some(f) = n.ov.on_char.clone() {
            return f(self, id, ch);
        }
        let f = n.vt.on_char;
        f(self, id, ch)
    }
    pub fn on_hover_key_up(&mut self, id: CtrlId, key: Key) {
        let n = self.ui.node(id);
        if let Some(f) = n.ov.on_hover_key_up.clone() {
            return f(self, id, key);
        }
        let f = n.vt.on_hover_key_up;
        f(self, id, key)
    }
    pub fn on_focus_gained(&mut self, id: CtrlId) {
        let n = self.ui.node(id);
        if let Some(f) = n.ov.on_focus_gained.clone() {
            return f(self, id);
        }
        let f = n.vt.on_focus_gained;
        f(self, id)
    }
    pub fn on_focus_lost(&mut self, id: CtrlId) {
        let n = self.ui.node(id);
        if let Some(f) = n.ov.on_focus_lost.clone() {
            return f(self, id);
        }
        let f = n.vt.on_focus_lost;
        f(self, id)
    }
    pub fn click(&mut self, id: CtrlId) {
        let f = self.ui.node(id).vt.click;
        f(self, id)
    }
    pub fn can_receive_drag(&self, id: CtrlId, ty: &str, v: &DragValue) -> bool {
        let n = self.ui.node(id);
        if let Some(f) = n.ov.can_receive_drag.clone() {
            return f(self, id, ty, v);
        }
        let f = n.vt.can_receive_drag;
        f(self, id, ty, v)
    }
    pub fn receive_drag(&mut self, id: CtrlId, ty: &str, v: &DragValue, source: CtrlId) {
        let n = self.ui.node(id);
        if let Some(f) = n.ov.receive_drag.clone() {
            return f(self, id, ty, v, source);
        }
        let f = n.vt.receive_drag;
        f(self, id, ty, v, source)
    }
    pub fn duck(&self, id: CtrlId, q: Duck) -> bool {
        let f = self.ui.node(id).vt.duck;
        f(self, id, q)
    }
    /// Per-instance method assignment: `cx.set_ov(id, |ov| ov.on_focus_lost = Some(Rc::new(..)))`.
    pub fn set_ov(&mut self, id: impl Into<CtrlId>, f: impl FnOnce(&mut Overrides)) {
        f(&mut self.ui.node_mut(id.into()).ov);
    }
}
