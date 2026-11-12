//! Unit DF-B acceptance (calc doc §11.1 row DF-B, §9.4 L4): the defence segment
//! `calc_defence::primary::seg` (CalcDefence.lua 659-1210) against PoB.
//!
//! For every `defence:659#n` hit recorded under `corpus/DF-B/stages.toml` (each env's
//! player *and* minion `calcs.defence` call the oracle run dumped), the test injects the
//! `defence:659#n` state, runs the Rust segment and diffs the result against
//! `defence:1211#n` (env, GlobalCache and the dumped locals, `testkit::diff_against`).
//!
//! The segment's callees owned by other units are replayed while those are unported
//! (`primary::replay` hooks, calc doc §9.2 "Replay"):
//!
//! * `calcs.actionSpeedMod` and `calcs.resistances`: the recorded state delta between the
//!   line probes `defence:659 → defence:665` (only line 663 runs in between) and
//!   `defence:665 → defence:666` (line 665), applied onto the live Rust state with
//!   `pob_oracle::replay` (the hook returns `output.ActionSpeedMod` after the first);
//! * `doActorLifeMana` / `doActorLifeManaReservation` (minion actors only): the deltas
//!   `defence:674 → defence:675` and `defence:675 → defence:676` of the same call (same
//!   function-probe stack);
//! * `calcs.hitChance` (pure): PoB's own source text (Common.lua `round`, CalcDefence.lua
//!   58-64) evaluated by LuaJIT with the JIT off.
//!
//! Dumps: one `pob_oracle probe` run per build (a LuaJIT PoB in its own process, under the
//! machine-wide oracle lock) of
//!
//! ```text
//! --stages crates/pob-oracle/corpus/DF-B/stages.toml \
//!     --select 'defence:659,defence:665,defence:666,defence:674,defence:675,defence:676,defence:1211'
//! ```
//!
//! from the shared probe cache (`testkit::probe_dumps`, CK2: keyed by the patched source, the
//! build's bytes, the selection and the probe code; probed here when absent). This read the
//! pre-SelectionCache `.parity/probe-cache/<sha>/<build>/` of whichever patched-source sha held
//! `defence:665` dumps most recently, i.e. dumps of any harness or PoB tree.
//!
//! Memory: each worker of the bounded pool (`$POB_TEST_PAR`, default 3; TP1) runs one build at a
//! time and each call's dumps are parsed just before it runs and dropped after it (only the
//! build's few `defence:674..676` minion dumps and its model stay alive for the build). An earlier
//! version kept every parsed dump of all 59 builds and reached 6 GB RSS; one worker peaks at
//! ~1.4 GB over the full corpus (printed at the end).
//!
//! `DF_B_ONLY=<stem>[,..]` restricts the builds; `DF_B_VERBOSE=1` prints every mismatch and a
//! per-build line with the peak RSS so far.
#![allow(non_snake_case)]
#![allow(clippy::type_complexity)]

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::rc::Rc;

use pob_calc::env::EnvRef;
use pob_calc::lua_table::Val;
use pob_calc::modules::calc_defence::primary::replay as hooks;
use pob_oracle::replay::{Call, Replay};
use pob_oracle::schema::StateDump;
use pob_oracle::testkit;

/// Hit ids of the boundary and callee probes (corpus/DF-B/stages.toml).
const P_IN: &str = "defence:659";
const P_OUT: &str = "defence:1211";
const P_RES_IN: &str = "defence:665";
const P_RES_OUT: &str = "defence:666";
const P_LM_IN: &str = "defence:674";
const P_LM_OUT: &str = "defence:675";
const P_LMR_OUT: &str = "defence:676";

/// One recorded `calcs.defence` call (loaded just before it runs and dropped right after:
/// the parsed dumps of a build's ~20 calls do not fit in memory together).
struct Case {
    stem: String,
    hit: String,
    din: StateDump,
    dout: StateDump,
    d665: StateDump,
    d666: StateDump,
    /// The Formless Inferno dumps (minion actor only).
    minion: Option<(StateDump, StateDump, StateDump)>,
}

/// The probe selection (module docs).
const SELECT: &[&str] = &[
    "defence:659",
    "defence:665",
    "defence:666",
    "defence:674",
    "defence:675",
    "defence:676",
    "defence:1211",
];

/// This unit's dumps of `xml`: one probe run (module docs). Hold the result while reading them.
fn build_dumps(xml: &Path) -> Result<testkit::BuildDumps, String> {
    testkit::probe_dumps(
        Path::new(env!("CARGO_BIN_EXE_pob_oracle")),
        xml,
        Some(&Path::new(env!("CARGO_MANIFEST_DIR")).join("corpus/DF-B/stages.toml")),
        &testkit::selection(SELECT),
    )
}

