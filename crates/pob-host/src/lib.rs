//! `pob-host`: hosts for the PoB UI port (ui doc §5.2). Owner: U7 (loaders: U3).
//!
//! The [`App`] trait is what `pob-app` implements (`launch:OnInit`, `OnFrame`, the key callbacks).
//! Hosts: [`run_desktop`] (winit + pob-sg-wgpu), `run_web` (wasm32), [`run_headless`]
//! (deterministic, windowless; tests and `ui-parity`). Restructured by UI-U0 from the pre-port
//! `pob-sg/src/host.rs` (AMENDMENTS A4).
//!
//! All three hosts deliver input through [`deliver`] (SimpleGraphic `ui_main.cpp:516-557`
//! `KeyEvent`/`CallKeyHandler`, held-key bookkeeping of `sys_video.cpp`) and run frames through
//! [`run_frame`] (`ui_main.cpp:401-433`: BeginFrame, cursor snapshot, subscript completions,
//! OnFrame, EndFrame), so their callback order is identical.
//!
//! Net completions (CCR-0013 A6, ui doc §13 item 11; decided by U7, CCR-0026 A): the host polls
//! `sys.net` inside the frame, after `sg.begin_frame`/`sys.begin_frame` and before `on_frame`,
//! and hands each completion to [`App::on_net`] (SG runs `SubScriptFrame` →
//! `launch:OnSubFinished` at that point).
#![allow(clippy::too_many_lines)]

use pob_sg::Key;

pub mod bundle;
#[cfg(not(target_arch = "wasm32"))]
pub mod desktop;
pub mod headless;
pub mod input;
pub mod pacing;
pub mod script;

#[cfg(not(target_arch = "wasm32"))]
pub mod fs_native;
#[cfg(not(target_arch = "wasm32"))]
pub mod loader_native;
#[cfg(not(target_arch = "wasm32"))]
pub mod net_native;
#[cfg(not(target_arch = "wasm32"))]
pub mod platform_native;

pub mod loader_headless;

#[cfg(target_arch = "wasm32")]
pub mod fs_web;
#[cfg(target_arch = "wasm32")]
pub mod loader_web;
#[cfg(target_arch = "wasm32")]
pub mod net_web;
#[cfg(target_arch = "wasm32")]
pub mod platform_web;
#[cfg(target_arch = "wasm32")]
pub mod web;

#[cfg(not(target_arch = "wasm32"))]
pub use desktop::run_desktop;
pub use headless::run_headless;
#[cfg(target_arch = "wasm32")]
pub use web::run_web;

/// The Content-Type curl adds to a DownloadPage POST (Launch.lua:279-282, `OPT_POST` and
/// `OPT_POSTFIELDS`): `application/x-www-form-urlencoded` when the request has a body and none of
/// its header lines (`OPT_HTTPHEADER`) names a Content-Type, else `None`. Both
/// [`Net`](pob_sg::Net) hosts send it (FOLLOWUPS "CCRDOC" 3: ureq and `fetch` add none for a byte
/// body).
pub fn post_content_type(req: &pob_sg::HttpRequest) -> Option<&'static str> {
    let named = req.headers.iter().any(|line| {
        line.split_once(':')
            .is_some_and(|(k, _)| k.trim().eq_ignore_ascii_case("content-type"))
    });
    (req.body.is_some() && !named).then_some("application/x-www-form-urlencoded")
}

/// The PoB side of the host (Launch.lua's callbacks).
pub trait App: 'static {
    fn sg(&mut self) -> &mut pob_sg::Sg;
    fn sys(&mut self) -> &mut pob_sg::Sys;
    /// launch:OnInit
    fn on_init(&mut self);
    /// launch:OnFrame (between sg.begin_frame/end_frame)
    fn on_frame(&mut self);
    fn on_key_down(&mut self, key: Key, double_click: bool);
    fn on_key_up(&mut self, key: Key);
    fn on_char(&mut self, ch: Key);
    fn can_exit(&mut self) -> bool {
        true
    }
    fn on_exit(&mut self) {}
    /// hasActiveCoroutine || hasSubscript (ui_main.cpp:389): keeps frames running while unfocused.
    /// The hosts also keep frames running while `sys.net.busy()`.
    fn wants_frames(&self) -> bool {
        false
    }
    /// A callback panicked with a LuaError (D17): show Launch's error prompt (Launch.lua:381 ShowErrMsg).
    fn on_lua_error(&mut self, msg: String);
    /// A `Net` request finished (`launch:OnSubFinished(id, ...)` / `OnSubError`): called by the
    /// host inside the frame, after `sg.begin_frame` and before `on_frame`, once per completion
    /// in `Net::poll` order (CCR-0026 A). The default drops it.
    fn on_net(&mut self, id: u64, resp: pob_sg::HttpResponse) {
        let _ = (id, resp);
    }
    /// A `.pobscript` step only the app can perform (`load-build`, `snapshot`, and `fixture`
    /// keys the host does not know), called by [`run_headless`] between frames. The default
    /// ignores it (CCR-0026 E).
    fn on_script_step(&mut self, step: &script::Step) {
        let _ = step;
    }
}

/// built by the host (fonts loaded, loader installed)
pub struct PlatformParts {
    pub sg: pob_sg::Sg,
    pub sys: pob_sg::Sys,
}

