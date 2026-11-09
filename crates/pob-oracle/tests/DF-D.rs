//! Unit DF-D acceptance (calc doc §11.1 row DF-B..G, §9.4 L4): CalcDefence.lua
//! 1681-2152 (`calc_defence::est_inputs::seg`).
//!
//! For every build and every hit `n` of the stage pair
//! `buildDefenceEstimations:1681#n` → `buildDefenceEstimations:2153#n` (one per
//! `buildDefenceEstimations` call: the main env, the calcs env with
//! breakdowns, and the extra envs PoB builds), the `:in` dump is injected, the
//! Rust segment runs, and the state is diffed against the `:out` dump
//! (`pob_oracle::testkit::segment_diff`, §9.2 tolerance).
//!
//! Corpora: all57 (`.parity/builds`, gate ≥ 98 % of hits clean, the rest
//! listed in `known_diffs.toml`) and the unit corpus `corpus/DF-D/*.xml`
//! (`derive.py`: every `enemyDamageType`, unlucky EHP, explicit enemy damage,
//! pen, overwhelm and crit inputs, boss skill presets with enemy conversion;
//! gate 100 %), plus slice5.
//!
//! A mismatching hit whose segment run reached an `unported!` stub is listed as
//! *blocked* with the stub sites (for diagnosis), and any blocked hit fails the
//! test: the only stub the segment used to reach, DF-A's `calcs.armourReduction`
//! (CalcDefence.lua:75-77), is ported, so a stub hit now is a regression.
//!
//! Dumps are read first from the private `target/tmp/dfd-probe/<stem>/state/` (see
//! [`private_dir`]), then from the shared locations of `testkit::dump_dirs`; the `:in` and
//! `:out` of a hit always come from the same directory (CCR-0067 §3). The shared cache is
//! filled with
//! `scripts/port/oracle.sh run --build <xml> --stage buildDefenceEstimations:1681,buildDefenceEstimations:2153`. Knobs: `DFD_BUILDS=stem1,stem2`, `DFD_VERBOSE=1` (print
//! every mismatch). A corpus without dumps skips with a message; with
//! `POB_ORACLE_REQUIRE=1` (or `CI`) it fails instead.

use std::path::{Path, PathBuf};
use std::sync::Mutex;

use pob_calc::lua_table::Val;
use pob_oracle::oracle::find_parity_dir;
use pob_oracle::schema::StateDump;
use pob_oracle::testkit;

const STAGE_IN: &str = "buildDefenceEstimations:1681";
const STAGE_OUT: &str = "buildDefenceEstimations:2153";

/// The stub registry is process-global: one corpus at a time.
static SERIAL: Mutex<()> = Mutex::new(());

fn skip(why: &str) {
    let required = std::env::var_os("POB_ORACLE_REQUIRE").is_some_and(|v| v == "1")
        || std::env::var_os("CI").is_some_and(|v| !v.is_empty() && v != "false" && v != "0");
    assert!(!required, "required input missing: {why}");
    eprintln!("SKIPPED: {why}");
}

fn stem(p: &Path) -> &str {
    p.file_stem().and_then(|s| s.to_str()).unwrap_or_default()
}

fn xmls(dir: &Path) -> Vec<PathBuf> {
    let mut v: Vec<PathBuf> = std::fs::read_dir(dir)
        .map(|r| {
            r.filter_map(Result::ok)
                .map(|e| e.path())
                .filter(|p| p.extension().is_some_and(|x| x == "xml"))
                .collect()
        })
        .unwrap_or_default();
    v.sort();
    if let Ok(list) = std::env::var("DFD_BUILDS") {
        let want: Vec<&str> = list.split(',').map(str::trim).collect();
        v.retain(|p| want.contains(&stem(p)));
    }
    v
}

#[derive(Default)]
struct Tally {
    builds_with_dumps: usize,
    builds_without: Vec<String>,
    hits: usize,
    clean: usize,
    failed: Vec<(String, Vec<String>)>,
    blocked: Vec<(String, Vec<String>, usize)>,
}

