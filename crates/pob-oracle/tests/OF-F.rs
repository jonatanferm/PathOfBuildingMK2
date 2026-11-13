//! Unit OF-F acceptance (calc doc §11.1 row OF-F, §9.4 L4): CalcOffence.lua 3346-3955 (hit pass
//! body B, `hit_dps::seg_passHit`) and 3956-4154 (pass combination, `hit_dps::seg_combinePasses`).
//!
//! * **passHit**: every `offence:3346#m` dump is injected, the Rust segment runs, and the state is
//!   diffed against the next probe of the same `calcs.offence` call: `offence:2607#(m+1)` (the next
//!   pass) or `offence:3957` (after the last pass). After the pass body the locals in scope are the
//!   function's, so the exported `output`/`breakdown` are `actor.output`/`actor.breakdown` and, at
//!   `2607`, `pass` is the next `passList` entry; the pass-body locals (`source`, `cfg`,
//!   `hitRate`) are dropped. All dumps of a build come from one oracle run (one directory).
//! * **combinePasses**: every `offence:3957#k` dump against `offence:4155` of the same call.
//!
//! The module upvalues `globalOutput`/`globalBreakdown` (CalcOffence.lua:65/67) are not part of a
//! dump; at both stages they are the function's `output`/`breakdown` (assigned at 2607), i.e.
//! `actor.output` / `actor.breakdown`, which `probe::run_segment` sets from the injected actor
//! (CCR-0075); it also accepts the dumped `passList` table as the ctx field (CCR-0036).
//!
//! Callees owned by other units may still be `unported!` (OF-A `calcDamage`/`calcResistForType`,
//! OF-C `combineStat`, DF-A `armourReductionF`): a hit whose run reaches such a stub is reported
//! as **stub-blocked** with the stub sites and is not gated. The gate: every hit that ran without
//! hitting a stub has zero mismatches, and at least one hit per leg ran stub-free.
//!
//! Corpus: slice5 (`corpus/U0/slice5/*.xml`), the unit corpus (`corpus/OF-F/*.xml`) and every
//! all57 build (`.parity/builds/*.xml`). Dumps: one `pob_oracle probe` run per build selecting
//! `offence:2607*,offence:3346*,offence:3957*,offence:4155*` under `docs/port/stages.toml`, from
//! the shared probe cache (`testkit::probe_dumps`, CK2: keyed by the patched source, the build's
//! bytes, the selection and the probe code; probed here when absent; OF-E shares the entry).
//! Knobs: `OFF_BUILDS=stem1,stem2`, `OFF_VERBOSE=1` (print every mismatch).
//!
//! CK3 (CCR-0148): a probe that fails or dumps nothing for a build, and a hit without its next
//! dump, fail the leg; so does a leg that compared no hit. The only skip is a missing input
//! (`data/dump`, the PoB checkout, `.parity`: `pob_testpaths::require`, which also fails under
//! `POB_ORACLE_REQUIRE=1`). Each leg reports how many hits it compared. The dumps are read one
//! hit at a time (the run's `hits.json` gives each dump's stack and env), not a build at once
//! (that peaked at 3.5 GB RSS on the full corpus).

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use pob_calc::lua_table::Table;
use pob_calc::Val;
use pob_oracle::schema::StateDump;
use pob_oracle::testkit;

const PASS_LOCALS: &[&str] = &["pass", "source", "cfg", "hitRate", "output", "breakdown"];

fn builds() -> Vec<PathBuf> {
    let mut v = Vec::new();
    let here = Path::new(env!("CARGO_MANIFEST_DIR"));
    for dir in [here.join("corpus/U0/slice5"), here.join("corpus/OF-F")] {
        if let Ok(rd) = std::fs::read_dir(&dir) {
            let mut b: Vec<PathBuf> = rd
                .filter_map(Result::ok)
                .map(|e| e.path())
                .filter(|p| p.extension().is_some_and(|x| x == "xml"))
                .collect();
            b.sort();
            v.extend(b);
        }
    }
    if let Some(parity) = pob_testpaths::parity_dir() {
        if let Ok(rd) = std::fs::read_dir(parity.join("builds")) {
            let mut b: Vec<PathBuf> = rd
                .filter_map(Result::ok)
                .map(|e| e.path())
                .filter(|p| p.extension().is_some_and(|x| x == "xml"))
                .collect();
            b.sort();
            v.extend(b);
        }
    }
    if let Ok(list) = std::env::var("OFF_BUILDS") {
        let want: Vec<&str> = list.split(',').map(str::trim).collect();
        v.retain(|p| want.contains(&stem(p)));
    }
    v
}

