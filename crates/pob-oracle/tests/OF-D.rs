//! Unit OF-D acceptance (calc doc §11.1 row OF-D, §9.4 L4): for every build,
//! inject PoB's `offence:2174#n` dump (the state before the first pass loop,
//! CalcOffence.lua:2175), run the Rust segment
//! (`calc_offence::speed::seg_speed`, CalcOffence.lua 2174-2605) and diff the
//! exported state against PoB's `offence:2606#n` dump (the state before the
//! second pass loop) with the §9.2 tolerance. The env, the GlobalCache and
//! the ctx locals are compared.
//!
//! Gate (§9.4 L4): 100% of the unit corpus (`corpus/OF-D/`) and ≥ 98% of the
//! all57 hits clean. A hit whose run reached an `unported!` body of another
//! unit (`combineStat`, OF-C; `calcs.hitChance`, DF-A) is reported as
//! *blocked* with the stub sites it reached and is left out of the gate's
//! denominator, so the gate measures this segment only. Likewise a hit with
//! mismatches whose skill mod-store chain (`locals.skillModList` and its
//! `parent`s, i.e. `activeSkill.baseSkillModList` and the actor `modDB` it
//! hangs off) reaches a `{"$foreign": …}` object is reported as *foreign*:
//! mirage and trigger envs copy skills whose stores' parent is the `modDB` of
//! the env they were copied from, `inject` turns that into an empty
//! placeholder (inject.rs module docs), and so the mods it holds are absent
//! on the Rust side. A `$foreign` anywhere else in the dump (most non-first
//! envs carry some, e.g. `globalCache.cachedData.*.ActiveSkill`,
//! `env.auxSkillList[*].socketGroup`) does not make a hit foreign: its
//! mismatches count. `OFD_STRICT=1` counts blocked and foreign hits as
//! failures.
//!
//! Dumps come from the shared probe cache (`testkit::probe_selection`), probed by this test
//! when absent (LuaJIT, the machine-wide oracle lock, docs/port/stages.toml):
//! every `offence:2174` / `offence:2606` hit of the build load (all envs: the
//! MAIN env, the CALCS env with breakdowns, the full-DPS and calculator
//! envs), up to `OFD_MAX_DUMPS` dumps (default 80); with `OFD_MAIN_ONLY=1`
//! only the main env's pair (`@main`). CK2: only this run's entry is read (no fallback to
//! the pre-SelectionCache `testkit::dump_dirs`, which served dumps of any patched-source sha).
//!
//! Knobs: `OFD_BUILDS=stem1,stem2`, `OFD_LIMIT=n`, `OFD_VERBOSE=1` (every
//! mismatch). A missing input skips with a message; with
//! `POB_ORACLE_REQUIRE=1` (or `CI`) it fails instead.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use pob_calc::Val;
use pob_oracle::diff::Mismatch;
use pob_oracle::oracle::find_parity_dir;
use pob_oracle::schema::StateDump;
use pob_oracle::testkit;

const STAGE_IN: &str = "offence:2174";
const STAGE_OUT: &str = "offence:2606";
/// The Lua range this unit owns (stub sites inside it are this unit's own).
const OWN_SITE: &str = "Modules/CalcOffence.lua:2174-2605";

fn skip(why: &str) {
    let required = std::env::var_os("POB_ORACLE_REQUIRE").is_some_and(|v| v == "1")
        || std::env::var_os("CI").is_some_and(|v| !v.is_empty() && v != "false" && v != "0");
    assert!(!required, "required input missing: {why}");
    eprintln!("SKIPPED: {why}");
}

fn flag(name: &str) -> bool {
    std::env::var_os(name).is_some_and(|v| !v.is_empty() && v != "0")
}

fn stem(p: &Path) -> &str {
    p.file_stem().and_then(|s| s.to_str()).unwrap_or_default()
}

