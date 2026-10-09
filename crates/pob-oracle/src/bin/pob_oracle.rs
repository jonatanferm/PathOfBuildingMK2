//! PoB under LuaJIT as a parity oracle: one build per process.
//!
//! ```text
//! pob_oracle outputs <build.xml> [--json OUT] [--pob SRC]
//! pob_oracle stages  <build.xml>  --json OUT  [--pob SRC]
//! pob_oracle all     <build.xml>  --out-dir DIR [--pob SRC]   # DIR/{outputs,stages}.json
//! ```
//!
//! Without `--json`, `outputs` prints to stdout. PoB is found with
//! `--pob`, `$POB_SRC`, or `.PathOfBuilding/src` in an ancestor directory.
//! See crates/pob-oracle/README.md for the JSON format.

use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::time::Instant;

use pob_oracle::oracle::{dump_in_process, find_pob_src, Dump};

const USAGE: &str = "usage:
  pob_oracle outputs <build.xml> [--json OUT] [--pob SRC]
  pob_oracle stages  <build.xml>  --json OUT  [--pob SRC]
  pob_oracle all     <build.xml>  --out-dir DIR [--pob SRC]";

fn write(path: &Path, s: &str) -> Result<(), String> {
    if let Some(dir) = path.parent().filter(|d| !d.as_os_str().is_empty()) {
        std::fs::create_dir_all(dir).map_err(|e| format!("{}: {e}", dir.display()))?;
    }
    // Write-then-rename so a killed run never leaves a truncated dump.
    let tmp = path.with_extension("json.part");
    std::fs::write(&tmp, s).map_err(|e| format!("{}: {e}", tmp.display()))?;
    std::fs::rename(&tmp, path).map_err(|e| format!("{}: {e}", path.display()))
}

fn run() -> Result<(), String> {
    let mut args = std::env::args().skip(1);
    let cmd = args.next().ok_or(USAGE)?;
    let mut build: Option<PathBuf> = None;
    let mut json: Option<PathBuf> = None;
    let mut out_dir: Option<PathBuf> = None;
    let mut pob: Option<PathBuf> = None;
    while let Some(a) = args.next() {
        match a.as_str() {
            "--json" => json = Some(args.next().ok_or(USAGE)?.into()),
            "--out-dir" => out_dir = Some(args.next().ok_or(USAGE)?.into()),
            "--pob" => pob = Some(args.next().ok_or(USAGE)?.into()),
            "-h" | "--help" => return Err(USAGE.into()),
            _ if build.is_none() => build = Some(a.into()),
            _ => return Err(format!("unexpected argument {a}\n{USAGE}")),
        }
    }
    let build = build.ok_or(USAGE)?;
    let pob = match pob {
        Some(p) if p.join("src/HeadlessWrapper.lua").is_file() => p.join("src"),
        Some(p) => p,
        None => find_pob_src().ok_or("PoB not found: pass --pob or set POB_SRC")?,
    };
    let t = Instant::now();
    match cmd.as_str() {
        "outputs" => {
            let r = dump_in_process(&pob, &build, &[Dump::Outputs]).map_err(|e| e.0)?;
            match json {
                Some(p) => write(&p, &r[0])?,
                None => println!("{}", r[0]),
            }
        }
        "stages" => {
            let p = json.ok_or("stages needs --json OUT")?;
            let r = dump_in_process(&pob, &build, &[Dump::Stages]).map_err(|e| e.0)?;
            write(&p, &r[0])?;
        }
        "all" => {
            let dir = out_dir.ok_or("all needs --out-dir DIR")?;
            let r =
                dump_in_process(&pob, &build, &[Dump::Outputs, Dump::Stages]).map_err(|e| e.0)?;
            write(&dir.join("outputs.json"), &r[0])?;
            write(&dir.join("stages.json"), &r[1])?;
        }
        _ => return Err(format!("unknown command {cmd}\n{USAGE}")),
    }
    eprintln!("pob_oracle {cmd} {}: {:.2?}", build.display(), t.elapsed());
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
