//! Unit SK-A acceptance (calc doc §11.1, row SK-A): the 68 skill hooks of
//! Data/Skills/act_int.lua (`pob_calc::data_fns::skills_act_int`) against PoB's own under
//! LuaJIT (the oracle, `jit.off()`).
//!
//! * `ska_rows_cover_act_int` — `skills_act_int::ROWS` is exactly the closures.json skill hooks
//!   defined in Data/Skills/act_int.lua (`registry_complete` for SK-A's ids: every id once, the
//!   same sha256), each id resolves to its own row through `SKILL_FUNCS`, and no body is a stub.
//! * `ska_hooks_match_oracle` — per hook hit: the patched oracle wraps every skill hook in a
//!   function probe (`skill:<skillId>:<hook>:in#n` / `:out#n`, crates/pob-oracle/src/patcher.rs
//!   `add_skill_hooks`). Each `:in#n` dump is injected with the ORC-X testkit, the hook is called
//!   through the registry path `runSkillFunc` uses (`skillFunc(grantedEffect.<hook>)`) with the
//!   dumped `(activeSkill, output, breakdown)`, and the exported state (env, activeSkill, output,
//!   breakdown) is diffed against `:out#n` (calc doc §9.2 tolerance). A hit PoB raised on (no
//!   `:out#n`) must raise in Rust too.
//!
//!   The corpus is `corpus/SK-A/builds/*.xml` (`corpus/SK-A/gen_builds.py`: one build per hook
//!   skill and Calcs-tab part; main socket group in MAIN and CALCS (breakdown) mode, every part —
//!   and Spell Totem where the hook branches on `skillFlags.totem` — through `calcFullDPS`)
//!   plus every all57 build (`.parity/builds`) that socket a hook skill. Builds are probed into
//!   `target/tmp/ska-probe/<stem>/` on first use (one oracle process per build, under the
//!   machine-wide oracle lock). `SKA_BUILDS=a,b` limits the run to those stems; `SKA_VERBOSE=1`
//!   prints every mismatch.
//!
//! A missing input (data/dump, the PoB checkout) skips with a message; with
//! `POB_ORACLE_REQUIRE=1` (or `CI`) it fails instead.
#![allow(non_snake_case, clippy::pedantic)]

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

use pob_calc::data_fns::{skillFunc, skills_act_int, SkillFuncImpl, SkillHook, SKILL_FUNCS};

const OWN_SRC: &str = include_str!("../../pob-calc-offence/src/data_fns/skills_act_int.rs");
const LUA_FILE: &str = "Data/Skills/act_int.lua";

fn skip(why: &str) {
    let required = std::env::var_os("POB_ORACLE_REQUIRE").is_some_and(|v| v == "1")
        || std::env::var_os("CI").is_some_and(|v| !v.is_empty() && v != "false" && v != "0");
    assert!(!required, "required input missing: {why}");
    eprintln!("SKIPPED: {why}");
}

/// closures.json `{id → sha256}` of the act_int hooks.
fn manifest() -> Option<BTreeMap<String, String>> {
    let path = pob_oracle::oracle::find_up("data/dump/closures.json")?;
    let j: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(path).ok()?).ok()?;
    let mut out = BTreeMap::new();
    for c in j["closures"].as_array()? {
        let src = c["src"].as_str().unwrap_or_default();
        if src.starts_with(&format!("{LUA_FILE}:")) {
            assert!(
                c["family"].as_str().unwrap_or_default().starts_with("skill-hook:"),
                "{c}"
            );
            let prev = out.insert(
                c["id"].as_str().unwrap().to_owned(),
                c["hash"].as_str().unwrap().to_owned(),
            );
            assert!(prev.is_none(), "duplicate manifest id {}", c["id"]);
        }
    }
    Some(out)
}

#[test]
fn ska_rows_cover_act_int() {
    let Some(want) = manifest() else {
        return skip("no data/dump/closures.json");
    };
    assert_eq!(want.len(), 68, "act_int hook count (calc doc §8)");
    let mut have = BTreeMap::new();
    for r in skills_act_int::ROWS {
        assert!(have.insert(r.id.to_owned(), r.hash.to_owned()).is_none(), "duplicate row {}", r.id);
        let row = SKILL_FUNCS.get(r.id).unwrap_or_else(|| panic!("{} not in SKILL_FUNCS", r.id));
        assert!(std::ptr::eq(*row, r), "{} resolves to another row", r.id);
    }
    assert_eq!(have, want, "ROWS ids/hashes vs closures.json");
    let stubs: Vec<&str> = OWN_SRC
        .lines()
        .filter(|l| l.trim_start().starts_with("unported!("))
        .collect();
    assert!(stubs.is_empty(), "stub bodies left: {stubs:?}");
    eprintln!("SK-A rows: {} ids, hashes equal, no stub bodies", have.len());
}