fn stem(p: &Path) -> &str {
    p.file_stem().and_then(|s| s.to_str()).unwrap_or_default()
}

/// The probe selection (module docs).
const SELECT: &[&str] = &["offence:2607*", "offence:3346*", "offence:3957*", "offence:4155*"];

/// The OF-F dumps of one build (all from one directory, i.e. one oracle process: hit numbers
/// and PoB's per-process orders only agree within a run), by (probe, n), with each dump's call
/// stack and env from the run's `hits.json` (else from the dump itself), loaded on demand.
struct Dumps {
    /// Held while the dumps are read (a private run under `POB_QUERY_CACHE=off`).
    _held: testkit::BuildDumps,
    files: BTreeMap<(String, u64), PathBuf>,
    meta: BTreeMap<(String, u64), (Vec<String>, Option<String>)>,
}

impl Dumps {
    fn load(&self, key: &(String, u64)) -> Result<StateDump, String> {
        let p = self
            .files
            .get(key)
            .ok_or_else(|| format!("no dump {}#{}", key.0, key.1))?;
        StateDump::load(p).map_err(|e| e.0)
    }

    /// The probe that follows `din` in the same `calcs.offence` call.
    fn next(&self, din: &StateDump) -> Option<(String, u64)> {
        let same = |k: &(String, u64)| {
            self.meta
                .get(k)
                .is_some_and(|(stack, env)| *stack == din.stack && *env == din.env_id)
        };
        let first_of = |probe: &str| {
            self.files
                .keys()
                .filter(|(p, _)| p == probe)
                .find(|k| same(k))
                .cloned()
        };
        match din.probe.as_str() {
            "offence:3346" => Some(("offence:2607".to_owned(), din.n + 1))
                .filter(|k| self.files.contains_key(k) && same(k))
                .or_else(|| first_of("offence:3957")),
            "offence:3957" => first_of("offence:4155"),
            _ => None,
        }
    }
}

/// The OF-F dumps of `xml` ([`SELECT`], from the shared probe cache, probed when absent).
/// `Err` when the probe fails or dumps none of the four probes.
fn dumps(xml: &Path) -> Result<Dumps, String> {
    let held = testkit::probe_dumps(
        Path::new(env!("CARGO_BIN_EXE_pob_oracle")),
        xml,
        Some(&testkit::default_stages()),
        &testkit::selection(SELECT),
    )
    .map_err(|e| format!("probe failed: {e}"))?;
    let mut files: BTreeMap<(String, u64), PathBuf> = BTreeMap::new();
    let rd = std::fs::read_dir(held.path())
        .map_err(|e| format!("probe dumps {}: {e}", held.path().display()))?;
    for e in rd.filter_map(Result::ok) {
        let name = e.file_name().to_string_lossy().into_owned();
        let base = name
            .strip_suffix(".json.zst")
            .or_else(|| name.strip_suffix(".json"))
            .unwrap_or("");
        let Some((probe, n)) = base.split_once('#') else {
            continue;
        };
        if !matches!(
            probe,
            "offence:2607" | "offence:3346" | "offence:3957" | "offence:4155"
        ) {
            continue;
        }
        let Ok(n) = n.parse::<u64>() else {
            continue;
        };
        files.insert((probe.to_owned(), n), e.path());
    }
    if files.is_empty() {
        return Err(format!(
            "empty probe: no dump of {SELECT:?} in {}",
            held.path().display()
        ));
    }
    // each dump's stack and env: from the run's index, else (an override directory without
    // one) from the dump, loaded and dropped one at a time
    let run = held.run_dir();
    let index = ["hits.json.zst", "hits.json"]
        .iter()
        .map(|f| run.join(f))
        .find(|p| p.is_file())
        .and_then(|p| pob_oracle::schema::HitsIndex::load(&p).ok());
    let mut meta = BTreeMap::new();
    if let Some(index) = index {
        for d in index.dumps {
            if let Some((probe, n)) = d.hit.split_once('#') {
                if let Ok(n) = n.parse::<u64>() {
                    meta.insert((probe.to_owned(), n), (d.stack, d.env));
                }
            }
        }
    }
    for (key, path) in &files {
        if !meta.contains_key(key) {
            let s = StateDump::load(path).map_err(|e| format!("{}: {e}", path.display()))?;
            meta.insert(key.clone(), (s.stack, s.env_id));
        }
    }
    Ok(Dumps {
        _held: held,
        files,
        meta,
    })
}

