//! P3 acceptance (calc-architecture.md §11.1 row P3, §9.2, §9.5):
//!
//! * the patcher round-trips a dummy `stages.toml` (corpus/P3/stages.toml)
//!   through `loadstring`, every edit reverts to the original text and no
//!   line number moves;
//! * function and line probes dump `pob-state/2` with a consistent `$id` /
//!   `$ref` graph, `<id>#<n>` hit numbering, call stacks and env identities;
//! * the probed PoB computes exactly what the unpatched PoB computes;
//! * write provenance covers >= 99% of the output keys;
//! * two runs agree outside the recorded nondeterministic set (a third run
//!   adds nothing structurally new);
//! * `pob_calc_diff` output is accepted by `scripts/parity/report.py`.
//!
//! The PoB runs need the PoB checkout and `.parity/builds` (`pob_testpaths`:
//! the environment, this workspace, the main checkout); a missing input fails
//! the test unless `POB_ALLOW_SKIP=1`.
//! Every PoB process takes the machine-wide oracle lock.
//!
//! `all57_corpus` (ignored by default) checks the corpus run that
//! `scripts/port/oracle.sh corpus --stages crates/pob-oracle/corpus/P3/stages.toml`
//! leaves in `.parity/probe/<stem>/`, and the nondet records of
//! `scripts/port/oracle.sh nondet` in `.parity/nondet/`.
#![allow(non_snake_case)]

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};
use std::process::Command;

use pob_oracle::diff::{diff_flat, flat_outputs, DiffOptions};
use pob_oracle::nondet::{compare_runs, NondetRecord};
use pob_oracle::oracle::{load_outputs, Leaf};
use pob_oracle::patcher::{self, SiteKind, StagesFile};
use pob_oracle::runner::ProbeCache;
use pob_oracle::schema::{coverage, flatten_state, HitsIndex, Provenance, StateDump};
use pob_testpaths::require;
use serde_json::Value as J;

const BUILD: &str = "upstream_OccVortex";

fn stages_path() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("corpus/P3/stages.toml")
}

fn scratch(name: &str) -> PathBuf {
    let d = PathBuf::from(env!("CARGO_TARGET_TMPDIR"))
        .join("P3")
        .join(name);
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    d
}

/// PoB's `src/` (missing: fail, or `None` with `POB_ALLOW_SKIP=1`).
fn pob_src() -> Option<PathBuf> {
    require(
        pob_testpaths::pob_src(),
        "PoB checkout (.PathOfBuilding/src)",
    )
}

/// `.parity/builds/<stem>.xml`, with the PoB checkout to run it.
fn build_xml(stem: &str) -> Option<PathBuf> {
    pob_src()?;
    require(
        pob_testpaths::parity_build(stem),
        &format!(".parity/builds/{stem}.xml"),
    )
}

/// The patched copy for the dummy stages (shared by the tests: the
/// publish is atomic and keyed by content).
fn patched() -> patcher::Patched {
    let src = pob_testpaths::pob_src().expect("PoB checkout");
    let root = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join("oracle-src");
    patcher::patch(&src, Some(&stages_path()), &root).unwrap_or_else(|e| panic!("{e}"))
}

fn probe(build: &Path, out: &Path, src: &Path, extra: &[&str]) {
    let status = Command::new(env!("CARGO_BIN_EXE_pob_oracle"))
        .arg("probe")
        .arg(build)
        .arg("--out-dir")
        .arg(out)
        .arg("--patched")
        .arg(src)
        .args(extra)
        .stderr(std::fs::File::create(out.with_extension("log")).unwrap())
        .status()
        .unwrap();
    let log = out.with_extension("log");
    assert!(
        status.success(),
        "pob_oracle probe failed ({}):\n{}",
        log.display(),
        std::fs::read_to_string(&log).unwrap_or_default()
    );
}

// ---- the patcher -------------------------------------------------------------------------------

