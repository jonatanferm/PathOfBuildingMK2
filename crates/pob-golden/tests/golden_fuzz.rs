//! The fuzz output sets (`fuzz.*`, GOLDEN-PLAN §1.4): every `fuzz/` file MANIFEST lists,
//! against the Rust side through `pob_golden::adapter`. Settings: `pob_golden::runner`.

#[test]
fn golden_fuzz() {
    pob_golden::runner::test_main("fuzz");
}
