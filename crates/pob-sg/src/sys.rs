//! System, platform, file and network access (ui doc §4.6). Owner: U1 (traits implemented by U7
//! in pob-host).
//!
//! `GetCursorPos` / `IsKeyDown` / `GetTime` semantics (PathOfBuilding-SimpleGraphic `3b1a346`):
//! `ui_api.cpp:1534-1541` `l_GetCursorPos` (`lround(VirtualMap(cursor) / dpiScale)`, int over
//! float; `VirtualMap` is the identity under `DPI_AWARE`), `ui_main.cpp:398`
//! (`GetRelativeCursor` snapshotted right after `BeginFrame`), `sys_video.cpp:735-742`
//! (`floor` of the GLFW cursor position), `ui_api.cpp:1567-1580` `l_IsKeyDown` +
//! `ui_main.cpp:574-591` `KeyForName`, `sys_video.cpp:533-536` (`heldKeyState` is written
//! before the key event is dispatched), `ui_api.cpp:1698-1703` `l_GetTime` (`sys->GetTime()`,
//! ms since start). Moved from the pre-port `pob-sg/src/host.rs` `InputState` by UI-U0.

use std::collections::HashSet;

use pob_lua::lua_error;

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
    ///
    /// sg: ui_api.cpp:1578 (`sys->IsKeyDown(key)` reads `heldKeyState`) @3b1a346
    pub fn is_key_down(&self, k: Key) -> bool {
        self.keys.contains(&k)
    }

    /// IsKeyDown(keyName) with SG's argument checks: an empty name and a name `KeyForName`
    /// does not know are Lua errors.
    ///
    /// sg: ui_api.cpp:1567-1580, ui_main.cpp:574-591 @3b1a346
    #[track_caller]
    pub fn is_key_down_name(&self, key_name: &str) -> bool {
        if key_name.is_empty() {
            lua_error!("IsKeyDown() argument 1: string is empty");
        }
        // KeyForName reads a C string: a name with an embedded NUL is cut there.
        let name = key_name.split('\0').next().unwrap_or("");
        let key = if name.is_empty() {
            None
        } else {
            Key::from_name(name)
        };
        match key {
            Some(k) => self.is_key_down(k),
            None => lua_error!("IsKeyDown(): unrecognised key name"),
        }
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

    /// GetCursorPos rounding at dpi 1.0 / 1.25 / 1.5: `lround((float)int / float)`.
    #[test]
    fn cursor_rounding_at_dpi() {
        let mut st = sys();
        st.begin_frame((7, -3), 1.0);
        assert_eq!(st.cursor_pos(), (7.0, -3.0));
        st.begin_frame((5, 10), 1.25);
        assert_eq!(st.cursor_pos(), (4.0, 8.0)); // 4.0, 8.0
        st.begin_frame((3, 13), 1.25);
        assert_eq!(st.cursor_pos(), (2.0, 10.0)); // 2.4 -> 2, 10.4 -> 10
        st.begin_frame((8, 7), 1.25);
        assert_eq!(st.cursor_pos(), (6.0, 6.0)); // 6.4 -> 6, 5.6 -> 6
        st.begin_frame((3, 4), 1.5);
        assert_eq!(st.cursor_pos(), (2.0, 3.0)); // 2.0, 2.666 -> 3
        st.begin_frame((-3, 1), 1.5);
        assert_eq!(st.cursor_pos(), (-2.0, 1.0)); // -2.0, 0.666 -> 1
                                                  // 0.75 in float: lround(0.5) = 1 away from zero, also for negatives
        st.begin_frame((3, -3), 6.0);
        assert_eq!(st.cursor_pos(), (1.0, -1.0));
    }

    /// IsKeyDown(name): SG's argument errors (ui_api.cpp:1575-1577).
    #[test]
    fn is_key_down_name_errors() {
        let mut st = sys();
        st.set_key(Key::Shift, true);
        st.set_key(Key::Char(b'a'), true);
        assert!(st.is_key_down_name("SHIFT"));
        assert!(st.is_key_down_name("shift"));
        assert!(st.is_key_down_name("A"));
        assert!(st.is_key_down_name("a\0zz")); // the C string is "a"
        assert!(!st.is_key_down_name("CTRL"));
        let err = |name: &str| {
            pob_lua::error::catch(|| st.is_key_down_name(name))
                .unwrap_err()
                .0
        };
        assert_eq!(err(""), "IsKeyDown() argument 1: string is empty");
        assert_eq!(err("SHFT"), "IsKeyDown(): unrecognised key name");
        assert_eq!(err(";"), "IsKeyDown(): unrecognised key name");
        assert_eq!(err("\0a"), "IsKeyDown(): unrecognised key name");
    }
}
