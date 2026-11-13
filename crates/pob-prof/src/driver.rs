//! The measuring driver: one scenario on the real `PobApp`, headless. Owner: PROF1.
//!
//! It follows pob-host's `run_headless` action by action (the leading `screen`/`fixture` steps,
//! then `on_init`, then `Script::expand`'s actions: frames through `pob_host::run_frame`, input
//! through `pob_host::deliver`, double clicks from `pob_host::input::DoubleClick` on the virtual
//! input clock), with these differences, all for measuring:
//!
//! - **Clock.** `GetTime()` is the real monotonic clock by default (`fixture clock=virtual` makes
//!   it the virtual +16 ms per frame clock of `run_headless`). Code that budgets its work by time
//!   behaves as on the desktop: the power report yields after 100 ms of work
//!   (CalcsTab.lua:655), so it spreads over frames as a user sees it. Input timing (double-click
//!   detection) stays on the virtual clock, so the scripted input means the same in every run.
//! - **No trace.** The app runs without a `TraceRecorder` (as shipped); `inspect --texts` turns
//!   one on, and each `expect-text` guard records one extra, untimed frame.
//! - **Frames are rendered** by the run's [`Renderer`] (none, CPU raster, offscreen wgpu).
//! - **Services.** The platform is [`ProfPlatform`] (the clock above; otherwise pob-host's
//!   `HeadlessPlatform` behaviour); files are pob-host's copy-on-write `HeadlessFs` over the
//!   fixture user dir (saves stay in memory, so runs never change the fixtures); net is
//!   `ScriptedNet` (no network).
//! - **Lua errors fail the run** (a `LuaError` panic that reaches `on_lua_error`).
//!
//! Each `step` of the scenario gets a [`StepResult`]; guards and snapshots are untimed.

use std::cell::Cell;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::rc::{Rc, Weak};
use std::time::Instant;

use anyhow::{anyhow, Result};
use pob_app::app::CxApp;
use pob_app::build::on_frame as of;
use pob_app::PobApp;
use pob_host::headless::{load_net_fixtures, Headless, HeadlessFs, HeadlessState, ScriptedNet};
use pob_host::input::{DoubleClick, Input};
use pob_host::script::{expand_step, is_mouse_button, Action, Step};
use pob_host::{deliver, guarded, run_frame, App, PlatformParts};
use pob_sg::trace::{ApiCall, SideEffect, TraceRecorder};
use pob_sg::{AssetSource, FontSet, ImageRegistry, Key, Platform, ScreenInfo, Sg, Sys};
use serde::Serialize;

use crate::alloc::{self, AllocSnap};
use crate::calc_timing::{self, CalcCounters};
use crate::paths::Inputs;
use crate::render::{RenderMode, Renderer};
use crate::scenario::{Guard, Scenario, Seg};
use crate::stats;

/// The virtual input clock step per frame (pob-host's `FRAME_MS`).
pub const FRAME_MS: i64 = pob_host::headless::FRAME_MS;

// ---- services ------------------------------------------------------------------------------

/// Reads `src/` and the SimpleGraphic runtime directory from disk.
pub struct DirAssets {
    pub src: PathBuf,
    pub runtime: PathBuf,
}

impl AssetSource for DirAssets {
    fn read_src(&self, path: &str) -> std::io::Result<Vec<u8>> {
        std::fs::read(self.src.join(path))
    }
    fn read_runtime(&self, path: &str) -> std::io::Result<Vec<u8>> {
        std::fs::read(self.runtime.join(path))
    }
}

/// pob-host's `HeadlessPlatform` with a switchable clock (module docs).
pub struct ProfPlatform {
    pub h: Headless,
    pub start: Instant,
    pub real_clock: Rc<Cell<bool>>,
}

impl Platform for ProfPlatform {
    fn time_ms(&self) -> i64 {
        if self.real_clock.get() {
            self.start.elapsed().as_millis() as i64
        } else {
            self.h.clock_ms.get()
        }
    }
    fn copy(&mut self, text: &str) {
        *self.h.clipboard.borrow_mut() = Some(text.to_owned());
    }
    fn paste(&mut self) -> Option<String> {
        self.h.clipboard.borrow().clone()
    }
    fn open_url(&mut self, _url: &str) {}
    fn set_window_title(&mut self, _title: &str) {}
    fn take_screenshot(&mut self) {}
    fn restart(&mut self) {
        self.h.restart_requested.set(true);
    }
    fn exit(&mut self, msg: Option<&str>) {
        if let Some(m) = msg {
            self.h.console.borrow_mut().push(m.to_owned());
        }
        self.h.exit_requested.set(true);
    }
    fn script_path(&self) -> String {
        self.h.script_path.borrow().clone()
    }
    fn runtime_path(&self) -> String {
        self.h.runtime_path.borrow().clone()
    }
    fn user_path(&self) -> Result<String, (String, String)> {
        Ok(self.h.user_path.borrow().clone())
    }
    fn con_print(&mut self, s: &str) {
        let mut c = self.h.console.borrow_mut();
        // keep the tail only (ConPrintf output is not measured)
        if c.len() > 200 {
            c.drain(..100);
        }
        c.push(s.to_owned());
    }
}

