//! Unit ORC-X acceptance (calc doc §11.1 row ORC-X, §9.2 "Inject and export",
//! "Replay"):
//!
//! * **identity round trip**: for every all57 build and each of `initEnv:out`,
//!   `perform:in` and `offence:in`, the pob-state/1 dump PoB wrote is injected
//!   into the Rust graph (`pob_oracle::inject`) and exported again
//!   (`pob_calc::probe::export_root`); the export must equal the dump. Two
//!   measures are reported: exact JSON equality of the root (same `$id`
//!   numbering, keys and values) and the state diff of the resolved roots
//!   (`pob_calc_diff state`'s comparison, §9.2 tolerance). The gate is the
//!   state diff: zero mismatches on every dump.
//! * **replay**: injecting a callee's `:in` dump and applying its recorded
//!   `in → out` delta (`pob_oracle::replay::Replay`) reproduces its `:out`
//!   dump, for `offence` (all57) and `runSkillFunc` (dumped by this test into
//!   `target/orcx-probe/<stem>/` when absent; needs LuaJIT and the machine-wide
//!   oracle lock).
//!
//! Inputs: `.parity/probe/<stem>/state/` (`scripts/port/oracle.sh corpus`),
//! `data/dump`. Knobs: `ORCX_BUILDS=stem1,stem2`, `ORCX_LIMIT=n`,
//! `ORCX_VERBOSE=1` (print every mismatch). A missing input skips with a
//! message; with `POB_ORACLE_REQUIRE=1` (or `CI`) it fails instead.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use pob_oracle::diff::Mismatch;
use pob_oracle::oracle::find_parity_dir;
use pob_oracle::schema::StateDump;
use pob_oracle::testkit;

fn skip(why: &str) {
    let required = std::env::var_os("POB_ORACLE_REQUIRE").is_some_and(|v| v == "1")
        || std::env::var_os("CI").is_some_and(|v| !v.is_empty() && v != "false" && v != "0");
    assert!(!required, "required input missing: {why}");
    eprintln!("SKIPPED: {why}");
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
    if let Ok(list) = std::env::var("ORCX_BUILDS") {
        let want: Vec<&str> = list.split(',').map(str::trim).collect();
        v.retain(|p| want.contains(&stem(p)));
    }
    if let Some(n) = std::env::var("ORCX_LIMIT")
        .ok()
        .and_then(|s| s.parse::<usize>().ok())
    {
        v.truncate(n);
    }
    v
}

fn stem(p: &Path) -> &str {
    p.file_stem().and_then(|s| s.to_str()).unwrap_or_default()
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

#[derive(Default)]
struct Tally {
    dumps: usize,
    exact_measured: bool,
    exact: usize,
    clean: usize,
    missing: Vec<String>,
    failed: Vec<String>,
    by_path: BTreeMap<String, (usize, String)>,
    fallbacks: BTreeMap<String, u64>,
    notes: BTreeMap<String, usize>,
}

impl Tally {
    fn add(&mut self, what: &str, mm: &[Mismatch]) {
        let verbose = std::env::var_os("ORCX_VERBOSE").is_some();
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
                eprintln!(
                    "  {what}: {} {:?} pob={:?} ours={:?}",
                    m.path, m.kind, m.pob, m.ours
                );
            }
        }
    }

    fn report(&self, title: &str) {
        let exact = if self.exact_measured {
            format!("{} exact JSON, ", self.exact)
        } else {
            String::new()
        };
        eprintln!(
            "{title}: {} dumps, {exact}{} with zero state-diff mismatches, {} missing, {} failed",
            self.dumps,
            self.clean,
            self.missing.len(),
            self.failed.len()
        );
        for m in self.missing.iter().take(10) {
            eprintln!("  missing: {m}");
        }
        for f in self.failed.iter().take(10) {
            eprintln!("  failed: {f}");
        }
        let mut paths: Vec<_> = self.by_path.iter().collect();
        paths.sort_by(|a, b| b.1 .0.cmp(&a.1 .0));
        for (p, (n, ex)) in paths.iter().take(40) {
            eprintln!("  {n:6} {p}\n         e.g. {ex}");
        }
        if !self.fallbacks.is_empty() {
            eprintln!("  injection fallbacks (typed field could not hold the value; CCR-0030):");
            for (k, n) in &self.fallbacks {
                eprintln!("    {n:7} {k}");
            }
        }
        if !self.notes.is_empty() {
            eprintln!("  injection notes:");
            let mut v: Vec<_> = self.notes.iter().collect();
            v.sort_by(|a, b| b.1.cmp(a.1));
            for (k, n) in v.iter().take(20) {
                eprintln!("    {n:7} {k}");
            }
        }
    }
}

