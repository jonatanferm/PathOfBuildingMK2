//! The SimpleGraphic globals as Rust functions over `Rc<RefCell<Sg>>` / `Rc<RefCell<Sys>>` (ui doc
//! §11.2 step 4), with `ui_api.cpp`'s exact argument-count dispatch. Owner: U8.
//!
//! Covered: SetDrawLayer, GetDrawLayer, SetViewport, SetDrawColor, GetDrawColor,
//! DrawImage/DrawImageQuad, DrawString, DrawStringWidth, DrawStringCursorIndex, StripEscapes,
//! NewImageHandle (Load/Unload/IsValid/IsLoading/ImageSize/SetLoadingPriority), GetScreenSize,
//! GetScreenScale, Get/SetDPIScaleOverridePercent, GetCursorPos, IsKeyDown, GetTime, Copy, Paste,
//! SetWindowTitle, OpenURL, TakeScreenshot, ConPrintf. Also bound, because PoB's boot and the
//! fixtures need real values where `_SimpleGraphic.def.lua` has headless dummies: GetScriptPath,
//! GetRuntimePath, GetUserPath, GetWorkDir, MakeDir, RemoveDir, Restart, Exit, LaunchSubScript,
//! IsSubScriptRunning, AbortSubScript (patch 8: scripted network fixtures), and NewFileSearch
//! (SG's semantics; pob-oracle's native ignores `findDirectories`, CCR-0028 item 4;
//! `GetFileModifiedTime` follows U7's headless rule, [`Mtimes`], CCR-0028 item 10).
//!
//! Source: SimpleGraphic `engine/ui_api.cpp` at the pinned `3b1a346` (CCR-0021): `l_SetDrawLayer`
//! 759-781, `l_GetDrawLayer` 783-788, `l_SetViewport` 790-812, `l_SetDrawColor` 826-874,
//! `l_GetDrawColor` 876-887, `l_DrawImage` 889-1007, `l_DrawImageQuad` 1009-1130, `l_DrawString`
//! 1132-1177, `l_DrawStringWidth` 1179-1204, `l_DrawStringCursorIndex` 1206-1237,
//! `l_SetDPIScaleOverridePercent` 1239-1249, `l_GetDPIScaleOverridePercent` 1251-1257,
//! `l_StripEscapes` 1259-1281, `l_NewImageHandle` + `l_imgHandle*` 366-482, `l_GetScreenSize`
//! 722-728, `l_GetScreenScale` 730-735, `l_SetWindowTitle` 1523-1532, `l_GetCursorPos` 1534-1541,
//! `l_IsKeyDown` 1567-1580, `l_Copy` 1582-1590, `l_Paste` 1592-1604, `l_GetTime` 1698-1703,
//! `l_GetScriptPath`/`l_GetRuntimePath`/`l_GetUserPath` 1705-1766, `l_MakeDir` 1768-1786,
//! `l_RemoveDir` 1788-1814, `l_GetWorkDir` 1831-1836, `l_LaunchSubScript` 1838-1873,
//! `l_AbortSubScript` 1875-1887, `l_IsSubScriptRunning` 1889-1899, `l_ConPrintf` 1972-1984,
//! `l_OpenURL` 2104-2115, `l_Restart` 2126-2131, `l_Exit` 2133-2147, `l_TakeScreenshot` 2149-2154.
//!
//! Argument checks follow SG's order and messages: SetDrawLayer, SetViewport, SetDrawColor,
//! DrawImage, DrawImageQuad and DrawString check `renderEnable` ("X() called outside of OnFrame",
//! [`Bridge::render_enable`], set by the host around the sub-script pass and `OnFrame`) before
//! their arguments, and DrawImage/DrawImageQuad check "X(): image handle has no image loaded"
//! after the argument-count switch, as SG does. DIVERGENCE: the stack index / mask arguments of
//! DrawImage (6, 7, 10, 11 args) and DrawImageQuad (10, 11, 18, 19 args) are validated as SG does,
//! but `pob_sg::Sg` has no stacked textures, so any value other than stack 1 without a mask is a
//! Lua error here (PoB 2.67.2 never passes them).
//!
//! The functions are `'static + Send` (mlua's `send` feature is on workspace-wide) while `Sg`/`Sys`
//! are not, so they reach their state through a thread-local registry keyed by an id stored in
//! the VM's app data. A `lua_error!` raised inside `Sg` (SG's `luaL_error`) becomes a Lua error.
//! Every recorded call first sets the trace `src` from the calling Lua function (`debug.getinfo(2,
//! "Sl")` as seen from a Lua wrapper; level 1 of `lua_getstack` from this C function), ui doc
//! §11.2 step 5, e.g. `"Classes/ButtonControl.lua:54"`.
//!
//! Side effects (`SideEffect`, T3): `pob_sg` does not record them yet (CCR-0028 item 2), so the
//! bridge records `ImageLoad`, `WindowTitle`, `Copy`, `OpenUrl`, `Screenshot` and `Exit` itself,
//! but only when `Sg` did not already record that effect for the call (no double counting once
//! the CCR lands). `FileWrite` is not recorded on either side yet (CCR-0028 item 3).

use std::any::Any;
use std::cell::{Cell, RefCell};
use std::collections::{HashMap, VecDeque};
use std::path::{Component, Path, PathBuf};
use std::rc::Rc;
use std::sync::atomic::{AtomicU64, Ordering};

use mlua::{AnyUserData, LightUserData, Lua, MultiValue, UserData, UserDataMethods, Value};
use pob_sg::trace::{ImgRef, SideEffect};
use pob_sg::{Align, Font, ImageFlags, ImageHandle, Key, Sg, Sys};

/// A scripted `launch:DownloadPage` response (patch 8). `body: None` is a sub-script error
/// (`launch:OnSubError(id, error)`; U7's `HttpResponse::body == None`, CCR-0028 item 10).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct NetResponse {
    pub body: Option<Vec<u8>>,
    pub error: Option<String>,
    pub headers: String,
}

/// The `fixture net=FILE.json` responses, in U7's format and with U7's matching rule (CCR-0026,
/// `pob_host::headless` on `port/U7`; CCR-0028 item 10): a JSON array of `{"url": .., "body":
/// "text" | null, "body_file": "path", "error": "msg", "headers": ".."}` (`body_file` relative to
/// the JSON file, `body: null` a sub-script error, no `body` an empty one). A request takes the
/// first unused fixture for its URL, else reuses the last one for it; an unknown URL gets an empty
/// body and `error = "No net fixture for <url>"`.
#[derive(Clone, Debug, Default)]
pub struct NetFixtures {
    fixtures: Vec<(String, NetResponse, bool)>,
}

impl NetFixtures {
    /// Parse a fixture file.
    pub fn load(path: &Path) -> Result<NetFixtures, String> {
        let text = std::fs::read_to_string(path).map_err(|e| format!("{}: {e}", path.display()))?;
        let v: serde_json::Value =
            serde_json::from_str(&text).map_err(|e| format!("{}: {e}", path.display()))?;
        let arr = v
            .as_array()
            .ok_or_else(|| format!("{}: expected a JSON array", path.display()))?;
        let dir = path.parent().unwrap_or_else(|| Path::new("."));
        let mut fixtures = Vec::new();
        for (i, f) in arr.iter().enumerate() {
            let url = f
                .get("url")
                .and_then(|u| u.as_str())
                .ok_or_else(|| format!("{}: entry {i} has no url", path.display()))?;
            let body = if let Some(bf) = f.get("body_file").and_then(|b| b.as_str()) {
                Some(std::fs::read(dir.join(bf)).map_err(|e| format!("{bf}: {e}"))?)
            } else {
                match f.get("body") {
                    Some(serde_json::Value::Null) => None,
                    Some(b) => Some(b.as_str().unwrap_or_default().as_bytes().to_vec()),
                    None => Some(Vec::new()),
                }
            };
            let resp = NetResponse {
                body,
                error: f.get("error").and_then(|e| e.as_str()).map(str::to_owned),
                headers: f
                    .get("headers")
                    .and_then(|h| h.as_str())
                    .unwrap_or_default()
                    .to_owned(),
            };
            fixtures.push((url.to_owned(), resp, false));
        }
        Ok(NetFixtures { fixtures })
    }

