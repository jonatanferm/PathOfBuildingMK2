//! U25 acceptance (ui doc §12 U25 row): the Tree tab (`TreeTab.lua:1-1046`, the UI half;
//! `PassiveSpecListControl.lua`, `PassiveMasteryControl.lua`) on both sides of the UI oracle.
//! Owner: U25.
//!
//! * `u25_scenarios`: every `tests/u25/*.pobscript` (at 1280x800 @1.0 and 1920x1080 @1.5, or
//!   once at the script's own `screen`, e.g. 1600x900) through the full app, as u22/u23: zero T3
//!   divergences per frame, the same window titles, and T4 (the build XML of every `snapshot`:
//!   the `Tree` section (spec list, active spec, titles, converted/imported specs) and the
//!   viewer's `TreeView` section are compared strictly).
//!
//! The Rust side's calc is U15's LuaCalcBridge (ext:calc, as u20-u23). Both sides run the real
//! TreeTab (its Draw, controls, popups and the spec UndoHandler).
//!
//! Stand-ins (both sides run the same ones) for the units not ported yet:
//! * U24's `AddNodeTooltip` / `AddCompareNodeTooltip`: two lines (as u23).
//! * U26's `calcsTab:BuildPower()` (the heat map): as u23. `PowerReportListControl` (U26) stays
//!   hidden in every scenario (its Rust stand-in draws nothing; PoB's draws nothing while
//!   hidden).
//! * The other tabs' `Draw` draw nothing; `utf8.next` (as u22). The Items tab's `Draw` draws
//!   one line, `itemsTab.controls.specSelect.selIndex` (TreeTab.lua:579 and
//!   PassiveSpecListControl.lua:125-131 set it): `i_active_spec2` loads a build with
//!   `activeSpec="2"` (the load-time `SetActiveSpec` tails: that line, the version dropdown).
//!
//! T3 multiset buckets: as u23 (sub-layers 0..=30 of layer 0, the tree's `pairs` loops).
//!
//! Compared side effects exclude `ImageLoad` (as u23).
//!
//! T4: a `Spec`'s `nodes` attribute is compared as a set. Save writes it in `pairs(allocNodes)`
//! order; after `ResetNodes` (Reset Tree, and every `RestoreUndoState` / `ImportFromNodeList`)
//! that order depends on the dead keys `self.allocNodes[id] = nil` leaves for each id of
//! `pairs(self.nodes)` (PassiveSpec.lua:769-772), and `pairs(spec.nodes)` is per-process random
//! in PoB (ui doc §8 rule 6 (b)); `h_undo` reaches it. The Spec's `URL` child (`EncodeURL`'s
//! `pairs(self.allocNodes)`, PassiveSpec.lua:537, the same node order) is compared strictly
//! where the two sides' `nodes` attributes are byte-identical, and set aside where only their
//! node sets match (the node set, class and ascendancy are compared through the attributes).
//!
//! `e_convert` uses a 3_19 build without a search string (`tree_3_19_plain.xml`): with
//! `searchStr="life"` the search highlights after the conversion differ (U23's search-result
//! cache, `searchStrResults[nodeId]`, keeps the 3_19 spec's results and is read for the 3_29
//! spec's node ids; PoB's cache holds results for ids the Rust 3_19 spec does not match), which
//! is outside U25's range and reported to the integrator.

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
use pob_ui_core::prelude::ViewPort;
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
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/u25")
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
                let sel =
                    pob_app::tabs::tree::core::items_spec_select(cx).expect("U33's specSelect");
                let line = format!(
                    "^7specSelect: {}",
                    cx.w::<pob_ui_core::controls::DropDownControl>(sel)
                        .sel_index
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
            XmlChild::Text(_) if &*node.elem == "URL" => {
                kids.push("<URL: allocNodes order>".to_owned())
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
            let ids = spec
                .attrib
                .get("nodes")
                .map(|v| v.to_string())
                .unwrap_or_default();
            let url = nodes(&spec)
                .iter()
                .filter(|n| &*n.elem == "URL")
                .map(text)
                .collect();
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
            return Err(format!(
                "{what}: Spec {} URL differs: lua {lu:?} | rust {ru:?}",
                i + 1
            ));
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
        "ui-u25",
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
fn u25_scenarios() {
    let Some(root) = inputs() else { return };
    let mut scripts: Vec<PathBuf> = std::fs::read_dir(test_dir())
        .expect("tests/u25")
        .filter_map(Result::ok)
        .map(|e| e.path())
        .filter(|p| p.extension().is_some_and(|x| x == "pobscript"))
        .collect();
    scripts.sort();
    assert!(!scripts.is_empty(), "no tests/u25/*.pobscript");
    let filter = std::env::var("U25_FILTER").ok();
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
                format!("u25_{stem}")
            } else {
                format!("u25_{stem}@{w}x{h}@{s}")
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
    println!("u25: {ran} runs, {total_ops} Lua draw ops, T4 {t4_pairs} pairs ({t4_bytes} byte-identical)");
    println!("{}", oracle_cache::summary());
    assert!(ran > 0 || filter.is_some(), "no run");
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

/// Scenario authoring aid (not an acceptance test): `U25_PROBE=Builds/<file>` loads that build
/// of `tests/u25/user` on the Rust side and prints, for the spec's nodes up to `U25_PROBE_DIST`
/// (default 4) steps from the allocated tree, their screen position at `U25_SCREEN=WxH@S`
/// (default 1280x800@1), with the TreeTab's real tree viewport; and the TreeTab controls' rects.
#[test]
#[ignore = "authoring aid"]
fn u25_probe() {
    let Some(root) = inputs() else { return };
    let Ok(build) = std::env::var("U25_PROBE") else {
        return;
    };
    let scr = std::env::var("U25_SCREEN").unwrap_or_else(|_| "1280x800@1".to_owned());
    let (wh, s) = scr.split_once('@').unwrap();
    let (w, h) = wh.split_once('x').unwrap();
    let (w, h, s): (i32, i32, f32) = (w.parse().unwrap(), h.parse().unwrap(), s.parse().unwrap());
    let max_dist: f64 = std::env::var("U25_PROBE_DIST")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(4.0);
    let steps = std::env::var("U25_PROBE_STEPS")
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
    let tab = pob_app::tabs::tree::core::tab(cx).expect("treeTab");
    let ctl = |k: &'static str| pob_app::tabs::tree::core::ctl(cx, tab, k);
    for k in [
        "specSelect",
        "compareCheck",
        "compareSelect",
        "reset",
        "versionText",
        "versionSelect",
        "treeSearch",
        "findTimelessJewel",
        "treeHeatMap",
        "nodePowerMaxDepthSelect",
        "treeHeatMapStatSelect",
        "powerReport",
        "specConvertText",
        "specConvert",
        "specConvertAll",
    ] {
        let id = ctl(k);
        let (x, y) = cx.pos(id);
        let (cw, ch) = cx.size(id);
        println!(
            "control {k}: shown={} x={x} y={y} w={cw} h={ch}",
            cx.is_shown(id)
        );
    }
    for spec in cx.build().model.treeTab.specList.snapshot() {
        println!(
            "spec {:?} {}",
            spec.title.get().map(|t| t.to_string()),
            spec.treeVersion.get()
        );
    }
    println!("popups {}", cx.main.popups.len());
    let lines_height = -cx.base(ctl("specSelect")).y.raw();
    let m = cx.build().model.clone();
    let convert = if m.treeTab.showConvert(&m) {
        64.0
    } else {
        32.0
    };
    let vp = ViewPort {
        x: 312.0,
        y: 32.0,
        width: sw - 312.0,
        height: sh - 32.0 - convert - lines_height,
    };
    let spec = m.spec.get();
    let tree = spec.tree.get();
    let v = &cx.w::<pob_app::tabs::tree::TreeTab>(tab).core.viewer;
    let (zoom, zx, zy) = (v.zoom, v.zoom_x.unwrap(), v.zoom_y.unwrap());
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
