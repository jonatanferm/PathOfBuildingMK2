//! U26 acceptance (ui doc §12 U26 row): the Tree tab's power report (`TreeTab.lua:1047-1169`
//! SetPowerCalc / BuildPowerReportList, `PowerReportListControl.lua`, the per-frame
//! `calcsTab:BuildPower()` call site) on both sides of the UI oracle. Owner: U26.
//!
//! * `u26_scenarios`: every `tests/u26/*.pobscript` (each sets its own `screen`) through the full
//!   app, as u22-u25: zero T3 divergences per frame and the same window titles. U26 saves no
//!   state (the report is not part of the build XML), so there is no T4.
//!
//! Both sides run the real PowerBuilder (PoB's `CalcsTab:PowerBuilder` coroutine resumed by
//! `CalcsTab:BuildPower`; calc:PWR's iterator stepped by U26's `build_power`), the real
//! `SetPowerCalc`, `BuildPowerReportList`, `PowerReportListControl` (SetReport, ReList, ReSort,
//! rows, tooltips, filter dropdown, masteries checkbox, the row click that focuses the tree) and
//! the real heat map (U23's PassiveTreeView, drawn from the `node.power` the builder wrote). The
//! main calc is U15's LuaCalcBridge (ext:calc, as u20-u25).
//!
//! The misc calculator (`calcsTab:GetMiscCalculator()`, the `calcFunc` the PowerBuilder calls
//! once per node, path and mastery effect) is a deterministic stand-in on both sides: the bridge's
//! `MiscCalc::call` boots a LuaJIT PoB per call (seconds each), far too slow for the ~2000 calls
//! of a report, and the Rust calc is not at parity yet (calc:PWR `pwr_l6_report`). The stand-in
//! returns a copy of a fixed base output plus, for every node in `addNodes` (minus for every node
//! in `removeNodes`), integer amounts of `Life`/`LifeUnreserved` and `CombinedDPS` computed from
//! the node's `modKey` (closure addresses normalised, as calc:PWR's L5 test does): its length
//! squared plus a byte hash. It depends on the `modKey` only, as PoB's per-`modKey` cache requires
//! (which node of a shared `modKey` fills the cache follows PoB's per-process `pairs` order), and
//! its sums are exact, so both sides compute the same powers (calc:PWR's statement order and
//! float ops are under test here, the stand-in is not). A `modKey` that several spec nodes share,
//! and every mastery effect, gets a small amount instead: the report's top rows then have distinct
//! powers, so their order does not depend on the per-process order of tied entries (power.rs
//! ORDER note).
//!
//! Stand-ins for the units not under test (both sides run the same ones, as u25): the node
//! tooltips (U24) are two lines, the other tabs' `Draw` draw nothing, `utf8.next` (as u22).
//!
//! The virtual clock is constant within a frame, so neither side's builder yields on time
//! (`GetTime() - start > 100` never holds): the first resume yields at CalcsTab.lua:514-516, the
//! second runs to the end and calls `powerBuilderCallback`. No progress toast appears in a
//! scenario; the progress path is covered by `u26_progress_steps` (Rust only, a clock that
//! advances on every read).
//!
//! T3 multiset buckets: as u23 (sub-layers 0..=30 of layer 0, the tree's `pairs` loops).
//! The report's rows are compared in order. Rows of equal power keep PoB's per-process `pairs`
//! order (power.rs module notes); the scenarios only show rows whose order is determined (distinct
//! powers, or ties whose rows draw the same text).
//!
//! Compared side effects exclude `ImageLoad` (as u23).

use std::cell::{Cell, RefCell};
use std::path::{Path, PathBuf};
use std::rc::Rc;

use pob_app::app::CxApp;
use pob_app::build::init;
use pob_app::build::on_frame as of;
use pob_app::tree_view;
use pob_calc::lua_table::Table;
use pob_host::{App, PlatformParts};
use pob_sg::trace::{ApiCall, Buckets, DiffConfig, Divergence, DrawOp, FrameTrace, SideEffect};
use pob_sg::{Key, Sg, Sys};
use pob_ui_oracle::boot::{self, Entry, OracleHandles};
use pob_ui_oracle::diff_report;
use pob_ui_oracle::lua_calc::LuaCalcBridge;
use pob_ui_oracle::oracle_cache::{self, LuaRun};
use pob_ui_oracle::runner::{self, Driver, LuaApp, RunResult, ScenarioApp, UserDir};

fn inputs() -> Option<PathBuf> {
    let root = pob_testpaths::require(pob_testpaths::pob_root(), "PoB checkout (.PathOfBuilding)")?;
    let dump = pob_testpaths::require(pob_testpaths::dump_dir(), "data/dump")?;
    assert!(pob_oracle::testkit::init_data(&dump), "data dump");
    let src = std::fs::canonicalize(root.join("src")).unwrap_or_else(|_| root.join("src"));
    pob_gamedata::timeless_lut::set_jewel_fs(std::sync::Arc::new(
        pob_gamedata::timeless_lut::StdJewelFs {
            script_path: src.to_string_lossy().into_owned(),
            writable: false,
        },
    ));
    Some(root)
}

fn test_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/u26")
}

// ---- the stand-ins (module notes) ----

/// The stand-in misc calculator's base output.
const BASE: [(&str, f64); 8] = [
    ("Life", 4000.0),
    ("LifeUnreserved", 4000.0),
    ("Armour", 5000.0),
    ("Evasion", 2000.0),
    ("EnergyShield", 0.0),
    ("LifeRegenRecovery", 100.0),
    ("EnergyShieldRegenRecovery", 0.0),
    ("CombinedDPS", 100000.0),
];

