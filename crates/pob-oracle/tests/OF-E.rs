//! Unit OF-E acceptance (calc doc §11.1 row OF-E, §9.4 L4): CalcOffence.lua
//! 2607-3345, the first half of the hit pass loop body (`crit::seg_passCrit`).
//!
//! For every build of the corpus (all57 `.parity/builds/*.xml`, slice5
//! `corpus/U0/slice5/*.xml` and the unit corpus `corpus/OF-E/*.xml`, slice5
//! builds with custom modifiers for the crit branches, see its `derive.py`) and every pass `p` of the main env's offence call
//! that has both dumps, inject `offence:2607#p`, run the Rust segment
//! (`pob_calc::probe::run_segment`) and diff the env and the ctx / pass locals
//! against `offence:3346#p` (§9.2 tolerance).
//!
//! The segment calls functions other units own (OF-A's `calcSkillCooldown`,
//! `calcSkillDuration`, `calcWarcryCastTime`, `calcAreaOfEffect`, `runSkillFunc`;
//! PF-*'s `calcs.actionSpeedMod`; the `explosiveArrowFunc` skill hook). While
//! those are `unported!` stubs a hit whose run reached one of them (with an
//! effect: `runSkillFunc` only matters when the skill has a `postCritFunc`)
//! cannot match PoB; it is counted as **blocked** and listed with the stubs it
//! reached. Hits nested in a `mirages` env whose dump carries `$foreign`
//! markers (state of another env `inject` cannot rebuild; CCR-0044 item 2, filed
//! by OF-B) are reported apart. Mid-segment callee replay is CCR-0044 too.
//! The gate: at least one hit compared, the clean hits had writes to
//! reproduce, and every other hit that reached no foreign stub matches PoB
//! exactly (`failed == 0`).
//!
//! Inputs: the probe cache (`scripts/port/oracle.sh run --build X.xml --stage
//! offence:2607@main,offence:3346@main`, plus `offence:2607#<p>,offence:3346#<p>`
//! for the off-hand pass), `data/dump`. A missing input fails unless
//! `POB_ALLOW_SKIP=1`. Knobs: `OFE_BUILDS=stem1,stem2`, `OFE_VERBOSE=1`.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use pob_calc::lua_table::Val;
use pob_oracle::schema::StateDump;
use pob_oracle::testkit;

const OWN_SITE: &str = "Modules/CalcOffence.lua:2607-3345";
const RUN_SKILL_FUNC_SITE: &str = "Modules/CalcOffence.lua:509-514";

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

fn corpus(parity: &Path) -> Vec<PathBuf> {
    let mut v = xmls(&parity.join("builds"));
    let corpora = Path::new(env!("CARGO_MANIFEST_DIR")).join("corpus");
    v.extend(xmls(&corpora.join("U0/slice5")));
    v.extend(xmls(&corpora.join("OF-E")));
    if let Ok(list) = std::env::var("OFE_BUILDS") {
        let want: Vec<&str> = list.split(',').map(str::trim).collect();
        v.retain(|p| want.contains(&stem(p)));
    }
    v
}

/// The `n` of every `offence:2607#n` dump present for a build.
fn pass_hits(dirs: &[PathBuf]) -> Vec<u64> {
    let mut ns = std::collections::BTreeSet::new();
    for d in dirs {
        let Ok(rd) = std::fs::read_dir(d) else {
            continue;
        };
        for e in rd.filter_map(Result::ok) {
            let name = e.file_name().to_string_lossy().into_owned();
            if let Some(rest) = name.strip_prefix("offence:2607#") {
                let n: String = rest.chars().take_while(char::is_ascii_digit).collect();
                if let Ok(n) = n.parse() {
                    ns.insert(n);
                }
            }
        }
    }
    ns.into_iter().collect()
}

fn stub_counts() -> BTreeMap<&'static str, u64> {
    pob_calc::stub::stub_report().into_iter().collect()
}

/// CCR-0036 (filed by OF-G; OF-D's duplicate CCR-0065 is superseded by it): `inject` decodes every local as `Kind::Any`, so the
/// `passList` array arrives as a plain table, which the `Shared<Vec<Table>>` ctx
/// field rejects. Rebuild it from the same pass tables (identity kept).
fn pass_list((k, v): (String, Val)) -> (String, Val) {
    use pob_calc::probe::ProbeField;
    if k != "passList" {
        return (k, v);
    }
    let Val::Table(t) = &v else { return (k, v) };
    let mut list = Vec::new();
    for (_, p) in t.ipairs() {
        match p {
            Val::Table(p) => list.push(p),
            _ => return (k, v),
        }
    }
    let v = pob_lua::Shared::new(list).to_val();
    (k, v)
}

/// The hit runs inside another env's call (`mirages` builds and performs an
/// env of its own) and its dump holds `$foreign` markers.
fn foreign_env(d: &StateDump) -> bool {
    d.stack.iter().any(|h| h.starts_with("mirages:in"))
        && d.root.to_string().contains("\"$foreign\"")
}

