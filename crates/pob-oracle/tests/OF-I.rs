//! OF-I acceptance (calc doc §11.1 row OF-I, §9.4 L4): segment `CalcOffence.lua` 5649-6263
//! (`calc_offence::dot_combined`): combineStat of the secondary effect stats, Decay, burning
//! ground, the skill DoT components, cost per second, self-hit damage and the combined DPS.
//!
//! **L4.** For every build of all57 (`.parity/builds`) and slice5 (`corpus/U0/slice5`), PoB's
//! main-env `offence:5650` dump (the stage before 5651; 5649-5650 are a blank line and a comment)
//! is injected, the Rust segment runs, and the state is diffed against PoB's `offence:end` dump
//! (line probe before the final `end` at 6263) of the same `offence` call (§9.2 tolerance: env,
//! GlobalCache and every dumped local, so `output`, `breakdown`, `skillFlags`, `activeSkill.dotCfg`
//! / `decayCfg` / `dot<Type>Cfg` are all compared).
//!
//! **Strict keys on dps30.** For the builds whose oracle `outputs.json` has `TotalDPS > 0` (the
//! dps30 set, §9.3), `CombinedDPS` and `TotalDot` of the main env's output must match exactly
//! (within the §9.2 numeric tolerance) on every comparable hit.
//!
//! **Callees of other units.** The segment calls OF-A's `calcResistForType` (494-508) and
//! `runSkillFunc` (509-514), and DF's `calcs.applyDmgTakenConversion` (CalcDefence 84-143). A
//! run that reaches the `calcResistForType` or `applyDmgTakenConversion` stub is reported as
//! BLOCKED (not compared). `runSkillFunc("preDotFunc")` (5806) is not blocking: no granted effect
//! defines `preDotFunc` at the pin (checked below against `data.skills`), so the call has no
//! effect in PoB and the stub's no-op is equivalent.
//!
//! **Foreign mirage output.** When `activeSkill.mirage.output` is the output table of the mirage's
//! own env (`calcs.mirages`), the dump records it as `{"$foreign": …}` without its contents, so
//! 6213-6247 cannot be replayed; such a pair is reported as FOREIGN (not compared).
//!
//! **Dumps.** Probed by this test into `$CARGO_TARGET_TMPDIR/OF-I/<stem>/` (one `pob_oracle
//! probe` process per build, selection `offence:5650@main,offence:end@main`; needs LuaJIT, the
//! PoB checkout and the machine-wide oracle lock) and reused while `complete` exists. Knobs:
//! `OFI_BUILDS=stem1,stem2`, `OFI_VERBOSE=1`.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use pob_calc::probe::ProbeField;
use pob_oracle::schema::StateDump;
use pob_oracle::testkit;

/// `unported!` sites of other units this segment calls whose stub result changes the state.
const DEP_STUBS: &[&str] = &[
    "Modules/CalcOffence.lua:494-508",
    "Modules/CalcDefence.lua:84-143",
];

