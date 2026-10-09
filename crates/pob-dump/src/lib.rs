//! Faithful dump of Path of Building's data, taken from PoB itself.
//!
//! PoB's own load sequence (`GameVersions`, `Modules/Common`,
//! `Modules/CalcFormat`, `Modules/Data`, as `Modules/Main.lua` runs them)
//! is executed under LuaJIT, and the resulting tables are serialized with a
//! canonical, lossless JSON encoding (see `FORMAT.md` next to this crate's
//! manifest). LuaJIT is only used here, at dump time.

use std::collections::{BTreeMap, HashMap};
use std::fmt::Write as _;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::Instant;

use anyhow::{anyhow, bail, Context, Result};
use mlua::{Function, Lua, Table, Value};
use serde_json::{json, Value as Json};
use sha2::{Digest, Sha256};

pub mod codegen;

const PRELUDE: &str = include_str!("lua/prelude.lua");
const WALKER: &str = include_str!("lua/walker.lua");
const BOOT: &str = include_str!("lua/boot.lua");

/// Version of the output format (bumped on incompatible encoding changes).
pub const FORMAT_VERSION: u32 = 1;

pub fn sha256_hex(bytes: &[u8]) -> String {
    let d = Sha256::digest(bytes);
    let mut s = String::with_capacity(64);
    for b in d {
        let _ = write!(s, "{b:02x}");
    }
    s
}

fn lerr(e: mlua::Error) -> anyhow::Error {
    anyhow!("{e}")
}

/// A Lua state rooted at PoB's `src/` directory with the dumper's prelude
/// and natives installed (but PoB not yet loaded).
pub struct Env {
    pub lua: Lua,
    pub root: PathBuf,
}

impl Env {
    pub fn new(pob_src: &Path) -> Result<Self> {
        let root = std::fs::canonicalize(pob_src)
            .with_context(|| format!("PoB src dir {}", pob_src.display()))?;
        let mut root_s = root.to_string_lossy().replace('\\', "/");
        if !root_s.ends_with('/') {
            root_s.push('/');
        }
        let lua = Lua::new();
        let g = lua.globals();
        g.set("__dump_root", root_s).map_err(lerr)?;
        g.set(
            "__dump_utf8_ok",
            lua.create_function(
                |_, s: mlua::String| Ok(std::str::from_utf8(&s.as_bytes()).is_ok()),
            )
            .map_err(lerr)?,
        )
        .map_err(lerr)?;
        // sha256 of a function's source text: lines first..=last of `file`
        // (as on disk, joined with "\n"); the whole file for a main chunk
        // (first == 0); or of `text` when there is no file.
        let cache: Arc<Mutex<HashMap<String, Arc<Vec<String>>>>> = Arc::default();
        g.set(
            "__dump_fn_hash",
            lua.create_function(
                move |_, (file, first, last, text): (String, i64, i64, Option<String>)| {
                    if file.is_empty() {
                        return Ok(sha256_hex(text.unwrap_or_default().as_bytes()));
                    }
                    let lines = {
                        let mut c = cache.lock().unwrap();
                        if let Some(l) = c.get(&file) {
                            l.clone()
                        } else {
                            let bytes = std::fs::read(&file).map_err(mlua::Error::external)?;
                            let text = String::from_utf8_lossy(&bytes);
                            let l: Arc<Vec<String>> = Arc::new(
                                text.split('\n')
                                    .map(|s| s.trim_end_matches('\r').to_owned())
                                    .collect(),
                            );
                            c.insert(file.clone(), l.clone());
                            l
                        }
                    };
                    let (a, b) = if first <= 0 {
                        (1, lines.len())
                    } else {
                        (first as usize, (last.max(first) as usize).min(lines.len()))
                    };
                    let body = lines[a.saturating_sub(1).min(lines.len())..b].join("\n");
                    Ok(sha256_hex(body.as_bytes()))
                },
            )
            .map_err(lerr)?,
        )
        .map_err(lerr)?;
        g.set(
            "__dump_getmetatable",
            lua.create_function(|_, v: Value| {
                Ok(match v {
                    Value::Table(t) => t.metatable(),
                    _ => None,
                })
            })
            .map_err(lerr)?,
        )
        .map_err(lerr)?;
        // (what, source, linedefined, lastlinedefined)
        g.set(
            "__dump_fninfo",
            lua.create_function(|_, f: Function| {
                let i = f.info();
                Ok((i.what, i.source, i.line_defined, i.last_line_defined))
            })
            .map_err(lerr)?,
        )
        .map_err(lerr)?;
        drop(g);
        lua.load(PRELUDE)
            .set_name("=pob-dump/prelude.lua")
            .exec()
            .map_err(lerr)?;
        Ok(Self { lua, root })
    }