// ---- the oracle diff ------------------------------------------------------------------------------

#[cfg(feature = "luajit")]
mod oracle {
    use super::*;
    use pob_calc::prelude::*;
    use pob_oracle::schema::StateDump;
    use pob_oracle::testkit;

    fn stem(p: &Path) -> &str {
        p.file_stem().and_then(|s| s.to_str()).unwrap_or_default()
    }

    /// `skill:<skillId>:<hook>` → closures id `data.skills.<skillId>.<hook>`.
    fn hook_id(probe: &str) -> Option<String> {
        let rest = probe.strip_prefix("skill:")?;
        let (skill, hook) = rest.split_once(':')?;
        Some(format!("data.skills.{skill}.{hook}"))
    }

    fn hook_of(name: &str) -> SkillHook {
        match name {
            "initialFunc" => SkillHook::initialFunc,
            "preSkillTypeFunc" => SkillHook::preSkillTypeFunc,
            "preDamageFunc" => SkillHook::preDamageFunc,
            "postCritFunc" => SkillHook::postCritFunc,
            "preDotFunc" => SkillHook::preDotFunc,
            "explosiveArrowFunc" => SkillHook::explosiveArrowFunc,
            other => panic!("unknown hook {other}"),
        }
    }

    /// The corpus: corpus/SK-A/builds plus the all57 builds that socket a hook skill.
    fn corpus(skills: &BTreeSet<String>) -> Vec<PathBuf> {
        let mut v: Vec<PathBuf> = Vec::new();
        let own = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("corpus/SK-A/builds");
        let mut add_dir = |dir: &Path, filter: bool| {
            let Ok(rd) = std::fs::read_dir(dir) else { return };
            let mut files: Vec<PathBuf> = rd
                .filter_map(Result::ok)
                .map(|e| e.path())
                .filter(|p| p.extension().is_some_and(|x| x == "xml"))
                .collect();
            files.sort();
            for f in files {
                if filter {
                    let text = std::fs::read_to_string(&f).unwrap_or_default();
                    if !skills
                        .iter()
                        .any(|s| text.contains(&format!("skillId=\"{s}\"")))
                    {
                        continue;
                    }
                }
                v.push(f);
            }
        };
        add_dir(&own, false);
        if let Some(parity) = pob_oracle::oracle::find_parity_dir() {
            add_dir(&parity.join("builds"), true);
        }
        if let Ok(list) = std::env::var("SKA_BUILDS") {
            let want: Vec<&str> = list.split(',').map(str::trim).collect();
            v.retain(|p| want.contains(&stem(p)));
        }
        v
    }

    /// The build's hook dumps (`skill:<S>:*` for every act_int hook skill): the shared probe
    /// cache's entry of that selection on the unpatched-stages copy, probed in a fresh
    /// `pob_oracle probe` process when absent (TP1: `testkit::probe_selection`, keyed by the
    /// patched source, the build's bytes and the selection, shared by every worktree). Hold the
    /// result while reading the dumps.
    fn probe(xml: &Path, skills: &BTreeSet<String>) -> Result<pob_oracle::runner::ProbeDumps, String> {
        let opts = pob_oracle::runner::ProbeOptions {
            select: skills.iter().map(|s| format!("skill:{s}:*")).collect(),
            max_dumps: 100_000,
            provenance: false,
        };
        pob_oracle::testkit::probe_selection(
            Path::new(env!("CARGO_BIN_EXE_pob_oracle")),
            xml,
            None,
            &opts,
        )
    }

    enum Outcome {
        Clean,
        Mismatch(Vec<String>),
        Failed(String),
    }

    fn catch_lua<R>(f: impl FnOnce() -> R) -> Result<R, String> {
        match std::panic::catch_unwind(std::panic::AssertUnwindSafe(f)) {
            Ok(r) => Ok(r),
            Err(p) => Err(p
                .downcast_ref::<LuaError>()
                .map(|e| e.0.clone())
                .or_else(|| p.downcast_ref::<String>().cloned())
                .or_else(|| p.downcast_ref::<&str>().map(|s| (*s).to_owned()))
                .unwrap_or_else(|| "panic".to_owned())),
        }
    }

