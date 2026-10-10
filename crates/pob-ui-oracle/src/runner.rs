//! Runs one scenario or gallery on both sides and collects their traces (ui doc §11.2, §11.3).
//! Owner: U8.
//!
//! Both sides are driven by the same [`drive`] loop through `pob_host::App`, so the input and
//! frame sequence is identical by construction: the Lua side is [`LuaApp`] (real PoB under
//! LuaJIT, [`crate::boot`]), the Rust side is `pob_app::PobApp` for scenarios and the gallery's
//! `src/gallery/<name>.rs` app for galleries.
//!
//! `drive` implements the `.pobscript` steps as documented in `pob_host::script` (U7):
//!
//! | step | effect |
//! |---|---|
//! | `screen W H dpi S` | framebuffer `W×H`, OS scale `S` for the following frames |
//! | `fixture userdir=DIR` | user dir (relative to `fixtures/`, default `user1`); applied before boot |
//! | `fixture net=FILE.json` | scripted `launch:DownloadPage` responses (patch 8; CCR-0028 item 7) |
//! | `load-build "PATH"` | `main:SetMode("BUILD", false, <file stem>, <xml>)`, PATH relative to the user dir |
//! | `frames N` | N frames |
//! | `move X Y` | cursor to virtual `(X, Y)` (physical `floor(v * scale)`), then a frame |
//! | `click KEY` | KeyDown + frame, KeyUp + frame (double click: same button within 500 ms and ±5 px) |
//! | `key-down KEY` / `key-up KEY` | the callback only (events collect until the next frame) |
//! | `type "TEXT"` | per character `OnChar`; `\t \n \r \b \x1b` as KeyDown + Char + KeyUp of TAB/RETURN/BACK/ESCAPE |
//! | `press KEY` | KeyDown (+ SG's extra OnChar for BACK/TAB/RETURN/ESCAPE) + frame + KeyUp |
//! | `wheel up\|down N` | N × (KeyDown + KeyUp of WHEELUP/WHEELDOWN) |
//! | `paste-clipboard "TEXT"` | sets the virtual clipboard |
//! | `snapshot NAME` | the build XML of both sides (T4) |
//!
//! Host rules from SG kept by the driver (ui doc §2): `KeyUp(F10)` and `KeyUp(PAUSE)` with Shift held
//! are not delivered; BACK/TAB/RETURN/ESCAPE KeyDown is followed by an OnChar. The virtual clock is
//! 0 during `OnInit` and the first frame and advances 16 ms after every frame (patch 1). Before
//! `OnInit` one untraced empty frame sets the screen size (SG knows it before Lua's OnInit).
//! The cursor starts outside the window at physical `(-1, -1)`.
//!
//! `pob_host::run_headless` and `Script::parse` are U7's and still stubs at this unit's base;
//! [`drive`] is this crate's driver for both sides, and [`parse_script`] falls back to a local
//! parser of the documented grammar while `Script::parse` is unimplemented (U7-PENDING).

use std::path::{Path, PathBuf};
use std::rc::Rc;

use pob_host::script::{Script, Step};
use pob_host::{App, PlatformParts};
use pob_sg::trace::{FrameTrace, TraceRecorder};
use pob_sg::{Key, ScreenInfo, Sg, Sys};

use crate::boot::{self, BootOptions, Entry, LuaPob, OracleHandles};
use crate::markers::AuditHit;

/// Both sides' per-frame traces for one run.
pub struct RunResult {
    pub lua: Vec<FrameTrace>,
    pub rust: Vec<FrameTrace>,
    /// Why the Rust side produced no (or a partial) trace, e.g. a unit it needs is not ported yet.
    pub rust_error: Option<String>,
    /// `snapshot NAME` results: (name, build XML) per side.
    pub lua_snapshots: Vec<(String, String)>,
    pub rust_snapshots: Vec<(String, String)>,
    /// Audit mode only: hosts whose native draw order differs from the canonical one.
    pub audit: Vec<AuditHit>,
    /// Whether the Rust side ran (false with `RunOptions::lua_only`): only then is the run a
    /// comparison.
    pub compared: bool,
}

/// What to run.
pub enum Target {
    Scenario(Script),
    Gallery(String),
}

/// Run options beyond the screen.
#[derive(Clone, Debug)]
pub struct RunOptions {
    pub screen: ScreenInfo,
    /// Audit mode (Lua side only, patch 6 disabled; §11.2).
    pub audit: bool,
    /// Skip the Rust side.
    pub lua_only: bool,
}

/// A screen of `w×h` framebuffer pixels at OS scale `dpi`.
pub fn screen(w: i32, h: i32, dpi: f32) -> ScreenInfo {
    ScreenInfo {
        fb_w: w,
        fb_h: h,
        win_h: h,
        dpi_scale: dpi,
    }
}

