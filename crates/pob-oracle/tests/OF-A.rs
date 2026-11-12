//! Unit OF-A acceptance (calc doc §11.1 row OF-A: CalcOffence.lua 1-347 helpers and the setup
//! segment 362-1036):
//!
//! * **L4** (`ofa_l4_setup`): for every `offence:362#n` dump of the unit corpus (slice5,
//!   `crates/pob-oracle/corpus/U0/slice5`) and of all57 (`.parity/builds`), inject it, run
//!   `seg_setup` and diff against the `offence:1037` dump of the same `calcs.offence` call
//!   (paired by the enclosing `offence:in#k` hit). Skill hooks (`runSkillFunc("initialFunc")`)
//!   are replayed from the recorded `runSkillFunc:in/out` dumps of the same call
//!   (the local `l4kit`, CCR-0041). A call whose skill is disabled returns at line 371
//!   (no `offence:1037` hit): the run must return `Flow::Return` with `output.CombinedDPS = 0`.
//!   Gate (§9.4 L4): 100 % on slice5, ≥ 98 % of the all57 hits. Each build runs in its own
//!   child process of this binary (`ofa_l4_child`), so memory is returned between builds.
//! * **fuzz** (`ofa_fuzz_vs_luajit`): the helpers against PoB's own local functions, extracted
//!   from CalcOffence.lua and run in a PoB booted under LuaJIT with the JIT off: `calcDamage`,
//!   `calcAilmentSourceDamage`, the area helpers (`calcAreaMod`, `calcRadius`,
//!   `calcRadiusBreakpoints`, `calcMoltenStrikeTertiaryRadius`,
//!   `setMoltenStrikeTertiaryRadiusBreakdown`, `calcAreaOfEffect`), `calcSkillCooldown`,
//!   `calcSkillDuration`, `calcs.calcTotemLife` and `calcResistForType` on random inputs; results,
//!   breakdowns and the tables the functions write are compared as canonical text.
//!
//! Dumps: `scripts/port/oracle.sh exec probe <xml> --out-dir D --cache --max-dumps 3000
//! --select 'offence:362,offence:1037,runSkillFunc:in,runSkillFunc:out'` per build (the
//! probe cache keeps them). Knobs: `OFA_BUILDS=stem1,stem2`, `OFA_VERBOSE=1`, `OFA_ONLY=a,b`
//! (fuzz checks), `OFA_FUZZ_N` (cases per check, default 1500).
#![allow(non_snake_case)]
#![allow(
    clippy::too_many_lines,
    clippy::type_complexity,
    clippy::format_push_string,
    clippy::many_single_char_names
)]

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::rc::Rc;

use pob_calc::modules::calc_offence::{helpers_setup as hs, OffenceCtx};
use pob_calc::prelude::*;
use pob_oracle::headless::{Engine, Ret};
use pob_oracle::jit::JitPob;
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
    if let Ok(list) = std::env::var("OFA_BUILDS") {
        let want: Vec<&str> = list.split(',').map(str::trim).collect();
        v.retain(|p| want.contains(&stem(p)));
    }
    v
}

fn slice5_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("corpus/U0/slice5")
}

// ---- L4 ------------------------------------------------------------------------------------

#[derive(Default)]
struct L4Tally {
    hits: usize,
    clean: usize,
    returns: usize,
    hooks_replayed: usize,
    /// hits whose `:362` state already differs from `:1037` (the segment has an effect)
    nonvacuous: usize,
    /// replayed hits that fail without the replay (the hook's delta is needed)
    hook_needed: usize,
    /// failed hits whose skill's mod-store chain ends in another env's store (`$foreign`)
    foreign: usize,
    missing: Vec<String>,
    failed: Vec<String>,
    hook_notes: Vec<String>,
}

/// `t += o` (the sums and lists the serial loop accumulated, in order; TP1).
fn l4_add(t: &mut L4Tally, o: L4Tally) {
    t.hits += o.hits;
    t.clean += o.clean;
    t.returns += o.returns;
    t.hooks_replayed += o.hooks_replayed;
    t.nonvacuous += o.nonvacuous;
    t.hook_needed += o.hook_needed;
    t.foreign += o.foreign;
    t.missing.extend(o.missing);
    t.failed.extend(o.failed);
    t.hook_notes.extend(o.hook_notes);
}

/// The innermost enclosing function-probe hit of a dump (`offence:in#4`).
fn enclosing(d: &StateDump) -> String {
    d.stack.last().cloned().unwrap_or_default()
}

fn l4_build(xml: &Path, t: &mut L4Tally) {
    let stem = stem(xml).to_owned();
    // the dumps of one probe run (the run that holds the most `offence:362` hits next to
    // `offence:1037` ones): dumps of different PoB processes do not pair (calc doc §9.5)
    let all = testkit::dump_dirs(&stem, Some(xml));
    let dirs: Vec<PathBuf> = all
        .iter()
        .filter(|d| !l4kit::hits_of(std::slice::from_ref(*d), "offence:1037").is_empty())
        .max_by_key(|d| l4kit::hits_of(std::slice::from_ref(*d), "offence:362").len())
        .map(|d| vec![d.clone()])
        .unwrap_or_default();
    let ins = l4kit::hits_of(&dirs, "offence:362");
    if ins.is_empty() {
        t.missing.push(format!("{stem}: no offence:362 dumps"));
        return;
    }
    // pair each `offence:362` call with the `offence:1037` hit of the same offence call
    let mut outs: BTreeMap<String, String> = BTreeMap::new();
    for h in l4kit::hits_of(&dirs, "offence:1037") {
        if let Some(d) = testkit::find_dump(&dirs, &h).and_then(|p| StateDump::load(&p).ok()) {
            outs.insert(enclosing(&d), h);
        }
    }
    let hooks = Rc::new(l4kit::HookCalls::new(dirs.clone()));
    let verbose = std::env::var_os("OFA_VERBOSE").is_some();
    for hin in ins {
        let Some(din) = testkit::find_dump(&dirs, &hin).and_then(|p| StateDump::load(&p).ok())
        else {
            t.failed.push(format!("{stem} {hin}: unreadable"));
            continue;
        };
        t.hits += 1;
        match outs.get(&enclosing(&din)) {
            Some(hout) => {
                let r = l4kit::segment_run_in(&dirs, xml, &hin, hout, Some(&hooks));
                t.hooks_replayed += r.hooks_replayed.len();
                if baseline_mismatches(&dirs, &din, hout, &stem) > 0 {
                    t.nonvacuous += 1;
                }
                if !r.hooks_replayed.is_empty() {
                    // the same run without recordings: the hook's delta must be missing
                    let none = Rc::new(l4kit::HookCalls::new(Vec::new()));
                    if !l4kit::segment_run_in(&dirs, xml, &hin, hout, Some(&none))
                        .mismatches
                        .is_empty()
                    {
                        t.hook_needed += 1;
                    }
                }
                t.hook_notes
                    .extend(r.hook_notes.iter().map(|n| format!("{stem} {hin}: {n}")));
                if r.mismatches.is_empty() {
                    t.clean += 1;
                } else {
                    if foreign_parent(&din) {
                        t.foreign += 1;
                    }
                    let shown = if verbose { r.mismatches.len() } else { 6 };
                    t.failed.push(format!(
                        "{stem} {hin}→{hout}: {} mismatches\n      {}",
                        r.mismatches.len(),
                        r.mismatches
                            .iter()
                            .take(shown)
                            .map(|m| clip(m))
                            .collect::<Vec<_>>()
                            .join("\n      ")
                    ));
                }
            }
            None => {
                // no `offence:1037` in this call: the skill is disabled and the segment returns
                match early_return(&din) {
                    Ok(()) => {
                        t.returns += 1;
                        t.clean += 1;
                    }
                    Err(e) => t.failed.push(format!("{stem} {hin}: {e}")),
                }
            }
        }
    }
}

/// The skill was copied from another env (`copyActiveSkill`: mirages): one of its mod stores
/// has a `$foreign` parent, which injection cannot link (a dump holds one env), so queries
/// through it miss that env's mods (docs/port/known_diffs.toml OF-A-1).
fn foreign_parent(din: &StateDump) -> bool {
    din.root.to_string().contains("\"parent\":{\"$foreign\"")
}

/// Mismatches of the injected `:362` state, the segment not run, against `:1037`.
fn baseline_mismatches(dirs: &[PathBuf], din: &StateDump, hout: &str, stem: &str) -> usize {
    let Some(dout) = testkit::find_dump(dirs, hout).and_then(|p| StateDump::load(&p).ok()) else {
        return 0;
    };
    let build = testkit::empty_build();
    let Ok(inj) = pob_oracle::inject::inject(din, &build) else {
        return 0;
    };
    let locals: Vec<(String, Val)> = inj.locals.into_iter().collect();
    testkit::diff_against(&dout, inj.env.as_ref(), &locals, stem).len()
}