fn clip(s: &str) -> String {
    if s.len() > 200 {
        let end = s.char_indices().nth(200).map_or(s.len(), |(i, _)| i);
        format!("{}…", &s[..end])
    } else {
        s.to_owned()
    }
}

fn note_key(n: &str) -> String {
    // strip addresses / ids so notes group
    let mut out = String::new();
    let mut digits = false;
    for c in n.chars().take(120) {
        if c.is_ascii_digit() {
            if !digits {
                out.push('#');
            }
            digits = true;
        } else {
            digits = false;
            out.push(c);
        }
    }
    out
}

/// The gate's stages, then `runSkillFunc:in/out` (present once
/// `orcx_replay_run_skill_func_all57` has probed them).
const ROUND_TRIP_STAGES: &[&str] = &[
    "initEnv:out",
    "perform:in",
    "offence:in",
    "runSkillFunc:in",
    "runSkillFunc:out",
];

#[test]
fn orcx_identity_round_trip_all57() {
    if !testkit::init_default_data() {
        return skip("no data dump (data/dump)");
    }
    let Some(parity) = find_parity_dir() else {
        return skip("no .parity directory");
    };
    let builds = corpus(&parity);
    if builds.is_empty() {
        return skip("no .parity/builds/*.xml");
    }
    let mut tallies: BTreeMap<&str, Tally> = BTreeMap::new();
    for xml in &builds {
        let stem = stem(xml);
        for stage in ROUND_TRIP_STAGES {
            let t = tallies.entry(stage).or_default();
            t.exact_measured = true;
            // the corpus dumps, plus the runSkillFunc dumps the replay test probes
            let extra = [PathBuf::from(env!("CARGO_TARGET_TMPDIR"))
                .join("orcx-probe")
                .join(stem)
                .join("state")];
            let dump = testkit::load_dump(stem, Some(xml), stage).or_else(|| {
                testkit::find_dump(&extra, stage).and_then(|p| StateDump::load(&p).ok())
            });
            let Some(dump) = dump else {
                t.missing.push(format!("{stem} {stage}"));
                continue;
            };
            t.dumps += 1;
            let what = format!("{stem} {}", dump.hit);
            match testkit::round_trip(&dump) {
                Ok(rt) => {
                    if rt.exact {
                        t.exact += 1;
                    }
                    if rt.mismatches.is_empty() {
                        t.clean += 1;
                    }
                    t.add(&what, &rt.mismatches);
                    for (k, n) in rt.fallbacks {
                        *t.fallbacks.entry(k).or_default() += n;
                    }
                    for n in rt.notes {
                        *t.notes.entry(note_key(&n)).or_default() += 1;
                    }
                }
                Err(e) => t.failed.push(format!("{what}: {e}")),
            }
        }
    }
    let mut ok = true;
    for (stage, t) in &tallies {
        t.report(&format!("round trip {stage}"));
        ok &= t.clean == t.dumps && t.failed.is_empty();
    }
    let total: usize = tallies.values().map(|t| t.dumps).sum();
    assert!(total > 0, "no dumps found under .parity/probe");
    assert!(ok, "identity round trip: some dumps do not export back to themselves");
}

/// A dump's root through export(inject()) — the unit test of the pieces on a
/// small hand-written document.
#[test]
fn orcx_round_trip_synthetic() {
    if !testkit::init_default_data() {
        return skip("no data dump (data/dump)");
    }
    let doc = serde_json::json!({
        "$env": null, "build": "synthetic", "format": "pob-state/1", "hit": "x:in#1",
        "kind": "in", "n": 1, "probe": "x:in", "stack": [],
        "root": {
            "globalCache": {"cachedData": {"CALCS": {}, "CALCULATOR": {}, "MAIN": {}}},
            "locals": {
                "a": {"$id": 1, "k": [1, 2, {"$float": "inf"}], "self": {"$ref": 1}},
                "b": {"$ref": 1},
                "m": {"name": "Life", "type": "BASE", "value": 10, "flags": 0, "keywordFlags": 0,
                      "source": "Tree:1", "tags": [{"type": "Condition", "var": "X", "neg": false}]},
                "s": {"$store": "ModList", "mods": [
                        {"name": "Life", "type": "INC", "value": 5, "flags": 0, "keywordFlags": 0,
                         "tags": [], "sourceSlot": "Ring 1", "$at": "Modules/CalcSetup.lua:10"}],
                      "actor": {}, "conditions": {}, "multipliers": {"X": 2}, "parent": false},
                "t": {"x": 1, "$seq": [true], "$num": {"5": "five"}, "$$esc": 3},
            }
        }
    });
    let dump: StateDump = serde_json::from_value(doc).unwrap();
    let rt = testkit::round_trip(&dump).unwrap();
    for m in &rt.mismatches {
        eprintln!("{} {:?} pob={:?} ours={:?}", m.path, m.kind, m.pob, m.ours);
    }
    assert!(rt.mismatches.is_empty());
    assert!(rt.exact, "the synthetic document exports byte-for-byte (as JSON)");
}

