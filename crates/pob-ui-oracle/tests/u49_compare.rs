//! U49 acceptance (ui doc §12 U49 row: CompareTab D, CompareTab.lua:3162-4048; "Summary, tree
//! and items views"): the full app (Launch, the build shell, the Compare tab) on both sides of
//! the UI oracle, in u49's form (its harness, T4 rules and directives). Owner: U49.
//!
//! `u49_scenarios`: every `tests/u49/*.pobscript`; a script without its own `screen` step runs at
//! 1280x800, 1600x900 and 1920x1080 (dpi 1.0); a script with one runs once. Each script opens
//! `allflame_exp_001` (u46's fixture user dir), presses the Compare button (the comparison build
//! is imported on the tab's first `Draw` from the script's `#! u49-import FILE`, both sides) and
//! drives the Summary, Tree and Items sub-views. Per run: zero T3 divergences (draw ops per
//! `(layer, sub)` bucket and side effects, frame by frame), the same window titles, and T4 at
//! every `snapshot` step (u46's rules).
//!
//! What runs on each side:
//! * `DrawSummary`, `DrawStatList`, `DrawTree`, `DrawItemExpanded`, `ShouldShowRing3`,
//!   `AddAbyssSockets`, `DrawItems` (with TradeHelpers' row, button and mod-map helpers):
//!   PoB's (Lua) and U49's (Rust);
//! * `CompareTab:Draw` and its layout passes (U49), the constructor and methods (U46), the power
//!   report driver and its list control (U48, drawn on both sides), `CompareEntry` (U49; the
//!   Rust entry's calc is U15's LuaCalcBridge, as the primary build's), the passive tree view
//!   (U23/U24: both trees of the Tree view are drawn), the ItemsTab tooltips (U38) and the
//!   stat-comparison tooltip (U21) on hover;
//! * `DrawSkills`, `DrawCalcs`, `DrawConfig` (U50) are not reached (the scripts do not open
//!   those sub-views); every other tab's `Draw` draws nothing (as u22), plus u22's `utf8.next`.

#![allow(non_snake_case, clippy::too_many_lines)]

use std::cell::{Cell, RefCell};
use std::path::{Path, PathBuf};
use std::rc::Rc;

use pob_app::app::CxApp;
use pob_app::build::init;
use pob_app::build::on_frame::{self as of, BuildTab, TabFlag};
use pob_app::tabs::compare::init as ci;
use pob_app::tabs::compare::layout as cl;
use pob_app::tabs::compare::CompareTab;
use pob_calc::model::{XmlChild, XmlNode};
use pob_host::{App, PlatformParts};
use pob_sg::trace::{ApiCall, Buckets, DiffConfig, Divergence, DrawOp, FrameTrace, SideEffect};
use pob_sg::{Key, Sg, Sys};
use pob_ui_core::prelude::*;
use pob_ui_oracle::boot::{self, Entry, OracleHandles};
use pob_ui_oracle::diff_report;
use pob_ui_oracle::lua_calc::LuaCalcBridge;
use pob_ui_oracle::runner::{self, Driver, LuaApp, RunResult, ScenarioApp, UserDir};

/// One LuaJIT PoB at a time (memory; ui doc §11.3).
static LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

const SCREENS: [(i32, i32, f32); 3] = [(1280, 800, 1.0), (1600, 900, 1.0), (1920, 1080, 1.0)];

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
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/u49")
}

/// The Lua stand-ins (see the module notes): u22's `utf8.next`, the other tabs' `Draw`, and
/// the import on the Compare tab's first `Draw`.
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
	for _, t in ipairs({ "importTab", "notesTab", "partyTab", "configTab", "treeTab", "skillsTab", "itemsTab", "calcsTab" }) do
		if self[t] then self[t].Draw = function() end end
	end
	local ct = self.compareTab
	if not ct then return end
	local cls = common.classes.CompareTab
	function ct:Draw(viewPort, inputEvents)
		if U49_XML and not self.u49Imported then
			self.u49Imported = true
			self:ImportBuild(U49_XML, U49_LABEL)
		end
		return cls.Draw(self, viewPort, inputEvents)
	end
