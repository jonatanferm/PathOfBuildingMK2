//! Host environment for running Path of Building's own Lua headless:
//! module loading (`require`, `loadfile`, `dofile`, `load`), `io`, a
//! run-to-completion `coroutine` shim, and the native helpers PoB's C host
//! normally provides (`Inflate` / `Deflate`, `NewFileSearch`, `lua-utf8`).
//!
//! Files come through [`HostFs`] so the same code runs against the real
//! disk (desktop) or an embedded file map (Wasm).

use std::cell::RefCell;
use std::collections::BTreeMap;
use std::io::{Read, Write};
use std::rc::Rc;

use super::interp::{Interp, LuaError};
use super::stdlib::{arg, arg_str, arg_table, opt_num, reg, tostring};
use super::value::{TableRef, Val};

type R<T> = Result<T, LuaError>;

/// A directory entry for `NewFileSearch`.
#[derive(Debug, Clone)]
pub struct DirEntry {
    pub name: String,
    pub is_dir: bool,
    pub modified: f64,
    pub size: f64,
}

/// File access for the Lua host. Paths are as PoB writes them (relative to
/// its `src/` working directory, `/`-separated).
pub trait HostFs {
    fn read(&self, path: &str) -> Option<Vec<u8>>;
    /// Whether `path` is an existing file (cheaper than reading it).
    fn exists(&self, path: &str) -> bool {
        self.read(path).is_some()
    }
    fn write(&self, path: &str, data: &[u8]) -> bool;
    /// Entries matching `pattern` (a path whose last component may end in `*`).
    fn search(&self, pattern: &str, dirs: bool) -> Vec<DirEntry>;
}

/// [`HostFs`] over the real file system, rooted at a working directory.
pub struct StdFs {
    pub root: std::path::PathBuf,
}

impl StdFs {
    fn resolve(&self, path: &str) -> std::path::PathBuf {
        let p = std::path::Path::new(path);
        if p.is_absolute() {
            p.to_path_buf()
        } else {
            self.root.join(p)
        }
    }
}

impl HostFs for StdFs {
    fn read(&self, path: &str) -> Option<Vec<u8>> {
        let r = std::fs::read(self.resolve(path)).ok();
        if r.is_some() && std::env::var_os("POB_FS_TRACE").is_some() {
            eprintln!("[fs] read {path}");
        }
        r
    }

    fn exists(&self, path: &str) -> bool {
        self.resolve(path).is_file()
    }

    fn write(&self, path: &str, data: &[u8]) -> bool {
        let p = self.resolve(path);
        if let Some(parent) = p.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        std::fs::write(p, data).is_ok()
    }

    fn search(&self, pattern: &str, dirs: bool) -> Vec<DirEntry> {
        if std::env::var_os("POB_FS_TRACE").is_some() {
            eprintln!("[fs] search {pattern}");
        }
        let full = self.resolve(pattern);
        let (dir, name) = (
            full.parent()
                .map(std::path::Path::to_path_buf)
                .unwrap_or_default(),
            full.file_name()
                .map(|n| n.to_string_lossy().into_owned())
                .unwrap_or_default(),
        );
        let (prefix, wildcard) = match name.split_once('*') {
            Some((p, _)) => (p.to_owned(), true),
            None => (name.clone(), false),
        };
        let Ok(rd) = std::fs::read_dir(&dir) else {
            return Vec::new();
        };
        let mut out: Vec<DirEntry> = rd
            .filter_map(Result::ok)
            .filter_map(|e| {
                let n = e.file_name().to_string_lossy().into_owned();
                let hit = if wildcard {
                    n.starts_with(&prefix)
                } else {
                    n == prefix
                };
                let meta = e.metadata().ok()?;
                (hit && meta.is_dir() == dirs).then(|| DirEntry {
                    name: n,
                    is_dir: meta.is_dir(),
                    modified: meta
                        .modified()
                        .ok()
                        .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
                        .map_or(0.0, |d| d.as_secs_f64().floor()),
                    size: meta.len() as f64,
                })
            })
            .collect();
        out.sort_by(|a, b| a.name.cmp(&b.name));
        out
    }
}

/// In-memory [`HostFs`] (e.g. for Wasm): path → bytes.
#[derive(Default)]
pub struct MemFs {
    pub files: RefCell<BTreeMap<String, Vec<u8>>>,
}

