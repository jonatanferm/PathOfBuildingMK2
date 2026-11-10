//! U16 acceptance (ui doc §12 U16 row): Launch.lua, Main.lua's core and ToastNotification.lua on
//! both sides of the UI oracle. Owner: U16.
//!
//! * `u16_scenarios`: every `tests/u16/*.pobscript` at the acceptance screens (a script that sets
//!   its own `screen` runs once). Zero T3 divergences: the same draw ops per `(layer, sub)`
//!   bucket and the same side effects, frame by frame.
//! * `u16_settings_roundtrip` (T4): every `tests/u16/settings/*.xml` is loaded by `main:Init`
//!   (`ChangeUserPath` -> `LoadSettings`/`LoadSharedItems`) and written back by
//!   `main:SaveSettings` on both sides. The Rust file is byte-identical to the input (the corpus
//!   is written in the port's canonical attribute order), and identical to PoB's after
//!   attribute-order normalisation (PoB writes attributes in `pairs` order, which is random per
//!   process; ui doc §11.1 T4). `*.migrate.xml` inputs (legacy attributes, missing attributes,
//!   an invalid mode or root) are rewritten by PoB itself; for them only Rust == PoB is checked.
//!
//! Why not `scenarios/u16/` under the shared parity runner: the mode objects (`main.modes.LIST`
//! = BuildList, U18; `main.modes.BUILD` = Build, U19-U21) and the Options/About/Update popups
//! (U17) are not ported yet, so a full-app scenario diverges in their draws. Here both sides run
//! U16's code against the same stand-ins: after `OnInit` the Lua side's `main.modes.LIST/BUILD`
//! become objects that draw nothing and echo their first two `Init` arguments from `GetArgs`
//! (exactly pob-app's `PendingMode`, CCR-0096), and `main:OpenAboutPopup` / `OpenOptionsPopup`
//! open a message popup naming their argument on both sides.
//!
//! Compared side effects exclude `ImageLoad`: the Lua side's frame 0 loads the images of modules
//! other units port (Classes/Tooltip.lua's recipe images are matched; PassiveTree's sprites,
//! U23, are not), and none of U16's lines loads an image. `FileWrite` is recorded by neither
//! side (CCR-0028 item 3); T4 checks the written Settings.xml instead.

use std::cell::RefCell;
use std::path::{Path, PathBuf};
use std::rc::Rc;

use pob_app::app::CxApp;
use pob_app::main_mode::{self, ModeArg};
use pob_host::{App, PlatformParts};
use pob_sg::trace::{FrameTrace, SideEffect};
use pob_sg::{Key, Sg, Sys};
use pob_ui_core::cx::Cx;
use pob_ui_oracle::boot::{self, Entry, OracleHandles};
use pob_ui_oracle::diff_report;
use pob_ui_oracle::runner::{self, Driver, LuaApp, RunResult, ScenarioApp, UserDir};

/// One LuaJIT PoB at a time (memory; ui doc §11.3).
static LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

fn inputs() -> Option<PathBuf> {
    let root = pob_testpaths::require(
        pob_testpaths::pob_root(),
        "PoB checkout (.PathOfBuilding)",
    )?;
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
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/u16")
}

/// The stand-ins both sides run (see the module notes).
const LUA_STUBS: &str = r#"
local function stubMode()
	local m = { }
	function m:Init(a1, a2) self.a1, self.a2 = a1, a2 end
	function m:GetArgs() return self.a1, self.a2 end
	function m:OnFrame(inputEvents, viewPort) end
	return m
end
main.modes.LIST = stubMode()
main.modes.BUILD = stubMode()
function main:OpenAboutPopup(helpSectionName) self:OpenMessagePopup("About", tostring(helpSectionName)) end
function main:OpenOptionsPopup() self:OpenMessagePopup("Options", "stub") end
"#;

fn rust_stubs(cx: &mut Cx) {
    let h = &mut cx.app_mut().main.hooks;
    h.open_about_popup = Rc::new(|cx, s: ModeArg| {
        pob_ui_core::popups::open_message_popup(cx, "About", &s.tostring());
    });
    h.open_options_popup = Rc::new(|cx| {
        pob_ui_core::popups::open_message_popup(cx, "Options", "stub");
    });
}

