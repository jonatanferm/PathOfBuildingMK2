//! The deterministic, windowless host (ui doc §5.2 `run_headless`). Owner: U7.
//!
//! Virtual clock (+16 ms per frame), virtual clipboard, fixture Fs, scripted Net, synchronous
//! image loads (`loader_headless`). Used by tests and by `ui-parity`: the oracle drives its
//! LuaJIT side as an [`App`] through the same function, so both sides see identical callback
//! sequences, clock values and fixtures.
//!
//! # What `run_headless` does
//!
//! 1. Replaces the app's `Sys` services with [`HeadlessPlatform`], [`HeadlessFs`] and
//!    [`ScriptedNet`] over one fresh [`HeadlessState`] (the script and runtime paths are carried
//!    over from the app's platform). Installs a `TraceRecorder` if the app's `Sg` has none.
//!    Images load however the app's `ImageRegistry` was built: use [`parts`] (or the desktop
//!    `build_parts(cfg, true)`) for synchronous loads.
//! 2. Applies the script's leading `screen`/`fixture` steps, then calls `on_init` at clock 0.
//! 3. Runs the remaining actions of [`Script::expand`] in order. A frame is [`crate::run_frame`]
//!    (`begin_frame`, cursor snapshot, `Net` completions → `on_net`, `on_frame`, `end_frame`),
//!    after which the clock advances by [`FRAME_MS`] and the side effects recorded since the
//!    previous frame (platform calls, file writes) are appended to that frame's trace.
//! 4. Stops after `exit` (when `CanExit` allows), after `Exit()`/`Restart()` from the app (both
//!    run `on_exit`), or after a fatal error; returns the frames recorded during the run.
//!
//! Defaults: screen 1280×800 at dpi 1.0 (`screen W H` is the framebuffer in physical px), cursor
//! at (0, 0) and unknown to double-click detection until the first `move`, user path
//! [`DEFAULT_USER_PATH`] (in-memory unless a `userdir` fixture names a directory).
//!
//! # Fixtures
//!
//! - `fixture userdir=DIR`: `GetUserPath()` returns DIR (resolved against the script's
//!   directory). Reads fall through to the disk; writes, new directories, renames and removals
//!   stay in memory (the fixture is never modified) and each write is a
//!   `SideEffect::FileWrite { path, xxh3 }`.
//! - `fixture net=FILE.json`: `DownloadPage` responses, a JSON array of
//!   `{"url": .., "body": "text" | null, "body_file": "path", "error": "msg", "headers": ".."}`
//!   (`body: null` is a subscript error: `HttpResponse::body == None`). A request completes at
//!   the start of the next frame with the first unused fixture for its URL (the last one is
//!   reused); an unknown URL gets `error = "No net fixture for <url>"`. `oauth_listen` is not
//!   available headless (`LaunchSubScript` returns nil).
//! - Other keys go to `App::on_script_step`.
//!
//! Determinism: modification times of files read from disk are reported as 0 and those written
//! during the run as a virtual time derived from the clock; listings are sorted.

use std::cell::{Cell, RefCell};
use std::collections::{BTreeMap, BTreeSet};
use std::path::{Component, Path, PathBuf};
use std::rc::Rc;

use pob_sg::trace::{FrameTrace, SideEffect, TraceRecorder};
use pob_sg::{
    AssetSource, FileEntry, FontSet, Fs, HttpRequest, HttpResponse, ImageRegistry, Net, Platform,
    ScreenInfo, Sg, Sys,
};

use crate::input::{DoubleClick, Input};
use crate::script::{is_mouse_button, Action, Script, Step};
use crate::{deliver, guarded, run_frame, App, PlatformParts};

/// The virtual clock step per frame.
pub const FRAME_MS: i64 = 16;
/// `GetUserPath()` without a `userdir` fixture (nothing exists there on disk).
pub const DEFAULT_USER_PATH: &str = "/pob-headless/user";

/// One `DownloadPage` fixture.
#[derive(Clone, Debug, PartialEq)]
pub struct NetFixture {
    pub url: String,
    pub response: HttpResponseFixture,
}

/// A cloneable [`HttpResponse`].
#[derive(Clone, Debug, PartialEq)]
pub struct HttpResponseFixture {
    pub body: Option<Vec<u8>>,
    pub error: Option<String>,
    pub headers: String,
}

impl HttpResponseFixture {
    fn to_response(&self) -> HttpResponse {
        HttpResponse {
            body: self.body.clone(),
            error: self.error.clone(),
            headers: self.headers.clone(),
        }
    }
}

#[derive(Default)]
struct NetState {
    fixtures: Vec<(NetFixture, bool)>,
    pending: Vec<(u64, String)>,
    next_id: u64,
}

#[derive(Default)]
struct Overlay {
    files: BTreeMap<PathBuf, (Vec<u8>, i64)>,
    dirs: BTreeSet<PathBuf>,
    /// Hides the disk entry at the path and everything below it.
    removed: BTreeSet<PathBuf>,
}

/// The shared state behind the headless services.
pub struct HeadlessState {
    pub clock_ms: Cell<i64>,
    pub clipboard: RefCell<Option<String>>,
    /// Side effects since the last frame ended.
    pub effects: RefCell<Vec<SideEffect>>,
    /// `ConPrintf` output.
    pub console: RefCell<Vec<String>>,
    pub script_path: RefCell<String>,
    pub runtime_path: RefCell<String>,
    pub user_path: RefCell<String>,
    pub exit_requested: Cell<bool>,
    pub restart_requested: Cell<bool>,
    overlay: RefCell<Overlay>,
    net: RefCell<NetState>,
    /// The script path's index when it is the asset bundle (VENDOR, [`crate::bundle`]; detected
    /// by [`HeadlessState::new`]).
    #[cfg(not(target_arch = "wasm32"))]
    bundle: Option<std::sync::Arc<crate::bundle::Bundle>>,
}