fn normalize(path: &str) -> String {
    let mut parts: Vec<&str> = Vec::new();
    for c in path.split(['/', '\\']) {
        match c {
            "" | "." => {}
            ".." => {
                if parts.last().is_some_and(|p| *p != "..") {
                    parts.pop();
                } else {
                    parts.push("..");
                }
            }
            other => parts.push(other),
        }
    }
    parts.join("/")
}

impl HostFs for MemFs {
    fn read(&self, path: &str) -> Option<Vec<u8>> {
        self.files.borrow().get(&normalize(path)).cloned()
    }

    fn write(&self, path: &str, data: &[u8]) -> bool {
        self.files
            .borrow_mut()
            .insert(normalize(path), data.to_vec());
        true
    }

    fn search(&self, pattern: &str, dirs: bool) -> Vec<DirEntry> {
        if dirs {
            return Vec::new();
        }
        let norm = normalize(pattern);
        let (prefix, wildcard) = match norm.split_once('*') {
            Some((p, _)) => (p.to_owned(), true),
            None => (norm.clone(), false),
        };
        self.files
            .borrow()
            .iter()
            .filter(|(k, _)| {
                if wildcard {
                    k.starts_with(&prefix) && !k[prefix.len()..].contains('/')
                } else {
                    **k == prefix
                }
            })
            .map(|(k, v)| DirEntry {
                name: k.rsplit('/').next().unwrap_or(k).to_owned(),
                is_dir: false,
                modified: 0.0,
                size: v.len() as f64,
            })
            .collect()
    }
}

fn load_source(fs: &dyn HostFs, path: &str) -> Option<String> {
    let bytes = fs.read(path)?;
    Some(String::from_utf8_lossy(&bytes).into_owned())
}

