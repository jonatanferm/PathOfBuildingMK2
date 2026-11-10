//! Port of PathOfBuilding v2.67.2 (16de4b82d) src/Modules/Build.lua:771-1617 (conversion/save/save-as popups, spectre library, similar popup).
//! Owner: U20.
//!
//! @pob-file Modules/Build.lua @16de4b82d
//!
//! Popup controls are created in the Lua `controls` table's creation order (the canonical host
//! order, ui doc D7); closures reach siblings through the popup id at call time (U14's rule).
//! The locals that several closures of one popup share (`newFileName`, `newBuildName` of the
//! Save As popup, `destList` of the Spectre library) are `Rc<RefCell<_>>`.
//!
//! The Spectre library's list controls (U31) and the similar-builds list (U52) come from
//! `on_frame::ShellHooks` (CCR-0104 §1).

use std::cell::RefCell;
use std::rc::Rc;

use pob_ui_core::controls::drop_down::SelFunc;
use pob_ui_core::controls::{
    ButtonControl, DropDownControl, DropItem, EditArgs, EditControl, LabelControl,
};
use pob_ui_core::popups::{self, PopupOpts};
use pob_ui_core::prelude::*;

use super::on_frame::shell_hooks;
use super::{init, io};
use crate::app::CxApp;
use crate::build_list::folder_list_control::FolderListControl;
use crate::main_mode::ModeArg;
use crate::model;
use crate::platform::storage;

/// Opens the game version conversion popup
/// lua: Build.lua:1393-1419 `buildMode:OpenConversionPopup()`
/// @pob Modules/Build.lua:1393-1419 h=14ef6d73
pub fn open_conversion_popup(cx: &mut Cx) {
    // L1395 local controls = { }
    // L1396
    let current_version = {
        let gv = &model::data().gameVersions;
        gv.treeVersions
            .get(&gv.latestTreeVersion)
            .map(|v| v.display.to_string())
            .unwrap_or_else(|| lua_error!("attempt to index field '?' (a nil value)"))
    };
    let tip = cx.main.cc.TIP.clone();
    let warning = cx.main.cc.WARNING.clone();
    // L1418
    popups::open_popup(
        cx,
        580.0,
        200.0,
        "Game Version",
        move |cx, p| {
            // L1397-1403
            cx.add(
                p,
                "note",
                LabelControl::new(
                    Anchor::NONE,
                    rect(0, 20, 0, 16),
                    format!("{tip}Info:^7 You are trying to load a build created for a version of Path of Exile that is\nnot supported by us. You will have to convert it to the current game version to load it.\nTo use a build newer than the current supported game version, you may have to update.\nTo use a build older than the current supported game version, we recommend loading it\nin an older version of Path of Building Community instead.\n"),
                ),
            );
            // L1404-1408
            cx.add(
                p,
                "label",
                LabelControl::new(
                    Anchor::NONE,
                    rect(0, 110, 0, 16),
                    format!("{warning}Warning:^7 Converting a build to a different game version may have side effects.\nFor example, if the passive tree has changed, then some passives may be deallocated.\nYou should create a backup copy of the build before proceeding.\n"),
                ),
            );
            // L1409
            cx.add(
                p,
                "convert",
                ButtonControl::new(
                    Anchor::NONE,
                    rect(-40, 170, 120, 20),
                    format!("Convert to {current_version}"),
                    |cx| {
                        // L1410
                        popups::close_popup(cx);
                        // L1411
                        io::shutdown(cx);
                        // L1412
                        let b = cx.build();
                        let db = match &b.db_file_name {
                            Some(s) => ModeArg::Str(s.clone()),
                            None if b.db_file_name_is_false => ModeArg::Bool(false),
                            None => ModeArg::Nil,
                        };
                        let name = b.build_name.clone().map_or(ModeArg::Nil, ModeArg::Str);
                        init::init(cx, &[db, name, ModeArg::Nil, ModeArg::Bool(true)]);
                    },
                ),
            );
            // L1414
            cx.add(
                p,
                "cancel",
                ButtonControl::new(Anchor::NONE, rect(60, 170, 70, 20), "Cancel", |cx| {
                    // L1415
                    popups::close_popup(cx);
                    // L1416
                    io::close_build(cx);
                }),
            );
        },
        PopupOpts {
            enter: Some("convert".into()),
            escape: Some("cancel".into()),
            ..PopupOpts::default()
        },
    );
}