/// The dump files of `dir` by hit id (`defence:659#3` → path); nothing is parsed.
fn dump_files(dir: &Path) -> BTreeMap<String, PathBuf> {
    let mut out = BTreeMap::new();
    if let Ok(rd) = std::fs::read_dir(dir) {
        for e in rd.flatten() {
            let name = e.file_name().to_string_lossy().into_owned();
            if !name.starts_with("defence:") {
                continue;
            }
            let hit = name.split(".json").next().unwrap_or(&name).to_owned();
            out.insert(hit, e.path());
        }
    }
    out
}

/// The `n` of every `<probe>#n` hit in `files`, ascending.
fn hits_of(files: &BTreeMap<String, PathBuf>, probe: &str) -> Vec<u64> {
    let pre = format!("{probe}#");
    let mut v: Vec<u64> = files
        .keys()
        .filter_map(|h| h.strip_prefix(&pre).and_then(|n| n.parse().ok()))
        .collect();
    v.sort_unstable();
    v
}

/// Load `<probe>#n`, or `None` when the file is absent or unreadable.
fn load(files: &BTreeMap<String, PathBuf>, probe: &str, n: u64) -> Option<StateDump> {
    StateDump::load(files.get(&format!("{probe}#{n}"))?).ok()
}

/// The Formless Inferno dumps of one build (`defence:674/675/676`, minion calls only; few).
fn minion_dumps(files: &BTreeMap<String, PathBuf>) -> Vec<StateDump> {
    [P_LM_IN, P_LM_OUT, P_LMR_OUT]
        .iter()
        .flat_map(|p| {
            hits_of(files, p)
                .into_iter()
                .filter_map(move |n| load(files, p, n))
        })
        .collect()
}

/// The recorded call `defence:659#n` of a build, or why it cannot be assembled.
fn case_of(
    stem: &str,
    files: &BTreeMap<String, PathBuf>,
    minion: &[StateDump],
    n: u64,
) -> Result<Case, String> {
    let get = |p: &str| {
        load(files, p, n).ok_or_else(|| format!("{stem} {P_IN}#{n}: {p}#{n} missing or unreadable"))
    };
    let din = get(P_IN)?;
    let (dout, d665, d666) = (get(P_OUT)?, get(P_RES_IN)?, get(P_RES_OUT)?);
    let same_call = |probe: &str| {
        minion
            .iter()
            .find(|d| d.probe == probe && d.stack == din.stack && d.env_id == din.env_id)
            .cloned()
    };
    let minion = match (
        same_call(P_LM_IN),
        same_call(P_LM_OUT),
        same_call(P_LMR_OUT),
    ) {
        (Some(a), Some(b), Some(c)) => Some((a, b, c)),
        _ => None,
    };
    Ok(Case {
        stem: stem.to_owned(),
        hit: din.hit.clone(),
        din,
        dout,
        d665,
        d666,
        minion,
    })
}

/// Peak resident set size of this process (`VmHWM`, kB), when the kernel reports it.
fn peak_rss_kb() -> Option<u64> {
    let s = std::fs::read_to_string("/proc/self/status").ok()?;
    let l = s.lines().find(|l| l.starts_with("VmHWM:"))?;
    l.split_whitespace().nth(1)?.parse().ok()
}

/// `calcs.hitChance` from PoB's own source, run by LuaJIT (JIT off).
fn lua_hit_chance(src: &Path) -> Rc<dyn Fn(f64, f64) -> f64> {
    let lines = |file: &str, a: usize, b: usize| -> String {
        let text =
            std::fs::read_to_string(src.join(file)).unwrap_or_else(|e| panic!("{file}: {e}"));
        text.lines()
            .skip(a - 1)
            .take(b - a + 1)
            .collect::<Vec<_>>()
            .join("\n")
    };
    let round = lines("Modules/Common.lua", 709, 715);
    let hit = lines("Modules/CalcDefence.lua", 58, 64);
    assert!(
        round.starts_with("function round(val, dec)"),
        "Common.lua:709 moved: {round}"
    );
    assert!(
        hit.starts_with("function calcs.hitChance(evasion, accuracy)"),
        "CalcDefence.lua:58 moved: {hit}"
    );
    let lua = mlua::Lua::new();
    let chunk = format!(
        "if jit then jit.off() jit.flush() end\nlocal m_floor, m_min, m_max = math.floor, math.min, math.max\n{round}\nlocal calcs = {{}}\n{hit}\nreturn calcs.hitChance"
    );
    let f: mlua::Function = lua
        .load(&chunk)
        .set_name("hitChance")
        .eval()
        .unwrap_or_else(|e| panic!("lua: {e}"));
    let lua = Rc::new(lua);
    Rc::new(move |e: f64, a: f64| {
        let _keep = &lua;
        f.call::<f64>((e, a))
            .unwrap_or_else(|err| panic!("calcs.hitChance({e}, {a}): {err}"))
    })
}

