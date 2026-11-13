//! Port of PathOfBuilding v2.67.2 (16de4b82d) src/Launch.lua 1-416 (except ApplyUpdate /
//! CheckForUpdate 325-357, stubbed). Owner: U16.
//!
//! @pob-file Launch.lua @16de4b82d
//!
//! The `launch` global is [`LaunchState`] (`cx.app().launch`); its methods are the functions
//! here. `PCall` (SimpleGraphic's protected call) is [`pcall`]: a panic (a `lua_error!`, ui doc
//! D17) inside it becomes the error message, as Lua's error does.
//!
//! DIVERGENCE(update-stub): the update machinery is stubbed (ui doc §12 U16 row):
//! `CheckForUpdate` returns at once (what pob-ui-oracle's patch 3 does to the Lua), `ApplyUpdate`
//! does nothing, and the first-run update (`LoadModule("UpdateCheck")`, L38) reports "none".
//! DIVERGENCE(error-text): SG's `PCall` appends a Lua traceback to the message; a Rust panic has
//! none, so the error prompt shows the message alone.
//! DIVERGENCE(subscripts): Lua sub-scripts do not exist in the port. `launch:DownloadPage` runs
//! over `Sys::net` (platform/net.rs) and completions arrive through `App::on_net` (CCR-0026 A),
//! which is `launch:OnSubFinished` / `launch:OnSubError` here; `launch:OnSubCall` (L201-208, the
//! sub-script's calls back into the main state) has no caller.
//! DIVERGENCE(branding): user decision 2026-11-12. The port shows its own date-based version,
//! [`PORT_VERSION`] (the repo-root `VERSION` file, `YYYY.MM.DD.N`, set by
//! `scripts/port/bump_version.sh`), in the main-mode `versionLabel` and the About popup.
//! `launch.versionNumber` stays PoB's manifest.xml number (build/archive compatibility, the error
//! prompt), and nothing parses or compares `PORT_VERSION` (DIVERGENCE(update-deferred): no update
//! checker, docs/port/FOLLOWUPS.md).

use std::rc::Rc;

use indexmap::IndexMap;
use pob_lua::FmtArg;
use pob_sg::{Align, Font, HttpResponse};
use pob_ui_core::popups::LaunchInfo;
use pob_ui_core::prelude::*;

use crate::app::CxApp;
use crate::main_mode;
use crate::main_mode::folders::APP_NAME;
use crate::model::XmlChild;
use crate::platform::net::PageResponse;

/// The port's own version (DIVERGENCE(branding), user decision 2026-11-12): the repo-root `VERSION`
/// file, `YYYY.MM.DD.N` (N: the release counter of that day, from 1), embedded at compile time so
/// the wasm32 build needs neither git nor the file system. Display only: nothing parses it.
pub const PORT_VERSION: &str =
    include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/../../VERSION")).trim_ascii();

/// A `self.subScripts[id]` entry (`type` and `callback`).
#[derive(Clone)]
pub enum SubScript {
    /// `type = "UPDATE"` (never created: CheckForUpdate is stubbed)
    Update,
    /// `type = "DOWNLOAD"`: `callback(responseBody, errMsg, responseHeader)` wraps the
    /// DownloadPage callback (Launch.lua:319-321).
    Download(Rc<dyn Fn(&mut Cx, PageResponse, Option<String>)>),
    /// `type = "CUSTOM"` (RegisterSubScript): the raw completion.
    Custom(Option<Rc<dyn Fn(&mut Cx, HttpResponse)>>),
}

/// `self.promptFunc(key)`: `true` dismisses the prompt.
pub type PromptFn = Rc<dyn Fn(&mut Cx, Key) -> bool>;

