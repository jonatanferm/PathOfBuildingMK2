//! Port of PathOfBuilding v2.67.2 (16de4b82d) src/Classes/ControlHost.lua 1-115 and
//! Control.lua:140-149 (SetFocus). Ui doc §6.6. Owner: U4.
//! Verified against the real ControlHost.lua/Control.lua by `tests/tier0_host.rs` (T0: random
//! dispatch sequences with per-instance methods, focus returns, holes, drag lock, imbued early
//! return, `mouseClipRect`, errors on a missing `OnKeyDown`).
//!
//! @pob-file Classes/ControlHost.lua @16de4b82d
//! @pob-file Classes/Control.lua @16de4b82d
//! @pob Classes/ControlHost.lua:10-13 h=5f643809 (the constructor: `New::host()`, arena.rs)
//!
//! ORDER: `pairs(self.controls)` at ControlHost.lua:32 and :110 iterates `cx.children` (D7
//! canonical order, oracle patch 6). A control removed from the table earlier in the same loop
//! is skipped (`is_child_of`), as `next` skips nil values. The key set is snapshotted by id, so a
//! key reassigned to a *different* control mid-loop is also skipped, where Lua (and the shim's
//! `__canonPairs`, which re-reads `t[key]`) would visit the new value. No PoB code path reassigns
//! a `controls` key from inside these loops' callees (`IsMouseOver`/`Draw`); T0 only removes.
//!
//! The `GetMouseOverControl(viewPort)` argument at ControlHost.lua:84 is ignored by the method, so
//! the port drops it.

use crate::cx::Cx;
use crate::ids::{CtrlId, ViewPort};
use crate::input::{EventKind, InputEvents};
use crate::layout::is_mouse_in_region;
use crate::widget::{Caps, Duck};

impl Cx {
    /// ControlHost.lua:15-29
    /// lua: ControlHost.lua:15-29
    /// @pob Classes/ControlHost.lua:15-29 h=fc0d8d4e
    pub fn select_control(&mut self, host: CtrlId, new: Option<CtrlId>) {
        let cur = self.host(host).sel_control;
        if cur == new {
            return;
        }
        if let Some(c) = cur {
            if self.duck(c, Duck::SelDragActiveWithTargets) {
                return;
            }
            self.set_focus(c, false);
        }
        self.host_mut(host).sel_control = new;
        if let Some(n) = new {
            self.set_focus(n, true);
        }
    }

    /// ControlHost.lua:31-47 (instance override: Main.lua:1307)
    /// lua: ControlHost.lua:31-47
    /// @pob Classes/ControlHost.lua:31-47 h=45bcd6c9
    pub fn get_mouse_over_control(&self, host: CtrlId) -> Option<CtrlId> {
        if let Some(f) = self.ui.node(host).ov.get_mouse_over_control.clone() {
            return f(self, host);
        }
        for id in self.children(host) {
            if !self.is_child_of(host, id) {
                continue; // removed earlier in this iteration
            }
            if self.has(id, Caps::IS_MOUSE_OVER) && self.is_mouse_over(id) {
                let clip = self
                    .base(id)
                    .mouse_clip_rect
                    .as_ref()
                    .and_then(|p| p.get(self, id));
                let Some([x, y, w, h]) = clip else {
                    return Some(id);
                };
                let (cx_, cy) = self.sys.cursor_pos();
                if cx_ >= x && cy >= y && cx_ < x + w && cy < y + h {
                    return Some(id);
                }
            }
        }
        None
    }

