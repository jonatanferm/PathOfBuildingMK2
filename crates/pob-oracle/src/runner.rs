//! Running the patched PoB (calc-architecture.md §9.2 "Process model").
//!
//! * [`probe_in_process`]: boot PoB from a patched copy ([`crate::patcher`])
//!   with the probe runtime (`lua/state_dump.lua`, `lua/provenance.lua`,
//!   `lua/probe.lua`), load one build, and write `outputs.json`,
//!   `hits.json`, `provenance.json` and `state/<hit>.json` into a directory.
//!   One build per process (PoB leaks state between builds).
//! * [`run_probe_process`]: the same in a fresh `pob_oracle probe` process.
//! * [`OracleLock`]: the machine-wide oracle lock (`flock` on
//!   `/tmp/pob-port-oracle.lock`, shared with `scripts/port/oracle.sh`), so
//!   only one LuaJIT PoB (≈400 MB) runs at a time.
//! * [`run_nondet`]: §9.5, two (or three) processes per build and the
//!   comparison of their outputs and dumps ([`crate::nondet`]).
//! * [`ProbeCache`]: the probe cache, `.parity/probe-cache/<sha>/<build-blake3>/<hit>.json.zst`.

use std::path::{Path, PathBuf};

use crate::nondet::{compare_runs, NondetRecord};
use crate::oracle::{OracleError, Result};
use crate::schema::{read_text, StateDump};

pub const STATE_DUMP_LUA: &str = include_str!("../lua/state_dump.lua");
pub const PROVENANCE_LUA: &str = include_str!("../lua/provenance.lua");
pub const PROBE_LUA: &str = include_str!("../lua/probe.lua");

/// Hits dumped by default (an env-wide dump each): in the build's final
/// main env (`calcsTab.mainEnv`, whose outputs are `outputs.json`) the
/// first `:in` / `:out` of the env-level function probes and the first call
/// of every skill hook; the process's first hit of the probes that run
/// without an env (config applies, `ConfigTab:BuildModList`, jewel funcs,
/// `createActiveSkill`, `getCachedOutputValue`). `pob_oracle probe` adds
/// `<id>@main` for every stages.toml line probe.
pub const DEFAULT_SELECT: &[&str] = &[
    "initEnv:out@main",
    "perform:in@main",
    "perform:out@main",
    "defence:in@main",
    "defence:out@main",
    "buildDefenceEstimations:in@main",
    "offence:in@main",
    "offence:out@main",
    "triggers:in@main",
    "mirages:in@main",
    "buildActiveSkillModList:in@main",
    "buildActiveSkillModList:out@main",
    "runSkillFunc:in@main",
    "skill:*:in@main",
    "createActiveSkill:out#1",
    "getCachedOutputValue:in#1",
    "ConfigTab:BuildModList:out#1",
    "jewelFunc:in#1",
    "config:*:apply:in#1",
];

#[derive(Debug, Clone)]
pub struct ProbeOptions {
    /// Hit globs (`perform:in#1`, `offence:*`, `*#1`); a glob without `#`
    /// matches every hit of the ids it matches.
    pub select: Vec<String>,
    /// Stop dumping after this many dumps (later selected hits are counted
    /// as skipped).
    pub max_dumps: usize,
    /// Proxy outputs / record AddMod callers.
    pub provenance: bool,
}

impl Default for ProbeOptions {
    fn default() -> Self {
        Self {
            select: DEFAULT_SELECT.iter().map(|s| (*s).to_owned()).collect(),
            max_dumps: 200,
            provenance: true,
        }
    }
}

impl ProbeOptions {
    /// The `pob_oracle probe` flags for these options.
    #[must_use]
    pub fn args(&self) -> Vec<String> {
        let mut a = vec![
            "--select".to_owned(),
            self.select.join(","),
            "--max-dumps".to_owned(),
            self.max_dumps.to_string(),
        ];
        if !self.provenance {
            a.push("--no-provenance".to_owned());
        }
        a
    }
}

fn lua_str(s: &str) -> String {
    let mut out = String::from("\"");
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

/// File name of a hit's dump (`lua/probe.lua` `fileName`).
#[must_use]
pub fn hit_file_name(hit: &str) -> String {
    let s: String = hit
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | ':' | '#' | '@' | '-') {
                c
            } else {
                '_'
            }
        })
        .collect();
    format!("{s}.json")
}

/// The Lua run before PoB boots: quiet print, `__probe_cfg`, the encoder,
/// provenance (when on) and the probe runtime.
#[must_use]
pub fn probe_prelude(src_root: &str, out_dir: &Path, build: &str, opts: &ProbeOptions) -> String {
    let select: Vec<String> = opts.select.iter().map(|s| lua_str(s)).collect();
    format!(
        // each file in its own block: its locals stay its own
        "{quiet}\n__probe_cfg = {{ outDir = {out}, build = {build}, root = {root}, select = {{ {sel} }}, maxDumps = {max}, provenance = {prov} }}\ndo\n{dump}\nend\ndo\n{provlua}\nend\ndo\n{probe}\nend\n",
        quiet = crate::oracle::QUIET_PRELUDE,
        out = lua_str(&out_dir.to_string_lossy()),
        build = lua_str(build),
        root = lua_str(src_root),
        sel = select.join(", "),
        max = opts.max_dumps,
        prov = opts.provenance,
        dump = STATE_DUMP_LUA,
        provlua = if opts.provenance { PROVENANCE_LUA } else { "" },
        probe = PROBE_LUA,
    )
}

