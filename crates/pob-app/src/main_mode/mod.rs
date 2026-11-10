//! Port of PathOfBuilding v2.67.2 (16de4b82d) src/Modules/Main.lua 1-1822 (the `main` object; its
//! generic-control part is pob_ui_core::main_core::MainCore). Owner: U16 (popups: U17). Module
//! declarations: UI-U0.
//!
//! `main` is split three ways (ui doc §6.11, §10.1): [`MainCore`](pob_ui_core::main_core::MainCore)
//! (`cx.main`: the fields generic controls read), [`MainState`] (`cx.app().main`: everything else)
//! and the `main` ControlHost node ([`MainHost`], `cx.main.node`) that owns the bottom-bar controls.
//!
//! The mode objects (`main.modes.LIST` = Modules/BuildList.lua, `main.modes.BUILD` =
//! Modules/Build.lua) are U18's and U19-U21's. `main:CallMode` reaches them through the [`Mode`]
//! trait; until those units register their objects, `main:Init` installs [`PendingMode`]
//! placeholders (CCR-0091). The popups `main:OpenOptionsPopup` / `OpenAboutPopup` /
//! `OpenUpdatePopup` are U17's and are reached through [`MainHooks`] (CCR-0091).
//!
//! Props with side effects (ui doc §7.3): none in U16's range (the bottom-bar closures only read).

pub mod about_popup;
pub mod background;
pub mod folders;
pub mod init;
pub mod on_frame;
pub mod options_popup;
pub mod path_popup;
pub mod settings;
pub mod toast;
pub mod update_popup;

use std::cell::RefCell;
use std::rc::Rc;

use indexmap::IndexMap;
use pob_ui_core::input::InputEvents;
use pob_ui_core::prelude::*;

use crate::model;

/// `main = new("ControlHost"):ControlHost()` (Main.lua:57): the `main` node. Its class name is
/// "ControlHost", which the control-path markers print.
pub struct MainHost;
impl Widget for MainHost {
    const NAME: &'static str = "ControlHost";
    const CAPS: Caps = Caps::empty();
}

/// One value of a mode's argument list (`main:SetMode(newMode, ...)`, `GetArgs`, the `Arg`
/// elements of Settings.xml): Lua `nil`, a boolean, a number or a string. These are the only
/// types PoB passes (BuildList.lua:15, Build.lua:64, Main.lua:546-559, :744-754).
#[derive(Clone, Debug, PartialEq)]
pub enum ModeArg {
    Nil,
    Bool(bool),
    Num(f64),
    Str(String),
}

impl ModeArg {
    /// Lua truthiness (nil and false are falsy).
    pub fn truthy(&self) -> bool {
        !matches!(self, ModeArg::Nil | ModeArg::Bool(false))
    }
    pub fn is_nil(&self) -> bool {
        matches!(self, ModeArg::Nil)
    }
    /// The string, if this is one.
    pub fn as_str(&self) -> Option<&str> {
        match self {
            ModeArg::Str(s) => Some(s),
            _ => None,
        }
    }
    /// `tostring(val)` (Main.lua:747-751).
    pub fn tostring(&self) -> String {
        match self {
            ModeArg::Nil => "nil".to_owned(),
            ModeArg::Bool(b) => b.to_string(),
            ModeArg::Num(n) => pob_lua::tostring_num(*n),
            ModeArg::Str(s) => s.clone(),
        }
    }
}

impl From<&str> for ModeArg {
    fn from(s: &str) -> ModeArg {
        ModeArg::Str(s.to_owned())
    }
}
impl From<String> for ModeArg {
    fn from(s: String) -> ModeArg {
        ModeArg::Str(s)
    }
}
impl From<bool> for ModeArg {
    fn from(b: bool) -> ModeArg {
        ModeArg::Bool(b)
    }
}
impl From<Option<String>> for ModeArg {
    fn from(s: Option<String>) -> ModeArg {
        s.map_or(ModeArg::Nil, ModeArg::Str)
    }
}

/// `unpack(args)` as seen by a callee: argument `i` (1-based), nil past the end.
pub fn mode_arg(args: &[ModeArg], i: usize) -> ModeArg {
    args.get(i - 1).cloned().unwrap_or(ModeArg::Nil)
}

