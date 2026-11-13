//! `golden`: the goldens' review and maintenance tool (GOLDEN-PLAN §2.3, §2.4, §3.6).
//!
//! ```text
//! golden diff [<git-rev>]               the goldens at <git-rev> (default HEAD) -> the working tree
//! golden diff --old <dir> --new <dir>   two goldens directories
//! golden manifest check                 every listed file against the disk (hash, size, cap)
//! golden manifest refresh               re-read every listed file's hash, size and line count
//! golden manifest sizes <working-tree bytes> <packed bytes>
//!                                       record measured sizes (scripts/golden/size.sh)
//! golden sets                           the sets that have an adapter
//! ```
//!
//! `--root <dir>` (or `$POB_GOLDEN_DIR`) picks the goldens directory, default
//! `<workspace>/tests/golden`. `golden diff` exits 1 when the two sides differ, so
//! `golden diff origin/rust-port` doubles as the "golden diff = 0" check of §3.4.

use std::path::PathBuf;
use std::process::ExitCode;

use pob_golden::manifest::{Manifest, Sizes};

fn usage() -> ExitCode {
    eprintln!(
        "usage: golden [--root <dir>] diff [<git-rev> | --old <dir> --new <dir>]\n\
         \x20      golden [--root <dir>] manifest check|refresh|sizes <working-tree> <packed>\n\
         \x20      golden sets"
    );
    ExitCode::from(2)
}

fn fail(e: impl std::fmt::Display) -> ExitCode {
    eprintln!("golden: {e}");
    ExitCode::from(2)
}

fn main() -> ExitCode {
    let mut args: Vec<String> = std::env::args().skip(1).collect();
    let mut root = pob_golden::golden_root();
    if let Some(i) = args.iter().position(|a| a == "--root") {
        if i + 1 >= args.len() {
            return usage();
        }
        root = PathBuf::from(args.remove(i + 1));
        args.remove(i);
    }
    let a: Vec<&str> = args.iter().map(String::as_str).collect();
    match a.as_slice() {
        ["diff", rest @ ..] => {
            let sum = match rest {
                [] => pob_golden::diff::diff_rev(&root, "HEAD"),
                [rev] if !rev.starts_with("--") => pob_golden::diff::diff_rev(&root, rev),
                ["--old", o, "--new", n] | ["--new", n, "--old", o] => {
                    pob_golden::diff::diff_dirs(&PathBuf::from(o), &PathBuf::from(n))
                }
                _ => return usage(),
            };
            match sum {
                Ok(s) => {
                    print!("{}", s.render());
                    if s.is_empty() {
                        ExitCode::SUCCESS
                    } else {
                        ExitCode::from(1)
                    }
                }
                Err(e) => fail(e),
            }
        }
        ["manifest", "check"] => match Manifest::load(&root) {
            Ok(m) => {
                let problems = m.check(&root);
                for p in &problems {
                    println!("{p}");
                }
                println!(
                    "manifest: {} files, {} builds, {} problems",
                    m.files.len(),
                    m.builds.len(),
                    problems.len()
                );
                if problems.is_empty() {
                    ExitCode::SUCCESS
                } else {
                    ExitCode::from(1)
                }
            }
            Err(e) => fail(e),
        },
        ["manifest", "refresh"] => {
            let mut m = match Manifest::load(&root) {
                Ok(m) => m,
                Err(e) => return fail(e),
            };
            let paths: Vec<String> = m.files.iter().map(|f| f.path.clone()).collect();
            for p in &paths {
                if let Err(e) = m.refresh_file(&root, p) {
                    return fail(e);
                }
            }
            if let Err(e) = m.save(&root) {
                return fail(e);
            }
            println!("manifest: {} files refreshed", paths.len());
            ExitCode::SUCCESS
        }
        ["manifest", "sizes", w, p] => {
            let (Ok(w), Ok(p)) = (w.parse::<u64>(), p.parse::<u64>()) else {
                return usage();
            };
            let mut m = match Manifest::load(&root) {
                Ok(m) => m,
                Err(e) => return fail(e),
            };
            let (largest, largest_path) = m
                .files
                .iter()
                .map(|f| (f.bytes, f.path.clone()))
                .chain(m.builds.iter().map(|b| (b.bytes, b.path.clone())))
                .max()
                .unwrap_or_default();
            let commit = pob_golden::runner::head_commit(&root);
            let date = std::process::Command::new("date")
                .arg("+%Y-%m-%d")
                .output()
                .ok()
                .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_owned())
                .unwrap_or_default();
            m.sizes = Some(Sizes {
                working_tree: w,
                packed: p,
                largest_file: largest,
                largest_path,
                measured: format!("{commit} {date}"),
            });
            if let Err(e) = m.save(&root) {
                return fail(e);
            }
            let over: Vec<String> = m
                .check(&root)
                .into_iter()
                .filter(|x| x.contains("cap"))
                .collect();
            println!(
                "sizes: working tree {w} B (cap {}), packed {p} B (cap {}), largest file {largest} B (cap {})",
                m.caps.working_tree, m.caps.packed, m.caps.per_file
            );
            for o in &over {
                println!("{o}");
            }
            if over.is_empty() {
                ExitCode::SUCCESS
            } else {
                ExitCode::from(1)
            }
        }
        ["sets"] => {
            for s in pob_golden::adapter::registered_sets() {
                println!("{s}");
            }
            ExitCode::SUCCESS
        }
        _ => usage(),
    }
}
