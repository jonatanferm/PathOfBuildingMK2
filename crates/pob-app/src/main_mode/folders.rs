//! Port of PathOfBuilding v2.67.2 (16de4b82d) src/Modules/Main.lua:1574-1659, 1787-1796 (StatColor, folder ops, SetWindowTitleSubtext).
//! Owner: U16.
//!
//! @pob-file Modules/Main.lua @16de4b82d
//!
//! `main:StatColor` (:1574-1582), `main:MoveFolder` (:1584-1623), `main:CopyFolder`
//! (:1625-1658), `main:SetWindowTitleSubtext` (:1787-1793), plus the SimpleGraphic file
//! functions they call (`MakeDir`, `RemoveDir`, `NewFileSearch`, `os.rename`) and Common.lua's
//! `copyFile`, over `cx.sys.fs`.
//! (Main.lua:1794-1796 is the `do` block of `main:WrapString`, U14's `pob_ui_core::wrap`.)
//!
//! DIVERGENCE(fs-errors): SG's `MakeDir`/`RemoveDir` report `strerror` of the C++
//! `std::filesystem` error, and `false` from `create_directory` (the directory exists) as
//! `nil, strerror(0)` ("Success" on Linux, "No error" on Windows; Main.lua:840 tests the latter).
//! `Fs` reports `io::Error`s: an `AlreadyExists` from `make_dir` maps to `strerror(0)` of the
//! host ("No error" on Windows, else "Success"), other errors to their message without the
//! " (os error N)" suffix.

use pob_ui_core::popups;
use pob_ui_core::prelude::*;

use super::settings::strerror;
use crate::app::CxApp;

/// `APP_NAME` (Launch.lua:9).
pub const APP_NAME: &str = "Path of Building";

/// `strerror(0)` of the host C library.
fn strerror_success() -> &'static str {
    if cfg!(windows) {
        "No error"
    } else {
        "Success"
    }
}

/// `MakeDir(path)`: `true` or `nil, msg` (ui_api.cpp `l_MakeDir`, `create_directory`).
pub fn make_dir(cx: &mut Cx, path: &str) -> (bool, Option<String>) {
    match cx.sys.fs.make_dir(path) {
        Ok(()) => (true, None),
        Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => {
            (false, Some(strerror_success().to_owned()))
        }
        Err(e) => (false, Some(strerror(&e))),
    }
}

/// `RemoveDir(path)` (not recursive): `true` or `nil, msg`.
pub fn remove_dir(cx: &mut Cx, path: &str) -> (bool, Option<String>) {
    match cx.sys.fs.remove_dir(path, false) {
        Ok(()) => (true, None),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            (false, Some(strerror_success().to_owned()))
        }
        Err(e) => (false, Some(strerror(&e))),
    }
}

/// `os.rename(src, dst)`: `true` or `nil, "<src>: <strerror>"` (lib_os.c `luaL_fileresult`).
fn os_rename(cx: &mut Cx, src: &str, dst: &str) -> (bool, Option<String>) {
    match cx.sys.fs.rename(src, dst) {
        Ok(()) => (true, None),
        Err(e) => (false, Some(format!("{src}: {}", strerror(&e)))),
    }
}

/// `copyFile(srcName, dstName)` (Common.lua:873-886) over `cx.sys.fs`.
/// lua: Modules/Common.lua:873-886
fn copy_file(cx: &mut Cx, src_name: &str, dst_name: &str) -> (bool, Option<String>) {
    // L874-877
    let data = match cx.sys.fs.read(src_name) {
        Ok(d) => d,
        Err(e) => {
            return (
                false,
                Some(format!(
                    "Couldn't open '{src_name}': {src_name}: {}",
                    strerror(&e)
                )),
            )
        }
    };
    // L878-884
    match cx.sys.fs.write(dst_name, &data) {
        Ok(()) => (true, None),
        Err(e) => (
            false,
            Some(format!(
                "Couldn't create '{dst_name}': {dst_name}: {}",
                strerror(&e)
            )),
        ),
    }
}

/// `msg` in a concatenation (nil is a Lua error).
fn msg_of(msg: Option<String>) -> String {
    msg.unwrap_or_else(|| lua_error!("attempt to concatenate local 'msg' (a nil value)"))
}

/// lua: Main.lua:1574-1582 main:StatColor(stat, base, limit)
/// @pob Modules/Main.lua:1574-1582 h=a3d58c66
pub fn stat_color(cx: &Cx, stat: f64, base: Option<f64>, limit: Option<f64>) -> String {
    // L1575
    if limit.is_some_and(|limit| stat > limit) {
        // L1576
        cx.main.cc.NEGATIVE.clone()
    } else if base.is_some_and(|base| stat != base) {
        // L1578
        cx.main.cc.MAGIC.clone()
    } else {
        // L1580
        "^7".to_owned()
    }
}

