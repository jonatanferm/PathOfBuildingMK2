//! U24 acceptance (ui doc §12 U24 row): the passive tree view's node names and tooltips
//! (`PassiveTreeView.lua:1412-1856`: AddNodeName, AddNodeTooltip, AddCompareNodeTooltip,
//! AddCompareNodeName) on both sides of the UI oracle. Owner: U24.
//!
//! * `u24_scenarios`: every `tests/u24/*.pobscript` (each at its own `screen`) through the full
//!   app, as u23: zero T3 divergences per frame (the tooltips are on sub-layer 100 and compared
//!   in order), the same window titles and side effects, and both sides error-free. Every
//!   scenario hovers nodes whose tooltip runs U24's code; the "Allocating / Unallocating this
//!   node will give you:" stat differences come from `calcFunc` through U15's LuaCalcBridge on
//!   the Rust side (ext:calc, as u20-u23) and from PoB's own calc on the Lua side. The run also
//!   checks that each `#! tooltip TEXT` line of a script was drawn on the tooltip sub-layer
//!   (100) of some Lua frame, so the hovers really reach the nodes the script names.
//! * `u24_tooltip_lines`: with `allflame_compare` loaded on both sides (`_probe_b.pobscript`),
//!   AddNodeName, AddNodeTooltip (plus its skill tooltip), AddNodeTooltip with `returnEarly`,
//!   AddCompareNodeName and AddCompareNodeTooltip for 16 nodes of every kind the code branches on,
//!   on fresh tooltips with `showStatDifferences` off: header, `maxWidth`, `center`, recipe and
//!   every line (size, font, centre flag, text; separators by size) compared one by one. This
//!   also covers AddCompareNodeTooltip, which no fixture reaches by hovering (a compare-only node
//!   needs an allocated node of the compare spec that the main spec lacks).
//!
//! Stand-ins (both sides run the same ones) for the units not ported yet:
//! * TreeTab (U25): its `Draw` is replaced by the viewer part of TreeTab.lua:359-452 (as u23).
//! * U26's `calcsTab:BuildPower()`: `powerMax` and `node.power` as functions of the node id (as
//!   u23; the tooltip reads `node.power` only under `launch.devModeAlt`).
//! * U38's `itemsTab:AddItemTooltip(tooltip, item, slot, dbMode)`: two lines (the item name; the
//!   slot's `slotName` and `dbMode`), so the jewel-socket branches (L1497-1531, L1779-1789) run
//!   and the slot argument is checked.
//! * The other tabs' `Draw` draw nothing; `spec:AddUndoState` (U25) sets a stand-in flag;
//!   `utf8.next` (as u22/u23).
//!
//! Not covered: L1704-1706, a node granted by an item (`removeNodes = { [node.id] = true }`):
//! the LuaCalcBridge cannot carry the id half of that override (CCR-0122 §1), so the anointed
//! Force of Darkness of the allflame builds is not hovered; L1695-1697 (reallocating a mastery)
//! needs the mastery popup (U25); `launch.devModeAlt` lines (the oracle runs PoB as a release
//! install).
//!
//! Compared side effects exclude `ImageLoad` (as u23).

use std::cell::RefCell;
use std::path::{Path, PathBuf};
use std::rc::Rc;

use pob_app::app::CxApp;
use pob_app::build::init;
use pob_app::build::on_frame::{self as of, BuildTab, TabFlag};
use pob_app::tabs::items::slot_helper;
use pob_app::tree_view::{self, PassiveTreeView};
use pob_host::{App, PlatformParts};
use pob_sg::trace::{ApiCall, Buckets, DiffConfig, Divergence, DrawOp, FrameTrace, SideEffect};
use pob_sg::{Key, Sg, Sys};
use pob_ui_core::prelude::{InputEvents, ViewPort};
use pob_ui_oracle::boot::{self, Entry, OracleHandles};
use pob_ui_oracle::diff_report;
use pob_ui_oracle::lua_calc::LuaCalcBridge;
use pob_ui_oracle::runner::{self, Driver, LuaApp, RunResult, ScenarioApp, UserDir};

