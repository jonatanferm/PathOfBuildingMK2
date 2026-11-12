//! LuaJIT timings of PoB's full recalc and power report: the baseline of gate INT-2
//! (calc-architecture.md §9.7 "Final (INT-2)", §11 row INT-2; docs/port/parity/INT-2-perf.md).
//!
//! ```text
//! pob_calc_timing [--recalc N] [--warmup N] [--power N] [--json OUT] [--pob SRC] <build.xml>...
//! ```
//!
//! PoB as it ships (unpatched, **JIT on**: the real behaviour on this host; `jit.off()` is only
//! for oracle determinism, CCR-0001), one boot for all the builds given. Per build:
//!
//! * load it (`main:SetMode("BUILD", …)` + one `OnFrame`, `headless::LOAD_SCRIPT`);
//! * **recalc**: what `Build:OnFrame` runs on `buildFlag` for the calc (Build.lua:1258-1266):
//!   `wipeGlobalCache()` then `calcsTab:BuildOutput()` (MAIN + CALCS `calcs.buildOutput` and
//!   `getMiscCalculator`); `--warmup` untimed runs (default 3), then `--recalc` timed runs
//!   (default 10);
//! * **power**: the power report over the full tree in Offence/Defence mode (`powerStat = nil`,
//!   `nodePowerMaxDepth = nil`, `powerMax = nil`), `calcsTab:PowerBuilder()` called directly (no
//!   coroutine, so it never yields; the work is the coroutine's), `--power` timed runs (default 2).
//!
//! PoB's own `Launch.lua` settings apply (`jit.opt.start('maxtrace=4000','maxmcode=8192')`,
//! `collectgarbage("setpause", 400)`): this is the app's VM, not the oracle's.
//!
//! Every run is timed from Rust around its own `pob.run` (wall clock, `Instant`, and process CPU
//! time, `CLOCK_PROCESS_CPUTIME_ID`), the same clocks the Rust bench (`pob-calc/benches/INT-2.rs`)
//! uses. Output: one JSON object per build (`{"build", "recalc": {…}, "power": {…}, "peakRssMB"}`,
//! each `{"wall": [s…], "cpu": [s…]}`; the peak RSS is the whole process's so far), one per line,
//! or a JSON array in `--json OUT`.
//! `scripts/port/perf_bench.sh` drives this and the Rust bench and tabulates both.

use std::path::PathBuf;
use std::process::ExitCode;
use std::time::Instant;

use pob_oracle::headless::Engine;
use pob_oracle::oracle::{find_pob_src, QUIET_PRELUDE};
use pob_oracle::runner::{write_atomic, OracleLock};

/// Build.lua:1258-1266, the calc part of `Build:OnFrame` on `buildFlag`.
const RECALC_LUA: &str = "wipeGlobalCache()\nbuild.calcsTab:BuildOutput()\n";

/// The power report over the full tree, Offence/Defence (as `corpus/PWR/pwr.lua` runs it).
const POWER_LUA: &str = "local ct = build.calcsTab\nct.powerStat = nil\nct.nodePowerMaxDepth = nil\nct.powerMax = nil\nct:PowerBuilder()\n";

fn cpu_now() -> f64 {
    let t = rustix::time::clock_gettime(rustix::time::ClockId::ProcessCPUTime);
    t.tv_sec as f64 + t.tv_nsec as f64 * 1e-9
}

/// Run `code` once, timed: (wall s, cpu s).
fn timed(pob: &mut pob_oracle::jit::JitPob, code: &str) -> Result<(f64, f64), String> {
    let (c0, t0) = (cpu_now(), Instant::now());
    pob.run(code)?;
    let wall = t0.elapsed().as_secs_f64();
    Ok((wall, cpu_now() - c0))
}

/// Peak RSS of this process in MB (VmHWM, /proc/self/status; 0 where unavailable).
fn peak_rss_mb() -> u64 {
    std::fs::read_to_string("/proc/self/status")
        .unwrap_or_default()
        .lines()
        .find_map(|l| l.strip_prefix("VmHWM:"))
        .and_then(|v| v.split_whitespace().next())
        .and_then(|v| v.parse::<u64>().ok())
        .map_or(0, |kb| kb / 1024)
}