/// Apply the recorded `din → dout` delta onto the live env; panics (as a Lua error would
/// abort the segment) when it cannot be placed.
fn apply_delta(env: &EnvRef, what: &str, din: &StateDump, dout: &StateDump) {
    let mut calls = BTreeMap::new();
    calls.insert(
        what.to_owned(),
        Call {
            din: din.clone(),
            dout: dout.clone(),
        },
    );
    let r = Replay { calls };
    match r.apply_report(what, env) {
        Ok(a) if a.notes.is_empty() => {}
        Ok(a) => panic!("replay {what}: {}", a.notes.join("; ")),
        Err(e) => panic!("replay {what}: {e}"),
    }
}

/// L4 for one case: the mismatch lines (empty = pass).
fn run_case(
    c: &Case,
    model: &pob_calc::model::BuildRef,
    hit_chance: &Rc<dyn Fn(f64, f64) -> f64>,
) -> Vec<String> {
    let inj = match pob_oracle::inject::inject(&c.din, model) {
        Ok(i) => i,
        Err(e) => return vec![format!("inject {}: {e}", c.hit)],
    };
    let Some(env) = inj.env.clone() else {
        return vec![format!("inject {}: no env", c.hit)];
    };
    let locals: Vec<(String, Val)> = inj.locals.into_iter().collect();

    let mut h = hooks::Hooks::default();
    {
        let (env, din, d665) = (env.clone(), c.din.clone(), c.d665.clone());
        h.actionSpeedMod = Some(Rc::new(move |actor| {
            apply_delta(&env, "actionSpeedMod", &din, &d665);
            actor.output.get().num(pob_lua::n!("ActionSpeedMod"))
        }));
    }
    {
        let (env, d665, d666) = (env.clone(), c.d665.clone(), c.d666.clone());
        h.resistances = Some(Rc::new(move |_| {
            apply_delta(&env, "resistances", &d665, &d666);
        }));
    }
    if let Some((a, b, cc)) = c.minion.clone() {
        let (env1, a1, b1) = (env.clone(), a, b.clone());
        h.doActorLifeMana = Some(Rc::new(move |_| {
            apply_delta(&env1, "doActorLifeMana", &a1, &b1);
        }));
        let env2 = env.clone();
        h.doActorLifeManaReservation = Some(Rc::new(move |_, _| {
            apply_delta(&env2, "doActorLifeManaReservation", &b, &cc);
        }));
    } else {
        h.doActorLifeMana = Some(Rc::new(|_| {
            panic!("doActorLifeMana called but no defence:674 dump of this call")
        }));
        h.doActorLifeManaReservation = Some(Rc::new(|_, _| {
            panic!("doActorLifeManaReservation called but no defence:675 dump of this call")
        }));
    }
    h.hitChance = Some(hit_chance.clone());
    hooks::set(h);
    let run = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        pob_calc::probe::run_segment(&c.din.probe, &locals)
    }));
    hooks::clear();
    let after = match run {
        Ok(Ok((_flow, after))) => after,
        Ok(Err(e)) => return vec![format!("run {}: {e}", c.hit)],
        Err(p) => {
            let msg = p
                .downcast_ref::<pob_lua::LuaError>()
                .map(|e| e.0.clone())
                .or_else(|| p.downcast_ref::<String>().cloned())
                .or_else(|| p.downcast_ref::<&str>().map(|s| (*s).to_owned()))
                .unwrap_or_else(|| "panic".to_owned());
            return vec![format!("run {}: {msg}", c.hit)];
        }
    };
    testkit::diff_against(&c.dout, Some(&env), &after, &c.stem)
        .into_iter()
        .map(|m| format!("{} {:?}: pob={:?} ours={:?}", m.path, m.kind, m.pob, m.ours))
        .collect()
}