/// Install the host environment into `it`. `package.path` follows PoB's
/// layout: modules relative to `src/`, runtime libraries in `../runtime/lua`.
#[allow(clippy::too_many_lines)]
pub fn install(it: &mut Interp, fs: Rc<dyn HostFs>) {
    let g = it.globals.clone();

    // ---- package / require / load ----
    let package = TableRef::new();
    package.set_str("loaded", Val::Table(TableRef::new()));
    package.set_str("preload", Val::Table(TableRef::new()));
    package.set_str(
        "path",
        Val::str("?.lua;?/init.lua;../runtime/lua/?.lua;../runtime/lua/?/init.lua"),
    );
    g.set_str("package", Val::Table(package.clone()));

    {
        let fs = fs.clone();
        reg(&g, "loadfile", move |it, a| {
            let path = arg_str(&a, 0, "loadfile")?;
            match load_source(fs.as_ref(), &path) {
                Some(src) => match it.load_chunk(&src, &path) {
                    Ok(f) => Ok(vec![f]),
                    Err(e) => Ok(vec![Val::Nil, Val::str(&e.0)]),
                },
                None => Ok(vec![Val::Nil, Val::str(&format!("cannot open {path}"))]),
            }
        });
    }
    {
        let fs = fs.clone();
        reg(&g, "dofile", move |it, a| {
            let path = arg_str(&a, 0, "dofile")?;
            let src = load_source(fs.as_ref(), &path)
                .ok_or_else(|| LuaError::msg(format!("cannot open {path}")))?;
            it.exec_chunk(&src, &path, Vec::new())
        });
    }
    let load = |it: &mut Interp, a: Vec<Val>| -> R<Vec<Val>> {
        let name = a
            .get(1)
            .and_then(|v| v.as_str().map(str::to_owned))
            .unwrap_or_else(|| "=(load)".into());
        let src = match arg(&a, 0) {
            Val::Str(s) => s.to_str().into_owned(),
            f @ Val::Func(_) => {
                let mut out = String::new();
                loop {
                    let piece = it
                        .call(&f, Vec::new())?
                        .into_iter()
                        .next()
                        .unwrap_or_default();
                    match piece {
                        Val::Str(s) if !s.is_empty() => out.push_str(&s.to_str()),
                        _ => break,
                    }
                }
                out
            }
            other => {
                return Err(LuaError::msg(format!(
                    "bad argument #1 to 'load' ({})",
                    other.type_name()
                )))
            }
        };
        match it.load_chunk(&src, &name) {
            Ok(f) => Ok(vec![f]),
            Err(e) => Ok(vec![Val::Nil, Val::str(&e.0)]),
        }
    };
    reg(&g, "load", load);
    reg(&g, "loadstring", load);
    {
        let fs = fs.clone();
        let package = package.clone();
        reg(&g, "require", move |it, a| {
            let name = arg_str(&a, 0, "require")?;
            let loaded = package
                .get_str("loaded")
                .as_table()
                .cloned()
                .unwrap_or_default();
            let hit = loaded.get_str(&name);
            if !hit.is_nil() {
                return Ok(vec![hit]);
            }
            let preload = package
                .get_str("preload")
                .as_table()
                .cloned()
                .unwrap_or_default();
            let loader = preload.get_str(&name);
            let result = if loader.is_nil() {
                let path_spec = package
                    .get_str("path")
                    .as_str()
                    .map(str::to_owned)
                    .unwrap_or_default();
                let file = name.replace('.', "/");
                let mut found = None;
                for tmpl in path_spec.split(';') {
                    let candidate = tmpl.replace('?', &file);
                    if let Some(src) = load_source(fs.as_ref(), &candidate) {
                        found = Some((candidate, src));
                        break;
                    }
                }
                let (path, src) =
                    found.ok_or_else(|| LuaError::msg(format!("module '{name}' not found")))?;
                it.exec_chunk(&src, &path, vec![Val::str(&name)])?
            } else {
                it.call(&loader, vec![Val::str(&name)])?
            };
            let value = result
                .into_iter()
                .next()
                .filter(Val::truthy)
                .unwrap_or(Val::Bool(true));
            // A module may have stored itself in package.loaded already.
            let existing = loaded.get_str(&name);
            if existing.is_nil() {
                loaded.set_str(&name, value.clone());
                Ok(vec![value])
            } else {
                Ok(vec![existing])
            }
        });
    }

    // ---- io ----
    let io = TableRef::new();
    {
        let fs = fs.clone();
        reg(&io, "open", move |_, a| {
            let path = arg_str(&a, 0, "open")?;
            let mode = a
                .get(1)
                .and_then(|v| v.as_str().map(str::to_owned))
                .unwrap_or_else(|| "r".into());
            let writing = mode.contains('w') || mode.contains('a');
            // Reading is lazy: PoB often opens files only to test they exist.
            let content = if writing {
                Some(if mode.contains('a') {
                    fs.read(&path).unwrap_or_default()
                } else {
                    Vec::new()
                })
            } else if fs.exists(&path) {
                None
            } else {
                return Ok(vec![
                    Val::Nil,
                    Val::str(&format!("{path}: No such file or directory")),
                ]);
            };
            Ok(vec![Val::Table(make_file(
                fs.clone(),
                path,
                content,
                writing,
            ))])
        });
    }
    reg(&io, "read", |_, _| Ok(vec![Val::Nil]));
    reg(&io, "write", |_, a| {
        let mut err = std::io::stderr();
        for v in &a {
            let _ = err.write_all(tostring(v).as_bytes());
        }
        Ok(Vec::new())
    });
    {
        let fs = fs.clone();
        reg(&io, "lines", move |_, a| {
            let path = arg_str(&a, 0, "lines")?;
            let text = load_source(fs.as_ref(), &path)
                .ok_or_else(|| LuaError::msg(format!("{path}: No such file")))?;
            let lines: Vec<String> = text.lines().map(str::to_owned).collect();
            let pos = RefCell::new(0usize);
            Ok(vec![Val::native(move |_, _| {
                let mut p = pos.borrow_mut();
                let out = lines.get(*p).map_or(Val::Nil, |l| Val::str(l));
                *p += 1;
                Ok(vec![out])
            })])
        });
    }
    g.set_str("io", Val::Table(io));

    // ---- os extras ----
    if let Some(os) = g.get_str("os").as_table().cloned() {
        reg(&os, "date", |_, a| {
            let fmt = a
                .first()
                .and_then(|v| v.as_str().map(str::to_owned))
                .unwrap_or_default();
            Ok(vec![if fmt.starts_with("*t") || fmt.starts_with("!*t") {
                let t = TableRef::new();
                for (k, v) in [
                    ("year", 2026.0),
                    ("month", 1.0),
                    ("day", 1.0),
                    ("hour", 0.0),
                    ("min", 0.0),
                    ("sec", 0.0),
                ] {
                    t.set_str(k, Val::Num(v));
                }
                Val::Table(t)
            } else {
                Val::str("2026-01-01 00:00:00")
            }])
        });
        {
            let fs = fs.clone();
            reg(&os, "remove", move |_, a| {
                let path = arg_str(&a, 0, "remove")?;
                Ok(vec![Val::Bool(fs.write(&path, &[]))])
            });
        }
        reg(&os, "rename", |_, _| Ok(vec![Val::Bool(true)]));
        reg(&os, "exit", |_, _| Err(LuaError::msg("os.exit called")));
    }

    // ---- math extras ----
    if let Some(m) = g.get_str("math").as_table().cloned() {
        let state = Rc::new(RefCell::new(0x2545_f491_4f6c_dd1d_u64));
        let st = state.clone();
        reg(&m, "randomseed", move |_, a| {
            *st.borrow_mut() = (opt_num(&a, 0).unwrap_or(0.0) as u64) | 1;
            Ok(Vec::new())
        });
        reg(&m, "random", move |_, a| {
            let mut s = state.borrow_mut();
            *s ^= *s << 13;
            *s ^= *s >> 7;
            *s ^= *s << 17;
            let r = (*s >> 11) as f64 / (1u64 << 53) as f64;
            Ok(vec![Val::Num(match (opt_num(&a, 0), opt_num(&a, 1)) {
                (None, _) => r,
                (Some(hi), None) => (r * hi).floor() + 1.0,
                (Some(lo), Some(hi)) => lo + (r * (hi - lo + 1.0)).floor(),
            })])
        });
    }

    // ---- coroutine: run to completion; yield is a no-op ----
    let co = TableRef::new();
    reg(&co, "create", |_, a| {
        let t = TableRef::new();
        t.set_str("__fn", arg(&a, 0));
        t.set_str("__status", Val::str("suspended"));
        Ok(vec![Val::Table(t)])
    });
    reg(&co, "resume", |it, mut a| {
        if a.is_empty() {
            return Err(LuaError::msg("bad argument #1 to 'resume'"));
        }
        let co = arg_table(&a, 0, "resume")?;
        if co.get_str("__status").as_str() == Some("dead") {
            return Ok(vec![
                Val::Bool(false),
                Val::str("cannot resume dead coroutine"),
            ]);
        }
        let f = co.get_str("__fn");
        a.remove(0);
        co.set_str("__status", Val::str("running"));
        let res = it.call(&f, a);
        co.set_str("__status", Val::str("dead"));
        Ok(match res {
            Ok(mut v) => {
                v.insert(0, Val::Bool(true));
                v
            }
            Err(e) => vec![Val::Bool(false), Val::str(&e.0)],
        })
    });
    reg(&co, "yield", |_, a| Ok(a));
    reg(&co, "status", |_, a| {
        let co = arg_table(&a, 0, "status")?;
        Ok(vec![co.get_str("__status")])
    });
    reg(&co, "running", |_, _| Ok(vec![Val::Nil, Val::Bool(true)]));
    reg(&co, "wrap", |_, a| {
        let f = arg(&a, 0);
        Ok(vec![Val::native(move |it, args| it.call(&f, args))])
    });
    g.set_str("coroutine", Val::Table(co));

    // ---- misc globals ----
    g.set_str("arg", Val::Table(TableRef::new()));
    reg(&g, "collectgarbage", |_, _| Ok(vec![Val::Num(0.0)]));
    let debug = TableRef::new();
    reg(&debug, "traceback", |_, a| {
        Ok(vec![a.first().cloned().unwrap_or_else(|| Val::str(""))])
    });
    g.set_str("debug", Val::Table(debug));
    let jit = TableRef::new();
    let opt = TableRef::new();
    reg(&opt, "start", |_, _| Ok(Vec::new()));
    jit.set_str("opt", Val::Table(opt));
    reg(&jit, "on", |_, _| Ok(Vec::new()));
    reg(&jit, "off", |_, _| Ok(Vec::new()));
    jit.set_str("version", Val::str("LuaJIT 2.1.0 (pob-calc)"));
    g.set_str("jit", Val::Table(jit));
    reg(&g, "setfenv", |_, a| Ok(vec![arg(&a, 0)]));
    reg(&g, "xpcall", |it, a| {
        let f = arg(&a, 0);
        let handler = arg(&a, 1);
        let rest = a.get(2..).map(<[Val]>::to_vec).unwrap_or_default();
        match it.call(&f, rest) {
            Ok(mut v) => {
                v.insert(0, Val::Bool(true));
                Ok(v)
            }
            Err(e) => {
                let h = it
                    .call(&handler, vec![Val::str(&e.0)])?
                    .into_iter()
                    .next()
                    .unwrap_or_default();
                Ok(vec![Val::Bool(false), h])
            }
        }
    });

    install_natives(it, fs.clone());

    // lua-utf8 (byte-based fallbacks; PoB only needs ASCII-safe behaviour).
    let preload = package
        .get_str("preload")
        .as_table()
        .cloned()
        .unwrap_or_default();
    preload.set_str(
        "lua-utf8",
        Val::native(|it, _| {
            let s = it.string_lib.clone();
            let m = TableRef::new();
            for name in [
                "len", "sub", "reverse", "gsub", "find", "match", "gmatch", "upper", "lower",
                "format", "byte", "char", "rep",
            ] {
                m.set_str(name, s.get_str(name));
            }
            reg(&m, "offset", |_, a| {
                let n = opt_num(&a, 1).unwrap_or(1.0);
                Ok(vec![Val::Num(n)])
            });
            reg(&m, "codepoint", |_, a| {
                let st = arg_str(&a, 0, "codepoint")?;
                let i = opt_num(&a, 1).unwrap_or(1.0) as usize;
                Ok(vec![st
                    .as_bytes()
                    .get(i.saturating_sub(1))
                    .map_or(Val::Nil, |b| Val::Num(f64::from(*b)))])
            });
            Ok(vec![Val::Table(m)])
        }),
    );
    for stub in [
        "lcurl",
        "lcurl.safe",
        "sha1",
        "lfs",
        "socket",
        "cjson",
        "re",
        "lua-profiler",
        "lpeg",
    ] {
        preload.set_str(stub, Val::native(|_, _| Ok(vec![Val::Nil])));
    }
}