/// A disabled skill (`skillFlags.disable`) returns at CalcOffence.lua:371 with `CombinedDPS = 0`.
fn early_return(din: &StateDump) -> Result<(), String> {
    let build = testkit::empty_build();
    let inj = pob_oracle::inject::inject(din, &build)?;
    let locals: Vec<(String, Val)> = inj.locals.into_iter().collect();
    let skill_disabled = locals
        .iter()
        .find(|(k, _)| k == "skillFlags")
        .and_then(|(_, v)| v.table())
        .is_some_and(|f| f.truthy(n!("disable")));
    if !skill_disabled {
        return Err("no offence:1037 dump for this call, but the skill is not disabled".into());
    }
    let (flow, after) = pob_calc::probe::run_segment("offence:362", &locals)?;
    if flow != Flow::Return {
        return Err(format!("flow {flow:?}, PoB returns at 371"));
    }
    let output = after
        .iter()
        .find(|(k, _)| k == "output")
        .and_then(|(_, v)| v.table())
        .ok_or("no output local")?;
    if !output.get(n!("CombinedDPS")).raw_eq(&Val::Num(0.0)) {
        return Err("output.CombinedDPS is not 0".into());
    }
    Ok(())
}

fn clip(s: &str) -> String {
    if s.len() > 400 && std::env::var_os("OFA_VERBOSE").is_none() {
        let mut e = 400;
        while !s.is_char_boundary(e) {
            e -= 1;
        }
        format!("{}…", &s[..e])
    } else {
        s.to_owned()
    }
}

fn l4_report(name: &str, t: &L4Tally) {
    eprintln!(
        "== OF-A L4 {name}: {} hits, {} clean ({} early returns), {} failed; {} hits where PoB's segment changes the state; \
         {} hook calls replayed ({} of those hits fail without the replay); {} builds without dumps",
        t.hits,
        t.clean,
        t.returns,
        t.failed.len(),
        t.nonvacuous,
        t.hooks_replayed,
        t.hook_needed,
        t.missing.len()
    );
    for m in &t.missing {
        eprintln!("  missing: {m}");
    }
    eprintln!(
        "   of the failed hits, {} run a skill copied from another env (`$foreign` mod-store parent; known_diffs OF-A-1)",
        t.foreign
    );
    for f in &t.failed {
        eprintln!("  FAIL {f}");
    }
    for n in t.hook_notes.iter().take(20) {
        eprintln!("  hook: {n}");
    }
}

// One child process per build: the injected envs of a run are not all freed, so one process
// running every all57 hit grows past 2 GB RSS. Each build runs in a fresh process of this test
// binary (`ofa_l4_child`, a no-op unless `OFA_L4_XML` names a build) that prints its tally as
// `OFA:` lines; the parent adds them up. Peak RSS is then one build's.

/// Separator of the fields of an `OFA:` line (not in mismatch texts).
const SEP: char = '\u{1f}';
/// Stands for a newline inside an `OFA:` field.
const NL: char = '\u{1e}';

fn tally_emit(t: &L4Tally) {
    let enc = |s: &str| s.replace('\n', &NL.to_string());
    println!(
        "OFA:T{SEP}{}{SEP}{}{SEP}{}{SEP}{}{SEP}{}{SEP}{}{SEP}{}",
        t.hits, t.clean, t.returns, t.hooks_replayed, t.nonvacuous, t.hook_needed, t.foreign
    );
    for m in &t.missing {
        println!("OFA:M{SEP}{}", enc(m));
    }
    for f in &t.failed {
        println!("OFA:F{SEP}{}", enc(f));
    }
    for n in &t.hook_notes {
        println!("OFA:N{SEP}{}", enc(n));
    }
}

/// Add the tally a child printed; false when it printed none.
fn tally_add(t: &mut L4Tally, stdout: &str) -> bool {
    let dec = |s: &str| s.replace(NL, "\n");
    let mut seen = false;
    for line in stdout.lines() {
        // the harness may print `test ofa_l4_child ... ` on the same line before the first one
        let Some(rest) = line.find("OFA:").map(|i| &line[i + 4..]) else {
            continue;
        };
        let f: Vec<&str> = rest.split(SEP).collect();
        match f.as_slice() {
            ["T", n @ ..] if n.len() == 7 => {
                let n: Vec<usize> = n.iter().map(|x| x.parse().unwrap_or(0)).collect();
                t.hits += n[0];
                t.clean += n[1];
                t.returns += n[2];
                t.hooks_replayed += n[3];
                t.nonvacuous += n[4];
                t.hook_needed += n[5];
                t.foreign += n[6];
                seen = true;
            }
            ["M", s] => t.missing.push(dec(s)),
            ["F", s] => t.failed.push(dec(s)),
            ["N", s] => t.hook_notes.push(dec(s)),
            _ => {}
        }
    }
    seen
}

/// [`l4_build`] for `xml` in a child process; its tally is added to `t`.
fn l4_build_isolated(xml: &Path, t: &mut L4Tally) {
    let stem = stem(xml).to_owned();
    let exe = match std::env::current_exe() {
        Ok(e) => e,
        Err(e) => {
            t.failed.push(format!("{stem}: no test binary path: {e}"));
            return;
        }
    };
    let out = std::process::Command::new(exe)
        .args([
            "ofa_l4_child",
            "--exact",
            "--ignored",
            "--nocapture",
            "--test-threads=1",
        ])
        .env("OFA_L4_XML", xml)
        .output();
    match out {
        Ok(o) if tally_add(t, &String::from_utf8_lossy(&o.stdout)) => {}
        Ok(o) => {
            let err = String::from_utf8_lossy(&o.stderr);
            let tail: Vec<&str> = err.lines().rev().take(5).collect();
            t.failed.push(format!(
                "{stem}: child process failed ({}): {}",
                o.status,
                tail.join(" | ")
            ));
        }
        Err(e) => t
            .failed
            .push(format!("{stem}: cannot start child process: {e}")),
    }
}

/// The child half of [`l4_build_isolated`] (a no-op unless `OFA_L4_XML` is set).
#[test]
#[ignore = "run by ofa_l4_setup, one process per build"]
fn ofa_l4_child() {
    let Some(xml) = std::env::var_os("OFA_L4_XML").map(PathBuf::from) else {
        return;
    };
    assert!(testkit::init_default_data(), "data/dump (pob-dump output)");
    std::panic::set_hook(Box::new(|_| {}));
    let mut t = L4Tally::default();
    l4_build(&xml, &mut t);
    tally_emit(&t);
}

#[test]
fn ofa_l4_setup() {
    if pob_testpaths::require(
        testkit::init_default_data().then_some(()),
        "data/dump (pob-dump output)",
    )
    .is_none()
    {
        return;
    }
    let Some(parity) = pob_testpaths::require(pob_testpaths::parity_dir(), ".parity") else {
        return;
    };
    // TP1: the child processes (one per build) run on the bounded pool (`$POB_TEST_PAR`); each
    // build's tally is parsed into its own `L4Tally` and added here in corpus order, so the
    // report and the gates are the serial ones
    let run = |dir: &Path| {
        let mut sum = L4Tally::default();
        testkit::par_for_each(
            &xmls(dir),
            |_, xml| {
                let mut t = L4Tally::default();
                l4_build_isolated(xml, &mut t);
                t
            },
            |_, _, t| l4_add(&mut sum, t),
        );
        sum
    };
    let unit = run(&slice5_dir());
    let all57 = run(&parity.join("builds"));
    l4_report("slice5 (unit corpus)", &unit);
    l4_report("all57", &all57);
    assert!(
        unit.hits > 0,
        "no offence:362 dumps for slice5 (see the module doc for the probe command)"
    );
    assert!(
        unit.clean == unit.hits && unit.failed.is_empty(),
        "OF-A L4 on slice5 is not 100%"
    );
    if all57.hits > 0 {
        let ratio = all57.clean as f64 / all57.hits as f64;
        assert!(
            ratio >= 0.98,
            "OF-A L4 on all57: {:.2}% < 98%",
            ratio * 100.0
        );
    }
}

// ---- local L4 kit (CCR-0041 §4) ----------------------------------------------------------------

/// L4 with skill-hook replay, kept local to this test (unit brief: testkit gaps are filled with
/// local helpers plus a CCR; CCR-0041 §4 proposes moving this into `pob_oracle::testkit`).
///
/// [`l4kit::segment_run_in`] injects `<stage_in>`, installs a `pob_calc::probe` skill-hook
/// handler, runs the Rust segment and diffs against `<stage_out>`. The handler answers each
/// `runSkillFunc(name)` call of the segment: when the skill's granted effect has no such hook
/// the Lua call does nothing and the Rust body runs (it does nothing either); otherwise it
/// applies the recorded `runSkillFunc:in -> :out` delta (`pob_oracle::replay::Replay`) of the
/// first not yet used recorded call with that `name` made inside the same enclosing
/// function-probe call as `<stage_in>` (the innermost entry of the dump's `stack`, e.g.
/// `offence:in#4`); calls made before `<stage_in>` are dropped when a `hits.json` of the dump
/// directory orders them. Both dumps must come from one probe run (the caller passes one run's
/// directory): PoB's state differs between processes (calc doc §9.5).
mod l4kit {
    use std::cell::{OnceCell, RefCell};
    use std::collections::{BTreeMap, BTreeSet};
    use std::path::{Path, PathBuf};
    use std::rc::Rc;

