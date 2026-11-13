//! U23 acceptance (ui doc §12 U23 row): the passive tree view (`PassiveTreeView.lua:1-1411`)
//! and the tree's UI assets (`PassiveTree.lua` image/sprite/connector parts, `main:LoadTree`) on
//! both sides of the UI oracle. Owner: U23.
//!
//! * `u23_assets`: for tree versions 2_6, 3_19, 3_27 and the latest, `main:LoadTree(v)` on both
//!   sides, then every `tree.assets[name]` (width, height, `[1..4]`, handle), every
//!   `tree.spriteMap[name][type]`, `tree.bloodlineSpritePrefixes` and every connector of
//!   `tree.connectors` (type, node ids, ascendancy, the three `vert` quads and `c[9..16]`)
//!   compared value by value (the connector list as a multiset: it is built in `pairs(tree.nodes)`
//!   order, which is random per PoB process, CCR-0024 §5).
//! * `u23_scenarios`: every `tests/u23/*.pobscript` (at 1280x800 @1.0 and 1920x1080 @1.5, or
//!   once at the script's own `screen`) through the full app, as u22: zero T3 divergences per
//!   frame, the same window titles, the same tree image loads over the run, and T4 (the build
//!   XML of every `snapshot`, with the `TreeView` section, the viewer's `Save`, compared
//!   strictly).
//!
//! The Rust side's calc is U15's LuaCalcBridge (ext:calc, as u20-u22).
//!
//! Stand-ins (both sides run the same ones) for the units not ported yet:
//! * TreeTab (U25): its `Draw` is replaced by the viewer part of TreeTab.lua:359-452: the tree
//!   viewport `{x, y, width, height - 32}` (one line of controls, no convert bar, no power report:
//!   TreeTab.lua:446 with `linesHeight = 0`, `bottomDrawerHeight = 0`, `showConvert = false`),
//!   `viewer.compareSpec = specList[2]` when the build has a second tree (the compare overlay,
//!   TreeTab.lua:451 with `isComparing` and `activeCompareSpec = 2`), and `viewer:Draw`. The
//!   TreeTab's `TreeView` saver is the viewer itself (Build.lua:629-639), on both sides.
//! * U24's `AddNodeTooltip` / `AddCompareNodeTooltip`: two lines (the node name and its
//!   allocation and type) on the tooltip the view passes.
//! * U26's `calcsTab:BuildPower()` (the heat map): `powerMax = {offence = 6, defence = 4,
//!   singleStat = 6}` and `node.power.offence/defence = id % 7 / id % 5` for every spec node, so
//!   the heat map colours are a function of the node id on both sides.
//! * The other tabs' `Draw` draw nothing; `spec:AddUndoState` (U25) sets the stand-in
//!   `spec.modFlag` (as u22); `utf8.next` (as u22).
//!
//! T3 multiset buckets (ui doc §8 rule 6 (b); the ORDER notes of `tree_view/draw.rs`): sub-layers
//! 0..=30 of layer 0, which the `pairs(tree.groups)`, `pairs(tree.connectors)`,
//! `pairs(spec.nodes)` and `pairs(tree.sockets)` loops fill. The tooltips (sub-layers 99/100)
//! and everything else are compared in order.
//!
//! Compared side effects exclude `ImageLoad`: the oracle traces only the Lua side's loads (its
//! `NewImageHandle` bridge), not `pob_sg::ImageHandle::load`; the images are covered by the
//! draws (every draw op names its image path and flags) and by `u23_assets` (every asset and
//! sprite sheet, with its size).

use std::cell::RefCell;
use std::path::{Path, PathBuf};
use std::rc::Rc;