/// A handle to the shared state.
pub type Headless = Rc<HeadlessState>;

impl HeadlessState {
    pub fn new(script_path: &str, runtime_path: &str) -> Headless {
        Rc::new(HeadlessState {
            clock_ms: Cell::new(0),
            clipboard: RefCell::new(None),
            effects: RefCell::new(Vec::new()),
            console: RefCell::new(Vec::new()),
            script_path: RefCell::new(script_path.to_owned()),
            runtime_path: RefCell::new(runtime_path.to_owned()),
            user_path: RefCell::new(DEFAULT_USER_PATH.to_owned()),
            exit_requested: Cell::new(false),
            restart_requested: Cell::new(false),
            overlay: RefCell::new(Overlay::default()),
            net: RefCell::new(NetState::default()),
            #[cfg(not(target_arch = "wasm32"))]
            bundle: crate::bundle::Bundle::detect(Path::new(script_path)),
        })
    }

    fn effect(&self, e: SideEffect) {
        self.effects.borrow_mut().push(e);
    }

    /// Install `DownloadPage` fixtures.
    pub fn set_net_fixtures(&self, fixtures: Vec<NetFixture>) {
        self.net.borrow_mut().fixtures = fixtures.into_iter().map(|f| (f, false)).collect();
    }

    /// Lexically normalised absolute path (relative paths resolve against the script path,
    /// SG's working directory during Lua calls).
    /// VENDOR: a logical path of the asset bundle resolves to the bundle file that holds it.
    fn resolve(&self, path: &str) -> PathBuf {
        #[cfg(not(target_arch = "wasm32"))]
        if let Some(p) = self
            .bundle
            .as_deref()
            .and_then(|b| crate::fs_native::bundle_file(b, path))
        {
            return normalize(&p);
        }
        let p = Path::new(path);
        let joined = if p.is_absolute() {
            p.to_path_buf()
        } else {
            Path::new(&*self.script_path.borrow()).join(p)
        };
        normalize(&joined)
    }

    /// VENDOR: a change at or below the asset bundle's root is refused (read-only, as on the
    /// desktop, [`crate::bundle::read_only`]); else the overlay path of `path`.
    fn resolve_mut(&self, path: &str) -> std::io::Result<PathBuf> {
        #[cfg(not(target_arch = "wasm32"))]
        if self
            .bundle
            .as_deref()
            .is_some_and(|b| crate::fs_native::in_read_only_bundle(b, path))
        {
            return Err(crate::bundle::read_only(path));
        }
        Ok(self.resolve(path))
    }

    fn virtual_mtime(&self) -> i64 {
        // 2020-09-13T12:26:40Z + the virtual clock
        crate::filetime_ticks(std::time::UNIX_EPOCH + std::time::Duration::from_secs(1_600_000_000))
            + self.clock_ms.get() * 10_000
    }
}

fn normalize(p: &Path) -> PathBuf {
    let mut out = PathBuf::new();
    for c in p.components() {
        match c {
            Component::CurDir => {}
            Component::ParentDir => {
                out.pop();
            }
            c => out.push(c.as_os_str()),
        }
    }
    out
}

/// Load `DownloadPage` fixtures from a JSON file (see the module docs).
pub fn load_net_fixtures(path: &Path) -> Result<Vec<NetFixture>, String> {
    let text = std::fs::read_to_string(path).map_err(|e| format!("{}: {e}", path.display()))?;
    let v: serde_json::Value =
        serde_json::from_str(&text).map_err(|e| format!("{}: {e}", path.display()))?;
    let arr = v
        .as_array()
        .ok_or_else(|| format!("{}: expected a JSON array", path.display()))?;
    let dir = path.parent().unwrap_or(Path::new("."));
    let mut out = Vec::new();
    for (i, f) in arr.iter().enumerate() {
        let url = f
            .get("url")
            .and_then(|u| u.as_str())
            .ok_or_else(|| format!("{}: entry {i} has no url", path.display()))?;
        let body = if let Some(bf) = f.get("body_file").and_then(|b| b.as_str()) {
            Some(std::fs::read(dir.join(bf)).map_err(|e| format!("{bf}: {e}"))?)
        } else {
            match f.get("body") {
                Some(serde_json::Value::Null) => None,
                Some(b) => Some(b.as_str().unwrap_or_default().as_bytes().to_vec()),
                None => Some(Vec::new()),
            }
        };
        out.push(NetFixture {
            url: url.to_owned(),
            response: HttpResponseFixture {
                body,
                error: f.get("error").and_then(|e| e.as_str()).map(str::to_owned),
                headers: f
                    .get("headers")
                    .and_then(|h| h.as_str())
                    .unwrap_or_default()
                    .to_owned(),
            },
        });
    }
    Ok(out)
}

/// Deterministic [`Platform`].
pub struct HeadlessPlatform(pub Headless);

