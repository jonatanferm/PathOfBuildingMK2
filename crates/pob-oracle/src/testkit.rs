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

use std::cell::{OnceCell, RefCell};
use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};
use std::rc::Rc;

use pob_calc::env::EnvRef;
use pob_calc::lua_table::Val;
use pob_calc::model::{Build, BuildRef};
use serde_json::Value as J;

use crate::diff::{diff_trees, DiffOptions, Mismatch};
use crate::oracle::{find_parity_dir, find_up};
use crate::schema::{resolve_ids, StateDump};

/// Initialise `pob_gamedata::data()` from `data/dump` (data.bin, else the JSON dump).
/// `$POB_DUMP_DIR` overrides the directory. False when no dump is found.
pub fn init_data(dump_dir: &Path) -> bool {
    if pob_gamedata::is_initialised() {
        return true;
    }
    let dir = if dump_dir.join("manifest.json").is_file() {
        dump_dir.to_path_buf()
    } else if let Some(d) = std::env::var_os("POB_DUMP_DIR").map(PathBuf::from) {
        d
    } else if let Some(m) = find_up("data/dump/manifest.json") {
        m.parent().map(Path::to_path_buf).unwrap_or_default()
    } else {
        return false;
    };
    if dir.join("data.bin").is_file() {
        pob_gamedata::init_from_dir(&dir).is_ok()
    } else {
        match pob_gamedata::load_dump_dir(&dir)
            .ok()
            .and_then(|t| pob_gamedata::Data::from_tree(&t).ok())
        {
            Some(d) => {
                pob_gamedata::init_with(d);
                true
            }
            None => false,
        }
    }
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
/// stage without a segment is reported as one mismatch line. Skill hooks the
/// segment calls through `runSkillFunc` are replayed from the build's
/// recorded `runSkillFunc:in/out` dumps ([`segment_run`]).
pub fn segment_diff(build: &Path, stage_in: &str, stage_out: &str) -> Vec<String> {
    segment_run(build, stage_in, stage_out, None).mismatches
}

/// What one L4 run ([`segment_run`]) did.
#[derive(Debug, Default)]
pub struct SegmentRun {
    /// Diff lines against `<stage_out>` (empty = pass), or one line naming why the run
    /// could not be made (missing dump, inject or segment error).
    pub mismatches: Vec<String>,
    /// The `runSkillFunc:in` hits whose recorded delta was applied (CCR-0041).
    pub hooks_replayed: Vec<String>,
    /// Hook calls of a skill that has the hook but no recorded call to replay
    /// (the Rust registry body ran instead), and replay notes.
    pub hook_notes: Vec<String>,
}

/// L4 with skill-hook replay (CCR-0041): inject `<stage_in>`, install a
/// `pob_calc::probe` skill-hook handler, run the Rust segment, diff against
/// `<stage_out>`.
///
/// The handler answers each `runSkillFunc(name)` call of the segment. When the
/// skill's granted effect has no such hook the Lua call does nothing and the
/// Rust body runs (it does nothing either). Otherwise the handler applies the
/// recorded `runSkillFunc:in → :out` delta ([`crate::replay::Replay`]) of the
/// first not yet used recorded call with that `name` made inside the same
/// enclosing function-probe call as `<stage_in>` (the innermost entry of the
/// dump's `stack`, e.g. `offence:in#4`); calls made before `<stage_in>` are
/// dropped when a `hits.json` of the dump directory orders them, so a segment
/// that starts mid-offence (a later pass) picks its own calls. `hooks` shares
/// the loaded recordings between the runs of one build (`None`: load here).
///
/// Both dumps come from one probe run ([`same_run_dirs`]): PoB's state differs
/// between processes (calc doc §9.5), so dumps of two runs do not pair.
pub fn segment_run(
    build: &Path,
    stage_in: &str,
    stage_out: &str,
    hooks: Option<&Rc<HookCalls>>,
) -> SegmentRun {
    let stem = build
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or_default()
        .to_owned();
    let dirs = same_run_dirs(&dump_dirs(&stem, Some(build)), &[stage_in, stage_out]);
    segment_run_in(&dirs, build, stage_in, stage_out, hooks)
}

/// [`segment_run`] on the dumps of `dirs` (e.g. one probe run's directory; `hooks`
/// should index the same directories).
pub fn segment_run_in(
    dirs: &[PathBuf],
    build: &Path,
    stage_in: &str,
    stage_out: &str,
    hooks: Option<&Rc<HookCalls>>,
) -> SegmentRun {
    let fail = |m: String| SegmentRun {
        mismatches: vec![m],
        ..SegmentRun::default()
    };
    if !init_default_data() {
        return fail("no data dump (data/dump)".to_owned());
    }
    let stem = build
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or_default()
        .to_owned();
    let dirs = dirs.to_vec();
    let (Some(pin), Some(pout)) = (find_dump(&dirs, stage_in), find_dump(&dirs, stage_out)) else {
        return fail(format!(
            "missing dump {stage_in} or {stage_out} for {stem} (fill with: scripts/port/oracle.sh run --build {} --stage {stage_in},{stage_out})",
            build.display()
        ));
    };
    let (Ok(din), Ok(dout)) = (StateDump::load(&pin), StateDump::load(&pout)) else {
        return fail(format!(
            "unreadable dump {} / {}",
            pin.display(),
            pout.display()
        ));
    };
    let model = load_build(build).unwrap_or_else(empty_build);
    let inj = match crate::inject::inject(&din, &model) {
        Ok(i) => i,
        Err(e) => return fail(format!("inject {stage_in}: {e}")),
    };
    let locals: Vec<(String, Val)> = inj.locals.into_iter().collect();
    let hooks = hooks
        .cloned()
        .unwrap_or_else(|| Rc::new(HookCalls::new(dirs.clone())));
    let report = Rc::new(RefCell::new(SegmentRun::default()));
    if let (Some(env), Some(enclosing)) = (inj.env.clone(), din.stack.last().cloned()) {
        let before = hooks.calls_before(&din.hit);
        let report = report.clone();
        let hooks = hooks.clone();
        let mut used: BTreeSet<String> = BTreeSet::new();
        let handler = move |skill: &pob_calc::active_skill::ActiveSkillRef,
                            hook: pob_calc::data_fns::SkillHook|
              -> bool {
            let ge = skill.activeEffect.get().grantedEffect.get();
            if hook.of(&ge).is_none() {
                return false;
            }
            let name = format!("{hook:?}");
            let next = hooks
                .calls(&enclosing, &name)
                .into_iter()
                .find(|c| !used.contains(&c.hit) && !before.contains(&c.hit));
            let Some(call) = next else {
                report.borrow_mut().hook_notes.push(format!(
                    "{name} of {}: no recorded runSkillFunc call under {enclosing}",
                    ge.name
                ));
                return false;
            };
            used.insert(call.hit.clone());
            match call.load() {
                Some(c) => {
                    let hit = c.din.hit.clone();
                    let mut calls = BTreeMap::new();
                    calls.insert(hit.clone(), c);
                    let replay = crate::replay::Replay { calls };
                    match replay.apply_report(&hit, &env) {
                        Ok(a) => {
                            let mut r = report.borrow_mut();
                            r.hook_notes
                                .extend(a.notes.into_iter().map(|n| format!("{hit}: {n}")));
                            r.hooks_replayed.push(hit);
                        }
                        Err(e) => report.borrow_mut().hook_notes.push(format!("{hit}: {e}")),
                    }
                }
                None => report
                    .borrow_mut()
                    .hook_notes
                    .push(format!("{}: unreadable runSkillFunc dumps", call.hit)),
            }
            true
        };
        pob_calc::probe::set_skill_hook_handler(Some(Box::new(handler)));
    }
    let run = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        pob_calc::probe::run_segment(&din.probe, &locals)
    }));
    pob_calc::probe::set_skill_hook_handler(None);
    let mut out = std::mem::take(&mut *report.borrow_mut());
    let after = match run {
        Ok(Ok((_flow, after))) => after,
        Ok(Err(e)) => {
            out.mismatches = vec![format!("run {stage_in}: {e}")];
            return out;
        }
        Err(p) => {
            let msg = p
                .downcast_ref::<pob_lua::LuaError>()
                .map(|e| e.0.clone())
                .or_else(|| p.downcast_ref::<String>().cloned())
                .or_else(|| p.downcast_ref::<&str>().map(|s| (*s).to_owned()))
                .unwrap_or_else(|| "panic".to_owned());
            out.mismatches = vec![format!("run {stage_in}: {msg}")];
            return out;
        }
    };
    let env = after
        .iter()
        .find(|(k, _)| k == "env")
        .and_then(|(_, v)| v.obj::<pob_calc::env::Env>())
        .or(inj.env);
    out.mismatches = diff_against(&dout, env.as_ref(), &after, &stem)
        .into_iter()
        .map(|m| format!("{} {:?}: pob={:?} ours={:?}", m.path, m.kind, m.pob, m.ours))
        .collect();
    out
}

