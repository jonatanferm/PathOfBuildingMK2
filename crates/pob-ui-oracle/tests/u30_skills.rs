//! U30 acceptance (ui doc §12 U30 row): the Skills tab (SkillsTab.lua 1-742, 1081-1502) on both
//! sides of the UI oracle: socket group create (paste) and update (label, slot, enabled, Full
//! DPS), skill sets (the set selector, the manager popup), the gem options, paste and undo/redo.
//! Delete, Copy (`OnSelCopy`) and the gem rows are U31's list/row controls (stand-ins here).
//! Owner: U30.
//!
//! `u30_scenarios`: every `tests/u30/*.pobscript`; a script without its own `screen` runs at
//! 1280x800 @1.0 and 1920x1080 @1.5. Each script loads a build (`load-build`), switches to the
//! Skills tab (Ctrl+2) and drives it. Per run: zero T3 divergences (the same draw ops per
//! `(layer, sub)` bucket and the same side effects, frame by frame), the same window titles, and
//! T4 for the `Skills` section of every `snapshot` step (every element of the section in document
//! order with its attributes; see [`mask`] for the written masks, and `flatten` for the gem
//! variant PoB picks through `pairs`, not compared as in u20).
//!
//! The Rust side's calc is U15's LuaCalcBridge (ext:calc, ui doc §12.1), as in u20-u22/u39.
//!
//! Real on both sides: Launch, Main, Init, OnFrame, the stat box, RefreshSkillSelectControls
//! (U16-U21), the Calcs tab (U39), GemSelectControl (U29, the imbued-support selector) and the
//! Skills tab. Stand-ins (both sides run the same ones) for the units not ported yet:
//! * U31's `SkillListControl` (`controls.groupList`) and `SkillSetListControl` (the manager
//!   popup's list): shown, drawing nothing, never under the mouse, ignoring keys (the Rust
//!   default `tabs::skills::core::ListStandIn`; the Lua side replaces the classes' `Draw`,
//!   `IsMouseOver`, `OnKeyDown`, `OnKeyUp`);
//! * U31's `CreateGemSlot`: the row's controls exist with their geometry but are never shown
//!   (the Rust default stand-in; the Lua side sets `shown = false` on each control of the row
//!   after the real CreateGemSlot), so the gem name/level/... headers anchored to row 1 are
//!   hidden on both sides;
//! * U38/U34's `itemsTab:GetSocketDescriptionLine(item)` (`#item.sockets .. " sockets"`),
//!   `itemsTab:AddItemTooltip(tooltip, item)` (one line with the item's name) and
//!   `itemsTab:AddUndoState()` (nothing);
//! * the other tabs' `Draw` draw nothing; the Calcs tab's breakdown, `utf8.next`, `spec.modFlag`
//!   and `TreeTab:GetSpecList` as in u39.
//!
//! Compared side effects exclude `ImageLoad` (the Lua tabs' constructors load images; U23/U31
//! port them).
//!
//! `U30_FILTER=<substring>` selects scripts; `U30_PNG=1` also writes the frames as PNGs
//! (`target/ui-parity/<tag>/frame_*`).

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
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/u30")
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
local function cls(name)
	if not common.classes[name] then
		LoadModule("Classes/" .. name)
	end
	return common.classes[name]
end
local ST = cls("SkillsTab")
local origCreateGemSlot = ST.CreateGemSlot
function ST:CreateGemSlot(index)
	origCreateGemSlot(self, index)
	local slot = self.gemSlots[index]
	for _, k in ipairs({ "delete", "nameSpec", "level", "quality", "enabled", "count", "errMsg", "enableGlobal1", "enableGlobal2" }) do
		slot[k].shown = false
	end
end
for _, name in ipairs({ "SkillListControl", "SkillSetListControl" }) do
	local c = cls(name)
	c.Draw = function() end
	c.IsMouseOver = function() end
	c.OnKeyDown = function() end
	c.OnKeyUp = function() end
