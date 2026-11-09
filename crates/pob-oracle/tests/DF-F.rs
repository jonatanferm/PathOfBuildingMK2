//! Unit DF-F acceptance (calc doc §11.1 row DF-B..G, §9.4 L4): the EHP
//! segment of `calcs.buildDefenceEstimations` (CalcDefence.lua 2612-3164,
//! `calc_defence/est_ehp.rs`).
//!
//! L4: for every all57 build and every recorded hit `n` of the segment's
//! stage pair, inject `buildDefenceEstimations:2612#n`, run the Rust segment
//! (`pob_calc::probe::run_segment`) and diff the exported state against
//! `buildDefenceEstimations:3165#n` (§9.2 tolerance, nondet records applied).
//! Gate: 100 % of the judged hits on the unit corpus, ≥ 98 % on all57.
//!
//! The segment calls `buildAllyLifePools` / `calcs.reducePoolsByDamage`
//! (DF-A) and `calcs.actionSpeedMod` (PF-B). A hit whose segment run reaches
//! an `unported!` callee is **pending**: it is reported with the stub sites
//! it reached and not judged (its numbers come from a `Default` stand-in).
//! `DFF_STRICT=1` (or `POB_ORACLE_REQUIRE=1` / `CI`) counts pending hits as
//! failures.
//!
//! Inputs: dumps from `testkit::dump_dirs` (`.parity/probe/<stem>/state/`, the
//! probe cache), `data/dump`. Fill a build's dumps with
//! `scripts/port/oracle.sh run --build X.xml --stage buildDefenceEstimations:2612,buildDefenceEstimations:3165`.
//! Knobs: `DFF_BUILDS=stem1,stem2`, `DFF_LIMIT=n`, `DFF_VERBOSE=1` (print
//! every mismatch), `DFF_ALL_HITS=1` (every recorded pair, not just the
//! main env's lowest one).

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

use pob_calc::Val;
use pob_oracle::oracle::find_parity_dir;
use pob_oracle::schema::StateDump;
use pob_oracle::testkit;

const STAGE_IN: &str = "buildDefenceEstimations:2612";
const STAGE_OUT: &str = "buildDefenceEstimations:3165";

fn flag(name: &str) -> bool {
    std::env::var_os(name).is_some_and(|v| !v.is_empty() && v != "0" && v != "false")
}

fn required() -> bool {
    flag("POB_ORACLE_REQUIRE") || flag("CI")
}

fn skip(why: &str) {
    assert!(!required(), "required input missing: {why}");
    eprintln!("SKIPPED: {why}");
}

fn stem(p: &Path) -> &str {
    p.file_stem().and_then(|s| s.to_str()).unwrap_or_default()
}

fn corpus(parity: &Path) -> Vec<PathBuf> {
    let mut v: Vec<PathBuf> = std::fs::read_dir(parity.join("builds"))
        .map(|r| {
            r.filter_map(Result::ok)
                .map(|e| e.path())
                .filter(|p| p.extension().is_some_and(|x| x == "xml"))
                .collect()
        })
        .unwrap_or_default();
    v.sort();
    if let Ok(list) = std::env::var("DFF_BUILDS") {
        let want: Vec<&str> = list.split(',').map(str::trim).collect();
        v.retain(|p| want.contains(&stem(p)));
    }
    if let Some(n) = std::env::var("DFF_LIMIT")
        .ok()
        .and_then(|s| s.parse::<usize>().ok())
    {
        v.truncate(n);
    }
    v
}

/// Hit numbers `n` with both `STAGE_IN#n` and `STAGE_OUT#n` present in `dirs`
/// (a function's line probes are numbered per process, and no top-level
/// `return` lies between the two lines, so the `n`th in matches the `n`th out).
fn paired_hits(dirs: &[PathBuf]) -> Vec<u64> {
    let numbers = |stage: &str| -> BTreeSet<u64> {
        let prefix = format!("{stage}#");
        let mut s = BTreeSet::new();
        for d in dirs {
            if let Ok(rd) = std::fs::read_dir(d) {
                for e in rd.filter_map(Result::ok) {
                    let name = e.file_name().to_string_lossy().into_owned();
                    if let Some(rest) = name.strip_prefix(&prefix) {
                        let n: String = rest.chars().take_while(char::is_ascii_digit).collect();
                        if let Ok(n) = n.parse() {
                            s.insert(n);
                        }
                    }
                }
            }
        }
        s
    };
    let ins = numbers(STAGE_IN);
    numbers(STAGE_OUT)
        .into_iter()
        .filter(|n| ins.contains(n))
        .collect()
}

