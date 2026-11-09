//! Unit DF-G acceptance (calc doc §11.1 row DF-B..G, §9.4 L4): the `calcs.buildDefenceEstimations`
//! segment CalcDefence.lua 3165-3873 (`pob_calc::modules::calc_defence::est_maxhit::seg`: max hit
//! taken, DoT EHP, degens / net regen, the incoming-hit breakdown) against PoB under LuaJIT.
//!
//! Every test injects the `buildDefenceEstimations:3165` dump of a build, runs the Rust segment
//! and diffs the env + ctx locals against the `buildDefenceEstimations:end` dump (line probe
//! before the function's final `end`, 3872). A build whose injected state already equals `:end`
//! fails as vacuous.
//!
//! * `dfg_l4_slice5` / `dfg_l4_all57` — the L4 gate (100% on slice5 + the unit corpus, ≥ 98% of
//!   the all57 builds). The segment calls three DF-A helpers (`calcs.takenHitFromDamage`,
//!   `calcs.reducePoolsByDamage`, `incomingDamageBreakdown`). While any of them is still an
//!   `unported!` stub the full diff cannot pass (the breakdown region indexes their results), so
//!   a build whose run hit one of those stubs is counted as BLOCKED (DF-A) and reported, not
//!   failed; with `DFG_REQUIRE_HELPERS=1` a blocked build fails.
//! * `dfg_l4_no_breakdown_all` — what does not depend on the helpers today: the segment runs
//!   with `breakdown = nil` (so only the conversion-smoothing loop, 3273-3302, calls the helpers)
//!   and every mismatch under a `breakdown` path is dropped (PoB's `:end` dump has the MAIN-mode
//!   breakdown). Builds that need the smoothing loop are BLOCKED (DF-A) as above; every other
//!   build must match exactly.
//!
//! Diff rules beyond `testkit::diff_against`: the breakdown row lists filled from `pairs` over
//! string-keyed tables compare as multisets (`unordered_lists`), and known_diffs.toml DF-G-1
//! (CCR-0001 `%d` of ±inf) is tolerated.
//!
//! Corpus: slice5 (`corpus/U0/slice5/*.xml`), the unit corpus `corpus/DF-G/*.xml`, all57
//! (`.parity/builds`). `DFG_BUILDS=a,b` restricts by stem, `DFG_VERBOSE=1` prints every mismatch.
//! Missing dumps print the `scripts/port/oracle.sh run` line that fills them and skip; with
//! `POB_ORACLE_REQUIRE=1` (or `CI`) a missing input fails.
#![allow(non_snake_case)]

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use pob_calc::Val;
use pob_oracle::diff::{diff_trees, DiffOptions, Mismatch};
use pob_oracle::oracle::find_parity_dir;
use pob_oracle::schema::{resolve_ids, StateDump};
use pob_oracle::testkit;
use serde_json::Value as J;

const STAGE_IN: &str = "buildDefenceEstimations:3165";
const STAGE_OUT: &str = "buildDefenceEstimations:end";
/// The DF-A helper bodies the segment calls (stub sites).
const HELPER_SITES: [&str; 3] = [
    "Modules/CalcDefence.lua:150-177",
    "Modules/CalcDefence.lua:190-424",
    "Modules/CalcDefence.lua:431-478",
];

fn required() -> bool {
    std::env::var_os("POB_ORACLE_REQUIRE").is_some_and(|v| v == "1")
        || std::env::var_os("CI").is_some_and(|v| !v.is_empty() && v != "false" && v != "0")
}

fn skip(why: &str) {
    assert!(!required(), "required input missing: {why}");
    eprintln!("SKIPPED: {why}");
}

fn verbose() -> bool {
    std::env::var_os("DFG_VERBOSE").is_some_and(|v| v == "1")
}

fn stem(p: &Path) -> &str {
    p.file_stem().and_then(|s| s.to_str()).unwrap_or_default()
}

fn xmls_in(dir: &Path) -> Vec<PathBuf> {
    let mut v: Vec<PathBuf> = std::fs::read_dir(dir)
        .map(|r| {
            r.filter_map(Result::ok)
                .map(|e| e.path())
                .filter(|p| p.extension().is_some_and(|x| x == "xml"))
                .collect()
        })
        .unwrap_or_default();
    v.sort();
    v
}

