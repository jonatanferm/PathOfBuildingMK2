//! OF-H acceptance (calc doc §11.1 row OF-H, §9.4 L4): CalcOffence.lua 4990-5648, the ailment
//! loop body B (`ailments_b::seg_igniteNonDmgImpale`: ignite, non-damaging ailments, knockback,
//! enemy stun, impale).
//!
//! Every `offence:4990#m` dump is injected, the Rust segment runs, and the state is diffed against
//! the next probe of the same `calcs.offence` call (same enclosing probe stack and env): the next
//! pass's `offence:4165#(m+1)` (4165 and 4990 both fire once per pass) or, after the last pass,
//! `offence:5650`. At the out stage the names `output`/`breakdown` denote the function's locals
//! (`actor.output`/`actor.breakdown`) and `pass` the next pass, so the test replaces the exported
//! loop locals by those before diffing (`cfg`/`ailmentMode` are not dumped there). Everything else
//! (env graph incl. every actor's output and breakdown, `passList`, `activeSkill`, `skillFlags`,
//! …) is compared as dumped.
//!
//! **Dumps.** An in/out pair must come from one PoB process (PoB's `pairs` order differs between
//! processes, and other units refill the shared probe cache). The test probes every build itself
//! (`pob_oracle probe`, which takes the machine-wide oracle lock) into
//! `$CARGO_TARGET_TMPDIR/OF-H/<stem>/` and reuses that directory while it is complete (delete it
//! to refresh). Selection: the main env's first hit of each of the three probes plus `#1..#12`.
//!
//! **State a dump does not carry** is set by `probe::run_segment` (ORC-X2): the plain `passList`
//! table is accepted as the ctx field (CCR-0036), `globalOutput`/`globalBreakdown`
//! (CalcOffence.lua:65/67) are the injected actor's `output`/`breakdown` (CCR-0075), and the
//! function-level `output`/`breakdown` of the ctx are the actor's while the loop locals fill the
//! pass locals (CCR-0049 §2).
//!
//! **Stubs.** Callees of other units may still be `unported!` (OF-A `calcAilmentSourceDamage` /
//! `calcResistForType` (ignite), DF-A `armourReductionF` (impale), PF-B `actionSpeedMod` (the
//! stun breakdown)). A hit whose run reaches such a stub is reported as **stub-blocked** with the
//! sites and is not gated. The gate: every hit that ran without reaching a stub has zero
//! mismatches, no hit raised, and at least one hit ran stub-free. A mismatching hit whose input
//! dump holds `$foreign` references (another env's state, e.g. a mirage skill whose
//! `baseSkillModList.parent` is the main env's modDB) is reported as **foreign-ref** and not
//! gated: injection cannot rebuild that state (ORC-X).
//! Knobs: `OFH_BUILDS=stem1,stem2`, `OFH_VERBOSE=1` (print every mismatch).

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use pob_calc::probe::ProbeField;
use pob_calc::Val;
use pob_oracle::schema::StateDump;
use pob_oracle::testkit;

/// Locals of the ailment loop body and the shadowing `output`/`breakdown` (replaced before the diff).
const PASS_LOCALS: &[&str] = &["pass", "cfg", "ailmentMode", "output", "breakdown"];

/// all57 builds: spells and attacks, fire / cold / lightning / physical, main hand and dual wield.
const PARITY_BUILDS: &[&str] = &[
    "allflame_exp_000",
    "allflame_exp_001",
    "allflame_exp_005",
    "allflame_exp_006",
    "allflame_exp_007",
    "allflame_exp_013",
    "allflame_exp_017",
    "allflame_exp_024",
    "allflame_exp_029",
    "allflame_exp_030",
    "allflame_exp_031",
    "allflame_exp_042",
    "allflame_exp_045",
    "allflame_exp_046",
    "allflame_exp_048",
    "allflame_exp_050",
    "upstream_Dual_Wield_Cospris_CoC",
    "upstream_Generals_Perforate_Zerker",
    "upstream_Mirage_Archer_Toxic_Rain",
];

