//! App state and the host entry (ui doc §10.1). Owner: U16. UI-U0: the §10.1 surface; `CxApp`
//! and the disjoint-borrow helpers are complete, the callbacks are U16's.
//!
//! `PobApp` is SimpleGraphic's main object: `SetMainObject(launch)` (Launch.lua:17) makes SG
//! call `launch:OnInit`, `OnFrame`, `OnKeyDown`, ... which are the `pob_host::App` callbacks here
//! (crate::launch ports them). `PobApp::new` runs Launch.lua's module-level code (:8-19), which in
//! SG runs when the script is loaded, before `OnInit`.

use std::any::Any;
use std::collections::HashMap;
use std::rc::Rc;

use pob_sg::Key;
use pob_ui_core::arena::Ui;
use pob_ui_core::cx::Cx;
use pob_ui_core::ids::{Ctrl, CtrlId};
use pob_ui_core::lua_error;
use pob_ui_core::main_core::MainCore;

use crate::build::BuildState;
use crate::build_list::ListMode;
use crate::calc::CalcFacade;
use crate::launch::{self, LaunchState};
use crate::main_mode::{self, MainState, ModeArg};
use crate::model;
use crate::tree_view::assets::TreeAssets;

/// implements pob_host::App
pub struct PobApp {
    pub cx: Cx,
}

impl PobApp {
    /// What `pob_host::run_desktop(cfg, pob_app::PobApp::new)` calls: builds `Cx` (MainCore,
    /// AppState) around the host's `Sg`/`Sys`, then runs Launch.lua's module-level code.
    pub fn new(parts: pob_host::PlatformParts) -> PobApp {
        Self::with_calc(parts, Rc::new(|| Box::new(crate::calc::RustCalc)))
    }

    /// [`PobApp::new`] with another calc (`LuaCalcBridge` in dev builds, ui doc §10.4).
    pub fn with_calc(
        parts: pob_host::PlatformParts,
        calc_factory: Rc<dyn Fn() -> Box<dyn CalcFacade>>,
    ) -> PobApp {
        let state = AppState {
            launch: LaunchState::default(),
            main: MainState::default(),
            trees: TreeCache::default(),
            list_mode: None,
            build: None,
            calc_factory,
        };
        let mut cx = Cx {
            ui: Ui::default(),
            sg: parts.sg,
            sys: parts.sys,
            main: MainCore::new(CtrlId::default()),
            ext: Box::new(state),
        };
        launch::load_module(&mut cx);
        PobApp { cx }
    }

    /// `main:SetMode("BUILD", false, name, xml)`: the `.pobscript` step `load-build` (ui doc §5.2).
    pub fn load_build(&mut self, name: &str, xml: &str) {
        main_mode::on_frame::set_mode(
            &mut self.cx,
            "BUILD",
            vec![
                ModeArg::Bool(false),
                ModeArg::Str(name.to_owned()),
                ModeArg::Str(xml.to_owned()),
            ],
        );
    }

    /// The `.pobscript` step `snapshot` (T4): `main.modes.BUILD:SaveDB("snapshot")` while in
    /// BUILD mode. CCR-0091: the build's SaveDB is U20's (build/io.rs); until it lands there is
    /// nothing to save.
    pub fn snapshot(&mut self) -> Option<String> {
        None
    }

    /// The Settings.xml text `main:SaveSettings` would write now (T4), or `None` where it
    /// refuses (an invalid mode element, or `errorReadingSettings`).
    pub fn settings_xml(&mut self) -> Option<String> {
        if self.cx.app().main.error_reading_settings {
            return None;
        }
        main_mode::settings::compose_settings(&mut self.cx).map(|x| model::xml::ComposeXML(&x))
    }
}