/// `Sg` (fonts, the synchronous headless image loader) and `Sys` (the services above) over
/// `inputs`' assets. Returns the shared headless state too.
pub fn platform_parts(
    inputs: &Inputs,
    real_clock: Rc<Cell<bool>>,
) -> Result<(PlatformParts, Headless)> {
    let assets = DirAssets {
        src: inputs.src.clone(),
        runtime: inputs.runtime.clone(),
    };
    let mut images = ImageRegistry::new(Box::new(pob_host::loader_headless::HeadlessLoader::new(
        Box::new(DirAssets {
            src: inputs.src.clone(),
            runtime: inputs.runtime.clone(),
        }),
    )));
    let fonts = FontSet::load(&assets, &mut images)
        .map_err(|e| anyhow!("loading fonts under {}: {e}", inputs.runtime.display()))?;
    let sg = Sg::new(Rc::new(fonts), images);
    let runtime_base = inputs
        .runtime
        .parent()
        .map_or_else(|| inputs.runtime.clone(), Path::to_path_buf);
    let h = HeadlessState::new(
        &inputs.src.to_string_lossy(),
        &runtime_base.to_string_lossy(),
    );
    let platform = ProfPlatform {
        h: h.clone(),
        start: Instant::now(),
        real_clock,
    };
    let sys = Sys::new(
        Box::new(platform),
        Box::new(HeadlessFs(h.clone())),
        Box::new(ScriptedNet(h.clone())),
    );
    Ok((PlatformParts { sg, sys }, h))
}

// ---- the app wrapper -----------------------------------------------------------------------

/// `PobApp` with its Lua errors recorded.
pub struct ProfApp {
    pub app: PobApp,
    pub errors: Vec<String>,
}

impl App for ProfApp {
    fn sg(&mut self) -> &mut Sg {
        self.app.sg()
    }
    fn sys(&mut self) -> &mut Sys {
        self.app.sys()
    }
    fn on_init(&mut self) {
        self.app.on_init();
    }
    fn on_frame(&mut self) {
        self.app.on_frame();
    }
    fn on_key_down(&mut self, key: Key, double_click: bool) {
        self.app.on_key_down(key, double_click);
    }
    fn on_key_up(&mut self, key: Key) {
        self.app.on_key_up(key);
    }
    fn on_char(&mut self, ch: Key) {
        self.app.on_char(ch);
    }
    fn can_exit(&mut self) -> bool {
        self.app.can_exit()
    }
    fn on_exit(&mut self) {
        self.app.on_exit();
    }
    fn wants_frames(&self) -> bool {
        self.app.wants_frames()
    }
    fn on_lua_error(&mut self, msg: String) {
        self.errors.push(msg.clone());
        self.app.on_lua_error(msg);
    }
    fn on_net(&mut self, id: u64, resp: pob_sg::HttpResponse) {
        self.app.on_net(id, resp);
    }
    fn on_script_step(&mut self, step: &Step) {
        self.app.on_script_step(step);
    }
}

// ---- tab draw timing -----------------------------------------------------------------------

thread_local! {
    static TAB_NS: Cell<u64> = const { Cell::new(0) };
}

/// Time the build tabs' `Draw` (the shell's `draw_tab` hook, Build.lua:1300-1318) on top of the
/// default hooks.
pub fn install_tab_timer() {
    of::set_shell_hooks(|h| {
        *h = of::default_shell_hooks();
        let inner = h.draw_tab.clone();
        h.draw_tab = Rc::new(move |cx, tab, vp, ev| {
            let t0 = Instant::now();
            inner(cx, tab, vp, ev);
            let d = t0.elapsed().as_nanos() as u64;
            TAB_NS.with(|t| t.set(t.get() + d));
        });
    });
}

fn tab_ns() -> u64 {
    TAB_NS.with(Cell::get)
}

// ---- memory --------------------------------------------------------------------------------

/// (VmRSS, VmHWM) in MB from /proc/self/status (0 where unavailable).
pub fn rss_mb() -> (f64, f64) {
    let s = std::fs::read_to_string("/proc/self/status").unwrap_or_default();
    let field = |k: &str| {
        s.lines()
            .find_map(|l| l.strip_prefix(k))
            .and_then(|v| v.split_whitespace().next())
            .and_then(|v| v.parse::<f64>().ok())
            .map_or(0.0, |kb| kb / 1024.0)
    };
    (field("VmRSS:"), field("VmHWM:"))
}

/// Reset VmHWM to the current RSS (`echo 5 > /proc/self/clear_refs`, Linux >= 4.0).
pub fn reset_hwm() -> bool {
    std::fs::write("/proc/self/clear_refs", "5").is_ok()
}

/// This thread's CPU time (s): the app, its calc and the driver all run on the main thread.
pub fn thread_cpu_s() -> f64 {
    let t = rustix::time::clock_gettime(rustix::time::ClockId::ThreadCPUTime);
    t.tv_sec as f64 + t.tv_nsec as f64 * 1e-9
}

/// Process CPU time (s).
pub fn cpu_s() -> f64 {
    let t = rustix::time::clock_gettime(rustix::time::ClockId::ProcessCPUTime);
    t.tv_sec as f64 + t.tv_nsec as f64 * 1e-9
}

// ---- results -------------------------------------------------------------------------------