#[test]
fn patcher_round_trips_dummy_stages() {
    let Some(src) = pob_src() else {
        return; // POB_ALLOW_SKIP=1
    };
    let stages = StagesFile::load(&stages_path()).unwrap();
    assert!(stages.stage.len() >= 8 && !stages.function.is_empty());
    let plan = patcher::plan(&src, &stages).unwrap_or_else(|e| panic!("{e}"));

    // loadstring accepts every patched file (after the loader's `+=` rewrite)
    patcher::validate(&plan).unwrap_or_else(|e| panic!("{e}"));

    for f in &plan.files {
        // line numbers never move
        assert_eq!(
            f.patched.matches('\n').count(),
            f.original.matches('\n').count(),
            "{}",
            f.file
        );
        // the edits revert to the original byte for byte
        let back =
            patcher::revert(&f.patched, &f.edits).unwrap_or_else(|e| panic!("{}: {e}", f.file));
        assert!(
            back == f.original,
            "{}: revert differs from the original",
            f.file
        );
        // and the original is what is on disk
        assert_eq!(
            std::fs::read_to_string(src.join(&f.file)).unwrap(),
            f.original
        );
    }

    // every stage is a line probe on its line, with its locals
    for st in &stages.stage {
        let site = plan
            .sites
            .iter()
            .find(|s| s.id == st.id)
            .unwrap_or_else(|| panic!("no site for {}", st.id));
        assert_eq!(site.kind, SiteKind::Line);
        assert_eq!((site.file.as_str(), site.line), (st.file.as_str(), st.line));
        let f = plan.files.iter().find(|f| f.file == st.file).unwrap();
        let line = f.patched.lines().nth(st.line - 1).unwrap();
        let head = format!("if __PROBE then __PROBE(\"{}\", {{ ", st.id);
        assert!(line.starts_with(&head), "{}:{}: {line}", st.file, st.line);
        for l in &st.locals {
            assert!(line.contains(&format!("{l} = {l}")), "{}: local {l}", st.id);
        }
    }
    // the compound assignment line was rewritten before prefixing
    let main = plan
        .files
        .iter()
        .find(|f| f.file == "Modules/Main.lua")
        .unwrap();
    assert!(main
        .patched
        .lines()
        .nth(341)
        .unwrap()
        .ends_with("count = count + (1)"));

    // the built-in function probes of §9.2 (+ the stages file's own)
    let fns: BTreeSet<&str> = plan
        .sites
        .iter()
        .filter(|s| s.kind == SiteKind::Function)
        .map(|s| s.id.as_str())
        .collect();
    for id in [
        "initEnv",
        "perform",
        "defence",
        "buildDefenceEstimations",
        "offence",
        "triggers",
        "mirages",
        "buildActiveSkillModList",
        "createActiveSkill",
        "runSkillFunc",
        "getCachedOutputValue",
        "ConfigTab:BuildModList",
        "mergeBuff",
    ] {
        assert!(fns.contains(id), "function probe {id}");
    }
    let hooks = fns.iter().filter(|i| i.starts_with("skill:")).count();
    let applies = fns.iter().filter(|i| i.starts_with("config:")).count();
    // pob-dump closures.json: 133 skill hooks, 532 config applies incl. the
    // shared mapAffixDropDownFunction (data/dump, AMENDMENTS A2)
    assert_eq!(hooks, 133, "skill hooks");
    assert_eq!(applies, 532, "config applies");
    let calls = plan
        .sites
        .iter()
        .filter(|s| s.kind == SiteKind::Call)
        .count();
    assert_eq!(calls, 3, "jewel func call sites");
    let prov = plan
        .sites
        .iter()
        .filter(|s| s.kind == SiteKind::Provenance)
        .count();
    let ins = plan
        .sites
        .iter()
        .filter(|s| s.kind == SiteKind::ModInsert)
        .count();
    assert_eq!((prov, ins), (7, 9));
    eprintln!(
        "{} files patched, {} sites ({} function probes)",
        plan.files.len(),
        plan.sites.len(),
        fns.len()
    );
}

/// Concurrent `patch_into` calls of one process (the tests' threads) publishing
/// the same sha into an empty root all succeed with the same copy. (`patch_into`,
/// not `patch`: since FUP1 `patch` publishes into the machine-wide shared root,
/// where the copy usually exists already and the publish race is never run.)
#[test]
fn patch_publishes_concurrently() {
    let Some(src) = pob_src() else {
        return; // POB_ALLOW_SKIP=1
    };
    let root = scratch("concurrent-patch");
    // the unpatched files of a copy are hard links to PoB's: a publish
    // must never write through one (the PoB checkout stays intact)
    fn sizes(dir: &Path, out: &mut BTreeMap<PathBuf, u64>) {
        for e in std::fs::read_dir(dir).unwrap().filter_map(Result::ok) {
            let p = e.path();
            if e.file_type().unwrap().is_dir() {
                sizes(&p, out);
            } else if p.extension().is_some_and(|x| x == "lua") {
                // (not PoB's runtime caches such as TimelessJewelData/*.bin)
                out.insert(p, e.metadata().unwrap().len());
            }
        }
    }
    let mut before = BTreeMap::new();
    sizes(&src, &mut before);
    let shas: Vec<String> = std::thread::scope(|s| {
        let hs: Vec<_> = (0..4)
            .map(|_| {
                s.spawn(|| {
                    patcher::patch_into(&src, Some(&stages_path()), &root)
                        .unwrap_or_else(|e| panic!("{e}"))
                        .sha
                })
            })
            .collect();
        hs.into_iter().map(|h| h.join().unwrap()).collect()
    });
    assert!(shas.iter().all(|s| *s == shas[0]), "{shas:?}");
    assert!(root
        .join(&shas[0])
        .join("src/Modules/CalcPerform.lua")
        .is_file());
    assert!(root.join(&shas[0]).join(".last-used").is_file());
    let leftovers: Vec<_> = std::fs::read_dir(&root)
        .unwrap()
        .filter_map(Result::ok)
        .map(|e| e.file_name().to_string_lossy().into_owned())
        .filter(|n| n.contains(".tmp."))
        .collect();
    assert!(leftovers.is_empty(), "{leftovers:?}");
    let mut after = BTreeMap::new();
    sizes(&src, &mut after);
    let changed: Vec<_> = before
        .iter()
        .filter(|(p, n)| after.get(*p) != Some(n))
        .take(5)
        .collect();
    assert!(
        changed.is_empty(),
        "PoB checkout files changed: {changed:?}"
    );
}