/// Window and asset configuration.
pub struct HostConfig {
    pub title: String,
    /// Initial inner size in logical px.
    pub size: (u32, u32),
    pub min_size: (u32, u32),
    /// The script directory (`GetScriptPath`; relative file and image paths resolve here): the
    /// asset bundle (`assets/`, [`bundle`]), or a directory laid out as PoB's `src/`.
    pub script_dir: String,
    /// SimpleGraphic's runtime directory (`Fonts/`): the asset bundle, or PoB's
    /// `runtime/SimpleGraphic/`.
    pub sg_runtime: String,
}

/// The message of a panic payload (`lua_error!` payloads are `pob_lua::LuaError`).
pub fn panic_message(payload: &(dyn std::any::Any + Send)) -> String {
    if let Some(e) = payload.downcast_ref::<pob_lua::LuaError>() {
        e.0.clone()
    } else if let Some(s) = payload.downcast_ref::<&str>() {
        (*s).to_owned()
    } else if let Some(s) = payload.downcast_ref::<String>() {
        s.clone()
    } else {
        "unknown panic".to_owned()
    }
}

/// D17: run one callback inside `catch_unwind`; a panic goes to [`App::on_lua_error`]
/// (Launch's `PCall` + `ShowErrMsg`). `Err` only when `on_lua_error` itself panics, which is
/// fatal (SG's `DoError` → `Exit`). On wasm a panic aborts (no unwinding).
pub fn guarded<A: App>(app: &mut A, f: impl FnOnce(&mut A)) -> Result<(), String> {
    match std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| f(app))) {
        Ok(()) => Ok(()),
        Err(p) => {
            let msg = panic_message(&*p);
            std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| app.on_lua_error(msg)))
                .map_err(|p2| format!("--- SCRIPT ERROR ---\n{}", panic_message(&*p2)))
        }
    }
}

/// lua: ui_main.cpp:516-546 `KeyEvent` (+ `ui_console.cpp:99-111`, `sys_video.cpp` held state):
/// deliver one SG input event to the app.
///
/// - A [`input::Input::Key`] sets `heldKeyState[key] = down` first (the GLFW key / mouse callbacks
///   write it before `KeyEvent`); a KeyDown of `` ` ``/F1 with Ctrl held is swallowed (SG's
///   console toggle); KeyUp of F10 and of PAUSE with Shift held are swallowed.
/// - A [`input::Input::Wheel`] is KeyDown then KeyUp, held state untouched.
/// - A [`input::Input::Char`] is `OnChar`.
pub fn deliver<A: App>(app: &mut A, ev: input::Input) -> Result<(), String> {
    use input::Input;
    match ev {
        Input::Key {
            key,
            down: true,
            double_click,
        } => {
            app.sys().set_key(key, true);
            // ui_console_c::KeyEvent sees KE_KEYDOWN only (a double click is KE_DBLCLK).
            if !double_click && input::swallow_key_down(key, app.sys().is_key_down(Key::Ctrl)) {
                return Ok(());
            }
            guarded(app, |a| a.on_key_down(key, double_click))
        }
        Input::Key {
            key, down: false, ..
        } => {
            app.sys().set_key(key, false);
            if input::swallow_key_up(key, app.sys().is_key_down(Key::Shift)) {
                return Ok(());
            }
            guarded(app, |a| a.on_key_up(key))
        }
        Input::Wheel { up } => {
            let key = if up { Key::WheelUp } else { Key::WheelDown };
            guarded(app, |a| a.on_key_down(key, false))?;
            guarded(app, |a| a.on_key_up(key))
        }
        Input::Char(ch) => guarded(app, |a| a.on_char(ch)),
    }
}

/// lua: ui_main.cpp:401-433 (the body of `Frame` after the pacing test): `renderer->BeginFrame()`,
/// `GetRelativeCursor`, the subscript completions (here: `Net::poll` → [`App::on_net`]),
/// `OnFrame`, `EndFrame`. `cursor_phys` is the floored physical cursor.
pub fn run_frame<A: App>(
    app: &mut A,
    screen: pob_sg::ScreenInfo,
    cursor_phys: (i32, i32),
) -> Result<pob_sg::Frame, String> {
    app.sg().begin_frame(screen);
    let scale = app.sg().scale();
    app.sys().begin_frame(cursor_phys, scale);
    let done = app.sys().net.poll();
    let mut res = Ok(());
    for (id, resp) in done {
        if res.is_ok() {
            res = guarded(app, |a| a.on_net(id, resp));
        }
    }
    if res.is_ok() {
        res = guarded(app, A::on_frame);
    }
    let frame = app.sg().end_frame();
    res.map(|()| frame)
}

/// `hasActiveCoroutine || hasSubscript`.
pub fn wants_frames<A: App>(app: &mut A) -> bool {
    app.wants_frames() || app.sys().net.busy()
}

/// What the OAuth listener (`LaunchServer.lua`, run through `Net::oauth_listen`) returns:
/// `code, errMsg, state, port`. `Net::poll` carries it as an [`pob_sg::HttpResponse`]
/// (CCR-0026 B: the frozen `Net` has no OAuth completion type): `body` = code, `error` =
/// errMsg, `headers` = a JSON object `{"state": .., "port": ..}`.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct OAuthResult {
    pub code: Option<String>,
    pub err: Option<String>,
    pub state: Option<String>,
    pub port: Option<String>,
}

