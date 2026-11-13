//! U52 acceptance (ui doc §12.7 U52 row: "Recorded-fixture scenarios"): ExtBuildListControl,
//! ExtBuildListProvider and PoBArchivesProvider on both sides of the UI oracle, with recorded
//! pobarchives.com responses. Owner: U52.
//!
//! `u52_scenarios`: every `tests/u52/*.pobscript` at the acceptance screens (a script that sets
//! its own `screen` runs once), over `fixtures/user1`, with the script's `fixture net=` responses
//! served to both sides. Zero T3 divergences (the same draw ops per `(layer, sub)` bucket and
//! the same side effects, frame by frame), the same window titles, and after the run the same
//! integer keys (and labels) in the list control's `controls` table (the `#controls` borders
//! `t_insert` used, module notes of `pob_app::ext_build_list::control`).
//!
//! Where the control lives. PoB 2.67.2 never shows it: BuildList mode creates it under
//! `if false` (BuildList.lua:20, :92) and `buildMode:OpenSimilarPopup` has no caller. The
//! scripts put it where PoB's own code would, right after the first frame on both sides
//! (`#! u52-setup NAME`):
//! * `list`: `main.modes.LIST.controls.ExtBuildList = main.modes.LIST:getPublicBuilds()`
//!   (BuildList.lua:93, the real Lua function; the Rust side runs
//!   `pob_app::ext_build_list::get_public_builds`);
//! * `similar`: the body of `buildMode:OpenSimilarPopup` (Build.lua:1577-1614, without the
//!   `resizeFunc`) with a fixed build XML in place of `self:SaveDB("code")`; the Rust side runs
//!   U20's hook entry `pob_app::ext_build_list::similar_build_list`.
//!
//! The dead draw body (`#! u52-draw-body`). The control's `Draw` returns at `if true then return
//! end` (ExtBuildListControl.lua:249), so in PoB nothing of it is drawn. Scripts without the
//! directive check exactly that (only the hit-testing and the network flows are live). Scripts
//! with it run the body on both sides: the Lua class is loaded from its source with that guard
//! replaced by `if not __u52DrawBody then` (the replacement asserts the original text), and the
//! Rust side calls `ext_build_list::control::set_draw_body(true)`.
//!
//! Stand-ins and harness fixes (both sides run the same ones):
//! * u18's: `main.modes.BUILD` draws nothing and records its `Init` arguments (pob-app's
//!   `PendingMode`), `main:DrawBackground` draws nothing;
//! * SimpleGraphic's `Deflate`/`Inflate` are `pob_gamedata::sg_zlib` on both sides (the
//!   oracle's natives since FUP7), so the similar-builds code and an imported build code
//!   decode the same on both sides;
//! * `PoBArchivesProvider:GetRecommendations` runs its own curl sub-script (a POST), which the
//!   oracle's `LaunchSubScript` cannot serve (it takes the URL from argument 4, which is the
//!   build code here, and finishes with `body, errMsg, headers` where this script returns
//!   `page, res`). The Lua stand-in re-launches it with the URL it embeds and hands the
//!   registered callback `(body, <the last HTTP status line's code in the headers>)`, which is
//!   what the Rust side derives from the same fixture;
//! * Net completions on the Rust side are held back from the runner's `poll` and handed to
//!   `App::on_net` at the start of the next `on_frame` (u43's `DeferredNet`).
//!
//! `u52_parse_builds`: `PoBArchivesProvider:ParseBuilds` on LuaJIT and in Rust for a set of JSON
//! documents (missing and extra fields, `"None"` classes, numbers as strings, `null`s, object-
//! form lists, invalid JSON): the same `statusMsg` and build entries, or the same Lua error.
//!
//! Compared side effects exclude `ImageLoad` (frame 0 loads images of modules other units port;
//! the portraits' loads are compared through the `DrawImage` ops that use them).

#![allow(clippy::needless_raw_string_hashes)] // the JSON cases read alike

use std::cell::RefCell;
use std::path::{Path, PathBuf};
use std::rc::Rc;