/// The Lua stand-ins (module notes).
const LUA_STUBS: &str = r#"
local lutf8 = require("lua-utf8")
if not lutf8.next then
	local function iscont(s, p)
		local b = s:byte(p + 1) or 0
		return b >= 0x80 and b < 0xC0
	end
	lutf8.next = function(s, i, off)
		local e = #s
		local p = i - 1
		local idx = off
		if idx >= 0 then
			while p < e and idx > 0 do
				while p < e and iscont(s, p + 1) do
					p = p + 1
				end
				p = (p < e) and p + 1 or e
				idx = idx - 1
			end
		else
			while 0 < p and idx < 0 do
				local q = p
				while 0 < q and iscont(s, q - 1) do
					q = q - 1
				end
				p = (0 < q) and q - 1 or 0
				idx = idx + 1
			end
		end
		if idx ~= 0 or p == e then
			return
		end
		return p + 1, s:byte(p + 1)
	end
end
local function keyOf(node)
	local key = (node.modKey or ""):gsub("function: %w+", "function")
	return key
end
local keyCounts = { }
local function standinDelta(node)
	local key = keyOf(node)
	local len, h = #key, 0
	for i = 1, #key do
		h = (h * 31 + key:byte(i)) % 997
	end
	if node.type == "Mastery" or (keyCounts[key] or 0) > 1 then
		return h % 50 + 1, (h * 7) % 13 + 1
	end
	return len * len + h, len * 7 + (h * 13) % 101
end
local b = main.modes.BUILD
local origInit = b.Init
function b:Init(...)
	origInit(self, ...)
	for _, t in ipairs({ "importTab", "partyTab", "configTab", "skillsTab", "itemsTab", "calcsTab", "compareTab" }) do
		if self[t] then self[t].Draw = function() end end
	end
	local cls = common.classes.PassiveTreeView
	if cls then
		cls.AddNodeTooltip = function(view, tooltip, node, build)
			tooltip:AddLine(16, "^7" .. node.dn)
			tooltip:AddLine(14, "^7" .. (node.alloc and "allocated" or "unallocated") .. " " .. node.type)
		end
		cls.AddCompareNodeTooltip = function(view, tooltip, node, build)
			tooltip:AddLine(16, "^2" .. node.dn)
			tooltip:AddLine(14, "^7compare " .. node.type)
		end
	end
	if self.calcsTab then
		self.calcsTab.GetMiscCalculator = function(ct)
			keyCounts = { }
			for _, node in pairs(ct.build.spec.nodes) do
				local key = keyOf(node)
				keyCounts[key] = (keyCounts[key] or 0) + 1
			end
			local base = { Life = 4000, LifeUnreserved = 4000, Armour = 5000, Evasion = 2000, EnergyShield = 0,
				LifeRegenRecovery = 100, EnergyShieldRegenRecovery = 0, CombinedDPS = 100000 }
			local function calcFunc(override, useFullDPS)
				local out = copyTable(base)
				for node in pairs(override.addNodes or { }) do
					local life, dps = standinDelta(node)
					out.Life = out.Life + life
					out.LifeUnreserved = out.LifeUnreserved + life
					out.CombinedDPS = out.CombinedDPS + dps
				end
				for node in pairs(override.removeNodes or { }) do
					local life, dps = standinDelta(node)
					out.Life = out.Life - life
					out.LifeUnreserved = out.LifeUnreserved - life
					out.CombinedDPS = out.CombinedDPS - dps
				end
				return out
			end
			return calcFunc, base
		end
	end
end
"#;

/// `standinDelta(node)` (LUA_STUBS).
fn standin_delta(node: &pob_calc::classes::Node) -> (f64, f64) {
    let key = norm_mod_key(&node.modKey.get());
    let (len, mut h) = (key.len() as u64, 0u64);
    for &b in key.as_bytes() {
        h = (h * 31 + u64::from(b)) % 997;
    }
    let shared = KEY_COUNTS.with(|c| c.borrow().get(&key).copied().unwrap_or(0) > 1);
    if node.r#type.get() == pob_lua::n!("Mastery") || shared {
        return ((h % 50 + 1) as f64, ((h * 7) % 13 + 1) as f64);
    }
    ((len * len + h) as f64, (len * 7 + (h * 13) % 101) as f64)
}

thread_local! {
    /// `keyCounts` (LUA_STUBS): how many spec nodes carry each normalised modKey.
    static KEY_COUNTS: RefCell<std::collections::HashMap<String, usize>> = RefCell::new(std::collections::HashMap::new());
}

/// `keyCounts` over `build.spec.nodes`, as the Lua stand-in's `GetMiscCalculator` counts them.
fn count_keys(build: &pob_calc::model::BuildRef) {
    let mut m = std::collections::HashMap::new();
    for (_, node) in build.spec.get().nodes.with(pob_lua::LjIntMap::pairs) {
        *m.entry(norm_mod_key(&node.modKey.get())).or_insert(0) += 1;
    }
    KEY_COUNTS.with(|c| *c.borrow_mut() = m);
}