/// Stub sites whose counters grew since `before`.
fn stub_hits_since(before: &[(&'static str, u64)]) -> BTreeSet<&'static str> {
    let before: BTreeMap<&str, u64> = before.iter().copied().collect();
    pob_calc::stub::stub_report()
        .into_iter()
        .filter(|(k, n)| before.get(k).map_or(true, |b| n > b))
        .map(|(k, _)| k)
        .collect()
}

enum Outcome {
    Pass,
    Fail(Vec<String>),
    /// The run reached these `unported!` callees.
    Pending(BTreeSet<&'static str>, usize),
    Error(String),
}

/// One L4 hit: inject `in`, run the segment, diff against `out`. Stub hits
/// are counted around the segment run only (injection and the build loader
/// have stubs of their own).
fn run_hit(xml: &Path, stem: &str, din: &StateDump, dout: &StateDump) -> Outcome {
    let model = testkit::load_build(xml).unwrap_or_else(testkit::empty_build);
    let inj = match pob_oracle::inject::inject(din, &model) {
        Ok(i) => i,
        Err(e) => return Outcome::Error(format!("inject {}: {e}", din.hit)),
    };
    let locals: Vec<(String, Val)> = inj.locals.into_iter().collect();
    let before = pob_calc::stub::stub_report();
    let run = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        pob_calc::probe::run_segment(&din.probe, &locals)
    }));
    let stubs = stub_hits_since(&before);
    let after = match run {
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
                return Outcome::Pending(stubs, 1);
            }
            return Outcome::Error(format!("run {}: {msg}", din.hit));
        }
    };
    let env = after
        .iter()
        .find(|(k, _)| k == "env")
        .and_then(|(_, v)| v.obj::<pob_calc::env::Env>())
        .or(inj.env);
    let mm: Vec<String> = testkit::diff_against(dout, env.as_ref(), &after, stem)
        .into_iter()
        .map(|m| format!("{} {:?}: pob={:?} ours={:?}", m.path, m.kind, m.pob, m.ours))
        .collect();
    if !stubs.is_empty() {
        return Outcome::Pending(stubs, mm.len());
    }
    if mm.is_empty() {
        Outcome::Pass
    } else {
        Outcome::Fail(mm)
    }
}

#[derive(Default)]
struct Tally {
    hits: usize,
    pass: usize,
    fail: Vec<(String, Vec<String>)>,
    errors: Vec<String>,
    pending: Vec<String>,
    pending_sites: BTreeMap<&'static str, usize>,
    missing: Vec<String>,
}

impl Tally {
    fn report(&self, what: &str) {
        let verbose = flag("DFF_VERBOSE");
        let judged = self.pass + self.fail.len() + self.errors.len();
        eprintln!(
            "{what}: {} hits; judged {judged}: {} pass, {} mismatch, {} error; {} pending (unported callees); {} builds without a paired dump",
            self.hits,
            self.pass,
            self.fail.len(),
            self.errors.len(),
            self.pending.len(),
            self.missing.len()
        );
        for (site, n) in &self.pending_sites {
            eprintln!("  pending on {site}: {n} hits");
        }
        for (hit, mm) in &self.fail {
            eprintln!("  FAIL {hit}: {} mismatches", mm.len());
            let shown = if verbose { mm.len() } else { mm.len().min(8) };
            for m in &mm[..shown] {
                eprintln!("    {m}");
            }
        }
        for e in &self.errors {
            eprintln!("  ERROR {e}");
        }
        if verbose {
            for p in &self.pending {
                eprintln!("  pending {p}");
            }
            for m in &self.missing {
                eprintln!("  missing {m}");
            }
        }
    }
}

#[test]
fn dff_l4_all57() {
    if !testkit::init_default_data() {
        return skip("no data dump (data/dump)");
    }
    let Some(parity) = find_parity_dir() else {
        return skip("no .parity directory");
    };
    let builds = corpus(&parity);
    let all_hits = flag("DFF_ALL_HITS");
    let mut t = Tally::default();
    for xml in &builds {
        let stem = stem(xml);
        let dirs = testkit::dump_dirs(stem, Some(xml));
        let mut hits = paired_hits(&dirs);
        if hits.is_empty() {
            t.missing.push(stem.to_owned());
            continue;
        }
        if !all_hits {
            hits.truncate(1);
        }
        for n in hits {
            let (hin, hout) = (format!("{STAGE_IN}#{n}"), format!("{STAGE_OUT}#{n}"));
            let (Some(pin), Some(pout)) = (
                testkit::find_dump(&dirs, &hin),
                testkit::find_dump(&dirs, &hout),
            ) else {
                t.missing.push(format!("{stem} {hin}"));
                continue;
            };
            let (Ok(din), Ok(dout)) = (StateDump::load(&pin), StateDump::load(&pout)) else {
                t.errors.push(format!("{stem} {hin}: unreadable dump"));
                continue;
            };
            t.hits += 1;
            let what = format!("{stem} {hin}");
            match run_hit(xml, stem, &din, &dout) {
                Outcome::Pass => t.pass += 1,
                Outcome::Fail(mm) => t.fail.push((what, mm)),
                Outcome::Error(e) => t.errors.push(format!("{what}: {e}")),
                Outcome::Pending(sites, nmm) => {
                    for s in &sites {
                        *t.pending_sites.entry(s).or_insert(0) += 1;
                    }
                    t.pending.push(format!("{what} ({nmm} mismatches) via {sites:?}"));
                }
            }
        }
    }
    t.report("DF-F L4 all57");
    if t.hits == 0 {
        return skip("no paired buildDefenceEstimations:2612/3165 dumps");
    }
    let strict = flag("DFF_STRICT") || required();
    assert!(
        !strict || t.pending.is_empty(),
        "{} hits pending on unported callees (DFF_STRICT)",
        t.pending.len()
    );
    let judged = t.pass + t.fail.len() + t.errors.len();
    if judged > 0 {
        let rate = t.pass as f64 / judged as f64;
        assert!(
            rate >= 0.98,
            "DF-F L4: {}/{judged} judged hits pass ({:.1} % < 98 %)",
            t.pass,
            rate * 100.0
        );
    }
}
