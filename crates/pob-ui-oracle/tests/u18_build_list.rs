//! U18 acceptance (ui doc §12 U18 row): Modules/BuildList.lua, BuildListHelpers.lua,
//! BuildListControl.lua and FolderListControl.lua on both sides of the UI oracle. Owner: U18.
//!
//! `u18_scenarios`: every `tests/u18/*.pobscript` at the acceptance screens (a script that sets
//! its own `screen` runs once), over the fixture user dirs `tests/u18/user*` (builds in nested
//! folders, headers with and without class/level). Zero T3 divergences: the same draw ops per
//! `(layer, sub)` bucket and the same side effects, frame by frame. After the run the build
//! folders of both sides must hold the same files (renames, copies, moves, deletes, new folders),
//! and Settings.xml must be written identically up to attribute order (T4).
//!
//! Why not `scenarios/u18/` under the shared parity runner: the Build mode (U19-U21) is not
//! ported and `main:DrawBackground` (U17) needs U23's tree assets. Both sides run against the
//! same stand-ins: after `OnInit`, the Lua side's `main.modes.BUILD` becomes an object that draws
//! nothing and echoes its first two `Init` arguments from `GetArgs` (exactly pob-app's
//! `PendingMode`, CCR-0096), and `main:DrawBackground` draws nothing on both sides (CCR-0100:
//! `pob_app::build_list::set_draw_background_override`).
//!
//! Compared side effects exclude `ImageLoad` (frame 0 loads the images of modules other units
//! port; none of U18's lines loads an image).

use std::cell::RefCell;
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::rc::Rc;

use pob_app::app::CxApp;
use pob_app::build_list::folder_list_control::FolderListControl;
use pob_host::{App, PlatformParts};
use pob_sg::trace::{FrameTrace, SideEffect};
use pob_sg::{Key, Sg, Sys};
use pob_ui_core::cx::Cx;
use pob_ui_core::prelude::{rect, Anchor};
use pob_ui_oracle::boot::{self, Entry, OracleHandles};
use pob_ui_oracle::diff_report;
use pob_ui_oracle::runner::{self, Driver, LuaApp, RunResult, ScenarioApp, UserDir};

/// FolderListControl's `onChange`.
type OnChange = Rc<dyn Fn(&mut Cx, &str)>;

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
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/u18")
}

/// The stand-ins both sides run (see the module notes).
const LUA_STUBS: &str = r"
local function stubMode()
	local m = { }
	function m:Init(a1, a2) self.a1, self.a2 = a1, a2 end
	function m:GetArgs() return self.a1, self.a2 end
	function m:OnFrame(inputEvents, viewPort) end
	return m
end
main.modes.BUILD = stubMode()
function main:DrawBackground(viewPort) end
";

/// `#! u18-setup NAME` steps run right after `OnInit` on both sides.
fn lua_setup(name: &str) -> &'static str {
    match name {
        // The Save As popup's folder picker (Build.lua:1481), alone in a popup.
        "folders" => {
            r#"local controls = { }
controls.folder = new("FolderListControl"):FolderListControl(nil, {0, 50, 450, 300}, "", function(subPath) end)
main:OpenPopup(470, 400, "Folders", controls)"#
        }
        "folders_edited" => {
            r#"local controls = { }
controls.folder = new("FolderListControl"):FolderListControl(nil, {0, 50, 450, 300}, "Leagues/", nil)
controls.folder.sortMode = "EDITED"
controls.folder:SortList()
main:OpenPopup(470, 400, "Folders", controls)"#
        }
        other => panic!("unknown u18-setup {other}"),
    }
}

