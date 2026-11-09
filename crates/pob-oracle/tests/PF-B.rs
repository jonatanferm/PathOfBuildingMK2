//! Unit PF-B acceptance (calc doc §11.1 row PF-B: "function probes: doActorMisc,
//! doActorCharges, actionSpeedMod, initMinionModDB, addMinionModifiers").
//!
//! For every corpus build, PoB is patched with `corpus/PF-B/stages.toml` (function probes on
//! the five helpers and on the two unported callees) and probed twice into
//! `target/tmp/pfb-probe/<stem>/`:
//!
//! 1. `<fn>:*@main` for every probe: the first call of each in the build's main env;
//! 2. a window of hit numbers after each first call (`<fn>:in#n … #n+W` and `:out`), which
//!    picks up the later calls of the same perform (`doActorCharges` 2191/3490/3493/3624,
//!    `doActorMisc` for player/minion/enemy, `actionSpeedMod` from defence and offence, …)
//!    and the callee calls nested in them (callee windows are three times as wide).
//!
//! Each recorded `<fn>:in#n`/`<fn>:out#n` pair is one case: the `:in` dump is injected
//! (`pob_oracle::inject`), the Rust function runs on the injected locals and the state is
//! exported and diffed against `:out` (`testkit::diff_against`, §9.2 tolerance; `ret` is the
//! packed return value). Callees that are still stubs are replayed
//! (`pob_oracle::replay::Replay`, their recorded in→out delta; hit found by the dump's
//! stack): `applyEnemyModifiers` (PF-A) after `doActorMisc` (its last statement, 976), and
//! `calcs.initModDB` (CS0) before `initMinionModDB` (its effects commute with 1157-1158,
//! which only set `minion.output` and `multipliers.Level`). A callee that is already ported
//! runs for real and is not replayed.
//!
//! Test-side adapter (CCR-0034 §1): `minion.lifeTable` / `minion.uses` are `$data` markers in
//! the dump, which inject keeps as opaque `DataRef`s; the case converts them to the Lua tables
//! they stand for before the run and puts the markers back before the export.
//!
//! Cases of `initMinionModDB` whose minion iterates `env.build.itemsTab.slots` (`minion.uses`,
//! 1222-1240) cannot pass until BM-A's build loader fills the model; they are listed as
//! "needs BM-A" and not gated.
//!
//! A case whose stub callee's nested call fell outside the probed hit window (no recorded
//! call to replay) is not evaluated; it is listed, not counted.
//!
//! Gate: every other case is value-clean (no mismatch other than LUA-ORDER) and no case raises.
//! Knobs: `PFB_BUILDS=stem1,stem2`, `PFB_LIMIT=n`,
//! `PFB_VERBOSE=1`, `PFB_WINDOW=n` (default 6). Inputs: `.parity/builds`, the PoB checkout,
//! `data/dump`, LuaJIT (the machine-wide oracle lock). A missing input skips; with
//! `POB_ORACLE_REQUIRE=1` (or `CI`) it fails instead.

use std::collections::BTreeMap;
use std::panic::{catch_unwind, AssertUnwindSafe};
use std::path::{Path, PathBuf};

use pob_calc::active_skill::ActiveSkill;
use pob_calc::actor::Actor;
use pob_calc::classes::ModStore;
use pob_calc::lua_table::{Table, Val};
use pob_calc::modules::calc_perform::actor_b::probe_fns;
use pob_calc::probe::DataRef;
use pob_oracle::diff::{Mismatch, MismatchKind};
use pob_oracle::oracle::find_parity_dir;
use pob_oracle::replay::Replay;
use pob_oracle::schema::StateDump;
use pob_oracle::testkit;

const FNS: [&str; 5] = [
    "doActorMisc",
    "doActorCharges",
    "actionSpeedMod",
    "initMinionModDB",
    "addMinionModifiers",
];
const CALLEES: [&str; 2] = ["applyEnemyModifiers", "initModDB"];
const STUB_APPLY_ENEMY: &str = "Modules/CalcPerform.lua:619-633";
const STUB_INIT_MODDB: &str = "Modules/CalcSetup.lua:19-120";