#[test]
fn patcher_rejects_bad_input() {
    let Some(src) = pob_src() else {
        return; // POB_ALLOW_SKIP=1
    };
    let bad_expect = StagesFile::parse(
        "[[stage]]\nid = \"x:in\"\nfile = \"Modules/CalcPerform.lua\"\nline = 1943\nexpect = \"not on this line\"\n",
    )
    .unwrap();
    assert!(patcher::plan(&src, &bad_expect).is_err());
    let bad_local = StagesFile::parse(
        "[[stage]]\nid = \"x:in\"\nfile = \"Modules/CalcPerform.lua\"\nline = 1943\nlocals = [\"a.b\"]\n",
    )
    .unwrap();
    assert!(patcher::plan(&src, &bad_local).is_err());
    let dup = StagesFile::parse(
        "[[stage]]\nid = \"perform:in\"\nfile = \"Modules/CalcPerform.lua\"\nline = 1943\n",
    )
    .unwrap();
    assert!(
        patcher::plan(&src, &dup).is_err(),
        "collides with the perform function probe"
    );
    assert!(
        StagesFile::parse("[[stage]]\nid = \"x\"\nfile = \"a\"\nline = 1\nbogus = 1\n").is_err()
    );

    // a stage that breaks the syntax is caught by loadstring
    let mut plan = patcher::plan(&src, &StagesFile::default()).unwrap();
    let f = plan
        .files
        .iter_mut()
        .find(|f| f.file == "Modules/CalcPerform.lua")
        .unwrap();
    assert!(f.patched.contains("__PROBE_FN(\"perform\""));
    f.patched = f
        .patched
        .replacen("__PROBE_FN(\"perform\"", "__PROBE_FN((\"perform\"", 1);
    let e = patcher::validate(&plan).unwrap_err();
    assert!(e.0.contains("Modules/CalcPerform.lua"), "{e}");
}

// ---- one build end to end ----------------------------------------------------------------------

/// `$id`s are unique and every `$ref` names one.
fn check_ids(v: &J, ids: &mut BTreeSet<u64>, refs: &mut BTreeSet<u64>) {
    match v {
        J::Array(a) => a.iter().for_each(|x| check_ids(x, ids, refs)),
        J::Object(m) => {
            if let Some(id) = m.get("$id").and_then(J::as_u64) {
                assert!(ids.insert(id), "duplicate $id {id}");
            }
            if m.len() == 1 {
                if let Some(r) = m.get("$ref").and_then(J::as_u64) {
                    refs.insert(r);
                }
            }
            m.values().for_each(|x| check_ids(x, ids, refs));
        }
        _ => {}
    }
}

/// (mods of mod stores with an inserting `file:line`, mods of mod stores).
fn count_at(v: &J, n: &mut usize, total: &mut usize) {
    match v {
        J::Array(a) => a.iter().for_each(|x| count_at(x, n, total)),
        J::Object(m) => {
            let store = m.contains_key("$store");
            if store {
                let mods: Vec<&J> = match m.get("mods") {
                    Some(J::Object(by_name)) => {
                        by_name.values().filter_map(J::as_array).flatten().collect()
                    }
                    Some(J::Array(list)) => list.iter().collect(),
                    _ => Vec::new(),
                };
                for md in mods {
                    *total += 1;
                    if md
                        .get("$at")
                        .and_then(J::as_str)
                        .is_some_and(|s| s.contains(".lua:"))
                    {
                        *n += 1;
                    }
                }
            }
            m.iter()
                .filter(|(k, _)| !(store && k.as_str() == "mods"))
                .for_each(|(_, x)| count_at(x, n, total));
        }
        _ => {}
    }
}