/// `modKey:gsub("function: %w+", "function")`: a closure's `tostring` is an address in PoB
/// (`function: 0x…`, per process) and `function: 0x` plus a hash of the closure's spec in the
/// port (`mod_tools::function_text`, FUP2; as calc:PWR's L5 test).
fn norm_mod_key(k: &str) -> String {
    let mut out = String::with_capacity(k.len());
    let mut rest = k;
    while let Some(i) = rest.find("function: ") {
        let after = &rest[i + "function: ".len()..];
        let end = after.find(|c: char| !c.is_ascii_alphanumeric()).unwrap_or(after.len());
        if end == 0 {
            out.push_str(&rest[..i + "function: ".len()]);
            rest = after;
            continue;
        }
        out.push_str(&rest[..i]);
        out.push_str("function");
        rest = &after[end..];
    }
    out.push_str(rest);
    out
}

fn base_output() -> Table {
    let t = Table::new();
    for (k, v) in BASE {
        t.set(pob_lua::Name::from(k), v);
    }
    t
}

/// The stand-in `calcFunc(override)` (LUA_STUBS) as calc:PWR's replay seam.
fn install_standin_calc() {
    pob_calc::modules::power::calc_replay::set(Some(Rc::new(
        |ov: &pob_calc::env::Override, _ufd: Option<bool>| {
            let out = base_output();
            let apply = |set: Option<pob_lua::Shared<pob_lua::LjIntMap<pob_calc::classes::NodeRef>>>, sign: f64| {
                for (_, node) in set.map(|s| s.with(pob_lua::LjIntMap::pairs)).unwrap_or_default() {
                    let (life, dps) = standin_delta(&node);
                    out.set(pob_lua::n!("Life"), out.num(pob_lua::n!("Life")) + sign * life);
                    out.set(
                        pob_lua::n!("LifeUnreserved"),
                        out.num(pob_lua::n!("LifeUnreserved")) + sign * life,
                    );
                    out.set(pob_lua::n!("CombinedDPS"), out.num(pob_lua::n!("CombinedDPS")) + sign * dps);
                }
            };
            apply(ov.addNodes.get(), 1.0);
            apply(ov.removeNodes.get(), -1.0);
            Some(out)
        },
    )));
}

