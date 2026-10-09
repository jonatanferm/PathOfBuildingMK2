//! `pob-dump` CLI.
//!
//! ```text
//! pob-dump [--pob <PoB src dir>] [--out <dir>] [--compact] [--skip-trees]
//! pob-dump [--pob <PoB src dir>] --codegen-enums <out.rs>
//! ```
//! Defaults: `--pob .PathOfBuilding/src`, `--out data/dump` (relative to the
//! current directory).

use std::path::PathBuf;

use anyhow::{bail, Context, Result};

fn main() -> Result<()> {
    let mut pob = PathBuf::from(".PathOfBuilding/src");
    let mut out = PathBuf::from("data/dump");
    let mut pretty = true;
    let mut skip_trees = false;
    let mut codegen: Option<PathBuf> = None;
    let mut args = std::env::args().skip(1);
    while let Some(a) = args.next() {
        match a.as_str() {
            "--pob" => pob = args.next().context("--pob needs a path")?.into(),
            "--out" => out = args.next().context("--out needs a path")?.into(),
            "--compact" => pretty = false,
            "--skip-trees" => skip_trees = true,
            "--codegen-enums" => {
                codegen = Some(args.next().context("--codegen-enums needs a path")?.into());
            }
            "-h" | "--help" => {
                println!(
                    "pob-dump [--pob <PoB src>] [--out <dir>] [--compact] [--skip-trees]\n\
                     pob-dump [--pob <PoB src>] --codegen-enums <out.rs>"
                );
                return Ok(());
            }
            other => bail!("unknown argument {other:?} (see --help)"),
        }
    }
    if let Some(path) = codegen {
        let src = pob_dump::codegen::codegen_enums(&pob)?;
        std::fs::write(&path, src).with_context(|| format!("write {}", path.display()))?;
        eprintln!("wrote {}", path.display());
        return Ok(());
    }
    let report = pob_dump::dump(&pob_dump::Options {
        pob_src: pob,
        out: out.clone(),
        pretty,
        skip_trees,
    })?;
    println!(
        "pob-dump: {} files, {:.1} MB in {}",
        report.files.len(),
        report.total_bytes() as f64 / 1e6,
        out.display()
    );
    for (k, v) in &report.timings {
        println!("  time {k}: {v:.2}s");
    }
    println!("  ModCache entries: {}", report.modcache_entries);
    println!("  closures: {}", report.closures);
    for (fam, n) in &report.families {
        println!("    {fam}: {n}");
    }
    for (k, v) in &report.stats {
        println!("  {k}: {v}");
    }
    for (f, why) in &report.skipped {
        println!("  skipped {f}: {why}");
    }
    Ok(())
}