    /// Run PoB's data load sequence (`lua/boot.lua`).
    pub fn boot_pob(&self) -> Result<()> {
        self.lua
            .load(BOOT)
            .set_name("=pob-dump/boot.lua")
            .exec()
            .map_err(|e| anyhow!("booting PoB data: {e}"))
    }

    pub fn walker(&self) -> Result<Table> {
        self.lua
            .load(WALKER)
            .set_name("=pob-dump/walker.lua")
            .eval()
            .map_err(lerr)
    }
}

#[derive(Debug, Clone)]
pub struct Options {
    pub pob_src: PathBuf,
    pub out: PathBuf,
    pub pretty: bool,
    /// Skip the (large, slow) tree dumps.
    pub skip_trees: bool,
}

#[derive(Debug, Default, Clone)]
pub struct FileInfo {
    pub name: String,
    /// Lua path(s) the file holds.
    pub roots: Vec<String>,
    pub bytes: u64,
    pub sha256: String,
}

#[derive(Debug, Default)]
pub struct Report {
    pub files: Vec<FileInfo>,
    pub families: BTreeMap<String, usize>,
    pub closures: usize,
    pub modcache_entries: usize,
    pub timings: Vec<(String, f64)>,
    pub skipped: Vec<(String, String)>,
    pub stats: BTreeMap<String, i64>,
}

impl Report {
    pub fn total_bytes(&self) -> u64 {
        self.files.iter().map(|f| f.bytes).sum()
    }
}

/// Family of a closure, from its canonical id.
pub fn closure_family(id: &str) -> String {
    let last = id
        .rsplit(['.', '['])
        .next()
        .unwrap_or("")
        .trim_end_matches(']')
        .trim_matches('"');
    if let Some(rest) = id.strip_prefix("data.skills") {
        let _ = rest;
        return format!("skill-hook:{last}");
    }
    if id.starts_with("data.minions") || id.starts_with("data.spectres") {
        return format!("minion-hook:{last}");
    }
    if id.starts_with("data.mapMods") {
        return format!("modmap:{last}");
    }
    if id.starts_with("data.powerStatList") {
        return "power-stat".into();
    }
    if id.starts_with("ConfigOptions") {
        return format!("config:{last}");
    }
    if id.starts_with("CalcSections") {
        return "calc-section".into();
    }
    if id.starts_with("global") {
        return "global-helper".into();
    }
    if id.starts_with("GameVersions") {
        return "game-versions".into();
    }
    if id.contains("<meta>") || id.starts_with("data.skillStatMapMeta") {
        return "data-metatable".into();
    }
    if id.starts_with("data.") {
        return "data-helper".into();
    }
    if id.starts_with("StatDescriptions") {
        return "stat-descriptions".into();
    }
    if id.starts_with("trees") || id.starts_with("treeSprites") || id.starts_with("treeAssets") {
        return "tree".into();
    }
    if id.starts_with("files") {
        return "extra-file".into();
    }
    "other".into()
}

struct Dumper<'a> {
    env: &'a Env,
    w: Table,
    opts: &'a Options,
    report: Report,
}