impl Platform for HeadlessPlatform {
    fn time_ms(&self) -> i64 {
        self.0.clock_ms.get()
    }
    fn copy(&mut self, text: &str) {
        *self.0.clipboard.borrow_mut() = Some(text.to_owned());
        self.0.effect(SideEffect::Copy(text.to_owned()));
    }
    fn paste(&mut self) -> Option<String> {
        self.0.clipboard.borrow().clone()
    }
    fn open_url(&mut self, url: &str) {
        self.0.effect(SideEffect::OpenUrl(url.to_owned()));
    }
    fn set_window_title(&mut self, title: &str) {
        self.0.effect(SideEffect::WindowTitle(title.to_owned()));
    }
    fn take_screenshot(&mut self) {
        self.0.effect(SideEffect::Screenshot);
    }
    fn restart(&mut self) {
        self.0.console.borrow_mut().push("Restart()".to_owned());
        self.0.restart_requested.set(true);
    }
    fn exit(&mut self, msg: Option<&str>) {
        if let Some(m) = msg {
            self.0.console.borrow_mut().push(m.to_owned());
        }
        self.0.effect(SideEffect::Exit);
        self.0.exit_requested.set(true);
    }
    fn script_path(&self) -> String {
        self.0.script_path.borrow().clone()
    }
    fn runtime_path(&self) -> String {
        self.0.runtime_path.borrow().clone()
    }
    fn user_path(&self) -> Result<String, (String, String)> {
        Ok(self.0.user_path.borrow().clone())
    }
    fn con_print(&mut self, s: &str) {
        self.0.console.borrow_mut().push(s.to_owned());
    }
}

/// Copy-on-write [`Fs`]: reads fall through to the disk, changes stay in memory.
pub struct HeadlessFs(pub Headless);

fn not_found(p: &Path) -> std::io::Error {
    std::io::Error::new(
        std::io::ErrorKind::NotFound,
        format!("{}: No such file or directory", p.display()),
    )
}

impl HeadlessFs {
    fn hidden(o: &Overlay, p: &Path) -> bool {
        p.ancestors().any(|a| o.removed.contains(a))
    }
    fn is_file(o: &Overlay, p: &Path) -> bool {
        o.files.contains_key(p) || (!Self::hidden(o, p) && p.is_file())
    }
    fn is_dir(o: &Overlay, p: &Path) -> bool {
        o.dirs.contains(p) || (!Self::hidden(o, p) && p.is_dir())
    }
    /// The merged children of directory `d`: (name, is_dir, size, modified).
    fn children(o: &Overlay, d: &Path) -> BTreeMap<String, (bool, u64, i64)> {
        let mut out = BTreeMap::new();
        if !Self::hidden(o, d) {
            if let Ok(rd) = std::fs::read_dir(d) {
                for e in rd.flatten() {
                    let p = d.join(e.file_name());
                    if Self::hidden(o, &p) {
                        continue;
                    }
                    let Ok(md) = std::fs::metadata(&p) else {
                        continue;
                    };
                    let size = if md.is_dir() { 0 } else { md.len() };
                    out.insert(
                        e.file_name().to_string_lossy().into_owned(),
                        (md.is_dir(), size, 0),
                    );
                }
            }
        }
        for dir in &o.dirs {
            if dir.parent() == Some(d) {
                if let Some(n) = dir.file_name() {
                    out.insert(n.to_string_lossy().into_owned(), (true, 0, 0));
                }
            }
        }
        for (f, (data, m)) in &o.files {
            if f.parent() == Some(d) {
                if let Some(n) = f.file_name() {
                    out.insert(
                        n.to_string_lossy().into_owned(),
                        (false, data.len() as u64, *m),
                    );
                }
            }
        }
        out
    }
    fn read_merged(o: &Overlay, p: &Path) -> std::io::Result<Vec<u8>> {
        if let Some((d, _)) = o.files.get(p) {
            return Ok(d.clone());
        }
        if Self::hidden(o, p) {
            return Err(not_found(p));
        }
        std::fs::read(p)
    }
    fn parent_exists(o: &Overlay, p: &Path) -> bool {
        p.parent().map_or(true, |d| Self::is_dir(o, d))
    }
    fn remove_tree(o: &mut Overlay, p: &Path) {
        o.files.retain(|f, _| !f.starts_with(p));
        o.dirs.retain(|d| !d.starts_with(p));
        o.removed.insert(p.to_path_buf());
    }
}