/// What a probe run wrote.
#[derive(Debug, Clone)]
pub struct ProbeSummary {
    pub out_dir: PathBuf,
    pub dumps: usize,
}

/// Boot the patched PoB at `src` (a patched copy's `src/`), load `build`
/// and write the run into `out_dir`. Call once per process.
#[cfg(all(not(target_arch = "wasm32"), feature = "luajit"))]
pub fn probe_in_process(
    src: &Path,
    build: &Path,
    out_dir: &Path,
    opts: &ProbeOptions,
) -> Result<ProbeSummary> {
    use crate::headless::Engine;
    let e = |m: String| OracleError(m);
    let xml = std::fs::read_to_string(build).map_err(|x| e(format!("{}: {x}", build.display())))?;
    let name = build
        .file_stem()
        .map_or_else(|| "build".to_owned(), |s| s.to_string_lossy().into_owned());
    std::fs::create_dir_all(out_dir.join("state"))
        .map_err(|x| e(format!("{}: {x}", out_dir.display())))?;
    let out_dir =
        std::fs::canonicalize(out_dir).map_err(|x| e(format!("{}: {x}", out_dir.display())))?;
    let root = std::fs::canonicalize(src).map_err(|x| e(format!("{}: {x}", src.display())))?;
    let mut root_s = root.to_string_lossy().replace('\\', "/");
    if !root_s.ends_with('/') {
        root_s.push('/');
    }
    let pre = probe_prelude(&root_s, &out_dir, &name, opts);
    let mut pob = crate::jit::JitPob::boot_oracle(&root, Some(&pre))
        .map_err(|x| e(format!("PoB boot failed: {x}")))?;
    pob.run(crate::oracle::ORACLE_LUA)
        .map_err(|x| e(format!("oracle.lua: {x}")))?;
    pob.load_build_xml(&xml, &name)
        .map_err(|x| e(format!("{}: {x}", build.display())))?;
    let outputs = pob
        .run("return __oracle_outputs()")
        .map_err(|x| e(format!("outputs: {x}")))?;
    let text = outputs
        .first()
        .map(|r| r.text().to_owned())
        .unwrap_or_default();
    write_atomic(&out_dir.join("outputs.json"), &text)?;
    let n = pob
        .run("return __probe_finish(__mainObject__.main.modes['BUILD'].calcsTab.mainEnv)")
        .map_err(|x| e(format!("probe finish: {x}")))?;
    let dumps = n
        .first()
        .and_then(|r| r.text().parse::<usize>().ok())
        .unwrap_or(0);
    Ok(ProbeSummary { out_dir, dumps })
}

/// Write-then-rename.
pub fn write_atomic(path: &Path, s: &str) -> Result<()> {
    if let Some(d) = path.parent().filter(|d| !d.as_os_str().is_empty()) {
        std::fs::create_dir_all(d).map_err(|e| OracleError(format!("{}: {e}", d.display())))?;
    }
    let tmp = path.with_extension("part");
    std::fs::write(&tmp, s).map_err(|e| OracleError(format!("{}: {e}", tmp.display())))?;
    std::fs::rename(&tmp, path).map_err(|e| OracleError(format!("{}: {e}", path.display())))
}

// ---- the machine-wide lock ------------------------------------------------------------------------

/// Set in a child's environment when its parent holds the lock.
pub const LOCK_HELD_ENV: &str = "POB_ORACLE_LOCK_HELD";

/// `$POB_ORACLE_LOCK`, else `/tmp/pob-port-oracle.lock` (machine-wide, not
/// per-user TMPDIR, so every worktree and agent shares it).
#[must_use]
pub fn lock_path() -> PathBuf {
    if let Some(p) = std::env::var_os("POB_ORACLE_LOCK") {
        return PathBuf::from(p);
    }
    if cfg!(unix) {
        PathBuf::from("/tmp/pob-port-oracle.lock")
    } else {
        std::env::temp_dir().join("pob-port-oracle.lock")
    }
}

/// An exclusive `flock` held for the lifetime of the value (none when the
/// parent process already holds it).
#[derive(Debug)]
pub struct OracleLock {
    _file: Option<std::fs::File>,
}

/// Block until this process holds an exclusive `flock` on `file`
/// (`flock(1)` in oracle.sh takes the same lock; released on close).
#[cfg(unix)]
fn lock_exclusive(file: &std::fs::File) -> std::io::Result<()> {
    loop {
        match rustix::fs::flock(file, rustix::fs::FlockOperation::LockExclusive) {
            Err(e) if e == rustix::io::Errno::INTR => {}
            r => return r.map_err(std::io::Error::from),
        }
    }
}

#[cfg(not(unix))]
#[allow(clippy::incompatible_msrv)]
fn lock_exclusive(file: &std::fs::File) -> std::io::Result<()> {
    file.lock()
}

impl OracleLock {
    pub fn acquire() -> Result<Self> {
        if std::env::var_os(LOCK_HELD_ENV).is_some_and(|v| v == "1") {
            return Ok(Self { _file: None });
        }
        let path = lock_path();
        let file = std::fs::OpenOptions::new()
            .create(true)
            .truncate(false)
            .write(true)
            .open(&path)
            .map_err(|e| OracleError(format!("{}: {e}", path.display())))?;
        lock_exclusive(&file)
            .map_err(|e| OracleError(format!("locking {}: {e}", path.display())))?;
        Ok(Self { _file: Some(file) })
    }
}

// ---- subprocess runs -------------------------------------------------------------------------------