/// lua: Main.lua:1584-1623 main:MoveFolder(name, srcPath, dstPath)
/// @pob Modules/Main.lua:1584-1623 h=ff7fa7ef
pub fn move_folder(cx: &mut Cx, name: &str, src_path: &str, dst_path: &str) {
    // L1585-1586 Create destination folder
    let (res, msg) = make_dir(cx, &format!("{dst_path}{name}"));
    // L1587
    if !res {
        // L1588
        popups::open_message_popup(
            cx,
            "Error",
            &format!("Couldn't move '{name}' to '{dst_path}' : {}", msg_of(msg)),
        );
        // L1589
        return;
    }

    // L1592-1593 Move subfolders
    let dirs = cx.sys.fs.search(&format!("{src_path}{name}/*"), true);
    // L1594-1599
    for entry in dirs {
        // L1595
        move_folder(
            cx,
            &entry.name,
            &format!("{src_path}{name}/"),
            &format!("{dst_path}{name}/"),
        );
    }

    // L1601-1602 Move files
    let files = cx.sys.fs.search(&format!("{src_path}{name}/*"), false);
    // L1603
    for entry in files {
        // L1604-1606
        let file_name = &entry.name;
        let src_name = format!("{src_path}{name}/{file_name}");
        let dst_name = format!("{dst_path}{name}/{file_name}");
        // L1607
        let (res, msg) = os_rename(cx, &src_name, &dst_name);
        // L1608
        if !res {
            // L1609
            popups::open_message_popup(
                cx,
                "Error",
                &format!(
                    "Couldn't move '{src_name}' to '{dst_name}': {}",
                    msg_of(msg)
                ),
            );
            // L1610
            return;
        }
    }

    // L1617-1618 Remove source folder
    let (res, msg) = remove_dir(cx, &format!("{src_path}{name}"));
    // L1619
    if !res {
        // L1620
        popups::open_message_popup(
            cx,
            "Error",
            &format!("Couldn't delete '{dst_path}{name}' : {}", msg_of(msg)),
        );
    }
}

/// lua: Main.lua:1625-1658 main:CopyFolder(srcName, dstName)
/// @pob Modules/Main.lua:1625-1658 h=4433bde2
pub fn copy_folder(cx: &mut Cx, src_name: &str, dst_name: &str) {
    // L1626-1627 Create destination folder
    let (res, msg) = make_dir(cx, dst_name);
    // L1628
    if !res {
        // L1629
        popups::open_message_popup(
            cx,
            "Error",
            &format!(
                "Couldn't copy '{src_name}' to '{dst_name}' : {}",
                msg_of(msg)
            ),
        );
        // L1630
        return;
    }

    // L1633-1634 Copy subfolders
    let dirs = cx.sys.fs.search(&format!("{src_name}/*"), true);
    // L1635
    for entry in dirs {
        // L1636-1637
        let file_name = &entry.name;
        copy_folder(
            cx,
            &format!("{src_name}/{file_name}"),
            &format!("{dst_name}/{file_name}"),
        );
    }

    // L1643-1644 Copy files
    let files = cx.sys.fs.search(&format!("{src_name}/*"), false);
    // L1645
    for entry in files {
        // L1646-1648
        let file_name = &entry.name;
        let src_name = format!("{src_name}/{file_name}");
        let dst_name = format!("{dst_name}/{file_name}");
        // L1649
        let (res, msg) = copy_file(cx, &src_name, &dst_name);
        // L1650
        if !res {
            // L1651
            popups::open_message_popup(
                cx,
                "Error",
                &format!(
                    "Couldn't copy '{src_name}' to '{dst_name}': {}",
                    msg_of(msg)
                ),
            );
            // L1652
            return;
        }
    }
}

/// lua: Main.lua:1787-1793 main:SetWindowTitleSubtext(subtext)
/// @pob Modules/Main.lua:1787-1793 h=615d47fc
pub fn set_window_title_subtext(cx: &mut Cx, subtext: Option<&str>) {
    // L1788
    match subtext {
        Some(subtext) if cx.app().main.show_titlebar_name => {
            // L1791
            cx.sys
                .platform
                .set_window_title(&format!("{subtext} - {APP_NAME}"));
        }
        _ => {
            // L1789
            cx.sys.platform.set_window_title(APP_NAME);
        }
    }
}