/// The `launch` global's fields.
#[derive(Default)]
pub struct LaunchState {
    pub dev_mode: bool,
    pub installed_mode: bool,
    /// `"?"` until manifest.xml is read; nil when its `<Version>` has no attribute.
    pub version_number: Option<String>,
    pub version_branch: Option<String>,
    pub version_platform: Option<String>,
    pub last_update_check: i64,
    /// LUA-ORDER: integer ids; nothing iterates them.
    pub sub_scripts: IndexMap<u64, SubScript>,
    /// The module-level `startTime` (Launch.lua:8).
    pub start_time: i64,
    pub update_err_msg: Option<String>,
    /// `self.main` is set (Modules/Main loaded).
    pub main: bool,
    pub prompt_msg: Option<String>,
    pub prompt_col: [f64; 3],
    pub prompt_func: Option<PromptFn>,
    pub do_restart: Option<String>,
    pub dev_mode_alt: bool,
    pub update_available: Option<String>,
    pub update_check_running: bool,
    pub update_check_background: Option<bool>,
    pub update_msg: Option<String>,
    pub update_progress: Option<String>,
    /// Set by LoadSettings (Main.lua:580-581).
    pub connection_protocol: Option<f64>,
    pub proxy_url: Option<String>,
    /// Main.lua:47 (`--no-ssl`)
    pub no_ssl: bool,
    /// The `arg` global: the command line after the program name (the desktop host sets it before
    /// `on_init`; Main.lua:41-49 and :71-83 read it).
    pub arg: Vec<String>,
    /// Not in PoB: treat a manifest without branch/platform as a release install (no dev mode).
    /// The UI oracle patches the Lua the same way (pob-ui-oracle patch 4: the checkout's
    /// manifest.xml has neither, which turns dev mode on); the Rust side of a parity run sets
    /// this before `on_init` (CCR-0096).
    pub force_release_install: bool,
}

/// The `profiler` global: nil unless a profiler module was loaded (never in PoB's release; the
/// port has none), so Launch.lua:150's branch is never taken.
const PROFILER: bool = false;

/// `PCall(f, ...)`: `Ok` with the results, or `Err(errMsg)` when `f` raised.
pub fn pcall<R>(cx: &mut Cx, f: impl FnOnce(&mut Cx) -> R) -> Result<R, String> {
    std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| f(cx)))
        .map_err(|p| pob_host::panic_message(&*p))
}

/// The `launch` fields main:OpenCloudErrorPopup reads (U14's popups, CCR-0084).
pub fn launch_info(cx: &Cx) -> LaunchInfo {
    let l = &cx.app().launch;
    LaunchInfo {
        app_name: APP_NAME.to_owned(),
        version_number: l.version_number.clone().unwrap_or_default(),
        version_branch: l.version_branch.clone(),
        dev_mode: l.dev_mode,
    }
}

/// The module-level code (Launch.lua:8-19): `startTime`, `APP_NAME`, the window title and the
/// `launch` table. `SetMainObject`, `ConExecute`, `jit.opt` and `collectgarbage` have no
/// equivalent.
/// lua: Launch.lua:1-19
/// @pob Launch.lua:1-19 h=8fb91575
pub fn load_module(cx: &mut Cx) {
    // L8
    let start_time = cx.sys.time_ms();
    // L11
    cx.sys.platform.set_window_title(APP_NAME);
    // L16
    cx.app_mut().launch = LaunchState {
        start_time,
        ..LaunchState::default()
    };
}

