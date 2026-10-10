//! `cargo run -p pob-ui-oracle --bin ui-parity -- <scenario.pobscript | --gallery name> [--dpi 1.5] [--png]`
//! (ui doc §11.2 CLI). Owner: U8.
//!
//! Runs both sides (PoB's Lua under LuaJIT and the Rust port) through the same driver and writes
//! `target/ui-parity/<name>/{lua,rust}.jsonl` and `report.txt` (first divergence with ±5 ops, both
//! `src` locations, control path), plus `frame_<n>_{lua,rust,diff}.png` with `--png`.
//!
//! Options:
//!
//! * `--dpi S`: OS scale; `--dpi 1.5` alone selects the second acceptance screen 1920×1080 @1.5
//!   (§11.3), any other value keeps 1280×800. Default 1280×800 @1.0.
//! * `--screen WxH`: framebuffer size (overrides the default for the dpi).
//! * `--png`: render the first divergent frame and the last frame of both sides.
//! * `--audit`: audit mode (§11.2): Lua side only, patch 6 off. Native hash order is randomised
//!   per process (calc doc D8), so the run is made twice, each in a fresh child process
//!   (`--audit-child N`, writing `audit_run<N>.txt`); `audit.txt` is the union: every host whose
//!   native draw order differs from the canonical one while two of its controls overlap.
//! * `--lua-only`: skip the Rust side (e.g. to inspect the Lua trace of a scenario).
//! * `--out DIR`: output root instead of `target/ui-parity`.
//! * `--name NAME`: report directory name (default: the gallery name or the script's file stem).
//!
//! A `screen` step in the script overrides the CLI screen from that step on.
//!
//! Exit status: 0 = zero divergences and the Rust side ran to the end; 1 = divergences or a Rust
//! side error; 2 = usage or Lua-side failure.

use std::path::{Path, PathBuf};
use std::process::ExitCode;

use pob_ui_oracle::diff_report;
use pob_ui_oracle::runner::{self, RunOptions, Target};

struct Args {
    target: Target,
    name: String,
    opts: RunOptions,
    png: bool,
    out: PathBuf,
    /// `--audit-child N`: one audit run of the parent's two.
    audit_child: Option<u32>,
}

fn usage() -> ExitCode {
    eprintln!(
        "usage: ui-parity <scenario.pobscript | --gallery NAME> [--dpi S] [--screen WxH] [--png] [--audit] [--lua-only] [--out DIR] [--name NAME]"
    );
    ExitCode::from(2)
}

/// `target/ui-parity` of this workspace (`$CARGO_TARGET_DIR` if set).
fn default_out() -> PathBuf {
    let target = std::env::var_os("CARGO_TARGET_DIR").map_or_else(
        || PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../target"),
        PathBuf::from,
    );
    target.join("ui-parity")
}

fn parse_args() -> Result<Args, String> {
    let mut it = std::env::args().skip(1);
    let mut script: Option<PathBuf> = None;
    let mut gallery: Option<String> = None;
    let mut dpi: f32 = 1.0;
    let mut size: Option<(i32, i32)> = None;
    let mut png = false;
    let mut audit = false;
    let mut lua_only = false;
    let mut out = default_out();
    let mut name: Option<String> = None;
    let mut audit_child: Option<u32> = None;
    while let Some(a) = it.next() {
        match a.as_str() {
            "--gallery" => gallery = Some(it.next().ok_or("--gallery needs a name")?),
            "--dpi" => {
                dpi = it
                    .next()
                    .and_then(|v| v.parse().ok())
                    .ok_or("--dpi needs a number")?;
            }
            "--screen" => {
                let v = it.next().ok_or("--screen needs WxH")?;
                let (w, h) = v.split_once('x').ok_or("--screen needs WxH")?;
                size = Some((
                    w.parse().map_err(|_| "bad --screen width")?,
                    h.parse().map_err(|_| "bad --screen height")?,
                ));
            }
            "--png" => png = true,
            "--audit" => audit = true,
            "--lua-only" => lua_only = true,
            "--out" => out = PathBuf::from(it.next().ok_or("--out needs a directory")?),
            "--name" => name = Some(it.next().ok_or("--name needs a value")?),
            "--audit-child" => {
                audit = true;
                audit_child = Some(
                    it.next()
                        .and_then(|v| v.parse().ok())
                        .ok_or("--audit-child needs a run number")?,
                );
            }
            s if s.starts_with('-') => return Err(format!("unknown option {s}")),
            s => {
                if script.is_some() {
                    return Err("more than one scenario given".into());
                }
                script = Some(PathBuf::from(s));
            }
        }
    }
    let (w, h) = size.unwrap_or(if (dpi - 1.5).abs() < f32::EPSILON {
        (1920, 1080)
    } else {
        (1280, 800)
    });
    let (target, default_name) = match (script, gallery) {
        (Some(p), None) => {
            let text = std::fs::read_to_string(&p).map_err(|e| format!("{}: {e}", p.display()))?;
            let s = runner::parse_script(&text).map_err(|e| format!("{}: {e}", p.display()))?;
            let stem = Path::new(&p).file_stem().map_or_else(
                || "scenario".to_owned(),
                |s| s.to_string_lossy().into_owned(),
            );
            (Target::Scenario(s), stem)
        }
        (None, Some(g)) => (Target::Gallery(g.clone()), g),
        _ => return Err("give exactly one of <scenario.pobscript> and --gallery NAME".into()),
    };
    Ok(Args {
        target,
        name: name.unwrap_or(default_name),
        opts: RunOptions {
            screen: runner::screen(w, h, dpi),
            audit,
            lua_only: lua_only || audit,
        },
        png,
        out,
        audit_child,
    })
}