#[test]
fn probe_one_build() {
    let Some(build) = build_xml(BUILD) else {
        return; // POB_ALLOW_SKIP=1
    };
    let p = patched();
    let out = scratch("probe");
    let t = std::time::Instant::now();
    probe(&build, &out, &p.src, &[]);
    eprintln!("probe: {:?}", t.elapsed());

    let hits = HitsIndex::load(&out.join("hits.json")).unwrap();
    assert_eq!(hits.format, "pob-hits/1");
    assert_eq!(hits.build, BUILD);
    assert!(hits.envs >= 2, "envs {}", hits.envs);
    assert_eq!(hits.skipped, 0);
    // function probes fire :in and :out equally often
    for id in [
        "initEnv",
        "perform",
        "defence",
        "offence",
        "buildActiveSkillModList",
        "ConfigTab:BuildModList",
    ] {
        let i = hits.counts.get(&format!("{id}:in")).copied().unwrap_or(0);
        let o = hits.counts.get(&format!("{id}:out")).copied().unwrap_or(0);
        assert!(i > 0 && i == o, "{id}: in {i} out {o}");
    }
    // line probes reached while loading a build (the Main.lua one is not)
    for id in [
        "initEnv:items:in",
        "initEnv:skills:in",
        "perform:pools:in",
        "perform:buffs:in",
        "offence:speed:in",
        "offence:passHit:in",
        "defence:recovery:in",
    ] {
        assert!(
            hits.counts.get(id).copied().unwrap_or(0) > 0,
            "line probe {id}"
        );
    }
    assert!(!hits.counts.contains_key("main:modcache:count"));
    assert!(hits
        .counts
        .keys()
        .any(|k| k.starts_with("skill:") && k.ends_with(":in")));
    assert!(hits
        .counts
        .keys()
        .any(|k| k.starts_with("config:") && k.ends_with(":in")));
    assert!(hits.counts.contains_key("mergeBuff:in"));

    // the default selection: the env-level and line probes' first hit in
    // the final main env (whose outputs are outputs.json), config applies
    let main_env = hits.main_env.clone().expect("main env");
    let main_hit = |probe: &str| -> String {
        let found: Vec<&str> = hits
            .dumps
            .iter()
            .filter(|d| d.hit.rsplit_once('#').is_some_and(|(p, _)| p == probe))
            .map(|d| {
                assert_eq!(d.env.as_deref(), Some(main_env.as_str()), "{}", d.hit);
                d.hit.as_str()
            })
            .collect();
        assert_eq!(found.len(), 1, "{probe}: {found:?}");
        found[0].to_owned()
    };
    for want in [
        "initEnv:out",
        "initEnv:items:in",
        "perform:in",
        "perform:out",
        "offence:in",
        "offence:passHit:in",
        "defence:recovery:in",
    ] {
        main_hit(want);
    }
    assert!(hits.dumps.iter().any(|d| d.hit.starts_with("config:")));

    // every dump: the current pob-state format, its hit name, a consistent identity graph
    let mut mods_at = (0, 0);
    for d in &hits.dumps {
        let s = StateDump::load(&out.join(&d.file)).unwrap();
        assert!(s.is_current(), "{}: {}", d.file, s.format);
        assert_eq!(s.hit, d.hit);
        let (probe_id, n) = d.hit.rsplit_once('#').unwrap();
        assert_eq!(
            (s.probe.as_str(), s.n.to_string()),
            (probe_id, n.to_owned())
        );
        assert_eq!(s.stack, d.stack);
        let (mut ids, mut refs) = (BTreeSet::new(), BTreeSet::new());
        check_ids(&s.root, &mut ids, &mut refs);
        let dangling: Vec<_> = refs.difference(&ids).collect();
        assert!(dangling.is_empty(), "{}: dangling $ref {dangling:?}", d.hit);
        count_at(&s.root, &mut mods_at.0, &mut mods_at.1);
    }
    // AddMod caller lines on the mods of mod stores (the few without are
    // put into a ModList by CalcPerform's mergeBuff with a plain t_insert)
    eprintln!("store mods with $at: {}/{}", mods_at.0, mods_at.1);
    assert!(mods_at.1 > 1000 && mods_at.0 as f64 >= 0.99 * mods_at.1 as f64);

    // the identity graph: the player's modDB is env.modDB, the main skill
    // is an entry of activeSkillList, nested probes record their stack
    let perf_hit = main_hit("perform:out");
    let perf = StateDump::load(&out.join(format!("state/{perf_hit}.json"))).unwrap();
    let r = perf.resolved();
    let env = &r["env"];
    assert_eq!(
        env["player"]["modDB"],
        serde_json::json!({"$ref": "env.modDB"})
    );
    assert_eq!(env["modDB"]["$store"], "ModDB");
    // (breadth-first: `mainSkill` is a shorter path than `activeSkillList[i]`)
    let list = &env["player"]["activeSkillList"];
    let entries: Vec<&J> = list.as_array().map_or_else(
        || list["$seq"].as_array().unwrap().iter().collect(),
        |a| a.iter().collect(),
    );
    assert!(
        entries.contains(&&serde_json::json!({"$ref": "env.player.mainSkill"})),
        "mainSkill is one of activeSkillList"
    );
    assert_eq!(perf.env_id.as_deref(), Some(main_env.as_str()));
    let flat = flatten_state(&r);
    assert!(flat.keys().any(|k| k.starts_with("env.player.output.Life")));
    let pass_hit = main_hit("offence:passHit:in");
    let pass = StateDump::load(&out.join(format!("state/{pass_hit}.json"))).unwrap();
    assert!(
        pass.stack.iter().any(|h| h.starts_with("offence:in#")),
        "{:?}",
        pass.stack
    );
    assert!(pass.locals().unwrap().get("cfg").is_some());
    assert!(
        env["player"]["mainSkill"]
            .get("$uuid")
            .is_some_and(J::is_string),
        "active skills carry their uuid"
    );

    // the probed PoB computes what the unpatched PoB computes (the same
    // checkout the patched copy was made from: the binary's own ancestor
    // search does not reach the main checkout from an outside worktree)
    let plain = scratch("plain").join("outputs.json");
    let plain_log = plain.with_extension("log");
    let st = Command::new(env!("CARGO_BIN_EXE_pob_oracle"))
        .arg("outputs")
        .arg(&build)
        .arg("--json")
        .arg(&plain)
        .arg("--pob")
        .arg(pob_testpaths::pob_src().expect("PoB checkout"))
        .stderr(std::fs::File::create(&plain_log).unwrap())
        .status()
        .unwrap();
    assert!(
        st.success(),
        "pob_oracle outputs failed:\n{}",
        std::fs::read_to_string(&plain_log).unwrap_or_default()
    );
    let a = load_outputs(&plain).unwrap();
    let b = load_outputs(&out.join("outputs.json")).unwrap();
    assert_eq!(a.pob_version, b.pob_version);
    assert_eq!(a.main_skill, b.main_skill);
    let (fa, fb) = (a.flat(), b.flat());
    let differing: Vec<&String> = fa.keys().filter(|k| fa.get(*k) != fb.get(*k)).collect();
    assert_eq!(fa.len(), fb.len());
    assert!(differing.is_empty(), "patched PoB differs at {differing:?}");

    // write provenance
    let prov = Provenance::load(&out.join("provenance.json")).unwrap();
    let c = coverage(&b, &prov);
    eprintln!(
        "provenance: {}/{} output keys ({:.2}%), {}/{} proxied keys direct",
        c.covered,
        c.keys,
        100.0 * c.ratio(),
        c.proxied_covered,
        c.proxied_keys
    );
    assert!(c.ratio() >= 0.99, "uncovered: {:?}", c.uncovered);
    assert!(prov
        .writer("Life")
        .is_some_and(|w| w.starts_with("Modules/CalcDefence.lua:")
            || w.starts_with("Modules/CalcPerform.lua:")));
    assert!(prov
        .writer("TotalDPS")
        .is_some_and(|w| w.starts_with("Modules/CalcOffence.lua:")));
    assert!(!prov.breakdown.is_empty());
    assert!(fb.get("Life").is_some_and(|l| matches!(l, Leaf::Num(_))));

    // pob_calc_diff state: a dump against itself, and :in against :out
    let state_diff = |a: &str, b: &str| {
        Command::new(env!("CARGO_BIN_EXE_pob_calc_diff"))
            .arg("state")
            .arg(out.join("state").join(a))
            .arg(out.join("state").join(b))
            .arg("--fail")
            .output()
            .unwrap()
    };
    let perf_file = format!("{perf_hit}.json");
    let same = state_diff(&perf_file, &perf_file);
    assert!(
        same.status.success(),
        "{}",
        String::from_utf8_lossy(&same.stdout)
    );
    let moved = state_diff(&format!("{}.json", main_hit("perform:in")), &perf_file);
    assert_eq!(moved.status.code(), Some(1));
    let text = String::from_utf8_lossy(&moved.stdout);
    assert!(
        text.contains("== upstream_OccVortex perform:in#") && text.contains("\"extra\": "),
        "{text}"
    );

    // the probe cache round trip (in a scratch parity dir)
    let cache = ProbeCache::new(&scratch("cache"), &p.sha, &build).unwrap();
    assert_eq!(cache.store_run(&out).unwrap(), hits.dumps.len());
    let got = cache.get(&perf_hit).unwrap().expect("cached perform:out");
    assert_eq!(got.root, perf.root);
    assert!(cache.get("perform:out#999").unwrap().is_none());
}

