//! App state and the host entry (ui doc §10.1). Owner: U16. UI-U0: the §10.1 surface; `CxApp`
//! and the disjoint-borrow helpers are complete, the callbacks are U16's.

use std::any::Any;
use std::collections::HashMap;
use std::rc::Rc;

use pob_sg::Key;
use pob_ui_core::cx::Cx;
use pob_ui_core::ids::Ctrl;
use pob_ui_core::lua_error;

use crate::build::BuildState;
use crate::build_list::ListMode;
use crate::calc::CalcFacade;
use crate::launch::LaunchState;
use crate::main_mode::MainState;
use crate::model;
use crate::tree_view::assets::TreeAssets;

/// implements pob_host::App
pub struct PobApp {
    pub cx: Cx,
}

impl PobApp {
    /// What `pob_host::run_desktop(cfg, pob_app::PobApp::new)` calls: builds `Cx` (MainCore,
    /// AppState) around the host's `Sg`/`Sys`.
    pub fn new(parts: pob_host::PlatformParts) -> PobApp {
        let _ = parts;
        todo!("U16: PobApp::new (Launch.lua:21-108 before OnInit)")
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
        todo!("U16: launch:OnInit")
    }
    /// launch:OnFrame
    fn on_frame(&mut self) {
        todo!("U16: launch:OnFrame")
    }
    fn on_key_down(&mut self, key: Key, double_click: bool) {
        let _ = (key, double_click);
        todo!("U16: launch:OnKeyDown")
    }
    fn on_key_up(&mut self, key: Key) {
        let _ = key;
        todo!("U16: launch:OnKeyUp")
    }
    fn on_char(&mut self, ch: Key) {
        let _ = ch;
        todo!("U16: launch:OnChar")
    }
    fn on_lua_error(&mut self, msg: String) {
        let _ = msg;
        todo!("U16: launch:ShowErrMsg (Launch.lua:381)")
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
