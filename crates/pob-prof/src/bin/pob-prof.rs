//! `pob-prof`: the profiling suite's command line (docs/perf/PROFILING.md). Owner: PROF1.
//!
//! ```text
//! pob-prof list
//! pob-prof run      [--scenarios a,b|all] [--reps 5] [--warmup 1] [--render none|cpu|wgpu]
//!                   [--stages a,b|all|none] [--stage-iters 5] [--out FILE] [--label TEXT]
//! pob-prof exec     <scenario> [--rep N] [--render MODE]          one repetition, JSON on stdout
//! pob-prof stages   <build.xml>... [--iters 5] [--warmup 2]      calc stage table
//! pob-prof inspect  <scenario> [--until STEP] [--png FILE] [--texts] [--render MODE]
//! pob-prof show     <results.json>
//! pob-prof bless    <results.json> [--out crates/pob-prof/baseline/baseline.json]
//! pob-prof compare  <base.json> <new.json> [--threshold PCT] [--field-threshold FIELD=PCT]...
//!                   [--alpha 0.05] [--all] [--scenarios a,b]
//! pob-prof collapse [perf-script.txt|-]                          perf script -> folded stacks
//! pob-prof hotspots <folded.txt> [--top 5] [--filter FRAME]
//! common: --assets DIR --data DIR (see crate::paths)
//! ```

#![allow(clippy::many_single_char_names, clippy::single_match_else)]

use std::collections::BTreeMap;
use std::io::Write as _;
use std::path::{Path, PathBuf};
use std::process::{Command, ExitCode, Stdio};
use std::time::Instant;

use anyhow::{anyhow, bail, Context, Result};
use pob_prof::driver::{self, RepResult, RunOptions};
use pob_prof::render::RenderMode;
use pob_prof::results::{self, Meta, ResultsFile, ScenarioResults};
use pob_prof::{compare, hotspots, paths, scenario, stages, sysinfo};

#[cfg(not(feature = "dhat-heap"))]
#[global_allocator]
static GLOBAL: pob_prof::alloc::Counting = pob_prof::alloc::Counting;

#[cfg(feature = "dhat-heap")]
#[global_allocator]
static GLOBAL: dhat::Alloc = dhat::Alloc;

/// Parsed `--flag value` / `--switch` / positional arguments.
struct Args {
    pos: Vec<String>,
    opts: BTreeMap<String, Vec<String>>,
}

const SWITCHES: &[&str] = &["--texts", "--all", "--quiet"];

impl Args {
    fn parse(v: &[String]) -> Result<Args> {
        let mut pos = Vec::new();
        let mut opts: BTreeMap<String, Vec<String>> = BTreeMap::new();
        let mut it = v.iter();
        while let Some(a) = it.next() {
            if a.starts_with("--") {
                if SWITCHES.contains(&a.as_str()) {
                    opts.entry(a.clone()).or_default().push(String::new());
                } else {
                    let val = it.next().ok_or_else(|| anyhow!("{a} needs a value"))?;
                    opts.entry(a.clone()).or_default().push(val.clone());
                }
            } else {
                pos.push(a.clone());
            }
        }
        Ok(Args { pos, opts })
    }
    fn get(&self, k: &str) -> Option<&str> {
        self.opts.get(k).and_then(|v| v.last()).map(String::as_str)
    }
    fn all(&self, k: &str) -> Vec<String> {
        self.opts.get(k).cloned().unwrap_or_default()
    }
    fn has(&self, k: &str) -> bool {
        self.opts.contains_key(k)
    }
    fn num<T: std::str::FromStr>(&self, k: &str, d: T) -> Result<T> {
        match self.get(k) {
            None => Ok(d),
            Some(s) => s.parse::<T>().map_err(|_| anyhow!("{k}: bad number {s:?}")),
        }
    }
    fn render(&self) -> Result<RenderMode> {
        RenderMode::parse(self.get("--render").unwrap_or("none"))
    }
    fn inputs(&self) -> Result<paths::Inputs> {
        paths::locate(
            self.get("--assets").map(Path::new),
            self.get("--data").map(Path::new),
        )
    }
    fn list(&self, k: &str) -> Option<Vec<String>> {
        self.get(k).map(|s| {
            s.split(',')
                .map(str::trim)
                .filter(|x| !x.is_empty())
                .map(str::to_owned)
                .collect()
        })
    }
}