enum Outcome {
    /// Matches PoB; `n` = mismatches against `:out` of the state right after
    /// 2607-2613 (the writes the segment body had to reproduce).
    Clean(usize),
    /// Mismatches, and the run reached these foreign stubs.
    Blocked(Vec<String>, Vec<String>),
    /// Mismatches with no foreign stub reached, in a hit nested in another
    /// env's call (`mirages:in` on the stack) whose dump holds `$foreign`
    /// markers: the stores reach objects of the other env, which `inject`
    /// cannot rebuild (allflame_exp_013: the injected skillModList sums 0 for
    /// `CritChance` INC and `CritChanceCap` where PoB's sum is not), so the
    /// injected state is not PoB's. Reported, not gated.
    ForeignEnv(Vec<String>),
    /// Mismatches with no foreign stub reached (an OF-E defect).
    Failed(Vec<String>),
    /// The run raised.
    Raised(String, Vec<String>),
}

fn run_one(xml: &Path, stem: &str, din: &StateDump, dout: &StateDump) -> Outcome {
    let model = testkit::load_build(xml).unwrap_or_else(testkit::empty_build);
    let inj = match pob_oracle::inject::inject(din, &model) {
        Ok(i) => i,
        Err(e) => return Outcome::Raised(format!("inject: {e}"), Vec::new()),
    };
    let locals: Vec<(String, Val)> = inj.locals.into_iter().map(pass_list).collect();
    // does `runSkillFunc("postCritFunc")` reach a hook (CalcOffence.lua:510-513)?
    let has_post_crit = locals
        .iter()
        .find(|(k, _)| k == "activeSkill")
        .and_then(|(_, v)| v.obj::<pob_calc::active_skill::ActiveSkill>())
        .is_some_and(|s| {
            s.activeEffect
                .get()
                .grantedEffect
                .get()
                .postCritFunc
                .is_some()
        });
    // non-vacuity: what the segment body has to write. The locals as they are
    // right after CalcOffence.lua:2607-2613 (the pass locals taken from `pass`,
    // `hitRate` not yet declared) against `:out`.
    let pass = locals
        .iter()
        .find(|(k, _)| k == "pass")
        .and_then(|(_, v)| v.table());
    let mut pre_locals = locals.clone();
    if let Some(pass) = &pass {
        for k in ["source", "output", "cfg", "breakdown"] {
            let v = pass.get(k);
            match pre_locals.iter_mut().find(|(n, _)| n == k) {
                Some(slot) => slot.1 = v,
                None => pre_locals.push((k.to_owned(), v)),
            }
        }
    }
    let pre = testkit::diff_against(dout, inj.env.as_ref(), &pre_locals, stem);
    if std::env::var_os("OFE_DEBUG_PRE").is_some() {
        for m in &pre {
            eprintln!("    pre {} {:?}", m.path, m.kind);
        }
    }
    let pending = pre.iter().filter(|m| m.path != "locals.hitRate").count();
    let before = stub_counts();
    let run = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        pob_calc::probe::run_segment(&din.probe, &locals)
    }));
    let after_counts = stub_counts();
    let mut foreign: Vec<String> = after_counts
        .iter()
        .filter(|(site, n)| before.get(*site).copied().unwrap_or(0) < **n)
        .map(|(site, _)| (*site).to_owned())
        .filter(|site| site != OWN_SITE)
        .filter(|site| site != RUN_SKILL_FUNC_SITE || has_post_crit)
        .collect();
    foreign.sort();
    let after = match run {
        Ok(Ok((_flow, after))) => after,
        Ok(Err(e)) => return Outcome::Raised(e, foreign),
        Err(p) => {
            let msg = p
                .downcast_ref::<pob_lua::LuaError>()
                .map(|e| e.0.clone())
                .or_else(|| p.downcast_ref::<String>().cloned())
                .or_else(|| p.downcast_ref::<&str>().map(|s| (*s).to_owned()))
                .unwrap_or_else(|| "panic".to_owned());
            return Outcome::Raised(msg, foreign);
        }
    };
    let env = after
        .iter()
        .find(|(k, _)| k == "env")
        .and_then(|(_, v)| v.obj::<pob_calc::env::Env>())
        .or(inj.env);
    let mm: Vec<String> = testkit::diff_against(dout, env.as_ref(), &after, stem)
        .into_iter()
        .map(|m| format!("{} {:?}: pob={:?} ours={:?}", m.path, m.kind, m.pob, m.ours))
        .collect();
    if mm.is_empty() {
        Outcome::Clean(pending)
    } else if foreign.is_empty() && foreign_env(din) {
        Outcome::ForeignEnv(mm)
    } else if foreign.is_empty() {
        Outcome::Failed(mm)
    } else {
        Outcome::Blocked(foreign, mm)
    }
}