/// The two acceptance screens (ui doc §11.3): 1280×800 @1.0 and 1920×1080 @1.5.
pub const ACCEPTANCE_SCREENS: [(i32, i32, f32); 2] = [(1280, 800, 1.0), (1920, 1080, 1.5)];

/// This crate's directory (`fixtures/`, `scenarios/`, `lua/`).
pub fn crate_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

/// Run `target` at `screen` (1280x800@1.0 and 1920x1080@1.5 for acceptance).
pub fn run(target: &Target, screen: pob_sg::ScreenInfo) -> anyhow::Result<RunResult> {
    run_with(
        target,
        &RunOptions {
            screen,
            audit: false,
            lua_only: false,
        },
    )
}

/// The app a side runs, with the scenario operations the `App` trait does not cover.
pub trait ScenarioApp: App {
    /// `load-build`: `main:SetMode("BUILD", false, name, xml)`.
    fn load_build(&mut self, name: &str, xml: &str) -> anyhow::Result<()>;
    /// `snapshot`: the current build's XML (`build:SaveDB`), if a build is open.
    fn snapshot(&mut self) -> anyhow::Result<Option<String>>;
    /// After each frame's `on_frame` (audit bookkeeping).
    fn after_frame(&mut self) -> anyhow::Result<()> {
        Ok(())
    }
    /// A callback that failed since the last call (Lua error escaping `launch`'s own PCall).
    fn take_error(&mut self) -> Option<String> {
        None
    }
}

// ---------------------------------------------------------------------------------------------
// The Lua side
// ---------------------------------------------------------------------------------------------

/// Real PoB under LuaJIT as a `pob_host::App`.
///
/// `App::sg()`/`sys()` hand out `&mut Sg`, while the Lua functions reach the `Sg` through
/// `Rc<RefCell<..>>` ([`LuaPob`]). Between callbacks the real `Sg`/`Sys` are parked here (the
/// cells hold inert placeholders); each callback swaps them in for its duration (the "lend"
/// pattern of ui doc §6.5).
pub struct LuaApp {
    pub pob: LuaPob,
    sg: Sg,
    sys: Sys,
    error: Option<String>,
}

fn placeholder_parts() -> (Sg, Sys) {
    struct NullLoader;
    impl pob_sg::ImageLoader for NullLoader {
        fn request(
            &mut self,
            id: pob_sg::TexId,
            _path: &str,
            _flags: pob_sg::ImageFlags,
            _sync: bool,
            _priority: i32,
        ) -> Option<pob_sg::LoadEvent> {
            Some(pob_sg::LoadEvent::Failed(id))
        }
        fn read_size(&mut self, _id: pob_sg::TexId, _path: &str) -> Option<(u32, u32)> {
            None
        }
        fn set_priority(&mut self, _id: pob_sg::TexId, _priority: i32) {}
        fn poll(&mut self) -> Vec<pob_sg::LoadEvent> {
            Vec::new()
        }
    }
    let sg = Sg::new(
        Rc::new(pob_sg::FontSet::empty()),
        pob_sg::ImageRegistry::new(Box::new(NullLoader)),
    );
    let handles = OracleHandles::default();
    let sys = Sys::new(
        Box::new(boot::OraclePlatform {
            clock: handles.clock,
            clipboard: handles.clipboard,
            script_path: String::new(),
            runtime_path: String::new(),
            user_path: String::new(),
            echo_console: false,
        }),
        Box::new(boot::OracleFs {
            root: PathBuf::new(),
        }),
        Box::new(boot::NoNet),
    );
    (sg, sys)
}

impl LuaApp {
    /// Park the booted PoB's `Sg`/`Sys`.
    pub fn new(pob: LuaPob) -> LuaApp {
        let (mut sg, mut sys) = placeholder_parts();
        std::mem::swap(&mut sg, &mut *pob.sg.borrow_mut());
        std::mem::swap(&mut sys, &mut *pob.sys.borrow_mut());
        LuaApp {
            pob,
            sg,
            sys,
            error: None,
        }
    }

    fn swap(&mut self) {
        std::mem::swap(&mut self.sg, &mut *self.pob.sg.borrow_mut());
        std::mem::swap(&mut self.sys, &mut *self.pob.sys.borrow_mut());
    }

    /// Run `f` with the real `Sg`/`Sys` inside the VM's cells.
    pub fn with_vm<R>(&mut self, f: impl FnOnce(&LuaPob) -> mlua::Result<R>) -> mlua::Result<R> {
        self.swap();
        let r = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| f(&self.pob)));
        self.swap();
        match r {
            Ok(r) => r,
            Err(p) => std::panic::resume_unwind(p),
        }
    }

    fn callback(&mut self, name: &str, args: impl mlua::IntoLuaMulti) {
        let r = self.with_vm(|pob| pob.run_callback(name, args));
        if let Err(e) = r {
            self.error.get_or_insert_with(|| format!("{name}: {e}"));
        }
    }
}

