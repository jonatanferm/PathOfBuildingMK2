//! Unit DF-C acceptance (calc doc §11.1 row DF-B..G, §9.4 L4): CalcDefence.lua 1211-1673, the
//! recovery / leech / regeneration / recoup / avoidance segment of `calcs.defence`
//! (`pob_calc::modules::calc_defence::recovery::seg`).
//!
//! **L4.** For every build, the main env's `defence:1211` dump (the segment's `:in`, written by
//! the line probe before 1212) is injected into the Rust graph (`pob_oracle::inject`), the Rust
//! segment runs on a `DefenceCtx` built from the injected locals
//! (`pob_calc::probe::run_segment`), and the exported state (env, GlobalCache, the 7 ctx
//! locals) is diffed against the `defence:end` dump (the line probe before the function's final
//! `end`, CalcDefence.lua:1671; §9.2 tolerance, `.parity/nondet` sets).
//!
//! Gate: 100 % of the unit corpus (`corpus/DF-C/*.xml`, custom-modifier variants of all57
//! builds for the rare branches) and of the curated corpus (`corpus/DF-C/curated.txt`), and ≥ 98 % of all57
//! (`.parity/builds`). The segment calls only ported code (ModStore, calcLib, the breakdown
//! helpers); a hit whose run nonetheless reaches an `unported!` stub is reported as
//! **pending** with the stub sites, and the gate applies to the other hits.
//!
//! Inputs: `data/dump`, `.parity/builds`, and the dumps: one `pob_oracle probe` run per build
//! selecting `defence:1211@main,defence:end@main` under `docs/port/stages.toml`, from the shared
//! probe cache (`testkit::probe_dumps`, CK2: keyed by the patched source, the build's bytes, the
//! selection and the probe code; probed here when absent). Knobs: `DFC_BUILDS=stem1,stem2`, `DFC_VERBOSE=1` (every mismatch).
//! A missing input skips with a message; with `POB_ORACLE_REQUIRE=1` (or `CI`) it fails.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use pob_oracle::diff::Mismatch;
use pob_oracle::oracle::find_parity_dir;
use pob_oracle::schema::StateDump;
use pob_oracle::testkit;

const STAGE_IN: &str = "defence:1211";
const STAGE_OUT: &str = "defence:end";
/// This unit's own segment (must never show up as a stub hit).
const OWN_SITE: &str = "Modules/CalcDefence.lua:1211-1673";

fn skip(why: &str) {
    let required = std::env::var_os("POB_ORACLE_REQUIRE").is_some_and(|v| v == "1")
        || std::env::var_os("CI").is_some_and(|v| !v.is_empty() && v != "false" && v != "0");
    assert!(!required, "required input missing: {why}");
    eprintln!("SKIPPED: {why}");
}

fn stem(p: &Path) -> &str {
    p.file_stem().and_then(|s| s.to_str()).unwrap_or_default()
}

/// `.parity/builds/*.xml`, optionally narrowed by `DFC_BUILDS`.
fn all57(parity: &Path) -> Vec<PathBuf> {
    let only: Option<Vec<String>> = std::env::var("DFC_BUILDS")
        .ok()
        .map(|s| s.split(',').map(str::to_owned).collect());
    let mut v: Vec<PathBuf> = std::fs::read_dir(parity.join("builds"))
        .map(|rd| {
            rd.filter_map(Result::ok)
                .map(|e| e.path())
                .filter(|p| p.extension().is_some_and(|e| e == "xml"))
                .collect()
        })
        .unwrap_or_default();
    v.sort();
    if let Some(only) = only {
        v.retain(|p| only.iter().any(|o| o == stem(p)));
    }
    v
}

/// The curated stems (`corpus/DF-C/curated.txt`).
fn curated() -> Vec<String> {
    let p = Path::new(env!("CARGO_MANIFEST_DIR")).join("corpus/DF-C/curated.txt");
    std::fs::read_to_string(p)
        .unwrap_or_default()
        .lines()
        .map(|l| l.split('#').next().unwrap_or("").trim().to_owned())
        .filter(|l| !l.is_empty())
        .collect()
}

/// The unit corpus (`corpus/DF-C/*.xml`): all57 builds with custom modifier blocks that reach
/// the segment's rare branches (Zealot's Oath, life regen applied to ES, regen-to-rage, recoup
/// conversions and `Add…RecoupTo…Recoup`, Foulborn Ancestral Vision, Firesong, ES recharge
/// applied to life with and without `CannotRecoverLifeOutsideLeech`, movement speed floor,
/// damage reduction cap override, pseudo recoup).
fn unit_corpus() -> Vec<PathBuf> {
    let d = Path::new(env!("CARGO_MANIFEST_DIR")).join("corpus/DF-C");
    let mut v: Vec<PathBuf> = std::fs::read_dir(d)
        .map(|rd| {
            rd.filter_map(Result::ok)
                .map(|e| e.path())
                .filter(|p| p.extension().is_some_and(|e| e == "xml"))
                .collect()
        })
        .unwrap_or_default();
    v.sort();
    v
}

