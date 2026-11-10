//! Port of PathOfBuilding v2.67.2 (16de4b82d) build storage glue for BuildList mode: the
//! SimpleGraphic and LuaJIT file functions that Modules/BuildList.lua, Modules/BuildListHelpers.lua,
//! Classes/BuildListControl.lua and Classes/FolderListControl.lua call (`io.open`,
//! `NewFileSearch`, `os.rename`, `os.remove`, `RemoveDir`, Common.lua's `copyFile`), over
//! `cx.sys.fs` (ui doc §3 wasm rules: no `std::fs` here). Owner: U18.
//!
//! These are SG/LuaJIT natives and one Common.lua helper, not Lua lines of the unit's files, so
//! they carry no `@pob` markers. `main:MoveFolder`/`CopyFolder` and the non-recursive `RemoveDir`
//! are U16's (`crate::main_mode::folders`); U16's `os.rename`/`copyFile` helpers there are
//! private, so this file has its own (same messages).
//!
//! DIVERGENCE(io-open-dir): `io.open(path, "r")` succeeds on a directory on POSIX hosts (glibc
//! `fopen`); [`file_exists`] reads through `Fs::read`, which fails on a directory, so a directory
//! named like the build file does not count as "exists". PoB only probes `<name>.xml` paths.
//! DIVERGENCE(text-mode): `io.open(.., "r")` is text mode; on Windows the CRT turns CRLF into LF
//! before `read(2048)` counts bytes. [`read_head`] returns the raw first 2048 bytes (what POSIX
//! LuaJIT returns). Only the `<Build ...>` attribute values are read from them.

use pob_sg::FileEntry;
use pob_ui_core::prelude::*;

use crate::main_mode::settings::strerror;

/// `local f = io.open(path, "r") if f then f:close() ... end`: whether the file can be opened.
pub fn file_exists(cx: &Cx, path: &str) -> bool {
    cx.sys.fs.read(path).is_ok()
}

/// What `io.open(path, "r")` + `fileHnd:read(2048)` give: `Err` when the open fails
/// (`fileHnd` nil), `Ok(None)` when the file is empty (`read` returns nil at EOF), else the first
/// 2048 bytes.
pub fn read_head(cx: &Cx, path: &str, n: usize) -> std::io::Result<Option<Vec<u8>>> {
    match cx.sys.fs.read(path) {
        Err(e) => Err(e),
        Ok(b) if b.is_empty() => Ok(None),
        Ok(mut b) => {
            b.truncate(n);
            Ok(Some(b))
        }
    }
}

/// `NewFileSearch(spec, dirOnly)` iterated with `GetFileName`/`GetFileModifiedTime`/`NextFile`:
/// every match in SG's order (empty: the search handle is nil).
pub fn new_file_search(cx: &Cx, spec: &str, dir_only: bool) -> Vec<FileEntry> {
    cx.sys.fs.search(spec, dir_only)
}

/// `NewFileSearch(spec, dirOnly)` in a truth test (`if NewFileSearch(..) then`).
pub fn file_search_any(cx: &Cx, spec: &str, dir_only: bool) -> bool {
    !cx.sys.fs.search(spec, dir_only).is_empty()
}

/// `os.rename(src, dst)`: `true` or `nil, "<src>: <strerror>"` (lib_os.c `luaL_fileresult`).
pub fn os_rename(cx: &mut Cx, src: &str, dst: &str) -> (bool, Option<String>) {
    match cx.sys.fs.rename(src, dst) {
        Ok(()) => (true, None),
        Err(e) => (false, Some(format!("{src}: {}", strerror(&e)))),
    }
}

/// `os.remove(path)`: `true` or `nil, "<path>: <strerror>"`.
pub fn os_remove(cx: &mut Cx, path: &str) -> (bool, Option<String>) {
    match cx.sys.fs.remove(path) {
        Ok(()) => (true, None),
        Err(e) => (false, Some(format!("{path}: {}", strerror(&e)))),
    }
}

/// `RemoveDir(path, true)` (`std::filesystem::remove_all`): `true` or `nil, strerror`.
pub fn remove_dir_recursive(cx: &mut Cx, path: &str) -> (bool, Option<String>) {
    match cx.sys.fs.remove_dir(path, true) {
        Ok(()) => (true, None),
        Err(e) => (false, Some(strerror(&e))),
    }
}

/// `RemoveDir(path)` (not recursive): U16's port.
pub fn remove_dir(cx: &mut Cx, path: &str) -> (bool, Option<String>) {
    crate::main_mode::folders::remove_dir(cx, path)
}

/// `copyFile(srcName, dstName)` (Common.lua:873-886): `true` or `nil, msg`.
/// lua: Modules/Common.lua:873-886
pub fn copy_file(cx: &mut Cx, src_name: &str, dst_name: &str) -> (bool, Option<String>) {
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
            );
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

/// `main.buildPath` in a concatenation (nil is a Lua error).
pub fn build_path(cx: &Cx) -> String {
    use crate::app::CxApp;
    cx.app()
        .main
        .build_path
        .clone()
        .unwrap_or_else(|| lua_error!("attempt to concatenate field 'buildPath' (a nil value)"))
}