/// L4 over every recorded `calcs.defence` call (gate: 100%, calc doc §9.4).
#[test]
fn df_b_l4() {
    let Some(_dump) = pob_testpaths::require(pob_testpaths::dump_dir(), "data/dump") else {
        return;
    };
    let Some(parity) = pob_testpaths::require(pob_testpaths::parity_dir(), ".parity") else {
        return;
    };
    let Some(src) = pob_testpaths::require(
        pob_testpaths::pob_src(),
        "PoB checkout (.PathOfBuilding/src)",
    ) else {
        return;
    };
    assert!(testkit::init_default_data(), "data/dump did not load");
    let only: Option<Vec<String>> = std::env::var("DF_B_ONLY")
        .ok()
        .map(|s| s.split(',').map(str::to_owned).collect());
    let verbose = std::env::var_os("DF_B_VERBOSE").is_some();
    let mut builds: Vec<PathBuf> = std::fs::read_dir(parity.join("builds"))
        .expect(".parity/builds")
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.extension().is_some_and(|x| x == "xml"))
        .collect();
    builds.sort();
    // the unit corpus (corpus/DF-B/derive.py)
    let unit_dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("corpus/DF-B");
    let mut unit: Vec<PathBuf> = std::fs::read_dir(&unit_dir)
        .expect("corpus/DF-B")
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.extension().is_some_and(|x| x == "xml"))
        .collect();
    unit.sort();
    builds.extend(unit);
    // TP1: builds run on the bounded pool (`testkit::par_for_each_init`, `$POB_TEST_PAR`), each
    // worker with its own LuaJIT `calcs.hitChance`; the per-build results are merged and printed
    // on this thread in corpus order, so the report is the serial one.
    let builds: Vec<PathBuf> = builds
        .into_iter()
        .filter(|xml| {
            let stem = xml.file_stem().unwrap().to_string_lossy().into_owned();
            only.as_ref().map_or(true, |o| o.contains(&stem))
        })
        .collect();
    let nbuilds = builds.len();

    // One call at a time per worker: only the current call's dumps, the build's few minion
    // dumps and its parsed model are alive at once per worker (memory budget, AMENDMENTS).
    let hook = std::panic::take_hook();
    std::panic::set_hook(Box::new(|_| {}));
    let mut missing = Vec::new();
    let mut failures: Vec<(String, String, Vec<String>)> = Vec::new();
    let (mut total, mut pass, mut minion_cases) = (0usize, 0usize, 0usize);
    let (mut unit_cases, mut unit_pass) = (0usize, 0usize);
    #[derive(Default)]
    struct BuildOut {
        missing: Vec<String>,
        failures: Vec<(String, String, Vec<String>)>,
        total: usize,
        pass: usize,
        minion_cases: usize,
        unit_cases: usize,
        unit_pass: usize,
    }
    testkit::par_for_each_init(
        &builds,
        || lua_hit_chance(&src),
        |hit_chance, _, xml| {
            let mut o = BuildOut::default();
            let stem = xml.file_stem().unwrap().to_string_lossy().into_owned();
            let held = match build_dumps(xml) {
                Ok(d) => d,
                Err(e) => {
                    o.missing.push(format!("{stem}: no DF-B dumps (probe failed: {e})"));
                    return o;
                }
            };
            let is_unit = xml.starts_with(&unit_dir);
            let files = dump_files(held.path());
            let minion = minion_dumps(&files);
            let model = testkit::load_build(xml).unwrap_or_else(testkit::empty_build);
            for n in hits_of(&files, P_IN) {
                let c = match case_of(&stem, &files, &minion, n) {
                    Ok(c) => c,
                    Err(e) => {
                        o.missing.push(e);
                        continue;
                    }
                };
                o.total += 1;
                o.minion_cases += usize::from(c.minion.is_some());
                o.unit_cases += usize::from(is_unit);
                let m = run_case(&c, &model, hit_chance);
                if m.is_empty() {
                    o.pass += 1;
                    o.unit_pass += usize::from(is_unit);
                } else {
                    let keep = if verbose { m.len() } else { 8 };
                    let n = m.len();
                    let mut m = m;
                    m.truncate(keep);
                    m.push(format!("({n} mismatches)"));
                    o.failures.push((c.stem.clone(), c.hit.clone(), m));
                }
            }
            o
        },
        |_, xml, o| {
            if verbose {
                let stem = xml.file_stem().unwrap().to_string_lossy();
                eprintln!(
                    "{stem}: {}/{} (peak RSS {} MB)",
                    o.pass,
                    o.total,
                    peak_rss_kb().unwrap_or(0) / 1024
                );
            }
            missing.extend(o.missing);
            failures.extend(o.failures);
            total += o.total;
            pass += o.pass;
            minion_cases += o.minion_cases;
            unit_cases += o.unit_cases;
            unit_pass += o.unit_pass;
        },
    );
    std::panic::set_hook(hook);
    for (stem, hit, m) in &failures {
        eprintln!("FAIL {stem} {hit}");
        for l in m {
            eprintln!("    {l}");
        }
    }
    for l in &missing {
        eprintln!("MISSING {l}");
    }
    eprintln!(
        "DF-B L4: {pass}/{total} calcs.defence calls exact ({minion_cases} minion calls; unit corpus {unit_pass}/{unit_cases}) over {nbuilds} builds; {} builds/hits without dumps; peak RSS {} MB",
        missing.len(),
        peak_rss_kb().unwrap_or(0) / 1024
    );
    assert!(
        total > 0,
        "no DF-B dumps found (fill the probe cache, see the module doc)"
    );
    assert!(
        failures.is_empty(),
        "{} of {total} DF-B L4 cases differ",
        failures.len()
    );
}