fn rust_setup(cx: &mut Cx, name: &str) {
    match name {
        "folders" | "folders_edited" => {
            let edited = name == "folders_edited";
            pob_ui_core::popups::open_popup(
                cx,
                470.0,
                400.0,
                "Folders",
                move |cx, p| {
                    let cb: Option<OnChange> = if edited {
                        None
                    } else {
                        Some(Rc::new(|_cx, _sub_path| {}))
                    };
                    let f = cx.add(
                        p,
                        "folder",
                        FolderListControl::new(
                            Anchor::NONE,
                            rect(0, 50, 450, 300),
                            Some(if edited { "Leagues/" } else { "" }.to_owned()),
                            cb,
                        ),
                    );
                    if edited {
                        cx.get_mut(f).sort_mode = Some("EDITED".to_owned());
                        FolderListControl::sort_list(cx, f.id());
                    }
                },
                pob_ui_core::popups::PopupOpts::default(),
            );
        }
        other => panic!("unknown u18-setup {other}"),
    }
}

fn setups(text: &str) -> Vec<String> {
    text.lines()
        .filter_map(|l| l.trim().strip_prefix("#! u18-setup "))
        .map(|s| s.trim().to_owned())
        .collect()
}

// ---- the Lua side ----

struct LuaSide {
    app: LuaApp,
    setups: Vec<String>,
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
        let mut code = LUA_STUBS.to_owned();
        for s in &self.setups {
            code.push_str(lua_setup(s));
            code.push('\n');
        }
        self.app
            .with_vm(|pob| pob.eval::<()>(&code))
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
    fn load_build(&mut self, _name: &str, _xml: &str) -> anyhow::Result<()> {
        anyhow::bail!("u18 scenarios do not load builds")
    }
    fn snapshot(&mut self) -> anyhow::Result<Option<String>> {
        Ok(None)
    }
    // PoB's error prompt fails the run (a Lua error in the unit's code on the Lua side).
    fn after_frame(&mut self) -> anyhow::Result<()> {
        self.app.after_frame()
    }
    fn take_error(&mut self) -> Option<String> {
        self.app.take_error()
    }
}

// ---- the Rust side ----

/// Records the platform side effects the Lua side's bridge records (the oracle's Rust platform
/// does not, CCR-0028 item 2).
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
    setups: Vec<String>,
    effects: Rc<RefCell<Vec<SideEffect>>>,
}

