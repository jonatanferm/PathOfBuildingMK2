//! Shared helpers for the unit acceptance tests (`tests/<unit>.rs`, calc doc
//! §9.4 L4/L5): load the data and a build, inject a dump, run one Rust
//! segment from an injected `<seg>:in` dump and diff it against `<seg>:out`.
//!
//! Owner: ORC-X.
//!
//! Dumps come from the shared content-addressed probe cache: [`probe_dumps`] (one
//! `bin probe` run of a selection per build, [`crate::runner::SelectionCache`], probed when
//! absent) or [`oracle_dir`] (a custom PoB run). `$POB_PROBE_DIR/<stem>/state/` is an explicit
//! override that [`probe_dumps`] serves instead (CK2, CCR-0147 §3: the keyless
//! `.parity/probe/<stem>/state/` and the pre-SelectionCache `.parity/probe-cache/<sha>/<build>/`
//! of any sha are no longer read).
//! A hit is named exactly (`offence:in#4`) or by probe id alone
//! (`offence:in`, `perform:1943`), which takes the lowest-numbered dump of that
//! probe present (the selection `@main` dumps one per probe: the main env's).

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use pob_calc::env::EnvRef;
use pob_calc::lua_table::Val;
use pob_calc::model::{Build, BuildRef};
use serde_json::Value as J;

use crate::diff::{diff_trees, DiffOptions, Mismatch, MismatchKind};
use crate::oracle::{find_parity_dir, find_up};
use crate::schema::{resolve_ids, StateDump};

// TP1: the bounded corpus pool (`$POB_TEST_PAR`, default 3); see `crate::par`.
pub use crate::par::{
    par_for_each, par_for_each_bounded, par_for_each_init, par_for_each_init_with, par_map,
    pool_running, release_shared, test_par, TEST_PAR_ENV,
};

/// Initialise `pob_gamedata::data()` from `data/dump` (data.bin, else the JSON dump).
/// `$POB_DUMP_DIR` overrides the directory. `Ok(false)` when no dump is found; `Err` when a
/// dump is found but does not load (a corrupt or stale data.bin, a broken JSON dump), so a
/// caller can never mistake a bad input for a missing one.
pub fn try_init_data(dump_dir: &Path) -> Result<bool, String> {
    if pob_gamedata::is_initialised() {
        return Ok(true);
    }
    // HYG3 (test audit S5): parallel `#[test]`s of one binary decode the dump once
    static INIT: std::sync::Mutex<()> = std::sync::Mutex::new(());
    let _once = INIT
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    if pob_gamedata::is_initialised() {
        return Ok(true);
    }
    let dir = if dump_dir.join("manifest.json").is_file() {
        dump_dir.to_path_buf()
    } else if let Some(d) = std::env::var_os("POB_DUMP_DIR").map(PathBuf::from) {
        d
    } else if let Some(m) = find_up("data/dump/manifest.json") {
        m.parent().map(Path::to_path_buf).unwrap_or_default()
    } else {
        return Ok(false);
    };
    if dir.join("data.bin").is_file() {
        pob_gamedata::init_from_dir(&dir)
            .map_err(|e| format!("{}: {e}", dir.join("data.bin").display()))?;
    } else {
        let tree =
            pob_gamedata::load_dump_dir(&dir).map_err(|e| format!("{}: {e}", dir.display()))?;
        let d =
            pob_gamedata::Data::from_tree(&tree).map_err(|e| format!("{}: {e}", dir.display()))?;
        pob_gamedata::init_with(d);
    }
    Ok(true)
}

/// [`try_init_data`] for tests: false only when no dump is found (the caller applies the
/// skip policy, `pob_testpaths::require`); a dump that is found but does not load panics
/// with the load error.
pub fn init_data(dump_dir: &Path) -> bool {
    try_init_data(dump_dir).unwrap_or_else(|e| panic!("data dump does not load: {e}"))
}

/// `init_data` from the default location.
pub fn init_default_data() -> bool {
    init_data(Path::new(""))
}

/// A build model from a build XML file (BM-A's loader: `build:LoadDB`). Until
/// BM-A lands the loader is a stub and this is the empty `Build:Init` model,
/// which is all injection needs (the dump carries the env's state).
pub fn load_build(xml: &Path) -> Option<BuildRef> {
    if !pob_gamedata::is_initialised() {
        return None;
    }
    let text = std::fs::read_to_string(xml).ok()?;
    let build = Build::new(pob_gamedata::data());
    let name = xml.file_name().and_then(|n| n.to_str()).unwrap_or("");
    let _ = build.LoadDB(&text, name);
    Some(build)
}

/// The empty build model (`Build:Init`), for injecting dumps.
pub fn empty_build() -> BuildRef {
    Build::new(pob_gamedata::data())
}

// ---- finding dumps ----------------------------------------------------------------------------

/// `$POB_PROBE_DIR/<stem>/state/` when that variable is set and the directory exists: an
/// explicit, hand-made dump directory that [`probe_dumps`] serves instead of the cache.
pub fn override_dir(stem: &str) -> Option<PathBuf> {
    let d = std::env::var_os("POB_PROBE_DIR").map(PathBuf::from)?;
    Some(d.join(stem).join("state")).filter(|d| d.is_dir())
}

/// The dumps of one build for one probe selection ([`probe_dumps`]): a single directory, so
/// every `:in` / `:out` pair in it comes from one PoB process. Hold it while reading the dumps
/// (under `POB_QUERY_CACHE=off` the directory is a private run deleted on drop).
#[derive(Debug)]
pub struct BuildDumps {
    dirs: Vec<PathBuf>,
    held: Option<crate::runner::ProbeDumps>,
}

