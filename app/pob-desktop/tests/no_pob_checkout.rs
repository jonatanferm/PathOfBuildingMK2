//! VENDOR: the shipped apps read no PoB checkout. This test fails when product code names one
//! again: `POB_ROOT`, `.PathOfBuilding` or `pob_src` in the sources or manifests of any workspace
//! member except the oracle, dump and test-input crates (deleted at the cut-over) and the vendoring
//! tool (the one program that reads PoB, once). Test code is exempt: `tests/`, `benches/`,
//! `examples/` and `#[cfg(test)]` items. It also fails when a product crate gets a normal
//! (non-dev) dependency on one of the exempt crates.

use std::path::{Path, PathBuf};

/// Workspace members that may read the checkout (module docs).
const EXEMPT_MEMBERS: [&str; 5] = [
    "crates/pob-oracle",
    "crates/pob-ui-oracle",
    "crates/pob-dump",
    "crates/pob-testpaths",
    "tools/pob-vendor",
];

/// Crates a product crate must not depend on outside `[dev-dependencies]`.
const EXEMPT_CRATES: [&str; 5] = [
    "pob-oracle",
    "pob-ui-oracle",
    "pob-dump",
    "pob-testpaths",
    "pob-vendor",
];

const FORBIDDEN: [&str; 3] = ["POB_ROOT", ".PathOfBuilding", "pob_src"];

/// Directories of test code inside a member.
const TEST_DIRS: [&str; 3] = ["tests", "benches", "examples"];

fn workspace_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

/// The `members = [ … ]` of the workspace manifest.
fn members(root: &Path) -> Vec<String> {
    let text = std::fs::read_to_string(root.join("Cargo.toml")).expect("workspace Cargo.toml");
    let start = text.find("members = [").expect("workspace members") + "members = [".len();
    let end = start + text[start..].find(']').expect("end of members");
    text[start..end]
        .lines()
        .map(|l| l.split('#').next().unwrap_or_default().trim())
        .filter_map(|l| l.strip_prefix('"').and_then(|l| l.split('"').next()))
        .map(str::to_owned)
        .collect()
}

/// The text of a Rust source without its `#[cfg(test)]` items: an item whose first line ends
/// in `{` is dropped up to the `}` at its own indentation (rustfmt's layout); other items are
/// one line (`mod tests;`, `use …;`).
fn without_test_items(text: &str) -> String {
    let lines: Vec<&str> = text.lines().collect();
    let mut out = Vec::new();
    let mut i = 0;
    while i < lines.len() {
        let l = lines[i];
        if l.trim() == "#[cfg(test)]" {
            let indent = &l[..l.len() - l.trim_start().len()];
            let mut j = i + 1;
            while j < lines.len() && lines[j].trim_start().starts_with("#[") {
                j += 1;
            }
            let item = lines.get(j).copied().unwrap_or_default();
            if item.trim_end().ends_with('{') {
                let close = format!("{indent}}}");
                let mut k = j + 1;
                while k < lines.len() && lines[k] != close {
                    k += 1;
                }
                i = k + 1;
            } else {
                i = j + 1;
            }
            continue;
        }
        out.push(l);
        i += 1;
    }
    out.join("\n")
}

/// The `#[cfg(test)] mod name;` files of a source (`name.rs` / `name/mod.rs` next to it).
fn test_module_files(path: &Path, text: &str) -> Vec<PathBuf> {
    let lines: Vec<&str> = text.lines().collect();
    let dir = if matches!(
        path.file_name().and_then(|n| n.to_str()),
        Some("mod.rs" | "lib.rs" | "main.rs")
    ) {
        path.parent().map(Path::to_path_buf).unwrap_or_default()
    } else {
        path.with_extension("")
    };
    let mut out = Vec::new();
    for (i, l) in lines.iter().enumerate() {
        if l.trim() != "#[cfg(test)]" {
            continue;
        }
        let Some(item) = lines.get(i + 1).map(|s| s.trim()) else {
            continue;
        };
        let name = item
            .strip_prefix("pub ")
            .unwrap_or(item)
            .strip_prefix("mod ")
            .and_then(|r| r.strip_suffix(';'));
        if let Some(n) = name {
            out.push(dir.join(format!("{n}.rs")));
            out.push(dir.join(n).join("mod.rs"));
        }
    }
    out
}

fn walk(dir: &Path, out: &mut Vec<PathBuf>) {
    let Ok(rd) = std::fs::read_dir(dir) else {
        return;
    };
    let mut v: Vec<PathBuf> = rd.flatten().map(|e| e.path()).collect();
    v.sort();
    for p in v {
        if p.is_dir() {
            walk(&p, out);
        } else {
            out.push(p);
        }
    }
}