/// One step's measurements (times in ms, memory in MB).
#[derive(Clone, Debug, Default, Serialize, serde::Deserialize)]
pub struct StepResult {
    pub name: String,
    /// Elapsed time of the step without its guards/snapshots.
    pub wall_ms: f64,
    /// The main thread's CPU time in the step (without guards/snapshots): less sensitive to other
    /// load on the machine than `wall_ms` (preemption does not count).
    #[serde(default)]
    pub cpu_ms: f64,
    pub frames: u64,
    /// Frame time = `run_frame` (begin, net, `OnFrame`, end) + rendering.
    pub frame_p50_ms: f64,
    pub frame_p95_ms: f64,
    pub frame_max_ms: f64,
    pub frame_sum_ms: f64,
    /// Input callbacks and app steps (`load-build`) outside frames.
    pub input_ms: f64,
    pub render_ms: f64,
    /// The build tabs' `Draw` (inside the frames).
    pub tab_draw_ms: f64,
    /// Full recalcs (`calcsTab:BuildOutput`) and their time.
    pub recalcs: u64,
    pub recalc_ms: f64,
    pub recalc_max_ms: f64,
    /// `GetMiscCalculator` calls / misc calculator calls (tooltips, previews) and their time.
    pub misc_gets: u64,
    pub misc_calls: u64,
    pub misc_ms: f64,
    pub allocs: u64,
    pub alloc_mb: f64,
    /// Peak live heap during the step (counting allocator).
    pub heap_peak_mb: f64,
    /// Peak RSS during the step (VmHWM, reset at the step start).
    pub rss_peak_mb: f64,
    /// A build opened in this step: step start to the end of the first frame showing it with its
    /// first recalc done.
    pub build_load_ms: Option<f64>,
    /// `wait-power`: until the power report finished.
    pub power_ms: Option<f64>,
}

/// Process start-up (ms from `main`, except `exec_ms`).
#[derive(Clone, Debug, Default, Serialize, serde::Deserialize)]
pub struct Startup {
    /// The parent's spawn to this process's `main` (exec, dynamic loading); `None` when run by hand.
    pub exec_ms: Option<f64>,
    /// `pob_gamedata::init_from_dir` (data.bin).
    pub data_ms: f64,
    /// Fonts and the image registry.
    pub assets_ms: f64,
    /// `PobApp::new` (Launch.lua's module code).
    pub app_new_ms: f64,
    /// `launch:OnInit`.
    pub on_init_ms: f64,
    /// The first frame.
    pub first_frame_ms: f64,
    /// `main` to the end of the first frame in LIST mode (or BUILD, if the settings open one).
    pub to_first_screen_ms: f64,
}

/// One repetition of one scenario.
#[derive(Clone, Debug, Default, Serialize, serde::Deserialize)]
pub struct RepResult {
    pub scenario: String,
    pub rep: u32,
    pub ok: bool,
    pub error: Option<String>,
    pub render: String,
    pub adapter: Option<String>,
    pub startup: Startup,
    pub steps: Vec<StepResult>,
    /// Sum of the steps' `wall_ms` (startup excluded).
    pub total_ms: f64,
    pub cpu_ms: f64,
    pub peak_rss_mb: f64,
    pub alloc_counting: bool,
}

// ---- the driver ----------------------------------------------------------------------------

struct StepAcc {
    name: String,
    t0: Instant,
    cpu0: f64,
    guard_ns: u64,
    guard_cpu_s: f64,
    frames: Vec<f64>,
    input_ns: u64,
    render_ns: u64,
    tab0: u64,
    calc0: CalcCounters,
    alloc0: AllocSnap,
    guard_allocs: u64,
    guard_bytes: u64,
    build0: Option<Weak<pob_calc::model::Build>>,
    build_load_ms: Option<f64>,
    power_ms: Option<f64>,
    full_log: Vec<u64>,
    file_writes: u64,
}

impl StepAcc {
    fn new(name: &str, build0: Option<Weak<pob_calc::model::Build>>) -> StepAcc {
        let _ = calc_timing::take_full_log();
        alloc::reset_peak();
        reset_hwm();
        StepAcc {
            name: name.to_owned(),
            t0: Instant::now(),
            cpu0: thread_cpu_s(),
            guard_ns: 0,
            guard_cpu_s: 0.0,
            frames: Vec::new(),
            input_ns: 0,
            render_ns: 0,
            tab0: tab_ns(),
            calc0: calc_timing::counters(),
            alloc0: alloc::snapshot(),
            guard_allocs: 0,
            guard_bytes: 0,
            build0,
            build_load_ms: None,
            power_ms: None,
            full_log: Vec::new(),
            file_writes: 0,
        }
    }

    fn elapsed_ms(&self) -> f64 {
        (self.t0.elapsed().as_nanos() as u64).saturating_sub(self.guard_ns) as f64 / 1e6
    }

    fn finish(mut self) -> StepResult {
        let wall_ms = self.elapsed_ms();
        let cpu_ms = ((thread_cpu_s() - self.cpu0 - self.guard_cpu_s) * 1000.0).max(0.0);
        let calc = calc_timing::counters().since(&self.calc0);
        self.full_log.extend(calc_timing::take_full_log());
        let a = alloc::snapshot().since(&self.alloc0);
        let (_, hwm) = rss_mb();
        let f = &self.frames;
        StepResult {
            name: self.name,
            wall_ms,
            cpu_ms,
            frames: f.len() as u64,
            frame_p50_ms: q0(f, 0.5),
            frame_p95_ms: q0(f, 0.95),
            frame_max_ms: q0(f, 1.0),
            frame_sum_ms: f.iter().sum(),
            input_ms: self.input_ns as f64 / 1e6,
            render_ms: self.render_ns as f64 / 1e6,
            tab_draw_ms: (tab_ns() - self.tab0) as f64 / 1e6,
            recalcs: calc.full_n,
            recalc_ms: calc.full_ns as f64 / 1e6,
            recalc_max_ms: self.full_log.iter().copied().max().unwrap_or(0) as f64 / 1e6,
            misc_gets: calc.misc_get_n,
            misc_calls: calc.misc_call_n,
            misc_ms: (calc.misc_call_ns + calc.misc_get_ns) as f64 / 1e6,
            allocs: a.allocs.saturating_sub(self.guard_allocs),
            alloc_mb: a.bytes.saturating_sub(self.guard_bytes) as f64 / (1024.0 * 1024.0),
            heap_peak_mb: a.peak as f64 / (1024.0 * 1024.0),
            rss_peak_mb: hwm,
            build_load_ms: self.build_load_ms,
            power_ms: self.power_ms,
        }
    }
}