fn fixture_builds() -> Result<Vec<PathBuf>> {
    let dir = paths::crate_dir().join("fixtures/user/Path of Building/Builds");
    let mut v: Vec<PathBuf> = std::fs::read_dir(&dir)
        .with_context(|| dir.display().to_string())?
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.extension().is_some_and(|x| x == "xml"))
        .collect();
    v.sort();
    Ok(v)
}

fn main() -> ExitCode {
    let t_main = Instant::now();
    let argv: Vec<String> = std::env::args().skip(1).collect();
    let Some(cmd) = argv.first().cloned() else {
        eprintln!("usage: pob-prof <list|run|exec|stages|inspect|show|compare|collapse|hotspots> … (docs/perf/PROFILING.md)");
        return ExitCode::from(2);
    };
    let args = match Args::parse(&argv[1..]) {
        Ok(a) => a,
        Err(e) => {
            eprintln!("pob-prof: {e}");
            return ExitCode::from(2);
        }
    };
    let r = match cmd.as_str() {
        "list" => cmd_list(),
        "run" => cmd_run(&args),
        "exec" => cmd_exec(&args, t_main),
        "exec-stages" => cmd_exec_stages(&args),
        "stages" => cmd_stages(&args),
        "inspect" => cmd_inspect(&args, t_main),
        "show" => cmd_show(&args),
        "bless" => cmd_bless(&args),
        "compare" => return cmd_compare(&args),
        "collapse" => cmd_collapse(&args),
        "hotspots" => cmd_hotspots(&args),
        _ => Err(anyhow!("unknown command {cmd:?}")),
    };
    match r {
        Ok(code) => code,
        Err(e) => {
            eprintln!("pob-prof {cmd}: {e:#}");
            ExitCode::FAILURE
        }
    }
}

fn cmd_list() -> Result<ExitCode> {
    for p in scenario::list(&paths::scenario_dir())? {
        let s = scenario::Scenario::load(&p)?;
        println!("{:<22} {}", s.name, s.description);
        println!("{:<22}   steps: {}", "", s.step_names().join(", "));
    }
    for b in fixture_builds()? {
        let stem = b.file_stem().unwrap_or_default().to_string_lossy();
        println!(
            "calc_{stem:<17} calc stages of {stem} ({})",
            stages::STAGES.join(", ")
        );
    }
    Ok(ExitCode::SUCCESS)
}

fn epoch_s() -> f64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0.0, |d| d.as_secs_f64())
}

/// `exec`: one repetition of one scenario; the RepResult as one JSON line on stdout.
fn cmd_exec(a: &Args, t_main: Instant) -> Result<ExitCode> {
    #[cfg(feature = "dhat-heap")]
    let _dhat = dhat::Profiler::new_heap();
    let name = a.pos.first().ok_or_else(|| anyhow!("exec <scenario>"))?;
    let path = scenario::find(&paths::scenario_dir(), name)?;
    let sc = scenario::Scenario::load(&path)?;
    let inputs = a.inputs()?;
    let data_ms = driver::init_process(&inputs)?;
    let opts = RunOptions {
        render: a.render()?,
        spawn_epoch_s: a
            .num::<f64>("--spawn-epoch", -1.0)
            .ok()
            .filter(|s| *s > 0.0),
        rep: a.num("--rep", 0)?,
        ..RunOptions::default()
    };
    let r = driver::run(&sc, &inputs, &opts, t_main, data_ms);
    let line = serde_json::to_string(&r)?;
    let mut out = std::io::stdout().lock();
    writeln!(out, "{line}")?;
    out.flush()?;
    let code: u8 = if r.ok { 0 } else { 1 };
    #[cfg(feature = "dhat-heap")]
    {
        // the profile is written when the profiler drops
        drop(_dhat);
        Ok(ExitCode::from(code))
    }
    // skip the teardown of the app and the data (not measured; can take a while)
    #[cfg(not(feature = "dhat-heap"))]
    std::process::exit(i32::from(code))
}

