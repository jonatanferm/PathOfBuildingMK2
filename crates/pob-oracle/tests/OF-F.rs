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
//! `actor.output` / `actor.breakdown`, so the test sets `OFFENCE_STATE` from the injected actor
//! (CCR-0075). `passList` is rebuilt as `Shared<Vec<Table>>` (CCR-0034, OF-D).
//!
//! Callees owned by other units may still be `unported!` (OF-A `calcDamage`/`calcResistForType`,
//! OF-C `combineStat`, DF-A `armourReductionF`): a hit whose run reaches such a stub is reported
//! as **stub-blocked** with the stub sites and is not gated. The gate: every hit that ran without
//! hitting a stub has zero mismatches, and at least one hit per leg ran stub-free.
//!
//! Corpus: slice5 (`corpus/U0/slice5/*.xml`), the unit corpus (`corpus/OF-F/*.xml`) and every
//! all57 build (`.parity/builds/*.xml`) whose dumps are present. Dumps:
//! `scripts/port/oracle.sh run --build X.xml --stage 'offence:2607*,offence:3346*,offence:3957*,offence:4155*'`.
//! Knobs: `OFF_BUILDS=stem1,stem2`, `OFF_VERBOSE=1` (print every mismatch).

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

/// Every dump of the four OF-F probes for a build, keyed by (probe, n). All dumps come from one
/// directory (one oracle process: hit numbers and PoB's per-process orders only agree within a
/// run); the directory with the most OF-F dumps wins.
fn dumps(xml: &Path) -> BTreeMap<(String, u64), StateDump> {
    let mut best: BTreeMap<(String, u64), PathBuf> = BTreeMap::new();
    for d in testkit::dump_dirs(stem(xml), Some(xml)) {
        let Ok(rd) = std::fs::read_dir(&d) else {
            continue;
        };
        let mut files: BTreeMap<(String, u64), PathBuf> = BTreeMap::new();
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
        if files.len() > best.len() {
            best = files;
        }
    }
    let mut out = BTreeMap::new();
    for (key, path) in best {
        if let Ok(s) = StateDump::load(&path) {
            out.insert(key, s);
        }
    }
    out
}

/// The probe that follows `din` in the same `calcs.offence` call.
fn next_dump<'a>(
    all: &'a BTreeMap<(String, u64), StateDump>,
    din: &StateDump,
) -> Option<&'a StateDump> {
    let same = |d: &&StateDump| d.stack == din.stack && d.env_id == din.env_id;
    match din.probe.as_str() {
        "offence:3346" => all
            .get(&("offence:2607".to_owned(), din.n + 1))
            .filter(same)
            .or_else(|| {
                all.iter()
                    .filter(|((p, _), _)| p == "offence:3957")
                    .map(|(_, d)| d)
                    .find(same)
            }),
        "offence:3957" => all
            .iter()
            .filter(|((p, _), _)| p == "offence:4155")
            .map(|(_, d)| d)
            .find(same),
        _ => None,
    }
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
    let mut locals: Vec<(String, Val)> = inj.locals.into_iter().collect();
    // CCR-0034 (OF-D): `passList` is injected as a plain table; the ctx field is a
    // `Shared<Vec<Table>>`. Rebuild it from the same pass tables (identity kept).
    for (k, v) in locals.iter_mut() {
        if k == "passList" {
            if let Some(t) = v.table() {
                let list: Vec<pob_calc::lua_table::Table> =
                    t.ipairs().into_iter().filter_map(|(_, p)| p.table()).collect();
                *v = Val::Obj(std::rc::Rc::new(pob_calc::probe::SBox(pob_lua::Shared::new(list))));
            }
        }
    }
    // globalOutput, globalBreakdown = output, breakdown (CalcOffence.lua:2607): the actor's.
    let actor = locals
        .iter()
        .find(|(k, _)| k == "actor")
        .and_then(|(_, v)| v.obj::<pob_calc::actor::Actor>());
    let Some(actor) = actor else {
        return Outcome::Error(format!("{}: local actor is not an actor", din.hit));
    };
    pob_calc::module_state::OFFENCE_STATE.with(|s| {
        s.globalOutput.set(Some(actor.output.get()));
        s.globalBreakdown.set(actor.breakdown.get());
    });
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
            .and_then(|(_, v)| v.obj::<pob_calc::probe::SBox<Vec<Table>>>())
            .map(|b| b.0.with(|x| x.clone()));
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
    pass: usize,
    fail: usize,
    blocked: usize,
    errors: usize,
    foreign: usize,
    missing_out: usize,
    blocked_sites: BTreeMap<String, usize>,
}

/// The stub counters (`pob_calc::stub`) are process-wide: one leg at a time.
static LEG_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

fn run_leg(probe_in: &str) -> Option<Tally> {
    let _guard = LEG_LOCK
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    testkit::init_default_data()
        .then_some(())
        .or_else(|| pob_testpaths::require(None::<()>, "data dump (data/dump)"))?;
    let mut t = Tally::default();
    let verbose = std::env::var_os("OFF_VERBOSE").is_some();
    let mut failures = Vec::new();
    for xml in builds() {
        let stem = stem(&xml).to_owned();
        let all = dumps(&xml);
        for ((probe, _), din) in &all {
            if probe != probe_in {
                continue;
            }
            let Some(dout) = next_dump(&all, din) else {
                t.missing_out += 1;
                continue;
            };
            match run_hit(din, dout, &stem) {
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
        "OF-F {probe_in}: pass {} / fail {} / stub-blocked {} / foreign-parent {} / errors {} / no next dump {}",
        t.pass, t.fail, t.blocked, t.foreign, t.errors, t.missing_out
    );
    for (s, n) in &t.blocked_sites {
        eprintln!("  stub {s}: blocked {n} hits");
    }
    for f in &failures {
        eprintln!("{f}");
    }
    Some(t)
}

fn gate(t: &Tally, leg: &str) {
    let ran = t.pass + t.fail + t.blocked + t.foreign + t.errors;
    if ran == 0 {
        let _ = pob_testpaths::require(
            None::<()>,
            &format!("{leg} dumps (scripts/port/oracle.sh run --build X.xml --stage 'offence:2607*,offence:3346*,offence:3957*,offence:4155*')"),
        );
        return;
    }
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
        return;
    };
    gate(&t, "offence:3346");
}

/// L4 `offence:3957` → `offence:4155`.
#[test]
fn off_combine_passes_l4() {
    let Some(t) = run_leg("offence:3957") else {
        return;
    };
    gate(&t, "offence:3957");
}