impl App for LuaApp {
    fn sg(&mut self) -> &mut Sg {
        &mut self.sg
    }
    fn sys(&mut self) -> &mut Sys {
        &mut self.sys
    }
    fn on_init(&mut self) {
        self.callback("OnInit", ());
    }
    fn on_frame(&mut self) {
        let r = self.with_vm(|pob| {
            crate::sg_bridge::deliver_subscripts(&pob.lua)?;
            pob.run_callback("OnFrame", ())
        });
        if let Err(e) = r {
            self.error.get_or_insert_with(|| format!("OnFrame: {e}"));
        }
    }
    fn on_key_down(&mut self, key: Key, double_click: bool) {
        self.callback("OnKeyDown", (key.name().into_owned(), double_click));
    }
    fn on_key_up(&mut self, key: Key) {
        self.callback("OnKeyUp", key.name().into_owned());
    }
    fn on_char(&mut self, ch: Key) {
        self.callback("OnChar", ch.name().into_owned());
    }
    fn can_exit(&mut self) -> bool {
        let r = self.with_vm(|pob| {
            let f: mlua::Function = pob.lua.globals().get("runCallback")?;
            f.call::<Option<bool>>("CanExit")
        });
        r.ok().flatten().unwrap_or(true)
    }
    fn on_exit(&mut self) {
        self.callback("OnExit", ());
    }
    fn on_lua_error(&mut self, msg: String) {
        self.error.get_or_insert(msg);
    }
}

impl ScenarioApp for LuaApp {
    fn load_build(&mut self, name: &str, xml: &str) -> anyhow::Result<()> {
        let (name, xml) = (name.to_owned(), xml.to_owned());
        self.with_vm(|pob| {
            let main: mlua::Table = pob.lua.globals().get("main")?;
            let set_mode: mlua::Function = main.get("SetMode")?;
            set_mode.call::<()>((main, "BUILD", false, name, pob.lua.create_string(xml)?))
        })
        .map_err(|e| anyhow::anyhow!("load-build: {e}"))
    }
    fn snapshot(&mut self) -> anyhow::Result<Option<String>> {
        self.with_vm(|pob| {
            pob.eval::<Option<String>>(
                "if main.mode == 'BUILD' then return main.modes.BUILD:SaveDB('snapshot') end",
            )
        })
        .map_err(|e| anyhow::anyhow!("snapshot: {e}"))
    }
    fn after_frame(&mut self) -> anyhow::Result<()> {
        let audit = self
            .with_vm(|pob| pob.lua.globals().get::<Option<bool>>("__oracleAudit"))
            .map_err(|e| anyhow::anyhow!("{e}"))?
            .unwrap_or(false);
        if audit {
            self.with_vm(|pob| crate::markers::audit_end_frame(&pob.lua))
                .map_err(|e| anyhow::anyhow!("audit: {e}"))?;
        }
        Ok(())
    }
    fn take_error(&mut self) -> Option<String> {
        self.error.take()
    }
}

// ---------------------------------------------------------------------------------------------
// The Rust side
// ---------------------------------------------------------------------------------------------

/// `pob_app::PobApp` (scenarios). `load-build`/`snapshot` need pob-app's mode API (U16).
pub struct RustPobApp(pub pob_app::PobApp);

impl App for RustPobApp {
    fn sg(&mut self) -> &mut Sg {
        self.0.sg()
    }
    fn sys(&mut self) -> &mut Sys {
        self.0.sys()
    }
    fn on_init(&mut self) {
        self.0.on_init();
    }
    fn on_frame(&mut self) {
        self.0.on_frame();
    }
    fn on_key_down(&mut self, key: Key, double_click: bool) {
        self.0.on_key_down(key, double_click);
    }
    fn on_key_up(&mut self, key: Key) {
        self.0.on_key_up(key);
    }
    fn on_char(&mut self, ch: Key) {
        self.0.on_char(ch);
    }
    fn on_lua_error(&mut self, msg: String) {
        self.0.on_lua_error(msg);
    }
}

impl ScenarioApp for RustPobApp {
    fn load_build(&mut self, _name: &str, _xml: &str) -> anyhow::Result<()> {
        anyhow::bail!("load-build on the Rust side needs pob-app's main:SetMode (U16)")
    }
    fn snapshot(&mut self) -> anyhow::Result<Option<String>> {
        anyhow::bail!("snapshot on the Rust side needs pob-app's build:SaveDB (U16/calc:BM-A)")
    }
}

/// A gallery's Rust app (`src/gallery/<name>.rs`).
pub struct GalleryApp(pub Box<dyn App>);

