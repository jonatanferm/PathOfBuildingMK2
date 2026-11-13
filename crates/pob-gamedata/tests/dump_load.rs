//! `data.bin`: the strict schema, the pinned counts, the round trip and the runtime load time.
//!
//! GOLD1 merged U0's two PoB-free tests in here (docs/standalone/GOLDEN-PLAN.md §1.7), from
//! `crates/pob-oracle/tests/U0.rs`:
//!
//! * `compile_zero_unknown_keys`: `data.bin` decodes and loads into the strict schema
//!   (`pob_gamedata::Data`, `deny_unknown_fields` everywhere, every root typed or listed as
//!   dynamic), with the counts the docs pin (calc doc §13, AMENDMENTS A2); it encodes and
//!   decodes back to an equal tree; and the runtime load is timed (< 1 s gate in optimised
//!   builds, calc doc §11.1 U0, which CI's release perf job enforces). U0 started from PoB's
//!   JSON dump (`data/dump/*.json`); that part guarded the dumper and is gone (§1.7). This test
//!   also took over the old `data_bin_loads_fast` (the same decode, load and gate).
//! * `unknown_key_is_an_error`: an unknown key fails the load and names its Lua path.
//!
//! Run with `scripts/port/cargo.sh test --release -p pob-gamedata --test dump_load -- --nocapture`
//! for the time gate. Needs `data.bin`, found by `pob_testpaths` (`$POB_DUMP_DIR`, this
//! workspace, the main checkout) until GOLD13 points it at the asset bundle (§1.8); a missing
//! file fails the test unless `POB_ALLOW_SKIP=1`.

use std::time::Instant;

#[test]
fn compile_zero_unknown_keys() {
    let Some(path) = pob_testpaths::require(
        pob_testpaths::data_bin(),
        "data/dump/data.bin (run `pob-dump --compile`)",
    ) else {
        return;
    };
    let bytes = std::fs::read(&path).unwrap();
    let t = Instant::now();
    let tree = pob_gamedata::loader::decode_bin(&bytes).unwrap_or_else(|e| panic!("{e}"));
    let data = pob_gamedata::Data::from_tree(&tree)
        .unwrap_or_else(|e| panic!("schema (unknown key or root): {e}"));
    let t_load = t.elapsed().as_secs_f64();
    // Counts the docs pin (calc doc §13, AMENDMENTS A2).
    assert_eq!(data.gems.len(), 871, "gems (822 + 49 Vaal AltX/AltY)");
    assert!(data.skills.len() > 1500, "skills: {}", data.skills.len());
    assert_eq!(data.modCache.len(), 23207, "ModCache lines");
    assert_eq!(data.gameVersions.treeVersionList.len(), 41, "tree versions");
    assert_eq!(pob_gamedata::SkillType::Area, 8);
    assert_eq!(pob_gamedata::SkillType::Aura, 39);
    assert_eq!(pob_gamedata::SkillType::Pact, 135);
    // shared Arcs follow Lua identity: a gem's grantedEffect is the data.skills entry
    let fb = &data.gems[&pob_gamedata::Name::from("Metadata/Items/Gems/SkillGemFireball")];
    assert!(std::sync::Arc::ptr_eq(
        &fb.grantedEffect,
        &data.skills[&pob_gamedata::Name::from("Fireball")]
    ));
    // the calc-read roots CCR-0011 §7 types (they are not in `Data::other`)
    {
        use pob_gamedata::schema::item_data::EnchantmentList;
        use pob_gamedata::Name;
        let e = &data.enchantments;
        assert!(std::sync::Arc::ptr_eq(
            e.get("UtilityFlask").unwrap(),
            e.get("Flask").unwrap()
        ));
        assert!(matches!(
            e.get("Helmet").unwrap()[&Name::from("Arc")],
            EnchantmentList::BySource(_)
        ));
        assert!(matches!(
            e.get("Claw").unwrap()[&Name::from("HARVEST")],
            EnchantmentList::Lines(_)
        ));
        assert_eq!(data.foulbornMap.0.len(), 248);
        assert_eq!(data.rareLikeUniques.0.len(), 8);
        assert!(data
            .casterTagCrucibleUniques
            .contains(&Name::from("Mjolner")));
        assert_eq!(data.bossStats.UberArmourMean, 125.0);
        assert_eq!(data.nodeIDList.nodes.len(), 1937);
        assert_eq!(data.nodeIDList.size, 1937.0);
        assert_eq!(data.powerStatList.stats.len(), 85);
        assert!(data.modScalability.len() > 15000);
        let fb = &data.gemsByGameId.0[&Name::from("Metadata/Items/Gems/SkillGemFireball")];
        assert!(fb
            .values()
            .any(|g| std::sync::Arc::ptr_eq(g, &data.gems[&g.id])));
        for root in pob_gamedata::data::DYNAMIC_ROOTS {
            assert!(
                !pob_gamedata::data::TYPED_ROOTS.contains(root),
                "{root} both typed and dynamic"
            );
        }
    }
    eprintln!(
        "data.bin {:.1} MB: {} roots ({} dynamic), {} skills, {} gems; decode + typed load {t_load:.3}s",
        bytes.len() as f64 / 1e6,
        tree.roots.len(),
        data.other.len(),
        data.skills.len(),
        data.gems.len(),
    );
    drop(data);

    // encode and decode to an equal tree: the encoding is deterministic (roots in order, every
    // table a list, the ref targets sorted), so equal bytes are an equal tree, NaNs included
    let again = pob_gamedata::loader::encode_bin(&tree).unwrap();
    assert!(
        again == bytes,
        "data.bin does not re-encode to the same bytes"
    );
    let tree2 = pob_gamedata::loader::decode_bin(&again).unwrap();
    assert_eq!(
        pob_gamedata::loader::encode_bin(&tree2).unwrap().len(),
        bytes.len(),
        "the re-decoded tree encodes to another size"
    );
    assert!(
        tree2.roots.keys().eq(tree.roots.keys()),
        "the re-decoded roots are in another order"
    );
    let mut t1: Vec<_> = tree.index.keys().collect();
    let mut t2: Vec<_> = tree2.index.keys().collect();
    t1.sort();
    t2.sort();
    assert_eq!(t1, t2, "ref targets");
    drop((tree, tree2));

    if !cfg!(debug_assertions) {
        assert!(
            t_load < 1.0,
            "data.bin load took {t_load:.3}s (gate: < 1 s)"
        );
    }
}

/// An unknown key fails the load and names its Lua path.
#[test]
fn unknown_key_is_an_error() {
    let mut t = pob_gamedata::loader::DumpTree::default();
    let v = pob_gamedata::loader::parse_json(
        r#"{"x":{"id":"x","name":"X","levels":[],"modSource":"Skill:x","notAKey":1}}"#,
    )
    .unwrap();
    t.add_file(&["data.skills".into()], v).unwrap();
    let e = t
        .get::<indexmap::IndexMap<String, pob_gamedata::GrantedEffect>>("data.skills")
        .expect_err("unknown key must fail");
    assert!(e.to_string().contains("data.skills.x.notAKey"), "{e}");
}