impl pob_host::App for PobApp {
    fn sg(&mut self) -> &mut pob_sg::Sg {
        &mut self.cx.sg
    }
    fn sys(&mut self) -> &mut pob_sg::Sys {
        &mut self.cx.sys
    }
    /// launch:OnInit
    fn on_init(&mut self) {
        launch::on_init(&mut self.cx);
    }
    /// launch:OnFrame
    fn on_frame(&mut self) {
        launch::on_frame(&mut self.cx);
    }
    fn on_key_down(&mut self, key: Key, double_click: bool) {
        launch::on_key_down(&mut self.cx, key, double_click);
    }
    fn on_key_up(&mut self, key: Key) {
        launch::on_key_up(&mut self.cx, key);
    }
    fn on_char(&mut self, ch: Key) {
        launch::on_char(&mut self.cx, ch);
    }
    /// launch:CanExit
    fn can_exit(&mut self) -> bool {
        launch::can_exit(&mut self.cx)
    }
    /// launch:OnExit
    fn on_exit(&mut self) {
        launch::on_exit(&mut self.cx);
    }
    /// hasActiveCoroutine (`coroutine._list()`, Common.lua:55-57; the item DB loader of
    /// main:Init and the others). `sys.net.busy()` is added by the host (hasSubscript).
    fn wants_frames(&self) -> bool {
        pob_ui_core::coroutine::co::any_alive()
    }
    /// A callback panicked outside launch's own `PCall`s: Launch's error prompt
    /// (Launch.lua:381 ShowErrMsg).
    fn on_lua_error(&mut self, msg: String) {
        launch::show_err_msg(&mut self.cx, "%s", &[pob_lua::FmtArg::from(msg.as_str())]);
    }
    /// launch:OnSubFinished / OnSubError (CCR-0026 A).
    fn on_net(&mut self, id: u64, resp: pob_sg::HttpResponse) {
        launch::on_net(&mut self.cx, id, resp);
    }
    /// `load-build` and `snapshot` (CCR-0026 E).
    fn on_script_step(&mut self, step: &pob_host::script::Step) {
        match step {
            pob_host::script::Step::LoadBuild(path) => {
                // relative to `<userdir>/Path of Building/` (U7's rule); the app's user path is
                // that directory
                let user_path = self.cx.app().main.user_path.clone().unwrap_or_default();
                let file = format!("{user_path}{path}");
                let xml = match self.cx.sys.fs.read(&file) {
                    Ok(b) => String::from_utf8_lossy(&b).into_owned(),
                    Err(e) => lua_error!("load-build {file}: {e}"),
                };
                let name = std::path::Path::new(path)
                    .file_stem()
                    .map(|s| s.to_string_lossy().into_owned())
                    .unwrap_or_default();
                self.load_build(&name, &xml);
            }
            pob_host::script::Step::Snapshot(_) => {
                let _ = self.snapshot();
            }
            _ => {}
        }
    }
}

/// Typed access to pob-app's state through `cx.ext`.
pub trait CxApp {
    fn app(&self) -> &AppState;
    fn app_mut(&mut self) -> &mut AppState;
    fn build(&self) -> &BuildState {
        self.app()
            .build
            .as_ref()
            .unwrap_or_else(|| lua_error!("build is nil"))
    }
    fn build_mut(&mut self) -> &mut BuildState {
        self.app_mut()
            .build
            .as_mut()
            .unwrap_or_else(|| lua_error!("build is nil"))
    }
    /// pob_gamedata::Data (process global, calc doc §4)
    fn data(&self) -> &'static model::Data {
        model::data()
    }
}

/// downcast cx.ext
impl CxApp for Cx {
    fn app(&self) -> &AppState {
        app(&*self.ext)
    }
    fn app_mut(&mut self) -> &mut AppState {
        app_mut(&mut *self.ext)
    }
}

/// Disjoint-borrow helpers: `let a = app(&*cx.ext); cx.sg.draw_string(.., &a.main.x)`.
pub fn app(ext: &dyn Any) -> &AppState {
    ext.downcast_ref::<AppState>()
        .expect("cx.ext is not pob_app::AppState")
}
pub fn app_mut(ext: &mut dyn Any) -> &mut AppState {
    ext.downcast_mut::<AppState>()
        .expect("cx.ext is not pob_app::AppState")
}

/// `main.tree[version]`: the calc plan's tree plus the UI images.
#[derive(Default)]
pub struct TreeCache {
    pub trees: HashMap<String, (model::PassiveTreeRef, Rc<TreeAssets>)>,
}

/// pob-app's globals.
pub struct AppState {
    /// Launch.lua fields (devMode, versionNumber, promptMsg, subscripts…)
    pub launch: LaunchState,
    /// Main.lua fields not in MainCore: mode/newMode/modes, settings, paths, sharedItems, option
    /// values, inputEvents, toasts…
    pub main: MainState,
    /// main.tree[version]: model::PassiveTreeRef + Rc<TreeAssets> (UI images)
    pub trees: TreeCache,
    pub list_mode: Option<Ctrl<ListMode>>,
    /// Some while in BUILD mode
    pub build: Option<BuildState>,
    /// Rust calc (default) | LuaCalcBridge (dev)
    pub calc_factory: Rc<dyn Fn() -> Box<dyn CalcFacade>>,
}
