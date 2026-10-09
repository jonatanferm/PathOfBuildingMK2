//! T3/T4 parity runner (ui doc §11.2, §11.3): discovers `src/gallery/*` and
//! `scenarios/<unit>/*.pobscript` automatically, so units never edit shared files. Owner: U8.
//! UI-U0: discovery only (the gallery list is build.rs output; there are no galleries yet).

#[test]
fn galleries_are_discovered() {
    // Every generated gallery module name is unique and non-empty.
    let mut names: Vec<&str> = pob_ui_oracle::gallery::GALLERIES.to_vec();
    names.sort_unstable();
    names.dedup();
    assert_eq!(names.len(), pob_ui_oracle::gallery::GALLERIES.len());
    assert!(names.iter().all(|n| !n.is_empty()));
}
