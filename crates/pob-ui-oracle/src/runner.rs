//! Runs one scenario or gallery on both sides and collects their traces (ui doc §11.2, §11.3).
//! Owner: U8.
//!
//! Both sides are driven by the same [`drive`] loop through `pob_host::App`, so the input and
//! frame sequence is identical by construction: the Lua side is [`LuaApp`] (real PoB under
//! LuaJIT, [`crate::boot`]), the Rust side is `pob_app::PobApp` for scenarios and the gallery's
//! `src/gallery/<name>.rs` app for galleries.
//!
//! TRANSITIONAL (CCR-0028 item 10). ui doc §5.2 makes `pob_host::run_headless` (U7) the one
//! headless driver "used by tests and by `ui-parity`", and CCR-0026 E asks the oracle to replay
//! U7's `Script::expand()`. At this unit's base `run_headless` is a `todo!()` and the frozen `App`
//! has neither `on_script_step` (load-build, snapshot) nor `on_net` (CCR-0026 A), so [`drive`] is a
//! stand-in that follows U7's documented rules (`port/U7` `crates/pob-host/src/{headless,script}.rs`)
//! step for step. It is deleted when U7 merges: `LuaApp` then implements `on_script_step` and
//! `run_with` calls `pob_host::run_headless` for both sides (the recipe is CCR-0028 item 10).
//! Until then the test `runner::tests::drive_matches_run_headless` replays one script through both
//! drivers as soon as `run_headless` is implemented and fails on any difference in the callback
//! sequence, clock or cursor, so the two cannot drift apart unnoticed.
//!
//! The steps (U7's `Script::expand` table):
//!
//! | step | effect |
//! |---|---|
//! | `screen W H [dpi S]` | framebuffer `W×H` (physical px), OS scale `S` (default 1) for the following frames |
//! | `fixture userdir=DIR` | `GetUserPath()` is (a fresh copy of) DIR, relative to the script's directory; PoB uses `DIR/Path of Building/`. Without it: an empty directory |
//! | `fixture net=FILE.json` | scripted `launch:DownloadPage` responses, relative to the script's directory (patch 8; U7's format, [`crate::sg_bridge::NetFixtures`]) |
//! | `load-build "PATH"` | an app step: `main:SetMode("BUILD", false, <file stem>, <xml>)`, PATH relative to `DIR/Path of Building/`; no frame |
//! | `frames N` | N frames |
//! | `move X Y` | the cursor moves to virtual `(X, Y)` (physical `round(v * scale)`, scale = the DPI override or the screen's); no frame |
//! | `click KEY` | frame, KeyDown, frame, KeyUp, frame |
//! | `key-down KEY` | KeyDown (+ SG's extra OnChar for BACK/TAB/RETURN/ESCAPE); no frame |
//! | `key-up KEY` | KeyUp; no frame |
//! | `press KEY` | `key-down KEY`, frame, `key-up KEY` |
//! | `type "TEXT"` | per codepoint: `\b \t \r \x1b` as KeyDown + Char + KeyUp of BACK/TAB/RETURN/ESCAPE; other control characters dropped; U+00A0..U+00AA as SG key names; other non-ASCII `?`; no frame |
//! | `wheel up\|down [N]` | N × (OnKeyDown + OnKeyUp of WHEELUP/WHEELDOWN), default 1; no frame |
//! | `paste-clipboard "TEXT"` | sets the virtual clipboard |
//! | `snapshot NAME` | an app step: the build XML of the side (T4) |
//!
//! Host rules (ui doc §2, U7's `pob_host::deliver`): `sys.set_key` precedes each key callback;
//! KeyDown of `` ` ``/F1 with Ctrl held (SG's console toggle) and KeyUp of F10 / of PAUSE with
//! Shift held are not delivered. A mouse-button KeyDown is a double click when the same button
//! went down within 500 ms (virtual clock) and ±5 px; detection is off until the first `move`.
//! The cursor starts at physical `(0, 0)`. A frame is: `sg.begin_frame`, `sys.begin_frame`
//! (cursor snapshot), `Net::poll` (completions go to `App::on_net` once U7 lands; the Lua side
//! delivers its sub-script completions at the same point), `on_frame`, `sg.end_frame`; then the
//! virtual clock advances 16 ms (0 during `OnInit` and the first frame, patch 1). Side effects
//! recorded between frames belong to the next frame, those after the last frame to the last one.
//! Fixture and leading `screen` steps apply before `OnInit`. One deliberate difference from
//! U7's draft, proposed to U7 in CCR-0028 item 10: before `OnInit` one untraced empty frame tells
//! `Sg` the screen size (SG knows it before Lua's OnInit).
//!
//! [`parse_script`] uses U7's `Script::parse` once implemented, else a local parser of the same
//! grammar (U7-PENDING).

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
    /// A `.pobscript`; fixture paths resolve against `base_dir` (the script's directory; U7's
    /// `Script::base_dir`).
    Scenario {
        script: Script,
        base_dir: PathBuf,
    },
    Gallery(String),
}