impl Fs for HeadlessFs {
    fn read(&self, path: &str) -> std::io::Result<Vec<u8>> {
        let p = self.0.resolve(path);
        Self::read_merged(&self.0.overlay.borrow(), &p)
    }
    fn write(&mut self, path: &str, data: &[u8]) -> std::io::Result<()> {
        let p = self.0.resolve_mut(path)?;
        let mtime = self.0.virtual_mtime();
        {
            let mut o = self.0.overlay.borrow_mut();
            if !Self::parent_exists(&o, &p) || Self::is_dir(&o, &p) {
                return Err(not_found(&p));
            }
            o.files.insert(p, (data.to_vec(), mtime));
        }
        self.0.effect(SideEffect::FileWrite {
            path: path.to_owned(),
            xxh3: xxhash_rust::xxh3::xxh3_64(data),
        });
        Ok(())
    }
    fn search(&self, spec: &str, find_dirs: bool) -> Vec<FileEntry> {
        let Some((dir, pattern)) = crate::search_split_spec(spec) else {
            return Vec::new();
        };
        // VENDOR: a logical directory of the asset bundle lists its index (read-only; disk
        // modification times are reported as 0, see the module docs)
        #[cfg(not(target_arch = "wasm32"))]
        if let Some(mut found) = self
            .0
            .bundle
            .as_deref()
            .and_then(|b| crate::fs_native::bundle_search(b, dir, pattern, find_dirs))
        {
            for e in &mut found {
                e.modified = 0;
            }
            return found;
        }
        let d = self.0.resolve(dir);
        let o = self.0.overlay.borrow();
        let mut out: Vec<FileEntry> = Self::children(&o, &d)
            .into_iter()
            .filter(|(name, (is_dir, _, _))| {
                *is_dir == find_dirs && crate::glob_match(pattern, name)
            })
            .map(|(name, (is_dir, size, modified))| FileEntry {
                name,
                is_dir,
                size,
                modified,
            })
            .collect();
        crate::sort_file_entries(&mut out);
        out
    }
    fn make_dir(&mut self, path: &str) -> std::io::Result<()> {
        let p = self.0.resolve_mut(path)?;
        let mut o = self.0.overlay.borrow_mut();
        if Self::is_dir(&o, &p) || Self::is_file(&o, &p) {
            return Err(std::io::Error::new(
                std::io::ErrorKind::AlreadyExists,
                "File exists",
            ));
        }
        if !Self::parent_exists(&o, &p) {
            return Err(not_found(&p));
        }
        o.dirs.insert(p);
        Ok(())
    }
    fn remove_dir(&mut self, path: &str, recursive: bool) -> std::io::Result<()> {
        let p = self.0.resolve_mut(path)?;
        let mut o = self.0.overlay.borrow_mut();
        if !Self::is_dir(&o, &p) {
            return Err(not_found(&p));
        }
        if !recursive && !Self::children(&o, &p).is_empty() {
            return Err(std::io::Error::other("Directory not empty"));
        }
        Self::remove_tree(&mut o, &p);
        Ok(())
    }
    fn rename(&mut self, from: &str, to: &str) -> std::io::Result<()> {
        let (src, dst) = (self.0.resolve_mut(from)?, self.0.resolve_mut(to)?);
        let mut o = self.0.overlay.borrow_mut();
        if !Self::parent_exists(&o, &dst) {
            return Err(not_found(&dst));
        }
        if Self::is_file(&o, &src) {
            let data = Self::read_merged(&o, &src)?;
            let m = o.files.get(&src).map_or(0, |f| f.1);
            Self::remove_tree(&mut o, &src);
            o.files.insert(dst, (data, m));
            return Ok(());
        }
        if !Self::is_dir(&o, &src) {
            return Err(not_found(&src));
        }
        // Copy the merged tree, then hide the source.
        let mut stack = vec![(src.clone(), dst.clone())];
        let mut files = Vec::new();
        let mut dirs = Vec::new();
        while let Some((s, d)) = stack.pop() {
            dirs.push(d.clone());
            for (name, (is_dir, _, m)) in Self::children(&o, &s) {
                if is_dir {
                    stack.push((s.join(&name), d.join(&name)));
                } else {
                    files.push((d.join(&name), Self::read_merged(&o, &s.join(&name))?, m));
                }
            }
        }
        Self::remove_tree(&mut o, &src);
        o.dirs.extend(dirs);
        for (p, data, m) in files {
            o.files.insert(p, (data, m));
        }
        Ok(())
    }
    fn remove(&mut self, path: &str) -> std::io::Result<()> {
        let p = self.0.resolve_mut(path)?;
        let mut o = self.0.overlay.borrow_mut();
        if !Self::is_file(&o, &p) {
            return Err(not_found(&p));
        }
        o.files.remove(&p);
        o.removed.insert(p);
        Ok(())
    }
}

/// Scripted [`Net`]: responses come from fixtures, delivered at the start of the next frame.
pub struct ScriptedNet(pub Headless);

impl Net for ScriptedNet {
    fn download_page(&mut self, req: HttpRequest) -> u64 {
        let mut n = self.0.net.borrow_mut();
        n.next_id += 1;
        let id = n.next_id;
        n.pending.push((id, req.url));
        id
    }
    fn poll(&mut self) -> Vec<(u64, HttpResponse)> {
        let mut n = self.0.net.borrow_mut();
        let pending = std::mem::take(&mut n.pending);
        pending
            .into_iter()
            .map(|(id, url)| {
                let idx = n
                    .fixtures
                    .iter()
                    .position(|(f, used)| !used && f.url == url)
                    .or_else(|| n.fixtures.iter().rposition(|(f, _)| f.url == url));
                let resp = match idx {
                    Some(i) => {
                        n.fixtures[i].1 = true;
                        n.fixtures[i].0.response.to_response()
                    }
                    None => HttpResponse {
                        body: Some(Vec::new()),
                        error: Some(format!("No net fixture for {url}")),
                        headers: String::new(),
                    },
                };
                (id, resp)
            })
            .collect()
    }
    fn busy(&self) -> bool {
        !self.0.net.borrow().pending.is_empty()
    }
}

/// The three headless services over `h`.
pub fn services(h: &Headless) -> (Box<dyn Platform>, Box<dyn Fs>, Box<dyn Net>) {
    (
        Box::new(HeadlessPlatform(h.clone())),
        Box::new(HeadlessFs(h.clone())),
        Box::new(ScriptedNet(h.clone())),
    )
}

struct RcAssets(Rc<dyn AssetSource>);

impl AssetSource for RcAssets {
    fn read_src(&self, path: &str) -> std::io::Result<Vec<u8>> {
        self.0.read_src(path)
    }
    fn read_runtime(&self, path: &str) -> std::io::Result<Vec<u8>> {
        self.0.read_runtime(path)
    }
}

