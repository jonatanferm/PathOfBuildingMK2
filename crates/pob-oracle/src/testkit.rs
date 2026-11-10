//! Shared helpers for the unit acceptance tests (`tests/<unit>.rs`, calc doc
//! §9.4 L4/L5): load the data and a build, inject a dump, run one Rust
//! segment from an injected `<seg>:in` dump and diff it against `<seg>:out`.
//!
//! Owner: ORC-X.
//!
//! Dumps are looked up (in this order) in `$POB_PROBE_DIR/<stem>/state/`,
//! `.parity/probe/<stem>/state/` (`scripts/port/oracle.sh corpus`) and the
//! probe cache `.parity/probe-cache/<sha>/<build-key>/` of any patched-source
//! sha (`scripts/port/oracle.sh run --build X.xml --stage <hit>` fills it).
//! A hit is named exactly (`offence:in#4`) or by probe id alone
//! (`offence:in`, `perform:1943`), which takes the lowest-numbered dump of that
//! probe present (the selection `@main` dumps one per probe: the main env's).

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use pob_calc::env::EnvRef;
use pob_calc::lua_table::Val;
use pob_calc::model::{Build, BuildRef};
use serde_json::Value as J;

use crate::diff::{diff_trees, DiffOptions, Mismatch};
use crate::oracle::{find_parity_dir, find_up};
use crate::schema::{resolve_ids, StateDump};

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

/// Directories that may hold a build's dumps, most specific first.
pub fn dump_dirs(stem: &str, xml: Option<&Path>) -> Vec<PathBuf> {
    let mut out = Vec::new();
    if let Some(d) = std::env::var_os("POB_PROBE_DIR").map(PathBuf::from) {
        out.push(d.join(stem).join("state"));
    }
    if let Some(parity) = find_parity_dir() {
        out.push(parity.join("probe").join(stem).join("state"));
        if let Some(xml) = xml {
            if let Ok(key) = crate::runner::build_key(xml) {
                if let Ok(rd) = std::fs::read_dir(parity.join("probe-cache")) {
                    let mut shas: Vec<PathBuf> = rd.filter_map(Result::ok).map(|e| e.path()).collect();
                    shas.sort();
                    for s in shas {
                        out.push(s.join(&key));
                    }
                }
            }
        }
    }
    out.retain(|d| d.is_dir());
    out
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
/// there. The key is the patched source's sha, the build's BLAKE3 and the selection, so a cache
/// entry is never stale and every worktree shares it.
#[cfg(feature = "luajit")]
pub fn probe_selection(
    bin: &Path,
    xml: &Path,
    stages: Option<&Path>,
    opts: &crate::runner::ProbeOptions,
) -> Result<PathBuf, String> {
    let cache = selection_cache(xml, stages, opts)?;
    if !cache.is_filled() {
        let patched = shared_patch(stages)?;
        cache.fill(bin, xml, &patched.src, opts).map_err(|e| e.0)?;
    }
    Ok(cache.state_dir())
}

/// Load the dump of `hit` for a build.
pub fn load_dump(stem: &str, xml: Option<&Path>, hit: &str) -> Option<StateDump> {
    let p = find_dump(&dump_dirs(stem, xml), hit)?;
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
/// stage without a segment is reported as one mismatch line.
pub fn segment_diff(build: &Path, stage_in: &str, stage_out: &str) -> Vec<String> {
    if !init_default_data() {
        return vec!["no data dump (data/dump)".to_owned()];
    }
    let stem = build
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or_default()
        .to_owned();
    let dirs = dump_dirs(&stem, Some(build));
    let (Some(pin), Some(pout)) = (find_dump(&dirs, stage_in), find_dump(&dirs, stage_out)) else {
        return vec![format!(
            "missing dump {stage_in} or {stage_out} for {stem} (fill with: scripts/port/oracle.sh run --build {} --stage {stage_in},{stage_out})",
            build.display()
        )];
    };
    let (Ok(din), Ok(dout)) = (StateDump::load(&pin), StateDump::load(&pout)) else {
        return vec![format!("unreadable dump {} / {}", pin.display(), pout.display())];
    };
    let model = load_build(build).unwrap_or_else(empty_build);
    let inj = match crate::inject::inject(&din, &model) {
        Ok(i) => i,
        Err(e) => return vec![format!("inject {stage_in}: {e}")],
    };
    let locals: Vec<(String, Val)> = inj.locals.into_iter().collect();
    let run = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        pob_calc::probe::run_segment(&din.probe, &locals)
    }));
    let after = match run {
        Ok(Ok((_flow, after))) => after,
        Ok(Err(e)) => return vec![format!("run {stage_in}: {e}")],
        Err(p) => {
            let msg = p
                .downcast_ref::<pob_lua::LuaError>()
                .map(|e| e.0.clone())
                .or_else(|| p.downcast_ref::<String>().cloned())
                .or_else(|| p.downcast_ref::<&str>().map(|s| (*s).to_owned()))
                .unwrap_or_else(|| "panic".to_owned());
            return vec![format!("run {stage_in}: {msg}")];
        }
    };
    let env = after
        .iter()
        .find(|(k, _)| k == "env")
        .and_then(|(_, v)| v.obj::<pob_calc::env::Env>())
        .or(inj.env);
    diff_against(&dout, env.as_ref(), &after, &stem)
        .into_iter()
        .map(|m| format!("{} {:?}: pob={:?} ours={:?}", m.path, m.kind, m.pob, m.ours))
        .collect()
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
    diff_trees(&resolve_ids(&pob), &resolve_ids(&ours), &opts)
}

/// A dumped function value (`{"$fn": "<file>:<a>-<b>", …}`).
fn is_fn_marker(v: &J) -> bool {
    v.as_object().is_some_and(|m| m.contains_key("$fn"))
}