impl OAuthResult {
    pub fn into_response(self) -> pob_sg::HttpResponse {
        let mut h = serde_json::Map::new();
        if let Some(s) = self.state {
            h.insert("state".into(), serde_json::Value::String(s));
        }
        if let Some(p) = self.port {
            h.insert("port".into(), serde_json::Value::String(p));
        }
        pob_sg::HttpResponse {
            body: self.code.map(String::into_bytes),
            error: self.err,
            headers: serde_json::Value::Object(h).to_string(),
        }
    }

    pub fn from_response(r: &pob_sg::HttpResponse) -> OAuthResult {
        let h: serde_json::Value = serde_json::from_str(&r.headers).unwrap_or_default();
        let get = |k: &str| h.get(k).and_then(|v| v.as_str()).map(str::to_owned);
        OAuthResult {
            code: r
                .body
                .as_ref()
                .map(|b| String::from_utf8_lossy(b).into_owned()),
            err: r.error.clone(),
            state: get("state"),
            port: get("port"),
        }
    }
}

// ---- NewFileSearch helpers shared by the native, headless and web `Fs`s ----
// (`sys_main.cpp:139-245` `BuildGlobPattern`, `GlobMatch`, `find_c::FindFirst/FindNext`;
// `ui_api.cpp:1341-1430` `NewFileSearch` and the search handle methods.)

/// lua: sys_main.cpp:207-225 `FindFirst`: split a `NewFileSearch` spec into the directory to list
/// and the file-name pattern. `None` when the spec has no directory part (`parent_path()` is
/// empty, `directory_iterator("")` fails and `NewFileSearch` returns nil).
pub fn search_split_spec(spec: &str) -> Option<(&str, &str)> {
    let i = spec.rfind(['/', '\\'])?;
    let (dir, pat) = (&spec[..i], &spec[i + 1..]);
    // `path("/x").parent_path()` is "/": keep the root.
    Some((if dir.is_empty() { &spec[..=i] } else { dir }, pat))
}

/// lua: sys_main.cpp:139-205 `BuildGlobPattern` + `GlobMatch`: `*` and `*.*` match everything;
/// otherwise `*` is any run of codepoints, `?` exactly one, everything else literal; the
/// comparison is case-insensitive (RE2 `set_case_sensitive(false)`; simple case folding
/// approximated by `char::to_lowercase`).
pub fn glob_match(pattern: &str, name: &str) -> bool {
    if pattern == "*" || pattern == "*.*" {
        return true;
    }
    let fold = |c: char| c.to_lowercase().next().unwrap_or(c);
    let p: Vec<char> = pattern.chars().map(fold).collect();
    let n: Vec<char> = name.chars().map(fold).collect();
    // Wildcard matching with one backtrack point.
    let (mut pi, mut ni) = (0usize, 0usize);
    let mut star: Option<(usize, usize)> = None;
    while ni < n.len() {
        if pi < p.len() && (p[pi] == '?' || (p[pi] != '*' && p[pi] == n[ni])) {
            pi += 1;
            ni += 1;
        } else if pi < p.len() && p[pi] == '*' {
            star = Some((pi, ni));
            pi += 1;
        } else if let Some((sp, sn)) = star {
            pi = sp + 1;
            ni = sn + 1;
            star = Some((sp, sn + 1));
        } else {
            return false;
        }
    }
    while pi < p.len() && p[pi] == '*' {
        pi += 1;
    }
    pi == p.len()
}

/// `GetFileModifiedTime`: `last_write_time().time_since_epoch().count()` of MSVC's `file_clock`
/// (100 ns ticks since 1601-01-01), the reference platform's value. PoB only compares them.
pub fn filetime_ticks(t: std::time::SystemTime) -> i64 {
    const UNIX_TO_1601: i64 = 116_444_736_000_000_000;
    match t.duration_since(std::time::UNIX_EPOCH) {
        Ok(d) => UNIX_TO_1601 + (d.as_nanos() / 100) as i64,
        Err(e) => UNIX_TO_1601 - (e.duration().as_nanos() / 100) as i64,
    }
}

/// DIVERGENCE(listing order): SG yields `directory_iterator` order (NTFS: case-insensitive name
/// order). The ports sort case-insensitively, then bytewise, on every platform.
pub fn sort_file_entries(entries: &mut [pob_sg::FileEntry]) {
    entries.sort_by(|a, b| {
        a.name
            .to_lowercase()
            .cmp(&b.name.to_lowercase())
            .then_with(|| a.name.cmp(&b.name))
    });
}

// ---- In-memory file tree (the web `Fs`; persisted by `fs_web` into IndexedDB) ----

/// Reads of files a [`MemFs`] does not hold.
pub type MemFsFallback = Box<dyn Fn(&str) -> Option<Vec<u8>>>;

/// A change [`MemFs`] reports to its persistence hook, in the order the changes happen.
#[derive(Clone, Debug, PartialEq)]
pub enum MemFsChange {
    /// A file (`data: Some`) or directory (`data: None`) now exists at `path` with `modified`.
    Put {
        path: String,
        data: Option<Vec<u8>>,
        modified: i64,
    },
    /// `path` (a file or an empty directory) no longer exists.
    Delete { path: String },
}