fn builds() -> Vec<PathBuf> {
    let mut v = Vec::new();
    let here = Path::new(env!("CARGO_MANIFEST_DIR"));
    for dir in [here.join("corpus/U0/slice5"), here.join("corpus/OF-H")] {
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
    for stem in PARITY_BUILDS {
        if let Some(p) = pob_testpaths::parity_build(stem) {
            v.push(p);
        }
    }
    if let Ok(list) = std::env::var("OFH_BUILDS") {
        let want: Vec<&str> = list.split(',').map(str::trim).collect();
        v.retain(|p| want.contains(&stem(p)));
    }
    v
}

fn stem(p: &Path) -> &str {
    p.file_stem().and_then(|s| s.to_str()).unwrap_or_default()
}

/// The build's dumps from one `pob_oracle probe` run (made on first use).
fn ensure_dumps(xml: &Path, stem: &str) -> Result<PathBuf, String> {
    let dir = Path::new(env!("CARGO_TARGET_TMPDIR"))
        .join("OF-H")
        .join(stem);
    let state = dir.join("state");
    let done = dir.join("complete");
    if done.is_file() {
        return Ok(state);
    }
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).map_err(|e| format!("{}: {e}", dir.display()))?;
    let stages = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../docs/port/stages.toml");
    let mut sel: Vec<String> = ["4165", "4990", "5650"]
        .iter()
        .map(|l| format!("offence:{l}@main"))
        .collect();
    for k in 1..=12 {
        for l in ["4165", "4990", "5650"] {
            sel.push(format!("offence:{l}#{k}"));
        }
    }
    let mut cmd = std::process::Command::new(env!("CARGO_BIN_EXE_pob_oracle"));
    cmd.arg("probe")
        .arg(xml)
        .arg("--out-dir")
        .arg(&dir)
        .arg("--stages")
        .arg(&stages)
        .arg("--select")
        .arg(sel.join(","))
        .arg("--no-provenance")
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null());
    if let Some(src) = pob_testpaths::pob_src() {
        cmd.arg("--pob").arg(src);
    }
    let status = cmd.status().map_err(|e| format!("pob_oracle probe: {e}"))?;
    if !status.success() {
        return Err(format!("pob_oracle probe {} failed ({status})", xml.display()));
    }
    std::fs::write(&done, b"").map_err(|e| format!("{}: {e}", done.display()))?;
    Ok(state)
}

/// Every dump of the three probes in `dir`, keyed by (probe, n).
fn dumps(dir: &Path) -> BTreeMap<(String, u64), StateDump> {
    let mut out = BTreeMap::new();
    let Ok(rd) = std::fs::read_dir(dir) else {
        return out;
    };
    for e in rd.filter_map(Result::ok) {
        let name = e.file_name().to_string_lossy().into_owned();
        let base = name
            .strip_suffix(".json.zst")
            .or_else(|| name.strip_suffix(".json"))
            .unwrap_or("");
        let Some((probe, n)) = base.split_once('#') else {
            continue;
        };
        if !matches!(probe, "offence:4165" | "offence:4990" | "offence:5650") {
            continue;
        }
        let Ok(n) = n.parse::<u64>() else {
            continue;
        };
        if let Ok(s) = StateDump::load(&e.path()) {
            out.insert((probe.to_owned(), n), s);
        }
    }
    out
}

/// The probe that follows `din` (an `offence:4990` hit) in the same `calcs.offence` call.
fn next_dump<'a>(
    all: &'a BTreeMap<(String, u64), StateDump>,
    din: &StateDump,
) -> Option<&'a StateDump> {
    let same = |d: &&StateDump| d.stack == din.stack && d.env_id == din.env_id;
    all.get(&("offence:4165".to_owned(), din.n + 1))
        .filter(same)
        .or_else(|| {
            all.iter()
                .filter(|((p, _), _)| p == "offence:5650")
                .map(|(_, d)| d)
                .find(same)
        })
}