/// lua: Launch.lua:21-88 launch:OnInit()
/// @pob Launch.lua:21-88 h=09f59249
pub fn on_init(cx: &mut Cx) {
    {
        let now = cx.sys.time_ms();
        let l = &mut cx.app_mut().launch;
        // L22-29
        l.dev_mode = false;
        l.installed_mode = false;
        l.version_number = Some("?".to_owned());
        l.version_branch = Some("?".to_owned());
        l.version_platform = Some("?".to_owned());
        l.last_update_check = now;
        l.sub_scripts = IndexMap::new();
    }
    // L30
    let first_run_file = cx.sys.fs.read("first.run").is_ok();
    // L31
    if first_run_file {
        // L32-33
        let _ = cx.sys.fs.remove("first.run");
        // L34-36 This is a fresh installation (ConClear: no console to clear)
        // L37
        cx.sys
            .platform
            .con_print("Please wait while we complete installation...\n");
        // L38
        let update_mode = load_update_check();
        match update_mode {
            // L39-40 self.updateErrMsg = errMsg (the stub never fails)
            None => {}
            Some(mode) if mode != "none" => {
                // L42-43
                apply_update(cx, mode);
                return;
            }
            Some(_) => {}
        }
    }
    // L46-47
    let local_man_xml = crate::main_mode::settings::load_xml_file(cx, "manifest.xml")
        .or_else(|_| crate::main_mode::settings::load_xml_file(cx, "../manifest.xml"))
        .ok();
    // L48 if localManXML and localManXML[1].elem == "PoBVersion" then
    let root = match local_man_xml.as_ref().map(|m| m.children.first()) {
        None => None,
        // `localManXML[1].elem` on a missing first node is a Lua error
        Some(None) => lua_error!("attempt to index field '?' (a nil value)"),
        // a text run's `.elem` is nil
        Some(Some(XmlChild::Text(_))) => None,
        Some(Some(XmlChild::Node(n))) => (&*n.elem == "PoBVersion").then_some(n),
    };
    if let Some(root) = root {
        // L49
        for node in &root.children {
            // L50
            let XmlChild::Node(node) = node else { continue };
            // L51
            if &*node.elem == "Version" {
                let l = &mut cx.app_mut().launch;
                // L52-54
                l.version_number = node.attrib.get("number").map(|v| v.to_string());
                l.version_branch = node.attrib.get("branch").map(|v| v.to_string());
                l.version_platform = node.attrib.get("platform").map(|v| v.to_string());
            }
        }
    }
    // L59
    let l = &cx.app().launch;
    if local_man_xml.is_some()
        && l.version_branch.is_none()
        && l.version_platform.is_none()
        && !l.force_release_install
    {
        // L60-62 Looks like a remote manifest, so we're probably running from a repository
        // Enable dev mode to disable updates and set user path to be the script path
        cx.app_mut().launch.dev_mode = true;
    }
    // L64
    if cx.sys.fs.read("installed.cfg").is_ok() {
        // L66-67
        cx.app_mut().launch.installed_mode = true;
    }
    // L69 RenderInit("DPI_AWARE"): the hosts are always DPI aware
    // L70
    cx.sys.platform.con_print("Loading main script...");
    // L71-72 errMsg, self.main = PLoadModule("Modules/Main")
    let err_msg = pcall(cx, main_mode::init::load_module).err();
    if err_msg.is_none() {
        cx.app_mut().launch.main = true;
    }
    // L73
    if let Some(err_msg) = err_msg {
        // L74
        show_err_msg(
            cx,
            "Error loading main script: %s",
            &[FmtArg::from(err_msg.as_str())],
        );
    } else {
        // L77-78
        let err_msg = pcall(cx, main_mode::init::main_init).err();
        // L79
        if let Some(err_msg) = err_msg {
            // L80
            show_err_msg(cx, "In 'Init': %s", &[FmtArg::from(err_msg.as_str())]);
        }
    }

    // L84
    if !cx.app().launch.dev_mode && !first_run_file {
        // L85-86 Run a background update check if developer mode is off
        check_for_update(cx, Some(true));
    }
}

/// lua: Launch.lua:90-101 launch:CanExit()
/// @pob Launch.lua:90-101 h=68d97964
pub fn can_exit(cx: &mut Cx) -> bool {
    // L91
    if cx.app().launch.main && cx.app().launch.prompt_msg.is_none() {
        // L92
        match pcall(cx, main_mode::init::can_exit) {
            // L93-95
            Err(err_msg) => {
                show_err_msg(cx, "In 'CanExit': %s", &[FmtArg::from(err_msg.as_str())]);
                return false;
            }
            // L97
            Ok(ret) => return ret,
        }
    }
    // L100
    true
}

/// lua: Launch.lua:103-107 launch:OnExit()
/// @pob Launch.lua:103-107 h=e67e6cbe
pub fn on_exit(cx: &mut Cx) {
    // L104
    if cx.app().launch.main {
        // L105
        let _ = pcall(cx, main_mode::init::shutdown);
    }
}