/// An in-memory file system with the error behaviour of the native one (`std::fs` on the
/// reference platform): writes need an existing parent directory, `make_dir` is not recursive,
/// `remove_dir` without `recursive` needs an empty directory, `rename` moves files and trees.
/// Paths use `/`; relative paths resolve against `base` (the script directory). Reads of paths
/// the tree does not hold go to `fallback` (the web host's fetched PoB `src/`).
pub struct MemFs {
    base: String,
    files: std::collections::BTreeMap<String, (Vec<u8>, i64)>,
    dirs: std::collections::BTreeSet<String>,
    /// `GetFileModifiedTime` ticks for new and changed entries.
    pub clock: Box<dyn Fn() -> i64>,
    /// Called once per change (persistence).
    pub on_change: Box<dyn FnMut(MemFsChange)>,
    /// Reads of absent files.
    pub fallback: MemFsFallback,
}

fn mem_err(kind: std::io::ErrorKind, msg: &str, path: &str) -> std::io::Error {
    std::io::Error::new(kind, format!("{path}: {msg}"))
}

impl MemFs {
    /// An empty tree (only `/` exists).
    pub fn new(base: &str) -> MemFs {
        MemFs {
            base: base.replace('\\', "/"),
            files: std::collections::BTreeMap::new(),
            dirs: std::collections::BTreeSet::new(),
            clock: Box::new(|| 0),
            on_change: Box::new(|_| {}),
            fallback: Box::new(|_| None),
        }
    }

    /// Absolute, `/`-separated, `.`/`..` collapsed; no trailing `/` except for the root.
    pub fn normalize(&self, path: &str) -> String {
        let p = path.replace('\\', "/");
        let joined = if p.starts_with('/') {
            p
        } else {
            format!("{}/{p}", self.base)
        };
        let mut parts: Vec<&str> = Vec::new();
        for c in joined.split('/') {
            match c {
                "" | "." => {}
                ".." => {
                    parts.pop();
                }
                c => parts.push(c),
            }
        }
        format!("/{}", parts.join("/"))
    }

    fn parent(p: &str) -> Option<&str> {
        if p == "/" {
            return None;
        }
        let i = p.rfind('/')?;
        Some(if i == 0 { "/" } else { &p[..i] })
    }

    fn is_dir(&self, p: &str) -> bool {
        p == "/" || self.dirs.contains(p)
    }

    fn is_file(&self, p: &str) -> bool {
        self.files.contains_key(p)
    }

    fn parent_is_dir(&self, p: &str) -> bool {
        Self::parent(p).is_some_and(|d| self.is_dir(d))
    }

    /// Whether `p` is `d` or below it.
    fn under(p: &str, d: &str) -> bool {
        p == d || (p.starts_with(d) && (d == "/" || p.as_bytes().get(d.len()) == Some(&b'/')))
    }

    fn children(&self, d: &str) -> Vec<(String, bool, u64, i64)> {
        let direct = |p: &str| Self::parent(p) == Some(d);
        let mut out: Vec<(String, bool, u64, i64)> = self
            .dirs
            .iter()
            .filter(|p| direct(p))
            .map(|p| (p.rsplit('/').next().unwrap_or("").to_owned(), true, 0, 0))
            .collect();
        out.extend(
            self.files
                .iter()
                .filter(|(p, _)| direct(p))
                .map(|(p, (data, m))| {
                    (
                        p.rsplit('/').next().unwrap_or("").to_owned(),
                        false,
                        data.len() as u64,
                        *m,
                    )
                }),
        );
        out
    }

    /// Insert without reporting a change (hydration from storage). Missing parent directories
    /// are created.
    pub fn restore(&mut self, path: &str, data: Option<Vec<u8>>, modified: i64) {
        let p = self.normalize(path);
        let mut d = Self::parent(&p);
        while let Some(x) = d {
            if x != "/" {
                self.dirs.insert(x.to_owned());
            }
            d = Self::parent(x);
        }
        match data {
            Some(b) => {
                self.files.insert(p, (b, modified));
            }
            None => {
                if p != "/" {
                    self.dirs.insert(p);
                }
            }
        }
    }

    /// `mkdir -p` without reporting parents that already exist.
    pub fn make_dir_all(&mut self, path: &str) {
        let p = self.normalize(path);
        let mut chain = Vec::new();
        let mut d = Some(p.as_str());
        while let Some(x) = d {
            if self.is_dir(x) {
                break;
            }
            chain.push(x.to_owned());
            d = Self::parent(x);
        }
        let m = (self.clock)();
        for x in chain.into_iter().rev() {
            self.dirs.insert(x.clone());
            (self.on_change)(MemFsChange::Put {
                path: x,
                data: None,
                modified: m,
            });
        }
    }

    fn remove_tree(&mut self, p: &str) {
        let files: Vec<String> = self
            .files
            .keys()
            .filter(|f| Self::under(f, p))
            .cloned()
            .collect();
        let mut dirs: Vec<String> = self
            .dirs
            .iter()
            .filter(|d| Self::under(d, p))
            .cloned()
            .collect();
        for f in files {
            self.files.remove(&f);
            (self.on_change)(MemFsChange::Delete { path: f });
        }
        dirs.sort_by_key(|d| std::cmp::Reverse(d.len()));
        for d in dirs {
            self.dirs.remove(&d);
            (self.on_change)(MemFsChange::Delete { path: d });
        }
    }
}