enum Outcome {
    Pass,
    /// The out dump is `offence:5650` but the pass is not the last one (the next pass's
    /// `offence:4165` was not dumped): no comparable pair.
    NoNext,
    /// Mismatches in a hit whose input dump holds `$foreign` references (state of another env,
    /// e.g. a mirage skill built in the main env), which injection cannot rebuild.
    Foreign(usize),
    Fail(Vec<String>),
    StubBlocked(Vec<String>, usize),
    Error(String),
}

fn panic_msg(p: &(dyn std::any::Any + Send)) -> String {
    p.downcast_ref::<pob_lua::LuaError>()
        .map(|e| e.0.clone())
        .or_else(|| p.downcast_ref::<String>().cloned())
        .or_else(|| p.downcast_ref::<&str>().map(|s| (*s).to_owned()))
        .unwrap_or_else(|| "panic".to_owned())
}

fn run_hit(xml: &Path, din: &StateDump, dout: &StateDump, stem: &str) -> Outcome {
    let build = testkit::load_build(xml).unwrap_or_else(testkit::empty_build);
    let inj = match pob_oracle::inject::inject(din, &build) {
        Ok(i) => i,
        Err(e) => return Outcome::Error(format!("inject {}: {e}", din.hit)),
    };
    let locals: Vec<(String, Val)> = inj.locals.into_iter().collect();
    let this_pass: Option<pob_calc::Table> = locals
        .iter()
        .find(|(k, _)| k == "pass")
        .and_then(|(_, v)| v.table());
    // `passList` is injected as the plain Lua sequence (`run_segment` accepts it, CCR-0036).
    let passes: Vec<pob_calc::Table> = locals
        .iter()
        .find(|(k, _)| k == "passList")
        .and_then(|(_, v)| v.table())
        .map(|t| t.ipairs().into_iter().filter_map(|(_, p)| p.table()).collect())
        .unwrap_or_default();
    if dout.probe == "offence:5650"
        && passes.last().is_some_and(|l| !this_pass.as_ref().is_some_and(|t| t.ptr_eq(l)))
    {
        return Outcome::NoNext;
    }
    // `run_segment` sets `globalOutput, globalBreakdown = output, breakdown` (CalcOffence.lua:4165)
    // from the injected actor (CCR-0075).
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
            if !stubs.is_empty() {
                return Outcome::StubBlocked(stubs, 1);
            }
            return Outcome::Error(format!("run {}: {}", din.hit, panic_msg(&*p)));
        }
    };
    let env = after
        .iter()
        .find(|(k, _)| k == "env")
        .and_then(|(_, v)| v.obj::<pob_calc::env::Env>())
        .or(inj.env);
    // At the out stage (`offence:4165` of the next pass, before its `local output = pass.output`,
    // or `offence:5650` after the loop) `output`/`breakdown` are the function's locals, i.e.
    // `actor.output`/`actor.breakdown` (CalcOffence.lua 354/356), and `pass` is the next pass.
    // Our exported values are the loop locals, so replace them by what those names denote there
    // (the dumps share table identities through `$ref`, so the names stay in the comparison).
    after.retain(|(k, _)| !PASS_LOCALS.contains(&k.as_str()));
    after.push(("output".to_owned(), Val::Table(actor.output.get())));
    after.push(("breakdown".to_owned(), actor.breakdown.get().to_val()));
    if dout.probe == "offence:4165" {
        let next = passes
            .iter()
            .position(|p| this_pass.as_ref().is_some_and(|t| t.ptr_eq(p)))
            .and_then(|i| passes.get(i + 1).cloned());
        match next {
            Some(p) => after.push(("pass".to_owned(), Val::Table(p))),
            None => return Outcome::Error(format!("{}: no next pass in passList", din.hit)),
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
    let foreign = serde_json::to_string(&din.root).is_ok_and(|s| s.contains("\"$foreign\""));
    match (lines.is_empty(), stubs.is_empty()) {
        (true, true) => Outcome::Pass,
        (false, true) if foreign => {
            if std::env::var_os("OFH_VERBOSE").is_some() {
                for l in &lines {
                    eprintln!("    (foreign) {l}");
                }
            }
            Outcome::Foreign(lines.len())
        }
        (false, true) => Outcome::Fail(lines),
        (_, false) => {
            let n = lines.len();
            if std::env::var_os("OFH_VERBOSE").is_some() {
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
    missing_out: usize,
    foreign: usize,
    blocked_sites: BTreeMap<String, usize>,
}

#[test]
fn ofh_l4_ignite_nondamaging_impale() {
    if pob_testpaths::require(pob_testpaths::dump_dir(), "data/dump").is_none() {
        return;
    }
    assert!(testkit::init_default_data(), "data/dump does not load");
    let verbose = std::env::var_os("OFH_VERBOSE").is_some();
    let mut t = Tally::default();
    let mut lines = Vec::new();
    let mut missing = Vec::new();
    for xml in builds() {
        let stem = stem(&xml).to_owned();
        let state = match ensure_dumps(&xml, &stem) {
            Ok(s) => s,
            Err(e) => {
                missing.push(format!("{stem}: {e}"));
                continue;
            }
        };
        let all = dumps(&state);
        let mut any = false;
        for ((probe, _), din) in &all {
            if probe != "offence:4990" {
                continue;
            }
            any = true;
            let Some(dout) = next_dump(&all, din) else {
                t.missing_out += 1;
                lines.push(format!("no-out  {stem} {}", din.hit));
                continue;
            };
            match run_hit(&xml, din, dout, &stem) {
                Outcome::NoNext => {
                    t.missing_out += 1;
                    lines.push(format!("no-out  {stem} {} (not the last pass)", din.hit));
                }
                Outcome::Foreign(n) => {
                    t.foreign += 1;
                    lines.push(format!(
                        "FOREIGN {stem} {} -> {}: {n} mismatches, input has $foreign refs",
                        din.hit, dout.hit
                    ));
                }
                Outcome::Pass => {
                    t.pass += 1;
                    lines.push(format!("ok      {stem} {} -> {}", din.hit, dout.hit));
                }
                Outcome::Fail(mm) => {
                    t.fail += 1;
                    lines.push(format!(
                        "FAIL    {stem} {} -> {}: {} mismatches",
                        din.hit,
                        dout.hit,
                        mm.len()
                    ));
                    let show = if verbose { mm.len() } else { 15 };
                    for l in mm.iter().take(show) {
                        lines.push(format!("          {l}"));
                    }
                }
                Outcome::StubBlocked(sites, n) => {
                    t.blocked += 1;
                    lines.push(format!(
                        "BLOCKED {stem} {} -> {} ({n} mismatches): {}",
                        din.hit,
                        dout.hit,
                        sites.join(", ")
                    ));
                    for s in sites {
                        *t.blocked_sites
                            .entry(s.split(" x").next().unwrap_or("").to_owned())
                            .or_default() += 1;
                    }
                }
                Outcome::Error(e) => {
                    t.errors += 1;
                    lines.push(format!("ERROR   {stem}: {e}"));
                }
            }
        }
        if !any {
            lines.push(format!("note    {stem}: no offence:4990 hit dumped"));
        }
    }
    for l in &lines {
        eprintln!("{l}");
    }
    eprintln!(
        "OF-H L4 offence:4990: pass {} / fail {} / stub-blocked {} / foreign-ref {} / errors {} / no next dump {}; {} builds without inputs",
        t.pass,
        t.fail,
        t.blocked,
        t.foreign,
        t.errors,
        t.missing_out,
        missing.len()
    );
    for (s, n) in &t.blocked_sites {
        eprintln!("  stub {s}: blocked {n} hits");
    }
    for m in &missing {
        eprintln!("missing: {m}");
    }
    if !missing.is_empty() && pob_testpaths::require::<()>(None, &missing.join("; ")).is_none() {
        return;
    }
    assert_eq!(t.errors, 0, "OF-H L4: {} hits raised (see stderr)", t.errors);
    assert_eq!(t.fail, 0, "OF-H L4: {} stub-free hits mismatch (see stderr)", t.fail);
    assert!(t.pass > 0, "OF-H L4: no hit ran stub-free");
}