// ---- nondeterminism --------------------------------------------------------------------------

#[test]
fn two_runs_agree_outside_the_nondet_set() {
    let Some(build) = build_xml(BUILD) else {
        return; // POB_ALLOW_SKIP=1
    };
    let p = patched();
    let work = scratch("nondet");
    let opts = [
        "--select",
        "perform:out#1,offence:in#1,initEnv:out#1,offence:out@main",
    ];
    let runs: Vec<PathBuf> = (1..=3).map(|i| work.join(format!("run{i}"))).collect();
    for r in &runs {
        probe(&build, r, &p.src, &opts);
    }
    // `@main`: the first offence:out hit in the env that became calcsTab.mainEnv
    let hits = HitsIndex::load(&runs[0].join("hits.json")).unwrap();
    let main_env = hits.main_env.clone().expect("main env");
    let at_main: Vec<_> = hits
        .dumps
        .iter()
        .filter(|d| d.hit.starts_with("offence:out#"))
        .collect();
    assert_eq!(at_main.len(), 1, "{:?}", hits.dumps);
    assert_eq!(at_main[0].env.as_deref(), Some(main_env.as_str()));
    let pair = compare_runs(BUILD, &[&runs[0], &runs[1]]).unwrap();
    let third = compare_runs(BUILD, &[&runs[0], &runs[2]]).unwrap();
    eprintln!(
        "pair: {} entries, third: {} entries",
        pair.len(),
        third.len()
    );
    eprintln!("pair labels: {:?}", pair.labels());
    // outputs: every run agrees with run 1 outside the pair's set
    let flat = |r: &Path| {
        flat_outputs(
            &serde_json::from_str(&std::fs::read_to_string(r.join("outputs.json")).unwrap())
                .unwrap(),
        )
    };
    let f1 = flat(&runs[0]);
    let opts = DiffOptions {
        rel: 0.0,
        nondet_values: Some(&pair.outputs),
        nondet_orders: Some(&pair.output_orders),
    };
    for r in &runs[1..] {
        let d = diff_flat(&f1, &flat(r), &opts, None, None);
        assert!(
            d.mismatches.is_empty() && d.missing == 0 && d.extra == 0,
            "{}: {:?}",
            r.display(),
            d.mismatches
        );
    }
    // states: the third run shows no value difference the pair did not
    // (list positions generalised). PoB's run-to-run *order* differences
    // (mods within a store name, string-keyed iteration) are only sampled
    // by two or three runs, so new order-only entries can still appear;
    // the diff compares mod stores as multisets and reports them LUA-ORDER.
    let uncovered = pair.uncovered(&third, true);
    let (orders, values): (Vec<&String>, Vec<&String>) =
        uncovered.iter().partition(|l| l.starts_with("order:"));
    eprintln!(
        "third run: {} new order-only entries {orders:?}",
        orders.len()
    );
    assert!(values.is_empty(), "uncovered values: {values:?}");
    let rec_path = work.join("rec.json");
    pair.save(&rec_path).unwrap();
    assert_eq!(NondetRecord::load(&rec_path).unwrap(), pair);
    // the state diff of run 2 against run 1 accepts what the pair recorded
    let (kinds, n, lines) = state_diff_runs(&runs[1], &runs[0], &rec_path);
    eprintln!("state diff run2 vs run1: {n} hits, {kinds:?}");
    assert!(n >= 4, "{n} hits in common");
    assert!(kinds.is_empty(), "{kinds:?} {lines:#?}");
}

