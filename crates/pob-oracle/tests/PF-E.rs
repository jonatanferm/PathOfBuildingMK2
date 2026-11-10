//! Unit PF-E acceptance (calc doc §11.1 row PF-E, §9.4 L4): CalcPerform.lua 2308-2996, the
//! skill buff loop (`pob_calc::modules::calc_perform::buffs::seg`).
//!
//! **L4.** For every build, the main env's `perform:2308` dump (the segment's `:in`, written by
//! the line probe before 2308) is injected into the Rust graph (`pob_oracle::inject`), the Rust
//! segment runs on a `PerformCtx` built from the injected locals
//! (`pob_calc::probe::run_segment`), and the exported state (env, GlobalCache, the 24 ctx
//! locals) is diffed against the `perform:2997` dump (the segment's `:out`; §9.2 tolerance,
//! `.parity/nondet` sets).
//!
//! Gate: 100 % of the curated corpus (`corpus/PF-E/curated.txt`: aura, herald, curse and
//! warcry builds) and ≥ 98 % of all57 (`.parity/builds`). A hit whose run calls a callee that is
//! still an `unported!` stub (PF-A's `mergeBuff` and `determineCursePriority`, OF-A's
//! `calcSkillDuration`, …) cannot match PoB and is reported as **pending**, with the stub
//! sites, instead of passing or failing; the gate applies to the hits that ran on ported code.
//!
//! Inputs: `data/dump`, `.parity/builds`, and the dumps
//! (`scripts/port/oracle.sh run --build X.xml --stage 'perform:2308@main,perform:2997@main'`
//! fills the probe cache). Knobs: `PFE_BUILDS=stem1,stem2`, `PFE_VERBOSE=1` (every mismatch).
//! A missing input skips with a message; with `POB_ORACLE_REQUIRE=1` (or `CI`) it fails.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use pob_oracle::diff::Mismatch;
use pob_oracle::oracle::find_parity_dir;
use pob_oracle::schema::StateDump;
use pob_oracle::testkit;

const STAGE_IN: &str = "perform:2308";
const STAGE_OUT: &str = "perform:2997";
/// This unit's own segment (must never show up as a stub hit).
const OWN_SITE: &str = "Modules/CalcPerform.lua:2308-2996";

fn skip(why: &str) {
    let required = std::env::var_os("POB_ORACLE_REQUIRE").is_some_and(|v| v == "1")
        || std::env::var_os("CI").is_some_and(|v| !v.is_empty() && v != "false" && v != "0");
    assert!(!required, "required input missing: {why}");
    eprintln!("SKIPPED: {why}");
}

fn stem(p: &Path) -> &str {
    p.file_stem().and_then(|s| s.to_str()).unwrap_or_default()
}

/// `.parity/builds/*.xml`, optionally narrowed by `PFE_BUILDS`.
fn all57(parity: &Path) -> Vec<PathBuf> {
    let only: Option<Vec<String>> = std::env::var("PFE_BUILDS")
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

/// The curated stems (`corpus/PF-E/curated.txt`).
fn curated() -> Vec<String> {
    let p = Path::new(env!("CARGO_MANIFEST_DIR")).join("corpus/PF-E/curated.txt");
    std::fs::read_to_string(p)
        .unwrap_or_default()
        .lines()
        .map(|l| l.split('#').next().unwrap_or("").trim().to_owned())
        .filter(|l| !l.is_empty())
        .collect()
}

/// A mismatch path with list indices and `#n` parts collapsed (for the summary).
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

/// L4 on one build: inject `perform:2308`, run the segment, diff against `perform:2997`.
fn run_hit(xml: &Path) -> Outcome {
    let stem = stem(xml).to_owned();
    // `:in` and `:out` from one PoB process (one dump directory): two runs differ in mod
    // order and in the gem variants PoB resolves (§9.5), so a pair across runs never matches
    let pair = testkit::dump_dirs(&stem, Some(xml))
        .into_iter()
        .find_map(|d| {
            let one = [d];
            Some((
                testkit::find_dump(&one, STAGE_IN)?,
                testkit::find_dump(&one, STAGE_OUT)?,
            ))
        });
    let Some((pin, pout)) = pair else {
        return Outcome::Missing(format!(
            "{stem}: no {STAGE_IN}/{STAGE_OUT} dump (scripts/port/oracle.sh run --build {} --stage '{STAGE_IN}@main,{STAGE_OUT}@main')",
            xml.display()
        ));
    };
    let (din, dout) = match (StateDump::load(&pin), StateDump::load(&pout)) {
        (Ok(a), Ok(b)) => (a, b),
        _ => return Outcome::Error(format!("{stem}: unreadable dump {}", pin.display())),
    };
    if din.n != dout.n || din.env_id != dout.env_id {
        return Outcome::Error(format!(
            "{stem}: {} and {} are not one perform call",
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
        "the PF-E segment is still a stub"
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
                // a stub's Default value made Lua-faithful code raise (e.g. `buffs[name]` nil
                // because mergeBuff did nothing)
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
        let verbose = std::env::var_os("PFE_VERBOSE").is_some();
        eprintln!(
            "== PF-E L4 {title}: {} hits: {} pass, {} fail, {} error, {} pending (callee stubs); {} missing dumps",
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
fn pfe_l4_buffs_segment() {
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
    assert!(!cur.is_empty(), "corpus/PF-E/curated.txt is empty");
    let mut curated_t = Tally::default();
    let mut all_t = Tally::default();
    for xml in &builds {
        let s = stem(xml).to_owned();
        let o = run_hit(xml);
        // the same outcome counts for both corpora
        let o2 = match &o {
            Outcome::Pass => Outcome::Pass,
            Outcome::Fail(mm) => Outcome::Fail(mm.clone()),
            Outcome::Pending(a, b) => Outcome::Pending(a.clone(), b.clone()),
            Outcome::Missing(m) => Outcome::Missing(m.clone()),
            Outcome::Error(e) => Outcome::Error(e.clone()),
        };
        if cur.contains(&s) {
            curated_t.add(&s, o2);
        }
        all_t.add(&s, o);
    }
    curated_t.report("curated");
    all_t.report("all57");
    if all_t.hits == 0 {
        return skip("no perform:2308/perform:2997 dumps for any build (see the missing list)");
    }
    // the curated corpus: every hit that ran on ported code is exact
    assert!(
        curated_t.fail.is_empty() && curated_t.errors.is_empty(),
        "PF-E L4 curated: {} fail, {} error (gate 100 %)",
        curated_t.fail.len(),
        curated_t.errors.len()
    );
    // all57: ≥ 98 % of the gated hits
    let gated = all_t.gated();
    if gated > 0 {
        let ok = all_t.pass as f64 / gated as f64;
        assert!(
            ok >= 0.98,
            "PF-E L4 all57: {}/{gated} hits exact ({:.1} %, gate 98 %)",
            all_t.pass,
            ok * 100.0
        );
    }
}