/// Run `pob_oracle probe` in a fresh process (its stderr goes to
/// `<out_dir>/log.txt`).
pub fn run_probe_process(
    bin: &Path,
    build: &Path,
    out_dir: &Path,
    patched_src: &Path,
    opts: &ProbeOptions,
) -> Result<()> {
    std::fs::create_dir_all(out_dir)
        .map_err(|e| OracleError(format!("{}: {e}", out_dir.display())))?;
    let log = std::fs::File::create(out_dir.join("log.txt"))
        .map_err(|e| OracleError(format!("{}: {e}", out_dir.display())))?;
    let status = std::process::Command::new(bin)
        .arg("probe")
        .arg(build)
        .arg("--out-dir")
        .arg(out_dir)
        .arg("--patched")
        .arg(patched_src)
        .args(opts.args())
        .stdout(std::process::Stdio::null())
        .stderr(log)
        .status()
        .map_err(|e| OracleError(format!("running {}: {e}", bin.display())))?;
    if status.success() {
        Ok(())
    } else {
        let tail = read_text(&out_dir.join("log.txt")).unwrap_or_default();
        let tail: Vec<&str> = tail.lines().rev().take(5).collect();
        Err(OracleError(format!(
            "pob_oracle probe {} failed ({status}): {}",
            build.display(),
            tail.into_iter().rev().collect::<Vec<_>>().join(" | ")
        )))
    }
}

/// Result of [`run_nondet`].
#[derive(Debug, Clone)]
pub struct NondetOutcome {
    /// Everything recorded (all runs).
    pub record: NondetRecord,
    /// Runs 1 and 2 only.
    pub pair: NondetRecord,
    /// Differences between runs 1 and 3 that runs 1 and 2 did not show
    /// (only when a third run happened).
    pub uncovered: Vec<String>,
    /// The same, comparing paths with list indices generalised
    /// ([`crate::nondet::generalize`]).
    pub uncovered_structural: Vec<String>,
    /// What the last run added to the record (structurally); empty when the
    /// record converged within `max_runs`.
    pub last_new: Vec<String>,
    pub runs: usize,
}

/// §9.5: probe `build` in two processes; when they differ (or always with
/// `verify`) run a third, and keep running (up to `max_runs`, at least 3)
/// while a run still shows a difference the record does not cover
/// structurally. Every run is compared with run 1 and merged into the
/// record. Run directories are `<work>/run<k>`; run 1 is left in place.
pub fn run_nondet(
    bin: &Path,
    build: &Path,
    patched_src: &Path,
    opts: &ProbeOptions,
    work: &Path,
    verify: bool,
    max_runs: usize,
) -> Result<NondetOutcome> {
    let stem = build
        .file_stem()
        .map_or_else(|| "build".to_owned(), |s| s.to_string_lossy().into_owned());
    let max_runs = max_runs.max(3);
    let dir = |k: usize| work.join(format!("run{k}"));
    for k in 1..=max_runs {
        let _ = std::fs::remove_dir_all(dir(k));
    }
    run_probe_process(bin, build, &dir(1), patched_src, opts)?;
    run_probe_process(bin, build, &dir(2), patched_src, opts)?;
    let pair = compare_runs(&stem, &[&dir(1), &dir(2)])?;
    let _ = std::fs::remove_dir_all(dir(2));
    let mut out = NondetOutcome {
        record: pair.clone(),
        pair,
        uncovered: Vec::new(),
        uncovered_structural: Vec::new(),
        last_new: Vec::new(),
        runs: 2,
    };
    if out.pair.is_empty() && !verify {
        return Ok(out);
    }
    for k in 3..=max_runs {
        run_probe_process(bin, build, &dir(k), patched_src, opts)?;
        let rec = compare_runs(&stem, &[&dir(1), &dir(k)])?;
        let _ = std::fs::remove_dir_all(dir(k));
        if k == 3 {
            out.uncovered = out.pair.uncovered(&rec, false);
            out.uncovered_structural = out.pair.uncovered(&rec, true);
        }
        out.last_new = out.record.uncovered(&rec, true);
        out.record.merge(&rec);
        out.runs = k;
        out.record.runs = k;
        if out.last_new.is_empty() {
            break;
        }
    }
    Ok(out)
}

// ---- the probe cache --------------------------------------------------------------------------------

/// `<parity>/probe-cache/<sha>/<build-key>/`: zstd-compressed dumps of one
/// build under one patched source.
#[derive(Debug, Clone)]
pub struct ProbeCache {
    pub dir: PathBuf,
}

/// The cache key of a build file: the first 16 hex digits of the BLAKE3 of
/// its bytes.
pub fn build_key(build: &Path) -> Result<String> {
    let bytes =
        std::fs::read(build).map_err(|e| OracleError(format!("{}: {e}", build.display())))?;
    Ok(blake3::hash(&bytes).to_hex()[..16].to_owned())
}

impl ProbeCache {
    pub fn new(parity: &Path, sha: &str, build: &Path) -> Result<Self> {
        Ok(Self {
            dir: parity.join("probe-cache").join(sha).join(build_key(build)?),
        })
    }

    #[must_use]
    pub fn state_path(&self, hit: &str) -> PathBuf {
        self.dir.join(format!("{}.zst", hit_file_name(hit)))
    }

    /// The cached dump of `hit`, if present.
    pub fn get(&self, hit: &str) -> Result<Option<StateDump>> {
        let p = self.state_path(hit);
        if !p.is_file() {
            return Ok(None);
        }
        StateDump::load(&p).map(Some)
    }