impl BuildDumps {
    /// The directory, as the one-element slice [`find_dump`] / [`find_pair`] take.
    #[must_use]
    pub fn dirs(&self) -> &[PathBuf] {
        &self.dirs
    }

    #[must_use]
    pub fn path(&self) -> &Path {
        &self.dirs[0]
    }

    /// The run's directory (`state/`'s parent: `hits.json[.zst]`, `outputs.json[.zst]`).
    #[must_use]
    pub fn run_dir(&self) -> PathBuf {
        self.dirs[0]
            .parent()
            .map_or_else(|| self.dirs[0].clone(), Path::to_path_buf)
    }

    /// Served from `$POB_PROBE_DIR` ([`override_dir`])?
    #[must_use]
    pub fn is_override(&self) -> bool {
        self.held.is_none()
    }

    /// Did the dumps come from the shared cache (rather than a private run or the override)?
    #[must_use]
    pub fn is_cached(&self) -> bool {
        self.held
            .as_ref()
            .is_some_and(crate::runner::ProbeDumps::is_cached)
    }
}

/// CK2 (CCR-0147 §3 item 1): the dumps of `xml` for the probe selection `opts` under the
/// patched copy for `stages`, from the shared probe cache ([`probe_selection`]: keyed by the
/// patched-source sha, the build's BLAKE3, the selection and the probe's Rust code; probed in a
/// fresh `bin probe` process when absent; `POB_QUERY_CACHE` applies). `$POB_PROBE_DIR/<stem>/`
/// ([`override_dir`]) is served instead when it exists. `bin` is the test's
/// `env!("CARGO_BIN_EXE_pob_oracle")`.
pub fn probe_dumps(
    bin: &Path,
    xml: &Path,
    stages: Option<&Path>,
    opts: &crate::runner::ProbeOptions,
) -> Result<BuildDumps, String> {
    let stem = xml.file_stem().and_then(|s| s.to_str()).unwrap_or_default();
    if let Some(d) = override_dir(stem) {
        return Ok(BuildDumps {
            dirs: vec![d],
            held: None,
        });
    }
    #[cfg(feature = "luajit")]
    {
        let held = probe_selection(bin, xml, stages, opts)?;
        Ok(BuildDumps {
            dirs: vec![held.path().to_path_buf()],
            held: Some(held),
        })
    }
    #[cfg(not(feature = "luajit"))]
    {
        let _ = (bin, stages, opts);
        Err("built without the luajit feature (no probe), and no $POB_PROBE_DIR".to_owned())
    }
}

/// CK3 (CCR-0148): `POB_ORACLE_REQUIRE=1` or `CI` (set, not empty / `0` / `false`): a test
/// whose input is missing must fail rather than skip (the tests' `skip` helpers assert this).
#[must_use]
pub fn oracle_required() -> bool {
    std::env::var_os("POB_ORACLE_REQUIRE").is_some_and(|v| v == "1")
        || std::env::var_os("CI").is_some_and(|v| !v.is_empty() && v != "false" && v != "0")
}

/// CK3 (CCR-0148): `Err(what is missing)` when [`probe_dumps`] cannot probe at all because an
/// input is missing (no PoB checkout, no `.parity` for the cache, a build without the `luajit`
/// feature) and no `$POB_PROBE_DIR` override is set. A test checks this before probing and
/// applies its skip policy to it; once it is `Ok`, a probe that fails or dumps nothing is a
/// test failure, never a skip.
pub fn probe_inputs() -> Result<(), String> {
    if std::env::var_os("POB_PROBE_DIR").is_some() {
        return Ok(());
    }
    if !cfg!(feature = "luajit") {
        return Err(
            "built without the luajit feature (no probe), and no $POB_PROBE_DIR".to_owned(),
        );
    }
    if crate::oracle::find_pob_src().is_none() {
        return Err("no PoB checkout (.PathOfBuilding/src)".to_owned());
    }
    if find_parity_dir().is_none() {
        return Err("no .parity directory (probe cache)".to_owned());
    }
    Ok(())
}

// ---- `POB_QUERY_CACHE=refresh`: once per process, never a partial entry (CK3) ------------------

/// CK3 (CCR-0148 §2): the refresh of the cache entry `entry` (a directory), at most once per
/// process. Tests of one binary that share an entry (ORC-X's four tests, DF-G's three, OF-F's
/// two legs) used to re-run and rewrite it each, while another test read it. The first caller
/// of a key runs `run` (which re-runs PoB and publishes the entry); concurrent callers of the
/// same key wait for it, and every later caller gets `served()` (the entry as published) while
/// `filled()` holds. A failed `run` is not remembered (the next caller runs again).
pub(crate) fn refresh_once<T>(
    entry: &Path,
    filled: impl FnOnce() -> bool,
    served: impl FnOnce() -> T,
    run: impl FnOnce() -> Result<T, String>,
) -> Result<T, String> {
    use std::collections::HashMap;
    use std::sync::{Arc, Mutex, OnceLock, PoisonError};
    type Slots = Mutex<HashMap<PathBuf, Arc<Mutex<bool>>>>;
    static SLOTS: OnceLock<Slots> = OnceLock::new();
    let slot = SLOTS
        .get_or_init(Mutex::default)
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .entry(entry.to_path_buf())
        .or_default()
        .clone();
    let mut done = slot.lock().unwrap_or_else(PoisonError::into_inner);
    if *done && filled() {
        return Ok(served());
    }
    let out = run()?;
    *done = true;
    Ok(out)
}