impl RustSide {
    fn new(mut parts: PlatformParts, setups: Vec<String>) -> RustSide {
        let effects = Rc::new(RefCell::new(Vec::new()));
        let inner = std::mem::replace(&mut parts.sys.platform, Box::new(NullPlatform));
        parts.sys.platform = Box::new(RecordingPlatform {
            inner,
            effects: effects.clone(),
        });
        let mut app = pob_app::PobApp::new(parts);
        effects.borrow_mut().clear(); // Launch.lua:11 runs at script load, before tracing
                                      // pob-ui-oracle patch 4's equivalent (CCR-0096)
        app.cx.app_mut().launch.force_release_install = true;
        // CCR-0100: main:DrawBackground draws nothing on both sides
        pob_app::build_list::set_draw_background_override(Some(Rc::new(|_cx, _vp| {})));
        RustSide {
            app,
            setups,
            effects,
        }
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

impl Drop for RustSide {
    fn drop(&mut self) {
        pob_app::build_list::set_draw_background_override(None);
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
        self.app.on_init();
        for s in self.setups.clone() {
            rust_setup(&mut self.app.cx, &s);
        }
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
    fn load_build(&mut self, _name: &str, _xml: &str) -> anyhow::Result<()> {
        anyhow::bail!("u18 scenarios do not load builds")
    }
    fn snapshot(&mut self) -> anyhow::Result<Option<String>> {
        Ok(None)
    }
}

// ---- runs ----

fn drop_image_loads(frames: &mut [FrameTrace]) {
    for f in frames {
        f.effects.retain(|e| !matches!(e, SideEffect::ImageLoad(_)));
    }
}

/// Every file and directory below `dir` (relative path -> file bytes; directories map to None).
fn tree(dir: &Path) -> BTreeMap<String, Option<Vec<u8>>> {
    fn walk(base: &Path, d: &Path, out: &mut BTreeMap<String, Option<Vec<u8>>>) {
        let Ok(rd) = std::fs::read_dir(d) else { return };
        for e in rd.flatten() {
            let p = e.path();
            let rel = p.strip_prefix(base).unwrap().to_string_lossy().into_owned();
            if p.is_dir() {
                out.insert(format!("{rel}/"), None);
                walk(base, &p, out);
            } else {
                out.insert(rel, std::fs::read(&p).ok());
            }
        }
    }
    let mut out = BTreeMap::new();
    walk(dir, dir, &mut out);
    out
}

struct Sides {
    lua: Vec<FrameTrace>,
    rust: Vec<FrameTrace>,
    rust_error: Option<String>,
    lua_tree: BTreeMap<String, Option<Vec<u8>>>,
    rust_tree: BTreeMap<String, Option<Vec<u8>>>,
    lua_settings: Option<String>,
    rust_settings: Option<String>,
}

/// Run `script` on both sides with `fixture` as the user dir (fresh copies per side).
fn run_sides(
    root: &Path,
    script: &pob_host::script::Script,
    fixture: Option<&Path>,
    screen: pob_sg::ScreenInfo,
    setups: &[String],
) -> anyhow::Result<Sides> {
    let user = UserDir::new(fixture)?;
    let builds = user.path.join("Path of Building/Builds");
    let settings_file = user.path.join("Path of Building/Settings.xml");

    // Lua
    let handles = OracleHandles::default();
    let lua_app = runner::lua_side(root, &user, &handles, Entry::Launch, false)?;
    let mut lua_side = LuaSide {
        app: lua_app,
        setups: setups.to_vec(),
    };
    let mut d = Driver::new(screen, handles, user.path.clone());
    let (mut lua, r) = runner::drive(&mut lua_side, script, &mut d);
    r.map_err(|e| anyhow::anyhow!("Lua side: {e}"))?;
    lua_side
        .app
        .with_vm(|pob| pob.eval::<()>("main:SaveSettings()"))
        .map_err(|e| anyhow::anyhow!("Lua SaveSettings: {e}"))?;
    let lua_settings = std::fs::read_to_string(&settings_file).ok();
    drop(lua_side);
    let lua_tree = tree(&builds);

    // Rust
    user.reset()?;
    let handles = OracleHandles::default();
    let (sg, sys) = boot::oracle_parts(root, &user.path, &handles)?;
    let mut rust_side = RustSide::new(PlatformParts { sg, sys }, setups.to_vec());
    let mut d = Driver::new(screen, handles, user.path.clone());
    let res = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        runner::drive(&mut rust_side, script, &mut d)
    }));
    let (mut rust, rust_error) = match res {
        Ok((frames, r)) => (frames, r.err().map(|e| e.to_string())),
        Err(p) => (
            Vec::new(),
            Some(format!("panicked: {}", pob_host::panic_message(&*p))),
        ),
    };
    let mut rust_settings = None;
    if rust_error.is_none() {
        pob_app::main_mode::settings::save_settings(&mut rust_side.app.cx);
        rust_settings = std::fs::read_to_string(&settings_file).ok();
    }
    drop(rust_side);
    let rust_tree = tree(&builds);
    drop_image_loads(&mut lua);
    drop_image_loads(&mut rust);
    Ok(Sides {
        lua,
        rust,
        rust_error,
        lua_tree,
        rust_tree,
        lua_settings,
        rust_settings,
    })
}