    /// The response to a request for `url` (marks the fixture used).
    pub fn take(&mut self, url: &str) -> NetResponse {
        let idx = self
            .fixtures
            .iter()
            .position(|(u, _, used)| !used && u == url)
            .or_else(|| self.fixtures.iter().rposition(|(u, _, _)| u == url));
        match idx {
            Some(i) => {
                self.fixtures[i].2 = true;
                self.fixtures[i].1.clone()
            }
            None => NetResponse {
                body: Some(Vec::new()),
                error: Some(format!("No net fixture for {url}")),
                headers: String::new(),
            },
        }
    }
}

/// Lexically normalised path (`.` dropped, `..` pops): the key of the write-time maps.
pub fn normalize_path(p: &Path) -> PathBuf {
    let mut out = PathBuf::new();
    for c in p.components() {
        match c {
            Component::CurDir => {}
            Component::ParentDir => {
                out.pop();
            }
            c => out.push(c.as_os_str()),
        }
    }
    out
}

/// `GetFileModifiedTime` of a file written during a run at virtual clock `clock_ms`: MSVC
/// `file_clock` ticks (100 ns since 1601) of 2020-09-13T12:26:40Z plus the clock, as U7's
/// headless `Fs` reports it; files not written during the run report 0 (CCR-0028 item 10).
pub fn virtual_write_ticks(clock_ms: i64) -> i64 {
    const UNIX_TO_1601: i64 = 116_444_736_000_000_000;
    UNIX_TO_1601 + 1_600_000_000 * 10_000_000 + clock_ms * 10_000
}

/// How a search reports `GetFileModifiedTime`.
#[derive(Clone, Copy)]
pub enum Mtimes<'a> {
    /// U7's headless rule: [`virtual_write_ticks`] of the write time for files in the map
    /// (normalised absolute paths), 0 for every other file.
    Virtual(&'a HashMap<PathBuf, i64>),
    /// The file system's modification time (live sessions).
    Disk,
}

/// Per-VM state of the bridge.
pub struct Bridge {
    pub sg: Rc<RefCell<Sg>>,
    pub sys: Rc<RefCell<Sys>>,
    /// `"@" + <abs src root>` with a trailing `/`: stripped from chunk names for trace `src`.
    pub chunk_root: RefCell<String>,
    /// Image handles by userdata index.
    handles: RefCell<Vec<ImageHandle>>,
    /// Scripted network fixtures (patch 8).
    pub net: RefCell<NetFixtures>,
    /// Sub-script slots (`ui->subScriptList`): `Some(response)` while the slot's script "runs"
    /// (until the next frame's sub-script pass, `deliver_subscripts`).
    subs: RefCell<Vec<Option<NetResponse>>>,
    /// Slots finished in launch order (SG runs the slots in index order; see `deliver_subscripts`).
    pub finished_subs: RefCell<VecDeque<usize>>,
    /// Extra per-VM objects other oracle modules register (markers).
    pub extra: RefCell<Vec<Rc<dyn Any>>>,
    /// SG's `renderEnable`: set by the host around the sub-script pass and `OnFrame`
    /// (ui_main.cpp:401-421, [`crate::runner::LuaApp`]); the draw-state and draw functions check
    /// it first, as SG does (ui_api.cpp:763-1136).
    pub render_enable: Cell<bool>,
    /// Files PoB opened for writing (`io.open` with "w"/"a", noted by `lua/determinism.lua`
    /// through `__oracleNoteWrite`) with the virtual clock at that moment.
    pub writes: RefCell<HashMap<PathBuf, i64>>,
    /// `GetFileModifiedTime` follows U7's headless rule ([`Mtimes::Virtual`]); false in a live
    /// session (`pob-lua-gui`), which reports the real modification time.
    pub virtual_mtimes: Cell<bool>,
}

/// `ui->LAssert(L, ui->renderEnable, "X() called outside of OnFrame")`.
fn check_render(b: &Bridge, f: &str) -> mlua::Result<()> {
    assert(b.render_enable.get(), || {
        format!("{f}() called outside of OnFrame")
    })
}

#[derive(Clone, Copy)]
struct BridgeId(u64);

thread_local! {
    static BRIDGES: RefCell<HashMap<u64, Rc<Bridge>>> = RefCell::new(HashMap::new());
}
static NEXT_ID: AtomicU64 = AtomicU64::new(1);

/// The bridge of `lua` (installed by [`install`]).
pub fn bridge(lua: &Lua) -> mlua::Result<Rc<Bridge>> {
    let id = lua
        .app_data_ref::<BridgeId>()
        .map(|b| b.0)
        .ok_or_else(|| mlua::Error::runtime("pob-ui-oracle: SG bridge not installed"))?;
    BRIDGES
        .with(|m| m.borrow().get(&id).cloned())
        .ok_or_else(|| mlua::Error::runtime("pob-ui-oracle: SG bridge used on another thread"))
}

/// Drop the registry entry of `lua` (LuaPob's Drop).
pub fn uninstall(lua: &Lua) {
    if let Some(id) = lua.remove_app_data::<BridgeId>() {
        BRIDGES.with(|m| m.borrow_mut().remove(&id.0));
    }
}

/// SG's `luaL_error` from a `lua_error!` panic inside `Sg`/`Sys`.
fn sg_call<R>(f: impl FnOnce() -> R) -> mlua::Result<R> {
    pob_lua::error::catch(f).map_err(|e| mlua::Error::RuntimeError(e.0))
}

fn rt(msg: impl Into<String>) -> mlua::Error {
    mlua::Error::RuntimeError(msg.into())
}

/// `ui->LAssert(L, cond, msg)`.
fn assert(cond: bool, msg: impl FnOnce() -> String) -> mlua::Result<()> {
    if cond {
        Ok(())
    } else {
        Err(rt(msg()))
    }
}

/// `lua_isnumber` + `lua_tonumber`: numbers, and strings `lj_strscan` converts.
fn to_num(v: &Value) -> Option<f64> {
    match v {
        Value::Integer(i) => Some(*i as f64),
        Value::Number(n) => Some(*n),
        Value::String(s) => pob_lua::format::tonumber_bytes(&s.as_bytes()),
        _ => None,
    }
}

/// `luaL_typename`.
fn type_name(v: Option<&Value>) -> &'static str {
    match v {
        None | Some(Value::Nil) => "nil",
        Some(Value::Boolean(_)) => "boolean",
        Some(Value::Integer(_) | Value::Number(_)) => "number",
        Some(Value::String(_)) => "string",
        Some(Value::Table(_)) => "table",
        Some(Value::Function(_)) => "function",
        Some(Value::Thread(_)) => "thread",
        Some(_) => "userdata",
    }
}

fn arg(args: &[Value], i: usize) -> Option<&Value> {
    args.get(i - 1)
}

fn is_nil(args: &[Value], i: usize) -> bool {
    matches!(arg(args, i), None | Some(Value::Nil))
}

/// `lua_isnumber(L, i)`.
fn is_number(args: &[Value], i: usize) -> bool {
    arg(args, i).and_then(to_num).is_some()
}

/// `lua_isstring(L, i)`: strings and numbers.
fn is_string(args: &[Value], i: usize) -> bool {
    matches!(
        arg(args, i),
        Some(Value::String(_) | Value::Integer(_) | Value::Number(_))
    )
}

/// `lua_tonumber(L, i)` after an `is_number` check.
fn num(args: &[Value], i: usize) -> f64 {
    arg(args, i).and_then(to_num).unwrap_or(0.0)
}

