//! Unit SK-B acceptance (calc doc §11.1 row SK-B, §8 "Skill hooks"): the skill hooks of
//! Data/Skills act_dex.lua, act_str.lua, other.lua, minion.lua, spectre.lua and sup_str.lua
//! (`pob_calc::data_fns::{skills_act_dex, skills_act_str, skills_misc}`) against PoB's own
//! closures under LuaJIT (the oracle, `jit.off()`).
//!
//! * `skb_rows_match_closures`: the three files' `ROWS` are exactly the closures.json
//!   skill-hook ids whose source is not act_int.lua (SK-A's), with the same sha256, and no
//!   body is a stub (`registry_complete` for SK-B's ids).
//! * `skb_hooks_match_oracle` (needs LuaJIT and the machine-wide oracle lock): per hook, the
//!   `skill:<skill>:<hook>:in#n` dump PoB wrote on entry to the closure is injected
//!   (`pob_oracle::inject`), the Rust body from the registry (`data_fns::SKILL_FUNCS`, the path
//!   `runSkillFunc` takes) runs on the injected `activeSkill`/`output`/`breakdown` (the
//!   `explosiveArrowFunc` also gets `globalOutput`/`globalBreakdown`/`env`), and the exported
//!   state must equal the `:out#n` dump (the env, the GlobalCache and the call's arguments;
//!   §9.2 tolerance). The builds are `corpus/SK-B/builds/*.xml` (`corpus/SK-B/gen.py`: one
//!   minimal build per skill and skill part with a weapon the skill can use); each is probed
//!   with `--select skill:*` into the shared, content-keyed probe cache
//!   (`testkit::probe_selection`; CK1, CCR-0146 §4), which keeps every env's hits: MAIN (no
//!   breakdown) and CALCS (with a breakdown). A Rust panic (`LuaError`) passes only when PoB
//!   raised too (an `:in` without its `:out`). A dump whose hook reaches another
//!   unit's `unported!` stub (ExplosiveArrow calls OF-A's `calcSkillDuration`) is reported as
//!   blocked, not compared; once that body lands it is compared like the others.
//!
//! Knobs: `SKB_BUILDS=stem1,stem2`, `SKB_VERBOSE=1` (print every mismatch). A missing input
//! skips with a message; with `POB_ORACLE_REQUIRE=1` (or `CI`) it fails instead.
#![allow(non_snake_case)]

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

use pob_calc::data_fns::{skills_act_dex, skills_act_str, skills_misc, SkillFuncImpl, SKILL_FUNCS};
use pob_calc::prelude::*;
use pob_oracle::schema::StateDump;
use pob_oracle::testkit;
use serde_json::Value as J;

fn skip(why: &str) {
    let required = std::env::var_os("POB_ORACLE_REQUIRE").is_some_and(|v| v == "1")
        || std::env::var_os("CI").is_some_and(|v| !v.is_empty() && v != "false" && v != "0");
    assert!(!required, "required input missing: {why}");
    eprintln!("SKIPPED: {why}");
}

fn manifest_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

/// SK-B's registry rows.
fn rows() -> impl Iterator<Item = &'static pob_calc::data_fns::FnRow<SkillFuncImpl>> {
    skills_act_dex::ROWS
        .iter()
        .chain(skills_act_str::ROWS.iter())
        .chain(skills_misc::ROWS.iter())
}

#[test]
fn skb_rows_match_closures() {
    let Some(dump) = pob_testpaths::dump_dir() else {
        return skip("data/dump not found");
    };
    let text = std::fs::read_to_string(dump.join("closures.json")).expect("closures.json");
    let j: J = serde_json::from_str(&text).unwrap();
    let mut want = BTreeMap::new();
    for c in j["closures"].as_array().unwrap() {
        let fam = c["family"].as_str().unwrap();
        let src = c["src"].as_str().unwrap();
        if fam.starts_with("skill-hook:") && !src.starts_with("Data/Skills/act_int.lua:") {
            want.insert(
                c["id"].as_str().unwrap().to_owned(),
                c["hash"].as_str().unwrap().to_owned(),
            );
        }
    }
    let have: BTreeMap<String, String> = rows()
        .map(|r| (r.id.to_owned(), r.hash.to_owned()))
        .collect();
    assert_eq!(have.len(), rows().count(), "duplicate ids in SK-B's ROWS");
    assert_eq!(want, have, "SK-B ROWS vs closures.json");
    assert_eq!(
        have.len(),
        65,
        "act_dex 41 + act_str 15 + other 5 + minion 2 + spectre 1 + sup_str 1"
    );
    for f in ["skills_act_dex.rs", "skills_act_str.rs", "skills_misc.rs"] {
        let src = std::fs::read_to_string(manifest_dir().join("../pob-calc-offence/src/data_fns").join(f))
            .unwrap();
        assert!(
            !src.contains("unported!("),
            "{f} still has an unported body"
        );
    }
    // the registry resolves every SK-B id to its row
    for r in rows() {
        assert!(
            SKILL_FUNCS.get(r.id).is_some_and(|x| std::ptr::eq(*x, r)),
            "{}",
            r.id
        );
    }
}

