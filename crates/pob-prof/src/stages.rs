//! Calc stage breakdown of one build (`pob-prof stages`, and the `calc_<build>` entries of a
//! `run`). Owner: PROF1.
//!
//! The app's recalc is `calcsTab:BuildOutput()` after `wipeGlobalCache()` (Build.lua:1258-1266):
//! `calcs.buildOutput(build, "MAIN")` (Calcs.lua:384: `initEnv`, `perform`, `calcFullDPS`, the
//! MAIN bookkeeping), `calcs.buildOutput(build, "CALCS")`, then `getMiscCalculator`
//! (CalcsTab.lua:441-472). The calc has no timing hooks, so this module calls those public
//! functions itself, in that order, on a build loaded with `loadBuildFromXML`, and times each one:
//!
//! | step | what is timed (each after `wipeGlobalCache()` where marked *) |
//! |---|---|
//! | `full` | * `power::BuildOutput` (the app's recalc) |
//! | `initEnv` | * `calcs.initEnv(build, "MAIN")` |
//! | `perform` | `calcs.perform(env)` on that env |
//! | `buildOutput_MAIN` | * `calcs.buildOutput(build, "MAIN")` (initEnv + perform + calcFullDPS + bookkeeping) |
//! | `buildOutput_CALCS` | `calcs.buildOutput(build, "CALCS")` |
//! | `getMiscCalculator` | `calcs.getMiscCalculator(build)` |
//! | `misc_call` | one `calcFunc({})` (what a tooltip's stat difference costs) |
//!
//! `offence` and `defence` run inside `perform` (and `calcFullDPS`) and cannot be timed from
//! outside without a hook in the calc (CCR-0150, docs/port/ccr/0150-prof1-calc-stage-timing.md);
//! their share comes from the sampling profile (`hotspots`: inclusive share of the
//! `calc_offence::` and `calc_defence::` frames).

use std::path::Path;
use std::rc::Rc;
use std::time::Instant;

use anyhow::{anyhow, Result};
use pob_calc::modules::{calc_perform, calc_setup, calcs, common, power};
use pob_calc::Mode;

use crate::alloc;
use crate::driver::{RepResult, StepResult};
use crate::stats;

// CCR-0150: no offence/defence entries (no hook in the calc); see the module docs.
pub const STAGES: &[&str] = &[
    "full",
    "initEnv",
    "perform",
    "buildOutput_MAIN",
    "buildOutput_CALCS",
    "getMiscCalculator",
    "misc_call",
];

fn catch<R>(f: impl FnOnce() -> R) -> Result<R> {
    std::panic::catch_unwind(std::panic::AssertUnwindSafe(f)).map_err(|p| {
        anyhow!(
            "{}",
            p.downcast_ref::<pob_lua::LuaError>()
                .map(|e| format!("Lua error: {}", e.0))
                .or_else(|| p.downcast_ref::<String>().cloned())
                .or_else(|| p.downcast_ref::<&str>().map(|s| (*s).to_owned()))
                .unwrap_or_else(|| "panic".to_owned())
        )
    })
}

/// `(result, wall ms, allocations)` of `f`; its CPU time goes to [`last_cpu`].
fn timed<R>(f: impl FnOnce() -> R) -> (R, f64, u64) {
    let a0 = alloc::snapshot();
    let c0 = crate::driver::thread_cpu_s();
    let t0 = Instant::now();
    let r = f();
    let d = t0.elapsed().as_secs_f64() * 1000.0;
    CPU.with(|c| c.set((crate::driver::thread_cpu_s() - c0) * 1000.0));
    (r, d, alloc::snapshot().since(&a0).allocs)
}

thread_local! {
    /// The CPU time (ms) of the last [`timed`] call.
    static CPU: std::cell::Cell<f64> = const { std::cell::Cell::new(0.0) };
}

fn last_cpu() -> f64 {
    CPU.with(std::cell::Cell::get)
}