enum Outcome {
    Pass,
    Fail(Vec<String>),
    StubBlocked(Vec<String>, usize),
    /// Mismatches on a hit whose skill mod list inherits from another env's store (below).
    Foreign(usize),
    Error(String),
}

/// True when the `skillModList` parent chain of `din` reaches a `$foreign` store: PoB reused an
/// active skill built in another env (its `baseSkillModList.parent` is that env's `modDB`), which
/// the dump records only as a `{"$foreign": {env, path}}` marker, so injection cannot rebuild the
/// mods that store contributes (seen on allflame_exp_013/036/051, upstream_Dual_Savior,
/// upstream_Mirage_Archer_Toxic_Rain). A mismatch there is an inject limit, not a port result.
fn foreign_parent(din: &StateDump) -> bool {
    use serde_json::Value as J;
    fn index<'a>(v: &'a J, ids: &mut BTreeMap<String, &'a J>) {
        match v {
            J::Object(m) => {
                if let Some(id) = m.get("$id") {
                    let k = id.as_str().map(str::to_owned).unwrap_or_else(|| id.to_string());
                    ids.insert(k, v);
                }
                for x in m.values() {
                    index(x, ids);
                }
            }
            J::Array(a) => a.iter().for_each(|x| index(x, ids)),
            _ => {}
        }
    }
    let mut ids = BTreeMap::new();
    index(&din.root, &mut ids);
    let resolve = |v: &'_ J| -> Option<J> {
        let mut v = v.clone();
        for _ in 0..8 {
            match v.get("$ref") {
                Some(r) => {
                    let k = r.as_str().map(str::to_owned).unwrap_or_else(|| r.to_string());
                    v = (*ids.get(&k)?).clone();
                }
                None => return Some(v),
            }
        }
        Some(v)
    };
    let Some(mut cur) = din
        .root
        .get("locals")
        .and_then(|l| l.get("skillModList"))
        .and_then(|v| resolve(v))
    else {
        return false;
    };
    for _ in 0..8 {
        if cur.get("$foreign").is_some() {
            return true;
        }
        match cur.get("parent").and_then(|p| resolve(p)) {
            Some(p) => cur = p,
            None => return false,
        }
    }
    false
}