/// CK3 (CCR-0148 §2): take the published directory `entry` out of the cache in one rename
/// (to a private name next to it, then deleted), so a concurrent reader finds the complete old
/// entry, no entry, or the complete new one, never one half deleted (`remove_dir_all` on the
/// entry itself deletes file by file while its completion marker may still be there).
pub(crate) fn retire_entry(entry: &Path) {
    static SEQ: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    let name = entry
        .file_name()
        .map_or_else(String::new, |n| n.to_string_lossy().into_owned());
    let trash = entry.with_file_name(format!(
        ".{name}.retired.{}.{}",
        std::process::id(),
        SEQ.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
    ));
    if std::fs::rename(entry, &trash).is_ok() {
        let _ = std::fs::remove_dir_all(&trash);
    }
}

/// `docs/port/stages.toml` of this checkout: the stages file `scripts/port/oracle.sh run`
/// patches with by default.
#[must_use]
pub fn default_stages() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../docs/port/stages.toml")
}

/// A probe selection of `hits` (globs, `@main`, `#n`), `pob_oracle probe`'s defaults otherwise
/// (`--max-dumps 200`, provenance on): what `scripts/port/oracle.sh run --stage <hits>` ran.
#[must_use]
pub fn selection(hits: &[&str]) -> crate::runner::ProbeOptions {
    crate::runner::ProbeOptions {
        select: hits.iter().map(|s| (*s).to_owned()).collect(),
        ..crate::runner::ProbeOptions::default()
    }
}

/// The dump file of `hit` (`id#n`, or `id` for the lowest `n`) in `dirs`.
pub fn find_dump(dirs: &[PathBuf], hit: &str) -> Option<PathBuf> {
    for d in dirs {
        if hit.contains('#') {
            for ext in [".json", ".json.zst"] {
                let p = d.join(format!(
                    "{}{}",
                    crate::runner::hit_file_name(hit).trim_end_matches(".json"),
                    ext
                ));
                if p.is_file() {
                    return Some(p);
                }
            }
            continue;
        }
        let prefix = format!(
            "{}#",
            crate::runner::hit_file_name(hit).trim_end_matches(".json")
        );
        let mut best: Option<(u64, PathBuf)> = None;
        if let Ok(rd) = std::fs::read_dir(d) {
            for e in rd.filter_map(Result::ok) {
                let name = e.file_name().to_string_lossy().into_owned();
                let Some(rest) = name.strip_prefix(&prefix) else {
                    continue;
                };
                let n: String = rest.chars().take_while(char::is_ascii_digit).collect();
                if let Ok(n) = n.parse::<u64>() {
                    if best.as_ref().map_or(true, |(b, _)| n < *b) {
                        best = Some((n, e.path()));
                    }
                }
            }
        }
        if let Some((_, p)) = best {
            return Some(p);
        }
    }
    None
}

// ---- probing a selection into the shared cache (HYG3, test audit S4) ---------------------------

/// The patched copy of the PoB checkout for `stages` (`None`: no stages file), published under
/// [`crate::runner::shared_patch_root`] and memoised per process (planning reads every patched
/// file).
#[cfg(feature = "luajit")]
pub fn shared_patch(stages: Option<&Path>) -> Result<crate::patcher::Patched, String> {
    use std::collections::HashMap;
    use std::sync::{Mutex, OnceLock};
    type Memo = Mutex<HashMap<Option<PathBuf>, crate::patcher::Patched>>;
    static PATCHED: OnceLock<Memo> = OnceLock::new();
    let memo = PATCHED.get_or_init(Mutex::default);
    let mut memo = memo
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let key = stages.map(Path::to_path_buf);
    if let Some(p) = memo.get(&key) {
        return Ok(p.clone());
    }
    let src = crate::oracle::find_pob_src().ok_or("no PoB checkout (.PathOfBuilding/src)")?;
    let root = crate::runner::shared_patch_root(&src);
    let p = crate::patcher::patch(&src, stages, &root).map_err(|e| e.0)?;
    memo.insert(key, p.clone());
    Ok(p)
}

/// The cache entry of probing `xml` with `opts` under the patched copy for `stages`
/// ([`crate::runner::SelectionCache`] in `.parity/probe-cache`), whether filled or not.
#[cfg(feature = "luajit")]
pub fn selection_cache(
    xml: &Path,
    stages: Option<&Path>,
    opts: &crate::runner::ProbeOptions,
) -> Result<crate::runner::SelectionCache, String> {
    let parity = find_parity_dir().ok_or("no .parity directory")?;
    let patched = shared_patch(stages)?;
    crate::runner::SelectionCache::new(&parity, &patched.sha, xml, opts).map_err(|e| e.0)
}

/// The `state/` directory of probing `xml` with `opts`: from the shared probe cache, or probed now
/// with `bin probe` (a fresh process per build; it takes the machine-wide oracle lock) and stored
/// there. The key is the patched source's sha, the build's BLAKE3, the selection and the probe's
/// Rust code, so every worktree shares an entry. Under `POB_QUERY_CACHE=off` the probe runs into a
/// private directory (never the cache), which the returned [`crate::runner::ProbeDumps`] deletes
/// when dropped: hold it while reading the dumps.
#[cfg(feature = "luajit")]
pub fn probe_selection(
    bin: &Path,
    xml: &Path,
    stages: Option<&Path>,
    opts: &crate::runner::ProbeOptions,
) -> Result<crate::runner::ProbeDumps, String> {
    use crate::runner::{cache_mode, CacheMode, ProbeDumps};
    let cache = selection_cache(xml, stages, opts)?;
    if cache.is_filled() {
        return Ok(ProbeDumps::cached(cache.state_dir()));
    }
    let patched = shared_patch(stages)?;
    if cache_mode() != CacheMode::Refresh {
        return cache.fill(bin, xml, &patched.src, opts).map_err(|e| e.0);
    }
    // CK3 (CCR-0148 §2): `refresh` re-probes each entry once per process, and the old entry
    // leaves the cache in one rename (`SelectionCache::fill` then finds nothing to delete), so
    // no test of this binary (or reader elsewhere) sees the entry half rewritten
    refresh_once(
        &cache.dir,
        || cache.dir.join("hits.json.zst").is_file(),
        || ProbeDumps::cached(cache.state_dir()),
        || {
            retire_entry(&cache.dir);
            cache.fill(bin, xml, &patched.src, opts).map_err(|e| e.0)
        },
    )
}