use pob_app::app::CxApp;
use pob_app::ext_build_list::control::{set_draw_body, ExtBuildListControl};
use pob_host::{App, PlatformParts};
use pob_sg::trace::{FrameTrace, SideEffect};
use pob_sg::{Key, Sg, Sys};
use pob_ui_core::controls::ButtonControl;
use pob_ui_core::cx::Cx;
use pob_ui_core::prelude::*;
use pob_ui_oracle::boot::{self, Entry, OracleHandles};
use pob_ui_oracle::diff_report;
use pob_ui_oracle::runner::{self, Driver, LuaApp, RunResult, ScenarioApp, UserDir};

/// The requests the Rust side sent: (url, body).
type Sent = Rc<RefCell<Vec<(String, Option<Vec<u8>>)>>>;

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
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/u52")
}

/// The build XML the `similar` setup encodes (both sides).
const SIMILAR_XML: &str = "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<PathOfBuilding>\n\t<Build level=\"90\" className=\"Witch\" ascendClassName=\"Necromancer\"/>\n</PathOfBuilding>";

/// The stand-ins both sides run (module notes), installed after `OnInit`.
const LUA_STUBS: &str = r#"
local function stubMode()
	local m = { }
	function m:Init(a1, a2) self.a1, self.a2 = a1, a2 end
	function m:GetArgs() return self.a1, self.a2 end
	function m:OnFrame(inputEvents, viewPort) end
	return m
end
main.modes.BUILD = stubMode()
function main:DrawBackground(viewPort) end

-- PoBArchivesProvider:GetRecommendations' sub-script (module notes)
local rawLaunchSubScript = LaunchSubScript
local recommendations = { }
function LaunchSubScript(script, funcList, subList, ...)
	local url = script:match("easy:setopt_url%('([^']*)'%)")
	if url and script:find("importCode=", 1, true) then
		local id = rawLaunchSubScript(script, funcList, subList, url)
		if id then
			recommendations[id] = true
		end
		return id
	end
	return rawLaunchSubScript(script, funcList, subList, ...)
end
local rawRegisterSubScript = launch.RegisterSubScript
function launch:RegisterSubScript(id, callback)
	if id and recommendations[id] and callback then
		recommendations[id] = nil
		local cb = callback
		callback = function(body, errMsg, headers)
			local res = 0
			for line in (headers or ""):gmatch("[^\r\n]+") do
				local code = line:match("^HTTP/[%d%.]+ (%d+)")
				if code then
					res = tonumber(code)
				end
			end
			return cb(body, res)
		end
	end
	return rawRegisterSubScript(self, id, callback)
end
"#;

/// Load ExtBuildListControl.lua with its dead-code guard behind `__u52DrawBody` (module notes).
const LUA_DRAW_BODY: &str = r#"
__u52DrawBody = true
local path = GetScriptPath() .. "/Classes/ExtBuildListControl.lua"
local f = assert(io.open(path, "r"))
local text = f:read("*a")
f:close()
local n
text, n = text:gsub("\tif true then\n\t\treturn\n\tend\n", "\tif not __u52DrawBody then\n\t\treturn\n\tend\n", 1)
assert(n == 1, "ExtBuildListControl.lua: the Draw guard (L249-251) moved")
assert(loadstring(text, "@" .. path))()
"#;

fn lua_setup(name: &str) -> String {
    match name {
        // BuildList.lua:93 (under `if false` in PoB)
        "list" => {
            "main.modes.LIST.controls.ExtBuildList = main.modes.LIST:getPublicBuilds()\n".to_owned()
        }
        // Build.lua:1577-1614 (no resizeFunc; SaveDB("code") is SIMILAR_XML)
        "similar" => format!(
            r#"local controls = {{ }}
local buildProviders = {{
	{{
		name = "PoB Archives",
		impl = new("PoBArchivesProvider"):PoBArchivesProvider("similar")
	}}
}}
local width = 600
local height = function()
	return main.screenH * 0.8
end
local padding = 50
controls.similarBuildList = new("ExtBuildListControl"):ExtBuildListControl(nil, {{0, padding, width, height() - 2 * padding}}, buildProviders)
controls.similarBuildList.shown = true
controls.similarBuildList.height = function()
	return height() - 2 * padding
end
controls.similarBuildList.width = function ()
	return width - padding
end
controls.similarBuildList:SetImportCode(common.base64.encode(Deflate({xml:?})):gsub("+","-"):gsub("/","_"))
controls.similarBuildList:Init("PoB Archives")
controls.close = new("ButtonControl"):ButtonControl(nil, {{0, height() - (padding + 20) / 2, 80, 20}}, "Close", function()
	main:ClosePopup()
end)
main:OpenPopup(width, height(), "Similar Builds", controls)
"#,
            xml = SIMILAR_XML
        ),
        other => panic!("unknown u52-setup {other}"),
    }
}