/// A mode object (`main.modes[name]`): the methods `main:CallMode` (Main.lua:513-518) and the
/// other `main.modes.*` readers call. CCR-0091: U18 implements it for BuildList.lua's
/// `listMode`, U19-U21 for Build.lua's `buildMode`.
///
/// Methods whose Lua function may be absent return `Option` (`modeTbl[func]` is nil: CallMode
/// returns nothing).
pub trait Mode {
    /// `mode:Init(...)` with `unpack(main.newModeArgs)`.
    fn init(&self, cx: &mut Cx, args: &[ModeArg]);
    /// `mode:Shutdown()`.
    fn shutdown(&self, cx: &mut Cx) {
        let _ = cx;
    }
    /// `mode:OnFrame(inputEvents, viewPort)`.
    fn on_frame(&self, cx: &mut Cx, input_events: &mut InputEvents, vp: &ViewPort);
    /// `{ mode:GetArgs() }` (Main.lua:744); `None` when the mode has no `GetArgs`.
    fn get_args(&self, cx: &Cx) -> Option<Vec<ModeArg>>;
    /// `mode:CanExit(reason)` (Main.lua:366 "EXIT", :1362 "UPDATE"); `None` when absent or
    /// when it returns nothing.
    fn can_exit(&self, cx: &mut Cx, reason: &str) -> Option<bool> {
        let _ = (cx, reason);
        None
    }
    /// `mode.viewMode` (Main.lua:479); `None` is nil.
    fn view_mode(&self, cx: &Cx) -> Option<String> {
        let _ = cx;
        None
    }
    /// `mode.viewMode = v` (Main.lua:395).
    fn set_view_mode(&self, cx: &mut Cx, v: &str);
    /// `main.modes.BUILD.outputRevision` (Launch.lua:115); `None` is nil.
    fn output_revision(&self, cx: &Cx) -> Option<f64> {
        let _ = cx;
        None
    }
    /// `main.modes.BUILD.buildFlag` (Launch.lua:115).
    fn build_flag(&self, cx: &Cx) -> bool {
        let _ = cx;
        false
    }
}

/// CCR-0091: the stand-in mode object `main:Init` installs for `main.modes.LIST` and
/// `main.modes.BUILD` until U18 / U19 register theirs. It draws nothing; `GetArgs` returns the
/// first two `Init` arguments (what `listMode:GetArgs` and `buildMode:GetArgs` return for a mode
/// that was just opened with them: BuildList.lua:131-133, Build.lua:967-969), so Settings.xml
/// still round-trips.
pub struct PendingMode {
    pub name: &'static str,
    args: RefCell<Vec<ModeArg>>,
    view_mode: RefCell<Option<String>>,
}

impl PendingMode {
    pub fn new(name: &'static str) -> PendingMode {
        PendingMode {
            name,
            args: RefCell::new(Vec::new()),
            view_mode: RefCell::new(None),
        }
    }
}

impl Mode for PendingMode {
    fn init(&self, _cx: &mut Cx, args: &[ModeArg]) {
        *self.args.borrow_mut() = vec![mode_arg(args, 1), mode_arg(args, 2)];
    }
    fn on_frame(&self, _cx: &mut Cx, _input_events: &mut InputEvents, _vp: &ViewPort) {}
    fn get_args(&self, _cx: &Cx) -> Option<Vec<ModeArg>> {
        Some(self.args.borrow().clone())
    }
    fn view_mode(&self, _cx: &Cx) -> Option<String> {
        self.view_mode.borrow().clone()
    }
    fn set_view_mode(&self, _cx: &mut Cx, v: &str) {
        *self.view_mode.borrow_mut() = Some(v.to_owned());
    }
}

/// `main:OpenOptionsPopup()`, `main:OpenAboutPopup(helpSectionName)`, `main:OpenUpdatePopup()`
/// (U17's Main.lua:865-1485) and `main:LoadTree(treeVersion)` (U23's Main.lua:352-364), which
/// U16's code calls. CCR-0091: the owners install their functions here (one line each in
/// `init::default_hooks`); until then the popups raise a Lua error naming the missing unit and
/// `LoadTree` does nothing.
#[derive(Clone)]
pub struct MainHooks {
    pub open_options_popup: Rc<dyn Fn(&mut Cx)>,
    /// `helpSectionName`: a tab name such as `"tree tab"`, or the number 1.
    pub open_about_popup: Rc<dyn Fn(&mut Cx, ModeArg)>,
    pub open_update_popup: Rc<dyn Fn(&mut Cx)>,
    pub load_tree: Rc<dyn Fn(&mut Cx, &str)>,
}