/// Native helpers PoB's C host provides (`Inflate`, `Deflate`,
/// `NewFileSearch`). Call again after loading `_SimpleGraphic.def.lua`,
/// which defines inert Lua stubs for them.
pub fn install_natives(it: &mut Interp, fs: Rc<dyn HostFs>) {
    let g = it.globals.clone();
    reg(&g, "Inflate", |_, a| {
        let data = arg_bytes(&a, 0)?;
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
        Ok(vec![if ok { bytes_val(&out) } else { Val::str("") }])
    });
    reg(&g, "Deflate", |_, a| {
        let data = arg_bytes(&a, 0)?;
        let mut enc = flate2::write::ZlibEncoder::new(Vec::new(), flate2::Compression::default());
        let _ = enc.write_all(&data);
        Ok(vec![bytes_val(&enc.finish().unwrap_or_default())])
    });
    {
        let fs = fs.clone();
        reg(&g, "NewFileSearch", move |_, a| {
            let pattern = arg_str(&a, 0, "NewFileSearch")?;
            let dirs = arg(&a, 1).truthy();
            let entries = fs.search(&pattern, dirs);
            if entries.is_empty() {
                return Ok(vec![Val::Nil]);
            }
            Ok(vec![Val::Table(make_search(entries))])
        });
    }
}

