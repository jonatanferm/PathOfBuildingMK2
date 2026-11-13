//! The E2E build goldens (`e2e.*`, GOLDEN-PLAN §1.2): every `e2e/` file MANIFEST lists, against
//! the Rust calc through `pob_golden::adapter`. Settings: `pob_golden::runner`.
//!
//! Also the crate's dependency rule (§3.3): no PoB machinery in pob-golden's graph.

use std::process::Command;

#[test]
fn golden_e2e() {
    pob_golden::runner::test_main("e2e");
}

/// `cargo tree` of this crate, normal and dev edges (or features).
fn cargo_tree(edges: &str) -> String {
    let cargo = std::env::var("CARGO").unwrap_or_else(|_| "cargo".to_owned());
    let manifest = concat!(env!("CARGO_MANIFEST_DIR"), "/Cargo.toml");
    let out = Command::new(cargo)
        .args([
            "tree",
            "--offline",
            "--manifest-path",
            manifest,
            "-p",
            "pob-golden",
            "-e",
            edges,
        ])
        .output()
        .expect("cargo tree");
    assert!(
        out.status.success(),
        "cargo tree -e {edges}: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    String::from_utf8_lossy(&out.stdout).into_owned()
}

/// §3.3: `cargo tree -p pob-golden -e normal,dev` names none of mlua, pob-oracle,
/// pob-ui-oracle, pob-dump and pob-testpaths, and no crate in it has its `probe` feature on.
#[test]
fn no_oracle_dependencies() {
    let tree = cargo_tree("normal,dev");
    let banned = [
        "mlua",
        "pob-oracle",
        "pob-ui-oracle",
        "pob-dump",
        "pob-testpaths",
    ];
    let hits: Vec<&str> = tree
        .lines()
        .filter(|l| {
            l.split(|c: char| !(c.is_alphanumeric() || c == '-' || c == '_'))
                .any(|w| banned.contains(&w))
        })
        .collect();
    assert!(
        hits.is_empty(),
        "PoB machinery in pob-golden's graph: {hits:?}"
    );
    assert!(
        tree.contains("pob-calc"),
        "the adapters' dependencies are missing:\n{tree}"
    );
    let features = cargo_tree("features");
    let probe: Vec<&str> = features
        .lines()
        .filter(|l| l.contains("feature \"probe\""))
        .collect();
    assert!(
        probe.is_empty(),
        "a `probe` feature in pob-golden's graph: {probe:?}"
    );
}