fn cmd_exec_stages(a: &Args) -> Result<ExitCode> {
    let xml = a
        .pos
        .first()
        .ok_or_else(|| anyhow!("exec-stages <build.xml>"))?;
    let inputs = a.inputs()?;
    driver::init_process(&inputs)?;
    let r = stages::run(
        Path::new(xml),
        a.num("--iters", 5)?,
        a.num("--warmup", 2)?,
        a.num("--rep", 0)?,
    );
    println!("{}", serde_json::to_string(&r)?);
    std::process::exit(if r.ok { 0 } else { 1 })
}

fn cmd_stages(a: &Args) -> Result<ExitCode> {
    if a.pos.is_empty() {
        bail!("stages <build.xml>...");
    }
    let inputs = a.inputs()?;
    driver::init_process(&inputs)?;
    for xml in &a.pos {
        let r = stages::run(
            Path::new(xml),
            a.num("--iters", 5)?,
            a.num("--warmup", 2)?,
            0,
        );
        println!("{} ({}):", r.scenario, if r.ok { "ok" } else { "FAILED" });
        if let Some(e) = &r.error {
            println!("  {e}");
        }
        for s in &r.steps {
            println!(
                "  {:<20} {:>9.2} ms  {:>9} allocs",
                s.name, s.wall_ms, s.allocs
            );
        }
    }
    Ok(ExitCode::SUCCESS)
}

fn print_steps(r: &RepResult) {
    eprintln!(
        "{:<22} {:>9} {:>6} {:>8} {:>8} {:>8} {:>5} {:>9} {:>6} {:>9} {:>9} {:>8}",
        "step",
        "wall ms",
        "frames",
        "p50",
        "p95",
        "max",
        "recl",
        "recalc ms",
        "misc",
        "allocs",
        "load ms",
        "RSS MB"
    );
    for s in &r.steps {
        eprintln!(
            "{:<22} {:>9.1} {:>6} {:>8.2} {:>8.2} {:>8.2} {:>5} {:>9.1} {:>6} {:>9} {:>9} {:>8.0}",
            s.name,
            s.wall_ms,
            s.frames,
            s.frame_p50_ms,
            s.frame_p95_ms,
            s.frame_max_ms,
            s.recalcs,
            s.recalc_ms,
            s.misc_calls,
            s.allocs,
            s.build_load_ms
                .map_or_else(|| "-".to_owned(), |x| format!("{x:.1}")),
            s.rss_peak_mb
        );
    }
    let st = &r.startup;
    eprintln!(
        "startup: data {:.0} ms, assets {:.0} ms, PobApp::new {:.0} ms, OnInit {:.0} ms, first frame {:.1} ms, main -> first screen {:.0} ms; peak RSS {:.0} MB; cpu {:.0} ms",
        st.data_ms, st.assets_ms, st.app_new_ms, st.on_init_ms, st.first_frame_ms, st.to_first_screen_ms, r.peak_rss_mb, r.cpu_ms
    );
    if let Some(a) = &r.adapter {
        eprintln!("render: {} on {a}", r.render);
    }
    if let Some(e) = &r.error {
        eprintln!("ERROR: {e}");
    }
}

/// `inspect`: run in-process, print the step table, optionally the last frame (PNG, strings).
fn cmd_inspect(a: &Args, t_main: Instant) -> Result<ExitCode> {
    let name = a.pos.first().ok_or_else(|| anyhow!("inspect <scenario>"))?;
    let path = scenario::find(&paths::scenario_dir(), name)?;
    let sc = scenario::Scenario::load(&path)?;
    let inputs = a.inputs()?;
    eprintln!(
        "inputs: src {} | runtime {} | data {} ({})",
        inputs.src.display(),
        inputs.runtime.display(),
        inputs.dump.display(),
        inputs.how
    );
    let data_ms = driver::init_process(&inputs)?;
    let opts = RunOptions {
        render: a.render()?,
        until: a.get("--until").map(str::to_owned),
        png: a.get("--png").map(PathBuf::from),
        texts: a.has("--texts"),
        ..RunOptions::default()
    };
    let r = driver::run(&sc, &inputs, &opts, t_main, data_ms);
    print_steps(&r);
    let missing = driver::missing_steps(&sc, &r);
    if r.ok && opts.until.is_none() && !missing.is_empty() {
        eprintln!("steps that did not run: {missing:?}");
    }
    std::process::exit(if r.ok { 0 } else { 1 })
}