/// Options of one run.
#[derive(Clone, Debug)]
pub struct RunOptions {
    pub render: RenderMode,
    /// Stop after this step (inspect).
    pub until: Option<String>,
    /// Write the last frame as a PNG (inspect; needs nothing else).
    pub png: Option<PathBuf>,
    /// Print the strings drawn in the last frame with their positions (inspect).
    pub texts: bool,
    /// The parent's `SystemTime` (s since the epoch) just before it spawned this process.
    pub spawn_epoch_s: Option<f64>,
    pub rep: u32,
}

impl Default for RunOptions {
    fn default() -> Self {
        RunOptions {
            render: RenderMode::None,
            until: None,
            png: None,
            texts: false,
            spawn_epoch_s: None,
            rep: 0,
        }
    }
}

/// The state of one run.
pub struct Driver {
    h: Headless,
    real_clock: Rc<Cell<bool>>,
    screen: ScreenInfo,
    cursor_phys: (i32, i32),
    last_cursor: Option<(i32, i32)>,
    dbl: DoubleClick,
    base: PathBuf,
    renderer: Renderer,
    inited: bool,
    t_main: Instant,
    pub startup: Startup,
    saw_first_screen: bool,
    steps: Vec<StepResult>,
    cur: Option<StepAcc>,
    snapshots: HashMap<String, String>,
    stopped: bool,
    peak_rss_mb: f64,
    texts: bool,
    /// The strings of the last frame (inspect `--texts`).
    pub last_texts: Vec<String>,
    perf: PerfCtl,
}

/// `perf record --control fifo:CTL`: sampling is enabled only while a measured step runs
/// (`POB_PROF_PERF_CTL` names the control fifo; `POB_PROF_PERF_STEPS`, a comma list, limits it
/// to those steps). Used by scripts/perf/flamegraph.sh; inert otherwise.
struct PerfCtl {
    fifo: Option<std::fs::File>,
    steps: Option<Vec<String>>,
    on: bool,
}

impl PerfCtl {
    fn from_env() -> PerfCtl {
        let fifo = std::env::var_os("POB_PROF_PERF_CTL").and_then(|p| {
            std::fs::OpenOptions::new()
                .write(true)
                .open(&p)
                .map_err(|e| eprintln!("pob-prof: perf control {}: {e}", p.to_string_lossy()))
                .ok()
        });
        let steps = std::env::var("POB_PROF_PERF_STEPS")
            .ok()
            .map(|s| -> Vec<String> {
                s.split(',')
                    .map(str::trim)
                    .filter(|x| !x.is_empty())
                    .map(str::to_owned)
                    .collect()
            })
            .filter(|v| !v.is_empty());
        PerfCtl {
            fifo,
            steps,
            on: false,
        }
    }

    fn set(&mut self, on: bool) {
        use std::io::Write as _;
        if self.on == on {
            return;
        }
        if let Some(f) = self.fifo.as_mut() {
            let _ = f.write_all(if on { b"enable\n" } else { b"disable\n" });
            let _ = f.flush();
            self.on = on;
        }
    }

    /// The step `name` starts.
    fn step(&mut self, name: &str) {
        let want = !name.starts_with('_')
            && self
                .steps
                .as_ref()
                .map_or(true, |v| v.iter().any(|s| s == name));
        self.set(want);
    }
}

fn model_weak(app: &ProfApp) -> Option<Weak<pob_calc::model::Build>> {
    app.app
        .cx
        .app()
        .build
        .as_ref()
        .map(|b| Rc::downgrade(&b.model))
}

impl Driver {
    fn with_cur<R>(&mut self, f: impl FnOnce(&mut StepAcc) -> R) -> R {
        let c = self.cur.as_mut().expect("a step is open");
        f(c)
    }

    /// Run `f` untimed (guards, snapshots): its time and allocations leave the step.
    fn untimed<R>(&mut self, f: impl FnOnce(&mut Driver) -> R) -> R {
        let t0 = Instant::now();
        let c0 = thread_cpu_s();
        let a0 = alloc::snapshot();
        let r = f(self);
        let a = alloc::snapshot().since(&a0);
        let d = t0.elapsed().as_nanos() as u64;
        let dc = thread_cpu_s() - c0;
        if let Some(c) = self.cur.as_mut() {
            c.guard_ns += d;
            c.guard_cpu_s += dc;
            c.guard_allocs += a.allocs;
            c.guard_bytes += a.bytes;
        }
        r
    }

    fn check_errors(app: &ProfApp) -> Result<()> {
        if let Some(e) = app.errors.first() {
            return Err(anyhow!("Lua error in the app: {e}"));
        }
        Ok(())
    }

    fn ensure_init(&mut self, app: &mut ProfApp) -> Result<()> {
        if self.inited {
            return Ok(());
        }
        self.inited = true;
        let t0 = Instant::now();
        guarded(app, ProfApp::on_init).map_err(|e| anyhow!(e))?;
        self.startup.on_init_ms = ms(t0);
        Self::check_errors(app)
    }

