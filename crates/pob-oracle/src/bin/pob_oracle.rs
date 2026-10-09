//! PoB under LuaJIT as a parity oracle: one build per process.
//!
//! ```text
//! pob_oracle outputs <build.xml> [--json OUT] [--pob SRC]
//! pob_oracle stages  <build.xml>  --json OUT  [--pob SRC]
//! pob_oracle all     <build.xml>  --out-dir DIR [--pob SRC]   # DIR/{outputs,stages}.json
//! pob_oracle patch   [--stages F] [--pob SRC] [--root DIR]    # prints the patched src/
//! pob_oracle probe   <build.xml>  --out-dir DIR [--stages F | --patched SRC] [--select G,..]
//!                    [--max-dumps N] [--no-provenance] [--cache] [--pob SRC]
//! pob_oracle nondet  <build.xml>  [--out FILE] [--keep DIR] [--verify] [--max-runs N] [--cache] [--stages F]
//!                    [--select G,..] [--no-provenance] [--pob SRC]
//! ```
//!
//! Without `--json`, `outputs` prints to stdout. PoB is found with
//! `--pob`, `$POB_SRC`, or `.PathOfBuilding/src` in an ancestor directory.
//! Every command that boots PoB holds the machine-wide oracle lock
//! (`runner::OracleLock`; skipped when `$POB_ORACLE_LOCK_HELD=1`, which
//! `scripts/port/oracle.sh` and `nondet` set for their children).
//! See crates/pob-oracle/README.md for the JSON formats.

use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::time::Instant;

use pob_oracle::oracle::{dump_in_process, find_parity_dir, find_pob_src, Dump};
use pob_oracle::patcher;
use pob_oracle::runner::{self, OracleLock, ProbeCache, ProbeOptions};

const USAGE: &str = "usage:
  pob_oracle outputs <build.xml> [--json OUT] [--pob SRC]
  pob_oracle stages  <build.xml>  --json OUT  [--pob SRC]
  pob_oracle all     <build.xml>  --out-dir DIR [--pob SRC]
  pob_oracle patch   [--stages F] [--pob SRC] [--root DIR]
  pob_oracle probe   <build.xml>  --out-dir DIR [--stages F | --patched SRC] [--select G,..]
                     [--max-dumps N] [--no-provenance] [--cache] [--pob SRC]
  pob_oracle nondet  <build.xml>  [--out FILE] [--keep DIR] [--verify] [--max-runs N] [--cache] [--stages F]
                     [--select G,..] [--no-provenance] [--pob SRC]";

fn write(path: &Path, s: &str) -> Result<(), String> {
    runner::write_atomic(path, s).map_err(|e| e.0)
}

#[derive(Default)]
struct Args {
    build: Option<PathBuf>,
    json: Option<PathBuf>,
    out_dir: Option<PathBuf>,
    out: Option<PathBuf>,
    keep: Option<PathBuf>,
    pob: Option<PathBuf>,
    stages: Option<PathBuf>,
    patched: Option<PathBuf>,
    root: Option<PathBuf>,
    select: Option<Vec<String>>,
    max_dumps: Option<usize>,
    no_provenance: bool,
    cache: bool,
    verify: bool,
    max_runs: Option<usize>,
}

fn parse(mut args: impl Iterator<Item = String>) -> Result<Args, String> {
    let mut a = Args::default();
    while let Some(x) = args.next() {
        let mut val = || {
            args.next()
                .ok_or_else(|| format!("{x} needs a value\n{USAGE}"))
        };
        match x.as_str() {
            "--json" => a.json = Some(val()?.into()),
            "--out-dir" => a.out_dir = Some(val()?.into()),
            "--out" => a.out = Some(val()?.into()),
            "--keep" => a.keep = Some(val()?.into()),
            "--pob" => a.pob = Some(val()?.into()),
            "--stages" => a.stages = Some(val()?.into()),
            "--patched" => a.patched = Some(val()?.into()),
            "--root" => a.root = Some(val()?.into()),
            "--select" => {
                a.select = Some(
                    val()?
                        .split(',')
                        .map(str::trim)
                        .filter(|s| !s.is_empty())
                        .map(str::to_owned)
                        .collect(),
                );
            }
            "--max-dumps" => {
                a.max_dumps = Some(val()?.parse().map_err(|e| format!("--max-dumps: {e}"))?);
            }
            "--no-provenance" => a.no_provenance = true,
            "--cache" => a.cache = true,
            "--verify" => a.verify = true,
            "--max-runs" => {
                a.max_runs = Some(val()?.parse().map_err(|e| format!("--max-runs: {e}"))?);
            }
            "-h" | "--help" => return Err(USAGE.into()),
            _ if a.build.is_none() && !x.starts_with("--") => a.build = Some(x.into()),
            _ => return Err(format!("unexpected argument {x}\n{USAGE}")),
        }
    }
    Ok(a)
}