/// One child run; its RepResult (a failed one when the child produced none).
fn spawn_child(
    exe: &Path,
    args: &[String],
    scenario: &str,
    rep: u32,
    log: &mut std::fs::File,
) -> RepResult {
    let t0 = Instant::now();
    let child = Command::new(exe)
        .args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn();
    let fail = |e: String| RepResult {
        scenario: scenario.to_owned(),
        rep,
        ok: false,
        error: Some(e),
        ..RepResult::default()
    };
    let out = match child.and_then(std::process::Child::wait_with_output) {
        Ok(o) => o,
        Err(e) => return fail(format!("spawn: {e}")),
    };
    let _ = writeln!(
        log,
        "=== {scenario} rep {rep} ({:.1} s, {})",
        t0.elapsed().as_secs_f64(),
        out.status
    );
    let _ = log.write_all(&out.stderr);
    let stdout = String::from_utf8_lossy(&out.stdout);
    match stdout.lines().rev().find(|l| l.starts_with('{')) {
        Some(l) => match serde_json::from_str::<RepResult>(l) {
            Ok(mut r) => {
                if !out.status.success() && r.ok {
                    r.ok = false;
                    r.error = Some(format!("exit status {}", out.status));
                }
                r
            }
            Err(e) => fail(format!("bad JSON from the child: {e}")),
        },
        None => {
            let err = String::from_utf8_lossy(&out.stderr);
            let tail: Vec<&str> = err.lines().rev().take(5).collect();
            fail(format!(
                "{} without a result; stderr tail: {}",
                out.status,
                tail.into_iter().rev().collect::<Vec<_>>().join(" | ")
            ))
        }
    }
}