/// lua: Launch.lua:109-141 launch:OnFrame()
/// @pob Launch.lua:109-141 h=903a583d
pub fn on_frame(cx: &mut Cx) {
    // L110-111
    if cx.app().launch.main {
        // L112
        let err_msg = pcall(cx, main_mode::on_frame::main_on_frame).err();
        // L113
        if let Some(err_msg) = err_msg {
            // L114-115 Send user to build list menu if a build crashes on initial load
            let build = cx
                .app()
                .main
                .modes
                .get("BUILD")
                .cloned()
                .unwrap_or_else(|| lua_error!("attempt to index field 'BUILD' (a nil value)"));
            if build.output_revision(cx) == Some(1.0)
                && build.build_flag(cx)
                && !cx.app().launch.dev_mode
            {
                // L116
                main_mode::on_frame::set_mode(cx, "LIST", Vec::new());
            }
            // L118
            show_err_msg(cx, "In 'OnFrame': %s", &[FmtArg::from(err_msg.as_str())]);
        }
    }
    // L122
    let alt = cx.sys.is_key_down(Key::Alt);
    let dev_mode = cx.app().launch.dev_mode;
    cx.app_mut().launch.dev_mode_alt = dev_mode && alt;
    // L123
    cx.sg.set_draw_layer(Some(1000), None);
    // L124
    cx.sg.reset_viewport();
    // L125
    if let Some(msg) = cx.app().launch.prompt_msg.clone() {
        // L126
        let [r, g, b] = cx.app().launch.prompt_col;
        // L127
        draw_popup(cx, r, g, b, "^0%s", &[FmtArg::from(msg.as_str())]);
    }
    // L129
    if let Some(text) = cx.app().launch.do_restart.clone() {
        // L130
        let (screen_w, screen_h) = cx.sg.get_virtual_screen_size();
        // L131
        cx.sg.set_draw_color_a(0.0, 0.0, 0.0, 0.75);
        // L132
        cx.sg.draw_image(None, 0.0, 0.0, screen_w, screen_h);
        // L133
        cx.sg.set_draw_color(1.0, 1.0, 1.0);
        // L134
        cx.sg
            .draw_string(0.0, screen_h / 2.0, Align::Center, 24.0, Font::Fixed, &text);
        // L135
        cx.sys.platform.restart();
    }
    // L137
    if !cx.app().launch.dev_mode
        && (cx.sys.time_ms() - cx.app().launch.last_update_check) > 1000 * 60 * 60 * 12
    {
        // L138-139 Do an update check every 12 hours if the user keeps the program open
        check_for_update(cx, Some(true));
    }
    // D8: controls closed or replaced during this frame are freed now (Lua: garbage).
    cx.end_frame_gc();
}

/// lua: Launch.lua:143-175 launch:OnKeyDown(key, doubleClick)
/// @pob Launch.lua:143-175 h=8d76a831
pub fn on_key_down(cx: &mut Cx, key: Key, double_click: bool) {
    let dev_mode = cx.app().launch.dev_mode;
    // L144
    if key == Key::F(5) && dev_mode {
        // L145
        cx.app_mut().launch.do_restart = Some("Restarting...".to_owned());
    } else if key == Key::F(6) && dev_mode {
        // L147-149 collectgarbage: no Lua heap; the report is printed with zeros
        cx.sys.platform.con_print("0kB => 0kB");
    } else if key == Key::Pause && dev_mode && PROFILER {
        // L150-158 profiler.start / stop / report (no profiler in the port)
    } else if key == Key::Char(b'u') && cx.sys.is_key_down(Key::Ctrl) {
        // L160
        if !dev_mode {
            // L161
            check_for_update(cx, None);
        }
    } else if key == Key::PrintScreen && cx.sys.is_key_down(Key::Ctrl) {
        // L164
        cx.sys.platform.take_screenshot();
    } else if cx.app().launch.prompt_msg.is_some() {
        // L166
        run_prompt_func(cx, key);
    } else {
        // L168
        if cx.app().launch.main {
            // L169
            let err_msg = pcall(cx, |cx| {
                main_mode::on_frame::main_on_key_down(cx, key, double_click);
            })
            .err();
            // L170
            if let Some(err_msg) = err_msg {
                // L171
                show_err_msg(cx, "In 'OnKeyDown': %s", &[FmtArg::from(err_msg.as_str())]);
            }
        }
    }
}