impl Tally {
    fn judged(&self) -> usize {
        self.hits - self.blocked.len()
    }

    fn report(&self, name: &str) {
        let verbose = std::env::var_os("DFD_VERBOSE").is_some();
        eprintln!(
            "DF-D L4 {name}: builds with dumps {} (without {}), hits {}, judged {}, clean {}, failed {}, blocked by other units' stubs {}",
            self.builds_with_dumps,
            self.builds_without.len(),
            self.hits,
            self.judged(),
            self.clean,
            self.failed.len(),
            self.blocked.len()
        );
        if !self.builds_without.is_empty() {
            eprintln!("  no dumps: {}", self.builds_without.join(", "));
        }
        for (what, mm) in &self.failed {
            eprintln!("  FAIL {what}: {} mismatches", mm.len());
            for m in mm.iter().take(if verbose { usize::MAX } else { 8 }) {
                eprintln!("    {m}");
            }
        }
        for (what, sites, n) in &self.blocked {
            eprintln!("  BLOCKED {what}: stubs {sites:?}, {n} mismatches");
        }
    }
}

/// `testkit::segment_diff`, with the stub registry reset right before the
/// segment runs, so the stub sites reported are the ones the segment reached
/// (not those of loading the build or injecting the dump).
/// → (mismatches as text, stub sites hit by the segment run).
fn segment_diff(
    xml: &Path,
    st: &str,
    dirs: &[PathBuf],
    hin: &str,
    hout: &str,
) -> (Vec<String>, Vec<String>) {
    let one = |s: String| (vec![s], Vec::new());
    let (Some(pin), Some(pout)) = (
        testkit::find_dump(dirs, hin),
        testkit::find_dump(dirs, hout),
    ) else {
        return one(format!("missing dump {hin} or {hout}"));
    };
    let (Ok(din), Ok(dout)) = (StateDump::load(&pin), StateDump::load(&pout)) else {
        return one(format!(
            "unreadable dump {} / {}",
            pin.display(),
            pout.display()
        ));
    };
    let model = testkit::load_build(xml).unwrap_or_else(testkit::empty_build);
    let inj = match pob_oracle::inject::inject(&din, &model) {
        Ok(i) => i,
        Err(e) => return one(format!("inject {hin}: {e}")),
    };
    let locals: Vec<(String, Val)> = inj.locals.into_iter().collect();
    pob_calc::stub::stub_reset();
    let run = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        pob_calc::probe::run_segment(&din.probe, &locals)
    }));
    let stubs: Vec<String> = pob_calc::stub::stub_report()
        .into_iter()
        .map(|(s, c)| format!("{s} x{c}"))
        .collect();
    let after = match run {
        Ok(Ok((_flow, after))) => after,
        Ok(Err(e)) => return (vec![format!("run {hin}: {e}")], stubs),
        Err(p) => {
            let msg = p
                .downcast_ref::<pob_lua::LuaError>()
                .map(|e| e.0.clone())
                .or_else(|| p.downcast_ref::<String>().cloned())
                .or_else(|| p.downcast_ref::<&str>().map(|s| (*s).to_owned()))
                .unwrap_or_else(|| "panic".to_owned());
            return (vec![format!("run {hin}: {msg}")], stubs);
        }
    };
    let env = after
        .iter()
        .find(|(k, _)| k == "env")
        .and_then(|(_, v)| v.obj::<pob_calc::env::Env>())
        .or(inj.env);
    let mm = testkit::diff_against(&dout, env.as_ref(), &after, st)
        .into_iter()
        .map(|m| format!("{} {:?}: pob={:?} ours={:?}", m.path, m.kind, m.pob, m.ours))
        .collect();
    (mm, stubs)
}

/// The private dump directory of a build: `target/tmp/dfd-probe/<stem>/state/`, filled by
/// `scripts/port/oracle.sh exec probe <xml> --out-dir target/tmp/dfd-probe/<stem> --stages
/// docs/port/stages.toml --select buildDefenceEstimations:1681,buildDefenceEstimations:2153`.
fn private_dir(st: &str) -> PathBuf {
    Path::new(env!("CARGO_TARGET_TMPDIR"))
        .join("dfd-probe")
        .join(st)
        .join("state")
}