    /// ControlHost.lua:49-107
    /// lua: ControlHost.lua:49-107
    /// @pob Classes/ControlHost.lua:49-107 h=0776ec29
    pub fn process_controls_input(&mut self, host: CtrlId, ev: &mut InputEvents, vp: &ViewPort) {
        let mut i = 0;
        while let Some(e) = ev.next_ipairs(&mut i) {
            match e.kind {
                EventKind::KeyDown { double_click } => {
                    let prev_sel = self.host(host).sel_control;
                    if let Some(sel) = prev_sel {
                        let imbued = self.duck(sel, Duck::ImbuedSelectDropped) && e.key.is_button();
                        let next = self.on_key_down(sel, e.key, double_click); // no caps check, as Lua :56
                        self.select_control(host, next);
                        ev.consume(i);
                        if imbued {
                            return; // no click-through
                        }
                    }
                    if self.host(host).sel_control.is_none() && e.key.is_button() {
                        self.select_control(host, None);
                        if is_mouse_in_region(self, vp) {
                            if let Some(m) = self.get_mouse_over_control(host) {
                                if Some(m) != prev_sel && self.has(m, Caps::KEY_DOWN) {
                                    let next = self.on_key_down(m, e.key, double_click);
                                    self.select_control(host, next);
                                    ev.consume(i);
                                }
                            }
                        }
                    }
                }
                EventKind::KeyUp => {
                    let sel = self.host(host).sel_control;
                    if let Some(s) = sel {
                        if self.has(s, Caps::KEY_UP) {
                            let n = self.on_key_up(s, e.key);
                            self.select_control(host, n);
                        }
                        ev.consume(i);
                    }
                    if let Some(m) = self.get_mouse_over_control(host) {
                        if sel.is_none() || self.has(m, Caps::HOVER_KEY_UP) {
                            if is_mouse_in_region(self, vp) {
                                if sel.is_none()
                                    && self.has(m, Caps::KEY_UP)
                                    && self.on_key_up(m, e.key).is_some()
                                {
                                    ev.consume(i);
                                }
                                if self.has(m, Caps::HOVER_KEY_UP) {
                                    self.on_hover_key_up(m, e.key);
                                }
                            }
                        }
                    }
                }
                EventKind::Char => {
                    if let Some(s) = self.host(host).sel_control {
                        if self.has(s, Caps::CHAR) {
                            let n = self.on_char(s, e.key);
                            self.select_control(host, n);
                        }
                        ev.consume(i);
                    }
                }
            }
        }
    }

    /// ControlHost.lua:109-115 (instance override: Main.lua:1297)
    pub fn draw_controls(&mut self, host: CtrlId, vp: &ViewPort, sel_control: Option<CtrlId>) {
        if let Some(f) = self.ui.node(host).ov.draw_controls.clone() {
            return f(self, host, vp, sel_control);
        }
        self.draw_controls_default(host, vp, sel_control)
    }

    /// The class method (`originalDrawControls` in Main.lua:1276).
    /// lua: ControlHost.lua:109-115
    /// @pob Classes/ControlHost.lua:109-115 h=d9ab0b08
    pub fn draw_controls_default(
        &mut self,
        host: CtrlId,
        vp: &ViewPort,
        sel_control: Option<CtrlId>,
    ) {
        for id in self.children(host) {
            if !self.is_child_of(host, id) {
                continue;
            }
            if self.is_shown(id) && self.has(id, Caps::DRAW) {
                let focused = |cx: &Cx, s: Option<CtrlId>| {
                    s.is_some_and(|s| cx.base(s).has_focus == Some(true) && s != id)
                };
                let no_tooltip =
                    focused(self, self.host(host).sel_control) || focused(self, sel_control);
                let traced = self.trace_push(host, id); // control-path marker (no-op unless tracing)
                self.draw(id, vp, no_tooltip);
                if traced {
                    self.trace_pop();
                }
            }
        }
    }

    /// Control.lua:140-149
    /// lua: Control.lua:140-149
    /// @pob Classes/Control.lua:140-149 h=170a5d6c
    pub fn set_focus(&mut self, id: CtrlId, focus: bool) {
        if Some(focus) != self.base(id).has_focus {
            if focus {
                if self.has(id, Caps::FOCUS_GAINED) {
                    self.on_focus_gained(id)
                }
            } else if self.has(id, Caps::FOCUS_LOST) {
                self.on_focus_lost(id)
            }
            self.base_mut(id).has_focus = Some(focus);
        }
    }
}
