//! Port of PathOfBuilding v2.67.2 (16de4b82d) src/Modules/Build.lua:771-1617 (CanExit, Shutdown, CloseBuild, Load, Save, ResetModFlags), 2222-2335 (LoadDB/SaveDB; model halves are calc:BM-A's).
//! Owner: U20.
//!
//! @pob-file Modules/Build.lua @16de4b82d
//!
//! `buildMode:Load` (976-1015), `Save` (1017-1098), `LoadDB` (2222-2260), `LoadDBFile`
//! (2262-2275) and `SaveDB` (2277-2301) have no UI statement: they are calc:BM-A's
//! (`pob_calc::model::build_xml`, ui doc §10.2), which carries their `@pob` markers. The UI calls
//! them through [`save_db`] and `init.rs` (`Init_model` runs LoadDB).
//!
//! `buildMode:SaveDBFile` (2304-2333) is a UI function around the model's `SaveDB`: BM-A's
//! `Build::SaveDBFile` (which carries the `@pob` marker and ports the model statements, with the
//! UI statements left as `// UI:` comments) writes with `std::fs` and reports errors on stderr.
//! [`save_db_file`] runs the same statements with the UI ones in place: the Save As popup, the
//! file written through `cx.sys.fs` (the platform's storage, wasm included), the error popup,
//! `actionOnSave` and ResetModFlags.
//!
//! `self.dbFileName` has two homes: the UI's `BuildState::db_file_name` and BM-A's side table
//! (`build_xml::dbFileName`, which `SaveDBFile`/`LoadDBFile` read). [`set_db_file_name`] writes
//! both.

use pob_ui_core::popups;
use pob_ui_core::prelude::*;

use super::on_frame::{shell_hooks, TabFlag};
use super::BuildMode;
use crate::app::CxApp;
use crate::main_mode::{self, ModeArg};
use crate::model::{self, build_xml};

/// `self.dbFileName = name` (both homes, see the module doc).
pub fn set_db_file_name(cx: &mut Cx, name: Option<String>) {
    let model = cx.build().model.clone();
    build_xml::set_dbFileName(&model, name.as_deref());
    let b = cx.build_mut();
    b.db_file_name = name;
    b.db_file_name_is_false = false;
}

/// lua: Build.lua:943-949 `buildMode:CanExit(mode)`
/// @pob Modules/Build.lua:943-949 h=95cfdc5b
pub fn can_exit(cx: &mut Cx, mode: &str) -> Option<bool> {
    // L944
    if !cx.build().unsaved {
        // L945
        return Some(true);
    }
    // L947
    super::popups::open_save_popup(cx, mode);
    // L948
    Some(false)
}

/// lua: Build.lua:951-965 `buildMode:Shutdown()`
/// @pob Modules/Build.lua:951-965 h=bfdcf05c
pub fn shutdown(cx: &mut Cx) {
    let Some(me) = cx.app().build.as_ref().map(|b| b.node.id()) else {
        return;
    };
    // L952
    let target_version = !cx.w::<BuildMode>(me).init.target_version_nil;
    if cx.app().launch.dev_mode
        && !cx.app().main.disable_dev_auto_save.unwrap_or(false)
        && target_version
        && cx.build().abort_save != Some(true)
    {
        // L953
        if cx.build().db_file_name.is_some() {
            // L954
            save_db_file(cx);
        } else if cx.build().unsaved {
            // L955
            // L956
            let build_path = cx.app().main.build_path.clone().unwrap_or_else(|| {
                lua_error!("attempt to concatenate field 'buildPath' (a nil value)")
            });
            set_db_file_name(cx, Some(format!("{build_path}~~temp~~.xml")));
            // L957
            cx.build_mut().build_name = Some("~~temp~~".to_owned());
            // L958
            cx.build_mut().db_file_sub_path = Some(String::new());
            // L959
            save_db_file(cx);
        }
    }
    // L962
    cx.build_mut().abort_save = None;

    // L964 self.savers = nil (calc:BM-A's SAVERS are a constant list)
}

/// lua: Build.lua:971-974 `buildMode:CloseBuild()`
/// @pob Modules/Build.lua:971-974 h=eb3f4063
pub fn close_build(cx: &mut Cx) {
    // L972
    main_mode::folders::set_window_title_subtext(cx, None);
    // L973 (`self.dbFileName and self.buildName`: the build name when there is a file name,
    // else the file name, nil or false)
    let b = cx.build();
    let a1 = if b.db_file_name.is_some() {
        b.build_name.clone().map_or(ModeArg::Nil, ModeArg::Str)
    } else if b.db_file_name_is_false {
        ModeArg::Bool(false)
    } else {
        ModeArg::Nil
    };
    let a2 = b
        .db_file_sub_path
        .clone()
        .map_or(ModeArg::Nil, ModeArg::Str);
    main_mode::on_frame::set_mode(cx, "LIST", vec![a1, a2]);
}