impl pob_sg::Fs for MemFs {
    fn read(&self, path: &str) -> std::io::Result<Vec<u8>> {
        let p = self.normalize(path);
        if let Some((d, _)) = self.files.get(&p) {
            return Ok(d.clone());
        }
        if self.is_dir(&p) {
            return Err(mem_err(std::io::ErrorKind::Other, "Is a directory", path));
        }
        (self.fallback)(&p).ok_or_else(|| {
            mem_err(
                std::io::ErrorKind::NotFound,
                "No such file or directory",
                path,
            )
        })
    }
    fn write(&mut self, path: &str, data: &[u8]) -> std::io::Result<()> {
        let p = self.normalize(path);
        if self.is_dir(&p) {
            return Err(mem_err(std::io::ErrorKind::Other, "Is a directory", path));
        }
        if !self.parent_is_dir(&p) {
            return Err(mem_err(
                std::io::ErrorKind::NotFound,
                "No such file or directory",
                path,
            ));
        }
        let m = (self.clock)();
        self.files.insert(p.clone(), (data.to_vec(), m));
        (self.on_change)(MemFsChange::Put {
            path: p,
            data: Some(data.to_vec()),
            modified: m,
        });
        Ok(())
    }
    fn search(&self, spec: &str, find_dirs: bool) -> Vec<pob_sg::FileEntry> {
        let Some((dir, pattern)) = search_split_spec(spec) else {
            return Vec::new();
        };
        let d = self.normalize(dir);
        let mut out: Vec<pob_sg::FileEntry> = self
            .children(&d)
            .into_iter()
            .filter(|(name, is_dir, ..)| *is_dir == find_dirs && glob_match(pattern, name))
            .map(|(name, is_dir, size, modified)| pob_sg::FileEntry {
                name,
                is_dir,
                size,
                modified,
            })
            .collect();
        sort_file_entries(&mut out);
        out
    }
    fn make_dir(&mut self, path: &str) -> std::io::Result<()> {
        let p = self.normalize(path);
        if self.is_dir(&p) || self.is_file(&p) {
            return Err(mem_err(
                std::io::ErrorKind::AlreadyExists,
                "File exists",
                path,
            ));
        }
        if !self.parent_is_dir(&p) {
            return Err(mem_err(
                std::io::ErrorKind::NotFound,
                "No such file or directory",
                path,
            ));
        }
        self.dirs.insert(p.clone());
        let m = (self.clock)();
        (self.on_change)(MemFsChange::Put {
            path: p,
            data: None,
            modified: m,
        });
        Ok(())
    }
    fn remove_dir(&mut self, path: &str, recursive: bool) -> std::io::Result<()> {
        let p = self.normalize(path);
        if p == "/" || !self.is_dir(&p) {
            return Err(mem_err(
                std::io::ErrorKind::NotFound,
                "No such file or directory",
                path,
            ));
        }
        if !recursive && !self.children(&p).is_empty() {
            return Err(mem_err(
                std::io::ErrorKind::Other,
                "Directory not empty",
                path,
            ));
        }
        self.remove_tree(&p);
        Ok(())
    }
    fn rename(&mut self, from: &str, to: &str) -> std::io::Result<()> {
        let (src, dst) = (self.normalize(from), self.normalize(to));
        if !self.parent_is_dir(&dst) {
            return Err(mem_err(
                std::io::ErrorKind::NotFound,
                "No such file or directory",
                to,
            ));
        }
        if src == dst {
            return Ok(());
        }
        if let Some((data, m)) = self.files.get(&src).cloned() {
            if self.is_dir(&dst) {
                return Err(mem_err(std::io::ErrorKind::Other, "Is a directory", to));
            }
            self.remove_tree(&src);
            self.files.insert(dst.clone(), (data.clone(), m));
            (self.on_change)(MemFsChange::Put {
                path: dst,
                data: Some(data),
                modified: m,
            });
            return Ok(());
        }
        if !self.is_dir(&src) || src == "/" {
            return Err(mem_err(
                std::io::ErrorKind::NotFound,
                "No such file or directory",
                from,
            ));
        }
        if Self::under(&dst, &src) || self.is_file(&dst) {
            return Err(mem_err(std::io::ErrorKind::Other, "Invalid argument", to));
        }
        if self.is_dir(&dst) && !self.children(&dst).is_empty() {
            return Err(mem_err(
                std::io::ErrorKind::Other,
                "Directory not empty",
                to,
            ));
        }
        let moved = |p: &str| format!("{dst}{}", &p[src.len()..]);
        let dirs: Vec<String> = self
            .dirs
            .iter()
            .filter(|d| Self::under(d, &src))
            .cloned()
            .collect();
        let files: Vec<(String, (Vec<u8>, i64))> = self
            .files
            .iter()
            .filter(|(f, _)| Self::under(f, &src))
            .map(|(f, v)| (f.clone(), v.clone()))
            .collect();
        self.remove_tree(&src);
        let m = (self.clock)();
        let mut new_dirs: Vec<String> = dirs.iter().map(|d| moved(d)).collect();
        new_dirs.sort();
        for d in new_dirs {
            self.dirs.insert(d.clone());
            (self.on_change)(MemFsChange::Put {
                path: d,
                data: None,
                modified: m,
            });
        }
        for (f, (data, fm)) in files {
            let nf = moved(&f);
            self.files.insert(nf.clone(), (data.clone(), fm));
            (self.on_change)(MemFsChange::Put {
                path: nf,
                data: Some(data),
                modified: fm,
            });
        }
        Ok(())
    }
    fn remove(&mut self, path: &str) -> std::io::Result<()> {
        let p = self.normalize(path);
        if !self.is_file(&p) {
            return Err(mem_err(
                std::io::ErrorKind::NotFound,
                "No such file or directory",
                path,
            ));
        }
        self.files.remove(&p);
        (self.on_change)(MemFsChange::Delete { path: p });
        Ok(())
    }
}