fn skip(why: &str) {
    let required = std::env::var_os("POB_ORACLE_REQUIRE").is_some_and(|v| v == "1")
        || std::env::var_os("CI").is_some_and(|v| !v.is_empty() && v != "false" && v != "0");
    assert!(!required, "required input missing: {why}");
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
    if let Ok(list) = std::env::var("PFB_BUILDS") {
        let want: Vec<&str> = list.split(',').map(str::trim).collect();
        v.retain(|p| want.contains(&stem(p)));
    }
    if let Some(n) = std::env::var("PFB_LIMIT")
        .ok()
        .and_then(|s| s.parse::<usize>().ok())
    {
        v.truncate(n);
    }
    v
}

fn stages_file() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("corpus/PF-B/stages.toml")
}

// ---- probing --------------------------------------------------------------------------------

fn run_probe(xml: &Path, patched: &Path, out: &Path, select: &[String]) -> bool {
    let _ = std::fs::remove_dir_all(out);
    if std::fs::create_dir_all(out).is_err() {
        return false;
    }
    let Ok(log) = std::fs::File::create(out.with_extension("log")) else {
        return false;
    };
    std::process::Command::new(env!("CARGO_BIN_EXE_pob_oracle"))
        .arg("probe")
        .arg(xml)
        .arg("--out-dir")
        .arg(out)
        .arg("--patched")
        .arg(patched)
        .arg("--select")
        .arg(select.join(","))
        .args(["--no-provenance", "--max-dumps", "400"])
        .stdout(std::process::Stdio::null())
        .stderr(log)
        .status()
        .is_ok_and(|s| s.success())
}

/// `(probe, n)` of a dump file name `<probe>#<n>.json`.
fn hit_of(name: &str) -> Option<(String, u64)> {
    let base = name.strip_suffix(".json")?;
    let (p, n) = base.rsplit_once('#')?;
    Some((p.to_owned(), n.parse().ok()?))
}

fn dump_files(dir: &Path) -> BTreeMap<String, PathBuf> {
    let mut out = BTreeMap::new();
    if let Ok(rd) = std::fs::read_dir(dir.join("state")) {
        for e in rd.filter_map(Result::ok) {
            let name = e.file_name().to_string_lossy().into_owned();
            if let Some((p, n)) = hit_of(&name) {
                out.insert(format!("{p}#{n}"), e.path());
            }
        }
    }
    out
}

/// The build's dumps (hit id → file), probing when the cache is absent.
fn build_dumps(xml: &Path, patched: &Path) -> Option<BTreeMap<String, PathBuf>> {
    let dir = PathBuf::from(env!("CARGO_TARGET_TMPDIR"))
        .join("pfb-probe")
        .join(stem(xml));
    let done = dir.join("done");
    if !done.is_file() {
        let p1 = dir.join("p1");
        let mut sel: Vec<String> = FNS
            .iter()
            .chain(CALLEES.iter())
            .map(|f| format!("{f}:*@main"))
            .collect();
        if !run_probe(xml, patched, &p1, &sel) {
            return None;
        }
        let window: u64 = std::env::var("PFB_WINDOW")
            .ok()
            .and_then(|s| s.parse().ok())
            .unwrap_or(6);
        sel.clear();
        for (hit, _) in dump_files(&p1) {
            let Some((probe, n)) = hit_of(&format!("{hit}.json")) else {
                continue;
            };
            let Some(f) = probe.strip_suffix(":in") else {
                continue;
            };
            // a callee runs up to ~3 times per caller call in the window
            let w = if CALLEES.contains(&f) {
                3 * (window + 1)
            } else {
                window
            };
            for j in n..=n + w {
                sel.push(format!("{f}:in#{j}"));
                sel.push(format!("{f}:out#{j}"));
            }
        }
        if !sel.is_empty() && !run_probe(xml, patched, &dir.join("p2"), &sel) {
            return None;
        }
        std::fs::write(&done, "").ok()?;
    }
    let mut all = dump_files(&dir.join("p1"));
    all.extend(dump_files(&dir.join("p2")));
    Some(all)
}