/// lua: Build.lua:1100-1111 `buildMode:ResetModFlags()`
/// @pob Modules/Build.lua:1100-1111 h=8dad7e55
pub fn reset_mod_flags(cx: &mut Cx) {
    let h = shell_hooks();
    // L1101
    cx.build_mut().mod_flag = false;
    // L1102-1110
    for flag in [
        TabFlag::Notes,
        TabFlag::Party,
        TabFlag::Config,
        TabFlag::Tree,
        TabFlag::TreeSearch,
        TabFlag::Spec,
        TabFlag::Skills,
        TabFlag::Items,
        TabFlag::Calcs,
    ] {
        (h.set_tab_flag)(cx, flag, false);
    }
}

/// `self:SaveDB(fileName)` (calc:BM-A `Build::SaveDB`, Build.lua:2277-2301): the build XML
/// text. `None` when no build is open. The UI-only savers' sections (`Notes`, `TreeView`,
/// `Import`) come from `ShellHooks::save_ui_section` once their owners install it; until then
/// BM-A's SaveDB writes the loaded sections back (CCR-0080 §8).
/// `&mut Cx`: a UI saver's `Save` may change its tab (CCR-0106 §1).
pub fn save_db(cx: &mut Cx, file_name: &str) -> Option<String> {
    let m = cx.app().build.as_ref()?.model.clone();
    let text = m.SaveDB(file_name)?;
    let h = shell_hooks();
    let ui: Vec<(&str, model::XmlNode)> = ["Notes", "TreeView", "Import"]
        .into_iter()
        .filter_map(|elem| (h.save_ui_section)(cx, elem).map(|n| (elem, n)))
        .collect();
    if ui.is_empty() {
        return Some(text);
    }
    // L2288-2292 for the UI savers: replace their sections in BM-A's document
    let mut root = model::xml::ParseXML_root(&text)
        .unwrap_or_else(|e| lua_error!("Error saving '{file_name}': {e}"));
    for child in &mut root.children {
        if let model::XmlChild::Node(pob) = child {
            for c in &mut pob.children {
                if let model::XmlChild::Node(section) = c {
                    if let Some((_, n)) = ui.iter().find(|(e, _)| **e == *section.elem) {
                        section.attrib = n.attrib.clone();
                        section.children = n.children.clone();
                    }
                }
            }
        }
    }
    // CCR-0106 §1: compose the `PathOfBuilding` element (ParseXML_root's result is the
    // parser's nameless top node, whose child it is)
    let pob = root
        .children
        .iter()
        .find_map(|c| match c {
            model::XmlChild::Node(n) => Some(n),
            model::XmlChild::Text(_) => None,
        })
        .unwrap_or_else(|| lua_error!("Error saving '{file_name}': no root element"));
    Some(model::xml::ComposeXML(pob))
}

/// The `.pobscript` step `snapshot` (T4): `main.modes.BUILD:SaveDB("snapshot")` while the
/// current mode is BUILD.
pub fn snapshot(cx: &mut Cx) -> Option<String> {
    if cx.app().main.mode.as_deref() != Some("BUILD") {
        return None;
    }
    save_db(cx, "snapshot")
}

/// lua: Build.lua:2304-2333 `buildMode:SaveDBFile()` (model part: calc:BM-A `Build::SaveDB`;
/// the `@pob` marker is BM-A's `Build::SaveDBFile`, see the module doc). → true on failure.
pub fn save_db_file(cx: &mut Cx) -> bool {
    // L2305
    let Some(db_file_name) = cx.build().db_file_name.clone() else {
        // L2306
        super::popups::open_save_as_popup(cx);
        // L2307
        return false;
    };
    // L2309
    let Some(xml_text) = save_db(cx, &db_file_name) else {
        // L2311
        return true;
    };
    // L2313-2318
    if cx.sys.fs.write(&db_file_name, xml_text.as_bytes()).is_err() {
        // L2315
        popups::open_message_popup(
            cx,
            "Error",
            &format!(
                "Couldn't save the build file:\n{db_file_name}\nMake sure the save folder exists and is writable."
            ),
        );
        // L2316
        return true;
    }
    let me = cx.build().node.id();
    // L2320
    let action = cx.w::<BuildMode>(me).on_frame.action_on_save.clone();
    // L2321
    cx.w_mut::<BuildMode>(me).on_frame.action_on_save = None;

    // Reset all modFlags
    // L2324
    reset_mod_flags(cx);

    // L2326
    match action.as_deref() {
        Some("LIST") => {
            // L2327
            close_build(cx);
        }
        Some("EXIT") => {
            // L2329
            cx.sys.platform.exit(None);
        }
        Some("UPDATE") => {
            // L2331
            // nil `updateAvailable` reads "": ApplyUpdate (Launch.lua:325-337) only acts on
            // "basic" / "normal", so "" does nothing, as nil does
            let update = cx.app().launch.update_available.clone().unwrap_or_default();
            crate::launch::apply_update(cx, &update);
        }
        _ => {}
    }
    false
}
