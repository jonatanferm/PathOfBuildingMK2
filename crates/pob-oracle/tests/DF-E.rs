//! Unit DF-E acceptance (calc doc §11.1 row DF-B..G, §9.4 L4): the Rust segment
//! `calc_defence::est_pools::seg` (CalcDefence.lua 2153-2611) is run on the state PoB dumped at
//! stage `buildDefenceEstimations:2153#n` and diffed against PoB's dump at
//! `buildDefenceEstimations:2612#n`, for every hit `n` (one per `buildDefenceEstimations` call:
//! player and minions, each calc pass).
//!
//! * `df_e_l4_corpus`: the unit corpus `crates/pob-oracle/corpus/DF-E/*.xml` (builds made to hit
//!   the segment's branches: MoM + Eldritch Battery, typed MoM, ES bypass overrides, stun
//!   threshold variants, low life, Petrified Blood, guard, aegis, Frost Shield, ally redirects,
//!   Vaal Arctic Armour, …). Gate: 100% of hits.
//! * `df_e_l4_all57`: every `.parity/builds/*.xml`. Gate: ≥ 98% of hits (calc doc §9.4).
//!
//! Inputs: `data/dump` and the dumps of both stages **from one PoB run** per build (PoB's
//! string-keyed `pairs` order differs per process, so an `:in` dump of one run and an `:out`
//! dump of another disagree on mod order; the shared probe cache can hold such a mix when other
//! units re-probe the same build). Taken from `$DFE_PROBE_DIR/<stem>/state/` (default
//! `<workspace>/target/dfe-probe`) when it exists, else from this test's selection entry of the
//! shared probe cache (`testkit::probe_selection`: one PoB run per build, probed when absent,
//! under the machine-wide oracle lock; `POB_QUERY_CACHE=refresh` re-probes). Fill the private
//! directory with
//! `scripts/port/oracle.sh exec probe X.xml --out-dir target/dfe-probe/<stem> --stages
//! docs/port/stages.toml --select buildDefenceEstimations:2153,buildDefenceEstimations:2612`
//! (with `POB_PARITY_DIR` set). Knobs: `DFE_BUILDS=stem1,stem2`, `DFE_VERBOSE=1` (print every
//! mismatch). A missing input skips with a message; with `POB_ORACLE_REQUIRE=1` (or `CI`) it
//! fails instead.
//!
//! Tolerated (counted and reported, not failed): mismatches the `:in` dump's own round trip
//! shows, the ARM64 `%d` cast host artefact, and (FUP1) buff mod lists that differ only in
//! PoB's per-process `pairs(stats)` order (`testkit::normalise_lua_order`, the check PF-C
//! uses; on dumps of one run none occurs, since the segment does not rebuild buff lists).

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use pob_calc::lua_table::Val;
use pob_oracle::diff::{Mismatch, MismatchKind};
use pob_oracle::oracle::find_parity_dir;
use pob_oracle::schema::StateDump;
use pob_oracle::testkit;
use serde_json::Value as J;

const STAGE_IN: &str = "buildDefenceEstimations:2153";
const STAGE_OUT: &str = "buildDefenceEstimations:2612";

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
    if let Ok(list) = std::env::var("DFE_BUILDS") {
        let want: Vec<&str> = list.split(',').map(str::trim).collect();
        v.retain(|p| want.contains(&stem(p)));
    }
    v
}

/// The DF-E selection: both stages, every hit (all actors and passes).
const SELECT: &str = "buildDefenceEstimations:2153,buildDefenceEstimations:2612";

/// A build's dumps, all from one PoB run.
enum Dumps {
    /// `$DFE_PROBE_DIR/<stem>/state/` (default `<workspace>/target/dfe-probe`).
    Private(PathBuf),
    /// This selection's entry of the shared probe cache (held while the dumps are read).
    #[cfg_attr(not(feature = "luajit"), allow(dead_code))]
    Probed(pob_oracle::runner::ProbeDumps),
}

impl Dumps {
    fn dir(&self) -> &Path {
        match self {
            Dumps::Private(p) => p,
            Dumps::Probed(d) => d.path(),
        }
    }
}