    /// Inject `din`, run the hook, diff against `dout` (`None`: PoB raised).
    fn run_hit(stem: &str, id: &str, din: &StateDump, dout: Option<&StateDump>) -> Outcome {
        let build = testkit::empty_build();
        let inj = match pob_oracle::inject::inject(din, &build) {
            Ok(i) => i,
            Err(e) => return Outcome::Failed(format!("inject: {e}")),
        };
        let locals: Vec<(String, Val)> = inj.locals.into_iter().collect();
        let get = |k: &str| {
            locals
                .iter()
                .find(|(n, _)| n == k)
                .map(|(_, v)| v.clone())
                .unwrap_or(Val::Nil)
        };
        let Some(activeSkill) = get("activeSkill").obj::<ActiveSkill>() else {
            return Outcome::Failed("no activeSkill local".into());
        };
        let Some(output) = get("output").table() else {
            return Outcome::Failed("no output local".into());
        };
        // `function(activeSkill, output)` hooks get the breakdown as their first vararg
        let bd_val = match get("breakdown") {
            Val::Nil => get("varargs").table().map_or(Val::Nil, |t| t.get(1)),
            v => v,
        };
        let breakdown = bd_val.obj::<Breakdown>();
        if bd_val.truthy() && breakdown.is_none() {
            return Outcome::Failed(format!("breakdown local is a {}", bd_val.lua_type()));
        }
        let hook = hook_of(id.rsplit('.').next().unwrap());
        let ge = activeSkill.activeEffect.get().grantedEffect.get();
        let Some(r) = hook.of(&ge) else {
            return Outcome::Failed(format!("{} has no {hook:?}", ge.id));
        };
        if r.id.as_str() != id {
            return Outcome::Failed(format!("grantedEffect hook is {}, probe is {id}", r.id));
        }
        let SkillFuncImpl::Std(f) = skillFunc(r) else {
            return Outcome::Failed("explosiveArrow-shaped row".into());
        };
        let ran = catch_lua(|| f(&activeSkill, &output, breakdown.as_deref()));
        match (ran, dout) {
            (Err(_), None) => Outcome::Clean, // both raise
            (Err(e), Some(_)) => Outcome::Failed(format!("Rust raised, PoB did not: {e}")),
            (Ok(()), None) => Outcome::Failed("PoB raised (no :out dump), Rust did not".into()),
            (Ok(()), Some(dout)) => {
                // the function probe's `:out` records the packed return values (`ret = { n = 0 }`:
                // every hook returns nothing)
                let ret = Table::new();
                ret.set(n!("n"), 0.0);
                let mut locals = locals.clone();
                locals.push(("ret".to_owned(), Val::Table(ret)));
                let mm = testkit::diff_against(dout, inj.env.as_ref(), &locals, stem);
                if mm.is_empty() {
                    Outcome::Clean
                } else {
                    Outcome::Mismatch(
                        mm.iter()
                            .map(|m| format!("{} {:?}: pob={:?} ours={:?}", m.path, m.kind, m.pob, m.ours))
                            .collect(),
                    )
                }
            }
        }
    }

    #[derive(Default)]
    struct PerHook {
        hits: u64,
        clean: u64,
        breakdown_hits: u64,
        builds: BTreeSet<String>,
    }