/// `pob_calc_diff state` of every hit both runs dumped, one run as PoB and
/// the other as "ours", with the nondet record `rec`: (mismatches by kind,
/// hits compared, the first few mismatch lines).
fn state_diff_runs(
    pob: &Path,
    ours: &Path,
    rec: &Path,
) -> (BTreeMap<String, usize>, usize, Vec<String>) {
    let hp = HitsIndex::load(&pob.join("hits.json")).unwrap();
    let ho = HitsIndex::load(&ours.join("hits.json")).unwrap();
    let theirs: BTreeMap<&str, &str> = ho
        .dumps
        .iter()
        .map(|d| (d.hit.as_str(), d.file.as_str()))
        .collect();
    let out = pob.with_extension("diff.json");
    let (mut kinds, mut n, mut lines) = (BTreeMap::new(), 0usize, Vec::new());
    for d in &hp.dumps {
        let Some(f) = theirs.get(d.hit.as_str()) else {
            continue;
        };
        n += 1;
        let o = Command::new(env!("CARGO_BIN_EXE_pob_calc_diff"))
            .arg("state")
            .arg(pob.join(&d.file))
            .arg(ours.join(f))
            .arg("--nondet")
            .arg(rec)
            .arg("--json-out")
            .arg(&out)
            .output()
            .unwrap();
        assert!(o.status.success(), "{}", String::from_utf8_lossy(&o.stderr));
        let doc: J = serde_json::from_str(&std::fs::read_to_string(&out).unwrap()).unwrap();
        for m in doc["mismatches"].as_array().unwrap() {
            let kind = m["kind"].as_str().unwrap().to_owned();
            if kind != "LUA-ORDER" && lines.len() < 10 {
                lines.push(format!("{} {} {kind}", d.hit, m["path"]));
            }
            *kinds.entry(kind).or_default() += 1;
        }
    }
    (kinds, n, lines)
}

/// The state diff accepts everything PoB itself produced: PoB run 2
/// against PoB run 1 (and back) with the record of exactly those runs has
/// no mismatch (§9.2 / §9.5). allflame_exp_019 resolves a skill's gem to
/// different variants in different processes (ExplosiveTrap /
/// ExplosiveTrapAltX for one uuid), so its mod stores differ beyond order.
#[test]
fn state_diff_accepts_pob_against_itself() {
    const STEM: &str = "allflame_exp_019";
    let Some(build) = build_xml(STEM) else {
        return; // POB_ALLOW_SKIP=1
    };
    let p = patched();
    let work = scratch("self_diff");
    let first = work.join("run1");
    probe(&build, &first, &p.src, &[]);
    // a second process that differs in values from the first (whether a
    // process picks the AltX variant is a per-process coin toss): up to 5
    let mut second = PathBuf::new();
    let mut rec = NondetRecord::default();
    for i in 2..=6 {
        second = work.join(format!("run{i}"));
        probe(&build, &second, &p.src, &[]);
        rec = compare_runs(STEM, &[&first, &second]).unwrap();
        if !rec.states.is_empty() {
            break;
        }
    }
    let runs = [first, second];
    eprintln!(
        "{} vs {}: {} hits with value differences",
        runs[0].display(),
        runs[1].display(),
        rec.states.len()
    );
    let rec_path = work.join("rec.json");
    rec.save(&rec_path).unwrap();
    // record paths are those the state diff walks (no `root.` prefix)
    for paths in rec.state_orders.values() {
        assert!(paths.iter().all(|p| !p.starts_with("root")), "{paths:?}");
    }
    // the store-level entries the gem-variant difference leaves are keyed
    // where diff_store looks (per store name), not per mod position
    for vals in rec.states.values() {
        let positional: Vec<&String> = vals
            .keys()
            .filter(|k| k.contains(".mods[") && k.ends_with(".name"))
            .collect();
        assert!(positional.is_empty(), "{positional:?}");
    }
    eprintln!("record: {} entries", rec.len());
    for (a, b) in [(&runs[1], &runs[0]), (&runs[0], &runs[1])] {
        let (kinds, n, lines) = state_diff_runs(a, b, &rec_path);
        eprintln!("{} vs {}: {n} hits, {kinds:?}", a.display(), b.display());
        assert!(n > 10, "{n} hits in common");
        assert!(kinds.is_empty(), "{kinds:?} {lines:#?}");
    }
    // without the record the same diff does see PoB's differences
    // (whenever this pair of processes produced any)
    if !rec.states.is_empty() || !rec.state_orders.is_empty() {
        let none = work.join("empty.json");
        NondetRecord::new(STEM).save(&none).unwrap();
        let (kinds, _, _) = state_diff_runs(&runs[1], &runs[0], &none);
        assert!(!kinds.is_empty());
    }
}

// ---- pob_calc_diff -> report.py ---------------------------------------------------------------