    /// Compress a probe run's files into the cache; returns the number of
    /// state dumps stored.
    pub fn store_run(&self, run_dir: &Path) -> Result<usize> {
        std::fs::create_dir_all(&self.dir)
            .map_err(|e| OracleError(format!("{}: {e}", self.dir.display())))?;
        let put = |src: &Path, name: &str| -> Result<()> {
            let raw =
                std::fs::read(src).map_err(|e| OracleError(format!("{}: {e}", src.display())))?;
            let z = zstd::encode_all(&raw[..], 9).map_err(|e| OracleError(e.to_string()))?;
            let dst = self.dir.join(format!("{name}.zst"));
            let tmp = self.dir.join(format!("{name}.zst.part"));
            std::fs::write(&tmp, z).map_err(|e| OracleError(format!("{}: {e}", tmp.display())))?;
            std::fs::rename(&tmp, &dst).map_err(|e| OracleError(format!("{}: {e}", dst.display())))
        };
        for f in ["outputs.json", "hits.json", "provenance.json"] {
            let p = run_dir.join(f);
            if p.is_file() {
                put(&p, f)?;
            }
        }
        let mut n = 0;
        if let Ok(rd) = std::fs::read_dir(run_dir.join("state")) {
            for e in rd.filter_map(std::result::Result::ok) {
                let name = e.file_name().to_string_lossy().into_owned();
                if name.ends_with(".json") {
                    put(&e.path(), &name)?;
                    n += 1;
                }
            }
        }
        Ok(n)
    }
}

#[cfg(all(test, feature = "luajit"))]
mod tests {
    use super::*;

    /// The prelude loads in a bare LuaJIT state and defines the probe API.
    #[test]
    fn prelude_defines_the_probe_api() {
        let lua = mlua::Lua::new();
        let pre = probe_prelude(
            "/x/src/",
            Path::new("/tmp/none"),
            "b",
            &ProbeOptions::default(),
        );
        lua.load(&pre).set_name("pre").exec().unwrap();
        let t: String = lua
            .load("return type(__PROBE_FN) .. type(__PROBE) .. type(__PROV) .. type(__PROV_MOD) .. type(__state_dump.encodeDoc)")
            .eval()
            .unwrap();
        assert_eq!(t, "functionfunctionfunctionfunctionfunction");
        // a wrapped function keeps its results and errors
        let r: String = lua
            .load(
                r##"local f = __PROBE_FN("t", {"a"}, function(a, ...) return a + 1, select("#", ...) end)
                   local x, n = f(1, nil, nil)
                   local ok, e = pcall(__PROBE_FN("u", {}, function() error({ tag = 1 }) end))
                   return x .. "," .. n .. "," .. tostring(ok) .. "," .. tostring(type(e) == "table" and e.tag)"##,
            )
            .eval()
            .unwrap();
        assert_eq!(r, "2,2,false,1");
    }
}

// ---- the shared caches of the oracle tests (HYG3, test audit S1/S3/S4) ---------------------------
//
// Every entry lives in the probe cache, `<parity>/probe-cache/<sha>/<input-key>/…`, keyed like the
// dumps above: `<sha>` is the PoB source the run boots (a patched copy's manifest sha, or
// [`source_key`] for the unpatched checkout), `<input-key>` the first 16 hex digits of the BLAKE3 of
// the run's input (a build's XML bytes: [`build_key`]), and the last component hashes whatever else
// decides the result (the query scripts, the probe selection). An entry is written to a private
// temp name and renamed into place, so a reader sees it whole or not at all, and concurrent writers
// of one key (other tests, other worktrees) each publish a valid copy.
//
// `$POB_QUERY_CACHE`: unset / `on` reads and writes; `refresh` runs PoB again and overwrites;
// `off` neither reads nor writes (every query and probe runs, as before HYG3).

/// What `$POB_QUERY_CACHE` allows.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CacheMode {
    On,
    Refresh,
    Off,
}

#[must_use]
pub fn cache_mode() -> CacheMode {
    match std::env::var("POB_QUERY_CACHE").as_deref() {
        Ok("off" | "0") => CacheMode::Off,
        Ok("refresh") => CacheMode::Refresh,
        _ => CacheMode::On,
    }
}

/// The first 16 hex digits of the BLAKE3 of `parts` (each length-prefixed, so part boundaries
/// count).
#[must_use]
pub fn content_key(domain: &str, parts: &[&[u8]]) -> String {
    let mut h = blake3::Hasher::new();
    h.update(domain.as_bytes());
    h.update(&[0]);
    for p in parts {
        h.update(&(p.len() as u64).to_le_bytes());
        h.update(p);
    }
    h.finalize().to_hex()[..16].to_owned()
}

/// The Rust and Lua code an in-process oracle run boots PoB with (`JitPob::boot_oracle`, the
/// headless loader, `oracle.lua`): part of every [`source_key`], so a change to any of it
/// invalidates the cached query results.
const ORACLE_RUNTIME: &[&str] = &[
    include_str!("jit.rs"),
    include_str!("headless.rs"),
    include_str!("oracle.rs"),
    include_str!("oracle.lua"),
];

/// Bumped to retire every cached oracle result (query and selection entries) when something the
/// keys do not see changes what PoB returns.
pub const ORACLE_CACHE_EPOCH: u32 = 1;

