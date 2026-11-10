//! Unit OF-C acceptance (calc doc §11.1 row OF-C, §9.4 L4): segment
//! `CalcOffence.lua` 1692-2173 (`calc_offence::costs_passes`) — costs, Sacrificial
//! Zeal, corpse/monster explosions, `canDeal`, the conversion tables
//! (`activeSkill.conversionTable`, `weapon1Cfg/weapon2Cfg.conversionTable`) and
//! the damage passes.
//!
//! **L4**: for every build of all57 (`.parity/builds`) and slice5
//! (`corpus/U0/slice5`), PoB's main-env `offence:1692` dump is injected, the Rust
//! segment runs, and the state is diffed against PoB's `offence:2174` dump of the
//! same `offence` call (§9.2 tolerance; the env, the GlobalCache and every dumped
//! local, so `costs`, `canDeal`, `monsterLife`, `passList` and the conversion
//! tables reached through `activeSkill` are all compared).
//!
//! The segment calls `runSkillFunc("preDamageFunc")` (1908). OF-A has ported it
//! (`helpers_setup::runSkillFunc`), so the segment runs the hook itself and nothing
//! is replayed: a callee is replayed only while it still hits an unported stub (as
//! PF-C's `setup_flasks::callee_replay`). FUP1 (HYG3 exposed 1): replaying PoB's
//! recorded `skill:<id>:preDamageFunc` delta on top applied the hook twice, and
//! PoisonousConcoction's `NewMod` calls doubled its `ChaosMin`/`ChaosMax` mods on
//! allflame_exp_029/046/049.
//!
//! Dumps: the shared probe cache, `.parity/probe-cache/<sha>/<build-key>/sel-<key>/state/`
//! (probed by this test when absent, selection
//! `offence:1692@main,offence:2174@main`; needs LuaJIT, PoB's
//! checkout and the machine-wide oracle lock; `POB_QUERY_CACHE=refresh` re-probes). Knobs:
//! `OFC_BUILDS=stem1,stem2`, `OFC_VERBOSE=1`. A missing input fails
//! (`pob_testpaths::require`; `POB_ALLOW_SKIP=1` skips instead).

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use pob_oracle::oracle::find_parity_dir;
use pob_oracle::schema::StateDump;
use pob_oracle::testkit;

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
    v
}