    use pob_calc::lua_table::Val;
    use pob_oracle::schema::StateDump;
    use pob_oracle::testkit::{
        diff_against, empty_build, find_dump, init_default_data, load_build,
    };
    use serde_json::Value as J;

    /// What one L4 run ([`segment_run_in`]) did.
    #[derive(Debug, Default)]
    pub struct SegmentRun {
        /// Diff lines against `<stage_out>` (empty = pass), or one line naming why the run
        /// could not be made (missing dump, inject or segment error).
        pub mismatches: Vec<String>,
        /// The `runSkillFunc:in` hits whose recorded delta was applied (CCR-0041).
        pub hooks_replayed: Vec<String>,
        /// Hook calls of a skill that has the hook but no recorded call to replay
        /// (the Rust registry body ran instead), and replay notes.
        pub hook_notes: Vec<String>,
    }

    /// L4 on the dumps of `dirs` (one probe run's directory; `hooks` indexes the same
    /// directories and is shared between the runs of one build).
    pub fn segment_run_in(
        dirs: &[PathBuf],
        build: &Path,
        stage_in: &str,
        stage_out: &str,
        hooks: Option<&Rc<HookCalls>>,
    ) -> SegmentRun {
        let fail = |m: String| SegmentRun {
            mismatches: vec![m],
            ..SegmentRun::default()
        };
        if !init_default_data() {
            return fail("no data dump (data/dump)".to_owned());
        }
        let stem = build
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or_default()
            .to_owned();
        let dirs = dirs.to_vec();
        let (Some(pin), Some(pout)) = (find_dump(&dirs, stage_in), find_dump(&dirs, stage_out))
        else {
            return fail(format!(
                "missing dump {stage_in} or {stage_out} for {stem} (fill with: scripts/port/oracle.sh run --build {} --stage {stage_in},{stage_out})",
                build.display()
            ));
        };
        let (Ok(din), Ok(dout)) = (StateDump::load(&pin), StateDump::load(&pout)) else {
            return fail(format!(
                "unreadable dump {} / {}",
                pin.display(),
                pout.display()
            ));
        };
        let model = load_build(build).unwrap_or_else(empty_build);
        let inj = match pob_oracle::inject::inject(&din, &model) {
            Ok(i) => i,
            Err(e) => return fail(format!("inject {stage_in}: {e}")),
        };
        let locals: Vec<(String, Val)> = inj.locals.into_iter().collect();
        let hooks = hooks
            .cloned()
            .unwrap_or_else(|| Rc::new(HookCalls::new(dirs.clone())));
        let report = Rc::new(RefCell::new(SegmentRun::default()));
        if let (Some(env), Some(enclosing)) = (inj.env.clone(), din.stack.last().cloned()) {
            let before = hooks.calls_before(&din.hit);
            let report = report.clone();
            let hooks = hooks.clone();
            let mut used: BTreeSet<String> = BTreeSet::new();
            let handler = move |skill: &pob_calc::active_skill::ActiveSkillRef,
                                hook: pob_calc::data_fns::SkillHook|
                  -> bool {
                let ge = skill.activeEffect.get().grantedEffect.get();
                if hook.of(&ge).is_none() {
                    return false;
                }
                let name = format!("{hook:?}");
                let next = hooks
                    .calls(&enclosing, &name)
                    .into_iter()
                    .find(|c| !used.contains(&c.hit) && !before.contains(&c.hit));
                let Some(call) = next else {
                    report.borrow_mut().hook_notes.push(format!(
                        "{name} of {}: no recorded runSkillFunc call under {enclosing}",
                        ge.name
                    ));
                    return false;
                };
                used.insert(call.hit.clone());
                match call.load() {
                    Some(c) => {
                        let hit = c.din.hit.clone();
                        let mut calls = BTreeMap::new();
                        calls.insert(hit.clone(), c);
                        let replay = pob_oracle::replay::Replay { calls };
                        match replay.apply_report(&hit, &env) {
                            Ok(a) => {
                                let mut r = report.borrow_mut();
                                r.hook_notes
                                    .extend(a.notes.into_iter().map(|n| format!("{hit}: {n}")));
                                r.hooks_replayed.push(hit);
                            }
                            Err(e) => report.borrow_mut().hook_notes.push(format!("{hit}: {e}")),
                        }
                    }
                    None => report
                        .borrow_mut()
                        .hook_notes
                        .push(format!("{}: unreadable runSkillFunc dumps", call.hit)),
                }
                true
            };
            pob_calc::probe::set_skill_hook_handler(Some(Box::new(handler)));
        }
        let run = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            pob_calc::probe::run_segment(&din.probe, &locals)
        }));
        pob_calc::probe::set_skill_hook_handler(None);
        let mut out = std::mem::take(&mut *report.borrow_mut());
        let after = match run {
            Ok(Ok((_flow, after))) => after,
            Ok(Err(e)) => {
                out.mismatches = vec![format!("run {stage_in}: {e}")];
                return out;
            }
            Err(p) => {
                let msg = p
                    .downcast_ref::<pob_lua::LuaError>()
                    .map(|e| e.0.clone())
                    .or_else(|| p.downcast_ref::<String>().cloned())
                    .or_else(|| p.downcast_ref::<&str>().map(|s| (*s).to_owned()))
                    .unwrap_or_else(|| "panic".to_owned());
                out.mismatches = vec![format!("run {stage_in}: {msg}")];
                return out;
            }
        };
        let env = after
            .iter()
            .find(|(k, _)| k == "env")
            .and_then(|(_, v)| v.obj::<pob_calc::env::Env>())
            .or(inj.env);
        out.mismatches = diff_against(&dout, env.as_ref(), &after, &stem)
            .into_iter()
            .map(|m| format!("{} {:?}: pob={:?} ours={:?}", m.path, m.kind, m.pob, m.ours))
            .collect();
        out
    }

    /// Every dumped hit of `probe` (`offence:362` → `offence:362#1`, `#2`, …) in `dirs`, by `n`.
    pub fn hits_of(dirs: &[PathBuf], probe: &str) -> Vec<String> {
        let prefix = format!(
            "{}#",
            pob_oracle::runner::hit_file_name(probe).trim_end_matches(".json")
        );
        let mut ns: BTreeSet<u64> = BTreeSet::new();
        for d in dirs {
            let Ok(rd) = std::fs::read_dir(d) else {
                continue;
            };
            for e in rd.filter_map(Result::ok) {
                let name = e.file_name().to_string_lossy().into_owned();
                let Some(rest) = name.strip_prefix(&prefix) else {
                    continue;
                };
                let n: String = rest.chars().take_while(char::is_ascii_digit).collect();
                if let Ok(n) = n.parse::<u64>() {
                    ns.insert(n);
                }
            }
        }
        ns.into_iter().map(|n| format!("{probe}#{n}")).collect()
    }

    // ---- skill hooks (CCR-0041) ----------------------------------------------------------------------

    /// One recorded `runSkillFunc` call.
    #[derive(Clone, Debug)]
    pub struct HookCall {
        /// `runSkillFunc:in#n`.
        pub hit: String,
        pub n: u64,
        /// The `name` local (`initialFunc`, `preDamageFunc`, …).
        pub name: String,
        pub din: PathBuf,
        pub dout: Option<PathBuf>,
    }

    impl HookCall {
        /// The `:in`/`:out` dumps (None when one is missing or unreadable).
        pub fn load(&self) -> Option<pob_oracle::replay::Call> {
            Some(pob_oracle::replay::Call {
                din: StateDump::load(&self.din).ok()?,
                dout: StateDump::load(self.dout.as_ref()?).ok()?,
            })
        }
    }

    /// The recorded `runSkillFunc` calls of one build's dump directories, indexed
    /// on first use by their enclosing function-probe hit (`offence:in#4`).
    pub struct HookCalls {
        dirs: Vec<PathBuf>,
        index: OnceCell<BTreeMap<String, Vec<HookCall>>>,
        order: OnceCell<Option<BTreeMap<String, usize>>>,
    }

    impl HookCalls {
        pub fn new(dirs: Vec<PathBuf>) -> Self {
            HookCalls {
                dirs,
                index: OnceCell::new(),
                order: OnceCell::new(),
            }
        }

        fn index(&self) -> &BTreeMap<String, Vec<HookCall>> {
            self.index.get_or_init(|| {
                let mut by: BTreeMap<String, Vec<HookCall>> = BTreeMap::new();
                let mut seen: BTreeSet<String> = BTreeSet::new();
                for hit in hits_of(&self.dirs, "runSkillFunc:in") {
                    if !seen.insert(hit.clone()) {
                        continue;
                    }
                    let Some(pin) = find_dump(&self.dirs, &hit) else {
                        continue;
                    };
                    let Ok(d) = StateDump::load(&pin) else {
                        continue;
                    };
                    let name = d
                        .locals()
                        .and_then(|l| l.get("name"))
                        .and_then(J::as_str)
                        .unwrap_or_default()
                        .to_owned();
                    let out_hit = hit.replacen("runSkillFunc:in#", "runSkillFunc:out#", 1);
                    let call = HookCall {
                        hit: hit.clone(),
                        n: d.n,
                        name,
                        din: pin,
                        dout: find_dump(&self.dirs, &out_hit),
                    };
                    by.entry(d.stack.last().cloned().unwrap_or_default())
                        .or_default()
                        .push(call);
                }
                for v in by.values_mut() {
                    v.sort_by_key(|c| c.n);
                }
                by
            })
        }

        /// The recorded calls named `name` inside the enclosing hit, in call order.
        pub fn calls(&self, enclosing: &str, name: &str) -> Vec<HookCall> {
            self.index()
                .get(enclosing)
                .map(|v| v.iter().filter(|c| c.name == name).cloned().collect())
                .unwrap_or_default()
        }

        /// The `runSkillFunc:in` hits a `hits.json` of the dump directories lists before `hit`
        /// (empty when no index lists `hit`).
        pub fn calls_before(&self, hit: &str) -> BTreeSet<String> {
            let order = self.order.get_or_init(|| {
                for d in &self.dirs {
                    for f in ["hits.json", "hits.json.zst"] {
                        let p = d.join(f);
                        let alt = d.parent().map(|q| q.join(f));
                        for p in std::iter::once(p).chain(alt) {
                            if let Ok(h) = pob_oracle::schema::HitsIndex::load(&p) {
                                return Some(
                                    h.dumps
                                        .iter()
                                        .enumerate()
                                        .map(|(i, e)| (e.hit.clone(), i))
                                        .collect(),
                                );
                            }
                        }
                    }
                }
                None
            });
            let Some(order) = order else {
                return BTreeSet::new();
            };
            let Some(&at) = order.get(hit) else {
                return BTreeSet::new();
            };
            order
                .iter()
                .filter(|(h, &i)| i < at && h.starts_with("runSkillFunc:in#"))
                .map(|(h, _)| h.clone())
                .collect()
        }
    }
}