fn rust_setup(cx: &mut Cx, name: &str) {
    match name {
        "list" => {
            let lm = cx.app().list_mode.expect("LIST mode initialised").id();
            let ext = pob_app::ext_build_list::get_public_builds(cx, lm);
            cx.put(lm, "ExtBuildList", ext);
        }
        "similar" => {
            let width = 600.0;
            let padding = 50.0;
            let h0 = cx.main.screen_h * 0.8;
            pob_ui_core::popups::open_popup(
                cx,
                width,
                h0,
                "Similar Builds",
                move |cx, p| {
                    pob_app::ext_build_list::similar_build_list(cx, p.id(), &|_cx: &mut Cx| {
                        SIMILAR_XML.to_owned()
                    });
                    cx.add(
                        p,
                        "close",
                        ButtonControl::new(
                            Anchor::NONE,
                            rect(0, h0 - (padding + 20.0) / 2.0, 80, 20),
                            "Close",
                            pob_ui_core::popups::close_popup,
                        ),
                    );
                },
                pob_ui_core::popups::PopupOpts::default(),
            );
        }
        other => panic!("unknown u52-setup {other}"),
    }
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
    setups: Vec<String>,
    draw_body: bool,
    frames: usize,
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
        if self.draw_body {
            code.push_str(LUA_DRAW_BODY);
        }
        self.app
            .with_vm(|pob| pob.eval::<()>(&code))
            .unwrap_or_else(|e| panic!("Lua stubs: {e}"));
    }
    fn on_frame(&mut self) {
        self.app.on_frame();
        self.frames += 1;
        if self.frames == 1 {
            let code: String = self.setups.iter().map(|s| lua_setup(s)).collect();
            self.app
                .with_vm(|pob| pob.eval::<()>(&code))
                .unwrap_or_else(|e| panic!("Lua setup: {e}"));
        }
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
        anyhow::bail!("u52 scenarios do not load builds")
    }
    fn snapshot(&mut self) -> anyhow::Result<Option<String>> {
        Ok(None)
    }
    fn after_frame(&mut self) -> anyhow::Result<()> {
        self.app.after_frame()
    }
    fn take_error(&mut self) -> Option<String> {
        self.app.take_error()
    }
}

// ---- the Rust side ----

/// Records the platform side effects the Lua side's bridge records (CCR-0028 item 2), as u18.
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

/// Holds the inner Net's completions back from `Driver::frame`'s `poll` and hands them to
/// [`RustSide::on_frame`] (module notes; u43's).
struct DeferredNet {
    inner: Box<dyn pob_sg::Net>,
    held: Rc<RefCell<Vec<(u64, pob_sg::HttpResponse)>>>,
    /// Every request sent, for the run's checks.
    sent: Sent,
}