fn xml_files(dir: &Path) -> Vec<PathBuf> {
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
    if let Ok(list) = std::env::var("OFD_BUILDS") {
        let want: Vec<&str> = list.split(',').map(str::trim).collect();
        v.retain(|p| want.contains(&stem(p)));
    }
    if let Some(n) = std::env::var("OFD_LIMIT")
        .ok()
        .and_then(|s| s.parse::<usize>().ok())
    {
        v.truncate(n);
    }
    v
}

/// `a.b[3].c["x y"]` → `a.b[#].c["x y"]`.
fn generalize(path: &str) -> String {
    let mut out = String::new();
    let mut chars = path.chars().peekable();
    while let Some(c) = chars.next() {
        if c == '[' && chars.peek().is_some_and(|d| d.is_ascii_digit() || *d == '-' || *d == '@') {
            out.push_str("[#]");
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

fn clip(s: &str) -> String {
    if s.len() > 200 {
        let end = s.char_indices().nth(200).map_or(s.len(), |(i, _)| i);
        format!("{}…", &s[..end])
    } else {
        s.to_owned()
    }
}

// ---- probing --------------------------------------------------------------------------------------

/// The `offence:2174/2606` dumps of `xml`: the shared probe cache's entry of the selection under
/// `docs/port/stages.toml`, probed in a fresh `pob_oracle probe` process when absent (TP1:
/// `testkit::probe_selection`, keyed by the patched source, the build's bytes and the selection,
/// shared by every worktree). Hold the result while reading the dumps.
#[cfg(feature = "luajit")]
fn probe_dumps(xml: &Path, all_hits: bool) -> Option<pob_oracle::runner::ProbeDumps> {
    let stages = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../docs/port/stages.toml");
    let select = if all_hits {
        vec![STAGE_IN.to_owned(), STAGE_OUT.to_owned()]
    } else {
        vec![format!("{STAGE_IN}@main"), format!("{STAGE_OUT}@main")]
    };
    let max = std::env::var("OFD_MAX_DUMPS")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(80);
    let opts = pob_oracle::runner::ProbeOptions {
        select,
        max_dumps: max,
        provenance: false,
    };
    testkit::probe_selection(
        Path::new(env!("CARGO_BIN_EXE_pob_oracle")),
        xml,
        Some(&stages),
        &opts,
    )
    .ok()
}

#[cfg(not(feature = "luajit"))]
fn probe_dumps(_xml: &Path, _all_hits: bool) -> Option<pob_oracle::runner::ProbeDumps> {
    None
}

/// `(n, in, out)` for every hit number with both dumps present.
fn pairs(state: &Path) -> Vec<(u64, PathBuf, PathBuf)> {
    let mut ins: BTreeMap<u64, PathBuf> = BTreeMap::new();
    let mut outs: BTreeMap<u64, PathBuf> = BTreeMap::new();
    for e in std::fs::read_dir(state).into_iter().flatten().flatten() {
        let name = e.file_name().to_string_lossy().into_owned();
        // `.json` (a private probe run) or `.json.zst` (the shared probe cache)
        let Some(base) = name
            .strip_suffix(".json")
            .or_else(|| name.strip_suffix(".json.zst"))
        else {
            continue;
        };
        let Some((id, n)) = base.split_once('#') else {
            continue;
        };
        let Ok(n) = n.parse::<u64>() else { continue };
        if id == STAGE_IN {
            ins.insert(n, e.path());
        } else if id == STAGE_OUT {
            outs.insert(n, e.path());
        }
    }
    ins.into_iter()
        .filter_map(|(n, pin)| outs.remove(&n).map(|pout| (n, pin, pout)))
        .collect()
}

// ---- the L4 run -----------------------------------------------------------------------------------

enum Outcome {
    Clean,
    Mismatch(Vec<Mismatch>),
    /// The run reached `unported!` bodies of other units (sites listed):
    /// the diff when it ran to the end, else the error it stopped with.
    Blocked(Vec<String>, Result<Vec<Mismatch>, String>),
    /// Mismatches in a hit whose input references other envs' objects.
    Foreign(Vec<Mismatch>),
    Failed(String),
}

fn panic_text(p: &(dyn std::any::Any + Send)) -> String {
    p.downcast_ref::<pob_lua::LuaError>()
        .map(|e| e.0.clone())
        .or_else(|| p.downcast_ref::<String>().cloned())
        .or_else(|| p.downcast_ref::<&str>().map(|s| (*s).to_owned()))
        .unwrap_or_else(|| "panic".to_owned())
}

/// Whether the skill's mod-store chain (`locals.skillModList` and its
/// `parent`s: `skillModList` → `activeSkill.baseSkillModList` → the copied-
/// from env's `modDB`) reaches a `{"$foreign": …}` placeholder. Only then are
/// mods the segment reads missing on the Rust side; a `$foreign` elsewhere in
/// the dump (e.g. `globalCache.cachedData.*.ActiveSkill`,
/// `env.auxSkillList[*].socketGroup`, present in most non-first envs) does not
/// excuse a mismatch.
fn foreign_skill_store(root: &serde_json::Value) -> bool {
    use serde_json::Value as J;
    fn index<'a>(v: &'a J, ids: &mut BTreeMap<u64, &'a J>) {
        match v {
            J::Object(m) => {
                if let Some(id) = m.get("$id").and_then(J::as_u64) {
                    ids.entry(id).or_insert(v);
                }
                m.values().for_each(|c| index(c, ids));
            }
            J::Array(a) => a.iter().for_each(|c| index(c, ids)),
            _ => {}
        }
    }
    let mut ids = BTreeMap::new();
    index(root, &mut ids);
    let mut seen = std::collections::BTreeSet::new();
    let mut cur = root.get("locals").and_then(|l| l.get("skillModList"));
    while let Some(v) = cur {
        if let Some(r) = v.get("$ref").and_then(J::as_u64) {
            if !seen.insert(r) {
                return false;
            }
            cur = ids.get(&r).copied();
            continue;
        }
        if v.get("$foreign").is_some() {
            return true;
        }
        cur = v.get("parent");
    }
    false
}

fn run_one(stem: &str, pin: &Path, pout: &Path) -> Outcome {
    let (Ok(din), Ok(dout)) = (StateDump::load(pin), StateDump::load(pout)) else {
        return Outcome::Failed(format!("unreadable dump {} / {}", pin.display(), pout.display()));
    };
    let model = testkit::empty_build();
    let inj = match pob_oracle::inject::inject(&din, &model) {
        Ok(i) => i,
        Err(e) => return Outcome::Failed(format!("inject {}: {e}", din.hit)),
    };
    let locals: Vec<(String, Val)> = inj.locals.into_iter().collect();
    pob_calc::stub::stub_reset();
    let run = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        pob_calc::probe::run_segment(&din.probe, &locals)
    }));
    let stubs: Vec<String> = pob_calc::stub::stub_report()
        .into_iter()
        .filter(|(site, _)| *site != OWN_SITE)
        .map(|(site, n)| format!("{site} x{n}"))
        .collect();
    let own_stub = pob_calc::stub::stub_report()
        .iter()
        .any(|(site, _)| *site == OWN_SITE);
    if own_stub {
        return Outcome::Failed("the OF-D segment is still a stub".to_owned());
    }
    let after = match run {
        Ok(Ok((_flow, after))) => after,
        Ok(Err(e)) => return Outcome::Failed(format!("run {}: {e}", din.hit)),
        Err(p) => {
            let msg = format!("run {}: {}", din.hit, panic_text(&*p));
            return if stubs.is_empty() {
                Outcome::Failed(msg)
            } else {
                Outcome::Blocked(stubs, Err(msg))
            };
        }
    };
    let env = after
        .iter()
        .find(|(k, _)| k == "env")
        .and_then(|(_, v)| v.obj::<pob_calc::env::Env>())
        .or(inj.env);
    let mm = testkit::diff_against(&dout, env.as_ref(), &after, stem);
    if !stubs.is_empty() {
        Outcome::Blocked(stubs, Ok(mm))
    } else if mm.is_empty() {
        Outcome::Clean
    } else if foreign_skill_store(&din.root) {
        Outcome::Foreign(mm)
    } else {
        Outcome::Mismatch(mm)
    }
}

