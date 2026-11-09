//! Port of PathOfBuilding v2.67.2 (16de4b82d) src/Classes/Control.lua (dispatch of GetPos,
//! GetSize, GetProperty, IsShown, IsEnabled, IsMouseInBounds, SetAnchor through the object's
//! metatable chain) and Modules/Common.lua:158-230 (`new`). `Cx`: the single context every UI
//! function receives (ui doc §6.5, D3). Owner: U4.
//!
//! @pob-file Classes/Control.lua @16de4b82d
//!
//! Arena operations (one call per Lua assignment, D7/D8):
//! - `add`: Lua `host.controls[key] = new("X"):X(...) { mixin }`. The object is created (creation
//!   seq stamped, as the oracle's patched `new()` does), its constructor tail (`inits`) and mixins
//!   run, and only then is it stored under `key`, which is the Lua evaluation order of the
//!   assignment. A control that `key` held before and that this host owns is buried; one that
//!   another owner placed there with `put` is only unlinked.
//! - `clear_children` (wipeTable(self.controls)) buries the children this host owns.
//!   DIVERGENCE(gc): see arena.rs.
//!
//! ORDER: `children` is the canonical order of D7 (integer keys ascending, then named keys by
//! creation seq); the oracle iterates `pairs(self.controls)` the same way (patch 6).

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
    /// A context with inert services (no fonts, no images, no platform): for tests and galleries
    /// that need a `Cx` without a host. `main.node` is the null id until the caller creates one.
    /// CCR-0034 (U4 addition).
    #[doc(hidden)]
    pub fn headless() -> Cx {
        let sg = pob_sg::Sg::new(
            std::rc::Rc::new(pob_sg::FontSet::empty()),
            pob_sg::ImageRegistry::new(Box::new(inert::Inert)),
        );
        let sys = pob_sg::Sys::new(
            Box::new(inert::Inert),
            Box::new(inert::Inert),
            Box::new(inert::Inert),
        );
        Cx {
            ui: Ui::default(),
            sg,
            sys,
            main: MainCore::new(CtrlId::default()),
            ext: Box::new(()),
        }
    }

    // ---- arena (one call per Lua assignment) ----

    /// `host.controls[key] = new(..)`. Assigns the creation seq, runs inits, then mixins. If `key` already held a
    /// control, that control is buried (D8); use `detach_child` first when the Lua keeps referencing it.
    pub fn add<W: Widget>(
        &mut self,
        host: impl Into<CtrlId>,
        key: impl Into<CtrlKey>,
        n: New<W>,
    ) -> Ctrl<W> {
        let host = host.into();
        let key = key.into();
        // Lua evaluates `new(..)` (constructor and mixin) first; the store into `host.controls`
        // is the last step of the assignment.
        let c = self.construct(Some(host), n);
        self.ui.node_mut(c.0).parent = Some((host, key.clone()));
        self.store(host, key, c.0);
        c
    }
    /// `local c = new(..)` not stored in a controls table (anchor helpers, tabs, popups, mode objects).
    pub fn add_free<W: Widget>(&mut self, owner: Option<CtrlId>, n: New<W>) -> Ctrl<W> {
        self.construct(owner, n)
    }
    /// `host.controls[key] = existing` (an existing object; ownership unchanged).
    pub fn put(&mut self, host: impl Into<CtrlId>, key: impl Into<CtrlKey>, id: CtrlId) {
        let host = host.into();
        let key = key.into();
        let n = self.ui.node_mut(id);
        // Storing a buried object again keeps it alive (a Lua reference does).
        n.buried = false;
        if n.parent.is_none() {
            n.parent = Some((host, key.clone()));
        }
        self.store(host, key, id);
    }
    /// `host.controls[key] = nil` where nothing else keeps the object: detach + bury (deferred).
    pub fn remove_child(&mut self, host: impl Into<CtrlId>, key: &CtrlKey) {
        if let Some(id) = self.detach_child(host, key) {
            self.bury(id);
        }
    }
    /// `host.controls[key] = nil` where the object is still referenced elsewhere.
    pub fn detach_child(&mut self, host: impl Into<CtrlId>, key: &CtrlKey) -> Option<CtrlId> {
        let host = host.into();
        let id = self.host_mut(host).map.remove(key)?;
        self.unlink_parent(host, key, id);
        Some(id)
    }
    /// wipeTable(self.controls)
    /// lua: Common.lua:549-557 wipeTable (the @pob marker lives on pob-calc lua_table.rs)
    pub fn clear_children(&mut self, host: impl Into<CtrlId>) {
        let host = host.into();
        // ORDER: wipeTable's `pairs` order is unobservable here (every key becomes nil; nothing
        // runs in between).
        let map = std::mem::take(&mut self.host_mut(host).map);
        for (key, id) in map {
            self.release(host, &key, id);
        }
    }
    /// Free `id` and everything it owns at end of frame.
    pub fn bury(&mut self, id: impl Into<CtrlId>) {
        let id = id.into();
        if let Some(n) = self.ui.nodes.get_mut(id) {
            if !n.buried {
                n.buried = true;
                self.ui.graveyard.push(id);
            }
        }
    }
    /// Host calls this after on_frame. Drops buried nodes (their Rc closures drop here, outside any borrow).
    pub fn end_frame_gc(&mut self) {
        let graveyard = std::mem::take(&mut self.ui.graveyard);
        // The buried nodes that are still buried (a `put` revives one) and, transitively, what
        // they own.
        let mut doomed: Vec<CtrlId> = Vec::new();
        let mut stack: Vec<CtrlId> = graveyard
            .into_iter()
            .filter(|&id| self.ui.nodes.get(id).is_some_and(|n| n.buried))
            .collect();
        let mut seen = std::collections::HashSet::new();
        while let Some(id) = stack.pop() {
            if !seen.insert(id) {
                continue;
            }
            let Some(n) = self.ui.nodes.get(id) else {
                continue;
            };
            doomed.push(id);
            stack.extend(n.owned.iter().copied());
        }
        // Unlink each doomed node from survivors: its host's controls table and its owner's list.
        for &id in &doomed {
            let (parent, owner) = {
                let n = &self.ui.nodes[id];
                (n.parent.clone(), n.owner)
            };
            if let Some((h, key)) = parent {
                if !seen.contains(&h) {
                    if let Some(hd) = self.ui.nodes.get_mut(h).and_then(|n| n.host.as_mut()) {
                        if hd.map.get(&key) == Some(&id) {
                            hd.map.remove(&key);
                        }
                    }
                }
            }
            if let Some(o) = owner {
                if !seen.contains(&o) {
                    if let Some(on) = self.ui.nodes.get_mut(o) {
                        on.owned.retain(|&x| x != id);
                    }
                }
            }
        }
        let mut freed = Vec::with_capacity(doomed.len());
        for id in doomed {
            if let Some(n) = self.ui.nodes.remove(id) {
                freed.push(n);
            }
        }
        // A tab group whose members are all gone goes with them.
        let nodes = &self.ui.nodes;
        self.ui
            .tab_groups
            .retain(|_, g| g.iter().any(|&m| nodes.contains_key(m)));
        // The nodes (and the Rc closures they hold) drop here, outside any borrow of `self`.
        drop(freed);
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
    #[track_caller]
    pub fn child(&self, host: impl Into<CtrlId>, key: impl Into<CtrlKey>) -> Option<CtrlId> {
        self.host(host).map.get(&key.into()).copied()
    }
    /// Typed `self.controls.key`; a missing key panics like a Lua nil index.
    #[track_caller]
    pub fn child_as<W: Widget>(&self, host: impl Into<CtrlId>, key: &'static str) -> Ctrl<W> {
        let Some(id) = self.child(host, key) else {
            crate::lua_error!(
                "attempt to index field '{}' (a nil value) (expected {})",
                key,
                W::NAME
            )
        };
        if !self.ui.is::<W>(id) {
            crate::lua_error!(
                "controls.{} is a {}, not a {}",
                key,
                self.ui.class_name(id),
                W::NAME
            );
        }
        Ctrl::from_raw(id)
    }
    /// Canonical iteration order (D7): Idx keys ascending, then Name keys by control creation seq. Snapshot.
    #[track_caller]
    pub fn children(&self, host: impl Into<CtrlId>) -> SmallVec<[CtrlId; 32]> {
        let h = self.host(host);
        let mut out: SmallVec<[CtrlId; 32]> = SmallVec::new();
        let mut named: SmallVec<[(u64, CtrlId); 32]> = SmallVec::new();
        // BTreeMap order: every Idx key sorts before every Name key, Idx keys ascending.
        for (k, &id) in &h.map {
            match k {
                CtrlKey::Idx(_) => out.push(id),
                CtrlKey::Name(_) => named.push((self.ui.node(id).seq, id)),
            }
        }
        // Stable: one object stored under two names keeps the names' (byte) order.
        named.sort_by_key(|&(seq, _)| seq);
        out.extend(named.into_iter().map(|(_, id)| id));
        out
    }
    #[track_caller]
    pub fn host(&self, id: impl Into<CtrlId>) -> &HostData {
        let id = id.into();
        let n = self.ui.node(id);
        match n.host.as_ref() {
            Some(h) => h,
            None => crate::lua_error!(
                "attempt to index field 'controls' (a nil value) on {} {:?}",
                n.vt.name,
                id
            ),
        }
    }
    #[track_caller]
    pub fn host_mut(&mut self, id: impl Into<CtrlId>) -> &mut HostData {
        let id = id.into();
        let n = self.ui.node_mut(id);
        let name = n.vt.name;
        match n.host.as_mut() {
            Some(h) => h,
            None => crate::lua_error!(
                "attempt to index field 'controls' (a nil value) on {} {:?}",
                name,
                id
            ),
        }
    }
    /// Still a member of `host` (a control removed earlier in the same iteration is skipped, as with Lua `pairs`).
    pub fn is_child_of(&self, host: CtrlId, id: CtrlId) -> bool {
        self.ui
            .nodes
            .get(host)
            .and_then(|n| n.host.as_ref())
            .is_some_and(|h| h.map.values().any(|&v| v == id))
    }
    /// Control-path markers for the trace (no-op unless tracing): "<HostClass>.<key>". Returns whether a
    /// marker was pushed; the caller pops only then, so toggling tracing mid-draw cannot unbalance the stack.
    pub fn trace_push(&mut self, host: CtrlId, id: CtrlId) -> bool {
        if self.sg.trace.is_none() {
            return false;
        }
        let class = self.ui.class_name(host);
        let key = self
            .host(host)
            .map
            .iter()
            .find(|(_, &v)| v == id)
            .map(|(k, _)| k.clone());
        let elem = match key {
            Some(CtrlKey::Name(s)) => format!("{class}.{s}"),
            Some(CtrlKey::Idx(i)) => format!("{class}.{i}"),
            None => format!("{class}.?"),
        };
        self.sg.trace_ctl_push(&elem);
        true
    }
    pub fn trace_pop(&mut self) {
        self.sg.trace_ctl_pop();
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
    /// @pob Classes/Control.lua:73-79 h=9bc09063
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
    /// lua: Control.lua:81-90
    /// @pob Classes/Control.lua:81-90 h=b0408a83
    pub fn set_anchor(
        &mut self,
        id: impl Into<CtrlId>,
        point: AnchorPoint,
        other: Option<CtrlId>,
        other_point: AnchorPoint,
        xy: Option<(Prop<f64>, Prop<f64>)>,
        collapse: bool,
    ) {
        let b = self.base_mut(id);
        b.anchor.point = Some(point);
        b.anchor.other = other;
        // `anchor.other` is now an object: a pending popup sibling key no longer applies.
        b.anchor.other_key = None;
        b.anchor.other_point = Some(other_point);
        b.anchor.collapse = collapse;
        if let Some((x, y)) = xy {
            b.x = x;
            b.y = y;
        }
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

    // ---- private arena plumbing ----

    /// `new(..)` + constructor + mixin: insert the node (creation seq, D7), then run the
    /// constructor tail and the mixins in order.
    fn construct<W: Widget>(&mut self, owner: Option<CtrlId>, n: New<W>) -> Ctrl<W> {
        let New {
            base,
            w,
            is_host,
            inits,
            mixins,
        } = n;
        let id = self.ui.insert_node(base, w, is_host, owner);
        let c = Ctrl::from_raw(id);
        for f in inits {
            f(self, c);
        }
        for f in mixins {
            f(self, c);
        }
        c
    }

    /// The store of `host.controls[key] = id`; a different control that `key` held is released.
    fn store(&mut self, host: CtrlId, key: CtrlKey, id: CtrlId) {
        let old = self.host_mut(host).map.insert(key.clone(), id);
        if let Some(old) = old {
            if old != id {
                self.release(host, &key, old);
            }
        }
    }

    /// `old` is no longer `host.controls[key]`: bury it when `host` owns it (nothing else in the
    /// Lua keeps it), else only unlink it.
    fn release(&mut self, host: CtrlId, key: &CtrlKey, old: CtrlId) {
        if !self.ui.is_alive(old) {
            return;
        }
        self.unlink_parent(host, key, old);
        // Still stored under another key of this host (a `put`): the table keeps it alive.
        if self.ui.node(old).owner == Some(host) && !self.is_child_of(host, old) {
            self.bury(old);
        }
    }

    fn unlink_parent(&mut self, host: CtrlId, key: &CtrlKey, id: CtrlId) {
        if let Some(n) = self.ui.nodes.get_mut(id) {
            if n.parent
                .as_ref()
                .is_some_and(|(h, k)| *h == host && k == key)
            {
                n.parent = None;
            }
        }
    }
}

/// Inert services for `Cx::headless`.
mod inert {
    use pob_sg::{FileEntry, HttpRequest, HttpResponse, ImageFlags, LoadEvent, TexId};

    pub(super) struct Inert;

    impl pob_sg::Platform for Inert {
        fn time_ms(&self) -> i64 {
            0
        }
        fn copy(&mut self, _text: &str) {}
        fn paste(&mut self) -> Option<String> {
            None
        }
        fn open_url(&mut self, _url: &str) {}
        fn set_window_title(&mut self, _title: &str) {}
        fn take_screenshot(&mut self) {}
        fn restart(&mut self) {}
        fn exit(&mut self, _msg: Option<&str>) {}
        fn script_path(&self) -> String {
            String::new()
        }
        fn runtime_path(&self) -> String {
            String::new()
        }
        fn user_path(&self) -> Result<String, (String, String)> {
            Ok(String::new())
        }
        fn con_print(&mut self, _s: &str) {}
    }

    impl pob_sg::Fs for Inert {
        fn read(&self, _path: &str) -> std::io::Result<Vec<u8>> {
            Err(std::io::ErrorKind::NotFound.into())
        }
        fn write(&mut self, _path: &str, _data: &[u8]) -> std::io::Result<()> {
            Err(std::io::ErrorKind::Unsupported.into())
        }
        fn search(&self, _spec: &str, _find_dirs: bool) -> Vec<FileEntry> {
            Vec::new()
        }
        fn make_dir(&mut self, _path: &str) -> std::io::Result<()> {
            Err(std::io::ErrorKind::Unsupported.into())
        }
        fn remove_dir(&mut self, _path: &str, _recursive: bool) -> std::io::Result<()> {
            Err(std::io::ErrorKind::Unsupported.into())
        }
        fn rename(&mut self, _from: &str, _to: &str) -> std::io::Result<()> {
            Err(std::io::ErrorKind::Unsupported.into())
        }
        fn remove(&mut self, _path: &str) -> std::io::Result<()> {
            Err(std::io::ErrorKind::Unsupported.into())
        }
    }

    impl pob_sg::Net for Inert {
        fn download_page(&mut self, _req: HttpRequest) -> u64 {
            0
        }
        fn poll(&mut self) -> Vec<(u64, HttpResponse)> {
            Vec::new()
        }
        fn busy(&self) -> bool {
            false
        }
    }

    impl pob_sg::ImageLoader for Inert {
        fn request(
            &mut self,
            id: TexId,
            _path: &str,
            _flags: ImageFlags,
            _sync: bool,
            _priority: i32,
        ) -> Option<LoadEvent> {
            Some(LoadEvent::Failed(id))
        }
        fn read_size(&mut self, _id: TexId, _path: &str) -> Option<(u32, u32)> {
            None
        }
        fn set_priority(&mut self, _id: TexId, _priority: i32) {}
        fn poll(&mut self) -> Vec<LoadEvent> {
            Vec::new()
        }
    }
}

#[cfg(test)]
mod tests {
    //! Arena acceptance (ui doc §12.3 U4): deferred free while a click closes its own popup, a
    //! dead id panics with the type name, canonical child order, `impl_parts!` upcast.
    use std::cell::Cell;
    use std::rc::Rc;

    use super::*;
    use crate::layout::{rect, Anchor};
    use crate::widget::PlainControl;

    struct Popup {
        title: &'static str,
    }
    impl Widget for Popup {
        const NAME: &'static str = "PopupDialog";
        const CAPS: Caps = Caps::empty();
    }

    struct Btn {
        clicks: u32,
    }
    impl Widget for Btn {
        const NAME: &'static str = "ButtonControl";
        const CAPS: Caps = Caps::KEY_DOWN;
        fn on_key_down(cx: &mut Cx, me: CtrlId, _key: Key, _dbl: bool) -> Focus {
            // onClick = function() main:ClosePopup() end, then the button keeps running.
            let popup = cx.ui.node(me).parent.as_ref().unwrap().0;
            cx.main.popups.clear();
            cx.bury(popup);
            cx.w_mut::<Btn>(me).clicks += 1;
            let _ = cx.base(me).width.get(cx, me);
            let _ = cx.w::<Popup>(popup).title;
            Some(me)
        }
    }

    fn plain(cx: &mut Cx, host: CtrlId, key: impl Into<CtrlKey>) -> CtrlId {
        cx.add(host, key, PlainControl::new(Anchor::NONE, rect(0, 0, 0, 0)))
            .id()
    }

    fn catch_msg(f: impl FnOnce()) -> String {
        match pob_lua::error::catch(f) {
            Ok(()) => panic!("expected a Lua error"),
            Err(e) => e.0,
        }
    }

    #[test]
    fn deferred_free_while_a_click_closes_its_own_popup() {
        let mut cx = Cx::headless();
        let popup = cx.add_free(
            None,
            New::new(
                Base::new(Anchor::NONE, rect(0, 0, 100, 50)),
                Popup { title: "t" },
            )
            .host(),
        );
        let btn = cx.add(
            popup,
            "close",
            New::new(
                Base::new(Anchor::NONE, rect(0, 0, 10, 10)),
                Btn { clicks: 0 },
            ),
        );
        let label = plain(&mut cx, popup.id(), "label");
        // The click runs to completion although it buried its own popup.
        let r = cx.on_key_down(btn.id(), Key::LeftButton, false);
        assert_eq!(r, Some(btn.id()));
        cx.select_control(popup.id(), r);
        assert_eq!(cx.get(btn).clicks, 1);
        assert!(cx.ui.is_alive(popup.id()) && cx.ui.is_alive(btn.id()));
        cx.end_frame_gc();
        assert!(!cx.ui.is_alive(popup.id()));
        assert!(!cx.ui.is_alive(btn.id()));
        assert!(!cx.ui.is_alive(label));
        assert!(cx.ui.nodes.is_empty());
    }

    #[test]
    fn dead_id_panics_with_type_name() {
        let mut cx = Cx::headless();
        let b = cx.add_free(
            None,
            New::new(Base::new(Anchor::NONE, rect(0, 0, 1, 1)), Btn { clicks: 0 }),
        );
        cx.bury(b);
        cx.end_frame_gc();
        let msg = catch_msg(|| {
            let _ = cx.get(b).clicks;
        });
        assert!(msg.contains("ButtonControl"), "{msg}");
        assert!(msg.contains("dead"), "{msg}");
        let msg = catch_msg(|| {
            let _ = cx.w::<Popup>(b.id()).title;
        });
        assert!(msg.contains("PopupDialog"), "{msg}");
    }

    #[test]
    fn wrong_type_names_both_classes() {
        let mut cx = Cx::headless();
        let b = cx.add_free(
            None,
            New::new(Base::new(Anchor::NONE, rect(0, 0, 1, 1)), Btn { clicks: 0 }),
        );
        let msg = catch_msg(|| {
            let _ = cx.w::<Popup>(b.id()).title;
        });
        assert!(
            msg.contains("ButtonControl") && msg.contains("PopupDialog"),
            "{msg}"
        );
        assert!(!cx.is::<Popup>(b.id()));
        assert!(cx.is::<Btn>(b.id()));
    }

    #[test]
    fn canonical_child_order() {
        let mut cx = Cx::headless();
        let host = cx
            .add_free(
                None,
                PlainControl::new(Anchor::NONE, rect(0, 0, 0, 0)).host(),
            )
            .id();
        let z = plain(&mut cx, host, "z");
        let i3 = plain(&mut cx, host, 3i64);
        let a = plain(&mut cx, host, "a");
        let i1 = plain(&mut cx, host, 1i64);
        let m = plain(&mut cx, host, "m");
        let in2 = plain(&mut cx, host, -2i64);
        assert_eq!(cx.children(host).as_slice(), &[in2, i1, i3, z, a, m]);
        // Re-assigning a key stores a NEW control (later seq) and buries the old one.
        let z2 = plain(&mut cx, host, "z");
        assert_eq!(cx.children(host).as_slice(), &[in2, i1, i3, a, m, z2]);
        assert!(cx.ui.node(z).buried);
        // `put` of an existing object keeps its creation seq.
        cx.put(host, "zz", z);
        assert_eq!(cx.children(host).as_slice(), &[in2, i1, i3, z, a, m, z2]);
        // Removing a key: `is_child_of` turns false at once, the node is freed after the frame.
        cx.remove_child(host, &"a".into());
        assert!(!cx.is_child_of(host, a));
        assert!(cx.ui.is_alive(a));
        cx.end_frame_gc();
        assert!(!cx.ui.is_alive(a));
        assert!(cx.ui.is_alive(z), "z was revived by put");
        assert_eq!(cx.children(host).as_slice(), &[in2, i1, i3, z, m, z2]);
        let d = cx.detach_child(host, &"m".into());
        assert_eq!(d, Some(m));
        assert!(cx.ui.is_alive(m));
        cx.clear_children(host);
        assert!(cx.children(host).is_empty());
        cx.end_frame_gc();
        assert!(!cx.ui.is_alive(z2) && !cx.ui.is_alive(i1));
    }

    #[test]
    fn inits_then_mixins_then_store() {
        let mut cx = Cx::headless();
        let host = cx
            .add_free(
                None,
                PlainControl::new(Anchor::NONE, rect(0, 0, 0, 0)).host(),
            )
            .id();
        let log = Rc::new(std::cell::RefCell::new(Vec::new()));
        let (l1, l2, l3) = (log.clone(), log.clone(), log.clone());
        let c = cx.add(
            host,
            "x",
            PlainControl::new(Anchor::NONE, rect(1, 2, 3, 4))
                .init(move |cx, me| {
                    l1.borrow_mut()
                        .push(format!("init stored={}", cx.child(host, "x").is_some()));
                    cx.base_mut(me).width = Prop::Val(10.0);
                })
                .mixin(move |cx, me| {
                    l2.borrow_mut().push(format!("mixin w={}", cx.width(me)));
                })
                .init(move |_, _| l3.borrow_mut().push("init2".into()))
                .width(Prop::Val(20.0)),
        );
        assert_eq!(
            *log.borrow(),
            vec!["init stored=false", "init2", "mixin w=10"]
        );
        assert_eq!(cx.width(c), 20.0);
        assert_eq!(cx.child(host, "x"), Some(c.id()));
    }

    #[test]
    fn set_anchor_needs_both_x_and_y() {
        let mut cx = Cx::headless();
        let o = cx
            .add_free(None, PlainControl::new(Anchor::NONE, rect(5, 7, 10, 10)))
            .id();
        let c = cx
            .add_free(None, PlainControl::new(Anchor::NONE, rect(1, 2, 4, 4)))
            .id();
        cx.set_anchor(
            c,
            AnchorPoint::TopLeft,
            Some(o),
            AnchorPoint::BottomRight,
            None,
            true,
        );
        assert_eq!(cx.pos(c), (16.0, 19.0));
        assert!(cx.base(c).anchor.collapse);
        cx.set_anchor(
            c,
            AnchorPoint::Center,
            Some(o),
            AnchorPoint::Center,
            Some((Prop::Val(0.0), Prop::Val(0.0))),
            false,
        );
        assert_eq!(cx.pos(c), (8.0, 10.0));
    }

    // ---- impl_parts! upcast ----

    struct Base0 {
        v: i32,
    }
    impl Widget for Base0 {
        const NAME: &'static str = "EditControl";
        const CAPS: Caps = Caps::KEY_DOWN;
        fn on_key_down(cx: &mut Cx, me: CtrlId, _k: Key, _d: bool) -> Focus {
            cx.w_mut::<Base0>(me).v += 1;
            Some(me)
        }
    }
    struct Sub {
        edit: Base0,
        extra: Cell<i32>,
    }
    impl Widget for Sub {
        const NAME: &'static str = "GemSelectControl";
        const CAPS: Caps = Caps::KEY_DOWN;
        crate::impl_parts!(Sub: edit => Base0);
        fn on_key_down(cx: &mut Cx, me: CtrlId, k: Key, d: bool) -> Focus {
            // self.EditControl:OnKeyDown(...) super call
            let r = Base0::on_key_down(cx, me, k, d);
            cx.w::<Sub>(me).extra.set(cx.w::<Base0>(me).v * 10);
            r
        }
    }
    struct SubSub {
        gem: Sub,
    }
    impl Widget for SubSub {
        const NAME: &'static str = "SubSub";
        const CAPS: Caps = Caps::KEY_DOWN;
        crate::impl_parts!(SubSub: gem => Sub);
    }

    #[test]
    fn impl_parts_upcast() {
        let mut cx = Cx::headless();
        let s = cx.add_free(
            None,
            New::new(
                Base::new(Anchor::NONE, rect(0, 0, 1, 1)),
                Sub {
                    edit: Base0 { v: 1 },
                    extra: Cell::new(0),
                },
            ),
        );
        assert!(cx.is::<Base0>(s.id()) && cx.is::<Sub>(s.id()));
        assert!(!cx.is::<Btn>(s.id()));
        assert_eq!(cx.ui.class_name(s.id()), "GemSelectControl");
        assert_eq!(cx.on_key_down(s.id(), Key::Return, false), Some(s.id()));
        assert_eq!(cx.w::<Base0>(s.id()).v, 2);
        assert_eq!(cx.get(s).extra.get(), 20);
        cx.w_mut::<Base0>(s.id()).v = 7;
        assert_eq!(cx.get(s).edit.v, 7);
        let ss = cx.add_free(
            None,
            New::new(
                Base::new(Anchor::NONE, rect(0, 0, 1, 1)),
                SubSub {
                    gem: Sub {
                        edit: Base0 { v: 3 },
                        extra: Cell::new(0),
                    },
                },
            ),
        );
        assert_eq!(cx.w::<Base0>(ss.id()).v, 3);
        assert!(cx.is::<Sub>(ss.id()) && cx.is::<SubSub>(ss.id()));
        let msg = catch_msg(|| {
            let _ = cx.w::<Btn>(ss.id()).clicks;
        });
        assert!(
            msg.contains("SubSub") && msg.contains("ButtonControl"),
            "{msg}"
        );
    }

    #[test]
    fn trace_markers_name_host_class_and_key() {
        let mut cx = Cx::headless();
        cx.sg.trace = Some(pob_sg::trace::TraceRecorder::new());
        let host = cx
            .add_free(
                None,
                PlainControl::new(Anchor::NONE, rect(0, 0, 0, 0)).host(),
            )
            .id();
        let a = plain(&mut cx, host, "notesDesc");
        let b = plain(&mut cx, host, 2i64);
        let rec = |cx: &mut Cx| {
            cx.sg.trace.as_mut().unwrap().record(
                pob_sg::trace::ApiCall::SetViewport(None),
                std::panic::Location::caller(),
            );
        };
        cx.trace_push(host, a);
        rec(&mut cx);
        cx.trace_push(host, b);
        rec(&mut cx);
        cx.trace_pop();
        cx.trace_pop();
        rec(&mut cx);
        let ctl: Vec<_> = cx
            .sg
            .trace
            .as_ref()
            .unwrap()
            .cur
            .calls
            .iter()
            .map(|c| c.ctl.clone())
            .collect();
        assert_eq!(
            ctl,
            vec![
                Some("Control.notesDesc".to_owned()),
                Some("Control.notesDesc/Control.2".to_owned()),
                None
            ]
        );
        // Without a recorder both are no-ops.
        cx.sg.trace = None;
        cx.trace_push(host, a);
        cx.trace_pop();
    }
}
