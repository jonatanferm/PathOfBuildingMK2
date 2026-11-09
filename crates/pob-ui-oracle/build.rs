//! Generates the gallery module list from `src/gallery/*.rs` (ui doc §11.2, §12.1: gallery files
//! need no `mod` edits). Owner: U8.
//!
//! Every `src/gallery/<name>.rs` becomes `pub mod <name>;`, is listed in `GALLERIES` and is
//! reachable through `rust_app(name, parts)`, which calls the module's
//! `pub fn app(parts: pob_host::PlatformParts) -> Box<dyn pob_host::App>`. A file whose name starts
//! with `_` is a support module shared by galleries: it becomes `pub mod _<x>;` but is not a
//! gallery.
//!
//! Rerun: cargo re-runs this script when the directory listing changes (the directory's mtime).
//! If `src/gallery` does not exist yet, the script watches `src` instead (whose mtime changes when
//! the directory is created): watching a missing path would make cargo re-run it on every build.

use std::fmt::Write as _;
use std::path::PathBuf;

fn main() {
    let manifest = PathBuf::from(std::env::var("CARGO_MANIFEST_DIR").expect("CARGO_MANIFEST_DIR"));
    let dir = manifest.join("src/gallery");
    if dir.is_dir() {
        println!("cargo:rerun-if-changed={}", dir.display());
    } else {
        println!("cargo:rerun-if-changed={}", manifest.join("src").display());
    }
    println!("cargo:rerun-if-changed=build.rs");
    let mut names: Vec<(String, PathBuf)> = std::fs::read_dir(&dir)
        .map(|rd| {
            rd.filter_map(Result::ok)
                .map(|e| e.path())
                .filter(|p| p.extension().is_some_and(|x| x == "rs"))
                .filter_map(|p| Some((p.file_stem()?.to_str()?.to_owned(), p)))
                .collect()
        })
        .unwrap_or_default();
    names.sort();
    let mut out = String::new();
    for (name, path) in &names {
        let _ = writeln!(
            out,
            "#[path = {:?}]\npub mod {name};",
            path.display().to_string()
        );
    }
    let galleries: Vec<&String> = names
        .iter()
        .map(|(n, _)| n)
        .filter(|n| !n.starts_with('_'))
        .collect();
    let list: Vec<String> = galleries.iter().map(|n| format!("{n:?}")).collect();
    let _ = writeln!(
        out,
        "/// Every gallery under src/gallery, by file stem.\npub const GALLERIES: &[&str] = &[{}];",
        list.join(", ")
    );
    let _ = writeln!(
        out,
        "/// The Rust side of gallery `name` (`src/gallery/<name>.rs`'s `app`).\n\
         #[allow(clippy::single_match_else, clippy::match_single_binding)]\n\
         pub fn rust_app(name: &str, parts: pob_host::PlatformParts) -> Option<Box<dyn pob_host::App>> {{\n    match name {{"
    );
    for n in &galleries {
        let _ = writeln!(out, "        {n:?} => Some({n}::app(parts)),");
    }
    let _ = writeln!(out, "        _ => {{ let _ = parts; None }}\n    }}\n}}");
    let dest = PathBuf::from(std::env::var("OUT_DIR").expect("OUT_DIR")).join("galleries.rs");
    std::fs::write(dest, out).expect("write galleries.rs");
}
