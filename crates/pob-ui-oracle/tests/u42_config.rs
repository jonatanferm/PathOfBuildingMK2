//! U42 acceptance (ui doc §12 U42 row: config sets; mod browser; visibility identical through
//! the bridge): the full app (Launch, the build list, the build shell, the whole Config tab) on
//! both sides of the UI oracle. Owner: U42 (the harness is U41's u41_config.rs with its U42
//! stand-ins removed).
//!
//! `u42_scenarios`: every `tests/u42/*.pobscript` (plus the directories named in `U42_DIRS`,
//! e.g. `U42_DIRS=u41` reruns U41's scripts with the real Draw) at 1280x800 @1.0 and 1920x1080
//! @1.5 (both 1280 virtual px wide, so the tab coordinates hold at both; a script that sets its
//! own `screen` runs once: the popups are centred vertically, so a popup script exists once per
//! resolution with its own coordinates). Each script starts at the real build list (U18) of the
//! fixture user dir, opens a build from it, switches to the Config tab (Ctrl+5) and drives it.
//! Per run: zero T3 divergences (the same draw ops per `(layer, sub)` bucket and the same side
//! effects, frame by frame), the same window titles, and T4: every `snapshot` step's build XML
//! (`buildMode:SaveDB`) and every file a script names with `#! u42-saved FILE` identical
//! between the sides after the normalisation below. `#! u42-expect-config` / `u42-expect-absent`
//! (and their `u41-` spellings) guard that the scripted input reached the controls.
//!
//! The Rust side's calc is U15's LuaCalcBridge (ext:calc, ui doc §12.1), as in u20-u22: the
//! option tooltips' compares (`calcFunc()` with the option toggled) run PoB's own calc on the
//! Rust model.
//!
//! Real on both sides: Launch, Main (U16), the build list (U18), Init (U19), OnFrame,
//! SyncLoadouts, SaveDBFile and ResetModFlags (U20), RefreshStatList and the stat box (U21),
//! the ConfigTab constructor, its option controls, CustomModBlockControl and
//! UpdateCustomModsControls (U41); ConfigTab Draw, UpdateControls, undo/redo, the config sets
//! (SetActiveConfigSet, the manage popup, ConfigSetListControl), the mod browser and
//! ConfigVisibility (U42). Stand-ins for the units not ported yet:
//! * the other tabs' `Draw` (U25-U46, Notes aside) draw nothing (the Lua tabs' `Draw` are
//!   replaced after `buildMode:Init`);
//! * `utf8.next` on the Lua side (the oracle's `lua-utf8` stand-in lacks it; see `LUA_STUBS`);
//! * `spec.modFlag` (U25's spec undo) reads from the stand-in of `spec:AddUndoState`, and
//!   `TreeTab:GetSpecList` (U25) is TreeTab.lua:492-498 (as in u20).
//!
//! T4 normalisation (as u20, which documents each rule): attributes as sets; the top-level
//! sections as a multiset (`pairs(self.savers)`); the children of `Calcs`, `ConfigSet`,
//! `ItemSet`, `Sockets` and `Overrides` as multisets; the Lua `nil` gem/group fields equal to
//! the frozen Rust defaults; the `pairs`-chosen gem variants not compared. The UI-saver
//! sections `TreeView` and `Import` (U25-U28, U43) are compared on their own and a difference is
//! reported, not failed.
//!
//! Compared side effects exclude `ImageLoad` (the Lua tabs' constructors load the tree's
//! images; U23 ports them).

use std::cell::RefCell;
use std::path::{Path, PathBuf};
use std::rc::Rc;

use pob_app::app::CxApp;
use pob_app::build::init;
use pob_app::build::on_frame::{self as of, TabFlag};
use pob_app::tabs::config::ctor as cfg;
use pob_calc::model::{XmlChild, XmlNode};
use pob_host::{App, PlatformParts};
use pob_sg::trace::{FrameTrace, SideEffect};
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

/// The scenario directories: `tests/u42`, plus those named in `U42_DIRS` (comma-separated
/// names under `tests/`, e.g. `u41`: U41's scripts with the real Draw instead of its stand-in).
fn test_dirs() -> Vec<PathBuf> {
    let base = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests");
    let mut v = vec![base.join("u42")];
    if let Ok(extra) = std::env::var("U42_DIRS") {
        v.extend(
            extra
                .split(',')
                .filter(|s| !s.is_empty() && *s != "u42")
                .map(|s| base.join(s)),
        );
    }
    v
}