/// TP1: the text of an in-process PoB query, from the shared content-addressed query cache
/// ([`crate::runner::cached_query_result`], `$POB_QUERY_CACHE` / `$POB_ORACLE_NOCACHE`), else
/// computed by `run` under the machine-wide [`crate::runner::OracleLock`] (taken here, only on a
/// miss) and stored. The key is the PoB tree `src` boots ([`crate::runner::source_key`]: its
/// files, the oracle runtime, the LuaJIT build and mode), `input` (a build's XML bytes, or what
/// the test generates) and `key`: every other text that decides the result, in order (the
/// prelude and scripts run, the build name passed to PoB, call parameters, a version tag). An
/// `Err` from `run` is returned and not cached.
#[cfg(feature = "luajit")]
pub fn oracle_query(
    src: &Path,
    input: &[u8],
    key: &[&str],
    run: impl FnOnce() -> Result<String, String>,
) -> Result<String, String> {
    crate::runner::cached_query_result(src, input, key, || {
        let _lock =
            crate::runner::OracleLock::acquire().map_err(|e| format!("oracle lock: {e}"))?;
        run()
    })
}

/// A directory a PoB run wrote ([`oracle_dir`]): a shared cache entry, or (cache off / verify /
/// no `.parity`) a private run under `target/tmp/oracle-dir-off/` that is deleted when this is
/// dropped. Hold it while reading the files.
#[derive(Debug)]
pub struct OracleDir {
    path: PathBuf,
    private: bool,
}

impl OracleDir {
    #[must_use]
    pub fn path(&self) -> &Path {
        &self.path
    }

    /// Did the files come from the shared cache?
    #[must_use]
    pub fn is_cached(&self) -> bool {
        !self.private
    }
}

impl std::ops::Deref for OracleDir {
    type Target = Path;
    fn deref(&self) -> &Path {
        &self.path
    }
}

impl AsRef<Path> for OracleDir {
    fn as_ref(&self) -> &Path {
        &self.path
    }
}

impl Drop for OracleDir {
    fn drop(&mut self) {
        if self.private {
            let _ = std::fs::remove_dir_all(&self.path);
        }
    }
}

/// TP1: a whole directory of files one PoB run writes (dumps, a result JSON, ...), from the
/// shared cache, else filled now by `fill(dir)` into a private directory that is then renamed
/// into place as a whole (a reader sees a complete entry or none; when a concurrent writer of the
/// key published first, its entry is used). Keyed like [`oracle_query`]: the PoB tree `src`
/// boots, `input` and `key` (`<parity>/probe-cache/<source key>/<input key>/dir-<key>/`).
/// `fill` takes the oracle lock itself (an [`crate::runner::OracleLock`] held here around it
/// would deadlock a `fill` that also takes it). An `Err` from `fill` is returned and nothing is
/// published.
pub fn oracle_dir(
    src: &Path,
    input: &[u8],
    key: &[&str],
    fill: impl FnOnce(&Path) -> Result<(), String>,
) -> Result<OracleDir, String> {
    use crate::runner::{cache_mode, CacheMode, QueryCache};
    static SEQ: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    let tmp_name = |base: &Path| {
        let name = base
            .file_name()
            .map_or_else(String::new, |n| n.to_string_lossy().into_owned());
        base.with_file_name(format!(
            ".{name}.tmp.{}.{}",
            std::process::id(),
            SEQ.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
        ))
    };
    const DONE: &str = ".complete";
    let mode = cache_mode();
    let entry = QueryCache::new(src, input, key)
        .map_err(|e| e.0)?
        .filter(|_| mode != CacheMode::Verify)
        .map(|q| {
            let name = q.dir.file_name().map_or_else(String::new, |n| {
                n.to_string_lossy().replacen("query-", "dir-", 1)
            });
            q.dir.with_file_name(name)
        });
    let Some(entry) = entry else {
        // off / verify / no `.parity`: a private run, deleted when the result is dropped
        let root =
            PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../target/tmp/oracle-dir-off");
        let dir = tmp_name(&root.join("run"));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).map_err(|e| format!("{}: {e}", dir.display()))?;
        let out = OracleDir {
            path: dir,
            private: true,
        };
        fill(&out.path)?;
        return Ok(out);
    };
    let cached = || OracleDir {
        path: entry.clone(),
        private: false,
    };
    if mode == CacheMode::On && entry.join(DONE).is_file() {
        return Ok(cached());
    }
    /// The run that fills a staged directory.
    type Fill<'a> = Box<dyn FnOnce(&Path) -> Result<(), String> + 'a>;
    let publish = |fill: Fill<'_>| {
        let tmp = tmp_name(&entry);
        let _ = std::fs::remove_dir_all(&tmp);
        std::fs::create_dir_all(&tmp).map_err(|e| format!("{}: {e}", tmp.display()))?;
        let res = (|| {
            fill(&tmp)?;
            std::fs::write(tmp.join(DONE), b"").map_err(|e| format!("{}: {e}", tmp.display()))?;
            // publish by rename first (atomic; it fails onto an existing entry), so a
            // concurrent writer's complete entry is never deleted under its reader; only
            // `refresh` (below), or an incomplete entry left over (older layout, interrupted
            // delete), is replaced
            crate::runner::publish_dir(&tmp, &entry, &entry.join(DONE)).map_err(|e| e.0)
        })();
        let _ = std::fs::remove_dir_all(&tmp);
        res.map(|()| cached())
    };
    if mode != CacheMode::Refresh {
        return publish(Box::new(fill));
    }
    // CK3 (CCR-0148 §2): `refresh` re-runs each entry once per process (later tests of this
    // binary read that run), and the old entry leaves the cache in one rename right before the
    // new one is published, so a reader never finds a half-deleted entry with its marker
    refresh_once(
        &entry,
        || entry.join(DONE).is_file(),
        cached,
        || {
            publish(Box::new(|tmp: &Path| {
                fill(tmp)?;
                retire_entry(&entry);
                Ok(())
            }))
        },
    )
}

