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

use crate::diff::{diff_trees, DiffOptions, Mismatch, MismatchKind};
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
    let cache = selection_cache(xml, stages, opts)?;
    if cache.is_filled() {
        return Ok(crate::runner::ProbeDumps::cached(cache.state_dir()));
    }
    let patched = shared_patch(stages)?;
    cache.fill(bin, xml, &patched.src, opts).map_err(|e| e.0)
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
/// stage without a segment is reported as one mismatch line. Both dumps come from
/// one directory ([`find_pair`]).
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
    let Some((pin, pout)) = find_pair(&dirs, stage_in, stage_out) else {
        return vec![format!(
            "missing dumps {stage_in} + {stage_out} in one directory for {stem} (fill with: scripts/port/oracle.sh run --build {} --stage {stage_in},{stage_out})",
            build.display()
        )];
    };
    let (Ok(din), Ok(dout)) = (StateDump::load(&pin), StateDump::load(&pout)) else {
        return vec![format!("unreadable dump {} / {}", pin.display(), pout.display())];
    };
    segment_diff_dumps(build, &stem, &din, &dout)
}

/// [`segment_diff`] on loaded dumps: the mismatches as text, or the injection / run error as
/// one line.
pub fn segment_diff_dumps(build: &Path, stem: &str, din: &StateDump, dout: &StateDump) -> Vec<String> {
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
        let tree = |list: Vec<J>| {
            json!({"env": {"skill": {"buffList": [{"name": "B", "modList": list}]}}})
        };
        let pob = tree(vec![m("A", 1.0, "S1"), m("B", 2.0, "S1"), m("C", 3.0, "S2")]);
        let ours = tree(vec![m("B", 2.0, "S1"), m("A", 1.0, "S1"), m("C", 3.0, "S2")]);
        let mm = diff_trees(&pob, &ours, &DiffOptions::strict());
        assert!(mm.len() > 1, "{mm:?}");
        let n = normalise_lua_order(mm, &pob, &ours);
        assert_eq!(n.len(), 1, "{n:?}");
        assert_eq!(n[0].kind, MismatchKind::LuaOrder);
        assert_eq!(n[0].path, "env.skill.buffList[1].modList");
        // across sources: not order-only
        let ours = tree(vec![m("C", 3.0, "S2"), m("B", 2.0, "S1"), m("A", 1.0, "S1")]);
        let mm = diff_trees(&pob, &ours, &DiffOptions::strict());
        let k = mm.len();
        let n = normalise_lua_order(mm, &pob, &ours);
        assert_eq!(n.len(), k);
        assert!(n.iter().all(|x| x.kind != MismatchKind::LuaOrder));
        // another value: not order-only
        let ours = tree(vec![m("B", 2.0, "S1"), m("A", 1.5, "S1"), m("C", 3.0, "S2")]);
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
}