impl App for GalleryApp {
    fn sg(&mut self) -> &mut Sg {
        self.0.sg()
    }
    fn sys(&mut self) -> &mut Sys {
        self.0.sys()
    }
    fn on_init(&mut self) {
        self.0.on_init();
    }
    fn on_frame(&mut self) {
        self.0.on_frame();
    }
    fn on_key_down(&mut self, key: Key, double_click: bool) {
        self.0.on_key_down(key, double_click);
    }
    fn on_key_up(&mut self, key: Key) {
        self.0.on_key_up(key);
    }
    fn on_char(&mut self, ch: Key) {
        self.0.on_char(ch);
    }
    fn on_lua_error(&mut self, msg: String) {
        self.0.on_lua_error(msg);
    }
}

impl ScenarioApp for GalleryApp {
    fn load_build(&mut self, _name: &str, _xml: &str) -> anyhow::Result<()> {
        anyhow::bail!("load-build is not available in a gallery")
    }
    fn snapshot(&mut self) -> anyhow::Result<Option<String>> {
        Ok(None)
    }
}

// ---------------------------------------------------------------------------------------------
// Scripts
// ---------------------------------------------------------------------------------------------

/// Runs `f`, a call into a unit this crate depends on that may still be a `todo!()` stub at this
/// unit's base (U1 `trace::normalize`/`diff`, U6 `raster`, U7 `Script::parse`): `Some(result)` if
/// it is implemented, `None` if it panicked with `todo!()` (without printing the panic). Any other
/// panic propagates. The probes serialise on a lock because they swap the process panic hook.
pub fn if_implemented<R>(f: impl FnOnce() -> R) -> Option<R> {
    static LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());
    let _g = LOCK
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let hook = std::panic::take_hook();
    std::panic::set_hook(Box::new(|_| {}));
    let r = std::panic::catch_unwind(std::panic::AssertUnwindSafe(f));
    std::panic::set_hook(hook);
    match r {
        Ok(v) => Some(v),
        Err(p) if pob_host::panic_message(&*p).contains("not yet implemented") => None,
        Err(p) => std::panic::resume_unwind(p),
    }
}

/// Parse a `.pobscript`: `pob_host::script::Script::parse` (U7), or this crate's parser of the
/// same documented grammar while U7's is unimplemented (U7-PENDING).
pub fn parse_script(text: &str) -> Result<Script, String> {
    static U7: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    let u7 = *U7.get_or_init(|| if_implemented(|| Script::parse("frames 1")).is_some());
    if u7 {
        Script::parse(text)
    } else {
        parse_script_local(text)
    }
}

fn tokenize(line: &str) -> Result<Vec<Vec<String>>, String> {
    let mut steps = vec![Vec::new()];
    let mut cur = String::new();
    let mut in_tok = false;
    let mut chars = line.chars().peekable();
    while let Some(c) = chars.next() {
        match c {
            '"' => {
                let mut s = String::new();
                loop {
                    match chars.next() {
                        None => return Err(format!("unterminated string in {line:?}")),
                        Some('"') => break,
                        Some('\\') => match chars.next() {
                            Some('n') => s.push('\n'),
                            Some('t') => s.push('\t'),
                            Some('r') => s.push('\r'),
                            Some('b') => s.push('\u{8}'),
                            Some('e') => s.push('\u{1b}'),
                            Some(o) => s.push(o),
                            None => return Err(format!("dangling escape in {line:?}")),
                        },
                        Some(o) => s.push(o),
                    }
                }
                cur.push_str(&s);
                in_tok = true;
            }
            '#' => break,
            ';' => {
                if in_tok {
                    steps.last_mut().unwrap().push(std::mem::take(&mut cur));
                    in_tok = false;
                }
                steps.push(Vec::new());
            }
            c if c.is_whitespace() => {
                if in_tok {
                    steps.last_mut().unwrap().push(std::mem::take(&mut cur));
                    in_tok = false;
                }
            }
            c => {
                cur.push(c);
                in_tok = true;
            }
        }
    }
    if in_tok {
        steps.last_mut().unwrap().push(cur);
    }
    Ok(steps.into_iter().filter(|s| !s.is_empty()).collect())
}

fn parse_key(s: &str) -> Result<Key, String> {
    Key::from_name(s)
        .or_else(|| (s.len() == 1 && s.is_ascii()).then(|| Key::Char(s.as_bytes()[0])))
        .ok_or_else(|| format!("unknown key {s:?}"))
}

