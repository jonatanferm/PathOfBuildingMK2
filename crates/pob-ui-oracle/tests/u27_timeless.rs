//! U27 acceptance (ui doc §12 U27 row: "Popup layout, filters, sliders (with the `tooltip.Draw`
//! overrides at `:1749/1785/1814/1841`), and results identical to Lua for 5 searches"): the
//! "Find a Timeless Jewel" popup (`TreeTab.lua:1170-2888`) on both sides of the UI oracle.
//! Owner: U27.
//!
//! * `u27_scenarios`: every `tests/u27/*.pobscript` through the full app (as u25): the scripts
//!   open a fixture build, click "Find Timeless Jewel" and drive the popup. A script that sets
//!   its own `screen` runs once at that size, the others at 1280x800 @1.0 and 1920x1080 @1.5.
//!   Per run: zero T3 divergences per frame (the same draw ops per `(layer, sub)` bucket and the
//!   same side effects, `OpenURL`/`Copy` of the trade URL included), the same window titles, and
//!   T4 (the build XML of every `snapshot`: the `TimelessData` section the popup writes is
//!   compared strictly, as every other section).
//!
//!   The scripts click by coordinates, so each comes in a 1280x800 and a 1920x1080 @1.5 copy
//!   (`*_1920`, the popup 40 virtual px higher). The six searches: Lethal Pride (`a_lp`, with the
//!   trade URL), Glorious Vanity (`b_gv`), Militant Faith with the node filter, a protected
//!   notable and the devotion trade filters (`c_mf`), Elegant Hubris over all sockets (`d_eh`),
//!   an abyss jewel (`e_abyss`), and the fallback weights (`g_generate`, Lethal Pride;
//!   `h_generate_gv`, Glorious Vanity's per-stat mod lists and their `modListGenerated` cache);
//!   `f_controls` drives the dropdowns, the node list, the sliders and Reset / Cancel. The
//!   scripts are written by `tests/u27/gen_scenarios.py` (both copies from one template).
//!
//!   Directives: `#! u27-expect-text TEXT` (some Lua `DrawString` contains TEXT: a result row, a
//!   tooltip), `#! u27-expect-effect NAME` (the Lua side recorded such a side effect), and
//!   `#! u27-rust-misc-calc` (below).
//!
//! The Rust side's calc is U15's LuaCalcBridge (ext:calc), as u20-u25. `#! u27-rust-misc-calc`
//! (the "Generate" button) gives the Rust side [`HybridCalc`]: the bridge for `build_output`,
//! the Rust calc for the misc calculator, because the fallback-weight pseudo nodes cannot cross
//! the bridge (CCR-0132 §4). That scenario therefore also compares the Rust calc's misc
//! calculator against PoB's (the generated weights are rounded to 3 decimals of x100).
//!
//! Stand-ins (both sides run the same ones), as u25: the other tabs' `Draw` draw nothing, the
//! Items tab one line; U24's `AddNodeTooltip` / `AddCompareNodeTooltip` draw two lines (the
//! popup's protect check boxes, conqueror and ascendancy dropdowns reach `AddNodeTooltip` too:
//! `tabs::tree::timeless::set_timeless_hooks`); U26's `calcsTab:BuildPower()`.
//!
//! Net: `TradeQueryRequests:FetchLeagues` (ported in timeless.rs, CCR-0132) downloads
//! `https://www.pathofexile.com/api/trade/data/leagues` through `launch:DownloadPage`: the
//! scripts give it a `fixture net=` response. The Rust side's completions are held back from
//! `Driver::frame`'s `poll` and handed to `App::on_net` at the next `on_frame` (as u43).
//!
//! T3 multiset buckets: as u23/u25 (sub-layers 0..=30 of layer 0, the tree's `pairs` loops).
//! The result list's node-name tooltips follow `pairs(targetNodes)` (timeless.rs ORDER notes,
//! (b)); the scripts do not hover result rows.
//!
//! Compared side effects exclude `ImageLoad` (as u23).

use std::cell::RefCell;
use std::path::{Path, PathBuf};
use std::rc::Rc;

use pob_app::app::CxApp;
use pob_app::build::init;
use pob_app::build::on_frame as of;
use pob_app::tree_view;
use pob_calc::model::{XmlChild, XmlNode};
use pob_host::{App, PlatformParts};
use pob_sg::trace::{ApiCall, Buckets, DiffConfig, Divergence, DrawOp, FrameTrace, SideEffect};
use pob_sg::{Key, Sg, Sys};
use pob_ui_oracle::boot::{self, Entry, OracleHandles};
use pob_ui_oracle::diff_report;
use pob_ui_oracle::lua_calc::LuaCalcBridge;
use pob_ui_oracle::runner::{self, Driver, LuaApp, RunResult, ScenarioApp, UserDir};