/// One LuaJIT PoB at a time (memory; ui doc §11.3).
static LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

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
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/u24")
}

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
local b = main.modes.BUILD
local origInit = b.Init
function b:Init(...)
	origInit(self, ...)
	for _, t in ipairs({ "importTab", "partyTab", "configTab", "skillsTab", "itemsTab", "calcsTab", "compareTab" }) do
		if self[t] then self[t].Draw = function() end end
	end
	if self.treeTab then
		self.treeTab.Draw = function(tab, viewPort, inputEvents)
			local treeViewPort = { x = viewPort.x, y = viewPort.y, width = viewPort.width, height = viewPort.height - 32 }
			__u24_vp = treeViewPort
			tab.viewer.compareSpec = (#tab.specList > 1) and tab.specList[2] or nil
			tab.viewer:Draw(tab.build, treeViewPort, inputEvents)
		end
	end
	if self.calcsTab then
		self.calcsTab.BuildPower = function(ct)
			ct.powerMax = { offence = 6, defence = 4, singleStat = 6 }
			for id, node in pairs(ct.build.spec.nodes) do
				node.power = node.power or { }
				node.power.offence = id % 7
				node.power.defence = id % 5
			end
		end
	end
	if self.itemsTab then
		self.itemsTab.AddItemTooltip = function(it, tooltip, item, slot, dbMode)
			tooltip:AddLine(16, "^7" .. item.name)
			tooltip:AddLine(14, "^7" .. tostring(slot and slot.slotName) .. " " .. tostring(dbMode))
		end
	end
end
"#;

thread_local! {
    /// The stand-in `spec.modFlag` (UndoHandler.lua:37 via `spec:AddUndoState`, U25).
    static SPEC_MOD_FLAG: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
    /// `treeTab.viewer` (the TreeTab stand-in's viewer, made where TreeTab.lua:33 makes it).
    static VIEWER: RefCell<Option<PassiveTreeView>> = const { RefCell::new(None) };
}

/// `f(viewer)` with the stand-in TreeTab's viewer lent out (ui doc §7.2 `lend`).
fn with_viewer<R>(f: impl FnOnce(&mut PassiveTreeView) -> R) -> R {
    let mut v = VIEWER
        .with(|x| x.borrow_mut().take())
        .expect("treeTab.viewer");
    let r = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| f(&mut v)));
    VIEWER.with(|x| *x.borrow_mut() = Some(v));
    r.unwrap_or_else(|p| std::panic::resume_unwind(p))
}

/// The TreeTab stand-in's Draw (module notes).
fn tree_tab_draw(cx: &mut pob_ui_core::cx::Cx, vp: &ViewPort, ev: &mut InputEvents) {
    let tree_vp = ViewPort {
        x: vp.x,
        y: vp.y,
        width: vp.width,
        height: vp.height - 32.0,
    };
    let specs = cx.build().model.treeTab.specList.with(Clone::clone);
    with_viewer(|v| {
        v.compare_spec = if specs.len() > 1 {
            Some(specs[1].clone())
        } else {
            None
        };
        v.draw(cx, &tree_vp, ev);
    });
}