// ---- oracle ---------------------------------------------------------------------------------------

fn builds() -> Vec<PathBuf> {
    let dir = manifest_dir().join("corpus/SK-B/builds");
    let mut v: Vec<PathBuf> = std::fs::read_dir(&dir)
        .map(|r| {
            r.filter_map(Result::ok)
                .map(|e| e.path())
                .filter(|p| p.extension().is_some_and(|x| x == "xml"))
                .collect()
        })
        .unwrap_or_default();
    v.sort();
    if let Ok(list) = std::env::var("SKB_BUILDS") {
        let want: Vec<&str> = list.split(',').map(str::trim).collect();
        v.retain(|p| want.contains(&stem(p)));
    }
    v
}

fn stem(p: &Path) -> &str {
    p.file_stem().and_then(|s| s.to_str()).unwrap_or_default()
}

/// The `skill:*` dumps of a build (every env): the shared probe cache's entry of that selection
/// on the unpatched-stages copy, probed in a fresh `pob_oracle probe` process when absent
/// (`testkit::probe_selection`, `runner::SelectionCache`). CK1 (CCR-0146 §4): the entry is
/// content-keyed (the patched source's sha, which covers the patcher and the dump/probe Lua; the
/// build's BLAKE3; the selection and dump cap; the probe's Rust code), so a harness, dump-format
/// or PoB change probes again. The earlier `target/tmp/skb-probe/<stem>/` directory was reused
/// whenever it held a `hits.json`, whatever made it. Hold the result while reading the dumps.
#[cfg(feature = "luajit")]
fn probe_dumps(xml: &Path) -> Result<pob_oracle::runner::ProbeDumps, String> {
    let opts = pob_oracle::runner::ProbeOptions {
        select: vec!["skill:*".to_owned()],
        // the `pob_oracle probe` default the earlier private run used
        max_dumps: 200,
        provenance: false,
    };
    testkit::probe_selection(Path::new(env!("CARGO_BIN_EXE_pob_oracle")), xml, None, &opts)
}

/// `skill:<skill>:<hook>:in#<n>` → (`data.skills.<skill>.<hook>`, n).
fn hit_row(file: &str) -> Option<(String, String)> {
    // a cache entry's dumps are `<hit>.json.zst`, an uncached run's (`POB_QUERY_CACHE=off`) plain
    let name = file
        .strip_suffix(".json.zst")
        .or_else(|| file.strip_suffix(".json"))?;
    let rest = name.strip_prefix("skill:")?;
    let (probe, n) = rest.split_once(":in#")?;
    let (skill, hook) = probe.rsplit_once(':')?;
    Some((format!("data.skills.{skill}.{hook}"), n.to_owned()))
}

/// Prefix of a `run_hook` error that names stubs the hook reached.
const BLOCKED: &str = "blocked by unported ";

#[derive(Default)]
struct Tally {
    blocked: Vec<String>,
    dumps: usize,
    clean: usize,
    errors_both: usize,
    breakdown: usize,
    mismatched: Vec<String>,
}

fn panic_msg(p: &(dyn std::any::Any + Send)) -> String {
    p.downcast_ref::<pob_lua::LuaError>()
        .map(|e| e.0.clone())
        .or_else(|| p.downcast_ref::<String>().cloned())
        .or_else(|| p.downcast_ref::<&str>().map(|s| (*s).to_owned()))
        .unwrap_or_else(|| "panic".to_owned())
}