impl pob_sg::Net for DeferredNet {
    fn download_page(&mut self, req: pob_sg::HttpRequest) -> u64 {
        self.sent
            .borrow_mut()
            .push((req.url.clone(), req.body.clone()));
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
    setups: Vec<String>,
    effects: Rc<RefCell<Vec<SideEffect>>>,
    held: Rc<RefCell<Vec<(u64, pob_sg::HttpResponse)>>>,
    sent: Sent,
    frames: usize,
}

impl RustSide {
    fn new(
        mut parts: PlatformParts,
        setups: Vec<String>,
        net: pob_ui_oracle::sg_bridge::NetFixtures,
    ) -> RustSide {
        let held = Rc::new(RefCell::new(Vec::new()));
        let sent = Rc::new(RefCell::new(Vec::new()));
        parts.sys.net = Box::new(DeferredNet {
            inner: Box::new(boot::ScriptedNet::new(net)),
            held: held.clone(),
            sent: sent.clone(),
        });
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
            held,
            sent,
            frames: 0,
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
        // u18's stand-in (CCR-0098 §5)
        self.app.cx.app_mut().main.modes.insert(
            "BUILD".to_owned(),
            Rc::new(pob_app::main_mode::PendingMode::new("BUILD")),
        );
        self.flush();
    }
    fn on_frame(&mut self) {
        // the completions `poll` held back (pob_host::run_frame: before on_frame)
        let done: Vec<_> = self.held.borrow_mut().drain(..).collect();
        for (id, resp) in done {
            self.app.on_net(id, resp);
        }
        self.app.on_frame();
        self.frames += 1;
        if self.frames == 1 {
            for s in self.setups.clone() {
                rust_setup(&mut self.app.cx, &s);
            }
        }
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
        anyhow::bail!("u52 scenarios do not load builds")
    }
    fn snapshot(&mut self) -> anyhow::Result<Option<String>> {
        Ok(None)
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

/// The list control's integer `controls` keys and their button labels, plus the mode and the
/// BUILD stand-in's second `Init` argument (the build name an Import passes).
#[derive(Debug, Default, PartialEq)]
struct EndState {
    keys: Vec<(i64, String)>,
    mode: Option<String>,
    build_name: Option<String>,
}

const LUA_END_STATE: &str = r#"
local host
if main.popups[1] and main.popups[1].controls.similarBuildList then
	host = main.popups[1].controls.similarBuildList
elseif main.modes.LIST.controls and main.modes.LIST.controls.ExtBuildList then
	host = main.modes.LIST.controls.ExtBuildList
end
local out = { }
if host then
	for k, v in pairs(host.controls) do
		if type(k) == "number" then
			table.insert(out, string.format("%d=%s", k, tostring(v.label)))
		end
	end
end
table.sort(out)
local b = main.modes.BUILD.a2
return table.concat(out, "\n"), main.mode, b ~= nil and tostring(b) or nil
"#;

fn parse_keys(s: &str) -> Vec<(i64, String)> {
    let mut v: Vec<(i64, String)> = s
        .lines()
        .filter_map(|l| {
            let (k, lbl) = l.split_once('=')?;
            Some((k.parse().ok()?, lbl.to_owned()))
        })
        .collect();
    v.sort();
    v
}

fn rust_end_state(cx: &Cx) -> EndState {
    let host = cx
        .main
        .popups
        .first()
        .and_then(|p| cx.child(p.id(), "similarBuildList"))
        .or_else(|| {
            cx.app()
                .list_mode
                .and_then(|lm| cx.child(lm.id(), "ExtBuildList"))
        });
    let mut keys = Vec::new();
    if let Some(h) = host.filter(|&h| cx.is::<ExtBuildListControl>(h)) {
        for i in 1..=200 {
            if let Some(c) = cx.child(h, CtrlKey::Idx(i)) {
                let label = cx
                    .try_w::<ButtonControl>(c)
                    .map_or_else(|| "nil".to_owned(), |b| b.label.raw());
                keys.push((i, label));
            }
        }
    }
    keys.sort();
    EndState {
        keys,
        mode: cx.app().main.mode.clone(),
        build_name: None,
    }
}

struct Sides {
    lua: Vec<FrameTrace>,
    rust: Vec<FrameTrace>,
    lua_error: Option<String>,
    rust_error: Option<String>,
    lua_end: EndState,
    rust_end: EndState,
    rust_sent: Vec<(String, Option<Vec<u8>>)>,
}

fn run_sides(
    root: &Path,
    script: &pob_host::script::Script,
    fixture: Option<&Path>,
    screen: pob_sg::ScreenInfo,
    setups: &[String],
    draw_body: bool,
    net: &pob_ui_oracle::sg_bridge::NetFixtures,
) -> anyhow::Result<Sides> {
    let user = UserDir::new(fixture)?;

    // Lua
    let handles = OracleHandles::default();
    let lua_app = runner::lua_side(root, &user, &handles, Entry::Launch, false)?;
    *pob_ui_oracle::sg_bridge::bridge(&lua_app.pob.lua)
        .map_err(|e| anyhow::anyhow!("{e}"))?
        .net
        .borrow_mut() = net.clone();
    let mut lua_side = LuaSide {
        app: lua_app,
        setups: setups.to_vec(),
        draw_body,
        frames: 0,
    };
    let mut d = Driver::new(screen, handles, user.path.clone());
    let res = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        runner::drive(&mut lua_side, script, &mut d)
    }));
    let (mut lua, lua_error) = match res {
        Ok((frames, r)) => (frames, r.err().map(|e| e.to_string())),
        Err(p) => (
            Vec::new(),
            Some(format!("panicked: {}", pob_host::panic_message(&*p))),
        ),
    };
    let lua_end = lua_side
        .app
        .with_vm(|pob| pob.eval::<(String, Option<String>, Option<String>)>(LUA_END_STATE))
        .map(|(k, mode, name)| EndState {
            keys: parse_keys(&k),
            mode,
            build_name: name,
        })
        .unwrap_or_default();
    drop(lua_side);