fn filter(mut v: Vec<PathBuf>) -> Vec<PathBuf> {
    if let Ok(list) = std::env::var("DFG_BUILDS") {
        let want: Vec<&str> = list.split(',').map(str::trim).collect();
        v.retain(|p| want.contains(&stem(p)));
    }
    v
}

fn manifest_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

/// slice5 + the unit corpus.
fn curated() -> Vec<PathBuf> {
    let mut v = xmls_in(&manifest_dir().join("corpus/U0/slice5"));
    v.extend(xmls_in(&manifest_dir().join("corpus/DF-G")));
    filter(v)
}

fn all57() -> Vec<PathBuf> {
    find_parity_dir()
        .map(|p| filter(xmls_in(&p.join("builds"))))
        .unwrap_or_default()
}

fn helper_stub_hits() -> u64 {
    pob_calc::stub::stub_report()
        .iter()
        .filter(|(site, _)| HELPER_SITES.contains(site))
        .map(|(_, n)| *n)
        .sum()
}

/// Breakdown row lists this segment fills from `pairs` over string-keyed tables (LUA-ORDER sites
/// 3326, 3389, 3556, 3618/3625): PoB's order is its per-process string hash order (calc doc §3.4,
/// §9.5), so they compare as multisets. Values and float sums stay strict.
fn unordered_lists() -> BTreeSet<String> {
    let mut s = BTreeSet::new();
    for dt in ["Physical", "Lightning", "Cold", "Fire", "Chaos"] {
        for k in ["MaximumHitTaken", "BuildDegen", "EnemyDegen"] {
            s.insert(format!("locals.breakdown.{dt}{k}.rowList"));
        }
    }
    s.insert("locals.breakdown.TotalBuildDegen.rowList".to_owned());
    s.insert("locals.breakdown.TotalDegen.rowList".to_owned());
    s
}

/// known_diffs.toml DF-G-1 (CCR-0001): `s_format("%d", x)` of `±inf` (e.g. the Chaos `DotEHP`
/// of a CI build, 3378) is the x64 `cvttsd2si` result -9223372036854775808 in the port and the
/// saturated 9223372036854775807 of this ARM64 host's oracle.
fn is_ccr0001_int_cast(m: &Mismatch) -> bool {
    let in_entry = ["Physical", "Lightning", "Cold", "Fire", "Chaos"]
        .iter()
        .any(|dt| {
            ["DotEHP", "MaximumHitTaken", "TakenHit"]
                .iter()
                .any(|k| m.path.starts_with(&format!("locals.breakdown.{dt}{k}")))
        });
    if !in_entry {
        return false;
    }
    match (&m.pob, &m.ours) {
        (Some(J::String(p)), Some(J::String(o))) => {
            p != o && p.replace("9223372036854775807", "-9223372036854775808") == *o
        }
        _ => false,
    }
}

/// `testkit::diff_against` plus this unit's unordered lists and the DF-G-1 tolerance.
fn diff_dfg(
    dump: &StateDump,
    env: Option<&pob_calc::env::EnvRef>,
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
    let ours = testkit::export_after(env, &kept);
    let mut pob = dump.root.clone();
    if let Some(J::Object(l)) = pob.get_mut("locals") {
        let ours_names: Vec<&String> = kept.iter().map(|(k, _)| k).collect();
        l.retain(|k, v| {
            ours_names.contains(&k) || !v.as_object().is_some_and(|m| m.contains_key("$fn"))
        });
    }
    let nondet = find_parity_dir()
        .map(|p| p.join("nondet").join(format!("{stem}.json")))
        .filter(|p| p.is_file())
        .and_then(|p| pob_oracle::nondet::NondetRecord::load(&p).ok());
    let mut orders = unordered_lists();
    if let Some(o) = nondet.as_ref().and_then(|n| n.state_orders.get(&dump.hit)) {
        orders.extend(o.iter().cloned());
    }
    let opts = DiffOptions {
        nondet_values: nondet.as_ref().and_then(|n| n.states.get(&dump.hit)),
        nondet_orders: Some(&orders),
        ..DiffOptions::strict()
    };
    diff_trees(&resolve_ids(&pob), &resolve_ids(&ours), &opts)
        .into_iter()
        .filter(|m| !is_ccr0001_int_cast(m))
        .collect()
}