/// `run`: every repetition of every selected scenario in a fresh process.
fn cmd_run(a: &Args) -> Result<ExitCode> {
    let t0 = Instant::now();
    let reps: u32 = a.num("--reps", 5)?;
    let warmup: u32 = a.num("--warmup", 1)?;
    let render = a.render()?;
    let quiet = a.has("--quiet");
    let all_sc = scenario::list(&paths::scenario_dir())?;
    let wanted = a.list("--scenarios").filter(|v| v != &["all".to_owned()]);
    let mut jobs: Vec<(String, Vec<String>)> = Vec::new(); // (name, exec args minus --rep)
    let mut descr: BTreeMap<String, (String, Vec<String>)> = BTreeMap::new();
    let mut common: Vec<String> = Vec::new();
    for k in ["--assets", "--data"] {
        if let Some(v) = a.get(k) {
            common.push(k.to_owned());
            common.push(v.to_owned());
        }
    }
    for p in &all_sc {
        let s = scenario::Scenario::load(p)?;
        if let Some(w) = &wanted {
            if !w.contains(&s.name) {
                continue;
            }
        }
        let mut args = vec![
            "exec".to_owned(),
            p.to_string_lossy().into_owned(),
            "--render".to_owned(),
            render.name().to_owned(),
        ];
        args.extend(common.iter().cloned());
        descr.insert(s.name.clone(), (s.description.clone(), s.step_names()));
        jobs.push((s.name.clone(), args));
    }
    let stage_sel = a.get("--stages").unwrap_or("all");
    if stage_sel != "none" {
        let sel: Option<Vec<&str>> = (stage_sel != "all").then(|| stage_sel.split(',').collect());
        for b in fixture_builds()? {
            let stem = b
                .file_stem()
                .unwrap_or_default()
                .to_string_lossy()
                .into_owned();
            let name = format!("calc_{stem}");
            if let Some(s) = &sel {
                if !s.contains(&stem.as_str()) && !s.contains(&name.as_str()) {
                    continue;
                }
            }
            if let Some(w) = &wanted {
                if !w.contains(&name) && a.get("--stages").is_none() {
                    continue;
                }
            }
            let mut args = vec![
                "exec-stages".to_owned(),
                b.to_string_lossy().into_owned(),
                "--iters".to_owned(),
                a.num::<u32>("--stage-iters", 5)?.to_string(),
            ];
            args.extend(common.iter().cloned());
            descr.insert(
                name.clone(),
                (
                    format!("calc stages of {stem} (crate::stages)"),
                    stages::STAGES.iter().map(|s| (*s).to_owned()).collect(),
                ),
            );
            jobs.push((name, args));
        }
    }
    if jobs.is_empty() {
        bail!("no scenario selected");
    }
    let out = a.get("--out").map_or_else(
        || {
            PathBuf::from(format!(
                "target/pob-prof/{}.json",
                sysinfo::now_utc().replace(':', "")
            ))
        },
        PathBuf::from,
    );
    if let Some(d) = out.parent() {
        std::fs::create_dir_all(d).ok();
    }
    let log_path = out.with_extension("log");
    let mut log =
        std::fs::File::create(&log_path).with_context(|| log_path.display().to_string())?;
    let exe = std::env::current_exe()?;
    let (commit, dirty) = sysinfo::git(&paths::crate_dir());
    let mut meta = Meta {
        commit,
        dirty,
        date: sysinfo::now_utc(),
        label: a.get("--label").unwrap_or("").to_owned(),
        render: render.name().to_owned(),
        reps,
        warmup,
        // target/<profile>/pob-prof
        profile: exe
            .parent()
            .and_then(Path::file_name)
            .map(|s| s.to_string_lossy().into_owned())
            .unwrap_or_default(),
        cpus: sysinfo::cpus_allowed(),
        governor: sysinfo::governor(),
        cpu_model: sysinfo::cpu_model(),
        kernel: sysinfo::kernel(),
        load_start: sysinfo::loadavg(),
        ..Meta::default()
    };
    if cfg!(debug_assertions) {
        eprintln!(
            "pob-prof: WARNING: a debug build; numbers are meaningless (build with --profile prof)"
        );
    }
    if !meta.governor.split(',').all(|g| g == "performance") {
        eprintln!(
            "pob-prof: CPU governor is {:?} (not pinned to performance; needs root: see docs/perf/PROFILING.md \"Noise\")",
            meta.governor
        );
    }
    let mut reps_by: BTreeMap<String, Vec<RepResult>> = BTreeMap::new();
    let total = (warmup + reps) as usize * jobs.len();
    let mut done = 0;
    for rep in 0..warmup + reps {
        // rotate the order each repetition, so no scenario always runs first or last
        let n = jobs.len();
        for i in 0..n {
            let (name, args) = &jobs[(i + rep as usize) % n];
            let mut args = args.clone();
            args.extend([
                "--rep".to_owned(),
                rep.to_string(),
                "--spawn-epoch".to_owned(),
                format!("{:.6}", epoch_s()),
            ]);
            let t = Instant::now();
            let r = spawn_child(&exe, &args, name, rep, &mut log);
            done += 1;
            if !quiet {
                eprintln!(
                    "[{done}/{total}] {name} rep {rep}{} {} {:.1} s{}",
                    if rep < warmup { " (warm-up)" } else { "" },
                    if r.ok { "ok" } else { "FAILED" },
                    t.elapsed().as_secs_f64(),
                    r.error
                        .as_ref()
                        .map(|e| format!(": {e}"))
                        .unwrap_or_default()
                );
            }
            reps_by.entry(name.clone()).or_default().push(r);
        }
    }
    meta.load_end = sysinfo::loadavg();
    meta.wall_s = t0.elapsed().as_secs_f64();
    let mut file = ResultsFile {
        schema: results::SCHEMA.to_owned(),
        meta,
        scenarios: BTreeMap::new(),
    };
    let mut failed = 0;
    for (name, mut rs) in reps_by {
        rs.sort_by_key(|r| r.rep);
        let errors: Vec<String> = rs
            .iter()
            .filter(|r| !r.ok)
            .map(|r| format!("rep {}: {}", r.rep, r.error.clone().unwrap_or_default()))
            .collect();
        failed += errors.len();
        let (description, steps) = descr.remove(&name).unwrap_or_default();
        file.scenarios.insert(
            name,
            ScenarioResults {
                description,
                steps,
                metrics: results::summarize(&rs, warmup),
                reps: rs,
                errors,
            },
        );
    }
    std::fs::write(&out, serde_json::to_string_pretty(&file)?)?;
    eprintln!(
        "pob-prof: results in {} (log {}), {:.0} s",
        out.display(),
        log_path.display(),
        file.meta.wall_s
    );
    print!("{}", show(&file));
    if failed > 0 {
        eprintln!("pob-prof: {failed} repetition(s) FAILED");
        return Ok(ExitCode::FAILURE);
    }
    Ok(ExitCode::SUCCESS)
}