/// The Lua runtime the oracle runs PoB on, as `Cargo.lock` pins it (`mlua`, `mlua-sys`,
/// `luajit-src` versions) plus [`ORACLE_CACHE_EPOCH`]: part of [`source_key`] and
/// `patcher::plan_sha`, so a LuaJIT or binding bump invalidates the cached results.
#[must_use]
pub fn lua_runtime_versions() -> &'static str {
    static V: std::sync::OnceLock<String> = std::sync::OnceLock::new();
    V.get_or_init(|| {
        const LOCK: &str = include_str!("../../../Cargo.lock");
        let mut out = format!("epoch {ORACLE_CACHE_EPOCH}");
        let mut name: Option<&str> = None;
        for line in LOCK.lines() {
            if line == "[[package]]" {
                name = None;
            } else if let Some(n) = line.strip_prefix("name = \"") {
                name = n.strip_suffix('"');
            } else if let Some(v) = line.strip_prefix("version = \"") {
                if let Some(n @ ("mlua" | "mlua-sys" | "luajit-src")) = name {
                    out.push(' ');
                    out.push_str(n);
                    out.push('@');
                    out.push_str(v.trim_end_matches('"'));
                }
            }
        }
        out
    })
}

/// The `<sha>` of an unpatched PoB source tree booted in-process: BLAKE3 of the oracle runtime,
/// the Lua runtime versions, the JIT mode (`POB_ORACLE_JIT`), the canonical path and the stat
/// fingerprint (relative path, size, mtime) of every file but PoB's derived caches
/// (`patcher::is_derived_cache`), the fingerprint the patcher's sha takes of the tree it copies
/// (`patcher::plan_sha`). Memoised per process.
pub fn source_key(src: &Path) -> Result<String> {
    use std::collections::HashMap;
    use std::sync::{Mutex, OnceLock};
    static KEYS: OnceLock<Mutex<HashMap<PathBuf, String>>> = OnceLock::new();
    let canon = std::fs::canonicalize(src).unwrap_or_else(|_| src.to_path_buf());
    let keys = KEYS.get_or_init(Mutex::default);
    if let Some(k) = keys
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .get(&canon)
    {
        return Ok(k.clone());
    }
    let mut files = Vec::new();
    walk(&canon, "", &mut files).map_err(|e| OracleError(format!("{}: {e}", canon.display())))?;
    let key = source_fingerprint(&canon, &files)?;
    keys.lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .insert(canon, key.clone());
    Ok(key)
}

/// Every file under `dir` as `(path relative to the walk root, path)`, sorted by name.
fn walk(dir: &Path, rel: &str, out: &mut Vec<(String, PathBuf)>) -> std::io::Result<()> {
    let mut entries: Vec<_> = std::fs::read_dir(dir)?
        .filter_map(std::result::Result::ok)
        .collect();
    entries.sort_by_key(std::fs::DirEntry::file_name);
    for e in entries {
        let name = e.file_name().to_string_lossy().into_owned();
        let r = if rel.is_empty() {
            name
        } else {
            format!("{rel}/{name}")
        };
        if e.file_type()?.is_dir() {
            walk(&e.path(), &r, out)?;
        } else {
            out.push((r, e.path()));
        }
    }
    Ok(())
}

/// [`source_key`] of the canonical tree `canon` with the files `files`, not memoised.
fn source_fingerprint(canon: &Path, files: &[(String, PathBuf)]) -> Result<String> {
    let mut h = blake3::Hasher::new();
    h.update(b"pob-oracle unpatched source 2\n");
    for code in ORACLE_RUNTIME {
        h.update(code.as_bytes());
        h.update(&[0]);
    }
    h.update(lua_runtime_versions().as_bytes());
    h.update(&[0]);
    #[cfg(all(not(target_arch = "wasm32"), feature = "luajit"))]
    h.update(if crate::jit::oracle_jit_off() {
        b"jit.off"
    } else {
        b"jit.on"
    });
    h.update(canon.to_string_lossy().as_bytes());
    crate::patcher::hash_stat_fingerprint(&mut h, files)
        .map_err(|(p, e)| OracleError(format!("{}: {e}", p.display())))?;
    Ok(h.finalize().to_hex()[..16].to_owned())
}

/// A temp path next to `dst` that no other writer (thread or process) uses.
fn private_tmp(dst: &Path) -> PathBuf {
    static SEQ: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    let name = dst
        .file_name()
        .map_or_else(String::new, |n| n.to_string_lossy().into_owned());
    dst.with_file_name(format!(
        ".{name}.tmp.{}.{}",
        std::process::id(),
        SEQ.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
    ))
}

/// zstd level of the cache entries (fast; the entries are read far more often than written).
const CACHE_ZSTD_LEVEL: i32 = 3;

/// The cached results of one in-process oracle query:
/// `<parity>/probe-cache/<source-key>/<input-key>/query-<script-key>/<name>.zst`.
#[derive(Debug, Clone)]
pub struct QueryCache {
    pub dir: PathBuf,
}

/// Hits and misses of [`QueryCache::get`] in this process (for the tests' summaries).
static QUERY_HITS: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
static QUERY_MISSES: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);

/// `(hits, misses)` of the query cache in this process.
#[must_use]
pub fn query_cache_stats() -> (usize, usize) {
    (
        QUERY_HITS.load(std::sync::atomic::Ordering::Relaxed),
        QUERY_MISSES.load(std::sync::atomic::Ordering::Relaxed),
    )
}