    // Rust
    user.reset()?;
    set_draw_body(draw_body);
    let handles = OracleHandles::default();
    let (sg, sys) = boot::oracle_parts(root, &user.path, &handles)?;
    let mut rust_side = RustSide::new(PlatformParts { sg, sys }, setups.to_vec(), net.clone());
    let mut d = Driver::new(screen, handles, user.path.clone());
    let res = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        runner::drive(&mut rust_side, script, &mut d)
    }));
    set_draw_body(false);
    let (mut rust, rust_error) = match res {
        Ok((frames, r)) => (frames, r.err().map(|e| e.to_string())),
        Err(p) => (
            Vec::new(),
            Some(format!("panicked: {}", pob_host::panic_message(&*p))),
        ),
    };
    let rust_end = rust_end_state(&rust_side.app.cx);
    let rust_sent = rust_side.sent.borrow().clone();
    drop(rust_side);
    drop_image_loads(&mut lua);
    drop_image_loads(&mut rust);
    Ok(Sides {
        lua,
        rust,
        lua_error,
        rust_error,
        lua_end,
        rust_end,
        rust_sent,
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
                ApiCall::DrawImage { .. }
                    | ApiCall::DrawImageQuad { .. }
                    | ApiCall::DrawString { .. }
            )
        })
        .count()
}

fn effects(frames: &[FrameTrace]) -> Vec<SideEffect> {
    frames.iter().flat_map(|f| f.effects.clone()).collect()
}