    #[test]
    fn ska_hooks_match_oracle() {
        if !testkit::init_default_data() {
            return skip("no data dump (data/dump)");
        }
        let Some(want) = manifest() else {
            return skip("no data/dump/closures.json");
        };
        let Some(src) = pob_oracle::oracle::find_pob_src() else {
            return skip("no PoB checkout");
        };
        let skills: BTreeSet<String> = want
            .keys()
            .map(|id| id.split('.').nth(2).unwrap().to_owned())
            .collect();
        let builds = corpus(&skills);
        assert!(!builds.is_empty(), "no SK-A corpus builds");
        let _ = &src;
        let verbose = std::env::var_os("SKA_VERBOSE").is_some();
        let mut per: BTreeMap<String, PerHook> = BTreeMap::new();
        let mut failed: Vec<String> = Vec::new();
        let mut mism: Vec<String> = Vec::new();
        let mut pairs = 0u64;
        let mut lua_errors = 0u64;
        for xml in &builds {
            let stem = stem(xml).to_owned();
            let state = match probe(xml, &skills) {
                Ok(s) => s,
                Err(e) => {
                    failed.push(format!("{stem}: {e}"));
                    continue;
                }
            };
            let mut ins: Vec<(String, u64, PathBuf)> = Vec::new();
            for e in std::fs::read_dir(&state).into_iter().flatten().filter_map(Result::ok) {
                let name = e.file_name().to_string_lossy().into_owned();
                let Some(base) = name.strip_suffix(".json").or_else(|| name.strip_suffix(".json.zst")) else {
                    continue;
                };
                let Some((probe_in, n)) = base.split_once('#') else { continue };
                let Some(probe) = probe_in.strip_suffix(":in") else { continue };
                let (Some(id), Ok(n)) = (hook_id(probe), n.parse::<u64>()) else { continue };
                if want.contains_key(&id) {
                    ins.push((id, n, e.path()));
                }
            }
            ins.sort();
            for (id, n, pin) in ins {
                let skill_probe = format!(
                    "skill:{}:{}",
                    id.split('.').nth(2).unwrap(),
                    id.rsplit('.').next().unwrap()
                );
                let dirs = vec![state.path().to_path_buf()];
                let pout = testkit::find_dump(&dirs, &format!("{skill_probe}:out#{n}"));
                let din = match StateDump::load(&pin) {
                    Ok(d) => d,
                    Err(e) => {
                        failed.push(format!("{stem} {id}#{n}: {e}"));
                        continue;
                    }
                };
                let dout = match pout.as_deref().map(StateDump::load) {
                    None => None,
                    Some(Ok(d)) => Some(d),
                    Some(Err(e)) => {
                        failed.push(format!("{stem} {id}#{n}: {e}"));
                        continue;
                    }
                };
                pairs += 1;
                if dout.is_none() {
                    lua_errors += 1;
                }
                let has_bd = din
                    .locals()
                    .is_some_and(|l| {
                        l.get("breakdown").is_some_and(|b| !b.is_null())
                            || l.get("varargs").and_then(|v| v.get("$seq")).and_then(|s| s.get(0)).is_some_and(|b| !b.is_null())
                    });
                let ph = per.entry(id.clone()).or_default();
                ph.hits += 1;
                ph.builds.insert(stem.clone());
                if has_bd {
                    ph.breakdown_hits += 1;
                }
                match run_hit(&stem, &id, &din, dout.as_ref()) {
                    Outcome::Clean => ph.clean += 1,
                    Outcome::Mismatch(m) => {
                        let shown = if verbose { m.len() } else { m.len().min(4) };
                        mism.push(format!(
                            "{stem} {id}#{n}: {} mismatches\n    {}",
                            m.len(),
                            m[..shown].join("\n    ")
                        ));
                    }
                    Outcome::Failed(e) => failed.push(format!("{stem} {id}#{n}: {e}")),
                }
            }
        }
        let clean: u64 = per.values().map(|p| p.clean).sum();
        let unhit: Vec<&String> = want.keys().filter(|id| !per.contains_key(*id)).collect();
        let no_bd: Vec<&String> = per
            .iter()
            .filter(|(_, p)| p.breakdown_hits == 0)
            .map(|(id, _)| id)
            .collect();
        eprintln!("SK-A hooks vs oracle: {} builds, {pairs} hook hits ({lua_errors} raised in PoB), {clean} clean", builds.len());
        eprintln!(
            "  hooks hit: {}/{} ; hit without a breakdown only: {}",
            per.len(),
            want.len(),
            no_bd.len()
        );
        for (id, p) in &per {
            eprintln!(
                "  {:<55} hits {:>4} clean {:>4} with-breakdown {:>3} builds {}",
                id.trim_start_matches("data.skills."),
                p.hits,
                p.clean,
                p.breakdown_hits,
                p.builds.len()
            );
        }
        if !unhit.is_empty() {
            eprintln!("  never hit: {unhit:?}");
        }
        for m in mism.iter().take(if verbose { usize::MAX } else { 30 }) {
            eprintln!("MISMATCH {m}");
        }
        for f in &failed {
            eprintln!("FAILED {f}");
        }
        assert!(failed.is_empty(), "{} hook hits failed", failed.len());
        assert!(mism.is_empty(), "{} hook hits differ from the oracle", mism.len());
        if std::env::var_os("SKA_BUILDS").is_none() {
            assert!(unhit.is_empty(), "act_int hooks the corpus never runs: {unhit:?}");
        }
    }
}
