//! Path of Building booted headless under LuaJIT — the runtime PoB itself
//! ships with. Same boot sequence and helpers as [`crate::headless::Pob`]
//! (which runs the pure-Rust interpreter, used on wasm), but roughly 25×
//! faster, so recalculation, tooltips and power reports are interactive.
//!
//! PoB reads its data files with relative paths; `io.open`, `loadfile`,
//! `dofile` and `require` are rooted at PoB's `src/` directory here, so the
//! process working directory is left alone.

use std::io::{Read, Write};
use std::path::{Path, PathBuf};

use mlua::{Lua, MultiValue, Value};

use crate::headless::{Engine, Outputs, Ret, BOOT_SCRIPT};

pub struct JitPob {
    lua: Lua,
}

fn err(e: mlua::Error) -> String {
    e.to_string()
}

/// Lua run before `HeadlessWrapper.lua`: root file access at `__mk2_root`
/// and provide the optional C modules PoB's desktop host bundles.
const PRELUDE: &str = r##"
local root = __mk2_root
local function abs(p)
    if type(p) ~= "string" or p:sub(1, 1) == "/" or p:match("^%a:[/\\]") then return p end
    return root .. p
end
__mk2_abs = abs
package.path = root .. "?.lua;" .. root .. "../runtime/lua/?.lua;" .. root .. "../runtime/lua/?/init.lua"
package.cpath = ""
local raw_open, raw_lines = io.open, io.lines
io.open = function(path, mode)
    if mode then mode = mode:gsub("([rwa])b%+", "%1+b") end
    return raw_open(abs(path), mode)
end
io.lines = function(path, ...)
    if path then return raw_lines(abs(path), ...) end
    return raw_lines()
end
-- PoB's own LuaJIT build accepts compound assignment (`n += 1`); rewrite
-- `lhs op= rhs` statements to `lhs = lhs op (rhs)` as files load.
local compound_ops = { ["+"] = true, ["-"] = true, ["*"] = true, ["/"] = true, ["%"] = true, [".."] = true }
local function rewrite_line(line)
    local ind, lhs, op, rhs = line:match("^([ \t]*)([%a_][%w_%.%[%]\"']*)[ \t]*([%+%-%*/%%%.]%.?)=[ \t]*(.-)[ \t]*\r?$")
    if ind and compound_ops[op] and rhs ~= "" then
        local code, comment = rhs:match("^(.-)[ \t]*(%-%-.*)$")
        if code then rhs = code else comment = "" end
        return ind .. lhs .. " = " .. lhs .. " " .. op .. " (" .. rhs .. ")" .. (comment ~= "" and (" " .. comment) or "")
    end
    return line
end
loadfile = function(path)
    local full = abs(path)
    local f, e = raw_open(full, "rb")
    if not f then return nil, e end
    local src = f:read("*a")
    f:close()
    if src:sub(1, 1) == "#" then src = "--" .. src end
    if src:find("[%+%-%*/%%%.]=") then
        src = src:gsub("[^\n]+", rewrite_line)
    end
    return loadstring(src, "@" .. full)
end
dofile = function(path)
    local fn = assert(loadfile(path))
    return fn()