fn run_hit(din: &StateDump, dout: &StateDump, stem: &str) -> Outcome {
    let build = testkit::empty_build();
    let inj = match pob_oracle::inject::inject(din, &build) {
        Ok(i) => i,
        Err(e) => return Outcome::Error(format!("inject {}: {e}", din.hit)),
    };
    let locals: Vec<(String, Val)> = inj.locals.into_iter().collect();
    // `run_segment` sets `globalOutput`/`globalBreakdown` (CalcOffence.lua:2607) from the
    // injected actor (CCR-0075) and accepts the plain `passList` table (CCR-0036).
    let actor = locals
        .iter()
        .find(|(k, _)| k == "actor")
        .and_then(|(_, v)| v.obj::<pob_calc::actor::Actor>());
    let Some(actor) = actor else {
        return Outcome::Error(format!("{}: local actor is not an actor", din.hit));
    };
    pob_calc::stub::stub_reset();
    let run = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        pob_calc::probe::run_segment(&din.probe, &locals)
    }));
    let stubs: Vec<String> = pob_calc::stub::stub_report()
        .iter()
        .map(|(site, n)| format!("{site} x{n}"))
        .collect();
    let mut after = match run {
        Ok(Ok((_flow, after))) => after,
        Ok(Err(e)) => return Outcome::Error(format!("run {}: {e}", din.hit)),
        Err(p) => {
            let msg = p
                .downcast_ref::<pob_lua::LuaError>()
                .map(|e| e.0.clone())
                .or_else(|| p.downcast_ref::<String>().cloned())
                .or_else(|| p.downcast_ref::<&str>().map(|s| (*s).to_owned()))
                .unwrap_or_else(|| "panic".to_owned());
            if !stubs.is_empty() {
                return Outcome::StubBlocked(stubs, 1);
            }
            return Outcome::Error(format!("run {}: {msg}", din.hit));
        }
    };
    let env = after
        .iter()
        .find(|(k, _)| k == "env")
        .and_then(|(_, v)| v.obj::<pob_calc::env::Env>())
        .or(inj.env);
    if din.probe == "offence:3346" {
        // After the pass body the locals in scope are the function's: `output`/`breakdown` are
        // `actor.output`/`actor.breakdown` (354/356) and, at `2607` of the next pass, `pass` is
        // the next entry of `passList`. The pass-body locals (`source`, `cfg`, `hitRate`) are
        // out of scope (no probe after 3955 lists them).
        let pass_list: Option<Vec<Table>> = locals
            .iter()
            .find(|(k, _)| k == "passList")
            .and_then(|(_, v)| v.table())
            .map(|t| t.ipairs().into_iter().filter_map(|(_, p)| p.table()).collect());
        let pass = locals
            .iter()
            .find(|(k, _)| k == "pass")
            .and_then(|(_, v)| v.table());
        let next_pass = match (&pass_list, &pass) {
            (Some(l), Some(p)) => l
                .iter()
                .position(|t| t.ptr_eq(p))
                .and_then(|i| l.get(i + 1).cloned()),
            _ => None,
        };
        after.retain(|(k, _)| !PASS_LOCALS.contains(&k.as_str()));
        after.push(("output".to_owned(), Val::Table(actor.output.get())));
        if let Some(b) = actor.breakdown.get() {
            after.push(("breakdown".to_owned(), Val::Obj(std::rc::Rc::new(b))));
        }
        if let Some(p) = next_pass {
            after.push(("pass".to_owned(), Val::Table(p)));
        }
    }
    let mm = testkit::diff_against(dout, env.as_ref(), &after, stem);
    let lines: Vec<String> = mm
        .iter()
        .map(|m| {
            format!(
                "{} {:?}: pob={:?} ours={:?}{}",
                m.path,
                m.kind,
                m.pob,
                m.ours,
                m.writer.as_deref().map(|w| format!(" [{w}]")).unwrap_or_default()
            )
        })
        .collect();
    match (lines.is_empty(), stubs.is_empty()) {
        (true, _) => Outcome::Pass,
        (false, true) if foreign_parent(din) => Outcome::Foreign(lines.len()),
        (false, true) => Outcome::Fail(lines),
        (false, false) => {
            let n = lines.len();
            if std::env::var_os("OFF_VERBOSE").is_some() {
                for l in &lines {
                    eprintln!("    (stub-blocked) {l}");
                }
            }
            Outcome::StubBlocked(stubs, n)
        }
    }
}

#[derive(Default)]
struct Tally {
    builds: usize,
    pass: usize,
    fail: usize,
    blocked: usize,
    errors: usize,
    foreign: usize,
    /// CK3: `<stem> <hit>` whose next dump the run does not hold.
    missing_out: Vec<String>,
    /// CK3: `<stem>: <why>` of builds whose probe failed or dumped nothing.
    probe_failed: Vec<String>,
    blocked_sites: BTreeMap<String, usize>,
}

impl Tally {
    /// Hits injected, run and diffed (the non-vacuity count).
    fn compared(&self) -> usize {
        self.pass + self.fail + self.blocked + self.foreign + self.errors
    }
}

/// The stub counters (`pob_calc::stub`) are process-wide: one leg at a time.
static LEG_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

/// `pob_testpaths::require` (fails, or skips with `POB_ALLOW_SKIP=1`), failing under
/// `POB_ORACLE_REQUIRE=1` / `CI` even with `POB_ALLOW_SKIP=1` (CK3).
#[track_caller]
fn require<T>(v: Option<T>, what: &str) -> Option<T> {
    assert!(
        v.is_some() || !testkit::oracle_required(),
        "required input missing (POB_ORACLE_REQUIRE): {what}"
    );
    pob_testpaths::require(v, what)
}

