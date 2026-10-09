//! Guard for AMENDMENTS A6 / CCR-0001: the test-only `oracle-dualnum` feature is
//! enabled solely by pob-lua's own dev-dependency on itself. No shipping crate
//! (and no other manifest in the workspace) may name it.
//!
//! A textual scan of every workspace manifest: `oracle-dualnum` may appear only
//! in `crates/pob-lua/Cargo.toml`, and there only as the `[features]` definition
//! (`oracle-dualnum = []`) and in the `[dev-dependencies]` self-dependency.

use std::path::{Path, PathBuf};

fn workspace_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .canonicalize()
        .expect("workspace root")
}

/// The root manifest plus every `crates/*/Cargo.toml` and `app/*/Cargo.toml`.
fn manifests(root: &Path) -> Vec<PathBuf> {
    let mut out = vec![root.join("Cargo.toml")];
    for dir in ["crates", "app"] {
        let Ok(rd) = std::fs::read_dir(root.join(dir)) else { continue };
        for e in rd.flatten() {
            let m = e.path().join("Cargo.toml");
            if m.is_file() {
                out.push(m);
            }
        }
    }
    out.sort();
    out
}

/// `(section, line)` for every non-comment line naming the feature.
fn hits(text: &str) -> Vec<(String, String)> {
    let mut section = String::new();
    let mut out = Vec::new();
    for raw in text.lines() {
        let line = raw.split('#').next().unwrap_or("").trim();
        if line.starts_with('[') {
            section = line.to_string();
            continue;
        }
        if line.contains("oracle-dualnum") {
            out.push((section.clone(), line.to_string()));
        }
    }
    out
}

#[test]
fn oracle_dualnum_enabled_only_by_pob_lua_dev_deps() {
    let root = workspace_root();
    let own = root.join("crates/pob-lua/Cargo.toml");
    let all = manifests(&root);
    assert!(all.contains(&own), "pob-lua manifest not found under {}", root.display());
    assert!(all.len() > 10, "workspace scan found only {} manifests", all.len());

    let mut bad = Vec::new();
    let mut own_def = false;
    let mut own_dev = false;
    for m in &all {
        let text = std::fs::read_to_string(m).expect("read manifest");
        for (section, line) in hits(&text) {
            let ok = if *m == own {
                match section.as_str() {
                    "[features]" if line == "oracle-dualnum = []" => {
                        own_def = true;
                        true
                    }
                    "[dev-dependencies]" if line.starts_with("pob-lua ") => {
                        own_dev = true;
                        true
                    }
                    _ => false,
                }
            } else {
                false
            };
            if !ok {
                bad.push(format!("{} {section}: {line}", m.strip_prefix(&root).unwrap_or(m).display()));
            }
        }
    }
    assert!(
        bad.is_empty(),
        "`oracle-dualnum` is test-only (AMENDMENTS A6); not allowed here:\n{}",
        bad.join("\n")
    );
    assert!(own_def, "pob-lua [features] no longer defines `oracle-dualnum = []`");
    assert!(own_dev, "pob-lua [dev-dependencies] no longer enables `oracle-dualnum` on itself");
}
