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
//! The segment calls `runSkillFunc("preDamageFunc")` (1908), OF-A's still
//! unported local function. When PoB ran a `preDamageFunc` hook inside the probed
//! `offence` call, its recorded `in → out` delta (`skill:<id>:preDamageFunc`) is
//! replayed onto the state after the segment (`pob_oracle::replay`). The hooks
//! write `skillData` / `output` fields the rest of the segment does not read, so
//! applying the delta after the segment instead of at 1908 is equivalent for the
//! corpus; the count of such builds is reported.
//!
//! Dumps: `target/tmp/ofc-probe/<stem>/` (probed by this test when absent,
//! selection `offence:1692@main,offence:2174@main,skill:*:preDamageFunc:*@main`;
//! needs LuaJIT, PoB's checkout and the machine-wide oracle lock). Knobs:
//! `OFC_BUILDS=stem1,stem2`, `OFC_VERBOSE=1`. A missing input skips with a
//! message; with `POB_ORACLE_REQUIRE=1` (or `CI`) it fails instead.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use pob_oracle::oracle::find_parity_dir;
use pob_oracle::schema::StateDump;
use pob_oracle::testkit;

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

const SELECT: &str = "offence:1692@main,offence:2174@main,skill:*:preDamageFunc:*@main";

/// The build's OF-C dumps (`target/tmp/ofc-probe/<stem>/state`), probed when absent.
fn ofc_dumps(xml: &Path) -> Option<PathBuf> {
    let dir = PathBuf::from(env!("CARGO_TARGET_TMPDIR"))
        .join("ofc-probe")
        .join(stem(xml));
    if dir.join("hits.json").is_file() {
        return Some(dir.join("state"));
    }
    probe(xml, &dir)
}

#[cfg(feature = "luajit")]
fn probe(xml: &Path, dir: &Path) -> Option<PathBuf> {
    let src = pob_oracle::oracle::find_pob_src()?;
    let stages = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../docs/port/stages.toml");
    let root = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join("oracle-src");
    let patched = pob_oracle::patcher::patch(&src, Some(&stages), &root).ok()?;
    let _ = std::fs::remove_dir_all(dir);
    std::fs::create_dir_all(dir).ok()?;
    let status = std::process::Command::new(env!("CARGO_BIN_EXE_pob_oracle"))
        .arg("probe")
        .arg(xml)
        .arg("--out-dir")
        .arg(dir)
        .arg("--patched")
        .arg(&patched.src)
        .args(["--select", SELECT, "--no-provenance"])
        .stdout(std::process::Stdio::null())
        .stderr(std::fs::File::create(dir.with_extension("log")).ok()?)
        .status()
        .ok()?;
    status.success().then(|| dir.join("state"))
}

#[cfg(not(feature = "luajit"))]
fn probe(_xml: &Path, _dir: &Path) -> Option<PathBuf> {
    None
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
    replayed: usize,
    no_hit: Vec<String>,
    failed: Vec<String>,
    dirty: Vec<(String, &'static str, Vec<String>)>,
    conv_tables: usize,
}

/// One L4 run: inject `offence:1692`, run the segment, replay a recorded
/// preDamageFunc, diff against `offence:2174`.
fn l4_one(stem: &str, dumps: &BTreeMap<String, StateDump>) -> Result<(Vec<String>, bool), String> {
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
    // the preDamageFunc hook PoB ran inside this offence call (1908)
    let mut replayed = false;
    let hook = dumps.values().find(|d| {
        d.kind == "in"
            && d.probe.starts_with("skill:")
            && d.probe.ends_with(":preDamageFunc:in")
            && offence_call(d) == Some(call)
    });
    if let Some(h) = hook {
        let callee = h.probe.trim_end_matches(":in");
        let replay = pob_oracle::replay::Replay::load(callee, dumps);
        let applied = replay
            .apply_report(&h.hit, &env)
            .map_err(|e| format!("replay {}: {e}", h.hit))?;
        if !applied.notes.is_empty() {
            return Err(format!("replay {} notes: {:?}", h.hit, applied.notes));
        }
        replayed = true;
    }
    let mm = testkit::diff_against(dout, Some(&env), &after, stem);
    Ok((
        mm.into_iter()
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
            .collect(),
        replayed,
    ))
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
    if !testkit::init_default_data() {
        return skip("no data dump (data/dump)");
    }
    let Some(parity) = find_parity_dir() else {
        return skip("no .parity directory");
    };
    let builds = corpus(&parity);
    if builds.is_empty() {
        return skip("no corpus builds");
    }
    let verbose = std::env::var_os("OFC_VERBOSE").is_some();
    let mut t = Tally::default();
    for &(set, ref xml) in &builds {
        let stem = stem(xml);
        let Some(state) = ofc_dumps(xml) else {
            t.failed
                .push(format!("{stem}: probing failed (LuaJIT / PoB checkout?)"));
            continue;
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
            Ok((mm, replayed)) => {
                if replayed {
                    t.replayed += 1;
                }
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
        "OF-C L4 offence:1692 -> offence:2174: {} builds, {} hits, {} clean, {} with mismatches, {} failed, {} without a main-env hit ({}); preDamageFunc replayed in {}; conversion tables in {} :out dumps",
        builds.len(),
        t.hits,
        t.clean,
        t.dirty.len(),
        t.failed.len(),
        t.no_hit.len(),
        t.no_hit.join(","),
        t.replayed,
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
    if !testkit::init_default_data() {
        return skip("no data dump (data/dump)");
    }
    let Some(parity) = find_parity_dir() else {
        return skip("no .parity directory");
    };
    let Some(dout) = corpus(&parity).iter().find_map(|(_, xml)| {
        let dumps = load_all(&ofc_dumps(xml)?);
        dumps.into_values().find(|d| d.probe == "offence:2174")
    }) else {
        return skip("no offence:2174 dump");
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