/// The build's DF-E dumps: the private directory when it exists, else the selection's entry in
/// the shared probe cache (`testkit::probe_selection`, probed when absent). FUP1: the earlier
/// fallback to testkit's directories (`.parity/probe`, `.parity/probe-cache/<sha>/<build-key>/`)
/// could pair an `:in` and an `:out` dump of different PoB runs, whose string-keyed `pairs`
/// orders differ: on 2026-11-10 that alone made 139 of the 363 all57 hits fail (mod order of
/// `env.auxSkillList[].baseSkillModList`, `env.buffs.*`, buff mod lists, …: state the segment
/// never touches); on dumps of one run per build every hit passes.
fn build_dumps(build: &Path) -> Result<Dumps, String> {
    let root = std::env::var_os("DFE_PROBE_DIR").map(PathBuf::from).unwrap_or_else(|| {
        std::env::var_os("CARGO_TARGET_DIR")
            .map(PathBuf::from)
            .unwrap_or_else(|| Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target"))
            .join("dfe-probe")
    });
    let private = root.join(stem(build)).join("state");
    if private.is_dir() {
        return Ok(Dumps::Private(private));
    }
    probe(build).map(Dumps::Probed)
}

#[cfg(feature = "luajit")]
fn probe(build: &Path) -> Result<pob_oracle::runner::ProbeDumps, String> {
    let stages = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../docs/port/stages.toml");
    let opts = pob_oracle::runner::ProbeOptions {
        select: SELECT.split(',').map(str::to_owned).collect(),
        provenance: false,
        ..Default::default()
    };
    testkit::probe_selection(
        Path::new(env!("CARGO_BIN_EXE_pob_oracle")),
        build,
        Some(&stages),
        &opts,
    )
}

#[cfg(not(feature = "luajit"))]
fn probe(_build: &Path) -> Result<pob_oracle::runner::ProbeDumps, String> {
    Err("needs the luajit feature".to_owned())
}

/// The hit numbers `n` of `<STAGE_IN>#n` dumped in `dir`.
fn hits(dir: &Path) -> BTreeSet<u64> {
    let prefix = format!("{STAGE_IN}#");
    let mut out = BTreeSet::new();
    let Ok(rd) = std::fs::read_dir(dir) else {
        return out;
    };
    for e in rd.filter_map(Result::ok) {
        let name = e.file_name().to_string_lossy().into_owned();
        if let Some(rest) = name.strip_prefix(&prefix) {
            let n: String = rest.chars().take_while(char::is_ascii_digit).collect();
            if let Ok(n) = n.parse() {
                out.insert(n);
            }
        }
    }
    out
}

/// One L4 run: `testkit::segment_diff` with the mismatches kept as values.
fn seg_mismatches(build: &Path, dir: &Path, n: u64) -> Result<(Vec<Mismatch>, StateDump), String> {
    let s = stem(build);
    let (hin, hout) = (format!("{STAGE_IN}#{n}"), format!("{STAGE_OUT}#{n}"));
    // Both dumps from one directory of one PoB process (`build_dumps`).
    let one = [dir.to_path_buf()];
    let (pin, pout) = testkit::find_dump(&one, &hin)
        .zip(testkit::find_dump(&one, &hout))
        .ok_or_else(|| format!("missing dumps {hin} + {hout} in {}", dir.display()))?;
    let din = StateDump::load(&pin).map_err(|e| format!("{}: {e}", pin.display()))?;
    let dout = StateDump::load(&pout).map_err(|e| format!("{}: {e}", pout.display()))?;
    let model = testkit::load_build(build).unwrap_or_else(testkit::empty_build);
    let inj = pob_oracle::inject::inject(&din, &model).map_err(|e| format!("inject {hin}: {e}"))?;
    let locals: Vec<(String, Val)> = inj.locals.into_iter().collect();
    let run = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        pob_calc::probe::run_segment(&din.probe, &locals)
    }));
    let after = match run {
        Ok(Ok((_flow, after))) => after,
        Ok(Err(e)) => return Err(format!("run {hin}: {e}")),
        Err(p) => {
            let msg = p
                .downcast_ref::<pob_lua::LuaError>()
                .map(|e| e.0.clone())
                .or_else(|| p.downcast_ref::<String>().cloned())
                .or_else(|| p.downcast_ref::<&str>().map(|s| (*s).to_owned()))
                .unwrap_or_else(|| "panic".to_owned());
            return Err(format!("run {hin}: {msg}"));
        }
    };
    let env = after
        .iter()
        .find(|(k, _)| k == "env")
        .and_then(|(_, v)| v.obj::<pob_calc::env::Env>())
        .or(inj.env);
    // FUP1: with the shared LUA-ORDER normalisation (order-only buff mod lists, see
    // `is_buff_list_order`)
    Ok((testkit::diff_against_normalised(&dout, env.as_ref(), &after, s), din))
}