fn rust_stubs() {
    SPEC_MOD_FLAG.with(|f| f.set(false));
    VIEWER.with(|x| *x.borrow_mut() = None);
    init::set_hooks(|h| {
        *h = init::default_hooks();
        h.spec_add_undo_state = Rc::new(|_| SPEC_MOD_FLAG.with(|f| f.set(true)));
        // U25 stand-in: TreeTab.lua:492-498 (the buildLoadouts selFunc), as in u20-u23
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
        // TreeTab.lua:33 self.viewer = new("PassiveTreeView"):PassiveTreeView()
        let prev_tree_tab = h.tree_tab.clone();
        h.tree_tab = Rc::new(move |cx, build| {
            let id = prev_tree_tab(cx, build);
            let v = PassiveTreeView::new(cx);
            VIEWER.with(|x| *x.borrow_mut() = Some(v));
            id
        });
        // the `TreeView` saver (Build.lua:631: `self.treeTab.viewer`)
        let prev_load = h.load_ui_section.clone();
        h.load_ui_section = Rc::new(move |cx, elem, node, db| {
            if elem == "TreeView" {
                with_viewer(|v| v.load(node, db));
                false
            } else {
                prev_load(cx, elem, node, db)
            }
        });
    });
    of::set_shell_hooks(|h| {
        *h = of::default_shell_hooks();
        let prev_draw = h.draw_tab.clone();
        h.draw_tab = Rc::new(move |cx, tab, vp, ev| {
            if tab == BuildTab::Tree {
                tree_tab_draw(cx, vp, ev);
            } else {
                prev_draw(cx, tab, vp, ev);
            }
        });
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
        let prev_save = h.save_ui_section.clone();
        h.save_ui_section = Rc::new(move |cx, elem| {
            if elem == "TreeView" {
                let mut node = pob_calc::model::XmlNode {
                    elem: "TreeView".into(),
                    attrib: Default::default(),
                    children: Vec::new(),
                };
                with_viewer(|v| v.save(&mut node));
                Some(node)
            } else {
                prev_save(cx, elem)
            }
        });
    });
    // U24's tooltips are the defaults (CCR-0115 A); only BuildPower is a stand-in
    tree_view::set_hooks(|h| {
        *h = tree_view::default_hooks();
        h.build_power = Rc::new(|cx| {
            let m = cx.build().model.clone();
            let ct = pob_calc::model::build_xml::build_extra(&m).get_or_new_table(pob_lua::n!("calcsTab"));
            let pm = pob_calc::lua_table::Table::new();
            pm.set(pob_lua::n!("offence"), 6.0);
            pm.set(pob_lua::n!("defence"), 4.0);
            pm.set(pob_lua::n!("singleStat"), 6.0);
            ct.set(pob_lua::n!("powerMax"), pm);
            for (id, node) in m.spec.get().nodes.with(pob_lua::LjIntMap::pairs) {
                let p = node.extra.get_or_new_table(pob_lua::n!("power"));
                p.set(pob_lua::n!("offence"), pob_lua::fmod_lua(id as f64, 7.0));
                p.set(pob_lua::n!("defence"), pob_lua::fmod_lua(id as f64, 5.0));
            }
        });
    });
    // U38 stand-in (module notes)
    slot_helper::set_items_hooks(|h| {
        *h = slot_helper::default_items_hooks();
        h.add_item_tooltip = Rc::new(|cx, tt, item, slot, db_mode| {
            tt.add_line(cx, 16.0, &format!("^7{}", item.name.get()));
            let slot_name = slot
                .and_then(|s| s.str(pob_lua::n!("slotName")))
                .map_or_else(|| "nil".to_owned(), |s| s.to_string());
            tt.add_line(cx, 14.0, &format!("^7{slot_name} {}", if db_mode { "true" } else { "nil" }));
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

/// Records the platform side effects the Lua side's bridge records (as in u16-u22).
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

/// The tree images of a run (module notes), sorted.
fn tree_image_loads(frames: &[FrameTrace]) -> Vec<String> {
    // frame 0 (OnInit, main:Init's latest tree): the Rust side's OnInit runs outside a traced
    // frame, so its loads are not recorded; the build's loads come later on both sides
    let mut v: Vec<String> = frames
        .iter()
        .skip(1)
        .flat_map(|f| &f.effects)
        .filter_map(|e| match e {
            SideEffect::ImageLoad(img)
                if img.path.contains("TreeData/") || img.path.contains("ring.png") || img.path.contains("Ring") =>
            {
                Some(format!("{} {:?}", img.path, img.flags))
            }
            _ => None,
        })
        .collect();
    v.sort();
    v
}

fn drop_image_loads(frames: &mut [FrameTrace]) {
    for f in frames {
        f.effects.retain(|e| !matches!(e, SideEffect::ImageLoad(_)));
    }
}

struct Side {
    frames: Vec<FrameTrace>,
    error: Option<String>,
    snapshots: Vec<(String, String)>,
    images: Vec<String>,
}

fn run_sides(
    root: &Path,
    script: &pob_host::script::Script,
    fixture: Option<&Path>,
    screen: pob_sg::ScreenInfo,
) -> anyhow::Result<(Side, Side)> {
    let user = UserDir::new(fixture)?;

    // Lua
    let handles = OracleHandles::default();
    let lua_app = runner::lua_side(root, &user, &handles, Entry::Launch, false)?;
    let mut lua_side = LuaSide { app: lua_app };
    let mut d = Driver::new(screen, handles, user.path.clone());
    let (mut frames, r) = runner::drive(&mut lua_side, script, &mut d);
    drop(lua_side);
    let images = tree_image_loads(&frames);
    drop_image_loads(&mut frames);
    let lua = Side {
        frames,
        error: r.err().map(|e| e.to_string()),
        snapshots: std::mem::take(&mut d.snapshots),
        images,
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
    let images = tree_image_loads(&frames);
    drop_image_loads(&mut frames);
    let rust = Side {
        frames,
        error,
        snapshots: std::mem::take(&mut d.snapshots),
        images,
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

/// The multiset sub-layers (module notes).
fn multiset_bucket(k: (i32, i32)) -> bool {
    k.0 == 0 && (0..=30).contains(&k.1)
}

/// A sort key of an op: its arguments at 1e-3 (ops equal within the diff's 1e-6 almost always
/// share it; a pair that straddles a rounding step falls back to the O(n²) multiset match).
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

/// One frame's divergences: the multiset buckets sorted by [`op_key`] and compared in order,
/// with the O(n²) multiset match where that finds a difference.
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

/// Every frame's divergences: `(frame, divergences)` for the frames with any.
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


/// The `#! tooltip TEXT` expectations of a script: each TEXT is drawn on the tooltip
/// sub-layer (100) of some Lua frame (the hovers really showed the nodes the script names).
fn tooltip_expectations(path: &Path) -> Vec<String> {
    std::fs::read_to_string(path)
        .unwrap_or_default()
        .lines()
        .filter_map(|l| l.strip_prefix("#! tooltip "))
        .map(|s| s.trim().to_owned())
        .collect()
}

/// Every string drawn on draw layer 0, sub-layer 100 (the node tooltips) of a run.
fn tooltip_strings(frames: &[FrameTrace]) -> Vec<String> {
    let mut out = Vec::new();
    for f in frames {
        let mut cur = (0, 0);
        for c in &f.calls {
            match &c.call {
                ApiCall::SetDrawLayer { layer, sub } => {
                    cur = (layer.unwrap_or(cur.0), sub.unwrap_or(0));
                }
                ApiCall::DrawString { text, .. } if cur.1 == 100 => out.push(text.clone()),
                _ => {}
            }
        }
    }
    out
}

#[test]
fn u24_scenarios() {
    let _g = LOCK
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let Some(root) = inputs() else { return };
    let mut scripts: Vec<PathBuf> = std::fs::read_dir(test_dir())
        .expect("tests/u24")
        .filter_map(Result::ok)
        .map(|e| e.path())
        .filter(|p| p.extension().is_some_and(|x| x == "pobscript"))
        // `_*.pobscript`: probe prefixes (u24_probe), not scenarios
        .filter(|p| !p.file_name().is_some_and(|n| n.to_string_lossy().starts_with('_')))
        .collect();
    scripts.sort();
    assert!(!scripts.is_empty(), "no tests/u24/*.pobscript");
    let filter = std::env::var("U24_FILTER").ok();
    let mut failures = Vec::new();
    let mut ran = 0;
    let mut total_ops = 0;
    let mut total_expect = 0;
    for path in scripts {
        let stem = path.file_stem().unwrap().to_string_lossy().into_owned();
        if filter.as_ref().is_some_and(|f| !stem.contains(f.as_str())) {
            continue;
        }
        let (script, base_dir) = runner::load_script(&path).unwrap();
        let fx = runner::fixtures(&script, &base_dir).unwrap();
        let own_screen = script
            .steps
            .iter()
            .find(|s| !matches!(s, pob_host::script::Step::Fixture { .. }))
            .is_some_and(|s| matches!(s, pob_host::script::Step::Screen { .. }));
        assert!(own_screen, "{stem}: every u24 script sets its screen (the hovers are screen positions)");
        let tag = format!("u24_{stem}");
        let t0 = std::time::Instant::now();
        let (lua, rust) = match run_sides(&root, &script, fx.user_dir.as_deref(), runner::screen(1280, 800, 1.0)) {
            Ok(s) => s,
            Err(e) => {
                failures.push(format!("{tag}: {e:#}"));
                continue;
            }
        };
        ran += 1;
        let titles = (window_titles(&lua.frames), window_titles(&rust.frames));
        let divs = diff_run(&lua.frames, &rust.frames);
        let n: usize = divs.iter().map(|(_, d)| d.len().max(1)).sum();
        // the hovers of the script showed the tooltips it names
        let drawn = tooltip_strings(&lua.frames);
        let expects = tooltip_expectations(&path);
        total_expect += expects.len();
        let missing: Vec<&String> = expects
            .iter()
            .filter(|e| !drawn.iter().any(|d| d.contains(e.as_str())))
            .collect();
        let tooltip_lines = drawn.len();
        let run = RunResult {
            lua: lua.frames,
            rust: rust.frames,
            rust_error: rust.error.clone(),
            lua_snapshots: lua.snapshots.clone(),
            rust_snapshots: rust.snapshots.clone(),
            audit: Vec::new(),
            compared: true,
        };
        // the jsonl traces for inspection (its report.txt counts ordered divergences)
        let _ = diff_report::write_report(&out_dir(), &tag, &run, false);
        let first = divs.first().map(|(fi, d)| {
            format!(
                "frame {fi}: {}",
                d.first().map_or_else(|| "frame missing".to_owned(), describe)
            )
        });
        let ops = draw_ops(&run.lua);
        total_ops += ops;
        println!(
            "{tag}: {} frames, {ops} Lua draw ops ({tooltip_lines} tooltip strings, {} Lua tree image loads), {n} divergences, {} of {} tooltip expectations met, titles {:?}, {:.1} s{}{}{}",
            run.lua.len(),
            lua.images.len(),
            expects.len() - missing.len(),
            expects.len(),
            distinct_titles(&titles.0),
            t0.elapsed().as_secs_f64(),
            lua.error
                .as_ref()
                .map_or_else(String::new, |e| format!(", Lua error: {e}")),
            rust.error
                .as_ref()
                .map_or_else(String::new, |e| format!(", Rust error: {e}")),
            first.as_ref().map_or_else(String::new, |f| format!(", first: {f}")),
        );
        if n != 0
            || rust.error.is_some()
            || lua.error.is_some()
            || run.lua.len() != run.rust.len()
            || ops == 0
            || titles.0 != titles.1
            || !missing.is_empty()
        {
            failures.push(format!(
                "{tag}: {n} divergences ({}), frames lua {} rust {}, {ops} ops, missing tooltips {missing:?}, titles lua {:?} rust {:?}, errors lua {:?} rust {:?}",
                first.unwrap_or_default(),
                run.lua.len(),
                run.rust.len(),
                distinct_titles(&titles.0),
                distinct_titles(&titles.1),
                lua.error,
                rust.error,
            ));
        }
    }
    println!("u24: {ran} runs, {total_ops} Lua draw ops, {total_expect} tooltip expectations");
    assert!(ran > 0 || filter.is_some(), "no run");
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

// ---- probe (a development aid for writing the scripts; not part of the acceptance) ----

/// After the script ran on the Lua side: the on-screen nodes of the build's spec with their
/// screen position (PassiveTreeView.lua:279-285 `treeToScreen`) and what the tooltip branches
/// on; the compare spec's compare-only nodes are listed with `CMPONLY`.
const PROBE_LUA: &str = r#"
local b = main.modes.BUILD
local v = b.treeTab.viewer
local vp = __u24_vp
local spec = b.spec
local tree = spec.tree
local scale = math.min(vp.width, vp.height) / tree.size * v.zoom
local ox = v.zoomX + vp.x + vp.width / 2
local oy = v.zoomY + vp.y + vp.height / 2
local out = { }
local function describe(id, node, tag)
	if not node.x or not node.y or node.type == "ClassStart" then return end
	local sx, sy = node.x * scale + ox, node.y * scale + oy
	if sx < vp.x + 20 or sx > vp.x + vp.width - 20 or sy < vp.y + 20 or sy > vp.y + vp.height - 20 then return end
	local extra = tag
	for _, m in ipairs(node.finalModList or node.modList or { }) do
		if m.name == "ExtraSkill" or m.name == "ExtraSupport" then extra = extra .. " SKILL" end
	end
	if node.type == "Socket" and tag == "" then
		local s, j = b.itemsTab:GetSocketAndJewelForNodeID(id)
		extra = extra .. (j and (" jewel=" .. j.name) or " empty") .. " enabled=" .. tostring(s:IsEnabled())
	end
	if b.calcsTab.mainEnv.grantedPassives[id] then extra = extra .. " GRANTED" end
	if node.reminderText then extra = extra .. " reminder" end
	if node.flavourText then extra = extra .. " flavour" end
	if node.allMasteryOptions then extra = extra .. " allopts" end
	if node.ascendancyName then extra = extra .. " asc" end
	if node.isTattoo then extra = extra .. " tattoo" end
	if node.expansionSkill then extra = extra .. " expansion" end
	local cmp = v.compareSpec and v.compareSpec.nodes[id]
	if cmp and tag == "" then extra = extra .. " cmpalloc=" .. tostring(cmp.alloc) .. ((cmp.sd[1] ~= node.sd[1]) and " cmpsd" or "") end
	out[#out + 1] = string.format("%6d %-9s %-5s %-36s %5d %5d path=%d dep=%d r=%.1f%s", id, node.type, tostring(node.alloc), string.format("%q", node.dn), sx, sy,
		node.path and #node.path or -1, node.depends and #node.depends or -1, (node.size or 0) * scale, extra)
end
for id, node in pairs(spec.nodes) do describe(id, node, "") end
if v.compareSpec then
	for id, node in pairs(v.compareSpec.nodes) do
		if not spec.nodes[id] then describe(id, node, " CMPONLY") end
	end
end
table.sort(out)
return string.format("vp %d %d %d %d zoom %.3f\n", vp.x, vp.y, vp.width, vp.height, v.zoom) .. table.concat(out, "\n")
"#;

/// `U24_PROBE=<scripts in tests/u24, comma-separated> cargo test -p pob-ui-oracle --test u24_tree_tooltips --
/// --ignored u24_probe --nocapture`: run the script on the Lua side and print [`PROBE_LUA`].
#[test]
#[ignore = "development aid for writing the scripts (needs U24_PROBE)"]
fn u24_probe() {
    let _g = LOCK
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let Some(root) = inputs() else { return };
    let Ok(names) = std::env::var("U24_PROBE") else { return };
    for name in names.split(',') {
        let path = test_dir().join(name);
        let (script, base_dir) = runner::load_script(&path).unwrap();
        let fx = runner::fixtures(&script, &base_dir).unwrap();
        let user = UserDir::new(fx.user_dir.as_deref()).unwrap();
        let handles = OracleHandles::default();
        let lua_app = runner::lua_side(&root, &user, &handles, Entry::Launch, false).unwrap();
        let mut lua_side = LuaSide { app: lua_app };
        let mut d = Driver::new(runner::screen(1280, 800, 1.0), handles, user.path.clone());
        let (frames, r) = runner::drive(&mut lua_side, &script, &mut d);
        r.unwrap();
        let text: String = lua_side
            .app
            .with_vm(|pob| pob.eval::<String>(PROBE_LUA))
            .unwrap_or_else(|e| panic!("probe: {e}"));
        println!("==== {name}\n{text}");
        for s in tooltip_strings(&frames) {
            println!("TT {s}");
        }
    }
}

// ---- u24_tooltip_lines ----

/// The nodes of `allflame_compare` whose tooltips `u24_tooltip_lines` builds: allocated and
/// unallocated masteries, notables, keystones, normal nodes, jewel sockets (with and without a
/// jewel), a cluster subgraph node, the runegraft and the item-granted Force of Darkness.
const LINE_NODES: [i64; 16] = [
    63861, 5826, 62506, 61981, 27788, 62791, 31703, 61320, 57226, 61834, 41263, 33989, 65824, 857, 5574, 18182,
];

/// The Lua side of `u24_tooltip_lines`: for each node, AddNodeName, AddNodeTooltip (and its skill
/// tooltip), AddNodeTooltip with `returnEarly`, AddCompareNodeName and AddCompareNodeTooltip on
/// fresh tooltips, with `showStatDifferences` off (no calc).
const LINES_LUA: &str = r###"
local b = main.modes.BUILD
local v = b.treeTab.viewer
v.showStatDifferences = false
local function dump(tt)
	-- `tooltipHeader = false` after Clear (Tooltip.lua:61) is the Rust None
	local out = { "header=" .. tostring(tt.tooltipHeader or nil) .. " maxWidth=" .. tostring(tt.maxWidth) .. " center=" .. tostring(tt.center)
		.. " recipe=" .. (tt.recipe and table.concat(tt.recipe, ",") or "nil") }
	for _, l in ipairs(tt.lines) do
		if l.text then
			out[#out + 1] = string.format("%s %s %s %s", tostring(l.size), tostring(l.font), tostring(l.center), l.text)
		else
			out[#out + 1] = "SEP " .. tostring(l.size)
		end
	end
	return table.concat(out, "\n")
end
local res = { }
for _, id in ipairs(__u24_ids) do
	local node = b.spec.nodes[id]
	for _, kind in ipairs({ "name", "tooltip", "early", "cname", "ctooltip" }) do
		local tt = new("Tooltip"):Tooltip()
		if kind == "name" then v:AddNodeName(tt, node, b)
		elseif kind == "tooltip" then v:AddNodeTooltip(tt, node, b)
		elseif kind == "early" then v:AddNodeTooltip(tt, node, b, true)
		elseif kind == "cname" then v:AddCompareNodeName(tt, node)
		else v:AddCompareNodeTooltip(tt, node, b) end
		res[#res + 1] = "## " .. id .. " " .. kind .. "\n" .. dump(tt)
		if kind == "tooltip" then res[#res + 1] = "# skill\n" .. dump(v.skillTooltip) end
	end
end
return table.concat(res, "\n")
"###;

fn rust_dump(tt: &pob_ui_core::tooltip::Tooltip) -> String {
    let mut out = vec![format!(
        "header={} maxWidth={} center={} recipe={}",
        tt.tooltip_header.as_deref().unwrap_or("nil"),
        tt.max_width.map_or_else(|| "nil".to_owned(), pob_lua::tostring),
        tt.center,
        tt.recipe.as_ref().map_or_else(|| "nil".to_owned(), |r| r.join(","))
    )];
    for l in &tt.lines {
        match &l.text {
            Some(t) => out.push(format!(
                "{} {} {} {t}",
                pob_lua::tostring(l.size),
                l.font.map_or("nil", pob_sg::Font::lua_name),
                l.center
            )),
            None => out.push(format!("SEP {}", pob_lua::tostring(l.size))),
        }
    }
    out.join("\n")
}

/// The Rust side of [`LINES_LUA`].
fn rust_lines(cx: &mut pob_ui_core::cx::Cx) -> String {
    let mut res = Vec::new();
    with_viewer(|v| {
        v.show_stat_differences = false;
        for id in LINE_NODES {
            let node = cx
                .build()
                .model
                .spec
                .get()
                .nodes
                .with(|m| m.get(id))
                .unwrap_or_else(|| panic!("node {id}"));
            for kind in ["name", "tooltip", "early", "cname", "ctooltip"] {
                let mut tt = pob_ui_core::tooltip::Tooltip::default();
                // `new("Tooltip")` (Tooltip.lua:46-51 runs Clear)
                tt.clear(false);
                match kind {
                    "name" => v.add_node_name(cx, &mut tt, &node),
                    "tooltip" => v.add_node_tooltip(cx, &mut tt, &node, false),
                    "early" => v.add_node_tooltip(cx, &mut tt, &node, true),
                    "cname" => v.add_compare_node_name(cx, &mut tt, &node),
                    _ => v.add_compare_node_tooltip(cx, &mut tt, &node),
                }
                res.push(format!("## {id} {kind}\n{}", rust_dump(&tt)));
                if kind == "tooltip" {
                    res.push(format!("# skill\n{}", rust_dump(&v.skill_tooltip)));
                }
            }
        }
    });
    res.join("\n")
}

#[test]
fn u24_tooltip_lines() {
    let _g = LOCK
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let Some(root) = inputs() else { return };
    let path = test_dir().join("_probe_b.pobscript");
    let (script, base_dir) = runner::load_script(&path).unwrap();
    let fx = runner::fixtures(&script, &base_dir).unwrap();
    let screen = runner::screen(1280, 800, 1.0);
    let user = UserDir::new(fx.user_dir.as_deref()).unwrap();

    // Lua
    let ids = LINE_NODES.iter().map(i64::to_string).collect::<Vec<_>>().join(", ");
    let lua_text = {
        let handles = OracleHandles::default();
        let lua_app = runner::lua_side(&root, &user, &handles, Entry::Launch, false).unwrap();
        let mut lua_side = LuaSide { app: lua_app };
        let mut d = Driver::new(screen, handles, user.path.clone());
        let (_, r) = runner::drive(&mut lua_side, &script, &mut d);
        r.unwrap();
        lua_side
            .app
            .with_vm(|pob| pob.eval::<String>(&format!("__u24_ids = {{ {ids} }}\n{LINES_LUA}")))
            .unwrap_or_else(|e| panic!("Lua lines: {e}"))
    };

    // Rust
    user.reset().unwrap();
    let handles = OracleHandles::default();
    let (sg, sys) = boot::oracle_parts(&root, &user.path, &handles).unwrap();
    let src = std::fs::canonicalize(root.join("src")).unwrap_or_else(|_| root.join("src"));
    let mut rust_side = RustSide::new(PlatformParts { sg, sys }, src);
    let mut d = Driver::new(screen, handles, user.path.clone());
    let (_, res) = runner::drive(&mut rust_side, &script, &mut d);
    res.unwrap();
    let rust_text = rust_lines(&mut rust_side.app.cx);

    let lua_lines: Vec<&str> = lua_text.lines().collect();
    let rust_lines_v: Vec<&str> = rust_text.lines().collect();
    let mut diffs = Vec::new();
    let mut section = "";
    for i in 0..lua_lines.len().max(rust_lines_v.len()) {
        let lua_line = lua_lines.get(i).copied().unwrap_or("<none>");
        let rust_line = rust_lines_v.get(i).copied().unwrap_or("<none>");
        if lua_line.starts_with("## ") {
            section = lua_line;
        }
        if lua_line != rust_line {
            diffs.push(format!("{section} line {i}: lua {lua_line:?} | rust {rust_line:?}"));
        }
    }
    let sections = lua_lines.iter().filter(|x| x.starts_with("## ")).count();
    let text_lines = lua_lines
        .iter()
        .filter(|x| !x.starts_with('#') && !x.starts_with("header="))
        .count();
    println!(
        "u24_tooltip_lines: {} nodes, {sections} tooltips, {} lines ({text_lines} tooltip lines), {} differences",
        LINE_NODES.len(),
        lua_lines.len(),
        diffs.len()
    );
    let _ = std::fs::create_dir_all(out_dir().join("u24_tooltip_lines"));
    let _ = std::fs::write(out_dir().join("u24_tooltip_lines/lua.txt"), &lua_text);
    let _ = std::fs::write(out_dir().join("u24_tooltip_lines/rust.txt"), &rust_text);
    assert!(sections == LINE_NODES.len() * 5, "sections: {sections}");
    assert!(
        diffs.is_empty(),
        "{} differences:\n{}",
        diffs.len(),
        diffs.iter().take(40).cloned().collect::<Vec<_>>().join("\n")
    );
}