use pob_app::app::CxApp;
use pob_app::build::init;
use pob_app::build::on_frame::{self as of, BuildTab, TabFlag};
use pob_app::tree_view::{self, assets, PassiveTreeView};
use pob_calc::model::{XmlChild, XmlNode};
use pob_host::{App, PlatformParts};
use pob_sg::trace::{ApiCall, Buckets, DiffConfig, Divergence, DrawOp, FrameTrace, SideEffect};
use pob_sg::{Key, Sg, Sys};
use pob_ui_core::prelude::{InputEvents, ViewPort};
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
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/u23")
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
			tab.viewer.compareSpec = (#tab.specList > 1) and tab.specList[2] or nil
			tab.viewer:Draw(tab.build, treeViewPort, inputEvents)
		end
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
		self.calcsTab.BuildPower = function(ct)
			ct.powerMax = { offence = 6, defence = 4, singleStat = 6 }
			for id, node in pairs(ct.build.spec.nodes) do
				node.power = node.power or { }
				node.power.offence = id % 7
				node.power.defence = id % 5
			end
		end
	end
end
"#;

thread_local! {
    /// The stand-in `spec.modFlag` (UndoHandler.lua:37 via `spec:AddUndoState`, U25).
    static SPEC_MOD_FLAG: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
    /// `treeTab.viewer` (the TreeTab stand-in's viewer, made where TreeTab.lua:33 makes it).
    static VIEWER: RefCell<Option<PassiveTreeView>> = const { RefCell::new(None) };
    /// The Rust side's `viewer:Draw` times (ms) of the current run, with the tree version.
    static DRAW_MS: RefCell<Vec<(String, f64)>> = const { RefCell::new(Vec::new()) };
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
    let mut v = VIEWER
        .with(|x| x.borrow_mut().take())
        .expect("treeTab.viewer");
    v.compare_spec = if specs.len() > 1 {
        Some(specs[1].clone())
    } else {
        None
    };
    let version = cx.build().model.spec.get().treeVersion.get().to_string();
    let t0 = std::time::Instant::now();
    let r = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| v.draw(cx, &tree_vp, ev)));
    let ms = t0.elapsed().as_secs_f64() * 1000.0;
    DRAW_MS.with(|d| d.borrow_mut().push((version, ms)));
    VIEWER.with(|x| *x.borrow_mut() = Some(v));
    r.unwrap_or_else(|p| std::panic::resume_unwind(p));
}