end
-- `require` for Lua modules goes through the same loader.
package.loaders[2] = function(name)
    local rel = name:gsub("%.", "/")
    local tried = {}
    for pattern in package.path:gmatch("[^;]+") do
        local file = pattern:gsub("%?", rel)
        local f = raw_open(file, "rb")
        if f then
            f:close()
            local fn, e = loadfile(file)
            if not fn then error(e, 2) end
            return fn
        end
        tried[#tried + 1] = "\n\tno file '" .. file .. "'"
    end
    return table.concat(tried)
end
local raw_remove, raw_rename = os.remove, os.rename
os.remove = function(path) return raw_remove(abs(path)) end
os.rename = function(a, b) return raw_rename(abs(a), abs(b)) end
package.preload["lua-utf8"] = function()
    local m = {}
    for _, n in ipairs({ "len", "sub", "reverse", "gsub", "find", "match", "gmatch", "upper", "lower", "format", "byte", "char", "rep" }) do
        m[n] = string[n]
    end
    m.offset = function(_, n) return n or 1 end
    m.codepoint = function(s, i) return s:byte(i or 1) end
    return m
end
for _, name in ipairs({ "lcurl", "lcurl.safe", "lfs", "socket", "cjson", "re", "lua-profiler", "lpeg" }) do
    package.preload[name] = function() return false end
end
arg = {}
"##;

/// Wraps the raw `NewFileSearch` result in PoB's handle API.
const FILE_SEARCH: &str = r"
local function current(self) return self.files[self.idx] end
local raw_search = __mk2_file_search
function NewFileSearch(pattern, dirs)
    local h = raw_search(__mk2_abs(pattern), dirs)
    if not h then return nil end
    function h:GetFileName() return current(self).name end
    function h:GetFileModifiedTime() return current(self).modified end
    function h:GetFileSize() return 0 end
    function h:NextFile()
        self.idx = self.idx + 1
        return self.files[self.idx] ~= nil
    end
    return h
end
";

impl JitPob {
    /// Boot PoB from its `src/` directory.
    pub fn boot(src: &Path) -> Result<Self, String> {
        let lua = Lua::new();
        let root = std::fs::canonicalize(src).map_err(|e| format!("{}: {e}", src.display()))?;
        let mut root_s = root.to_string_lossy().replace('\\', "/");
        if !root_s.ends_with('/') {
            root_s.push('/');
        }
        let g = lua.globals();
        g.set("__mk2_root", root_s).map_err(err)?;
        lua.load(PRELUDE).set_name("prelude").exec().map_err(err)?;

        let wrapper = std::fs::read_to_string(root.join("HeadlessWrapper.lua"))
            .map_err(|e| format!("HeadlessWrapper.lua: {e}"))?;
        // Everything before `dofile("Launch.lua")`: SimpleGraphic stubs,
        // callback plumbing and the `require` hack.
        let prefix = wrapper
            .find("dofile(\"Launch.lua\")")
            .map_or(wrapper.as_str(), |i| &wrapper[..i]);
        lua.load(prefix)
            .set_name("HeadlessWrapper.lua")
            .exec()
            .map_err(err)?;
        install_natives(&lua).map_err(err)?;
        lua.load("dofile('Launch.lua')")
            .set_name("boot")
            .exec()
            .map_err(err)?;
        lua.load(BOOT_SCRIPT).set_name("boot").exec().map_err(err)?;
        drop(g);
        Ok(Self { lua })
    }
}

fn to_ret(v: &Value) -> Ret {
    match v {
        Value::String(s) => Ret::Str(String::from_utf8_lossy(&s.as_bytes()).into_owned()),
        Value::Nil => Ret::Other("nil".to_owned()),
        Value::Boolean(b) => Ret::Other(b.to_string()),
        Value::Integer(i) => Ret::Other(i.to_string()),
        Value::Number(n) => Ret::Other(crate::lua::value::num_to_string(*n)),
        other => Ret::Other(other.type_name().to_owned()),
    }
}

impl Engine for JitPob {
    fn run(&mut self, code: &str) -> Result<Vec<Ret>, String> {
        let vals: MultiValue = self.lua.load(code).set_name("exec").eval().map_err(err)?;
        Ok(vals.iter().map(to_ret).collect())
    }

    fn load_build_xml(&mut self, xml: &str, name: &str) -> Result<(), String> {
        let g = self.lua.globals();
        g.set("__pob_xml", self.lua.create_string(xml).map_err(err)?)
            .map_err(err)?;
        g.set("__pob_name", name).map_err(err)?;
        self.run(crate::headless::LOAD_SCRIPT).map(|_| ())
    }

    fn player_output(&mut self) -> Result<Outputs, String> {
        let (player, minion): (Value, Value) = self
            .lua
            .load(crate::headless::OUTPUT_SCRIPT)
            .set_name("output")
            .eval()
            .map_err(err)?;
        let mut out = Outputs::default();
        for (prefix, t) in [("", player), ("Minion", minion)] {
            let Value::Table(t) = t else { continue };
            for pair in t.pairs::<Value, Value>() {
                let Ok((Value::String(k), v)) = pair else {
                    continue;
                };
                let k = format!("{prefix}{}", k.to_string_lossy());
                match v {
                    Value::Number(n) => {
                        out.numbers.insert(k, n);
                    }
                    Value::Integer(i) => {
                        out.numbers.insert(k, i as f64);
                    }
                    Value::Boolean(b) => {
                        out.flags.insert(k, b);
                    }
                    Value::String(s) => {
                        out.strings.insert(k, s.to_string_lossy());
                    }
                    _ => {}
                }
            }
        }
        Ok(out)
    }
}

/// `Inflate` / `Deflate` / `NewFileSearch`, which PoB's C host provides.
fn install_natives(lua: &Lua) -> mlua::Result<()> {
    let g = lua.globals();
    g.set(
        "Inflate",
        lua.create_function(|lua, data: mlua::String| {
            let data = data.as_bytes();
            let mut out = Vec::new();
            let ok = flate2::read::ZlibDecoder::new(&data[..])
                .read_to_end(&mut out)
                .is_ok()
                || {
                    out.clear();
                    flate2::read::DeflateDecoder::new(&data[..])
                        .read_to_end(&mut out)
                        .is_ok()
                };
            lua.create_string(if ok { &out[..] } else { b"" })
        })?,
    )?;
    g.set(
        "Deflate",
        lua.create_function(|lua, data: mlua::String| {
            let mut enc =
                flate2::write::ZlibEncoder::new(Vec::new(), flate2::Compression::default());
            let _ = enc.write_all(&data.as_bytes());
            lua.create_string(enc.finish().unwrap_or_default())
        })?,
    )?;
    g.set(
        "__mk2_file_search",
        lua.create_function(|lua, (pattern, _dirs): (String, Option<bool>)| {
            let (pattern, exact) = match pattern.strip_suffix('*') {
                Some(p) => (p.to_owned(), false),
                None => (pattern, true),
            };
            let path = PathBuf::from(&pattern);
            let (dir, prefix) = if pattern.ends_with('/') {
                (path.clone(), String::new())
            } else {
                (
                    path.parent().map(Path::to_path_buf).unwrap_or_default(),
                    path.file_name()
                        .map(|n| n.to_string_lossy().into_owned())
                        .unwrap_or_default(),
                )
            };
            let mut files: Vec<(String, f64)> = std::fs::read_dir(&dir)
                .map(|rd| {
                    rd.filter_map(Result::ok)
                        .filter_map(|e| {
                            let name = e.file_name().to_string_lossy().into_owned();
                            let hit = if exact {
                                name == prefix
                            } else {
                                name.starts_with(&prefix)
                            };
                            let modified = e
                                .metadata()
                                .and_then(|m| m.modified())
                                .ok()
                                .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
                                .map_or(0.0, |d| d.as_secs_f64());
                            hit.then_some((name, modified))
                        })
                        .collect()
                })
                .unwrap_or_default();
            if files.is_empty() {
                return Ok(Value::Nil);
            }
            files.sort_by(|a, b| a.0.cmp(&b.0));
            let handle = lua.create_table()?;
            let list = lua.create_table()?;
            for (i, (name, modified)) in files.into_iter().enumerate() {
                let entry = lua.create_table()?;
                entry.set("name", name)?;
                entry.set("modified", modified)?;
                list.set(i + 1, entry)?;
            }
            handle.set("files", list)?;
            handle.set("idx", 1)?;
            Ok(Value::Table(handle))
        })?,
    )?;
    // HTTP for launch:DownloadPage: (url, headerLines, body, userAgent) ->
    // body, errMsg, responseHeader.
    g.set(
        "__mk2_http",
        lua.create_function(
            |lua,
             (url, header, body, agent): (
                String,
                Option<String>,
                Option<mlua::String>,
                Option<String>,
            )| {
                let agent_b = ureq::AgentBuilder::new()
                    .user_agent(agent.as_deref().unwrap_or("Path of Building"))
                    .timeout_connect(std::time::Duration::from_secs(10))
                    .timeout(std::time::Duration::from_secs(30))
                    .build();
                let mut req = match body {
                    Some(_) => agent_b.post(&url),
                    None => agent_b.get(&url),
                };
                for line in header.as_deref().unwrap_or("").lines() {
                    if let Some((k, v)) = line.split_once(':') {
                        req = req.set(k.trim(), v.trim());
                    }
                }
                let result = match &body {
                    Some(b) => req.send_bytes(&b.as_bytes()),
                    None => req.call(),
                };
                let (resp, err) = match result {
                    Ok(r) => (Some(r), None),
                    Err(ureq::Error::Status(code, r)) => {
                        (Some(r), Some(format!("Response code: {code}")))
                    }
                    Err(e) => (None, Some(e.to_string())),
                };
                let mut head = String::new();
                let mut data = Vec::new();
                if let Some(r) = resp {
                    head = r
                        .headers_names()
                        .iter()
                        .filter_map(|n| r.header(n).map(|v| format!("{n}: {v}\r\n")))
                        .collect();
                    let _ = r.into_reader().read_to_end(&mut data);
                }
                let err = err.or_else(|| data.is_empty().then(|| "No data returned".to_owned()));
                Ok((lua.create_string(&data)?, err, head))
            },
        )?,
    )?;
    // Open a URL in the user's browser.
    g.set(
        "OpenURL",
        lua.create_function(|_, url: String| {
            #[cfg(target_os = "macos")]
            let cmd = ("open", vec![url]);
            #[cfg(target_os = "windows")]
            let cmd = (
                "cmd",
                vec!["/C".to_owned(), "start".to_owned(), String::new(), url],
            );
            #[cfg(not(any(target_os = "macos", target_os = "windows")))]
            let cmd = ("xdg-open", vec![url]);
            let _ = std::process::Command::new(cmd.0).args(cmd.1).spawn();
            Ok(())
        })?,
    )?;
    lua.load(FILE_SEARCH).set_name("file_search").exec()
}