/// lua: Launch.lua:177-186 launch:OnKeyUp(key)
/// @pob Launch.lua:177-186 h=31b4ebe2
pub fn on_key_up(cx: &mut Cx, key: Key) {
    // L178
    if cx.app().launch.prompt_msg.is_none() {
        // L179
        if cx.app().launch.main {
            // L180
            let err_msg = pcall(cx, |cx| main_mode::on_frame::main_on_key_up(cx, key)).err();
            // L181
            if let Some(err_msg) = err_msg {
                // L182
                show_err_msg(cx, "In 'OnKeyUp': %s", &[FmtArg::from(err_msg.as_str())]);
            }
        }
    }
}

/// lua: Launch.lua:188-199 launch:OnChar(key)
/// @pob Launch.lua:188-199 h=cc933874
pub fn on_char(cx: &mut Cx, key: Key) {
    // L189
    if cx.app().launch.prompt_msg.is_some() {
        // L190
        run_prompt_func(cx, key);
    } else {
        // L192
        if cx.app().launch.main {
            // L193
            let err_msg = pcall(cx, |cx| main_mode::on_frame::main_on_char(cx, key)).err();
            // L194
            if let Some(err_msg) = err_msg {
                // L195
                show_err_msg(cx, "In 'OnChar': %s", &[FmtArg::from(err_msg.as_str())]);
            }
        }
    }
}

// @pob-skip Launch.lua:201-208 reason="launch:OnSubCall: Lua sub-scripts do not exist in the port (DIVERGENCE(subscripts))"

/// `self.subScripts[id]` (indexing a missing entry is a Lua error).
fn sub_script(cx: &Cx, id: u64) -> SubScript {
    cx.app()
        .launch
        .sub_scripts
        .get(&id)
        .cloned()
        .unwrap_or_else(|| lua_error!("attempt to index field '?' (a nil value)"))
}

/// lua: Launch.lua:210-221 launch:OnSubError(id, errMsg)
/// @pob Launch.lua:210-221 h=e4299d12
pub fn on_sub_error(cx: &mut Cx, id: u64, err_msg: &str) {
    match sub_script(cx, id) {
        // L211
        SubScript::Update => {
            // L212
            show_err_msg(cx, "In update thread: %s", &[FmtArg::from(err_msg)]);
            // L213
            cx.app_mut().launch.update_check_running = false;
        }
        // L214
        SubScript::Download(callback) => {
            // L215 PCall(self.subScripts[id].callback, nil, errMsg)
            let msg = err_msg.to_owned();
            let e = pcall(cx, |cx| {
                callback(
                    cx,
                    PageResponse {
                        header: None,
                        body: None,
                    },
                    Some(msg),
                );
            })
            .err();
            // L216
            if let Some(e) = e {
                // L217
                show_err_msg(cx, "In download callback: %s", &[FmtArg::from(e.as_str())]);
            }
        }
        SubScript::Custom(_) => {}
    }
    // L220
    cx.app_mut().launch.sub_scripts.shift_remove(&id);
}

/// lua: Launch.lua:223-244 launch:OnSubFinished(id, ...)
/// @pob Launch.lua:223-244 h=aa8547c7
pub fn on_sub_finished(cx: &mut Cx, id: u64, resp: HttpResponse) {
    match sub_script(cx, id) {
        // L224
        SubScript::Update => {
            // L225 self.updateAvailable, self.updateErrMsg = ...
            let l = &mut cx.app_mut().launch;
            l.update_available = resp.body.map(|b| String::from_utf8_lossy(&b).into_owned());
            l.update_err_msg = resp.error;
            // L226
            l.update_check_running = false;
            // L227
            if l.update_check_background == Some(true)
                && l.update_available.as_deref() == Some("none")
            {
                // L228
                l.update_available = None;
            }
        }
        // L230
        SubScript::Download(callback) => {
            // L231 PCall(callback, responseBody, errMsg, responseHeader)
            let page = PageResponse {
                header: Some(resp.headers),
                body: resp.body.map(|b| String::from_utf8_lossy(&b).into_owned()),
            };
            let err = resp.error;
            let e = pcall(cx, |cx| callback(cx, page, err)).err();
            // L232
            if let Some(e) = e {
                // L233
                show_err_msg(cx, "In download callback: %s", &[FmtArg::from(e.as_str())]);
            }
        }
        // L235
        SubScript::Custom(callback) => {
            // L236
            if let Some(callback) = callback {
                // L237
                let e = pcall(cx, |cx| callback(cx, resp)).err();
                // L238
                if let Some(e) = e {
                    // L239
                    show_err_msg(cx, "In subscript callback: %s", &[FmtArg::from(e.as_str())]);
                }
            }
        }
    }
    // L243
    cx.app_mut().launch.sub_scripts.shift_remove(&id);
}