/// The pob-state dump of an item (`$item`) carries `id`/`name`/`modList`/… but not
/// `modSource` (Item.lua:2554, `"Item:"..(self.id or -1)..":"..self.name`), which EnemyExplode
/// matches its explode mods against (other.lua:6209-6210). Rebuild it on the injected explode
/// source exactly as BuildModList sets it; nothing else in a hook reads an item.
fn restore_item_mod_source(activeSkill: &ActiveSkillRef) {
    let Some(src) = activeSkill.activeEffect.get().srcInstance.get() else {
        return;
    };
    if let Some(item) = src.extra.get(n!("explodeSource")).obj::<Item>() {
        if item.modSource.get().is_none() {
            let id = item.id.get().unwrap_or(-1.0);
            let ms = format!("Item:{}:{}", tostring_num(id), item.name.get());
            item.modSource.set(Some(LStr::from(ms.as_str())));
        }
    }
}

/// Run one hook on an injected `:in` dump; the mismatches against `:out` (None: no `:out`).
fn run_hook(
    row_id: &str,
    din: &StateDump,
    dout: Option<&StateDump>,
    stem: &str,
) -> Result<(Vec<String>, bool), String> {
    let row = SKILL_FUNCS
        .get(row_id)
        .ok_or_else(|| format!("{row_id}: not in the registry"))?;
    let build = testkit::empty_build();
    let inj = pob_oracle::inject::inject(din, &build).map_err(|e| format!("inject: {e}"))?;
    let locals: Vec<(String, Val)> = inj.locals.clone().into_iter().collect();
    let local = |k: &str| inj.locals.get(k).cloned().unwrap_or_default();
    let activeSkill = local("activeSkill")
        .obj::<ActiveSkill>()
        .ok_or("no activeSkill local")?;
    let output = local("output").table().ok_or("no output local")?;
    restore_item_mod_source(&activeSkill);
    // a closure declared without `breakdown` received it as a vararg
    let bd_val = match local("breakdown") {
        Val::Nil => local("varargs")
            .table()
            .map(|t| t.get(1))
            .unwrap_or_default(),
        v => v,
    };
    let breakdown = bd_val.obj::<Breakdown>();
    let has_bd = breakdown.is_some();
    let stubs_before: BTreeMap<&str, u64> = pob_calc::stub::stub_report().into_iter().collect();
    let run = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| match row.f {
        SkillFuncImpl::Std(f) => f(&activeSkill, &output, breakdown.as_deref()),
        SkillFuncImpl::ExplosiveArrow(f) => {
            let globalOutput = local("globalOutput").table().expect("globalOutput local");
            let globalBreakdown = local("globalBreakdown").obj::<Breakdown>();
            let env = local("env")
                .obj::<pob_calc::env::Env>()
                .or_else(|| inj.env.clone())
                .expect("env local");
            f(
                &activeSkill,
                &output,
                &globalOutput,
                globalBreakdown.as_deref(),
                &env,
            );
        }
    }));
    // a stub reached from the hook (another unit's unported body) makes the comparison moot
    let blocked: Vec<&str> = pob_calc::stub::stub_report()
        .into_iter()
        .filter(|(k, v)| stubs_before.get(k).copied().unwrap_or(0) < *v)
        .map(|(k, _)| k)
        .collect();
    if !blocked.is_empty() {
        return Err(format!("{BLOCKED}{}", blocked.join(", ")));
    }
    match (run, dout) {
        (Err(p), None) => {
            let _ = panic_msg(&*p);
            Ok((Vec::new(), has_bd))
        }
        (Err(p), Some(_)) => Err(format!("Rust raised, PoB returned: {}", panic_msg(&*p))),
        (Ok(()), None) => Err("PoB raised (no :out dump), Rust returned".to_owned()),
        (Ok(()), Some(dout)) => {
            // the wrapper's return values (`ret = { n = 0 }`): hooks return nothing
            let mut dout = dout.clone();
            if let Some(J::Object(l)) = dout.root.get_mut("locals") {
                if let Some(ret) = l.remove("ret") {
                    if ret.get("n").and_then(J::as_f64) != Some(0.0) {
                        return Err(format!("hook returned values: {ret}"));
                    }
                }
            }
            let mm = testkit::diff_against(&dout, inj.env.as_ref(), &locals, stem);
            Ok((
                mm.into_iter()
                    .map(|m| format!("{} {:?}: pob={:?} ours={:?}", m.path, m.kind, m.pob, m.ours))
                    .collect(),
                has_bd,
            ))
        }
    }
}