fn arg_bytes(a: &[Val], i: usize) -> R<Vec<u8>> {
    match a.get(i) {
        Some(Val::Str(s)) => Ok(s.as_bytes().to_vec()),
        _ => Err(LuaError::msg("bad argument (string expected)")),
    }
}

fn bytes_val(b: &[u8]) -> Val {
    Val::bytes(b)
}

fn make_search(entries: Vec<DirEntry>) -> TableRef {
    let h = TableRef::new();
    let state = Rc::new(RefCell::new(0usize));
    let entries = Rc::new(entries);
    let (s, e) = (state.clone(), entries.clone());
    reg(&h, "GetFileName", move |_, _| {
        Ok(vec![Val::str(&e[*s.borrow()].name)])
    });
    let (s, e) = (state.clone(), entries.clone());
    reg(&h, "GetFileModifiedTime", move |_, _| {
        Ok(vec![Val::Num(e[*s.borrow()].modified)])
    });
    let (s, e) = (state.clone(), entries.clone());
    reg(&h, "GetFileSize", move |_, _| {
        Ok(vec![Val::Num(e[*s.borrow()].size)])
    });
    reg(&h, "NextFile", move |_, _| {
        let mut i = state.borrow_mut();
        *i += 1;
        Ok(vec![Val::Bool(*i < entries.len())])
    });
    h
}

