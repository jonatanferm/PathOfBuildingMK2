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
//! Fill the cache (one LuaJIT PoB per build, `scripts/port/oracle.sh` holds the lock):
//!
//! ```text
//! for b in .parity/builds/*.xml crates/pob-oracle/corpus/DF-B/*.xml; do
//!   scripts/port/oracle.sh run --build "$b" --stages crates/pob-oracle/corpus/DF-B/stages.toml \
//!     --stage 'defence:659,defence:665,defence:666,defence:674,defence:675,defence:676,defence:1211'
//! done
//! ```
//!
//! `DF_B_ONLY=<stem>[,..]` restricts the builds; `DF_B_VERBOSE=1` prints every mismatch.
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

/// One recorded `calcs.defence` call.
struct Case {
    stem: String,
    xml: PathBuf,
    hit: String,
    din: StateDump,
    dout: StateDump,
    d665: StateDump,
    d666: StateDump,
    /// The Formless Inferno dumps (minion actor only).
    minion: Option<(StateDump, StateDump, StateDump)>,
}

/// The probe-cache directory of `xml` that holds this unit's dumps (`defence:665#*`), newest
/// first when several patched shas have them.
fn cache_dir(parity: &Path, xml: &Path) -> Option<PathBuf> {
    let key = pob_oracle::runner::build_key(xml).ok()?;
    let mut best: Option<(std::time::SystemTime, PathBuf)> = None;
    for sha in std::fs::read_dir(parity.join("probe-cache")).ok()?.flatten() {
        let d = sha.path().join(&key);
        let Ok(rd) = std::fs::read_dir(&d) else {
            continue;
        };
        let has = rd
            .flatten()
            .any(|e| e.file_name().to_string_lossy().starts_with("defence:665#"));
        if !has {
            continue;
        }
        let t = std::fs::metadata(&d)
            .and_then(|m| m.modified())
            .unwrap_or(std::time::UNIX_EPOCH);
        if best.as_ref().is_none_or(|(b, _)| t > *b) {
            best = Some((t, d));
        }
    }
    best.map(|(_, d)| d)
}

/// Every dump of `dir`, by hit id.
fn load_dumps(dir: &Path) -> BTreeMap<String, StateDump> {
    let mut out = BTreeMap::new();
    if let Ok(rd) = std::fs::read_dir(dir) {
        for e in rd.flatten() {
            let name = e.file_name().to_string_lossy().into_owned();
            if !name.starts_with("defence:") {
                continue;
            }
            if let Ok(d) = StateDump::load(&e.path()) {
                out.insert(d.hit.clone(), d);
            }
        }
    }
    out
}

fn cases_of(stem: &str, xml: &Path, dumps: &BTreeMap<String, StateDump>, missing: &mut Vec<String>) -> Vec<Case> {
    let mut out = Vec::new();
    let same_call = |probe: &str, of: &StateDump| {
        dumps
            .values()
            .find(|d| d.probe == probe && d.stack == of.stack && d.env_id == of.env_id)
            .cloned()
    };
    for din in dumps.values().filter(|d| d.probe == P_IN) {
        let n = din.n;
        let get = |p: &str| dumps.get(&format!("{p}#{n}")).cloned();
        let (Some(dout), Some(d665), Some(d666)) = (get(P_OUT), get(P_RES_IN), get(P_RES_OUT)) else {
            missing.push(format!("{stem} {}: a paired {P_OUT}/{P_RES_IN}/{P_RES_OUT} dump is missing", din.hit));
            continue;
        };
        let minion = match (same_call(P_LM_IN, din), same_call(P_LM_OUT, din), same_call(P_LMR_OUT, din)) {
            (Some(a), Some(b), Some(c)) => Some((a, b, c)),
            _ => None,
        };
        out.push(Case {
            stem: stem.to_owned(),
            xml: xml.to_path_buf(),
            hit: din.hit.clone(),
            din: din.clone(),
            dout,
            d665,
            d666,
            minion,
        });
    }
    out
}