#[derive(Default)]
struct Tally {
    hits: usize,
    clean: usize,
    mismatched: Vec<String>,
    blocked: Vec<String>,
    blocked_clean: usize,
    blocked_sites: BTreeMap<String, usize>,
    foreign: Vec<String>,
    failed: Vec<String>,
    missing: Vec<String>,
    by_path: BTreeMap<String, (usize, String)>,
}

impl Tally {
    fn add_mismatches(&mut self, what: &str, mm: &[Mismatch]) {
        let verbose = flag("OFD_VERBOSE");
        for m in mm {
            let key = format!("{:?} {}", m.kind, generalize(&m.path));
            let e = self.by_path.entry(key).or_insert_with(|| {
                (
                    0,
                    format!(
                        "{what}: {} pob={} ours={}",
                        m.path,
                        m.pob.as_ref().map_or("-".into(), |x| clip(&x.to_string())),
                        m.ours.as_ref().map_or("-".into(), |x| clip(&x.to_string()))
                    ),
                )
            });
            e.0 += 1;
            if verbose {
                eprintln!("  {what}: {} {:?} pob={:?} ours={:?}", m.path, m.kind, m.pob, m.ours);
            }
        }
    }

    fn run_build(&mut self, xml: &Path, state: &Path) {
        let stem = stem(xml);
        let ps = pairs(state);
        if ps.is_empty() {
            // the build's main skill never reached the first pass loop (e.g. disabled at 371)
            self.missing.push(format!("{stem}: no {STAGE_IN}/{STAGE_OUT} pair"));
            return;
        }
        for (n, pin, pout) in ps {
            self.hits += 1;
            let what = format!("{stem}#{n}");
            match run_one(stem, &pin, &pout) {
                Outcome::Clean => self.clean += 1,
                Outcome::Mismatch(mm) => {
                    self.mismatched.push(format!("{what}: {} mismatches", mm.len()));
                    self.add_mismatches(&what, &mm);
                }
                Outcome::Blocked(sites, res) => {
                    let how = match &res {
                        Ok(mm) if mm.is_empty() => {
                            self.blocked_clean += 1;
                            "ran to the end, clean".to_owned()
                        }
                        Ok(mm) => {
                            self.add_mismatches(&format!("{what} (blocked)"), mm);
                            format!("{} mismatches", mm.len())
                        }
                        Err(e) => format!("stopped: {}", clip(e)),
                    };
                    for s in &sites {
                        *self.blocked_sites.entry(s.split(" x").next().unwrap_or(s).to_owned()).or_default() += 1;
                    }
                    self.blocked.push(format!("{what}: {} ({how})", sites.join(", ")));
                }
                Outcome::Foreign(mm) => {
                    self.foreign.push(format!("{what}: {} mismatches", mm.len()));
                    self.add_mismatches(&format!("{what} (foreign)"), &mm);
                }
                Outcome::Failed(e) => self.failed.push(format!("{what}: {e}")),
            }
        }
    }

