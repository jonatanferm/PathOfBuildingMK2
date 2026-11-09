//! Web [`Platform`] (ui doc §4.6, §5.2 "Web host"; wasm32 only). Owner: U7.
//!
//! Counterparts of SimpleGraphic's `l_GetTime`, `l_Copy`, `l_Paste`, `l_OpenURL`,
//! `l_SetWindowTitle`, `l_TakeScreenshot`, `l_Restart`, `l_Exit`, `l_GetScriptPath`,
//! `l_GetRuntimePath`, `l_GetUserPath`, `l_ConPrintf` (`ui_api.cpp`) in a browser tab.
//!
//! Paste: browsers hand clipboard text to the page only in a `paste` event. The web host
//! caches that text ([`WebPlatform::paste_cache`]) and then delivers the Ctrl+V KeyDown it held
//! back, so `Paste()` inside PoB's Ctrl+V handler returns the pasted text. `Copy()` writes
//! through `navigator.clipboard.writeText` and also updates the cache (a later `Paste()` without
//! a paste event returns what PoB copied, as the system clipboard would).
//!
//! DIVERGENCE(web platform): `TakeScreenshot` only logs (no file system to write to);
//! `Restart` reloads the page; `Exit` stops the frame loop (a tab cannot close itself);
//! `GetUserPath` is the virtual [`USER_PATH`] backed by IndexedDB (`fs_web`).

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use pob_sg::Platform;

/// `GetUserPath()` on the web: a directory of the IndexedDB-backed tree.
pub const USER_PATH: &str = "/user";

/// Browser services (performance.now clock, DOM clipboard, window.open, document.title).
pub struct WebPlatform {
    start: f64,
    /// The text of the last `paste` event (or `Copy()`).
    pub paste_cache: Rc<RefCell<Option<String>>>,
    /// `Exit()` was called: the host stops.
    pub exit_requested: Rc<Cell<bool>>,
    script_path: String,
    runtime_path: String,
}

fn now_ms() -> f64 {
    web_sys::window()
        .and_then(|w| w.performance())
        .map_or_else(js_sys::Date::now, |p| p.now())
}

impl WebPlatform {
    /// `script_path`: where PoB's `src/` is served; `runtime_path`: SG's base path (the
    /// directory holding `SimpleGraphic/`).
    pub fn new(script_path: String, runtime_path: String) -> WebPlatform {
        WebPlatform {
            start: now_ms(),
            paste_cache: Rc::default(),
            exit_requested: Rc::default(),
            script_path,
            runtime_path,
        }
    }
}

impl Platform for WebPlatform {
    /// `GetTime`: ms since start.
    fn time_ms(&self) -> i64 {
        (now_ms() - self.start) as i64
    }
    fn copy(&mut self, text: &str) {
        *self.paste_cache.borrow_mut() = Some(text.to_owned());
        if let Some(w) = web_sys::window() {
            // A rejected promise (no permission, no focus) leaves the system clipboard as is.
            let _ = w.navigator().clipboard().write_text(text);
        }
    }
    fn paste(&mut self) -> Option<String> {
        self.paste_cache.borrow().clone()
    }
    fn open_url(&mut self, url: &str) {
        if let Some(w) = web_sys::window() {
            let _ = w.open_with_url_and_target(url, "_blank");
        }
    }
    fn set_window_title(&mut self, title: &str) {
        if let Some(d) = web_sys::window().and_then(|w| w.document()) {
            d.set_title(title);
        }
    }
    fn take_screenshot(&mut self) {
        self.con_print("TakeScreenshot is not available in the browser");
    }
    fn restart(&mut self) {
        if let Some(w) = web_sys::window() {
            let _ = w.location().reload();
        }
    }
    fn exit(&mut self, msg: Option<&str>) {
        if let Some(m) = msg {
            web_sys::console::error_1(&m.into());
        }
        self.exit_requested.set(true);
    }
    fn script_path(&self) -> String {
        self.script_path.clone()
    }
    fn runtime_path(&self) -> String {
        self.runtime_path.clone()
    }
    fn user_path(&self) -> Result<String, (String, String)> {
        Ok(USER_PATH.to_owned())
    }
    fn con_print(&mut self, s: &str) {
        web_sys::console::log_1(&s.into());
    }
}