fn fmt_mismatch(m: &Mismatch) -> String {
    format!("{} {:?}: pob={:?} ours={:?}", m.path, m.kind, m.pob, m.ours)
}

enum Outcome {
    Pass,
    Fail(Vec<String>),
    /// The run hit a DF-A helper stub.
    Blocked(Vec<String>),
    Missing(String),
}

/// The run of one build: inject `:3165`, run, diff against `:end`.
/// `no_breakdown`: run with `breakdown = nil` and drop breakdown mismatches.
fn run_build(xml: &Path, no_breakdown: bool) -> Outcome {
    let stem = stem(xml).to_owned();
    let dirs = testkit::dump_dirs(&stem, Some(xml));
    let (Some(pin), Some(pout)) = (
        testkit::find_dump(&dirs, STAGE_IN),
        testkit::find_dump(&dirs, STAGE_OUT),
    ) else {
        return Outcome::Missing(format!(
            "missing dump {STAGE_IN} / {STAGE_OUT} for {stem} (fill with: scripts/port/oracle.sh run --build {} --stage '{STAGE_IN}@main,{STAGE_OUT}@main')",
            xml.display()
        ));
    };
    let (Ok(din), Ok(dout)) = (StateDump::load(&pin), StateDump::load(&pout)) else {
        return Outcome::Fail(vec![format!("unreadable dump {}", pin.display())]);
    };
    let keep = |m: &Mismatch| !no_breakdown || !m.path.contains("breakdown");
    let before = helper_stub_hits();
    let model = testkit::load_build(xml).unwrap_or_else(testkit::empty_build);
    let inj = match pob_oracle::inject::inject(&din, &model) {
        Ok(i) => i,
        Err(e) => return Outcome::Fail(vec![format!("inject: {e}")]),
    };
    let mut locals: Vec<(String, Val)> = inj.locals.into_iter().collect();
    // Not vacuous: before the run, the injected state must differ from PoB's `:end`
    // (outside the breakdown when it is left out).
    let pending = diff_dfg(&dout, inj.env.as_ref(), &locals, &stem)
        .iter()
        .filter(|m| keep(m))
        .count();
    if pending == 0 {
        return Outcome::Fail(vec![
            "vacuous: the :3165 state already equals :end".to_owned()
        ]);
    }
    if no_breakdown {
        for (k, v) in &mut locals {
            if k == "breakdown" {
                *v = Val::Nil;
            }
        }
    }
    let run = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        pob_calc::probe::run_segment(&din.probe, &locals)
    }));
    let lines: Vec<String> = match run {
        Ok(Ok((_flow, after))) => {
            let env = after
                .iter()
                .find(|(k, _)| k == "env")
                .and_then(|(_, v)| v.obj::<pob_calc::env::Env>())
                .or(inj.env);
            // with `no_breakdown` our breakdown local is nil: leave it out (PoB's is not)
            let after: Vec<(String, Val)> = after
                .into_iter()
                .filter(|(k, _)| !no_breakdown || k != "breakdown")
                .collect();
            diff_dfg(&dout, env.as_ref(), &after, &stem)
                .iter()
                .filter(|m| keep(m))
                .map(fmt_mismatch)
                .collect()
        }
        Ok(Err(e)) => vec![format!("run: {e}")],
        Err(p) => {
            let msg = p
                .downcast_ref::<pob_lua::LuaError>()
                .map(|e| e.0.clone())
                .or_else(|| p.downcast_ref::<String>().cloned())
                .or_else(|| p.downcast_ref::<&str>().map(|s| (*s).to_owned()))
                .unwrap_or_else(|| "panic".to_owned());
            vec![format!("run: {msg}")]
        }
    };
    let blocked = helper_stub_hits() > before;
    if lines.is_empty() {
        Outcome::Pass
    } else if blocked {
        Outcome::Blocked(lines)
    } else {
        Outcome::Fail(lines)
    }
}

struct Tally {
    pass: Vec<String>,
    fail: Vec<(String, Vec<String>)>,
    blocked: Vec<(String, Vec<String>)>,
    missing: Vec<String>,
}