/// Deterministic `PlatformParts`: fonts from `assets` (`runtime/SimpleGraphic/Fonts`), every image
/// load synchronous through `HeadlessLoader` over `assets` (PoB `src/`), headless services.
pub fn parts(assets: Rc<dyn AssetSource>) -> std::io::Result<PlatformParts> {
    let mut images = ImageRegistry::new(Box::new(crate::loader_headless::HeadlessLoader::new(
        Box::new(RcAssets(assets.clone())),
    )));
    let fonts = FontSet::load(&*assets, &mut images)?;
    Ok(parts_with(fonts, images))
}

/// Deterministic `PlatformParts` without PoB's assets (`FontSet::empty()`: text measures 0 and
/// draws only its colour escapes; every image is missing).
pub fn parts_without_assets() -> PlatformParts {
    struct NoAssets;
    impl AssetSource for NoAssets {
        fn read_src(&self, _: &str) -> std::io::Result<Vec<u8>> {
            Err(std::io::ErrorKind::NotFound.into())
        }
        fn read_runtime(&self, _: &str) -> std::io::Result<Vec<u8>> {
            Err(std::io::ErrorKind::NotFound.into())
        }
    }
    let images = ImageRegistry::new(Box::new(crate::loader_headless::HeadlessLoader::new(
        Box::new(NoAssets),
    )));
    parts_with(FontSet::empty(), images)
}

fn parts_with(fonts: FontSet, images: ImageRegistry) -> PlatformParts {
    let h = HeadlessState::new("", "");
    let (p, f, n) = services(&h);
    PlatformParts {
        sg: Sg::new(Rc::new(fonts), images),
        sys: Sys::new(p, f, n),
    }
}

/// The run's mutable host state.
struct Run {
    h: Headless,
    screen: ScreenInfo,
    cursor_phys: (i32, i32),
    last_cursor: Option<(i32, i32)>,
    dbl: DoubleClick,
    base: PathBuf,
    /// Frames already in the recorder before this run.
    first: usize,
}

impl Run {
    fn fixture<A: App>(&mut self, app: &mut A, key: &str, value: &str) -> Result<(), String> {
        match key {
            "userdir" => {
                let p = normalize(
                    &std::path::absolute(self.base.join(value))
                        .unwrap_or_else(|_| self.base.join(value)),
                );
                *self.h.user_path.borrow_mut() = p.to_string_lossy().replace('\\', "/");
                Ok(())
            }
            "net" => {
                let f = load_net_fixtures(&self.base.join(value))?;
                self.h.set_net_fixtures(f);
                Ok(())
            }
            _ => guarded(app, |a| {
                a.on_script_step(&Step::Fixture {
                    key: key.to_owned(),
                    value: value.to_owned(),
                });
            }),
        }
    }

    /// Virtual px → physical px: `GetCursorPos` divides by `VirtualScreenScaleFactor`.
    fn scale<A: App>(&self, app: &mut A) -> f64 {
        let o = app.sg().get_dpi_scale_override_percent();
        if o > 0 {
            f64::from(o) / 100.0
        } else {
            f64::from(self.screen.dpi_scale)
        }
    }

    /// Append the pending side effects to the last frame of this run (they wait for the first
    /// frame when none has run yet).
    fn flush_effects<A: App>(&self, app: &mut A) {
        let Some(t) = app.sg().trace.as_mut() else {
            return;
        };
        if t.frames.len() <= self.first {
            return;
        }
        let effects = std::mem::take(&mut *self.h.effects.borrow_mut());
        if let Some(last) = t.frames.last_mut() {
            last.effects.extend(effects);
        }
    }
}

/// Run `script` against `app` and return one trace per frame (`app.sg().trace` is installed by
/// the host when absent). See the module docs for the exact semantics.
#[allow(clippy::many_single_char_names)]
pub fn run_headless<A: App>(app: &mut A, script: &Script) -> Vec<FrameTrace> {
    let h = HeadlessState::new(
        &app.sys().platform.script_path(),
        &app.sys().platform.runtime_path(),
    );
    let (p, f, n) = services(&h);
    {
        let sys = app.sys();
        sys.platform = p;
        sys.fs = f;
        sys.net = n;
    }
    let installed = app.sg().trace.is_none();
    if installed {
        app.sg().trace = Some(TraceRecorder::new());
    }
    let first = app.sg().trace.as_ref().map_or(0, |t| t.frames.len());
    let mut run = Run {
        h: h.clone(),
        screen: ScreenInfo {
            fb_w: 1280,
            fb_h: 800,
            win_h: 800,
            dpi_scale: 1.0,
        },
        cursor_phys: (0, 0),
        last_cursor: None,
        dbl: DoubleClick::default(),
        base: script.base_dir.clone().unwrap_or_default(),
        first,
    };
    let actions = script.expand();
    let lead = actions
        .iter()
        .take_while(|a| matches!(a, Action::Screen { .. } | Action::Fixture { .. }))
        .count();
    let mut result: Result<(), String> = Ok(());
    for a in &actions[..lead] {
        result = result.and_then(|()| step(app, &mut run, a).map(|_| ()));
    }
    if result.is_ok() {
        result = guarded(app, A::on_init);
    }
    if result.is_ok() {
        for a in &actions[lead..] {
            match step(app, &mut run, a) {
                Ok(true) => {}
                Ok(false) => break,
                Err(e) => {
                    result = Err(e);
                    break;
                }
            }
            if h.exit_requested.get() || h.restart_requested.get() {
                // SG: the main loop ends (Exit) or the script restarts (Restart); both run OnExit.
                let _ = guarded(app, A::on_exit);
                break;
            }
        }
    }
    if let Err(e) = &result {
        h.console.borrow_mut().push(e.clone());
        eprintln!("run_headless: {e}");
    }
    run.flush_effects(app);
    let sg = app.sg();
    if installed {
        sg.trace.take().map(|t| t.frames).unwrap_or_default()
    } else {
        sg.trace
            .as_mut()
            .map(|t| t.frames.split_off(first.min(t.frames.len())))
            .unwrap_or_default()
    }
}