/// `calcs.hitChance` from PoB's own source, run by LuaJIT (JIT off).
fn lua_hit_chance(src: &Path) -> Rc<dyn Fn(f64, f64) -> f64> {
    let lines = |file: &str, a: usize, b: usize| -> String {
        let text = std::fs::read_to_string(src.join(file)).unwrap_or_else(|e| panic!("{file}: {e}"));
        text.lines().skip(a - 1).take(b - a + 1).collect::<Vec<_>>().join("\n")
    };
    let round = lines("Modules/Common.lua", 709, 715);
    let hit = lines("Modules/CalcDefence.lua", 58, 64);
    assert!(round.starts_with("function round(val, dec)"), "Common.lua:709 moved: {round}");
    assert!(hit.starts_with("function calcs.hitChance(evasion, accuracy)"), "CalcDefence.lua:58 moved: {hit}");
    let lua = mlua::Lua::new();
    let chunk = format!(
        "if jit then jit.off() jit.flush() end\nlocal m_floor, m_min, m_max = math.floor, math.min, math.max\n{round}\nlocal calcs = {{}}\n{hit}\nreturn calcs.hitChance"
    );
    let f: mlua::Function = lua.load(&chunk).set_name("hitChance").eval().unwrap_or_else(|e| panic!("lua: {e}"));
    let lua = Rc::new(lua);
    Rc::new(move |e: f64, a: f64| {
        let _keep = &lua;
        f.call::<f64>((e, a)).unwrap_or_else(|err| panic!("calcs.hitChance({e}, {a}): {err}"))
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
fn run_case(c: &Case, hit_chance: &Rc<dyn Fn(f64, f64) -> f64>) -> Vec<String> {
    let model = testkit::load_build(&c.xml).unwrap_or_else(testkit::empty_build);
    let inj = match pob_oracle::inject::inject(&c.din, &model) {
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
        h.resistances = Some(Rc::new(move |_| apply_delta(&env, "resistances", &d665, &d666)));
    }
    if let Some((a, b, cc)) = c.minion.clone() {
        let (env1, a1, b1) = (env.clone(), a, b.clone());
        h.doActorLifeMana = Some(Rc::new(move |_| apply_delta(&env1, "doActorLifeMana", &a1, &b1)));
        let env2 = env.clone();
        h.doActorLifeManaReservation = Some(Rc::new(move |_, _| {
            apply_delta(&env2, "doActorLifeManaReservation", &b, &cc)
        }));
    } else {
        h.doActorLifeMana = Some(Rc::new(|_| panic!("doActorLifeMana called but no defence:674 dump of this call")));
        h.doActorLifeManaReservation =
            Some(Rc::new(|_, _| panic!("doActorLifeManaReservation called but no defence:675 dump of this call")));
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
    let Some(src) = pob_testpaths::require(pob_testpaths::pob_src(), "PoB checkout (.PathOfBuilding/src)") else {
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
    let hit_chance = lua_hit_chance(&src);

    let mut missing = Vec::new();
    let mut cases = Vec::new();
    for xml in &builds {
        let stem = xml.file_stem().unwrap().to_string_lossy().into_owned();
        if only.as_ref().is_some_and(|o| !o.contains(&stem)) {
            continue;
        }
        let Some(dir) = cache_dir(&parity, xml) else {
            missing.push(format!("{stem}: no DF-B dumps (see the fill command in tests/DF-B.rs)"));
            continue;
        };
        cases.extend(cases_of(&stem, xml, &load_dumps(&dir), &mut missing));
    }
    let hook = std::panic::take_hook();
    std::panic::set_hook(Box::new(|_| {}));
    let mut failed = Vec::new();
    let (mut pass, mut minion_cases) = (0usize, 0usize);
    let (mut unit_cases, mut unit_pass) = (0usize, 0usize);
    for c in &cases {
        minion_cases += usize::from(c.minion.is_some());
        let is_unit = c.xml.starts_with(&unit_dir);
        unit_cases += usize::from(is_unit);
        let m = run_case(c, &hit_chance);
        if m.is_empty() {
            pass += 1;
            unit_pass += usize::from(is_unit);
        } else {
            failed.push((c, m));
        }
    }
    std::panic::set_hook(hook);
    for (c, m) in &failed {
        eprintln!("FAIL {} {} ({} mismatches)", c.stem, c.hit, m.len());
        for l in m.iter().take(if verbose { usize::MAX } else { 8 }) {
            eprintln!("    {l}");
        }
    }
    for l in &missing {
        eprintln!("MISSING {l}");
    }
    eprintln!(
        "DF-B L4: {pass}/{} calcs.defence calls exact ({minion_cases} minion calls; unit corpus {unit_pass}/{unit_cases}) over {} builds; {} builds/hits without dumps",
        cases.len(),
        builds.len(),
        missing.len()
    );
    assert!(!cases.is_empty(), "no DF-B dumps found (fill the probe cache, see the module doc)");
    assert!(failed.is_empty(), "{} of {} DF-B L4 cases differ", failed.len(), cases.len());
}