impl QueryCache {
    /// The entry of the query `script` (every text that decides the result besides PoB and the
    /// input: the scripts run, in order) over `input` (a build's XML bytes, or the input a test
    /// generates) on the PoB checkout `src`. `None` when there is no `.parity` directory or the
    /// cache is off.
    pub fn new(src: &Path, input: &[u8], script: &[&str]) -> Result<Option<Self>> {
        if cache_mode() == CacheMode::Off {
            return Ok(None);
        }
        let Some(parity) = crate::oracle::find_parity_dir() else {
            return Ok(None);
        };
        let parts: Vec<&[u8]> = script.iter().map(|s| s.as_bytes()).collect();
        Ok(Some(Self {
            dir: parity
                .join("probe-cache")
                .join(source_key(src)?)
                .join(&blake3::hash(input).to_hex()[..16])
                .join(format!(
                    "query-{}",
                    content_key("pob-oracle query 1", &parts)
                )),
        }))
    }

    #[must_use]
    pub fn path(&self, name: &str) -> PathBuf {
        self.dir.join(format!("{name}.zst"))
    }

    /// Is `name` present (and readable under the cache mode)?
    #[must_use]
    pub fn has(&self, name: &str) -> bool {
        cache_mode() == CacheMode::On && self.path(name).is_file()
    }

    /// The cached text `name` (counted as a hit or a miss).
    #[must_use]
    pub fn get(&self, name: &str) -> Option<String> {
        let got = if self.has(name) {
            read_text(&self.path(name)).ok()
        } else {
            None
        };
        let c = if got.is_some() {
            &QUERY_HITS
        } else {
            &QUERY_MISSES
        };
        c.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        got
    }

    /// Store `text` as `name`.
    pub fn put(&self, name: &str, text: &str) -> Result<()> {
        let dst = self.path(name);
        std::fs::create_dir_all(&self.dir)
            .map_err(|e| OracleError(format!("{}: {e}", self.dir.display())))?;
        let tmp = private_tmp(&dst);
        let z = zstd::encode_all(text.as_bytes(), CACHE_ZSTD_LEVEL)
            .map_err(|e| OracleError(e.to_string()))?;
        std::fs::write(&tmp, z).map_err(|e| OracleError(format!("{}: {e}", tmp.display())))?;
        std::fs::rename(&tmp, &dst).map_err(|e| OracleError(format!("{}: {e}", dst.display())))
    }

    /// Store the file `src` as `name`, compressed as a stream (large JSONL outputs).
    pub fn put_file(&self, name: &str, src: &Path) -> Result<()> {
        let dst = self.path(name);
        std::fs::create_dir_all(&self.dir)
            .map_err(|e| OracleError(format!("{}: {e}", self.dir.display())))?;
        let tmp = private_tmp(&dst);
        let io = |e: std::io::Error, p: &Path| OracleError(format!("{}: {e}", p.display()));
        let mut from = std::fs::File::open(src).map_err(|e| io(e, src))?;
        let to = std::fs::File::create(&tmp).map_err(|e| io(e, &tmp))?;
        let mut enc =
            zstd::stream::write::Encoder::new(to, CACHE_ZSTD_LEVEL).map_err(|e| io(e, &tmp))?;
        std::io::copy(&mut from, &mut enc).map_err(|e| io(e, &tmp))?;
        enc.finish().map_err(|e| io(e, &tmp))?;
        std::fs::rename(&tmp, &dst).map_err(|e| io(e, &dst))
    }

    /// The cached file `name` as a decompressing line reader.
    pub fn open(&self, name: &str) -> Result<Box<dyn std::io::BufRead>> {
        let p = self.path(name);
        let f =
            std::fs::File::open(&p).map_err(|e| OracleError(format!("{}: {e}", p.display())))?;
        let dec = zstd::stream::read::Decoder::new(f)
            .map_err(|e| OracleError(format!("{}: {e}", p.display())))?;
        Ok(Box::new(std::io::BufReader::new(dec)))
    }
}

/// The text a query returns, from the cache when present; otherwise `run` computes it (the
/// caller boots PoB under the [`OracleLock`] inside `run`) and it is stored.
pub fn cached_query(
    src: &Path,
    input: &[u8],
    script: &[&str],
    run: impl FnOnce() -> String,
) -> String {
    let cache = QueryCache::new(src, input, script).unwrap_or_else(|e| panic!("query cache: {e}"));
    if let Some(text) = cache.as_ref().and_then(|c| c.get("result")) {
        return text;
    }
    let text = run();
    if let Some(c) = &cache {
        if let Err(e) = c.put("result", &text) {
            eprintln!("query cache: not stored: {e}");
        }
    }
    text
}

/// Where the oracle tests publish patched copies: `$POB_ORACLE_SRC_ROOT`, else
/// `target/oracle-src` of the checkout that holds the PoB source (`<checkout>/.PathOfBuilding/src`;
/// from a git worktree that is the main checkout, so every worktree shares one copy per sha),
/// else [`crate::patcher::default_root`].
#[must_use]
pub fn shared_patch_root(pob_src: &Path) -> PathBuf {
    if let Some(p) = std::env::var_os("POB_ORACLE_SRC_ROOT") {
        return PathBuf::from(p);
    }
    let canon = std::fs::canonicalize(pob_src).unwrap_or_else(|_| pob_src.to_path_buf());
    if let Some(checkout) = canon.parent().and_then(Path::parent) {
        if checkout.join("Cargo.toml").is_file() {
            return checkout.join("target").join("oracle-src");
        }
    }
    crate::patcher::default_root()
}