fn rust_stubs() {
    install_standin_calc();
    init::set_hooks(|h| *h = init::default_hooks());
    of::set_shell_hooks(|h| *h = of::default_shell_hooks());
    pob_app::tabs::tree::core::set_tree_tab_hooks(|h| {
        *h = pob_app::tabs::tree::core::default_tree_tab_hooks();
    });
    tree_view::set_hooks(|h| {
        *h = tree_view::default_hooks();
        h.add_node_tooltip = Rc::new(|cx, _v, tt, node| {
            tt.add_line(cx, 16.0, &format!("^7{}", node.dn.get()));
            tt.add_line(
                cx,
                14.0,
                &format!(
                    "^7{} {}",
                    if node.alloc.get() { "allocated" } else { "unallocated" },
                    node.r#type.get()
                ),
            );
        });
        h.add_compare_node_tooltip = Rc::new(|cx, _v, tt, node| {
            tt.add_line(cx, 16.0, &format!("^2{}", node.dn.get()));
            tt.add_line(cx, 14.0, &format!("^7compare {}", node.r#type.get()));
        });
        // `calcsTab:GetMiscCalculator()` returns the stand-in (the bridge sets no
        // `calcsTab.miscCalculator`), then U26's real call site
        let real = h.build_power.clone();
        h.build_power = Rc::new(move |cx| {
            let m = cx.build().model.clone();
            let f = Rc::new(pob_calc::modules::calcs::MiscCalculator {
                build: m.clone(),
                usedFullDPS: Cell::new(false),
            });
            pob_calc::modules::power::set_miscCalculator(&m, Some((f, base_output())));
            count_keys(&m);
            real(cx);
        });
    });
}

// ---- the Lua side ----

struct LuaSide {
    app: LuaApp,
}

impl App for LuaSide {
    fn sg(&mut self) -> &mut Sg {
        self.app.sg()
    }
    fn sys(&mut self) -> &mut Sys {
        self.app.sys()
    }
    fn on_init(&mut self) {
        self.app.on_init();
        self.app
            .with_vm(|pob| pob.eval::<()>(LUA_STUBS))
            .unwrap_or_else(|e| panic!("Lua stubs: {e}"));
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
    fn on_lua_error(&mut self, msg: String) {
        self.app.on_lua_error(msg);
    }
}

impl ScenarioApp for LuaSide {
    fn load_build(&mut self, name: &str, xml: &str) -> anyhow::Result<()> {
        self.app.load_build(name, xml)
    }
    fn snapshot(&mut self) -> anyhow::Result<Option<String>> {
        self.app.snapshot()
    }
    fn after_frame(&mut self) -> anyhow::Result<()> {
        self.app.after_frame()
    }
    fn take_error(&mut self) -> Option<String> {
        self.app.take_error()
    }
}

// ---- the Rust side ----

/// Records the platform side effects the Lua side's bridge records (as in u16-u25).
struct RecordingPlatform {
    inner: Box<dyn pob_sg::Platform>,
    effects: Rc<RefCell<Vec<SideEffect>>>,
}

impl pob_sg::Platform for RecordingPlatform {
    fn time_ms(&self) -> i64 {
        self.inner.time_ms()
    }
    fn copy(&mut self, text: &str) {
        self.effects.borrow_mut().push(SideEffect::Copy(text.to_owned()));
        self.inner.copy(text);
    }
    fn paste(&mut self) -> Option<String> {
        self.inner.paste()
    }
    fn open_url(&mut self, url: &str) {
        self.effects.borrow_mut().push(SideEffect::OpenUrl(url.to_owned()));
        self.inner.open_url(url);
    }
    fn set_window_title(&mut self, title: &str) {
        self.effects
            .borrow_mut()
            .push(SideEffect::WindowTitle(title.to_owned()));
        self.inner.set_window_title(title);
    }
    fn take_screenshot(&mut self) {
        self.effects.borrow_mut().push(SideEffect::Screenshot);
        self.inner.take_screenshot();
    }
    fn restart(&mut self) {
        self.inner.restart();
    }
    fn exit(&mut self, msg: Option<&str>) {
        self.effects.borrow_mut().push(SideEffect::Exit);
        self.inner.exit(msg);
    }
    fn script_path(&self) -> String {
        self.inner.script_path()
    }
    fn runtime_path(&self) -> String {
        self.inner.runtime_path()
    }
    fn user_path(&self) -> Result<String, (String, String)> {
        self.inner.user_path()
    }
    fn con_print(&mut self, s: &str) {
        self.inner.con_print(s);
    }
}

struct NullPlatform;
impl pob_sg::Platform for NullPlatform {
    fn time_ms(&self) -> i64 {
        0
    }
    fn copy(&mut self, _: &str) {}
    fn paste(&mut self) -> Option<String> {
        None
    }
    fn open_url(&mut self, _: &str) {}
    fn set_window_title(&mut self, _: &str) {}
    fn take_screenshot(&mut self) {}
    fn restart(&mut self) {}
    fn exit(&mut self, _: Option<&str>) {}
    fn script_path(&self) -> String {
        String::new()
    }
    fn runtime_path(&self) -> String {
        String::new()
    }
    fn user_path(&self) -> Result<String, (String, String)> {
        Ok(String::new())
    }
    fn con_print(&mut self, _: &str) {}
}

struct RustSide {
    app: pob_app::PobApp,
    effects: Rc<RefCell<Vec<SideEffect>>>,
}

impl RustSide {
    fn new(mut parts: PlatformParts, src: PathBuf) -> RustSide {
        let effects = Rc::new(RefCell::new(Vec::new()));
        let inner = std::mem::replace(&mut parts.sys.platform, Box::new(NullPlatform));
        parts.sys.platform = Box::new(RecordingPlatform {
            inner,
            effects: effects.clone(),
        });
        let mut app = pob_app::PobApp::with_calc(
            parts,
            Rc::new(move || Box::new(LuaCalcBridge::new(Some(src.clone())))),
        );
        effects.borrow_mut().clear();
        app.cx.app_mut().launch.force_release_install = true;
        RustSide { app, effects }
    }
    fn flush(&mut self) {
        let e: Vec<_> = self.effects.borrow_mut().drain(..).collect();
        if let Some(t) = self.app.cx.sg.trace.as_mut() {
            for e in e {
                t.effect(e);
            }
        }
    }
}

impl App for RustSide {
    fn sg(&mut self) -> &mut Sg {
        self.app.sg()
    }
    fn sys(&mut self) -> &mut Sys {
        self.app.sys()
    }
    fn on_init(&mut self) {
        rust_stubs();
        self.app.on_init();
        self.flush();
    }
    fn on_frame(&mut self) {
        self.app.on_frame();
        self.flush();
    }
    fn on_key_down(&mut self, key: Key, double_click: bool) {
        self.app.on_key_down(key, double_click);
        self.flush();
    }
    fn on_key_up(&mut self, key: Key) {
        self.app.on_key_up(key);
        self.flush();
    }
    fn on_char(&mut self, ch: Key) {
        self.app.on_char(ch);
        self.flush();
    }
    fn on_lua_error(&mut self, msg: String) {
        self.app.on_lua_error(msg);
    }
}

impl ScenarioApp for RustSide {
    fn load_build(&mut self, name: &str, xml: &str) -> anyhow::Result<()> {
        self.app.load_build(name, xml);
        Ok(())
    }
    fn snapshot(&mut self) -> anyhow::Result<Option<String>> {
        Ok(self.app.snapshot())
    }
    fn after_frame(&mut self) -> anyhow::Result<()> {
        if let Some(p) = self
            .app
            .cx
            .app()
            .launch
            .prompt_msg
            .clone()
            .filter(|p| p.starts_with("^1Error"))
        {
            anyhow::bail!("the error prompt (launch:ShowErrMsg): {p}");
        }
        Ok(())
    }
}

// ---- runs ----

fn drop_image_loads(frames: &mut [FrameTrace]) {
    for f in frames {
        f.effects.retain(|e| !matches!(e, SideEffect::ImageLoad(_)));
    }
}

/// The Lua side of one run through the Lua-side cache (TP4, as u41): PoB boot, `script` and what
/// the test keeps of the VM and the user dir afterwards (`extra`, JSON). The key covers this file
/// (stubs and Lua-side wrapper), the Lua-side code, PoB, this test's scripts and fixtures, the
/// fixture user dir, the script, the screen and the other arguments.
fn lua_run(
    root: &Path,
    script: &pob_host::script::Script,
    user: &UserDir,
    fixture: Option<&Path>,
    screen: pob_sg::ScreenInfo,
) -> anyhow::Result<LuaRun> {
    let scripts = test_dir();
    let mut inputs: Vec<&Path> = vec![&scripts];
    inputs.extend(fixture);
    let key = oracle_cache::ui_key(
        "ui-u26",
        pob_ui_oracle::test_source!(),
        root,
        &inputs,
        &[&format!("{script:?}"), &format!("{screen:?}")],
    );
    oracle_cache::cached_lua_run(key, || {
        let handles = OracleHandles::default();
        let lua_app = runner::lua_side(root, user, &handles, Entry::Launch, false)?;
        let mut lua_side = LuaSide { app: lua_app };
        let mut d = Driver::new(screen, handles, user.path.clone());
        let (mut frames, r) = runner::drive(&mut lua_side, script, &mut d);
        drop(lua_side);
        drop_image_loads(&mut frames);
        Ok(LuaRun {
            frames,
            error: r.err().map(|e| e.to_string()),
            snapshots: Vec::new(),
            extra: Vec::new(),
        })
    })
}

struct Side {
    frames: Vec<FrameTrace>,
    error: Option<String>,
}

fn run_sides(
    root: &Path,
    script: &pob_host::script::Script,
    fixture: Option<&Path>,
    screen: pob_sg::ScreenInfo,
) -> anyhow::Result<(Side, Side)> {
    let user = UserDir::new(fixture)?;

    // Lua (cached, TP4)
    let run = lua_run(root, script, &user, fixture, screen)?;
    let lua = Side {
        frames: run.frames,
        error: run.error,
    };

    // Rust
    user.reset()?;
    let handles = OracleHandles::default();
    let (sg, sys) = boot::oracle_parts(root, &user.path, &handles)?;
    let src = std::fs::canonicalize(root.join("src")).unwrap_or_else(|_| root.join("src"));
    let mut rust_side = RustSide::new(PlatformParts { sg, sys }, src);
    let mut d = Driver::new(screen, handles, user.path.clone());
    let res = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        runner::drive(&mut rust_side, script, &mut d)
    }));
    let (mut frames, error) = match res {
        Ok((frames, r)) => (frames, r.err().map(|e| e.to_string())),
        Err(p) => (
            Vec::new(),
            Some(format!("panicked: {}", pob_host::panic_message(&*p))),
        ),
    };
    drop(rust_side);
    pob_calc::modules::power::calc_replay::set(None);
    drop_image_loads(&mut frames);
    Ok((lua, Side { frames, error }))
}