/// The local `.pobscript` parser (see [`parse_script`]).
pub fn parse_script_local(text: &str) -> Result<Script, String> {
    let mut steps = Vec::new();
    for (ln, line) in text.lines().enumerate() {
        for toks in tokenize(line).map_err(|e| format!("line {}: {e}", ln + 1))? {
            let err = |m: &str| format!("line {}: {m}: {line:?}", ln + 1);
            let num = |i: usize| -> Result<f64, String> {
                toks.get(i)
                    .and_then(|t| t.parse::<f64>().ok())
                    .ok_or_else(|| err("expected a number"))
            };
            let arg = |i: usize| -> Result<String, String> {
                toks.get(i).cloned().ok_or_else(|| err("missing argument"))
            };
            let step = match toks[0].as_str() {
                "screen" => {
                    if toks.get(3).map(String::as_str) != Some("dpi") {
                        return Err(err("expected `screen W H dpi S`"));
                    }
                    Step::Screen {
                        w: num(1)? as i32,
                        h: num(2)? as i32,
                        dpi: num(4)? as f32,
                    }
                }
                "fixture" => {
                    let kv = arg(1)?;
                    let (k, v) = kv
                        .split_once('=')
                        .ok_or_else(|| err("expected key=value"))?;
                    Step::Fixture {
                        key: k.to_owned(),
                        value: v.to_owned(),
                    }
                }
                "load-build" => Step::LoadBuild(arg(1)?),
                "frames" => Step::Frames(num(1)? as u32),
                "move" => Step::Move(num(1)?, num(2)?),
                "click" => Step::Click(parse_key(&arg(1)?)?),
                "key-down" => Step::KeyDown(parse_key(&arg(1)?)?),
                "key-up" => Step::KeyUp(parse_key(&arg(1)?)?),
                "type" => Step::Type(arg(1)?),
                "press" => Step::Press(parse_key(&arg(1)?)?),
                "wheel" => Step::Wheel {
                    up: match arg(1)?.as_str() {
                        "up" => true,
                        "down" => false,
                        _ => return Err(err("expected `wheel up|down N`")),
                    },
                    count: num(2)? as u32,
                },
                "paste-clipboard" => Step::PasteClipboard(arg(1)?),
                "snapshot" => Step::Snapshot(arg(1)?),
                other => return Err(err(&format!("unknown step {other:?}"))),
            };
            steps.push(step);
        }
    }
    Ok(Script { steps })
}

// ---------------------------------------------------------------------------------------------
// The driver
// ---------------------------------------------------------------------------------------------

/// Driver state shared by both sides' runs.
pub struct Driver {
    pub screen: ScreenInfo,
    pub handles: OracleHandles,
    /// The user dir (`GetUserPath()`), for `load-build`.
    pub user_dir: PathBuf,
    cursor_phys: (i32, i32),
    last_click: Option<(Key, (i32, i32), i64)>,
    pub snapshots: Vec<(String, String)>,
}

impl Driver {
    pub fn new(screen: ScreenInfo, handles: OracleHandles, user_dir: PathBuf) -> Driver {
        Driver {
            screen,
            handles,
            user_dir,
            cursor_phys: (-1, -1),
            last_click: None,
            snapshots: Vec::new(),
        }
    }

    fn check<A: ScenarioApp + ?Sized>(app: &mut A) -> anyhow::Result<()> {
        match app.take_error() {
            Some(e) => anyhow::bail!("{e}"),
            None => Ok(()),
        }
    }

    /// One frame (ui_main.cpp:380-445): BeginFrame, cursor snapshot, OnFrame, EndFrame.
    pub fn frame<A: ScenarioApp + ?Sized>(&mut self, app: &mut A) -> anyhow::Result<()> {
        app.sg().begin_frame(self.screen);
        let scale = app.sg().scale();
        app.sys().begin_frame(self.cursor_phys, scale);
        app.on_frame();
        Self::check(app)?;
        app.after_frame()?;
        let _ = app.sg().end_frame();
        self.handles.clock.set(self.handles.clock.get() + 16);
        Ok(())
    }

    fn key_down<A: ScenarioApp + ?Sized>(&mut self, app: &mut A, key: Key) -> anyhow::Result<()> {
        let mut dbl = false;
        if matches!(
            key,
            Key::LeftButton | Key::MiddleButton | Key::RightButton | Key::Mouse4 | Key::Mouse5
        ) {
            // sys_video.cpp:568-604: same button within 500 ms and ±5 px; a detected double
            // click resets the memory.
            let now = self.handles.clock.get();
            let pos = self.cursor_phys;
            if let Some((k, p, t)) = self.last_click {
                if k == key && now - t < 500 && (p.0 - pos.0).abs() <= 5 && (p.1 - pos.1).abs() <= 5
                {
                    dbl = true;
                }
            }
            self.last_click = if dbl { None } else { Some((key, pos, now)) };
        }
        app.sys().set_key(key, true);
        app.on_key_down(key, dbl);
        Self::check(app)?;
        // sys_main.cpp:343-357: BACK/TAB/RETURN/ESCAPE also send OnChar.
        let extra = match key {
            Key::Back => Some(b'\x08'),
            Key::Tab => Some(b'\t'),
            Key::Return => Some(b'\r'),
            Key::Escape => Some(b'\x1b'),
            _ => None,
        };
        if let Some(c) = extra {
            app.on_char(Key::Char(c));
            Self::check(app)?;
        }
        Ok(())
    }