fn dep_hits() -> u64 {
    pob_calc::stub::stub_report()
        .iter()
        .filter(|(s, _)| DEP_STUBS.contains(s))
        .map(|(_, n)| *n)
        .sum()
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

/// all57 then slice5, filtered by `OFI_BUILDS`.
fn corpus(parity: &Path) -> Vec<(&'static str, PathBuf)> {
    let slice5 = Path::new(env!("CARGO_MANIFEST_DIR")).join("corpus/U0/slice5");
    let mut v: Vec<(&'static str, PathBuf)> = xmls(&parity.join("builds"))
        .into_iter()
        .map(|p| ("all57", p))
        .chain(xmls(&slice5).into_iter().map(|p| ("slice5", p)))
        .collect();
    if let Ok(list) = std::env::var("OFI_BUILDS") {
        let want: Vec<&str> = list.split(',').map(str::trim).collect();
        v.retain(|(_, p)| want.contains(&stem(p)));
    }
    v
}

/// dps30 (§9.3): the oracle's main-skill `TotalDPS > 0` (`.parity/oracle/<stem>/outputs.json`).
fn is_dps30(parity: &Path, stem: &str) -> bool {
    let p = parity.join("oracle").join(stem).join("outputs.json");
    let Ok(text) = std::fs::read_to_string(&p) else {
        return false;
    };
    let Ok(j) = serde_json::from_str::<serde_json::Value>(&text) else {
        return false;
    };
    let player = &j["player"];
    let out = player.get("output").unwrap_or(player);
    out.get("TotalDPS")
        .and_then(serde_json::Value::as_f64)
        .is_some_and(|x| x > 0.0)
}

const SELECT: &str = "offence:5650@main,offence:end@main";

/// The build's dumps from one `pob_oracle probe` run (made on first use).
fn ensure_dumps(xml: &Path) -> Result<PathBuf, String> {
    let dir = Path::new(env!("CARGO_TARGET_TMPDIR"))
        .join("OF-I")
        .join(stem(xml));
    let state = dir.join("state");
    let done = dir.join("complete");
    if done.is_file() {
        return Ok(state);
    }
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).map_err(|e| format!("{}: {e}", dir.display()))?;
    let stages = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../docs/port/stages.toml");
    let mut cmd = std::process::Command::new(env!("CARGO_BIN_EXE_pob_oracle"));
    cmd.arg("probe")
        .arg(xml)
        .arg("--out-dir")
        .arg(&dir)
        .arg("--stages")
        .arg(&stages)
        .arg("--select")
        .arg(SELECT)
        .arg("--no-provenance")
        .stdout(std::process::Stdio::null())
        .stderr(
            std::fs::File::create(dir.with_extension("log"))
                .map_or_else(|_| std::process::Stdio::null(), Into::into),
        );
    if let Some(src) = pob_testpaths::pob_src() {
        cmd.arg("--pob").arg(src);
    }
    let status = cmd.status().map_err(|e| format!("pob_oracle probe: {e}"))?;
    if !status.success() {
        return Err(format!(
            "pob_oracle probe {} failed ({status})",
            xml.display()
        ));
    }
    std::fs::write(&done, b"").map_err(|e| format!("{}: {e}", done.display()))?;
    Ok(state)
}

/// Every dump in `state` (hit → dump).
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

/// The strict keys (`CombinedDPS`, `TotalDot`) of the main env's actor output.
const STRICT: &[&str] = &["CombinedDPS", "TotalDot"];

/// Is the mismatch path the actor output's key `k` (`…output.<k>`, not a nested table's)?
fn is_strict_path(path: &str) -> Option<&'static str> {
    STRICT.iter().copied().find(|k| {
        path.strip_suffix(k)
            .is_some_and(|p| p.ends_with("locals.output.") || p.ends_with(".player.output."))
    })
}

enum Outcome {
    Clean,
    Dirty(Vec<String>, Vec<String>),
    Blocked(usize),
    Foreign,
}

/// Whether `locals.activeSkill.mirage.output` is a table of another env (`{"$foreign": …}`): the
/// output of the mirage's own env (`calcs.mirages`), which the dump does not carry, so injection
/// cannot rebuild what 6213-6247 read and the pair is not comparable.
fn foreign_mirage_output(d: &StateDump) -> bool {
    fn index<'a>(
        v: &'a serde_json::Value,
        ids: &mut std::collections::HashMap<String, &'a serde_json::Value>,
    ) {
        match v {
            serde_json::Value::Object(m) => {
                if let Some(id) = m.get("$id") {
                    ids.insert(id.to_string().trim_matches('"').to_owned(), v);
                }
                m.values().for_each(|x| index(x, ids));
            }
            serde_json::Value::Array(a) => a.iter().for_each(|x| index(x, ids)),
            _ => {}
        }
    }
    let mut ids = std::collections::HashMap::new();
    index(&d.root, &mut ids);
    let resolve = |v: Option<&serde_json::Value>| -> Option<serde_json::Value> {
        let v = v?;
        match v.get("$ref") {
            Some(r) => ids
                .get(r.to_string().trim_matches('"'))
                .map(|x| (*x).clone()),
            None => Some(v.clone()),
        }
    };
    let skill = resolve(d.root.get("locals").and_then(|l| l.get("activeSkill")));
    let mirage = skill.and_then(|s| resolve(s.get("mirage")));
    let output = mirage.and_then(|m| resolve(m.get("output")));
    output.is_some_and(|o| o.get("$foreign").is_some())
}