fn rust_stubs() {
    SPEC_MOD_FLAG.with(|f| f.set(false));
    VIEWER.with(|x| *x.borrow_mut() = None);
    DRAW_MS.with(|d| d.borrow_mut().clear());
    init::set_hooks(|h| {
        *h = init::default_hooks();
        h.spec_add_undo_state = Rc::new(|_| SPEC_MOD_FLAG.with(|f| f.set(true)));
        // U25 stand-in: TreeTab.lua:492-498 (the buildLoadouts selFunc), as in u20/u22
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
                let mut node = XmlNode {
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
    tree_view::set_hooks(|h| {
        *h = tree_view::default_hooks();
        h.add_node_tooltip = Rc::new(|cx, _v, tt, node| {
            tt.add_line(cx, 16.0, &format!("^7{}", node.dn.get()));
            tt.add_line(
                cx,
                14.0,
                &format!(
                    "^7{} {}",
                    if node.alloc.get() {
                        "allocated"
                    } else {
                        "unallocated"
                    },
                    node.r#type.get()
                ),
            );
        });
        h.add_compare_node_tooltip = Rc::new(|cx, _v, tt, node| {
            tt.add_line(cx, 16.0, &format!("^2{}", node.dn.get()));
            tt.add_line(cx, 14.0, &format!("^7compare {}", node.r#type.get()));
        });
        h.build_power = Rc::new(|cx| {
            let m = cx.build().model.clone();
            let ct = pob_calc::model::build_xml::build_extra(&m)
                .get_or_new_table(pob_lua::n!("calcsTab"));
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

// ---- T4 (u20's rules; the `TreeView` section is this unit's and compared strictly) ----

fn unordered_children(elem: &str) -> bool {
    matches!(
        elem,
        "Calcs" | "ConfigSet" | "ItemSet" | "Sockets" | "Overrides"
    )
}

/// The UI-only savers not ported yet: compared separately, a difference is reported.
const PENDING_SAVERS: [&str; 1] = ["Import"];

#[derive(Default)]
struct T4Notes {
    pending_sections: Vec<String>,
    gem_nil_bools: usize,
    ambiguous_variants: usize,
}

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

fn normalise(node: &XmlNode, top: bool, lua: bool, notes: &mut T4Notes) -> String {
    let mut attrs: Vec<(String, String)> = Vec::new();
    for (k, v) in &node.attrib {
        let rust_default = match (&*node.elem, &**k) {
            ("Gem", "enableGlobal1" | "enableGlobal2") | ("Skill", "includeInFullDPS") => {
                Some("false")
            }
            ("Skill", "mainActiveSkill" | "mainActiveSkillCalcs") => Some("0"),
            _ => None,
        };
        if let Some(d) = rust_default.filter(|_| &**v == "nil") {
            notes.gem_nil_bools += 1;
            attrs.push((k.to_string(), d.to_owned()));
        } else if &*node.elem == "Gem"
            && matches!(&**k, "variantId" | "nameSpec")
            && ambiguous_variant(node)
        {
            if k.as_ref() == "variantId" {
                notes.ambiguous_variants += 1;
            }
            attrs.push((k.to_string(), "<pairs-chosen variant>".to_owned()));
        } else {
            attrs.push((k.to_string(), v.to_string()));
        }
    }
    attrs.sort();
    let mut kids: Vec<String> = Vec::new();
    for c in &node.children {
        match c {
            XmlChild::Node(n) => {
                if top && notes.pending_sections.iter().any(|p| **p == *n.elem) {
                    continue;
                }
                kids.push(normalise(n, false, lua, notes));
            }
            XmlChild::Text(t) => kids.push(format!("{t:?}")),
        }
    }
    if top || unordered_children(&node.elem) {
        kids.sort();
    }
    format!("<{} {:?}>[{}]", node.elem, attrs, kids.join(","))
}

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

fn first_diff(a: &str, b: &str) -> String {
    let (a, b): (Vec<&str>, Vec<&str>) = (a.split('<').collect(), b.split('<').collect());
    for (i, (x, y)) in a.iter().zip(b.iter()).enumerate() {
        if x != y {
            return format!("element {i}: lua <{x} | rust <{y}");
        }
    }
    format!("lengths {} vs {}", a.len(), b.len())
}

/// Compare one T4 pair; `Ok(byte_identical)`.
fn compare_t4(what: &str, lua: &str, rust: &str, notes: &mut T4Notes) -> Result<bool, String> {
    let lr = root_element(lua).map_err(|e| format!("{what}: Lua XML: {e}"))?;
    let rr = root_element(rust).map_err(|e| format!("{what}: Rust XML: {e}"))?;
    let mut local = T4Notes::default();
    let section = |root: &XmlNode, elem: &str, lua: bool| {
        root.children.iter().find_map(|c| match c {
            XmlChild::Node(n) if *n.elem == *elem => {
                Some(normalise(n, false, lua, &mut T4Notes::default()))
            }
            _ => None,
        })
    };
    if section(&lr, "TreeView", true).is_none() {
        return Err(format!("{what}: no TreeView section"));
    }
    for elem in PENDING_SAVERS {
        if section(&lr, elem, true) != section(&rr, elem, false) {
            local.pending_sections.push(elem.to_owned());
        }
    }
    let ln = normalise(&lr, true, true, &mut local);
    let rn = normalise(&rr, true, false, &mut local);
    notes.gem_nil_bools += local.gem_nil_bools;
    notes.ambiguous_variants += local.ambiguous_variants;
    notes.pending_sections.extend(local.pending_sections);
    if ln != rn {
        return Err(format!("{what}: T4 differs: {}", first_diff(&ln, &rn)));
    }
    Ok(lua == rust)
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
                if img.path.contains("TreeData/")
                    || img.path.contains("ring.png")
                    || img.path.contains("Ring") =>
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
        "ui-u23",
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
        let images = tree_image_loads(&frames);
        drop_image_loads(&mut frames);
        Ok(LuaRun {
            frames,
            error: r.err().map(|e| e.to_string()),
            snapshots: std::mem::take(&mut d.snapshots),
            extra: vec![serde_json::to_string(&images)?],
        })
    })
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

    // Lua (cached, TP4)
    let run = lua_run(root, script, &user, fixture, screen)?;
    let lua = Side {
        frames: run.frames,
        error: run.error,
        snapshots: run.snapshots,
        images: serde_json::from_str(run.extra.first().map_or("null", String::as_str))?,
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
        } => format!(
            "S {} {} {align:?} {} {font:?} {text}",
            r(*x),
            r(*y),
            r(*height)
        ),
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
        } => format!(
            "changed {bucket:?}#{idx}: lua {} | rust {}",
            op(lua),
            op(rust)
        ),
        Divergence::Effect { lua, rust } => format!("effect: lua {lua:?} rust {rust:?}"),
    }
}

/// Every T4 pair of a run; `Ok(number of pairs, byte-identical pairs)`.
fn t4(lua: &Side, rust: &Side, notes: &mut T4Notes) -> Result<(usize, usize), Vec<String>> {
    let mut errs = Vec::new();
    let (mut n, mut same) = (0, 0);
    if lua.snapshots.len() != rust.snapshots.len() {
        errs.push(format!(
            "snapshots: lua {} rust {}",
            lua.snapshots.len(),
            rust.snapshots.len()
        ));
    }
    for ((ln, lx), (_, rx)) in lua.snapshots.iter().zip(&rust.snapshots) {
        n += 1;
        match compare_t4(&format!("snapshot {ln}"), lx, rx, notes) {
            Ok(b) => same += usize::from(b),
            Err(e) => errs.push(e),
        }
    }
    if errs.is_empty() {
        Ok((n, same))
    } else {
        Err(errs)
    }
}

/// One case's outcome (TP4: cases run on `oracle_cache::par_map`; collected in case order).
#[derive(Default)]
struct CaseOut {
    lines: Vec<String>,
    failures: Vec<String>,
    ran: usize,
    t4_pairs: usize,
    t4_bytes: usize,
    total_ops: usize,
}

#[test]
fn u23_scenarios() {
    let Some(root) = inputs() else { return };
    let mut scripts: Vec<PathBuf> = std::fs::read_dir(test_dir())
        .expect("tests/u23")
        .filter_map(Result::ok)
        .map(|e| e.path())
        .filter(|p| p.extension().is_some_and(|x| x == "pobscript"))
        .collect();
    scripts.sort();
    assert!(!scripts.is_empty(), "no tests/u23/*.pobscript");
    let filter = std::env::var("U23_FILTER").ok();
    let mut cases = Vec::new();
    for path in scripts {
        let stem = path.file_stem().unwrap().to_string_lossy().into_owned();
        if filter.as_ref().is_some_and(|f| !stem.contains(f.as_str())) {
            continue;
        }
        let (script, _) = runner::load_script(&path).unwrap();
        let own_screen = script
            .steps
            .iter()
            .find(|s| !matches!(s, pob_host::script::Step::Fixture { .. }))
            .is_some_and(|s| matches!(s, pob_host::script::Step::Screen { .. }));
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
        let (script, base_dir) = runner::load_script(&path).unwrap();
        let fx = runner::fixtures(&script, &base_dir).unwrap();
        {
            let tag = if own_screen {
                format!("u23_{stem}")
            } else {
                format!("u23_{stem}@{w}x{h}@{s}")
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
            let times = DRAW_MS.with(|d| d.borrow().clone());
            let mut by_version: std::collections::BTreeMap<String, (usize, f64, f64)> =
                Default::default();
            for (v, ms) in &times {
                let e = by_version.entry(v.clone()).or_default();
                e.0 += 1;
                e.1 += ms;
                e.2 = e.2.max(*ms);
            }
            let perf: Vec<String> = by_version
                .iter()
                .map(|(v, (n, sum, max))| {
                    format!(
                        "{v}: {n} draws, mean {:.1} ms, max {:.1} ms",
                        sum / *n as f64,
                        max
                    )
                })
                .collect();
            out.lines.push(format!(
                "{tag}: viewer:Draw (Rust, this build profile) {}",
                perf.join("; ")
            ));
            let titles = (window_titles(&lua.frames), window_titles(&rust.frames));
            let mut notes = T4Notes::default();
            let t4r = t4(&lua, &rust, &mut notes);
            let divs = diff_run(&lua.frames, &rust.frames);
            let n: usize = divs.iter().map(|(_, d)| d.len().max(1)).sum();
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
                    d.first()
                        .map_or_else(|| "frame missing".to_owned(), describe)
                )
            });
            let ops = draw_ops(&run.lua);
            out.total_ops += ops;
            let t4_line = match &t4r {
                Ok((p, b)) => {
                    out.t4_pairs += p;
                    out.t4_bytes += b;
                    format!(
                        "T4 {p} identical ({b} byte-identical; set aside: {} Lua nil gem/group fields, {} pairs-chosen gem variants, pending UI-saver sections differing {:?})",
                        notes.gem_nil_bools, notes.ambiguous_variants, notes.pending_sections
                    )
                }
                Err(e) => format!("T4 FAILED: {}", e.join("; ")),
            };
            out.lines.push(format!(
                "{tag}: {} frames, {ops} Lua draw ops, {n} divergences, {} Lua tree image loads, {t4_line}, titles {:?}{}{}{}",
                run.lua.len(),
                lua.images.len(),
                distinct_titles(&titles.0),
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
                || t4r.is_err()
            {
                out.failures.push(format!(
                    "{tag}: {n} divergences ({}), frames lua {} rust {}, {ops} ops, {t4_line}, titles lua {:?} rust {:?}, errors lua {:?} rust {:?}",
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
        out
    });
    let mut failures = Vec::new();
    let mut ran = 0;
    let (mut t4_pairs, mut t4_bytes) = (0, 0);
    let mut total_ops = 0;
    for case in results {
        for l in &case.lines {
            println!("{l}");
        }
        failures.extend(case.failures);
        ran += case.ran;
        t4_pairs += case.t4_pairs;
        t4_bytes += case.t4_bytes;
        total_ops += case.total_ops;
    }
    println!("u23: {ran} runs, {total_ops} Lua draw ops, T4 {t4_pairs} pairs ({t4_bytes} byte-identical)");
    println!("{}", oracle_cache::summary());
    assert!(ran > 0 || filter.is_some(), "no run");
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

// ---- u23_assets ----

/// The Lua side's description of `main:LoadTree(V)` (one line per asset, sprite, prefix and
/// connector; the connectors sorted).
const LUA_DESCRIBE: &str = r#"
local function f(x)
	if type(x) == "number" then return string.format("%.17g", x) end
	return tostring(x)
end
local tree = main:LoadTree(__u23_version)
local out = { }
local names = { }
for k in pairs(tree.assets) do names[#names + 1] = k end
table.sort(names)
for _, k in ipairs(names) do
	local a = tree.assets[k]
	out[#out + 1] = "A " .. k .. " " .. f(a.width) .. " " .. f(a.height) .. " " .. f(a[1]) .. " " .. f(a[2]) .. " " .. f(a[3]) .. " " .. f(a[4]) .. " " .. (a.handle and "h" or "nil")
end
names = { }
for k in pairs(tree.spriteMap) do names[#names + 1] = k end
table.sort(names)
for _, k in ipairs(names) do
	local types = { }
	for t in pairs(tree.spriteMap[k]) do types[#types + 1] = t end
	table.sort(types)
	for _, t in ipairs(types) do
		local a = tree.spriteMap[k][t]
		out[#out + 1] = "S " .. k .. " " .. t .. " " .. f(a.width) .. " " .. f(a.height) .. " " .. f(a[1]) .. " " .. f(a[2]) .. " " .. f(a[3]) .. " " .. f(a[4])
	end
end
names = { }
for k, v in pairs(tree.bloodlineSpritePrefixes or { }) do names[#names + 1] = k .. "=" .. v end
table.sort(names)
for _, k in ipairs(names) do out[#out + 1] = "P " .. k end
local cs = { }
for _, c in ipairs(tree.connectors) do
	local parts = { "C", c.type, f(c.nodeId1), f(c.nodeId2), tostring(c.ascendancyName) }
	for _, st in ipairs({ "Normal", "Intermediate", "Active" }) do
		for i = 1, 8 do parts[#parts + 1] = f(c.vert[st][i]) end
	end
	for i = 9, 16 do parts[#parts + 1] = f(c.c[i]) end
	cs[#cs + 1] = table.concat(parts, " ")
end
table.sort(cs)
for _, c in ipairs(cs) do out[#out + 1] = c end
return table.concat(out, "\n")
"#;

fn f17(x: f64) -> String {
    pob_lua::lua_format("%.17g", &[pob_lua::FmtArg::from(x)]).to_string()
}

/// `f(a[i])` of an asset (numbers, or the URL strings of a URL-list asset).
fn seq_text(a: &assets::Asset, i: usize) -> String {
    match &a.seq {
        assets::AssetSeq::Empty => "nil".to_owned(),
        assets::AssetSeq::Tc(tc) => f17(tc[i - 1]),
        assets::AssetSeq::Other(v) => match v.get(i - 1) {
            Some(pob_gamedata::DataValue::Num(x)) => f17(*x),
            Some(pob_gamedata::DataValue::Str(s)) => s.to_string(),
            _ => "nil".to_owned(),
        },
    }
}

fn nan_nil(x: f64) -> String {
    if x.is_nan() {
        "nil".to_owned()
    } else {
        f17(x)
    }
}

fn rust_describe(a: &assets::TreeAssets) -> String {
    let mut out: Vec<String> = Vec::new();
    let mut names: Vec<&String> = a.assets.keys().collect();
    names.sort();
    for k in names {
        let x = &a.assets[k];
        out.push(format!(
            "A {k} {} {} {} {} {} {} {}",
            nan_nil(x.width.get()),
            nan_nil(x.height.get()),
            seq_text(x, 1),
            seq_text(x, 2),
            seq_text(x, 3),
            seq_text(x, 4),
            if x.handle().is_some() { "h" } else { "nil" }
        ));
    }
    let mut names: Vec<&String> = a.sprite_map.keys().collect();
    names.sort();
    for k in names {
        let set = &a.sprite_map[k];
        let mut types: Vec<&String> = set.keys().collect();
        types.sort();
        for t in types {
            let x = &set[t];
            out.push(format!(
                "S {k} {t} {} {} {} {} {} {}",
                nan_nil(x.width.get()),
                nan_nil(x.height.get()),
                seq_text(x, 1),
                seq_text(x, 2),
                seq_text(x, 3),
                seq_text(x, 4)
            ));
        }
    }
    let mut p: Vec<String> = a
        .bloodline_sprite_prefixes
        .iter()
        .map(|(k, v)| format!("{k}={v}"))
        .collect();
    p.sort();
    out.extend(p.into_iter().map(|k| format!("P {k}")));
    let mut cs: Vec<String> = a
        .connectors
        .iter()
        .map(|c| {
            let mut parts = vec![
                "C".to_owned(),
                c.r#type.clone(),
                f17(c.node_id1),
                f17(c.node_id2),
                c.ascendancy_name
                    .as_ref()
                    .map_or_else(|| "nil".to_owned(), |n| n.to_string()),
            ];
            for v in &c.vert {
                parts.extend(v.iter().map(|x| f17(*x)));
            }
            parts.extend(c.c.borrow()[8..].iter().map(|x| f17(*x)));
            parts.join(" ")
        })
        .collect();
    cs.sort();
    out.extend(cs);
    out.join("\n")
}

#[test]
fn u23_assets() {
    // TP4: the process-wide case pool (shared with the `par_map` cases) instead of the lock
    let _permit = oracle_cache::Permit::acquire();
    let Some(root) = inputs() else { return };
    let latest = pob_gamedata::data()
        .gameVersions
        .latestTreeVersion
        .to_string();
    let versions = ["2_6", "3_19", "3_27", latest.as_str()];
    let script = runner::parse_script("frames 2\n").unwrap();
    let screen = runner::screen(1280, 800, 1.0);
    let user = UserDir::new(Some(&runner::crate_dir().join("fixtures/user1"))).unwrap();

    // Lua: one process, the versions in order (the 3.19 Assets.lua tables are shared)
    let handles = OracleHandles::default();
    let lua_app = runner::lua_side(&root, &user, &handles, Entry::Launch, false).unwrap();
    let mut lua_side = LuaSide { app: lua_app };
    let mut d = Driver::new(screen, handles, user.path.clone());
    let (_, r) = runner::drive(&mut lua_side, &script, &mut d);
    r.unwrap();
    let mut lua_desc = Vec::new();
    for v in versions {
        let text: String = lua_side
            .app
            .with_vm(|pob| {
                pob.eval::<()>(&format!("__u23_version = {v:?}"))?;
                pob.eval::<String>(LUA_DESCRIBE)
            })
            .unwrap_or_else(|e| panic!("Lua describe {v}: {e}"));
        lua_desc.push(text);
    }
    drop(lua_side);

    // Rust
    user.reset().unwrap();
    let handles = OracleHandles::default();
    let (sg, sys) = boot::oracle_parts(&root, &user.path, &handles).unwrap();
    let src = std::fs::canonicalize(root.join("src")).unwrap_or_else(|_| root.join("src"));
    let mut rust_side = RustSide::new(PlatformParts { sg, sys }, src);
    let mut d = Driver::new(screen, handles, user.path.clone());
    let (_, r) = runner::drive(&mut rust_side, &script, &mut d);
    r.unwrap();
    let mut failures = Vec::new();
    for (v, lua_text) in versions.iter().zip(&lua_desc) {
        let t0 = std::time::Instant::now();
        assets::main_load_tree(&mut rust_side.app.cx, v).expect("tree version");
        let built = t0.elapsed();
        let a = rust_side.app.cx.app().trees.trees[*v].1.clone();
        let rust_text = rust_describe(&a);
        let (ll, rl): (Vec<&str>, Vec<&str>) =
            (lua_text.lines().collect(), rust_text.lines().collect());
        let count = |p: &str, l: &[&str]| l.iter().filter(|x| x.starts_with(p)).count();
        let first = ll
            .iter()
            .zip(&rl)
            .position(|(a, b)| a != b)
            .map(|i| format!("line {i}: lua {:?} | rust {:?}", ll[i], rl[i]));
        println!(
            "u23_assets {v}: {} assets, {} sprites, {} prefixes, {} connectors (lua) / {} {} {} {} (rust); {} lines, identical: {}; Rust build {:.0} ms",
            count("A ", &ll),
            count("S ", &ll),
            count("P ", &ll),
            count("C ", &ll),
            count("A ", &rl),
            count("S ", &rl),
            count("P ", &rl),
            count("C ", &rl),
            ll.len(),
            lua_text == &rust_text,
            built.as_secs_f64() * 1000.0
        );
        if lua_text != &rust_text {
            failures.push(format!(
                "{v}: {} (lua {} lines, rust {})",
                first.unwrap_or_else(|| "length".to_owned()),
                ll.len(),
                rl.len()
            ));
        }
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

/// Scenario authoring aid (not an acceptance test): `U23_PROBE=Builds/<file>` loads that build
/// of `tests/u23/user` on the Rust side and prints, for the spec's nodes up to `U23_PROBE_DIST`
/// (default 4) steps from the allocated tree, their screen position at `U23_SCREEN=WxH@S`
/// (default 1280x800@1).
#[test]
#[ignore = "authoring aid"]
fn u23_probe() {
    let Some(root) = inputs() else { return };
    let Ok(build) = std::env::var("U23_PROBE") else {
        return;
    };
    let scr = std::env::var("U23_SCREEN").unwrap_or_else(|_| "1280x800@1".to_owned());
    let (wh, s) = scr.split_once('@').unwrap();
    let (w, h) = wh.split_once('x').unwrap();
    let (w, h, s): (i32, i32, f32) = (w.parse().unwrap(), h.parse().unwrap(), s.parse().unwrap());
    let max_dist: f64 = std::env::var("U23_PROBE_DIST")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(4.0);
    let steps = std::env::var("U23_PROBE_STEPS")
        .unwrap_or_default()
        .replace(';', "\n");
    let script = runner::parse_script(&format!(
        "frames 2\nload-build \"{build}\"\nframes 2\n{steps}\nframes 1\n"
    ))
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
    let (sw, sh) = (cx.main.screen_w, cx.main.screen_h);
    let vp = ViewPort {
        x: 312.0,
        y: 32.0,
        width: sw - 312.0,
        height: sh - 32.0 - 32.0,
    };
    let spec = cx.build().model.spec.get();
    let tree = spec.tree.get();
    let (zoom, zx, zy) = with_viewer(|v| (v.zoom, v.zoom_x.unwrap(), v.zoom_y.unwrap()));
    let scale = vp.width.min(vp.height) / tree.extra.num(pob_lua::n!("size")) * zoom;
    let (ox, oy) = (zx + vp.x + vp.width / 2.0, zy + vp.y + vp.height / 2.0);
    println!("screen {sw}x{sh} vp {vp:?} zoom {zoom} scale {scale}");
    let mut rows = Vec::new();
    for (id, node) in spec.nodes.with(pob_lua::LjIntMap::pairs) {
        let (x, y) = (node.x.get(), node.y.get());
        if x.is_nan() {
            continue;
        }
        let (sx, sy) = (x * scale + ox, y * scale + oy);
        if sx < vp.x || sx >= vp.x + vp.width || sy < vp.y || sy >= vp.y + vp.height {
            continue;
        }
        let dist = node.pathDist.get().unwrap_or(1000.0);
        if !node.alloc.get() && dist > max_dist {
            continue;
        }
        rows.push(format!(
            "{id} {:?} {} alloc={} dist={dist} rsq={} screen {:.0} {:.0}",
            node.dn.get(),
            node.r#type.get(),
            node.alloc.get(),
            node.extra.get(pob_lua::n!("rsq")).num_or(0.0),
            sx,
            sy
        ));
    }
    rows.sort();
    for r in rows {
        println!("{r}");
    }
}