end
local b = main.modes.BUILD
local origInit = b.Init
function b:Init(...)
	origInit(self, ...)
	for _, t in ipairs({ "importTab", "partyTab", "configTab", "treeTab", "itemsTab", "compareTab" }) do
		if self[t] then self[t].Draw = function() end end
	end
	if self.itemsTab then
		self.itemsTab.GetSocketDescriptionLine = function(_, item)
			return #item.sockets .. " sockets"
		end
		self.itemsTab.AddItemTooltip = function(_, tooltip, item)
			tooltip:AddLine(16, "^7" .. item.name)
		end
		self.itemsTab.AddUndoState = function() end
	end
	local bd = self.calcsTab and self.calcsTab.controls.breakdown
	if bd then
		bd.Draw = function() end
		bd.IsMouseOver = function() end
		bd.OnKeyDown = function() end
		bd.OnKeyUp = function() end
		bd.SetBreakdownData = function(self, displayData, pinned)
			self.pinned = pinned
			if displayData == self.sourceData then
				return
			end
			self.sourceData = displayData
			self.shown = false
		end
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
    pob_app::tabs::skills::core::set_skills_hooks(|h| {
        *h = pob_app::tabs::skills::core::default_skills_hooks();
        // U31's lists and gem rows are stand-ins here (module notes; CCR-0119 §2)
        h.skill_list_ctor = Rc::new(pob_app::tabs::skills::core::standin_skill_list);
        h.create_gem_slot = Rc::new(pob_app::tabs::skills::core::standin_create_gem_slot);
        h.skill_set_list_ctor = Rc::new(pob_app::tabs::skills::core::standin_skill_set_list);
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
        "ui-u30",
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

// ---- T4: the `Skills` section ----

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

/// The attribute masks (written reasons, ui doc §11.1 T4), applied to `(elem, attribute, value)`:
/// * A8 item 15 (CCR-0019 §4-7, the mask of u20/u22/u32/u43): the frozen
///   `GemInstance.enableGlobal1/2` and `SocketGroup.includeInFullDPS` (`Cell<bool>`) and
///   `mainActiveSkill(Calcs)` (`Cell<f64>`) cannot hold nil. A gem or group whose field is nil in
///   Lua (the `EnemyExplode` gems of CalcSetup.lua:1535-1542, the item-granted groups, a pasted
///   group, an old-format group without `includeInFullDPS`) saves the Rust default where PoB
///   writes `tostring(nil)`; the CCR-0019 markers that would keep the nil do not survive U15's
///   bridge (`lua_calc.rs` `copy_group` copies the Lua group's `extra` over the model's). A Lua
///   `"nil"` compares equal to `"false"` for the booleans;
/// * `mainActiveSkill(Calcs)`: a Lua `"nil"` and a Rust `"0"` (the default of a group the calc
///   creates, as in u20) or `"nan"`/`"-nan"` (the model's nil number, which a pasted group
///   carries: DIVERGENCE(paste-main-active-skill) of `tabs/skills/core.rs`, CCR-0118 §3) all read
///   `"<nil>"`.
fn mask(elem: &str, k: &str, v: &str, lua: bool) -> Option<&'static str> {
    match (elem, k) {
        ("Gem", "enableGlobal1" | "enableGlobal2") | ("Skill", "includeInFullDPS")
            if lua && v == "nil" =>
        {
            Some("false")
        }
        ("Skill", "mainActiveSkill" | "mainActiveSkillCalcs")
            if (lua && v == "nil") || (!lua && matches!(v, "0" | "nan" | "-nan")) =>
        {
            Some("<nil>")
        }
        _ => None,
    }
}

/// A `Gem` element whose `gemId` has more than one variant with its `skillId` as granted
/// effect (`data.gemsByGameId[gemId]`; u20's test).
fn ambiguous_variant(node: &XmlNode) -> bool {
    let (Some(game_id), Some(skill_id)) = (node.attrib.get("gemId"), node.attrib.get("skillId"))
    else {
        return false;
    };
    let n = pob_gamedata::data()
        .gems
        .values()
        .filter(|g| *g.gameId == **game_id && *g.grantedEffectId == **skill_id)
        .count();
    n > 1
}

/// `elem {attributes sorted}` of `n` and of every descendant element, in document order, indented
/// by depth (SkillsTab:Save writes the sets in `skillSetOrderList` order and the groups and gems
/// in list order, so the order is compared too). Attributes are compared as sets (Lua builds
/// `xml.attrib` with a table constructor, `pairs` order). `masked` counts [`mask`] hits.
fn flatten(n: &XmlNode, depth: usize, lua: bool, masked: &mut usize, out: &mut Vec<String>) {
    let mut a: Vec<String> = n
        .attrib
        .iter()
        .map(|(k, v)| match mask(&n.elem, k, v, lua) {
            // SkillsTab.lua:472-478 (as in u20): an old-format gem (no variantId) picks the first
            // variant of `pairs(possibleVariants)` (string keys, per-process order) whose
            // grantedEffectId matches, and several variants share one (Vaal Righteous Fire and
            // its AltX): PoB's own pick differs between its runs, so the pick is not compared
            _ if &*n.elem == "Gem"
                && matches!(&**k, "variantId" | "nameSpec")
                && ambiguous_variant(n) =>
            {
                *masked += 1;
                format!("{k}=<pairs-chosen variant>")
            }
            Some(m) => {
                *masked += 1;
                format!("{k}={m:?}")
            }
            None => format!("{k}={v:?}"),
        })
        .collect();
    a.sort();
    out.push(format!(
        "{}{} {{{}}}",
        "  ".repeat(depth),
        n.elem,
        a.join(" ")
    ));
    for c in &n.children {
        if let XmlChild::Node(c) = c {
            flatten(c, depth + 1, lua, masked, out);
        }
    }
}

/// The `<Skills>` element, flattened.
fn skills_section(xml: &str, lua: bool, masked: &mut usize) -> Result<Vec<String>, String> {
    let root = root_element(xml)?;
    let skills = root
        .children
        .iter()
        .find_map(|c| match c {
            XmlChild::Node(n) if &*n.elem == "Skills" => Some(n),
            _ => None,
        })
        .ok_or_else(|| "no Skills section".to_owned())?;
    let mut out = Vec::new();
    flatten(skills, 0, lua, masked, &mut out);
    Ok(out)
}

/// Every snapshot pair's `Skills` section: `Ok((pairs, masked attributes))`.
fn t4(lua: &Side, rust: &Side) -> Result<(usize, usize), Vec<String>> {
    let mut errs = Vec::new();
    if lua.snapshots.len() != rust.snapshots.len() {
        errs.push(format!(
            "snapshots: lua {} rust {}",
            lua.snapshots.len(),
            rust.snapshots.len()
        ));
    }
    let mut n = 0;
    let mut masked = 0;
    for ((name, lx), (_, rx)) in lua.snapshots.iter().zip(&rust.snapshots) {
        n += 1;
        match (
            skills_section(lx, true, &mut masked),
            skills_section(rx, false, &mut masked),
        ) {
            (Ok(l), Ok(r)) => {
                if l.len() != r.len() {
                    errs.push(format!(
                        "snapshot {name}: Skills has {} elements in lua, {} in rust",
                        l.len(),
                        r.len()
                    ));
                }
                if let Some((a, b)) = l.iter().zip(&r).find(|(a, b)| a != b) {
                    errs.push(format!("snapshot {name}: lua {a} | rust {b}"));
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
        Ok((n, masked))
    } else {
        Err(errs)
    }
}

/// `#! u30-expect-skills TEXT`: some Lua snapshot's Skills section has an element line containing
/// TEXT (guards a script whose clicks silently stopped reaching the control, which both sides
/// would agree on).
fn check_expectations(text: &str, lua: &Side) -> Vec<String> {
    let mut errs = Vec::new();
    for want in directives(text, "u30-expect-skills") {
        let found = lua.snapshots.iter().any(|(_, x)| {
            skills_section(x, true, &mut 0)
                .is_ok_and(|s| s.iter().any(|e| e.contains(want.as_str())))
        });
        if !found {
            errs.push(format!("no Lua Skills section element contains {want:?}"));
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
    t4_masked: usize,
}

#[test]
fn u30_scenarios() {
    let Some(root) = inputs() else { return };
    let mut scripts: Vec<PathBuf> = std::fs::read_dir(test_dir())
        .expect("tests/u30")
        .filter_map(Result::ok)
        .map(|e| e.path())
        .filter(|p| p.extension().is_some_and(|x| x == "pobscript"))
        .collect();
    scripts.sort();
    assert!(!scripts.is_empty(), "no tests/u30/*.pobscript");
    let filter = std::env::var("U30_FILTER").ok();
    let png = std::env::var_os("U30_PNG").is_some();
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
                format!("u30_{stem}")
            } else {
                format!("u30_{stem}@{w}x{h}@{s}")
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
                Ok((p, m)) => {
                    out.t4_pairs += p;
                    out.t4_masked += m;
                    format!("T4 {p} Skills sections identical ({m} masked attributes)")
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
    let mut t4_masked = 0;
    for case in results {
        for l in &case.lines {
            println!("{l}");
        }
        failures.extend(case.failures);
        ran += case.ran;
        t4_pairs += case.t4_pairs;
        t4_masked += case.t4_masked;
    }
    println!("u30: {ran} runs, T4 {t4_pairs} Skills sections ({t4_masked} masked attributes)");
    println!("{}", oracle_cache::summary());
    assert!(ran > 0 || filter.is_some(), "no run");
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}