/// The medians of the main metrics, one line per step.
fn show(f: &ResultsFile) -> String {
    use std::fmt::Write as _;
    let mut o = String::new();
    let m = &f.meta;
    let _ = writeln!(
        o,
        "{} {} render={} reps={}+{} warm-up cpus={} governor={} load {:?} -> {:?}",
        m.date,
        m.commit.get(..10).unwrap_or(&m.commit),
        m.render,
        m.reps,
        m.warmup,
        m.cpus,
        m.governor,
        m.load_start,
        m.load_end
    );
    let _ = writeln!(
        o,
        "{:<26} {:<20} {:>10} {:>6} {:>10} {:>6} {:>9} {:>9} {:>9} {:>10} {:>9} {:>7}",
        "scenario",
        "step",
        "wall ms",
        "±%",
        "cpu ms",
        "±%",
        "frame p50",
        "frame p95",
        "recalc ms",
        "allocs",
        "load ms",
        "RSS MB"
    );
    for (name, s) in &f.scenarios {
        let g = |k: &str| s.metrics.get(k);
        for step in &s.steps {
            let med = |f: &str| g(&format!("{step}.{f}")).map(|x| x.median);
            let Some(w) = g(&format!("{step}.wall_ms")) else {
                continue;
            };
            let fmt =
                |x: Option<f64>, p: usize| x.map_or_else(|| "-".to_owned(), |v| format!("{v:.p$}"));
            let cpu = g(&format!("{step}.cpu_ms"));
            let _ = writeln!(
                o,
                "{:<26} {:<20} {:>10.2} {:>6.1} {:>10} {:>6} {:>9} {:>9} {:>9} {:>10} {:>9} {:>7}",
                name,
                step,
                w.median,
                w.rel_mad * 100.0,
                fmt(cpu.map(|c| c.median), 2),
                fmt(cpu.map(|c| c.rel_mad * 100.0), 1),
                fmt(med("frame_p50_ms"), 2),
                fmt(med("frame_p95_ms"), 2),
                fmt(med("recalc_ms"), 1),
                fmt(med("allocs"), 0),
                fmt(med("build_load_ms"), 1),
                fmt(med("rss_peak_mb"), 0),
            );
        }
        if let Some(st) = g("startup.to_first_screen_ms") {
            let d = g("startup.data_ms").map_or(0.0, |x| x.median);
            let ex = g("startup.exec_ms").map_or(0.0, |x| x.median);
            let _ = writeln!(
                o,
                "{:<26} {:<20} {:>10.2} {:>6.1}   (main to the first screen; exec {ex:.1} ms, data.bin {d:.0} ms; peak RSS {:.0} MB)",
                name,
                "startup",
                st.median,
                st.rel_mad * 100.0,
                g("total.peak_rss_mb").map_or(0.0, |x| x.median)
            );
        }
        for e in &s.errors {
            let _ = writeln!(o, "{name}: FAILED {e}");
        }
    }
    o
}

fn load_results(p: &str) -> Result<ResultsFile> {
    let text = std::fs::read_to_string(p).with_context(|| p.to_owned())?;
    let f: ResultsFile = serde_json::from_str(&text).with_context(|| p.to_owned())?;
    if f.schema != results::SCHEMA {
        bail!("{p}: schema {:?}, expected {}", f.schema, results::SCHEMA);
    }
    Ok(f)
}