/// One L4 run: inject `offence:5650`, run the segment, diff against `offence:end`.
fn l4_one(stem: &str, dumps: &BTreeMap<String, StateDump>) -> Result<Outcome, String> {
    let din = dumps
        .values()
        .filter(|d| d.probe == "offence:5650")
        .min_by_key(|d| d.n)
        .ok_or("no offence:5650 dump")?;
    let call = offence_call(din).ok_or("offence:5650 dump has no offence:in on its stack")?;
    if foreign_mirage_output(din) {
        return Ok(Outcome::Foreign);
    }
    let dout = dumps
        .values()
        .find(|d| d.probe == "offence:end" && offence_call(d) == Some(call))
        .ok_or_else(|| format!("no offence:end dump in {call}"))?;
    let build = testkit::empty_build();
    let inj = pob_oracle::inject::inject(din, &build).map_err(|e| format!("inject: {e}"))?;
    let mut locals: Vec<(String, pob_calc::Val)> = inj.locals.into_iter().collect();
    // CCR-0036 (OF-G): injection decodes locals with `Kind::Any`, so the dumped `passList`
    // arrives as a plain table; hand it to the ctx as the `Shared<Vec<Table>>` its field is.
    for (k, v) in &mut locals {
        if k == "passList" {
            if let Some(t) = v.table() {
                let list: Vec<pob_calc::Table> = t
                    .ipairs()
                    .into_iter()
                    .filter_map(|(_, p)| p.table())
                    .collect();
                *v = pob_lua::Shared::new(list).to_val();
            }
        }
    }
    let before = dep_hits();
    let run = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        pob_calc::probe::run_segment("offence:5650", &locals)
    }));
    let blocked = dep_hits() > before;
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
            if blocked {
                return Ok(Outcome::Blocked(0));
            }
            return Err(format!("run: {msg}"));
        }
    };
    let env = after
        .iter()
        .find(|(k, _)| k == "env")
        .and_then(|(_, v)| v.obj::<pob_calc::env::Env>())
        .or(inj.env)
        .ok_or("no env")?;
    let mm = testkit::diff_against(dout, Some(&env), &after, stem);
    if blocked {
        // `OFI_VERBOSE=1`: what differs downstream of the stubbed callees
        if std::env::var_os("OFI_VERBOSE").is_some() {
            for m in &mm {
                eprintln!(
                    "    blocked {stem}: {} {:?}: pob={} ours={}",
                    m.path,
                    m.kind,
                    clip(m.pob.as_ref()),
                    clip(m.ours.as_ref())
                );
            }
        }
        return Ok(Outcome::Blocked(mm.len()));
    }
    if mm.is_empty() {
        return Ok(Outcome::Clean);
    }
    let strict: Vec<String> = mm
        .iter()
        .filter_map(|m| {
            is_strict_path(&m.path).map(|k| {
                format!(
                    "{k}: pob={} ours={}",
                    clip(m.pob.as_ref()),
                    clip(m.ours.as_ref())
                )
            })
        })
        .collect();
    let lines = mm
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
        .collect();
    Ok(Outcome::Dirty(lines, strict))
}