fn run_all(builds: &[PathBuf], no_breakdown: bool, title: &str) -> Tally {
    let mut t = Tally {
        pass: Vec::new(),
        fail: Vec::new(),
        blocked: Vec::new(),
        missing: Vec::new(),
    };
    for xml in builds {
        let s = stem(xml).to_owned();
        match run_build(xml, no_breakdown) {
            Outcome::Pass => t.pass.push(s),
            Outcome::Fail(l) => t.fail.push((s, l)),
            Outcome::Blocked(l) => t.blocked.push((s, l)),
            Outcome::Missing(m) => {
                eprintln!("{m}");
                t.missing.push(s);
            }
        }
    }
    let ran = t.pass.len() + t.fail.len() + t.blocked.len();
    eprintln!(
        "DF-G {title}: {} builds, {ran} ran: {} pass, {} fail, {} blocked (DF-A helper stubs), {} missing dumps",
        builds.len(),
        t.pass.len(),
        t.fail.len(),
        t.blocked.len(),
        t.missing.len()
    );
    let show = |what: &str, v: &[(String, Vec<String>)]| {
        for (s, l) in v {
            eprintln!("  {what} {s}: {} mismatches", l.len());
            let n = if verbose() { l.len() } else { 5 };
            for m in l.iter().take(n) {
                eprintln!("    {}", clip(m));
            }
        }
    };
    show("FAIL", &t.fail);
    show("BLOCKED", &t.blocked);
    t
}

fn clip(s: &str) -> String {
    if s.len() > 300 && !verbose() {
        let mut e = 300;
        while !s.is_char_boundary(e) {
            e -= 1;
        }
        format!("{}…", &s[..e])
    } else {
        s.to_owned()
    }
}

fn require_helpers() -> bool {
    std::env::var_os("DFG_REQUIRE_HELPERS").is_some_and(|v| v == "1")
}

fn ready() -> bool {
    if !testkit::init_default_data() {
        skip("no data dump (data/dump)");
        return false;
    }
    if find_parity_dir().is_none() {
        skip("no .parity directory");
        return false;
    }
    true
}

/// L4 gate on slice5 + the unit corpus: 100%.
#[test]
fn dfg_l4_slice5() {
    if !ready() {
        return;
    }
    let builds = curated();
    let t = run_all(&builds, false, "L4 slice5+unit");
    if t.pass.len() + t.fail.len() + t.blocked.len() == 0 {
        return skip("no dumps for slice5 (see the oracle.sh lines above)");
    }
    assert!(t.fail.is_empty(), "{} curated builds fail L4", t.fail.len());
    assert!(
        t.blocked.is_empty() || !require_helpers(),
        "{} curated builds blocked on DF-A helper stubs",
        t.blocked.len()
    );
}

/// L4 gate on all57: ≥ 98% of the builds that ran.
#[test]
fn dfg_l4_all57() {
    if !ready() {
        return;
    }
    let builds = all57();
    let t = run_all(&builds, false, "L4 all57");
    let ran = t.pass.len() + t.fail.len() + t.blocked.len();
    if ran == 0 {
        return skip("no dumps for all57 (see the oracle.sh lines above)");
    }
    let judged = t.pass.len() + t.fail.len();
    if judged > 0 {
        let rate = t.pass.len() as f64 / judged as f64;
        assert!(rate >= 0.98, "L4 all57 pass rate {rate:.3} < 0.98");
    }
    assert!(
        t.blocked.is_empty() || !require_helpers(),
        "{} all57 builds blocked on DF-A helper stubs",
        t.blocked.len()
    );
}

/// Everything but the breakdown, on every corpus build: exact.
#[test]
fn dfg_l4_no_breakdown_all() {
    if !ready() {
        return;
    }
    let mut builds = curated();
    builds.extend(all57());
    let t = run_all(&builds, true, "no-breakdown all");
    if t.pass.len() + t.fail.len() + t.blocked.len() == 0 {
        return skip("no dumps (see the oracle.sh lines above)");
    }
    assert!(
        t.fail.is_empty(),
        "{} builds fail the no-breakdown diff",
        t.fail.len()
    );
    assert!(
        t.blocked.is_empty() || !require_helpers(),
        "{} builds blocked on DF-A helper stubs",
        t.blocked.len()
    );
}