/// `{ title = .., slots = { [slotName] = item } }` (Main.lua:717).
#[derive(Clone, Default)]
pub struct SharedItemSet {
    pub title: Option<String>,
    /// LUA-ORDER: string keys, insertion order (PoB's `pairs` order is random per process).
    pub slots: IndexMap<String, model::ItemRef>,
}

/// `main.gameAccounts[accountName] = { sessionID = .. }` (Main.lua:571).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct GameAccount {
    pub session_id: Option<String>,
}

/// `main.uniqueDB` / `main.rareDB` (Main.lua:159-161): `{ list = { }, byTitle = { }, loading = true }`.
#[derive(Clone, Default)]
pub struct ItemDb {
    /// `list[item.name] = item` (string keys; insertion order, LUA-ORDER).
    pub list: IndexMap<String, model::ItemRef>,
    /// `byTitle[item.title:lower()]` (uniqueDB only).
    pub by_title: IndexMap<String, model::ItemRef>,
    /// `loading` (true until the item coroutine finishes; nil after).
    pub loading: bool,
}

/// Main.lua fields not in MainCore: mode/newMode/modes, settings, paths, sharedItems, option
/// values, inputEvents, toasts...
pub struct MainState {
    /// main.inputEvents. `Rc<RefCell<..>>` so main:OnFrame can lend it to the mode and the
    /// controls while `cx` is borrowed, and a Lua error mid-frame leaves the events in place
    /// (the Lua table survives a failed `PCall`).
    pub input_events: Rc<RefCell<InputEvents>>,
    /// main.modes (Main.lua:61-63)
    pub modes: IndexMap<String, Rc<dyn Mode>>,
    /// main.mode / main.newMode / main.newModeArgs
    pub mode: Option<String>,
    pub new_mode: Option<String>,
    pub new_mode_args: Vec<ModeArg>,
    /// main.predefinedBuildName (cleared by SetMode)
    pub predefined_build_name: Option<String>,
    /// main.newModeChangeToTree
    pub new_mode_change_to_tree: bool,
    pub hooks: MainHooks,
    pub shared_item_list: Vec<model::ItemRef>,
    pub shared_item_set_list: Vec<SharedItemSet>,
    /// LUA-ORDER: string keys, insertion order.
    pub game_accounts: IndexMap<String, GameAccount>,
    pub user_path: Option<String>,
    pub default_build_path: Option<String>,
    pub build_path: Option<String>,
    pub build_sort_mode: String,
    /// main.connectionProtocol (Main.lua:103; LoadSettings writes launch.connectionProtocol)
    pub connection_protocol: f64,
    pub node_power_theme: String,
    pub color_positive: String,
    pub color_negative: String,
    pub color_highlight: String,
    pub use_compact_values: bool,
    pub edge_search_highlight: bool,
    pub default_item_affix_quality: Option<f64>,
    pub show_titlebar_name: bool,
    pub dpi_scale_override_percent: f64,
    pub show_warnings: bool,
    pub slot_only_tooltips: bool,
    pub migrate_eldritch_implicits: bool,
    pub not_supported_tooltip_text: String,
    pub show_public_builds: bool,
    pub show_animations: bool,
    pub show_all_item_affixes: bool,
    pub error_reading_settings: bool,
    pub save_new_mod_cache: bool,
    pub beta_test: Option<bool>,
    pub default_gem_quality: Option<f64>,
    pub default_char_level: Option<f64>,
    pub last_exported_website: Option<String>,
    pub invert_slider_scroll_direction: Option<bool>,
    pub disable_dev_auto_save: Option<bool>,
    pub last_account_name: Option<String>,
    pub last_realm: Option<String>,
    pub last_league: Option<String>,
    pub last_token: Option<String>,
    pub last_refresh_token: Option<String>,
    pub token_expiry: Option<f64>,
    pub unique_db: ItemDb,
    pub rare_db: ItemDb,
    /// main.anchorMain (Main.lua:224)
    pub anchor_main: Option<CtrlId>,
    pub main_bar_height: f64,
    pub portrait_mode: bool,
    pub update_available_shown: bool,
    /// `itemLib.wiki.triggered` (ItemTools.lua:408; the wiki opener is a UI tooltip unit's,
    /// main:OnFrame reads and clears it, Main.lua:476-485).
    pub item_lib_wiki_triggered: bool,
    /// Whether `Classes/Tooltip.lua` was loaded (its module code runs once, at the first
    /// `new("Tooltip")`).
    pub tooltip_class_loaded: bool,
    /// The ToastNotification module's state (Modules/ToastNotification.lua).
    pub toasts: toast::ToastState,
    /// The `rgbColor` global of Data/Global.lua:84 (`updateColorCode("HIGHLIGHT", ..)`; read by
    /// PassiveTreeView.lua:1023); nil until a highlight colour is loaded.
    pub rgb_color: Option<[f64; 3]>,
}