#[derive(Default)]
struct Tally {
    hits: usize,
    hits_by_set: BTreeMap<&'static str, usize>,
    clean: usize,
    dps30_compared: usize,
    blocked: Vec<String>,
    foreign: Vec<String>,
    no_hit: Vec<String>,
    failed: Vec<String>,
    dirty: Vec<(String, &'static str, Vec<String>)>,
    strict_fail: Vec<(String, Vec<String>)>,
}

#[test]
fn ofi_l4_dot_combined() {
    if pob_testpaths::require(pob_testpaths::dump_dir(), "data/dump").is_none() {
        return;
    }
    assert!(testkit::init_default_data(), "data/dump does not load");
    let Some(parity) = pob_testpaths::require(pob_testpaths::parity_dir(), ".parity") else {
        return;
    };
    // the premise that makes the runSkillFunc stub at 5806 equivalent to PoB's call
    let with_pre_dot: Vec<&str> = pob_gamedata::data()
        .skills
        .iter()
        .filter(|(_, ge)| ge.preDotFunc.is_some())
        .map(|(id, _)| id.as_str())
        .collect();
    assert!(
        with_pre_dot.is_empty(),
        "granted effects define preDotFunc (runSkillFunc at 5806 is then not a no-op): {with_pre_dot:?}"
    );
    let builds = corpus(&parity);
    assert!(!builds.is_empty(), "no corpus builds");
    let verbose = std::env::var_os("OFI_VERBOSE").is_some();
    let mut t = Tally::default();
    for &(set, ref xml) in &builds {
        let stem = stem(xml);
        let state = match ensure_dumps(xml) {
            Ok(s) => s,
            Err(e) => {
                t.failed.push(format!("{stem}: {e}"));
                continue;
            }
        };
        let dumps = load_all(&state);
        if !dumps.values().any(|d| d.probe == "offence:5650") {
            // the main env never ran offence past 371 (no main skill / disabled)
            t.no_hit.push(stem.to_owned());
            continue;
        }
        t.hits += 1;
        *t.hits_by_set.entry(set).or_default() += 1;
        let dps30 = set == "all57" && is_dps30(&parity, stem);
        match l4_one(stem, &dumps) {
            Ok(Outcome::Clean) => {
                t.clean += 1;
                if dps30 {
                    t.dps30_compared += 1;
                }
            }
            Ok(Outcome::Dirty(mm, strict)) => {
                if dps30 {
                    t.dps30_compared += 1;
                    if !strict.is_empty() {
                        t.strict_fail.push((stem.to_owned(), strict));
                    }
                }
                t.dirty.push((stem.to_owned(), set, mm));
            }
            Ok(Outcome::Blocked(n)) => t.blocked.push(format!("{set} {stem} ({n} mismatches)")),
            Ok(Outcome::Foreign) => t.foreign.push(format!("{set} {stem}")),
            Err(e) => t.failed.push(format!("{stem}: {e}")),
        }
    }
    eprintln!(
        "OF-I L4 offence:5650 -> offence:end: {} builds, {} hits, {} clean, {} with mismatches, {} blocked on OF-A/DF stubs, {} not comparable (foreign mirage output), {} failed, {} without a main-env hit ({}); dps30 compared {} (strict CombinedDPS/TotalDot failures {})",
        builds.len(),
        t.hits,
        t.clean,
        t.dirty.len(),
        t.blocked.len(),
        t.foreign.len(),
        t.failed.len(),
        t.no_hit.len(),
        t.no_hit.join(","),
        t.dps30_compared,
        t.strict_fail.len()
    );
    for b in &t.blocked {
        eprintln!("BLOCKED {b}");
    }
    for f in &t.foreign {
        eprintln!(
            "FOREIGN {f}: activeSkill.mirage.output is another env's table (not in the dump)"
        );
    }
    for f in &t.failed {
        eprintln!("FAILED {f}");
    }
    for (stem, set, mm) in &t.dirty {
        eprintln!("MISMATCH {set} {stem}: {} mismatches", mm.len());
        for m in mm.iter().take(if verbose { usize::MAX } else { 12 }) {
            eprintln!("    {m}");
        }
    }
    for (stem, s) in &t.strict_fail {
        eprintln!("STRICT {stem}: {}", s.join("; "));
    }
    assert!(t.hits > 0, "no offence:5650 hits in the corpus");
    assert!(t.failed.is_empty(), "{} builds failed", t.failed.len());
    assert!(
        t.strict_fail.is_empty(),
        "CombinedDPS/TotalDot differ on dps30"
    );
    // calc doc §9.4 L4: 100 % on the unit's corpus (slice5); >= 98 % of the all57 hits
    let slice5_dirty = t.dirty.iter().filter(|(_, s, _)| *s == "slice5").count();
    let all57_hits = t.hits_by_set.get("all57").copied().unwrap_or(0);
    let all57_dirty = t.dirty.iter().filter(|(_, s, _)| *s == "all57").count();
    assert_eq!(slice5_dirty, 0, "slice5 must be 100 % clean");
    assert!(
        all57_dirty as f64 <= 0.02 * all57_hits as f64,
        "{all57_dirty} of {all57_hits} all57 hits differ (> 2 %)"
    );
}
