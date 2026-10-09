//! Port of PathOfBuilding v2.67.2 (16de4b82d) src/Modules/Build.lua 1-2335 (the build mode
//! object). Owner: U19 (UI-U0 declares the split, ui doc §12.1). The calc reads `model`; UI-only
//! buildMode fields live in the `BuildMode` widget state and in `BuildState`.

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

use crate::calc::{CalcFacade, MiscCalc};
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

/// The tab nodes (`build.treeTab`, `build.skillsTab`, ...).
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

/// Some while in BUILD mode (ui doc §10.1).
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
    pub build_flag: bool,
    pub mod_flag: bool,
    pub db_file_name: Option<String>,
    pub db_file_sub_path: Option<String>,
    pub build_name: String,
    // … remaining UI-side buildMode fields (characterLevelAutoMode, …) are U19-U21's; fields the
    // calc reads live in `model`.
}