/// `#! u16-setup NAME` steps run right after `OnInit` on both sides.
fn lua_setup(name: &str) -> &'static str {
    match name {
        "toast" => r#"ToastNotification:Add("Toast title\nfirst line of the body\nsecond line")"#,
        "toast2" => {
            r#"ToastNotification:Add("One") ToastNotification:Add("Two\nwith a body")"#
        }
        "prompt" => r#"launch:ShowErrMsg("Test error: %s", "boom ^3coloured")"#,
        "path_popup" => r#"main:OpenPathPopup("/no/such?dir", "Permission denied", false)"#,
        "status" => r#"main:SetWindowTitleSubtext("My build") main:SetWindowTitleSubtext()"#,
        other => panic!("unknown u16-setup {other}"),
    }
}

fn rust_setup(cx: &mut Cx, name: &str) {
    match name {
        "toast" => {
            main_mode::toast::add(cx, "Toast title\nfirst line of the body\nsecond line");
        }
        "toast2" => {
            main_mode::toast::add(cx, "One");
            main_mode::toast::add(cx, "Two\nwith a body");
        }
        "prompt" => pob_app::launch::show_err_msg(
            cx,
            "Test error: %s",
            &[pob_lua::FmtArg::from("boom ^3coloured")],
        ),
        "path_popup" => main_mode::path_popup::open_path_popup(
            cx,
            "/no/such?dir",
            "Permission denied",
            false,
        ),
        "status" => {
            main_mode::folders::set_window_title_subtext(cx, Some("My build"));
            main_mode::folders::set_window_title_subtext(cx, None);
        }
        other => panic!("unknown u16-setup {other}"),
    }
}

fn setups(text: &str) -> Vec<String> {
    text.lines()
        .filter_map(|l| l.trim().strip_prefix("#! u16-setup "))
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
        anyhow::bail!("u16 scenarios do not open builds")
    }
    fn snapshot(&mut self) -> anyhow::Result<Option<String>> {
        Ok(None)
    }
    // No error-prompt check: the prompt scenarios show it on purpose.
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

impl App for RustSide {
    fn sg(&mut self) -> &mut Sg {
        self.app.sg()
    }
    fn sys(&mut self) -> &mut Sys {
        self.app.sys()
    }
    fn on_init(&mut self) {
        rust_stubs(&mut self.app.cx);
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
        anyhow::bail!("u16 scenarios do not open builds")
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

struct Sides {
    lua: Vec<FrameTrace>,
    rust: Vec<FrameTrace>,
    rust_error: Option<String>,
    /// Settings.xml as each side's main:SaveSettings wrote it at the end (when asked for).
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
    save_settings: bool,
) -> anyhow::Result<Sides> {
    let user = UserDir::new(fixture)?;
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
    let mut lua_settings = None;
    if save_settings {
        lua_side
            .app
            .with_vm(|pob| pob.eval::<()>("main:SaveSettings()"))
            .map_err(|e| anyhow::anyhow!("Lua SaveSettings: {e}"))?;
        lua_settings = Some(std::fs::read_to_string(&settings_file)?);
    }
    drop(lua_side);

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
    if save_settings && rust_error.is_none() {
        main_mode::settings::save_settings(&mut rust_side.app.cx);
        rust_settings = Some(std::fs::read_to_string(&settings_file)?);
    }
    drop_image_loads(&mut lua);
    drop_image_loads(&mut rust);
    Ok(Sides {
        lua,
        rust,
        rust_error,
        lua_settings,
        rust_settings,
    })
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
                ApiCall::DrawImage { .. } | ApiCall::DrawImageQuad { .. } | ApiCall::DrawString { .. }
            )
        })
        .count()
}