fn pob_src(a: &Args) -> Result<PathBuf, String> {
    Ok(match &a.pob {
        Some(p) if p.join("src/HeadlessWrapper.lua").is_file() => p.join("src"),
        Some(p) => p.clone(),
        None => find_pob_src().ok_or("PoB not found: pass --pob or set POB_SRC")?,
    })
}

/// `--select` as given, else the default selection plus, for every line
/// probe of the patched copy, its first hit in the final main env.
fn probe_options(a: &Args, src: &Path) -> ProbeOptions {
    let mut o = ProbeOptions::default();
    if let Some(s) = &a.select {
        o.select = s.clone();
    } else if let Some(m) = patcher::manifest_of(src) {
        for site in m.sites.iter().filter(|s| s.kind == patcher::SiteKind::Line) {
            o.select.push(format!("{}@main", site.id));
        }
    }
    if let Some(m) = a.max_dumps {
        o.max_dumps = m;
    }
    o.provenance = !a.no_provenance;
    o
}

/// The patched copy: `--patched SRC` as given, else patch now.
fn patched(a: &Args) -> Result<(PathBuf, Option<String>), String> {
    if let Some(p) = &a.patched {
        let sha = patcher::manifest_of(p).map(|m| m.sha);
        return Ok((p.clone(), sha));
    }
    let src = pob_src(a)?;
    let root = a.root.clone().unwrap_or_else(patcher::default_root);
    let p = patcher::patch(&src, a.stages.as_deref(), &root).map_err(|e| e.0)?;
    Ok((p.src, Some(p.sha)))
}

