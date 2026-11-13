//! Port of PathOfBuilding v2.67.2 (16de4b82d) src/Modules/Build.lua 1-2335 (the build mode
//! object). Owner: U19 (UI-U0 declares the split, ui doc §12.1). The calc reads `model`; UI-only
//! buildMode fields live in the `BuildMode` widget state and in `BuildState`.
//!
//! @pob-file Modules/Build.lua @16de4b82d
//!
//! `buildMode` is one Lua object (`new("ControlHost"):ControlHost()`, Build.lua:22) whose methods
//! are spread over U19 (Init, init.rs), U20 (loadouts, io, on_frame, popups, class_drop) and U21
//! (skill_select, stats, display_stats). `main.modes.BUILD` is [`BuildModeObj`] (the
//! `main_mode::Mode` trait, CCR-0096 A); methods of U20/U21 and of the tabs that U19's code calls
//! go through [`init::BuildHooks`] (CCR-0098 §3) until their owners install
//! theirs. `main:Init` installs [`mode`] as `main.modes.BUILD` (CCR-0098 §5, done by U20).
//!
//! The Rust node is created afresh by every `Init` (the Lua object persists across builds; every
//! field `Init` reads before writing is listed in init.rs).

pub mod class_drop;
pub mod display_stats;
pub mod init;
pub mod io;
pub mod loadouts;
pub mod on_frame;
pub mod popups;
pub mod skill_select;
pub mod stats;

use std::rc::Rc;

use pob_ui_core::ids::Ctrl;
use pob_ui_core::input::InputEvents;
use pob_ui_core::prelude::*;

use crate::app::CxApp;
use crate::calc::{CalcFacade, MiscCalc};
use crate::main_mode::{Mode, ModeArg};
use crate::model;
use crate::tabs::{
    calcs::CalcsTab, compare::CompareTab, config::ConfigTab, import::ImportTab, items::ItemsTab,
    notes::NotesTab, party::PartyTab, skills::SkillsTab, tree::TreeTab,
};

/// The buildMode ControlHost's widget state (one Lua object split over U19, U20, U21).
#[derive(Default)]
pub struct BuildMode {
    pub init: init::State,
    pub on_frame: on_frame::State,
    pub stats: stats::State,
}

/// `buildMode = new("ControlHost"):ControlHost()` (Build.lua:22): its class name is
/// "ControlHost", which the control-path markers print. It has no `Draw` of its own: `OnFrame`
/// (U20) draws the tabs and calls `DrawControls`.
impl Widget for BuildMode {
    const NAME: &'static str = "ControlHost";
    const CAPS: Caps = Caps::empty();
}

/// The tab nodes (`build.treeTab`, `build.skillsTab`, ...). A tab whose UI constructor is not
/// ported yet holds `Ctrl::null()` (the `*_tab` entries of [`init::BuildHooks`]).
pub struct TabIds {
    pub tree: Ctrl<TreeTab>,
    pub skills: Ctrl<SkillsTab>,
    pub items: Ctrl<ItemsTab>,
    pub calcs: Ctrl<CalcsTab>,
    pub config: Ctrl<ConfigTab>,
    pub import: Ctrl<ImportTab>,
    pub notes: Ctrl<NotesTab>,
    pub party: Ctrl<PartyTab>,
    pub compare: Ctrl<CompareTab>,
}

impl Default for TabIds {
    fn default() -> TabIds {
        TabIds {
            tree: Ctrl::null(),
            skills: Ctrl::null(),
            items: Ctrl::null(),
            calcs: Ctrl::null(),
            config: Ctrl::null(),
            import: Ctrl::null(),
            notes: Ctrl::null(),
            party: Ctrl::null(),
            compare: Ctrl::null(),
        }
    }
}