/// The product files of a member: its manifest, `build.rs`, the web page files, and every `.rs`
/// outside the test directories.
fn product_files(member: &Path) -> Vec<PathBuf> {
    let mut files = Vec::new();
    walk(member, &mut files);
    files
        .into_iter()
        .filter(|p| {
            let rel = p.strip_prefix(member).unwrap_or(p);
            let first = rel
                .components()
                .next()
                .and_then(|c| c.as_os_str().to_str())
                .unwrap_or_default();
            if TEST_DIRS.contains(&first) || first == "target" || first == "dist" {
                return false;
            }
            let name = p.file_name().and_then(|n| n.to_str()).unwrap_or_default();
            p.extension().is_some_and(|e| e == "rs")
                || matches!(name, "Cargo.toml" | "index.html" | "Trunk.toml")
        })
        .collect()
}

#[test]
fn product_code_names_no_pob_checkout() {
    let root = workspace_root();
    let product: Vec<String> = members(&root)
        .into_iter()
        .filter(|m| !EXEMPT_MEMBERS.contains(&m.as_str()))
        .collect();
    assert!(
        product.iter().any(|m| m == "crates/pob-app") && product.iter().any(|m| m == "app/pob-web"),
        "workspace members not found: {product:?}"
    );
    let mut hits = Vec::new();
    let mut scanned = 0;
    for m in &product {
        let dir = root.join(m);
        let files = product_files(&dir);
        let mut test_only: Vec<PathBuf> = Vec::new();
        for p in &files {
            if p.extension().is_some_and(|e| e == "rs") {
                let text = std::fs::read_to_string(p).unwrap_or_default();
                test_only.extend(test_module_files(p, &text));
            }
        }
        for p in &files {
            if test_only.contains(p) {
                continue;
            }
            let raw = std::fs::read_to_string(p).unwrap_or_default();
            let text = if p.extension().is_some_and(|e| e == "rs") {
                without_test_items(&raw)
            } else {
                raw
            };
            scanned += 1;
            for (n, line) in text.lines().enumerate() {
                for f in FORBIDDEN {
                    if line.contains(f) {
                        hits.push(format!(
                            "{}: `{f}` in: {}",
                            p.strip_prefix(&root).unwrap_or(p).display(),
                            line.trim()
                        ));
                    }
                }
                let _ = n;
            }
        }
    }
    eprintln!(
        "scanned {scanned} product files in {} members",
        product.len()
    );
    assert!(scanned > 300, "only {scanned} files scanned");
    assert!(
        hits.is_empty(),
        "product code names a PoB checkout again (VENDOR):\n{}",
        hits.join("\n")
    );
}

#[test]
fn product_crates_do_not_depend_on_the_oracles() {
    let root = workspace_root();
    let mut bad = Vec::new();
    for m in members(&root) {
        if EXEMPT_MEMBERS.contains(&m.as_str()) {
            continue;
        }
        let text = std::fs::read_to_string(root.join(&m).join("Cargo.toml")).unwrap_or_default();
        let mut section = String::new();
        for line in text.lines() {
            let t = line.trim();
            if t.starts_with('[') {
                section = t.to_owned();
                continue;
            }
            let normal = section == "[dependencies]"
                || (section.starts_with("[target.") && section.ends_with(".dependencies]"))
                || section == "[build-dependencies]";
            if !normal {
                continue;
            }
            let name = t.split(['=', ' ', '.']).next().unwrap_or_default();
            if EXEMPT_CRATES.contains(&name) {
                bad.push(format!("{m}: {section} {t}"));
            }
        }
    }
    assert!(
        bad.is_empty(),
        "product crates depend on oracle/test crates:\n{}",
        bad.join("\n")
    );
}

#[test]
fn the_scan_finds_what_it_must() {
    let src = "fn a() { let r = \"POB_ROOT\"; }\n#[cfg(test)]\nmod tests {\n    fn b() { \".PathOfBuilding\"; }\n}\nfn c() {}\n";
    let t = without_test_items(src);
    assert!(t.contains("POB_ROOT"));
    assert!(!t.contains(".PathOfBuilding"));
    assert!(t.contains("fn c()"));
    let files = test_module_files(Path::new("/x/src/lib.rs"), "#[cfg(test)]\nmod probe;\n");
    assert!(files.contains(&PathBuf::from("/x/src/probe.rs")));
}
