//! U40 acceptance (ui doc §12 U40 row): CalcBreakdownControl (CalcBreakdownControl.lua 1-789)
//! on both sides of the UI oracle: breakdown panes beside hovered and pinned Calcs cells, inside a
//! pinned overlay pane, and beside the sidebar stat lines; the text, radius, generic, slot and
//! modifier tables; cell tooltips, the node viewer frame and the scroll bar.
//! Owner: U40.
//!
//! `u40_scenarios`: every `tests/u40/*.pobscript`; a script without its own `screen` runs at
//! 1280x800 @1.0 and 1920x1080 @1.5, a script with one runs once. Per run: zero T3 divergences
//! (the same draw ops per `(layer, sub)` bucket and the same side effects, frame by frame), the
//! same window titles, T4 of the `Calcs` section for every `snapshot` step, and every
//! `#! u40-expect-text TEXT` line: some Lua frame draws a string containing TEXT (guards a
//! script whose hover silently stopped showing a breakdown, which both sides would agree on).
//!
//! The Rust side's calc is U15's LuaCalcBridge (ext:calc), so the breakdown data and the mod
//! stores are PoB's (injected). Real on both sides: Launch, Main, Init, OnFrame, the stat box,
//! the Calcs tab (U39) and both CalcBreakdownControls. Stand-ins (both sides run the same ones)
//! for the units not ported yet:
//! * the other tabs' `Draw` draw nothing (as in u39);
//! * `ItemsTab:AddItemTooltip` (U38; an item row's source tooltip) adds one line with the item
//!   name;
//! * the breakdowns' `nodeViewer` (PassiveTreeView, U23) is `{ Draw = function() end }`: no tree
//!   drawing and no `highlightRing` (L640 then draws an untextured ring quad on both sides);
//! * `SkillsTab:AddSocketGroupTooltip`, `utf8.next`, `spec.modFlag` and `TreeTab:GetSpecList`
//!   as in u39.
//!
//! Compared side effects exclude `ImageLoad`.
//!
//! `U40_FILTER=<substring>` selects scripts; `U40_PNG=1` also writes the frames as PNGs.

use std::cell::RefCell;
use std::path::{Path, PathBuf};
use std::rc::Rc;

use pob_app::app::CxApp;
use pob_app::build::init;
use pob_app::build::on_frame::{self as of, TabFlag};
use pob_calc::model::{XmlChild, XmlNode};
use pob_host::{App, PlatformParts};
use pob_sg::trace::{FrameTrace, SideEffect};
use pob_sg::{Key, Sg, Sys};
use pob_ui_oracle::boot::{self, Entry, OracleHandles};
use pob_ui_oracle::diff_report;
use pob_ui_oracle::lua_calc::LuaCalcBridge;
use pob_ui_oracle::oracle_cache::{self, LuaRun};
use pob_ui_oracle::runner::{self, Driver, LuaApp, RunResult, ScenarioApp, UserDir};

const SCREENS: [(i32, i32, f32); 2] = [(1280, 800, 1.0), (1920, 1080, 1.5)];

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
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/u40")
}

/// The Lua stand-ins (see the module notes).
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
local b = main.modes.BUILD
local origInit = b.Init
function b:Init(...)
	origInit(self, ...)
	for _, t in ipairs({ "importTab", "partyTab", "configTab", "treeTab", "skillsTab", "itemsTab", "compareTab" }) do
		if self[t] then self[t].Draw = function() end end
	end
	if self.skillsTab then
		self.skillsTab.AddSocketGroupTooltip = function(_, tooltip, socketGroup)
			tooltip:AddLine(16, "^7" .. tostring(socketGroup.label))
		end
	end
	if self.itemsTab then
		self.itemsTab.AddItemTooltip = function(_, tooltip, item, slot)
			tooltip:AddLine(16, "^7" .. tostring(item.name))
		end
	end
	for _, bd in ipairs({ self.calcsTab and self.calcsTab.controls.breakdown, self.controls.breakdown }) do
		bd.nodeViewer = { Draw = function() end }
	end
end
"#;

thread_local! {
    /// The stand-in `spec.modFlag` (UndoHandler.lua:37 via `spec:AddUndoState`, U25).
    static SPEC_MOD_FLAG: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
}