/// `ui->LAssert(L, lua_isnumber(L, i), "<f>() argument %d: expected number, got %s", ...)`, then
/// `lua_tonumber`.
fn arg_num(args: &[Value], i: usize, f: &str) -> mlua::Result<f64> {
    assert(is_number(args, i), || {
        format!(
            "{f}() argument {i}: expected number, got {}",
            type_name(arg(args, i))
        )
    })?;
    Ok(num(args, i))
}

/// `lua_tostring(L, i)` after an `is_string` check, read as a C string (SG stops at the first
/// NUL, CCR-0021 item 9). Non-UTF-8 bytes become U+FFFD (`pob_sg` takes `&str`).
fn tostr(lua: &Lua, args: &[Value], i: usize) -> mlua::Result<String> {
    let v = arg(args, i).cloned().unwrap_or(Value::Nil);
    let s = lua.coerce_string(v)?.ok_or_else(|| rt("expected string"))?;
    Ok(c_str(&s.to_string_lossy()).to_owned())
}

/// `ui->LAssert(L, lua_isstring(L, i), "<f>() argument %d: expected string, got %s", ...)`, then
/// [`tostr`].
fn arg_str(lua: &Lua, args: &[Value], i: usize, f: &str) -> mlua::Result<String> {
    assert(is_string(args, i), || {
        format!(
            "{f}() argument {i}: expected string, got {}",
            type_name(arg(args, i))
        )
    })?;
    tostr(lua, args, i)
}

/// SG reads string arguments up to the first NUL (CCR-0021 item 9).
fn c_str(s: &str) -> &str {
    s.split('\0').next().unwrap_or("")
}

/// `luaL_checkoption`'s message for a non-string argument (`luaL_optstring` -> `luaL_typerror`).
fn option_type_error(i: usize, f: &str, v: &Value) -> mlua::Error {
    rt(format!(
        "bad argument #{i} to '{f}' (string expected, got {})",
        type_name(Some(v))
    ))
}

/// `luaL_checkoption(L, i, "LEFT", alignMap)`.
fn arg_align(lua: &Lua, args: &[Value], i: usize, f: &str) -> mlua::Result<Align> {
    let name = match arg(args, i) {
        None | Some(Value::Nil) => None,
        Some(Value::String(_) | Value::Integer(_) | Value::Number(_)) => Some(tostr(lua, args, i)?),
        Some(v) => return Err(option_type_error(i, f, v)),
    };
    Align::from_lua(name.as_deref()).ok_or_else(|| {
        rt(format!(
            "bad argument #{i} to '{f}' (invalid option '{}')",
            name.unwrap_or_default()
        ))
    })
}

/// `luaL_checkoption(L, i, "FIXED", fontMap)`.
fn arg_font(lua: &Lua, args: &[Value], i: usize, f: &str) -> mlua::Result<Font> {
    let name = match arg(args, i) {
        None | Some(Value::Nil) => return Ok(Font::Fixed),
        Some(Value::String(_) | Value::Integer(_) | Value::Number(_)) => tostr(lua, args, i)?,
        Some(v) => return Err(option_type_error(i, f, v)),
    };
    Font::from_lua(&name).ok_or_else(|| {
        rt(format!(
            "bad argument #{i} to '{f}' (invalid option '{name}')"
        ))
    })
}

/// `(int)lua_tointegerx(L, k, &isInt)` for the stack/mask arguments: LuaJIT converts any number
/// (and numeric string) by truncation and reports success.
fn to_int(v: Option<&Value>) -> Option<i32> {
    v.and_then(to_num).map(|n| n as i32)
}

/// Set the trace `src` of the next recorded call from the calling Lua function.
fn set_src(lua: &Lua, b: &Bridge) {
    if b.sg.borrow().trace.is_none() {
        return;
    }
    let src = lua.inspect_stack(1).map(|d| {
        let line = d.curr_line();
        let s = d.source();
        let name = s.source.as_deref().unwrap_or("?");
        let root = b.chunk_root.borrow();
        let rel = match name.strip_prefix(root.as_str()) {
            Some(rel) => rel.to_owned(),
            // The oracle's own Lua (galleries): "pob-ui-oracle/lua/gallery/<name>.lua".
            None => match name.strip_prefix(concat!("@", env!("CARGO_MANIFEST_DIR"), "/")) {
                Some(rel) => format!("pob-ui-oracle/{rel}"),
                None => name.trim_start_matches('@').to_owned(),
            },
        };
        format!("{rel}:{line}")
    });
    b.sg.borrow_mut().set_trace_src(src);
}

/// The number of effects recorded in the current frame (for "record unless Sg did").
fn effect_count(b: &Bridge) -> usize {
    b.sg.borrow()
        .trace
        .as_ref()
        .map_or(0, |t| t.cur.effects.len())
}

/// Record `e` unless `Sg` recorded an effect since `before` (CCR-0028 item 2).
fn effect_unless_recorded(b: &Bridge, before: usize, e: SideEffect) {
    if let Some(t) = &mut b.sg.borrow_mut().trace {
        if t.cur.effects.len() == before {
            t.effect(e);
        }
    }
}

/// An `imgHandle` userdata (`uiimghandlemeta`): an index into [`Bridge`]'s handle list.
#[derive(Clone, Copy)]
pub struct ImgUd(usize);

impl ImgUd {
    fn handle(self, b: &Bridge) -> ImageHandle {
        b.handles.borrow()[self.0].clone()
    }
}

impl UserData for ImgUd {
    fn add_methods<M: UserDataMethods<Self>>(m: &mut M) {
        // imgHandle:Load(fileName[, flag1[, flag2...]]) (ui_api.cpp:394-433)
        m.add_method("Load", |lua, this, args: MultiValue| {
            let b = bridge(lua)?;
            let args: Vec<Value> = args.into_iter().collect();
            assert(!args.is_empty(), || {
                "Usage: imgHandle:Load(fileName[, flag1[, flag2...]])".into()
            })?;
            let name = arg_str(lua, &args, 1, "imgHandle:Load")?;
            let mut flags = Vec::new();
            for f in 2..=args.len() {
                // `if (!lua_isstring(L, f)) continue;`: nil (PassiveTree.lua:931 forwards `...`)
                // and other non-strings are skipped.
                if !is_string(&args, f) {
                    continue;
                }
                flags.push(tostr(lua, &args, f)?);
            }
            let refs: Vec<&str> = flags.iter().map(String::as_str).collect();
            let flags = ImageFlags::from_lua(&refs).map_err(rt)?;
            let h = this.handle(&b);
            let before = effect_count(&b);
            sg_call(|| h.load(&mut b.sg.borrow_mut(), &name, flags))?;
            effect_unless_recorded(
                &b,
                before,
                SideEffect::ImageLoad(ImgRef { path: name, flags }),
            );
            Ok(())
        });
        m.add_method("Unload", |lua, this, ()| {
            let b = bridge(lua)?;
            this.handle(&b).unload();
            Ok(())
        });
        m.add_method("IsValid", |lua, this, ()| {
            let b = bridge(lua)?;
            Ok(this.handle(&b).is_valid())
        });
        // GetImgHandle(..., loaded = true): an unloaded handle is a Lua error (in pob_sg).
        m.add_method("IsLoading", |lua, this, ()| {
            let b = bridge(lua)?;
            let h = this.handle(&b);
            sg_call(|| h.is_loading(&mut b.sg.borrow_mut()))
        });
        m.add_method("ImageSize", |lua, this, ()| {
            let b = bridge(lua)?;
            let h = this.handle(&b);
            sg_call(|| h.image_size(&mut b.sg.borrow_mut()))
        });
        m.add_method("SetLoadingPriority", |lua, this, args: MultiValue| {
            let b = bridge(lua)?;
            let args: Vec<Value> = args.into_iter().collect();
            assert(!args.is_empty(), || {
                "Usage: imgHandle:SetLoadingPriority(pri)".into()
            })?;
            let pri = arg_num(&args, 1, "imgHandle:SetLoadingPriority")?;
            let h = this.handle(&b);
            sg_call(|| h.set_loading_priority(&mut b.sg.borrow_mut(), pri as i32))
        });
    }
}