/// A mismatch path with list indices collapsed (for the summary).
fn generalize(path: &str) -> String {
    let mut out = String::with_capacity(path.len());
    let mut chars = path.chars().peekable();
    while let Some(c) = chars.next() {
        if c == '[' {
            out.push_str("[*]");
            for d in chars.by_ref() {
                if d == ']' {
                    break;
                }
            }
        } else {
            out.push(c);
        }
    }
    out
}

/// What one L4 hit gave.
#[derive(Clone)]
enum Outcome {
    /// No mismatch, no stub callee.
    Pass,
    /// Mismatches on ported code only.
    Fail(Vec<Mismatch>),
    /// A callee was a stub: (stub sites, mismatches).
    Pending(Vec<(&'static str, u64)>, Vec<Mismatch>),
    /// No dump pair for this build.
    Missing(String),
    /// Inject or run failed.
    Error(String),
}

/// L4 on one build: inject `defence:1211`, run the segment, diff against `defence:end`.
fn run_hit(xml: &Path) -> Outcome {
    let stem = stem(xml).to_owned();
    // `:in` and `:out` from one PoB process (one dump directory): two runs differ in mod order
    // (§9.5), so a pair across runs is not compared
    let dumps = match testkit::probe_dumps(
        Path::new(env!("CARGO_BIN_EXE_pob_oracle")),
        xml,
        Some(&testkit::default_stages()),
        &testkit::selection(&[&format!("{STAGE_IN}@main"), &format!("{STAGE_OUT}@main")]),
    ) {
        Ok(d) => d,
        Err(e) => return Outcome::Missing(format!("{stem}: probe failed: {e}")),
    };
    let Some((pin, pout)) = testkit::find_pair(dumps.dirs(), STAGE_IN, STAGE_OUT) else {
        return Outcome::Missing(format!("{stem}: no {STAGE_IN}/{STAGE_OUT} dump"));
    };
    let (din, dout) = match (StateDump::load(&pin), StateDump::load(&pout)) {
        (Ok(a), Ok(b)) => (a, b),
        _ => return Outcome::Error(format!("{stem}: unreadable dump {}", pin.display())),
    };
    if din.n != dout.n || din.env_id != dout.env_id {
        return Outcome::Error(format!(
            "{stem}: {} and {} are not one defence call",
            din.hit, dout.hit
        ));
    }
    let model = testkit::empty_build();
    let inj = match pob_oracle::inject::inject(&din, &model) {
        Ok(i) => i,
        Err(e) => return Outcome::Error(format!("{stem}: inject {}: {e}", din.hit)),
    };
    let locals: Vec<(String, pob_calc::Val)> = inj.locals.into_iter().collect();
    pob_calc::stub::stub_reset();
    let run = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        pob_calc::probe::run_segment(&din.probe, &locals)
    }));
    let stubs: Vec<(&'static str, u64)> = pob_calc::stub::stub_report();
    assert!(
        !stubs.iter().any(|(s, _)| *s == OWN_SITE),
        "the DF-C segment is still a stub"
    );
    let after = match run {
        Ok(Ok((_flow, after))) => after,
        Ok(Err(e)) => return Outcome::Error(format!("{stem}: run {}: {e}", din.hit)),
        Err(p) => {
            let msg = p
                .downcast_ref::<pob_lua::LuaError>()
                .map(|e| e.0.clone())
                .or_else(|| p.downcast_ref::<String>().cloned())
                .or_else(|| p.downcast_ref::<&str>().map(|s| (*s).to_owned()))
                .unwrap_or_else(|| "panic".to_owned());
            if !stubs.is_empty() {
                return Outcome::Pending(stubs, Vec::new());
            }
            return Outcome::Error(format!("{stem}: run {}: {msg}", din.hit));
        }
    };
    let env = after
        .iter()
        .find(|(k, _)| k == "env")
        .and_then(|(_, v)| v.obj::<pob_calc::env::Env>())
        .or(inj.env);
    let mm = testkit::diff_against(&dout, env.as_ref(), &after, &stem);
    if !stubs.is_empty() {
        Outcome::Pending(stubs, mm)
    } else if mm.is_empty() {
        Outcome::Pass
    } else {
        Outcome::Fail(mm)
    }
}