/// The Lua stand-ins (see the module notes): every tab but Notes and Config draws nothing. Also the
/// oracle's `lua-utf8` stand-in has no `utf8.next` (EditControl.lua:558/:572, LEFT/RIGHT;
/// CCR-0033 item 4): luautf8's `Lutf8_next` for EditControl's call shape, as the u11
/// galleries install it (`lua/gallery/u11_single.lua`).
const LUA_STUBS: &str = r#"
-- CCR-0121 §6 (ui doc §8 rule 6 (a)): ConfigModBrowser's `pairs` loops over string-keyed (and
-- `root`-tainted) tables in a canonical order, keys ascending, as mod_browser.rs walks them.
-- The module is reloaded from its own text with those loops patched (line count unchanged) and
-- its OpenAddModPopup replaces the one ConfigTab.lua:14 required.
function __u42SortedPairs(t)
	local keys = { }
	for k in pairs(t) do
		keys[#keys + 1] = k
	end
	table.sort(keys, function(a, b)
		local ta, tb = type(a), type(b)
		if ta ~= tb then
			return ta < tb
		end
		return a < b
	end)
	local i = 0
	return function()
		i = i + 1
		local k = keys[i]
		if k ~= nil then
			return k, t[k]
		end
	end
end
do
	local f = assert(io.open("Modules/ConfigModBrowser.lua", "rb"))
	local text = f:read("*a")
	f:close()
	for _, loop in ipairs({ "pairs(mod.tradeHashes)", "pairs(tree.nodes)", "pairs(bData.itemMods)",
			"pairs(catMods)", "pairs(bData.veiledMods)", "pairs(bData.beastCraft)", "pairs(value.sources)" }) do
		local a, b = text:find(loop, 1, true)
		assert(a and not text:find(loop, b + 1, true), "u42 patch: " .. loop)
		text = text:sub(1, a - 1) .. "__u42SortedPairs" .. text:sub(a + 5)
	end
	local patched = assert(loadstring(text, "@Modules/ConfigModBrowser.lua"))()
	require("Modules.ConfigModBrowser").OpenAddModPopup = patched.OpenAddModPopup
end
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
	for _, t in ipairs({ "importTab", "partyTab", "treeTab", "skillsTab", "itemsTab", "calcsTab", "compareTab" }) do
		if self[t] then self[t].Draw = function() end end
	end
end
"#;

thread_local! {
    /// The stand-in `spec.modFlag` (UndoHandler.lua:37 via `spec:AddUndoState`, U25).
    static SPEC_MOD_FLAG: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
}

/// The Rust side's hooks: the real ones (U41's constructor, U42's Draw, undo, sets, mod
/// browser and visibility through the default hooks), with the stand-ins of the module notes.
fn rust_stubs() {
    SPEC_MOD_FLAG.with(|f| f.set(false));
    cfg::set_config_hooks(|h| *h = cfg::default_config_hooks());
    init::set_hooks(|h| {
        *h = init::default_hooks();
        h.spec_add_undo_state = Rc::new(|_| SPEC_MOD_FLAG.with(|f| f.set(true)));
        // U25 stand-in: TreeTab.lua:492-498 (the buildLoadouts selFunc, reached from
        // SyncLoadouts' SetSel), as in u20
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
        // keep the default arms (Notes, Config, ...) and add the spec stand-in
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
}

/// `#! NAME ARG` directives; a `u41-X` name also matches `#! u42-X`.
fn directives(text: &str, name: &str) -> Vec<String> {
    let mut tags = vec![format!("#! {name}")];
    if let Some(rest) = name.strip_prefix("u41-") {
        tags.push(format!("#! u42-{rest}"));
    }
    text.lines()
        .filter_map(|l| {
            let l = l.trim();
            tags.iter().find_map(|t| l.strip_prefix(t.as_str()))
        })
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
/// does not, CCR-0028 item 2); as in u16-u21.
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

// ---- T4 (the rules are u20's; see its module notes) ----

fn unordered_children(elem: &str) -> bool {
    matches!(
        elem,
        "Calcs" | "ConfigSet" | "ItemSet" | "Sockets" | "Overrides"
    )
}

/// The UI-only savers not ported yet: compared separately, a difference is reported.
const PENDING_SAVERS: [&str; 2] = ["Import", "TreeView"];

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

/// The raw text of the `<Notes>` element (from `<Notes` to `</Notes>`), for the strict check.
fn notes_text(xml: &str) -> Option<&str> {
    let a = xml.find("<Notes")?;
    let b = xml[a..]
        .find("</Notes>")
        .map(|b| a + b + "</Notes>".len())
        .or_else(|| {
            // an empty element written as `<Notes/>`
            xml[a..].find("/>").map(|b| a + b + 2)
        })?;
    Some(&xml[a..b])
}

/// Compare one T4 pair; `Ok(byte_identical)`.
fn compare_t4(what: &str, lua: &str, rust: &str, notes: &mut T4Notes) -> Result<bool, String> {
    let lr = root_element(lua).map_err(|e| format!("{what}: Lua XML: {e}"))?;
    let rr = root_element(rust).map_err(|e| format!("{what}: Rust XML: {e}"))?;
    // Notes: byte-identical
    let (ln, rn) = (notes_text(lua), notes_text(rust));
    if ln.is_none() || ln != rn {
        return Err(format!("{what}: Notes differ: lua {ln:?} rust {rn:?}"));
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

fn drop_image_loads(frames: &mut [FrameTrace]) {
    for f in frames {
        f.effects.retain(|e| !matches!(e, SideEffect::ImageLoad(_)));
    }
}

struct Side {
    frames: Vec<FrameTrace>,
    error: Option<String>,
    snapshots: Vec<(String, String)>,
    saved: Vec<(String, Option<String>)>,
    /// `#! u42-visibility`: per option of ConfigOptions (in list order) `-` (no `var`), else
    /// `isRelevantForBuild` and `isShowAllExcluded` as `0`/`1` pairs, at the end of the run.
    visibility: Option<String>,
}

/// The Lua side's visibility probe (ConfigVisibility.lua through PoB's own module, on the open
/// build `main.modes.BUILD`).
const VISIBILITY_LUA: &str = r#"
local cv = LoadModule("Modules/ConfigVisibility")
local vl = LoadModule("Modules/ConfigOptions")
local b = main.modes.BUILD
local out = { }
for i, v in ipairs(vl) do
	if v.var then
		out[i] = (cv.isRelevantForBuild(v, b) and "1" or "0") .. (cv.isShowAllExcluded(v) and "1" or "0")
	else
		out[i] = "-"
	end
end
return table.concat(out, " ")
"#;

/// The Rust side's visibility probe (U42's visibility.rs on the live model).
fn rust_visibility(app: &pob_app::PobApp) -> Option<String> {
    use pob_app::tabs::config::visibility as cv;
    let m = app.cx.app().build.as_ref().map(|b| b.model.clone())?;
    let out: Vec<String> = pob_gamedata::data()
        .configOptions
        .iter()
        .map(|v| {
            if v.var.is_some() {
                format!(
                    "{}{}",
                    u8::from(cv::isRelevantForBuild(v, Some(&m))),
                    u8::from(cv::isShowAllExcluded(v))
                )
            } else {
                "-".to_owned()
            }
        })
        .collect();
    Some(out.join(" "))
}

fn read_saved(user: &UserDir, names: &[String]) -> Vec<(String, Option<String>)> {
    names
        .iter()
        .map(|n| {
            let p = user.path.join("Path of Building").join(n);
            (n.clone(), std::fs::read_to_string(p).ok())
        })
        .collect()
}

/// Run `script` on both sides with `fixture` as the user dir (fresh copies per side). The Lua
/// VM is dropped before the Rust side starts (memory).
fn run_sides(
    root: &Path,
    script: &pob_host::script::Script,
    fixture: Option<&Path>,
    screen: pob_sg::ScreenInfo,
    saved: &[String],
    probe_visibility: bool,
) -> anyhow::Result<(Side, Side)> {
    let user = UserDir::new(fixture)?;

    // Lua
    let handles = OracleHandles::default();
    let lua_app = runner::lua_side(root, &user, &handles, Entry::Launch, false)?;
    let mut lua_side = LuaSide { app: lua_app };
    let mut d = Driver::new(screen, handles, user.path.clone());
    let (mut frames, r) = runner::drive(&mut lua_side, script, &mut d);
    let lua_visibility = probe_visibility.then(|| {
        lua_side
            .app
            .with_vm(|pob| pob.eval::<String>(VISIBILITY_LUA))
            .unwrap_or_else(|e| format!("Lua error: {e}"))
    });
    drop(lua_side);
    drop_image_loads(&mut frames);
    let lua = Side {
        frames,
        error: r.err().map(|e| e.to_string()),
        snapshots: std::mem::take(&mut d.snapshots),
        saved: read_saved(&user, saved),
        visibility: lua_visibility,
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
    let rust_visibility = if probe_visibility {
        std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            rust_visibility(&rust_side.app)
        }))
        .unwrap_or_else(|p| Some(format!("panicked: {}", pob_host::panic_message(&*p))))
    } else {
        None
    };
    drop(rust_side);
    drop_image_loads(&mut frames);
    let rust = Side {
        frames,
        error,
        snapshots: std::mem::take(&mut d.snapshots),
        saved: read_saved(&user, saved),
        visibility: rust_visibility,
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
    for ((name, l), (_, r)) in lua.saved.iter().zip(&rust.saved) {
        match (l, r) {
            (Some(l), Some(r)) => {
                n += 1;
                match compare_t4(&format!("saved {name}"), l, r, notes) {
                    Ok(b) => same += usize::from(b),
                    Err(e) => errs.push(e),
                }
            }
            (None, None) => errs.push(format!("saved {name}: written by neither side")),
            (l, r) => errs.push(format!(
                "saved {name}: lua {} rust {}",
                if l.is_some() { "wrote it" } else { "did not" },
                if r.is_some() { "wrote it" } else { "did not" }
            )),
        }
    }
    if errs.is_empty() {
        Ok((n, same))
    } else {
        Err(errs)
    }
}

/// `#! u41-expect-config ATTR...`: some element of a Lua side saved/snapshot XML must hold every
/// whitespace-separated `ATTR` (`name="x"`, `number="1"`; PoB writes attributes in `pairs`
/// order, so they are matched as a set). Guards a script whose input silently stopped reaching
/// a control, which both sides would agree on.
fn check_expectations(text: &str, lua: &Side) -> Vec<String> {
    let mut errs = Vec::new();
    for want in directives(text, "u41-expect-config") {
        let attrs: Vec<String> = want
            .split_whitespace()
            .map(|a| a.replace("\\t", "\t").replace("\\n", "\n"))
            .collect();
        let element_has_all = |xml: &str| {
            xml.split('<')
                .any(|el| attrs.iter().all(|a| el.contains(&format!(" {a}"))))
        };
        let found = lua
            .snapshots
            .iter()
            .map(|(_, x)| x.as_str())
            .chain(lua.saved.iter().filter_map(|(_, x)| x.as_deref()))
            .any(element_has_all);
        if !found {
            errs.push(format!("no Lua build XML contains {want:?}"));
        }
    }
    // `#! u41-expect-absent ATTR...`: no element of the Lua side's last snapshot holds them all
    for want in directives(text, "u41-expect-absent") {
        let attrs: Vec<&str> = want.split_whitespace().collect();
        let Some((_, last)) = lua.snapshots.last() else {
            errs.push("u41-expect-absent: no snapshot".to_owned());
            continue;
        };
        if last
            .split('<')
            .any(|el| attrs.iter().all(|a| el.contains(&format!(" {a}"))))
        {
            errs.push(format!("the last Lua snapshot still contains {want:?}"));
        }
    }
    errs
}

#[test]
fn u42_scenarios() {
    let _g = LOCK
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let Some(root) = inputs() else { return };
    let mut scripts: Vec<(String, PathBuf)> = Vec::new();
    for dir in test_dirs() {
        let unit = dir.file_name().unwrap().to_string_lossy().into_owned();
        let mut s: Vec<PathBuf> = std::fs::read_dir(&dir)
            .unwrap_or_else(|e| panic!("{}: {e}", dir.display()))
            .filter_map(Result::ok)
            .map(|e| e.path())
            .filter(|p| p.extension().is_some_and(|x| x == "pobscript"))
            .collect();
        s.sort();
        assert!(!s.is_empty(), "no {}/*.pobscript", dir.display());
        scripts.extend(s.into_iter().map(|p| (unit.clone(), p)));
    }
    let filter = std::env::var("U42_FILTER").ok();
    let mut failures = Vec::new();
    let mut ran = 0;
    let (mut t4_pairs, mut t4_bytes) = (0, 0);
    for (unit, path) in scripts {
        let stem = format!(
            "{}{}",
            if unit == "u42" {
                String::new()
            } else {
                format!("{unit}_")
            },
            path.file_stem().unwrap().to_string_lossy()
        );
        if filter.as_ref().is_some_and(|f| !stem.contains(f.as_str())) {
            continue;
        }
        let text = std::fs::read_to_string(&path).unwrap();
        let (script, base_dir) = runner::load_script(&path).unwrap();
        let fx = runner::fixtures(&script, &base_dir).unwrap();
        let own = match script.steps.first() {
            Some(&pob_host::script::Step::Screen { w, h, dpi }) => Some((w, h, dpi)),
            _ => None,
        };
        let own_screen = own.is_some();
        let screens: Vec<(i32, i32, f32)> = own.map_or_else(|| SCREENS.to_vec(), |s| vec![s]);
        let saved = directives(&text, "u41-saved");
        let probe_visibility = text.lines().any(|l| l.trim() == "#! u42-visibility");
        for (w, h, s) in screens {
            let tag = if own_screen {
                format!("u42_{stem}")
            } else {
                format!("u42_{stem}@{w}x{h}@{s}")
            };
            let (lua, rust) = match run_sides(
                &root,
                &script,
                fx.user_dir.as_deref(),
                runner::screen(w, h, s),
                &saved,
                probe_visibility,
            ) {
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
            let mut expect = check_expectations(&text, &lua);
            // `#! u42-visibility`: ConfigVisibility on both sides' live builds
            let mut vis_line = String::new();
            if probe_visibility {
                match (&lua.visibility, &rust.visibility) {
                    (Some(l), Some(r)) if l == r => {
                        let relevant = l.split(' ').filter(|e| e.starts_with('1')).count();
                        let excluded = l.split(' ').filter(|e| e.ends_with('1')).count();
                        let opts = l.split(' ').filter(|e| *e != "-").count();
                        vis_line = format!(
                            ", visibility identical over {opts} options ({relevant} relevant, {excluded} show-all excluded)"
                        );
                    }
                    (l, r) => {
                        let first = l
                            .as_deref()
                            .zip(r.as_deref())
                            .and_then(|(l, r)| {
                                l.split(' ')
                                    .zip(r.split(' '))
                                    .enumerate()
                                    .find(|(_, (a, b))| a != b)
                                    .map(|(i, (a, b))| {
                                        let label = pob_gamedata::data().configOptions[i]
                                            .var
                                            .map_or_else(String::new, |v| v.to_string());
                                        format!("option {i} ({label}): lua {a} rust {b}")
                                    })
                            })
                            .unwrap_or_else(|| format!("lua {l:?} rust {r:?}"));
                        expect.push(format!("visibility differs: {first}"));
                    }
                }
            }
            let run = RunResult {
                lua: lua.frames,
                rust: rust.frames,
                rust_error: rust.error.clone(),
                lua_snapshots: lua.snapshots.clone(),
                rust_snapshots: rust.snapshots.clone(),
                audit: Vec::new(),
                compared: true,
            };
            let png = std::env::var_os("U42_PNG").is_some();
            let n = diff_report::write_report(&out_dir(), &tag, &run, png).unwrap_or(usize::MAX);
            let ops = draw_ops(&run.lua);
            let t4_line = match &t4r {
                Ok((p, b)) => {
                    t4_pairs += p;
                    t4_bytes += b;
                    let mut pending = notes.pending_sections.clone();
                    pending.sort();
                    pending.dedup();
                    format!(
                        "T4 {p} identical incl. Notes ({b} byte-identical; set aside: {} Lua nil gem/group fields, {} pairs-chosen gem variants, pending UI-saver sections differing {:?})",
                        notes.gem_nil_bools, notes.ambiguous_variants, pending
                    )
                }
                Err(e) => format!("T4 FAILED: {}", e.join("; ")),
            };
            println!(
                "{tag}: {} frames, {ops} Lua draw ops, {n} divergences, {t4_line}, titles {:?}{vis_line}{}{}{}",
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
            );
            if n != 0
                || rust.error.is_some()
                || lua.error.is_some()
                || run.lua.len() != run.rust.len()
                || ops == 0
                || titles.0 != titles.1
                || t4r.is_err()
                || !expect.is_empty()
            {
                failures.push(format!(
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
    }
    println!("u42: {ran} runs, T4 {t4_pairs} pairs ({t4_bytes} byte-identical)");
    assert!(ran > 0 || filter.is_some(), "no run");
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}