    /// (gate numerator, gate denominator).
    fn gate(&self) -> (usize, usize) {
        if flag("OFD_STRICT") {
            (self.clean, self.hits)
        } else {
            (self.clean, self.hits - self.blocked.len() - self.foreign.len())
        }
    }

    fn report(&self, title: &str) {
        let (num, den) = self.gate();
        eprintln!(
            "{title}: {} hits, {} clean, {} with mismatches, {} failed, {} blocked by other units' stubs \
             ({} of them ran clean anyway), {} foreign-state hits with mismatches, {} builds without \
             a pair; gate {num}/{den}{}",
            self.hits,
            self.clean,
            self.mismatched.len(),
            self.failed.len(),
            self.blocked.len(),
            self.blocked_clean,
            self.foreign.len(),
            self.missing.len(),
            if flag("OFD_STRICT") { " (strict)" } else { "" }
        );
        for (s, n) in &self.blocked_sites {
            eprintln!("  blocked by {s}: {n} hits");
        }
        for b in self.blocked.iter().take(60) {
            eprintln!("  blocked: {b}");
        }
        for f in self.foreign.iter().take(40) {
            eprintln!("  foreign: {f}");
        }
        for m in self.missing.iter().take(20) {
            eprintln!("  missing: {m}");
        }
        for f in self.failed.iter().take(20) {
            eprintln!("  failed: {f}");
        }
        for m in self.mismatched.iter().take(40) {
            eprintln!("  mismatched: {m}");
        }
        let mut paths: Vec<_> = self.by_path.iter().collect();
        paths.sort_by(|a, b| b.1 .0.cmp(&a.1 .0));
        for (p, (n, ex)) in paths.iter().take(40) {
            eprintln!("  {n:6} {p}\n         e.g. {ex}");
        }
    }
}