    #[allow(clippy::unused_self)]
    fn key_up<A: ScenarioApp + ?Sized>(&mut self, app: &mut A, key: Key) -> anyhow::Result<()> {
        app.sys().set_key(key, false);
        // ui_main.cpp:530-541: KeyUp(F10) and Shift+KeyUp(PAUSE) are swallowed.
        let shift = app.sys().is_key_down(Key::Shift);
        if key == Key::F(10) || (key == Key::Pause && shift) {
            return Ok(());
        }
        app.on_key_up(key);
        Self::check(app)
    }

    #[allow(clippy::unused_self)]
    fn char<A: ScenarioApp + ?Sized>(&mut self, app: &mut A, ch: Key) -> anyhow::Result<()> {
        app.on_char(ch);
        Self::check(app)
    }

    /// Run one step.
    pub fn step<A: ScenarioApp + ?Sized>(
        &mut self,
        app: &mut A,
        step: &Step,
    ) -> anyhow::Result<()> {
        match step {
            Step::Screen { w, h, dpi } => self.screen = screen(*w, *h, *dpi),
            Step::Fixture { .. } => {} // applied before boot (run_with)
            Step::LoadBuild(path) => {
                let file = self.user_dir.join("Path of Building").join(path);
                let xml = std::fs::read_to_string(&file)
                    .map_err(|e| anyhow::anyhow!("load-build {}: {e}", file.display()))?;
                let name = Path::new(path)
                    .file_stem()
                    .map(|s| s.to_string_lossy().into_owned())
                    .unwrap_or_default();
                app.load_build(&name, &xml)?;
                Self::check(app)?;
            }
            Step::Frames(n) => {
                for _ in 0..*n {
                    self.frame(app)?;
                }
            }
            Step::Move(x, y) => {
                let s = f64::from(self.screen.dpi_scale);
                self.cursor_phys = ((x * s).floor() as i32, (y * s).floor() as i32);
                self.frame(app)?;
            }
            Step::Click(k) => {
                self.key_down(app, *k)?;
                self.frame(app)?;
                self.key_up(app, *k)?;
                self.frame(app)?;
            }
            Step::KeyDown(k) => self.key_down(app, *k)?,
            Step::KeyUp(k) => self.key_up(app, *k)?,
            Step::Type(text) => {
                for c in text.chars() {
                    let named = match c {
                        '\t' => Some(Key::Tab),
                        '\n' | '\r' => Some(Key::Return),
                        '\u{8}' => Some(Key::Back),
                        '\u{1b}' => Some(Key::Escape),
                        _ => None,
                    };
                    if let Some(k) = named {
                        self.key_down(app, k)?; // includes the OnChar
                        self.key_up(app, k)?;
                    } else if c.is_ascii() {
                        self.char(app, Key::Char(c as u8))?;
                    } else {
                        self.char(app, Key::Unknown)?;
                    }
                }
            }
            Step::Press(k) => {
                self.key_down(app, *k)?;
                self.frame(app)?;
                self.key_up(app, *k)?;
            }
            Step::Wheel { up, count } => {
                let k = if *up { Key::WheelUp } else { Key::WheelDown };
                for _ in 0..*count {
                    self.key_down(app, k)?;
                    self.key_up(app, k)?;
                }
            }
            Step::PasteClipboard(t) => *self.handles.clipboard.borrow_mut() = Some(t.clone()),
            Step::Snapshot(name) => {
                if let Some(xml) = app.snapshot()? {
                    self.snapshots.push((name.clone(), xml));
                }
            }
        }
        Ok(())
    }
}

/// Drive `app` through `script`: prime the screen, `OnInit`, then the steps, with tracing on
/// from the first traced frame. Returns the frames recorded (also on error: the frames so far).
pub fn drive<A: ScenarioApp + ?Sized>(
    app: &mut A,
    script: &Script,
    d: &mut Driver,
) -> (Vec<FrameTrace>, anyhow::Result<()>) {
    // SG knows the window size before OnInit: one untraced empty frame.
    app.sg().trace = None;
    app.sg().begin_frame(d.screen);
    let scale = app.sg().scale();
    app.sys().begin_frame(d.cursor_phys, scale);
    let _ = app.sg().end_frame();
    app.sg().trace = Some(TraceRecorder::new());
    let r = (|| {
        app.on_init();
        Driver::check(app)?;
        for step in &script.steps {
            d.step(app, step)?;
        }
        Ok(())
    })();
    let frames = app.sg().trace.take().map(|t| t.frames).unwrap_or_default();
    (frames, r)
}