/// Attribute-order normal form of an XML text (xml.lua's own reader and writer).
fn normalise(xml: &str) -> String {
    use pob_app::model::{XmlChild, XmlNode};
    fn sort(n: &mut XmlNode) {
        n.attrib.sort_keys();
        for c in &mut n.children {
            if let XmlChild::Node(c) = c {
                sort(c);
            }
        }
    }
    let mut root = pob_app::model::xml::ParseXML_root(xml).expect("parse");
    sort(&mut root);
    let XmlChild::Node(top) = &root.children[0] else {
        panic!("no root element")
    };
    pob_app::model::xml::ComposeXML(top)
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

/// `#! expect-text TEXT`: some DrawString of the run contains TEXT (the scenario reached the
/// state it is about, on the Lua side).
fn check_texts(lua: &[FrameTrace], text: &str) -> Result<(), String> {
    use pob_sg::trace::ApiCall;
    for t in text
        .lines()
        .filter_map(|l| l.trim().strip_prefix("#! expect-text "))
    {
        let t = t.trim();
        let found = lua.iter().flat_map(|f| &f.calls).any(|c| match &c.call {
            ApiCall::DrawString { text, .. } => text.contains(t),
            _ => false,
        });
        if !found {
            return Err(format!("no DrawString containing {t:?}"));
        }
    }
    Ok(())
}

#[test]
fn u18_scenarios() {
    let _g = LOCK
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let Some(root) = inputs() else { return };
    let mut scripts: Vec<PathBuf> = std::fs::read_dir(test_dir())
        .expect("tests/u18")
        .filter_map(Result::ok)
        .map(|e| e.path())
        .filter(|p| p.extension().is_some_and(|x| x == "pobscript"))
        .collect();
    scripts.sort();
    assert!(!scripts.is_empty(), "no tests/u18/*.pobscript");
    let filter = std::env::var("U18_FILTER").ok();
    let mut failures = Vec::new();
    let mut ran = 0;
    for path in scripts {
        let stem = path.file_stem().unwrap().to_string_lossy().into_owned();
        if filter.as_ref().is_some_and(|f| !stem.contains(f.as_str())) {
            continue;
        }
        let text = std::fs::read_to_string(&path).unwrap();
        let (script, base_dir) = runner::load_script(&path).unwrap();
        let fx = runner::fixtures(&script, &base_dir).unwrap();
        let own_screen = matches!(
            script.steps.first(),
            Some(pob_host::script::Step::Screen { .. })
        );
        let screens: Vec<(i32, i32, f32)> = if own_screen {
            vec![(1280, 800, 1.0)]
        } else {
            runner::ACCEPTANCE_SCREENS.to_vec()
        };
        for (w, h, s) in screens {
            let tag = format!("u18_{stem}@{w}x{h}@{s}");
            let sides = match run_sides(
                &root,
                &script,
                fx.user_dir.as_deref(),
                runner::screen(w, h, s),
                &setups(&text),
            ) {
                Ok(s) => s,
                Err(e) => {
                    failures.push(format!("{tag}: {e:#}"));
                    continue;
                }
            };
            ran += 1;
            let texts = check_texts(&sides.lua, &text);
            let run = RunResult {
                lua: sides.lua,
                rust: sides.rust,
                rust_error: sides.rust_error.clone(),
                lua_snapshots: Vec::new(),
                rust_snapshots: Vec::new(),
                audit: Vec::new(),
                compared: true,
            };
            let n = diff_report::write_report(&out_dir(), &tag, &run, false).unwrap_or(usize::MAX);
            let ops = draw_ops(&run.lua);
            let same_tree = sides.lua_tree == sides.rust_tree;
            let same_settings = match (&sides.lua_settings, &sides.rust_settings) {
                (Some(l), Some(r)) => normalise(l) == normalise(r),
                _ => false,
            };
            println!(
                "{tag}: {} frames, {ops} Lua draw ops, {n} divergences, {} files/dirs, same tree: {same_tree}, same Settings.xml: {same_settings}{}",
                run.lua.len(),
                sides.lua_tree.len(),
                sides
                    .rust_error
                    .as_ref()
                    .map_or_else(String::new, |e| format!(", Rust error: {e}"))
            );
            if n != 0
                || sides.rust_error.is_some()
                || run.lua.len() != run.rust.len()
                || ops == 0
                || !same_tree
                || !same_settings
                || texts.is_err()
            {
                let diff_files: Vec<_> = sides
                    .lua_tree
                    .keys()
                    .chain(sides.rust_tree.keys())
                    .filter(|k| sides.lua_tree.get(*k) != sides.rust_tree.get(*k))
                    .collect();
                failures.push(format!(
                    "{tag}: {n} divergences, frames lua {} rust {}, {ops} ops, tree differs at {diff_files:?}, settings same {same_settings}, texts {texts:?} (see {})",
                    run.lua.len(),
                    run.rust.len(),
                    out_dir().join(&tag).join("report.txt").display()
                ));
            }
        }
    }
    assert!(ran > 0 || filter.is_some(), "no run");
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}