#[cfg(feature = "luajit")]
#[test]
fn skb_hooks_match_oracle() {
    if !testkit::init_default_data() {
        return skip("no data dump (data/dump)");
    }
    if pob_oracle::oracle::find_pob_src().is_none() {
        return skip("no PoB checkout");
    }
    if pob_oracle::oracle::find_parity_dir().is_none() {
        return skip("no .parity directory (the probe cache)");
    }
    let builds = builds();
    if builds.is_empty() {
        return skip("no corpus/SK-B/builds/*.xml");
    }
    let verbose = std::env::var_os("SKB_VERBOSE").is_some();
    let sk_b: BTreeSet<&str> = rows().map(|r| r.id).collect();
    let mut per_hook: BTreeMap<String, Tally> = BTreeMap::new();
    let mut probe_failed = Vec::new();
    let mut failed = Vec::new();
    for xml in &builds {
        let stem = stem(xml);
        let state = match probe_dumps(xml) {
            Ok(s) => s,
            Err(e) => {
                probe_failed.push(format!("{stem}: {e}"));
                continue;
            }
        };
        let mut files: Vec<String> = std::fs::read_dir(&state)
            .map(|r| {
                r.filter_map(Result::ok)
                    .map(|e| e.file_name().to_string_lossy().into_owned())
                    .collect()
            })
            .unwrap_or_default();
        files.sort();
        for f in &files {
            let Some((row_id, n)) = hit_row(f) else {
                continue;
            };
            if !sk_b.contains(row_id.as_str()) {
                continue;
            }
            let din = match StateDump::load(&state.join(f)) {
                Ok(d) => d,
                Err(e) => {
                    failed.push(format!("{stem} {f}: {e}"));
                    continue;
                }
            };
            let out_name = f.replacen(":in#", ":out#", 1);
            let dout = state
                .join(&out_name)
                .is_file()
                .then(|| StateDump::load(&state.join(&out_name)))
                .transpose()
                .unwrap_or_else(|e| panic!("{stem} {out_name}: {e}"));
            let _ = n;
            let t = per_hook.entry(row_id.clone()).or_default();
            t.dumps += 1;
            match run_hook(&row_id, &din, dout.as_ref(), stem) {
                Ok((mm, has_bd)) => {
                    if has_bd {
                        t.breakdown += 1;
                    }
                    if dout.is_none() {
                        t.errors_both += 1;
                    }
                    if mm.is_empty() {
                        t.clean += 1;
                    } else {
                        let shown = if verbose { mm.len() } else { 3.min(mm.len()) };
                        t.mismatched.push(format!(
                            "{stem} {}: {} mismatches\n      {}",
                            din.hit,
                            mm.len(),
                            mm[..shown].join("\n      ")
                        ));
                    }
                }
                Err(e) if e.starts_with(BLOCKED) => {
                    t.blocked.push(format!("{stem} {}: {e}", din.hit))
                }
                Err(e) => t.mismatched.push(format!("{stem} {}: {e}", din.hit)),
            }
        }
    }
    let mut total = Tally::default();
    for (id, t) in &per_hook {
        eprintln!(
            "{id}: {}/{} clean, {} blocked ({} with a breakdown, {} raised in both)",
            t.clean,
            t.dumps,
            t.blocked.len(),
            t.breakdown,
            t.errors_both
        );
        for m in t.mismatched.iter().chain(&t.blocked) {
            eprintln!("    {m}");
        }
        total.dumps += t.dumps;
        total.clean += t.clean;
        total.blocked.extend(t.blocked.iter().cloned());
        total.breakdown += t.breakdown;
    }
    let uncovered: Vec<&&str> = sk_b
        .iter()
        .filter(|id| !per_hook.contains_key(**id))
        .collect();
    eprintln!(
        "SK-B oracle: {} builds, {} hooks covered of {}, {}/{} dumps clean, {} blocked by other units' stubs ({} with a breakdown)",
        builds.len(),
        per_hook.len(),
        sk_b.len(),
        total.clean,
        total.dumps,
        total.blocked.len(),
        total.breakdown
    );
    eprintln!("not covered by the corpus: {uncovered:?}");
    if !probe_failed.is_empty() {
        eprintln!("probe failed: {probe_failed:?}");
    }
    for f in &failed {
        eprintln!("failed: {f}");
    }
    assert!(total.dumps > 0, "no skill hook dumps");
    assert!(probe_failed.is_empty() && failed.is_empty());
    // every dump either reproduces PoB's :out or reached another unit's stub (reported above)
    assert_eq!(
        total.clean + total.blocked.len(),
        total.dumps,
        "some hooks do not reproduce PoB's :out"
    );
}