/// Run the L4 diff over every hit pair of every build in `builds`.
fn run_corpus(builds: &[PathBuf]) -> Tally {
    let mut t = Tally::default();
    for xml in builds {
        let st = stem(xml);
        let mut all_dirs = vec![private_dir(st)];
        all_dirs.extend(testkit::dump_dirs(st, Some(xml)));
        all_dirs.retain(|d| d.is_dir());
        let mut any = false;
        for n in 1.. {
            let hin = format!("{STAGE_IN}#{n}");
            let hout = format!("{STAGE_OUT}#{n}");
            // both dumps from one directory, i.e. one PoB process (CCR-0067 §3: the shared
            // cache can pair an `:in` and an `:out` written by different runs)
            let Some(dir) = all_dirs.iter().find(|d| {
                let one = [(*d).clone()];
                testkit::find_dump(&one, &hin).is_some()
                    && testkit::find_dump(&one, &hout).is_some()
            }) else {
                break;
            };
            let dirs = [dir.clone()];
            any = true;
            t.hits += 1;
            let (mm, stubs) = segment_diff(xml, st, &dirs, &hin, &hout);
            let what = format!("{st} {hin}");
            if mm.is_empty() {
                t.clean += 1;
            } else if !stubs.is_empty() {
                t.blocked.push((what, stubs, mm.len()));
            } else {
                t.failed.push((what, mm));
            }
        }
        if any {
            t.builds_with_dumps += 1;
        } else {
            t.builds_without.push(st.to_owned());
        }
    }
    t
}

fn gate(t: &Tally, name: &str, min_clean: f64) {
    t.report(name);
    if t.hits == 0 {
        return skip(&format!(
            "{name}: no {STAGE_IN}/{STAGE_OUT} dumps (scripts/port/oracle.sh run --build <xml> --stage {STAGE_IN},{STAGE_OUT})"
        ));
    }
    // the only stub this segment reaches (DF-A's `calcs.armourReduction`) is ported: a hit
    // that reaches any `unported!` site now is a regression, not a pending dependency
    assert!(
        t.blocked.is_empty(),
        "{name}: {} hits reached `unported!` stubs (first: {:?})",
        t.blocked.len(),
        t.blocked.first()
    );
    let ratio = t.clean as f64 / t.judged() as f64;
    assert!(
        ratio >= min_clean,
        "{name}: {} of {} judged hits clean ({:.2} %), gate {:.0} %",
        t.clean,
        t.judged(),
        ratio * 100.0,
        min_clean * 100.0
    );
}

#[test]
fn dfd_l4_all57() {
    let _g = SERIAL
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    if !testkit::init_default_data() {
        return skip("no data dump (data/dump)");
    }
    let Some(parity) = find_parity_dir() else {
        return skip("no .parity directory");
    };
    let builds = xmls(&parity.join("builds"));
    if builds.is_empty() {
        return skip("no .parity/builds/*.xml");
    }
    let t = run_corpus(&builds);
    gate(&t, "all57", 0.98);
}

#[test]
fn dfd_l4_unit_corpus() {
    let _g = SERIAL
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    if !testkit::init_default_data() {
        return skip("no data dump (data/dump)");
    }
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("corpus/DF-D");
    let builds = xmls(&dir);
    assert!(
        !builds.is_empty(),
        "corpus/DF-D has no builds (run derive.py)"
    );
    let t = run_corpus(&builds);
    gate(&t, "unit corpus", 1.0);
}

#[test]
fn dfd_l4_slice5() {
    let _g = SERIAL
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    if !testkit::init_default_data() {
        return skip("no data dump (data/dump)");
    }
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("corpus/U0/slice5");
    let t = run_corpus(&xmls(&dir));
    gate(&t, "slice5", 1.0);
}