// ---- fuzz vs LuaJIT -------------------------------------------------------------------------

/// splitmix64.
struct Rng(u64);
impl Rng {
    fn next(&mut self) -> u64 {
        self.0 = self.0.wrapping_add(0x9E37_79B9_7F4A_7C15);
        let mut z = self.0;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        z ^ (z >> 31)
    }
    fn below(&mut self, n: u64) -> u64 {
        self.next() % n
    }
    fn chance(&mut self, p: f64) -> bool {
        (self.next() >> 11) as f64 / (1u64 << 53) as f64 <= p
    }
    /// An integer in `lo..=hi`.
    fn int(&mut self, lo: i64, hi: i64) -> f64 {
        (lo + self.below((hi - lo + 1) as u64) as i64) as f64
    }
    /// A number in `lo..hi` with `dec` decimals.
    fn dec(&mut self, lo: f64, hi: f64, dec: i32) -> f64 {
        let f = 10f64.powi(dec);
        let n = ((hi - lo) * f) as u64;
        lo + self.below(n.max(1) + 1) as f64 / f
    }
    fn pick<'a, T>(&mut self, xs: &'a [T]) -> &'a T {
        &xs[self.below(xs.len() as u64) as usize]
    }
}

/// A Lua number literal that reads back as exactly `x` (finite).
fn lnum(x: f64) -> String {
    format!("{x:?}")
}

fn f17(x: f64) -> String {
    lfmt!("%.17g", x)
}

/// Canonical text of a value: tables as sorted `k=v` lists (function values skipped).
fn ser(v: &Val) -> String {
    match v {
        Val::Nil => "nil".into(),
        Val::Bool(b) => b.to_string(),
        Val::Num(n) => f17(*n),
        Val::Str(s) => format!("\"{s}\""),
        Val::Table(t) => ser_table(t),
        other => format!("?{}", other.lua_type()),
    }
}

fn ser_table(t: &Table) -> String {
    let mut parts: Vec<String> = Vec::new();
    for (k, v) in t.pairs() {
        let ks = match k {
            Key::S(n) => n.to_string(),
            Key::I(i) => format!("#{}", f17(i as f64)),
            Key::B(b) => format!("@{b}"),
            Key::F(f) => format!("#{}", f17(f.get())),
        };
        parts.push(format!("{ks}={}", ser(&v)));
    }
    parts.sort();
    format!("{{{}}}", parts.join(","))
}

fn ser_opt(t: Option<&Table>) -> String {
    t.map_or("nil".to_owned(), ser_table)
}

