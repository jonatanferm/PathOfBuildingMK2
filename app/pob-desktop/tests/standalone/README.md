# Standalone test inputs (VENDOR)

`scenarios/` are the 15 U53 e2e scenarios (`crates/pob-ui-oracle/scenarios/e2e`, copied
2026-11-13 so that they outlive the oracle crate; only the `fixture userdir=` path changed), and
`user1/` is their fixture user dir (`crates/pob-ui-oracle/fixtures/user1`). `../standalone.rs`
runs them on the real app with the asset bundle and no PoB checkout. The `#! expect-src` and
`#! expect-snapshot` lines are the oracle's and are ignored here; `#! expect-text` is checked
against the last frame.