/// lua: Build.lua:1421-1448 `buildMode:OpenSavePopup(mode)`
/// @pob Modules/Build.lua:1421-1448 h=340b3803
pub fn open_save_popup(cx: &mut Cx, mode: &str) {
    // L1422-1426
    let mode_desc = match mode {
        "LIST" => "now?",
        "EXIT" => "before exiting?",
        "UPDATE" => "before updating?",
        _ => lua_error!("attempt to concatenate a nil value"),
    };
    let mode = mode.to_owned();
    // L1427 local controls = { }
    // L1447
    popups::open_popup(
        cx,
        300.0,
        100.0,
        "Save Changes",
        move |cx, p| {
            // L1428
            cx.add(
                p,
                "label",
                LabelControl::new(
                    Anchor::NONE,
                    rect(0, 20, 0, 16),
                    format!(
                        "^7This build has unsaved changes.\nDo you want to save them {mode_desc}"
                    ),
                ),
            );
            // L1429
            let m = mode.clone();
            cx.add(
                p,
                "save",
                ButtonControl::new(Anchor::NONE, rect(-90, 70, 80, 20), "Save", move |cx| {
                    // L1430
                    popups::close_popup(cx);
                    // L1431
                    let me = cx.build().node.id();
                    cx.w_mut::<super::BuildMode>(me).on_frame.action_on_save = Some(m.clone());
                    // L1432
                    io::save_db_file(cx);
                }),
            );
            // L1434
            let m = mode.clone();
            cx.add(
                p,
                "noSave",
                ButtonControl::new(Anchor::NONE, rect(0, 70, 80, 20), "Don't Save", move |cx| {
                    // L1435
                    popups::close_popup(cx);
                    // L1436
                    match m.as_str() {
                        "LIST" => {
                            // L1437
                            io::close_build(cx);
                        }
                        "EXIT" => {
                            // L1439
                            cx.sys.platform.exit(None);
                        }
                        "UPDATE" => {
                            // L1441
                            // nil `updateAvailable` reads "": ApplyUpdate (Launch.lua:325-337) only acts on
                            // "basic" / "normal", so "" does nothing, as nil does
                            let update =
                                cx.app().launch.update_available.clone().unwrap_or_default();
                            crate::launch::apply_update(cx, &update);
                        }
                        _ => {}
                    }
                }),
            );
            // L1444
            cx.add(
                p,
                "close",
                ButtonControl::new(Anchor::NONE, rect(90, 70, 80, 20), "Cancel", |cx| {
                    // L1445
                    popups::close_popup(cx);
                }),
            );
        },
        PopupOpts::default(),
    );
}

/// The Save As popup's sort entries `{ label = .., mode = .. }`.
struct SortModeItem {
    mode: &'static str,
}

