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
    let mut pob = crate::jit::JitPob::boot_with(&root, Some(&pre))
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

impl OracleLock {
    #[allow(clippy::incompatible_msrv)]
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
        file.lock()
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