#[test]
fn calc_diff_is_accepted_by_report_py() {
    let Some(parity) = require(pob_testpaths::parity_dir(), ".parity (build corpus)") else {
        return; // POB_ALLOW_SKIP=1
    };
    let pob = parity.join("oracle").join(BUILD).join("outputs.json");
    let Some(pob) = require(
        Some(pob.clone()).filter(|p| p.is_file()),
        &format!("{} (scripts/parity/oracle_dump.sh)", pob.display()),
    ) else {
        return; // POB_ALLOW_SKIP=1
    };
    let dir = scratch("calc_diff");
    // "ours": PoB's outputs as a plain object, with three perturbations
    let mut ours =
        flat_outputs(&serde_json::from_str(&std::fs::read_to_string(&pob).unwrap()).unwrap());
    let life = ours["Life"].as_f64().unwrap();
    ours.insert("Life".into(), serde_json::json!(life * 2.0));
    ours.insert("TotalDPS".into(), serde_json::json!({"$num": "inf"}));
    ours.remove("Mana");
    let ours_dir = dir.join("ours");
    std::fs::create_dir_all(&ours_dir).unwrap();
    std::fs::write(
        ours_dir.join(format!("{BUILD}.json")),
        serde_json::to_string(&ours.into_iter().collect::<serde_json::Map<String, J>>()).unwrap(),
    )
    .unwrap();
    let pob_dir = dir.join("pob").join(BUILD);
    std::fs::create_dir_all(&pob_dir).unwrap();
    std::fs::copy(&pob, pob_dir.join("outputs.json")).unwrap();
    let results = dir.join("results");
    let out = Command::new(env!("CARGO_BIN_EXE_pob_calc_diff"))
        .arg("outputs")
        .arg(dir.join("pob"))
        .arg(&ours_dir)
        .arg("--json-out")
        .arg(&results)
        .arg("--nondet")
        .arg(dir.join("no-nondet"))
        .output()
        .unwrap();
    let text = String::from_utf8_lossy(&out.stdout);
    eprintln!("{text}");
    assert!(out.status.success());
    assert!(text.contains("2 mismatches, 1 PoB keys missing"), "{text}");
    let doc: J = serde_json::from_str(
        &std::fs::read_to_string(results.join(format!("{BUILD}.json"))).unwrap(),
    )
    .unwrap();
    assert_eq!(doc["build"], BUILD);
    assert!(doc["pob"].is_object() && doc["ours"].is_object());
    let kinds: BTreeMap<&str, usize> =
        doc["mismatches"]
            .as_array()
            .unwrap()
            .iter()
            .fold(BTreeMap::new(), |mut m, x| {
                *m.entry(x["path"].as_str().unwrap()).or_default() += 1;
                m
            });
    assert_eq!(
        kinds.keys().copied().collect::<Vec<_>>(),
        ["Life", "TotalDPS"]
    );

    // --unit: keep the mismatches whose PoB writer lies in the unit's ranges
    // (TotalDPS is written in CalcOffence's pass body B = OF-F, Life in
    // CalcPerform's helpers = PF-A)
    let prov = parity.join("probe").join(BUILD).join("provenance.json");
    if prov.is_file() {
        let pdir = dir.join("prov").join(BUILD);
        std::fs::create_dir_all(&pdir).unwrap();
        std::fs::copy(&prov, pdir.join("provenance.json")).unwrap();
        let doc_md =
            pob_testpaths::find("docs/port/calc-architecture.md").expect("calc-architecture.md");
        for (unit, kept) in [("OF-F", "TotalDPS"), ("PF-A", "Life")] {
            let unit_dir = dir.join(format!("unit-{unit}"));
            let o = Command::new(env!("CARGO_BIN_EXE_pob_calc_diff"))
                .args(["outputs"])
                .arg(dir.join("pob"))
                .arg(&ours_dir)
                .args(["--unit", unit, "--doc"])
                .arg(&doc_md)
                .arg("--provenance")
                .arg(dir.join("prov"))
                .arg("--nondet")
                .arg(dir.join("no-nondet"))
                .arg("--json-out")
                .arg(&unit_dir)
                .output()
                .unwrap();
            assert!(o.status.success(), "{}", String::from_utf8_lossy(&o.stderr));
            let d: J = serde_json::from_str(
                &std::fs::read_to_string(unit_dir.join(format!("{BUILD}.json"))).unwrap(),
            )
            .unwrap();
            let paths: Vec<&str> = d["mismatches"]
                .as_array()
                .unwrap()
                .iter()
                .map(|m| m["path"].as_str().unwrap())
                .collect();
            assert_eq!(paths, [kept], "{unit}");
            assert_eq!(d["summary"]["filtered"], 1);
            assert!(d["mismatches"][0]["writer"]
                .as_str()
                .is_some_and(|w| w.contains(".lua:")));
        }
    } else {
        // The --unit check only (POB_ALLOW_SKIP=1); the rest of the test runs.
        let _ = require(
            None::<()>,
            &format!("--unit: {} (run oracle.sh corpus)", prov.display()),
        );
    }

    let script = pob_testpaths::find("scripts/parity/report.py").expect("report.py");
    let rep = Command::new("python3").arg(&script).arg(&results).output();
    let Some(rep) = require(rep.ok(), "python3 (for scripts/parity/report.py)") else {
        return; // POB_ALLOW_SKIP=1
    };
    let rtext = String::from_utf8_lossy(&rep.stdout);
    eprintln!("{rtext}{}", String::from_utf8_lossy(&rep.stderr));
    assert!(rep.status.success());
    assert!(rtext.contains("== 1 builds =="));
    assert!(
        rtext.contains("Life                         0/1"),
        "{rtext}"
    );
    assert!(
        rtext.contains("TotalDPS                     0/1"),
        "{rtext}"
    );
    let rk = Command::new("python3")
        .arg(&script)
        .arg(&results)
        .arg("--key")
        .arg("Life")
        .output()
        .unwrap();
    assert!(rk.status.success());
    assert!(String::from_utf8_lossy(&rk.stdout).contains("<--"));
}