// ---- injection is typed and the export sees changes -----------------------------------------------

/// Injection fills the typed fields (not only `extra`), and a change to the
/// injected state shows up in the export at its path (the round trip is not
/// vacuous).
#[test]
fn orcx_inject_is_typed_and_export_sensitive() {
    use pob_calc::prelude::*;
    if !testkit::init_default_data() {
        return skip("no data dump (data/dump)");
    }
    let Some(parity) = find_parity_dir() else {
        return skip("no .parity directory");
    };
    let Some(xml) = corpus(&parity).into_iter().next() else {
        return skip("no .parity/builds/*.xml");
    };
    let Some(dump) = testkit::load_dump(stem(&xml), Some(&xml), "perform:in") else {
        return skip("no perform:in dump under .parity/probe");
    };
    let build = testkit::empty_build();
    let (env, locals) = pob_oracle::inject::state(&dump, &build).expect("inject");
    let jenv = &dump.root["env"];
    // typed fields hold the dumped values
    assert_eq!(env.mode.as_str(), jenv["mode"].as_str().unwrap());
    let player = env.player.clone();
    if let Some(l) = pob_oracle::schema::decode_num(&jenv["player"]["level"]) {
        assert_eq!(player.level.get(), l);
    }
    assert!(
        player.modDB.get().all_mods().len() > 100,
        "player modDB mods are typed"
    );
    let main = player.mainSkill.get().expect("main skill");
    assert!(
        main.skillCfg.get().skillName.get().is_some(),
        "skillCfg.skillName typed"
    );
    assert!(
        !main.activeEffect.get().grantedEffect.get().levels.is_empty(),
        "activeEffect.grantedEffect resolved to data"
    );
    assert!(
        main.skillTypes.get().with(|s| s.iter().next().is_some()),
        "skillTypes typed"
    );
    assert!(
        !env.allocNodes.get().with(|n| n.keys().is_empty()),
        "allocNodes typed"
    );
    assert!(locals.contains_key("env"));
    // the cfg's skillTypes is the skill's (identity kept)
    if let Some(st) = main.skillCfg.get().skillTypes.get() {
        assert!(st.ptr_eq(&main.skillTypes.get()));
    }
    // sensitivity: change one value, the export differs exactly there
    let before = player.level.get();
    player.level.set(before + 1.0);
    let locals: Vec<(String, Val)> = locals.into_iter().collect();
    let diff = |ours: &serde_json::Value| {
        pob_oracle::diff::diff_trees(
            &pob_oracle::schema::resolve_ids(&dump.root),
            &pob_oracle::schema::resolve_ids(ours),
            &pob_oracle::diff::DiffOptions::strict(),
        )
    };
    let mm = diff(&pob_calc::probe::export_root(Some(&env), &locals));
    assert_eq!(
        mm.len(),
        1,
        "{:?}",
        mm.iter().map(|m| &m.path).collect::<Vec<_>>()
    );
    assert!(mm[0].path.ends_with(".level"), "{}", mm[0].path);
    player.level.set(before);
    // and a mod added to a store shows up as a mod-extra
    player
        .modDB
        .get()
        .NewMod(n!("Life"), ModType::BASE, 1.0, "ORC-X", 0, 0, &[]);
    let mm = diff(&pob_calc::probe::export_root(Some(&env), &locals));
    assert_eq!(mm.len(), 1);
    assert_eq!(mm[0].kind, pob_oracle::diff::MismatchKind::ModExtra);
}

// ---- replay ------------------------------------------------------------------------------------------

