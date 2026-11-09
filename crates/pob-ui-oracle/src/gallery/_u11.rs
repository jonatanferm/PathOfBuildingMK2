//! Support module of the U11 galleries (`u11_*`). Owner: U11.
//!
//! `pob-sg` does not record the `Copy`/`OpenURL` side effects yet (CCR-0028 item 2, proposed for
//! U1/U3), and the oracle's Rust-side platform (`boot::OraclePlatform`) does not either; the
//! Lua side's bridge records them itself (`sg_bridge.rs` "record unless Sg did"). EditControl
//! copies and opens URLs from inside its key handlers, so the U11 galleries wrap the platform:
//! [`RecordingPlatform`] queues the effects and [`flush_effects`] moves them into the frame's
//! trace, right after the call that produced them. Remove with CCR-0028 item 2.
#![allow(clippy::type_complexity)]

use std::cell::RefCell;
use std::rc::Rc;

use pob_host::PlatformParts;
use pob_sg::trace::SideEffect;
use pob_sg::Platform;
use pob_ui_core::prelude::*;

/// The effects queued by a [`RecordingPlatform`].
pub type EffectQueue = Rc<RefCell<Vec<SideEffect>>>;

/// A `Platform` that delegates to the oracle's and queues the side effects SG would record.
pub struct RecordingPlatform {
    inner: Box<dyn Platform>,
    queue: EffectQueue,
}

struct Placeholder;
impl Platform for Placeholder {
    fn time_ms(&self) -> i64 {
        0
    }
    fn copy(&mut self, _: &str) {}
    fn paste(&mut self) -> Option<String> {
        None
    }
    fn open_url(&mut self, _: &str) {}
    fn set_window_title(&mut self, _: &str) {}
    fn take_screenshot(&mut self) {}
    fn restart(&mut self) {}
    fn exit(&mut self, _: Option<&str>) {}
    fn script_path(&self) -> String {
        String::new()
    }
    fn runtime_path(&self) -> String {
        String::new()
    }
    fn user_path(&self) -> Result<String, (String, String)> {
        Ok(String::new())
    }
    fn con_print(&mut self, _: &str) {}
}

/// Wrap `parts.sys.platform`; returns the queue to flush.
pub fn wrap_platform(parts: &mut PlatformParts) -> EffectQueue {
    let queue = EffectQueue::default();
    let inner = std::mem::replace(&mut parts.sys.platform, Box::new(Placeholder));
    parts.sys.platform = Box::new(RecordingPlatform {
        inner,
        queue: queue.clone(),
    });
    queue
}

/// Move the queued effects into the current frame's trace.
pub fn flush_effects(cx: &mut Cx, queue: &EffectQueue) {
    let effects = std::mem::take(&mut *queue.borrow_mut());
    if let Some(t) = &mut cx.sg.trace {
        for e in effects {
            t.effect(e);
        }
    }
}

impl Platform for RecordingPlatform {
    fn time_ms(&self) -> i64 {
        self.inner.time_ms()
    }
    fn copy(&mut self, text: &str) {
        self.inner.copy(text);
        self.queue
            .borrow_mut()
            .push(SideEffect::Copy(text.to_owned()));
    }
    fn paste(&mut self) -> Option<String> {
        self.inner.paste()
    }
    fn open_url(&mut self, url: &str) {
        self.inner.open_url(url);
        self.queue
            .borrow_mut()
            .push(SideEffect::OpenUrl(url.to_owned()));
    }
    fn set_window_title(&mut self, title: &str) {
        self.inner.set_window_title(title);
        self.queue
            .borrow_mut()
            .push(SideEffect::WindowTitle(title.to_owned()));
    }
    fn take_screenshot(&mut self) {
        self.inner.take_screenshot();
        self.queue.borrow_mut().push(SideEffect::Screenshot);
    }
    fn restart(&mut self) {
        self.inner.restart();
    }
    fn exit(&mut self, msg: Option<&str>) {
        self.inner.exit(msg);
        self.queue.borrow_mut().push(SideEffect::Exit);
    }
    fn script_path(&self) -> String {
        self.inner.script_path()
    }
    fn runtime_path(&self) -> String {
        self.inner.runtime_path()
    }
    fn user_path(&self) -> Result<String, (String, String)> {
        self.inner.user_path()
    }
    fn con_print(&mut self, s: &str) {
        self.inner.con_print(s);
    }
}
