//! System, platform, file and network access (ui doc §4.6). Owner: U1 (traits implemented by U7
//! in pob-host).
//!
//! `GetCursorPos` / `IsKeyDown` / `GetTime` semantics: `ui_api.cpp` `l_GetCursorPos`
//! (`lround(cursorPhys / dpiScale)`, int over float), `l_IsKeyDown` + `KeyForName`,
//! `sys_main_c::GetTime` (ms since start). Moved from the pre-port `pob-sg/src/host.rs`
//! `InputState` by UI-U0.

use std::collections::HashSet;

use crate::types::{Key, Num};

/// Process-level services (`GetTime`, clipboard, window, paths, console).
pub trait Platform {
    /// GetTime (ms since start)
    fn time_ms(&self) -> i64;
    /// Copy
    fn copy(&mut self, text: &str);
    /// Paste
    fn paste(&mut self) -> Option<String>;
    /// OpenURL
    fn open_url(&mut self, url: &str);
    /// SetWindowTitle
    fn set_window_title(&mut self, title: &str);
    /// TakeScreenshot
    fn take_screenshot(&mut self);
    /// Restart
    fn restart(&mut self);
    /// Exit
    fn exit(&mut self, msg: Option<&str>);
    /// GetScriptPath
    fn script_path(&self) -> String;
    /// GetRuntimePath
    fn runtime_path(&self) -> String;
    /// GetUserPath -> path | (invalidPath, errMsg)
    fn user_path(&self) -> Result<String, (String, String)>;
    /// ConPrintf
    fn con_print(&mut self, s: &str);
}

/// One `NewFileSearch` result.
pub struct FileEntry {
    pub name: String,
    pub is_dir: bool,
    pub size: u64,
    pub modified: i64,
}

/// io.open / NewFileSearch / MakeDir / RemoveDir / os.*
pub trait Fs {
    fn read(&self, path: &str) -> std::io::Result<Vec<u8>>;
    fn write(&mut self, path: &str, data: &[u8]) -> std::io::Result<()>;
    fn search(&self, spec: &str, find_dirs: bool) -> Vec<FileEntry>;
    fn make_dir(&mut self, path: &str) -> std::io::Result<()>;
    fn remove_dir(&mut self, path: &str, recursive: bool) -> std::io::Result<()>;
    fn rename(&mut self, from: &str, to: &str) -> std::io::Result<()>;
    fn remove(&mut self, path: &str) -> std::io::Result<()>;
}

/// A `launch:DownloadPage` request.
pub struct HttpRequest {
    pub url: String,
    pub headers: Vec<String>,
    pub body: Option<Vec<u8>>,
    pub user_agent: Option<String>,
}

/// A `launch:DownloadPage` completion.
pub struct HttpResponse {
    pub body: Option<Vec<u8>>,
    pub error: Option<String>,
    pub headers: String,
}

/// launch:DownloadPage (LaunchSubScript+curl)
pub trait Net {
    fn download_page(&mut self, req: HttpRequest) -> u64;
    /// Completions are delivered by the host BEFORE on_frame (ui_main.cpp subscript order).
    fn poll(&mut self) -> Vec<(u64, HttpResponse)>;
    /// desktop only
    fn oauth_listen(&mut self, url: &str) -> Option<u64> {
        let _ = url;
        None
    }
    /// "hasSubscript" for frame pacing
    fn busy(&self) -> bool;
}

/// What Lua's system globals read: cursor, held keys, time, plus the platform services.
pub struct Sys {
    pub platform: Box<dyn Platform>,
    pub fs: Box<dyn Fs>,
    pub net: Box<dyn Net>,
    /// snapshotted at begin_frame (GetRelativeCursor)
    cursor_phys: (i32, i32),
    /// Sg::scale() at begin_frame
    scale: f32,
    /// heldKeyState, updated by the host before each key callback
    keys: HashSet<Key>,
}

impl Sys {
    pub fn new(platform: Box<dyn Platform>, fs: Box<dyn Fs>, net: Box<dyn Net>) -> Sys {
        Sys {
            platform,
            fs,
            net,
            cursor_phys: (0, 0),
            scale: 1.0,
            keys: HashSet::new(),
        }
    }

    /// Snapshot the cursor (physical px, already floored by the host) and the DPI scale.
    pub fn begin_frame(&mut self, cursor_phys: (i32, i32), scale: f32) {
        self.cursor_phys = cursor_phys;
        self.scale = scale;
    }

    /// GetCursorPos: lround(cursorPhys / dpiScale) (int / float)
    pub fn cursor_pos(&self) -> (Num, Num) {
        (
            ((self.cursor_phys.0 as f32) / self.scale).round() as f64,
            ((self.cursor_phys.1 as f32) / self.scale).round() as f64,
        )
    }

    /// IsKeyDown(key)
    pub fn is_key_down(&self, k: Key) -> bool {
        self.keys.contains(&k)
    }

    /// The host records key state before each key callback.
    pub fn set_key(&mut self, k: Key, down: bool) {
        if down {
            self.keys.insert(k);
        } else {
            self.keys.remove(&k);
        }
    }

    pub fn time_ms(&self) -> i64 {
        self.platform.time_ms()
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    /// Inert services for tests.
    pub(crate) struct Nop;
    impl Platform for Nop {
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
    impl Fs for Nop {
        fn read(&self, _path: &str) -> std::io::Result<Vec<u8>> {
            Err(std::io::ErrorKind::NotFound.into())
        }
        fn write(&mut self, _path: &str, _data: &[u8]) -> std::io::Result<()> {
            Ok(())
        }
        fn search(&self, _spec: &str, _find_dirs: bool) -> Vec<FileEntry> {
            Vec::new()
        }
        fn make_dir(&mut self, _path: &str) -> std::io::Result<()> {
            Ok(())
        }
        fn remove_dir(&mut self, _path: &str, _recursive: bool) -> std::io::Result<()> {
            Ok(())
        }
        fn rename(&mut self, _from: &str, _to: &str) -> std::io::Result<()> {
            Ok(())
        }
        fn remove(&mut self, _path: &str) -> std::io::Result<()> {
            Ok(())
        }
    }
    impl Net for Nop {
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

    pub(crate) fn sys() -> Sys {
        Sys::new(Box::new(Nop), Box::new(Nop), Box::new(Nop))
    }

    /// Moved from the pre-port host.rs `key_for_name` test (held-key half).
    #[test]
    fn held_keys() {
        let mut st = sys();
        st.set_key(Key::from_name("CTRL").unwrap(), true);
        st.set_key(Key::Char(b'z'), true);
        assert!(st.is_key_down(Key::from_name("CTRL").unwrap()));
        assert!(st.is_key_down(Key::from_name("Ctrl").unwrap()));
        assert!(st.is_key_down(Key::from_name("Z").unwrap()));
        assert!(!st.is_key_down(Key::Shift));
        st.set_key(Key::Ctrl, false);
        assert!(!st.is_key_down(Key::Ctrl));
    }

    /// Moved from the pre-port host.rs `cursor_virtual` test.
    #[test]
    fn cursor_virtual() {
        let mut st = sys();
        // The host floors the physical position (GetRelativeCursor): (301.7, 99.2) -> (301, 99).
        st.begin_frame((301, 99), 2.0);
        assert_eq!(st.cursor_pos(), (151.0, 50.0)); // lround(150.5)=151 (away from zero), lround(49.5)=50
    }
}