/// `#! expect-text TEXT` (some Lua DrawString contains TEXT), `#! expect-no-text TEXT` (none
/// does), `#! u52-expect-effect open-url URL`, `#! u52-expect-mode MODE`,
/// `#! u52-expect-build-name NAME`, `#! u52-expect-keys N` (the list holds N integer keys at
/// the end), `#! u52-expect-post URL BODY-PREFIX` (the Rust side POSTed it).
fn check_expectations(text: &str, s: &Sides) -> Vec<String> {
    use pob_sg::trace::ApiCall;
    let mut errs = Vec::new();
    let has_text = |t: &str| {
        s.lua.iter().flat_map(|f| &f.calls).any(|c| match &c.call {
            ApiCall::DrawString { text, .. } => text.contains(t),
            _ => false,
        })
    };
    for t in directives(text, "expect-text") {
        if !has_text(&t) {
            errs.push(format!("no Lua DrawString containing {t:?}"));
        }
    }
    for t in directives(text, "expect-no-text") {
        if has_text(&t) {
            errs.push(format!("a Lua DrawString contains {t:?}"));
        }
    }
    let lua_effects = effects(&s.lua);
    for e in directives(text, "u52-expect-effect") {
        let want = match e.split_once(' ') {
            Some(("open-url", url)) => SideEffect::OpenUrl(url.to_owned()),
            _ => panic!("bad u52-expect-effect {e}"),
        };
        if !lua_effects.contains(&want) {
            errs.push(format!("no Lua side effect {want:?}"));
        }
    }
    for m in directives(text, "u52-expect-mode") {
        if s.lua_end.mode.as_deref() != Some(m.as_str()) {
            errs.push(format!("Lua mode {:?}, expected {m}", s.lua_end.mode));
        }
    }
    for n in directives(text, "u52-expect-build-name") {
        if s.lua_end.build_name.as_deref() != Some(n.as_str()) {
            errs.push(format!(
                "Lua build name {:?}, expected {n}",
                s.lua_end.build_name
            ));
        }
    }
    for n in directives(text, "u52-expect-keys") {
        let n: usize = n.parse().expect("u52-expect-keys N");
        if s.lua_end.keys.len() != n {
            errs.push(format!(
                "Lua integer keys {:?}, expected {n}",
                s.lua_end.keys
            ));
        }
    }
    for p in directives(text, "u52-expect-post") {
        let (url, prefix) = p.split_once(' ').unwrap_or((p.as_str(), ""));
        let found = s.rust_sent.iter().any(|(u, body)| {
            u == url
                && body
                    .as_ref()
                    .is_some_and(|b| String::from_utf8_lossy(b).starts_with(prefix))
        });
        if !found {
            errs.push(format!(
                "no Rust POST to {url} with a body starting {prefix:?}"
            ));
        }
    }
    errs
}

#[test]
fn u52_scenarios() {
    let _g = LOCK
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let Some(root) = inputs() else { return };
    let mut scripts: Vec<PathBuf> = std::fs::read_dir(test_dir())
        .expect("tests/u52")
        .filter_map(Result::ok)
        .map(|e| e.path())
        .filter(|p| p.extension().is_some_and(|x| x == "pobscript"))
        .collect();
    scripts.sort();
    assert!(!scripts.is_empty(), "no tests/u52/*.pobscript");
    let filter = std::env::var("U52_FILTER").ok();
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
        let setups = directives(&text, "u52-setup");
        let draw_body = !directives(&text, "u52-draw-body").is_empty();
        for (w, h, s) in screens {
            let tag = if own_screen {
                format!("u52_{stem}")
            } else {
                format!("u52_{stem}@{w}x{h}@{s}")
            };
            let sides = match run_sides(
                &root,
                &script,
                fx.user_dir.as_deref(),
                runner::screen(w, h, s),
                &setups,
                draw_body,
                &fx.net,
            ) {
                Ok(s) => s,
                Err(e) => {
                    failures.push(format!("{tag}: {e:#}"));
                    continue;
                }
            };
            ran += 1;
            let expect = check_expectations(&text, &sides);
            let same_keys = sides.lua_end.keys == sides.rust_end.keys;
            let same_mode = sides.lua_end.mode == sides.rust_end.mode;
            let run = RunResult {
                lua: sides.lua.clone(),
                rust: sides.rust.clone(),
                rust_error: sides.rust_error.clone(),
                lua_snapshots: Vec::new(),
                rust_snapshots: Vec::new(),
                audit: Vec::new(),
                compared: true,
            };
            let n = diff_report::write_report(&out_dir(), &tag, &run, false).unwrap_or(usize::MAX);
            let ops = draw_ops(&run.lua);
            println!(
                "{tag}: {} frames, {ops} Lua draw ops, {n} divergences, {} Lua side effects, list keys {:?} (same: {same_keys}), mode {:?} (same: {same_mode}){}{}{}",
                run.lua.len(),
                effects(&run.lua).len(),
                sides.lua_end.keys,
                sides.lua_end.mode,
                sides
                    .lua_error
                    .as_ref()
                    .map_or_else(String::new, |e| format!(", Lua error: {e}")),
                sides
                    .rust_error
                    .as_ref()
                    .map_or_else(String::new, |e| format!(", Rust error: {e}")),
                if expect.is_empty() {
                    String::new()
                } else {
                    format!(", expectations: {}", expect.join("; "))
                }
            );
            if n != 0
                || sides.rust_error.is_some()
                || sides.lua_error.is_some()
                || run.lua.len() != run.rust.len()
                || ops == 0
                || !same_keys
                || !same_mode
                || !expect.is_empty()
            {
                failures.push(format!(
                    "{tag}: {n} divergences, frames lua {} rust {}, {ops} ops, keys lua {:?} rust {:?}, mode lua {:?} rust {:?}, errors lua {:?} rust {:?}, expectations {expect:?} (see {})",
                    run.lua.len(),
                    run.rust.len(),
                    sides.lua_end.keys,
                    sides.rust_end.keys,
                    sides.lua_end.mode,
                    sides.rust_end.mode,
                    sides.lua_error,
                    sides.rust_error,
                    out_dir().join(&tag).join("report.txt").display()
                ));
            }
        }
    }
    println!("u52: {ran} runs");
    assert!(ran > 0 || filter.is_some(), "no run");
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