/// One probe run of a selection, content-keyed:
/// `<parity>/probe-cache/<sha>/<build-key>/sel-<options-key>/` with `hits.json.zst`,
/// `outputs.json.zst` (and `provenance.json.zst`) and `state/<hit>.json.zst`.
#[derive(Debug, Clone)]
pub struct SelectionCache {
    pub dir: PathBuf,
}

impl SelectionCache {
    pub fn new(parity: &Path, sha: &str, build: &Path, opts: &ProbeOptions) -> Result<Self> {
        let sel = opts.select.join(",");
        let max = opts.max_dumps.to_string();
        // the Rust/Lua runtime the `bin probe` process boots PoB with (the `<sha>` covers the
        // patched tree, the probe's Lua and the Lua runtime versions)
        let mut parts: Vec<&[u8]> = vec![
            sel.as_bytes(),
            max.as_bytes(),
            if opts.provenance { b"prov" } else { b"noprov" },
        ];
        parts.extend(ORACLE_RUNTIME.iter().map(|s| s.as_bytes()));
        let key = content_key("pob-oracle selection 2", &parts);
        Ok(Self {
            dir: ProbeCache::new(parity, sha, build)?
                .dir
                .join(format!("sel-{key}")),
        })
    }

    /// The dumps (`<hit>.json.zst`).
    #[must_use]
    pub fn state_dir(&self) -> PathBuf {
        self.dir.join("state")
    }

    /// Has the selection been probed (and may it be read under the cache mode)?
    #[must_use]
    pub fn is_filled(&self) -> bool {
        cache_mode() == CacheMode::On && self.dir.join("hits.json.zst").is_file()
    }

    /// Probe `build` with `opts` in a fresh `bin probe` process (which takes the [`OracleLock`]
    /// itself) and publish the run as this entry.
    pub fn fill(
        &self,
        bin: &Path,
        build: &Path,
        patched_src: &Path,
        opts: &ProbeOptions,
    ) -> Result<()> {
        let tmp = private_tmp(&self.dir);
        let _ = std::fs::remove_dir_all(&tmp);
        let res = (|| {
            let run = tmp.join("run");
            run_probe_process(bin, build, &run, patched_src, opts)?;
            let out = tmp.join("out");
            let state = out.join("state");
            std::fs::create_dir_all(&state)
                .map_err(|e| OracleError(format!("{}: {e}", state.display())))?;
            // the run is compressed into `out`, which is then renamed into place as a whole
            if let Ok(rd) = std::fs::read_dir(run.join("state")) {
                for e in rd.filter_map(std::result::Result::ok) {
                    let name = e.file_name().to_string_lossy().into_owned();
                    if name.ends_with(".json") {
                        compress_into(&e.path(), &state.join(format!("{name}.zst")))?;
                    }
                }
            }
            for f in ["outputs.json", "provenance.json", "hits.json"] {
                let p = run.join(f);
                if p.is_file() {
                    compress_into(&p, &out.join(format!("{f}.zst")))?;
                }
            }
            if let Some(d) = self.dir.parent() {
                std::fs::create_dir_all(d)
                    .map_err(|e| OracleError(format!("{}: {e}", d.display())))?;
            }
            if cache_mode() == CacheMode::Refresh {
                let _ = std::fs::remove_dir_all(&self.dir);
            }
            match std::fs::rename(&out, &self.dir) {
                Ok(()) => Ok(()),
                // another test or worktree published the same entry first
                Err(_) if self.dir.join("hits.json.zst").is_file() => Ok(()),
                Err(e) => Err(OracleError(format!("{}: {e}", self.dir.display()))),
            }
        })();
        let _ = std::fs::remove_dir_all(&tmp);
        res
    }
}

#[cfg(test)]
mod cache_tests {
    use super::*;
    use std::io::BufRead;