#[derive(Default)]
struct Tally {
    hits: usize,
    pass: usize,
    fail: Vec<(String, Vec<Mismatch>)>,
    pending: Vec<(String, Vec<(&'static str, u64)>, usize)>,
    missing: Vec<String>,
    errors: Vec<String>,
}

impl Tally {
    fn add(&mut self, stem: &str, o: Outcome) {
        match o {
            Outcome::Pass => {
                self.hits += 1;
                self.pass += 1;
            }
            Outcome::Fail(mm) => {
                self.hits += 1;
                self.fail.push((stem.to_owned(), mm));
            }
            Outcome::Pending(s, mm) => {
                self.hits += 1;
                self.pending.push((stem.to_owned(), s, mm.len()));
            }
            Outcome::Missing(m) => self.missing.push(m),
            Outcome::Error(e) => {
                self.hits += 1;
                self.errors.push(e);
            }
        }
    }

    /// Hits that ran on ported code only (the gate's denominator).
    fn gated(&self) -> usize {
        self.hits - self.pending.len()
    }

    fn report(&self, title: &str) {
        let verbose = std::env::var_os("DFC_VERBOSE").is_some();
        eprintln!(
            "== DF-C L4 {title}: {} hits: {} pass, {} fail, {} error, {} pending (callee stubs); {} missing dumps",
            self.hits,
            self.pass,
            self.fail.len(),
            self.errors.len(),
            self.pending.len(),
            self.missing.len()
        );
        let mut groups: BTreeMap<String, (usize, String)> = BTreeMap::new();
        for (stem, mm) in &self.fail {
            eprintln!("  FAIL {stem}: {} mismatches", mm.len());
            for m in mm {
                let e = groups
                    .entry(generalize(&m.path))
                    .or_insert((0, String::new()));
                e.0 += 1;
                if e.1.is_empty() {
                    e.1 = format!(
                        "{stem} {} {:?}: pob={:?} ours={:?}",
                        m.path, m.kind, m.pob, m.ours
                    );
                }
                if verbose {
                    eprintln!(
                        "    {} {:?}: pob={:?} ours={:?}",
                        m.path, m.kind, m.pob, m.ours
                    );
                }
            }
        }
        for (path, (n, ex)) in groups.iter().take(40) {
            eprintln!("  {n:5} × {path}\n        e.g. {}", clip(ex));
        }
        for e in &self.errors {
            eprintln!("  ERROR {e}");
        }
        let mut sites: BTreeMap<&str, usize> = BTreeMap::new();
        for (_, s, _) in &self.pending {
            for (site, _) in s {
                *sites.entry(site).or_default() += 1;
            }
        }
        for (site, n) in &sites {
            eprintln!("  pending: {n} hits call the stub {site}");
        }
        if verbose {
            for (stem, s, n) in &self.pending {
                eprintln!("  PENDING {stem}: {n} mismatches; stubs {s:?}");
            }
        }
        for m in &self.missing {
            eprintln!("  missing: {m}");
        }
    }
}

fn clip(s: &str) -> String {
    if s.len() > 400 {
        let mut e = 400;
        while !s.is_char_boundary(e) {
            e -= 1;
        }
        format!("{}…", &s[..e])
    } else {
        s.to_owned()
    }
}

/// One test: the stub hit counter is process-global, so the hits run one at a time.
#[test]
fn dfc_l4_recovery_segment() {
    if !testkit::init_default_data() {
        return skip("no data dump (data/dump)");
    }
    let Some(parity) = find_parity_dir() else {
        return skip("no .parity directory");
    };
    let builds = all57(&parity);
    if builds.is_empty() {
        return skip("no .parity/builds/*.xml");
    }
    let cur = curated();
    assert!(!cur.is_empty(), "corpus/DF-C/curated.txt is empty");
    let mut curated_t = Tally::default();
    let mut all_t = Tally::default();
    for xml in &builds {
        let s = stem(xml).to_owned();
        let o = run_hit(xml);
        if cur.contains(&s) {
            curated_t.add(&s, o.clone());
        }
        all_t.add(&s, o);
    }
    let mut unit_t = Tally::default();
    for xml in unit_corpus() {
        let s = stem(&xml).to_owned();
        unit_t.add(&s, run_hit(&xml));
    }
    unit_t.report("unit corpus");
    curated_t.report("curated");
    all_t.report("all57");
    if all_t.hits == 0 {
        return skip("no defence:1211/defence:end dumps for any build (see the missing list)");
    }
    // every curated stem must have a dump pair (a missing one would pass silently)
    assert!(
        curated_t.missing.is_empty() || std::env::var_os("DFC_BUILDS").is_some(),
        "DF-C L4 curated: {} builds without dumps",
        curated_t.missing.len()
    );
    // the unit corpus: every build has dumps and is exact
    assert!(
        unit_t.missing.is_empty() && unit_t.fail.is_empty() && unit_t.errors.is_empty(),
        "DF-C L4 unit corpus: {} missing, {} fail, {} error (gate 100 %)",
        unit_t.missing.len(),
        unit_t.fail.len(),
        unit_t.errors.len()
    );
    // the curated corpus: every hit that ran on ported code is exact
    assert!(
        curated_t.fail.is_empty() && curated_t.errors.is_empty(),
        "DF-C L4 curated: {} fail, {} error (gate 100 %)",
        curated_t.fail.len(),
        curated_t.errors.len()
    );
    // all57: ≥ 98 % of the gated hits
    let gated = all_t.gated();
    if gated > 0 {
        let ok = all_t.pass as f64 / gated as f64;
        assert!(
            ok >= 0.98,
            "DF-C L4 all57: {}/{gated} hits exact ({:.1} %, gate 98 %)",
            all_t.pass,
            ok * 100.0
        );
    }
}