// ---- the whole corpus (ignored: reads the results of oracle.sh) ---------------------------------

/// `scripts/port/oracle.sh corpus --stages crates/pob-oracle/corpus/P3/stages.toml`
/// then `scripts/port/oracle.sh nondet --verify --stages crates/pob-oracle/corpus/P3/stages.toml`,
/// then `cargo test -p pob-oracle --test P3 -- --ignored all57`.
#[test]
#[ignore = "needs the oracle.sh corpus and nondet runs over .parity/builds"]
fn all57_corpus() {
    let parity = pob_testpaths::parity_dir().expect(".parity");
    let mut builds: Vec<String> = std::fs::read_dir(parity.join("builds"))
        .unwrap()
        .filter_map(Result::ok)
        .filter_map(|e| {
            e.file_name()
                .to_string_lossy()
                .strip_suffix(".xml")
                .map(str::to_owned)
        })
        .collect();
    builds.sort();
    let (mut keys, mut covered, mut dumps, mut fn_dumps) = (0usize, 0usize, 0usize, 0usize);
    let mut failures = Vec::new();
    let mut differ = Vec::new();
    let mut no_main_offence: Vec<String> = Vec::new();
    let mut nondet_entries = 0usize;
    let mut nondet_builds = 0usize;
    for stem in &builds {
        let dir = parity.join("probe").join(stem);
        let Ok(hits) = HitsIndex::load(&dir.join("hits.json")) else {
            failures.push(format!("{stem}: no probe run"));
            continue;
        };
        for d in &hits.dumps {
            let s = StateDump::load(&dir.join(&d.file)).unwrap_or_else(|e| panic!("{stem}: {e}"));
            // pob-state/1 from dumps made before FUP3, /2 since (`schema::STATE_FORMATS`)
            assert!(pob_oracle::schema::STATE_FORMATS.contains(&s.format.as_str()), "{}", s.format);
            dumps += 1;
            if s.kind != "line" {
                fn_dumps += 1;
            }
        }
        // the env-level function probes in the build's final main env
        // (offence only runs there when the main env has a main skill:
        // upstream_Dual_Savior's has none)
        let main = hits.main_env.as_deref();
        let in_main = |id: &str| {
            hits.dumps.iter().any(|d| {
                d.hit.rsplit_once('#').is_some_and(|(p, _)| p == id) && d.env.as_deref() == main
            })
        };
        for id in ["initEnv:out", "perform:in", "perform:out", "defence:out"] {
            if main.is_none() || !in_main(id) {
                failures.push(format!("{stem}: no {id} dump in the main env"));
            }
        }
        if !in_main("offence:out") {
            no_main_offence.push(stem.clone());
        }
        let outputs = load_outputs(&dir.join("outputs.json")).unwrap();
        let prov = Provenance::load(&dir.join("provenance.json")).unwrap();
        let c = coverage(&outputs, &prov);
        keys += c.keys;
        covered += c.covered;
        if c.ratio() < 0.99 {
            failures.push(format!(
                "{stem}: provenance {:.2}% {:?}",
                100.0 * c.ratio(),
                &c.uncovered[..c.uncovered.len().min(5)]
            ));
        }
        // the probed PoB against the unpatched oracle dump (where present),
        // accepting PoB's own nondeterminism
        let nd = NondetRecord::load(&parity.join("nondet").join(format!("{stem}.json"))).ok();
        if let Some(n) = &nd {
            nondet_entries += n.len();
            nondet_builds += usize::from(!n.is_empty());
        } else {
            failures.push(format!("{stem}: no nondet record"));
        }
        let plain = parity.join("oracle").join(stem).join("outputs.json");
        if plain.is_file() {
            let pa = flat_outputs(
                &serde_json::from_str(&std::fs::read_to_string(&plain).unwrap()).unwrap(),
            );
            let pb = flat_outputs(
                &serde_json::from_str(&std::fs::read_to_string(dir.join("outputs.json")).unwrap())
                    .unwrap(),
            );
            let opts = DiffOptions {
                rel: 0.0,
                nondet_values: nd.as_ref().map(|n| &n.outputs),
                nondet_orders: nd.as_ref().map(|n| &n.output_orders),
            };
            let d = diff_flat(&pa, &pb, &opts, None, None);
            if !d.mismatches.is_empty() || d.missing > 0 || d.extra > 0 {
                differ.push(format!(
                    "{stem}: {} mismatches {:?}, {} missing, {} extra",
                    d.mismatches.len(),
                    d.mismatches
                        .iter()
                        .take(3)
                        .map(|m| &m.path)
                        .collect::<Vec<_>>(),
                    d.missing,
                    d.extra
                ));
            }
        }
    }
    eprintln!(
        "{} builds, {dumps} dumps ({fn_dumps} function-probe), provenance {covered}/{keys} = {:.3}%, nondet: {nondet_builds} builds with {nondet_entries} entries",
        builds.len(),
        100.0 * covered as f64 / keys.max(1) as f64
    );
    eprintln!("main env without offence: {no_main_offence:?}");
    // the probed PoB computes what the unpatched PoB computes
    assert!(differ.is_empty(), "patched vs plain: {differ:#?}");
    assert!(failures.is_empty(), "{failures:#?}");
    assert!(covered as f64 >= 0.99 * keys as f64);
}