// ---- one case -------------------------------------------------------------------------------

#[derive(Default)]
struct Tally {
    cases: BTreeMap<&'static str, (u64, u64)>,
    failures: Vec<String>,
    lua_order: u64,
    replayed: BTreeMap<&'static str, u64>,
    missing_callee: Vec<String>,
    unpaired: u64,
    builds: u64,
    probe_failed: Vec<String>,
    /// Cases that need the build model's `itemsTab.slots` (BM-A's loader is a stub, so the
    /// injected build has none): reported, not gated.
    needs_bm_a: Vec<String>,
    order_cases: Vec<String>,
}

fn stub_hits(site: &str) -> u64 {
    pob_calc::stub::stub_report()
        .into_iter()
        .find(|(s, _)| *s == site)
        .map_or(0, |(_, c)| c)
}

/// The recorded call of `callee` directly inside `caller_hit` (`<fn>:in#n`).
fn callee_call<'a>(
    callee: &str,
    caller_hit: &str,
    loaded: &'a BTreeMap<String, StateDump>,
) -> Option<&'a StateDump> {
    loaded.values().find(|d| {
        d.probe == format!("{callee}:in") && d.stack.last().map(String::as_str) == Some(caller_hit)
    })
}

/// `minion.lifeTable` / `minion.uses` `$data` markers → the Lua tables (CCR-0034 §1).
/// Returns the originals to restore.
fn adapt_minion(minion: &Actor, saved: &mut Vec<(Table, &'static str, Val)>) {
    for key in ["lifeTable", "uses"] {
        let v = minion.extra.get(key);
        let Some(d) = v.obj::<DataRef>() else {
            continue;
        };
        let data = pob_gamedata::data();
        let seq = |t: &[f64]| {
            let out = Table::new();
            for x in t {
                out.push(*x);
            }
            Some(out)
        };
        let conv = match d.path.as_str() {
            "data.monsterLifeTable" => seq(&data.monsterLifeTable),
            "data.monsterLifeTable2" => seq(&data.monsterLifeTable2),
            "data.monsterLifeTable3" => seq(&data.monsterLifeTable3),
            "data.monsterAllyLifeTable" => seq(&data.monsterAllyLifeTable),
            p => p
                .strip_prefix("data.skills.")
                .and_then(|r| r.strip_suffix(".minionUses"))
                .and_then(|id| data.skills.get(&pob_lua::Name::from(id)))
                .and_then(|g| g.minionUses.clone())
                .map(|set| {
                    let t = Table::new();
                    for s in set.iter() {
                        t.set(*s, true);
                    }
                    t
                }),
        };
        if let Some(t) = conv {
            saved.push((minion.extra.clone(), key, v.clone()));
            minion.extra.set(key, t);
        }
    }
}

fn minions_of(
    env: &pob_calc::env::EnvRef,
    locals: &BTreeMap<String, Val>,
) -> Vec<std::rc::Rc<Actor>> {
    let mut out = Vec::new();
    if let Some(m) = env.minion.get() {
        out.push(m);
    }
    for s in env.player.activeSkillList.get().snapshot() {
        if let Some(m) = s.minion.get() {
            out.push(m);
        }
    }
    if let Some(s) = locals.get("activeSkill").and_then(Val::obj::<ActiveSkill>) {
        if let Some(m) = s.minion.get() {
            out.push(m);
        }
    }
    if let Some(m) = locals.get("minion").and_then(Val::obj::<Actor>) {
        out.push(m);
    }
    out
}

fn run_case(
    t: &mut Tally,
    stem: &str,
    f: &'static str,
    din: &StateDump,
    dout: &StateDump,
    loaded: &BTreeMap<String, StateDump>,
) {
    t.cases.entry(f).or_default().0 += 1;
    let label = format!("{stem} {}", din.hit);
    let model = testkit::empty_build();
    let inj = match pob_oracle::inject::inject(din, &model) {
        Ok(i) => i,
        Err(e) => {
            t.failures.push(format!("{label}: inject: {e}"));
            return;
        }
    };
    let locals: BTreeMap<String, Val> = inj.locals.into_iter().collect();
    let get = |k: &str| locals.get(k).cloned().unwrap_or(Val::Nil);
    let Some(env) = get("env").obj::<pob_calc::env::Env>().or(inj.env.clone()) else {
        t.failures.push(format!("{label}: no env"));
        return;
    };

    // calcs.initModDB before initMinionModDB, when it is still a stub
    if f == "initMinionModDB" {
        let before = stub_hits(STUB_INIT_MODDB);
        pob_calc::calcs::initModDB(&env, &std::rc::Rc::new(ModStore::default()));
        if stub_hits(STUB_INIT_MODDB) > before {
            match callee_call("initModDB", &din.hit, loaded) {
                Some(cin) => {
                    let r = Replay::load("initModDB", loaded);
                    match r.apply_report(&cin.hit, &env) {
                        Ok(_) => *t.replayed.entry("initModDB").or_default() += 1,
                        Err(e) => t.failures.push(format!("{label}: replay initModDB: {e}")),
                    }
                }
                None => {
                    t.missing_callee.push(format!("{label}: initModDB"));
                    t.cases.entry(f).or_default().0 -= 1;
                    return;
                }
            }
        }
    }

    let mut saved = Vec::new();
    for m in minions_of(&env, &locals) {
        adapt_minion(&m, &mut saved);
    }

    let stub_before = stub_hits(STUB_APPLY_ENEMY);
    let run = catch_unwind(AssertUnwindSafe(|| -> Result<Val, String> {
        let actor = || {
            get("actor")
                .obj::<Actor>()
                .ok_or("local actor is not an actor")
        };
        Ok(match f {
            "doActorMisc" => {
                probe_fns::doActorMisc(&env, &actor()?);
                Val::Nil
            }
            "doActorCharges" => {
                probe_fns::doActorCharges(&env, &actor()?);
                Val::Nil
            }
            "actionSpeedMod" => Val::Num(pob_calc::calcs::actionSpeedMod(&actor()?)),
            "initMinionModDB" => {
                let skill = get("activeSkill")
                    .obj::<ActiveSkill>()
                    .ok_or("local activeSkill is not an active skill")?;
                let output = get("output").table();
                probe_fns::initMinionModDB(&env, &skill, output.as_ref());
                Val::Nil
            }
            "addMinionModifiers" => {
                let mod_list = get("modList")
                    .obj::<ModStore>()
                    .ok_or("local modList is not a mod store")?;
                let cfg = get("skillCfg").obj::<pob_calc::cfg::ModCfg>();
                let minion = get("minion")
                    .obj::<Actor>()
                    .ok_or("local minion is not an actor")?;
                probe_fns::addMinionModifiers(&mod_list, cfg.as_deref(), &minion);
                Val::Nil
            }
            _ => unreachable!(),
        })
    }));
    for (tbl, key, v) in saved {
        tbl.set(key, v);
    }
    // inject marks a field PoB had as nil (`minion.output` before 1157) absent; the Rust
    // write makes it present again (the probe registry does not track writes)
    if f == "initMinionModDB" {
        if let Some(m) = get("activeSkill")
            .obj::<ActiveSkill>()
            .and_then(|s| s.minion.get())
        {
            pob_calc::probe::unmark_absent(pob_calc::probe::rc_ptr(&m), "output");
        }
    }
    let needs_slots = f == "initMinionModDB"
        && env.build.itemsTab.slots.with(|s| s.is_empty())
        && get("activeSkill")
            .obj::<ActiveSkill>()
            .and_then(|s| s.minion.get())
            .is_some_and(|m| m.extra.get("uses").truthy() || m.extra.get("itemSet").truthy());
    let ret = match run {
        Ok(Ok(v)) => v,
        Ok(Err(e)) => {
            t.failures.push(format!("{label}: {e}"));
            return;
        }
        Err(p) => {
            let msg = p
                .downcast_ref::<pob_lua::LuaError>()
                .map(|e| e.0.clone())
                .or_else(|| p.downcast_ref::<String>().cloned())
                .or_else(|| p.downcast_ref::<&str>().map(|s| (*s).to_owned()))
                .unwrap_or_else(|| "panic".to_owned());
            t.failures.push(format!("{label}: raised: {msg}"));
            return;
        }
    };

    // applyEnemyModifiers after doActorMisc, when it is still a stub
    if f == "doActorMisc" && stub_hits(STUB_APPLY_ENEMY) > stub_before {
        match callee_call("applyEnemyModifiers", &din.hit, loaded) {
            Some(cin) => {
                let r = Replay::load("applyEnemyModifiers", loaded);
                match r.apply_report(&cin.hit, &env) {
                    Ok(_) => *t.replayed.entry("applyEnemyModifiers").or_default() += 1,
                    Err(e) => t
                        .failures
                        .push(format!("{label}: replay applyEnemyModifiers: {e}")),
                }
            }
            None => {
                t.missing_callee
                    .push(format!("{label}: applyEnemyModifiers"));
                t.cases.entry(f).or_default().0 -= 1;
                return;
            }
        }
    }

    // `ret` = the packed results `{ n = k, … }`
    let packed = Table::new();
    if ret.is_nil() {
        packed.set("n", 0.0);
    } else {
        packed.set("n", 1.0);
        packed.set(1i64, ret);
    }
    let mut out_locals: Vec<(String, Val)> = locals.into_iter().collect();
    out_locals.push(("ret".to_owned(), Val::Table(packed)));
    let mm: Vec<Mismatch> = testkit::diff_against(dout, Some(&env), &out_locals, stem);
    let (order, value): (Vec<&Mismatch>, Vec<&Mismatch>) = mm
        .iter()
        .partition(|m| matches!(m.kind, MismatchKind::LuaOrder));
    t.lua_order += order.len() as u64;
    if !order.is_empty() {
        t.order_cases.push(format!(
            "{label}: {}",
            order
                .iter()
                .take(3)
                .map(|m| m.path.as_str())
                .collect::<Vec<_>>()
                .join(", ")
        ));
    }
    if !value.is_empty() && needs_slots {
        t.needs_bm_a.push(format!(
            "{label}: {} mismatches (minion.uses over itemsTab.slots)",
            value.len()
        ));
    } else if value.is_empty() {
        t.cases.entry(f).or_default().1 += 1;
    } else {
        let verbose = std::env::var_os("PFB_VERBOSE").is_some();
        let shown: Vec<String> = value
            .iter()
            .take(if verbose { usize::MAX } else { 4 })
            .map(|m| {
                format!(
                    "    {} {:?}: pob={:?} ours={:?}",
                    m.path, m.kind, m.pob, m.ours
                )
            })
            .collect();
        t.failures.push(format!(
            "{label}: {} mismatches\n{}",
            value.len(),
            shown.join("\n")
        ));
    }
}

fn run_build(t: &mut Tally, xml: &Path, patched: &Path) {
    let stem = stem(xml);
    let Some(files) = build_dumps(xml, patched) else {
        t.probe_failed.push(stem.to_owned());
        return;
    };
    t.builds += 1;
    let mut loaded: BTreeMap<String, StateDump> = BTreeMap::new();
    for (hit, p) in &files {
        if let Ok(d) = StateDump::load(p) {
            loaded.insert(hit.clone(), d);
        }
    }
    for f in FNS {
        let ins: Vec<&StateDump> = loaded
            .values()
            .filter(|d| d.probe == format!("{f}:in"))
            .collect();
        for din in ins {
            let dout = loaded
                .values()
                .find(|o| o.probe == format!("{f}:out") && o.n == din.n && o.env_id == din.env_id);
            match dout {
                Some(dout) => run_case(t, stem, f, din, dout, &loaded),
                None => t.unpaired += 1,
            }
        }
    }
}

#[cfg(feature = "luajit")]
#[test]
fn pfb_function_probes_all57() {
    if !testkit::init_default_data() {
        return skip("no data dump (data/dump)");
    }
    let Some(parity) = find_parity_dir() else {
        return skip("no .parity directory");
    };
    let Some(src) = pob_oracle::oracle::find_pob_src() else {
        return skip("no PoB checkout");
    };
    let root = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join("oracle-src");
    let patched = pob_oracle::patcher::patch(&src, Some(&stages_file()), &root)
        .unwrap_or_else(|e| panic!("patch with {}: {e}", stages_file().display()));
    let builds = corpus(&parity);
    assert!(!builds.is_empty(), "no builds in {}", parity.display());
    let mut t = Tally::default();
    for xml in &builds {
        run_build(&mut t, xml, &patched.src);
    }

    eprintln!("PF-B function probes over {} builds:", t.builds);
    let (mut all, mut clean) = (0, 0);
    for (f, (n, c)) in &t.cases {
        eprintln!("  {f:<20} {c}/{n} clean");
        all += n;
        clean += c;
    }
    eprintln!("  total                {clean}/{all} clean");
    eprintln!(
        "  LUA-ORDER {}; unpaired :in {}; replayed {:?}; probe failures {:?}",
        t.lua_order, t.unpaired, t.replayed, t.probe_failed
    );
    for m in &t.order_cases {
        eprintln!("  LUA-ORDER in {m}");
    }
    for m in &t.needs_bm_a {
        eprintln!("  needs BM-A (not gated): {m}");
    }
    for m in &t.missing_callee {
        eprintln!("  not evaluated (stub callee's call outside the probe window): {m}");
    }
    for f in &t.failures {
        eprintln!("  FAIL {f}");
    }
    for f in FNS {
        assert!(
            t.cases.get(f).is_some_and(|(n, _)| *n > 0)
                || f == "initMinionModDB"
                || f == "addMinionModifiers",
            "no {f} case in the corpus"
        );
    }
    assert!(
        t.probe_failed.is_empty(),
        "probing failed: {:?}",
        t.probe_failed
    );
    let gated = all - t.needs_bm_a.len() as u64;
    assert!(
        t.failures.is_empty() && clean == gated,
        "{} of {gated} gated cases not clean",
        gated - clean
    );
}

/// The ally life pool tables are module-level locals: one identity, PoB's contents.
#[test]
fn pfb_ally_life_pools() {
    let a = probe_fns::allyLifePools();
    let b = probe_fns::allyLifePools();
    assert!(a.ptr_eq(&b), "allyLifePools must return the same tables");
    let spectre = a.table("spectreLifePool").unwrap();
    assert_eq!(spectre.str("life").as_deref(), Some("TotalSpectreLife"));
    assert_eq!(spectre.str("limit").as_deref(), Some("ActiveSpectreLimit"));
    let by_skill = a.table("minionLifePoolBySkill").unwrap();
    assert_eq!(
        by_skill
            .table("Summon Void Spawn")
            .and_then(|p| p.str("redirect"))
            .as_deref(),
        Some("takenFromVoidSpawnBeforeYou")
    );
    let redirects = a.table("allyLifeRedirects").unwrap();
    assert_eq!(redirects.len(), 7);
    assert_eq!(
        redirects.str(1i64).as_deref(),
        Some("takenFromSpectresBeforeYou")
    );
    assert_eq!(
        redirects.str(2i64).as_deref(),
        Some("takenFromMinionBeforeYou")
    );
    let mut rest: Vec<String> = (3..=7i64)
        .map(|i| redirects.str(i).unwrap().to_string())
        .collect();
    rest.sort();
    assert_eq!(
        rest,
        [
            "takenFromRadianceSentinelBeforeYou",
            "takenFromStoneGolemBeforeYou",
            "takenFromTotemsBeforeYou",
            "takenFromVaalRejuvenationTotemsBeforeYou",
            "takenFromVoidSpawnBeforeYou",
        ]
    );
}
