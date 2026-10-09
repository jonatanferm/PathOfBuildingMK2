//! Native [`Platform`] (ui doc §4.6, §5.2 "Clipboard and network"). Owner: U7.
//!
//! Moved from the pre-port `pob-sg/src/host.rs` `InputState` (time, clipboard via arboard,
//! `open_url`, window title, exit) by UI-U0. Requests that need the window (title, exit) are
//! queued in [`HostRequests`] and applied by the desktop host after the frame.

use std::cell::RefCell;
use std::rc::Rc;
use std::time::Instant;

use pob_sg::Platform;

/// Window-side effects requested by Lua during a callback.
#[derive(Default, Debug)]
pub struct HostRequests {
    pub title: Option<String>,
    pub exit: bool,
    pub restart: bool,
    pub screenshot: bool,
}

/// Desktop services.
pub struct NativePlatform {
    start: Instant,
    clipboard: Option<arboard::Clipboard>,
    script_path: String,
    runtime_path: String,
    pub requests: Rc<RefCell<HostRequests>>,
}

impl NativePlatform {
    pub fn new(script_path: String, runtime_path: String) -> NativePlatform {
        NativePlatform {
            start: Instant::now(),
            clipboard: None,
            script_path,
            runtime_path,
            requests: Rc::default(),
        }
    }

    fn clipboard(&mut self) -> Option<&mut arboard::Clipboard> {
        if self.clipboard.is_none() {
            self.clipboard = arboard::Clipboard::new().ok();
        }
        self.clipboard.as_mut()
    }
}

/// Open a URL with the desktop's default handler.
pub fn open_url(url: &str) -> Result<(), String> {
    let mut cmd = if cfg!(target_os = "windows") {
        let mut c = std::process::Command::new("cmd");
        c.args(["/C", "start", "", url]);
        c
    } else if cfg!(target_os = "macos") {
        let mut c = std::process::Command::new("open");
        c.arg(url);
        c
    } else {
        let mut c = std::process::Command::new("xdg-open");
        c.arg(url);
        c
    };
    cmd.spawn().map(|_| ()).map_err(|e| e.to_string())
}

impl Platform for NativePlatform {
    fn time_ms(&self) -> i64 {
        self.start.elapsed().as_millis() as i64
    }
    fn copy(&mut self, text: &str) {
        if let Some(cb) = self.clipboard() {
            let _ = cb.set_text(text.to_owned());
        }
    }
    fn paste(&mut self) -> Option<String> {
        self.clipboard()?.get_text().ok()
    }
    fn open_url(&mut self, url: &str) {
        if let Err(e) = open_url(url) {
            eprintln!("OpenURL: {e}");
        }
    }
    fn set_window_title(&mut self, title: &str) {
        self.requests.borrow_mut().title = Some(title.to_owned());
    }
    fn take_screenshot(&mut self) {
        self.requests.borrow_mut().screenshot = true;
    }
    fn restart(&mut self) {
        self.requests.borrow_mut().restart = true;
    }
    fn exit(&mut self, msg: Option<&str>) {
        if let Some(m) = msg {
            eprintln!("{m}");
        }
        self.requests.borrow_mut().exit = true;
    }
    fn script_path(&self) -> String {
        self.script_path.clone()
    }
    fn runtime_path(&self) -> String {
        self.runtime_path.clone()
    }
    fn user_path(&self) -> Result<String, (String, String)> {
        todo!("U7: GetUserPath (sys_main.cpp; Documents/Path of Building on Windows)")
    }
    fn con_print(&mut self, s: &str) {
        println!("{s}");
    }
}