/// all57 then slice5, filtered by `OFC_BUILDS`.
fn corpus(parity: &Path) -> Vec<(&'static str, PathBuf)> {
    let slice5 = Path::new(env!("CARGO_MANIFEST_DIR")).join("corpus/U0/slice5");
    let mut v: Vec<(&'static str, PathBuf)> = xmls(&parity.join("builds"))
        .into_iter()
        .map(|p| ("all57", p))
        .chain(xmls(&slice5).into_iter().map(|p| ("slice5", p)))
        .collect();
    if let Ok(list) = std::env::var("OFC_BUILDS") {
        let want: Vec<&str> = list.split(',').map(str::trim).collect();
        v.retain(|(_, p)| want.contains(&stem(p)));
    }
    v
}

const SELECT: &str = "offence:1692@main,offence:2174@main";

/// The build's OF-C dumps: the `state/` dir of this selection's entry in the shared probe cache
/// (`.parity/probe-cache/<sha>/<build-key>/sel-<key>/`, `testkit::probe_selection`), probed when
/// absent. HYG3 (test audit S4): this was a per-worktree `target/tmp/ofc-probe/<stem>/`, keyed by
/// stem only and never invalidated, under a per-worktree patched copy of PoB. Under
/// `POB_QUERY_CACHE=off` it is a private run, deleted when the result is dropped.
#[cfg(feature = "luajit")]
fn ofc_dumps(xml: &Path) -> Result<pob_oracle::runner::ProbeDumps, String> {
    let stages = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../docs/port/stages.toml");
    let opts = pob_oracle::runner::ProbeOptions {
        select: SELECT.split(',').map(str::to_owned).collect(),
        provenance: false,
        ..Default::default()
    };
    testkit::probe_selection(
        Path::new(env!("CARGO_BIN_EXE_pob_oracle")),
        xml,
        Some(&stages),
        &opts,
    )
}

#[cfg(not(feature = "luajit"))]
fn ofc_dumps(_xml: &Path) -> Result<pob_oracle::runner::ProbeDumps, String> {
    Err("needs the luajit feature".to_owned())
}

/// Every dump in `state` (file name → dump).
fn load_all(state: &Path) -> BTreeMap<String, StateDump> {
    let mut out = BTreeMap::new();
    if let Ok(rd) = std::fs::read_dir(state) {
        for e in rd.filter_map(Result::ok) {
            if let Ok(d) = StateDump::load(&e.path()) {
                out.insert(d.hit.clone(), d);
            }
        }
    }
    out
}

/// The innermost `offence:in#n` of a dump's stack.
fn offence_call(d: &StateDump) -> Option<&str> {
    d.stack
        .iter()
        .rev()
        .find(|h| h.starts_with("offence:in#"))
        .map(String::as_str)
}

#[derive(Default)]
struct Tally {
    hits: usize,
    hits_by_set: BTreeMap<&'static str, usize>,
    clean: usize,
    no_hit: Vec<String>,
    failed: Vec<String>,
    dirty: Vec<(String, &'static str, Vec<String>)>,
    conv_tables: usize,
}

/// One L4 run: inject `offence:1692`, run the segment, diff against `offence:2174`.
fn l4_one(stem: &str, dumps: &BTreeMap<String, StateDump>) -> Result<Vec<String>, String> {
    let pick = |probe: &str| {
        dumps
            .values()
            .filter(|d| d.probe == probe)
            .min_by_key(|d| d.n)
    };
    let din = pick("offence:1692").ok_or("no offence:1692 dump")?;
    let call = offence_call(din).ok_or("offence:1692 dump has no offence:in on its stack")?;
    let dout = dumps
        .values()
        .find(|d| d.probe == "offence:2174" && offence_call(d) == Some(call))
        .ok_or_else(|| format!("no offence:2174 dump in {call}"))?;
    let build = testkit::empty_build();
    let inj = pob_oracle::inject::inject(din, &build).map_err(|e| format!("inject: {e}"))?;
    let locals: Vec<(String, pob_calc::Val)> = inj.locals.into_iter().collect();
    let run = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        pob_calc::probe::run_segment("offence:1692", &locals)
    }));
    let after = match run {
        Ok(Ok((flow, after))) => {
            if flow != pob_calc::Flow::Normal {
                return Err(format!("segment returned {flow:?}"));
            }
            after
        }
        Ok(Err(e)) => return Err(format!("run: {e}")),
        Err(p) => {
            let msg = p
                .downcast_ref::<pob_lua::LuaError>()
                .map(|e| e.0.clone())
                .or_else(|| p.downcast_ref::<String>().cloned())
                .or_else(|| p.downcast_ref::<&str>().map(|s| (*s).to_owned()))
                .unwrap_or_else(|| "panic".to_owned());
            return Err(format!("run: {msg}"));
        }
    };
    let env = after
        .iter()
        .find(|(k, _)| k == "env")
        .and_then(|(_, v)| v.obj::<pob_calc::env::Env>())
        .or(inj.env)
        .ok_or("no env")?;
    // CCR-0055 §2: `weaponNCfg.skillStats = output.MainHand/OffHand` (2008/2025) stores a still
    // empty table in a typed field the injection marked absent (nil at 1692); the export elides
    // an absent-marked field whose value looks default (probe.rs `struct_members`), so the write
    // is announced to the registry here, as an injection of the :out dump would.
    if let Some(skill) = after
        .iter()
        .find(|(k, _)| k == "activeSkill")
        .and_then(|(_, v)| v.obj::<pob_calc::active_skill::ActiveSkill>())
    {
        for cfg in [skill.weapon1Cfg.get(), skill.weapon2Cfg.get()]
            .into_iter()
            .flatten()
        {
            if cfg.skillStats.get().is_some() {
                pob_calc::probe::unmark_absent(pob_calc::probe::rc_ptr(&cfg), "skillStats");
            }
        }
    }
    // runSkillFunc("preDamageFunc") (1908) is ported: the segment ran the hook itself
    let mm = testkit::diff_against(dout, Some(&env), &after, stem);
    Ok(mm
        .into_iter()
        .map(|m| {
            format!(
                "{} {:?}: pob={} ours={}{}",
                m.path,
                m.kind,
                clip(m.pob.as_ref()),
                clip(m.ours.as_ref()),
                m.writer
                    .map(|w| format!(" (pob writer {w})"))
                    .unwrap_or_default()
            )
        })
        .collect())
}