/// The wgpu side of a winit window (desktop and web hosts).
pub(crate) struct Gfx {
    pub(crate) window: std::sync::Arc<winit::window::Window>,
    surface: wgpu::Surface<'static>,
    device: wgpu::Device,
    queue: wgpu::Queue,
    config: wgpu::SurfaceConfiguration,
    view_format: wgpu::TextureFormat,
    renderer: pob_sg_wgpu::SgRenderer,
}

impl Gfx {
    pub(crate) fn new(
        window: std::sync::Arc<winit::window::Window>,
        surface: wgpu::Surface<'static>,
        adapter: &wgpu::Adapter,
        device: wgpu::Device,
        queue: wgpu::Queue,
    ) -> Gfx {
        let caps = surface.get_capabilities(adapter);
        let (format, view_format) = pob_sg_wgpu::pick_surface_format(&caps);
        let size = window.inner_size();
        let config = wgpu::SurfaceConfiguration {
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
            format,
            color_space: Default::default(),
            width: size.width.max(1),
            height: size.height.max(1),
            present_mode: wgpu::PresentMode::AutoVsync,
            desired_maximum_frame_latency: 2,
            alpha_mode: caps
                .alpha_modes
                .first()
                .copied()
                .unwrap_or(wgpu::CompositeAlphaMode::Auto),
            view_formats: if view_format == format {
                vec![]
            } else {
                vec![view_format]
            },
        };
        surface.configure(&device, &config);
        let renderer = pob_sg_wgpu::SgRenderer::new(&device, &adapter.limits(), view_format);
        Gfx {
            window,
            surface,
            device,
            queue,
            config,
            view_format,
            renderer,
        }
    }

    /// `ScreenInfo` of the window now (`vid.fbSize`, `vid.size[1]`, `vid.dpiScale`); `None`
    /// while it has no area (minimised): SG keeps the last size then
    /// (`FramebufferSizeChanged` ignores iconified sizes).
    pub(crate) fn screen(&self) -> Option<pob_sg::ScreenInfo> {
        let size = self.window.inner_size();
        if size.width == 0 || size.height == 0 {
            return None;
        }
        Some(pob_sg::ScreenInfo {
            fb_w: size.width as i32,
            fb_h: size.height as i32,
            win_h: size.height as i32,
            dpi_scale: self.window.scale_factor() as f32,
        })
    }

    /// Upload, draw and present one frame (skipped while the window has no area).
    pub(crate) fn render(&mut self, frame: &pob_sg::Frame) {
        self.renderer.prepare(&self.device, &self.queue, frame);
        let size = self.window.inner_size();
        if size.width == 0 || size.height == 0 {
            return;
        }
        if size.width != self.config.width || size.height != self.config.height {
            self.config.width = size.width;
            self.config.height = size.height;
            self.surface.configure(&self.device, &self.config);
        }
        let tex = match self.surface.get_current_texture() {
            wgpu::CurrentSurfaceTexture::Success(t)
            | wgpu::CurrentSurfaceTexture::Suboptimal(t) => Some(t),
            wgpu::CurrentSurfaceTexture::Outdated | wgpu::CurrentSurfaceTexture::Lost => {
                self.surface.configure(&self.device, &self.config);
                None
            }
            _ => None,
        };
        let Some(tex) = tex else { return };
        let view = tex.texture.create_view(&wgpu::TextureViewDescriptor {
            format: Some(self.view_format),
            ..Default::default()
        });
        let mut enc = self
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("pob-host frame"),
            });
        self.renderer.render(&mut enc, &view);
        self.queue.submit(Some(enc.finish()));
        self.window.pre_present_notify();
        self.queue.present(tex);
    }

    /// The last rendered frame (the renderer's offscreen RT), RGBA8.
    #[cfg(not(target_arch = "wasm32"))]
    pub(crate) fn read_back(&self) -> (u32, u32, Vec<u8>) {
        self.renderer.read_back(&self.device, &self.queue)
    }
}

/// The host loop shared by the desktop and web hosts: app, input state, pacing.
pub(crate) struct Driver<A: App> {
    pub(crate) app: A,
    pub(crate) input: input::WinitInput,
    pub(crate) pacing: pacing::Pacing,
    /// A fatal error (on_lua_error panicked): the host exits with it.
    pub(crate) error: Option<String>,
    last_screen: Option<pob_sg::ScreenInfo>,
}

/// What [`Driver::frame`] did.
pub(crate) enum FrameOutcome {
    /// The pacing test skipped the frame: sleep [`pacing::IDLE_WAIT`].
    Skipped,
    /// The frame ran; sleep this long before the next one.
    Ran(Option<std::time::Duration>),
}

impl<A: App> Driver<A> {
    pub(crate) fn new(app: A) -> Self {
        Driver {
            app,
            input: input::WinitInput::default(),
            pacing: pacing::Pacing::default(),
            error: None,
            last_screen: None,
        }
    }

    fn fail(&mut self, r: Result<(), String>) {
        if let Err(e) = r {
            self.error.get_or_insert(e);
        }
    }