// ---- ParseBuilds on both sides ----

/// The JSON documents of `u52_parse_builds`.
const PARSE_CASES: &[&str] = &[
    r#"{"builds":[{"build_info":{"title":"Arc Witch","author":"me","mainSkill":"Arc","ascendancy":"Elementalist","class":"Witch","short_uuid":"u1","build_link":"https://pobb.in/abc"},"stats":{"TotalEHP":12345.5,"LifeUnreserved":4000,"EnergyShield":100},"fullDPS":1234567.8,"tree":{"version":"3_25"}}]}"#,
    r#"{"builds":[{"build_info":{"title":"None classes","ascendancy":"None","class":"None","short_uuid":7},"stats":{}}]}"#,
    r#"{"builds":[{"build_info":{"title":"strings","short_uuid":"s","build_link":5},"stats":{"TotalEHP":"12","LifeUnreserved":null,"EnergyShield":"3.5"},"fullDPS":"99","tree":false,"similarity_score":0.875}]}"#,
    r#"{"builds":[{"build_info":{"title":"t","short_uuid":"x"},"stats":{"TotalEHP":1},"tree":{"version":325},"similarity_score":false},null,{"build_info":{"title":"second","short_uuid":"y"},"stats":{}}]}"#,
    r#"{"builds":{"b":{"build_info":{"title":"B","short_uuid":"b"},"stats":{}},"a":{"build_info":{"title":"A","short_uuid":"a"},"stats":{}}}}"#,
    r#"{"builds":[]}"#,
    r#"{"builds":{}}"#,
    r#"{"builds":[null]}"#,
    r#"{"builds":false}"#,
    r#"{"other":1}"#,
    r#"[1,2]"#,
    r#""text""#,
    r#"not json"#,
    r#"null"#,
    r#"false"#,
    r#"5"#,
    r#"true"#,
    r#"{"builds":"x"}"#,
    r#"{"builds":[{}]}"#,
    r#"{"builds":[5]}"#,
    r#"{"builds":[{"build_info":{"title":"no uuid"},"stats":{}}]}"#,
    r#"{"builds":[{"build_info":{"title":"no stats","short_uuid":"z"}}]}"#,
    r#"{"builds":[{"build_info":"str","stats":{}}]}"#,
];

const LUA_DESCRIBE: &str = r#"
local function v(x)
	if x == nil then return "nil" end
	local t = type(x)
	if t == "string" then return "s:" .. x end
	if t == "number" then return "n:" .. string.format("%.17g", x) end
	if t == "boolean" then return "b:" .. tostring(x) end
	return t
end
local fields = { "buildName", "author", "mainSkill", "ascendancy", "class", "previewLink", "buildLink", "ehp", "life", "es", "dps", "version" }
function __u52Describe(json)
	local p = new("PoBArchivesProvider"):PoBArchivesProvider("builds")
	local ok, err = pcall(p.ParseBuilds, p, json)
	local out = { }
	if not ok then
		return "error: " .. tostring(err):gsub("^[^:]*:%d+: ", "")
	end
	table.insert(out, "status=" .. v(p.statusMsg))
	for i, b in ipairs(p.buildList) do
		local line = { tostring(i) }
		for _, f in ipairs(fields) do
			table.insert(line, f .. "=" .. v(b[f]))
		end
		if b.metadata then
			for _, m in ipairs(b.metadata) do
				table.insert(line, "meta:" .. m.key .. "=" .. v(m.value))
			end
		end
		table.insert(out, table.concat(line, " "))
	end
	return table.concat(out, "\n")