/// Read and parse a `.pobscript` file: the script and its directory.
pub fn load_script(path: &Path) -> anyhow::Result<(Script, PathBuf)> {
    let text =
        std::fs::read_to_string(path).map_err(|e| anyhow::anyhow!("{}: {e}", path.display()))?;
    let script = parse_script(&text).map_err(|e| anyhow::anyhow!("{}: {e}", path.display()))?;
    let dir = std::path::absolute(path)
        .unwrap_or_else(|_| path.to_path_buf())
        .parent()
        .map(Path::to_path_buf)
        .unwrap_or_default();
    Ok((script, dir))
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
    inert_parts(&OracleHandles::default())
}

/// `Sg`/`Sys` without assets (`FontSet::empty()`, every image fails) on `handles`.
pub fn inert_parts(handles: &OracleHandles) -> (Sg, Sys) {
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
    let sys = Sys::new(
        Box::new(boot::OraclePlatform {
            clock: handles.clock.clone(),
            clipboard: handles.clipboard.clone(),
            script_path: String::new(),
            runtime_path: String::new(),
            user_path: String::new(),
            echo_console: false,
        }),
        Box::new(boot::OracleFs::new(PathBuf::new(), handles.clock.clone())),
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
        // ui_main.cpp:401-421: renderEnable around the sub-script pass and OnFrame.
        let r = self.with_vm(|pob| {
            let b = crate::sg_bridge::bridge(&pob.lua)?;
            b.render_enable.set(true);
            let r = crate::sg_bridge::deliver_subscripts(&pob.lua)
                .and_then(|()| pob.run_callback("OnFrame", ()));
            b.render_enable.set(false);
            r
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
        // A Lua error that PoB's own PCall caught (Launch.lua:112-118, :231-233, ...) is shown
        // with launch:ShowErrMsg (Launch.lua:381-388) instead of escaping: fail the run on it.
        let prompt = self
            .with_vm(|pob| {
                pob.eval::<Option<String>>(
                    "local l = rawget(_G, 'launch') \
                     if type(l) == 'table' and type(l.promptMsg) == 'string' then return l.promptMsg end",
                )
            })
            .map_err(|e| anyhow::anyhow!("{e}"))?;
        if let Some(p) = prompt.filter(|p| p.starts_with("^1Error")) {
            anyhow::bail!("PoB shows its error prompt (launch:ShowErrMsg): {p}");
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
                        // U7's escapes: \\ \" \n \r \t \b \e \xHH (ASCII) \u{HEX}.
                        Some('\\') => match chars.next() {
                            Some('n') => s.push('\n'),
                            Some('t') => s.push('\t'),
                            Some('r') => s.push('\r'),
                            Some('b') => s.push('\u{8}'),
                            Some('e') => s.push('\u{1b}'),
                            Some(c @ ('\\' | '"')) => s.push(c),
                            Some('x') => {
                                let h: String = chars.by_ref().take(2).collect();
                                match u8::from_str_radix(&h, 16) {
                                    Ok(v) if v < 0x80 => s.push(char::from(v)),
                                    _ => return Err(format!("bad escape \\x{h} in {line:?}")),
                                }
                            }
                            Some('u') => {
                                if chars.next() != Some('{') {
                                    return Err(format!("expected \\u{{HEX}} in {line:?}"));
                                }
                                let mut h = String::new();
                                loop {
                                    match chars.next() {
                                        Some('}') => break,
                                        Some(c) => h.push(c),
                                        None => {
                                            return Err(format!("unterminated \\u{{ in {line:?}"))
                                        }
                                    }
                                }
                                let c = u32::from_str_radix(&h, 16)
                                    .ok()
                                    .and_then(char::from_u32)
                                    .ok_or_else(|| {
                                    format!("bad escape \\u{{{h}}} in {line:?}")
                                })?;
                                s.push(c);
                            }
                            Some(o) => return Err(format!("unknown escape \\{o} in {line:?}")),
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

/// A key argument (U7): one ASCII character verbatim, else an SG key name.
fn parse_key(s: &str) -> Result<Key, String> {
    let mut it = s.chars();
    if let (Some(c), None) = (it.next(), it.next()) {
        return if c.is_ascii() {
            Ok(Key::Char(c as u8))
        } else {
            Err(format!("key {s:?} is not ASCII"))
        };
    }
    Key::from_name(s).ok_or_else(|| format!("unknown key {s:?}"))
}

/// The local `.pobscript` parser (see [`parse_script`]).
#[allow(clippy::needless_update)] // U7 adds `Script::base_dir`
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
                    let dpi = match toks.len() {
                        3 => 1.0,
                        5 if toks[3] == "dpi" => num(4)? as f32,
                        _ => return Err(err("expected `screen W H [dpi S]`")),
                    };
                    Step::Screen {
                        w: num(1)? as i32,
                        h: num(2)? as i32,
                        dpi,
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
                    count: if toks.len() > 2 { num(2)? as u32 } else { 1 },
                },
                "paste-clipboard" => Step::PasteClipboard(arg(1)?),
                "snapshot" => Step::Snapshot(arg(1)?),
                "exit" => return Err(err("`exit` needs U7's Step::Exit (U7-PENDING)")),
                other => return Err(err(&format!("unknown step {other:?}"))),
            };
            steps.push(step);
        }
    }
    Ok(Script {
        steps,
        ..Script::default()
    })
}

// ---------------------------------------------------------------------------------------------
// The driver
// ---------------------------------------------------------------------------------------------

/// The virtual clock step per frame (U7 `headless::FRAME_MS`).
pub const FRAME_MS: i64 = 16;

/// `pob_host::input::char_key` on `port/U7` (GLFW + `CallKeyHandler`): the `OnChar` key of a
/// typed codepoint; `None` for the control characters GLFW drops (`cp < 32`, `127..=159`).
/// U+00A0..U+00AA arrive as SG key names (`keyList_e` values 160..170), other non-ASCII as `?`.
pub fn char_key(cp: u32) -> Option<Key> {
    if cp < 32 || (127..160).contains(&cp) {
        return None;
    }
    if cp < 128 {
        return Some(Key::Char(cp as u8));
    }
    Some(match cp {
        // keylist.h: KEY_F1 = 154 .. KEY_F15 = 168, KEY_NUMLOCK = 169, KEY_SCROLL = 170.
        160..=168 => Key::F((cp - 153) as u8),
        169 => Key::NumLock,
        170 => Key::ScrollLock,
        _ => Key::Unknown,
    })
}

fn is_mouse_button(k: Key) -> bool {
    matches!(
        k,
        Key::LeftButton | Key::MiddleButton | Key::RightButton | Key::Mouse4 | Key::Mouse5
    )
}

/// Driver state shared by both sides' runs (TRANSITIONAL, see the module docs).
pub struct Driver {
    pub screen: ScreenInfo,
    pub handles: OracleHandles,
    /// The user dir (`GetUserPath()`), for `load-build`.
    pub user_dir: PathBuf,
    cursor_phys: (i32, i32),
    /// `lastCursorPos` for double-click detection: unknown until the first `move`.
    last_cursor: Option<(i32, i32)>,
    last_click: Option<(Key, (i32, i32), i64)>,
    pub snapshots: Vec<(String, String)>,
    /// Steps before the first non-`screen`/`fixture` step have been applied.
    started: bool,
}

impl Driver {
    pub fn new(screen: ScreenInfo, handles: OracleHandles, user_dir: PathBuf) -> Driver {
        Driver {
            screen,
            handles,
            user_dir,
            cursor_phys: (0, 0),
            last_cursor: None,
            last_click: None,
            snapshots: Vec::new(),
            started: false,
        }
    }

    fn check<A: ScenarioApp + ?Sized>(app: &mut A) -> anyhow::Result<()> {
        match app.take_error() {
            Some(e) => anyhow::bail!("{e}"),
            None => Ok(()),
        }
    }

    /// One frame (ui_main.cpp:393-433, U7 `pob_host::run_frame`): BeginFrame, cursor snapshot,
    /// the Net completions, OnFrame, EndFrame; then the clock advances.
    pub fn frame<A: ScenarioApp + ?Sized>(&mut self, app: &mut A) -> anyhow::Result<()> {
        // Effects recorded since the last frame (input callbacks, OnInit) belong to this frame
        // (U7 attaches them when the frame ends); `begin_frame` would drop them.
        let carried = app
            .sg()
            .trace
            .as_mut()
            .map(|t| std::mem::take(&mut t.cur.effects))
            .unwrap_or_default();
        app.sg().begin_frame(self.screen);
        if let Some(t) = app.sg().trace.as_mut() {
            t.cur.effects.splice(0..0, carried);
        }
        let scale = app.sg().scale();
        app.sys().begin_frame(self.cursor_phys, scale);
        // The Net completions (SG's sub-script pass). CCR-0026 A hands them to `App::on_net`,
        // which the frozen `App` of this base lacks; until U7 merges they are dropped here, as
        // `on_net`'s default does. The Lua side delivers its own at this point (`LuaApp`).
        let _ = app.sys().net.poll();
        app.on_frame();
        Self::check(app)?;
        app.after_frame()?;
        let _ = app.sg().end_frame();
        self.handles.clock.set(self.handles.clock.get() + FRAME_MS);
        Ok(())
    }

    /// Virtual px → physical px: `GetCursorPos` divides by the DPI override or the screen scale.
    fn scale<A: ScenarioApp + ?Sized>(&self, app: &mut A) -> f64 {
        let o = app.sg().get_dpi_scale_override_percent();
        if o > 0 {
            f64::from(o) / 100.0
        } else {
            f64::from(self.screen.dpi_scale)
        }
    }

    /// sys_video.cpp:568-604 / U7 `DoubleClick::press`: the same button within 500 ms and
    /// ±5 px; a detected double click resets the memory; off while the cursor is unknown.
    fn double_click(&mut self, key: Key) -> bool {
        let Some(pos) = self.last_cursor else {
            return false;
        };
        let now = self.handles.clock.get();
        let dbl = matches!(self.last_click, Some((k, p, t)) if k == key
            && now - t <= 500
            && (p.0 - pos.0).abs() <= 5
            && (p.1 - pos.1).abs() <= 5);
        self.last_click = if dbl { None } else { Some((key, pos, now)) };
        dbl
    }

    fn key_down<A: ScenarioApp + ?Sized>(&mut self, app: &mut A, key: Key) -> anyhow::Result<()> {
        let dbl = is_mouse_button(key) && self.double_click(key);
        app.sys().set_key(key, true);
        // ui_console.cpp:101-104: Ctrl+` / Ctrl+F1 toggle SG's console and do not reach Lua
        // (a double click is KE_DBLCLK, not KE_KEYDOWN).
        let ctrl = app.sys().is_key_down(Key::Ctrl);
        if dbl || !(ctrl && (key == Key::Char(b'`') || key == Key::F(1))) {
            app.on_key_down(key, dbl);
            Self::check(app)?;
        }
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
        if pob_host::input::swallow_key_up(key, shift) {
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
        let lead = matches!(step, Step::Screen { .. } | Step::Fixture { .. });
        if !lead {
            self.started = true;
        }
        match step {
            Step::Screen { w, h, dpi } => self.screen = screen(*w, *h, *dpi),
            // Applied before boot (run_with). U7 also applies later ones where they stand.
            Step::Fixture { key, .. } => anyhow::ensure!(
                !self.started,
                "fixture {key}=..: the oracle applies fixtures before boot; put them first"
            ),
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
                let s = self.scale(app);
                self.cursor_phys = ((x * s).round() as i32, (y * s).round() as i32);
                self.last_cursor = Some(self.cursor_phys);
            }
            Step::Click(k) => {
                self.frame(app)?;
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
                        '\u{8}' => Some(Key::Back),
                        '\t' => Some(Key::Tab),
                        '\r' => Some(Key::Return),
                        '\u{1b}' => Some(Key::Escape),
                        _ => None,
                    };
                    if let Some(k) = named {
                        self.key_down(app, k)?; // includes the OnChar
                        self.key_up(app, k)?;
                    } else if let Some(k) = char_key(c as u32) {
                        self.char(app, k)?;
                    }
                }
            }
            Step::Press(k) => {
                self.key_down(app, *k)?;
                self.frame(app)?;
                self.key_up(app, *k)?;
            }
            Step::Wheel { up, count } => {
                // U7 `deliver(Input::Wheel)`: the callbacks only (no key state, no double click).
                let k = if *up { Key::WheelUp } else { Key::WheelDown };
                for _ in 0..*count {
                    app.on_key_down(k, false);
                    Self::check(app)?;
                    app.on_key_up(k);
                    Self::check(app)?;
                }
            }
            Step::PasteClipboard(t) => *self.handles.clipboard.borrow_mut() = Some(t.clone()),
            Step::Snapshot(name) => {
                if let Some(xml) = app.snapshot()? {
                    self.snapshots.push((name.clone(), xml));
                }
            }
            // U7 adds `Step::Exit` (`exit`).
            #[allow(unreachable_patterns)]
            other => anyhow::bail!("step {other:?} needs U7's run_headless (U7-PENDING)"),
        }
        Ok(())
    }
}