    fn frame(&mut self, app: &mut ProfApp) -> Result<()> {
        self.ensure_init(app)?;
        if self.texts {
            app.sg().trace = Some(TraceRecorder::new());
        }
        let t0 = Instant::now();
        let r = run_frame(app, self.screen, self.cursor_phys);
        let frame_ns = t0.elapsed().as_nanos() as u64;
        self.drain_effects();
        self.h.clock_ms.set(self.h.clock_ms.get() + FRAME_MS);
        let frame = r.map_err(|e| anyhow!(e))?;
        let rns = self.renderer.render(frame);
        if self.texts {
            if let Some(t) = app.sg().trace.take() {
                self.last_texts = drawn_strings(&t, true);
            }
        }
        let total_ms = (frame_ns + rns) as f64 / 1e6;
        if self.startup.first_frame_ms == 0.0 {
            self.startup.first_frame_ms = total_ms;
        }
        if !self.saw_first_screen {
            let mode = app.app.cx.app().main.mode.clone().unwrap_or_default();
            if mode == "LIST" || mode == "BUILD" {
                self.saw_first_screen = true;
                self.startup.to_first_screen_ms = ms(self.t_main);
            }
        }
        // build load detection
        let now_build = model_weak(app);
        let full_n = calc_timing::counters().full_n;
        if let Some(c) = self.cur.as_mut() {
            c.frames.push(total_ms);
            c.render_ns += rns;
            if c.build_load_ms.is_none() {
                if let Some(nb) = &now_build {
                    let new = match &c.build0 {
                        None => true,
                        Some(old) => !Weak::ptr_eq(old, nb),
                    };
                    if new && full_n > c.calc0.full_n {
                        c.build_load_ms = Some(c.elapsed_ms());
                    }
                }
            }
        }
        Self::check_errors(app)
    }

    /// One extra frame with a `TraceRecorder`, for `expect-text`: the strings it draws. It runs
    /// inside the guard's `untimed` and is not one of the step's frames; it is rendered (so the
    /// renderer sees every frame's textures) and advances the virtual clock like any frame.
    fn traced_frame(&mut self, app: &mut ProfApp) -> Result<Vec<String>> {
        self.ensure_init(app)?;
        let had = app.sg().trace.take();
        app.sg().trace = Some(TraceRecorder::new());
        let r = run_frame(app, self.screen, self.cursor_phys);
        let t = app.sg().trace.take();
        app.sg().trace = had;
        self.drain_effects();
        self.h.clock_ms.set(self.h.clock_ms.get() + FRAME_MS);
        let frame = r.map_err(|e| anyhow!(e))?;
        self.renderer.render(frame);
        Self::check_errors(app)?;
        Ok(t.map(|t| drawn_strings(&t, false)).unwrap_or_default())
    }

    /// Count the file writes since the last call (the save guard) and drop the side effects
    /// (only a trace would need them).
    fn drain_effects(&mut self) {
        let writes = {
            let mut e = self.h.effects.borrow_mut();
            let n = e
                .iter()
                .filter(|x| matches!(x, SideEffect::FileWrite { .. }))
                .count() as u64;
            e.retain(|x| matches!(x, SideEffect::Exit));
            n
        };
        if let Some(c) = self.cur.as_mut() {
            c.file_writes += writes;
        }
    }

    fn timed_input(
        &mut self,
        app: &mut ProfApp,
        f: impl FnOnce(&mut ProfApp) -> Result<(), String>,
    ) -> Result<()> {
        self.ensure_init(app)?;
        let t0 = Instant::now();
        let r = f(app);
        let d = t0.elapsed().as_nanos() as u64;
        if let Some(c) = self.cur.as_mut() {
            c.input_ns += d;
        }
        r.map_err(|e| anyhow!(e))?;
        Self::check_errors(app)
    }

    fn fixture(&mut self, app: &mut ProfApp, key: &str, value: &str) -> Result<()> {
        match key {
            "userdir" => {
                let p = self.base.join(value);
                let p = std::fs::canonicalize(&p)
                    .map_err(|e| anyhow!("userdir {}: {e}", p.display()))?;
                *self.h.user_path.borrow_mut() = p.to_string_lossy().replace('\\', "/");
            }
            "net" => {
                let f = load_net_fixtures(&self.base.join(value)).map_err(|e| anyhow!(e))?;
                self.h.set_net_fixtures(f);
            }
            "clock" => match value {
                "real" => self.real_clock.set(true),
                "virtual" => self.real_clock.set(false),
                v => return Err(anyhow!("fixture clock={v:?}: expected real or virtual")),
            },
            _ => {
                let s = Step::Fixture {
                    key: key.to_owned(),
                    value: value.to_owned(),
                };
                guarded(app, |a| a.on_script_step(&s)).map_err(|e| anyhow!(e))?;
            }
        }
        Ok(())
    }