fn clip(v: Option<&serde_json::Value>) -> String {
    let s = v.map_or_else(|| "-".to_owned(), ToString::to_string);
    if s.len() > 160 {
        format!(
            "{}…",
            &s[..s.char_indices().nth(160).map_or(s.len(), |(i, _)| i)]
        )
    } else {
        s
    }
}

/// Does the dump hold a conversion table for the skill (non-vacuity of the
/// conversionTable comparison)?
fn has_conversion_table(d: &StateDump) -> bool {
    fn walk(v: &serde_json::Value, depth: usize) -> bool {
        if depth > 12 {
            return false;
        }
        match v {
            serde_json::Value::Object(m) => {
                if let Some(ct) = m.get("conversionTable") {
                    if ct.get("Physical").and_then(|p| p.get("mult")).is_some() {
                        return true;
                    }
                }
                m.values().any(|x| walk(x, depth + 1))
            }
            serde_json::Value::Array(a) => a.iter().any(|x| walk(x, depth + 1)),
            _ => false,
        }
    }
    walk(&d.root, 0)
}

#[test]
fn ofc_l4_costs_passes() {
    if pob_testpaths::require(
        testkit::init_default_data().then_some(()),
        "no data dump (data/dump)",
    )
    .is_none()
    {
        return; // POB_ALLOW_SKIP=1
    }
    let Some(parity) = pob_testpaths::require(find_parity_dir(), "no .parity directory") else {
        return; // POB_ALLOW_SKIP=1
    };
    let builds = corpus(&parity);
    if builds.is_empty() {
        let _ = pob_testpaths::require(None::<()>, "no corpus builds");
        return; // POB_ALLOW_SKIP=1
    }
    let verbose = std::env::var_os("OFC_VERBOSE").is_some();
    let mut t = Tally::default();
    for &(set, ref xml) in &builds {
        let stem = stem(xml);
        let state = match ofc_dumps(xml) {
            Ok(s) => s,
            Err(e) => {
                t.failed.push(format!("{stem}: probing failed: {e}"));
                continue;
            }
        };
        let dumps = load_all(&state);
        if !dumps.values().any(|d| d.probe == "offence:1692") {
            // the main env never ran offence past 371 (no main skill / disabled)
            t.no_hit.push(stem.to_owned());
            continue;
        }
        t.hits += 1;
        *t.hits_by_set.entry(set).or_default() += 1;
        if let Some(d) = dumps.values().find(|d| d.probe == "offence:2174") {
            if has_conversion_table(d) {
                t.conv_tables += 1;
            }
        }
        match l4_one(stem, &dumps) {
            Ok(mm) => {
                if mm.is_empty() {
                    t.clean += 1;
                } else {
                    t.dirty.push((stem.to_owned(), set, mm));
                }
            }
            Err(e) => t.failed.push(format!("{stem}: {e}")),
        }
    }
    eprintln!(
        "OF-C L4 offence:1692 -> offence:2174: {} builds, {} hits, {} clean, {} with mismatches, {} failed, {} without a main-env hit ({}); conversion tables in {} :out dumps",
        builds.len(),
        t.hits,
        t.clean,
        t.dirty.len(),
        t.failed.len(),
        t.no_hit.len(),
        t.no_hit.join(","),
        t.conv_tables
    );
    for f in &t.failed {
        eprintln!("FAILED {f}");
    }
    for (stem, set, mm) in &t.dirty {
        eprintln!("MISMATCH {set} {stem}: {} mismatches", mm.len());
        for m in mm.iter().take(if verbose { usize::MAX } else { 12 }) {
            eprintln!("    {m}");
        }
    }
    assert!(t.hits > 0, "no offence:1692 hits in the corpus");
    assert!(
        t.conv_tables == t.hits,
        "conversionTable missing from some :out dumps"
    );
    // calc doc §9.4 L4: 100 % on the unit's corpus (slice5); >= 98 % of all57 hits
    let slice5_dirty = t.dirty.iter().filter(|(_, s, _)| *s == "slice5").count();
    let all57_hits = t.hits_by_set.get("all57").copied().unwrap_or(0);
    let all57_dirty = t.dirty.iter().filter(|(_, s, _)| *s == "all57").count();
    assert!(t.failed.is_empty(), "{} builds failed", t.failed.len());
    assert_eq!(slice5_dirty, 0, "slice5 must be 100 % clean");
    assert!(
        all57_dirty as f64 <= 0.02 * all57_hits as f64,
        "{all57_dirty} of {all57_hits} all57 hits differ (> 2 %)"
    );
    // HYG2/V2: the 2 % slack is only for differences recorded in docs/port/known_diffs.toml
    // (calc doc §9.4 L4: "the rest in known_diffs.toml"): every dirty all57 build needs an
    // `offence:2174` entry naming it (or "*")
    let listed = known_diff_builds("offence:2174");
    let unlisted: Vec<&str> = t
        .dirty
        .iter()
        .filter(|(_, s, _)| *s == "all57")
        .map(|(stem, _, _)| stem.as_str())
        .filter(|stem| !listed.iter().any(|b| b == "*" || b == stem))
        .collect();
    assert!(
        unlisted.is_empty(),
        "all57 builds differ without a docs/port/known_diffs.toml offence:2174 entry: {unlisted:?}"
    );
    // coverage (HYG2/V2): every build was probed, and the builds whose main env never reaches
    // offence:1692 are exactly the listed ones
    if std::env::var_os("OFC_BUILDS").is_none() {
        assert_eq!(builds.len(), OFC_BUILDS, "all57 + slice5 builds");
        let mut no_hit: Vec<&str> = t.no_hit.iter().map(String::as_str).collect();
        no_hit.sort_unstable();
        assert_eq!(
            no_hit, OFC_NO_HIT,
            "builds without a main-env offence:1692 hit"
        );
        assert_eq!(t.hits, builds.len() - OFC_NO_HIT.len(), "offence:1692 hits");
    }
}