/// Drive `app` through `script` (TRANSITIONAL, see the module docs): the leading `screen` /
/// `fixture` steps, one untraced empty frame that tells `Sg` the screen size, `OnInit` (clock 0),
/// then the remaining steps, with tracing on from `OnInit`. Returns the frames recorded (also on
/// error: the frames so far).
pub fn drive<A: ScenarioApp + ?Sized>(
    app: &mut A,
    script: &Script,
    d: &mut Driver,
) -> (Vec<FrameTrace>, anyhow::Result<()>) {
    let lead = script
        .steps
        .iter()
        .take_while(|s| matches!(s, Step::Screen { .. } | Step::Fixture { .. }))
        .count();
    let r = (|| {
        for step in &script.steps[..lead] {
            d.step(app, step)?;
        }
        // SG knows the window size before OnInit: one untraced empty frame.
        app.sg().trace = None;
        app.sg().begin_frame(d.screen);
        let scale = app.sg().scale();
        app.sys().begin_frame(d.cursor_phys, scale);
        let _ = app.sg().end_frame();
        app.sg().trace = Some(TraceRecorder::new());
        app.on_init();
        Driver::check(app)?;
        for step in &script.steps[lead..] {
            d.step(app, step)?;
        }
        Ok(())
    })();
    let mut frames = Vec::new();
    if let Some(mut t) = app.sg().trace.take() {
        // Effects after the last frame belong to it (U7 `flush_effects`).
        if let Some(last) = t.frames.last_mut() {
            last.effects.append(&mut t.cur.effects);
        }
        frames = t.frames;
    }
    (frames, r)
}