/// The dump files of `stage_in` and `stage_out` from the first directory of `dirs` that holds
/// both, so the pair comes from one PoB process. PoB's string-keyed `pairs` order differs per
/// process (calc doc §3.4), and the shared probe cache can hold an `:in` of one run next to an
/// `:out` of another when a unit re-probes a build with a selection naming only one of the two
/// stages; such a mixed pair differs on state the segment never touches. CCR-0067 §3.
pub fn find_pair(dirs: &[PathBuf], stage_in: &str, stage_out: &str) -> Option<(PathBuf, PathBuf)> {
    dirs.iter().find_map(|d| {
        let one = std::slice::from_ref(d);
        Some((find_dump(one, stage_in)?, find_dump(one, stage_out)?))
    })
}

/// Load the dump of `hit` from `dirs` (e.g. [`BuildDumps::dirs`]).
pub fn load_dump(dirs: &[PathBuf], hit: &str) -> Option<StateDump> {
    let p = find_dump(dirs, hit)?;
    StateDump::load(&p).ok()
}

// ---- round trip ---------------------------------------------------------------------------------

/// The result of `export(inject(dump))` against the dump.
pub struct RoundTrip {
    /// The exported root equals the dumped root as JSON (ids, keys, values).
    pub exact: bool,
    /// State-diff mismatches between the resolved roots (§9.2 tolerance).
    pub mismatches: Vec<Mismatch>,
    pub fallbacks: BTreeMap<String, u64>,
    pub notes: Vec<String>,
}

/// Inject `dump` into an empty build model and export it again.
pub fn round_trip(dump: &StateDump) -> Result<RoundTrip, String> {
    let build = empty_build();
    let inj = crate::inject::inject(dump, &build)?;
    let locals: Vec<(String, Val)> = inj.locals.into_iter().collect();
    let ours = pob_calc::probe::export_root(inj.env.as_ref(), &locals);
    let exact = ours == dump.root;
    let mismatches = if exact {
        Vec::new()
    } else {
        diff_trees(
            &resolve_ids(&dump.root),
            &resolve_ids(&ours),
            &DiffOptions::strict(),
        )
    };
    Ok(RoundTrip {
        exact,
        mismatches,
        fallbacks: inj.fallbacks,
        notes: inj.notes,
    })
}

// ---- L4 -------------------------------------------------------------------------------------------

/// Export the state after a segment run: the env plus the ctx locals.
pub fn export_after(env: Option<&EnvRef>, locals: &[(String, Val)]) -> J {
    pob_calc::probe::export_root(env, locals)
}

/// L4: inject `<stage_in>`, run the Rust segment, diff against `<stage_out>`'s
/// dump. Returns the mismatches as text (empty = pass). A missing dump or a
/// stage without a segment is reported as one mismatch line. Both dumps come from
/// one directory of `dirs` ([`find_pair`]; e.g. [`BuildDumps::dirs`]).
pub fn segment_diff(
    build: &Path,
    dirs: &[PathBuf],
    stage_in: &str,
    stage_out: &str,
) -> Vec<String> {
    if !init_default_data() {
        return vec!["no data dump (data/dump)".to_owned()];
    }
    let stem = build
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or_default()
        .to_owned();
    let Some((pin, pout)) = find_pair(dirs, stage_in, stage_out) else {
        return vec![format!(
            "missing dumps {stage_in} + {stage_out} in one directory for {stem} (fill with: scripts/port/oracle.sh run --build {} --stage {stage_in},{stage_out})",
            build.display()
        )];
    };
    let (Ok(din), Ok(dout)) = (StateDump::load(&pin), StateDump::load(&pout)) else {
        return vec![format!(
            "unreadable dump {} / {}",
            pin.display(),
            pout.display()
        )];
    };
    segment_diff_dumps(build, &stem, &din, &dout)
}

/// [`segment_diff`] on loaded dumps: the mismatches as text, or the injection / run error as
/// one line.
pub fn segment_diff_dumps(
    build: &Path,
    stem: &str,
    din: &StateDump,
    dout: &StateDump,
) -> Vec<String> {
    match segment_mismatches(build, stem, din, dout) {
        Ok(mm) => mm
            .into_iter()
            .map(|m| format!("{} {:?}: pob={:?} ours={:?}", m.path, m.kind, m.pob, m.ours))
            .collect(),
        Err(e) => vec![e],
    }
}

/// The state after one L4 run of a segment.
pub struct SegmentRun {
    pub flow: pob_calc::flow::Flow,
    /// The env after the run (the `env` local, else the injected env).
    pub env: Option<EnvRef>,
    /// The ctx locals after the run (pass locals included), as `run_segment` returns them.
    pub after: Vec<(String, Val)>,
    /// The injected locals the segment ran on.
    pub locals: Vec<(String, Val)>,
}