    fn action(&mut self, app: &mut ProfApp, a: &Action) -> Result<()> {
        match a {
            Action::Screen { w, h, dpi } => {
                self.screen = ScreenInfo {
                    fb_w: *w,
                    fb_h: *h,
                    win_h: *h,
                    dpi_scale: *dpi,
                };
            }
            Action::Fixture { key, value } => self.fixture(app, key, value)?,
            Action::Frame => self.frame(app)?,
            Action::Move(x, y) => {
                let o = app.sg().get_dpi_scale_override_percent();
                let s = if o > 0 {
                    f64::from(o) / 100.0
                } else {
                    f64::from(self.screen.dpi_scale)
                };
                self.cursor_phys = ((x * s).round() as i32, (y * s).round() as i32);
                self.last_cursor = Some(self.cursor_phys);
            }
            Action::Input(Input::Key {
                key, down: true, ..
            }) if is_mouse_button(*key) => {
                let dbl = self
                    .dbl
                    .press(*key, self.last_cursor, self.h.clock_ms.get() as f64);
                let key = *key;
                self.timed_input(app, |a| {
                    deliver(
                        a,
                        Input::Key {
                            key,
                            down: true,
                            double_click: dbl,
                        },
                    )
                })?;
            }
            Action::Input(i) => {
                let i = *i;
                self.timed_input(app, |a| deliver(a, i))?;
            }
            Action::PasteClipboard(t) => *self.h.clipboard.borrow_mut() = Some(t.clone()),
            Action::App(Step::Snapshot(name)) => {
                self.ensure_init(app)?;
                let name = name.clone();
                let xml = self.untimed(|_| app.app.snapshot());
                self.snapshots.insert(name, xml.unwrap_or_default());
            }
            Action::App(s) => {
                let s = s.clone();
                self.timed_input(app, |a| guarded(a, |a| a.on_script_step(&s)))?;
            }
            Action::Exit => {
                self.ensure_init(app)?;
                let mut ok = false;
                guarded(app, |a| ok = a.can_exit()).map_err(|e| anyhow!(e))?;
                if ok {
                    guarded(app, ProfApp::on_exit).map_err(|e| anyhow!(e))?;
                    self.stopped = true;
                }
            }
        }
        if self.h.exit_requested.get() || self.h.restart_requested.get() {
            let _ = guarded(app, ProfApp::on_exit);
            self.stopped = true;
        }
        Ok(())
    }

    fn guard(&mut self, app: &mut ProfApp, g: &Guard) -> Result<()> {
        self.drain_effects();
        // expect-text draws its frame before `fail` borrows the step
        let texts = match g {
            Guard::Text(_) | Guard::NoText(_) => self.traced_frame(app)?,
            _ => Vec::new(),
        };
        let fail = |m: String| {
            Err(anyhow!(
                "guard failed in step {:?}: {m}",
                self.cur
                    .as_ref()
                    .map(|c| c.name.clone())
                    .unwrap_or_default()
            ))
        };
        match g {
            Guard::Recalc(n) => {
                let c = self.cur.as_ref().expect("step");
                let got = calc_timing::counters().full_n - c.calc0.full_n;
                if got < *n {
                    return fail(format!("expect-recalc {n}: {got} recalcs"));
                }
            }
            Guard::Misc(n) => {
                let c = self.cur.as_ref().expect("step");
                let got = calc_timing::counters().misc_call_n - c.calc0.misc_call_n;
                if got < *n {
                    return fail(format!("expect-misc {n}: {got} misc calculator calls"));
                }
            }
            Guard::FileWrite(n) => {
                let got = self.cur.as_ref().map_or(0, |c| c.file_writes);
                if got < *n {
                    return fail(format!("expect-file-write {n}: {got} files written"));
                }
            }
            Guard::NoRecalc => {
                let c = self.cur.as_ref().expect("step");
                let got = calc_timing::counters().full_n - c.calc0.full_n;
                if got != 0 {
                    return fail(format!("expect-no-recalc: {got} recalcs"));
                }
            }
            Guard::Mode(m) => {
                let got = app.app.cx.app().main.mode.clone().unwrap_or_default();
                if &got != m {
                    return fail(format!("expect-mode {m}: mode is {got:?}"));
                }
            }
            Guard::Tab(t) => {
                let got = app
                    .app
                    .cx
                    .app()
                    .build
                    .as_ref()
                    .map(|b| b.model.viewMode.get().to_string());
                if got.as_deref() != Some(t.as_str()) {
                    return fail(format!("expect-tab {t}: viewMode is {got:?}"));
                }
            }
            Guard::Power => {
                if self.cur.as_ref().and_then(|c| c.power_ms).is_none() {
                    return fail("expect-power: no power report finished".into());
                }
            }
            Guard::Differs(a, b) | Guard::Same(a, b) => {
                let (Some(x), Some(y)) = (self.snapshots.get(a), self.snapshots.get(b)) else {
                    return fail(format!("snapshot {a:?} or {b:?} missing"));
                };
                let differs = matches!(g, Guard::Differs(..));
                if (x != y) != differs {
                    return fail(format!(
                        "snapshots {a} and {b} are {}",
                        if differs { "equal" } else { "different" }
                    ));
                }
            }
            Guard::Text(want) | Guard::NoText(want) => {
                let found = texts.iter().any(|t| t.contains(want.as_str()));
                let expect = matches!(g, Guard::Text(_));
                if found != expect {
                    let mut seen: Vec<&str> = texts.iter().map(String::as_str).collect();
                    seen.dedup();
                    seen.truncate(40);
                    return fail(format!(
                        "{} {want:?}: {} in the {} strings of a traced frame (first: {seen:?})",
                        if expect {
                            "expect-text"
                        } else {
                            "expect-no-text"
                        },
                        if found { "found" } else { "not found" },
                        texts.len()
                    ));
                }
            }
            Guard::CalcFunc => {
                let ok = app
                    .app
                    .cx
                    .app()
                    .build
                    .as_ref()
                    .is_some_and(|b| b.calc_func.is_some());
                if !ok {
                    return fail("expect-calc-func: no misc calculator".into());
                }
            }
        }
        Ok(())
    }