/// lua: Build.lua:1450-1519 `buildMode:OpenSaveAsPopup()`
/// @pob Modules/Build.lua:1450-1519 h=c3467b88
pub fn open_save_as_popup(cx: &mut Cx) {
    // L1451 local newFileName, newBuildName
    let new_names: Rc<RefCell<(Option<String>, Option<String>)>> =
        Rc::new(RefCell::new((None, None)));
    // L1452 local controls = { }
    // L1453-1466 local function updateBuildName(): see update_build_name
    let (db_file_name, build_name, db_file_sub_path, save_as_sort_mode) = {
        let b = cx.build();
        (
            b.db_file_name.clone(),
            b.build_name.clone(),
            b.db_file_sub_path.clone(),
            b.save_as_sort_mode.clone(),
        )
    };
    // L1468-1469 (`not self.dbFileName and main.predefinedBuildName or (self.buildName or
    // self.dbFileName):gsub(...)`)
    let predefined = cx.app().main.predefined_build_name.clone();
    let init_text = if let (None, Some(p)) = (&db_file_name, predefined) {
        p
    } else {
        let base = build_name
            .clone()
            .or_else(|| db_file_name.clone())
            // a build without a file has `dbFileName = false` (SetMode("BUILD", false, ..)), so
            // `(nil or false):gsub` indexes a boolean
            .unwrap_or_else(|| lua_error!("attempt to index a boolean value"));
        pob_lua::gsub(&base, "[\\/:%*%?\"<>|%c]", "-", None).0
    };
    // L1518
    let title = if db_file_name.is_some() {
        "Save As"
    } else {
        "Save"
    };
    let nn = new_names.clone();
    popups::open_popup(
        cx,
        470.0,
        555.0,
        title,
        move |cx, p| {
            let p_id = p.id();
            // L1467
            cx.add(
                p,
                "label",
                LabelControl::new(Anchor::NONE, rect(0, 20, 0, 16), "^7Enter new build name:"),
            );
            // L1468-1471
            let nn1 = nn.clone();
            cx.add(
                p,
                "edit",
                EditControl::new(
                    Anchor::NONE,
                    rect(0, 40, 450, 20),
                    EditArgs {
                        init: Some(init_text.clone()),
                        filter: Some("\\/:%*%?\"<>|%c".to_owned()),
                        limit: Some(100),
                        change_func: Some(Rc::new(move |cx: &mut Cx, _buf: &str| {
                            // L1470
                            update_build_name(cx, p_id, &nn1);
                        })),
                        ..EditArgs::default()
                    },
                ),
            );
            // L1472
            cx.add(
                p,
                "folderLabel",
                LabelControl::new(
                    Anchor::nil(TopLeft, TopLeft),
                    rect(10, 70, 0, 16),
                    "^7Folder:",
                ),
            );
            // L1473
            cx.add(
                p,
                "newFolder",
                ButtonControl::new(
                    Anchor::nil(TopLeft, TopLeft),
                    rect(100, 67, 94, 20),
                    "New Folder...",
                    move |cx| {
                        // L1474
                        let folder = cx.child_as::<FolderListControl>(p_id, "folder");
                        let path = format!(
                            "{}{}",
                            storage::build_path(cx),
                            cx.w::<FolderListControl>(folder.id()).sub_path
                        );
                        let folder_id = folder.id();
                        popups::open_new_folder_popup(
                            cx,
                            &path,
                            Some(Rc::new(
                                move |cx: &mut Cx, new_folder_name: Option<String>| {
                                    // L1475
                                    if let Some(name) = new_folder_name {
                                        // L1476
                                        FolderListControl::open_folder(cx, folder_id, &name);
                                    }
                                },
                            )),
                        );
                    },
                ),
            );

            // L1481
            let nn2 = nn.clone();
            let folder = cx.add(
                p,
                "folder",
                FolderListControl::new(
                    Anchor::NONE,
                    rect(0, 115, 450, 400),
                    db_file_sub_path.clone(),
                    Some(Rc::new(move |cx: &mut Cx, _sub_path: &str| {
                        // L1482
                        update_build_name(cx, p_id, &nn2);
                    })),
                ),
            );
            // L1484
            cx.w_mut::<FolderListControl>(folder.id()).sort_mode = Some(save_as_sort_mode.clone());
            // L1485
            FolderListControl::sort_list(cx, folder.id());

            // L1487
            let nn3 = nn.clone();
            let save = cx.add(
                p,
                "save",
                ButtonControl::new(Anchor::NONE, rect(-45, 525, 80, 20), "Save", move |cx| {
                    // L1488
                    popups::close_popup(cx);
                    let (new_file_name, new_build_name) = nn3.borrow().clone();
                    // L1489
                    io::set_db_file_name(cx, new_file_name);
                    // L1490
                    cx.build_mut().build_name = new_build_name;
                    // L1491
                    let sub = cx.w::<FolderListControl>(folder.id()).sub_path.clone();
                    cx.build_mut().db_file_sub_path = Some(sub);
                    // L1492
                    io::save_db_file(cx);
                    // L1493
                    init::set_window_title_with_build_class(cx);
                }),
            );
            // L1495
            cx.add(
                p,
                "close",
                ButtonControl::new(Anchor::NONE, rect(45, 525, 80, 20), "Cancel", |cx| {
                    // L1496
                    popups::close_popup(cx);
                    // L1497
                    let me = cx.build().node.id();
                    cx.w_mut::<super::BuildMode>(me).on_frame.action_on_save = None;
                }),
            );

            // L1500
            if db_file_name.is_some() || build_name.is_some() {
                // L1501
                cx.base_mut(save.id()).enabled = Prop::Val(true);
                // L1502
                update_build_name(cx, p_id, &nn);
            } else {
                // L1504
                cx.base_mut(save.id()).enabled = Prop::Val(false);
            }

            // L1507-1510
            let folder_id = folder.id();
            let sel: SelFunc = Rc::new(move |cx, _index, value| {
                let mode = value
                    .and_then(|v| v.get::<SortModeItem>())
                    .map(|v| v.mode)
                    .unwrap_or_else(|| lua_error!("attempt to index local 'value' (a nil value)"));
                // L1511
                cx.build_mut().save_as_sort_mode = mode.to_owned();
                // L1512
                cx.w_mut::<FolderListControl>(folder_id).sort_mode = Some(mode.to_owned());
                // L1513
                FolderListControl::sort_list(cx, folder_id);
            });
            let sort = cx.add(
                p,
                "buildSortMode",
                DropDownControl::new(
                    Anchor::nil(TopRight, TopRight),
                    rect(-10, 70, 120, 18),
                    vec![
                        DropItem::new("Sort By Name", Rc::new(SortModeItem { mode: "NAME" })),
                        DropItem::new(
                            "Sort By Last Edited",
                            Rc::new(SortModeItem { mode: "EDITED" }),
                        ),
                    ],
                    Some(sel),
                    None,
                    false,
                ),
            );
            // L1515
            cx.w_mut::<DropDownControl>(sort.id()).tt.tooltip_text = Some(Prop::Val(Some(
                "Sort folders by name or date modified.".to_owned(),
            )));
            // L1516
            let mode = cx.build().save_as_sort_mode.clone();
            DropDownControl::sel_by_value(cx, sort.id(), &|it: &DropItem| {
                it.get::<SortModeItem>().is_some_and(|v| v.mode == mode)
            });
        },
        PopupOpts {
            enter: Some("save".into()),
            default: Some("edit".into()),
            escape: Some("close".into()),
            ..PopupOpts::default()
        },
    );
}

