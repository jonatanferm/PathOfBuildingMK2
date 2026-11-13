//! U45: `PartyTab`'s `notesDesc` is PoB's long string (PartyTab.lua:63-66), byte for byte.
//! Moved out of `src/tabs/party.rs` by VENDOR (product sources read no PoB checkout); like every
//! oracle-backed test it needs the checkout and goes with it at the cut-over.

#[test]
fn notes_desc_is_the_long_string() {
    // a missing checkout fails unless POB_ALLOW_SKIP=1 (CCR-0031); POB_ROOT / POB_SRC
    // override the search (worktrees do not hold the gitignored checkout)
    let Some(src) = pob_testpaths::require(
        pob_testpaths::pob_src(),
        "PoB checkout (.PathOfBuilding/src)",
    ) else {
        return;
    };
    let lua = std::fs::read_to_string(src.join("Classes/PartyTab.lua")).expect("PartyTab.lua");
    let a = lua.find("local notesDesc = [[").unwrap() + "local notesDesc = [[".len();
    let b = a + lua[a..].find("]]").unwrap();
    assert_eq!(&lua[a..b], pob_app::tabs::party::NOTES_DESC);
}