fn rust_stubs() {
    SPEC_MOD_FLAG.with(|f| f.set(false));
    init::set_hooks(|h| {
        *h = init::default_hooks();
        h.spec_add_undo_state = Rc::new(|_| SPEC_MOD_FLAG.with(|f| f.set(true)));
        // U30 stand-in (see the module notes)
        h.skills_add_socket_group_tooltip = Rc::new(|cx, tooltip, group| {
            let label = group.label.get().to_string();
            tooltip.add_line(cx, 16.0, &format!("^7{label}"));
        });
        // U25 stand-in: TreeTab.lua:492-498, as in u20/u22
        h.tree_get_spec_list = Rc::new(|cx| {
            let m = &cx.build().model;
            let gv = &m.data.gameVersions;
            m.treeTab
                .specList
                .with(Clone::clone)
                .iter()
                .map(|spec| {
                    let tv = spec.treeVersion.get();
                    let prefix = if tv == gv.latestTreeVersion {
                        String::new()
                    } else {
                        format!("[{}] ", gv.treeVersions[&tv].display)
                    };
                    let title = spec
                        .title
                        .get()
                        .map_or_else(|| "Default".to_owned(), |s| s.to_string());
                    format!("{prefix}{title}")
                })
                .collect()
        });
    });
    of::set_shell_hooks(|h| {
        *h = of::default_shell_hooks();
        // LUA_STUBS replaces `treeTab.Draw`: the Tree tab (U25) draws nothing here as well
        let all_tabs = h.draw_tab.clone();
        h.draw_tab = Rc::new(move |cx, tab, vp, ev| {
            if tab != of::BuildTab::Tree {
                all_tabs(cx, tab, vp, ev);
            }
        });
        // keep the default arms (Notes, Calcs) and add the spec stand-in
        let (flag, set_flag) = (h.tab_flag.clone(), h.set_tab_flag.clone());
        h.tab_flag = Rc::new(move |cx, f| {
            if f == TabFlag::Spec {
                SPEC_MOD_FLAG.with(std::cell::Cell::get)
            } else {
                flag(cx, f)
            }
        });
        h.set_tab_flag = Rc::new(move |cx, f, v| {
            if f == TabFlag::Spec {
                SPEC_MOD_FLAG.with(|c| c.set(v));
            } else {
                set_flag(cx, f, v);
            }
        });
    });
    pob_app::tabs::calcs::core::set_calcs_hooks(|h| {
        *h = pob_app::tabs::calcs::core::default_calcs_hooks();
    });
    // U38 stand-in (see the module notes)
    pob_app::tabs::calcs::breakdown::set_breakdown_hooks(|h| {
        *h = pob_app::tabs::calcs::breakdown::default_breakdown_hooks();
        h.add_item_tooltip = Rc::new(|cx, tooltip, item, _slot| {
            tooltip.add_line(cx, 16.0, &format!("^7{}", item.name.get()));
        });
        // the Lua side's `nodeViewer = { Draw = function() end }` (no `highlightRing`); the
        // defaults are U23's viewer since the cut-over (CCR-0145 §5)
        h.node_viewer_draw = Rc::new(|_, _, _, _, _, _| {});
        h.node_viewer_highlight_ring = Rc::new(|_, _| None);
    });
}

/// `#! NAME ARG` directives.
fn directives(text: &str, name: &str) -> Vec<String> {
    let tag = format!("#! {name}");
    text.lines()
        .filter_map(|l| l.trim().strip_prefix(tag.as_str()))
        .map(|s| s.trim().to_owned())
        .collect()
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

/// Records the platform side effects the Lua side's bridge records (the oracle's Rust platform
/// does not, CCR-0028 item 2); as in u16-u22.
struct RecordingPlatform {
    inner: Box<dyn pob_sg::Platform>,
    effects: Rc<RefCell<Vec<SideEffect>>>,
}

impl pob_sg::Platform for RecordingPlatform {
    fn time_ms(&self) -> i64 {
        self.inner.time_ms()
    }
    fn copy(&mut self, text: &str) {
        self.effects
            .borrow_mut()
            .push(SideEffect::Copy(text.to_owned()));
        self.inner.copy(text);
    }
    fn paste(&mut self) -> Option<String> {
        self.inner.paste()
    }
    fn open_url(&mut self, url: &str) {
        self.effects
            .borrow_mut()
            .push(SideEffect::OpenUrl(url.to_owned()));
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
        effects.borrow_mut().clear(); // Launch.lua:11 runs at script load, before tracing
                                      // pob-ui-oracle patch 4's equivalent (CCR-0091)
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
        // the Lua side fails the run on PoB's error prompt; so does this one
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

struct Side {
    frames: Vec<FrameTrace>,
    error: Option<String>,
    snapshots: Vec<(String, String)>,
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
        "ui-u40",
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
            snapshots: std::mem::take(&mut d.snapshots),
            extra: Vec::new(),
        })
    })
}