/// LUA-ORDER (calc doc §3.4/§9.5), FUP1: a `…buffList[i].modList` whose mods are PoB's in
/// another order inside each source. The order comes from `pairs(stats)` in
/// `mergeSkillInstanceMods` (CalcActiveSkill.lua:62), PoB's own string-key order, random per
/// process; `testkit::normalise_lua_order` reduces such a list to one `LuaOrder` mismatch at
/// the list's path (the same check PF-C applies).
fn is_buff_list_order(m: &Mismatch) -> bool {
    m.kind == MismatchKind::LuaOrder && testkit::buff_mod_list_path(&m.path) == Some(m.path.as_str())
}

fn key(m: &Mismatch) -> String {
    format!("{} {:?}", m.path, m.kind)
}

/// `path kind` of each mismatch that `export(inject(<STAGE_IN>#n))` already shows against the
/// `:in` dump itself: an injection round-trip artefact of state this segment never touches
/// (ORC-X scope), not a segment difference.
fn round_trip_artefacts(din: &StateDump) -> BTreeSet<String> {
    match testkit::round_trip(din) {
        Ok(rt) => rt.mismatches.iter().map(key).collect(),
        Err(_) => BTreeSet::new(),
    }
}

/// Host artefact (CCR-0001, AMENDMENTS A6): `string.format("%d", x)` with a NaN or ±inf `x`
/// prints `-9223372036854775808` on x64 PoB (`cvttsd2si`, the profile the port follows) but
/// saturates on the ARM64 dual-number oracle (`0` for NaN, `±9223372036854775807` for ±inf).
/// E.g. `breakdown.FrostShieldLife[5]` = `s_format("= %d", 0 / (0 / 100) * 1)` (CalcDefence.lua:2498).
/// Only tolerated against an ARM64 oracle (this host); see known_diffs.toml `DF-E-1`.
fn is_int_cast_artefact(m: &Mismatch) -> bool {
    const X64_MIN: &str = "-9223372036854775808";
    // the keys of known_diffs.toml `DF-E-1`
    const KEYS: &[&str] = &["locals.breakdown.FrostShieldLife[5]"];
    if !cfg!(target_arch = "aarch64") || m.kind != MismatchKind::Value || !KEYS.contains(&m.path.as_str()) {
        return false;
    }
    let (Some(J::String(pob)), Some(J::String(ours))) = (&m.pob, &m.ours) else {
        return false;
    };
    ours.contains(X64_MIN)
        && ["0", "9223372036854775807", "-9223372036854775807"]
            .iter()
            .any(|arm| ours.replace(X64_MIN, arm) == *pob)
}

struct Tally {
    hits: usize,
    passed: usize,
    /// Mismatches dropped because the `:in` dump's own round trip shows them.
    artefacts: usize,
    /// Mismatches dropped as the ARM64 `%d` cast host artefact.
    cast_artefacts: usize,
    cast_artefact_paths: BTreeSet<String>,
    /// Buff mod lists accepted as LUA-ORDER ([`is_buff_list_order`]).
    lua_order: usize,
    builds_without_dumps: Vec<String>,
    failures: Vec<String>,
}