/// Time the stages of `xml`: `warmup` untimed iterations, then `iters` timed ones. The result is a
/// [`RepResult`] whose steps are the stages (`wall_ms` = median over the iterations).
pub fn run(xml_path: &Path, iters: u32, warmup: u32, rep: u32) -> RepResult {
    let name = format!(
        "calc_{}",
        xml_path
            .file_stem()
            .map(|s| s.to_string_lossy().into_owned())
            .unwrap_or_default()
    );
    let mut res = RepResult {
        scenario: name,
        rep,
        render: "none".into(),
        alloc_counting: alloc::active(),
        ..RepResult::default()
    };
    let cpu0 = crate::driver::cpu_s();
    match run_inner(xml_path, iters, warmup) {
        Ok(steps) => {
            res.ok = true;
            res.steps = steps;
        }
        Err(e) => res.error = Some(format!("{e:#}")),
    }
    res.total_ms = res.steps.iter().map(|s| s.wall_ms).sum();
    res.cpu_ms = (crate::driver::cpu_s() - cpu0) * 1000.0;
    res.peak_rss_mb = crate::driver::rss_mb().1;
    res
}

fn run_inner(xml_path: &Path, iters: u32, warmup: u32) -> Result<Vec<StepResult>> {
    let xml =
        std::fs::read_to_string(xml_path).map_err(|e| anyhow!("{}: {e}", xml_path.display()))?;
    pob_calc::global_cache::clear_all();
    let build = catch(|| pob_calc::model::build_xml::loadBuildFromXML(&xml, pob_gamedata::data()))?;
    let mut samples: Vec<(Vec<f64>, Vec<f64>, Vec<f64>)> =
        vec![(Vec::new(), Vec::new(), Vec::new()); STAGES.len()];
    for i in 0..warmup + iters {
        let mut put = |k: usize, ms: f64, allocs: u64| {
            if i >= warmup {
                samples[k].0.push(ms);
                samples[k].1.push(allocs as f64);
                samples[k].2.push(last_cpu());
            }
        };
        // full: the app's recalc
        common::wipeGlobalCache();
        let ((), d, a) = timed(|| catch(|| power::BuildOutput(&build))).map0()?;
        put(0, d, a);
        // initEnv + perform
        common::wipeGlobalCache();
        let (env, d, a) =
            timed(|| catch(|| calc_setup::initEnv(&build, Mode::MAIN, None, None))).map0()?;
        put(1, d, a);
        let env = env.0;
        let ((), d, a) = timed(|| catch(|| calc_perform::perform(&env, false))).map0()?;
        put(2, d, a);
        drop(env);
        // buildOutput MAIN, CALCS, getMiscCalculator
        common::wipeGlobalCache();
        let (main_env, d, a) = timed(|| catch(|| calcs::buildOutput(&build, Mode::MAIN))).map0()?;
        put(3, d, a);
        let (calcs_env, d, a) =
            timed(|| catch(|| calcs::buildOutput(&build, Mode::CALCS))).map0()?;
        put(4, d, a);
        let ((func, _base), d, a) = timed(|| catch(|| calcs::getMiscCalculator(&build))).map0()?;
        put(5, d, a);
        let (_out, d, a) =
            timed(|| catch(|| func.call(Rc::new(pob_calc::env::Override::default()), None)))
                .map0()?;
        put(6, d, a);
        drop((main_env, calcs_env, func));
    }
    // leave the build as the app would have it
    common::wipeGlobalCache();
    catch(|| power::BuildOutput(&build))?;
    Ok(STAGES
        .iter()
        .zip(samples)
        .map(|(n, (ms, al, cpu))| StepResult {
            name: (*n).to_owned(),
            wall_ms: stats::median(&ms),
            cpu_ms: stats::median(&cpu),
            allocs: stats::median(&al) as u64,
            ..StepResult::default()
        })
        .collect())
}

/// `(Result<R>, ms, allocs)` -> `Result<(R, ms, allocs)>`.
trait Map0<R> {
    fn map0(self) -> Result<(R, f64, u64)>;
}

impl<R> Map0<R> for (Result<R>, f64, u64) {
    fn map0(self) -> Result<(R, f64, u64)> {
        let (r, d, a) = self;
        r.map(|r| (r, d, a))
    }
}