impl Dumper<'_> {
    fn assign(&self, v: &Value, path: &str) -> Result<()> {
        let f: Function = self.w.get("assign").map_err(lerr)?;
        f.call::<()>((v.clone(), path))
            .map_err(|e| anyhow!("assign {path}: {e}"))
    }

    fn encode(&self, v: &Value, path: &str) -> Result<mlua::String> {
        let f: Function = self.w.get("encode").map_err(lerr)?;
        f.call::<mlua::String>((v.clone(), path))
            .map_err(|e| anyhow!("encode {path}: {e}"))
    }

    fn write(&mut self, name: &str, roots: Vec<String>, bytes: &[u8]) -> Result<()> {
        let path = self.opts.out.join(name);
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir)?;
        }
        let mut data = bytes.to_vec();
        data.push(b'\n');
        std::fs::write(&path, &data).with_context(|| format!("write {}", path.display()))?;
        self.report.files.push(FileInfo {
            name: name.to_owned(),
            roots,
            bytes: data.len() as u64,
            sha256: sha256_hex(&data),
        });
        Ok(())
    }

    /// Assign + encode + write one root value.
    fn dump_root(&mut self, name: &str, v: &Value, path: &str) -> Result<()> {
        self.assign(v, path)?;
        let s = self.encode(v, path)?;
        self.write(name, vec![path.to_owned()], &s.as_bytes())
    }

    fn load_module(&self, rel: &str) -> Result<Value> {
        let f: Function = self.env.lua.globals().get("LoadModule").map_err(lerr)?;
        f.call::<Value>(rel)
            .map_err(|e| anyhow!("LoadModule({rel}): {e}"))
    }

    fn lua_path(&self, s: &str) -> Result<String> {
        let f: Function = self.w.get("strKeyPath").map_err(lerr)?;
        f.call::<String>(s).map_err(lerr)
    }

    fn timed<T>(&mut self, label: &str, f: impl FnOnce(&mut Self) -> Result<T>) -> Result<T> {
        let t = Instant::now();
        let r = f(self)?;
        let secs = t.elapsed().as_secs_f64();
        eprintln!("[pob-dump] {label}: {secs:.2}s");
        self.report.timings.push((label.to_owned(), secs));
        Ok(r)
    }

    fn dump_data(&mut self) -> Result<()> {
        let g = self.env.lua.globals();
        let data: Table = g.get("data").map_err(lerr)?;
        let misc_keys: Table = g.get("__dump_misc_keys").map_err(lerr)?;
        self.assign(&Value::Table(data.clone()), "data")?;
        let mut keys: Vec<String> = Vec::new();
        for pair in data.pairs::<Value, Value>() {
            let (k, _) = pair.map_err(lerr)?;
            match k {
                Value::String(s) => keys.push(s.to_str().map_err(lerr)?.to_owned()),
                other => bail!("non-string key in data: {other:?}"),
            }
        }
        keys.sort();
        let mut misc: Vec<(String, mlua::String)> = Vec::new();
        for k in &keys {
            let v: Value = data.raw_get(k.as_str()).map_err(lerr)?;
            let path = format!("data{}", self.lua_path(k)?);
            let is_misc = misc_keys.raw_get::<bool>(k.as_str()).unwrap_or(false)
                || !matches!(v, Value::Table(_));
            let s = self.encode(&v, &path)?;
            if is_misc {
                misc.push((k.clone(), s));
            } else {
                self.write(&format!("{k}.json"), vec![path], &s.as_bytes())?;
            }
        }
        // misc.json: {"<data key>": <encoded value>, ...}
        let mut buf: Vec<u8> = b"{".to_vec();
        let mut roots = Vec::new();
        for (i, (k, s)) in misc.iter().enumerate() {
            buf.extend_from_slice(if i == 0 { b"\n " } else { b",\n " });
            buf.extend_from_slice(serde_json::to_string(k)?.as_bytes());
            buf.push(b':');
            // re-indent nested lines by one space
            for (j, line) in s.as_bytes().split(|&b| b == b'\n').enumerate() {
                if j > 0 {
                    buf.extend_from_slice(b"\n ");
                }
                buf.extend_from_slice(line);
            }
            roots.push(format!("data{}", self.lua_path(k)?));
        }
        buf.extend_from_slice(b"\n}");
        self.write("misc.json", roots, &buf)?;
        Ok(())
    }

    fn dump_modcache(&mut self) -> Result<()> {
        let c = self.load_module("Data/ModCache")?;
        self.assign(&c, "ModCache")?;
        let f: Function = self.w.get("encodeModCache").map_err(lerr)?;
        let (s, n): (mlua::String, usize) = f.call((c, "ModCache")).map_err(lerr)?;
        self.report.modcache_entries = n;
        self.write("modCache.json", vec!["ModCache".into()], &s.as_bytes())
    }

    fn list_lua(dir: &Path) -> Result<Vec<String>> {
        let mut v: Vec<String> = std::fs::read_dir(dir)
            .with_context(|| format!("read_dir {}", dir.display()))?
            .filter_map(Result::ok)
            .filter_map(|e| {
                let n = e.file_name().to_string_lossy().into_owned();
                n.strip_suffix(".lua").map(str::to_owned)
            })
            .collect();
        v.sort();
        Ok(v)
    }

    fn dump_stat_descriptions(&mut self) -> Result<()> {
        // Stat descriptions, extra files and trees share nothing with `data`:
        // start a new ref group (lets the GC reclaim what is done).
        self.w
            .get::<Function>("resetCanon")
            .map_err(lerr)?
            .call::<()>(())
            .map_err(lerr)?;
        let dir = self.env.root.join("Data/StatDescriptions");
        for name in Self::list_lua(&dir)? {
            let v = self.load_module(&format!("Data/StatDescriptions/{name}"))?;
            let path = format!("StatDescriptions{}", self.lua_path(&name)?);
            self.dump_root(&format!("statDescriptions/{name}.json"), &v, &path)?;
        }
        Ok(())
    }

    /// Data/**/*.lua files PoB's data load never touched (plus a few loaded
    /// only into locals), dumped standalone as `files["Data/..."]`.
    fn dump_extra_files(&mut self) -> Result<()> {
        let loaded: Table = self.env.lua.globals().get("__dump_loaded").map_err(lerr)?;
        let data_dir = self.env.root.join("Data");
        let mut all = Vec::new();
        walk_files(&data_dir, &mut all)?;
        all.sort();
        // Loaded by PoB's data sequence but kept only in locals / module
        // caches (not reachable from `data`).
        let forced = ["Data/PearlSupports.lua"];
        for file in all {
            let rel = file
                .strip_prefix(&self.env.root)
                .unwrap_or(&file)
                .to_string_lossy()
                .replace('\\', "/");
            if !file.extension().is_some_and(|e| e == "lua")
                || rel.starts_with("Data/StatDescriptions/")
            {
                continue;
            }
            let abs = file.to_string_lossy().replace('\\', "/");
            let was_loaded = loaded.raw_get::<bool>(abs.as_str()).unwrap_or(false);
            if was_loaded && !forced.contains(&rel.as_str()) {
                continue;
            }
            let v = match self.load_module(&rel) {
                Ok(v) => v,
                Err(e) => {
                    self.report
                        .skipped
                        .push((rel.clone(), format!("load error: {e}")));
                    continue;
                }
            };
            if !matches!(v, Value::Table(_)) {
                self.report
                    .skipped
                    .push((rel.clone(), format!("returns {}", v.type_name())));
                continue;
            }
            let path = format!("files[{}]", serde_json::to_string(&rel)?);
            let stem = rel
                .trim_start_matches("Data/")
                .trim_end_matches(".lua")
                .replace('/', ".");
            self.dump_root(&format!("files/{stem}.json"), &v, &path)?;
        }
        Ok(())
    }

    fn dump_trees(&mut self) -> Result<()> {
        let tdir = self.env.root.join("TreeData");
        let mut vers: Vec<String> = std::fs::read_dir(&tdir)?
            .filter_map(Result::ok)
            .filter(|e| e.path().is_dir())
            .map(|e| e.file_name().to_string_lossy().into_owned())
            .collect();
        vers.sort();
        let reset: Function = self.w.get("resetCanon").map_err(lerr)?;
        // Trees are last and need nothing from PoB's loaded data: release it
        // so the GC does not keep re-traversing it while trees churn.
        {
            let g = self.env.lua.globals();
            g.set("data", Value::Nil).map_err(lerr)?;
            g.set("__dump_roots", Value::Nil).map_err(lerr)?;
            let loaded: Table = g
                .get::<Table>("package")
                .and_then(|p| p.get("loaded"))
                .map_err(lerr)?;
            let data_mods: Vec<String> = loaded
                .pairs::<String, Value>()
                .filter_map(Result::ok)
                .map(|(k, _)| k)
                .filter(|k| k.starts_with("Data.") || k.starts_with("Modules."))
                .collect();
            for k in data_mods {
                loaded.set(k, Value::Nil).map_err(lerr)?;
            }
            reset.call::<()>(()).map_err(lerr)?;
            self.env.lua.gc_collect().map_err(lerr)?;
        }
        for ver in vers {
            // Trees share nothing with `data` or with each other.
            reset.call::<()>(()).map_err(lerr)?;
            for (file, suffix, root) in [
                ("tree.lua", "", "trees"),
                ("sprites.lua", ".sprites", "treeSprites"),
                ("Assets.lua", ".assets", "treeAssets"),
                ("tree-legion.lua", "", "trees"),
            ] {
                let rel = format!("TreeData/{ver}/{file}");
                if !self.env.root.join(&rel).exists() {
                    continue;
                }
                let v = self.load_module(&rel)?;
                let path = format!("{root}[{}]", serde_json::to_string(&ver)?);
                self.dump_root(&format!("trees/{ver}{suffix}.json"), &v, &path)?;
            }
        }
        Ok(())
    }

    fn write_closures(&mut self) -> Result<()> {
        let f: Function = self.w.get("manifest").map_err(lerr)?;
        let list: Table = f.call(()).map_err(lerr)?;
        let mut entries: Vec<Json> = Vec::new();
        let mut families: BTreeMap<String, usize> = BTreeMap::new();
        for rec in list.sequence_values::<Table>() {
            let rec = rec.map_err(lerr)?;
            let id: String = rec.get("id").map_err(lerr)?;
            let fam = closure_family(&id);
            *families.entry(fam.clone()).or_default() += 1;
            let aliases: Vec<String> = rec
                .get::<Table>("aliases")
                .map_err(lerr)?
                .sequence_values::<String>()
                .collect::<mlua::Result<_>>()
                .map_err(lerr)?;
            entries.push(json!({
                "id": id,
                "family": fam,
                "src": rec.get::<String>("src").map_err(lerr)?,
                "hash": rec.get::<String>("hash").map_err(lerr)?,
                "what": rec.get::<String>("what").map_err(lerr)?,
                "aliases": aliases,
            }));
        }
        entries.sort_by(|a, b| a["id"].as_str().cmp(&b["id"].as_str()));
        self.report.closures = entries.len();
        self.report.families = families.clone();
        let doc = json!({ "count": entries.len(), "families": families, "closures": entries });
        let s = serde_json::to_string_pretty(&doc)?;
        self.write("closures.json", vec![], s.as_bytes())
    }

    fn collect_stats(&mut self) -> Result<()> {
        let stats: Table = self.w.get("stats").map_err(lerr)?;
        for pair in stats.pairs::<String, i64>() {
            let (k, v) = pair.map_err(lerr)?;
            self.report.stats.insert(k, v);
        }
        Ok(())
    }
}