end
"#;

// ---- the Rust stand-ins ----

thread_local! {
    /// The stand-in `spec.modFlag` (UndoHandler.lua:37 via `spec:AddUndoState`, U25), as u46.
    static SPEC_MOD_FLAG: Cell<bool> = const { Cell::new(false) };
    /// `#! u49-import`: (xml, label)
    static IMPORT: RefCell<Option<(String, String)>> = const { RefCell::new(None) };
    /// `self.u49Imported`
    static IMPORTED: Cell<bool> = const { Cell::new(false) };
}

/// The stub's `ct:Draw`: the import on the first Draw, then the real Draw.
fn draw_compare(cx: &mut Cx, vp: &ViewPort, ev: &mut InputEvents) {
    let me = ci::tab(cx).expect("compareTab");
    if !IMPORTED.with(Cell::get) {
        if let Some((xml, label)) = IMPORT.with(|i| i.borrow().clone()) {
            IMPORTED.with(|i| i.set(true));
            CompareTab::ImportBuild(cx, me, Some(&xml), Some(&label));
        }
    }
    cl::draw_tab(cx, vp, ev);
}

fn rust_stubs() {
    SPEC_MOD_FLAG.with(|f| f.set(false));
    IMPORTED.with(|i| i.set(false));
    init::set_hooks(|h| {
        *h = init::default_hooks();
        h.spec_add_undo_state = Rc::new(|_| SPEC_MOD_FLAG.with(|f| f.set(true)));
    });
    // U48's real power report list, U49's passes (the defaults)
    ci::set_compare_hooks(|h| *h = ci::default_compare_hooks());
    cl::set_draw_hooks(|h| *h = cl::default_draw_hooks());
    of::set_shell_hooks(|h| {
        *h = of::default_shell_hooks();
        h.draw_tab = Rc::new(move |cx, tab, vp, ev| match tab {
            BuildTab::Compare => draw_compare(cx, vp, ev),
            // LUA_STUBS replaces the other tabs' Draw
            BuildTab::Notes
            | BuildTab::Party
            | BuildTab::Tree
            | BuildTab::Skills
            | BuildTab::Items
            | BuildTab::Calcs
            | BuildTab::Config
            | BuildTab::Import => {}
        });
        let (flag, set_flag) = (h.tab_flag.clone(), h.set_tab_flag.clone());
        h.tab_flag = Rc::new(move |cx, f| {
            if f == TabFlag::Spec {
                SPEC_MOD_FLAG.with(Cell::get)
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

/// `#! NAME ARG` directives.
fn directives(text: &str, name: &str) -> Vec<String> {
    let tag = format!("#! {name}");
    text.lines()
        .filter_map(|l| l.trim().strip_prefix(tag.as_str()))
        .map(|s| s.trim().to_owned())
        .collect()
}

/// `#! u49-import FILE` (relative to the script's user dir `Path of Building/Builds`): the XML
/// and the label (the file stem).
fn import_of(text: &str, user_dir: Option<&Path>) -> Option<(String, String)> {
    let f = directives(text, "u49-import").into_iter().next()?;
    let p = user_dir?.join("Path of Building/Builds").join(&f);
    let xml = std::fs::read_to_string(&p).unwrap_or_else(|e| panic!("{}: {e}", p.display()));
    let label = Path::new(&f).file_stem().unwrap().to_string_lossy().into_owned();
    Some((xml, label))
}

// ---- the Lua side ----

struct LuaSide {
    app: LuaApp,
    import: Option<(String, String)>,
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
            .with_vm(|pob| {
                if let Some((xml, label)) = &self.import {
                    pob.lua.globals().set("U49_XML", xml.as_str())?;
                    pob.lua.globals().set("U49_LABEL", label.as_str())?;
                }
                pob.eval::<()>(LUA_STUBS)
            })
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

/// Records the platform side effects the Lua side's bridge records (as in u16-u45).
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
    import: Option<(String, String)>,
}

impl RustSide {
    fn new(mut parts: PlatformParts, src: PathBuf, import: Option<(String, String)>) -> RustSide {
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
        RustSide { app, effects, import }
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
        IMPORT.with(|i| *i.borrow_mut() = self.import.clone());
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

// ---- T4 (the rules are u20's; see its module notes) ----

fn unordered_children(elem: &str) -> bool {
    matches!(elem, "Calcs" | "ConfigSet" | "ItemSet" | "Sockets" | "Overrides")
}

/// The UI-only savers compared separately (a difference is reported, not failed).
const PENDING_SAVERS: [&str; 2] = ["Import", "TreeView"];

#[derive(Default)]
struct T4Notes {
    pending_sections: Vec<String>,
    gem_nil_bools: usize,
    ambiguous_variants: usize,
}

fn ambiguous_variant(node: &XmlNode) -> bool {
    let (Some(game_id), Some(skill_id)) = (node.attrib.get("gemId"), node.attrib.get("skillId")) else {
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
            ("Gem", "enableGlobal1" | "enableGlobal2") | ("Skill", "includeInFullDPS") => Some("false"),
            ("Skill", "mainActiveSkill" | "mainActiveSkillCalcs") => Some("0"),
            _ => None,
        };
        if let Some(d) = rust_default.filter(|_| lua && &**v == "nil") {
            notes.gem_nil_bools += 1;
            attrs.push((k.to_string(), d.to_owned()));
        } else if &*node.elem == "Gem" && matches!(&**k, "variantId" | "nameSpec") && ambiguous_variant(node) {
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
    for ch in &node.children {
        match ch {
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
        .find_map(|ch| match ch {
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
        root.children.iter().find_map(|ch| match ch {
            XmlChild::Node(n) if *n.elem == *elem => Some(normalise(n, false, lua, &mut T4Notes::default())),
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
}

/// Run `script` on both sides with `fixture` as the user dir (fresh copies per side). The Lua
/// VM is dropped before the Rust side starts (memory).
fn run_sides(
    root: &Path,
    script: &pob_host::script::Script,
    fixture: Option<&Path>,
    screen: pob_sg::ScreenInfo,
    import: Option<(String, String)>,
) -> anyhow::Result<(Side, Side)> {
    let user = UserDir::new(fixture)?;

    // Lua
    let handles = OracleHandles::default();
    let lua_app = runner::lua_side(root, &user, &handles, Entry::Launch, false)?;
    let mut lua_side = LuaSide {
        app: lua_app,
        import: import.clone(),
    };
    let mut d = Driver::new(screen, handles, user.path.clone());
    let (mut frames, r) = runner::drive(&mut lua_side, script, &mut d);
    drop(lua_side);
    drop_image_loads(&mut frames);
    let lua = Side {
        frames,
        error: r.err().map(|e| e.to_string()),
        snapshots: std::mem::take(&mut d.snapshots),
    };

    // Rust
    user.reset()?;
    let handles = OracleHandles::default();
    let (sg, sys) = boot::oracle_parts(root, &user.path, &handles)?;
    let src = std::fs::canonicalize(root.join("src")).unwrap_or_else(|_| root.join("src"));
    let mut rust_side = RustSide::new(PlatformParts { sg, sys }, src, import);
    let mut d = Driver::new(screen, handles, user.path.clone());
    let res = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| runner::drive(&mut rust_side, script, &mut d)));
    let (mut frames, error) = match res {
        Ok((frames, r)) => (frames, r.err().map(|e| e.to_string())),
        Err(p) => (Vec::new(), Some(format!("panicked: {}", pob_host::panic_message(&*p)))),
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
        .map_or_else(|| PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../target"), PathBuf::from)
        .join("ui-parity")
}

// ---- T3 with the tree's multiset buckets (u25's rules) ----

/// The multiset sub-layers: sub-layers 0..=30 of layer 0, where PassiveTreeView draws its
/// `pairs` loops (tree_view/draw.rs ORDER notes; per-process random order in PoB). A frame is
/// compared in order first; only a frame whose ordered diff fails in those buckets is compared
/// with them as multisets (as u23/u25).
fn multiset_bucket(k: (i32, i32)) -> bool {
    k.0 == 0 && (0..=30).contains(&k.1)
}

/// A sort key of an op: its arguments at 1e-3 (u25's).
fn op_key(op: &DrawOp) -> String {
    let r = |x: f64| format!("{x:.3}");
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

/// One frame's divergences (u25's `diff_frame`).
fn diff_frame(lua: &FrameTrace, rust: &FrameTrace) -> Vec<Divergence> {
    let screen = lua
        .screen
        .or(rust.screen)
        .unwrap_or_else(|| runner::screen(1280, 800, 1.0));
    let lb = pob_sg::trace::normalize(lua, &screen);
    let rb = pob_sg::trace::normalize(rust, &screen);
    let ordered = DiffConfig::default();
    let mut out = pob_sg::trace::diff(&lb, &rb, &ordered);
    if out.iter().any(|d| match d {
        Divergence::Missing { bucket, .. } | Divergence::Extra { bucket, .. } | Divergence::Changed { bucket, .. } => {
            multiset_bucket(*bucket)
        }
        Divergence::Effect { .. } => false,
    }) {
        let sort = |mut b: Buckets| {
            for (k, ops) in &mut b {
                if multiset_bucket(*k) {
                    ops.sort_by_cached_key(op_key);
                }
            }
            b
        };
        let (lb, rb) = (sort(lb), sort(rb));
        out = pob_sg::trace::diff(&lb, &rb, &ordered);
        if !out.is_empty() {
            let cfg = DiffConfig {
                multiset: vec![((0, 0), (0, 30))],
                ..DiffConfig::default()
            };
            out = pob_sg::trace::diff(&lb, &rb, &cfg);
        }
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
    let op = |o: &DrawOp| format!("{:?} color {:?} vp {:?} ({:?} {:?})", o.call, o.color, o.viewport, o.src, o.ctl);
    match d {
        Divergence::Missing { bucket, idx, op: o } => format!("missing on rust {bucket:?}#{idx}: {}", op(o)),
        Divergence::Extra { bucket, idx, op: o } => format!("extra on rust {bucket:?}#{idx}: {}", op(o)),
        Divergence::Changed {
            bucket,
            idx,
            lua,
            rust,
        } => format!("changed {bucket:?}#{idx}: lua {} | rust {}", op(lua), op(rust)),
        Divergence::Effect { lua, rust } => format!("effect: lua {lua:?} rust {rust:?}"),
    }
}

fn draw_ops(frames: &[FrameTrace]) -> usize {

    frames
        .iter()
        .flat_map(|f| &f.calls)
        .filter(|c| matches!(c.call, ApiCall::DrawImage { .. } | ApiCall::DrawImageQuad { .. } | ApiCall::DrawString { .. }))
        .count()
}

/// The texts of a run's DrawString calls (for `#! u49-expect-text`).
fn draw_texts(frames: &[FrameTrace]) -> Vec<String> {

    frames
        .iter()
        .flat_map(|f| &f.calls)
        .filter_map(|c| match &c.call {
            ApiCall::DrawString { text, .. } => Some(text.clone()),
            _ => None,
        })
        .collect()
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
        errs.push(format!("snapshots: lua {} rust {}", lua.snapshots.len(), rust.snapshots.len()));
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

/// `#! u49-expect-text TEXT`: some Lua DrawString contains TEXT (guards a script whose clicks
/// silently stopped reaching the tab, which both sides would agree on); `#! u49-expect-snapshot
/// NAME TEXT`: the Lua snapshot NAME contains TEXT.
fn check_expectations(text: &str, lua: &Side) -> Vec<String> {
    let mut errs = Vec::new();
    let texts = draw_texts(&lua.frames);
    for want in directives(text, "u49-expect-text") {
        if !texts.iter().any(|t| t.contains(want.as_str())) {
            errs.push(format!("no Lua DrawString contains {want:?}"));
        }
    }
    for d in directives(text, "u49-expect-snapshot") {
        let (name, want) = d.split_once(' ').unwrap_or((d.as_str(), ""));
        let found = lua.snapshots.iter().any(|(n, x)| n == name && x.contains(want));
        if !found {
            errs.push(format!("Lua snapshot {name} does not contain {want:?}"));
        }
    }
    errs
}

#[test]
fn u49_scenarios() {
    let _g = LOCK.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
    let Some(root) = inputs() else { return };
    let mut scripts: Vec<PathBuf> = std::fs::read_dir(test_dir())
        .expect("tests/u49")
        .filter_map(Result::ok)
        .map(|e| e.path())
        .filter(|p| p.extension().is_some_and(|x| x == "pobscript"))
        .collect();
    scripts.sort();
    assert!(!scripts.is_empty(), "no tests/u49/*.pobscript");
    let filter = std::env::var("U49_FILTER").ok();
    let only_screen = std::env::var("U49_SCREEN").ok();
    let mut failures = Vec::new();
    let mut ran = 0;
    let (mut t4_pairs, mut t4_bytes) = (0, 0);
    for path in scripts {
        let stem = path.file_stem().unwrap().to_string_lossy().into_owned();
        if filter.as_ref().is_some_and(|f| !stem.contains(f.as_str())) {
            continue;
        }
        let text = std::fs::read_to_string(&path).unwrap();
        let (script, base_dir) = runner::load_script(&path).unwrap();
        let fx = runner::fixtures(&script, &base_dir).unwrap();
        let own_screen = matches!(script.steps.first(), Some(pob_host::script::Step::Screen { .. }));
        let screens: Vec<(i32, i32, f32)> = if own_screen {
            vec![(1280, 800, 1.0)]
        } else {
            SCREENS.to_vec()
        };
        for (w, h, s) in screens {
            if only_screen.as_ref().is_some_and(|o| *o != w.to_string()) {
                continue;
            }
            let tag = if own_screen {
                format!("u49_{stem}")
            } else {
                format!("u49_{stem}@{w}x{h}@{s}")
            };
            let import = import_of(&text, fx.user_dir.as_deref());
            let (lua, rust) = match run_sides(&root, &script, fx.user_dir.as_deref(), runner::screen(w, h, s), import) {
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
            let divs = diff_run(&run.lua, &run.rust);
            let n: usize = divs.iter().map(|(_, d)| d.len().max(1)).sum();
            // the jsonl traces for inspection (its report.txt counts ordered divergences)
            let _ = diff_report::write_report(&out_dir(), &tag, &run, false);
            if let Some((fi, d)) = divs.first() {
                println!(
                    "{tag}: first divergence frame {fi}: {}",
                    d.first().map_or_else(|| "frame missing".to_owned(), describe)
                );
            }
            let ops = draw_ops(&run.lua);
            let t4_line = match &t4r {
                Ok((p, b)) => {
                    t4_pairs += p;
                    t4_bytes += b;
                    let mut pending = notes.pending_sections.clone();
                    pending.sort();
                    pending.dedup();
                    format!(
                        "T4 {p} identical ({b} byte-identical; set aside: {} Lua nil gem/group fields, {} pairs-chosen gem variants, pending UI-saver sections differing {:?})",
                        notes.gem_nil_bools, notes.ambiguous_variants, pending
                    )
                }
                Err(e) => format!("T4 FAILED: {}", e.join("; ")),
            };
            println!(
                "{tag}: {} frames, {ops} Lua draw ops, {n} divergences, {t4_line}, titles {:?}{}{}{}",
                run.lua.len(),
                distinct_titles(&titles.0),
                lua.error.as_ref().map_or_else(String::new, |e| format!(", Lua error: {e}")),
                rust.error.as_ref().map_or_else(String::new, |e| format!(", Rust error: {e}")),
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
    println!("u49: {ran} runs, T4 {t4_pairs} pairs ({t4_bytes} byte-identical)");
    assert!(ran > 0 || filter.is_some(), "no run");
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}