fn json_list(v: &[f64]) -> String {
    let parts: Vec<String> = v.iter().map(|x| format!("{x:.6}")).collect();
    format!("[{}]", parts.join(","))
}

struct Opts {
    recalc: u32,
    warmup: u32,
    power: u32,
}

fn one_build(pob: &mut pob_oracle::jit::JitPob, build: &PathBuf, o: &Opts) -> Result<String, String> {
    let xml = std::fs::read_to_string(build).map_err(|e| format!("{}: {e}", build.display()))?;
    let name = build
        .file_stem()
        .map_or_else(|| "build".to_owned(), |s| s.to_string_lossy().into_owned());
    let t = Instant::now();
    pob.load_build_xml(&xml, &name)?;
    let load = t.elapsed().as_secs_f64();
    for _ in 0..o.warmup {
        pob.run(RECALC_LUA)?;
    }
    let (mut rw, mut rc) = (Vec::new(), Vec::new());
    for _ in 0..o.recalc {
        let (w, c) = timed(pob, RECALC_LUA)?;
        rw.push(w);
        rc.push(c);
    }
    let (mut pw, mut pc) = (Vec::new(), Vec::new());
    for _ in 0..o.power {
        let (w, c) = timed(pob, POWER_LUA)?;
        pw.push(w);
        pc.push(c);
    }
    Ok(format!(
        r#"{{"build":"{name}","engine":"luajit","load":{load:.6},"recalc":{{"wall":{},"cpu":{}}},"power":{{"wall":{},"cpu":{}}},"peakRssMB":{}}}"#,
        json_list(&rw),
        json_list(&rc),
        json_list(&pw),
        json_list(&pc),
        peak_rss_mb()
    ))
}

fn run() -> Result<(), String> {
    let mut args = std::env::args().skip(1);
    let mut builds: Vec<PathBuf> = Vec::new();
    let mut o = Opts { recalc: 10, warmup: 3, power: 2 };
    let mut json: Option<PathBuf> = None;
    let mut pob: Option<PathBuf> = None;
    let num = |v: Option<String>, what: &str| -> Result<u32, String> {
        v.ok_or(format!("{what} N"))?.parse().map_err(|e| format!("{what}: {e}"))
    };
    while let Some(a) = args.next() {
        match a.as_str() {
            "--recalc" => o.recalc = num(args.next(), "--recalc")?,
            "--warmup" => o.warmup = num(args.next(), "--warmup")?,
            "--power" => o.power = num(args.next(), "--power")?,
            "--json" => json = Some(args.next().ok_or("--json OUT")?.into()),
            "--pob" => pob = Some(args.next().ok_or("--pob SRC")?.into()),
            _ if a.starts_with("--") => return Err(format!("unexpected argument {a}")),
            _ => builds.push(a.into()),
        }
    }
    if builds.is_empty() {
        return Err("usage: pob_calc_timing [--recalc N] [--warmup N] [--power N] [--json OUT] [--pob SRC] <build.xml>...".to_owned());
    }
    let src = match pob {
        Some(p) if p.join("src/HeadlessWrapper.lua").is_file() => p.join("src"),
        Some(p) => p,
        None => find_pob_src().ok_or("PoB not found: pass --pob or set POB_SRC")?,
    };
    let _lock = OracleLock::acquire().map_err(|e| e.0)?;
    // Timing baseline: PoB as it ships, JIT on (not an oracle parity run, so not
    // `boot_oracle`; CCR-0001).
    let mut pob = pob_oracle::jit::JitPob::boot_with(&src, Some(QUIET_PRELUDE))?;
    let mut lines = Vec::new();
    for b in &builds {
        let line = match one_build(&mut pob, b, &o) {
            Ok(l) => l,
            Err(e) => {
                let name = b.file_stem().map_or_else(String::new, |s| s.to_string_lossy().into_owned());
                format!(r#"{{"build":"{name}","engine":"luajit","error":{}}}"#, serde_json::Value::String(e))
            }
        };
        if json.is_none() {
            println!("{line}");
        }
        lines.push(line);
    }
    if let Some(p) = json {
        write_atomic(&p, &format!("[{}]\n", lines.join(",\n"))).map_err(|e| e.0)?;
    }
    Ok(())
}

fn main() -> ExitCode {
    match run() {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("{e}");
            ExitCode::FAILURE
        }
    }
}