fn walk_files(dir: &Path, out: &mut Vec<PathBuf>) -> Result<()> {
    for e in std::fs::read_dir(dir)? {
        let p = e?.path();
        if p.is_dir() {
            walk_files(&p, out)?;
        } else {
            out.push(p);
        }
    }
    Ok(())
}

/// Delete the files a previous dump listed in `out/manifest.json` (so keys
/// that disappeared upstream do not linger). Nothing else is touched.
fn remove_previous_dump(out: &Path) -> Result<()> {
    let Ok(text) = std::fs::read_to_string(out.join("manifest.json")) else {
        return Ok(());
    };
    let Ok(old) = serde_json::from_str::<Json>(&text) else {
        return Ok(());
    };
    for f in old["files"].as_array().into_iter().flatten() {
        if let Some(name) = f["file"].as_str() {
            if !name.contains("..") && !name.starts_with('/') {
                let _ = std::fs::remove_file(out.join(name));
            }
        }
    }
    Ok(())
}

pub fn pob_version_of(src: &Path) -> Option<String> {
    let text = std::fs::read_to_string(src.join("../manifest.xml")).ok()?;
    let i = text.find("<Version number=\"")? + "<Version number=\"".len();
    let j = text[i..].find('"')?;
    Some(text[i..i + j].to_owned())
}