/// Run `script` on both sides with `fixture` as the user dir (fresh copies per side). The Lua
/// VM is dropped before the Rust side starts (memory).
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
        snapshots: run.snapshots,
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
    drop_image_loads(&mut frames);
    let rust = Side {
        frames,
        error,
        snapshots: std::mem::take(&mut d.snapshots),
    };
    Ok((lua, rust))
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
    use pob_sg::trace::ApiCall;
    frames
        .iter()
        .flat_map(|f| &f.calls)
        .filter(|c| {
            matches!(
                c.call,
                ApiCall::DrawImage { .. }
                    | ApiCall::DrawImageQuad { .. }
                    | ApiCall::DrawString { .. }
            )
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

fn distinct_titles(t: &[String]) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for s in t {
        if out.last() != Some(s) {
            out.push(s.clone());
        }
    }
    out
}

// ---- T4: the `Calcs` section ----

fn root_element(text: &str) -> Result<XmlNode, String> {
    let root = pob_calc::model::xml::ParseXML_root(text)?;
    root.children
        .into_iter()
        .find_map(|c| match c {
            XmlChild::Node(n) => Some(n),
            XmlChild::Text(_) => None,
        })
        .ok_or_else(|| "no root element".to_owned())
}

/// The `<Calcs>` element's children as a sorted multiset of `elem {attributes}`.
fn calcs_section(xml: &str) -> Result<Vec<String>, String> {
    let root = root_element(xml)?;
    let calcs = root
        .children
        .iter()
        .find_map(|c| match c {
            XmlChild::Node(n) if &*n.elem == "Calcs" => Some(n),
            _ => None,
        })
        .ok_or_else(|| "no Calcs section".to_owned())?;
    let mut out: Vec<String> = calcs
        .children
        .iter()
        .filter_map(|c| match c {
            XmlChild::Node(n) => {
                let mut a: Vec<String> =
                    n.attrib.iter().map(|(k, v)| format!("{k}={v:?}")).collect();
                a.sort();
                Some(format!("{} {{{}}}", n.elem, a.join(" ")))
            }
            XmlChild::Text(_) => None,
        })
        .collect();
    out.sort();
    Ok(out)
}

/// Every snapshot pair's `Calcs` section; `Ok(pairs)`.
fn t4(lua: &Side, rust: &Side) -> Result<usize, Vec<String>> {
    let mut errs = Vec::new();
    if lua.snapshots.len() != rust.snapshots.len() {
        errs.push(format!(
            "snapshots: lua {} rust {}",
            lua.snapshots.len(),
            rust.snapshots.len()
        ));
    }
    let mut n = 0;
    for ((name, lx), (_, rx)) in lua.snapshots.iter().zip(&rust.snapshots) {
        n += 1;
        match (calcs_section(lx), calcs_section(rx)) {
            (Ok(l), Ok(r)) => {
                if l != r {
                    let d = l.iter().zip(&r).find(|(a, b)| a != b).map_or_else(
                        || format!("lengths {} vs {}", l.len(), r.len()),
                        |(a, b)| format!("lua {a} | rust {b}"),
                    );
                    errs.push(format!("snapshot {name}: Calcs differs: {d}"));
                }
            }
            (l, r) => errs.push(format!(
                "snapshot {name}: lua {:?} rust {:?}",
                l.err(),
                r.err()
            )),
        }
    }
    if errs.is_empty() {
        Ok(n)
    } else {
        Err(errs)
    }
}

/// `#! u39-expect-calcs TEXT` (a Calcs section entry of some snapshot) and `#! u40-expect-text
/// TEXT` (a string some Lua frame draws).
fn check_expectations(text: &str, lua: &Side) -> Vec<String> {
    use pob_sg::trace::ApiCall;
    let mut errs = Vec::new();
    for want in directives(text, "u39-expect-calcs") {
        let found = lua.snapshots.iter().any(|(_, x)| {
            calcs_section(x).is_ok_and(|s| s.iter().any(|e| e.contains(want.as_str())))
        });
        if !found {
            errs.push(format!("no Lua Calcs section entry contains {want:?}"));
        }
    }
    for want in directives(text, "u40-expect-text") {
        let found = lua.frames.iter().flat_map(|f| &f.calls).any(
            |c| matches!(&c.call, ApiCall::DrawString { text, .. } if text.contains(want.as_str())),
        );
        if !found {
            errs.push(format!("no Lua frame draws a string containing {want:?}"));
        }
    }
    errs
}

/// One case's outcome (TP4: cases run on `oracle_cache::par_map`; collected in case order).
#[derive(Default)]
struct CaseOut {
    lines: Vec<String>,
    failures: Vec<String>,
    ran: usize,
    t4_pairs: usize,
}

#[test]
fn u40_scenarios() {
    let Some(root) = inputs() else { return };
    let mut scripts: Vec<PathBuf> = std::fs::read_dir(test_dir())
        .expect("tests/u40")
        .filter_map(Result::ok)
        .map(|e| e.path())
        .filter(|p| p.extension().is_some_and(|x| x == "pobscript"))
        .collect();
    scripts.sort();
    assert!(!scripts.is_empty(), "no tests/u40/*.pobscript");
    let filter = std::env::var("U40_FILTER").ok();
    let png = std::env::var_os("U40_PNG").is_some();
    let mut cases = Vec::new();
    for path in scripts {
        let stem = path.file_stem().unwrap().to_string_lossy().into_owned();
        if filter.as_ref().is_some_and(|f| !stem.contains(f.as_str())) {
            continue;
        }
        let (script, _) = runner::load_script(&path).unwrap();
        let own_screen = matches!(
            script.steps.first(),
            Some(pob_host::script::Step::Screen { .. })
        );
        let screens: Vec<(i32, i32, f32)> = if own_screen {
            vec![(1280, 800, 1.0)]
        } else {
            SCREENS.to_vec()
        };
        for (w, h, s) in screens {
            cases.push((path.clone(), stem.clone(), own_screen, (w, h, s)));
        }
    }
    let results = oracle_cache::par_map(cases, |(path, stem, own_screen, (w, h, s))| {
        let mut out = CaseOut::default();
        let text = std::fs::read_to_string(&path).unwrap();
        let (script, base_dir) = runner::load_script(&path).unwrap();
        let fx = runner::fixtures(&script, &base_dir).unwrap();
        {
            let tag = if own_screen {
                format!("u40_{stem}")
            } else {
                format!("u40_{stem}@{w}x{h}@{s}")
            };
            let (lua, rust) = match run_sides(
                &root,
                &script,
                fx.user_dir.as_deref(),
                runner::screen(w, h, s),
            ) {
                Ok(s) => s,
                Err(e) => {
                    out.failures.push(format!("{tag}: {e:#}"));
                    return out;
                }
            };
            out.ran += 1;
            let titles = (window_titles(&lua.frames), window_titles(&rust.frames));
            let t4r = t4(&lua, &rust);
            let expect = check_expectations(&text, &lua);
            let run = RunResult {
                lua: lua.frames,
                rust: rust.frames,
                rust_error: rust.error.clone(),
                lua_snapshots: lua.snapshots.clone(),
                rust_snapshots: rust.snapshots.clone(),
                audit: Vec::new(),
                compared: true,
            };
            let n = diff_report::write_report(&out_dir(), &tag, &run, png).unwrap_or(usize::MAX);
            let ops = draw_ops(&run.lua);
            let t4_line = match &t4r {
                Ok(p) => {
                    out.t4_pairs += p;
                    format!("T4 {p} Calcs sections identical")
                }
                Err(e) => format!("T4 FAILED: {}", e.join("; ")),
            };
            out.lines.push(format!(
                "{tag}: {} frames, {ops} Lua draw ops, {n} divergences, {t4_line}, titles {:?}{}{}{}",
                run.lua.len(),
                distinct_titles(&titles.0),
                lua.error
                    .as_ref()
                    .map_or_else(String::new, |e| format!(", Lua error: {e}")),
                rust.error
                    .as_ref()
                    .map_or_else(String::new, |e| format!(", Rust error: {e}")),
                if expect.is_empty() {
                    String::new()
                } else {
                    format!(", expectations: {}", expect.join("; "))
                }
            ));
            if n != 0
                || rust.error.is_some()
                || lua.error.is_some()
                || run.lua.len() != run.rust.len()
                || ops == 0
                || titles.0 != titles.1
                || t4r.is_err()
                || !expect.is_empty()
            {
                out.failures.push(format!(
                    "{tag}: {n} divergences, frames lua {} rust {}, {ops} ops, {t4_line}, titles lua {:?} rust {:?}, errors lua {:?} rust {:?}, expectations {expect:?} (see {})",
                    run.lua.len(),
                    run.rust.len(),
                    distinct_titles(&titles.0),
                    distinct_titles(&titles.1),
                    lua.error,
                    rust.error,
                    out_dir().join(&tag).join("report.txt").display()
                ));
            }
        }
        out
    });
    let mut failures = Vec::new();
    let mut ran = 0;
    let mut t4_pairs = 0;
    for case in results {
        for l in &case.lines {
            println!("{l}");
        }
        failures.extend(case.failures);
        ran += case.ran;
        t4_pairs += case.t4_pairs;
    }
    println!("u40: {ran} runs, T4 {t4_pairs} Calcs sections");
    println!("{}", oracle_cache::summary());
    assert!(ran > 0 || filter.is_some(), "no run");
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

// ---- u40_format_helpers ---------------------------------------------------------------------

/// The pure helpers against PoB's own functions in one Lua VM: `FormatModName`,
/// `FormatVarNameOrList`, `FormatModBase`, `FormatModValue` (CalcBreakdownControl.lua 529-569) and
/// Common.lua's `formatRound` / `formatPercent` / `formatSec` (836-871), over hand-picked inputs
/// (case, digits, colon names, signs, fractions, huge values, nil decimals).
#[test]
fn u40_format_helpers() {
    use pob_app::model::{Table, Val};
    use pob_app::tabs::calcs::breakdown::{
        formatPercent, formatRound, formatSec, CalcBreakdownControl as B,
    };
    // TP4: the process-wide case pool (shared with the `par_map` cases) instead of the lock
    let _permit = oracle_cache::Permit::acquire();
    let Some(root) = inputs() else { return };
    let names = [
        "Life",
        "LifeRegenPercent",
        "EnemyChillEffect",
        "Multiplier:PowerCharge",
        "PerStat",
        "ColdDamage2",
        "a1B",
        "ElementalResistByScorch",
        "Damage:Fire",
        "x",
        "ABC",
        "aB:cD2e",
    ];
    let nums = [10.0, -10.0, 0.0, 2.5, -0.25, 1e20, 123.456, 1.0 / 3.0];
    let types = ["BASE", "INC", "MORE", "OVERRIDE", "MAX"];
    let rounds: [(f64, Option<f64>); 6] = [
        (1234.56, Some(1.0)),
        (1.005, Some(2.0)),
        (-2.5, None),
        (2.5, None),
        (0.123456, Some(3.0)),
        (1e15 + 0.3, Some(1.0)),
    ];
    let lnum = |x: f64| format!("({})", pob_lua::lfmt!("%.17g", x));
    let lopt = |d: Option<f64>| d.map_or_else(|| "nil".to_owned(), lnum);
    let s = |v: &Val| match v {
        Val::Str(s) => s.to_string(),
        Val::Num(x) => pob_lua::tostring_num(*x),
        Val::Bool(b) => b.to_string(),
        Val::Nil => "nil".to_owned(),
        other => format!("{other:?}"),
    };

    // the Lua program: one line per case
    let mut lua = String::from(
        "if not common.classes.CalcBreakdownControl then LoadModule('Classes/CalcBreakdownControl') end\n\
         local C = common.classes.CalcBreakdownControl\nlocal out = { }\n\
         local function add(v) out[#out + 1] = tostring(v) end\n",
    );
    let mut rust: Vec<String> = Vec::new();
    for n in names {
        lua.push_str(&format!("add((C:FormatModName({n:?})))\n"));
        rust.push(B::format_mod_name(n));
    }
    let list = Table::new();
    list.push("Foo");
    list.push("BarBaz");
    lua.push_str(
        "add(C:FormatVarNameOrList(nil, { 'Foo', 'BarBaz' }))\nadd(C:FormatVarNameOrList('PowerCharge', nil))\n",
    );
    rust.push(B::format_var_name_or_list(&Val::Nil, &Val::Table(list)));
    rust.push(B::format_var_name_or_list(
        &Val::from("PowerCharge"),
        &Val::Nil,
    ));
    for t in ["BASE", "INC"] {
        for x in nums {
            lua.push_str(&format!(
                "add(C:FormatModBase({{ type = {t:?} }}, {}))\n",
                lnum(x)
            ));
            let m = Table::new();
            m.set(pob_lua::n!("type"), t);
            rust.push(B::format_mod_base(&Val::Table(m), x));
        }
    }
    for t in types {
        for x in nums {
            lua.push_str(&format!("add(C:FormatModValue({}, {t:?}))\n", lnum(x)));
            rust.push(s(&B::format_mod_value(&Val::Num(x), &Val::from(t))));
        }
    }
    lua.push_str("add(C:FormatModValue(true, 'FLAG'))\nadd(C:FormatModValue(nil, 'FLAG'))\n");
    rust.push(s(&B::format_mod_value(
        &Val::Bool(true),
        &Val::from("FLAG"),
    )));
    rust.push(s(&B::format_mod_value(&Val::Nil, &Val::from("FLAG"))));
    lua.push_str(
        "add(C:FormatModValue({ mod = { name = 'FooBar2' } }, 'LIST'))\nadd(C:FormatModValue({ }, 'LIST'))\n",
    );
    let inner = Table::new();
    inner.set(pob_lua::n!("name"), "FooBar2");
    let lv = Table::new();
    lv.set(pob_lua::n!("mod"), inner);
    rust.push(s(&B::format_mod_value(&Val::Table(lv), &Val::from("LIST"))));
    rust.push(s(&B::format_mod_value(
        &Val::Table(Table::new()),
        &Val::from("LIST"),
    )));
    for (v, d) in rounds {
        lua.push_str(&format!(
            "add(formatRound({v}, {d}))\nadd(formatPercent({v}, {d}))\nadd(formatSec({v}, {d}))\n",
            v = lnum(v),
            d = lopt(d)
        ));
        rust.push(pob_lua::tostring_num(formatRound(v, d)));
        rust.push(formatPercent(Some(v), d));
        rust.push(formatSec(v, d));
    }
    lua.push_str("add(formatPercent(nil, nil))\nadd(formatSec(0, 2))\n");
    rust.push(formatPercent(None, None));
    rust.push(formatSec(0.0, Some(2.0)));
    lua.push_str("return table.concat(out, '\\n')\n");

    let user = UserDir::new(None).expect("user dir");
    let handles = OracleHandles::default();
    let mut app = runner::lua_side(&root, &user, &handles, Entry::Launch, false).expect("Lua side");
    app.on_init();
    let got: String = app
        .with_vm(|pob| pob.eval::<String>(&lua))
        .unwrap_or_else(|e| panic!("Lua helpers: {e}"));
    let got: Vec<&str> = got.split('\n').collect();
    assert_eq!(got.len(), rust.len(), "case count");
    let mut bad = Vec::new();
    for (i, (l, r)) in got.iter().zip(&rust).enumerate() {
        if *l != r.as_str() {
            bad.push(format!("case {i}: lua {l:?} rust {r:?}"));
        }
    }
    println!(
        "u40_format_helpers: {} cases, {} mismatches",
        rust.len(),
        bad.len()
    );
    assert!(bad.is_empty(), "{}", bad.join("\n"));
}