/// `App::on_net` (CCR-0026 A): a `Net` completion is `launch:OnSubFinished(id, ...)`, or
/// `launch:OnSubError(id, errMsg)` when the request produced no body at all (the sub-script
/// failed).
pub fn on_net(cx: &mut Cx, id: u64, resp: HttpResponse) {
    if resp.body.is_none() && matches!(sub_script(cx, id), SubScript::Download(_)) {
        let msg = resp.error.unwrap_or_default();
        on_sub_error(cx, id, &msg);
    } else {
        on_sub_finished(cx, id, resp);
    }
}

/// lua: Launch.lua:246-253 launch:RegisterSubScript(id, callback)
/// @pob Launch.lua:246-253 h=8c231878
pub fn register_sub_script(
    cx: &mut Cx,
    id: Option<u64>,
    callback: Option<Rc<dyn Fn(&mut Cx, HttpResponse)>>,
) {
    // L247
    if let Some(id) = id {
        // L248-251
        cx.app_mut()
            .launch
            .sub_scripts
            .insert(id, SubScript::Custom(callback));
    }
}

/// `LoadModule("UpdateCheck")` (the synchronous first-run update check): DIVERGENCE(update-stub),
/// always "none".
fn load_update_check() -> Option<&'static str> {
    Some("none")
}

/// launch:ApplyUpdate(mode): DIVERGENCE(update-stub), nothing to apply.
// @pob-skip Launch.lua:325-337 reason="launch:ApplyUpdate: the update machinery is stubbed (ui doc §12 U16 row)"
pub fn apply_update(cx: &mut Cx, mode: &str) {
    let _ = (cx, mode);
}

/// launch:CheckForUpdate(inBackground): DIVERGENCE(update-stub), returns at once (as the UI
/// oracle's patch 3 makes the Lua do).
// @pob-skip Launch.lua:339-356 reason="launch:CheckForUpdate: the update machinery is stubbed (ui doc §12 U16 row)"
pub fn check_for_update(cx: &mut Cx, in_background: Option<bool>) {
    let _ = (cx, in_background);
}

/// lua: Launch.lua:358-379 launch:ShowPrompt(r, g, b, str, func)
/// @pob Launch.lua:358-379 h=17e47b57
pub fn show_prompt(cx: &mut Cx, r: f64, g: f64, b: f64, str: &str, func: Option<PromptFn>) {
    // L359-360
    cx.app_mut().launch.prompt_msg = Some(str.to_owned());
    cx.app_mut().launch.prompt_col = [r, g, b];
    let s = str.to_owned();
    // L361
    let default: PromptFn = Rc::new(move |cx: &mut Cx, key: Key| {
        // L362
        if key == Key::Return || key == Key::Escape {
            // L363
            true
        } else if key == Key::F(4) {
            // L365
            if cx.app().launch.main {
                // L366 self.main.popups = { }
                let old: Vec<_> = std::mem::take(&mut cx.main.popups);
                for p in old {
                    cx.bury(p);
                }
                // L367 self.main.inputEvents = { }
                *cx.app().main.input_events.borrow_mut() = InputEvents::default();
                // L368
                main_mode::on_frame::set_mode(cx, "LIST", Vec::new());
            }
            // L370
            true
        } else if key == Key::Char(b'c') && cx.sys.is_key_down(Key::Ctrl) {
            // L372
            let clean_str = pob_ui_core::luastr::gsub(&s, "%^%d", "");
            // L373
            cx.sys.platform.copy(&clean_str);
            false
        } else if key == Key::F(5) {
            // L375
            cx.app_mut().launch.do_restart = Some("Restarting...".to_owned());
            // L376
            true
        } else {
            false
        }
    });
    cx.app_mut().launch.prompt_func = Some(func.unwrap_or(default));
}