fn run() -> Result<(), String> {
    let mut args = std::env::args().skip(1);
    let cmd = args.next().ok_or(USAGE)?;
    let a = parse(args)?;
    let t = Instant::now();
    match cmd.as_str() {
        "outputs" | "stages" | "all" => {
            let build = a.build.clone().ok_or(USAGE)?;
            let pob = pob_src(&a)?;
            let _lock = OracleLock::acquire().map_err(|e| e.0)?;
            match cmd.as_str() {
                "outputs" => {
                    let r = dump_in_process(&pob, &build, &[Dump::Outputs]).map_err(|e| e.0)?;
                    match &a.json {
                        Some(p) => write(p, &r[0])?,
                        None => println!("{}", r[0]),
                    }
                }
                "stages" => {
                    let p = a.json.clone().ok_or("stages needs --json OUT")?;
                    let r = dump_in_process(&pob, &build, &[Dump::Stages]).map_err(|e| e.0)?;
                    write(&p, &r[0])?;
                }
                _ => {
                    let dir = a.out_dir.clone().ok_or("all needs --out-dir DIR")?;
                    let r = dump_in_process(&pob, &build, &[Dump::Outputs, Dump::Stages])
                        .map_err(|e| e.0)?;
                    write(&dir.join("outputs.json"), &r[0])?;
                    write(&dir.join("stages.json"), &r[1])?;
                }
            }
            eprintln!("pob_oracle {cmd} {}: {:.2?}", build.display(), t.elapsed());
        }
        "patch" => {
            let src = pob_src(&a)?;
            let root = a.root.clone().unwrap_or_else(patcher::default_root);
            let p = patcher::patch(&src, a.stages.as_deref(), &root).map_err(|e| e.0)?;
            let mut kinds = std::collections::BTreeMap::new();
            for s in &p.manifest.sites {
                *kinds.entry(format!("{:?}", s.kind)).or_insert(0usize) += 1;
            }
            eprintln!(
                "patched {} files, sites {kinds:?}, sha {} ({:.2?})",
                p.manifest.files.len(),
                p.sha,
                t.elapsed()
            );
            println!("{}", p.src.display());
        }
        "probe" => {
            let build = a.build.clone().ok_or(USAGE)?;
            let out_dir = a.out_dir.clone().ok_or("probe needs --out-dir DIR")?;
            let (src, sha) = patched(&a)?;
            let opts = probe_options(&a, &src);
            let summary = {
                let _lock = OracleLock::acquire().map_err(|e| e.0)?;
                runner::probe_in_process(&src, &build, &out_dir, &opts).map_err(|e| e.0)?
            };
            if a.cache {
                let parity = find_parity_dir().ok_or("--cache: no .parity directory found")?;
                let sha = sha.ok_or("--cache: the patched source has no manifest")?;
                let cache = ProbeCache::new(&parity, &sha, &build).map_err(|e| e.0)?;
                let n = cache.store_run(&out_dir).map_err(|e| e.0)?;
                eprintln!("cached {n} dumps in {}", cache.dir.display());
            }
            eprintln!(
                "pob_oracle probe {}: {} dumps in {} ({:.2?})",
                build.display(),
                summary.dumps,
                summary.out_dir.display(),
                t.elapsed()
            );
        }
        "nondet" => {
            let build = a.build.clone().ok_or(USAGE)?;
            let (src, sha) = patched(&a)?;
            let opts = probe_options(&a, &src);
            let stem = build
                .file_stem()
                .map_or_else(|| "build".to_owned(), |s| s.to_string_lossy().into_owned());
            let out = match &a.out {
                Some(p) => p.clone(),
                None => find_parity_dir()
                    .ok_or("no .parity directory: pass --out FILE")?
                    .join("nondet")
                    .join(format!("{stem}.json")),
            };
            let bin = std::env::current_exe().map_err(|e| e.to_string())?;
            // next to --keep (same filesystem, so run 1 can be renamed there)
            let work = match &a.keep {
                Some(k) => k.with_extension(format!("work{}", std::process::id())),
                None => std::env::temp_dir()
                    .join(format!("pob_oracle_nondet_{}_{stem}", std::process::id())),
            };
            let outcome = runner::run_nondet(
                &bin,
                &build,
                &src,
                &opts,
                &work,
                a.verify,
                a.max_runs.unwrap_or(3),
            )
            .map_err(|e| e.0)?;
            outcome.record.save(&out).map_err(|e| e.0)?;
            if a.cache {
                let parity = find_parity_dir().ok_or("--cache: no .parity directory found")?;
                let sha = sha.ok_or("--cache: the patched source has no manifest")?;
                let cache = ProbeCache::new(&parity, &sha, &build).map_err(|e| e.0)?;
                cache.store_run(&work.join("run1")).map_err(|e| e.0)?;
            }
            if let Some(keep) = &a.keep {
                let _ = std::fs::remove_dir_all(keep);
                if let Some(d) = keep.parent() {
                    let _ = std::fs::create_dir_all(d);
                }
                if std::fs::rename(work.join("run1"), keep).is_err() {
                    return Err(format!("could not move run 1 to {}", keep.display()));
                }
            }
            let _ = std::fs::remove_dir_all(&work);
            println!(
                "{}",
                serde_json::json!({
                    "build": stem,
                    "runs": outcome.runs,
                    "entries": outcome.record.len(),
                    "pairEntries": outcome.pair.len(),
                    "uncovered": outcome.uncovered,
                    "uncoveredStructural": outcome.uncovered_structural,
                    "lastNew": outcome.last_new,
                    "out": out.display().to_string(),
                })
            );
            eprintln!("pob_oracle nondet {}: {:.2?}", build.display(), t.elapsed());
        }
        _ => return Err(format!("unknown command {cmd}\n{USAGE}")),
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