const LUA_SER: &str = r##"
local function ser(v)
  local t = type(v)
  if t == "nil" then return "nil"
  elseif t == "boolean" then return tostring(v)
  elseif t == "number" then return string.format("%.17g", v)
  elseif t == "string" then return '"' .. v .. '"'
  elseif t == "table" then
    local parts = {}
    for k, x in pairs(v) do
      if type(x) ~= "function" then
        local ks
        if type(k) == "number" then ks = "#" .. string.format("%.17g", k)
        elseif type(k) == "boolean" then ks = "@" .. tostring(k)
        else ks = tostring(k) end
        parts[#parts + 1] = ks .. "=" .. ser(x)
      end
    end
    table.sort(parts)
    return "{" .. table.concat(parts, ",") .. "}"
  end
  return "?" .. t
end
local function f17(x) if x == nil then return "nil" end return string.format("%.17g", x) end
"##;

/// The OF-A local functions of CalcOffence.lua, extracted from PoB's own source: lines 1-347
/// (module header and helpers) plus the closures `calcAreaOfEffect` (374-493, upvalue
/// `actor`) and `calcResistForType` (494-507, upvalues `env`, `enemyDB`).
fn lua_helpers(src: &Path) -> String {
    let text = std::fs::read_to_string(src.join("Modules/CalcOffence.lua"))
        .unwrap_or_else(|e| panic!("CalcOffence.lua: {e}"));
    let lines: Vec<&str> = text.lines().collect();
    let range = |a: usize, b: usize| lines[a - 1..b].join("\n");
    assert!(lines[373].contains("local function calcAreaOfEffect"));
    assert!(lines[493].contains("local function calcResistForType"));
    assert_eq!(lines[506].trim(), "end");
    format!(
        "__OFA = (function()\n{}\nlocal M = {{ calcDamage = calcDamage, calcAilmentSourceDamage = calcAilmentSourceDamage, \
         calcAreaMod = calcAreaMod, calcRadius = calcRadius, calcMoltenStrikeTertiaryRadius = calcMoltenStrikeTertiaryRadius, \
         calcRadiusBreakpoints = calcRadiusBreakpoints, setMoltenStrikeTertiaryRadiusBreakdown = setMoltenStrikeTertiaryRadiusBreakdown, \
         calcTotemLife = calcs.calcTotemLife }}\n\
         function M.makeArea(actor)\n{}\nreturn calcAreaOfEffect\nend\n\
         function M.makeResist(env, enemyDB)\n{}\nreturn calcResistForType\nend\n\
         return M\nend)()\nreturn 'ok'\n",
        range(1, 347),
        range(374, 493),
        range(494, 507)
    )
}

struct Lua(JitPob);

impl Lua {
    fn eval(&mut self, code: &str) -> String {
        let r = self.0.run(code).unwrap_or_else(|e| panic!("lua: {e}"));
        match r.first() {
            Some(Ret::Str(s)) => s.clone(),
            other => panic!("lua returned {other:?}"),
        }
    }
}

/// Lua `pcall`: a `lua_error!` becomes `"ERR"`.
fn pcall_text(f: impl FnOnce() -> String) -> String {
    match std::panic::catch_unwind(std::panic::AssertUnwindSafe(f)) {
        Ok(r) => r,
        Err(p) if p.downcast_ref::<LuaError>().is_some() => "ERR".to_owned(),
        Err(p) => std::panic::resume_unwind(p),
    }
}

/// `s` with every standalone `-0` written as `0` (CCR-0002: the oracle host is a
/// dual-number LuaJIT).
fn unsign_zero(s: &str) -> String {
    let b = s.as_bytes();
    let mut out: Vec<u8> = Vec::with_capacity(b.len());
    let mut i = 0;
    while i < b.len() {
        if b[i] == b'-'
            && b.get(i + 1) == Some(&b'0')
            && !b
                .get(i + 2)
                .is_some_and(|c| c.is_ascii_digit() || *c == b'.')
            && !(i > 0 && (b[i - 1].is_ascii_digit() || b[i - 1] == b'.'))
        {
            i += 1;
            continue;
        }
        out.push(b[i]);
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

/// One check: Lua case blocks and the Rust results, compared record by record.
struct Check {
    name: &'static str,
    lua: Vec<String>,
    rust: Vec<String>,
    what: Vec<String>,
}

impl Check {
    fn new(name: &'static str) -> Self {
        Check {
            name,
            lua: Vec::new(),
            rust: Vec::new(),
            what: Vec::new(),
        }
    }
    fn push(&mut self, what: String, lua_body: String, rust: String) {
        self.what.push(what);
        self.lua.push(lua_body);
        self.rust.push(rust);
    }
    /// Run the Lua side; → (compared, mismatches, zero-sign artefacts, errors on both sides).
    fn run(&self, lua: &mut Lua) -> (usize, Vec<String>, usize, usize) {
        let mut code = String::from(LUA_SER);
        code.push_str("local out = {}\n");
        for (i, l) in self.lua.iter().enumerate() {
            code.push_str(&format!(
                "do local ok, r = pcall(function()\n{l}\nend) out[{}] = ok and r or (\"ERR:\" .. tostring(r)) end\n",
                i + 1
            ));
        }
        code.push_str("return table.concat(out, \"\\30\")\n");
        let l = lua.eval(&code);
        let ls: Vec<&str> = l.split('\x1e').collect();
        assert_eq!(ls.len(), self.rust.len(), "{}: record count", self.name);
        let mut bad = Vec::new();
        let mut zero = 0;
        let mut errs = 0;
        for (i, (a, b)) in ls.iter().zip(&self.rust).enumerate() {
            if *a == b.as_str() {
                continue;
            }
            // a Lua error on both sides (the messages are not compared)
            if a.starts_with("ERR:") && b == "ERR" {
                errs += 1;
                continue;
            }
            // CCR-0001/0002: the oracle host is a dual-number (ARM64) LuaJIT: `-0` literals load
            // as +0, and `%d` of NaN gives 0 where x64 PoB (the port's profile) gives INT64_MIN.
            if unsign_zero(a) == unsign_zero(b)
                || (std::env::consts::ARCH == "aarch64"
                    && unsign_zero(a) == unsign_zero(&b.replace("-9223372036854775808", "0")))
            {
                zero += 1;
            } else {
                bad.push(format!(
                    "{}\n    lua  {}\n    rust {}",
                    self.what[i],
                    clip(a),
                    clip(b)
                ));
            }
        }
        (ls.len(), bad, zero, errs)
    }
}

/// A mod spec `(name, type, value, flags)` for both sides.
#[derive(Clone)]
struct M(String, &'static str, f64, u32);

fn lua_mods(var: &str, mods: &[M]) -> String {
    let mut s = String::new();
    for m in mods {
        s.push_str(&format!(
            "{var}:NewMod({:?}, {:?}, {}, \"Src\", {}, 0)\n",
            m.0,
            m.1,
            lnum(m.2),
            m.3
        ));
    }
    s
}

fn rust_mods(store: &ModStore, mods: &[M]) {
    for m in mods {
        store.NewMod(
            Name::from(m.0.as_str()),
            ModType::parse(m.1).expect("mod type"),
            m.2,
            "Src",
            m.3,
            0,
            &[],
        );
    }
}

const DMG: [&str; 5] = ["Physical", "Lightning", "Cold", "Fire", "Chaos"];

fn new_skill() -> ActiveSkillRef {
    let player = Actor::new();
    let skill = ActiveSkill::new(
        SkillEffect::new(
            pob_gamedata::data().skills[&n!("Fireball")].clone(),
            1.0,
            0.0,
        ),
        Shared::new(Vec::new()),
        Rc::downgrade(&player),
    );
    // keep the actor alive with the skill (the Weak is all the helpers read)
    std::mem::forget(player);
    skill
}

fn new_env() -> EnvRef {
    let build = testkit::empty_build();
    Env::new(
        &build,
        Mode::MAIN,
        Rc::new(Override::default()),
        Rc::new(PassiveSpec::default()),
    )
}

// -- calcDamage / calcAilmentSourceDamage

struct DmgCase {
    mods: Vec<M>,
    cfgFlags: u32,
    ctOnCfg: bool,
    /// ct[i] = (mult, [(j, conv mult)], conversion[j], gain[j]) for j > i
    ct: Vec<(f64, Vec<f64>, Vec<f64>, Vec<f64>)>,
    bases: Vec<(f64, f64)>,
    withBreakdown: bool,
    damageType: usize,
    typeFlags: u32,
    convDst: Option<usize>,
}

fn dmg_case(rng: &mut Rng) -> DmgCase {
    const NAMES: [&str; 16] = [
        "Damage",
        "PhysicalDamage",
        "FireDamage",
        "ColdDamage",
        "LightningDamage",
        "ChaosDamage",
        "ElementalDamage",
        "MinDamage",
        "MaxDamage",
        "MinFireDamage",
        "MaxFireDamage",
        "MinPhysicalDamage",
        "MaxColdDamage",
        "MinChaosDamage",
        "MaxLightningDamage",
        "MinLightningDamage",
    ];
    let flagsOf = |rng: &mut Rng| -> u32 {
        *rng.pick(&[0, 0, 0, ModFlag::Attack, ModFlag::Spell, ModFlag::Hit])
    };
    let mut mods = Vec::new();
    for _ in 0..rng.below(8) {
        let name = (*rng.pick(&NAMES)).to_owned();
        let ty = *rng.pick(&["INC", "MORE", "INC"]);
        let v = rng.int(-60, 150);
        let f = flagsOf(rng);
        mods.push(M(name, ty, v, f));
    }
    let mut ct = Vec::new();
    for i in 0..5 {
        let mult = *rng.pick(&[1.0, 1.0, 0.5, 0.0, 0.75, 0.25]);
        let mut conv = Vec::new();
        let mut conversion = Vec::new();
        let mut gain = Vec::new();
        for j in 0..5 {
            if j > i && rng.chance(0.35) {
                conv.push(*rng.pick(&[0.0, 0.1, 0.25, 0.5, 0.5, 1.0, 0.333]));
            } else {
                conv.push(0.0);
            }
            conversion.push(*rng.pick(&[0.0, 0.0, 0.1, 0.5, 1.0, 0.333]));
            gain.push(*rng.pick(&[0.0, 0.0, 0.2, 0.15]));
        }
        ct.push((mult, conv, conversion, gain));
    }
    let mut bases = Vec::new();
    for _ in 0..5 {
        if rng.chance(0.4) {
            bases.push((0.0, 0.0));
        } else {
            bases.push((rng.int(0, 400), rng.int(0, 900)));
        }
    }
    let damageType = rng.below(5) as usize;
    let convDst = if rng.chance(0.15) && damageType < 4 {
        Some(damageType + 1 + rng.below((4 - damageType) as u64) as usize)
    } else {
        None
    };
    DmgCase {
        mods,
        cfgFlags: flagsOf(rng),
        ctOnCfg: rng.chance(0.5),
        ct,
        bases,
        withBreakdown: rng.chance(0.5),
        damageType,
        typeFlags: (rng.below(32) as u32) & if rng.chance(0.5) { 0 } else { 0x1F },
        convDst,
    }
}

fn dmg_lua(c: &DmgCase, ailment: bool) -> String {
    let mut s = String::from("local ml = new(\"ModList\"):ModList()\n");
    s.push_str(&lua_mods("ml", &c.mods));
    s.push_str("local ct = {}\n");
    for (i, (mult, conv, conversion, gain)) in c.ct.iter().enumerate() {
        let mut row = format!("ct.{} = {{ mult = {}", DMG[i], lnum(*mult));
        for j in 0..5 {
            row.push_str(&format!(", {} = {}", DMG[j], lnum(conv[j])));
        }
        row.push_str(", conversion = {");
        for j in 0..5 {
            row.push_str(&format!("{} = {}, ", DMG[j], lnum(conversion[j])));
        }
        row.push_str("}, gain = {");
        for j in 0..5 {
            row.push_str(&format!("{} = {}, ", DMG[j], lnum(gain[j])));
        }
        row.push_str("} }\n");
        s.push_str(&row);
    }
    let (ctCfg, ctSkill) = if c.ctOnCfg {
        ("ct", "nil")
    } else {
        ("nil", "ct")
    };
    s.push_str(&format!(
        "local skill = {{ skillModList = ml, conversionTable = {ctSkill} }}\n\
         local cfg = {{ flags = {}, keywordFlags = 0, conversionTable = {ctCfg} }}\nlocal out = {{}}\n",
        c.cfgFlags
    ));
    for (i, (a, b)) in c.bases.iter().enumerate() {
        s.push_str(&format!(
            "out.{0}MinBase = {1} out.{0}MaxBase = {2}\n",
            DMG[i],
            lnum(*a),
            lnum(*b)
        ));
    }
    s.push_str(if c.withBreakdown {
        "local bd = { damageTypes = {} }\n"
    } else {
        "local bd = nil\n"
    });
    if ailment {
        s.push_str(&format!(
            "local a, b = __OFA.calcAilmentSourceDamage(skill, out, cfg, bd, {:?}, {})\n",
            DMG[c.damageType], c.typeFlags
        ));
    } else {
        s.push_str(&format!(
            "local a, b = __OFA.calcDamage(skill, out, cfg, bd, {:?}, {}, {})\n",
            DMG[c.damageType],
            c.typeFlags,
            c.convDst
                .map_or("nil".to_owned(), |j| format!("{:?}", DMG[j]))
        ));
    }
    s.push_str("return f17(a) .. '|' .. f17(b) .. '|' .. ser(bd)");
    s
}

fn dmg_rust(c: &DmgCase, ailment: bool) -> String {
    pcall_text(|| {
        let ml = ModStore::ModList(None);
        rust_mods(&ml, &c.mods);
        let ct = Table::new();
        for (i, (mult, conv, conversion, gain)) in c.ct.iter().enumerate() {
            let row = Table::new();
            row.set(n!("mult"), *mult);
            for j in 0..5 {
                row.set(Name::from(DMG[j]), conv[j]);
            }
            let cv = Table::new();
            let g = Table::new();
            for j in 0..5 {
                cv.set(Name::from(DMG[j]), conversion[j]);
                g.set(Name::from(DMG[j]), gain[j]);
            }
            row.set(n!("conversion"), cv);
            row.set(n!("gain"), g);
            ct.set(Name::from(DMG[i]), row);
        }
        let skill = new_skill();
        skill.skillModList.set(ml);
        let cfg = ModCfg::with_flags(c.cfgFlags, 0);
        if c.ctOnCfg {
            cfg.conversionTable.set(Some(ct.clone()));
        } else {
            skill.conversionTable.set(Some(ct.clone()));
        }
        let out = Table::new();
        for (i, (a, b)) in c.bases.iter().enumerate() {
            out.set(cat!(DMG[i], "MinBase"), *a);
            out.set(cat!(DMG[i], "MaxBase"), *b);
        }
        let bd = c.withBreakdown.then(|| tbl! { damageTypes = Table::new() });
        let (a, b) = if ailment {
            hs::calcAilmentSourceDamage(
                &skill,
                &out,
                &cfg,
                bd.as_ref(),
                Name::from(DMG[c.damageType]),
                c.typeFlags,
            )
        } else {
            hs::calcDamage(
                &skill,
                &out,
                &cfg,
                bd.as_ref(),
                Name::from(DMG[c.damageType]),
                c.typeFlags,
                c.convDst.map(|j| Name::from(DMG[j])),
            )
        };
        format!("{}|{}|{}", f17(a), f17(b), ser_opt(bd.as_ref()))
    })
}

fn check_damage(n: usize, ailment: bool) -> Check {
    let mut c = Check::new(if ailment {
        "calcAilmentSourceDamage"
    } else {
        "calcDamage"
    });
    let mut rng = Rng(if ailment { 0x0FA0_0002 } else { 0x0FA0_0001 });
    for i in 0..n {
        let case = dmg_case(&mut rng);
        c.push(
            format!("{} #{i}", c.name),
            dmg_lua(&case, ailment),
            dmg_rust(&case, ailment),
        );
    }
    c
}

// -- area helpers

fn lopt(x: Option<f64>) -> String {
    x.map_or("nil".to_owned(), lnum)
}

fn fopt(x: Option<f64>) -> String {
    x.map_or("nil".to_owned(), f17)
}

fn check_area_numeric(n: usize) -> Check {
    let mut c = Check::new("area");
    let mut rng = Rng(0x0FA0_0003);
    for i in 0..n {
        let inc = rng.dec(-1.2, 5.0, 2);
        let more = if rng.chance(0.1) {
            rng.dec(-0.5, 0.2, 2)
        } else {
            rng.dec(0.0, 4.0, 3)
        };
        let base = rng.int(0, 90);
        let radiusMod = match rng.below(3) {
            0 => None,
            1 => Some(1.0),
            _ => Some(rng.dec(0.5, 1.6, 2)),
        };
        // calcAreaMod
        c.push(
            format!("calcAreaMod #{i}"),
            format!(
                "return f17(__OFA.calcAreaMod({}, {}))",
                lnum(inc),
                lnum(more)
            ),
            f17(hs::calcAreaMod(inc, more)),
        );
        // calcRadius
        let areaMod = rng.dec(0.0, 6.0, 2);
        c.push(
            format!("calcRadius #{i}"),
            format!(
                "return f17(__OFA.calcRadius({}, {}, {}))",
                lnum(base),
                lnum(areaMod),
                lopt(radiusMod)
            ),
            f17(hs::calcRadius(base, areaMod, radiusMod)),
        );
        // calcRadiusBreakpoints
        let (a, b, cc, d) = hs::calcRadiusBreakpointsOpt(base, inc, more, radiusMod);
        // the frozen-signature wrapper (CCR-0041 §1): the same numbers, NaN for each nil
        let frozen = hs::calcRadiusBreakpoints(base, inc, more, radiusMod);
        let nan = |x: Option<f64>| x.unwrap_or(f64::NAN).to_bits();
        assert_eq!(
            [
                frozen.0.to_bits(),
                frozen.1.to_bits(),
                frozen.2.to_bits(),
                frozen.3.to_bits()
            ],
            [nan(a), nan(b), nan(cc), nan(d)],
            "calcRadiusBreakpoints wrapper #{i}"
        );
        c.push(
            format!("calcRadiusBreakpoints #{i}"),
            format!(
                "local a, b, c, d = __OFA.calcRadiusBreakpoints({}, {}, {}, {}) return f17(a)..'|'..f17(b)..'|'..f17(c)..'|'..f17(d)",
                lnum(base),
                lnum(inc),
                lnum(more),
                lopt(radiusMod)
            ),
            format!("{}|{}|{}|{}", fopt(a), fopt(b), fopt(cc), fopt(d)),
        );
        // calcMoltenStrikeTertiaryRadius
        let r = rng.int(0, 30);
        let R = r + rng.int(-5, 60);
        let am = rng.dec(-0.5, 4.0, 2);
        let sm = rng.dec(-0.5, 4.0, 2);
        c.push(
            format!("calcMoltenStrikeTertiaryRadius #{i}"),
            format!(
                "return f17(__OFA.calcMoltenStrikeTertiaryRadius({}, {}, {}, {}))",
                lnum(R),
                lnum(r),
                lnum(am),
                lnum(sm)
            ),
            f17(hs::calcMoltenStrikeTertiaryRadius(R, r, am, sm)),
        );
    }
    c
}

fn check_molten_breakdown(n: usize) -> Check {
    let mut c = Check::new("setMoltenStrikeTertiaryRadiusBreakdown");
    let mut rng = Rng(0x0FA0_0004);
    for i in 0..n {
        let r = rng.int(3, 25);
        let R = r + rng.int(8, 60);
        let (ia, ma, is, ms) = (
            rng.dec(0.6, 3.0, 2),
            rng.dec(0.6, 2.0, 2),
            rng.dec(0.6, 3.0, 2),
            rng.dec(0.6, 2.0, 2),
        );
        let label = if rng.chance(0.8) {
            Some("Chain Maximum Distance:")
        } else {
            None
        };
        let pre = rng.chance(0.2);
        c.push(
            format!("molten #{i}"),
            format!(
                "local bd = {{}} {} __OFA.setMoltenStrikeTertiaryRadiusBreakdown(bd, {}, {}, {}, {}, {}, {}, {}) return ser(bd)",
                if pre { "bd.AreaOfEffectRadiusTertiary = { \"x\" }" } else { "" },
                lnum(r),
                lnum(R),
                label.map_or("nil".to_owned(), |s| format!("{s:?}")),
                lnum(ia),
                lnum(ma),
                lnum(is),
                lnum(ms)
            ),
            pcall_text(|| {
                let bd = Table::new();
                if pre {
                    bd.set(n!("AreaOfEffectRadiusTertiary"), tbl!["x"]);
                }
                hs::setMoltenStrikeTertiaryRadiusBreakdown(&bd, r, R, label, ia, ma, is, ms);
                ser_table(&bd)
            }),
        );
    }
    c
}

// -- calcAreaOfEffect

struct AoeCase {
    mods: Vec<M>,
    weaponRange1: Option<f64>,
    weaponRange2: Option<f64>,
    flags: Vec<&'static str>,
    data: Vec<(&'static str, Val)>,
    withBreakdown: bool,
}

fn aoe_case(rng: &mut Rng) -> AoeCase {
    const NAMES: [(&str, &[&str]); 7] = [
        ("AreaOfEffect", &["INC", "MORE", "BASE"]),
        ("AreaOfEffectPrimary", &["INC", "MORE"]),
        ("AreaOfEffectSecondary", &["INC", "MORE"]),
        ("AreaOfEffectTertiary", &["INC", "MORE"]),
        ("AreaOfEffectRadius", &["INC", "MORE"]),
        ("ProjectileSpeed", &["INC", "MORE"]),
        ("AreaOfEffect", &["INC"]),
    ];
    let mut mods = Vec::new();
    for _ in 0..rng.below(7) {
        let (name, tys) = *rng.pick(&NAMES);
        let ty = *rng.pick(tys);
        let v = match ty {
            "BASE" => rng.int(-3, 8),
            "MORE" => rng.int(-40, 60),
            _ => rng.int(-50, 150),
        };
        mods.push(M(name.to_owned(), ty, v, 0));
    }
    let mut flags = Vec::new();
    for f in ["weapon1Attack", "weapon2Attack"] {
        if rng.chance(0.4) {
            flags.push(f);
        }
    }
    let mut data: Vec<(&'static str, Val)> = Vec::new();
    if rng.chance(0.85) {
        data.push(("radius", Val::Num(rng.int(4, 50))));
    }
    if rng.chance(0.2) {
        data.push(("radiusIsWeaponRange", Val::Bool(true)));
    }
    if rng.chance(0.3) {
        data.push(("radiusExtra", Val::Num(rng.int(-2, 10))));
    }
    if rng.chance(0.5) {
        data.push(("radiusLabel", Val::from("Primary:")));
    }
    let tertiary_mode = rng.below(4);
    if rng.chance(0.4) || tertiary_mode == 1 {
        data.push(("radiusSecondary", Val::Num(rng.int(5, 25))));
        if rng.chance(0.5) {
            data.push(("radiusSecondaryLabel", Val::from("Secondary:")));
        }
    }
    if rng.chance(0.5) {
        data.push(("radiusTertiary", Val::Num(rng.int(10, 60))));
        if rng.chance(0.7) {
            data.push(("radiusTertiaryLabel", Val::from("Tertiary:")));
        }
        match tertiary_mode {
            1 => data.push(("projectileSpeedAppliesToMSAreaOfEffect", Val::Bool(true))),
            2 => data.push(("radiusTertiaryBaseMargin", Val::Num(rng.int(0, 40)))),
            _ => {}
        }
    }
    AoeCase {
        mods,
        weaponRange1: rng.chance(0.8).then(|| rng.int(5, 25)),
        weaponRange2: rng.chance(0.8).then(|| rng.int(5, 25)),
        flags,
        data,
        withBreakdown: rng.chance(0.6),
    }
}

fn lval(v: &Val) -> String {
    match v {
        Val::Num(n) => lnum(*n),
        Val::Bool(b) => b.to_string(),
        Val::Str(s) => format!("{s:?}"),
        _ => "nil".into(),
    }
}

fn aoe_lua(c: &AoeCase) -> String {
    let mut s = String::from("local ml = new(\"ModList\"):ModList()\n");
    s.push_str(&lua_mods("ml", &c.mods));
    s.push_str(&format!(
        "local actor = {{ weaponRange1 = {}, weaponRange2 = {} }}\n",
        lopt(c.weaponRange1),
        lopt(c.weaponRange2)
    ));
    s.push_str("local skillFlags = {}\n");
    for f in &c.flags {
        s.push_str(&format!("skillFlags.{f} = true\n"));
    }
    s.push_str("local skillData = {}\n");
    for (k, v) in &c.data {
        s.push_str(&format!("skillData.{k} = {}\n", lval(v)));
    }
    s.push_str("local output = {}\n");
    s.push_str(if c.withBreakdown {
        "local bd = require(\"Modules.CalcBreakdown\")(ml, output, actor)\n"
    } else {
        "local bd = nil\n"
    });
    s.push_str(
        "__OFA.makeArea(actor)(ml, {}, skillData, skillFlags, output, bd)\n\
         return ser(output) .. '|' .. ser(skillData) .. '|' .. ser(skillFlags) .. '|' .. ser(bd)",
    );
    s
}

fn aoe_rust(c: &AoeCase) -> String {
    pcall_text(|| {
        let ml = ModStore::ModList(None);
        rust_mods(&ml, &c.mods);
        let actor = Actor::new();
        actor.weaponRange1.set(c.weaponRange1);
        actor.weaponRange2.set(c.weaponRange2);
        let skillFlags = Table::new();
        for f in &c.flags {
            skillFlags.set(Name::from(f), true);
        }
        let skillData = Table::new();
        for (k, v) in &c.data {
            skillData.set(Name::from(k), v.clone());
        }
        let output = Table::new();
        let bd = c.withBreakdown.then(|| newBreakdown(&ml, &output, &actor));
        let ctx = offence_ctx(&actor);
        let skillCfg = ModCfg::new();
        hs::calcAreaOfEffect(
            &ctx,
            &ml,
            &skillCfg,
            &skillData,
            &skillFlags,
            &output,
            bd.as_ref(),
        );
        format!(
            "{}|{}|{}|{}",
            ser_table(&output),
            ser_table(&skillData),
            ser_table(&skillFlags),
            ser_opt(bd.as_ref().map(|b| &b.t))
        )
    })
}

/// An `OffenceCtx` around `actor` (the closures read only `actor`, `env` and `enemyDB`).
fn offence_ctx(actor: &ActorRef) -> OffenceCtx {
    offence_ctx_with(new_env(), actor.clone(), ModStore::ModDB(None))
}

fn offence_ctx_with(env: EnvRef, actor: ActorRef, enemyDB: ModDB) -> OffenceCtx {
    let skill = new_skill();
    OffenceCtx {
        env,
        actor,
        activeSkill: skill.clone(),
        modDB: ModStore::ModDB(None),
        enemyDB,
        output: Table::new(),
        breakdown: None,
        skillModList: skill.skillModList.get(),
        skillData: skill.skillData.get(),
        skillFlags: skill.skillFlags.get(),
        skillCfg: skill.skillCfg.get(),
        isAttack: false,
        debuffDurationMult: 0.0,
        costs: Table::new(),
        monsterLife: 0.0,
        canDeal: Table::new(),
        passList: Shared::new(Vec::new()),
        storedMainHandAccuracy: None,
        storedMainHandAccuracyVsEnemy: None,
        storedSustainedTraumaBreakdown: Table::new(),
        quantityMultiplier: 0.0,
        ailmentData: None,
    }
}

fn check_aoe(n: usize) -> Check {
    let mut c = Check::new("calcAreaOfEffect");
    let mut rng = Rng(0x0FA0_0005);
    for i in 0..n {
        let case = aoe_case(&mut rng);
        c.push(format!("aoe #{i}"), aoe_lua(&case), aoe_rust(&case));
    }
    c
}

// -- cooldown / duration / totem life

fn check_cooldown(n: usize) -> Check {
    let mut c = Check::new("calcSkillCooldown");
    let mut rng = Rng(0x0FA0_0006);
    for i in 0..n {
        let mut mods = Vec::new();
        for _ in 0..rng.below(5) {
            let (ty, v) = match rng.below(5) {
                0 => ("BASE", rng.dec(-0.5, 3.0, 2)),
                1 | 2 => ("INC", rng.int(-60, 150)),
                3 => ("MORE", rng.int(-50, 80)),
                _ => ("OVERRIDE", rng.dec(0.0, 5.0, 2)),
            };
            mods.push(M("CooldownRecovery".into(), ty, v, 0));
        }
        if rng.chance(0.2) {
            mods.push(M("AdditionalCooldownUses".into(), "BASE", rng.int(0, 2), 0));
        }
        let cooldown = rng.chance(0.85).then(|| rng.dec(0.0, 12.0, 3));
        let stored = rng.chance(0.4).then(|| rng.int(0, 3));
        let vaal = rng.chance(0.2).then(|| rng.int(0, 3));
        let cfgEmpty = rng.chance(0.5);
        c.push(
            format!("cooldown #{i}"),
            format!(
                "local ml = new(\"ModList\"):ModList()\n{}local sd = {{ cooldown = {}, storedUses = {}, VaalStoredUses = {} }}\n\
                 local a, b, d = calcSkillCooldown(ml, {}, sd) return f17(a)..'|'..tostring(b)..'|'..f17(d)",
                lua_mods("ml", &mods),
                lopt(cooldown),
                lopt(stored),
                lopt(vaal),
                if cfgEmpty { "{}" } else { "nil" }
            ),
            pcall_text(|| {
                let ml = ModStore::ModList(None);
                rust_mods(&ml, &mods);
                let sd = Table::new();
                sd.set(n!("cooldown"), Val::from(cooldown));
                sd.set(n!("storedUses"), Val::from(stored));
                sd.set(n!("VaalStoredUses"), Val::from(vaal));
                let empty = ModCfg::default();
                let cfg = cfgEmpty.then_some(&empty);
                let (a, b, d) = hs::calcSkillCooldown(&ml, cfg, &sd);
                format!("{}|{}|{}", f17(a), b, fopt(d))
            }),
        );
    }
    c
}

fn check_duration(n: usize) -> Check {
    let mut c = Check::new("calcSkillDuration");
    let mut rng = Rng(0x0FA0_0007);
    for i in 0..n {
        let mut mods = Vec::new();
        for _ in 0..rng.below(6) {
            let name = *rng.pick(&["Duration", "PrimaryDuration", "MineDuration"]);
            let (ty, v) = match rng.below(3) {
                0 => ("BASE", rng.dec(-1.0, 4.0, 2)),
                1 => ("INC", rng.int(-120, 150)),
                _ => ("MORE", rng.int(-50, 80)),
            };
            mods.push(M(name.into(), ty, v, 0));
        }
        let mut emods = Vec::new();
        for _ in 0..rng.below(3) {
            let (ty, v) = if rng.chance(0.5) {
                ("INC", rng.int(-90, 200))
            } else {
                ("MORE", rng.int(-80, 100))
            };
            emods.push(M("BuffExpireFaster".into(), ty, v, 0));
        }
        let duration = rng.chance(0.85).then(|| rng.dec(0.0, 20.0, 3));
        let debuff = rng.chance(0.4);
        let mine = rng.chance(0.3);
        let effective = rng.chance(0.6);
        c.push(
            format!("duration #{i}"),
            format!(
                "local ml = new(\"ModList\"):ModList()\n{}local edb = new(\"ModDB\"):ModDB()\n{}\
                 local sd = {{ duration = {}, debuff = {}, mineDurationAppliesToSkill = {} }}\n\
                 return f17(calcSkillDuration(ml, nil, sd, {{ mode_effective = {} }}, edb))",
                lua_mods("ml", &mods),
                lua_mods("edb", &emods),
                lopt(duration),
                if debuff { "true" } else { "nil" },
                if mine { "true" } else { "nil" },
                effective
            ),
            pcall_text(|| {
                let ml = ModStore::ModList(None);
                rust_mods(&ml, &mods);
                let edb = ModStore::ModDB(None);
                rust_mods(&edb, &emods);
                let sd = Table::new();
                sd.set(n!("duration"), Val::from(duration));
                if debuff {
                    sd.set(n!("debuff"), true);
                }
                if mine {
                    sd.set(n!("mineDurationAppliesToSkill"), true);
                }
                let env = new_env();
                env.mode_effective.set(effective);
                f17(hs::calcSkillDuration(&ml, None, &sd, Some(&env), Some(&edb)))
            }),
        );
    }
    c
}

fn check_totem_life(n: usize) -> Check {
    let mut c = Check::new("calcTotemLife");
    let mut rng = Rng(0x0FA0_0008);
    let ids: Vec<f64> = pob_gamedata::data()
        .totemLifeMult
        .keys()
        .map(|k| *k as f64)
        .collect();
    for i in 0..n {
        let mut mods = Vec::new();
        for _ in 0..rng.below(4) {
            let (ty, v) = if rng.chance(0.6) {
                ("INC", rng.int(-50, 200))
            } else {
                ("MORE", rng.int(-30, 60))
            };
            mods.push(M("TotemLife".into(), ty, v, 0));
        }
        let level = if rng.chance(0.05) {
            *rng.pick(&[0.0, 101.0, 1.5, 200.0])
        } else {
            rng.int(1, 100)
        };
        let id = if rng.chance(0.05) {
            *rng.pick(&[0.0, 999.0, 1.5])
        } else {
            *rng.pick(&ids)
        };
        c.push(
            format!("totemLife #{i}"),
            format!(
                "local ml = new(\"ModList\"):ModList()\n{}local skill = {{ skillModList = ml, skillCfg = {{}}, skillData = {{ totemLevel = {} }}, skillTotemId = {} }}\n\
                 local a, b = __OFA.calcTotemLife({{ data = data }}, skill) return f17(a)..'|'..f17(b)",
                lua_mods("ml", &mods),
                lnum(level),
                lnum(id)
            ),
            pcall_text(|| {
                let ml = ModStore::ModList(None);
                rust_mods(&ml, &mods);
                let skill = new_skill();
                skill.skillModList.set(ml);
                skill.skillData.get().set(n!("totemLevel"), level);
                skill.skillTotemId.set(Some(id));
                let env = new_env();
                let (a, b) = hs::calcTotemLife(&env, &skill);
                format!("{}|{}", f17(a), f17(b))
            }),
        );
    }
    c
}

// -- calcResistForType

fn check_resist(n: usize) -> Check {
    let mut c = Check::new("calcResistForType");
    let mut rng = Rng(0x0FA0_0009);
    for i in 0..n {
        let dt = *rng.pick(&["Fire", "Cold", "Lightning", "Chaos", "Physical"]);
        let mut emods = Vec::new();
        for _ in 0..rng.below(6) {
            let name = rng
                .pick(&[
                    format!("{dt}Resist"),
                    format!("{dt}Resist"),
                    "ElementalResist".to_owned(),
                ])
                .clone();
            let (ty, v) = match rng.below(5) {
                0 | 1 => ("BASE", rng.int(-60, 90)),
                2 => ("INC", rng.int(-50, 100)),
                3 => ("MORE", rng.int(-50, 50)),
                _ => ("OVERRIDE", rng.int(-30, 90)),
            };
            emods.push(M(name, ty, v, 0));
        }
        if rng.chance(0.15) {
            emods.push(M("DoNotChangeMaxResFromConfig".into(), "FLAG", 1.0, 0));
        }
        let config = rng.chance(0.5).then(|| rng.int(40, 110));
        let mine = rng.chance(0.15);
        let party = rng.chance(0.15);
        let playerRes = rng.chance(0.9).then(|| rng.int(-60, 90));
        let partyRes = rng.chance(0.9).then(|| rng.int(-60, 90));
        let lua = format!(
            "local edb = new(\"ModDB\"):ModDB()\n{}local pdb = new(\"ModDB\"):ModDB()\n{}local qdb = new(\"ModDB\"):ModDB()\n{}\
             local env = {{ configInput = {{ enemy{dt}Resist = {} }}, modDB = pdb, player = {{ output = {{ {dt}Resist = {} }} }}, \
             partyMembers = {{ modDB = qdb, output = {{ {dt}Resist = {} }} }} }}\n\
             return f17(__OFA.makeResist(env, edb)({dt:?}, nil))",
            lua_mods("edb", &emods),
            if mine {
                format!("pdb:NewMod(\"Enemy{dt}ResistEqualToYours\", \"FLAG\", true, \"Src\")\n")
            } else {
                String::new()
            },
            if party {
                format!("qdb:NewMod(\"Enemy{dt}ResistEqualToYours\", \"FLAG\", true, \"Src\")\n")
            } else {
                String::new()
            },
            lopt(config),
            lopt(playerRes),
            lopt(partyRes)
        );
        let rust = pcall_text(|| {
            let edb = ModStore::ModDB(None);
            rust_mods(&edb, &emods);
            let env = new_env();
            env.configInput.wipe();
            env.configInput
                .set(cat!("enemy", dt, "Resist"), Val::from(config));
            let pdb = ModStore::ModDB(None);
            if mine {
                pdb.NewMod(
                    cat!("Enemy", dt, "ResistEqualToYours"),
                    ModType::FLAG,
                    true,
                    "Src",
                    0,
                    0,
                    &[],
                );
            }
            env.modDB.set(pdb);
            env.player
                .output
                .get()
                .set(cat!(dt, "Resist"), Val::from(playerRes));
            let pm = Actor::new();
            let qdb = ModStore::ModDB(None);
            if party {
                qdb.NewMod(
                    cat!("Enemy", dt, "ResistEqualToYours"),
                    ModType::FLAG,
                    true,
                    "Src",
                    0,
                    0,
                    &[],
                );
            }
            pm.modDB.set(qdb);
            pm.output.get().set(cat!(dt, "Resist"), Val::from(partyRes));
            env.partyMembers.set(Some(pm));
            let ctx = offence_ctx_with(env, Actor::new(), edb);
            f17(hs::calcResistForType(&ctx, Name::from(dt), None))
        });
        c.push(format!("resist #{i}"), lua, rust);
    }
    c
}

#[test]
fn ofa_fuzz_vs_luajit() {
    if pob_testpaths::require(
        testkit::init_default_data().then_some(()),
        "data/dump (pob-dump output)",
    )
    .is_none()
    {
        return;
    }
    let Some(src) = pob_testpaths::require(pob_testpaths::pob_src(), "PoB checkout") else {
        return;
    };
    let n: usize = std::env::var("OFA_FUZZ_N")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(1500);
    let only = std::env::var("OFA_ONLY").ok();
    let want = |name: &str| {
        only.as_deref()
            .is_none_or(|o| o.split(',').any(|x| x == name))
    };

    // the Rust cases first (no Lua needed), then one LuaJIT PoB under the oracle lock
    let hook = std::panic::take_hook();
    std::panic::set_hook(Box::new(|_| {}));
    let mut checks: Vec<Check> = Vec::new();
    let all: [(&str, Box<dyn Fn() -> Check>); 9] = [
        ("damage", Box::new(move || check_damage(n, false))),
        ("ailment", Box::new(move || check_damage(n / 3, true))),
        ("area", Box::new(move || check_area_numeric(n))),
        ("molten", Box::new(move || check_molten_breakdown(n / 5))),
        ("aoe", Box::new(move || check_aoe(n))),
        ("cooldown", Box::new(move || check_cooldown(n))),
        ("duration", Box::new(move || check_duration(n))),
        ("totem", Box::new(move || check_totem_life(n / 3))),
        ("resist", Box::new(move || check_resist(n))),
    ];
    for (name, f) in &all {
        if want(name) {
            checks.push(f());
        }
    }
    std::panic::set_hook(hook);

    let _lock = pob_oracle::runner::OracleLock::acquire().unwrap_or_else(|e| panic!("{e}"));
    let t0 = std::time::Instant::now();
    let pob = JitPob::boot_oracle(&src, None).unwrap_or_else(|e| panic!("boot: {e}"));
    let mut lua = Lua(pob);
    eprintln!("PoB booted in {:?}", t0.elapsed());
    assert_eq!(lua.eval(&lua_helpers(&src)), "ok");
    let mut failed = false;
    for c in &checks {
        let (total, bad, zero, errs) = c.run(&mut lua);
        eprintln!(
            "== {}: {total} compared, {} mismatches, {zero} dual-number host artefacts (CCR-0001/0002), {errs} raise on both sides",
            c.name,
            bad.len()
        );
        for b in bad.iter().take(10) {
            eprintln!("  {b}");
        }
        failed |= !bad.is_empty();
    }
    assert!(!failed, "OF-A helpers differ from PoB (see above)");
}