// ---------------------------------------------------------------------------------------------
// Runs
// ---------------------------------------------------------------------------------------------

/// A fresh copy of a fixture user dir: `GetUserPath()` returns `path`, PoB uses
/// `path/Path of Building/` (Main.lua:94-98). Removed on drop.
pub struct UserDir {
    pub path: PathBuf,
    fixture: PathBuf,
}

static RUN_COUNTER: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);

/// The modification time of every file and directory of a run's user dir (the virtual epoch of
/// `lua/determinism.lua`), so `GetFileModifiedTime` (BuildListHelpers.lua:76, "Sort by Last
/// Edited") does not depend on when the fixture was checked out or copied.
pub const FIXTURE_MTIME_UNIX: u64 = 1_700_000_000;

fn set_fixed_mtime(p: &Path) -> std::io::Result<()> {
    let t = std::time::UNIX_EPOCH + std::time::Duration::from_secs(FIXTURE_MTIME_UNIX);
    std::fs::File::open(p)?.set_modified(t)
}

fn copy_dir(from: &Path, to: &Path) -> std::io::Result<()> {
    std::fs::create_dir_all(to)?;
    let mut entries: Vec<_> = std::fs::read_dir(from)?.collect::<Result<_, _>>()?;
    entries.sort_by_key(std::fs::DirEntry::file_name);
    for e in entries {
        let p = e.path();
        let dest = to.join(e.file_name());
        if e.file_type()?.is_dir() {
            copy_dir(&p, &dest)?;
        } else {
            std::fs::copy(&p, &dest)?;
            set_fixed_mtime(&dest)?;
        }
    }
    // After the contents: creating entries updates the directory's mtime.
    set_fixed_mtime(to)
}

impl UserDir {
    /// A new run dir with a copy of `fixture`.
    pub fn new(fixture: &Path) -> anyhow::Result<UserDir> {
        let n = RUN_COUNTER.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let path = std::env::temp_dir()
            .join("pob-ui-oracle")
            .join(format!("{}-{n}", std::process::id()));
        let d = UserDir {
            path,
            fixture: fixture.to_path_buf(),
        };
        d.reset()?;
        Ok(d)
    }

    /// Restore the pristine fixture (before each side).
    pub fn reset(&self) -> anyhow::Result<()> {
        let _ = std::fs::remove_dir_all(&self.path);
        copy_dir(&self.fixture, &self.path.join("Path of Building")).map_err(|e| {
            anyhow::anyhow!(
                "copying fixture {} to {}: {e}",
                self.fixture.display(),
                self.path.display()
            )
        })
    }
}

impl Drop for UserDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.path);
    }
}

/// The fixture user dir a script selects (`fixture userdir=..`, default `fixtures/user1`).
pub fn fixture_dir(script: &Script) -> PathBuf {
    let mut dir = crate_dir().join("fixtures/user1");
    for s in &script.steps {
        if let Step::Fixture { key, value } = s {
            if key == "userdir" {
                let p = Path::new(value);
                dir = if p.is_absolute() {
                    p.to_path_buf()
                } else if value.starts_with("fixtures/") {
                    crate_dir().join(p)
                } else {
                    crate_dir().join("fixtures").join(p)
                };
            }
        }
    }
    dir
}

/// The scripted network responses a script selects (`fixture net=FILE.json`, relative to
/// `fixtures/`; patch 8): `{"<url>": {"body": "...", "error": "..." | null, "header": "..."}}`.
/// Without one, every `launch:DownloadPage` fails with "no scripted response".
pub fn net_fixture(
    script: &Script,
) -> anyhow::Result<std::collections::HashMap<String, crate::sg_bridge::NetResponse>> {
    #[derive(serde::Deserialize)]
    struct Resp {
        #[serde(default)]
        body: Option<String>,
        #[serde(default)]
        error: Option<String>,
        #[serde(default)]
        header: String,
    }
    let mut out = std::collections::HashMap::new();
    for s in &script.steps {
        if let Step::Fixture { key, value } = s {
            if key == "net" {
                let p = crate_dir().join("fixtures").join(value);
                let text = std::fs::read_to_string(&p)
                    .map_err(|e| anyhow::anyhow!("fixture net={value}: {}: {e}", p.display()))?;
                let map: std::collections::BTreeMap<String, Resp> = serde_json::from_str(&text)
                    .map_err(|e| anyhow::anyhow!("fixture net={value}: {e}"))?;
                for (url, r) in map {
                    out.insert(
                        url,
                        crate::sg_bridge::NetResponse {
                            body: r.body.map(String::into_bytes),
                            error: r.error,
                            header: r.header,
                        },
                    );
                }
            } else if key != "userdir" {
                anyhow::bail!("unknown fixture key {key:?} (userdir, net)");
            }
        }
    }
    Ok(out)
}