/// lua: Launch.lua:381-388 launch:ShowErrMsg(fmt, ...)
/// @pob Launch.lua:381-388 h=d1b8d01a
pub fn show_err_msg(cx: &mut Cx, fmt: &str, args: &[FmtArg<'_>]) {
    // L382
    if cx.app().launch.prompt_msg.is_none() {
        let l = &cx.app().launch;
        // L383-385
        let version = match &l.version_number {
            Some(n) => format!(
                "^8v{n}{}",
                l.version_branch
                    .as_ref()
                    .map_or_else(String::new, |b| format!(" {b}"))
            ),
            None => String::new(),
        };
        // L386
        let msg = pob_lua::lua_format(fmt, args);
        let text = format!("^1Error:\n\n^0{msg}\n{version}\n^0Press Enter/Escape to dismiss, F4 to return to build selection, or F5 to restart the application.\nPress CTRL + C to copy error text.");
        show_prompt(cx, 1.0, 0.0, 0.0, &text, None);
    }
}

/// lua: Launch.lua:390-398 launch:RunPromptFunc(key)
/// @pob Launch.lua:390-398 h=3403ad3c
pub fn run_prompt_func(cx: &mut Cx, key: Key) {
    // L391
    let cur_msg = cx.app().launch.prompt_msg.clone();
    // L392
    let f = cx
        .app()
        .launch
        .prompt_func
        .clone()
        .unwrap_or_else(|| lua_error!("attempt to call field 'promptFunc' (a nil value)"));
    let r = pcall(cx, |cx| f(cx, key));
    match r {
        // L393-394
        Err(err_msg) => show_err_msg(cx, "In prompt func: %s", &[FmtArg::from(err_msg.as_str())]),
        // L395
        Ok(ret) => {
            if ret && cx.app().launch.prompt_msg == cur_msg {
                // L396
                cx.app_mut().launch.prompt_msg = None;
            }
        }
    }
}

/// lua: Launch.lua:400-416 launch:DrawPopup(r, g, b, fmt, ...)
/// @pob Launch.lua:400-416 h=89bff662
pub fn draw_popup(cx: &mut Cx, r: f64, g: f64, b: f64, fmt: &str, args: &[FmtArg<'_>]) {
    // L401
    let (screen_w, screen_h) = cx.sg.get_virtual_screen_size();
    // L402
    cx.sg.set_draw_color_a(0.0, 0.0, 0.0, 0.5);
    // L403
    cx.sg.draw_image(None, 0.0, 0.0, screen_w, screen_h);
    // L404
    let txt = pob_lua::lua_format(fmt, args);
    // L405
    let w = cx.sg.draw_string_width(20.0, Font::Var, &txt) + 20.0;
    // L406 (#txt:gsub("[^\n]","") + 2) * 20
    let h = (txt.bytes().filter(|&c| c == b'\n').count() as f64 + 2.0) * 20.0;
    // L407-408
    let ox = (screen_w - w) / 2.0;
    let oy = (screen_h - h) / 2.0;
    // L409-410
    cx.sg.set_draw_color(1.0, 1.0, 1.0);
    cx.sg.draw_image(None, ox, oy, w, h);
    // L411-412
    cx.sg.set_draw_color(r, g, b);
    cx.sg.draw_image(None, ox + 2.0, oy + 2.0, w - 4.0, h - 4.0);
    // L413-414
    cx.sg.set_draw_color(1.0, 1.0, 1.0);
    cx.sg.draw_image(None, ox + 4.0, oy + 4.0, w - 8.0, h - 8.0);
    // L415
    cx.sg
        .draw_string(0.0, oy + 10.0, Align::Center, 20.0, Font::Var, &txt);
}