/// Inject `<callee>:in`, apply the recorded delta, diff against `<callee>:out`.
fn replay_one(
    stem: &str,
    din: &StateDump,
    dout: &StateDump,
    callee: &str,
) -> Result<Vec<Mismatch>, String> {
    let build = testkit::empty_build();
    let inj = pob_oracle::inject::inject(din, &build)?;
    let env = inj.env.clone().ok_or("no env in the :in dump")?;
    let mut dumps = BTreeMap::new();
    dumps.insert(din.hit.clone(), din.clone());
    dumps.insert(dout.hit.clone(), dout.clone());
    let replay = pob_oracle::replay::Replay::load(callee, &dumps);
    // the caller's handles stay valid: the delta is applied in place
    let mod_db = env.player.modDB.get();
    let output = env.player.output.get();
    let main = env.player.mainSkill.get();
    if !replay.apply(&din.hit, &env) {
        return Err(format!("{} not replayed", din.hit));
    }
    if !std::rc::Rc::ptr_eq(&mod_db, &env.player.modDB.get()) {
        return Err("replay replaced env.player.modDB".to_owned());
    }
    if !output.ptr_eq(&env.player.output.get()) && dout.root["env"]["player"].get("output").is_some() {
        return Err("replay replaced env.player.output".to_owned());
    }
    if let (Some(a), Some(b)) = (main, env.player.mainSkill.get()) {
        if !std::rc::Rc::ptr_eq(&a, &b) {
            return Err("replay replaced env.player.mainSkill".to_owned());
        }
    }
    let locals: Vec<(String, pob_calc::Val)> = inj.locals.into_iter().collect();
    Ok(testkit::diff_against(dout, Some(&env), &locals, stem))
}

#[test]
fn orcx_replay_offence_all57() {
    if !testkit::init_default_data() {
        return skip("no data dump (data/dump)");
    }
    let Some(parity) = find_parity_dir() else {
        return skip("no .parity directory");
    };
    let builds = corpus(&parity);
    let mut t = Tally::default();
    for xml in &builds {
        let stem = stem(xml);
        let (Some(din), Some(dout)) = (
            testkit::load_dump(stem, Some(xml), "offence:in"),
            testkit::load_dump(stem, Some(xml), "offence:out"),
        ) else {
            t.missing.push(format!("{stem} offence:in/out"));
            continue;
        };
        t.dumps += 1;
        let what = format!("{stem} {}", din.hit);
        match replay_one(stem, &din, &dout, "offence") {
            Ok(mm) => {
                if mm.is_empty() {
                    t.clean += 1;
                }
                t.add(&what, &mm);
            }
            Err(e) => t.failed.push(format!("{what}: {e}")),
        }
    }
    t.report("replay offence");
    assert!(t.dumps > 0, "no offence:in/out dumps under .parity/probe");
    assert!(
        t.clean == t.dumps && t.failed.is_empty(),
        "replaying offence does not reproduce offence:out"
    );
}

/// `runSkillFunc:in/out` dumps of the main env (the corpus run dumps only
/// `:in`): probed here into `target/tmp/orcx-probe/<stem>/` when absent.
#[cfg(feature = "luajit")]
fn run_skill_func_dumps(xml: &Path) -> Option<PathBuf> {
    let stem = stem(xml);
    let dir = PathBuf::from(env!("CARGO_TARGET_TMPDIR"))
        .join("orcx-probe")
        .join(stem);
    let state = dir.join("state");
    let have = std::fs::read_dir(&state).is_ok_and(|rd| {
        rd.filter_map(Result::ok)
            .any(|e| e.file_name().to_string_lossy().starts_with("runSkillFunc:"))
    });
    if have || dir.join("hits.json").is_file() {
        return Some(state);
    }
    let src = pob_oracle::oracle::find_pob_src()?;
    let root = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join("oracle-src");
    let patched = pob_oracle::patcher::patch(&src, None, &root).ok()?;
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).ok()?;
    let status = std::process::Command::new(env!("CARGO_BIN_EXE_pob_oracle"))
        .arg("probe")
        .arg(xml)
        .arg("--out-dir")
        .arg(&dir)
        .arg("--patched")
        .arg(&patched.src)
        .args(["--select", "runSkillFunc:*@main", "--no-provenance"])
        .stdout(std::process::Stdio::null())
        .stderr(std::fs::File::create(dir.with_extension("log")).ok()?)
        .status()
        .ok()?;
    status.success().then_some(state)
}