    fn wait_power(&mut self, app: &mut ProfApp, max: u32) -> Result<()> {
        let t0 = Instant::now();
        let g0 = self.cur.as_ref().map_or(0, |c| c.guard_ns);
        for i in 0..max {
            self.frame(app)?;
            let done = match app.app.cx.app().build.as_ref() {
                Some(b) => {
                    !b.model.calcsTab.powerBuildFlag.get()
                        && !pob_calc::modules::power::powerBuilderRunning(&b.model)
                }
                None => return Err(anyhow!("wait-power: no build open")),
            };
            if done && i > 0 {
                let g = self.cur.as_ref().map_or(0, |c| c.guard_ns) - g0;
                let d = (t0.elapsed().as_nanos() as u64).saturating_sub(g) as f64 / 1e6;
                self.with_cur(|c| c.power_ms = Some(d));
                return Ok(());
            }
        }
        Err(anyhow!("wait-power: the power report did not finish in {max} frames (is the heat map or power report shown?)"))
    }

    fn wait_idle(&mut self, app: &mut ProfApp, max: u32) -> Result<()> {
        for _ in 0..max {
            self.frame(app)?;
            if !pob_host::wants_frames(app) {
                return Ok(());
            }
        }
        Err(anyhow!(
            "wait-idle: background work still running after {max} frames"
        ))
    }

    fn clipboard_code(&mut self, app: &mut ProfApp, path: &str) -> Result<()> {
        self.ensure_init(app)?;
        let user = app.app.cx.app().main.user_path.clone().unwrap_or_default();
        let file = format!("{user}{path}");
        let bytes = app
            .sys()
            .fs
            .read(&file)
            .map_err(|e| anyhow!("clipboard-build-code {file}: {e}"))?;
        let code = pob_app::tabs::import::core::encode_build_code(&String::from_utf8_lossy(&bytes));
        *self.h.clipboard.borrow_mut() = Some(code);
        Ok(())
    }

    fn close_step(&mut self) {
        if self.texts {
            if let Some(c) = &self.cur {
                eprintln!("--- strings drawn in the last frame of step {}:", c.name);
                for t in &self.last_texts {
                    eprintln!("{t}");
                }
            }
        }
        if let Some(c) = self.cur.take() {
            let r = c.finish();
            self.peak_rss_mb = self.peak_rss_mb.max(r.rss_peak_mb);
            self.steps.push(r);
        }
    }

    fn open_step(&mut self, app: &ProfApp, name: &str) {
        self.close_step();
        let (_, hwm) = rss_mb();
        self.peak_rss_mb = self.peak_rss_mb.max(hwm);
        self.perf.step(name);
        self.cur = Some(StepAcc::new(name, model_weak(app)));
    }
}

/// The strings drawn in the last frame of `t`, escapes stripped; `with_pos` prefixes each with
/// its position and height (inspect `--texts`).
fn drawn_strings(t: &TraceRecorder, with_pos: bool) -> Vec<String> {
    t.frames
        .last()
        .map(|f| {
            f.calls
                .iter()
                .filter_map(|c| match &c.call {
                    ApiCall::DrawString {
                        x, y, height, text, ..
                    } => {
                        let s = pob_sg::strip_escapes(text);
                        Some(if with_pos {
                            format!("{x:7.1} {y:7.1} h{height:<4} {s}")
                        } else {
                            s
                        })
                    }
                    _ => None,
                })
                .collect()
        })
        .unwrap_or_default()
}

/// [`stats::quantile`], 0 without samples (JSON has no NaN).
fn q0(v: &[f64], p: f64) -> f64 {
    if v.is_empty() {
        0.0
    } else {
        stats::quantile(v, p)
    }
}

fn ms(t0: Instant) -> f64 {
    t0.elapsed().as_secs_f64() * 1000.0
}

/// Process-wide set-up shared by `exec` and `inspect`: data, timeless jewel files, the panic hook.
pub fn init_process(inputs: &Inputs) -> Result<f64> {
    if std::env::var("POB_PROF_ALLOC_COUNT").as_deref() == Ok("0") {
        alloc::set_counting(false);
    }
    let t0 = Instant::now();
    pob_gamedata::init_from_dir(&inputs.dump)
        .map_err(|e| anyhow!("{}: {e}", inputs.dump.join("data.bin").display()))?;
    let data_ms = ms(t0);
    pob_gamedata::timeless_lut::set_jewel_fs(std::sync::Arc::new(
        pob_gamedata::timeless_lut::StdJewelFs {
            script_path: inputs.src.to_string_lossy().into_owned(),
            writable: false,
        },
    ));
    pob_gamedata::timeless_lut::set_con_printf(|_| {});
    // Lua errors are caught (`guarded`) and reported by the run; do not print them twice.
    let prev = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        if info.payload().downcast_ref::<pob_lua::LuaError>().is_none() {
            prev(info);
        }
    }));
    Ok(data_ms)
}