// ---------------------------------------------------------------------------------------------
// Runs
// ---------------------------------------------------------------------------------------------

/// A run's user dir: `GetUserPath()` returns `path`, PoB uses `path/Path of Building/`
/// (Main.lua:94-98). It is a fresh copy of the `userdir` fixture (whose own `Path of Building/`
/// holds `Settings.xml` and `Builds/`), or empty without one (U7: nothing exists at the default
/// user path). The copy stands in for U7's copy-on-write overlay: the Lua side writes through
/// LuaJIT's `io`, so the fixture itself must not be the target. Removed on drop.
pub struct UserDir {
    pub path: PathBuf,
    fixture: Option<PathBuf>,
}

static RUN_COUNTER: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);

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
        }
    }
    Ok(())
}

impl UserDir {
    /// A new run dir with a copy of `fixture` (empty for `None`).
    pub fn new(fixture: Option<&Path>) -> anyhow::Result<UserDir> {
        let n = RUN_COUNTER.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let path = std::env::temp_dir()
            .join("pob-ui-oracle")
            .join(format!("{}-{n}", std::process::id()));
        let d = UserDir {
            path,
            fixture: fixture.map(Path::to_path_buf),
        };
        d.reset()?;
        Ok(d)
    }

    /// Restore the pristine fixture (before each side).
    pub fn reset(&self) -> anyhow::Result<()> {
        let _ = std::fs::remove_dir_all(&self.path);
        match &self.fixture {
            Some(f) => copy_dir(f, &self.path).map_err(|e| {
                anyhow::anyhow!(
                    "copying fixture {} to {}: {e}",
                    f.display(),
                    self.path.display()
                )
            }),
            None => std::fs::create_dir_all(&self.path).map_err(Into::into),
        }
    }
}