/// all57 + the 5 slice5 builds.
const OFC_BUILDS: usize = 62;
/// Builds whose main env never runs offence past CalcOffence.lua:371 (no main skill / disabled);
/// measured by HYG2 on rust-port 746b103.
const OFC_NO_HIT: &[&str] = &[
    "allflame_exp_010",
    "allflame_exp_023",
    "allflame_exp_028",
    "upstream_Dual_Savior",
];

/// The `builds` of the docs/port/known_diffs.toml entries for `stage`.
fn known_diff_builds(stage: &str) -> Vec<String> {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../docs/port/known_diffs.toml");
    let text = std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("{}: {e}", path.display()));
    let doc: toml::Table = text
        .parse()
        .unwrap_or_else(|e| panic!("{}: {e}", path.display()));
    doc.get("diff")
        .and_then(toml::Value::as_array)
        .into_iter()
        .flatten()
        .filter(|d| d.get("stage").and_then(toml::Value::as_str) == Some(stage))
        .flat_map(|d| {
            d.get("builds")
                .and_then(toml::Value::as_array)
                .into_iter()
                .flatten()
                .filter_map(|b| b.as_str().map(str::to_owned))
                .collect::<Vec<_>>()
        })
        .collect()
}

// ---- combineStat (2066-2170) -----------------------------------------------------------------