    fn scratch(name: &str) -> PathBuf {
        let d = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../target/tmp")
            .join(format!("hyg3-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    /// Part boundaries and the domain are part of a content key.
    #[test]
    fn content_keys_separate_parts() {
        let k = |d: &str, p: &[&[u8]]| content_key(d, p);
        assert_eq!(k("a", &[b"xy"]), k("a", &[b"xy"]));
        assert_ne!(k("a", &[b"xy"]), k("a", &[b"x", b"y"]));
        assert_ne!(k("a", &[b"xy"]), k("b", &[b"xy"]));
        assert_eq!(k("a", &[]).len(), 16);
    }

    /// Texts and streamed files come back byte for byte; writes leave no temp files.
    #[test]
    fn query_cache_round_trips() {
        let d = scratch("query");
        let c = QueryCache { dir: d.join("q") };
        assert!(c.get("result").is_none());
        let text = "{\"a\": \"\u{e9}\\n\"}\n".repeat(1000);
        c.put("result", &text).unwrap();
        let src = d.join("records.jsonl");
        std::fs::write(&src, "{\"t\":1}\n{\"t\":2}\n").unwrap();
        c.put_file("records", &src).unwrap();
        if cache_mode() == CacheMode::On {
            assert_eq!(c.get("result").as_deref(), Some(text.as_str()));
        }
        let lines = c
            .open("records")
            .unwrap()
            .lines()
            .collect::<std::io::Result<Vec<String>>>()
            .unwrap();
        assert_eq!(lines, ["{\"t\":1}", "{\"t\":2}"]);
        let names: Vec<String> = std::fs::read_dir(&c.dir)
            .unwrap()
            .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
            .collect();
        assert!(names.iter().all(|n| !n.starts_with('.')), "{names:?}");
        let _ = std::fs::remove_dir_all(&d);
    }

    /// A selection key covers the selection, the dump cap and provenance.
    #[test]
    fn selection_keys_cover_the_options() {
        let d = scratch("sel");
        let xml = d.join("b.xml");
        std::fs::write(&xml, "<PathOfBuilding/>").unwrap();
        let o = ProbeOptions {
            select: vec!["offence:in@main".to_owned()],
            max_dumps: 200,
            provenance: false,
        };
        let dir = |o: &ProbeOptions| SelectionCache::new(&d, "sha", &xml, o).unwrap().dir;
        let base = dir(&o);
        assert!(base.starts_with(
            d.join("probe-cache")
                .join("sha")
                .join(build_key(&xml).unwrap())
        ));
        assert_ne!(
            base,
            dir(&ProbeOptions {
                provenance: true,
                ..o.clone()
            })
        );
        assert_ne!(
            base,
            dir(&ProbeOptions {
                max_dumps: 1,
                ..o.clone()
            })
        );
        assert_ne!(
            base,
            dir(&ProbeOptions {
                select: vec!["offence:out@main".to_owned()],
                ..o.clone()
            })
        );
        let _ = std::fs::remove_dir_all(&d);
    }

    /// Different trees get different source keys; a tree's key is stable (memoised).
    #[test]
    fn source_key_tracks_the_tree() {
        let d = scratch("src");
        std::fs::write(d.join("a.lua"), "return 1").unwrap();
        let k1 = source_key(&d).unwrap();
        assert_eq!(k1, source_key(&d).unwrap());
        let d2 = scratch("src2");
        std::fs::write(d2.join("a.lua"), "return 1").unwrap();
        std::fs::write(d2.join("b.lua"), "return 2").unwrap();
        assert_ne!(k1, source_key(&d2).unwrap());
        let _ = std::fs::remove_dir_all(&d);
        let _ = std::fs::remove_dir_all(&d2);
    }

    /// Rewriting (or deleting) a jewel `.bin`, as PoB's DataJewelFileLoader does on any run,
    /// leaves the source key unchanged; the zips and every other file stay fingerprinted.
    #[test]
    fn source_key_ignores_jewel_bins() {
        let d = scratch("src-bin");
        let jd = d.join("Data/TimelessJewelData");
        std::fs::create_dir_all(&jd).unwrap();
        std::fs::write(d.join("a.lua"), "return 1").unwrap();
        std::fs::write(jd.join("LethalPride.zip"), "zip").unwrap();
        std::fs::write(jd.join("LethalPride.bin"), "inflated").unwrap();
        let canon = std::fs::canonicalize(&d).unwrap();
        // the unmemoised key: the walk and the fingerprint `source_key` memoises
        let key = || {
            let k = source_key(&d).unwrap();
            let mut files = Vec::new();
            walk(&canon, "", &mut files).unwrap();
            assert_eq!(k.len(), 16);
            source_fingerprint(&canon, &files).unwrap()
        };
        let k1 = key();
        assert_eq!(k1, source_key(&d).unwrap());
        std::fs::write(jd.join("LethalPride.bin"), "inflated, written again").unwrap();
        std::fs::File::options()
            .write(true)
            .open(jd.join("LethalPride.bin"))
            .unwrap()
            .set_modified(std::time::SystemTime::now() + std::time::Duration::from_secs(3600))
            .unwrap();
        assert_eq!(k1, key(), "a .bin rewrite changed the source key");
        std::fs::remove_file(jd.join("LethalPride.bin")).unwrap();
        assert_eq!(k1, key(), "a missing .bin changed the source key");
        std::fs::write(jd.join("LethalPride.zip"), "zip v2").unwrap();
        assert_ne!(k1, key(), "the zip is not fingerprinted");
        let _ = std::fs::remove_dir_all(&d);
    }

    /// The Lua runtime versions come from Cargo.lock.
    #[test]
    fn lua_runtime_versions_name_luajit() {
        let v = lua_runtime_versions();
        assert!(v.starts_with(&format!("epoch {ORACLE_CACHE_EPOCH}")), "{v}");
        assert!(v.contains(" mlua@") && v.contains(" luajit-src@"), "{v}");
    }
}

/// `src` zstd-compressed into `dst`.
fn compress_into(src: &Path, dst: &Path) -> Result<()> {
    let raw = std::fs::read(src).map_err(|e| OracleError(format!("{}: {e}", src.display())))?;
    let z = zstd::encode_all(&raw[..], CACHE_ZSTD_LEVEL).map_err(|e| OracleError(e.to_string()))?;
    std::fs::write(dst, z).map_err(|e| OracleError(format!("{}: {e}", dst.display())))
}

/// The command that fills a missing cache entry (printed by tests that
/// skip on a cache miss).
#[must_use]
pub fn fill_command(build: &Path, hit: &str) -> String {
    format!(
        "scripts/port/oracle.sh run --build {} --stage '{hit}'",
        build.display()
    )
}

/// A cached dump for `build` at `hit` under the patched source `sha`, or
/// `None` after printing how to fill the cache.
pub fn cached_state(
    parity: &Path,
    sha: &str,
    build: &Path,
    hit: &str,
) -> Result<Option<StateDump>> {
    let cache = ProbeCache::new(parity, sha, build)?;
    let got = cache.get(hit)?;
    if got.is_none() {
        eprintln!(
            "probe cache miss: {} (fill with: {})",
            cache.state_path(hit).display(),
            fill_command(build, hit)
        );
    }
    Ok(got)
}