/// Run the whole dump into `opts.out`.
pub fn dump(opts: &Options) -> Result<Report> {
    let t0 = Instant::now();
    std::fs::create_dir_all(&opts.out)?;
    remove_previous_dump(&opts.out)?;
    let env = Env::new(&opts.pob_src)?;
    let t = Instant::now();
    env.boot_pob()?;
    let boot_secs = t.elapsed().as_secs_f64();
    eprintln!("[pob-dump] boot PoB data: {boot_secs:.2}s");
    let w = env.walker()?;
    w.set("pretty", opts.pretty).map_err(lerr)?;
    let mut d = Dumper {
        env: &env,
        w,
        opts,
        report: Report::default(),
    };
    d.report.timings.push(("boot".into(), boot_secs));

    let roots: Table = env.lua.globals().get("__dump_roots").map_err(lerr)?;
    d.timed("global + GameVersions", |d| {
        let gv: Value = roots.get("global").map_err(lerr)?;
        d.dump_root("global.json", &gv, "global")?;
        let gv: Value = roots.get("GameVersions").map_err(lerr)?;
        d.dump_root("gameVersions.json", &gv, "GameVersions")
    })?;
    d.timed("data", Dumper::dump_data)?;
    d.timed("CalcSections + ConfigOptions", |d| {
        let v = d.load_module("Modules/CalcSections")?;
        d.dump_root("calcSections.json", &v, "CalcSections")?;
        let v = d.load_module("Modules/ConfigOptions")?;
        d.dump_root("configOptions.json", &v, "ConfigOptions")
    })?;
    d.timed("ModCache", Dumper::dump_modcache)?;
    d.timed("StatDescriptions", Dumper::dump_stat_descriptions)?;
    d.timed("extra Data files", Dumper::dump_extra_files)?;
    if !opts.skip_trees {
        d.timed("trees", Dumper::dump_trees)?;
    }
    d.write_closures()?;
    d.collect_stats()?;

    let total = t0.elapsed().as_secs_f64();
    d.report.timings.push(("total".into(), total));
    let mut report = d.report;
    let files: Vec<Json> = report
        .files
        .iter()
        .map(|f| json!({ "file": f.name, "roots": f.roots, "bytes": f.bytes, "sha256": f.sha256 }))
        .collect();
    let manifest = json!({
        "format": FORMAT_VERSION,
        "pobVersion": pob_version_of(&env.root),
        "pobSrc": env.root.to_string_lossy(),
        "files": files,
        "totalBytes": report.total_bytes(),
        "closures": report.closures,
        "closureFamilies": report.families,
        "modCacheEntries": report.modcache_entries,
        "stats": report.stats,
        "skipped": report.skipped.iter().map(|(f, why)| json!({"file": f, "reason": why})).collect::<Vec<_>>(),
        "timingsSecs": report.timings.iter().map(|(k, v)| json!([k, (v * 1000.0).round() / 1000.0])).collect::<Vec<_>>(),
    });
    let s = serde_json::to_string_pretty(&manifest)?;
    std::fs::write(opts.out.join("manifest.json"), s + "\n")?;
    report.files.push(FileInfo {
        name: "manifest.json".into(),
        ..Default::default()
    });
    Ok(report)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Encode the value of a Lua expression with the walker.
    fn enc(env: &Env, w: &Table, expr: &str) -> String {
        let v: Value = env.lua.load(format!("return {expr}")).eval().unwrap();
        let assign: Function = w.get("assign").unwrap();
        assign.call::<()>((v.clone(), "root")).unwrap();
        let encode: Function = w.get("encode").unwrap();
        encode.call::<String>((v, "root")).unwrap()
    }

    #[test]
    fn canonical_encoding() {
        let env = Env::new(Path::new(env!("CARGO_MANIFEST_DIR"))).unwrap();
        let w = env.walker().unwrap();
        w.set("pretty", false).unwrap();
        let e = |s: &str| enc(&env, &w, s);
        assert_eq!(e("{}"), "{}");
        assert_eq!(e("{1, 2.5, 'x', true}"), r#"[1,2.5,"x",true]"#);
        assert_eq!(e("{b = 1, a = 2}"), r#"{"a":2,"b":1}"#);
        assert_eq!(
            e("{[2] = true, [8] = true}"),
            r#"{"$num":{"2":true,"8":true}}"#
        );
        assert_eq!(
            e("{10, 20, [4] = 40, [-1] = 0, [0.5] = 1}"),
            r#"{"$seq":[10,20],"$num":{"-1":0,"0.5":1,"4":40}}"#
        );
        assert_eq!(
            e("{ name = 'Damage', type = 'INC', value = 10, { type = 'Condition', var = 'X' } }"),
            r#"{"name":"Damage","type":"INC","value":10,"$seq":[{"type":"Condition","var":"X"}]}"#
        );
        assert_eq!(e("{['$x'] = 1}"), r#"{"$$x":1}"#);
        assert_eq!(e("{[true] = 1}"), r#"{"$keys":[[true,1]]}"#);
        assert_eq!(
            e("{0.1, 1/3, 2^53, -2^53 + 1, 1e300}"),
            "[0.1,0.3333333333333333,9007199254740992,-9007199254740991,1e+300]"
        );
        assert_eq!(
            e("{1/0, -1/0, 0/0, 1/(-1/0)}"),
            r#"[{"$float":"inf"},{"$float":"-inf"},{"$float":"nan"},{"$float":"-0"}]"#
        );
        assert_eq!(
            e("{'\\255\\254', 'a\"\\n'}"),
            r#"[{"$bytes":"fffe"},"a\"\n"]"#
        );
        // shared tables and cycles: BFS-shortest canonical path, $ref elsewhere
        assert_eq!(
            e("(function() local s = {v = 1}; local t = {z = {inner = s}, a = s}; t.self = t; return t end)()"),
            r#"{"a":{"v":1},"self":{"$ref":"root"},"z":{"inner":{"$ref":"root.a"}}}"#
        );
        // metatables
        assert_eq!(
            e("setmetatable({1}, {__metatable = false, k = 'm'})"),
            r#"{"$seq":[1],"$meta":{"__metatable":false,"k":"m"}}"#
        );
        // non-identifier keys use ["..."] in paths
        assert_eq!(
            e("(function() local s = {}; return {['a b'] = s, c = {s}} end)()"),
            r#"{"a b":{},"c":[{"$ref":"root[\"a b\"]"}]}"#
        );
        // functions: id = canonical path, same object at every alias
        let s = e("(function() local f = function() end; return {g = f, h = {f}} end)()");
        let j: Json = serde_json::from_str(&s).unwrap();
        assert_eq!(j["g"]["$fn"], "root.g");
        assert_eq!(j["g"], j["h"][0]);
        assert_eq!(j["g"]["hash"].as_str().unwrap().len(), 64);
    }

    #[test]
    fn families() {
        assert_eq!(
            closure_family("data.skills.Fireball.preDamageFunc"),
            "skill-hook:preDamageFunc"
        );
        assert_eq!(closure_family("ConfigOptions[12].apply"), "config:apply");
        assert_eq!(
            closure_family("data.mapMods.Prefix[3].apply"),
            "modmap:apply"
        );
        assert_eq!(closure_family("data.costs<meta>.__index"), "data-metatable");
    }
}