/// The first of `dirs` (one probe run each) that holds every hit of `hits`, alone; all of
/// `dirs` when none does.
pub fn same_run_dirs(dirs: &[PathBuf], hits: &[&str]) -> Vec<PathBuf> {
    for d in dirs {
        let one = std::slice::from_ref(d);
        if hits.iter().all(|h| find_dump(one, h).is_some()) {
            return vec![d.clone()];
        }
    }
    dirs.to_vec()
}

/// Every dumped hit of `probe` (`offence:362` → `offence:362#1`, `#2`, …) in `dirs`, by `n`.
pub fn hits_of(dirs: &[PathBuf], probe: &str) -> Vec<String> {
    let prefix = format!(
        "{}#",
        crate::runner::hit_file_name(probe).trim_end_matches(".json")
    );
    let mut ns: BTreeSet<u64> = BTreeSet::new();
    for d in dirs {
        let Ok(rd) = std::fs::read_dir(d) else {
            continue;
        };
        for e in rd.filter_map(Result::ok) {
            let name = e.file_name().to_string_lossy().into_owned();
            let Some(rest) = name.strip_prefix(&prefix) else {
                continue;
            };
            let n: String = rest.chars().take_while(char::is_ascii_digit).collect();
            if let Ok(n) = n.parse::<u64>() {
                ns.insert(n);
            }
        }
    }
    ns.into_iter().map(|n| format!("{probe}#{n}")).collect()
}