fn cmd_show(a: &Args) -> Result<ExitCode> {
    let p = a
        .pos
        .first()
        .ok_or_else(|| anyhow!("show <results.json>"))?;
    print!("{}", show(&load_results(p)?));
    Ok(ExitCode::SUCCESS)
}

/// `bless RESULTS --out BASELINE`: the results without the raw repetitions (the summaries keep
/// every sample), compact, as the committed baseline.
fn cmd_bless(a: &Args) -> Result<ExitCode> {
    let p = a
        .pos
        .first()
        .ok_or_else(|| anyhow!("bless <results.json> --out <baseline.json>"))?;
    let out = a.get("--out").map_or_else(
        || paths::crate_dir().join("baseline/baseline.json"),
        PathBuf::from,
    );
    let mut f = load_results(p)?;
    let mut failed = Vec::new();
    for (name, s) in &mut f.scenarios {
        if !s.errors.is_empty() {
            failed.push(name.clone());
        }
        s.reps.clear();
    }
    if !failed.is_empty() {
        bail!(
            "{p}: scenarios with failed repetitions ({}); not blessing",
            failed.join(", ")
        );
    }
    if let Some(d) = out.parent() {
        std::fs::create_dir_all(d)?;
    }
    std::fs::write(&out, serde_json::to_string(&f)? + "\n")?;
    eprintln!(
        "pob-prof: baseline {} <- {p} ({} scenarios)",
        out.display(),
        f.scenarios.len()
    );
    Ok(ExitCode::SUCCESS)
}

fn cmd_compare(a: &Args) -> ExitCode {
    let run = || -> Result<compare::Outcome> {
        if a.pos.len() != 2 {
            bail!("compare <base.json> <new.json>");
        }
        let base = load_results(&a.pos[0])?;
        let new = load_results(&a.pos[1])?;
        let mut o = compare::CompareOptions {
            threshold_pct: a.num("--threshold", 10.0)?,
            alpha: a.num("--alpha", 0.05)?,
            all: a.has("--all"),
            scenarios: a.list("--scenarios").unwrap_or_default(),
            gated: a.list("--gate").unwrap_or_default(),
            ..compare::CompareOptions::default()
        };
        for ft in a.all("--field-threshold") {
            let (k, v) = ft
                .split_once('=')
                .ok_or_else(|| anyhow!("--field-threshold FIELD=PCT"))?;
            o.field_threshold.insert(
                k.to_owned(),
                v.parse().map_err(|_| anyhow!("--field-threshold {ft}"))?,
            );
        }
        Ok(compare::compare(&base, &new, &o))
    };
    match run() {
        Ok(o) => {
            print!("{}", o.report);
            if o.regressed {
                ExitCode::FAILURE
            } else {
                ExitCode::SUCCESS
            }
        }
        Err(e) => {
            eprintln!("pob-prof compare: {e:#}");
            ExitCode::from(2)
        }
    }
}

fn cmd_collapse(a: &Args) -> Result<ExitCode> {
    let folded = match a.pos.first().map(String::as_str) {
        None | Some("-") => hotspots::collapse(std::io::stdin().lock()),
        Some(p) => hotspots::collapse(std::io::BufReader::new(
            std::fs::File::open(p).with_context(|| p.to_owned())?,
        )),
    };
    let mut out = std::io::stdout().lock();
    out.write_all(hotspots::to_folded(&folded).as_bytes())?;
    Ok(ExitCode::SUCCESS)
}

fn cmd_hotspots(a: &Args) -> Result<ExitCode> {
    let p = a
        .pos
        .first()
        .ok_or_else(|| anyhow!("hotspots <folded.txt>"))?;
    let text = std::fs::read_to_string(p).with_context(|| p.clone())?;
    let f = hotspots::parse_folded(&text);
    let r = hotspots::analyse(&f, a.get("--filter"));
    print!("{}", hotspots::render(&r, a.num("--top", 5)?));
    Ok(ExitCode::SUCCESS)
}