#[test]
fn ofe_l4_pass_crit() {
    let Some(_) = pob_testpaths::require(
        testkit::init_default_data().then_some(()),
        "data dump (data/dump)",
    ) else {
        return;
    };
    let Some(parity) = pob_testpaths::require(pob_testpaths::parity_dir(), ".parity") else {
        return;
    };
    let verbose = std::env::var_os("OFE_VERBOSE").is_some_and(|v| v == "1");
    let builds = corpus(&parity);
    let (mut hits, mut clean, mut clean_nontrivial, mut writes) = (0usize, 0usize, 0usize, 0usize);
    let mut blocked: Vec<String> = Vec::new();
    let mut blocked_by: BTreeMap<String, usize> = BTreeMap::new();
    let mut failed: Vec<String> = Vec::new();
    let mut foreign_envs: Vec<String> = Vec::new();
    let mut missing: Vec<String> = Vec::new();
    for xml in &builds {
        let stem = stem(xml);
        let dirs = testkit::dump_dirs(stem, Some(xml));
        let ns = pass_hits(&dirs);
        if ns.is_empty() {
            missing.push(stem.to_owned());
            continue;
        }
        for n in ns {
            let (hin, hout) = (format!("offence:2607#{n}"), format!("offence:3346#{n}"));
            let (Some(pin), Some(pout)) = (
                testkit::find_dump(&dirs, &hin),
                testkit::find_dump(&dirs, &hout),
            ) else {
                missing.push(format!("{stem} {hout}"));
                continue;
            };
            let (Ok(din), Ok(dout)) = (StateDump::load(&pin), StateDump::load(&pout)) else {
                failed.push(format!("{stem} {hin}: unreadable dump"));
                continue;
            };
            hits += 1;
            let what = format!("{stem} {hin}");
            match run_one(xml, stem, &din, &dout) {
                Outcome::Clean(pending) => {
                    if verbose {
                        eprintln!("  clean {what}: {pending} paths written");
                    }
                    clean += 1;
                    writes += pending;
                    if pending > 0 {
                        clean_nontrivial += 1;
                    }
                }
                Outcome::Blocked(stubs, mm) => {
                    for s in &stubs {
                        *blocked_by.entry(s.clone()).or_default() += 1;
                    }
                    if verbose {
                        for m in mm.iter().take(20) {
                            eprintln!("  blocked {what}: {m}");
                        }
                    }
                    blocked.push(format!("{what}: {} mismatches, stubs {stubs:?}", mm.len()));
                }
                Outcome::Raised(e, stubs) if !stubs.is_empty() => {
                    for s in &stubs {
                        *blocked_by.entry(s.clone()).or_default() += 1;
                    }
                    blocked.push(format!("{what}: raised {e}, stubs {stubs:?}"));
                }
                Outcome::Raised(e, _) => failed.push(format!("{what}: raised {e}")),
                Outcome::ForeignEnv(mm) => {
                    if verbose {
                        for m in mm.iter().take(20) {
                            eprintln!("  foreign-env {what}: {m}");
                        }
                    }
                    foreign_envs.push(format!("{what}: {} mismatches", mm.len()));
                }
                Outcome::Failed(mm) => {
                    let shown: Vec<&String> =
                        mm.iter().take(if verbose { 200 } else { 12 }).collect();
                    failed.push(format!(
                        "{what}: {} mismatches\n      {}",
                        mm.len(),
                        shown
                            .iter()
                            .map(|s| s.as_str())
                            .collect::<Vec<_>>()
                            .join("\n      ")
                    ));
                }
            }
        }
    }
    eprintln!(
        "OF-E L4 offence:2607#p -> offence:3346#p: {} builds, {hits} hits, {clean} clean, {} blocked by foreign stubs, {} in mirage envs with $foreign state, {} failed; {} builds/hits without dumps",
        builds.len(),
        blocked.len(),
        foreign_envs.len(),
        failed.len(),
        missing.len()
    );
    eprintln!(
        "  clean hits with writes to reproduce (state after 2607-2613 vs :out, hitRate aside): {clean_nontrivial} ({writes} mismatching paths before the body ran)"
    );
    for (s, n) in &blocked_by {
        eprintln!("  blocked by {s}: {n}");
    }
    for b in &blocked {
        eprintln!("  BLOCKED {b}");
    }
    for f in &foreign_envs {
        eprintln!("  FOREIGN-ENV {f}");
    }
    for m in &missing {
        eprintln!("  MISSING {m}");
    }
    for f in &failed {
        eprintln!("  FAILED {f}");
    }
    assert!(
        hits > 0,
        "no offence:2607/3346 dumps found (fill the probe cache, see the module docs)"
    );
    assert!(
        clean == 0 || clean_nontrivial > 0,
        "every clean hit was already equal to :out before the run (vacuous)"
    );
    assert!(
        failed.is_empty(),
        "{} OF-E hits differ from PoB without a foreign stub",
        failed.len()
    );
}