/// The stub registry is process-wide: the tests run one corpus at a time.
static SERIAL: std::sync::Mutex<()> = std::sync::Mutex::new(());

fn run_corpus(title: &str, builds: &[PathBuf], all_hits: bool) -> Option<Tally> {
    let _serial = SERIAL.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
    let hook = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        if info.payload().downcast_ref::<pob_lua::LuaError>().is_none() {
            hook(info);
        }
    }));
    let mut t = Tally::default();
    for xml in builds {
        let stem = stem(xml);
        let mut dirs: Vec<PathBuf> = Vec::new();
        let probed = probe_dumps(xml, all_hits);
        if let Some(d) = &probed {
            dirs.push(d.path().to_path_buf());
        }
        // `$POB_PROBE_DIR/<stem>/state/` (explicit override) also counts
        dirs.extend(testkit::override_dir(stem));
        let Some(state) = dirs.into_iter().find(|d| !pairs(d).is_empty()) else {
            t.missing.push(format!("{stem}: no {STAGE_IN}/{STAGE_OUT} dumps (probe failed or no hit)"));
            continue;
        };
        t.run_build(xml, &state);
    }
    t.report(title);
    (t.hits > 0).then_some(t)
}

/// L4 on all57 (every hit), gate ≥ 98% of the hits clean.
#[test]
fn ofd_l4_all57() {
    if !testkit::init_default_data() {
        return skip("no data dump (data/dump)");
    }
    let Some(parity) = find_parity_dir() else {
        return skip("no .parity directory");
    };
    if pob_oracle::oracle::find_pob_src().is_none() {
        return skip("no PoB checkout");
    }
    let builds = filter(xml_files(&parity.join("builds")));
    if builds.is_empty() {
        return skip("no .parity/builds/*.xml");
    }
    let all_hits = !flag("OFD_MAIN_ONLY");
    let Some(t) = run_corpus("OF-D L4 all57", &builds, all_hits) else {
        return skip("no offence:2174/2606 dumps could be produced");
    };
    let (num, den) = t.gate();
    assert!(t.failed.is_empty(), "OF-D L4 all57: failed runs");
    assert!(
        den == 0 || num * 100 >= den * 98,
        "OF-D L4 all57: {num}/{den} hits clean (< 98%)"
    );
    assert!(den > 0 || !flag("OFD_STRICT"), "no gated hits");
}

/// L4 on the unit corpus (`corpus/OF-D/*.xml`), gate 100%.
#[test]
fn ofd_l4_unit_corpus() {
    if !testkit::init_default_data() {
        return skip("no data dump (data/dump)");
    }
    if pob_oracle::oracle::find_pob_src().is_none() {
        return skip("no PoB checkout");
    }
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("corpus/OF-D");
    let builds = filter(xml_files(&dir));
    if builds.is_empty() {
        return skip("no corpus/OF-D/*.xml");
    }
    let Some(t) = run_corpus("OF-D L4 unit corpus", &builds, !flag("OFD_MAIN_ONLY")) else {
        return skip("no offence:2174/2606 dumps could be produced");
    };
    let (num, den) = t.gate();
    assert!(t.failed.is_empty(), "OF-D L4 unit corpus: failed runs");
    assert_eq!(num, den, "OF-D L4 unit corpus: {num}/{den} hits clean");
}