/// L1453-1466 `local function updateBuildName()` of OpenSaveAsPopup.
fn update_build_name(
    cx: &mut Cx,
    p: CtrlId,
    new_names: &Rc<RefCell<(Option<String>, Option<String>)>>,
) {
    // L1454
    let buf = cx.w::<EditControl>(child(cx, p, "edit")).buf.clone();
    // L1455
    let folder = child(cx, p, "folder");
    let new_file_name = format!(
        "{}{}{buf}.xml",
        storage::build_path(cx),
        cx.w::<FolderListControl>(folder).sub_path
    );
    // L1456
    *new_names.borrow_mut() = (Some(new_file_name.clone()), Some(buf.clone()));
    let save = child(cx, p, "save");
    // L1457
    cx.base_mut(save).enabled = Prop::Val(false);
    // L1458
    if pob_lua::lmatch(&buf, "%S", None).is_some() {
        // L1459-1464 (`io.open(newFileName, "r")`: the file exists)
        if !storage::file_exists(cx, &new_file_name) {
            // L1463
            cx.base_mut(save).enabled = Prop::Val(true);
        }
    }
}

/// `controls[key]` of a popup (a missing control is the Lua index error).
#[track_caller]
fn child(cx: &Cx, p: CtrlId, key: &'static str) -> CtrlId {
    cx.child(p, key)
        .unwrap_or_else(|| lua_error!("attempt to index field '{key}' (a nil value)"))
}