/// The script that drives a gallery: `lua/gallery/<name>.pobscript` if present, else `frames 3`.
pub fn gallery_script(name: &str) -> anyhow::Result<Script> {
    let p = crate_dir()
        .join("lua/gallery")
        .join(format!("{name}.pobscript"));
    if p.is_file() {
        let text = std::fs::read_to_string(&p)?;
        parse_script(&text).map_err(|e| anyhow::anyhow!("{}: {e}", p.display()))
    } else {
        Ok(Script {
            steps: vec![Step::Frames(3)],
        })
    }
}

/// Build the Lua side: boot PoB (or the gallery mini-launch) on oracle parts.
pub fn lua_side(
    pob_root: &Path,
    user: &UserDir,
    handles: &OracleHandles,
    entry: Entry,
    audit: bool,
) -> anyhow::Result<LuaApp> {
    let (sg, sys) = boot::oracle_parts(pob_root, &user.path, handles)?;
    let pob = boot::boot_with(
        &pob_root.join("src").to_string_lossy(),
        sg,
        sys,
        &BootOptions {
            entry,
            audit,
            live: false,
        },
    )?;
    Ok(LuaApp::new(pob))
}

/// Run `target` with `opts`.
pub fn run_with(target: &Target, opts: &RunOptions) -> anyhow::Result<RunResult> {
    let pob_root = boot::locate_pob_root()
        .ok_or_else(|| anyhow::anyhow!("PoB not found (set POB_ROOT to a .PathOfBuilding dir)"))?;
    let (script, entry) = match target {
        Target::Scenario(s) => (s.clone(), Entry::Launch),
        Target::Gallery(name) => {
            let lua = crate_dir().join("lua/gallery").join(format!("{name}.lua"));
            anyhow::ensure!(
                lua.is_file(),
                "gallery {name}: {} is missing",
                lua.display()
            );
            (gallery_script(name)?, Entry::Gallery(lua))
        }
    };
    let user = UserDir::new(&fixture_dir(&script))?;
    let net = net_fixture(&script)?;

    // The Lua side.
    let handles = OracleHandles::default();
    let mut lua_app = lua_side(&pob_root, &user, &handles, entry, opts.audit)?;
    *crate::sg_bridge::bridge(&lua_app.pob.lua)
        .map_err(|e| anyhow::anyhow!("{e}"))?
        .net
        .borrow_mut() = net.clone();
    let mut d = Driver::new(opts.screen, handles, user.path.clone());
    let (lua, r) = drive(&mut lua_app, &script, &mut d);
    r.map_err(|e| anyhow::anyhow!("Lua side: {e}"))?;
    let lua_snapshots = std::mem::take(&mut d.snapshots);
    let audit = if opts.audit {
        lua_app
            .with_vm(|pob| crate::markers::audit_hits(&pob.lua))
            .map_err(|e| anyhow::anyhow!("audit: {e}"))?
    } else {
        Vec::new()
    };
    drop(lua_app);

    // The Rust side.
    let mut rust = Vec::new();
    let mut rust_error = None;
    let mut rust_snapshots = Vec::new();
    if !opts.lua_only {
        user.reset()?;
        let handles = OracleHandles::default();
        let (sg, mut sys) = boot::oracle_parts(&pob_root, &user.path, &handles)?;
        sys.net = Box::new(boot::ScriptedNet::new(net, handles.clock.clone()));
        let parts = PlatformParts { sg, sys };
        let mut d = Driver::new(opts.screen, handles, user.path.clone());
        let res = std::panic::catch_unwind(std::panic::AssertUnwindSafe(
            || -> (Vec<FrameTrace>, anyhow::Result<()>) {
                match target {
                    Target::Scenario(_) => {
                        let mut app = RustPobApp(pob_app::PobApp::new(parts));
                        drive(&mut app, &script, &mut d)
                    }
                    Target::Gallery(name) => match crate::gallery::rust_app(name, parts) {
                        Some(app) => drive(&mut GalleryApp(app), &script, &mut d),
                        None => (
                            Vec::new(),
                            Err(anyhow::anyhow!("gallery {name}: no src/gallery/{name}.rs")),
                        ),
                    },
                }
            },
        ));
        match res {
            Ok((frames, r)) => {
                rust = frames;
                if let Err(e) = r {
                    rust_error = Some(e.to_string());
                }
            }
            Err(p) => {
                rust_error = Some(format!(
                    "Rust side panicked: {}",
                    pob_host::panic_message(&*p)
                ));
            }
        }
        rust_snapshots = d.snapshots;
    }
    Ok(RunResult {
        lua,
        rust,
        rust_error,
        lua_snapshots,
        rust_snapshots,
        audit,
        compared: !opts.lua_only,
    })
}