impl Drop for UserDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.path);
    }
}

/// The host fixtures of a script (U7's keys; paths relative to the script's directory).
pub struct Fixtures {
    /// `fixture userdir=DIR` (the last one).
    pub user_dir: Option<PathBuf>,
    /// `fixture net=FILE.json` (the last one; empty without one: every request gets "No net
    /// fixture for <url>").
    pub net: crate::sg_bridge::NetFixtures,
}

/// Collect the `fixture` steps of `script`. Keys other than `userdir` and `net` are app fixtures
/// in U7 (`App::on_script_step`); neither side has one yet, so they are an error.
pub fn fixtures(script: &Script, base_dir: &Path) -> anyhow::Result<Fixtures> {
    let mut out = Fixtures {
        user_dir: None,
        net: crate::sg_bridge::NetFixtures::default(),
    };
    for s in &script.steps {
        if let Step::Fixture { key, value } = s {
            let p = sg_normalize(&base_dir.join(value));
            match key.as_str() {
                "userdir" => {
                    anyhow::ensure!(
                        p.is_dir(),
                        "fixture userdir={value}: {} is not a directory",
                        p.display()
                    );
                    out.user_dir = Some(p);
                }
                "net" => {
                    out.net = crate::sg_bridge::NetFixtures::load(&p)
                        .map_err(|e| anyhow::anyhow!("fixture net={value}: {e}"))?;
                }
                _ => anyhow::bail!("unknown fixture key {key:?} (userdir, net)"),
            }
        }
    }
    Ok(out)
}