fn run_leg(probe_in: &str) -> Option<Tally> {
    let _guard = LEG_LOCK
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    require(testkit::init_default_data().then_some(()), "data dump (data/dump)")?;
    let why = testkit::probe_inputs().err();
    require(why.is_none().then_some(()), why.as_deref().unwrap_or_default())?;
    let mut t = Tally::default();
    let verbose = std::env::var_os("OFF_VERBOSE").is_some();
    let mut failures = Vec::new();
    for xml in builds() {
        let stem = stem(&xml).to_owned();
        t.builds += 1;
        let all = match dumps(&xml) {
            Ok(d) => d,
            Err(e) => {
                t.probe_failed.push(format!("{stem}: {e}"));
                continue;
            }
        };
        let ins: Vec<(String, u64)> = all
            .files
            .keys()
            .filter(|(p, _)| p == probe_in)
            .cloned()
            .collect();
        for key in ins {
            // one hit's pair at a time (module docs)
            let din = match all.load(&key) {
                Ok(d) => d,
                Err(e) => {
                    t.errors += 1;
                    failures.push(format!("{stem} {}#{}: {e}", key.0, key.1));
                    continue;
                }
            };
            let Some(out_key) = all.next(&din) else {
                t.missing_out.push(format!("{stem} {}", din.hit));
                continue;
            };
            let dout = match all.load(&out_key) {
                Ok(d) => d,
                Err(e) => {
                    t.errors += 1;
                    failures.push(format!("{stem} {}#{}: {e}", out_key.0, out_key.1));
                    continue;
                }
            };
            match run_hit(&din, &dout, &stem) {
                Outcome::Pass => t.pass += 1,
                Outcome::Fail(lines) => {
                    t.fail += 1;
                    failures.push(format!("{stem} {} -> {}: {} mismatches", din.hit, dout.hit, lines.len()));
                    let show = if verbose { lines.len() } else { 12 };
                    for l in lines.iter().take(show) {
                        failures.push(format!("    {l}"));
                    }
                }
                Outcome::StubBlocked(sites, n) => {
                    t.blocked += 1;
                    eprintln!("  stub-blocked {stem} {} -> {} ({n} mismatches)", din.hit, dout.hit);
                    for s in sites {
                        *t.blocked_sites.entry(s.split(" x").next().unwrap_or("").to_owned()).or_default() += 1;
                    }
                }
                Outcome::Foreign(n) => {
                    t.foreign += 1;
                    eprintln!(
                        "  foreign-parent {stem} {} -> {} ({n} mismatches; inject limit)",
                        din.hit, dout.hit
                    );
                }
                Outcome::Error(e) => {
                    t.errors += 1;
                    failures.push(format!("{stem}: {e}"));
                }
            }
        }
    }
    eprintln!(
        "OF-F {probe_in}: {} builds, {} hits compared: pass {} / fail {} / stub-blocked {} / foreign-parent {} / errors {}; no next dump {}, probe failed or empty {}",
        t.builds,
        t.compared(),
        t.pass,
        t.fail,
        t.blocked,
        t.foreign,
        t.errors,
        t.missing_out.len(),
        t.probe_failed.len()
    );
    for (s, n) in &t.blocked_sites {
        eprintln!("  stub {s}: blocked {n} hits");
    }
    for f in &t.probe_failed {
        eprintln!("  PROBE {f}");
    }
    for m in &t.missing_out {
        eprintln!("  NO NEXT DUMP {m}");
    }
    for f in &failures {
        eprintln!("{f}");
    }
    Some(t)
}

fn gate(t: &Tally, leg: &str) {
    // CK3 (CCR-0148): a failed or empty probe, or a hit without its pair, fails; never a skip
    assert!(
        t.probe_failed.is_empty(),
        "{leg}: {} builds' probes failed or dumped nothing (first: {:?})",
        t.probe_failed.len(),
        t.probe_failed.first()
    );
    assert!(
        t.missing_out.is_empty(),
        "{leg}: {} hits without their next dump (first: {:?})",
        t.missing_out.len(),
        t.missing_out.first()
    );
    let ran = t.compared();
    assert!(ran > 0, "{leg}: no hit compared over {} builds", t.builds);
    assert_eq!(t.errors, 0, "{leg}: {} hits raised", t.errors);
    assert_eq!(t.fail, 0, "{leg}: {} stub-free hits mismatch", t.fail);
    assert!(t.pass > 0, "{leg}: no hit ran stub-free");
    // calc doc §9.4 L4: the remainder beyond 98 % is documented (here: the inject limit above).
    assert!(t.foreign * 50 <= ran, "{leg}: {} foreign-parent hits exceed 2 %", t.foreign);
}

/// L4 `offence:3346#m` → next pass (`offence:2607#m+1`) / `offence:3957`.
#[test]
fn off_pass_hit_l4() {
    let Some(t) = run_leg("offence:3346") else {
        return; // POB_ALLOW_SKIP=1
    };
    gate(&t, "offence:3346");
}

/// L4 `offence:3957` → `offence:4155`.
#[test]
fn off_combine_passes_l4() {
    let Some(t) = run_leg("offence:3957") else {
        return; // POB_ALLOW_SKIP=1
    };
    gate(&t, "offence:3957");
}