/// Open the spectre library popup
/// lua: Build.lua:1521-1551 `buildMode:OpenSpectreLibrary()`
/// @pob Modules/Build.lua:1521-1551 h=81b195c0
pub fn open_spectre_library(cx: &mut Cx) {
    let h = shell_hooks();
    // L1523
    let dest_list: Rc<RefCell<Vec<model::Name>>> = Rc::new(RefCell::new(
        cx.build().model.spectreList.get().with(Clone::clone),
    ));
    // L1524
    let data = model::data();
    let mut source_list: Vec<model::Name> = Vec::new();
    // L1525 ORDER: sorted below by name, then id (a total order)
    for id in data.spectres.keys() {
        // L1526
        source_list.push(*id);
    }
    // L1528-1534
    let minion_name = |id: &model::Name| -> String {
        data.minions
            .get(id)
            .map(|m| m.name.to_string())
            .unwrap_or_else(|| lua_error!("attempt to index field '?' (a nil value)"))
    };
    pob_lua::sort(&mut source_list, |a, b| {
        let (na, nb) = (minion_name(a), minion_name(b));
        // L1529
        if na == nb {
            // L1530
            a.as_str() < b.as_str()
        } else {
            // L1532
            na < nb
        }
    });
    // L1535 local controls = { }
    // L1549
    let popup = popups::open_popup(
        cx,
        575.0,
        360.0,
        "Spectre Library",
        move |cx, p| {
            // L1536
            let list = (h.minion_list_control)(cx, p.id(), dest_list.clone());
            // L1537
            (h.minion_search_list_control)(cx, p.id(), source_list.clone(), list);
            // L1538
            let dl = dest_list.clone();
            cx.add(
                p,
                "save",
                ButtonControl::new(Anchor::NONE, rect(-45, 330, 80, 20), "Save", move |cx| {
                    // L1539
                    let list = dl.borrow().clone();
                    cx.build().model.spectreList.set(model::Shared::new(list));
                    // L1540
                    cx.build_mut().mod_flag = true;
                    // L1541
                    cx.build().model.buildFlag.set(true);
                    // L1542
                    popups::close_popup(cx);
                }),
            );
            // L1544
            cx.add(
                p,
                "cancel",
                ButtonControl::new(Anchor::NONE, rect(45, 330, 80, 20), "Cancel", |cx| {
                    // L1545
                    popups::close_popup(cx);
                }),
            );
            // L1547
            cx.add(
                p,
                "noteLine1",
                LabelControl::new(
                    Anchor::new(TopLeft, list, BottomLeft),
                    rect(99, 2, 0, 16),
                    "^7Spectres in your Library must be assigned to an active",
                ),
            );
            // L1548
            cx.add(
                p,
                "noteLine2",
                LabelControl::new(
                    Anchor::new(TopLeft, list, BottomLeft),
                    rect(95, 18, 0, 16),
                    "^7Raise Spectre gem for their buffs and curses to activate",
                ),
            );
        },
        PopupOpts::default(),
    );
    // L1550
    let source = child(cx, popup.id(), "source");
    let search_text = child(cx, source, "searchText");
    cx.select_control(popup.id(), Some(search_text));
}

/// lua: Build.lua:1576-1615 `buildMode:OpenSimilarPopup()` (no caller in PoB 2.67.2)
/// @pob Modules/Build.lua:1576-1615 h=db05ffe6
pub fn open_similar_popup(cx: &mut Cx) {
    let h = shell_hooks();
    // L1577 local controls = { }
    // local width, height = self:GetSize()
    // L1579-1584 buildProviders (the PoB Archives provider, U52's similar_build_list hook)
    // L1585
    let width = 600.0;
    // L1586-1588
    let height = |cx: &Cx| cx.main.screen_h * 0.8;
    // L1589
    let padding = 50.0;
    let h0 = height(cx);
    // L1614
    popups::open_popup(
        cx,
        width,
        h0,
        "Similar Builds",
        move |cx, p| {
            // L1590-1599 (the hook runs L1598's SaveDB after creating the control; Deflate /
            // base64 are U43's, inside the hook)
            (h.similar_build_list)(cx, p.id(), &|cx: &mut Cx| {
                io::save_db(cx, "code").unwrap_or_default()
            });

            // controls.similarBuildList.shown = not controls.similarBuildList:IsShown()

            // L1603
            let close = cx.add(
                p,
                "close",
                ButtonControl::new(
                    Anchor::NONE,
                    rect(0, h0 - (padding + 20.0) / 2.0, 80, 20),
                    "Close",
                    |cx| {
                        // L1604
                        popups::close_popup(cx);
                    },
                ),
            );
            let _ = close;
        },
        PopupOpts {
            // L1606-1613 used in PopupDialog to dynamically size the popup
            resize_func: Some(Rc::new(move |cx: &mut Cx| {
                let hh = height(cx);
                let Some(&popup) = cx.main.popups.first() else {
                    return;
                };
                // L1608
                cx.base_mut(popup.id()).height = Prop::Val(hh);
                // L1609-1611
                cx.base_mut(popup.id()).y = Prop::f(move |cx, _| {
                    pob_lua::math::m_floor((cx.main.screen_h - cx.main.screen_h * 0.8) / 2.0)
                });
                // L1612
                if let Some(close) = cx.child(popup.id(), "close") {
                    cx.base_mut(close).y = Prop::Val(hh - 35.0);
                }
            })),
            ..PopupOpts::default()
        },
    );
}
