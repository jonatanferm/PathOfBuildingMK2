//! `pob-dump` CLI.
//!
//! ```text
//! pob-dump [--pob <PoB src dir>] [--out <dir>] [--compact] [--skip-trees] [--skip-parse-oracle]
//! pob-dump [--pob <PoB src dir>] [--out <dir>] --parse-oracle-only
//! pob-dump [--pob <PoB src dir>] --codegen [--check] [--generated <dir>]
//! pob-dump [--pob <PoB src dir>] --codegen-enums <out.rs>
//! ```
//! Defaults: `--pob .PathOfBuilding/src`, `--out data/dump` (relative to the
//! current directory), `--generated crates/pob-dump/generated` (this crate's).

use std::path::PathBuf;

use anyhow::{bail, Context, Result};

const HELP: &str = "\
pob-dump [--pob <PoB src>] [--out <dir>] [--compact] [--skip-trees] [--skip-parse-oracle]
    dump PoB's loaded data (and parse_oracle.jsonl) into <dir>
pob-dump [--pob <PoB src>] [--out <dir>] --parse-oracle-only
    (re)write only <dir>/parse_oracle.jsonl and its manifest entry
pob-dump [--pob <PoB src>] --codegen [--check] [--generated <dir>]
    write the generated Rust files (enums.rs, global.rs, misc.rs) into <dir>;
    with --check, write nothing and fail when a committed file differs
pob-dump [--pob <PoB src>] --codegen-enums <out.rs>
    write only enums.rs to <out.rs>";

fn main() -> Result<()> {
    let mut pob = PathBuf::from(".PathOfBuilding/src");
    let mut out = PathBuf::from("data/dump");
    let mut pretty = true;
    let mut skip_trees = false;
    let mut parse_oracle = true;
    let mut parse_oracle_only = false;
    let mut codegen_enums: Option<PathBuf> = None;
    let mut codegen = false;
    let mut check = false;
    let mut generated = pob_dump::codegen::default_dir();
    let mut args = std::env::args().skip(1);
    while let Some(a) = args.next() {
        match a.as_str() {
            "--pob" => pob = args.next().context("--pob needs a path")?.into(),
            "--out" => out = args.next().context("--out needs a path")?.into(),
            "--compact" => pretty = false,
            "--skip-trees" => skip_trees = true,
            "--skip-parse-oracle" => parse_oracle = false,
            "--parse-oracle-only" => parse_oracle_only = true,
            "--codegen" => codegen = true,
            "--check" => check = true,
            "--generated" => generated = args.next().context("--generated needs a path")?.into(),
            "--codegen-enums" => {
                codegen_enums = Some(args.next().context("--codegen-enums needs a path")?.into());
            }
            "-h" | "--help" => {
                println!("{HELP}");
                return Ok(());
            }
            other => bail!("unknown argument {other:?} (see --help)"),
        }
    }
    if check && !codegen {
        bail!("--check goes with --codegen");
    }
    if let Some(path) = codegen_enums {
        let src = pob_dump::codegen::codegen_enums(&pob)?;
        std::fs::write(&path, src).with_context(|| format!("write {}", path.display()))?;
        eprintln!("wrote {}", path.display());
        return Ok(());
    }
    if codegen {
        if check {
            let stale = pob_dump::codegen::check_all(&pob, &generated)?;
            if !stale.is_empty() {
                for s in &stale {
                    eprintln!("stale: {}/{s}", generated.display());
                }
                bail!(
                    "{} generated file(s) out of date; run `pob-dump --codegen`",
                    stale.len()
                );
            }
            println!(
                "generated files in {} are up to date: {:?}",
                generated.display(),
                pob_dump::codegen::GENERATED
            );
        } else {
            for p in pob_dump::codegen::write_all(&pob, &generated)? {
                println!("wrote {}", p.display());
            }
        }
        return Ok(());
    }
    if parse_oracle_only {
        let po = pob_dump::parse_oracle::run(&pob, &pob_dump::parse_oracle::Options::default())?;
        let info = po.write(&out)?;
        pob_dump::update_manifest_parse_oracle(&out, &info, &po)?;
        println!(
            "pob-dump: {} ({} records, {:.1} MB)",
            out.join(&info.name).display(),
            po.lines.len(),
            info.bytes as f64 / 1e6
        );
        println!("{}", serde_json::to_string_pretty(&po.info)?);
        return Ok(());
    }
    let report = pob_dump::dump(&pob_dump::Options {
        pob_src: pob,
        out: out.clone(),
        pretty,
        skip_trees,
        parse_oracle,
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