/// `combineStat` in every mode on hand-made `output.MainHand/OffHand` values (its callers,
/// OF-D/OF-F/OF-I, are not ported yet, so no L4 reaches it). The ctx's env/actor/activeSkill come
/// from an injected `offence:2174` dump; output, skillFlags, skillData, breakdown and
/// `globalOutput` are replaced. Expected values restate the Lua expressions (same operation order).
#[test]
fn ofc_combine_stat_modes() {
    use pob_calc::modules::calc_offence::costs_passes::combineStat;
    use pob_calc::modules::calc_offence::OffenceCtx;
    use pob_calc::prelude::*;
    if pob_testpaths::require(
        testkit::init_default_data().then_some(()),
        "no data dump (data/dump)",
    )
    .is_none()
    {
        return; // POB_ALLOW_SKIP=1
    }
    let Some(parity) = pob_testpaths::require(find_parity_dir(), "no .parity directory") else {
        return; // POB_ALLOW_SKIP=1
    };
    let Some(dout) = corpus(&parity).iter().find_map(|(_, xml)| {
        let dumps = load_all(&ofc_dumps(xml).ok()?);
        dumps.into_values().find(|d| d.probe == "offence:2174")
    }) else {
        let _ = pob_testpaths::require(None::<()>, "no offence:2174 dump");
        return; // POB_ALLOW_SKIP=1
    };
    let build = testkit::empty_build();
    let inj = pob_oracle::inject::inject(&dout, &build).expect("inject");
    let get = |k: &str| {
        inj.locals
            .iter()
            .find(|(n, _)| n.as_str() == k)
            .map(|(_, v)| v.clone())
            .unwrap_or(Val::Nil)
    };
    let env = get("env").obj::<Env>().expect("env");
    let actor = get("actor").obj::<Actor>().expect("actor");
    let skill = get("activeSkill")
        .obj::<ActiveSkill>()
        .expect("activeSkill");
    let mut ctx = OffenceCtx::init(&env, &actor, &skill);

    let fresh = |ctx: &mut OffenceCtx, both: bool, double: bool| {
        let mh = Table::new();
        let oh = Table::new();
        for (t, stat, hit, bleed, bdps) in [
            (&mh, 100.0, 80.0, 50.0, 100.0),
            (&oh, 50.0, 50.0, 40.0, 50.0),
        ] {
            t.set(n!("Stat"), stat);
            t.set(n!("HitChance"), hit);
            t.set(n!("BleedChance"), bleed);
            t.set(n!("BleedDPS"), bdps);
            t.set(n!("Zero"), if std::ptr::eq(t, &mh) { 0.0 } else { 7.0 });
        }
        oh.set(n!("OnlyOff"), 3.0);
        let output = Table::new();
        output.set(n!("MainHand"), mh);
        output.set(n!("OffHand"), oh);
        ctx.output = output.clone();
        let flags = Table::new();
        flags.set(n!("bothWeaponAttack"), both);
        ctx.skillFlags = flags;
        let data = Table::new();
        if double {
            data.set(n!("doubleHitsWhenDualWielding"), true);
        }
        ctx.skillData = data;
        ctx.breakdown = Some(Breakdown {
            t: Table::new(),
            modDB: ctx.skillModList.clone(),
            output,
            actor: std::rc::Weak::new(),
        });
    };
    let close = |a: f64, b: f64| (a - b).abs() <= 1e-12 * b.abs().max(1.0);
    let out = |ctx: &OffenceCtx, k: Name| ctx.output.get(k);

    // OR, and any mode without bothWeaponAttack: MainHand[stat] or OffHand[stat]
    fresh(&mut ctx, false, false);
    combineStat(&ctx, n!("Stat"), n!("AVERAGE"), None);
    assert!(out(&ctx, n!("Stat")).raw_eq(&Val::Num(100.0)));
    fresh(&mut ctx, true, false);
    combineStat(&ctx, n!("OnlyOff"), n!("OR"), None);
    assert!(out(&ctx, n!("OnlyOff")).raw_eq(&Val::Num(3.0)));
    // ADD / AVERAGE (nil counts as 0)
    combineStat(&ctx, n!("Stat"), n!("ADD"), None);
    assert!(close(out(&ctx, n!("Stat")).num(), 100.0 + 50.0));
    combineStat(&ctx, n!("OnlyOff"), n!("AVERAGE"), None);
    assert!(close(out(&ctx, n!("OnlyOff")).num(), (0.0 + 3.0) / 2.0));
    // HARMONICMEAN, and its zero guard
    combineStat(&ctx, n!("Stat"), n!("HARMONICMEAN"), None);
    assert!(close(
        out(&ctx, n!("Stat")).num(),
        2.0 / ((1.0 / 100.0) + (1.0 / 50.0))
    ));
    combineStat(&ctx, n!("Zero"), n!("HARMONICMEAN"), None);
    assert!(out(&ctx, n!("Zero")).raw_eq(&Val::Num(0.0)));
    // CHANCE, portions
    combineStat(&ctx, n!("Stat"), n!("CHANCE"), Some(n!("BleedChance")));
    let (mc, oc) = (50.0 * 80.0, 40.0 * 50.0);
    let (mp, op) = (mc / (mc + oc), oc / (mc + oc));
    assert!(close(out(&ctx, n!("Stat")).num(), 100.0 * mp + 50.0 * op));
    let bd = ctx
        .breakdown
        .as_ref()
        .unwrap()
        .table(n!("Stat"))
        .expect("breakdown.Stat");
    let lines: Vec<String> = (1..=bd.len())
        .map(|i| bd.rawget(i).str().unwrap().to_string())
        .collect();
    assert_eq!(lines.len(), 11, "{lines:?}");
    assert_eq!(lines[0], "Contribution from Main Hand:");
    assert_eq!(lines[1], "100.0");
    assert_eq!(
        lines[2],
        "x 0.667 ^8(portion of instances created by main hand)"
    );
    assert_eq!(lines[9], "66.7 + 16.7");
    assert_eq!(lines[10], "= 83.3");
    // CHANCE without the chance stat: output.MainHand[nil] * … raises
    let r = pob_lua::error::catch(|| combineStat(&ctx, n!("Stat"), n!("CHANCE"), None));
    assert!(r.is_err(), "CHANCE without a chance stat must raise");
    // CHANCE with only one hand's stat falls back to MainHand[stat] or OffHand[stat]
    combineStat(&ctx, n!("OnlyOff"), n!("CHANCE"), Some(n!("BleedChance")));
    assert!(out(&ctx, n!("OnlyOff")).raw_eq(&Val::Num(3.0)));
    // CHANCE, doubleHitsWhenDualWielding
    fresh(&mut ctx, true, true);
    combineStat(&ctx, n!("Stat"), n!("CHANCE"), Some(n!("BleedChance")));
    assert!(close(
        out(&ctx, n!("Stat")).num(),
        100.0 * (mc / 10000.0) + 50.0 * (oc / 10000.0)
    ));
    let bd = ctx.breakdown.as_ref().unwrap().table(n!("Stat")).unwrap();
    assert_eq!(
        bd.rawget(7i64).str().as_deref(),
        Some("x 0.200 ^8(chance of  off hand)")
    );
    // DPS: doubled hits keep the sum, otherwise the average
    combineStat(&ctx, n!("Stat"), n!("DPS"), None);
    assert!(close(out(&ctx, n!("Stat")).num(), 100.0 + 50.0));
    fresh(&mut ctx, true, false);
    combineStat(&ctx, n!("Stat"), n!("DPS"), None);
    assert!(close(out(&ctx, n!("Stat")).num(), (100.0 + 50.0) / 2.0));
    // CHANCE_AILMENT: stack weighting from globalOutput[<stat minus DPS>Stacks(Max)]
    let g = Table::new();
    g.set(n!("BleedStacks"), 3.0);
    g.set(n!("BleedStacksMax"), 4.0);
    OFFENCE_STATE.with(|s| s.globalOutput.set(Some(g)));
    combineStat(
        &ctx,
        n!("BleedDPS"),
        n!("CHANCE_AILMENT"),
        Some(n!("BleedChance")),
    );
    let s = (3.0f64 / 4.0).min(1.0);
    assert!(close(
        out(&ctx, n!("BleedDPS")).num(),
        100.0 * s + 50.0 * (1.0 - s)
    ));
    let bd = ctx
        .breakdown
        .as_ref()
        .unwrap()
        .table(n!("BleedDPS"))
        .unwrap();
    let lines: Vec<String> = (1..=bd.len())
        .map(|i| bd.rawget(i).str().unwrap().to_string())
        .collect();
    assert_eq!(
        lines,
        [
            "",
            "75.00% of ailment stacks use maximum damage",
            "Max Damage comes from Main Hand",
            "= 75.0",
            "25.00% of ailment stacks use non-maximum damage",
            "= 12.5",
            "",
            "Total:",
            "75.0 + 12.5",
            "= 87.5",
        ]
    );
    // CHANCE_AILMENT with one hand only
    combineStat(
        &ctx,
        n!("OnlyOff"),
        n!("CHANCE_AILMENT"),
        Some(n!("BleedChance")),
    );
    let bd = ctx
        .breakdown
        .as_ref()
        .unwrap()
        .table(n!("OnlyOff"))
        .unwrap();
    assert_eq!(
        bd.rawget(1i64).str().as_deref(),
        Some("All ailment stacks comes from Off Hand")
    );
    OFFENCE_STATE.with(|s| s.globalOutput.set(None));
}
