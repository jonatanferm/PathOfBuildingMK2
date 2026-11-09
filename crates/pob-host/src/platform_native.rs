//! Native [`Platform`] (ui doc §4.6, §5.2 "Clipboard and network"). Owner: U7.
//!
//! Port of PathOfBuilding-SimpleGraphic `3b1a346`: `ui_api.cpp` `l_GetTime`, `l_Copy`, `l_Paste`,
//! `l_OpenURL`, `l_SetWindowTitle`, `l_TakeScreenshot`, `l_Restart`, `l_Exit`, `l_GetScriptPath`,
//! `l_GetRuntimePath`, `l_GetUserPath` (`:1700-1760`), `l_ConPrintf`; `sys_main.cpp:582-605`
//! `FindUserPath`. Requests that need the window (title, exit, restart, screenshot) are queued in
//! [`HostRequests`] and applied by the desktop host after the frame, as SG does (`Exit`/`Restart`
//! set flags the main loop reads; `TakeScreenshot` is the `screenshot` console command, taken in
//! `EndFrame`).
//!
//! DIVERGENCE(Documents folder): on Windows SG asks `SHGetKnownFolderPath(FOLDERID_Documents)`;
//! the port uses `%USERPROFILE%\Documents` (a redirected Documents folder is not followed).
//! DIVERGENCE(paths): `GetScriptPath`/`GetRuntimePath` are made absolute but not canonicalised
//! (symlinks are kept); the runtime path is the parent of `HostConfig::sg_runtime` (SG's
//! `basePath`, the directory holding `SimpleGraphic/`).

use std::cell::RefCell;
use std::path::{Path, PathBuf};
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

/// The system clipboard, created on first use and shared by the platform and the OAuth
/// listener (`LaunchServer.lua` calls `Copy`).
#[derive(Clone, Default)]
pub struct SharedClipboard(Rc<RefCell<Option<arboard::Clipboard>>>);

impl SharedClipboard {
    /// `Copy(text)` (`glfwSetClipboardString`).
    pub fn copy(&self, text: &str) {
        let mut cb = self.0.borrow_mut();
        if cb.is_none() {
            *cb = arboard::Clipboard::new().ok();
        }
        if let Some(cb) = cb.as_mut() {
            let _ = cb.set_text(text.to_owned());
        }
    }

    /// `Paste()` (`glfwGetClipboardString`).
    pub fn paste(&self) -> Option<String> {
        let mut cb = self.0.borrow_mut();
        if cb.is_none() {
            *cb = arboard::Clipboard::new().ok();
        }
        cb.as_mut()?.get_text().ok()
    }
}

/// `generic_u8string()`: `/` separators.
pub fn generic(p: &Path) -> String {
    let s = p.to_string_lossy();
    if cfg!(windows) {
        s.replace('\\', "/")
    } else {
        s.into_owned()
    }
}

fn absolute(p: &str) -> PathBuf {
    std::path::absolute(p).unwrap_or_else(|_| PathBuf::from(p))
}

/// lua: sys_main.cpp:582-605 `FindUserPath`.
pub fn find_user_path() -> Result<String, (String, String)> {
    if cfg!(windows) {
        // DIVERGENCE(Documents folder), see the module docs.
        match std::env::var_os("USERPROFILE") {
            Some(home) => Ok(generic(&PathBuf::from(home).join("Documents"))),
            None => Err((
                String::new(),
                "Could not obtain Documents path from Windows".to_owned(),
            )),
        }
    } else if let Some(d) = std::env::var_os("XDG_DATA_HOME") {
        Ok(generic(Path::new(&d)))
    } else if let Some(h) = std::env::var_os("HOME") {
        Ok(generic(&PathBuf::from(h).join(".local/share")))
    } else {
        // getpwuid(getuid())->pw_dir has no std equivalent.
        Err((
            String::new(),
            "Could not determine the home directory".to_owned(),
        ))
    }
}

/// Open a URL with the desktop's default handler (`l_OpenURL`: `ShellExecute` on Windows).
///
/// Windows uses `rundll32 url.dll,FileProtocolHandler`, which hands the URL to `ShellExecute`
/// without a shell in between. `cmd /C start "" url` would not work: Rust does not quote an
/// argument that has no spaces, so cmd would split an OAuth URL at its first `&`.
pub fn open_url(url: &str) -> Result<(), String> {
    let mut cmd = if cfg!(target_os = "windows") {
        let mut c = std::process::Command::new("rundll32");
        c.args(["url.dll,FileProtocolHandler", url]);
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

/// Desktop services.
pub struct NativePlatform {
    start: Instant,
    clipboard: SharedClipboard,
    script_path: String,
    runtime_path: String,
    user_path: Result<String, (String, String)>,
    pub requests: Rc<RefCell<HostRequests>>,
}

impl NativePlatform {
    /// `script_path` is PoB's `src/` (the directory of `Launch.lua`), `sg_runtime` the
    /// `runtime/SimpleGraphic/` directory (fonts); `GetRuntimePath` is its parent.
    pub fn new(script_path: String, sg_runtime: String) -> NativePlatform {
        let rt = absolute(&sg_runtime);
        let base = rt.parent().map_or(rt.clone(), Path::to_path_buf);
        NativePlatform {
            start: Instant::now(),
            clipboard: SharedClipboard::default(),
            script_path: generic(&absolute(&script_path)),
            runtime_path: generic(&base),
            user_path: find_user_path(),
            requests: Rc::default(),
        }
    }

    /// Share the clipboard with the OAuth listener ([`crate::net_native::NativeNet`]).
    pub fn clipboard(&self) -> SharedClipboard {
        self.clipboard.clone()
    }
}

impl Platform for NativePlatform {
    /// `GetTime`: ms since start.
    fn time_ms(&self) -> i64 {
        self.start.elapsed().as_millis() as i64
    }
    fn copy(&mut self, text: &str) {
        self.clipboard.copy(text);
    }
    fn paste(&mut self) -> Option<String> {
        self.clipboard.paste()
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
        self.user_path.clone()
    }
    fn con_print(&mut self, s: &str) {
        println!("{s}");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn paths() {
        let p = NativePlatform::new("/x/pob/src".into(), "/x/pob/runtime/SimpleGraphic".into());
        assert_eq!(p.script_path(), "/x/pob/src");
        assert_eq!(p.runtime_path(), "/x/pob/runtime");
        if !cfg!(windows) {
            // FindUserPath: $XDG_DATA_HOME, else $HOME/.local/share
            let want = std::env::var("XDG_DATA_HOME").ok().or_else(|| {
                std::env::var("HOME")
                    .ok()
                    .map(|h| format!("{h}/.local/share"))
            });
            assert_eq!(p.user_path().ok(), want);
        }
        assert!(p.time_ms() >= 0);
    }

    #[test]
    fn requests_are_queued() {
        let mut p = NativePlatform::new("src".into(), "rt/SimpleGraphic".into());
        let r = p.requests.clone();
        p.set_window_title("Path of Building");
        p.take_screenshot();
        p.restart();
        p.exit(None);
        let r = r.borrow();
        assert_eq!(r.title.as_deref(), Some("Path of Building"));
        assert!(r.screenshot && r.restart && r.exit);
        assert!(Path::new(&p.script_path()).is_absolute());
    }
}
