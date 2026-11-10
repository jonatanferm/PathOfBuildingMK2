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
//!   the shared probe cache `.parity/probe-cache/<sha>/<build-key>/sel-<key>/`
//!   when absent; needs LuaJIT and the machine-wide oracle lock). Each replay runs a second time after the caller changed
//!   state of its own (`output.Life`, a key and a mod only the caller has):
//!   those must survive and the callee's writes (`output.TotalDPS`) must
//!   still land, so the diff against `:out` is exactly the caller's changes.
//!
//! Inputs: `.parity/probe/<stem>/state/` (`scripts/port/oracle.sh corpus`),
//! `data/dump`. Knobs: `ORCX_BUILDS=stem1,stem2`, `ORCX_LIMIT=n`,
//! `ORCX_VERBOSE=1` (print every mismatch). A missing input fails
//! (`pob_testpaths::require`; `POB_ALLOW_SKIP=1` skips instead).

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use pob_oracle::diff::Mismatch;
use pob_oracle::oracle::find_parity_dir;
use pob_oracle::schema::StateDump;
use pob_oracle::testkit;

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
    /// replays with the caller's own changes (non-vacuity), and how many
    /// showed exactly those changes against `:out`
    perturbed: usize,
    perturbed_clean: usize,
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
        let perturbed = if self.perturbed > 0 {
            format!(
                ", {}/{} with caller changes kept (diff = exactly those changes)",
                self.perturbed_clean, self.perturbed
            )
        } else {
            String::new()
        };
        eprintln!(
            "{title}: {} dumps, {exact}{} with zero state-diff mismatches{perturbed}, {} missing, {} failed",
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
        let _ = pob_testpaths::require(None::<()>, "no .parity/builds/*.xml");
        return; // POB_ALLOW_SKIP=1
    }
    let mut tallies: BTreeMap<&str, Tally> = BTreeMap::new();
    for xml in &builds {
        let stem = stem(xml);
        for stage in ROUND_TRIP_STAGES {
            let t = tallies.entry(stage).or_default();
            t.exact_measured = true;
            // the corpus dumps, plus the runSkillFunc dumps the replay test probes (HYG3: in the
            // shared probe cache)
            let extra: Vec<PathBuf> = rsf_cached(xml).into_iter().collect();
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
    assert!(
        ok,
        "identity round trip: some dumps do not export back to themselves"
    );
    // coverage (HYG2/V2): every build has its gate-stage dumps (`.parity/probe`, filled by
    // `scripts/port/oracle.sh corpus`) except the listed ones. A runSkillFunc dump may only be
    // missing for a build the replay test has not probed yet (test order is not fixed) or one
    // whose main env runs no skill hook (ORCX_RSF_MISSING).
    if !filtered() {
        assert_eq!(builds.len(), CORPUS_BUILDS, ".parity/builds/*.xml count");
    }
    for (stage, t) in &tallies {
        let unlisted: Vec<&String> = t
            .missing
            .iter()
            .filter(|m| {
                let stem = m.split(' ').next().unwrap_or("");
                if stage.starts_with("runSkillFunc:") {
                    let probed = builds
                        .iter()
                        .find(|x| self::stem(x) == stem)
                        .is_some_and(|x| rsf_cached(x).is_some());
                    probed && !ORCX_RSF_MISSING.contains(&stem)
                } else {
                    !ORCX_GATE_MISSING.contains(&m.as_str())
                }
            })
            .collect();
        assert!(
            unlisted.is_empty(),
            "round trip {stage}: missing dumps {unlisted:?}"
        );
    }
}

/// The corpus size (`.parity/builds/*.xml`).
const CORPUS_BUILDS: usize = 57;
/// `<stem> <stage>` gate-stage dumps (initEnv:out, perform:in, offence:in) absent from
/// `.parity/probe`; measured by HYG2 on rust-port 746b103.
const ORCX_GATE_MISSING: &[&str] = &["upstream_Dual_Savior offence:in"];
/// Builds whose main env runs no skill hook through runSkillFunc (no `runSkillFunc:in/out`
/// dump); measured by HYG2 on rust-port 746b103.
const ORCX_RSF_MISSING: &[&str] = &[
    "allflame_exp_010",
    "allflame_exp_023",
    "allflame_exp_028",
    "upstream_Dual_Savior",
];
/// Builds without an `offence:in/out` dump pair; measured by HYG2 on rust-port 746b103.
const ORCX_OFFENCE_MISSING: &[&str] = &["upstream_Dual_Savior"];

/// `ORCX_BUILDS` / `ORCX_LIMIT` restrict the corpus.
fn filtered() -> bool {
    std::env::var_os("ORCX_BUILDS").is_some()
        || std::env::var_os("ORCX_LIMIT").is_some()
        || pob_testpaths::quick_corpus()
}

/// `missing` (`"<stem> <what>"`) names exactly the builds of `allowed`.
#[track_caller]
fn assert_missing(what: &str, missing: &[String], allowed: &[&str]) {
    let mut got: Vec<&str> = missing
        .iter()
        .map(|m| m.split(' ').next().unwrap_or(""))
        .collect();
    got.sort_unstable();
    let mut want = allowed.to_vec();
    want.sort_unstable();
    if filtered() {
        assert!(
            got.iter().all(|s| want.contains(s)),
            "{what}: missing dumps {got:?}, allowed {want:?}"
        );
    } else {
        assert_eq!(got, want, "{what}: builds without dumps");
    }
}

/// A dump's root through export(inject()) — the unit test of the pieces on a
/// small hand-written document.
#[test]
fn orcx_round_trip_synthetic() {
    if pob_testpaths::require(
        testkit::init_default_data().then_some(()),
        "no data dump (data/dump)",
    )
    .is_none()
    {
        return; // POB_ALLOW_SKIP=1
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
    let Some(xml) = pob_testpaths::require(
        corpus(&parity).into_iter().next(),
        "no .parity/builds/*.xml",
    ) else {
        return; // POB_ALLOW_SKIP=1
    };
    let Some(dump) = pob_testpaths::require(
        testkit::load_dump(stem(&xml), Some(&xml), "perform:in"),
        "no perform:in dump under .parity/probe",
    ) else {
        return; // POB_ALLOW_SKIP=1
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

/// The caller's own writes made before a replay ([`replay_one`] with
/// `perturb`): `output.Life` changed (`offence` and the skill hooks never
/// write it), a key only the caller has, a mod only the caller added.
const CALLER_KEY: &str = "ReviewCallerKey";
const CALLER_MOD: &str = "ReviewCallerMod";
const LIFE_SHIFT: f64 = 12345.0;

/// Inject `<callee>:in`, apply the recorded delta, diff against `<callee>:out`.
///
/// With `perturb`, the caller first changes state the callee does not write
/// (see [`CALLER_KEY`]); a delta replay must keep those changes and still
/// land every write of the callee, so the diff against `:out` must be
/// exactly the caller's three changes (a replay that overwrites the caller's
/// state with `:out` reports none of them, one that drops the callee's writes
/// reports more). Returns the remaining mismatches.
fn replay_one(
    stem: &str,
    din: &StateDump,
    dout: &StateDump,
    callee: &str,
    perturb: bool,
) -> Result<Vec<Mismatch>, String> {
    use pob_calc::prelude::*;
    let build = testkit::empty_build();
    let inj = pob_oracle::inject::inject(din, &build)?;
    let env = inj.env.clone().ok_or("no env in the :in dump")?;
    let life_in = env.player.output.get().opt_num("Life");
    let caller_life = life_in.unwrap_or(0.0) + LIFE_SHIFT;
    if perturb {
        let output = env.player.output.get();
        output.set("Life", caller_life);
        output.set(CALLER_KEY, 7.0);
        env.player
            .modDB
            .get()
            .NewMod(n!("ReviewCallerMod"), ModType::BASE, 3.0, "ORC-X review", 0, 0, &[]);
    }
    let mut dumps = BTreeMap::new();
    dumps.insert(din.hit.clone(), din.clone());
    dumps.insert(dout.hit.clone(), dout.clone());
    let replay = pob_oracle::replay::Replay::load(callee, &dumps);
    // the caller's handles stay valid: the delta is applied in place
    let mod_db = env.player.modDB.get();
    let output = env.player.output.get();
    let main = env.player.mainSkill.get();
    let applied = replay.apply_report(&din.hit, &env)?;
    if !applied.notes.is_empty() {
        return Err(format!("{} replay notes: {:?}", din.hit, applied.notes));
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
    let mut locals: Vec<(String, pob_calc::Val)> = inj.locals.into_iter().collect();
    // the callee's return value comes back from the replay
    if let Some(ret) = applied.ret {
        locals.push(("ret".to_owned(), ret));
    }
    let mm = testkit::diff_against(dout, Some(&env), &locals, stem);
    if !perturb {
        return Ok(mm);
    }
    // the caller's writes survive
    let output = env.player.output.get();
    if output.opt_num("Life") != Some(caller_life) {
        return Err(format!(
            "caller's output.Life lost: {:?}, want {caller_life}",
            output.opt_num("Life")
        ));
    }
    if output.opt_num(CALLER_KEY) != Some(7.0) {
        return Err(format!("caller's output.{CALLER_KEY} lost"));
    }
    let has_mod = env
        .player
        .modDB
        .get()
        .all_mods()
        .iter()
        .any(|m| m.name.as_str() == CALLER_MOD);
    if !has_mod {
        return Err(format!("caller's {CALLER_MOD} mod lost"));
    }
    // a path the callee writes ends at PoB's :out value
    let out_dps = pob_oracle::schema::get_path(
        &pob_oracle::schema::resolve_ids(&dout.root),
        "env.player.output.TotalDPS",
    )
    .and_then(pob_oracle::schema::decode_num);
    let in_dps = pob_oracle::schema::get_path(
        &pob_oracle::schema::resolve_ids(&din.root),
        "env.player.output.TotalDPS",
    )
    .and_then(pob_oracle::schema::decode_num);
    if let Some(want) = out_dps {
        let got = output.opt_num("TotalDPS");
        let same = |a: f64, b: f64| a == b || (a.is_nan() && b.is_nan());
        if !got.is_some_and(|g| same(g, want)) {
            return Err(format!(
                "callee's output.TotalDPS not replayed: {got:?}, PoB {want} (:in {in_dps:?})"
            ));
        }
    }
    // the diff against :out is the caller's changes and nothing else
    let mut seen = [false; 3];
    let mut rest = Vec::new();
    for m in mm {
        let i = if m.path == "env.player.output.Life" {
            0
        } else if m.path.contains(CALLER_KEY) {
            1
        } else if m.path.contains(CALLER_MOD)
            || m.ours.as_ref().is_some_and(|v| v.to_string().contains(CALLER_MOD))
        {
            2
        } else {
            rest.push(m);
            continue;
        };
        seen[i] = true;
    }
    // output.Life absent at :in and at :out only shows as an extra
    if !(seen[1] && seen[2] && (seen[0] || life_in.is_none())) {
        return Err(format!(
            "the caller's changes do not show in the diff against :out (Life {}, key {}, mod {})",
            seen[0], seen[1], seen[2]
        ));
    }
    Ok(rest)
}

/// Replay with and without the caller's own changes ([`replay_one`]).
fn replay_both(
    t: &mut Tally,
    stem: &str,
    din: &StateDump,
    dout: &StateDump,
    callee: &str,
) {
    for perturb in [false, true] {
        let what = format!("{stem} {}{}", din.hit, if perturb { " (caller changes)" } else { "" });
        if perturb {
            t.perturbed += 1;
        } else {
            t.dumps += 1;
        }
        match replay_one(stem, din, dout, callee, perturb) {
            Ok(mm) => {
                if mm.is_empty() {
                    if perturb {
                        t.perturbed_clean += 1;
                    } else {
                        t.clean += 1;
                    }
                }
                t.add(&what, &mm);
            }
            Err(e) => t.failed.push(format!("{what}: {e}")),
        }
    }
}

#[test]
fn orcx_replay_offence_all57() {
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
        replay_both(&mut t, stem, &din, &dout, "offence");
    }
    t.report("replay offence");
    assert!(t.dumps > 0, "no offence:in/out dumps under .parity/probe");
    assert!(
        t.clean == t.dumps && t.perturbed_clean == t.perturbed && t.failed.is_empty(),
        "replaying offence does not apply exactly its in→out delta"
    );
    assert_missing("replay offence", &t.missing, ORCX_OFFENCE_MISSING); // HYG2/V2
}

/// The probe selection of the `runSkillFunc:in/out` dumps (patched copy without a stages file).
#[cfg(feature = "luajit")]
fn rsf_options() -> pob_oracle::runner::ProbeOptions {
    pob_oracle::runner::ProbeOptions {
        select: vec!["runSkillFunc:*@main".to_owned()],
        provenance: false,
        ..Default::default()
    }
}

/// The `state/` dir of the build's runSkillFunc entry in the shared probe cache, if probed.
#[cfg(feature = "luajit")]
fn rsf_cached(xml: &Path) -> Option<PathBuf> {
    testkit::selection_cache(xml, None, &rsf_options())
        .ok()
        .filter(pob_oracle::runner::SelectionCache::is_filled)
        .map(|c| c.state_dir())
}

#[cfg(not(feature = "luajit"))]
fn rsf_cached(_xml: &Path) -> Option<PathBuf> {
    None
}

/// `runSkillFunc:in/out` dumps of the main env (the corpus run dumps only `:in`): the `state/`
/// dir of this selection's entry in the shared probe cache
/// (`.parity/probe-cache/<sha>/<build-key>/sel-<key>/`), probed here when absent. HYG3 (test
/// audit S4): this was a per-worktree `target/tmp/orcx-probe/<stem>/`, keyed by stem only and
/// never invalidated, under a per-worktree patched copy of PoB.
#[cfg(feature = "luajit")]
fn run_skill_func_dumps(xml: &Path) -> Result<pob_oracle::runner::ProbeDumps, String> {
    testkit::probe_selection(
        Path::new(env!("CARGO_BIN_EXE_pob_oracle")),
        xml,
        None,
        &rsf_options(),
    )
}

#[cfg(feature = "luajit")]
#[test]
fn orcx_replay_run_skill_func_all57() {
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
    if pob_oracle::oracle::find_pob_src().is_none() {
        let _ = pob_testpaths::require(None::<()>, "no PoB checkout");
        return; // POB_ALLOW_SKIP=1
    }
    let builds = corpus(&parity);
    let mut t = Tally::default();
    for xml in &builds {
        let stem = stem(xml);
        let state = match run_skill_func_dumps(xml) {
            Ok(s) => s,
            Err(e) => {
                t.failed
                    .push(format!("{stem}: probing runSkillFunc failed: {e}"));
                continue;
            }
        };
        // `state` (a private run under POB_QUERY_CACHE=off) is held to the end of the iteration
        let dirs = vec![state.to_path_buf()];
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
        replay_both(&mut t, stem, &din, &dout, "runSkillFunc");
    }
    t.report("replay runSkillFunc");
    assert!(t.dumps > 0, "no runSkillFunc:in/out dumps");
    assert!(
        t.clean == t.dumps && t.perturbed_clean == t.perturbed && t.failed.is_empty(),
        "replaying runSkillFunc does not apply exactly its in→out delta"
    );
    assert_missing("replay runSkillFunc", &t.missing, ORCX_RSF_MISSING); // HYG2/V2
}

// ---- the L4 segment runner -----------------------------------------------------------------------

/// `pob_calc::probe::run_segment` builds the segment ctx from injected locals
/// (`ProbeState::probe_set`), runs the segment and returns the ctx locals:
/// with the segment still a stub, the exported state equals the injected one
/// (the plumbing L4 tests stand on: inject → ctx → segment → export → diff).
#[test]
fn orcx_segment_runner_plumbing() {
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
    let Some(xml) = pob_testpaths::require(
        corpus(&parity).into_iter().next(),
        "no .parity/builds/*.xml",
    ) else {
        return; // POB_ALLOW_SKIP=1
    };
    let stem = stem(&xml);
    let Some(din) = pob_testpaths::require(
        testkit::load_dump(stem, Some(&xml), "offence:in"),
        "no offence:in dump under .parity/probe",
    ) else {
        return; // POB_ALLOW_SKIP=1
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