    /// Translate and deliver one window event (everything but redraw/close/resize). The web
    /// host has its own loop (it holds back Ctrl+V for the paste event).
    #[cfg_attr(target_arch = "wasm32", allow(dead_code))]
    pub(crate) fn window_event(
        &mut self,
        ev: &winit::event::WindowEvent,
        now_ms: f64,
        scale_factor: f64,
    ) {
        for i in self.input.translate(ev, now_ms, scale_factor) {
            let r = deliver(&mut self.app, i);
            self.fail(r);
            if self.error.is_some() {
                return;
            }
        }
    }

    /// `launch:OnInit` (guarded).
    pub(crate) fn init(&mut self) {
        let r = guarded(&mut self.app, A::on_init);
        self.fail(r);
    }

    /// The close button: `CanExit` (guarded; a panic counts as "no"). `true`: the host shuts down.
    /// A browser tab has no close request the page can veto.
    ///
    /// A panic counting as "no" matches an error in `main:CanExit`, which `launch:CanExit`
    /// catches: `ShowErrMsg`, then `return false` (Launch.lua:90-101). An error escaping
    /// `launch:CanExit` itself is different in SG: `PCall` calls `DoError` (`sys->Exit`, fatal)
    /// and `lua_toboolean` then reads the traceback string as true (ui_main.cpp:149-185,
    /// :500-510). Only that launch-level case differs (an edge case for U16, which ports
    /// `launch:CanExit`).
    #[cfg_attr(target_arch = "wasm32", allow(dead_code))]
    pub(crate) fn can_exit(&mut self) -> bool {
        let mut ok = false;
        let r = guarded(&mut self.app, |a| ok = a.can_exit());
        self.fail(r);
        ok
    }

    /// `ScriptShutdown`: `OnExit` (guarded).
    pub(crate) fn shutdown(&mut self) {
        let r = guarded(&mut self.app, A::on_exit);
        self.fail(r);
    }