/// One LuaJIT PoB at a time (memory; ui doc §11.3).
static LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

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
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/u27")
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
	-- the Items tab's stand-in: the tree dropdown's selIndex (TreeTab.lua:579)
	self.itemsTab.Draw = function(s, viewPort, inputEvents)
		DrawString(viewPort.x + 4, viewPort.y + 4, "LEFT", 16, "VAR", "^7specSelect: " .. tostring(s.controls.specSelect.selIndex))
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

fn rust_stubs() {
    init::set_hooks(|h| *h = init::default_hooks());
    of::set_shell_hooks(|h| {
        *h = of::default_shell_hooks();
        // the Items tab's stand-in (LUA_STUBS): the tree dropdown's selIndex (TreeTab.lua:579)
        let all_tabs = h.draw_tab.clone();
        h.draw_tab = Rc::new(move |cx, tab, vp, ev| {
            if tab == of::BuildTab::Items {
                let sel = pob_app::tabs::tree::core::items_spec_select(cx).expect("U33's specSelect");
                let line = format!(
                    "^7specSelect: {}",
                    cx.w::<pob_ui_core::controls::DropDownControl>(sel).sel_index
                );
                cx.sg.draw_string(
                    vp.x + 4.0,
                    vp.y + 4.0,
                    pob_sg::Align::Left,
                    16.0,
                    pob_sg::Font::Var,
                    &line,
                );
            } else {
                all_tabs(cx, tab, vp, ev);
            }
        });
    });
    pob_app::tabs::tree::core::set_tree_tab_hooks(|h| {
        *h = pob_app::tabs::tree::core::default_tree_tab_hooks();
    });
    pob_app::tabs::tree::timeless::set_timeless_hooks(|h| {
        *h = pob_app::tabs::tree::timeless::default_timeless_hooks();
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

/// Holds the inner Net's completions back from `Driver::frame`'s `poll` (which drops them) and
/// hands them to [`RustSide::on_frame`] (as u43).
struct DeferredNet {
    inner: Box<dyn pob_sg::Net>,
    held: Rc<RefCell<Vec<(u64, pob_sg::HttpResponse)>>>,
}

impl pob_sg::Net for DeferredNet {
    fn download_page(&mut self, req: pob_sg::HttpRequest) -> u64 {
        self.inner.download_page(req)
    }
    fn poll(&mut self) -> Vec<(u64, pob_sg::HttpResponse)> {
        let done = self.inner.poll();
        self.held.borrow_mut().extend(done);
        Vec::new()
    }
    fn oauth_listen(&mut self, url: &str) -> Option<u64> {
        self.inner.oauth_listen(url)
    }
    fn busy(&self) -> bool {
        self.inner.busy() || !self.held.borrow().is_empty()
    }
}

struct RustSide {
    app: pob_app::PobApp,
    effects: Rc<RefCell<Vec<SideEffect>>>,
    held: Rc<RefCell<Vec<(u64, pob_sg::HttpResponse)>>>,
}

thread_local! {
    /// `#! u27-rust-misc-calc`: the Rust side's misc calculator is the Rust calc (module notes).
    static RUST_MISC_CALC: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
}

/// The bridge for `build_output`, the Rust calc (`RustCalc`, calc:CA/PWR) for the misc
/// calculator: the fallback-weight pseudo nodes (`{ id, modList, divisor }`, TreeTab.lua:1962)
/// cannot cross the bridge (CCR-0132 §4). Each `build_output` runs the Rust calc's first (its
/// `calcsTab.miscCalculator` is kept here), then the bridge's (whose environments and outputs
/// replace the Rust calc's, so everything else the app draws is the bridge's).
struct HybridCalc {
    lua: LuaCalcBridge,
    misc: Option<(Rc<pob_calc::modules::calcs::MiscCalculator>, pob_app::model::Output)>,
}

impl pob_app::calc::CalcFacade for HybridCalc {
    fn build_output(&mut self, build: &pob_app::model::BuildRef) -> std::task::Poll<()> {
        let _ = pob_app::calc::RustCalc.build_output(build);
        self.misc = Some(pob_calc::modules::power::GetMiscCalculator(build));
        self.lua.build_output(build)
    }
    fn misc_calculator(
        &mut self,
        _build: &pob_app::model::BuildRef,
    ) -> std::task::Poll<(Rc<dyn pob_app::calc::MiscCalc>, pob_app::model::Output)> {
        let (f, base) = self.misc.clone().expect("HybridCalc: build_output first");
        std::task::Poll::Ready((Rc::new(pob_app::calc::rust::RustMiscCalc(f)), base))
    }
}

impl RustSide {
    fn new(mut parts: PlatformParts, src: PathBuf, net: pob_ui_oracle::sg_bridge::NetFixtures) -> RustSide {
        let held = Rc::new(RefCell::new(Vec::new()));
        parts.sys.net = Box::new(DeferredNet {
            inner: Box::new(boot::ScriptedNet::new(net)),
            held: held.clone(),
        });
        let effects = Rc::new(RefCell::new(Vec::new()));
        let inner = std::mem::replace(&mut parts.sys.platform, Box::new(NullPlatform));
        parts.sys.platform = Box::new(RecordingPlatform {
            inner,
            effects: effects.clone(),
        });
        let mut app = pob_app::PobApp::with_calc(
            parts,
            Rc::new(move || {
                let lua = LuaCalcBridge::new(Some(src.clone()));
                if RUST_MISC_CALC.with(std::cell::Cell::get) {
                    Box::new(HybridCalc { lua, misc: None })
                } else {
                    Box::new(lua)
                }
            }),
        );
        effects.borrow_mut().clear();
        app.cx.app_mut().launch.force_release_install = true;
        RustSide { app, effects, held }
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
        // the completions `poll` held back (pob_host::run_frame: before on_frame)
        let done: Vec<_> = self.held.borrow_mut().drain(..).collect();
        for (id, resp) in done {
            self.app.on_net(id, resp);
        }
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

// ---- T4 (u20's rules; the `Tree` and `TreeView` sections are this unit's and compared strictly) ----

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
        if let Some(d) = rust_default.filter(|_| lua && &**v == "nil") {
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
        } else if &*node.elem == "Spec" && &**k == "nodes" {
            // the allocNodes order (module notes): compared as a set
            let mut ids: Vec<&str> = v.split(',').collect();
            ids.sort_unstable();
            attrs.push((k.to_string(), ids.join(",")));
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
            // a Spec's URL lists the nodes in the same allocNodes order: compared in compare_t4
            // where the `nodes` attributes match byte for byte (module notes)
            XmlChild::Text(_) if &*node.elem == "URL" => kids.push("<URL: allocNodes order>".to_owned()),
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

/// Each `Tree`/`Spec`'s raw `nodes` attribute and `URL` text, in document order.
fn spec_urls(root: &XmlNode) -> Vec<(String, String)> {
    let nodes = |n: &XmlNode| -> Vec<XmlNode> {
        n.children
            .iter()
            .filter_map(|c| match c {
                XmlChild::Node(n) => Some(n.clone()),
                XmlChild::Text(_) => None,
            })
            .collect()
    };
    let text = |n: &XmlNode| -> String {
        n.children
            .iter()
            .filter_map(|c| match c {
                XmlChild::Text(t) => Some(t.to_string()),
                XmlChild::Node(_) => None,
            })
            .collect()
    };
    nodes(root)
        .into_iter()
        .filter(|n| &*n.elem == "Tree")
        .flat_map(|t| nodes(&t))
        .filter(|n| &*n.elem == "Spec")
        .map(|spec| {
            let ids = spec.attrib.get("nodes").map(|v| v.to_string()).unwrap_or_default();
            let url = nodes(&spec).iter().filter(|n| &*n.elem == "URL").map(text).collect();
            (ids, url)
        })
        .collect()
}

/// Compare one T4 pair; `Ok(byte_identical)`.
fn compare_t4(what: &str, lua: &str, rust: &str, notes: &mut T4Notes) -> Result<bool, String> {
    let lr = root_element(lua).map_err(|e| format!("{what}: Lua XML: {e}"))?;
    let rr = root_element(rust).map_err(|e| format!("{what}: Rust XML: {e}"))?;
    // the same allocNodes order on both sides: EncodeURL's text must match too (module notes)
    for (i, ((ln, lu), (rn, ru))) in spec_urls(&lr).iter().zip(&spec_urls(&rr)).enumerate() {
        if ln == rn && lu != ru {
            return Err(format!("{what}: Spec {} URL differs: lua {lu:?} | rust {ru:?}", i + 1));
        }
    }
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
    net: &pob_ui_oracle::sg_bridge::NetFixtures,
) -> anyhow::Result<(Side, Side)> {
    let user = UserDir::new(fixture)?;

    // Lua
    let handles = OracleHandles::default();
    let lua_app = runner::lua_side(root, &user, &handles, Entry::Launch, false)?;
    *pob_ui_oracle::sg_bridge::bridge(&lua_app.pob.lua)
        .map_err(|e| anyhow::anyhow!("{e}"))?
        .net
        .borrow_mut() = net.clone();
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
    let mut rust_side = RustSide::new(PlatformParts { sg, sys }, src, net.clone());
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

/// The script's `#! u27-expect-text TEXT` (some Lua `DrawString` of the run contains TEXT) and
/// `#! u27-expect-effect NAME` (the Lua side recorded a side effect of that kind, e.g. `OpenUrl`)
/// lines: they guard a scenario whose input silently stopped reaching the popup, which both
/// sides would agree on.
fn check_expectations(text: &str, lua: &[FrameTrace]) -> Vec<String> {
    let mut errs = Vec::new();
    for line in text.lines() {
        if let Some(want) = line.strip_prefix("#! u27-expect-text ") {
            let found = lua.iter().flat_map(|f| &f.calls).any(|c| match &c.call {
                ApiCall::DrawString { text, .. } => text.contains(want),
                _ => false,
            });
            if !found {
                errs.push(format!("no Lua DrawString contains {want:?}"));
            }
        } else if let Some(want) = line.strip_prefix("#! u27-expect-effect ") {
            let found = lua
                .iter()
                .flat_map(|f| &f.effects)
                .any(|e| format!("{e:?}").starts_with(want.trim()));
            if !found {
                errs.push(format!("no Lua side effect {want:?}"));
            }
        }
    }
    errs
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

#[test]
fn u27_scenarios() {
    let _g = LOCK
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let Some(root) = inputs() else { return };
    let mut scripts: Vec<PathBuf> = std::fs::read_dir(test_dir())
        .expect("tests/u27")
        .filter_map(Result::ok)
        .map(|e| e.path())
        .filter(|p| p.extension().is_some_and(|x| x == "pobscript"))
        .collect();
    scripts.sort();
    assert!(!scripts.is_empty(), "no tests/u27/*.pobscript");
    let filter = std::env::var("U27_FILTER").ok();
    let mut failures = Vec::new();
    let mut ran = 0;
    let (mut t4_pairs, mut t4_bytes) = (0, 0);
    let mut total_ops = 0;
    for path in scripts {
        let stem = path.file_stem().unwrap().to_string_lossy().into_owned();
        if filter.as_ref().is_some_and(|f| !f.split('|').any(|p| stem.contains(p))) {
            continue;
        }
        let (script, base_dir) = runner::load_script(&path).unwrap();
        let fx = runner::fixtures(&script, &base_dir).unwrap();
        let text = std::fs::read_to_string(&path).unwrap();
        RUST_MISC_CALC.with(|c| c.set(text.lines().any(|l| l.trim() == "#! u27-rust-misc-calc")));
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
            let tag = if own_screen {
                format!("u27_{stem}")
            } else {
                format!("u27_{stem}@{w}x{h}@{s}")
            };
            let (lua, rust) = match run_sides(&root, &script, fx.user_dir.as_deref(), runner::screen(w, h, s), &fx.net) {
                Ok(s) => s,
                Err(e) => {
                    failures.push(format!("{tag}: {e:#}"));
                    continue;
                }
            };
            ran += 1;
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
                    d.first().map_or_else(|| "frame missing".to_owned(), describe)
                )
            });
            let ops = draw_ops(&run.lua);
            total_ops += ops;
            let expect_errs = check_expectations(&std::fs::read_to_string(&path).unwrap(), &run.lua);
            if !expect_errs.is_empty() {
                failures.push(format!("{tag}: expectations: {}", expect_errs.join("; ")));
            }
            let t4_line = match &t4r {
                Ok((p, b)) => {
                    t4_pairs += p;
                    t4_bytes += b;
                    format!(
                        "T4 {p} identical ({b} byte-identical; set aside: {} Lua nil gem/group fields, {} pairs-chosen gem variants, pending UI-saver sections differing {:?})",
                        notes.gem_nil_bools, notes.ambiguous_variants, notes.pending_sections
                    )
                }
                Err(e) => format!("T4 FAILED: {}", e.join("; ")),
            };
            println!(
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
            );
            if n != 0
                || rust.error.is_some()
                || lua.error.is_some()
                || run.lua.len() != run.rust.len()
                || ops == 0
                || titles.0 != titles.1
                || t4r.is_err()
            {
                failures.push(format!(
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
    }
    println!("u27: {ran} runs, {total_ops} Lua draw ops, T4 {t4_pairs} pairs ({t4_bytes} byte-identical)");
    assert!(ran > 0 || filter.is_some(), "no run");
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}


/// Scenario authoring aid (not an acceptance test): `U27_PROBE=<steps; separated>` runs the steps
/// (`fixture` lines included) on the Rust side at `U27_SCREEN=WxH@S` (default 1280x800@1) and
/// prints the TreeTab's `findTimelessJewel` rect, the open popup's controls (rects, shown,
/// dropdown lists) and the timeless data.
#[test]
#[ignore = "authoring aid"]
fn u27_probe() {
    let Some(root) = inputs() else { return };
    let Ok(steps) = std::env::var("U27_PROBE") else { return };
    let scr = std::env::var("U27_SCREEN").unwrap_or_else(|_| "1280x800@1".to_owned());
    let (wh, s) = scr.split_once('@').unwrap();
    let (w, h) = wh.split_once('x').unwrap();
    let (w, h, s): (i32, i32, f32) = (w.parse().unwrap(), h.parse().unwrap(), s.parse().unwrap());
    let script = runner::parse_script(&format!("{}\nframes 1\n", steps.replace(';', "\n"))).unwrap();
    let fx = runner::fixtures(&script, &test_dir()).unwrap();
    let user = UserDir::new(fx.user_dir.as_deref()).unwrap();
    let handles = OracleHandles::default();
    let (sg, sys) = boot::oracle_parts(&root, &user.path, &handles).unwrap();
    let src = std::fs::canonicalize(root.join("src")).unwrap_or_else(|_| root.join("src"));
    let mut rust_side = RustSide::new(PlatformParts { sg, sys }, src, fx.net.clone());
    let mut d = Driver::new(runner::screen(w, h, s), handles, user.path.clone());
    let (_, r) = runner::drive(&mut rust_side, &script, &mut d);
    r.unwrap();
    let cx = &rust_side.app.cx;
    if let Some(tab) = pob_app::tabs::tree::core::tab(cx) {
        let id = pob_app::tabs::tree::core::ctl(cx, tab, "findTimelessJewel");
        println!("findTimelessJewel {:?} {:?}", cx.pos(id), cx.size(id));
    }
    if let Some(&p) = cx.main.popups.first() {
        println!("popup {:?} {:?}", cx.pos(p), cx.size(p));
        let mut keys: Vec<String> = [
            "devotionSelectLabel", "devotionSelect1", "devotionSelect2", "jewelSelect", "conquerorSelect", "socketSelect",
            "socketFilter", "socketAllocate", "protectAllocatedLabel", "abyssAscendancyLabel", "abyssAscendancySelect",
            "socketFilterAdditionalDistance", "socketFilterAdditionalDistanceValue", "nodeSlider", "nodeSliderValue",
            "nodeSlider2", "nodeSlider2Value", "nodeSlider3", "nodeSlider3Value", "nodeSelect", "fallbackWeightsList",
            "fallbackWeightsButton", "totalMinimumWeight", "searchList", "searchListFallback", "searchListButton",
            "searchListFallbackButton", "searchResults", "msg", "searchTradeButton", "searchTradeLeagueSelect",
            "realmSelection", "tradeTypeSelection", "searchMore", "resetButton", "closeButton", "searchButton",
        ]
        .iter()
        .map(|s| (*s).to_owned())
        .collect();
        keys.extend((1..=12).map(|i| format!("protectAllocatedBox{i}")));
        for key in keys {
            let Some(c) = cx.child(p, key.clone()) else { continue };
            println!(
                "  {key} {} pos {:?} size {:?} shown {}",
                cx.ui.class_name(c),
                cx.pos(c),
                cx.size(c),
                cx.is_shown(c)
            );
            if let Some(dd) = cx.try_w::<pob_ui_core::controls::DropDownControl>(c) {
                let labels: Vec<_> = dd.list.iter().take(60).map(|i| i.label.clone()).collect();
                println!("    sel {} n {} list {labels:?}", dd.sel_index, dd.list.len());
            }
        }
    }
    let m = cx.build().model.clone();
    let td = pob_calc::model::build_xml::timelessData(&m);
    println!("searchList {:?}", td.get("searchList"));
    if let Some(r) = td.table("searchResults") {
        println!("results {}", r.len());
        for (i, row) in r.ipairs().into_iter().take(12) {
            println!("result {i} {:?}", row.get("label"));
        }
    }
}