fn out_dir() -> PathBuf {
    std::env::var_os("CARGO_TARGET_DIR")
        .map_or_else(
            || PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../target"),
            PathBuf::from,
        )
        .join("ui-parity")
}

fn draw_ops(frames: &[FrameTrace]) -> usize {
    frames
        .iter()
        .flat_map(|f| &f.calls)
        .filter(|c| {
            matches!(
                c.call,
                ApiCall::DrawImage { .. } | ApiCall::DrawImageQuad { .. } | ApiCall::DrawString { .. }
            )
        })
        .count()
}

/// Draw ops inside the power report list (its control path), and how many of them are row text.
fn report_ops(frames: &[FrameTrace]) -> usize {
    frames
        .iter()
        .flat_map(|f| &f.calls)
        .filter(|c| {
            matches!(c.call, ApiCall::DrawString { .. })
                && c.ctl.as_deref().is_some_and(|p| p.contains("powerReportList"))
        })
        .count()
}

fn window_titles(frames: &[FrameTrace]) -> Vec<String> {
    frames
        .iter()
        .flat_map(|f| &f.effects)
        .filter_map(|e| match e {
            SideEffect::WindowTitle(t) => Some(t.clone()),
            _ => None,
        })
        .collect()
}

/// The multiset sub-layers (module notes).
fn multiset_bucket(k: (i32, i32)) -> bool {
    k.0 == 0 && (0..=30).contains(&k.1)
}

/// A sort key of an op (as u25).
fn op_key(op: &DrawOp) -> String {
    let r = |x: f64| format!("{:.3}", x);
    let rs = |v: &[f64]| v.iter().map(|x| r(*x)).collect::<Vec<_>>().join(",");
    let call = match &op.call {
        ApiCall::DrawImage { img, rect, tc } => format!(
            "I {:?} {} {}",
            img.as_ref().map(|i| (&i.path, i.flags)),
            rs(rect),
            tc.as_ref().map_or_else(String::new, |t| rs(t))
        ),
        ApiCall::DrawImageQuad { img, p, tc } => format!(
            "Q {:?} {} {}",
            img.as_ref().map(|i| (&i.path, i.flags)),
            rs(p),
            tc.as_ref().map_or_else(String::new, |t| rs(t))
        ),
        ApiCall::DrawString {
            x,
            y,
            align,
            height,
            font,
            text,
        } => format!("S {} {} {align:?} {} {font:?} {text}", r(*x), r(*y), r(*height)),
        other => format!("{other:?}"),
    };
    format!("{:?} {} {call}", op.color, rs(&op.viewport))
}

/// One frame's divergences (as u25).
fn diff_frame(lua: &FrameTrace, rust: &FrameTrace) -> Vec<Divergence> {
    let screen = lua
        .screen
        .or(rust.screen)
        .unwrap_or_else(|| runner::screen(1280, 800, 1.0));
    let sort = |mut b: Buckets| {
        for (k, ops) in &mut b {
            if multiset_bucket(*k) {
                ops.sort_by_cached_key(op_key);
            }
        }
        b
    };
    let lb = sort(pob_sg::trace::normalize(lua, &screen));
    let rb = sort(pob_sg::trace::normalize(rust, &screen));
    let ordered = DiffConfig::default();
    let mut out = pob_sg::trace::diff(&lb, &rb, &ordered);
    if out.iter().any(|d| match d {
        Divergence::Missing { bucket, .. }
        | Divergence::Extra { bucket, .. }
        | Divergence::Changed { bucket, .. } => multiset_bucket(*bucket),
        Divergence::Effect { .. } => false,
    }) {
        let cfg = DiffConfig {
            multiset: vec![((0, 0), (0, 30))],
            ..DiffConfig::default()
        };
        out = pob_sg::trace::diff(&lb, &rb, &cfg);
    }
    out.extend(pob_sg::trace::diff_effects(&lua.effects, &rust.effects));
    out
}