/// The image of a DrawImage/DrawImageQuad call: `ui->LAssert(L, imgHandle->hnd != NULL, "X(): image
/// handle has no image loaded")` (ui_api.cpp:931, :1050), checked after the argument-count switch.
fn loaded_handle(b: &Bridge, img: Option<ImgUd>, f: &str) -> mlua::Result<Option<ImageHandle>> {
    let Some(u) = img else { return Ok(None) };
    let h = u.handle(b);
    assert(h.tex().is_some(), || {
        format!("{f}(): image handle has no image loaded")
    })?;
    Ok(Some(h))
}

/// `!lua_isnil(L, 1) && !ui->IsUserData(L, 1, "uiimghandlemeta")` -> error; else the handle.
fn img_arg(args: &[Value], f: &str) -> mlua::Result<Option<ImgUd>> {
    match args.first() {
        None | Some(Value::Nil) => Ok(None),
        Some(Value::UserData(ud)) => ud.borrow::<ImgUd>().map(|u| Some(*u)).map_err(|_| {
            rt(format!(
                "{f}() argument 1: expected image handle or nil, got userdata"
            ))
        }),
        Some(v) => Err(rt(format!(
            "{f}() argument 1: expected image handle or nil, got {}",
            type_name(Some(v))
        ))),
    }
}

/// DrawImage/DrawImageQuad stack and mask arguments (ui_api.cpp:963-1003 / 1086-1125). `k` is the
/// 1-based index of the stack argument; `mask` says whether a mask argument follows.
fn stack_mask(args: &[Value], k: usize, mask: bool, f: &str) -> mlua::Result<()> {
    let stack = to_int(arg(args, k)).ok_or_else(|| {
        rt(format!(
            "{f}() argument {k}: expected integer, got {}",
            type_name(arg(args, k))
        ))
    })?;
    assert(stack > 0, || {
        format!("{f}() argument {k}: expected positive integer, got {stack}")
    })?;
    let mut has_mask = false;
    if mask && !is_nil(args, k + 1) {
        let m = to_int(arg(args, k + 1)).ok_or_else(|| {
            rt(format!(
                "{f}() argument {}: expected integer or nil, got {}",
                k + 1,
                type_name(arg(args, k + 1))
            ))
        })?;
        assert(m > 0, || {
            format!(
                "{f}() argument {}: expected positive integer, got {m}",
                k + 1
            )
        })?;
        has_mask = true;
    }
    // DIVERGENCE (module header): pob_sg has no texture arrays.
    assert(stack == 1 && !has_mask, || {
        format!("{f}(): stack index {stack} / mask: texture arrays are not supported by pob-sg")
    })
}

/// `strerror(ec.value())` from an `io::Error` ("File exists (os error 17)" -> "File exists").
fn strerror(e: &std::io::Error) -> String {
    let s = e.to_string();
    match s.rfind(" (os error ") {
        Some(i) => s[..i].to_owned(),
        None => s,
    }
}

// ---- file search (ui_api.cpp:1335-1426, sys_main.cpp:139-245) ----

/// One directory entry of a search.
#[derive(Clone, Debug)]
pub struct FoundFile {
    pub name: String,
    pub size: u64,
    /// `last_write_time().time_since_epoch().count()` of MSVC's `file_clock`: 100 ns ticks since
    /// 1601-01-01 (see [`Mtimes`]).
    pub modified: f64,
}

/// A `searchHandle` userdata (`uisearchhandlemeta`): the matching entries (already filtered by
/// `dirOnly`, `.` and `..`) and the current position; `None` once `NextFile` ran out (`find ==
/// NULL`).
pub struct SearchUd {
    found: Vec<FoundFile>,
    pos: Option<usize>,
}

impl SearchUd {
    fn cur(&self, method: &str) -> mlua::Result<&FoundFile> {
        self.pos.and_then(|i| self.found.get(i)).ok_or_else(|| {
            rt(format!(
                "searchHandle:{method}(): search handle is no longer valid (ran out of files to find)"
            ))
        })
    }
}

impl UserData for SearchUd {
    fn add_methods<M: UserDataMethods<Self>>(m: &mut M) {
        m.add_method_mut("NextFile", |_, this, ()| {
            this.cur("NextFile")?;
            let next = this.pos.map_or(0, |i| i + 1);
            if next < this.found.len() {
                this.pos = Some(next);
                Ok(Some(true))
            } else {
                this.pos = None;
                Ok(None)
            }
        });
        m.add_method("GetFileName", |_, this, ()| {
            Ok(this.cur("GetFileName")?.name.clone())
        });
        m.add_method("GetFileSize", |_, this, ()| {
            Ok(this.cur("GetFileSize")?.size as i64)
        });
        m.add_method("GetFileModifiedTime", |_, this, ()| {
            Ok(this.cur("GetFileModifiedTime")?.modified)
        });
    }
}

/// `GlobMatch(BuildGlobPattern(glob), name)`: case-insensitive; `*` is `.*`, `?` is `.`; `*` and
/// `*.*` match everything; without wildcards the name must equal the glob (case-insensitively).
pub fn glob_match(glob: &str, name: &str) -> bool {
    if glob == "*" || glob == "*.*" {
        return true;
    }
    let p: Vec<char> = glob.chars().flat_map(char::to_lowercase).collect();
    let s: Vec<char> = name.chars().flat_map(char::to_lowercase).collect();
    // Iterative wildcard match with backtracking on the last `*`.
    let (mut i, mut j) = (0usize, 0usize);
    let mut star: Option<(usize, usize)> = None;
    while j < s.len() {
        if i < p.len() && (p[i] == '?' || p[i] == s[j]) {
            i += 1;
            j += 1;
        } else if i < p.len() && p[i] == '*' {
            star = Some((i, j));
            i += 1;
        } else if let Some((si, sj)) = star {
            i = si + 1;
            j = sj + 1;
            star = Some((si, sj + 1));
        } else {
            return false;
        }
    }
    while i < p.len() && p[i] == '*' {
        i += 1;
    }
    i == p.len()
}

/// `find_c::FindFirst`/`FindNext` over `spec`'s parent directory, keeping what `NewFileSearch`
/// and `NextFile` keep (`isDirectory == dirOnly`, not `.`/`..`). DIVERGENCE: SG returns
/// `directory_iterator` order (on NTFS: names ordered by their upper-case form); the oracle sorts
/// by the upper-cased name, then the name, so runs are deterministic on every file system.
/// `GetFileModifiedTime` follows `mtimes`.
pub fn find_files(spec: &std::path::Path, dir_only: bool, mtimes: Mtimes<'_>) -> Vec<FoundFile> {
    let parent = spec.parent().unwrap_or_else(|| std::path::Path::new(""));
    let parent = if parent.as_os_str().is_empty() {
        std::path::Path::new(".")
    } else {
        parent
    };
    let glob = spec
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default();
    let mut out: Vec<FoundFile> = std::fs::read_dir(parent)
        .map(|rd| {
            rd.filter_map(Result::ok)
                .filter_map(|e| {
                    let name = e.file_name().to_string_lossy().into_owned();
                    if name == "." || name == ".." || !glob_match(&glob, &name) {
                        return None;
                    }
                    let meta = std::fs::metadata(e.path()).ok()?;
                    if meta.is_dir() != dir_only {
                        return None;
                    }
                    let modified = match mtimes {
                        Mtimes::Virtual(w) => w
                            .get(&normalize_path(&e.path()))
                            .map_or(0.0, |c| virtual_write_ticks(*c) as f64),
                        Mtimes::Disk => meta
                            .modified()
                            .ok()
                            .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
                            .map_or(0.0, |d| {
                                ((d.as_secs() + 11_644_473_600) * 10_000_000
                                    + u64::from(d.subsec_nanos() / 100))
                                    as f64
                            }),
                    };
                    Some(FoundFile {
                        name,
                        size: if meta.is_dir() { 0 } else { meta.len() },
                        modified,
                    })
                })
                .collect()
        })
        .unwrap_or_default();
    out.sort_by(|a, b| {
        a.name
            .to_uppercase()
            .cmp(&b.name.to_uppercase())
            .then_with(|| a.name.cmp(&b.name))
    });
    out
}