/// CCR-0079 §1: a perform dump's `mergeFlasks` / `mergeTinctures` locals are `{"$fn": …}`
/// markers (PoB dumps no upvalues), which leave `PerformCtx.mergeFlasks/.mergeTinctures` nil.
/// Replace each with its closure's upvalues read from the same run's line dump where PoB
/// created the closure (`perform:1691` / `perform:1862`, the PF-C stages; a nil boolean upvalue
/// is `false`). Returns the locals seeded; a closure whose dump is missing or undecodable stays
/// a marker.
pub fn seed_merge_closures(
    locals: &mut [(String, Val)],
    up1691: Option<&StateDump>,
    up1862: Option<&StateDump>,
) -> Vec<&'static str> {
    use pob_calc::modules::calc_perform::setup_flasks::{FlaskUpvalues, TinctureUpvalues};
    use std::rc::Rc;
    fn num(l: &J, k: &str) -> Option<f64> {
        let v = l.get(k)?;
        v.as_f64().or_else(|| crate::schema::decode_num(v))
    }
    fn flag(l: &J, k: &str) -> Option<bool> {
        match l.get(k) {
            None | Some(J::Null) => Some(false),
            Some(J::Bool(b)) => Some(*b),
            Some(_) => None,
        }
    }
    let flask = up1691.and_then(StateDump::locals).and_then(|l| {
        Some(Val::Obj(Rc::new(FlaskUpvalues {
            effectInc: num(l, "effectInc")?,
            effectIncMagic: num(l, "effectIncMagic")?,
            effectIncMagicNoAdjacent: num(l, "effectIncMagicNoAdjacent")?,
            effectIncNonPlayer: num(l, "effectIncNonPlayer")?,
            effectIncMagicNonPlayer: num(l, "effectIncMagicNonPlayer")?,
            flasksApplyToMinion: flag(l, "flasksApplyToMinion")?,
            quickSilverAppliesToAllies: flag(l, "quickSilverAppliesToAllies")?,
            flaskTotalRateInc: num(l, "flaskTotalRateInc")?,
            flaskDurInc: num(l, "flaskDurInc")?,
        })))
    });
    let tincture = up1862.and_then(StateDump::locals).and_then(|l| {
        Some(Val::Obj(Rc::new(TinctureUpvalues {
            effectInc: num(l, "effectInc")?,
            effectIncMagic: num(l, "effectIncMagic")?,
            tinctureLimit: std::cell::Cell::new(num(l, "tinctureLimit")?),
        })))
    });
    let mut seeded = Vec::new();
    for (k, v) in locals.iter_mut() {
        let (name, up) = match k.as_str() {
            "mergeFlasks" => ("mergeFlasks", &flask),
            "mergeTinctures" => ("mergeTinctures", &tincture),
            _ => continue,
        };
        if let Some(up) = up {
            *v = up.clone();
            seeded.push(name);
        }
    }
    seeded
}

/// The message of a caught panic (a raised `LuaError`, or a panic string).
pub fn panic_text(p: &(dyn std::any::Any + Send)) -> String {
    p.downcast_ref::<pob_lua::LuaError>()
        .map(|e| e.0.clone())
        .or_else(|| p.downcast_ref::<String>().cloned())
        .or_else(|| p.downcast_ref::<&str>().map(|s| (*s).to_owned()))
        .unwrap_or_else(|| "panic".to_owned())
}

/// Inject `din` into `model` and run the Rust segment of its probe (`run_segment`, which also
/// sets the state a dump does not carry: CCR-0036, CCR-0049 §2, CCR-0075). A raised Lua error
/// is caught and returned as `Err("run <hit>: <msg>")`.
pub fn run_dump(model: &BuildRef, din: &StateDump) -> Result<SegmentRun, String> {
    let inj = crate::inject::inject(din, model).map_err(|e| format!("inject {}: {e}", din.hit))?;
    let locals: Vec<(String, Val)> = inj.locals.into_iter().collect();
    let run = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        pob_calc::probe::run_segment(&din.probe, &locals)
    }));
    let (flow, after) = match run {
        Ok(Ok(r)) => r,
        Ok(Err(e)) => return Err(format!("run {}: {e}", din.hit)),
        Err(p) => return Err(format!("run {}: {}", din.hit, panic_text(&*p))),
    };
    let env = after
        .iter()
        .find(|(k, _)| k == "env")
        .and_then(|(_, v)| v.obj::<pob_calc::env::Env>())
        .or(inj.env);
    Ok(SegmentRun {
        flow,
        env,
        after,
        locals,
    })
}

/// `Err` when both dumps carry a run id (`"run"`, written by every probe process since ORC-X2)
/// and the ids differ: the pair comes from two PoB processes and differs on per-process `pairs`
/// order the segment never touches (CCR-0067 §3). Dumps without an id pass.
pub fn same_run(din: &StateDump, dout: &StateDump) -> Result<(), String> {
    match (&din.run, &dout.run) {
        (Some(a), Some(b)) if a != b => Err(format!(
            "mixed runs: {} is from probe run {a}, {} from {b} (re-probe both stages together)",
            din.hit, dout.hit
        )),
        _ => Ok(()),
    }
}

/// One L4 run with the mismatches kept as values: inject `din` into the build's model
/// ([`load_build`], else the empty model), run the segment, diff against `dout`. A pair from
/// two probe runs is an `Err` ([`same_run`]).
pub fn segment_mismatches(
    build: &Path,
    stem: &str,
    din: &StateDump,
    dout: &StateDump,
) -> Result<Vec<Mismatch>, String> {
    same_run(din, dout)?;
    let model = load_build(build).unwrap_or_else(empty_build);
    let r = run_dump(&model, din)?;
    Ok(diff_against(dout, r.env.as_ref(), &r.after, stem))
}