fn diff_run(lua: &[FrameTrace], rust: &[FrameTrace]) -> Vec<(usize, Vec<Divergence>)> {
    let empty = FrameTrace::default();
    let mut out = Vec::new();
    for i in 0..lua.len().max(rust.len()) {
        let (a, b) = (lua.get(i).unwrap_or(&empty), rust.get(i).unwrap_or(&empty));
        let d = diff_frame(a, b);
        if !d.is_empty() || i >= lua.len() || i >= rust.len() {
            out.push((i, d));
        }
    }
    out
}

fn describe(d: &Divergence) -> String {
    let op = |o: &DrawOp| {
        format!(
            "{:?} color {:?} vp {:?} ({:?} {:?})",
            o.call, o.color, o.viewport, o.src, o.ctl
        )
    };
    match d {
        Divergence::Missing { bucket, idx, op: o } => {
            format!("missing on rust {bucket:?}#{idx}: {}", op(o))
        }
        Divergence::Extra { bucket, idx, op: o } => {
            format!("extra on rust {bucket:?}#{idx}: {}", op(o))
        }
        Divergence::Changed {
            bucket,
            idx,
            lua,
            rust,
        } => format!("changed {bucket:?}#{idx}: lua {} | rust {}", op(lua), op(rust)),
        Divergence::Effect { lua, rust } => format!("effect: lua {lua:?} rust {rust:?}"),
    }
}

/// One case's outcome (TP4: cases run on `oracle_cache::par_map`; collected in case order).
#[derive(Default)]
struct CaseOut {
    lines: Vec<String>,
    failures: Vec<String>,
    ran: usize,
    total_ops: usize,
    total_report_ops: usize,
}