/// Run one scenario once (the body of `exec`). `t_main` is the process's `main` start;
/// `data_ms` what [`init_process`] took.
pub fn run(
    sc: &Scenario,
    inputs: &Inputs,
    opts: &RunOptions,
    t_main: Instant,
    data_ms: f64,
) -> RepResult {
    let mut res = RepResult {
        scenario: sc.name.clone(),
        rep: opts.rep,
        render: opts.render.name().to_owned(),
        alloc_counting: alloc::active(),
        ..RepResult::default()
    };
    let cpu0 = cpu_s();
    match run_inner(sc, inputs, opts, t_main, data_ms, &mut res) {
        Ok(()) => res.ok = true,
        Err(e) => {
            res.ok = false;
            res.error = Some(format!("{e:#}"));
        }
    }
    res.total_ms = res.steps.iter().map(|s| s.wall_ms).sum();
    res.cpu_ms = (cpu_s() - cpu0) * 1000.0;
    res
}

fn run_inner(
    sc: &Scenario,
    inputs: &Inputs,
    opts: &RunOptions,
    t_main: Instant,
    data_ms: f64,
    res: &mut RepResult,
) -> Result<()> {
    let real_clock = Rc::new(Cell::new(true));
    let t0 = Instant::now();
    let renderer = Renderer::new(opts.render)?;
    res.adapter = renderer.adapter();
    let (parts, h) = platform_parts(inputs, real_clock.clone())?;
    let assets_ms = ms(t0);
    install_tab_timer();
    let t1 = Instant::now();
    let mut app = ProfApp {
        app: PobApp::with_calc(parts, calc_timing::timed_factory()),
        errors: Vec::new(),
    };
    app.app.cx.app_mut().launch.force_release_install = true;
    let app_new_ms = ms(t1);
    let mut d = Driver {
        h,
        real_clock,
        screen: ScreenInfo {
            fb_w: 1920,
            fb_h: 1080,
            win_h: 1080,
            dpi_scale: 1.0,
        },
        cursor_phys: (0, 0),
        last_cursor: None,
        dbl: DoubleClick::default(),
        base: sc.base_dir(),
        renderer,
        inited: false,
        t_main,
        startup: Startup {
            exec_ms: opts.spawn_epoch_s.map(|s| {
                let now = std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .map_or(0.0, |d| d.as_secs_f64());
                // `main` started t_main.elapsed() ago
                ((now - t_main.elapsed().as_secs_f64()) - s) * 1000.0
            }),
            data_ms,
            assets_ms,
            app_new_ms,
            ..Startup::default()
        },
        saw_first_screen: false,
        steps: Vec::new(),
        cur: None,
        snapshots: HashMap::new(),
        stopped: false,
        peak_rss_mb: 0.0,
        texts: opts.texts,
        last_texts: Vec::new(),
        perf: PerfCtl::from_env(),
    };
    if opts.png.is_some() {
        d.renderer.track_textures = true;
    }
    let r = drive(&mut d, &mut app, sc, opts);
    d.perf.set(false);
    d.close_step();
    res.startup = d.startup.clone();
    res.steps = std::mem::take(&mut d.steps);
    let (_, hwm) = rss_mb();
    res.peak_rss_mb = d.peak_rss_mb.max(hwm);
    if let Some(p) = &opts.png {
        d.renderer.snapshot_png(p)?;
    }
    drop(app);
    r
}

fn drive(d: &mut Driver, app: &mut ProfApp, sc: &Scenario, opts: &RunOptions) -> Result<()> {
    let mut leading = true;
    for seg in &sc.segs {
        if d.stopped {
            break;
        }
        if let Seg::Script(steps) = seg {
            let mut acts = Vec::new();
            for s in steps {
                expand_step(s, &mut acts);
            }
            for a in &acts {
                if d.stopped {
                    break;
                }
                let lead_action = matches!(a, Action::Screen { .. } | Action::Fixture { .. });
                if leading && !lead_action {
                    leading = false;
                    if d.cur.is_none() {
                        // OnInit is start-up, not part of the first step
                        d.ensure_init(app)?;
                        d.open_step(app, "_setup");
                    }
                }
                d.action(app, a)?;
            }
            continue;
        }
        if leading {
            leading = false;
            if !matches!(seg, Seg::Step(_)) && d.cur.is_none() {
                d.ensure_init(app)?;
                d.open_step(app, "_setup");
            }
        }
        match seg {
            Seg::Step(name) => {
                if let Some(u) = &opts.until {
                    if d.cur.as_ref().is_some_and(|c| &c.name == u) {
                        return Ok(());
                    }
                }
                d.ensure_init(app)?;
                d.open_step(app, name);
            }
            Seg::Guard(g) => {
                if d.cur.is_none() {
                    d.open_step(app, "_setup");
                }
                let g = g.clone();
                d.untimed(|d| d.guard(app, &g))?;
            }
            Seg::WaitPower(max) => {
                if d.cur.is_none() {
                    d.open_step(app, "_setup");
                }
                d.wait_power(app, *max)?;
            }
            Seg::WaitIdle(max) => {
                if d.cur.is_none() {
                    d.open_step(app, "_setup");
                }
                d.wait_idle(app, *max)?;
            }
            Seg::ClipboardBuildCode(p) => {
                if d.cur.is_none() {
                    d.open_step(app, "_setup");
                }
                let p = p.clone();
                d.untimed(|d| d.clipboard_code(app, &p))?;
            }
            Seg::Script(_) => unreachable!(),
        }
    }
    Ok(())
}

/// The scenario's own step list paired with what ran (for reporting a missing step).
pub fn missing_steps(sc: &Scenario, r: &RepResult) -> Vec<String> {
    let ran: std::collections::BTreeSet<&str> = r.steps.iter().map(|s| s.name.as_str()).collect();
    sc.step_names()
        .into_iter()
        .filter(|n| !ran.contains(n.as_str()))
        .collect()
}