/// Register the SG globals in `lua`.
#[allow(clippy::too_many_lines, clippy::many_single_char_names)]
pub fn install(lua: &Lua, sg: Rc<RefCell<Sg>>, sys: Rc<RefCell<Sys>>) -> mlua::Result<()> {
    let id = NEXT_ID.fetch_add(1, Ordering::Relaxed);
    let b = Rc::new(Bridge {
        sg,
        sys,
        chunk_root: RefCell::new(String::new()),
        handles: RefCell::new(Vec::new()),
        net: RefCell::new(NetFixtures::default()),
        subs: RefCell::new(Vec::new()),
        finished_subs: RefCell::new(VecDeque::new()),
        extra: RefCell::new(Vec::new()),
        render_enable: Cell::new(false),
        writes: RefCell::new(HashMap::new()),
        virtual_mtimes: Cell::new(true),
    });
    BRIDGES.with(|m| m.borrow_mut().insert(id, b));
    lua.set_app_data(BridgeId(id));
    let g = lua.globals();
    macro_rules! def {
        ($name:literal, |$lua:ident, $b:ident, $args:ident| $body:expr) => {
            g.set(
                $name,
                lua.create_function(|$lua, $args: MultiValue| {
                    let $b = bridge($lua)?;
                    let $args: Vec<Value> = $args.into_iter().collect();
                    // Bind first: `RefCell` borrows in a tail expression would outlive `$b`.
                    let r = $body;
                    r
                })?,
            )?;
        };
    }

    // ---- draw state ----

    // SetDrawLayer({layer|nil}[, subLayer])
    def!("SetDrawLayer", |lua, b, args| {
        check_render(&b, "SetDrawLayer")?;
        let n = args.len();
        assert(n >= 1, || {
            "Usage: SetDrawLayer({layer|nil}[, subLayer])".into()
        })?;
        assert(is_number(&args, 1) || is_nil(&args, 1), || {
            format!(
                "SetDrawLayer() argument 1: expected number or nil, got {}",
                type_name(arg(&args, 1))
            )
        })?;
        if n >= 2 {
            assert(is_number(&args, 2), || {
                format!(
                    "SetDrawLayer() argument 2: expected number, got {}",
                    type_name(arg(&args, 2))
                )
            })?;
        }
        let (layer, sub) = if is_nil(&args, 1) {
            assert(n >= 2, || {
                "SetDrawLayer(): must provide subLayer if layer is nil".into()
            })?;
            (None, Some(num(&args, 2) as i32))
        } else if n >= 2 {
            (Some(num(&args, 1) as i32), Some(num(&args, 2) as i32))
        } else {
            (Some(num(&args, 1) as i32), None)
        };
        set_src(lua, &b);
        sg_call(|| b.sg.borrow_mut().set_draw_layer(layer, sub))
    });
    def!("GetDrawLayer", |_lua, b, _args| Ok(b
        .sg
        .borrow()
        .get_draw_layer()));
    // SetViewport([x, y, width, height])
    def!("SetViewport", |lua, b, args| {
        check_render(&b, "SetViewport")?;
        let n = args.len();
        if n == 0 {
            set_src(lua, &b);
            return sg_call(|| b.sg.borrow_mut().reset_viewport());
        }
        assert(n >= 4, || {
            "Usage: SetViewport([x, y, width, height])".into()
        })?;
        let mut v = [0.0; 4];
        for (i, x) in v.iter_mut().enumerate() {
            *x = arg_num(&args, i + 1, "SetViewport")?;
        }
        set_src(lua, &b);
        sg_call(|| b.sg.borrow_mut().set_viewport(v[0], v[1], v[2], v[3]))
    });
    // SetDrawColor(red, green, blue[, alpha]) or SetDrawColor(escapeStr)
    def!("SetDrawColor", |lua, b, args| {
        check_render(&b, "SetDrawColor")?;
        let n = args.len();
        let usage = "Usage: SetDrawColor(red, green, blue[, alpha]) or SetDrawColor(escapeStr)";
        assert(n >= 1, || usage.into())?;
        // `lua_type(L, 1) == LUA_TSTRING`: only a real string is an escape.
        if let Value::String(s) = &args[0] {
            let esc = c_str(&s.to_string_lossy()).to_owned();
            set_src(lua, &b);
            return sg_call(|| b.sg.borrow_mut().set_draw_color_esc(&esc));
        }
        assert(n >= 3, || usage.into())?;
        let r = arg_num(&args, 1, "SetDrawColor")?;
        let g = arg_num(&args, 2, "SetDrawColor")?;
        let bl = arg_num(&args, 3, "SetDrawColor")?;
        if n >= 4 && !is_nil(&args, 4) {
            assert(is_number(&args, 4), || {
                format!(
                    "SetDrawColor() argument 4: expected number or nil, got {}",
                    type_name(arg(&args, 4))
                )
            })?;
            let a = num(&args, 4);
            set_src(lua, &b);
            sg_call(|| b.sg.borrow_mut().set_draw_color_a(r, g, bl, a))
        } else {
            set_src(lua, &b);
            sg_call(|| b.sg.borrow_mut().set_draw_color(r, g, bl))
        }
    });
    def!("GetDrawColor", |_lua, b, _args| {
        let c = b.sg.borrow().get_draw_color();
        Ok((
            f64::from(c[0]),
            f64::from(c[1]),
            f64::from(c[2]),
            f64::from(c[3]),
        ))
    });

    // ---- drawing ----

    // DrawImage({imgHandle|nil}, left, top, width, height[, tcLeft, tcTop, tcRight, tcBottom][, stackIdx[, mask]])
    def!("DrawImage", |lua, b, args| {
        check_render(&b, "DrawImage")?;
        let n = args.len();
        let usage = "Usage: DrawImage({imgHandle|nil}, left, top, width, height[, tcLeft, tcTop, tcRight, tcBottom][, stackIdx[, mask]])";
        assert(n >= 5, || usage.into())?;
        let img = img_arg(&args, "DrawImage")?;
        // | n | 5: xy | 6: +stack | 7: +mask | 9: +uv | 10: +uv+stack | 11: +uv+stack+mask |
        let (uv, stack, mask) = match n {
            5 => (false, false, false),
            6 => (false, true, false),
            7 => (false, true, true),
            9 => (true, false, false),
            10 => (true, true, false),
            11 => (true, true, true),
            _ => return Err(rt(usage)),
        };
        let h = loaded_handle(&b, img, "DrawImage")?;
        let mut v = [0.0; 4];
        for (i, x) in v.iter_mut().enumerate() {
            *x = arg_num(&args, i + 2, "DrawImage")?;
        }
        let mut tc = [0.0; 4];
        if uv {
            for (i, x) in tc.iter_mut().enumerate() {
                *x = arg_num(&args, i + 6, "DrawImage")?;
            }
        }
        if stack {
            stack_mask(&args, if uv { 10 } else { 6 }, mask, "DrawImage")?;
        }
        set_src(lua, &b);
        if uv {
            sg_call(|| {
                b.sg.borrow_mut().draw_image_tc(
                    h.as_ref(),
                    v[0],
                    v[1],
                    v[2],
                    v[3],
                    tc[0],
                    tc[1],
                    tc[2],
                    tc[3],
                );
            })
        } else {
            sg_call(|| {
                b.sg.borrow_mut()
                    .draw_image(h.as_ref(), v[0], v[1], v[2], v[3]);
            })
        }
    });
    // DrawImageQuad({imgHandle|nil}, x1, y1, x2, y2, x3, y3, x4, y4[, s1, t1, s2, t2, s3, t3, s4, t4][, stackIdx[, mask]])
    def!("DrawImageQuad", |lua, b, args| {
        check_render(&b, "DrawImageQuad")?;
        let n = args.len();
        let usage = "Usage: DrawImageQuad({imgHandle|nil}, x1, y1, x2, y2, x3, y3, x4, y4[, s1, t1, s2, t2, s3, t3, s4, t4][, stackIdx[, mask]])";
        assert(n >= 9, || usage.into())?;
        let img = img_arg(&args, "DrawImageQuad")?;
        let (uv, stack, mask) = match n {
            9 => (false, false, false),
            10 => (false, true, false),
            11 => (false, true, true),
            17 => (true, false, false),
            18 => (true, true, false),
            19 => (true, true, true),
            _ => return Err(rt(usage)),
        };
        let h = loaded_handle(&b, img, "DrawImageQuad")?;
        let mut p = [0.0; 8];
        for (i, x) in p.iter_mut().enumerate() {
            *x = arg_num(&args, i + 2, "DrawImageQuad")?;
        }
        let mut tc = [0.0; 8];
        if uv {
            for (i, x) in tc.iter_mut().enumerate() {
                *x = arg_num(&args, i + 10, "DrawImageQuad")?;
            }
        }
        if stack {
            stack_mask(&args, if uv { 18 } else { 10 }, mask, "DrawImageQuad")?;
        }
        set_src(lua, &b);
        if uv {
            sg_call(|| b.sg.borrow_mut().draw_image_quad_tc(h.as_ref(), p, tc))
        } else {
            sg_call(|| b.sg.borrow_mut().draw_image_quad(h.as_ref(), p))
        }
    });
    // DrawString(left, top, align, height, font, text)
    def!("DrawString", |lua, b, args| {
        check_render(&b, "DrawString")?;
        assert(args.len() >= 6, || {
            "Usage: DrawString(left, top, align, height, font, text)".into()
        })?;
        let f = "DrawString";
        let x = arg_num(&args, 1, f)?;
        let y = arg_num(&args, 2, f)?;
        assert(is_string(&args, 3) || is_nil(&args, 3), || {
            format!(
                "DrawString() argument 3: expected string or nil, got {}",
                type_name(arg(&args, 3))
            )
        })?;
        let height = arg_num(&args, 4, f)?;
        assert(is_string(&args, 5), || {
            format!(
                "DrawString() argument 5: expected string, got {}",
                type_name(arg(&args, 5))
            )
        })?;
        let text = arg_str(lua, &args, 6, f)?;
        let align = arg_align(lua, &args, 3, f)?;
        let font = arg_font(lua, &args, 5, f)?;
        set_src(lua, &b);
        sg_call(|| {
            b.sg.borrow_mut()
                .draw_string(x, y, align, height, font, &text);
        })
    });
    // DrawStringWidth(height, font, text)
    def!("DrawStringWidth", |lua, b, args| {
        assert(args.len() >= 3, || {
            "Usage: DrawStringWidth(height, font, text)".into()
        })?;
        let f = "DrawStringWidth";
        let height = arg_num(&args, 1, f)?;
        assert(is_string(&args, 2), || {
            format!(
                "DrawStringWidth() argument 2: expected string, got {}",
                type_name(arg(&args, 2))
            )
        })?;
        let text = arg_str(lua, &args, 3, f)?;
        let font = arg_font(lua, &args, 2, f)?;
        sg_call(|| b.sg.borrow().draw_string_width(height, font, &text))
    });
    // DrawStringCursorIndex(height, font, text, cursorX, cursorY)
    def!("DrawStringCursorIndex", |lua, b, args| {
        assert(args.len() >= 5, || {
            "Usage: DrawStringCursorIndex(height, font, text, cursorX, cursorY)".into()
        })?;
        let f = "DrawStringCursorIndex";
        let height = arg_num(&args, 1, f)?;
        assert(is_string(&args, 2), || {
            format!(
                "DrawStringCursorIndex() argument 2: expected string, got {}",
                type_name(arg(&args, 2))
            )
        })?;
        let text = arg_str(lua, &args, 3, f)?;
        let cx = arg_num(&args, 4, f)?;
        let cy = arg_num(&args, 5, f)?;
        let font = arg_font(lua, &args, 2, f)?;
        sg_call(|| {
            b.sg.borrow()
                .draw_string_cursor_index(height, font, &text, cx, cy) as i64
        })
    });
    // StripEscapes(text)
    def!("StripEscapes", |lua, _b, args| {
        assert(!args.is_empty(), || "Usage: StripEscapes(string)".into())?;
        let text = arg_str(lua, &args, 1, "StripEscapes")?;
        Ok(pob_sg::strip_escapes(&text))
    });

    // ---- images ----

    def!("NewImageHandle", |lua, b, _args| {
        let h = b.sg.borrow_mut().new_image_handle();
        let idx = {
            let mut hs = b.handles.borrow_mut();
            hs.push(h);
            hs.len() - 1
        };
        let ud: AnyUserData = lua.create_userdata(ImgUd(idx))?;
        Ok(ud)
    });

    // ---- screen / DPI ----

    def!("GetScreenSize", |_lua, b, _args| Ok(b
        .sg
        .borrow()
        .get_screen_size()));
    def!("GetScreenScale", |_lua, b, _args| Ok(f64::from(
        b.sg.borrow().get_screen_scale()
    )));
    def!("SetDPIScaleOverridePercent", |_lua, b, args| {
        assert(!args.is_empty(), || {
            "Usage: SetDPIScaleOverridePercent(percent)".into()
        })?;
        let p = arg_num(&args, 1, "SetDPIScaleOverridePercent")?;
        b.sg.borrow_mut().set_dpi_scale_override_percent(p as i32);
        Ok(())
    });
    def!("GetDPIScaleOverridePercent", |_lua, b, _args| Ok(b
        .sg
        .borrow()
        .get_dpi_scale_override_percent()));

    // ---- input / time ----

    def!("GetCursorPos", |_lua, b, _args| Ok(b
        .sys
        .borrow()
        .cursor_pos()));
    // IsKeyDown(keyName): KeyForName; an unknown name is a Lua error.
    def!("IsKeyDown", |lua, b, args| {
        assert(!args.is_empty(), || "Usage: IsKeyDown(keyName)".into())?;
        let name = arg_str(lua, &args, 1, "IsKeyDown")?;
        assert(!name.is_empty(), || {
            "IsKeyDown() argument 1: string is empty".into()
        })?;
        let key = Key::from_name(&name).ok_or_else(|| rt("IsKeyDown(): unrecognised key name"))?;
        Ok(b.sys.borrow().is_key_down(key))
    });
    def!("GetTime", |_lua, b, _args| Ok(b.sys.borrow().time_ms()));

    // ---- platform ----

    def!("Copy", |lua, b, args| {
        assert(!args.is_empty(), || "Usage: Copy(string)".into())?;
        let text = arg_str(lua, &args, 1, "Copy")?;
        let before = effect_count(&b);
        b.sys.borrow_mut().platform.copy(&text);
        effect_unless_recorded(&b, before, SideEffect::Copy(text));
        Ok(())
    });
    // Paste(): the clipboard text, or no value at all.
    def!("Paste", |lua, b, _args| {
        let s = b.sys.borrow_mut().platform.paste();
        match s {
            Some(s) => Ok(MultiValue::from_iter([Value::String(
                lua.create_string(s)?,
            )])),
            None => Ok(MultiValue::new()),
        }
    });
    def!("SetWindowTitle", |lua, b, args| {
        assert(!args.is_empty(), || "Usage: SetWindowTitle(title)".into())?;
        let title = arg_str(lua, &args, 1, "SetWindowTitle")?;
        let before = effect_count(&b);
        b.sys.borrow_mut().platform.set_window_title(&title);
        effect_unless_recorded(&b, before, SideEffect::WindowTitle(title));
        Ok(())
    });
    def!("OpenURL", |lua, b, args| {
        assert(!args.is_empty(), || "Usage: OpenURL(url)".into())?;
        let url = arg_str(lua, &args, 1, "OpenURL")?;
        let before = effect_count(&b);
        b.sys.borrow_mut().platform.open_url(&url);
        effect_unless_recorded(&b, before, SideEffect::OpenUrl(url));
        Ok(())
    });
    def!("TakeScreenshot", |_lua, b, _args| {
        let before = effect_count(&b);
        b.sys.borrow_mut().platform.take_screenshot();
        effect_unless_recorded(&b, before, SideEffect::Screenshot);
        Ok(())
    });
    def!("Restart", |_lua, b, _args| {
        b.sys.borrow_mut().platform.restart();
        Ok(())
    });
    def!("Exit", |lua, b, args| {
        let msg = if args.is_empty() || is_nil(&args, 1) {
            None
        } else {
            assert(is_string(&args, 1), || {
                format!(
                    "Exit() argument 1: expected string or nil, got {}",
                    type_name(arg(&args, 1))
                )
            })?;
            Some(tostr(lua, &args, 1)?)
        };
        let before = effect_count(&b);
        b.sys.borrow_mut().platform.exit(msg.as_deref());
        effect_unless_recorded(&b, before, SideEffect::Exit);
        Ok(())
    });
    def!("__ui_con_print", |lua, b, args| {
        let text = arg_str(lua, &args, 1, "ConPrintf")?;
        b.sys.borrow_mut().platform.con_print(&text);
        Ok(())
    });
    // __oracleNoteWrite(absPath): `lua/determinism.lua`'s `io.open` noted a file opened for
    // writing; its `GetFileModifiedTime` becomes the virtual write time (U7's headless rule).
    def!("__oracleNoteWrite", |lua, b, args| {
        let p = arg_str(lua, &args, 1, "__oracleNoteWrite")?;
        let now = b.sys.borrow().time_ms();
        b.writes
            .borrow_mut()
            .insert(normalize_path(Path::new(&p)), now);
        Ok(())
    });
    // GetScriptPath/GetRuntimePath/GetUserPath push the UTF-8 path and the native one (identical
    // on Linux).
    def!("GetScriptPath", |_lua, b, _args| {
        let p = b.sys.borrow().platform.script_path();
        Ok((p.clone(), p))
    });
    def!("GetRuntimePath", |_lua, b, _args| {
        let p = b.sys.borrow().platform.runtime_path();
        Ok((p.clone(), p))
    });
    def!("GetWorkDir", |_lua, b, _args| Ok(b
        .sys
        .borrow()
        .platform
        .script_path()));
    def!("GetUserPath", |lua, b, _args| {
        let r = b.sys.borrow().platform.user_path();
        match r {
            Ok(p) => Ok(MultiValue::from_iter([
                Value::String(lua.create_string(&p)?),
                Value::String(lua.create_string(&p)?),
            ])),
            Err((_invalid, reason)) => Ok(MultiValue::from_iter([
                Value::Nil,
                Value::Nil,
                Value::String(lua.create_string(reason)?),
            ])),
        }
    });
    // MakeDir(path): std::filesystem::create_directory (not recursive); false (already a
    // directory) or an error gives nil, strerror(ec).
    def!("MakeDir", |lua, b, args| {
        assert(!args.is_empty(), || "Usage: MakeDir(path)".into())?;
        let path = arg_str(lua, &args, 1, "MakeDir")?;
        let exists = std::path::Path::new(&path).is_dir();
        let r = if exists {
            Err("Success".to_owned())
        } else {
            b.sys
                .borrow_mut()
                .fs
                .make_dir(&path)
                .map_err(|e| strerror(&e))
        };
        match r {
            Ok(()) => Ok(MultiValue::from_iter([Value::Boolean(true)])),
            Err(e) => Ok(MultiValue::from_iter([
                Value::Nil,
                Value::String(lua.create_string(e)?),
            ])),
        }
    });
    def!("RemoveDir", |lua, b, args| {
        let n = args.len();
        assert(n >= 1, || "Usage: l_RemoveDir(path, recurse)".into())?;
        let path = arg_str(lua, &args, 1, "l_RemoveDir")?;
        let mut recursive = false;
        if n > 1 {
            let Some(Value::Boolean(r)) = arg(&args, 2) else {
                return Err(rt(format!(
                    "l_RemoveDir() argument 2: expected boolean, got {}",
                    type_name(arg(&args, 2))
                )));
            };
            recursive = *r;
        }
        let r = if std::path::Path::new(&path).is_dir() {
            b.sys
                .borrow_mut()
                .fs
                .remove_dir(&path, recursive)
                .map_err(|e| strerror(&e))
        } else {
            Err("Success".to_owned())
        };
        match r {
            Ok(()) => Ok(MultiValue::from_iter([Value::Boolean(true)])),
            Err(e) => Ok(MultiValue::from_iter([
                Value::Nil,
                Value::String(lua.create_string(e)?),
            ])),
        }
    });

    // NewFileSearch(spec[, findDirectories]): a search handle on the first match, or nothing
    // (CCR-0028 item 4: replaces pob-oracle's version, which ignores findDirectories).
    // Relative specs resolve against PoB's src/ (the prelude's `io.open` root; SG's working dir).
    def!("NewFileSearch", |lua, b, args| {
        assert(!args.is_empty(), || {
            "Usage: NewFileSearch(spec[, findDirectories])".into()
        })?;
        let spec = arg_str(lua, &args, 1, "NewFileSearch")?;
        let dir_only = !matches!(
            arg(&args, 2),
            None | Some(Value::Nil | Value::Boolean(false))
        );
        let mut path = std::path::PathBuf::from(&spec);
        if path.is_relative() {
            let root = b.chunk_root.borrow();
            path = std::path::PathBuf::from(root.trim_start_matches('@')).join(path);
        }
        let found = if b.virtual_mtimes.get() {
            find_files(&path, dir_only, Mtimes::Virtual(&b.writes.borrow()))
        } else {
            find_files(&path, dir_only, Mtimes::Disk)
        };
        if found.is_empty() {
            return Ok(MultiValue::new());
        }
        let ud = lua.create_userdata(SearchUd {
            found,
            pos: Some(0),
        })?;
        Ok(MultiValue::from_iter([Value::UserData(ud)]))
    });

    // ---- sub-scripts (patch 8: scripted Net fixtures) ----

    // LaunchSubScript(scriptText, funcList, subList, ...): the id is a light userdata holding the
    // slot index (lowest free slot first). Only launch:DownloadPage's curl script (Launch.lua:
    // 259-324) gets a slot; its result is the scripted response for the URL (or an error), and
    // it "finishes" in the next frame's sub-script pass (deliver_subscripts, before OnFrame, as
    // ui_main.cpp:403-412 runs SubScriptFrame). Other sub-scripts (update check, OAuth listener)
    // fail to start (nil), as no fixture can answer them.
    def!("LaunchSubScript", |lua, b, args| {
        let n = args.len();
        assert(n >= 3, || {
            "Usage: LaunchSubScript(scriptText, funcList, subList[, ...])".into()
        })?;
        for i in 1..=3 {
            assert(is_string(&args, i), || {
                format!(
                    "LaunchSubScript() argument {i}: expected string, got {}",
                    type_name(arg(&args, i))
                )
            })?;
        }
        for i in 4..=n {
            assert(
                matches!(
                    arg(&args, i),
                    Some(
                        Value::Nil
                            | Value::Boolean(_)
                            | Value::Integer(_)
                            | Value::Number(_)
                            | Value::String(_)
                    )
                ),
                || {
                    format!("LaunchSubScript() argument {i}: only nil, boolean, number and string types can be passed to sub script")
                },
            )?;
        }
        let script = tostr(lua, &args, 1)?;
        if !script.contains("lcurl.safe") || !script.contains("setopt_url") {
            return Ok(Value::Nil);
        }
        let url = arg_str(lua, &args, 4, "LaunchSubScript")?;
        let resp = b.net.borrow_mut().take(&url);
        let slot = {
            let mut subs = b.subs.borrow_mut();
            let slot = subs.iter().position(Option::is_none).unwrap_or_else(|| {
                subs.push(None);
                subs.len() - 1
            });
            subs[slot] = Some(resp);
            slot
        };
        b.finished_subs.borrow_mut().push_back(slot);
        Ok(Value::LightUserData(LightUserData(
            slot as *mut std::ffi::c_void,
        )))
    });
    // IsSubScriptRunning(ssID) / AbortSubScript(ssID)
    fn sub_slot(b: &Bridge, args: &[Value], f: &str) -> mlua::Result<usize> {
        assert(!args.is_empty(), || format!("Usage: {f}(ssID)"))?;
        let Some(Value::LightUserData(p)) = arg(args, 1) else {
            return Err(rt(format!(
                "{f}() argument 1: expected subscript ID, got {}",
                type_name(arg(args, 1))
            )));
        };
        let slot = p.0 as usize;
        assert(
            b.subs.borrow().get(slot).is_some_and(Option::is_some),
            || format!("{f}() argument 1: invalid subscript ID"),
        )?;
        Ok(slot)
    }
    def!("IsSubScriptRunning", |_lua, b, args| {
        // A scripted sub-script runs until the next frame's sub-script pass frees its slot.
        sub_slot(&b, &args, "IsSubScriptRunning").map(|_| true)
    });
    def!("AbortSubScript", |_lua, b, args| {
        let slot = sub_slot(&b, &args, "AbortSubScript")?;
        b.subs.borrow_mut()[slot] = None;
        b.finished_subs.borrow_mut().retain(|s| *s != slot);
        Ok(())
    });

    // ConPrintf(fmt, ...): string.format (captured now, as SG's upvalue), then the console.
    lua.load(
        r##"
local s_format, con, type = string.format, __ui_con_print, type
function ConPrintf(...)
	local n, fmt = select("#", ...), ...
	if n < 1 then error("Usage: ConPrintf(fmt[, ...])", 0) end
	if type(fmt) ~= "string" and type(fmt) ~= "number" then
		error("ConPrintf() argument 1: expected string, got " .. type(fmt), 0)
	end
	con(s_format(...))
end
"##,
    )
    .set_name("sg_bridge")
    .exec()?;
    Ok(())
}