fn sg_normalize(p: &Path) -> PathBuf {
    crate::sg_bridge::normalize_path(&std::path::absolute(p).unwrap_or_else(|_| p.to_path_buf()))
}

/// The directory gallery scripts resolve fixtures against.
pub fn gallery_dir() -> PathBuf {
    crate_dir().join("lua/gallery")
}

/// The script that drives a gallery: `lua/gallery/<name>.pobscript` if present, else `frames 3`.
#[allow(clippy::needless_update)] // U7 adds `Script::base_dir`
pub fn gallery_script(name: &str) -> anyhow::Result<Script> {
    let p = gallery_dir().join(format!("{name}.pobscript"));
    if p.is_file() {
        load_script(&p).map(|(s, _)| s)
    } else {
        Ok(Script {
            steps: vec![Step::Frames(3)],
            ..Script::default()
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
    let (script, base_dir, entry) = match target {
        Target::Scenario { script, base_dir } => (script.clone(), base_dir.clone(), Entry::Launch),
        Target::Gallery(name) => {
            let lua = gallery_dir().join(format!("{name}.lua"));
            anyhow::ensure!(
                lua.is_file(),
                "gallery {name}: {} is missing",
                lua.display()
            );
            (gallery_script(name)?, gallery_dir(), Entry::Gallery(lua))
        }
    };
    let fx = fixtures(&script, &base_dir)?;
    let user = UserDir::new(fx.user_dir.as_deref())?;

    // The Lua side.
    let handles = OracleHandles::default();
    let mut lua_app = lua_side(&pob_root, &user, &handles, entry, opts.audit)?;
    *crate::sg_bridge::bridge(&lua_app.pob.lua)
        .map_err(|e| anyhow::anyhow!("{e}"))?
        .net
        .borrow_mut() = fx.net.clone();
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
        sys.net = Box::new(boot::ScriptedNet::new(fx.net));
        let parts = PlatformParts { sg, sys };
        let mut d = Driver::new(opts.screen, handles, user.path.clone());
        let res = std::panic::catch_unwind(std::panic::AssertUnwindSafe(
            || -> (Vec<FrameTrace>, anyhow::Result<()>) {
                match target {
                    Target::Scenario { .. } => {
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

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::RefCell;

    /// Logs every callback, with the clock and cursor `on_frame` sees.
    struct Recorder {
        sg: Sg,
        sys: Sys,
        log: Rc<RefCell<Vec<String>>>,
    }

    impl Recorder {
        fn log(&self, s: String) {
            self.log.borrow_mut().push(s);
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
            self.log(format!("init t={}", self.sys.time_ms()));
        }
        fn on_frame(&mut self) {
            let (x, y) = self.sys.cursor_pos();
            self.log(format!("frame t={} cur={x},{y}", self.sys.time_ms()));
        }
        fn on_key_down(&mut self, key: Key, double_click: bool) {
            self.log(format!("down {key:?} dbl={double_click}"));
        }
        fn on_key_up(&mut self, key: Key) {
            self.log(format!("up {key:?}"));
        }
        fn on_char(&mut self, ch: Key) {
            self.log(format!("char {ch:?}"));
        }
        fn on_lua_error(&mut self, msg: String) {
            self.log(format!("error {msg}"));
        }
    }

    impl ScenarioApp for Recorder {
        fn load_build(&mut self, _name: &str, _xml: &str) -> anyhow::Result<()> {
            Ok(())
        }
        fn snapshot(&mut self) -> anyhow::Result<Option<String>> {
            Ok(None)
        }
    }

    /// Exercises every input rule both drivers implement (no fixtures, no app steps).
    const SCRIPT: &str = r#"screen 1280 800 dpi 1.5
frames 1
click LEFTBUTTON                       # cursor unknown: no double-click detection
move 100.4 50.2                        # physical (151, 75): round, no frame
click LEFTBUTTON
click LEFTBUTTON                       # 48 ms later, same place: a double click
key-down CTRL ; key-down ` ; key-up ` ; key-up CTRL
type "a\tb\nc\u{A3}\u{E9}"
press RETURN
wheel down 2
key-down SHIFT ; key-up PAUSE ; key-up SHIFT ; key-up F10
"#;

    fn run_drive(script: &Script) -> (Vec<String>, usize) {
        let handles = OracleHandles::default();
        let (sg, sys) = inert_parts(&handles);
        let log = Rc::new(RefCell::new(Vec::new()));
        let mut app = Recorder {
            sg,
            sys,
            log: log.clone(),
        };
        let mut d = Driver::new(screen(1280, 800, 1.0), handles, PathBuf::new());
        let (frames, r) = drive(&mut app, script, &mut d);
        r.unwrap();
        let log = log.borrow().clone();
        (log, frames.len())
    }

    /// The driver follows U7's documented expansion (`port/U7` `pob_host::script` module docs).
    #[test]
    fn drive_follows_u7_expansion() {
        let script = parse_script(SCRIPT).unwrap();
        let (log, frames) = run_drive(&script);
        let expected = [
            "init t=0",
            "frame t=0 cur=0,0",
            "frame t=16 cur=0,0",
            "down LeftButton dbl=false",
            "frame t=32 cur=0,0",
            "up LeftButton",
            "frame t=48 cur=0,0",
            "frame t=64 cur=101,50",
            "down LeftButton dbl=false",
            "frame t=80 cur=101,50",
            "up LeftButton",
            "frame t=96 cur=101,50",
            "frame t=112 cur=101,50",
            "down LeftButton dbl=true",
            "frame t=128 cur=101,50",
            "up LeftButton",
            "frame t=144 cur=101,50",
            "down Ctrl dbl=false",
            "up Char(96)",
            "up Ctrl",
            "char Char(97)",
            "down Tab dbl=false",
            "char Char(9)",
            "up Tab",
            "char Char(98)",
            "char Char(99)",
            "char F(10)",
            "char Unknown",
            "down Return dbl=false",
            "char Char(13)",
            "frame t=160 cur=101,50",
            "up Return",
            "down WheelDown dbl=false",
            "up WheelDown",
            "down WheelDown dbl=false",
            "up WheelDown",
            "down Shift dbl=false",
            "up Shift",
        ];
        assert_eq!(log, expected);
        assert_eq!(frames, 11);
    }

    /// CCR-0028 item 10 tripwire: once U7's `pob_host::run_headless` is implemented, it and this
    /// crate's transitional [`drive`] must produce the same callback sequence, clock and cursor
    /// for the same script; any drift fails here. (The oracle then switches to `run_headless`.)
    #[test]
    fn drive_matches_run_headless() {
        let script = parse_script(SCRIPT).unwrap();
        let (ours, our_frames) = run_drive(&script);
        let handles = OracleHandles::default();
        let (sg, sys) = inert_parts(&handles);
        let log = Rc::new(RefCell::new(Vec::new()));
        let mut app = Recorder {
            sg,
            sys,
            log: log.clone(),
        };
        let Some(frames) = if_implemented(|| pob_host::run_headless(&mut app, &script)) else {
            eprintln!("pob_host::run_headless is a stub (U7-PENDING): drift check skipped");
            return;
        };
        let theirs = log.borrow().clone();
        assert_eq!(
            ours, theirs,
            "runner::drive and pob_host::run_headless disagree (CCR-0028 item 10)"
        );
        assert_eq!(our_frames, frames.len());
    }

    #[test]
    fn fixtures_resolve_against_the_script_dir() {
        let base = crate_dir().join("scenarios/_smoke");
        let (script, dir) = load_script(&base.join("boot_list.pobscript")).unwrap();
        assert_eq!(dir, base);
        let fx = fixtures(&script, &dir).unwrap();
        assert_eq!(
            fx.user_dir.as_deref(),
            Some(crate_dir().join("fixtures/user1").as_path())
        );
        assert!(fx
            .user_dir
            .unwrap()
            .join("Path of Building/Settings.xml")
            .is_file());
    }

    #[test]
    fn char_key_like_glfw_and_sg() {
        assert_eq!(char_key(u32::from('a')), Some(Key::Char(b'a')));
        assert_eq!(char_key(10), None);
        assert_eq!(char_key(127), None);
        assert_eq!(char_key(0xA0), Some(Key::F(7)));
        assert_eq!(char_key(0xA9), Some(Key::NumLock));
        assert_eq!(char_key(0xAA), Some(Key::ScrollLock));
        assert_eq!(char_key(0xE9), Some(Key::Unknown));
    }
}