/// One action. `Ok(false)`: the run ends.
fn step<A: App>(app: &mut A, run: &mut Run, a: &Action) -> Result<bool, String> {
    match a {
        Action::Screen { w, h, dpi } => {
            run.screen = ScreenInfo {
                fb_w: *w,
                fb_h: *h,
                win_h: *h,
                dpi_scale: *dpi,
            };
        }
        Action::Fixture { key, value } => run.fixture(app, key, value)?,
        Action::Frame => {
            let r = run_frame(app, run.screen, run.cursor_phys).map(|_| ());
            run.flush_effects(app);
            run.h.clock_ms.set(run.h.clock_ms.get() + FRAME_MS);
            r?;
        }
        Action::Move(x, y) => {
            let s = run.scale(app);
            run.cursor_phys = ((x * s).round() as i32, (y * s).round() as i32);
            run.last_cursor = Some(run.cursor_phys);
        }
        Action::Input(Input::Key {
            key,
            down: true,
            double_click: _,
        }) if is_mouse_button(*key) => {
            let dbl = run
                .dbl
                .press(*key, run.last_cursor, run.h.clock_ms.get() as f64);
            deliver(
                app,
                Input::Key {
                    key: *key,
                    down: true,
                    double_click: dbl,
                },
            )?;
        }
        Action::Input(i) => deliver(app, *i)?,
        Action::PasteClipboard(t) => *run.h.clipboard.borrow_mut() = Some(t.clone()),
        Action::App(s) => guarded(app, |a| a.on_script_step(s))?,
        Action::Exit => {
            let mut ok = false;
            guarded(app, |a| ok = a.can_exit())?;
            if ok {
                guarded(app, A::on_exit)?;
                return Ok(false);
            }
        }
    }
    Ok(true)
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use pob_sg::{Align, Font, Key};

    /// A test app that logs every callback (with the clock and cursor `on_frame` sees) and
    /// reacts to a few keys: `c` copies, `n` downloads, `w` writes a file, `x` raises a Lua
    /// error, `q` calls `Exit()`.
    pub(crate) struct Recorder {
        pub(crate) sg: Sg,
        pub(crate) sys: Sys,
        pub(crate) log: Vec<String>,
    }

    impl Recorder {
        pub(crate) fn new(parts: PlatformParts) -> Recorder {
            Recorder {
                sg: parts.sg,
                sys: parts.sys,
                log: Vec::new(),
            }
        }
    }

    impl App for Recorder {
        fn sg(&mut self) -> &mut Sg {
            &mut self.sg
        }
        fn sys(&mut self) -> &mut Sys {
            &mut self.sys
        }
        fn on_init(&mut self) {
            self.log.push(format!("init t={}", self.sys.time_ms()));
            self.sys.platform.set_window_title("Recorder");
        }
        fn on_frame(&mut self) {
            let (x, y) = self.sys.cursor_pos();
            let (vw, vh) = self.sg.get_virtual_screen_size();
            self.log.push(format!(
                "frame t={} cur=({x},{y}) vs=({vw},{vh})",
                self.sys.time_ms()
            ));
            self.sg.set_draw_color(0.5, 0.25, 1.0);
            self.sg.draw_image(None, x - 2.0, y - 2.0, 5.0, 5.0);
            let n = self.log.len().to_string();
            self.sg
                .draw_string(0.0, 0.0, Align::Left, 16.0, Font::Var, &n);
        }
        fn on_key_down(&mut self, key: Key, double_click: bool) {
            self.log.push(format!("down {key} dbl={double_click}"));
            match key {
                Key::Char(b'c') => self.sys.platform.copy("copied"),
                Key::Char(b'n') => {
                    let id = self.sys.net.download_page(HttpRequest {
                        url: "https://example.invalid/a".into(),
                        headers: vec![],
                        body: None,
                        user_agent: None,
                    });
                    self.log.push(format!("request {id}"));
                }
                Key::Char(b'w') => {
                    let up = self.sys.platform.user_path().unwrap_or_default();
                    let r = self.sys.fs.write(&format!("{up}/Settings.xml"), b"<x/>");
                    self.log.push(format!("write ok={}", r.is_ok()));
                }
                Key::Char(b'x') => pob_lua::lua_error!("boom {}", 1),
                Key::Char(b'q') => self.sys.platform.exit(None),
                _ => {}
            }
        }
        fn on_key_up(&mut self, key: Key) {
            self.log.push(format!("up {key}"));
        }
        fn on_char(&mut self, ch: Key) {
            self.log.push(format!("char {:?}", ch.name()));
        }
        fn on_lua_error(&mut self, msg: String) {
            self.log.push(format!("error {msg}"));
        }
        fn on_net(&mut self, id: u64, resp: HttpResponse) {
            self.log.push(format!(
                "net {id} body={:?} err={:?} t={}",
                resp.body.map(|b| String::from_utf8_lossy(&b).into_owned()),
                resp.error,
                self.sys.time_ms()
            ));
        }
        fn on_script_step(&mut self, step: &Step) {
            self.log.push(format!("step {step:?}"));
        }
        fn can_exit(&mut self) -> bool {
            self.log.push("can_exit".into());
            true
        }
        fn on_exit(&mut self) {
            self.log.push("exit".into());
        }
    }

    pub(crate) fn tmp(tag: &str) -> PathBuf {
        let d =
            std::env::temp_dir().join(format!("pob-host-headless-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    fn run(text: &str, base: Option<PathBuf>) -> (Vec<FrameTrace>, Vec<String>) {
        let mut app = Recorder::new(parts_without_assets());
        let mut s = Script::parse(text).unwrap();
        s.base_dir = base;
        let t = run_headless(&mut app, &s);
        (t, app.log)
    }

    const SCENARIO: &str = r#"screen 1920 1080 dpi 1.5
fixture net=net.json
fixture userdir=user
fixture theme=dark
load-build "Builds/a b.xml"
frames 2
move 650.3 300.7
click LEFTBUTTON
click LEFTBUTTON
click LEFTBUTTON
key-down CTRL ; key-down ` ; key-up ` ; key-up CTRL
type "Hi\té"
press TAB
wheel down 2
key-down SHIFT ; key-down PAUSE ; key-up PAUSE ; key-up SHIFT ; key-up F10
key-down c ; key-up c ; key-down n ; key-up n ; key-down w ; key-up w ; key-down x ; key-up x
frames 2
paste-clipboard "pasted"
snapshot s1
exit
frames 5
"#;

    fn fixture_dir(tag: &str) -> PathBuf {
        let d = tmp(tag);
        std::fs::create_dir_all(d.join("user")).unwrap();
        std::fs::write(
            d.join("net.json"),
            r#"[{"url": "https://example.invalid/a", "body": "hello", "headers": "HTTP/1.1 200 OK\r\n\r\n"}]"#,
        )
        .unwrap();
        d
    }

    /// Headless determinism: two runs of one script give byte-identical traces and callback
    /// logs (virtual clock, scripted net, in-memory fixture writes).
    #[test]
    fn deterministic() {
        let d = fixture_dir("det");
        let (t1, l1) = run(SCENARIO, Some(d.clone()));
        let (t2, l2) = run(SCENARIO, Some(d.clone()));
        assert_eq!(l1, l2);
        let j1 = serde_json::to_string(&t1).unwrap();
        let j2 = serde_json::to_string(&t2).unwrap();
        assert!(!t1.is_empty() && !t1[0].calls.is_empty());
        assert_eq!(j1, j2);
        // the fixture directory is never modified
        assert!(!d.join("user/Settings.xml").exists());
        let _ = std::fs::remove_dir_all(&d);
    }

    /// The callback sequence of the scenario: fixtures before init, DPI-scaled cursor rounding,
    /// double click incl. the reset, swallowed keys, extra OnChar, wheel pairs, net completions
    /// before the next OnFrame, side effects on the frame that made them, `exit` ends the run.
    #[test]
    #[allow(clippy::many_single_char_names)]
    fn sequence() {
        let d = fixture_dir("seq");
        let (traces, log) = run(SCENARIO, Some(d.clone()));
        let user = std::path::absolute(d.join("user")).unwrap();
        let want_write = format!("{}/Settings.xml", user.display());
        let f = |t: i64, cur: (i32, i32)| {
            format!("frame t={t} cur=({},{}) vs=(1280,720)", cur.0, cur.1)
        };
        let s = |v: &[&str]| v.iter().map(|x| (*x).to_owned()).collect::<Vec<_>>();
        let mut want = s(&[
            "step Fixture { key: \"theme\", value: \"dark\" }",
            "init t=0",
            "step LoadBuild(\"Builds/a b.xml\")",
        ]);
        want.push(f(0, (0, 0)));
        want.push(f(16, (0, 0)));
        // move 650.3 300.7 at dpi 1.5: physical lround -> (975, 451); GetCursorPos =
        // lround(975 / 1.5, 451 / 1.5) = (650, 301)
        let c = (650, 301);
        for (i, dbl) in [false, true, false].into_iter().enumerate() {
            let t = 32 + 48 * i as i64;
            want.push(f(t, c));
            want.push(format!("down LEFTBUTTON dbl={dbl}"));
            want.push(f(t + 16, c));
            want.push("up LEFTBUTTON".into());
            want.push(f(t + 32, c));
        }
        want.extend(s(&[
            "down CTRL dbl=false",
            // the Ctrl+` KeyDown (SG console toggle) is swallowed, its KeyUp is not
            "up `",
            "up CTRL",
            "char \"H\"",
            "char \"i\"",
            "down TAB dbl=false",
            "char \"\\t\"",
            "up TAB",
            "char \"?\"",
            "down TAB dbl=false",
            "char \"\\t\"",
        ]));
        want.push(f(176, c));
        want.extend(s(&[
            "up TAB",
            "down WHEELDOWN dbl=false",
            "up WHEELDOWN",
            "down WHEELDOWN dbl=false",
            "up WHEELDOWN",
            "down SHIFT dbl=false",
            "down PAUSE dbl=false",
            // KeyUp(PAUSE) with Shift held and KeyUp(F10) are swallowed
            "up SHIFT",
            "down c dbl=false",
            "up c",
            "down n dbl=false",
            "request 1",
            "up n",
            "down w dbl=false",
            "write ok=true",
            "up w",
            "down x dbl=false",
            "error boom 1",
            "up x",
            "net 1 body=Some(\"hello\") err=None t=192",
        ]));
        want.push(f(192, c));
        want.push(f(208, c));
        want.extend(s(&["step Snapshot(\"s1\")", "can_exit", "exit"]));
        assert_eq!(log, want, "{log:#?}");
        // 2 + 9 + 1 + 2 frames (nothing after `exit`), each with its screen
        assert_eq!(traces.len(), 14);
        assert!(traces
            .iter()
            .all(|t| t.screen.map(|s| (s.fb_w, s.fb_h, s.dpi_scale)) == Some((1920, 1080, 1.5))));
        // OnInit's title change lands on the first frame; Copy and the write on frame 12
        assert_eq!(
            traces[0].effects,
            vec![SideEffect::WindowTitle("Recorder".into())]
        );
        assert_eq!(
            traces[12].effects,
            vec![
                SideEffect::Copy("copied".into()),
                SideEffect::FileWrite {
                    path: want_write,
                    xxh3: xxhash_rust::xxh3::xxh3_64(b"<x/>"),
                },
            ]
        );
        let _ = std::fs::remove_dir_all(&d);
    }

    /// `Exit()` from Lua ends the run after `OnExit`; the virtual clipboard feeds `Paste()`.
    #[test]
    fn exit_and_clipboard() {
        let mut app = Recorder::new(parts_without_assets());
        let s = Script::parse("paste-clipboard \"abc\"\nframes 1\nkey-down q\nframes 3").unwrap();
        let t = run_headless(&mut app, &s);
        assert_eq!(app.sys.platform.paste().as_deref(), Some("abc"));
        assert_eq!(t.len(), 1);
        assert_eq!(app.log.last().map(String::as_str), Some("exit"));
        assert_eq!(app.log.iter().filter(|l| *l == "exit").count(), 1);
    }

    /// Headless Fs: a copy-on-write view of the fixture directory.
    #[test]
    fn overlay_fs() {
        let d = tmp("overlay");
        std::fs::create_dir_all(d.join("Builds/Sub")).unwrap();
        std::fs::write(d.join("Builds/a.xml"), b"disk").unwrap();
        let h = HeadlessState::new(d.to_str().unwrap(), "");
        let mut fs = HeadlessFs(h.clone());
        let names = |v: Vec<FileEntry>| v.into_iter().map(|e| e.name).collect::<Vec<_>>();
        assert_eq!(fs.read("Builds/a.xml").unwrap(), b"disk");
        fs.write("Builds/b.xml", b"mem").unwrap();
        assert!(fs.write("Nope/c.xml", b"x").is_err()); // parent missing
        assert_eq!(names(fs.search("Builds/*.xml", false)), ["a.xml", "b.xml"]);
        assert_eq!(names(fs.search("Builds/*", true)), ["Sub"]);
        fs.rename("Builds/a.xml", "Builds/c.xml").unwrap();
        assert!(fs.read("Builds/a.xml").is_err());
        assert_eq!(fs.read("Builds/c.xml").unwrap(), b"disk");
        fs.make_dir("Builds/New").unwrap();
        assert!(fs.make_dir("Builds/New").is_err());
        assert!(fs.remove_dir("Builds", false).is_err());
        fs.rename("Builds", "Moved").unwrap();
        assert_eq!(names(fs.search("Moved/*", false)), ["b.xml", "c.xml"]);
        assert_eq!(names(fs.search("Moved/*", true)), ["New", "Sub"]);
        fs.remove("Moved/b.xml").unwrap();
        fs.remove_dir("Moved", true).unwrap();
        assert!(fs.search("Moved/*", false).is_empty());
        assert!(fs.read("Builds/c.xml").is_err());
        // the disk is untouched
        assert_eq!(std::fs::read(d.join("Builds/a.xml")).unwrap(), b"disk");
        assert!(!d.join("Builds/b.xml").exists());
        assert_eq!(h.effects.borrow().len(), 1); // the successful write only
        let _ = std::fs::remove_dir_all(&d);
    }

    /// The first unused fixture per URL, then the last one again; unknown URLs error.
    #[test]
    fn scripted_net() {
        let h = HeadlessState::new("", "");
        let fx = |url: &str, body: Option<&str>| NetFixture {
            url: url.into(),
            response: HttpResponseFixture {
                body: body.map(|b| b.as_bytes().to_vec()),
                error: None,
                headers: String::new(),
            },
        };
        h.set_net_fixtures(vec![fx("u", Some("1")), fx("u", Some("2")), fx("e", None)]);
        let mut n = ScriptedNet(h);
        for u in ["u", "u", "u", "e", "zz"] {
            n.download_page(HttpRequest {
                url: u.into(),
                headers: vec![],
                body: None,
                user_agent: None,
            });
        }
        assert!(n.busy());
        let got: Vec<_> = n
            .poll()
            .into_iter()
            .map(|(id, r)| (id, r.body, r.error))
            .collect();
        assert!(!n.busy());
        assert_eq!(
            got,
            vec![
                (1, Some(b"1".to_vec()), None),
                (2, Some(b"2".to_vec()), None),
                (3, Some(b"2".to_vec()), None),
                (4, None, None),
                (5, Some(vec![]), Some("No net fixture for zz".into())),
            ]
        );
    }
}