/// The audit parent: two child processes, then the union of their host lists.
fn audit_parent(args: &Args) -> ExitCode {
    let dir = args.out.join(&args.name);
    let exe = match std::env::current_exe() {
        Ok(e) => e,
        Err(e) => {
            eprintln!("ui-parity: current_exe: {e}");
            return ExitCode::from(2);
        }
    };
    let mut hits: Vec<pob_ui_oracle::markers::AuditHit> = Vec::new();
    for run in 1..=2u32 {
        let status = std::process::Command::new(&exe)
            .args(std::env::args().skip(1))
            .args(["--audit-child", &run.to_string()])
            .status();
        match status {
            Ok(s) if s.success() => {}
            Ok(s) => {
                eprintln!("ui-parity: audit run {run} failed ({s})");
                return ExitCode::from(2);
            }
            Err(e) => {
                eprintln!("ui-parity: audit run {run}: {e}");
                return ExitCode::from(2);
            }
        }
        match diff_report::read_audit(&dir.join(format!("audit_run{run}.txt"))) {
            Ok(h) => hits.extend(h),
            Err(e) => {
                eprintln!("ui-parity: audit run {run}: {e:#}");
                return ExitCode::from(2);
            }
        }
    }
    let merged = diff_report::merge_audit(hits);
    if let Err(e) = diff_report::write_audit(&dir.join("audit.txt"), &merged) {
        eprintln!("ui-parity: writing audit.txt: {e:#}");
        return ExitCode::from(2);
    }
    println!(
        "{}: audit (2 runs): {} hosts with order-dependent overlap -> {}",
        args.name,
        merged.len(),
        dir.join("audit.txt").display()
    );
    ExitCode::SUCCESS
}

fn main() -> ExitCode {
    let args = match parse_args() {
        Ok(a) => a,
        Err(e) => {
            eprintln!("ui-parity: {e}");
            return usage();
        }
    };
    if args.opts.audit && args.audit_child.is_none() {
        return audit_parent(&args);
    }
    let run = match runner::run_with(&args.target, &args.opts) {
        Ok(r) => r,
        Err(e) => {
            eprintln!("ui-parity: {e:#}");
            return ExitCode::from(2);
        }
    };
    let dir = args.out.join(&args.name);
    let divergences = match diff_report::write_report(&args.out, &args.name, &run, args.png) {
        Ok(n) => n,
        Err(e) => {
            eprintln!("ui-parity: writing the report: {e:#}");
            return ExitCode::from(2);
        }
    };
    if let Some(n) = args.audit_child {
        let file = dir.join(format!("audit_run{n}.txt"));
        if let Err(e) = diff_report::write_audit(&file, &run.audit) {
            eprintln!("ui-parity: writing {}: {e:#}", file.display());
            return ExitCode::from(2);
        }
        println!(
            "{}: audit run {n}: {} frames, {} hosts -> {}",
            args.name,
            run.lua.len(),
            run.audit.len(),
            file.display()
        );
        return ExitCode::SUCCESS;
    }
    if args.opts.lua_only {
        println!(
            "{}: Lua side only: {} frames, {} calls -> {}",
            args.name,
            run.lua.len(),
            run.lua.iter().map(|f| f.calls.len()).sum::<usize>(),
            dir.display()
        );
        return ExitCode::SUCCESS;
    }
    println!(
        "{}: {} divergences (frames lua {}, rust {}){} -> {}",
        args.name,
        divergences,
        run.lua.len(),
        run.rust.len(),
        run.rust_error
            .as_ref()
            .map_or(String::new(), |e| format!("; rust side error: {e}")),
        dir.join("report.txt").display()
    );
    if divergences == 0 && run.rust_error.is_none() {
        ExitCode::SUCCESS
    } else {
        ExitCode::from(1)
    }
}