#[test]
fn u16_scenarios() {
    let _g = LOCK.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
    let Some(root) = inputs() else { return };
    let mut scripts: Vec<PathBuf> = std::fs::read_dir(test_dir())
        .expect("tests/u16")
        .filter_map(Result::ok)
        .map(|e| e.path())
        .filter(|p| p.extension().is_some_and(|x| x == "pobscript"))
        .collect();
    scripts.sort();
    assert!(!scripts.is_empty(), "no tests/u16/*.pobscript");
    let filter = std::env::var("U16_FILTER").ok();
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
            let tag = format!("u16_{stem}@{w}x{h}@{s}");
            let sides = match run_sides(
                &root,
                &script,
                fx.user_dir.as_deref(),
                runner::screen(w, h, s),
                &setups(&text),
                false,
            ) {
                Ok(s) => s,
                Err(e) => {
                    failures.push(format!("{tag}: {e:#}"));
                    continue;
                }
            };
            ran += 1;
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
            println!(
                "{tag}: {} frames, {ops} Lua draw ops, {n} divergences{}",
                run.lua.len(),
                sides
                    .rust_error
                    .as_ref()
                    .map_or_else(String::new, |e| format!(", Rust error: {e}"))
            );
            if n != 0 || sides.rust_error.is_some() || run.lua.len() != run.rust.len() || ops == 0
            {
                failures.push(format!(
                    "{tag}: {n} divergences, frames lua {} rust {}, {ops} ops (see {})",
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
        // `pairs(self.gameAccounts)` / `pairs(sharedItemSet.slots)` (Main.lua:763, :773): the
        // children's order is PoB's string-key order, random per process.
        if &*n.elem == "Accounts" || &*n.elem == "ItemSet" {
            n.children.sort_by_key(|c| match c {
                XmlChild::Node(c) => pob_app::model::xml::ComposeXML(c),
                XmlChild::Text(t) => t.to_string(),
            });
        }
    }
    let mut root = pob_app::model::xml::ParseXML_root(xml).expect("parse");
    sort(&mut root);
    let XmlChild::Node(top) = &root.children[0] else {
        panic!("no root element")
    };
    pob_app::model::xml::ComposeXML(top)
}

#[test]
fn u16_settings_roundtrip() {
    let _g = LOCK.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
    let Some(root) = inputs() else { return };
    let dir = test_dir().join("settings");
    let mut files: Vec<PathBuf> = std::fs::read_dir(&dir)
        .expect("tests/u16/settings")
        .filter_map(Result::ok)
        .map(|e| e.path())
        .filter(|p| p.extension().is_some_and(|x| x == "xml"))
        .collect();
    files.sort();
    assert!(files.len() >= 5, "the Settings.xml corpus");
    let script = runner::parse_script("frames 2").unwrap();
    let mut failures = Vec::new();
    let tmp = std::env::temp_dir().join(format!("pob-u16-settings-{}", std::process::id()));
    for f in &files {
        let name = f.file_name().unwrap().to_string_lossy().into_owned();
        let input = std::fs::read_to_string(f).unwrap();
        // a fixture user dir holding only this Settings.xml
        let _ = std::fs::remove_dir_all(&tmp);
        std::fs::create_dir_all(tmp.join("Path of Building/Builds")).unwrap();
        std::fs::write(tmp.join("Path of Building/Settings.xml"), &input).unwrap();
        let sides = run_sides(
            &root,
            &script,
            Some(&tmp),
            runner::screen(1280, 800, 1.0),
            &[],
            true,
        )
        .unwrap_or_else(|e| panic!("{name}: {e:#}"));
        if let Some(e) = &sides.rust_error {
            failures.push(format!("{name}: Rust error {e}"));
            continue;
        }
        let lua = sides.lua_settings.unwrap();
        let rust = sides.rust_settings.unwrap();
        // `*.migrate.xml`: inputs PoB itself rewrites (legacy or missing attributes, invalid
        // files); only Rust == PoB is required.
        let migrate = name.ends_with(".migrate.xml");
        let byte_identical = migrate || rust == input;
        let same_as_lua = normalise(&rust) == normalise(&lua);
        let lua_fixed_point = migrate || normalise(&lua) == normalise(&input);
        if migrate {
            println!(
                "{name}: Rust == PoB (attribute order normalised): {same_as_lua} (migrated input: identity not required)"
            );
        } else {
            println!(
                "{name}: Rust byte-identical to input: {byte_identical}; Rust == PoB (attribute order normalised): {same_as_lua}; PoB round-trips (normalised): {lua_fixed_point}"
            );
        }
        if !byte_identical || !same_as_lua || !lua_fixed_point {
            failures.push(format!(
                "{name}:\n--- input\n{input}\n--- rust\n{rust}\n--- lua\n{lua}"
            ));
        }
    }
    let _ = std::fs::remove_dir_all(&tmp);
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

/// Without a Settings.xml both sides write main:Init's defaults (and the Unnamed build mode).
#[test]
fn u16_settings_defaults() {
    let _g = LOCK.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
    let Some(root) = inputs() else { return };
    let tmp = std::env::temp_dir().join(format!("pob-u16-defaults-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&tmp);
    std::fs::create_dir_all(&tmp).unwrap();
    let script = runner::parse_script("frames 2").unwrap();
    let sides = run_sides(
        &root,
        &script,
        Some(&tmp),
        runner::screen(1280, 800, 1.0),
        &[],
        true,
    )
    .unwrap();
    let _ = std::fs::remove_dir_all(&tmp);
    assert_eq!(sides.rust_error, None);
    let (lua, rust) = (sides.lua_settings.unwrap(), sides.rust_settings.unwrap());
    println!("defaults:\n{rust}");
    assert_eq!(normalise(&rust), normalise(&lua));
}
