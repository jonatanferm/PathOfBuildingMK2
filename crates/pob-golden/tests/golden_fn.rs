//! The function-level query sets (`fn.*`, GOLDEN-PLAN §1.3): every `fn/` file MANIFEST lists,
//! against the Rust side through `pob_golden::adapter`. Settings: `pob_golden::runner`.

#[test]
fn golden_fn() {
    pob_golden::runner::test_main("fn");
}