impl Default for MainState {
    fn default() -> MainState {
        MainState {
            input_events: Rc::default(),
            modes: IndexMap::new(),
            mode: None,
            new_mode: None,
            new_mode_args: Vec::new(),
            predefined_build_name: None,
            new_mode_change_to_tree: false,
            hooks: init::default_hooks(),
            shared_item_list: Vec::new(),
            shared_item_set_list: Vec::new(),
            game_accounts: IndexMap::new(),
            user_path: None,
            default_build_path: None,
            build_path: None,
            build_sort_mode: String::new(),
            connection_protocol: 0.0,
            node_power_theme: String::new(),
            color_positive: String::new(),
            color_negative: String::new(),
            color_highlight: String::new(),
            use_compact_values: false,
            edge_search_highlight: false,
            default_item_affix_quality: None,
            show_titlebar_name: false,
            dpi_scale_override_percent: 0.0,
            show_warnings: false,
            slot_only_tooltips: false,
            migrate_eldritch_implicits: false,
            not_supported_tooltip_text: String::new(),
            show_public_builds: false,
            show_animations: false,
            show_all_item_affixes: false,
            error_reading_settings: false,
            save_new_mod_cache: false,
            beta_test: None,
            default_gem_quality: None,
            default_char_level: None,
            last_exported_website: None,
            invert_slider_scroll_direction: None,
            disable_dev_auto_save: None,
            last_account_name: None,
            last_realm: None,
            last_league: None,
            last_token: None,
            last_refresh_token: None,
            token_expiry: None,
            unique_db: ItemDb::default(),
            rare_db: ItemDb::default(),
            anchor_main: None,
            main_bar_height: 0.0,
            portrait_mode: false,
            update_available_shown: false,
            item_lib_wiki_triggered: false,
            tooltip_class_loaded: false,
            toasts: toast::ToastState::default(),
            rgb_color: None,
        }
    }
}

/// `new("Item", raw, rarity, highQuality)` (Classes/Item.lua:94-99, calc:IT-A).
/// CCR-0091: `model.rs` (U15) does not re-export the Item constructor, so this one call goes to
/// pob-calc directly.
pub fn new_item(raw: Option<&str>, rarity: Option<&str>, high_quality: bool) -> model::ItemRef {
    pob_calc::classes::item::new_Item(raw, rarity, high_quality)
}

/// `tostring(v)` of an optional boolean (`"nil"` when unset).
pub fn tostring_opt_bool(v: Option<bool>) -> String {
    v.map_or_else(|| "nil".to_owned(), |b| b.to_string())
}

/// `tostring(v)` of an optional number (`"nil"` when unset).
pub fn tostring_opt_num(v: Option<f64>) -> String {
    v.map_or_else(|| "nil".to_owned(), pob_lua::tostring_num)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mode_arg_tostring() {
        // Main.lua:747-751 `tostring(val)`
        assert_eq!(ModeArg::Num(3.0).tostring(), "3");
        assert_eq!(ModeArg::Num(2.5).tostring(), "2.5");
        assert_eq!(ModeArg::Bool(false).tostring(), "false");
        assert_eq!(ModeArg::Nil.tostring(), "nil");
        assert_eq!(tostring_opt_bool(None), "nil");
        assert_eq!(tostring_opt_num(Some(1_700_003_600.0)), "1700003600");
        assert!(!ModeArg::Bool(false).truthy() && ModeArg::Str(String::new()).truthy());
        assert_eq!(mode_arg(&[ModeArg::Bool(true)], 2), ModeArg::Nil);
    }

    #[test]
    fn strerror_text() {
        let e = std::io::Error::from(std::io::ErrorKind::NotFound);
        assert_eq!(settings::strerror(&e), "No such file or directory");
        let e = std::io::Error::from_raw_os_error(13);
        assert!(!settings::strerror(&e).contains("os error"));
    }
}