#[test]
fn u26_scenarios() {
    let Some(root) = inputs() else { return };
    let mut scripts: Vec<PathBuf> = std::fs::read_dir(test_dir())
        .expect("tests/u26")
        .filter_map(Result::ok)
        .map(|e| e.path())
        .filter(|p| p.extension().is_some_and(|x| x == "pobscript"))
        .collect();
    scripts.sort();
    assert!(!scripts.is_empty(), "no tests/u26/*.pobscript");
    let filter = std::env::var("U26_FILTER").ok();
    let mut cases = Vec::new();
    for path in scripts {
        let stem = path.file_stem().unwrap().to_string_lossy().into_owned();
        if filter.as_ref().is_some_and(|f| !stem.contains(f.as_str())) {
            continue;
        }
        cases.push((path.clone(), stem.clone()));
    }
    let results = oracle_cache::par_map(cases, |(path, stem)| {
        let mut out = CaseOut::default();
        let (script, base_dir) = runner::load_script(&path).unwrap();
        let fx = runner::fixtures(&script, &base_dir).unwrap();
        let tag = format!("u26_{stem}");
        let (lua, rust) = match run_sides(&root, &script, fx.user_dir.as_deref(), runner::screen(1280, 800, 1.0)) {
            Ok(s) => s,
            Err(e) => {
                out.failures.push(format!("{tag}: {e:#}"));
                return out;
            }
        };
        out.ran += 1;
        let titles = (window_titles(&lua.frames), window_titles(&rust.frames));
        let divs = diff_run(&lua.frames, &rust.frames);
        let n: usize = divs.iter().map(|(_, d)| d.len().max(1)).sum();
        let run = RunResult {
            lua: lua.frames,
            rust: rust.frames,
            rust_error: rust.error.clone(),
            lua_snapshots: Vec::new(),
            rust_snapshots: Vec::new(),
            audit: Vec::new(),
            compared: true,
        };
        let _ = diff_report::write_report(&out_dir(), &tag, &run, false);
        // `U26_VERBOSE=N`: the first N divergences of the first divergent frames (authoring aid)
        if let Some(nv) = std::env::var("U26_VERBOSE").ok().and_then(|v| v.parse::<usize>().ok()) {
            for (fi, d) in divs.iter().take(3) {
                out.lines
                    .push(format!("{tag} frame {fi}: {} divergences", d.len()));
                for x in d.iter().take(nv) {
                    out.lines.push(format!("  {}", describe(x)));
                }
            }
        }
        let first = divs.first().map(|(fi, d)| {
            format!(
                "frame {fi}: {}",
                d.first().map_or_else(|| "frame missing".to_owned(), describe)
            )
        });
        let ops = draw_ops(&run.lua);
        let rops = report_ops(&run.lua);
        out.total_ops += ops;
        out.total_report_ops += rops;
        out.lines.push(format!(
            "{tag}: {} frames, {ops} Lua draw ops ({rops} report-list strings), {n} divergences{}{}{}",
            run.lua.len(),
            lua.error
                .as_ref()
                .map_or_else(String::new, |e| format!(", Lua error: {e}")),
            rust.error
                .as_ref()
                .map_or_else(String::new, |e| format!(", Rust error: {e}")),
            first.as_ref().map_or_else(String::new, |f| format!(", first: {f}")),
        ));
        if n != 0
            || rust.error.is_some()
            || lua.error.is_some()
            || run.lua.len() != run.rust.len()
            || ops == 0
            || titles.0 != titles.1
        {
            out.failures.push(format!(
                "{tag}: {n} divergences ({}), frames lua {} rust {}, {ops} ops, errors lua {:?} rust {:?}",
                first.unwrap_or_default(),
                run.lua.len(),
                run.rust.len(),
                lua.error,
                rust.error,
            ));
        }

        out
    });
    let mut failures = Vec::new();
    let (mut ran, mut total_ops, mut total_report_ops) = (0, 0, 0);
    for case in results {
        for l in &case.lines {
            println!("{l}");
        }
        failures.extend(case.failures);
        ran += case.ran;
        total_ops += case.total_ops;
        total_report_ops += case.total_report_ops;
    }
    println!("u26: {ran} runs, {total_ops} Lua draw ops, {total_report_ops} report-list strings");
    println!("{}", oracle_cache::summary());
    assert!(ran > 0 || filter.is_some(), "no run");
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

/// Scenario authoring aid (not an acceptance test): `U26_PROBE=Builds/<file>` loads that build of
/// `tests/u26/user` on the Rust side, runs `U26_PROBE_STEPS` (`;`-separated script steps) at
/// `U26_SCREEN=WxH@S` (default 1280x800@1), and prints the power-report controls' rects, the
/// report list's label and its first rows.
#[test]
#[ignore = "authoring aid"]
fn u26_probe() {
    let Some(root) = inputs() else { return };
    let Ok(build) = std::env::var("U26_PROBE") else { return };
    let scr = std::env::var("U26_SCREEN").unwrap_or_else(|_| "1280x800@1".to_owned());
    let (wh, s) = scr.split_once('@').unwrap();
    let (w, h) = wh.split_once('x').unwrap();
    let (w, h, s): (i32, i32, f32) = (w.parse().unwrap(), h.parse().unwrap(), s.parse().unwrap());
    let steps = std::env::var("U26_PROBE_STEPS").unwrap_or_default().replace(';', "\n");
    let script =
        runner::parse_script(&format!("frames 2\nload-build \"{build}\"\nframes 2\n{steps}\nframes 1\n"))
            .unwrap();
    let user = UserDir::new(Some(&test_dir().join("user"))).unwrap();
    let handles = OracleHandles::default();
    let (sg, sys) = boot::oracle_parts(&root, &user.path, &handles).unwrap();
    let src = std::fs::canonicalize(root.join("src")).unwrap_or_else(|_| root.join("src"));
    let mut rust_side = RustSide::new(PlatformParts { sg, sys }, src);
    let mut d = Driver::new(runner::screen(w, h, s), handles, user.path.clone());
    let (_, r) = runner::drive(&mut rust_side, &script, &mut d);
    r.unwrap();
    let cx = &rust_side.app.cx;
    let tab = pob_app::tabs::tree::core::tab(cx).expect("treeTab");
    let ctl = |k: &'static str| pob_app::tabs::tree::core::ctl(cx, tab, k);
    let show = |name: &str, id: pob_ui_core::prelude::CtrlId| {
        let (x, y) = cx.pos(id);
        let (cw, ch) = cx.size(id);
        println!("control {name}: shown={} x={x} y={y} w={cw} h={ch}", cx.is_shown(id));
    };
    for k in ["treeHeatMap", "nodePowerMaxDepthSelect", "treeHeatMapStatSelect", "powerReport", "powerReportList"] {
        show(k, ctl(k));
    }
    let list = ctl("powerReportList");
    for k in ["filterSelect", "masteryCheck"] {
        show(k, cx.child(list, k).unwrap());
    }
    let l = cx.w::<pob_ui_core::controls::ListControl>(list);
    println!(
        "label {:?}",
        l.label.as_ref().map(|p| p.get(cx, list))
    );
    for (i, c) in l.col_list.iter().enumerate() {
        println!("col {} offset {:?} width {:?}", i + 1, c.offset, c.computed_width);
    }
    let n = pob_ui_core::controls::ListControl::list_len(cx, list);
    println!("rows {n}");
    let clusters: Vec<(String, f64)> = (1..=n)
        .filter_map(|i| pob_ui_core::controls::ListControl::list_get(cx, list, i))
        .filter_map(|v| v.rc::<pob_app::tabs::tree::power_report_list::ReportEntry>())
        .filter(|e| e.pathDist == pob_app::tabs::tree::power_report_list::PathDist::Str("Cluster"))
        .map(|e| (e.name.clone(), e.power))
        .collect();
    println!("cluster rows {} (first {:?})", clusters.len(), clusters.first());
    for i in 1..=n.min(40) {
        let v = pob_ui_core::controls::ListControl::list_get(cx, list, i).unwrap();
        let e = v.rc::<pob_app::tabs::tree::power_report_list::ReportEntry>().unwrap();
        println!(
            "row {i}: {} {:?} power {} {:?} path {:?} {:?} alloc {} xy {:?},{:?}",
            e.r#type, e.name, e.power, e.powerStr, e.pathDist, e.pathPowerStr, e.allocated, e.x, e.y
        );
    }
}

/// A platform whose clock passes `step` ms on every read (`GetTime()` inside one resume).
struct TickingPlatform {
    inner: Box<dyn pob_sg::Platform>,
    now: Cell<i64>,
    step: i64,
}

impl pob_sg::Platform for TickingPlatform {
    fn time_ms(&self) -> i64 {
        self.now.set(self.now.get() + self.step);
        self.now.get()
    }
    fn copy(&mut self, text: &str) {
        self.inner.copy(text);
    }
    fn paste(&mut self) -> Option<String> {
        self.inner.paste()
    }
    fn open_url(&mut self, url: &str) {
        self.inner.open_url(url);
    }
    fn set_window_title(&mut self, title: &str) {
        self.inner.set_window_title(title);
    }
    fn take_screenshot(&mut self) {
        self.inner.take_screenshot();
    }
    fn restart(&mut self) {
        self.inner.restart();
    }
    fn exit(&mut self, msg: Option<&str>) {
        self.inner.exit(msg);
    }
    fn script_path(&self) -> String {
        self.inner.script_path()
    }
    fn runtime_path(&self) -> String {
        self.inner.runtime_path()
    }
    fn user_path(&self) -> Result<String, (String, String)> {
        self.inner.user_path()
    }
    fn con_print(&mut self, s: &str) {
        self.inner.con_print(s);
    }
}

/// The progress path (Rust only, no oracle): with a clock that passes 60 ms per read, calc:PWR
/// yields on time, so `build_power` calls U25's `powerBuilderProgressCallback` through
/// `co::with_cx` on the steps before the last (the list label, the progress toast) and
/// `powerBuilderCallback` once at the end (the report rows, the toast removed); the pacer
/// coroutine is alive exactly while `calcsTab.powerBuilder` is set.
#[test]
fn u26_progress_steps() {
    // TP4: the process-wide case pool (shared with the `par_map` cases) instead of the lock
    let _permit = oracle_cache::Permit::acquire();
    let Some(root) = inputs() else { return };
    let script = runner::parse_script(
        "screen 1280 800\nframes 2\nload-build \"Builds/tree_3_27.xml\"\nframes 3\n",
    )
    .unwrap();
    let user = UserDir::new(Some(&test_dir().join("user"))).unwrap();
    let handles = OracleHandles::default();
    let (sg, sys) = boot::oracle_parts(&root, &user.path, &handles).unwrap();
    let src = std::fs::canonicalize(root.join("src")).unwrap_or_else(|_| root.join("src"));
    let mut rust_side = RustSide::new(PlatformParts { sg, sys }, src);
    let mut d = Driver::new(runner::screen(1280, 800, 1.0), handles, user.path.clone());
    let (_, r) = runner::drive(&mut rust_side, &script, &mut d);
    r.unwrap();
    let cx = &mut rust_side.app.cx;
    let inner = std::mem::replace(&mut cx.sys.platform, Box::new(NullPlatform));
    cx.sys.platform = Box::new(TickingPlatform {
        inner,
        now: Cell::new(1_000_000),
        step: 60,
    });
    let tab = pob_app::tabs::tree::core::tab(cx).expect("treeTab");
    let list = pob_app::tabs::tree::core::ctl(cx, tab, "powerReportList");
    let m = cx.build().model.clone();
    // the Life report on the stand-in calculator, started as the stat dropdown starts it
    let life = pob_calc::modules::power::powerStat_by_stat("Life").expect("Life");
    pob_app::tabs::tree::TreeTab::SetPowerCalc(cx, tab, Some(life));
    let f = Rc::new(pob_calc::modules::calcs::MiscCalculator {
        build: m.clone(),
        usedFullDPS: Cell::new(false),
    });
    pob_calc::modules::power::set_miscCalculator(&m, Some((f, base_output())));
    count_keys(&m);
    let label = |cx: &pob_ui_core::prelude::Cx| {
        cx.w::<pob_ui_core::controls::ListControl>(list)
            .label
            .as_ref()
            .map(|p| p.get(cx, list))
            .unwrap_or_default()
    };
    assert_eq!(label(cx), "Building Tree...");
    let mut steps = 0;
    let mut progress_labels: Vec<String> = Vec::new();
    let mut toast_seen = false;
    loop {
        pob_app::tabs::tree::power::build_power(cx);
        steps += 1;
        let running = pob_calc::modules::power::powerBuilderRunning(&m);
        let pacer_alive = !cx.w::<pob_app::tabs::tree::TreeTab>(tab).power.pacer.is_dead();
        assert_eq!(running, pacer_alive, "the pacer is alive while the report runs (step {steps})");
        if running {
            assert!(pob_ui_core::coroutine::co::any_alive(), "step {steps}");
        }
        if !running {
            break;
        }
        let l = label(cx);
        if l.starts_with("Building Power Report") && progress_labels.last() != Some(&l) {
            progress_labels.push(l);
        }
        toast_seen |= cx
            .w::<pob_app::tabs::tree::TreeTab>(tab)
            .core
            .power_builder_toast_id
            .is_some();
        assert!(steps < 100_000, "the report never ends");
    }
    let rows = pob_ui_core::controls::ListControl::list_len(cx, list);
    let toast_after = cx
        .w::<pob_app::tabs::tree::TreeTab>(tab)
        .core
        .power_builder_toast_id
        .clone();
    let final_label = label(cx);
    pob_calc::modules::power::calc_replay::set(None);
    println!(
        "u26_progress_steps: {steps} steps, {} distinct progress labels (first {:?}, last {:?}), toast seen {toast_seen}, {rows} rows, label {final_label:?}",
        progress_labels.len(),
        progress_labels.first(),
        progress_labels.last(),
    );
    assert!(steps > 2, "the report yielded on time");
    assert!(!progress_labels.is_empty(), "progress labels");
    assert!(toast_seen, "the progress toast");
    assert!(toast_after.is_none(), "the completion callback removes the toast");
    assert_eq!(final_label, "Click to focus node on tree");
    assert!(rows > 0, "report rows");
}