/// The sub-script pass of a frame (ui_main.cpp:403-412, after BeginFrame, before OnFrame): every
/// scripted sub-script launched since the last pass finishes, in slot order, with
/// `launch:OnSubFinished(id, body, errMsg, headers)` (or `launch:OnSubError(id, errMsg)` for a
/// fixture with `body: null`, a sub-script that failed), and its slot is freed.
pub fn deliver_subscripts(lua: &Lua) -> mlua::Result<()> {
    let b = bridge(lua)?;
    let mut done: Vec<usize> = b.finished_subs.borrow_mut().drain(..).collect();
    if done.is_empty() {
        return Ok(());
    }
    done.sort_unstable();
    let launch: mlua::Table = lua.globals().get("launch")?;
    for slot in done {
        let Some(r) = b.subs.borrow_mut().get_mut(slot).and_then(Option::take) else {
            continue;
        };
        let id = Value::LightUserData(LightUserData(slot as *mut std::ffi::c_void));
        let err = match r.error {
            Some(e) => Value::String(lua.create_string(e)?),
            None => Value::Nil,
        };
        // The curl script returns `responseBody` (initially "") even on failure.
        if let Some(body) = r.body {
            let f: mlua::Function = launch.get("OnSubFinished")?;
            let body = Value::String(lua.create_string(body)?);
            f.call::<()>((launch.clone(), id, body, err, r.headers))?;
        } else {
            let f: mlua::Function = launch.get("OnSubError")?;
            f.call::<()>((launch.clone(), id, err))?;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn glob_like_sg() {
        assert!(glob_match("*", "anything"));
        assert!(glob_match("*.*", "no_dot"));
        assert!(glob_match("*.xml", "Build.XML"));
        assert!(!glob_match("*.xml", "Build.xml.bak"));
        assert!(glob_match("LethalPride.zip.part*", "LethalPride.zip.part0"));
        assert!(glob_match("a?c", "abc"));
        assert!(!glob_match("a?c", "ac"));
        assert!(glob_match("Settings.xml", "settings.XML"));
        assert!(!glob_match("Settings.xml", "Settings.xmlx"));
        assert!(glob_match("*a*b*", "xxaYYbzz"));
    }

    #[test]
    fn find_files_filters_dirs_and_sorts() {
        let d = std::env::temp_dir().join(format!("pob-ui-oracle-find-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(d.join("Sub")).unwrap();
        std::fs::create_dir_all(d.join("alpha.xml")).unwrap();
        std::fs::write(d.join("b.xml"), "x").unwrap();
        std::fs::write(d.join("A.xml"), "xy").unwrap();
        std::fs::write(d.join("c.txt"), "").unwrap();
        let mut writes = HashMap::new();
        writes.insert(normalize_path(&d.join("./Sub/../b.xml")), 32);
        let files: Vec<(String, u64, f64)> =
            find_files(&d.join("*.xml"), false, Mtimes::Virtual(&writes))
                .into_iter()
                .map(|f| (f.name, f.size, f.modified))
                .collect();
        // Fixture files report 0, files written during the run the virtual write time.
        assert_eq!(
            files,
            vec![
                ("A.xml".into(), 2, 0.0),
                ("b.xml".into(), 1, virtual_write_ticks(32) as f64)
            ]
        );
        let dirs: Vec<String> = find_files(&d.join("*"), true, Mtimes::Disk)
            .into_iter()
            .map(|f| f.name)
            .collect();
        assert_eq!(dirs, vec!["alpha.xml".to_owned(), "Sub".to_owned()]);
        assert!(find_files(&d.join("missing*"), false, Mtimes::Disk).is_empty());
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn net_fixtures_like_u7() {
        let d = std::env::temp_dir().join(format!("pob-ui-oracle-net-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        std::fs::write(d.join("page.txt"), "from file").unwrap();
        std::fs::write(
            d.join("net.json"),
            r#"[{"url": "u1", "body": "one", "headers": "h"},
                {"url": "u1", "body": "two"},
                {"url": "u2", "body_file": "page.txt"},
                {"url": "u3", "body": null, "error": "boom"}]"#,
        )
        .unwrap();
        let mut net = NetFixtures::load(&d.join("net.json")).unwrap();
        let text = |resp: NetResponse| resp.body.map(|bytes| String::from_utf8(bytes).unwrap());
        let first = net.take("u1");
        assert_eq!(first.headers, "h");
        assert_eq!(text(first), Some("one".into()));
        assert_eq!(text(net.take("u1")), Some("two".into()));
        assert_eq!(text(net.take("u1")), Some("two".into())); // the last one is reused
        assert_eq!(text(net.take("u2")), Some("from file".into()));
        let failed = net.take("u3");
        assert_eq!((failed.body, failed.error.as_deref()), (None, Some("boom")));
        let unknown = net.take("nope");
        assert_eq!(unknown.body, Some(Vec::new()));
        assert_eq!(unknown.error.as_deref(), Some("No net fixture for nope"));
        assert!(NetFixtures::load(&d.join("page.txt")).is_err());
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn strerror_strips_os_error() {
        let e = std::io::Error::from_raw_os_error(17);
        assert_eq!(strerror(&e), "File exists");
    }
}