/// Diff an exported state against a dump: the env, the GlobalCache and the
/// dumped locals. Rust locals PoB did not dump are ignored; a dumped local
/// the Rust side does not export is reported as missing, except function
/// locals (`$fn` markers), which have no Rust counterpart.
pub fn diff_against(
    dump: &StateDump,
    env: Option<&EnvRef>,
    locals: &[(String, Val)],
    stem: &str,
) -> Vec<Mismatch> {
    diff_against_roots(dump, env, locals, stem).0
}

/// [`diff_against`] followed by the documented LUA-ORDER normalisations
/// ([`normalise_lua_order`]) on the same two trees.
pub fn diff_against_normalised(
    dump: &StateDump,
    env: Option<&EnvRef>,
    locals: &[(String, Val)],
    stem: &str,
) -> Vec<Mismatch> {
    let (mm, pob, ours) = diff_against_roots(dump, env, locals, stem);
    normalise_lua_order(mm, &pob, &ours)
}

/// [`diff_against`], also returning the two resolved trees it compared (PoB's, ours).
fn diff_against_roots(
    dump: &StateDump,
    env: Option<&EnvRef>,
    locals: &[(String, Val)],
    stem: &str,
) -> (Vec<Mismatch>, J, J) {
    let pob_locals: Vec<String> = dump
        .locals()
        .and_then(J::as_object)
        .map(|m| m.keys().cloned().collect())
        .unwrap_or_default();
    let kept: Vec<(String, Val)> = locals
        .iter()
        .filter(|(k, _)| pob_locals.contains(k))
        .cloned()
        .collect();
    let ours = export_after(env, &kept);
    let mut pob = dump.root.clone();
    if let Some(J::Object(l)) = pob.get_mut("locals") {
        let ours_names: Vec<&String> = kept.iter().map(|(k, _)| k).collect();
        l.retain(|k, v| ours_names.contains(&k) || !is_fn_marker(v));
    }
    let nondet = find_parity_dir()
        .map(|p| p.join("nondet").join(format!("{stem}.json")))
        .filter(|p| p.is_file())
        .and_then(|p| crate::nondet::NondetRecord::load(&p).ok());
    let opts = DiffOptions {
        nondet_values: nondet.as_ref().and_then(|n| n.states.get(&dump.hit)),
        nondet_orders: nondet.as_ref().and_then(|n| n.state_orders.get(&dump.hit)),
        ..DiffOptions::strict()
    };
    let (pob, ours) = (resolve_ids(&pob), resolve_ids(&ours));
    let mm = diff_trees(&pob, &ours, &opts);
    (mm, pob, ours)
}

// ---- LUA-ORDER normalisations (FUP1) ------------------------------------------------------------

/// Documented LUA-ORDER normalisations of a state diff (calc doc §3.4/§9.5): mismatches that
/// only reflect PoB's per-process string-key `pairs` order, which no port can reproduce, are
/// replaced by one [`MismatchKind::LuaOrder`] mismatch each. `pob` / `ours` are the two
/// resolved trees `mm` was computed from ([`diff_against_normalised`] passes them).
///
/// * **Buff mod lists** ([`buff_list_order_only`]): every mismatch under one
///   `…buffList[i].modList` list whose contents are the same mods in another order within each
///   source becomes a single `LuaOrder` mismatch at the list's path.
///
/// Any other mismatch is returned unchanged, in order.
#[must_use]
pub fn normalise_lua_order(mm: Vec<Mismatch>, pob: &J, ours: &J) -> Vec<Mismatch> {
    let mut lists: BTreeMap<String, bool> = BTreeMap::new();
    for m in &mm {
        if let Some(l) = buff_mod_list_path(&m.path) {
            if !lists.contains_key(l) {
                lists.insert(l.to_owned(), buff_list_order_only(pob, ours, l));
            }
        }
    }
    let mut reported = std::collections::BTreeSet::new();
    let mut out = Vec::with_capacity(mm.len());
    for m in mm {
        match buff_mod_list_path(&m.path) {
            Some(l) if lists[l] => {
                if reported.insert(l.to_owned()) {
                    out.push(Mismatch {
                        path: l.to_owned(),
                        kind: MismatchKind::LuaOrder,
                        pob: None,
                        ours: None,
                        writer: None,
                    });
                }
            }
            _ => out.push(m),
        }
    }
    out
}

/// `…buffList[i].modList` of a mismatch path at or under one (`None` otherwise).
#[must_use]
pub fn buff_mod_list_path(path: &str) -> Option<&str> {
    let b = path.find(".buffList[")?;
    let rest = &path[b..];
    let m = rest.find("].modList")? + "].modList".len();
    let end = b + m;
    matches!(path.as_bytes().get(end), None | Some(b'[' | b'.')).then(|| &path[..end])
}

/// LUA-ORDER for a skill's buff mod lists (CalcActiveSkill.lua:804-869): the buff's `modList`
/// takes the skill's global-effect mods in `skillModList` order, and the mods of one granted
/// effect's stat map come in `pairs(stats)` order (`mergeSkillInstanceMods`,
/// CalcActiveSkill.lua:62, over the string-keyed table `calcLib.buildSkillInstanceStats`
/// builds, CalcTools.lua:192-250; random per PoB process, calc doc §3.4/§9.5; AS's own
/// acceptance accepts the same within same-source runs). The list at `list` is order-only
/// when both trees hold it with the same length (non-empty), the same `source` at every
/// position (so the permutation stays inside one source's stat map) and the same multiset of
/// mods (§9.2 tolerance). Moved here from PF-C's test by FUP1.
#[must_use]
pub fn buff_list_order_only(pob: &J, ours: &J, list: &str) -> bool {
    use crate::diff::tree_eq;
    use crate::schema::{children, get_path};
    let (Some(a), Some(b)) = (get_path(pob, list), get_path(ours, list)) else {
        return false;
    };
    let la: Vec<&J> = children(list, a).into_iter().map(|(_, v)| v).collect();
    let lb: Vec<&J> = children(list, b).into_iter().map(|(_, v)| v).collect();
    if la.len() != lb.len() || la.is_empty() {
        return false;
    }
    if la
        .iter()
        .zip(&lb)
        .any(|(x, y)| x.get("source") != y.get("source"))
    {
        return false;
    }
    let mut used = vec![false; lb.len()];
    la.iter().all(|x| {
        (0..lb.len())
            .find(|&i| !used[i] && tree_eq(x, lb[i], 1e-9))
            .map(|i| used[i] = true)
            .is_some()
    })
}