/// Some while in BUILD mode (ui doc §10.1).
///
/// CCR-0098 §1: the frozen `build_flag` field is gone. Lua has one `self.buildFlag`, which the
/// model halves of the calc units write too (`build.buildFlag`, e.g. `ItemsTab:SetActiveItemSet`
/// model part), so the one flag is the model's `model.buildFlag`.
pub struct BuildState {
    /// the buildMode ControlHost (UI-only fields live in its widget state)
    pub node: Ctrl<BuildMode>,
    /// tree, skills, items, calcs, config, import, notes, party, compare (nodes)
    pub tabs: TabIds,
    /// the calc doc's Build: everything the calc reads and XML stores
    pub model: model::BuildRef,
    pub calc: Box<dyn CalcFacade>,
    /// configTab.calcFunc (UI-side home; the calc doc's ConfigTab has none)
    pub calc_func: Option<Rc<dyn MiscCalc>>,
    /// configTab.calcBase
    pub calc_base: Option<model::Output>,
    /// `self.modFlag` (the UI's flag; `Init` copies BM-A's load-time value, Build.lua:94/100)
    pub mod_flag: bool,
    /// `self.dbFileName` (nil and `false` are both `None`; see `db_file_name_is_false`)
    pub db_file_name: Option<String>,
    /// `self.dbFileName == false` (`GetArgs` and Settings.xml tell `false` from nil: `ipairs`
    /// stops at a nil first argument, Main.lua:744)
    pub db_file_name_is_false: bool,
    /// `self.dbFileSubPath` (nil → `None`)
    pub db_file_sub_path: Option<String>,
    /// `self.buildName` (nil → `None`: Init goes on with a nil name after L74, whose
    /// `SetMode("LIST")` takes effect on the next frame)
    pub build_name: Option<String>,
    /// `self.importLink`
    pub import_link: Option<String>,
    /// `self.saveAsSortMode`
    pub save_as_sort_mode: String,
    /// `self.abortSave` (nil → `None`)
    pub abort_save: Option<bool>,
    /// `self.unsaved` (computed by OnFrame, U20; nil until then)
    pub unsaved: bool,
    // … remaining UI-side buildMode fields of U20/U21 live in their `BuildMode` widget state
    // (`on_frame::State`, `stats::State`); fields the calc reads live in `model`.
}

/// `main.modes.BUILD` (Main.lua:63 `LoadModule("Modules/Build")`): the module object
/// `local buildMode = new("ControlHost"):ControlHost()`.
/// @pob Modules/Build.lua:22-22 h=43d84785
pub fn mode() -> Rc<dyn Mode> {
    Rc::new(BuildModeObj)
}

/// The `buildMode` module object as `main` sees it (CCR-0096 A). The state lives in
/// `cx.app().build` and the `BuildMode` node; this object only routes `main:CallMode`.
pub struct BuildModeObj;

impl Mode for BuildModeObj {
    /// `buildMode:Init(dbFileName, buildName, buildXML, convertBuild, importLink)`
    fn init(&self, cx: &mut Cx, args: &[ModeArg]) {
        init::init(cx, args);
    }
    /// `buildMode:Shutdown()` (Build.lua:951-965, U20).
    fn shutdown(&self, cx: &mut Cx) {
        let f = init::hooks(cx).shutdown.clone();
        f(cx);
    }
    /// `buildMode:OnFrame(inputEvents)` (Build.lua:1160-1352, U20).
    fn on_frame(&self, cx: &mut Cx, input_events: &mut InputEvents, vp: &ViewPort) {
        let f = init::hooks(cx).on_frame.clone();
        f(cx, input_events, vp);
    }
    /// lua: Build.lua:967-969 `buildMode:GetArgs()` (U20's range; the Mode trait needs it here)
    /// @pob Modules/Build.lua:967-969 h=3338c457
    fn get_args(&self, cx: &Cx) -> Option<Vec<ModeArg>> {
        let b = cx.app().build.as_ref()?;
        // L968 return self.dbFileName, self.buildName
        let db = match &b.db_file_name {
            Some(s) => ModeArg::Str(s.clone()),
            None if b.db_file_name_is_false => ModeArg::Bool(false),
            None => ModeArg::Nil,
        };
        let name = b.build_name.clone().map_or(ModeArg::Nil, ModeArg::Str);
        Some(vec![db, name])
    }
    /// `buildMode:CanExit(mode)` (Build.lua:943-949, U20).
    fn can_exit(&self, cx: &mut Cx, reason: &str) -> Option<bool> {
        let f = init::hooks(cx).can_exit.clone();
        f(cx, reason)
    }
    /// `buildMode.viewMode` (nil before the first Init)
    fn view_mode(&self, cx: &Cx) -> Option<String> {
        let b = cx.app().build.as_ref()?;
        Some(b.model.viewMode.get().to_string())
    }
    /// `buildMode.viewMode = v` (Main.lua:395)
    fn set_view_mode(&self, cx: &mut Cx, v: &str) {
        if let Some(b) = cx.app().build.as_ref() {
            b.model.viewMode.set(model::Name::from(v));
        }
    }
    /// `main.modes.BUILD.outputRevision` (Launch.lua:115)
    fn output_revision(&self, cx: &Cx) -> Option<f64> {
        let b = cx.app().build.as_ref()?;
        Some(b.model.outputRevision.get())
    }
    /// `main.modes.BUILD.buildFlag` (Launch.lua:115)
    fn build_flag(&self, cx: &Cx) -> bool {
        cx.app()
            .build
            .as_ref()
            .is_some_and(|b| b.model.buildFlag.get())
    }
}