#[cfg(feature = "luajit")]
#[test]
fn orcx_replay_run_skill_func_all57() {
    if !testkit::init_default_data() {
        return skip("no data dump (data/dump)");
    }
    let Some(parity) = find_parity_dir() else {
        return skip("no .parity directory");
    };
    if pob_oracle::oracle::find_pob_src().is_none() {
        return skip("no PoB checkout");
    }
    let builds = corpus(&parity);
    let mut t = Tally::default();
    for xml in &builds {
        let stem = stem(xml);
        let Some(state) = run_skill_func_dumps(xml) else {
            t.failed.push(format!("{stem}: probing runSkillFunc failed"));
            continue;
        };
        let dirs = vec![state];
        let (Some(pin), Some(pout)) = (
            testkit::find_dump(&dirs, "runSkillFunc:in"),
            testkit::find_dump(&dirs, "runSkillFunc:out"),
        ) else {
            // the main env never ran a skill hook through runSkillFunc
            t.missing.push(format!("{stem} runSkillFunc:in/out"));
            continue;
        };
        let (Ok(din), Ok(dout)) = (StateDump::load(&pin), StateDump::load(&pout)) else {
            t.failed.push(format!("{stem}: unreadable runSkillFunc dump"));
            continue;
        };
        t.dumps += 1;
        let what = format!("{stem} {}", din.hit);
        match replay_one(stem, &din, &dout, "runSkillFunc") {
            Ok(mm) => {
                if mm.is_empty() {
                    t.clean += 1;
                }
                t.add(&what, &mm);
            }
            Err(e) => t.failed.push(format!("{what}: {e}")),
        }
    }
    t.report("replay runSkillFunc");
    assert!(t.dumps > 0, "no runSkillFunc:in/out dumps");
    assert!(
        t.clean == t.dumps && t.failed.is_empty(),
        "replaying runSkillFunc does not reproduce runSkillFunc:out"
    );
}

// ---- the L4 segment runner -----------------------------------------------------------------------

/// `pob_calc::probe::run_segment` builds the segment ctx from injected locals
/// (`ProbeState::probe_set`), runs the segment and returns the ctx locals:
/// with the segment still a stub, the exported state equals the injected one
/// (the plumbing L4 tests stand on: inject → ctx → segment → export → diff).
#[test]
fn orcx_segment_runner_plumbing() {
    if !testkit::init_default_data() {
        return skip("no data dump (data/dump)");
    }
    let Some(parity) = find_parity_dir() else {
        return skip("no .parity directory");
    };
    let Some(xml) = corpus(&parity).into_iter().next() else {
        return skip("no .parity/builds/*.xml");
    };
    let stem = stem(&xml);
    let Some(din) = testkit::load_dump(stem, Some(&xml), "offence:in") else {
        return skip("no offence:in dump under .parity/probe");
    };
    let build = testkit::empty_build();
    let inj = pob_oracle::inject::inject(&din, &build).expect("inject");
    let locals: Vec<(String, pob_calc::Val)> = inj.locals.into_iter().collect();
    let names: Vec<&str> = locals.iter().map(|(k, _)| k.as_str()).collect();
    assert!(names.contains(&"activeSkill") && names.contains(&"actor"));
    // the ctx takes the injected locals by name
    let ctx_names = <pob_calc::modules::calc_offence::OffenceCtx as pob_calc::probe::ProbeState>::locals();
    assert!(ctx_names.contains(&"skillCfg") && ctx_names.contains(&"ailmentData"));
    let stubbed = pob_calc::stub::stub_report().len();
    let (flow, after) =
        pob_calc::probe::run_segment("offence:362", &locals).expect("offence:362 runs");
    assert_eq!(flow, pob_calc::Flow::Normal);
    let after_names: Vec<&str> = after.iter().map(|(k, _)| k.as_str()).collect();
    for n in ["env", "actor", "activeSkill", "skillModList", "output", "passList"] {
        assert!(after_names.contains(&n), "ctx local {n} exported");
    }
    let env = inj.env.clone();
    let mm = testkit::diff_against(&din, env.as_ref(), &after, stem);
    for m in mm.iter().take(10) {
        eprintln!("{} {:?} pob={:?} ours={:?}", m.path, m.kind, m.pob, m.ours);
    }
    let _ = stubbed;
    let still_stub = pob_calc::stub::stub_report()
        .iter()
        .any(|(site, _)| *site == "Modules/CalcOffence.lua:362-1036");
    if still_stub {
        // the segment is still a stub: nothing changed
        assert!(mm.is_empty(), "a stub segment changed the state");
    }
    // unknown stages are an error, not a silent pass
    assert!(pob_calc::probe::run_segment("offence:9999", &locals).is_err());
}