/// A dumped function value (`{"$fn": "<file>:<a>-<b>", …}`).
fn is_fn_marker(v: &J) -> bool {
    v.as_object().is_some_and(|m| m.contains_key("$fn"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn m(name: &str, value: f64, source: &str) -> J {
        json!({"name": name, "type": "BASE", "value": value, "flags": 0, "keywordFlags": 0, "source": source})
    }

    /// FUP1: a buff mod list permuted inside one source collapses to one LUA-ORDER mismatch;
    /// a permutation across sources, or different mods, is left alone.
    #[test]
    fn buff_list_lua_order_normalisation() {
        let tree = |list: Vec<J>| json!({"env": {"skill": {"buffList": [{"name": "B", "modList": list}]}}});
        let pob = tree(vec![
            m("A", 1.0, "S1"),
            m("B", 2.0, "S1"),
            m("C", 3.0, "S2"),
        ]);
        let ours = tree(vec![
            m("B", 2.0, "S1"),
            m("A", 1.0, "S1"),
            m("C", 3.0, "S2"),
        ]);
        let mm = diff_trees(&pob, &ours, &DiffOptions::strict());
        assert!(mm.len() > 1, "{mm:?}");
        let n = normalise_lua_order(mm, &pob, &ours);
        assert_eq!(n.len(), 1, "{n:?}");
        assert_eq!(n[0].kind, MismatchKind::LuaOrder);
        assert_eq!(n[0].path, "env.skill.buffList[1].modList");
        // across sources: not order-only
        let ours = tree(vec![
            m("C", 3.0, "S2"),
            m("B", 2.0, "S1"),
            m("A", 1.0, "S1"),
        ]);
        let mm = diff_trees(&pob, &ours, &DiffOptions::strict());
        let k = mm.len();
        let n = normalise_lua_order(mm, &pob, &ours);
        assert_eq!(n.len(), k);
        assert!(n.iter().all(|x| x.kind != MismatchKind::LuaOrder));
        // another value: not order-only
        let ours = tree(vec![
            m("B", 2.0, "S1"),
            m("A", 1.5, "S1"),
            m("C", 3.0, "S2"),
        ]);
        let mm = diff_trees(&pob, &ours, &DiffOptions::strict());
        let k = mm.len();
        assert_eq!(normalise_lua_order(mm, &pob, &ours).len(), k);
        assert_eq!(
            buff_mod_list_path("a.buffList[2].modList[3].name"),
            Some("a.buffList[2].modList")
        );
        assert_eq!(buff_mod_list_path("a.buffList[2].modListX"), None);
        assert_eq!(buff_mod_list_path("a.buffList[2].name"), None);
    }

    /// CK3 (CCR-0148 §2): under `refresh` an entry is re-run once per process: concurrent
    /// callers of one key wait for the first and are served its run; a failed run is retried.
    #[test]
    fn refresh_once_runs_each_key_once() {
        use std::sync::atomic::{AtomicUsize, Ordering::SeqCst};
        use std::sync::Arc;
        let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../target/tmp")
            .join(format!("ck3-refresh-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let entry = dir.join("sel-x");
        let runs = Arc::new(AtomicUsize::new(0));
        // a failed run publishes nothing and is not remembered
        let r: Result<u8, String> = refresh_once(
            &entry,
            || entry.is_dir(),
            || 0,
            || Err("probe failed".to_owned()),
        );
        assert!(r.is_err());
        let threads: Vec<_> = (0..6)
            .map(|_| {
                let (entry, runs) = (entry.clone(), runs.clone());
                std::thread::spawn(move || {
                    refresh_once(
                        &entry,
                        || entry.join("done").is_file(),
                        || "served",
                        || {
                            runs.fetch_add(1, SeqCst);
                            std::thread::sleep(std::time::Duration::from_millis(50));
                            std::fs::create_dir_all(&entry).unwrap();
                            std::fs::write(entry.join("done"), b"").unwrap();
                            Ok("ran")
                        },
                    )
                    .unwrap()
                })
            })
            .collect();
        let got: Vec<&str> = threads.into_iter().map(|t| t.join().unwrap()).collect();
        assert_eq!(runs.load(SeqCst), 1, "{got:?}");
        assert_eq!(got.iter().filter(|g| **g == "ran").count(), 1, "{got:?}");
        // a published entry that has since vanished is run again
        retire_entry(&entry);
        assert!(!entry.exists());
        let again = refresh_once(&entry, || entry.is_dir(), || "served", || Ok("ran")).unwrap();
        assert_eq!(again, "ran");
        // the retired copy is gone, nothing is left next to the entry
        let left: Vec<_> = std::fs::read_dir(&dir)
            .unwrap()
            .filter_map(Result::ok)
            .collect();
        assert!(left.is_empty(), "{left:?}");
        let _ = std::fs::remove_dir_all(&dir);
    }
}
