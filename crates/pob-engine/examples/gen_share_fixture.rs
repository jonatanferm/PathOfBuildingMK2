//! One-shot helper used to regenerate
//! `tests/fixtures/pobb_in_share_code.txt`. Run with
//! `cargo run -p pob-engine --example gen_share_fixture > crates/pob-engine/tests/fixtures/pobb_in_share_code.txt`.
//! The fixture is the smallest exporter output we can produce —
//! a default Scion / level 1 character with a one-line note. The
//! Issue #33 share-URL tests just need a body that round-trips
//! through `import_pob_code`; nothing in the tree / items / skills
//! is exercised here.

use pob_engine::{export_pob_code, Character, ClassRef};

fn main() {
    let mut c = Character::new(ClassRef::scion(), 1);
    c.notes = "issue-33 share-url fixture".into();
    let code = export_pob_code(&c).expect("export pob code");
    print!("{code}");
}