// ---- skill hooks (CCR-0041) ----------------------------------------------------------------------

/// One recorded `runSkillFunc` call.
#[derive(Clone, Debug)]
pub struct HookCall {
    /// `runSkillFunc:in#n`.
    pub hit: String,
    pub n: u64,
    /// The `name` local (`initialFunc`, `preDamageFunc`, …).
    pub name: String,
    pub din: PathBuf,
    pub dout: Option<PathBuf>,
}

impl HookCall {
    /// The `:in`/`:out` dumps (None when one is missing or unreadable).
    pub fn load(&self) -> Option<crate::replay::Call> {
        Some(crate::replay::Call {
            din: StateDump::load(&self.din).ok()?,
            dout: StateDump::load(self.dout.as_ref()?).ok()?,
        })
    }
}

/// The recorded `runSkillFunc` calls of one build's dump directories, indexed
/// on first use by their enclosing function-probe hit (`offence:in#4`).
pub struct HookCalls {
    dirs: Vec<PathBuf>,
    index: OnceCell<BTreeMap<String, Vec<HookCall>>>,
    order: OnceCell<Option<BTreeMap<String, usize>>>,
}

impl HookCalls {
    pub fn new(dirs: Vec<PathBuf>) -> Self {
        HookCalls {
            dirs,
            index: OnceCell::new(),
            order: OnceCell::new(),
        }
    }

    fn index(&self) -> &BTreeMap<String, Vec<HookCall>> {
        self.index.get_or_init(|| {
            let mut by: BTreeMap<String, Vec<HookCall>> = BTreeMap::new();
            let mut seen: BTreeSet<String> = BTreeSet::new();
            for hit in hits_of(&self.dirs, "runSkillFunc:in") {
                if !seen.insert(hit.clone()) {
                    continue;
                }
                let Some(pin) = find_dump(&self.dirs, &hit) else {
                    continue;
                };
                let Ok(d) = StateDump::load(&pin) else {
                    continue;
                };
                let name = d
                    .locals()
                    .and_then(|l| l.get("name"))
                    .and_then(J::as_str)
                    .unwrap_or_default()
                    .to_owned();
                let out_hit = hit.replacen("runSkillFunc:in#", "runSkillFunc:out#", 1);
                let call = HookCall {
                    hit: hit.clone(),
                    n: d.n,
                    name,
                    din: pin,
                    dout: find_dump(&self.dirs, &out_hit),
                };
                by.entry(d.stack.last().cloned().unwrap_or_default())
                    .or_default()
                    .push(call);
            }
            for v in by.values_mut() {
                v.sort_by_key(|c| c.n);
            }
            by
        })
    }

    /// The recorded calls named `name` inside the enclosing hit, in call order.
    pub fn calls(&self, enclosing: &str, name: &str) -> Vec<HookCall> {
        self.index()
            .get(enclosing)
            .map(|v| v.iter().filter(|c| c.name == name).cloned().collect())
            .unwrap_or_default()
    }

    /// The `runSkillFunc:in` hits a `hits.json` of the dump directories lists before `hit`
    /// (empty when no index lists `hit`).
    pub fn calls_before(&self, hit: &str) -> BTreeSet<String> {
        let order = self.order.get_or_init(|| {
            for d in &self.dirs {
                for f in ["hits.json", "hits.json.zst"] {
                    let p = d.join(f);
                    let alt = d.parent().map(|q| q.join(f));
                    for p in std::iter::once(p).chain(alt) {
                        if let Ok(h) = crate::schema::HitsIndex::load(&p) {
                            return Some(
                                h.dumps
                                    .iter()
                                    .enumerate()
                                    .map(|(i, e)| (e.hit.clone(), i))
                                    .collect(),
                            );
                        }
                    }
                }
            }
            None
        });
        let Some(order) = order else {
            return BTreeSet::new();
        };
        let Some(&at) = order.get(hit) else {
            return BTreeSet::new();
        };
        order
            .iter()
            .filter(|(h, &i)| i < at && h.starts_with("runSkillFunc:in#"))
            .map(|(h, _)| h.clone())
            .collect()
    }
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