fn make_file(
    fs: Rc<dyn HostFs>,
    path: String,
    content: Option<Vec<u8>>,
    writing: bool,
) -> TableRef {
    let f = TableRef::new();
    let pending = Rc::new(RefCell::new(content.is_none()));
    let buf = Rc::new(RefCell::new(content.unwrap_or_default()));
    // Load the file on first access.
    let ensure = {
        let (fs, path, buf, pending) = (fs.clone(), path.clone(), buf.clone(), pending.clone());
        Rc::new(move || {
            if *pending.borrow() {
                *buf.borrow_mut() = fs.read(&path).unwrap_or_default();
                *pending.borrow_mut() = false;
            }
        })
    };
    let pos = Rc::new(RefCell::new(0usize));
    let (b, p, en) = (buf.clone(), pos.clone(), ensure.clone());
    reg(&f, "read", move |_, a| {
        en();
        let fmt = a.get(1).cloned().unwrap_or_else(|| Val::str("*l"));
        let data = b.borrow();
        let mut i = p.borrow_mut();
        if *i >= data.len() {
            return Ok(vec![Val::Nil]);
        }
        let out = match &fmt {
            Val::Num(n) => {
                let end = (*i + *n as usize).min(data.len());
                let v = bytes_val(&data[*i..end]);
                *i = end;
                v
            }
            Val::Str(s) if s.as_bytes().starts_with(b"*a") || s.as_bytes().starts_with(b"a") => {
                let v = bytes_val(&data[*i..]);
                *i = data.len();
                v
            }
            Val::Str(s) if s.as_bytes().starts_with(b"*n") || s.as_bytes().starts_with(b"n") => {
                let rest = String::from_utf8_lossy(&data[*i..]).into_owned();
                let t = rest.trim_start();
                let len = t
                    .find(|c: char| !(c.is_ascii_digit() || "+-.eE".contains(c)))
                    .unwrap_or(t.len());
                *i += rest.len() - t.len() + len;
                t[..len].parse::<f64>().map_or(Val::Nil, Val::Num)
            }
            _ => {
                let end = data[*i..]
                    .iter()
                    .position(|&c| c == b'\n')
                    .map_or(data.len(), |p| *i + p);
                let mut line = &data[*i..end];
                if line.ends_with(b"\r") {
                    line = &line[..line.len() - 1];
                }
                let v = bytes_val(line);
                *i = (end + 1).min(data.len() + 1);
                v
            }
        };
        Ok(vec![out])
    });
    let (b, p, en) = (buf.clone(), pos.clone(), ensure.clone());
    reg(&f, "lines", move |_, _| {
        en();
        let text = String::from_utf8_lossy(&b.borrow()[*p.borrow()..]).into_owned();
        let lines: Vec<String> = text.lines().map(str::to_owned).collect();
        let idx = RefCell::new(0usize);
        Ok(vec![Val::native(move |_, _| {
            let mut i = idx.borrow_mut();
            let out = lines.get(*i).map_or(Val::Nil, |l| Val::str(l));
            *i += 1;
            Ok(vec![out])
        })])
    });
    let b = buf.clone();
    reg(&f, "write", move |_, a| {
        for v in a.iter().skip(1) {
            match v {
                Val::Str(s) => b.borrow_mut().extend_from_slice(s.as_bytes()),
                other => b.borrow_mut().extend(tostring(other).into_bytes()),
            }
        }
        Ok(vec![a.first().cloned().unwrap_or_default()])
    });
    reg(&f, "seek", |_, _| Ok(vec![Val::Num(0.0)]));
    reg(&f, "setvbuf", |_, _| Ok(Vec::new()));
    reg(&f, "flush", |_, _| Ok(Vec::new()));
    reg(&f, "close", move |_, _| {
        if writing {
            fs.write(&path, &buf.borrow());
        }
        Ok(vec![Val::Bool(true)])
    });
    f
}