fn run(builds: &[PathBuf]) -> Tally {
    let verbose = std::env::var_os("DFE_VERBOSE").is_some();
    let mut t = Tally {
        hits: 0,
        passed: 0,
        artefacts: 0,
        cast_artefacts: 0,
        cast_artefact_paths: BTreeSet::new(),
        lua_order: 0,
        builds_without_dumps: Vec::new(),
        failures: Vec::new(),
    };
    for b in builds {
        let dumps = match build_dumps(b) {
            Ok(d) => d,
            Err(e) => {
                t.hits += 1;
                t.failures.push(format!("{}: probing failed: {e}", stem(b)));
                continue;
            }
        };
        let ns = hits(dumps.dir());
        if ns.is_empty() {
            t.builds_without_dumps.push(stem(b).to_owned());
            continue;
        }
        for n in ns {
            t.hits += 1;
            let mm: Vec<String> = match seg_mismatches(b, dumps.dir(), n) {
                Err(e) => vec![e],
                Ok((all, din)) => {
                    let rt = if all.is_empty() { BTreeSet::new() } else { round_trip_artefacts(&din) };
                    let mut kept = Vec::new();
                    for m in all {
                        if rt.contains(&key(&m)) {
                            t.artefacts += 1;
                        } else if is_buff_list_order(&m) {
                            t.lua_order += 1;
                        } else if is_int_cast_artefact(&m) {
                            t.cast_artefacts += 1;
                            t.cast_artefact_paths.insert(m.path.clone());
                        } else {
                            kept.push(format!("{} {:?}: pob={:?} ours={:?}", m.path, m.kind, m.pob, m.ours));
                        }
                    }
                    kept
                }
            };
            if mm.is_empty() {
                t.passed += 1;
            } else {
                let shown = if verbose { mm.len() } else { mm.len().min(8) };
                t.failures.push(format!(
                    "{} #{n}: {} mismatches\n    {}",
                    stem(b),
                    mm.len(),
                    mm[..shown].join("\n    ")
                ));
            }
        }
    }
    t
}

fn report(name: &str, t: &Tally) {
    eprintln!(
        "{name}: {}/{} hits pass ({:.2}%); dropped: {} :in round-trip artefacts, {} ARM64 %d-cast artefacts, {} LUA-ORDER buff mod lists",
        t.passed,
        t.hits,
        if t.hits == 0 { 0.0 } else { 100.0 * t.passed as f64 / t.hits as f64 },
        t.artefacts,
        t.cast_artefacts,
        t.lua_order
    );
    if !t.cast_artefact_paths.is_empty() {
        eprintln!("{name}: %d-cast artefact paths: {:?}", t.cast_artefact_paths);
    }
    if !t.builds_without_dumps.is_empty() {
        eprintln!(
            "{name}: no {STAGE_IN} dumps for {} build(s): {} (PoB never reached the stage in the probed run)",
            t.builds_without_dumps.len(),
            t.builds_without_dumps.join(", ")
        );
    }
    for f in &t.failures {
        eprintln!("FAIL {f}");
    }
}

#[test]
fn df_e_l4_corpus() {
    if !testkit::init_default_data() {
        return skip("no data dump (data/dump)");
    }
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("corpus/DF-E");
    let builds = xmls(&dir);
    if builds.is_empty() {
        return skip("no unit corpus builds in corpus/DF-E");
    }
    let t = run(&builds);
    report("DF-E corpus", &t);
    if t.hits == 0 {
        return skip("no DF-E corpus dumps");
    }
    assert!(
        t.builds_without_dumps.is_empty() || std::env::var_os("DFE_BUILDS").is_some(),
        "corpus builds without dumps: {:?}",
        t.builds_without_dumps
    );
    assert_eq!(t.passed, t.hits, "DF-E corpus: every hit must pass (L4 100%)");
}

#[test]
fn df_e_l4_all57() {
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
    let t = run(&builds);
    report("DF-E all57", &t);
    if t.hits == 0 {
        return skip("no all57 dumps for the DF-E stages");
    }
    assert!(
        t.passed as f64 >= 0.98 * t.hits as f64,
        "DF-E all57: {}/{} hits pass, below the 98% gate",
        t.passed,
        t.hits
    );
}
