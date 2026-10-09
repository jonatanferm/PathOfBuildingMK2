//! Generates the gallery module list from `src/gallery/*.rs` (ui doc §11.2, §12.1: gallery files
//! need no `mod` edits). Owner: U8. UI-U0: the minimal generator.

use std::fmt::Write as _;
use std::path::PathBuf;

fn main() {
    let dir = PathBuf::from(std::env::var("CARGO_MANIFEST_DIR").expect("CARGO_MANIFEST_DIR"))
        .join("src/gallery");
    println!("cargo:rerun-if-changed={}", dir.display());
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
    let list: Vec<String> = names.iter().map(|(n, _)| format!("{n:?}")).collect();
    let _ = writeln!(
        out,
        "/// Every gallery under src/gallery, by file stem.\npub const GALLERIES: &[&str] = &[{}];",
        list.join(", ")
    );
    let dest = PathBuf::from(std::env::var("OUT_DIR").expect("OUT_DIR")).join("galleries.rs");
    std::fs::write(dest, out).expect("write galleries.rs");
}