    /// One pass of `ui_main_c::Frame` (pacing test, frame, render, post-frame sleep).
    pub(crate) fn frame(&mut self, gfx: &mut Gfx) -> FrameOutcome {
        let has_subscript = self.app.sys().net.busy();
        let wants = self.app.wants_frames() || has_subscript;
        if !self
            .pacing
            .begin_frame(self.input.active, self.input.cursor_over, wants)
        {
            return FrameOutcome::Skipped;
        }
        if let Some(s) = gfx.screen() {
            self.last_screen = Some(s);
        }
        let screen = self.last_screen.unwrap_or(pob_sg::ScreenInfo {
            fb_w: 1,
            fb_h: 1,
            win_h: 1,
            dpi_scale: gfx.window.scale_factor() as f32,
        });
        match run_frame(&mut self.app, screen, self.input.cursor_phys()) {
            Ok(frame) => gfx.render(&frame),
            Err(e) => {
                self.error.get_or_insert(e);
            }
        }
        // hasActiveCoroutine is refreshed by every PCall; hasSubscript is the frame-start value.
        let wants = self.app.wants_frames() || has_subscript;
        FrameOutcome::Ran(pacing::Pacing::sleep_after(self.input.active, wants))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn globs() {
        assert!(glob_match("*", "anything.xml"));
        assert!(glob_match("*.*", "no_dot"));
        assert!(glob_match("*.xml", "Build.XML"));
        assert!(!glob_match("*.xml", "Build.xml.bak"));
        assert!(glob_match("b?ild*", "Build 1.xml"));
        assert!(!glob_match("b?ild", "bld"));
        assert!(glob_match("Settings.xml", "settings.XML")); // verbatim, case-insensitive
        assert!(!glob_match("Settings.xml", "Settings.xmlx"));
        assert!(glob_match("*a*b*", "xxaYYbzz"));
        assert!(!glob_match("*a*b", "xxaYYbzz"));
        assert!(glob_match("é*", "Éa"));
        assert!(glob_match("[x]", "[X]")); // metacharacters are literal
        assert!(!glob_match("", "a"));
    }

    #[test]
    fn search_specs() {
        assert_eq!(search_split_spec("/a/b/*.xml"), Some(("/a/b", "*.xml")));
        assert_eq!(search_split_spec("Builds/"), Some(("Builds", "")));
        assert_eq!(search_split_spec("/x"), Some(("/", "x")));
        assert_eq!(search_split_spec("C:\\b\\*"), Some(("C:\\b", "*")));
        assert_eq!(search_split_spec("*.xml"), None);
        let t = std::time::UNIX_EPOCH + std::time::Duration::from_secs(1);
        assert_eq!(filetime_ticks(t), 116_444_736_010_000_000);
    }

    /// `run_frame`: the cursor snapshot is `GetCursorPos = lround(phys / scale)` (int / float)
    /// at the OS scale or the DPI override; the virtual screen is `floor(fb / scale)`.
    #[test]
    fn dpi_and_cursor_rounding() {
        let mut app = headless::tests::Recorder::new(headless::parts_without_assets());
        let screen = |dpi| pob_sg::ScreenInfo {
            fb_w: 1601,
            fb_h: 1001,
            win_h: 1001,
            dpi_scale: dpi,
        };
        let cases: [(f32, i32, (i32, i32), &str); 6] = [
            (1.0, 0, (101, 99), "cur=(101,99) vs=(1601,1001)"),
            (1.25, 0, (101, 99), "cur=(81,79) vs=(1280,800)"),
            // 1 / 1.5 = 0.67 -> 1; 2 / 1.5 = 1.33 -> 1; 4 / 1.5 = 2.67 -> 3
            (1.5, 0, (3, 1), "cur=(2,1) vs=(1067,667)"),
            (1.25, 0, (5, 3), "cur=(4,2) vs=(1280,800)"),
            (1.5, 0, (2, 4), "cur=(1,3) vs=(1067,667)"),
            // the override wins over the OS scale
            (1.0, 150, (2, 4), "cur=(1,3) vs=(1067,667)"),
        ];
        for (dpi, ov, cur, want) in cases {
            app.sg.set_dpi_scale_override_percent(ov);
            run_frame(&mut app, screen(dpi), cur).unwrap();
            let got = app.log.last().unwrap();
            assert!(got.ends_with(want), "{dpi} {ov} {cur:?}: {got}");
        }
    }

    #[test]
    fn mem_fs() {
        use pob_sg::Fs;
        use std::cell::RefCell;
        use std::rc::Rc;
        let log: Rc<RefCell<Vec<MemFsChange>>> = Rc::default();
        let mut fs = MemFs::new("/pob/src");
        let l2 = log.clone();
        fs.on_change = Box::new(move |c| l2.borrow_mut().push(c));
        fs.clock = Box::new(|| 7);
        fs.fallback = Box::new(|p| (p == "/pob/src/changelog.txt").then(|| b"v2.67.2".to_vec()));
        assert_eq!(fs.normalize("a/../b/./c"), "/pob/src/b/c");
        assert_eq!(fs.normalize("C:\\x\\..\\y"), "/pob/src/C:/y");
        assert_eq!(fs.normalize("/"), "/");
        assert_eq!(fs.read("changelog.txt").unwrap(), b"v2.67.2");
        assert!(fs.read("/nope").is_err());
        fs.make_dir_all("/user");
        assert!(fs
            .write("/user/Path of Building/Settings.xml", b"x")
            .is_err()); // no parent
        assert!(fs.make_dir("/user/a/b").is_err()); // not recursive
        fs.make_dir("/user/Path of Building").unwrap();
        assert_eq!(
            fs.make_dir("/user/Path of Building").unwrap_err().kind(),
            std::io::ErrorKind::AlreadyExists
        );
        fs.make_dir("/user/Path of Building/Builds").unwrap();
        fs.write("/user/Path of Building/Settings.xml", b"<s/>")
            .unwrap();
        fs.write("/user/Path of Building/Builds/B.xml", b"bb")
            .unwrap();
        fs.write("/user/Path of Building/Builds/a.xml", b"a")
            .unwrap();
        let names = |v: Vec<pob_sg::FileEntry>| v.into_iter().map(|e| e.name).collect::<Vec<_>>();
        assert_eq!(
            names(fs.search("/user/Path of Building/Builds/*.xml", false)),
            ["a.xml", "B.xml"]
        );
        assert_eq!(
            names(fs.search("/user/Path of Building/*", true)),
            ["Builds"]
        );
        let e = fs.search("/user/Path of Building/Builds/B.xml", false);
        assert_eq!((e[0].size, e[0].modified, e[0].is_dir), (2, 7, false));
        assert!(fs
            .remove_dir("/user/Path of Building/Builds", false)
            .is_err());
        fs.rename(
            "/user/Path of Building/Builds",
            "/user/Path of Building/Old",
        )
        .unwrap();
        assert_eq!(
            names(fs.search("/user/Path of Building/Old/*", false)),
            ["a.xml", "B.xml"]
        );
        assert!(fs.read("/user/Path of Building/Builds/a.xml").is_err());
        fs.rename(
            "/user/Path of Building/Old/a.xml",
            "/user/Path of Building/c.xml",
        )
        .unwrap();
        fs.remove("/user/Path of Building/c.xml").unwrap();
        assert!(fs.remove("/user/Path of Building/c.xml").is_err());
        fs.remove_dir("/user/Path of Building/Old", true).unwrap();
        assert!(fs.search("/user/Path of Building/*", true).is_empty());
        // Replaying the change log into a fresh tree (IndexedDB hydration) gives the same tree.
        let mut fresh = MemFs::new("/pob/src");
        let mut live: std::collections::BTreeMap<String, Option<Vec<u8>>> = Default::default();
        for c in log.borrow().iter() {
            match c {
                MemFsChange::Put { path, data, .. } => {
                    live.insert(path.clone(), data.clone());
                }
                MemFsChange::Delete { path } => {
                    live.remove(path);
                }
            }
        }
        for (p, d) in live {
            fresh.restore(&p, d, 7);
        }
        assert_eq!(fresh.files, fs.files);
        assert_eq!(fresh.dirs, fs.dirs);
        assert_eq!(
            names(fresh.search("/user/Path of Building/*", false)),
            ["Settings.xml"]
        );
    }

    #[test]
    fn oauth_result_round_trip() {
        for r in [
            OAuthResult::default(),
            OAuthResult {
                code: Some("abc".into()),
                err: None,
                state: Some("0123456789abcdef".into()),
                port: Some("49082".into()),
            },
            OAuthResult {
                code: None,
                err: Some("Timeout reached without a response received by the local server".into()),
                state: None,
                port: Some("49083".into()),
            },
        ] {
            assert_eq!(OAuthResult::from_response(&r.clone().into_response()), r);
        }
    }
}