end
"#;

fn rust_describe(json: &str) -> String {
    use pob_app::ext_build_list::{ExtBuildListProviderClass, LVal, PoBArchivesProvider};
    fn v(x: Option<&LVal>) -> String {
        match x {
            None => "nil".to_owned(),
            Some(LVal::Str(s)) => format!("s:{s}"),
            Some(LVal::Num(n)) => format!("n:{}", pob_lua::lfmt!("%.17g", *n)),
            Some(LVal::Bool(b)) => format!("b:{b}"),
            Some(LVal::Table(_)) => "table".to_owned(),
        }
    }
    let p = PoBArchivesProvider::new("builds");
    if let Err(e) = pob_lua::error::catch(|| p.parse_builds(Some(json))) {
        return format!("error: {}", e.0);
    }
    let b = p.base().borrow();
    let mut out = vec![format!(
        "status={}",
        v(b.statusMsg.clone().map(LVal::Str).as_ref())
    )];
    for (i, e) in b.buildList.iter().enumerate() {
        let preview = e.previewLink.clone().map(LVal::Str);
        let fields: [(&str, Option<&LVal>); 12] = [
            ("buildName", e.buildName.as_ref()),
            ("author", e.author.as_ref()),
            ("mainSkill", e.mainSkill.as_ref()),
            ("ascendancy", e.ascendancy.as_ref()),
            ("class", e.class.as_ref()),
            ("previewLink", preview.as_ref()),
            ("buildLink", e.buildLink.as_ref()),
            ("ehp", e.ehp.as_ref()),
            ("life", e.life.as_ref()),
            ("es", e.es.as_ref()),
            ("dps", e.dps.as_ref()),
            ("version", e.version.as_ref()),
        ];
        let mut line = vec![(i + 1).to_string()];
        for (f, x) in fields {
            line.push(format!("{f}={}", v(x)));
        }
        for (k, m) in e.metadata.iter().flatten() {
            line.push(format!("meta:{k}={}", v(m.as_ref())));
        }
        out.push(line.join(" "));
    }
    out.join("\n")
}

#[test]
fn u52_parse_builds() {
    let _g = LOCK
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let Some(root) = inputs() else { return };
    let user = UserDir::new(None).expect("user dir");
    let handles = OracleHandles::default();
    let mut lua = runner::lua_side(&root, &user, &handles, Entry::Launch, false).expect("boot");
    lua.on_init();
    lua.with_vm(|pob| pob.eval::<()>(LUA_DESCRIBE))
        .expect("describe");
    let mut failures = Vec::new();
    // the object-form list (case 4) is in per-process string-hash order in PoB: compared as a
    // set of entries (ORDER note of pob_archives.rs)
    for (i, json) in PARSE_CASES.iter().enumerate() {
        let l: String = lua
            .with_vm(|pob| {
                let f: mlua::Function = pob.lua.globals().get("__u52Describe")?;
                f.call::<String>(*json)
            })
            .expect("describe call");
        let r = rust_describe(json);
        let norm = |s: &str| {
            let mut v: Vec<String> = s
                .lines()
                .map(|l| l.split_once(' ').map_or(l, |(_, rest)| rest).to_owned())
                .collect();
            v.sort();
            v
        };
        let same = if i == 4 { norm(&l) == norm(&r) } else { l == r };
        println!("case {i}: {}", if same { "same" } else { "DIFFERENT" });
        if !same {
            failures.push(format!("case {i} {json}:\n lua:  {l}\n rust: {r}"));
        }
    }
    println!(
        "u52_parse_builds: {} cases, {} different",
        PARSE_CASES.len(),
        failures.len()
    );
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}
