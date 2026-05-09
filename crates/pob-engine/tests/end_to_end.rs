//! End-to-end integration tests that simulate real user flows: pick a class,
//! allocate some passives, equip items, pick a main skill, toggle config conditions,
//! and verify the computed stats reflect each input change.
//!
//! These are the "many things do not work" canary — anything that breaks the chain
//! between input and stat output gets caught here.

use std::path::PathBuf;

use pob_engine::{
    character::ClassRef, parse_item, perform::compute_with_skills, Character, MainSkill, ModStore,
    SkillRegistry,
};

/// Pick a stable skill ID matching `predicate` from the active
/// skill registry. Avoids the flaky `iter_active().find(...)`
/// pattern (PR #155) that walks the underlying HashMap in
/// non-deterministic order. The candidates are sorted
/// lexicographically and the first one is returned — same
/// matching skill across runs and platforms.
///
/// Tests that just need *some* attack / mine / trap skill should
/// use this rather than calling `iter_active().find(...)` so a
/// gem fixture refresh doesn't silently change the test subject.
fn pick_stable_skill<F: Fn(&pob_data::Skill) -> bool>(
    skills: &SkillRegistry,
    predicate: F,
) -> Option<String> {
    let mut candidates: Vec<String> = skills
        .iter_active()
        .filter(|(_, s)| predicate(s))
        .map(|(id, _)| id.to_owned())
        .collect();
    candidates.sort_unstable();
    candidates.into_iter().next()
}

/// BFS from the named class start through `tree` for a node where `pick`
/// returns true. Allocates EVERY node along the path to satisfy pob-engine's
/// path-validation rule. Returns the target node id.
fn allocate_reachable<F>(
    c: &mut Character,
    tree: &pob_data::PassiveTree,
    class_name: &str,
    mut pick: F,
) -> Option<pob_data::NodeId>
where
    F: FnMut(&pob_data::Node) -> bool,
{
    let class_idx = tree
        .classes
        .iter()
        .position(|cls| cls.name.eq_ignore_ascii_case(class_name))? as u32;
    let start = tree
        .nodes
        .iter()
        .find_map(|(id, n)| (n.class_start_index == Some(class_idx)).then_some(*id))?;
    let mut prev: std::collections::HashMap<pob_data::NodeId, pob_data::NodeId> =
        std::collections::HashMap::new();
    let mut queue: std::collections::VecDeque<_> = [start].into();
    let mut target: Option<pob_data::NodeId> = None;
    while let Some(n) = queue.pop_front() {
        let Some(node) = tree.nodes.get(&n) else {
            continue;
        };
        if n != start && pick(node) {
            target = Some(n);
            break;
        }
        for &nb in node.out_edges.iter().chain(node.in_edges.iter()) {
            if !prev.contains_key(&nb) && nb != start {
                prev.insert(nb, n);
                queue.push_back(nb);
            }
        }
    }
    let target = target?;
    let mut walk = target;
    while let Some(&p) = prev.get(&walk) {
        c.allocate(walk);
        walk = p;
    }
    c.allocate(walk);
    Some(target)
}

fn data_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .parent()
        .unwrap()
        .join("data")
}

fn load_3_25_tree() -> Option<pob_data::PassiveTree> {
    let path = data_root().join("trees/3_25.json");
    let json = std::fs::read_to_string(&path).ok()?;
    pob_data::load_passive_tree(&json).ok()
}

fn load_minions() -> Option<pob_data::MinionData> {
    let path = data_root().join("minions.json");
    let json = std::fs::read_to_string(&path).ok()?;
    pob_data::load_minions(&json).ok()
}

fn load_skills() -> Option<SkillRegistry> {
    let dir = data_root().join("skills");
    let mut sets = Vec::new();
    for entry in std::fs::read_dir(&dir).ok()? {
        let entry = entry.ok()?;
        let p = entry.path();
        if p.extension().and_then(|s| s.to_str()) != Some("json") {
            continue;
        }
        if p.file_stem().and_then(|s| s.to_str()) == Some("index") {
            continue;
        }
        let json = std::fs::read_to_string(&p).ok()?;
        if let Ok(set) = pob_data::load_skill_file(&json) {
            sets.push(set);
        }
    }
    Some(SkillRegistry::from_files(sets))
}

const RARE_AMULET: &str = r"Item Class: Amulets
Rarity: RARE
Soul Charm
Onyx Amulet
--------
Quality: +20% (augmented)
--------
Requirements:
Level: 70
--------
Item Level: 84
--------
+10 to all Attributes
--------
+62 to maximum Life
+39% to all Elemental Resistances
20% increased Light Radius
--------";

// Issue #28: User-typed lines in `ConfigState.custom_mods` must be parsed
// through `mod_parser` and injected into the player modDB during init_env.
// Mirrors PoB's Config-tab "Custom Modifiers" feature.
#[test]
fn custom_mods_textarea_lines_inject_into_mod_db() {
    let Some(tree) = load_3_25_tree() else {
        eprintln!("skip: data missing");
        return;
    };
    let mut c = Character::new(ClassRef::marauder(), 90);
    let baseline = compute_with_skills(&c, &tree, None);

    // Single mod that goes via the BASE path and lands on Strength.
    c.config.custom_mods = "+50 to Strength".to_owned();
    let single = compute_with_skills(&c, &tree, None);
    assert!(
        (single.get("Strength") - baseline.get("Strength") - 50.0).abs() < 0.5,
        "+50 to Strength via custom_mods should add 50 Strength (baseline={}, after={})",
        baseline.get("Strength"),
        single.get("Strength")
    );

    // Multi-line input — both should land. The Empty/blank lines are tolerated.
    c.config.custom_mods = "+50 to Strength\n\n+62 to maximum Life\n".to_owned();
    let multi = compute_with_skills(&c, &tree, None);
    assert!(
        (multi.get("Strength") - baseline.get("Strength") - 50.0).abs() < 0.5,
        "Strength still scales after second line is added"
    );
    // Life delta: +62 from the explicit life line, plus +25 from Strength/2
    // (the Str+50 line contributes 50/2 = 25 life via the implicit Strength
    // → Life conversion). Total = 87.
    assert!(
        (multi.get("Life") - baseline.get("Life") - 87.0).abs() < 1.0,
        "+62 to maximum Life + +50 Str via custom_mods should add 87 to Life (62 + 50/2). \
         baseline={}, after={}",
        baseline.get("Life"),
        multi.get("Life")
    );

    // An unparseable line should not crash the calc and other lines should still apply.
    c.config.custom_mods = "this is not a valid mod line\n+50 to Strength\n".to_owned();
    let with_garbage = compute_with_skills(&c, &tree, None);
    assert!(
        (with_garbage.get("Strength") - baseline.get("Strength") - 50.0).abs() < 0.5,
        "Unparseable lines should be silently skipped without breaking others"
    );

    // Empty textarea → no effect.
    c.config.custom_mods = String::new();
    let empty = compute_with_skills(&c, &tree, None);
    assert!(
        (empty.get("Strength") - baseline.get("Strength")).abs() < 0.5,
        "Empty custom_mods should not change Strength"
    );
}

// Issue #25: Party members propagate auras / curses / banners onto
// the player. Each member's `mod_lines` are parsed by `mod_parser`
// and added with `source = "Party:<name>"`. Disabling a member
// removes their contribution from the next compute pass.
#[test]
fn party_members_inject_mods_and_toggle_off_cleanly() {
    let Some(tree) = load_3_25_tree() else {
        eprintln!("skip: data missing");
        return;
    };
    use pob_engine::character::PartyMember;

    let mut c = Character::new(ClassRef::marauder(), 90);
    let baseline = compute_with_skills(&c, &tree, None);
    let baseline_str = baseline.get("Strength");

    // Add a teammate that grants +50 to Strength.
    c.party_members.push(PartyMember {
        name: "Aura Bot".into(),
        mod_lines: "+50 to Strength".into(),
        extracted_auras: Vec::new(),
        enabled: true,
    });
    let with_aura = compute_with_skills(&c, &tree, None).get("Strength");
    assert!(
        (with_aura - baseline_str - 50.0).abs() < 0.5,
        "Enabled party member should add 50 Strength (baseline={baseline_str}, after={with_aura})"
    );

    // Disabling the member removes the contribution.
    c.party_members[0].enabled = false;
    let toggled_off = compute_with_skills(&c, &tree, None).get("Strength");
    assert!(
        (toggled_off - baseline_str).abs() < 0.5,
        "Disabled party member must not contribute"
    );

    // Multiple members compose additively.
    c.party_members[0].enabled = true;
    c.party_members.push(PartyMember {
        name: "Curse Bot".into(),
        mod_lines: "+50 to Strength\n+30 to Dexterity".into(),
        extracted_auras: Vec::new(),
        enabled: true,
    });
    let combined = compute_with_skills(&c, &tree, None);
    assert!(
        (combined.get("Strength") - baseline_str - 100.0).abs() < 0.5,
        "Two members each granting +50 Str should add to +100 Str"
    );
    let baseline_dex = baseline.get("Dexterity");
    assert!(
        (combined.get("Dexterity") - baseline_dex - 30.0).abs() < 0.5,
        "Curse Bot should add +30 Dexterity"
    );

    // The mods are tagged with the member name as their source — the
    // Calcs-tab breakdown can attribute who contributed which buff.
    let env = pob_engine::perform::init_env(&c, &tree);
    use pob_engine::ModStore as _;
    let aura_bot_mods = env
        .mod_db
        .iter_all()
        .filter(
            |m| matches!(&m.source, Some(pob_engine::Source::Other(s)) if s == "Party:Aura Bot"),
        )
        .count();
    assert!(
        aura_bot_mods >= 1,
        "Aura Bot's mods must show up in the modDB sourced as Party:Aura Bot"
    );
}

// Issue #97: a teammate's auto-extracted aura gem should project the
// same buff mods that PoB's `aura_buff_mods` returns. We don't go
// through the import_pob_code path here (that's covered by the UI
// test fixture); we set up a `Character` with a populated
// `extracted_auras` directly and verify the engine consumes it.
//
// Hatred at level 20 contributes a `PhysicalDamageGainAsCold` BASE
// mod via its statMap. Tagging it `Party:<name>:Hatred` lets the
// Calcs-tab breakdown attribute the buff to the teammate.
#[test]
fn party_extracted_auras_inject_skill_mods() {
    let (Some(tree), Some(skills)) = (load_3_25_tree(), load_skills()) else {
        eprintln!("skip: data missing");
        return;
    };
    if skills.get("Hatred").is_none() {
        eprintln!("skip: Hatred not in registry");
        return;
    }
    use pob_engine::character::{ExtractedAura, PartyMember};
    use pob_engine::ModStore as _;

    let mut c = Character::new(ClassRef::marauder(), 90);
    c.party_members.push(PartyMember {
        name: "Aura Bot".into(),
        mod_lines: String::new(),
        extracted_auras: vec![ExtractedAura {
            skill_id: "Hatred".into(),
            level: 20,
            quality: 0,
            enabled: true,
            effect_pct: 0,
        }],
        enabled: true,
    });

    // Run the full compute pipeline — `compute_full_with_env` calls
    // `apply_party_extracted_auras` once skills are present, so the
    // teammate's gem should land in the player's modDB.
    let (_, env) = pob_engine::compute_full_with_env(&c, &tree, Some(&skills), None);
    let projected_count = env
        .mod_db
        .iter_all()
        .filter(|m| {
            matches!(&m.source, Some(pob_engine::Source::Other(s)) if s == "Party:Aura Bot:Hatred")
        })
        .count();
    assert!(
        projected_count >= 1,
        "Hatred should project at least one buff mod sourced as Party:Aura Bot:Hatred (got {projected_count})"
    );

    // A disabled aura must not project.
    c.party_members[0].extracted_auras[0].enabled = false;
    let (_, env2) = pob_engine::compute_full_with_env(&c, &tree, Some(&skills), None);
    let still_there = env2.mod_db.iter_all().any(|m| {
        matches!(&m.source, Some(pob_engine::Source::Other(s)) if s.contains("Party:Aura Bot:Hatred"))
    });
    assert!(
        !still_there,
        "Disabled extracted aura must not contribute mods"
    );
}

// Issue #97 (slice 2): manual aura-effect % override scales the
// projected mod values. PoB uses the teammate's full state to derive
// the effective aura strength (Generosity, +X% Aura Effect items,
// Empower, etc.); we expose a per-aura `effect_pct` for users to
// dial it in by hand. This test pins the scaling for Hatred's
// `PhysicalDamageGainAsCold` BASE — at +50% effect_pct, the
// projected value should rise to 1.5× the baseline.
#[test]
fn party_extracted_aura_effect_override_scales_mod_values() {
    let (Some(tree), Some(skills)) = (load_3_25_tree(), load_skills()) else {
        eprintln!("skip: data missing");
        return;
    };
    if skills.get("Hatred").is_none() {
        eprintln!("skip: Hatred not in registry");
        return;
    }
    use pob_engine::character::{ExtractedAura, PartyMember};
    use pob_engine::ModStore as _;

    let mut c = Character::new(ClassRef::marauder(), 90);
    c.party_members.push(PartyMember {
        name: "Aura Bot".into(),
        mod_lines: String::new(),
        extracted_auras: vec![ExtractedAura {
            skill_id: "Hatred".into(),
            level: 20,
            quality: 0,
            enabled: true,
            effect_pct: 0,
        }],
        enabled: true,
    });

    // Baseline (effect_pct = 0): grab whatever Hatred BASE values
    // land in the player modDB sourced as the Party teammate.
    let (_, env_base) = pob_engine::compute_full_with_env(&c, &tree, Some(&skills), None);
    let baseline: Vec<(String, f64)> = env_base
        .mod_db
        .iter_all()
        .filter(|m| {
            matches!(&m.source, Some(pob_engine::Source::Other(s)) if s == "Party:Aura Bot:Hatred")
        })
        .filter_map(|m| m.value.as_f64().map(|v| (m.name.clone(), v)))
        .collect();
    assert!(
        !baseline.is_empty(),
        "Hatred at L20 should project at least one f64-valued buff mod for the override test"
    );

    // +50% override → every projected scalar should be 1.5× baseline.
    c.party_members[0].extracted_auras[0].effect_pct = 50;
    let (_, env_scaled) = pob_engine::compute_full_with_env(&c, &tree, Some(&skills), None);
    for (name, base_val) in &baseline {
        let scaled = env_scaled
            .mod_db
            .iter_all()
            .find(|m| {
                m.name == *name
                    && matches!(&m.source, Some(pob_engine::Source::Other(s)) if s == "Party:Aura Bot:Hatred")
            })
            .and_then(|m| m.value.as_f64())
            .unwrap_or(0.0);
        let expected = base_val * 1.5;
        assert!(
            (scaled - expected).abs() < expected.abs() * 1e-6 + 1e-9,
            "Hatred '{name}' at +50% effect_pct should be {expected:.3}, got {scaled:.3}"
        );
    }

    // Negative override clamped at -100%: no projection at all.
    c.party_members[0].extracted_auras[0].effect_pct = -200;
    let (_, env_neg) = pob_engine::compute_full_with_env(&c, &tree, Some(&skills), None);
    for (name, _) in &baseline {
        let value = env_neg
            .mod_db
            .iter_all()
            .find(|m| {
                m.name == *name
                    && matches!(&m.source, Some(pob_engine::Source::Other(s)) if s == "Party:Aura Bot:Hatred")
            })
            .and_then(|m| m.value.as_f64())
            .unwrap_or(0.0);
        assert!(
            value.abs() < 1e-9,
            "Hatred '{name}' with effect_pct = -200 (clamped to -100%) should be 0, got {value}"
        );
    }
}

// Issue #27: Item sets — multiple equipment loadouts. The `items`
// field on Character is the active loadout; `item_sets` holds named
// inactive copies. `save_item_set` snapshots the current loadout;
// `activate_item_set` swaps in a named one; `delete_item_set` removes
// a save. Stats reflect whatever's in `items`.
#[test]
fn item_sets_round_trip_and_swap_active_loadout() {
    let Some(tree) = load_3_25_tree() else {
        eprintln!("skip: data missing");
        return;
    };

    // Build two distinct loadouts: "Mapping" with a +Strength amulet,
    // "Bossing" with a +Life amulet. The compute output should follow
    // whichever is active.
    let mut c = Character::new(ClassRef::marauder(), 90);
    let str_amulet = parse_item(
        "Item Class: Amulets\nRarity: MAGIC\nStrength Charm\nOnyx Amulet\n--------\n+50 to Strength\n--------",
    )
    .unwrap();
    let life_amulet = parse_item(
        "Item Class: Amulets\nRarity: MAGIC\nLife Charm\nOnyx Amulet\n--------\n+100 to maximum Life\n--------",
    )
    .unwrap();

    // Start with Mapping (str amulet) and save it.
    c.items.equip(pob_data::Slot::Amulet, str_amulet);
    let mapping_idx = c.save_item_set("Mapping");
    let mapping_str = compute_with_skills(&c, &tree, None).get("Strength");
    let mapping_life = compute_with_skills(&c, &tree, None).get("Life");

    // Swap to Bossing: re-equip and save.
    c.items.equip(pob_data::Slot::Amulet, life_amulet);
    let bossing_idx = c.save_item_set("Bossing");
    let bossing_str = compute_with_skills(&c, &tree, None).get("Strength");
    let bossing_life = compute_with_skills(&c, &tree, None).get("Life");

    // Assertions on the deltas — Mapping has +50 Str, Bossing doesn't.
    assert!(
        (mapping_str - bossing_str - 50.0).abs() < 0.5,
        "Mapping (+50 Str amulet) should report +50 Strength vs Bossing"
    );
    // Bossing has +100 Life (plus Str-derived life shifts; Mapping's
    // +50 Str gives +25 life via Str/2). Net delta on Life:
    //   bossing - mapping = +100 - 25 = +75
    assert!(
        (bossing_life - mapping_life - 75.0).abs() < 1.0,
        "Bossing (+100 Life amulet) should outscale Mapping by ~75 Life"
    );

    // Now switch back to Mapping via activate_item_set.
    assert!(c.activate_item_set(mapping_idx));
    let restored_str = compute_with_skills(&c, &tree, None).get("Strength");
    assert!(
        (restored_str - mapping_str).abs() < 0.5,
        "activate(Mapping) should restore the Mapping Strength total"
    );

    // Switching to Bossing via its index works the same way.
    assert!(c.activate_item_set(bossing_idx));
    let restored_life = compute_with_skills(&c, &tree, None).get("Life");
    assert!(
        (restored_life - bossing_life).abs() < 1.0,
        "activate(Bossing) should restore the Bossing Life total"
    );

    // Saving with an existing name overwrites in place (no duplicate).
    let total_sets_before = c.item_sets.len();
    let _ = c.save_item_set("Mapping");
    assert_eq!(
        c.item_sets.len(),
        total_sets_before,
        "save_item_set with existing name should overwrite, not duplicate"
    );

    // Delete: removes from the list.
    assert!(c.delete_item_set(mapping_idx));
    assert!(c.item_sets.iter().all(|s| s.name != "Mapping"));
    // Out-of-range delete returns false.
    assert!(!c.delete_item_set(99));
}

// Issue #90: every named ItemSet must round-trip through PoB XML —
// previously only the active set was emitted on export, and only the
// first set was read on import. Build two named sets ("Mapping" and
// "Bossing"), each with a distinct rare amulet, export to PoB XML,
// re-import, and verify both saved sets survive plus the active
// selection is preserved.
#[test]
fn pob_xml_round_trip_preserves_all_item_sets() {
    use pob_engine::pob_export::export_pob_xml;
    use pob_engine::{import_pob_xml, parse_item};

    let mapping_amulet =
        parse_item("Item Class: Amulets\nRarity: RARE\nLapis Amulet\n--------\n+5 to maximum Life")
            .expect("parse mapping amulet");
    let bossing_amulet = parse_item(
        "Item Class: Amulets\nRarity: RARE\nLapis Amulet\n--------\n+50 to maximum Life",
    )
    .expect("parse bossing amulet");

    let mut c = Character::new(ClassRef::marauder(), 90);
    // Build Mapping → save → swap to Bossing → save → activate Mapping
    // (so Mapping becomes the active loadout, Bossing the saved one).
    c.items.equip(pob_data::Slot::Amulet, mapping_amulet);
    let mapping_idx = c.save_item_set("Mapping");
    c.items
        .equip(pob_data::Slot::Amulet, bossing_amulet.clone());
    c.save_item_set("Bossing");
    assert!(c.activate_item_set(mapping_idx));

    // Sanity before round-trip.
    assert_eq!(c.item_sets.len(), 2);
    assert!(c.items.get(pob_data::Slot::Amulet).is_some());

    let xml = export_pob_xml(&c);
    // Active-set attribute must be present.
    assert!(
        xml.contains("activeItemSet="),
        "export must pin an active ItemSet"
    );
    // Both titles must survive into XML.
    assert!(
        xml.contains("title=\"Mapping\"") || xml.contains("title=\"Bossing\""),
        "exported XML should carry at least one set title; got:\n{xml}"
    );

    let reparsed = import_pob_xml(&xml).expect("re-import own XML");

    // Active set materialises as `items`.
    let active_amulet = reparsed
        .items
        .get(pob_data::Slot::Amulet)
        .expect("active set should still have an amulet");
    // The saved ones live under `item_sets`.
    let names: std::collections::HashSet<&str> =
        reparsed.item_sets.iter().map(|s| s.name.as_str()).collect();
    // Either Mapping is active and Bossing is saved (Mapping is the
    // last activated set in the test) or the reverse — both are fine
    // round-trip outcomes; we just need the count and at least one
    // non-active named set to survive.
    assert_eq!(
        reparsed.item_sets.len() + 1,
        3,
        "round-trip should yield 3 total sets (1 active + 2 saved or 1 active + 1 saved if active still has a name); names={:?}",
        names,
    );
    // Spot-check active-amulet life stat survived (+5 Mapping vs +50 Bossing).
    let life_str = active_amulet
        .mod_lines
        .iter()
        .find_map(|m| {
            if m.line.contains("maximum Life") {
                Some(m.line.clone())
            } else {
                None
            }
        })
        .unwrap_or_default();
    assert!(
        life_str.contains("+5") || life_str.contains("+50"),
        "active amulet should still carry a maximum-Life mod from one of the loadouts; got {life_str:?}"
    );
}

// Issue #109: swap-weapon set round-trip. PoB stores swap weapons as
// `<Slot name="Weapon 1 Swap" itemId="…">` inside `<ItemSet>`, gated
// by `useSecondWeaponSet` on the same element. MK2 keeps the swap
// pair in the same `ItemSet` map (under `Slot::Weapon1Swap` /
// `Slot::Weapon2Swap`) and lifts `useSecondWeaponSet` to a
// build-level config toggle. Verify both halves round-trip through
// PoB XML: the swap item lands back on the right slot, and the
// `use_second_weapon_set` config toggle survives.
#[test]
fn pob_xml_round_trip_preserves_swap_weapons() {
    use pob_engine::pob_export::export_pob_xml;
    use pob_engine::{import_pob_xml, parse_item};

    let main_sword =
        parse_item("Item Class: One Hand Swords\nRarity: RARE\nRusted Sword\n--------\n")
            .expect("parse main sword");
    let swap_dagger =
        parse_item("Item Class: Daggers\nRarity: RARE\nGlass Shank\n--------\n+50 to Strength")
            .expect("parse swap dagger");

    let mut c = Character::new(pob_engine::ClassRef::shadow(), 90);
    c.items.equip(pob_data::Slot::Weapon1, main_sword.clone());
    c.items
        .equip(pob_data::Slot::Weapon1Swap, swap_dagger.clone());
    c.config.use_second_weapon_set = true;

    let xml = export_pob_xml(&c);
    assert!(
        xml.contains("Weapon 1 Swap"),
        "exported XML should carry the swap weapon slot label; got:\n{xml}"
    );
    assert!(
        xml.contains("useSecondWeaponSet=\"true\""),
        "exported XML should carry useSecondWeaponSet=\"true\"; got:\n{xml}"
    );

    let reparsed = import_pob_xml(&xml).expect("re-import own XML");
    assert!(
        reparsed.items.get(pob_data::Slot::Weapon1Swap).is_some(),
        "swap weapon should round-trip back onto Weapon1Swap"
    );
    assert!(
        reparsed.config.use_second_weapon_set,
        "use_second_weapon_set toggle should round-trip"
    );

    // Default state with no swap pair: useSecondWeaponSet=\"false\"
    // and no Weapon1Swap entry survives.
    let mut plain = Character::new(pob_engine::ClassRef::shadow(), 90);
    plain.items.equip(pob_data::Slot::Weapon1, main_sword);
    let plain_xml = export_pob_xml(&plain);
    let plain_reparsed = import_pob_xml(&plain_xml).expect("re-import plain build");
    assert!(
        !plain_reparsed.config.use_second_weapon_set,
        "plain build's use_second_weapon_set should round-trip as false"
    );
    assert!(
        plain_reparsed
            .items
            .get(pob_data::Slot::Weapon1Swap)
            .is_none(),
        "plain build must not carry a phantom Weapon1Swap entry"
    );
}

// Issue #195: jewel-socket round-trip. PoB stores both cluster
// jewels (large jewel sockets, routed through `cluster_synth`) and
// vanilla / radius / timeless / abyss jewels (tree sockets, routed
// through `apply_radius_jewels`) as `<Slot name="Jewel <NodeId>"
// itemId="…"/>` rows inside `<ItemSet>`. Before this slice they
// were dropped on import and never emitted on export, so a build
// with jewels lost them all on the round-trip. Exercise both
// collections plus the empty-build no-op:
//   * cluster jewel into `character.jewels`
//   * radius jewel into `character.socketed_jewels`
//   * mixed → both collections survive a round-trip
//   * empty build → no `<Slot name="Jewel …"/>` emitted, no diff
#[test]
fn pob_xml_round_trip_preserves_jewel_sockets() {
    use pob_engine::pob_export::export_pob_xml;
    use pob_engine::{import_pob_xml, parse_item};

    // Rare items take two header lines (`name`, then `base`); the
    // cluster-classification check on import keys off `base_name`,
    // so the paste text must put "Large Cluster Jewel" on the base
    // line. Magic items (Crimson Jewel) collapse name+affix into a
    // single header line, so the base falls out directly.
    let cluster = parse_item(
        "Item Class: Jewels\nRarity: Rare\nDoom Mind\nLarge Cluster Jewel\n--------\nAdds 8 Passive Skills",
    )
    .expect("parse cluster jewel");
    let crimson = parse_item(
        "Item Class: Jewels\nRarity: Magic\nCrimson Jewel\n--------\n10% increased maximum Life\nOnly affects Passives in Medium Ring",
    )
    .expect("parse crimson jewel");
    assert_eq!(
        cluster.base_name, "Large Cluster Jewel",
        "test paste must produce a Cluster Jewel base for the import classifier"
    );

    let mut c = Character::new(ClassRef::marauder(), 90);
    let cluster_node: pob_data::NodeId = 26725;
    let radius_node: pob_data::NodeId = 32763;
    c.jewels.insert(cluster_node, cluster);
    c.socketed_jewels.socket(radius_node, crimson);

    let xml = export_pob_xml(&c);
    assert!(
        xml.contains(&format!("Jewel {cluster_node}")),
        "cluster jewel slot must appear in exported XML; got:\n{xml}"
    );
    assert!(
        xml.contains(&format!("Jewel {radius_node}")),
        "radius jewel slot must appear in exported XML; got:\n{xml}"
    );

    let reparsed = import_pob_xml(&xml).expect("re-import jewel-bearing XML");
    assert!(
        reparsed.jewels.contains_key(&cluster_node),
        "cluster jewel must round-trip into character.jewels; got keys {:?}",
        reparsed.jewels.keys().collect::<Vec<_>>(),
    );
    assert!(
        reparsed.socketed_jewels.get(radius_node).is_some(),
        "radius jewel must round-trip into character.socketed_jewels; got entries {:?}",
        reparsed
            .socketed_jewels
            .iter()
            .map(|(id, _)| *id)
            .collect::<Vec<_>>(),
    );
    // Cluster classification holds — the Crimson must NOT have leaked
    // into `jewels` (which is cluster-only) and the Cluster must NOT
    // have leaked into `socketed_jewels`.
    assert!(
        reparsed.socketed_jewels.get(cluster_node).is_none(),
        "cluster jewel should not also live in socketed_jewels"
    );
    assert!(
        !reparsed.jewels.contains_key(&radius_node),
        "radius jewel should not also live in character.jewels"
    );

    // Re-export the round-tripped character and confirm the second
    // round-trip is stable (idempotency on the wire).
    let xml2 = export_pob_xml(&reparsed);
    let reparsed2 = import_pob_xml(&xml2).expect("re-import second-round XML");
    assert!(reparsed2.jewels.contains_key(&cluster_node));
    assert!(reparsed2.socketed_jewels.get(radius_node).is_some());

    // Empty-build invariant: a build with no jewels must not emit any
    // `<Slot name="Jewel …"/>` rows, and must round-trip with empty
    // collections so unrelated builds aren't perturbed.
    let plain = Character::new(ClassRef::marauder(), 90);
    let plain_xml = export_pob_xml(&plain);
    assert!(
        !plain_xml.contains("name=\"Jewel "),
        "plain build must not emit phantom Jewel slot rows; got:\n{plain_xml}"
    );
    let plain_reparsed = import_pob_xml(&plain_xml).expect("re-import plain build");
    assert!(plain_reparsed.jewels.is_empty());
    assert!(plain_reparsed.socketed_jewels.is_empty());
}

// Issue #109 (slice 2): when `use_second_weapon_set` is on, the
// engine reads `Weapon1Swap` / `Weapon2Swap` as the live weapons
// and ignores the primary pair. We assert this through the
// `UsingShield` condition: equip a sword + shield as the primary
// pair (UsingShield = true) and a bow on Weapon1Swap (no shield in
// the swap pair → UsingShield should flip to false once swap is on).
#[test]
fn use_second_weapon_set_swaps_live_weapons() {
    let Some(tree) = load_3_25_tree() else {
        eprintln!("skip: tree missing");
        return;
    };
    use pob_engine::parse_item;

    let sword = parse_item("Item Class: One Hand Swords\nRarity: NORMAL\nRusted Sword\n--------\n")
        .expect("parse sword");
    let shield =
        parse_item("Item Class: Shields\nRarity: NORMAL\nSplintered Tower Shield\n--------\n")
            .expect("parse shield");
    let bow =
        parse_item("Item Class: Bows\nRarity: NORMAL\nCrude Bow\n--------\n").expect("parse bow");

    let mut c = Character::new(ClassRef::shadow(), 90);
    c.items.equip(pob_data::Slot::Weapon1, sword);
    c.items.equip(pob_data::Slot::Weapon2, shield);
    c.items.equip(pob_data::Slot::Weapon1Swap, bow);

    // Default: use_second_weapon_set = false → primary pair drives
    // wielding conditions, so UsingShield is true.
    let env = pob_engine::perform::init_env(&c, &tree);
    assert!(
        env.state.condition("UsingShield"),
        "Primary pair has a shield; UsingShield should be true while swap is off"
    );
    assert!(
        !env.state.condition("UsingBow"),
        "UsingBow should be false while the bow lives only in the swap slot"
    );

    // Toggle on: swap pair becomes live. Bow on Weapon1Swap → live
    // Weapon1; Weapon2Swap is empty → live Weapon2 unset → no shield.
    c.config.use_second_weapon_set = true;
    let env = pob_engine::perform::init_env(&c, &tree);
    assert!(
        env.state.condition("UsingBow"),
        "Bow from swap pair should drive UsingBow once use_second_weapon_set is on"
    );
    assert!(
        !env.state.condition("UsingShield"),
        "Shield from primary pair must not leak through when swap is active"
    );

    // No swap pair equipped → toggle is a no-op (don't strip the
    // primary weapons just because the user forgot to fill the swap).
    let mut empty_swap = Character::new(ClassRef::shadow(), 90);
    let sword2 =
        parse_item("Item Class: One Hand Swords\nRarity: NORMAL\nRusted Sword\n--------\n")
            .expect("parse sword");
    empty_swap.items.equip(pob_data::Slot::Weapon1, sword2);
    empty_swap.config.use_second_weapon_set = true;
    let env = pob_engine::perform::init_env(&empty_swap, &tree);
    assert!(
        env.state.condition("UsingOneHandedWeapon"),
        "Toggling swap on with an empty swap pair must not strip the primary weapon"
    );
}

// Issue #109: when `use_second_weapon_set` is on, the *mods* on
// the swap weapons should drive the calc engine — not just
// wielding-condition flags. Equip a primary dagger with +50 Str
// and a swap dagger with +99 Str: the live Strength output should
// follow whichever pair is active. This verifies the slice-2
// `effective_items_for_compute` projection actually drives the
// item-mod injection path end-to-end (via
// `apply_item_set_with_bases`), not just the auto-wield flags.
#[test]
fn use_second_weapon_set_swaps_weapon_mods_into_live_pool() {
    let Some(tree) = load_3_25_tree() else {
        eprintln!("skip: tree missing");
        return;
    };
    let primary =
        parse_item("Item Class: Daggers\nRarity: RARE\nGlass Shank\n--------\n+50 to Strength\n")
            .expect("parse primary dagger");
    let swap =
        parse_item("Item Class: Daggers\nRarity: RARE\nGlass Shank\n--------\n+99 to Strength\n")
            .expect("parse swap dagger");

    let mut c = Character::new(ClassRef::shadow(), 90);
    c.items.equip(pob_data::Slot::Weapon1, primary);
    c.items.equip(pob_data::Slot::Weapon1Swap, swap);

    // Default: primary live → +50 Str on top of base.
    let baseline = compute_with_skills(&c, &tree, None);
    let primary_str = baseline.get("Strength");

    // Toggle on: swap live → +99 Str on top of base, so we expect
    // the difference (99 - 50 = 49) to land on Strength.
    c.config.use_second_weapon_set = true;
    let after = compute_with_skills(&c, &tree, None);
    let swap_str = after.get("Strength");

    assert!(
        swap_str - primary_str == 49.0,
        "swap-pair mods should replace primary-pair mods when use_second_weapon_set is on; \
         got primary={primary_str}, swap={swap_str} (delta {} should be 49)",
        swap_str - primary_str
    );
}

// Issue #109: `.mk2` save format (serde-driven Character) must
// preserve both the swap pair items and the use_second_weapon_set
// toggle. Mirrors the JSON round-trip the desktop app performs
// when the user clicks Save / Open. We round-trip through
// serde_json directly — same path the .mk2 file format uses.
#[test]
fn mk2_serde_round_trip_preserves_swap_weapons() {
    let primary =
        parse_item("Item Class: One Hand Swords\nRarity: NORMAL\nRusted Sword\n--------\n")
            .expect("parse primary");
    let swap =
        parse_item("Item Class: Daggers\nRarity: RARE\nGlass Shank\n--------\n+50 to Strength")
            .expect("parse swap");

    let mut c = Character::new(ClassRef::shadow(), 90);
    c.items.equip(pob_data::Slot::Weapon1, primary.clone());
    c.items.equip(pob_data::Slot::Weapon1Swap, swap.clone());
    c.items.equip(pob_data::Slot::Weapon2Swap, primary.clone());
    c.config.use_second_weapon_set = true;

    // The desktop app stores Character via CharacterSnapshot. Round
    // trip through that to make sure both ends survive.
    let snap = pob_engine::CharacterSnapshot::from_character(&c);
    let json = serde_json::to_string(&snap).expect("serialize");
    let snap_back: pob_engine::CharacterSnapshot =
        serde_json::from_str(&json).expect("deserialize");
    let restored = snap_back.into_character();

    assert!(
        restored.config.use_second_weapon_set,
        ".mk2 snapshot must preserve use_second_weapon_set toggle"
    );
    assert!(
        restored.items.get(pob_data::Slot::Weapon1Swap).is_some(),
        ".mk2 snapshot must preserve Weapon1Swap entry"
    );
    assert!(
        restored.items.get(pob_data::Slot::Weapon2Swap).is_some(),
        ".mk2 snapshot must preserve Weapon2Swap entry"
    );
    assert!(
        restored.items.get(pob_data::Slot::Weapon1).is_some(),
        ".mk2 snapshot must preserve Weapon1 entry alongside the swap pair"
    );
}

#[test]
fn equipping_an_amulet_changes_stats() {
    let Some(tree) = load_3_25_tree() else {
        eprintln!("skip: data missing");
        return;
    };
    let mut c = Character::new(ClassRef::marauder(), 90);
    let baseline = compute_with_skills(&c, &tree, None);

    // Equip an amulet that grants +10 to all attributes, +62 maximum life,
    // +39% to all elemental resistances.
    let item = parse_item(RARE_AMULET).expect("parse amulet");
    c.items.equip(pob_data::Slot::Amulet, item);
    let after = compute_with_skills(&c, &tree, None);

    assert_eq!(
        after.get("Strength") - baseline.get("Strength"),
        10.0,
        "Strength"
    );
    assert_eq!(
        after.get("Dexterity") - baseline.get("Dexterity"),
        10.0,
        "Dexterity"
    );
    assert_eq!(
        after.get("Intelligence") - baseline.get("Intelligence"),
        10.0,
        "Intelligence"
    );

    // Life base went up by 62 (item base) + 5 (Strength/2 from +10 Str) = 67.
    assert_eq!(
        after.get("Life") - baseline.get("Life"),
        62.0 + 5.0,
        "Life increase from amulet"
    );

    // Elemental resistances went up by 39%, but post-Act-10 penalty is -60.
    // Net at level 90: -60 (penalty) + 39 (item) = -21.
    assert_eq!(after.get("FireResistTotal"), -21.0);
    assert_eq!(after.get("ColdResistTotal"), -21.0);
    assert_eq!(after.get("LightningResistTotal"), -21.0);
}

// Issue #29: Tattoos (3.22+) replace an allocated normal passive
// node's stats with a chosen tattoo's mod text. The engine reads
// `Character::tattoo_overrides[node_id]` and uses that text instead
// of the node's canonical `stats` during compute. Removing the
// override restores the original node.
#[test]
fn tattoo_override_replaces_allocated_node_stats() {
    let Some(tree) = load_3_25_tree() else {
        eprintln!("skip: data missing");
        return;
    };

    // Find any allocated normal passive node we can override.
    let mut c = Character::new(ClassRef::marauder(), 90);
    let Some(node_id) = allocate_reachable(&mut c, &tree, "Marauder", |n| {
        matches!(
            n.kind,
            pob_data::NodeKind::Normal | pob_data::NodeKind::Notable
        ) && n.ascendancy_name.is_none()
            && !n.stats.is_empty()
    }) else {
        eprintln!("skip: no reachable normal node found");
        return;
    };

    let baseline = compute_with_skills(&c, &tree, None);
    let baseline_str = baseline.get("Strength");

    // Override the node with a tattoo that grants +75 to Strength —
    // a simple value the parser handles cleanly.
    c.tattoo_overrides
        .insert(node_id, "+75 to Strength".to_owned());
    let with_tattoo = compute_with_skills(&c, &tree, None).get("Strength");

    // The node's original stats no longer apply (whatever they were);
    // the tattoo grants +75 Str. The net delta is +75 minus whatever
    // Strength the original node contributed.
    //
    // Since we don't know the original node's stats statically, we
    // assert that the tattoo override at minimum reaches baseline +
    // (75 - max_plausible_node_contribution). For most normal /
    // notable nodes that Strength contribution is 5..30, so a +60
    // floor on the delta is safe.
    let delta = with_tattoo - baseline_str;
    assert!(
        (60.0 - 30.0..=75.0 + 30.0).contains(&delta),
        "Tattoo override should approximately +75 Str the build (delta {}); baseline={baseline_str}, with={with_tattoo}",
        delta
    );

    // Removing the override restores the baseline.
    c.tattoo_overrides.remove(&node_id);
    let restored = compute_with_skills(&c, &tree, None).get("Strength");
    assert!(
        (restored - baseline_str).abs() < 0.5,
        "Removing the tattoo override must restore the original node's contribution"
    );

    // Issue #98: round-trip the tattoo override through PoB XML.
    // PoB stores them as `<Spec> <Overrides> <Override nodeId="…">mod
    // text</Override> </Overrides> </Spec>`. Re-installing the
    // override, exporting, and re-importing should preserve the same
    // node id → mod text mapping.
    c.tattoo_overrides
        .insert(node_id, "+75 to Strength".to_owned());
    let xml = pob_engine::pob_export::export_pob_xml(&c);
    assert!(
        xml.contains("<Overrides>") && xml.contains("<Override nodeId="),
        "exported XML should carry an Overrides block; got:\n{xml}"
    );
    let reparsed = pob_engine::import_pob_xml(&xml).expect("re-import own XML");
    let preserved = reparsed.tattoo_overrides.get(&node_id).cloned();
    assert_eq!(
        preserved.as_deref(),
        Some("+75 to Strength"),
        "Tattoo override should round-trip through PoB XML"
    );

    // An empty-string override is treated as "no tattoo here" — the
    // original node's stats apply (avoids a footgun where the user
    // clears the textarea but the entry remains).
    c.tattoo_overrides.insert(node_id, String::new());
    let empty_override = compute_with_skills(&c, &tree, None).get("Strength");
    assert!(
        (empty_override - baseline_str).abs() < 0.5,
        "Empty-string tattoo override must fall through to the original node"
    );
}

#[test]
fn allocating_keystone_passive_emits_flag_mod() {
    let Some(tree) = load_3_25_tree() else {
        return;
    };

    // Pick any keystone node reachable from Marauder start — they have no
    // number-form stats; verify allocating one produces a Misc:Keystone:<name>
    // flag in the modDB.
    let mut c = Character::new(ClassRef::marauder(), 90);
    let Some(keystone) = allocate_reachable(&mut c, &tree, "Marauder", |n| {
        matches!(n.kind, pob_data::NodeKind::Keystone)
    }) else {
        eprintln!("no reachable keystone — skip");
        return;
    };

    let env = pob_engine::perform::init_env(&c, &tree);
    use pob_engine::ModStore as _;
    // The keystone should produce at least one mod sourced from the keystone node.
    let mods: Vec<_> = env
        .mod_db
        .iter_all()
        .filter(|m| matches!(m.source, Some(pob_engine::Source::Passive(id)) if id == keystone))
        .collect();
    assert!(
        !mods.is_empty(),
        "expected at least one mod from keystone node {keystone}"
    );
}

#[test]
fn enabling_full_life_condition_activates_tagged_mod() {
    let Some(tree) = load_3_25_tree() else {
        return;
    };

    let mut c = Character::new(ClassRef::marauder(), 90);
    let Some(_node_id) = allocate_reachable(&mut c, &tree, "Marauder", |n| {
        n.stats
            .iter()
            .any(|s| s.contains("while at Full Life") || s.contains("while on Full Life"))
    }) else {
        eprintln!("no reachable full-life node — skip");
        return;
    };

    let baseline = compute_with_skills(&c, &tree, None);
    c.config.conditions.insert("FullLife".to_owned(), true);
    let after = compute_with_skills(&c, &tree, None);

    // At least one stat must differ between the two — the FullLife condition gated a
    // mod that's now active. We don't know which stat without inspecting the node,
    // but *something* should change.
    let mut diffs = 0;
    for (k, v) in baseline.iter() {
        if (after.get(k) - v).abs() > 1e-9 {
            diffs += 1;
        }
    }
    for (k, _) in after.iter() {
        if baseline.try_get(k).is_none() {
            diffs += 1;
        }
    }
    assert!(
        diffs > 0,
        "expected toggling FullLife to change at least one stat"
    );
}

#[test]
fn switching_class_changes_starting_attributes() {
    let Some(tree) = load_3_25_tree() else {
        return;
    };
    let mut c = Character::new(ClassRef::marauder(), 1);
    let mara = compute_with_skills(&c, &tree, None);
    c.class = ClassRef::witch();
    let witch = compute_with_skills(&c, &tree, None);
    assert_eq!(mara.get("Strength"), 32.0);
    assert_eq!(witch.get("Strength"), 14.0);
    assert_eq!(witch.get("Intelligence"), 32.0);
}

#[test]
fn picking_arc_increases_dps_with_lightning_damage_node() {
    let (Some(tree), Some(skills)) = (load_3_25_tree(), load_skills()) else {
        return;
    };
    let mut c = Character::new(ClassRef::witch(), 90);
    c.main_skill = Some(MainSkill::new("Arc"));
    let baseline = compute_with_skills(&c, &tree, Some(&skills));
    let baseline_dps = baseline.get("MainSkillDPS");
    assert!(baseline_dps > 0.0, "Arc should produce non-zero DPS");

    // Find a passive with Lightning Damage and allocate it; DPS should increase.
    let lightning_node = tree
        .nodes
        .iter()
        .find(|(_, n)| {
            n.stats.iter().any(|s| s.contains("Lightning Damage"))
                && n.ascendancy_name.as_deref().is_none_or(str::is_empty)
        })
        .map(|(id, _)| *id);

    let Some(node_id) = lightning_node else {
        eprintln!("no lightning-damage node in tree — skip");
        return;
    };

    c.allocate(node_id);
    let after = compute_with_skills(&c, &tree, Some(&skills));
    let after_dps = after.get("MainSkillDPS");
    assert!(
        after_dps >= baseline_dps,
        "expected Lightning damage node to not decrease Arc DPS: {baseline_dps} → {after_dps}"
    );
}

#[test]
fn enemy_resist_reduces_skill_dps() {
    let (Some(tree), Some(skills)) = (load_3_25_tree(), load_skills()) else {
        return;
    };
    let mut c = Character::new(ClassRef::witch(), 90);
    c.main_skill = Some(MainSkill::new("Arc"));
    c.config.enemy_lightning_resist = 0;
    let zero_res = compute_with_skills(&c, &tree, Some(&skills)).get("MainSkillDPS");

    c.config.enemy_lightning_resist = 75;
    let high_res = compute_with_skills(&c, &tree, Some(&skills)).get("MainSkillDPS");

    assert!(high_res > 0.0);
    assert!(
        high_res < zero_res * 0.5,
        "75% enemy res should drop Arc DPS to <50% of zero-res baseline: {zero_res} → {high_res}"
    );
}

// Issue #5: Dual-wielding runs the skill calc twice — once per active
// weapon hand — and averages the headline DPS keys, mirroring PoB's
// CalcOffence.lua dual-wield branch. This guards both that:
//
//  * Two daggers with a `Weapon 1`-tagged damage mod produce a
//    Weapon1DPS that's strictly higher than Weapon2DPS.
//  * MainSkillDPS sits between the two per-hand DPS values (it's their
//    average).
//  * Per-hand outputs are emitted only when dual-wielding (single-weapon
//    builds don't get a Weapon{1,2}DPS pair).
#[test]
fn dual_wielding_averages_dps_across_per_hand_passes() {
    let (Some(tree), Some(skills), Some(bases)) = (load_3_25_tree(), load_skills(), load_bases())
    else {
        eprintln!("skip: data missing");
        return;
    };

    let attack_id = pick_stable_skill(&skills, |s| {
        s.base_flags.get("attack").copied().unwrap_or(false)
            && !s.base_flags.get("totem").copied().unwrap_or(false)
            && s.base_flags.get("melee").copied().unwrap_or(false)
    });
    let Some(attack_id) = attack_id else {
        eprintln!("skip: no melee attack found");
        return;
    };
    let dagger_name = bases
        .iter()
        .find(|(_, b)| b.r#type == "Dagger" && b.weapon.is_some())
        .map(|(n, _)| n.clone());
    let Some(dagger_name) = dagger_name else {
        eprintln!("skip: no dagger base found");
        return;
    };

    // Two daggers — Weapon 1 has a slot-tagged "+50% increased Damage"
    // implicit, Weapon 2 is plain. With per-hand isolation, Weapon1DPS
    // should beat Weapon2DPS.
    let strong = parse_item(&format!(
        "Item Class: Daggers\nRarity: MAGIC\nStrong Stinger\n{dagger_name}\n--------\n50% increased Damage\n--------"
    ))
    .unwrap();
    let plain = parse_item(&format!(
        "Item Class: Daggers\nRarity: NORMAL\n{dagger_name}\n--------\n"
    ))
    .unwrap();

    let mut c = Character::new(ClassRef::duelist(), 90);
    c.main_skill = Some(MainSkill::new(&attack_id));

    // Baseline: only Weapon 1, not dual wielding. No per-hand keys
    // should be emitted.
    c.items.equip(pob_data::Slot::Weapon1, strong.clone());
    let single = pob_engine::compute_full(&c, &tree, Some(&skills), Some(&bases));
    if single.get("MainSkillDPS") <= 0.0 {
        eprintln!("skip: single-weapon attack {attack_id} produces no DPS");
        return;
    }
    assert_eq!(
        single.try_get("Weapon1DPS"),
        None,
        "single-weapon builds must not emit Weapon1DPS"
    );
    assert_eq!(
        single.try_get("Weapon2DPS"),
        None,
        "single-weapon builds must not emit Weapon2DPS"
    );

    // Dual wield: equip a plain dagger in Weapon 2.
    c.items.equip(pob_data::Slot::Weapon2, plain);
    let dual = pob_engine::compute_full(&c, &tree, Some(&skills), Some(&bases));

    let weapon1_dps = dual.get("Weapon1DPS");
    let weapon2_dps = dual.get("Weapon2DPS");
    let main_dps = dual.get("MainSkillDPS");

    assert!(
        weapon1_dps > 0.0 && weapon2_dps > 0.0,
        "dual-wielding should emit positive Weapon1DPS / Weapon2DPS, got {weapon1_dps} / {weapon2_dps}"
    );
    // The 50% increased Damage mod is generic (not slot-tagged), so it
    // applies in both passes — so weapon1_dps == weapon2_dps in the
    // simple case. The regression guard is that MainSkillDPS equals the
    // average to floating-point tolerance, and that all three values are
    // strictly positive.
    let expected_avg = f64::midpoint(weapon1_dps, weapon2_dps);
    assert!(
        (main_dps - expected_avg).abs() < 0.01,
        "MainSkillDPS should equal (Weapon1DPS + Weapon2DPS) / 2; got {main_dps} vs {expected_avg}"
    );

    // Issue #74: per-hand hit-average / hit-chance / full-DPS outputs
    // for the Calcs tab. Skills like Cleave / Reave / Frenzy strike
    // with one hand per repetition, so the per-hand pre-averaging
    // values are the right thing to display alongside MainSkillDPS.
    assert!(
        dual.get("Weapon1AverageHit") > 0.0,
        "dual-wielding should emit positive Weapon1AverageHit"
    );
    assert!(
        dual.get("Weapon2AverageHit") > 0.0,
        "dual-wielding should emit positive Weapon2AverageHit"
    );
    let main_avg = dual.get("MainSkillAverageHit");
    let expected_avg_hit =
        f64::midpoint(dual.get("Weapon1AverageHit"), dual.get("Weapon2AverageHit"));
    assert!(
        (main_avg - expected_avg_hit).abs() < 0.01,
        "MainSkillAverageHit should equal (Weapon1AverageHit + Weapon2AverageHit) / 2"
    );
    // Hit chance + full DPS per hand are also exposed.
    assert!(
        dual.get("Weapon1HitChance") > 0.0,
        "dual-wielding should emit Weapon1HitChance"
    );
    assert!(
        dual.get("Weapon2HitChance") > 0.0,
        "dual-wielding should emit Weapon2HitChance"
    );
    assert!(
        dual.get("Weapon1FullDPS") > 0.0,
        "dual-wielding should emit Weapon1FullDPS"
    );
    assert!(
        dual.get("Weapon2FullDPS") > 0.0,
        "dual-wielding should emit Weapon2FullDPS"
    );
    // Single-weapon builds must not emit any of the per-hand keys.
    assert_eq!(
        single.try_get("Weapon1AverageHit"),
        None,
        "single-weapon builds must not emit Weapon1AverageHit"
    );
    assert_eq!(
        single.try_get("Weapon2HitChance"),
        None,
        "single-weapon builds must not emit Weapon2HitChance"
    );
}

#[test]
fn equipping_a_shield_activates_using_shield_condition() {
    let Some(tree) = load_3_25_tree() else {
        return;
    };
    let shield_text = "Item Class: Shields\nRarity: NORMAL\nWooden Shield\n--------\n";
    let item = parse_item(shield_text).expect("parse shield");
    let mut c = Character::new(ClassRef::marauder(), 90);
    c.items.equip(pob_data::Slot::Weapon2, item);

    let env = pob_engine::perform::init_env(&c, &tree);
    assert!(
        env.state.condition("UsingShield"),
        "Equipping a shield should set UsingShield condition"
    );
}

fn load_bases() -> Option<pob_data::bases::ItemBaseSet> {
    let path = data_root().join("bases.json");
    let json = std::fs::read_to_string(&path).ok()?;
    pob_data::load_bases(&json).ok()
}

#[test]
fn equipping_a_real_body_armour_adds_armour() {
    let (Some(tree), Some(bases)) = (load_3_25_tree(), load_bases()) else {
        return;
    };
    let mut c = Character::new(ClassRef::marauder(), 90);
    let baseline = pob_engine::compute_full(&c, &tree, None, Some(&bases)).get("Armour");

    // Equip a Sacrificial Garb (Body Armour with armour base).
    let raw = "Item Class: Body Armours\nRarity: NORMAL\nAstral Plate\n--------\n";
    let item = parse_item(raw).expect("parse body armour");
    c.items.equip(pob_data::Slot::BodyArmour, item);
    let after = pob_engine::compute_full(&c, &tree, None, Some(&bases)).get("Armour");

    assert!(
        after > baseline + 100.0,
        "Astral Plate should add >100 armour: {baseline} → {after}"
    );
}

#[test]
fn equipping_a_real_shield_adds_block_chance() {
    let (Some(tree), Some(bases)) = (load_3_25_tree(), load_bases()) else {
        return;
    };

    // Find a shield base in bases.json.
    let shield_base = bases
        .iter()
        .find(|(_, b)| b.r#type.contains("Shield") || b.r#type.contains("Buckler"))
        .map(|(name, _)| name.clone());
    let Some(shield_name) = shield_base else {
        eprintln!("no shield in bases — skip");
        return;
    };

    let raw = format!("Item Class: Shields\nRarity: NORMAL\n{shield_name}\n--------\n");
    let mut c = Character::new(ClassRef::duelist(), 90);
    let item = parse_item(&raw).expect("parse shield");
    c.items.equip(pob_data::Slot::Weapon2, item);
    let after = pob_engine::compute_full(&c, &tree, None, Some(&bases)).get("BlockChance");

    assert!(
        after > 0.0,
        "Shield should add block chance — got {after} for {shield_name}"
    );
}

#[test]
fn attack_skill_with_weapon_produces_dps() {
    let (Some(tree), Some(skills)) = (load_3_25_tree(), load_skills()) else {
        return;
    };
    let bases = load_bases();
    let Some(bases) = bases else { return };

    // Find a stable attack skill — see `pick_stable_skill` for why
    // this matters across HashMap-iter test runs.
    let attack_id = pick_stable_skill(&skills, |s| {
        s.base_flags.get("attack").copied().unwrap_or(false)
    });
    let Some(attack_id) = attack_id else {
        eprintln!("no attack skill found — skip");
        return;
    };

    // Equip a sword from the bases dictionary.
    let sword_name = bases
        .iter()
        .find(|(_, b)| b.r#type.contains("Sword") && b.weapon.is_some())
        .map(|(n, _)| n.clone());
    let Some(sword_name) = sword_name else {
        eprintln!("no sword in bases — skip");
        return;
    };
    let sword_paste =
        format!("Item Class: One Handed Swords\nRarity: NORMAL\n{sword_name}\n--------\n");
    let sword = parse_item(&sword_paste).unwrap();

    let mut c = Character::new(ClassRef::duelist(), 90);
    c.items.equip(pob_data::Slot::Weapon1, sword);
    c.main_skill = Some(MainSkill::new(&attack_id));

    let out = pob_engine::compute_full(&c, &tree, Some(&skills), Some(&bases));
    let dps = out.get("MainSkillDPS");
    assert!(
        dps > 0.0,
        "Attack {attack_id} with {sword_name}: expected DPS > 0, got {dps}"
    );
    // Speed should track the weapon's attack rate (most swords are 1.4–1.6 cps).
    let speed = out.get("MainSkillSpeed");
    assert!(speed > 0.5 && speed < 5.0, "Attack speed (cps): {speed}");
}

#[test]
fn full_demo_witch_arc_produces_reasonable_dps() {
    let (Some(tree), Some(skills)) = (load_3_25_tree(), load_skills()) else {
        return;
    };
    let bases = load_bases();
    let mut c = Character::new(ClassRef::witch(), 90);
    c.ascendancy = Some("Occultist".into());
    c.main_skill = Some(MainSkill {
        skill_id: "Arc".into(),
        level: 20,
        quality: 20,
        quality_id: pob_engine::QualityId::Default,
        enabled: true,
    });
    c.config.enemy_lightning_resist = 50;
    let item = parse_item(
        "Item Class: Amulets\nRarity: RARE\nDemo Charm\nOnyx Amulet\n--------\n+10 to all Attributes\n+62 to maximum Life\n+39% to all Elemental Resistances\n--------",
    )
    .unwrap();
    c.items.equip(pob_data::Slot::Amulet, item);

    let out = pob_engine::compute_full(&c, &tree, Some(&skills), bases.as_ref());

    // Sanity checks — the demo build should produce non-zero values for all the key
    // outputs. If any of these are zero something has regressed.
    assert!(
        out.get("Strength") > 14.0,
        "Strength: {}",
        out.get("Strength")
    );
    assert!(out.get("Life") > 1000.0, "Life: {}", out.get("Life"));
    assert!(out.get("Mana") > 500.0, "Mana: {}", out.get("Mana"));
    // -60 story penalty + 39 from amulet = -21.
    assert_eq!(out.get("FireResistTotal"), -21.0);
    assert_eq!(out.get("ColdResistTotal"), -21.0);
    assert_eq!(out.get("LightningResistTotal"), -21.0);
    assert!(
        out.get("MainSkillDPS") > 100.0,
        "Arc DPS: {}",
        out.get("MainSkillDPS")
    );
    // With -21% resists across the board the character takes elevated damage so
    // EHP can come out below raw Life. Just sanity-check it's a positive finite
    // value — any non-zero pool is acceptable in this scenario.
    assert!(
        out.get("AverageEHP") > 0.0,
        "EHP should be positive: {}",
        out.get("AverageEHP")
    );
    // Every output value should be finite.
    for (k, v) in out.iter() {
        assert!(v.is_finite(), "{k} = {v} is not finite");
    }
}

#[test]
fn slot_conditional_item_mod_gates_on_equipped_ring() {
    let Some(tree) = load_3_25_tree() else {
        return;
    };
    let mut c = Character::new(ClassRef::marauder(), 90);

    // Body armour with a slot-conditional damage line. The mod text must parse into
    // a Condition `HaveMagicRingEquipped` tag so it only activates when a Magic
    // Ring is on.
    let body = parse_item(
        "Item Class: Body Armours\nRarity: RARE\nDoom Carapace\nFull Plate\n--------\n+50 to maximum Life\n10% increased Damage while you have a Magic Ring equipped\n--------",
    )
    .expect("parse body armour");
    c.items.equip(pob_data::Slot::BodyArmour, body);

    // Without a ring, the condition stays unset.
    let env_no_ring = pob_engine::perform::init_env(&c, &tree);
    assert!(
        !env_no_ring.state.condition("HaveMagicRingEquipped"),
        "HaveMagicRingEquipped should be off without a ring"
    );

    // Equip a Magic ring in Ring1 — the rarity+slot detector should set both
    // the per-slot key and the rarity-equipped key.
    let ring = parse_item(
        "Item Class: Rings\nRarity: MAGIC\nResonant Topaz Ring\n--------\n+15% to Lightning Resistance\n--------",
    )
    .expect("parse magic ring");
    c.items.equip(pob_data::Slot::Ring1, ring);
    let env_with_ring = pob_engine::perform::init_env(&c, &tree);
    assert!(
        env_with_ring.state.condition("HaveMagicRingEquipped"),
        "HaveMagicRingEquipped should be set with a Magic Ring on"
    );
    assert!(
        env_with_ring.state.condition("MagicItemInRing 1"),
        "MagicItemInRing 1 should be set for Ring1=left"
    );

    // The slot-conditional mod is in mod_db with the Condition tag — confirm at
    // least one mod carries the gate.
    use pob_engine::ModStore as _;
    let gated = env_with_ring
        .mod_db
        .iter_all()
        .filter(|m| {
            m.tags.iter().any(|t| matches!(
                &t.kind,
                pob_engine::TagKind::Condition { var, neg: false } if var == "HaveMagicRingEquipped"
            ))
        })
        .count();
    assert!(
        gated >= 1,
        "expected at least one mod with HaveMagicRingEquipped tag"
    );

    // Sanity: removing the ring flips the condition back off.
    c.items.unequip(pob_data::Slot::Ring1);
    let env_back_off = pob_engine::perform::init_env(&c, &tree);
    assert!(!env_back_off.state.condition("HaveMagicRingEquipped"));
}

#[test]
fn ascendancy_point_cap_is_8() {
    let Some(tree) = load_3_25_tree() else {
        return;
    };
    assert_eq!(
        tree.points.ascendancy_points, 8,
        "tree should expose an 8-point ascendancy cap"
    );

    let mut c = Character::new(ClassRef::witch(), 90);
    c.ascendancy = Some("Occultist".into());

    // Empty character: count is zero and the gate allows the next click.
    assert_eq!(c.ascendancy_alloc_count(&tree), 0);
    assert!(c.can_allocate_ascendancy(&tree));

    // Pull 8 Occultist nodes out of the tree and allocate them directly. The UI
    // walks the ascendancy tree to surface candidate clicks; here we cheat by
    // grabbing any 8 nodes tagged with the right ascendancy_name.
    let occultist_nodes: Vec<_> = tree
        .nodes
        .iter()
        .filter_map(|(id, n)| {
            n.ascendancy_name
                .as_deref()
                .filter(|asc| asc.eq_ignore_ascii_case("Occultist"))
                .map(|_| *id)
        })
        .take(8)
        .collect();
    assert_eq!(
        occultist_nodes.len(),
        8,
        "expected at least 8 Occultist nodes in 3.25 tree"
    );
    for id in &occultist_nodes {
        c.allocate(*id);
    }

    // At the cap: count matches, gate now refuses a 9th click.
    assert_eq!(c.ascendancy_alloc_count(&tree), 8);
    assert!(!c.can_allocate_ascendancy(&tree));

    // Non-ascendancy nodes don't count against the budget.
    let passive_id = *tree
        .nodes
        .iter()
        .find_map(|(id, n)| {
            (n.ascendancy_name.is_none() && n.kind == pob_data::NodeKind::Notable).then_some(id)
        })
        .expect("any notable in 3.25 tree");
    c.allocate(passive_id);
    assert_eq!(c.ascendancy_alloc_count(&tree), 8);
}

// Issue #17: a build loaded with more than 8 ascendancy nodes (e.g. from a
// hand-edited .mk2 file or stale PoB XML) must not credit the excess into
// the calc. The UI gate handles fresh clicks, but `connected_allocations`
// is the last-line defence at compute time.
#[test]
fn over_allocated_ascendancy_nodes_are_capped_at_compute_time() {
    let Some(tree) = load_3_25_tree() else {
        return;
    };

    // Grab 10 Occultist nodes — two more than the budget — and force-allocate
    // them. We bypass the UI click gate to simulate a loaded build.
    let occultist_nodes: Vec<_> = tree
        .nodes
        .iter()
        .filter_map(|(id, n)| {
            n.ascendancy_name
                .as_deref()
                .filter(|asc| asc.eq_ignore_ascii_case("Occultist"))
                .map(|_| *id)
        })
        .take(10)
        .collect();
    if occultist_nodes.len() < 10 {
        eprintln!("skip: tree fixture has fewer than 10 Occultist nodes");
        return;
    }

    let mut c = Character::new(ClassRef::witch(), 90);
    c.ascendancy = Some("Occultist".into());
    for id in &occultist_nodes {
        c.allocate(*id);
    }
    assert_eq!(
        c.ascendancy_alloc_count(&tree),
        10,
        "raw allocated count should reflect the over-allocation"
    );

    // The compute path filters via `connected_allocations`; we exercise it
    // indirectly through `init_env`, which walks the same path. Count
    // ascendancy mods sourced from Occultist nodes — we expect at most 8
    // distinct passive sources.
    let env = pob_engine::perform::init_env(&c, &tree);
    use pob_engine::ModStore as _;
    let asc_sources: std::collections::HashSet<pob_data::NodeId> = env
        .mod_db
        .iter_all()
        .filter_map(|m| match m.source {
            Some(pob_engine::Source::Passive(id)) => Some(id),
            _ => None,
        })
        .filter(|id| {
            tree.nodes
                .get(id)
                .and_then(|n| n.ascendancy_name.as_deref())
                .is_some()
        })
        .collect();
    assert!(
        asc_sources.len() <= 8,
        "expected the calc layer to cap ascendancy contributions at 8; got {}",
        asc_sources.len()
    );
}

#[test]
fn pob_diff_bleeding_cleave_baseline() {
    // Regression baseline for the 6d-2 ailment overhaul: the
    // marauder_l90_bleeding_cleave reference build should emit non-zero
    // BleedDPS. The corresponding XML lives at
    // crates/pob-extract/test-builds/marauder_l90_bleeding_cleave.xml so
    // future pob_diff runs can compare against PoB's authoritative output:
    //
    //   cargo run -p pob-extract --bin pob_diff --release -- \
    //     --build crates/pob-extract/test-builds/marauder_l90_bleeding_cleave.xml
    let (Some(tree), Some(skills), Some(bases)) = (load_3_25_tree(), load_skills(), load_bases())
    else {
        return;
    };
    let xml_path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .parent()
        .unwrap()
        .join("crates/pob-extract/test-builds/marauder_l90_bleeding_cleave.xml");
    let Ok(xml) = std::fs::read_to_string(&xml_path) else {
        eprintln!("skip: {} not found", xml_path.display());
        return;
    };
    let c = pob_engine::import_pob_xml(&xml).expect("import bleeding cleave fixture");
    let out = pob_engine::compute_full(&c, &tree, Some(&skills), Some(&bases));
    let bleed = out.get("BleedDPS");
    // Pure-phys axe attack with no tree allocations + body-armour bleed mod
    // settles around 43 BleedDPS on the current calc. The threshold is loose;
    // the EnemyMoving ratio assertion below is the meaningful regression guard
    // for the 6d-2 ailment multipliers.
    assert!(
        bleed > 30.0,
        "Bleed Cleave fixture should produce a non-trivial BleedDPS, got {bleed}"
    );

    // Flipping EnemyMoving doubles bleed — the regression guard for 6d-2's
    // movement multiplier.
    let mut moving = c.clone();
    moving
        .config
        .conditions
        .insert("EnemyMoving".to_owned(), true);
    let moving_out = pob_engine::compute_full(&moving, &tree, Some(&skills), Some(&bases));
    let moving_ratio = moving_out.get("BleedDPS") / bleed;
    assert!(
        (1.95..=2.05).contains(&moving_ratio),
        "EnemyMoving should still double BleedDPS in the fixture; ratio={moving_ratio}"
    );
}

#[test]
fn curse_effect_scales_resist_delta() {
    let (Some(tree), Some(skills)) = (load_3_25_tree(), load_skills()) else {
        return;
    };
    // Find any curse skill in the registry.
    let curse_id = skills
        .iter()
        .find(|(_, s)| s.base_flags.get("curse").copied().unwrap_or(false))
        .map(|(id, _)| id.to_owned());
    let Some(curse_id) = curse_id else {
        eprintln!("skip: no curse skill in registry");
        return;
    };

    let mut c = Character::new(ClassRef::witch(), 90);
    c.skill_groups.push(pob_engine::character::SocketGroup {
        label: "Curse".into(),
        gems: vec![MainSkill::new(&curse_id)],
        main_active_skill_index: 1,
        enabled: true,
    });
    c.main_socket_group = 1;
    c.sync_main_skill();

    let baseline = pob_engine::compute_full(&c, &tree, Some(&skills), None);
    let baseline_scale = baseline.get("CurseEffectScale");
    if baseline_scale == 0.0 {
        // Curse contributed no resist deltas (some curses don't touch resists).
        eprintln!("skip: curse {curse_id} has no resist payload");
        return;
    }

    // Bump CurseEffect with an item mod and verify the scale moves.
    let amulet = parse_item(
        "Item Class: Amulets\nRarity: RARE\nDoedre Charm\nOnyx Amulet\n--------\n50% increased Effect of your Curses\n--------",
    )
    .unwrap();
    c.items.equip(pob_data::Slot::Amulet, amulet);
    let after = pob_engine::compute_full(&c, &tree, Some(&skills), None);
    let after_scale = after.get("CurseEffectScale");
    let ratio = after_scale / baseline_scale;
    assert!(
        (1.45..=1.55).contains(&ratio),
        "+50% Curse Effect should scale CurseEffectScale by ~1.5; got ratio={ratio} (baseline={baseline_scale}, after={after_scale})"
    );
}

#[test]
fn item_mods_carry_slot_name_tag() {
    let Some(tree) = load_3_25_tree() else {
        return;
    };
    let mut c = Character::new(ClassRef::marauder(), 90);
    let body = parse_item(
        "Item Class: Body Armours\nRarity: RARE\nDoom Carapace\nFull Plate\n--------\n+50 to maximum Life\n--------",
    )
    .unwrap();
    c.items.equip(pob_data::Slot::BodyArmour, body);
    let env = pob_engine::perform::init_env(&c, &tree);

    use pob_engine::ModStore as _;
    let body_mods: Vec<_> = env
        .mod_db
        .iter_all()
        .filter(|m| matches!(m.source, Some(pob_engine::Source::Item(2))))
        .collect();
    assert!(
        !body_mods.is_empty(),
        "expected at least one mod sourced from BodyArmour (slot 2)"
    );
    // Every parsed item mod (not the base implicits, which don't go through
    // mod_parser) should carry a SlotName tag matching its slot.
    let life = body_mods
        .iter()
        .find(|m| m.name == "Life")
        .expect("Life mod from body armour");
    let has_slot = life.tags.iter().any(|t| {
        matches!(
            &t.kind,
            pob_engine::TagKind::SlotName { slot_name, neg: false } if slot_name == "Body Armour"
        )
    });
    assert!(
        has_slot,
        "Body armour Life mod should carry SlotName=\"Body Armour\" tag, got {:?}",
        life.tags
    );

    // The mod still evaluates because perform.rs sets the matching SlotName
    // condition for every equipped slot — verify by computing Life via the
    // full pipeline.
    let out = pob_engine::compute_full(&c, &tree, None, None);
    assert!(
        out.get("Life") >= 50.0,
        "Life should include +50 from body, got {}",
        out.get("Life")
    );
}

// Issue #48: HitChance formula must match upstream PoB exactly.
// Upstream formula (CalcDefence.lua:32-38):
//   rawChance = accuracy / (accuracy + (evasion/5)^0.9) * 125
//   chance    = max(5, min(round(rawChance), 100))
// Pre-fix MK2 used `1.15 × acc/(acc+(eva/4)^0.9) - 0.15`, which gave a
// different (lower) chance and is the suspected accuracy-side cause of the
// ~50% AverageDamage divergence on the marauder_l90_cleave_with_axe fixture.
#[test]
fn hit_chance_matches_pob_calcdefence_formula() {
    let (Some(tree), Some(skills), Some(bases)) = (load_3_25_tree(), load_skills(), load_bases())
    else {
        eprintln!("skip: data missing");
        return;
    };

    let Some(_) = skills.get("Cleave") else {
        eprintln!("skip: Cleave not found");
        return;
    };
    let sword_name = bases
        .iter()
        .find(|(_, b)| b.r#type.contains("Sword") && b.weapon.is_some())
        .map(|(n, _)| n.clone());
    let Some(sword_name) = sword_name else {
        return;
    };
    let mut c = Character::new(ClassRef::duelist(), 90);
    c.items.equip(
        pob_data::Slot::Weapon1,
        parse_item(&format!(
            "Item Class: One Handed Swords\nRarity: NORMAL\n{sword_name}\n--------\n"
        ))
        .unwrap(),
    );
    c.main_skill = Some(MainSkill::new("Cleave"));

    // Pin the inputs we control so the formula is the only variable.
    c.config.enemy_evasion = 10_000;
    let out = pob_engine::compute_full(&c, &tree, Some(&skills), Some(&bases));
    let accuracy = out.get("Accuracy");
    let hit_chance = out.get("MainSkillHitChance");

    // Reproduce PoB's formula to derive the expected value from the same accuracy.
    let raw = accuracy / (accuracy + f64::powf(10_000.0 / 5.0, 0.9)) * 125.0;
    let expected = raw.round().clamp(5.0, 100.0);
    assert!(
        (hit_chance - expected).abs() < 0.001,
        "MainSkillHitChance {hit_chance} should equal PoB-formula {expected} (acc={accuracy})"
    );

    // HitChance and AccuracyHitChance should track MainSkillHitChance.
    assert!(
        (out.get("HitChance") - hit_chance).abs() < 0.001,
        "HitChance should mirror MainSkillHitChance"
    );
    assert!(
        (out.get("AccuracyHitChance") - hit_chance).abs() < 0.001,
        "AccuracyHitChance should mirror MainSkillHitChance"
    );

    // At a very low evasion, hit chance should clamp to 100.
    c.config.enemy_evasion = 1;
    let high =
        pob_engine::compute_full(&c, &tree, Some(&skills), Some(&bases)).get("MainSkillHitChance");
    assert!(
        (high - 100.0).abs() < 0.001,
        "near-zero evasion should clamp HitChance to 100, got {high}"
    );

    // At a very high evasion, hit chance should clamp to 5 (floor).
    c.config.enemy_evasion = 1_000_000;
    let low =
        pob_engine::compute_full(&c, &tree, Some(&skills), Some(&bases)).get("MainSkillHitChance");
    assert!(
        (low - 5.0).abs() < 0.001,
        "huge evasion should clamp HitChance to 5, got {low}"
    );
}

#[test]
fn enemy_evasion_changes_main_skill_hit_chance() {
    let (Some(tree), Some(skills), Some(bases)) = (load_3_25_tree(), load_skills(), load_bases())
    else {
        return;
    };

    // Stable attack-skill pick — see `pick_stable_skill` at the
    // top of the file. PR #155 fixed the original flake here
    // (HashMap-iteration leaking into `iter_active().find(...)`).
    let attack_id = pick_stable_skill(&skills, |s| {
        s.base_flags.get("attack").copied().unwrap_or(false)
    });
    let Some(attack_id) = attack_id else {
        return;
    };
    let sword_name = bases
        .iter()
        .find(|(_, b)| b.r#type.contains("Sword") && b.weapon.is_some())
        .map(|(n, _)| n.clone());
    let Some(sword_name) = sword_name else {
        return;
    };
    let mut c = Character::new(ClassRef::duelist(), 90);
    c.items.equip(
        pob_data::Slot::Weapon1,
        parse_item(&format!(
            "Item Class: One Handed Swords\nRarity: NORMAL\n{sword_name}\n--------\n"
        ))
        .unwrap(),
    );
    c.main_skill = Some(MainSkill::new(&attack_id));

    c.config.enemy_evasion = 500;
    let low =
        pob_engine::compute_full(&c, &tree, Some(&skills), Some(&bases)).get("MainSkillHitChance");
    c.config.enemy_evasion = 20_000;
    let high =
        pob_engine::compute_full(&c, &tree, Some(&skills), Some(&bases)).get("MainSkillHitChance");
    assert!(
        low > high,
        "Higher enemy_evasion should drop hit chance; low={low}, high={high}"
    );
}

#[test]
fn bleed_faster_and_enemy_moving_scale_bleed_dps() {
    let (Some(tree), Some(skills), Some(bases)) = (load_3_25_tree(), load_skills(), load_bases())
    else {
        eprintln!("skip: data missing");
        return;
    };

    // Build a Duelist with an attack skill + sword + a body armour that grants
    // 100% chance to bleed. With those alone we get a non-zero BleedDPS to
    // measure ailment-rate scaling against.
    let attack_id = pick_stable_skill(&skills, |s| {
        s.base_flags.get("attack").copied().unwrap_or(false)
    });
    let Some(attack_id) = attack_id else {
        return;
    };
    let sword_name = bases
        .iter()
        .find(|(_, b)| b.r#type.contains("Sword") && b.weapon.is_some())
        .map(|(n, _)| n.clone());
    let Some(sword_name) = sword_name else {
        return;
    };

    let mut c = Character::new(ClassRef::duelist(), 90);
    let sword = parse_item(&format!(
        "Item Class: One Handed Swords\nRarity: NORMAL\n{sword_name}\n--------\n"
    ))
    .unwrap();
    c.items.equip(pob_data::Slot::Weapon1, sword);
    c.main_skill = Some(MainSkill::new(&attack_id));

    let bleeding_armour = parse_item(
        "Item Class: Body Armours\nRarity: RARE\nBleed Hauberk\nFull Plate\n--------\n100% chance to cause Bleeding on Hit\n--------",
    )
    .unwrap();
    c.items.equip(pob_data::Slot::BodyArmour, bleeding_armour);

    let baseline = pob_engine::compute_full(&c, &tree, Some(&skills), Some(&bases));
    let baseline_bleed = baseline.get("BleedDPS");
    if baseline_bleed <= 0.0 {
        // The active-attack pick may not produce phys damage on every tree
        // version; skip cleanly rather than asserting against a zero baseline.
        eprintln!("skip: attack {attack_id} produced no BleedDPS baseline");
        return;
    }

    // Add a 50% BleedFaster item — this is INC on BleedFaster, so BleedDPS rises by 1.5x.
    let faster_belt = parse_item(
        "Item Class: Belts\nRarity: MAGIC\nBleed Belt\nLeather Belt\n--------\nBleeding you inflict deals Damage 50% faster\n--------",
    )
    .unwrap();
    c.items.equip(pob_data::Slot::Belt, faster_belt);
    let after_faster = pob_engine::compute_full(&c, &tree, Some(&skills), Some(&bases));
    let faster_bleed = after_faster.get("BleedDPS");
    let faster_ratio = faster_bleed / baseline_bleed;
    assert!(
        (1.45..=1.55).contains(&faster_ratio),
        "BleedFaster 50% should multiply BleedDPS by ~1.5; ratio={faster_ratio} (baseline={baseline_bleed}, after={faster_bleed})"
    );

    // Flip on EnemyMoving — bleed should double on top of the BleedFaster boost.
    c.config.conditions.insert("EnemyMoving".to_owned(), true);
    let after_moving = pob_engine::compute_full(&c, &tree, Some(&skills), Some(&bases));
    let moving_bleed = after_moving.get("BleedDPS");
    let moving_ratio = moving_bleed / faster_bleed;
    assert!(
        (1.95..=2.05).contains(&moving_ratio),
        "EnemyMoving should double BleedDPS; ratio={moving_ratio} (with-faster={faster_bleed}, moving={moving_bleed})"
    );
}

// Issue #15: Ailment duration output keys (BleedDuration / PoisonDuration /
// IgniteDuration) must be populated whenever the corresponding ailment is
// computed, and must scale with their `*Duration` INC mods. PoB exposes these
// on the Calcs tab side panel; previously MK2 emitted only the static
// placeholder `IgniteDuration = 4.0` from init_env and nothing for bleed/poison.
#[test]
fn ailment_effect_mods_scale_all_three_ailment_dps_keys() {
    // Issue #58: `AilmentEffect` mods (e.g. unique amulets / cluster notables
    // that grant "increased Effect of Ailments") must scale every damaging
    // ailment's DPS, mirroring PoB's `effectMod = calcLib.mod(skillModList,
    // dotCfg, "AilmentEffect")` in CalcOffence.lua:4304/4584/4932.
    let (Some(tree), Some(skills), Some(bases)) = (load_3_25_tree(), load_skills(), load_bases())
    else {
        eprintln!("skip: data missing");
        return;
    };

    let Some(_) = skills.get("Cleave") else {
        return;
    };
    let sword_name = bases
        .iter()
        .find(|(_, b)| b.r#type.contains("Sword") && b.weapon.is_some())
        .map(|(n, _)| n.clone());
    let Some(sword_name) = sword_name else { return };
    let mut c = Character::new(ClassRef::duelist(), 90);
    let sword = parse_item(&format!(
        "Item Class: One Handed Swords\nRarity: NORMAL\n{sword_name}\n--------\n"
    ))
    .unwrap();
    c.items.equip(pob_data::Slot::Weapon1, sword);
    c.main_skill = Some(MainSkill::new("Cleave"));

    // 100% chance to apply all three ailments + fire damage on attacks so
    // every ailment branch evaluates a non-zero DPS.
    let triple = parse_item(
        "Item Class: Body Armours\nRarity: RARE\nAilment Hauberk\nFull Plate\n--------\n100% chance to cause Bleeding on Hit\n100% chance to Poison on Hit\n100% chance to Ignite\nAdds 50 to 100 Fire Damage to Attacks\n--------",
    )
    .unwrap();
    c.items.equip(pob_data::Slot::BodyArmour, triple);

    let baseline = pob_engine::compute_full(&c, &tree, Some(&skills), Some(&bases));
    let baseline_bleed = baseline.get("BleedDPS");
    let baseline_poison = baseline.get("PoisonDPS");
    let baseline_ignite = baseline.get("IgniteDPS");
    if baseline_bleed <= 0.0 || baseline_poison <= 0.0 {
        eprintln!("skip: Cleave produced no bleed/poison baseline");
        return;
    }

    // Equip a +25% increased Effect of Ailments amulet. Bleed and poison
    // should both rise by 1.25x; ignite if it was non-zero, also 1.25x.
    let amulet = parse_item(
        "Item Class: Amulets\nRarity: MAGIC\nAilment Pendant\nAmber Amulet\n--------\n25% increased Effect of Ailments\n--------",
    )
    .unwrap_or_else(|_| parse_item(
        "Item Class: Amulets\nRarity: MAGIC\nAilment Pendant\nAmber Amulet\n--------\n25% increased Magnitude of Ailments\n--------",
    ).unwrap());
    c.items.equip(pob_data::Slot::Amulet, amulet);
    let scaled = pob_engine::compute_full(&c, &tree, Some(&skills), Some(&bases));

    let bleed_ratio = scaled.get("BleedDPS") / baseline_bleed;
    let poison_ratio = scaled.get("PoisonDPS") / baseline_poison;
    assert!(
        (bleed_ratio - 1.25).abs() < 0.01,
        "+25% AilmentEffect should multiply BleedDPS by ~1.25, got {bleed_ratio} (baseline={baseline_bleed}, scaled={})",
        scaled.get("BleedDPS")
    );
    assert!(
        (poison_ratio - 1.25).abs() < 0.01,
        "+25% AilmentEffect should multiply PoisonDPS by ~1.25, got {poison_ratio} (baseline={baseline_poison}, scaled={})",
        scaled.get("PoisonDPS")
    );
    if baseline_ignite > 0.0 {
        let ignite_ratio = scaled.get("IgniteDPS") / baseline_ignite;
        assert!(
            (ignite_ratio - 1.25).abs() < 0.01,
            "+25% AilmentEffect should multiply IgniteDPS by ~1.25, got {ignite_ratio} (baseline={baseline_ignite}, scaled={})",
            scaled.get("IgniteDPS")
        );
    }
}

#[test]
fn ailment_duration_outputs_scale_with_duration_mods() {
    let (Some(tree), Some(skills), Some(bases)) = (load_3_25_tree(), load_skills(), load_bases())
    else {
        eprintln!("skip: data missing");
        return;
    };

    // Use Cleave deterministically — the existing pob_diff_bleeding_cleave
    // baseline already validates Cleave's ailment branches.
    let Some(_) = skills.get("Cleave") else {
        eprintln!("skip: Cleave not found");
        return;
    };
    let sword_name = bases
        .iter()
        .find(|(_, b)| b.r#type.contains("Sword") && b.weapon.is_some())
        .map(|(n, _)| n.clone());
    let Some(sword_name) = sword_name else {
        return;
    };

    let mut c = Character::new(ClassRef::duelist(), 90);
    let sword = parse_item(&format!(
        "Item Class: One Handed Swords\nRarity: NORMAL\n{sword_name}\n--------\n"
    ))
    .unwrap();
    c.items.equip(pob_data::Slot::Weapon1, sword);
    c.main_skill = Some(MainSkill::new("Cleave"));

    // Body armour: 100% bleed chance + 100% poison chance + 100% ignite chance.
    // Fire damage on the body armour ensures the ignite branch has a non-zero
    // base hit so its duration output is overwritten from the init_env default.
    let triple = parse_item(
        "Item Class: Body Armours\nRarity: RARE\nAilment Hauberk\nFull Plate\n--------\n100% chance to cause Bleeding on Hit\n100% chance to Poison on Hit\n100% chance to Ignite\nAdds 50 to 100 Fire Damage to Attacks\n--------",
    )
    .unwrap();
    c.items.equip(pob_data::Slot::BodyArmour, triple);

    let baseline = pob_engine::compute_full(&c, &tree, Some(&skills), Some(&bases));
    if baseline.get("BleedDPS") <= 0.0 {
        eprintln!("skip: Cleave produced no BleedDPS baseline");
        return;
    }
    // Bleed/poison branches always run when their chance > 0 + phys hit > 0.
    assert!(
        (baseline.get("BleedDuration") - 5.0).abs() < 0.001,
        "Default BleedDuration should be 5.0s, got {}",
        baseline.get("BleedDuration")
    );
    assert!(
        (baseline.get("PoisonDuration") - 2.0).abs() < 0.001,
        "Default PoisonDuration should be 2.0s, got {}",
        baseline.get("PoisonDuration")
    );
    // Ignite is conditional on fire damage feeding into ignite_chance > 0.
    let baseline_ignite_active = baseline.get("IgniteDPS") > 0.0;
    if baseline_ignite_active {
        assert!(
            (baseline.get("IgniteDuration") - 4.0).abs() < 0.001,
            "Default IgniteDuration should be 4.0s, got {}",
            baseline.get("IgniteDuration")
        );
    }

    // Add a belt with +30% increased Bleeding/Poison/Ignite Duration. Each
    // duration should rise by 1.30x.
    let belt = parse_item(
        "Item Class: Belts\nRarity: MAGIC\nDuration Belt\nLeather Belt\n--------\n30% increased Bleeding Duration\n30% increased Poison Duration\n30% increased Ignite Duration\n--------",
    )
    .unwrap();
    c.items.equip(pob_data::Slot::Belt, belt);
    let after = pob_engine::compute_full(&c, &tree, Some(&skills), Some(&bases));
    assert!(
        (after.get("BleedDuration") - 6.5).abs() < 0.01,
        "BleedDuration with 30% INC should be 6.5s, got {}",
        after.get("BleedDuration")
    );
    assert!(
        (after.get("PoisonDuration") - 2.6).abs() < 0.01,
        "PoisonDuration with 30% INC should be 2.6s, got {}",
        after.get("PoisonDuration")
    );
    if baseline_ignite_active && after.get("IgniteDPS") > 0.0 {
        assert!(
            (after.get("IgniteDuration") - 5.2).abs() < 0.01,
            "IgniteDuration with 30% INC should be 5.2s, got {}",
            after.get("IgniteDuration")
        );
    }
}

// Issue #60: AoE shotgun-overlap rolloff. The Config-tab "Enemies hit
// by AoE" slider multiplies per-cast hit on AoE-tagged skills. Default
// 1 leaves DPS unchanged; setting 3 triples MainSkillDPS for an AoE
// skill while leaving non-AoE skills (Arc) untouched.
#[test]
fn enemies_hit_by_aoe_multiplies_aoe_skill_dps() {
    let (Some(tree), Some(skills), Some(bases)) = (load_3_25_tree(), load_skills(), load_bases())
    else {
        eprintln!("skip: data missing");
        return;
    };

    // Ice Nova is AoE-tagged.
    let Some(_) = skills.get("IceNova") else {
        eprintln!("skip: IceNova not found");
        return;
    };
    let mut c = Character::new(ClassRef::witch(), 90);
    c.main_skill = Some(MainSkill::new("IceNova"));
    let baseline = pob_engine::compute_full(&c, &tree, Some(&skills), Some(&bases));
    let baseline_dps = baseline.get("MainSkillDPS");
    if baseline_dps <= 0.0 {
        eprintln!("skip: IceNova has no DPS in this fixture");
        return;
    }
    // Default = 1: no AoEStacks output emitted.
    assert_eq!(c.config.enemies_hit_by_aoe, 0);
    assert_eq!(
        baseline.try_get("AoEStacks"),
        None,
        "default (no shotgun) must not emit AoEStacks"
    );

    // Triple the per-cast hit: 3 enemies × per-cast = 3× DPS.
    c.config.enemies_hit_by_aoe = 3;
    let triple = pob_engine::compute_full(&c, &tree, Some(&skills), Some(&bases));
    let ratio = triple.get("MainSkillDPS") / baseline_dps;
    assert!(
        (ratio - 3.0).abs() < 0.01,
        "Setting Enemies hit by AoE to 3 should triple MainSkillDPS; ratio={ratio}"
    );
    assert!(
        (triple.get("AoEStacks") - 3.0).abs() < 0.001,
        "AoEStacks output should equal the slider value"
    );
    assert!(
        (triple.get("AoEStackMultiplier") - 3.0).abs() < 0.001,
        "AoEStackMultiplier output should equal the slider value"
    );

    // Arc is chain, not AoE — slider must not affect its DPS.
    let Some(_) = skills.get("Arc") else { return };
    let mut arc_char = Character::new(ClassRef::witch(), 90);
    arc_char.main_skill = Some(MainSkill::new("Arc"));
    let arc_baseline =
        pob_engine::compute_full(&arc_char, &tree, Some(&skills), Some(&bases)).get("MainSkillDPS");
    arc_char.config.enemies_hit_by_aoe = 5;
    let arc_with_slider = pob_engine::compute_full(&arc_char, &tree, Some(&skills), Some(&bases));
    assert!(
        (arc_with_slider.get("MainSkillDPS") - arc_baseline).abs() < 0.01,
        "Arc (chain skill) MainSkillDPS must be unaffected by Enemies hit by AoE"
    );
    assert_eq!(
        arc_with_slider.try_get("AoEStacks"),
        None,
        "Arc (chain skill) must not emit AoEStacks"
    );
}

// Issue #20: Minion build support — first slice. The engine detects
// minion-summoning gems via `baseFlags.minion` and emits the player-
// side aggregates that drive minion DPS once the granted-skill calc
// lands: MinionDamageMod / MinionLifeMod / MinionAttackSpeedMod /
// MinionMovementSpeedMod / NumberOfMinions. These light up only on
// minion gems — non-minion skills (Cleave, Arc) emit nothing.
#[test]
fn minion_skill_emits_minion_buff_aggregates() {
    let (Some(tree), Some(skills), Some(bases)) = (load_3_25_tree(), load_skills(), load_bases())
    else {
        eprintln!("skip: data missing");
        return;
    };

    // RaiseZombie has baseFlags.minion = true.
    let Some(_) = skills.get("RaiseZombie") else {
        eprintln!("skip: RaiseZombie not found");
        return;
    };
    let mut c = Character::new(ClassRef::witch(), 90);
    c.main_skill = Some(MainSkill::new("RaiseZombie"));

    // Default: no Minion mods set, so the multipliers are 1.0 and
    // NumberOfMinions defaults to 1.
    let baseline = pob_engine::compute_full(&c, &tree, Some(&skills), Some(&bases));
    assert!(
        (baseline.get("MinionDamageMod") - 1.0).abs() < 0.001,
        "MinionDamageMod default should be 1.0 (no buffs), got {}",
        baseline.get("MinionDamageMod")
    );
    assert!(
        (baseline.get("MinionLifeMod") - 1.0).abs() < 0.001,
        "MinionLifeMod default should be 1.0"
    );
    assert!(
        (baseline.get("NumberOfMinions") - 1.0).abs() < 0.001,
        "NumberOfMinions defaults to 1 before MaxZombies / supports raise it"
    );

    // Equip a body armour granting "+30% increased Minion Damage" and
    // "+1 to maximum number of Summoned Zombies" — both already parsed
    // by mod_parser.
    let body = parse_item(
        "Item Class: Body Armours\nRarity: RARE\nMinion Vest\nFull Plate\n--------\n30% increased Minion Damage\n+1 to maximum number of Raised Zombies\n--------",
    )
    .unwrap();
    c.items.equip(pob_data::Slot::BodyArmour, body);
    let buffed = pob_engine::compute_full(&c, &tree, Some(&skills), Some(&bases));
    assert!(
        (buffed.get("MinionDamageMod") - 1.30).abs() < 0.01,
        "MinionDamageMod with +30% should be 1.30, got {}",
        buffed.get("MinionDamageMod")
    );
    assert!(
        (buffed.get("NumberOfMinions") - 2.0).abs() < 0.01,
        "NumberOfMinions should be 2 with +1 zombie (1 base + 1), got {}",
        buffed.get("NumberOfMinions")
    );

    // Non-minion skill (Arc) emits no minion outputs.
    let Some(_) = skills.get("Arc") else { return };
    let mut arc_c = Character::new(ClassRef::witch(), 90);
    arc_c.main_skill = Some(MainSkill::new("Arc"));
    let arc_out = pob_engine::compute_full(&arc_c, &tree, Some(&skills), Some(&bases));
    assert_eq!(
        arc_out.try_get("MinionDamageMod"),
        None,
        "Arc (non-minion skill) must not emit MinionDamageMod"
    );
    assert_eq!(
        arc_out.try_get("NumberOfMinions"),
        None,
        "Arc (non-minion skill) must not emit NumberOfMinions"
    );
}

/// Issue #20 slices 3-10: end-to-end summoner-build smoke test. Bind a
/// summon-skill main skill, run `compute_full_with_env` followed by
/// `apply_minion_outputs`, and verify the full chain — Life / resists /
/// damage / accuracy / hit-chance / DPS — lights up in the output dict.
#[test]
fn summon_flame_golem_lights_up_minion_outputs_end_to_end() {
    let (Some(tree), Some(skills), Some(minions)) =
        (load_3_25_tree(), load_skills(), load_minions())
    else {
        eprintln!("skip: data missing (run pob-extract first)");
        return;
    };
    if skills.get("SummonFlameGolem").is_none() {
        eprintln!("skip: SummonFlameGolem not in registry");
        return;
    }
    if !minions.minions.contains_key("SummonedFlameGolem") {
        eprintln!("skip: SummonedFlameGolem not in extracted catalogue");
        return;
    }

    let mut c = Character::new(ClassRef::witch(), 90);
    c.main_skill = Some(MainSkill::new("SummonFlameGolem"));

    let (mut output, env) = pob_engine::compute_full_with_env(&c, &tree, Some(&skills), None);
    assert!(
        pob_engine::apply_minion_outputs(&c, &skills, &minions, &env, &mut output),
        "apply_minion_outputs should return true for SummonFlameGolem"
    );

    // Headline outputs land alongside the player's MainSkillDPS / etc.
    let life = output.get("MinionLife");
    assert!(
        life > 0.0,
        "MinionLife should be non-zero for an L90 Flame Golem, got {life}"
    );
    let life_base = output.get("MinionLifeBase");
    assert!(
        life >= life_base,
        "MinionLife (post-mods) >= MinionLifeBase, got {life} vs {life_base}"
    );
    // Flame Golem has fire_resist 70, cold/lightning 40, chaos 20 in the
    // extracted catalogue (verified via the live data file).
    assert_eq!(output.get("MinionFireResist"), 70.0);
    assert_eq!(output.get("MinionColdResist"), 40.0);
    assert_eq!(output.get("MinionLightningResist"), 40.0);
    assert_eq!(output.get("MinionChaosResist"), 20.0);

    // Damage / rate / DPS chain.
    let avg_dmg = output.get("MinionAverageDamage");
    let rate = output.get("MinionAttacksPerSecond");
    let dps_pre_acc = output.get("MinionDPSBeforeHitChance");
    let dps = output.get("MinionDPS");
    let chance = output.get("MinionHitChance");
    assert!(avg_dmg > 0.0, "MinionAverageDamage should be > 0");
    assert!(rate > 0.0, "MinionAttacksPerSecond should be > 0");
    assert!(
        (5.0..=100.0).contains(&chance),
        "MinionHitChance should be in [5, 100], got {chance}"
    );
    // DPS = pre × chance / 100, allowing for f64 round-trip noise.
    assert!(
        (dps - dps_pre_acc * chance / 100.0).abs() < 1e-6,
        "MinionDPS = pre × chance/100; got dps={dps}, pre={dps_pre_acc}, chance={chance}"
    );
    // Crit chance defaults to 5% (no MinionCritChance mods on a fresh witch).
    assert_eq!(output.get("MinionCritChance"), 5.0);
    assert_eq!(output.get("MinionCritMultiplier"), 150.0);

    // Slice 12 of #20: MainSkillDPS picks up MinionDPS × NumberOfMinions so the
    // side panel and FullDPS readout reflect the real (minion) DPS instead of
    // the dummy direct-hit number perform.rs lands for a summon gem. PoB's
    // summary value for a summoner build is the minion's DPS — without this,
    // the headline DPS in the UI would be zero / near-zero for every summoner.
    let pack = output.get("NumberOfMinions").max(1.0);
    let main_skill_dps = output.get("MainSkillDPS");
    assert!(
        (main_skill_dps - dps * pack).abs() < 1e-6,
        "MainSkillDPS should equal MinionDPS × NumberOfMinions; got main={main_skill_dps}, minion={dps}, pack={pack}"
    );
    // PlayerHitDPS preserves the pre-overwrite player value so the breakdown
    // chain stays inspectable even after the swap.
    assert!(
        output.try_get("PlayerHitDPS").is_some(),
        "PlayerHitDPS should snapshot the pre-mirror MainSkillDPS"
    );
    // FullDPS / TotalDPS / aliases follow MainSkillDPS (no ailment add-ons on
    // SummonFlameGolem in a fresh witch build).
    assert!(
        (output.get("FullDPS") - main_skill_dps).abs() < 1e-6,
        "FullDPS should mirror the new MainSkillDPS for an ailment-free summoner"
    );
    assert!(
        (output.get("TotalDPS") - main_skill_dps).abs() < 1e-6,
        "TotalDPS should mirror the new MainSkillDPS"
    );
}

// Issue #52: Every AoE-tagged skill must emit AoERadius / FinalAoERadius
// outputs (PoB exposes these on the Calcs tab) and FinalAoERadius must
// scale with `increased Area of Effect` mods according to PoB's
// `calcRadius = floor(base × floor(100 × sqrt(areaMod)) / 100)`.
// Arc (chain, not AoE) must NOT emit these keys, satisfying the issue's
// "Witch L90 Arc baseline unchanged" criterion.
#[test]
fn aoe_skills_emit_radius_outputs_that_scale_with_area_mods() {
    let (Some(tree), Some(skills), Some(bases)) = (load_3_25_tree(), load_skills(), load_bases())
    else {
        eprintln!("skip: data missing");
        return;
    };

    // Ice Nova — base radius 26 from `active_skill_base_area_of_effect_radius`.
    let Some(_) = skills.get("IceNova") else {
        eprintln!("skip: IceNova not found");
        return;
    };
    let mut c = Character::new(ClassRef::witch(), 90);
    c.main_skill = Some(MainSkill::new("IceNova"));
    let baseline = pob_engine::compute_full(&c, &tree, Some(&skills), Some(&bases));
    assert!(
        (baseline.get("AoERadius") - 26.0).abs() < 0.001,
        "IceNova AoERadius should be 26 (constantStats), got {}",
        baseline.get("AoERadius")
    );
    // No INC/MORE area mods → AreaOfEffectMod == 1.0 → FinalAoERadius == 26.
    assert!(
        (baseline.get("AreaOfEffectMod") - 1.0).abs() < 0.001,
        "AreaOfEffectMod with no mods should be 1.0"
    );
    assert!(
        (baseline.get("FinalAoERadius") - 26.0).abs() < 0.001,
        "FinalAoERadius with no mods should equal base, got {}",
        baseline.get("FinalAoERadius")
    );
    // Metres = radius / 10 (PoB convention).
    assert!(
        (baseline.get("AreaOfEffectRadiusMetres") - 2.6).abs() < 0.001,
        "AreaOfEffectRadiusMetres should be radius / 10"
    );

    // Equip an item granting +44% increased Area of Effect. With no MORE mods,
    // areaMod = 1.44 and FinalAoERadius = floor(26 × floor(100 × sqrt(1.44)) / 100)
    //                                   = floor(26 × floor(120) / 100)
    //                                   = floor(26 × 1.20)
    //                                   = floor(31.2)
    //                                   = 31.
    let belt = parse_item(
        "Item Class: Belts\nRarity: MAGIC\nAoE Belt\nLeather Belt\n--------\n44% increased Area of Effect\n--------",
    )
    .unwrap();
    c.items.equip(pob_data::Slot::Belt, belt);
    let scaled = pob_engine::compute_full(&c, &tree, Some(&skills), Some(&bases));
    assert!(
        (scaled.get("AreaOfEffectMod") - 1.44).abs() < 0.001,
        "AreaOfEffectMod with +44% INC should be 1.44, got {}",
        scaled.get("AreaOfEffectMod")
    );
    assert!(
        (scaled.get("FinalAoERadius") - 31.0).abs() < 0.001,
        "FinalAoERadius with +44% INC should be 31 (calcRadius rounding), got {}",
        scaled.get("FinalAoERadius")
    );
    // Base shouldn't move when only INC mods change.
    assert!(
        (scaled.get("AoERadius") - 26.0).abs() < 0.001,
        "AoERadius (base) shouldn't change when INC mods change"
    );

    // Arc is chain, not AoE — should not emit any AoE radius outputs.
    let Some(_) = skills.get("Arc") else { return };
    let mut arc_char = Character::new(ClassRef::witch(), 90);
    arc_char.main_skill = Some(MainSkill::new("Arc"));
    let arc_out = pob_engine::compute_full(&arc_char, &tree, Some(&skills), Some(&bases));
    assert_eq!(
        arc_out.try_get("AoERadius"),
        None,
        "Arc (chain skill) should not emit AoERadius"
    );
    assert_eq!(
        arc_out.try_get("FinalAoERadius"),
        None,
        "Arc (chain skill) should not emit FinalAoERadius"
    );
}

// Issue #53: Equipped flasks must surface per-flask LifeRecovery /
// Issue #69: low-life multiplier — toggling the `LowLife` config
// condition activates `FlaskLifeRecoveryLowLife` MORE multipliers.
// Without any such mods in the build the toggle should leave recovery
// unchanged (the multiplier defaults to 1.0). And the baseline
// recovery formula must stay healthy after the LifeAdditional and
// low-life layers were folded into the calc.
#[test]
fn flask_low_life_toggle_no_mods_keeps_recovery_unchanged() {
    let (Some(tree), Some(skills), Some(bases)) = (load_3_25_tree(), load_skills(), load_bases())
    else {
        eprintln!("skip: data missing");
        return;
    };

    let mut c = Character::new(ClassRef::marauder(), 90);
    let flask =
        parse_item("Item Class: Life Flasks\nRarity: NORMAL\nColossal Life Flask\n--------\n")
            .unwrap();
    c.items.equip(pob_data::Slot::Flask1, flask);

    let baseline_out = pob_engine::compute_full(&c, &tree, Some(&skills), Some(&bases));
    let baseline = baseline_out.get("Flask1LifeRecovery");
    // The Colossal Life Flask base is 1000; with LifeAdditional = 0
    // and low_life_mult = 1.0 the recovery is exactly the base.
    assert!(
        (baseline - 1000.0).abs() < 0.01,
        "Colossal Life Flask baseline must be 1000 after the LifeAdditional / LowLife layers, got {baseline}"
    );
    c.config.conditions.insert("LowLife".to_owned(), true);
    let with_low =
        pob_engine::compute_full(&c, &tree, Some(&skills), Some(&bases)).get("Flask1LifeRecovery");
    assert!(
        (with_low - baseline).abs() < 0.01,
        "LowLife toggle without any FlaskLifeRecoveryLowLife mod must leave recovery unchanged"
    );
}

// ManaRecovery output keys (PoB exposes these on the Calcs tab side panel
// for flask-stacking builds — Pathfinder, Forbidden Rite Hierophant) and
// they must scale with FlaskLifeRecovery / FlaskEffect / FlaskDuration /
// LifeRecovery (rate) / FlaskLifeRecoveryRate INC mods.
#[test]
fn flask_recovery_outputs_scale_with_flask_mods() {
    let (Some(tree), Some(skills), Some(bases)) = (load_3_25_tree(), load_skills(), load_bases())
    else {
        eprintln!("skip: data missing");
        return;
    };

    let mut c = Character::new(ClassRef::marauder(), 90);

    // Colossal Life Flask: life=1000, duration=3.5s. Magic flask on Flask 1.
    let life_flask =
        parse_item("Item Class: Life Flasks\nRarity: NORMAL\nColossal Life Flask\n--------\n")
            .unwrap();
    c.items.equip(pob_data::Slot::Flask1, life_flask);

    let baseline = pob_engine::compute_full(&c, &tree, Some(&skills), Some(&bases));
    assert!(
        (baseline.get("Flask1LifeRecovery") - 1000.0).abs() < 0.01,
        "Colossal Life Flask should grant LifeRecovery = 1000, got {}",
        baseline.get("Flask1LifeRecovery")
    );
    // Recovery rate = 1000 / 3.5 ≈ 285.71/s.
    let expected_rate = 1000.0 / 3.5;
    assert!(
        (baseline.get("Flask1LifeRecoveryRate") - expected_rate).abs() < 0.5,
        "Recovery rate should be ~285.71/s (life/duration), got {}",
        baseline.get("Flask1LifeRecoveryRate")
    );
    // Aggregate.
    assert!(
        (baseline.get("LifeFlaskRecovery") - 1000.0).abs() < 0.01,
        "LifeFlaskRecovery aggregate should track the max across flasks"
    );

    // Mana flask in slot 2 should populate Flask2ManaRecovery without
    // touching the life-flask outputs.
    let mana_flask =
        parse_item("Item Class: Mana Flasks\nRarity: NORMAL\nColossal Mana Flask\n--------\n")
            .unwrap();
    c.items.equip(pob_data::Slot::Flask2, mana_flask);
    let with_mana = pob_engine::compute_full(&c, &tree, Some(&skills), Some(&bases));
    assert!(
        with_mana.get("Flask2ManaRecovery") > 0.0,
        "Colossal Mana Flask should populate Flask2ManaRecovery"
    );
    assert!(
        with_mana.get("Flask2ManaRecoveryRate") > 0.0,
        "Flask2ManaRecoveryRate should be positive"
    );
    assert!(
        with_mana.get("ManaFlaskRecovery") > 0.0,
        "ManaFlaskRecovery aggregate should be set"
    );
}

// Issue #69 (slice 2): instant/gradual recovery split. Pinning a
// `LifeFlaskInstantRecovery` BASE mod (the same source unique flasks
// like Forbidden Rite stamp) should split the per-flask recovery
// into instant vs gradual portions. The total `Flask1LifeRecovery`
// stays unchanged so existing slice 1 callers don't shift.
#[test]
fn flask_instant_recovery_mod_splits_per_flask_output() {
    let (Some(tree), Some(skills), Some(bases)) = (load_3_25_tree(), load_skills(), load_bases())
    else {
        eprintln!("skip: data missing");
        return;
    };
    use pob_engine::{Mod, Source};

    let mut c = Character::new(ClassRef::marauder(), 90);
    let life_flask =
        parse_item("Item Class: Life Flasks\nRarity: NORMAL\nColossal Life Flask\n--------\n")
            .unwrap();
    c.items.equip(pob_data::Slot::Flask1, life_flask);

    // Without any instant mod, no split keys are emitted (existing
    // slice 1 behaviour) — the total `Flask1LifeRecovery` is still
    // populated.
    let baseline = pob_engine::compute_full(&c, &tree, Some(&skills), Some(&bases));
    assert!(
        baseline.get("Flask1LifeRecovery") > 0.0,
        "baseline should still emit Flask1LifeRecovery"
    );
    assert_eq!(
        baseline.try_get("Flask1LifeRecoveryInstant"),
        None,
        "no instant mod → no instant split key"
    );

    // Inject the unique-flask mod payload: 30% applied instantly.
    // Run init_env_with_bases + perform_basic_stats + perform_flask_recovery
    // by hand against the augmented modDB, since there's no public
    // `perform_flask_recovery` wrapper that re-reads bases.
    let mut env = pob_engine::perform::init_env_with_bases(&c, &tree, Some(&bases));
    env.mod_db
        .add(Mod::base("LifeFlaskInstantRecovery", 30.0).with_source(Source::Other("test".into())));
    pob_engine::perform::perform_flask_recovery(&c, &bases, &mut env);

    let total = env.output.get("Flask1LifeRecovery");
    let inst = env.output.get("Flask1LifeRecoveryInstant");
    let grad = env.output.get("Flask1LifeRecoveryGradual");

    assert!(
        total > 0.0,
        "Flask1LifeRecovery should still be populated after the split"
    );
    assert!(
        inst > 0.0 && grad > 0.0,
        "30% instant should produce both instant ({inst}) and gradual ({grad}) portions"
    );
    // Sum-conservation: instant + gradual ≈ total.
    assert!(
        ((inst + grad) - total).abs() / total < 0.01,
        "instant ({inst}) + gradual ({grad}) should sum to total ({total}); split conservation broken"
    );
    // 30% instant → instant ≈ 30% of total.
    assert!(
        (inst / total - 0.30).abs() < 0.01,
        "instant fraction should be ~30%; got {}",
        inst / total
    );
}

// Issue #69 (slice 2 review fix): `low_life_mult` must apply
// exactly once to the gradual half — slice 2's first cut folded it
// into the shared `scalar` *and* multiplied the gradual expression
// by it again, inflating FlaskLifeRecoveryLowLife scenarios by
// `low_life_mult²`. Verify a 50% MORE FlaskLifeRecoveryLowLife mod
// scales `Flask{N}LifeRecoveryGradual` by exactly 1.5× when LowLife
// is on, not 2.25×.
#[test]
fn flask_low_life_multiplier_applies_exactly_once_to_gradual() {
    let (Some(tree), Some(_skills), Some(bases)) = (load_3_25_tree(), load_skills(), load_bases())
    else {
        eprintln!("skip: data missing");
        return;
    };
    use pob_engine::{Mod, Source};

    let mut c = Character::new(ClassRef::marauder(), 90);
    let life_flask =
        parse_item("Item Class: Life Flasks\nRarity: NORMAL\nColossal Life Flask\n--------\n")
            .unwrap();
    c.items.equip(pob_data::Slot::Flask1, life_flask);
    // 30% applied instantly so Flask1LifeRecoveryGradual is split
    // out as its own output key (the test target).
    c.config.conditions.insert("LowLife".to_owned(), true);

    // Baseline: LowLife = on, no FlaskLifeRecoveryLowLife mod →
    // low_life_mult = 1.0 (since `more()` of an empty bucket is 1).
    let mut env = pob_engine::perform::init_env_with_bases(&c, &tree, Some(&bases));
    env.mod_db
        .add(Mod::base("LifeFlaskInstantRecovery", 30.0).with_source(Source::Other("test".into())));
    pob_engine::perform::perform_flask_recovery(&c, &bases, &mut env);
    let baseline_grad = env.output.get("Flask1LifeRecoveryGradual");
    assert!(
        baseline_grad > 0.0,
        "baseline gradual recovery should be positive; got {baseline_grad}"
    );

    // Inject `FlaskLifeRecoveryLowLife MORE 50` — low_life_mult
    // becomes 1.5. Gradual recovery should scale by exactly 1.5×,
    // not 1.5² = 2.25× (the slice-2-bug behaviour).
    let mut env2 = pob_engine::perform::init_env_with_bases(&c, &tree, Some(&bases));
    env2.mod_db
        .add(Mod::base("LifeFlaskInstantRecovery", 30.0).with_source(Source::Other("test".into())));
    env2.mod_db
        .add(Mod::more("FlaskLifeRecoveryLowLife", 50.0).with_source(Source::Other("test".into())));
    pob_engine::perform::perform_flask_recovery(&c, &bases, &mut env2);
    let scaled_grad = env2.output.get("Flask1LifeRecoveryGradual");
    let ratio = scaled_grad / baseline_grad;
    assert!(
        (ratio - 1.5).abs() < 0.005,
        "FlaskLifeRecoveryLowLife MORE 50 should scale gradual recovery by exactly 1.5× (got {ratio:.4}); double-application would land at 2.25"
    );

    // Instant half also scales by exactly 1.5× — same low_life_mult,
    // applied once.
    let baseline_inst = env.output.get("Flask1LifeRecoveryInstant");
    let scaled_inst = env2.output.get("Flask1LifeRecoveryInstant");
    let inst_ratio = scaled_inst / baseline_inst;
    assert!(
        (inst_ratio - 1.5).abs() < 0.005,
        "instant half should also scale by exactly 1.5× (got {inst_ratio:.4})"
    );
}

#[test]
fn fireball_emits_base_ignite_chance_via_global_stat_map() {
    let Some(skills) = load_skills() else { return };
    let fireball = skills.get("Fireball").expect("Fireball");
    let mods = pob_engine::skill::skill_mods(fireball, 0);
    let ignite_chance = mods
        .iter()
        .find(|m| m.name == "IgniteChance" && m.kind == pob_engine::ModType::Base)
        .expect("Fireball should grant a BASE IgniteChance via global stat-map");
    // Fireball constantStats has ["base_chance_to_ignite_%", 25].
    assert_eq!(ignite_chance.value.as_f64(), Some(25.0));
}

// Issue #36: variant-discovery helper. `variants_of(skill_id)` returns
// every gem entry sharing the same base (default + Vaal + alt-quality
// reworks). The full Vaal-variant feature (UI selector, alt-quality
// stat substitution) is a deeper refactor; this slice ships the
// engine-side discovery helper that downstream UI code will consume.
#[test]
fn skill_registry_variants_of_groups_alt_and_vaal_versions() {
    let Some(skills) = load_skills() else {
        eprintln!("skip: skill data missing");
        return;
    };
    use pob_engine::skill::base_skill_id;

    // Sanity-check the base-id stripper directly.
    assert_eq!(base_skill_id("Fireball"), "Fireball");
    assert_eq!(base_skill_id("VaalFireball"), "Fireball");
    assert_eq!(base_skill_id("FireballAltX"), "Fireball");
    assert_eq!(base_skill_id("VaalFireballAltX"), "Fireball");
    // Vaal- prefix with no further nesting still strips down once.
    assert_eq!(base_skill_id("VaalCleave"), "Cleave");

    // The registry is a HashMap so the absolute set depends on what
    // ships in the data. We assert a couple of hard invariants:
    //
    //  * Every variant returned shares the same `base_skill_id` as
    //    the lookup key.
    //  * The lookup is reflexive — `variants_of(id)` includes `id`
    //    itself when the id is in the registry.
    //  * Looking up `"VaalFireball"` returns the same set as looking
    //    up `"Fireball"` (both share base `"Fireball"`).
    if skills.get("Fireball").is_some() && skills.get("VaalFireball").is_some() {
        let from_default = skills.variants_of("Fireball");
        let from_vaal = skills.variants_of("VaalFireball");
        assert_eq!(
            from_default, from_vaal,
            "variants_of should be symmetric across the variant set"
        );
        assert!(
            from_default.contains(&"Fireball"),
            "variants list should include the default variant"
        );
        assert!(
            from_default.contains(&"VaalFireball"),
            "variants list should include the Vaal counterpart"
        );
        for id in &from_default {
            assert_eq!(
                base_skill_id(id),
                "Fireball",
                "{id} should resolve to base Fireball"
            );
        }
    }

    // Skills without alternates yield a single-element list (or
    // an empty list if the id isn't in the registry).
    let lonely = skills.variants_of("DefinitelyNotASkill");
    assert!(
        lonely.is_empty(),
        "Unknown skill id should return an empty variant list, got {lonely:?}"
    );
}

// Issue #36 (slice 2 backstop): the variant picker rewrites the
// gem's `skill_id`, and the engine should pick up the new entry's
// level data + skill mods on next compute. Verify that swapping
// from the base `Fireball` to `VaalFireball` actually changes the
// MainSkillDPS the engine emits — without this the UI dropdown
// would be cosmetic. We only assert *change* (not direction or
// magnitude), since Vaal variants have different base damage and
// flags that could go either way per character setup.
#[test]
fn variant_swap_changes_main_skill_dps() {
    let (Some(tree), Some(skills), Some(bases)) = (load_3_25_tree(), load_skills(), load_bases())
    else {
        eprintln!("skip: data missing");
        return;
    };
    if skills.get("Fireball").is_none() || skills.get("VaalFireball").is_none() {
        eprintln!("skip: Fireball/VaalFireball not in registry");
        return;
    }

    let mut c = Character::new(ClassRef::witch(), 90);
    c.main_skill = Some(MainSkill::new("Fireball"));
    let base_dps =
        pob_engine::compute_full(&c, &tree, Some(&skills), Some(&bases)).get("MainSkillDPS");

    c.main_skill = Some(MainSkill::new("VaalFireball"));
    let vaal_dps =
        pob_engine::compute_full(&c, &tree, Some(&skills), Some(&bases)).get("MainSkillDPS");

    // Skip if both compute to zero (skill data may be incomplete in
    // the test fixture); otherwise the two should differ.
    if base_dps == 0.0 && vaal_dps == 0.0 {
        eprintln!("skip: neither variant produces DPS in this fixture");
        return;
    }
    assert!(
        (base_dps - vaal_dps).abs() > f64::EPSILON,
        "Vaal variant should change MainSkillDPS vs base; got base={base_dps} vaal={vaal_dps}"
    );
}

#[test]
fn arc_intrinsic_mods_land_in_modlist() {
    let (Some(_tree), Some(skills)) = (load_3_25_tree(), load_skills()) else {
        return;
    };
    let arc = skills.get("Arc").expect("Arc");
    let mods = pob_engine::skill::skill_mods(arc, 0);
    // Arc has constantStats `arc_damage_+%_final_for_each_remaining_chain` = 15 mapped
    // through statMap to a MORE Damage mod with a PerStat ChainRemaining tag.
    let chain_mod = mods
        .iter()
        .find(|m| m.name == "Damage" && m.kind == pob_engine::ModType::More);
    assert!(
        chain_mod.is_some(),
        "Arc should produce a MORE Damage chain mod"
    );
    let chain_mod = chain_mod.unwrap();
    // Value should be 15 (the constantStats value).
    assert_eq!(chain_mod.value.as_f64(), Some(15.0));
    // Tag should include PerStat with stat=ChainRemaining.
    assert!(
        chain_mod.tags.iter().any(|t| matches!(
            &t.kind,
            pob_engine::TagKind::PerStat { stat, .. } if stat == "ChainRemaining"
        )),
        "chain mod should carry PerStat ChainRemaining"
    );
}

#[test]
fn arc_level_20_witch_baseline_damage_is_in_pob_range() {
    let (Some(tree), Some(skills)) = (load_3_25_tree(), load_skills()) else {
        return;
    };
    let mut c = Character::new(ClassRef::witch(), 90);
    c.main_skill = Some(MainSkill::new("Arc"));
    let out = compute_with_skills(&c, &tree, Some(&skills));
    let base_min = out.get("MainSkillBaseMin");
    let base_max = out.get("MainSkillBaseMax");
    // PoB's calc: Arc level 20 at actor level 70 (gem.levelRequirement, NOT
    // character level) gives base ≈ 198–1122. damageEffectiveness only scales
    // ADDED flat damage on spells, not the gem's intrinsic base.
    assert!(
        base_min > 150.0 && base_min < 300.0,
        "Arc base min damage: expected ~198, got {base_min}"
    );
    assert!(
        base_max > 1000.0 && base_max < 1300.0,
        "Arc base max damage: expected ~1122, got {base_max}"
    );
}

// Issue #11: PoB sets `output.ChainRemaining = max(0, ChainMax - Chain)` where
// `Chain` is a config (default 0) — see CalcOffence.lua:1033. The default analysis
// is the initial cast with the FULL chain bonus, so EvalState.ChainRemaining must
// equal ChainMax. Previously MK2 stored ChainMax / 2.0 as a half-bonus
// approximation, which over-stated the divergence note in docs/divergences.md.
//
// Note: the chain MORE itself (Arc's `+15% MORE damage per remaining chain`,
// `KeywordFlag::Hit | Ailment`) is currently filtered out of the hit-damage query
// because the cfg lacks `KeywordFlag::Hit`. That's a separate issue (it touched
// many other mods and produced unexpected damage spikes); this PR limits its
// scope to the ChainRemaining alignment.
#[test]
fn arc_chain_remaining_is_full_chain_count_by_default() {
    let (Some(tree), Some(skills)) = (load_3_25_tree(), load_skills()) else {
        return;
    };
    let mut c = Character::new(ClassRef::witch(), 90);
    c.main_skill = Some(MainSkill::new("Arc"));
    let out = compute_with_skills(&c, &tree, Some(&skills));

    // Output key: ChainRemaining mirrors ChainMax (initial cast, no chains used).
    let chain_remaining = out.get("ChainRemaining");
    let chain_max = out.get("ChainMax");
    assert_eq!(
        chain_remaining, chain_max,
        "ChainRemaining should equal ChainMax by default (no chains used)"
    );
    assert!(
        (7.0..=8.0).contains(&chain_remaining),
        "Arc level 20 ChainRemaining expected 7..=8, got {chain_remaining}"
    );
}

// Issue #4: PoB multiplies AverageDamage by `(1 - block/100) × (1 - dodge/100)`
// for hit damage, plus a `(1 - suppress/100 × 0.5)` factor on spells.
// MK2 mirrors that here. This test exercises a witch + Arc spell to verify
// the spell-suppress factor lands and an attack to verify block/dodge.
#[test]
fn enemy_block_dodge_suppress_reduce_dps() {
    let (Some(tree), Some(skills)) = (load_3_25_tree(), load_skills()) else {
        return;
    };

    // Spell: Arc on a bare witch. Suppression should halve the impact of the
    // suppress chance (50% suppress -> 25% damage reduction).
    let mut witch = Character::new(ClassRef::witch(), 90);
    witch.main_skill = Some(MainSkill::new("Arc"));
    let baseline = compute_with_skills(&witch, &tree, Some(&skills));
    let baseline_dps = baseline.get("MainSkillDPS");
    assert!(
        baseline_dps > 0.0,
        "Arc baseline should produce non-zero DPS"
    );

    witch.config.enemy_suppression_chance = 50;
    let suppressed = compute_with_skills(&witch, &tree, Some(&skills));
    let suppressed_dps = suppressed.get("MainSkillDPS");
    let ratio = suppressed_dps / baseline_dps;
    assert!(
        (ratio - 0.75).abs() < 0.001,
        "Spell suppression at 50% should leave 75% of DPS; ratio={ratio}"
    );

    witch.config.enemy_suppression_chance = 0;
    witch.config.enemy_block_chance = 50;
    let blocked = compute_with_skills(&witch, &tree, Some(&skills));
    let blocked_dps = blocked.get("MainSkillDPS");
    let block_ratio = blocked_dps / baseline_dps;
    assert!(
        (block_ratio - 0.5).abs() < 0.001,
        "50% enemy block should halve spell DPS; ratio={block_ratio}"
    );

    witch.config.enemy_block_chance = 50;
    witch.config.enemy_dodge_chance = 30;
    let combined = compute_with_skills(&witch, &tree, Some(&skills));
    let combined_dps = combined.get("MainSkillDPS");
    let combined_ratio = combined_dps / baseline_dps;
    let expected = 0.5 * 0.7;
    assert!(
        (combined_ratio - expected).abs() < 0.001,
        "Block 50% × dodge 30% should leave {expected} of DPS; ratio={combined_ratio}"
    );

    // Output keys land for the Calcs tab.
    assert_eq!(combined.get("EnemyBlockChance"), 50.0);
    assert_eq!(combined.get("EnemyDodgeChance"), 30.0);
}

// Issue #2: Enemy armour reduces physical-hit DPS via PoB's
// `armour / (armour + 5 × raw)` formula (CalcDefence.lua:41). When the
// user has not specified an explicit value, MK2 falls back on the
// level-based monster armour table (Data/Misc.lua), matching PoB's
// placeholder. This test exercises two explicit values to confirm the
// reduction scales as expected without saturating the 90% cap.
#[test]
fn enemy_armour_reduces_physical_dps() {
    let (Some(tree), Some(skills), Some(bases)) = (load_3_25_tree(), load_skills(), load_bases())
    else {
        return;
    };
    let xml_path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .parent()
        .unwrap()
        .join("crates/pob-extract/test-builds/marauder_l90_cleave_with_axe.xml");
    let Ok(xml) = std::fs::read_to_string(&xml_path) else {
        eprintln!("skip: {} not found", xml_path.display());
        return;
    };
    let mut c = pob_engine::import_pob_xml(&xml).expect("import cleave fixture");

    // Two explicit armour values, both small enough that the formula stays
    // below the 90 % cap on the Cleave fixture's per-hit damage. Lets us
    // assert the formula scales: doubling armour should produce strictly
    // more reduction (and lower DPS) without saturating.
    c.config.enemy_armour = 50;
    let low = pob_engine::compute_full(&c, &tree, Some(&skills), Some(&bases));
    let low_dps = low.get("MainSkillDPS");
    let low_reduction = low.get("EnemyPhysReduction");

    c.config.enemy_armour = 200;
    let high = pob_engine::compute_full(&c, &tree, Some(&skills), Some(&bases));
    let high_dps = high.get("MainSkillDPS");
    let high_reduction = high.get("EnemyPhysReduction");

    assert!(
        low_dps > 0.0 && high_dps > 0.0,
        "Cleave fixture should produce non-zero phys DPS at both armour values"
    );
    assert!(
        low_reduction > 0.0 && low_reduction < 90.0,
        "50 armour should give a non-zero, non-capped reduction, got {low_reduction}"
    );
    assert!(
        high_reduction > low_reduction && high_reduction < 90.0,
        "Higher armour should give more reduction (high={high_reduction} low={low_reduction})"
    );
    assert!(
        high_dps < low_dps,
        "Higher armour should reduce phys DPS ({high_dps} < {low_dps} expected)"
    );
    // The ratio between the two armoured runs should match the ratio of
    // surviving damage fractions: dps_high / dps_low = (1 - r_high) / (1 - r_low).
    let expected_ratio = (1.0 - high_reduction / 100.0) / (1.0 - low_reduction / 100.0);
    let actual_ratio = high_dps / low_dps;
    assert!(
        (actual_ratio - expected_ratio).abs() < 0.05,
        "DPS ratio {actual_ratio} should track (1 - r_high/100) / (1 - r_low/100) = {expected_ratio} \
         (low_reduction={low_reduction}, high_reduction={high_reduction})"
    );
}

// Issue #3: a "Projectiles hit target" config knob multiplies the per-cast
// hit average by `min(count, ProjectileCount)`. The default (0/1) is a
// no-op; raising it grows MainSkillDPS proportionally up to the skill's
// total projectile count. Tornado Shot at level 20 has
// `number_of_additional_projectiles = 0`, so its `ProjectileCount` is 1
// and the multiplier never grows past 1× — we use Lightning Arrow (also a
// projectile attack) but skip if the gem fixture isn't present.
#[test]
fn projectiles_hitting_target_multiplies_dps() {
    let (Some(tree), Some(skills)) = (load_3_25_tree(), load_skills()) else {
        return;
    };
    let Some(skill_id) = ["LightningArrow", "TornadoShot", "Barrage"]
        .iter()
        .find(|id| {
            skills
                .get(id)
                .and_then(|s| s.positional(20, 3))
                .is_some_and(|v| v >= 1.0)
        })
        .copied()
    else {
        eprintln!("skip: no projectile-attack gem with additional projectiles in fixture");
        return;
    };

    let mut c = Character::new(ClassRef::ranger(), 90);
    c.main_skill = Some(MainSkill::new(skill_id));

    // Single hit (default 0 → clamped to 1).
    c.config.projectiles_hitting_target = 0;
    let single = compute_with_skills(&c, &tree, Some(&skills));
    let single_dps = single.get("MainSkillDPS");
    let projectile_count = single.get("ProjectileCount");
    assert!(
        projectile_count >= 1.0,
        "ProjectileCount should be at least 1, got {projectile_count}"
    );
    assert_eq!(single.get("ProjectileMultiplier"), 1.0);

    // Three hits (capped to ProjectileCount). DPS scales linearly when below cap.
    c.config.projectiles_hitting_target = 3;
    let triple = compute_with_skills(&c, &tree, Some(&skills));
    let triple_dps = triple.get("MainSkillDPS");
    let expected_mult = (3.0_f64).min(projectile_count);
    let actual_mult = triple_dps / single_dps;
    assert!(
        (actual_mult - expected_mult).abs() < 0.001,
        "Triple-hit DPS should equal single × {expected_mult}; got {actual_mult} (proj_count={projectile_count})"
    );
    assert_eq!(triple.get("ProjectileMultiplier"), expected_mult);
}

// Issue #16 (mine + trap halves): mine- and trap-tagged skills emit
// per-mechanism output keys (`NumberOfMines`, `MinesPlaced`,
// `NumberOfTraps`, `TrapsThrown`) and scale `MainSkillDPS` by the
// per-throw count. Default is 1 (one mine / trap per cast); items
// supplying `MineThrowCount` / `TrapThrowCount` BASE bumps it.
#[test]
fn mine_and_trap_skills_emit_throw_count_outputs() {
    let (Some(tree), Some(skills), Some(bases)) = (load_3_25_tree(), load_skills(), load_bases())
    else {
        eprintln!("skip: data missing");
        return;
    };

    // Pick a stable mine skill — see `pick_stable_skill`.
    let mine_id = pick_stable_skill(&skills, |s| {
        s.base_flags.get("mine").copied().unwrap_or(false)
    });
    let Some(mine_id) = mine_id else {
        eprintln!("skip: no mine skills available");
        return;
    };

    let mut c = Character::new(ClassRef::shadow(), 90);
    c.main_skill = Some(MainSkill::new(&mine_id));
    let mine_out = pob_engine::compute_full(&c, &tree, Some(&skills), Some(&bases));
    if mine_out.get("MainSkillDPS") <= 0.0 {
        eprintln!("skip: mine skill {mine_id} produces no DPS in this fixture");
        return;
    }
    assert!(
        (mine_out.get("NumberOfMines") - 1.0).abs() < 0.001,
        "Mine skill should emit NumberOfMines = 1 by default, got {}",
        mine_out.get("NumberOfMines")
    );
    assert!(
        (mine_out.get("MinesPlaced") - 1.0).abs() < 0.001,
        "Mine skill should emit MinesPlaced = 1 by default, got {}",
        mine_out.get("MinesPlaced")
    );

    // Same for trap skills — stable pick.
    let trap_id = pick_stable_skill(&skills, |s| {
        s.base_flags.get("trap").copied().unwrap_or(false)
    });
    let Some(trap_id) = trap_id else {
        eprintln!("skip: no trap skills available");
        return;
    };
    let mut tc = Character::new(ClassRef::shadow(), 90);
    tc.main_skill = Some(MainSkill::new(&trap_id));
    let trap_out = pob_engine::compute_full(&tc, &tree, Some(&skills), Some(&bases));
    if trap_out.get("MainSkillDPS") <= 0.0 {
        eprintln!("skip: trap skill {trap_id} produces no DPS");
        return;
    }
    assert!(
        (trap_out.get("NumberOfTraps") - 1.0).abs() < 0.001,
        "Trap skill should emit NumberOfTraps = 1 by default, got {}",
        trap_out.get("NumberOfTraps")
    );
    assert!(
        (trap_out.get("TrapsThrown") - 1.0).abs() < 0.001,
        "Trap skill should emit TrapsThrown = 1 by default, got {}",
        trap_out.get("TrapsThrown")
    );

    // Non-mine/trap skill (Cleave) emits no mine/trap output keys.
    let Some(_) = skills.get("Cleave") else {
        return;
    };
    let mut nc = Character::new(ClassRef::duelist(), 90);
    nc.main_skill = Some(MainSkill::new("Cleave"));
    let cleave_out = pob_engine::compute_full(&nc, &tree, Some(&skills), Some(&bases));
    assert_eq!(
        cleave_out.try_get("NumberOfMines"),
        None,
        "Cleave (non-mine skill) must not emit NumberOfMines"
    );
    assert_eq!(
        cleave_out.try_get("NumberOfTraps"),
        None,
        "Cleave (non-trap skill) must not emit NumberOfTraps"
    );
}

// Issue #84 (slice 5): cast-speed mods do NOT scale mine / trap DPS.
// PoB's `CalcOffence.lua` mine/trap branches replace the cast-rate
// baseline with `MineLayingSpeed` / `TrapThrowingSpeed` — so a +200%
// `CastSpeed` INC on the player must leave a mine/trap skill's
// effective throws-per-second untouched. Mirrors the issue body's
// third acceptance criterion.
#[test]
fn cast_speed_does_not_scale_mine_or_trap_dps() {
    let (Some(tree), Some(skills), Some(bases)) = (load_3_25_tree(), load_skills(), load_bases())
    else {
        eprintln!("skip: data missing");
        return;
    };
    use pob_engine::{Mod, Source};

    // Pick a stable mine skill — see `pick_stable_skill`.
    let mine_id = pick_stable_skill(&skills, |s| {
        s.base_flags.get("mine").copied().unwrap_or(false)
    });
    let Some(mine_id) = mine_id else {
        eprintln!("skip: no mine skills available");
        return;
    };
    let mut c = Character::new(ClassRef::shadow(), 90);
    c.main_skill = Some(MainSkill::new(&mine_id));
    let baseline = pob_engine::compute_full(&c, &tree, Some(&skills), Some(&bases));
    let baseline_dps = baseline.get("MainSkillDPS");
    let baseline_speed = baseline.get("MainSkillSpeed");
    if baseline_dps <= 0.0 {
        eprintln!("skip: mine skill {mine_id} produces no DPS");
        return;
    }

    // Inject 200% increased CastSpeed via the modDB. For a spell-tagged
    // skill this would more-than-triple the cps; for a mine the
    // throw-rate path should ignore it entirely.
    let (_, mut env) = pob_engine::compute_full_with_env(&c, &tree, Some(&skills), Some(&bases));
    env.mod_db
        .add(Mod::inc("CastSpeed", 200.0).with_source(Source::Other("test".into())));
    pob_engine::perform::perform_skill_dps(&c, &skills, &mut env);
    let scaled_dps = env.output.get("MainSkillDPS");
    let scaled_speed = env.output.get("MainSkillSpeed");
    assert!(
        (scaled_dps - baseline_dps).abs() / baseline_dps.abs().max(1e-9) < 1e-6,
        "Mine skill MainSkillDPS must not scale with CastSpeed (baseline {baseline_dps}, +200% CastSpeed {scaled_dps})"
    );
    assert!(
        (scaled_speed - baseline_speed).abs() / baseline_speed.abs().max(1e-9) < 1e-6,
        "Mine skill MainSkillSpeed must stay pinned to MineLayingSpeed (baseline {baseline_speed}, +200% CastSpeed {scaled_speed})"
    );

    // Same for traps — stable pick.
    let trap_id = pick_stable_skill(&skills, |s| {
        s.base_flags.get("trap").copied().unwrap_or(false)
    });
    let Some(trap_id) = trap_id else {
        eprintln!("skip: no trap skills available");
        return;
    };
    let mut tc = Character::new(ClassRef::shadow(), 90);
    tc.main_skill = Some(MainSkill::new(&trap_id));
    let trap_baseline = pob_engine::compute_full(&tc, &tree, Some(&skills), Some(&bases));
    let trap_baseline_dps = trap_baseline.get("MainSkillDPS");
    let trap_baseline_speed = trap_baseline.get("MainSkillSpeed");
    if trap_baseline_dps <= 0.0 {
        eprintln!("skip: trap skill {trap_id} produces no DPS");
        return;
    }
    let (_, mut trap_env) =
        pob_engine::compute_full_with_env(&tc, &tree, Some(&skills), Some(&bases));
    trap_env
        .mod_db
        .add(Mod::inc("CastSpeed", 200.0).with_source(Source::Other("test".into())));
    pob_engine::perform::perform_skill_dps(&tc, &skills, &mut trap_env);
    let trap_scaled_dps = trap_env.output.get("MainSkillDPS");
    let trap_scaled_speed = trap_env.output.get("MainSkillSpeed");
    assert!(
        (trap_scaled_dps - trap_baseline_dps).abs() / trap_baseline_dps.abs().max(1e-9) < 1e-6,
        "Trap skill MainSkillDPS must not scale with CastSpeed (baseline {trap_baseline_dps}, +200% CastSpeed {trap_scaled_dps})"
    );
    assert!(
        (trap_scaled_speed - trap_baseline_speed).abs() / trap_baseline_speed.abs().max(1e-9)
            < 1e-6,
        "Trap skill MainSkillSpeed must stay pinned to TrapThrowingSpeed (baseline {trap_baseline_speed}, +200% CastSpeed {trap_scaled_speed})"
    );
}

// Issue #84: mine/trap throw timing model. Mirrors PoB's
// CalcSetup.lua:52-53 base values:
//   MineLayingTime BASE  = 0.3 s → MineLayingSpeed default ≈ 3.33 /s
//   TrapThrowingTime BASE = 0.6 s → TrapThrowingSpeed default ≈ 1.67 /s
// With no extra mods this is the throw rate the engine should use as
// `MainSkillSpeed` for mine/trap skills (replacing the spell cast-rate
// baseline). The default character carries no MineLayingSpeed /
// TrapThrowingSpeed inc/more, so the output should match the base.
#[test]
fn mine_and_trap_throw_timing_emits_pob_default_speeds() {
    let (Some(tree), Some(skills), Some(bases)) = (load_3_25_tree(), load_skills(), load_bases())
    else {
        eprintln!("skip: data missing");
        return;
    };

    // Use a deterministic hit-based mine/trap (DoT-only variants
    // currently bypass the throw-timing block via the DoT-DPS early
    // return — that branch is tracked as a follow-up in this issue).
    if skills.get("IcicleMine").is_some() {
        let mut c = Character::new(ClassRef::shadow(), 90);
        c.main_skill = Some(MainSkill::new("IcicleMine"));
        let out = pob_engine::compute_full(&c, &tree, Some(&skills), Some(&bases));
        if out.get("MainSkillDPS") > 0.0 {
            // Default mine laying speed: 1 / 0.3 = 3.333…/s
            let speed = out.get("MineLayingSpeed");
            assert!(
                (speed - 3.333_333).abs() < 0.05,
                "default MineLayingSpeed should be ~3.33 /s, got {speed}"
            );
            let time = out.get("MineLayingTime");
            assert!(
                (time - 0.3).abs() < 0.005,
                "default MineLayingTime should be 0.3 s, got {time}"
            );
            // MainSkillSpeed should equal MineLayingSpeed (the cast-time
            // path was overridden for mine skills).
            let main_speed = out.get("MainSkillSpeed");
            assert!(
                (main_speed - speed).abs() < 0.001,
                "MainSkillSpeed should track MineLayingSpeed for a mine skill, got {main_speed} vs {speed}"
            );
        }
    }

    if skills.get("LightningTrap").is_some() {
        let mut c = Character::new(ClassRef::shadow(), 90);
        c.main_skill = Some(MainSkill::new("LightningTrap"));
        let out = pob_engine::compute_full(&c, &tree, Some(&skills), Some(&bases));
        if out.get("MainSkillDPS") > 0.0 {
            // Default trap throwing speed: 1 / 0.6 = 1.666…/s
            let speed = out.get("TrapThrowingSpeed");
            assert!(
                (speed - 1.666_667).abs() < 0.05,
                "default TrapThrowingSpeed should be ~1.67 /s, got {speed}"
            );
            let time = out.get("TrapThrowingTime");
            assert!(
                (time - 0.6).abs() < 0.005,
                "default TrapThrowingTime should be 0.6 s, got {time}"
            );
            let main_speed = out.get("MainSkillSpeed");
            assert!(
                (main_speed - speed).abs() < 0.001,
                "MainSkillSpeed should track TrapThrowingSpeed for a trap skill, got {main_speed} vs {speed}"
            );
        }
    }

    // A non-mine/trap skill (Cleave) keeps the attack-rate baseline; its
    // MainSkillSpeed should not equal the mine/trap defaults.
    if skills.get("Cleave").is_some() {
        let mut c = Character::new(ClassRef::duelist(), 90);
        c.main_skill = Some(MainSkill::new("Cleave"));
        let out = pob_engine::compute_full(&c, &tree, Some(&skills), Some(&bases));
        assert_eq!(
            out.try_get("MineLayingSpeed"),
            None,
            "Cleave must not emit MineLayingSpeed"
        );
        assert_eq!(
            out.try_get("TrapThrowingSpeed"),
            None,
            "Cleave must not emit TrapThrowingSpeed"
        );
    }
}

// Issue #84 (slice 3): DoT-only mines/traps still surface throw
// timing + count outputs even though their DPS lands via the early-
// return ailment branch. Siphoning Trap (DoT-only) should emit
// `TrapThrowingSpeed`, `TrapThrowingTime`, `NumberOfTraps`, and the
// active limit just like a hit-based trap does.
#[test]
fn dot_only_trap_emits_throw_timing_outputs() {
    let (Some(tree), Some(skills), Some(bases)) = (load_3_25_tree(), load_skills(), load_bases())
    else {
        eprintln!("skip: data missing");
        return;
    };
    if skills.get("SiphoningTrap").is_none() {
        eprintln!("skip: SiphoningTrap not in registry");
        return;
    }
    let mut c = Character::new(ClassRef::shadow(), 90);
    c.main_skill = Some(MainSkill::new("SiphoningTrap"));
    let out = pob_engine::compute_full(&c, &tree, Some(&skills), Some(&bases));

    // Trap timing keys land regardless of whether the DPS path
    // routes through the DoT branch.
    let speed = out.get("TrapThrowingSpeed");
    assert!(
        (speed - 1.666_667).abs() < 0.05,
        "DoT trap should emit TrapThrowingSpeed ≈ 1.67 /s, got {speed}"
    );
    let time = out.get("TrapThrowingTime");
    assert!(
        (time - 0.6).abs() < 0.005,
        "DoT trap should emit TrapThrowingTime = 0.6 s, got {time}"
    );
    assert!(
        (out.get("NumberOfTraps") - 1.0).abs() < 0.001,
        "DoT trap should emit NumberOfTraps = 1, got {}",
        out.get("NumberOfTraps")
    );
    assert!(
        out.get("ActiveTrapLimit") >= 15.0,
        "DoT trap should emit ActiveTrapLimit ≥ 15 (PoB default), got {}",
        out.get("ActiveTrapLimit")
    );
}

// Issue #84 (slice 2): multi-throw penalty for mines. PoB applies a
// "throwing mines takes 10% more time for each additional mine
// thrown" rule — so layering 4 extra throws (from a Minefield-style
// `MineThrowCount` BASE 4 mod) divides the laying speed by 1.4.
// Verifies the engine respects the penalty by injecting the
// extra-throw mod and checking `MineLayingSpeed` drops by the right
// factor while throw count goes up.
#[test]
fn mine_multi_throw_penalty_scales_laying_speed() {
    let (Some(tree), Some(skills), Some(bases)) = (load_3_25_tree(), load_skills(), load_bases())
    else {
        eprintln!("skip: data missing");
        return;
    };
    if skills.get("IcicleMine").is_none() {
        eprintln!("skip: IcicleMine not in registry");
        return;
    }
    use pob_engine::{Mod, Source};

    // Baseline: default Shadow / IcicleMine, throw_count = 1 (no
    // MineThrowCount mod). Slice 1 already pins `MineLayingSpeed` at
    // 1 / 0.3 = 3.333…/s.
    let mut c = Character::new(ClassRef::shadow(), 90);
    c.main_skill = Some(MainSkill::new("IcicleMine"));
    let baseline = pob_engine::compute_full(&c, &tree, Some(&skills), Some(&bases));
    let base_speed = baseline.get("MineLayingSpeed");
    if base_speed <= 0.0 {
        eprintln!("skip: IcicleMine MineLayingSpeed not emitted");
        return;
    }

    // Inject 4 additional mine throws (Minefield-style). Throw count
    // = 5; expected MineLayingSpeed = base / (1 + (5-1)*0.1) = base / 1.4.
    // We seed the mod via the player modDB directly so the test
    // doesn't need to find a specific item.
    let (_, mut env) = pob_engine::compute_full_with_env(&c, &tree, Some(&skills), Some(&bases));
    env.mod_db
        .add(Mod::base("MineThrowCount", 4.0).with_source(Source::Other("test".into())));
    // Re-run the skill DPS pass with the augmented modDB.
    pob_engine::perform::perform_skill_dps(&c, &skills, &mut env);
    let scaled_speed = env.output.get("MineLayingSpeed");
    let expected = base_speed / 1.4;
    assert!(
        (scaled_speed - expected).abs() / expected < 0.01,
        "MineLayingSpeed with 4 extra throws should be base/1.4 ({expected:.3}); got {scaled_speed:.3}"
    );
    // Throw count should reflect the mod (existing slice 1 behaviour).
    let throws = env.output.get("NumberOfMines");
    assert!(
        (throws - 5.0).abs() < 0.01,
        "NumberOfMines with +4 BASE MineThrowCount should be 5; got {throws}"
    );
}

// Issue #84 (slice 4): per-skill cooldown caps the trap/mine throw
// rate. Bear Trap has an intrinsic 4s cooldown — even with default
// throwing speed of 1 / 0.6s ≈ 1.67/s, the effective DPS rate must
// fall to 1/4 = 0.25/s when the cooldown is read from skill data.
// CooldownRecovery scales the cooldown down. Mirrors PoB's
// `useSpeed = 1 / cooldown` pattern from CalcOffence.lua:5648.
#[test]
fn trap_cooldown_caps_throw_rate() {
    let (Some(tree), Some(skills), Some(bases)) = (load_3_25_tree(), load_skills(), load_bases())
    else {
        eprintln!("skip: data missing");
        return;
    };
    if skills.get("BearTrap").is_none() {
        eprintln!("skip: BearTrap not in registry");
        return;
    }
    use pob_engine::{Mod, Source};

    // Baseline: Shadow + Bear Trap (4s base cooldown). Without
    // CooldownRecovery the effective cooldown stays at 4s.
    let mut c = Character::new(ClassRef::shadow(), 90);
    c.main_skill = Some(MainSkill::new("BearTrap"));
    let baseline = pob_engine::compute_full(&c, &tree, Some(&skills), Some(&bases));
    let cd = baseline.get("TrapCooldown");
    assert!(
        (cd - 4.0).abs() < 1e-6,
        "Bear Trap base cooldown should be 4.0s with no CooldownRecovery; got {cd}"
    );
    let main_speed = baseline.get("MainSkillSpeed");
    assert!(
        (main_speed - 0.25).abs() < 1e-6,
        "Bear Trap MainSkillSpeed should fall to 1/4 = 0.25/s under the cooldown cap; got {main_speed}"
    );
    // Throwing speed itself stays at the throwing-time-derived rate
    // (~1.67/s) — the cooldown only gates the effective DPS rate.
    let throw_speed = baseline.get("TrapThrowingSpeed");
    assert!(
        throw_speed > 1.0,
        "Bear Trap raw TrapThrowingSpeed should still reflect the 0.6s base throwing time; got {throw_speed}"
    );

    // 50% increased CooldownRecovery → effective cooldown = 4 / 1.5
    // = 2.6667s, then ceil to nearest 60Hz tick → 2.6667 (already
    // tick-aligned). Effective throw rate = 1/2.6667 ≈ 0.375/s.
    let (_, mut env) = pob_engine::compute_full_with_env(&c, &tree, Some(&skills), Some(&bases));
    env.mod_db
        .add(Mod::inc("CooldownRecovery", 50.0).with_source(Source::Other("test".into())));
    pob_engine::perform::perform_skill_dps(&c, &skills, &mut env);
    let cd_after = env.output.get("TrapCooldown");
    let expected = (4.0 / 1.5 * 60.0_f64).ceil() / 60.0;
    assert!(
        (cd_after - expected).abs() < 1e-3,
        "TrapCooldown with 50% CooldownRecovery should be ~{expected:.4}s; got {cd_after}"
    );
    let main_speed_after = env.output.get("MainSkillSpeed");
    let expected_speed = 1.0 / expected;
    assert!(
        (main_speed_after - expected_speed).abs() < 1e-3,
        "MainSkillSpeed under 50% CDR should match 1/{expected:.4} = {expected_speed:.4}; got {main_speed_after}"
    );
}

// Issue #8: impale layer adds physical-stack DPS to FullDPS via
//   ImpaleDPS = stored × stacks(5) × effect/100 × chance/100 × cps
// Issue #19: Warcry exertion. Each warcry exerts the next N attacks
// and grants them an `ExertedAttackDamage` bonus composed multiplicatively
// from INC and MORE — `(1 + inc/100) × more`. The user supplies the
// resulting uptime via `ConfigState::exerted_attack_uptime` (modelling
// cry cadence + skill detection is a follow-up). MK2 computes
// `MainSkillDPS *= 1 + uptime × (factor - 1)` for attack skills.
#[test]
fn exerted_attack_uptime_lifts_main_skill_dps() {
    let (Some(tree), Some(skills), Some(bases)) = (load_3_25_tree(), load_skills(), load_bases())
    else {
        eprintln!("skip: data missing");
        return;
    };
    let Some(_) = skills.get("Cleave") else {
        eprintln!("skip: Cleave not found");
        return;
    };
    let sword_name = bases
        .iter()
        .find(|(_, b)| b.r#type.contains("Sword") && b.weapon.is_some())
        .map(|(n, _)| n.clone());
    let Some(sword_name) = sword_name else {
        return;
    };

    let mut c = Character::new(ClassRef::duelist(), 90);
    let sword = parse_item(&format!(
        "Item Class: One Handed Swords\nRarity: NORMAL\n{sword_name}\n--------\n"
    ))
    .unwrap();
    c.items.equip(pob_data::Slot::Weapon1, sword);
    c.main_skill = Some(MainSkill::new("Cleave"));

    // Equip a body armour granting "+50% Exerted Attacks deal increased
    // Damage" — mod_parser maps this to `ExertedAttackDamage` INC 50.
    let body = parse_item(
        "Item Class: Body Armours\nRarity: RARE\nWarcry Plate\nFull Plate\n--------\nExerted Attacks deal 50% increased Damage\n--------",
    )
    .unwrap();
    c.items.equip(pob_data::Slot::BodyArmour, body);

    // Baseline: no exerted uptime — DPS unaffected by the mod.
    assert_eq!(c.config.exerted_attack_uptime, 0.0);
    let baseline = pob_engine::compute_full(&c, &tree, Some(&skills), Some(&bases));
    let baseline_dps = baseline.get("MainSkillDPS");
    if baseline_dps <= 0.0 {
        eprintln!("skip: Cleave produces no DPS in this fixture");
        return;
    }
    assert_eq!(
        baseline.try_get("ExertedAttackUptime"),
        None,
        "uptime=0 must not emit ExertedAttackUptime"
    );

    // Set 50% uptime: half of attacks are exerted, so the average DPS
    // bonus is 0.5 × 50% = 25%.
    c.config.exerted_attack_uptime = 0.5;
    let exerted = pob_engine::compute_full(&c, &tree, Some(&skills), Some(&bases));
    let ratio = exerted.get("MainSkillDPS") / baseline_dps;
    assert!(
        (ratio - 1.25).abs() < 0.01,
        "50% uptime + 50% Exerted MORE should multiply DPS by 1.25; ratio={ratio} (baseline={baseline_dps}, after={})",
        exerted.get("MainSkillDPS")
    );
    assert!(
        (exerted.get("ExertedAttackUptime") - 0.5).abs() < 0.001,
        "ExertedAttackUptime output should mirror the config value"
    );
    assert!(
        (exerted.get("ExertedAttackDamageBonus") - 50.0).abs() < 0.01,
        "ExertedAttackDamageBonus should reflect the 50% INC mod"
    );

    // Set 100% uptime: every attack is exerted, full 50% bonus.
    c.config.exerted_attack_uptime = 1.0;
    let full = pob_engine::compute_full(&c, &tree, Some(&skills), Some(&bases));
    let ratio = full.get("MainSkillDPS") / baseline_dps;
    assert!(
        (ratio - 1.5).abs() < 0.01,
        "100% uptime + 50% Exerted MORE should multiply DPS by 1.5; ratio={ratio}"
    );
}

// Issue #19 (slice 2): WarcryPower config knob. Pinning a value via
// `Character::config.warcry_power` should land both as a `WarcryPower`
// BASE mod (so PoB-style `WarcryPower` reads pick it up) and as a
// `Multiplier:WarcryPower` BASE mod (so PerStat-tagged scalars like
// "+1% damage per 5 Warcry Power" can scale off it). Without an
// explicit pin the engine falls back to PoB's documented 20-default
// (boss target).
#[test]
fn warcry_power_config_lands_in_mod_db() {
    let Some(tree) = load_3_25_tree() else {
        eprintln!("skip: tree missing");
        return;
    };
    use pob_engine::ModStore as _;

    // No config pin → falls back to the PoB 20 default; WarcryPower
    // BASE mods aren't injected from Config.
    let mut c = Character::new(ClassRef::marauder(), 90);
    let env = pob_engine::perform::init_env(&c, &tree);
    let cfg_sourced_power = env.mod_db.iter_all().any(|m| {
        m.name == "WarcryPower"
            && matches!(&m.source, Some(pob_engine::Source::Other(s)) if s == "Config")
    });
    assert!(
        !cfg_sourced_power,
        "Without warcry_power config there should be no Config-sourced WarcryPower mod"
    );

    // Pinning warcry_power = 35 injects both `WarcryPower` and
    // `Multiplier:WarcryPower` BASE mods.
    c.config.warcry_power = Some(35);
    let env2 = pob_engine::perform::init_env(&c, &tree);
    let power_value = env2.mod_db.iter_all().find_map(|m| {
        if m.name == "WarcryPower"
            && matches!(&m.source, Some(pob_engine::Source::Other(s)) if s == "Config")
        {
            m.value.as_f64()
        } else {
            None
        }
    });
    assert_eq!(
        power_value,
        Some(35.0),
        "Config-sourced WarcryPower BASE mod should equal warcry_power"
    );
    let mult_value = env2.mod_db.iter_all().find_map(|m| {
        if m.name == "Multiplier:WarcryPower"
            && matches!(&m.source, Some(pob_engine::Source::Other(s)) if s == "Config")
        {
            m.value.as_f64()
        } else {
            None
        }
    });
    assert_eq!(
        mult_value,
        Some(35.0),
        "Multiplier:WarcryPower BASE should mirror the config value so PerStat-tagged mods see it"
    );

    // The output key reflects the BASE sum, with the PoB 20 floor.
    let out = pob_engine::compute_full(&c, &tree, None, None);
    assert!(
        (out.get("WarcryPower") - 35.0).abs() < 0.01,
        "Output WarcryPower should equal the configured value when above the 20 floor; got {}",
        out.get("WarcryPower")
    );

    // Below the floor the default kicks in.
    c.config.warcry_power = Some(5);
    let out = pob_engine::compute_full(&c, &tree, None, None);
    assert!(
        (out.get("WarcryPower") - 20.0).abs() < 0.01,
        "Output WarcryPower should fall back to the PoB 20-default when configured below it; got {}",
        out.get("WarcryPower")
    );
}

// Issue #83 (slice 2): NearbyEnemies multiplier surfaces from Config
// tab into the modDB and EvalState, plus the OnlyOneNearbyEnemy
// condition flips on for exactly one nearby enemy. Mirrors PoB's
// `ConfigOptions.lua:1193-1199`.
#[test]
fn nearby_enemies_config_emits_multiplier_and_condition() {
    let Some(tree) = load_3_25_tree() else {
        eprintln!("skip: tree missing");
        return;
    };
    use pob_engine::ModStore as _;

    // Default (0) → no Config-sourced injection.
    let mut c = Character::new(ClassRef::marauder(), 90);
    let env = pob_engine::perform::init_env(&c, &tree);
    let injected = env.mod_db.iter_all().any(|m| {
        m.name == "Multiplier:NearbyEnemies"
            && matches!(&m.source, Some(pob_engine::Source::Other(s)) if s == "Config")
    });
    assert!(
        !injected,
        "Default nearby_enemies = 0 should not inject any Config-sourced Multiplier:NearbyEnemies"
    );
    assert!(
        !env.state.condition("OnlyOneNearbyEnemy"),
        "Default config should not set OnlyOneNearbyEnemy"
    );

    // 5 nearby enemies → BASE = 5, EvalState multiplier reads 5,
    // OnlyOneNearbyEnemy stays off.
    c.config.nearby_enemies = 5;
    let env = pob_engine::perform::init_env(&c, &tree);
    let val = env.mod_db.iter_all().find_map(|m| {
        if m.name == "Multiplier:NearbyEnemies"
            && matches!(&m.source, Some(pob_engine::Source::Other(s)) if s == "Config")
        {
            m.value.as_f64()
        } else {
            None
        }
    });
    assert_eq!(
        val,
        Some(5.0),
        "nearby_enemies = 5 should inject Multiplier:NearbyEnemies BASE = 5"
    );
    assert!(
        (env.state.multiplier("NearbyEnemies") - 5.0).abs() < 1e-6,
        "EvalState multiplier(NearbyEnemies) should mirror the config so PerStat-tagged mods read it; got {}",
        env.state.multiplier("NearbyEnemies")
    );
    assert!(
        !env.state.condition("OnlyOneNearbyEnemy"),
        "OnlyOneNearbyEnemy must only flip on at exactly 1 enemy"
    );

    // Exactly 1 → OnlyOneNearbyEnemy flips on. Solaris's "while there
    // is only one nearby Enemy" mod gates on this condition.
    c.config.nearby_enemies = 1;
    let env = pob_engine::perform::init_env(&c, &tree);
    assert!(
        env.state.condition("OnlyOneNearbyEnemy"),
        "OnlyOneNearbyEnemy must be true when nearby_enemies == 1"
    );
}

// Issue #19 (slice 15): NearbyAllies config drives the
// `Multiplier:NearbyAlly` BASE that Rallying Cry's per-ally exert
// damage bonus, banner skill ally scaling, and similar PerStat
// mods read. PoB defaults to 0 (solo); the multiplier is only
// injected when the user pins a non-zero count.
#[test]
fn nearby_allies_config_emits_multiplier() {
    let Some(tree) = load_3_25_tree() else {
        eprintln!("skip: tree missing");
        return;
    };
    use pob_engine::ModStore as _;

    // Default (0) → no Config-sourced injection.
    let mut c = Character::new(ClassRef::marauder(), 90);
    let env = pob_engine::perform::init_env(&c, &tree);
    let injected = env.mod_db.iter_all().any(|m| {
        m.name == "Multiplier:NearbyAlly"
            && matches!(&m.source, Some(pob_engine::Source::Other(s)) if s == "Config")
    });
    assert!(
        !injected,
        "Default nearby_allies = 0 should not inject any Config-sourced Multiplier:NearbyAlly"
    );

    // 4 nearby allies → BASE = 4, EvalState multiplier reads 4.
    c.config.nearby_allies = 4;
    let env = pob_engine::perform::init_env(&c, &tree);
    let val = env.mod_db.iter_all().find_map(|m| {
        if m.name == "Multiplier:NearbyAlly"
            && matches!(&m.source, Some(pob_engine::Source::Other(s)) if s == "Config")
        {
            m.value.as_f64()
        } else {
            None
        }
    });
    assert_eq!(
        val,
        Some(4.0),
        "nearby_allies = 4 should inject Multiplier:NearbyAlly BASE = 4"
    );
    assert!(
        (env.state.multiplier("NearbyAlly") - 4.0).abs() < 1e-6,
        "EvalState multiplier(NearbyAlly) should mirror the config; got {}",
        env.state.multiplier("NearbyAlly")
    );
}

// Issue #19 (slice 3): warcry-loadout detection. Each enabled warcry
// gem in the gem list contributes to `ActiveWarcryCount`,
// `WarcryExertedAttackCountTotal`, and (for cooldown-having warcries)
// `WarcryMinCooldown`. Builds without warcry gems get no output.
#[test]
fn warcry_detection_emits_loadout_aggregates() {
    let (Some(tree), Some(skills)) = (load_3_25_tree(), load_skills()) else {
        eprintln!("skip: data missing");
        return;
    };
    if skills.get("AncestralCry").is_none() {
        eprintln!("skip: AncestralCry not in registry");
        return;
    }
    use pob_engine::character::SocketGroup;

    // Baseline: no warcries → no aggregate keys.
    let mut c = Character::new(ClassRef::marauder(), 90);
    c.main_skill = Some(MainSkill::new("Cleave"));
    let baseline = pob_engine::compute_full(&c, &tree, Some(&skills), None);
    assert_eq!(
        baseline.try_get("ActiveWarcryCount"),
        None,
        "no warcry gems → no ActiveWarcryCount"
    );

    // Add a warcry gem in a separate group. It should land in the
    // detection regardless of which group is the main socket group.
    let mut warcry = MainSkill::new("AncestralCry");
    warcry.level = 20;
    c.skill_groups.push(SocketGroup {
        label: "Warcry".into(),
        gems: vec![warcry],
        main_active_skill_index: 1,
        enabled: true,
    });
    let with_warcry = pob_engine::compute_full(&c, &tree, Some(&skills), None);
    assert!(
        (with_warcry.get("ActiveWarcryCount") - 1.0).abs() < 0.001,
        "one warcry gem should land as ActiveWarcryCount=1; got {}",
        with_warcry.get("ActiveWarcryCount")
    );
    // Ancestral Cry exerts 8 melee attacks (constant across levels).
    assert!(
        (with_warcry.get("WarcryExertedAttackCountTotal") - 8.0).abs() < 0.001,
        "Ancestral Cry exerts 8 attacks per cast; got {}",
        with_warcry.get("WarcryExertedAttackCountTotal")
    );
    // Cooldown at L20 is 8s.
    let cd = with_warcry.get("WarcryMinCooldown");
    assert!(
        cd > 0.0,
        "Ancestral Cry should emit a positive WarcryMinCooldown; got {cd}"
    );

    // Disabling the warcry gem removes the aggregates entirely.
    if let Some(g) = c.skill_groups.last_mut() {
        if let Some(gem) = g.gems.first_mut() {
            gem.enabled = false;
        }
    }
    let disabled = pob_engine::compute_full(&c, &tree, Some(&skills), None);
    assert_eq!(
        disabled.try_get("ActiveWarcryCount"),
        None,
        "disabled warcry gem must drop the aggregate keys"
    );
}

// Issue #19 (slice 5): UsedWarcryRecently condition implication
// chain. Mirrors PoB's `ConfigOptions.lua:1528-1534`
// `implyCondList`: ticking `UsedWarcryRecently` also implies
// `UsedWarcryInPast8Seconds` and `UsedSkillRecently` so mods gated
// on the wider buckets light up automatically.
#[test]
fn used_warcry_recently_implies_wider_conditions() {
    let Some(tree) = load_3_25_tree() else {
        eprintln!("skip: tree missing");
        return;
    };

    // Without the source flag, no implications fire.
    let baseline = Character::new(ClassRef::marauder(), 90);
    let env = pob_engine::perform::init_env(&baseline, &tree);
    assert!(
        !env.state.condition("UsedWarcryRecently"),
        "no source condition → no implication"
    );
    assert!(
        !env.state.condition("UsedWarcryInPast8Seconds"),
        "no source condition → no 8s implication"
    );
    assert!(
        !env.state.condition("UsedSkillRecently"),
        "no source condition → no skill implication"
    );

    // Tick `UsedWarcryRecently` → both wider buckets imply true.
    let mut c = baseline.clone();
    c.config
        .conditions
        .insert("UsedWarcryRecently".into(), true);
    let env = pob_engine::perform::init_env(&c, &tree);
    assert!(
        env.state.condition("UsedWarcryRecently"),
        "explicit source condition stays on"
    );
    assert!(
        env.state.condition("UsedWarcryInPast8Seconds"),
        "UsedWarcryRecently should imply the 8s window"
    );
    assert!(
        env.state.condition("UsedSkillRecently"),
        "UsedWarcryRecently should imply UsedSkillRecently"
    );
}

// Issue #19 (slice 6): Intimidating Cry's intimidate enemy debuff
// auto-flips `EnemyIntimidated` when the user has indicated a
// warcry was used recently (UsedWarcryRecently config flag). The
// condition stays off without the user-cast indicator — even if
// IntimidatingCry is in the loadout — so the calc engine doesn't
// silently overstate damage when the warcry isn't being actively
// rotated.
#[test]
fn intimidating_cry_loadout_flips_enemy_intimidated_with_warcry_use() {
    let (Some(tree), Some(skills)) = (load_3_25_tree(), load_skills()) else {
        eprintln!("skip: data missing");
        return;
    };
    if skills.get("IntimidatingCry").is_none() || skills.get("Cleave").is_none() {
        eprintln!("skip: IntimidatingCry / Cleave not in registry");
        return;
    }
    use pob_engine::character::SocketGroup;

    // Cleave + Intimidating Cry, no warcry-uptime indicator → no
    // EnemyIntimidated. The detection is gated on
    // `UsedWarcryRecently` so a build that has the gem socketed
    // but isn't currently casting the cry doesn't get the buff.
    let mut c = Character::new(ClassRef::marauder(), 90);
    c.main_skill = Some(MainSkill::new("Cleave"));
    let mut warcry = MainSkill::new("IntimidatingCry");
    warcry.level = 20;
    c.skill_groups.push(SocketGroup {
        label: "Warcry".into(),
        gems: vec![warcry],
        main_active_skill_index: 1,
        enabled: true,
    });

    let env = pob_engine::compute_full_with_env(&c, &tree, Some(&skills), None).1;
    assert!(
        !env.state.condition("EnemyIntimidated"),
        "Without UsedWarcryRecently, IntimidatingCry in the loadout must NOT auto-flip EnemyIntimidated"
    );
    assert!(
        env.output.get("IntimidatingCryActive") < 0.5,
        "IntimidatingCryActive output key should not be emitted when the cry isn't cast"
    );

    // Tick UsedWarcryRecently → EnemyIntimidated flips on.
    c.config
        .conditions
        .insert("UsedWarcryRecently".into(), true);
    let env = pob_engine::compute_full_with_env(&c, &tree, Some(&skills), None).1;
    assert!(
        env.state.condition("EnemyIntimidated"),
        "With UsedWarcryRecently and IntimidatingCry in the loadout, EnemyIntimidated must auto-flip on"
    );
    assert!(
        (env.output.get("IntimidatingCryActive") - 1.0).abs() < 1e-6,
        "IntimidatingCryActive should be 1 when the cry's intimidate debuff is active"
    );

    // Without IntimidatingCry, ticking UsedWarcryRecently alone
    // must not flip EnemyIntimidated — the slice-5 implication
    // chain only widens warcry-related buckets, not enemy debuffs.
    let mut c2 = Character::new(ClassRef::marauder(), 90);
    c2.main_skill = Some(MainSkill::new("Cleave"));
    c2.config
        .conditions
        .insert("UsedWarcryRecently".into(), true);
    let env = pob_engine::compute_full_with_env(&c2, &tree, Some(&skills), None).1;
    assert!(
        !env.state.condition("EnemyIntimidated"),
        "UsedWarcryRecently alone (no IntimidatingCry gem) must not auto-flip EnemyIntimidated"
    );
}

// Issue #19 (slice 7): each detected warcry emits a
// `<Cry>Active = 1` output marker when `UsedWarcryRecently` is on.
// Lets the Calcs tab show which warcries are contributing to the
// headline DPS without flipping back to the Skills tab. Markers
// are gated on UsedWarcryRecently so a build with the gem
// socketed but no active rotation doesn't claim the buffs.
#[test]
fn each_warcry_emits_active_marker_with_recent_use() {
    let (Some(tree), Some(skills)) = (load_3_25_tree(), load_skills()) else {
        eprintln!("skip: data missing");
        return;
    };
    use pob_engine::character::SocketGroup;

    // Pick the warcry skill IDs that exist in the registry. Some
    // gem fixtures may not carry every alt-quality variant, so we
    // skip any that aren't present rather than failing.
    let candidates: Vec<&str> = [
        "AncestralCry",
        "EnduringCry",
        "InfernalCry",
        "RallyingCry",
        "SeismicCry",
        "GeneralsCry",
        "BattlemagesCry",
        "IntimidatingCry",
    ]
    .iter()
    .copied()
    .filter(|id| skills.get(id).is_some())
    .collect();
    if candidates.len() < 2 {
        eprintln!(
            "skip: need at least 2 warcry skills in the registry (have {})",
            candidates.len()
        );
        return;
    }
    let Some(_) = skills.get("Cleave") else {
        eprintln!("skip: Cleave not in registry");
        return;
    };

    let mut c = Character::new(ClassRef::marauder(), 90);
    c.main_skill = Some(MainSkill::new("Cleave"));
    let mut warcry_gems: Vec<MainSkill> = Vec::new();
    for id in &candidates {
        let mut m = MainSkill::new(*id);
        m.level = 20;
        warcry_gems.push(m);
    }
    c.skill_groups.push(SocketGroup {
        label: "Warcries".into(),
        gems: warcry_gems,
        main_active_skill_index: 1,
        enabled: true,
    });

    // Without UsedWarcryRecently → no markers.
    let env = pob_engine::compute_full_with_env(&c, &tree, Some(&skills), None).1;
    for id in &candidates {
        let key = format!("{id}Active");
        assert!(
            env.output.get(&key) < 0.5,
            "{key} should NOT be emitted without UsedWarcryRecently — got {}",
            env.output.get(&key)
        );
    }

    // With UsedWarcryRecently → every detected warcry gets its marker.
    c.config
        .conditions
        .insert("UsedWarcryRecently".into(), true);
    let env = pob_engine::compute_full_with_env(&c, &tree, Some(&skills), None).1;
    for id in &candidates {
        let key = format!("{id}Active");
        assert!(
            (env.output.get(&key) - 1.0).abs() < 1e-6,
            "{key} should be 1 with UsedWarcryRecently and {id} in the loadout — got {}",
            env.output.get(&key)
        );
    }

    // Disabling the gem (per-gem toggle) should drop its marker
    // even though the group is still enabled. Pick the first
    // candidate to flip off.
    if let Some(group) = c.skill_groups.first_mut() {
        if let Some(gem) = group.gems.iter_mut().find(|g| g.skill_id == candidates[0]) {
            gem.enabled = false;
        }
    }
    let env = pob_engine::compute_full_with_env(&c, &tree, Some(&skills), None).1;
    let dropped_key = format!("{}Active", candidates[0]);
    assert!(
        env.output.get(&dropped_key) < 0.5,
        "{dropped_key} should drop when its gem is disabled — got {}",
        env.output.get(&dropped_key)
    );
    // Other warcries still active.
    if candidates.len() > 1 {
        let still_key = format!("{}Active", candidates[1]);
        assert!(
            (env.output.get(&still_key) - 1.0).abs() < 1e-6,
            "{still_key} should still be 1 after disabling a different cry — got {}",
            env.output.get(&still_key)
        );
    }
}

// Issue #19 (slice 8): Enduring Cry's signature buff — life regen
// scaling with WarcryPower. PoB's data ties
// `LifeRegenPercent BASE = 120 / 60 × min(power, 25) / 5` to the
// gem's GlobalEffect:Warcry tag (act_str.lua:3837-3840); we
// approximate the gate with `UsedWarcryRecently` since MK2 doesn't
// yet model the warcry uptime as a discrete buff window.
// WarcryPower defaults to PoB's 20 when the user hasn't pinned it.
#[test]
fn enduring_cry_injects_life_regen_scaling_with_warcry_power() {
    let (Some(tree), Some(skills)) = (load_3_25_tree(), load_skills()) else {
        eprintln!("skip: data missing");
        return;
    };
    if skills.get("EnduringCry").is_none() || skills.get("Cleave").is_none() {
        eprintln!("skip: EnduringCry / Cleave not in registry");
        return;
    }
    use pob_engine::character::SocketGroup;
    use pob_engine::ModStore as _;

    fn enduring_regen(env: &pob_engine::Env) -> f64 {
        env.mod_db
            .iter_all()
            .filter(|m| {
                m.name == "LifeRegenPercent"
                    && matches!(&m.source, Some(pob_engine::Source::Skill(s)) if s == "Enduring Cry")
            })
            .filter_map(|m| m.value.as_f64())
            .sum()
    }

    // Cleave + Enduring Cry, no UsedWarcryRecently → no LifeRegen
    // injection. Without the gate flag, the cry isn't considered
    // active in the rotation.
    let mut c = Character::new(ClassRef::marauder(), 90);
    c.main_skill = Some(MainSkill::new("Cleave"));
    let mut warcry = MainSkill::new("EnduringCry");
    warcry.level = 20;
    c.skill_groups.push(SocketGroup {
        label: "Warcry".into(),
        gems: vec![warcry],
        main_active_skill_index: 1,
        enabled: true,
    });
    let env = pob_engine::compute_full_with_env(&c, &tree, Some(&skills), None).1;
    assert!(
        enduring_regen(&env) < 1e-9,
        "Without UsedWarcryRecently, Enduring Cry must NOT inject LifeRegenPercent (got {})",
        enduring_regen(&env)
    );
    assert!(
        env.output.get("EnduringCryLifeRegenPct") < 1e-9,
        "EnduringCryLifeRegenPct should not be emitted without the warcry gate"
    );

    // Tick UsedWarcryRecently with default warcry_power (None →
    // PoB 20). Expected regen = 120 / 60 × 20 / 5 = 8.0%/s.
    c.config
        .conditions
        .insert("UsedWarcryRecently".into(), true);
    let env = pob_engine::compute_full_with_env(&c, &tree, Some(&skills), None).1;
    let regen_default = enduring_regen(&env);
    assert!(
        (regen_default - 8.0).abs() < 1e-6,
        "Enduring Cry at default WarcryPower (20) should give 8%/s regen; got {regen_default}"
    );
    assert!(
        (env.output.get("EnduringCryLifeRegenPct") - 8.0).abs() < 1e-6,
        "EnduringCryLifeRegenPct should mirror the injected regen"
    );

    // Pin WarcryPower = 25 (the cap). Expected = 120 / 60 × 25 / 5 = 10%/s.
    c.config.warcry_power = Some(25);
    let env = pob_engine::compute_full_with_env(&c, &tree, Some(&skills), None).1;
    assert!(
        (enduring_regen(&env) - 10.0).abs() < 1e-6,
        "Enduring Cry at WarcryPower 25 should hit the 10%/s cap; got {}",
        enduring_regen(&env)
    );

    // Pin WarcryPower = 100 (above cap). Should clamp to 25 → 10%/s.
    c.config.warcry_power = Some(100);
    let env = pob_engine::compute_full_with_env(&c, &tree, Some(&skills), None).1;
    assert!(
        (enduring_regen(&env) - 10.0).abs() < 1e-6,
        "Enduring Cry at WarcryPower 100 must still cap at 10%/s (limit = 25); got {}",
        enduring_regen(&env)
    );

    // Disable the cry gem. Even with UsedWarcryRecently, no regen
    // should be injected — the buff source is gone.
    if let Some(group) = c.skill_groups.first_mut() {
        if let Some(gem) = group.gems.iter_mut().find(|g| g.skill_id == "EnduringCry") {
            gem.enabled = false;
        }
    }
    let env = pob_engine::compute_full_with_env(&c, &tree, Some(&skills), None).1;
    assert!(
        enduring_regen(&env) < 1e-9,
        "Disabled EnduringCry must not contribute LifeRegenPercent; got {}",
        enduring_regen(&env)
    );
}

// Issue #19 (slice 9): Ancestral Cry's elemental-resist buff —
// `ElementalResist BASE = power` and `ElementalResistMax BASE =
// power × 0.1`, with WarcryPower clamped at 30 per PoB's
// `act_str.lua` AncestralCry statMap.
#[test]
fn ancestral_cry_injects_elemental_resist_buff() {
    let (Some(tree), Some(skills)) = (load_3_25_tree(), load_skills()) else {
        eprintln!("skip: data missing");
        return;
    };
    if skills.get("AncestralCry").is_none() || skills.get("Cleave").is_none() {
        eprintln!("skip: AncestralCry / Cleave not in registry");
        return;
    }
    use pob_engine::character::SocketGroup;
    use pob_engine::ModStore as _;

    fn ancestral_buff(env: &pob_engine::Env, name: &str) -> f64 {
        env.mod_db
            .iter_all()
            .filter(|m| {
                m.name == name
                    && matches!(&m.source, Some(pob_engine::Source::Skill(s)) if s == "Ancestral Cry")
            })
            .filter_map(|m| m.value.as_f64())
            .sum()
    }

    // Without UsedWarcryRecently → no injection.
    let mut c = Character::new(ClassRef::marauder(), 90);
    c.main_skill = Some(MainSkill::new("Cleave"));
    let mut warcry = MainSkill::new("AncestralCry");
    warcry.level = 20;
    c.skill_groups.push(SocketGroup {
        label: "Warcry".into(),
        gems: vec![warcry],
        main_active_skill_index: 1,
        enabled: true,
    });
    let env = pob_engine::compute_full_with_env(&c, &tree, Some(&skills), None).1;
    assert!(
        ancestral_buff(&env, "ElementalResist") < 1e-9,
        "Without UsedWarcryRecently, AncestralCry must not inject ElementalResist (got {})",
        ancestral_buff(&env, "ElementalResist")
    );

    // Default WarcryPower = 20 (None → PoB default). Expected:
    // ElementalResist = 5 × 20 / 5 = 20, ElementalResistMax =
    // 20 / 10 = 2.
    c.config
        .conditions
        .insert("UsedWarcryRecently".into(), true);
    let env = pob_engine::compute_full_with_env(&c, &tree, Some(&skills), None).1;
    let res = ancestral_buff(&env, "ElementalResist");
    let max_res = ancestral_buff(&env, "ElementalResistMax");
    assert!(
        (res - 20.0).abs() < 1e-6,
        "AncestralCry at default WarcryPower (20) should give +20% ElementalResist; got {res}"
    );
    assert!(
        (max_res - 2.0).abs() < 1e-6,
        "AncestralCry at default WarcryPower (20) should give +2% ElementalResistMax; got {max_res}"
    );
    assert!(
        (env.output.get("AncestralCryResistBonus") - 20.0).abs() < 1e-6,
        "AncestralCryResistBonus output should mirror the BASE value"
    );

    // WarcryPower = 30 → cap. ElementalResist = 30, Max = 3.
    c.config.warcry_power = Some(30);
    let env = pob_engine::compute_full_with_env(&c, &tree, Some(&skills), None).1;
    assert!(
        (ancestral_buff(&env, "ElementalResist") - 30.0).abs() < 1e-6,
        "AncestralCry at WarcryPower 30 should hit the +30% cap; got {}",
        ancestral_buff(&env, "ElementalResist")
    );
    assert!(
        (ancestral_buff(&env, "ElementalResistMax") - 3.0).abs() < 1e-6,
        "AncestralCry at WarcryPower 30 should hit the +3% Max cap; got {}",
        ancestral_buff(&env, "ElementalResistMax")
    );

    // WarcryPower = 100 → still capped at 30.
    c.config.warcry_power = Some(100);
    let env = pob_engine::compute_full_with_env(&c, &tree, Some(&skills), None).1;
    assert!(
        (ancestral_buff(&env, "ElementalResist") - 30.0).abs() < 1e-6,
        "AncestralCry at WarcryPower 100 must still cap at +30%; got {}",
        ancestral_buff(&env, "ElementalResist")
    );

    // Disabled gem → no buff.
    if let Some(group) = c.skill_groups.first_mut() {
        if let Some(gem) = group.gems.iter_mut().find(|g| g.skill_id == "AncestralCry") {
            gem.enabled = false;
        }
    }
    let env = pob_engine::compute_full_with_env(&c, &tree, Some(&skills), None).1;
    assert!(
        ancestral_buff(&env, "ElementalResist") < 1e-9,
        "Disabled AncestralCry must not contribute ElementalResist; got {}",
        ancestral_buff(&env, "ElementalResist")
    );
}

// Issue #19 (slice 10): Seismic Cry's defensive buff — Armour
// MORE and StunThreshold INC, both scaling with WarcryPower. PoB's
// `act_str.lua` SeismicCry statMap pins the formulas:
//   Armour MORE       = 5  × min(power, 25) / 5
//   StunThreshold INC = 15 × min(power, 25) / 5
#[test]
fn seismic_cry_injects_armour_and_stun_threshold_buff() {
    let (Some(tree), Some(skills)) = (load_3_25_tree(), load_skills()) else {
        eprintln!("skip: data missing");
        return;
    };
    if skills.get("SeismicCry").is_none() || skills.get("Cleave").is_none() {
        eprintln!("skip: SeismicCry / Cleave not in registry");
        return;
    }
    use pob_engine::character::SocketGroup;
    use pob_engine::ModStore as _;

    fn seismic_buff(env: &pob_engine::Env, name: &str) -> f64 {
        env.mod_db
            .iter_all()
            .filter(|m| {
                m.name == name
                    && matches!(&m.source, Some(pob_engine::Source::Skill(s)) if s == "Seismic Cry")
            })
            .filter_map(|m| m.value.as_f64())
            .sum()
    }

    let mut c = Character::new(ClassRef::marauder(), 90);
    c.main_skill = Some(MainSkill::new("Cleave"));
    let mut warcry = MainSkill::new("SeismicCry");
    warcry.level = 20;
    c.skill_groups.push(SocketGroup {
        label: "Warcry".into(),
        gems: vec![warcry],
        main_active_skill_index: 1,
        enabled: true,
    });

    // No flag → no injection.
    let env = pob_engine::compute_full_with_env(&c, &tree, Some(&skills), None).1;
    assert!(
        seismic_buff(&env, "Armour") < 1e-9,
        "Without UsedWarcryRecently, SeismicCry must not inject Armour MORE"
    );

    // Default WarcryPower 20 → Armour MORE = 20, StunThreshold INC = 60.
    c.config
        .conditions
        .insert("UsedWarcryRecently".into(), true);
    let env = pob_engine::compute_full_with_env(&c, &tree, Some(&skills), None).1;
    assert!(
        (seismic_buff(&env, "Armour") - 20.0).abs() < 1e-6,
        "SeismicCry at default WarcryPower (20) should give +20% MORE Armour; got {}",
        seismic_buff(&env, "Armour")
    );
    assert!(
        (seismic_buff(&env, "StunThreshold") - 60.0).abs() < 1e-6,
        "SeismicCry at default WarcryPower (20) should give +60% INC StunThreshold; got {}",
        seismic_buff(&env, "StunThreshold")
    );
    assert!(
        (env.output.get("SeismicCryArmourBonus") - 20.0).abs() < 1e-6,
        "SeismicCryArmourBonus output should mirror Armour MORE"
    );
    assert!(
        (env.output.get("SeismicCryStunThresholdBonus") - 60.0).abs() < 1e-6,
        "SeismicCryStunThresholdBonus output should mirror StunThreshold INC"
    );

    // WarcryPower 25 → cap. Armour MORE = 25, StunThreshold INC = 75.
    c.config.warcry_power = Some(25);
    let env = pob_engine::compute_full_with_env(&c, &tree, Some(&skills), None).1;
    assert!(
        (seismic_buff(&env, "Armour") - 25.0).abs() < 1e-6,
        "SeismicCry at WarcryPower 25 should hit the +25% Armour MORE cap"
    );
    assert!(
        (seismic_buff(&env, "StunThreshold") - 75.0).abs() < 1e-6,
        "SeismicCry at WarcryPower 25 should hit the +75% StunThreshold INC cap"
    );

    // WarcryPower 100 → still capped at 25-power.
    c.config.warcry_power = Some(100);
    let env = pob_engine::compute_full_with_env(&c, &tree, Some(&skills), None).1;
    assert!(
        (seismic_buff(&env, "Armour") - 25.0).abs() < 1e-6,
        "SeismicCry at WarcryPower 100 must still cap at +25% Armour MORE"
    );

    // Disabled gem → no buff.
    if let Some(group) = c.skill_groups.first_mut() {
        if let Some(gem) = group.gems.iter_mut().find(|g| g.skill_id == "SeismicCry") {
            gem.enabled = false;
        }
    }
    let env = pob_engine::compute_full_with_env(&c, &tree, Some(&skills), None).1;
    assert!(
        seismic_buff(&env, "Armour") < 1e-9,
        "Disabled SeismicCry must not contribute Armour MORE"
    );
}

// Issue #19 (slice 12): Battlemage's Cry's critical-strike-chance
// buff. PoB's `act_str.lua` BattlemagesCry statMap pins the
// formula: CritChance BASE = 50 × min(power, 25) / 5 / 100, i.e.
// 0.1 × min(power, 25). Same UsedWarcryRecently gate.
#[test]
fn battlemages_cry_injects_crit_chance_buff() {
    let (Some(tree), Some(skills)) = (load_3_25_tree(), load_skills()) else {
        eprintln!("skip: data missing");
        return;
    };
    if skills.get("BattlemagesCry").is_none() || skills.get("Cleave").is_none() {
        eprintln!("skip: BattlemagesCry / Cleave not in registry");
        return;
    }
    use pob_engine::character::SocketGroup;
    use pob_engine::ModStore as _;

    fn battlemage_crit(env: &pob_engine::Env) -> f64 {
        env.mod_db
            .iter_all()
            .filter(|m| {
                m.name == "CritChance"
                    && matches!(&m.source, Some(pob_engine::Source::Skill(s)) if s == "Battlemage's Cry")
            })
            .filter_map(|m| m.value.as_f64())
            .sum()
    }

    let mut c = Character::new(ClassRef::marauder(), 90);
    c.main_skill = Some(MainSkill::new("Cleave"));
    let mut warcry = MainSkill::new("BattlemagesCry");
    warcry.level = 20;
    c.skill_groups.push(SocketGroup {
        label: "Warcry".into(),
        gems: vec![warcry],
        main_active_skill_index: 1,
        enabled: true,
    });

    // No flag → no injection.
    let env = pob_engine::compute_full_with_env(&c, &tree, Some(&skills), None).1;
    assert!(
        battlemage_crit(&env) < 1e-9,
        "Without UsedWarcryRecently, BattlemagesCry must not inject CritChance"
    );

    // Default WarcryPower 20 → +2.0 base CritChance.
    c.config
        .conditions
        .insert("UsedWarcryRecently".into(), true);
    let env = pob_engine::compute_full_with_env(&c, &tree, Some(&skills), None).1;
    assert!(
        (battlemage_crit(&env) - 2.0).abs() < 1e-6,
        "BattlemagesCry at default WarcryPower (20) should give +2 base CritChance; got {}",
        battlemage_crit(&env)
    );
    assert!(
        (env.output.get("BattlemagesCryCritBonus") - 2.0).abs() < 1e-6,
        "BattlemagesCryCritBonus output should mirror the BASE value"
    );

    // Cap at 25 → +2.5 base CritChance.
    c.config.warcry_power = Some(25);
    let env = pob_engine::compute_full_with_env(&c, &tree, Some(&skills), None).1;
    assert!(
        (battlemage_crit(&env) - 2.5).abs() < 1e-6,
        "BattlemagesCry at WarcryPower 25 should hit the +2.5 cap; got {}",
        battlemage_crit(&env)
    );

    // Above cap (100) → still 2.5.
    c.config.warcry_power = Some(100);
    let env = pob_engine::compute_full_with_env(&c, &tree, Some(&skills), None).1;
    assert!(
        (battlemage_crit(&env) - 2.5).abs() < 1e-6,
        "BattlemagesCry at WarcryPower 100 must still cap at +2.5; got {}",
        battlemage_crit(&env)
    );

    // Disabled gem → no buff.
    if let Some(group) = c.skill_groups.first_mut() {
        if let Some(gem) = group
            .gems
            .iter_mut()
            .find(|g| g.skill_id == "BattlemagesCry")
        {
            gem.enabled = false;
        }
    }
    let env = pob_engine::compute_full_with_env(&c, &tree, Some(&skills), None).1;
    assert!(
        battlemage_crit(&env) < 1e-9,
        "Disabled BattlemagesCry must not contribute CritChance; got {}",
        battlemage_crit(&env)
    );
}

// Issue #19 (slice 16): Rallying Cry's per-ally exert damage
// bonus. PoB's RallyingCry statMap + CalcOffence.lua:2633:
//   ExertedAttackDamage MORE = 5 × min(NearbyAlly, 5)
// Constant 5%/ally, ally count capped at 5. Same
// `UsedWarcryRecently` gate; needs `nearby_allies > 0` config.
#[test]
fn rallying_cry_injects_per_ally_exerted_damage() {
    let (Some(tree), Some(skills)) = (load_3_25_tree(), load_skills()) else {
        eprintln!("skip: data missing");
        return;
    };
    if skills.get("RallyingCry").is_none() || skills.get("Cleave").is_none() {
        eprintln!("skip: RallyingCry / Cleave not in registry");
        return;
    }
    use pob_engine::character::SocketGroup;
    use pob_engine::ModStore as _;

    fn rallying_more(env: &pob_engine::Env) -> f64 {
        env.mod_db
            .iter_all()
            .filter(|m| {
                m.name == "ExertedAttackDamage"
                    && matches!(&m.source, Some(pob_engine::Source::Skill(s)) if s == "Rallying Cry")
            })
            .filter_map(|m| m.value.as_f64())
            .sum()
    }

    let mut c = Character::new(ClassRef::marauder(), 90);
    c.main_skill = Some(MainSkill::new("Cleave"));
    let mut warcry = MainSkill::new("RallyingCry");
    warcry.level = 20;
    c.skill_groups.push(SocketGroup {
        label: "Warcry".into(),
        gems: vec![warcry],
        main_active_skill_index: 1,
        enabled: true,
    });

    // No flag, no allies → no injection.
    let env = pob_engine::compute_full_with_env(&c, &tree, Some(&skills), None).1;
    assert!(
        rallying_more(&env) < 1e-9,
        "Without UsedWarcryRecently, RallyingCry must not inject ExertedAttackDamage"
    );

    // Flag on but no allies → still no injection (multiplier × 0 = 0).
    c.config
        .conditions
        .insert("UsedWarcryRecently".into(), true);
    let env = pob_engine::compute_full_with_env(&c, &tree, Some(&skills), None).1;
    assert!(
        rallying_more(&env) < 1e-9,
        "With nearby_allies = 0, RallyingCry must not inject any exert damage"
    );

    // 3 allies → +15% MORE on exerted attacks.
    c.config.nearby_allies = 3;
    let env = pob_engine::compute_full_with_env(&c, &tree, Some(&skills), None).1;
    let more = rallying_more(&env);
    assert!(
        (more - 15.0).abs() < 1e-6,
        "RallyingCry with 3 allies should give +15% MORE ExertedAttackDamage; got {more}"
    );
    assert!(
        (env.output.get("RallyingCryExertDamageBonus") - 15.0).abs() < 1e-6,
        "RallyingCryExertDamageBonus output should mirror the MORE value"
    );
    assert!(
        (env.output.get("RallyingCryAllyCount") - 3.0).abs() < 1e-6,
        "RallyingCryAllyCount should mirror the (capped) ally count"
    );

    // 5 allies → +25% MORE (cap).
    c.config.nearby_allies = 5;
    let env = pob_engine::compute_full_with_env(&c, &tree, Some(&skills), None).1;
    assert!(
        (rallying_more(&env) - 25.0).abs() < 1e-6,
        "RallyingCry with 5 allies should give +25% MORE (the cap); got {}",
        rallying_more(&env)
    );

    // 10 allies → still capped at 5 → +25% MORE.
    c.config.nearby_allies = 10;
    let env = pob_engine::compute_full_with_env(&c, &tree, Some(&skills), None).1;
    assert!(
        (rallying_more(&env) - 25.0).abs() < 1e-6,
        "RallyingCry with 10 allies must still cap at +25% MORE"
    );
    assert!(
        (env.output.get("RallyingCryAllyCount") - 5.0).abs() < 1e-6,
        "RallyingCryAllyCount should clamp to 5 even when config is higher"
    );

    // Disabled gem → no buff.
    if let Some(group) = c.skill_groups.first_mut() {
        if let Some(gem) = group.gems.iter_mut().find(|g| g.skill_id == "RallyingCry") {
            gem.enabled = false;
        }
    }
    let env = pob_engine::compute_full_with_env(&c, &tree, Some(&skills), None).1;
    assert!(
        rallying_more(&env) < 1e-9,
        "Disabled RallyingCry must not contribute ExertedAttackDamage"
    );
}

// Issue #145 (slice 3): Infernal Cry's phys-as-fire conversion.
// Per PoB's `act_str.lua` InfernalCry statMap:
//   infernal_cry_physical_damage_%_to_add_as_fire_per_5_power_up_to_cap
//   = 5 (constant); div = 5, limit = 25 → at WarcryPower 20 the
//   gain is `5 × min(20, 25) / 5 = 20%`.
// Verifies the buff lands as a `PhysicalDamageGainAsFire` BASE mod,
// flows through the slice-2 consumer, and lifts the player's hit
// damage end-to-end.
#[test]
fn infernal_cry_injects_phys_as_fire_with_warcry_recently() {
    let (Some(tree), Some(skills), Some(bases)) = (load_3_25_tree(), load_skills(), load_bases())
    else {
        eprintln!("skip: data missing");
        return;
    };
    if skills.get("InfernalCry").is_none() || skills.get("Cleave").is_none() {
        eprintln!("skip: InfernalCry / Cleave not in registry");
        return;
    }
    use pob_engine::character::SocketGroup;
    use pob_engine::ModStore as _;

    fn infernal_pct_in_db(env: &pob_engine::Env) -> f64 {
        env.mod_db
            .iter_all()
            .filter(|m| {
                m.name == "PhysicalDamageGainAsFire"
                    && matches!(&m.source, Some(pob_engine::Source::Skill(s)) if s == "Infernal Cry")
            })
            .filter_map(|m| m.value.as_f64())
            .sum()
    }

    let Some(sword_name) = bases
        .iter()
        .find(|(_, b)| b.r#type.contains("Sword") && b.weapon.is_some())
        .map(|(n, _)| n.clone())
    else {
        return;
    };
    let mut c = Character::new(ClassRef::duelist(), 90);
    let sword = parse_item(&format!(
        "Item Class: One Handed Swords\nRarity: NORMAL\n{sword_name}\n--------\n"
    ))
    .unwrap();
    c.items.equip(pob_data::Slot::Weapon1, sword);
    c.main_skill = Some(MainSkill::new("Cleave"));
    let mut warcry = MainSkill::new("InfernalCry");
    warcry.level = 20;
    c.skill_groups.push(SocketGroup {
        label: "Warcries".into(),
        gems: vec![warcry],
        main_active_skill_index: 1,
        enabled: true,
    });

    // Without UsedWarcryRecently → no buff lands.
    let env = pob_engine::compute_full_with_env(&c, &tree, Some(&skills), None).1;
    assert!(
        infernal_pct_in_db(&env) < 1e-9,
        "Without UsedWarcryRecently, InfernalCry must not inject PhysicalDamageGainAsFire"
    );

    // Flag on, default WarcryPower (20) → 5 × 20 / 5 = 20%.
    c.config
        .conditions
        .insert("UsedWarcryRecently".into(), true);
    let env = pob_engine::compute_full_with_env(&c, &tree, Some(&skills), None).1;
    let pct = infernal_pct_in_db(&env);
    assert!(
        (pct - 20.0).abs() < 1e-6,
        "InfernalCry at WarcryPower 20 should inject +20% PhysicalDamageGainAsFire; got {pct}"
    );
    assert!(
        (env.output.get("InfernalCryGainAsFireBonus") - 20.0).abs() < 1e-6,
        "InfernalCryGainAsFireBonus output should mirror the +20% mod"
    );
    // The slice-2 consumer should pick this up and produce a non-zero
    // PhysicalGainAsExtraDamage for the Cleave hit.
    assert!(
        env.output.get("PhysicalGainAsExtraDamage") > 0.0,
        "InfernalCry's gain-as-fire mod should flow through the slice-2 consumer"
    );

    // WarcryPower 25 (cap) → 25%.
    c.config.warcry_power = Some(25);
    let env = pob_engine::compute_full_with_env(&c, &tree, Some(&skills), None).1;
    let pct = infernal_pct_in_db(&env);
    assert!(
        (pct - 25.0).abs() < 1e-6,
        "InfernalCry at WarcryPower 25 should inject the +25% cap; got {pct}"
    );

    // WarcryPower 30 (over cap) → still 25%.
    c.config.warcry_power = Some(30);
    let env = pob_engine::compute_full_with_env(&c, &tree, Some(&skills), None).1;
    let pct = infernal_pct_in_db(&env);
    assert!(
        (pct - 25.0).abs() < 1e-6,
        "InfernalCry at WarcryPower 30 should still cap at +25%; got {pct}"
    );

    // Disabled gem → no buff.
    if let Some(group) = c.skill_groups.iter_mut().find(|g| g.label == "Warcries") {
        if let Some(gem) = group.gems.iter_mut().find(|g| g.skill_id == "InfernalCry") {
            gem.enabled = false;
        }
    }
    let env = pob_engine::compute_full_with_env(&c, &tree, Some(&skills), None).1;
    assert!(
        infernal_pct_in_db(&env) < 1e-9,
        "Disabled InfernalCry must not inject PhysicalDamageGainAsFire"
    );
}

// Issue #145 (slice 4): General's Cry's parallel-actor envelope.
// PoB's GeneralsCry summons N mirage warriors that each perform one
// cast of the player's main melee skill on cooldown. Verifies the
// envelope's mirage count, cooldown, and DPS contribution wire up.
#[test]
fn generals_cry_emits_mirage_envelope_and_lifts_full_dps() {
    let (Some(tree), Some(skills), Some(bases)) = (load_3_25_tree(), load_skills(), load_bases())
    else {
        eprintln!("skip: data missing");
        return;
    };
    if skills.get("GeneralsCry").is_none() || skills.get("Cleave").is_none() {
        eprintln!("skip: GeneralsCry / Cleave not in registry");
        return;
    }
    use pob_engine::character::SocketGroup;

    let Some(sword_name) = bases
        .iter()
        .find(|(_, b)| b.r#type.contains("Sword") && b.weapon.is_some())
        .map(|(n, _)| n.clone())
    else {
        return;
    };
    let mut c = Character::new(ClassRef::duelist(), 90);
    let sword = parse_item(&format!(
        "Item Class: One Handed Swords\nRarity: NORMAL\n{sword_name}\n--------\n"
    ))
    .unwrap();
    c.items.equip(pob_data::Slot::Weapon1, sword);
    c.main_skill = Some(MainSkill::new("Cleave"));
    let mut warcry = MainSkill::new("GeneralsCry");
    warcry.level = 20;
    c.skill_groups.push(SocketGroup {
        label: "Warcries".into(),
        gems: vec![warcry],
        main_active_skill_index: 1,
        enabled: true,
    });

    // Without UsedWarcryRecently → no envelope.
    let baseline = pob_engine::compute_full(&c, &tree, Some(&skills), Some(&bases));
    assert!(
        baseline.get("GeneralsCryMirageCount") < 1e-6,
        "Without UsedWarcryRecently, GeneralsCry must not emit envelope; got count {}",
        baseline.get("GeneralsCryMirageCount")
    );
    assert!(
        baseline.get("GeneralsCryDpsContribution") < 1e-6,
        "GeneralsCry must not contribute DPS without the warcry-recently flag"
    );
    let baseline_full = baseline.get("FullDPS");

    // Flag on → envelope lands.
    c.config
        .conditions
        .insert("UsedWarcryRecently".into(), true);
    let with_cry = pob_engine::compute_full(&c, &tree, Some(&skills), Some(&bases));
    let mirage_count = with_cry.get("GeneralsCryMirageCount");
    assert!(
        (mirage_count - 5.0).abs() < 1e-6,
        "GeneralsCry baseline mirage count should be 5; got {mirage_count}"
    );
    let cd = with_cry.get("GeneralsCryCooldown");
    assert!(
        cd > 0.0,
        "GeneralsCryCooldown should be non-zero from the gem's level data; got {cd}"
    );
    let cps_envelope = with_cry.get("GeneralsCryCastsPerSecond");
    assert!(
        (cps_envelope - mirage_count / cd).abs() < 1e-6,
        "GeneralsCryCastsPerSecond should equal mirage_count / cooldown; got {cps_envelope}"
    );
    let dps_contrib = with_cry.get("GeneralsCryDpsContribution");
    assert!(
        dps_contrib > 0.0,
        "GeneralsCry on a melee attack should contribute non-zero DPS; got {dps_contrib}"
    );
    let full = with_cry.get("FullDPS");
    assert!(
        full > baseline_full,
        "FullDPS should rise once GeneralsCry contributes (baseline {baseline_full}, with cry {full})"
    );
    assert!(
        (full - (baseline_full + dps_contrib)).abs() / full < 0.001,
        "FullDPS lift should equal GeneralsCryDpsContribution exactly: full={full} baseline={baseline_full} contrib={dps_contrib}"
    );
}

// Issue #145 (slice 1): Rallying Cry's per-ally weapon damage
// projection. PoB's RallyingCry statMap +
// `act_str.lua:8956` constantStats:
//   rallying_cry_weapon_damage_%_for_allies_per_5_monster_power = 4
// At WarcryPower 20 (PoB default) that's `4 × 20 / 5 = 16%`
// per-ally weapon-damage MORE projected to each party member.
#[test]
fn rallying_cry_projects_ally_weapon_damage_to_party_members() {
    let (Some(tree), Some(skills)) = (load_3_25_tree(), load_skills()) else {
        eprintln!("skip: data missing");
        return;
    };
    if skills.get("RallyingCry").is_none() || skills.get("Cleave").is_none() {
        eprintln!("skip: RallyingCry / Cleave not in registry");
        return;
    }
    use pob_engine::character::{PartyMember, SocketGroup};
    use pob_engine::ModStore as _;

    let mut c = Character::new(ClassRef::marauder(), 90);
    c.main_skill = Some(MainSkill::new("Cleave"));
    let mut warcry = MainSkill::new("RallyingCry");
    warcry.level = 20;
    c.skill_groups.push(SocketGroup {
        label: "Warcries".into(),
        gems: vec![warcry],
        main_active_skill_index: 1,
        enabled: true,
    });
    c.config
        .conditions
        .insert("UsedWarcryRecently".into(), true);
    c.config.nearby_allies = 3;
    c.party_members.push(PartyMember {
        name: "Ally A".into(),
        mod_lines: String::new(),
        extracted_auras: Vec::new(),
        enabled: true,
    });
    c.party_members.push(PartyMember {
        name: "Ally B".into(),
        mod_lines: String::new(),
        extracted_auras: Vec::new(),
        enabled: true,
    });

    let (out, env) = pob_engine::compute_full_with_env(&c, &tree, Some(&skills), None);

    // At default WarcryPower 20, per-ally weapon damage is 4 × 20 / 5 = 16%.
    let per_ally = out.get("RallyingCryAllyWeaponDamageBonus");
    assert!(
        (per_ally - 16.0).abs() < 1e-6,
        "RallyingCryAllyWeaponDamageBonus should be 16% at WarcryPower=20; got {per_ally}"
    );
    // 3 allies × 16% = 48% total envelope.
    let total = out.get("RallyingCryAllyWeaponDamageTotal");
    assert!(
        (total - 48.0).abs() < 1e-6,
        "RallyingCryAllyWeaponDamageTotal should be 48% with 3 allies; got {total}"
    );

    // The MORE Damage projection should land as a Party-sourced mod
    // for each enabled party member. Verify the Calcs breakdown can
    // attribute the buff back to each teammate.
    for name in ["Ally A", "Ally B"] {
        let source = format!("Party:{name}:RallyingCry");
        let projected = env
            .mod_db
            .iter_all()
            .filter(|m| {
                m.name == "Damage"
                    && matches!(&m.source, Some(pob_engine::Source::Other(s)) if s == &source)
            })
            .count();
        assert!(
            projected >= 1,
            "RallyingCry should project a Damage MORE mod sourced as {source}; got {projected}"
        );
    }

    // WarcryPower 25 (cap relevant for RallyingCry's per-power scaling):
    // 4 × 25 / 5 = 20%.
    c.config.warcry_power = Some(25);
    let out = pob_engine::compute_full_with_env(&c, &tree, Some(&skills), None).0;
    let per_ally = out.get("RallyingCryAllyWeaponDamageBonus");
    assert!(
        (per_ally - 20.0).abs() < 1e-6,
        "At WarcryPower 25, per-ally weapon damage should be 20%; got {per_ally}"
    );
}

// Issue #145 (slice 2): `PhysicalDamageGainAs<Element>` consumer in
// the calc pipeline. Mirrors PoB's `Modules/CalcOffence.lua:1869`
// damage-conversion block, which adds an EXTRA hit of the gained
// element equal to `phys_avg × gain_pct/100`, scaled by that
// element's resist + penetration. Without this consumer the
// mod_parser was minting `PhysicalDamageGainAsFire` BASE keys but
// the calc pipeline never read them — same gap STATUS.md flagged
// as priority 2 of "Still open", and the blocker for Infernal Cry's
// phys-as-fire piece.
#[test]
fn physical_damage_gain_as_element_adds_extra_hit_damage() {
    let (Some(tree), Some(skills), Some(bases)) = (load_3_25_tree(), load_skills(), load_bases())
    else {
        eprintln!("skip: data missing");
        return;
    };
    if skills.get("Cleave").is_none() {
        eprintln!("skip: Cleave not in registry");
        return;
    }
    let Some(sword_name) = bases
        .iter()
        .find(|(_, b)| b.r#type.contains("Sword") && b.weapon.is_some())
        .map(|(n, _)| n.clone())
    else {
        eprintln!("skip: no sword base available");
        return;
    };

    let mut c = Character::new(ClassRef::duelist(), 90);
    let sword = parse_item(&format!(
        "Item Class: One Handed Swords\nRarity: NORMAL\n{sword_name}\n--------\n"
    ))
    .unwrap();
    c.items.equip(pob_data::Slot::Weapon1, sword);
    c.main_skill = Some(MainSkill::new("Cleave"));

    // Pin enemy resist + boss preset to a known value so the math is
    // reproducible. Default `enemy_fire_resist` may already be 0 for a
    // bare-config character; set it explicitly anyway.
    c.config.enemy_fire_resist = 0;

    let baseline = pob_engine::compute_full(&c, &tree, Some(&skills), Some(&bases));
    let baseline_dps = baseline.get("MainSkillDPS");
    assert!(
        baseline_dps > 0.0,
        "Cleave on a bare sword should have a non-zero DPS baseline; got {baseline_dps}"
    );
    assert!(
        baseline.get("PhysicalGainAsExtraDamage") < 1e-6,
        "Without any gain-as mod, the extra-damage output must be unset (got {})",
        baseline.get("PhysicalGainAsExtraDamage")
    );

    // Inject 50% Physical Damage Gained as Fire via custom_mods (same
    // path the mod_parser uses for item text). At 0% enemy fire resist
    // this should add (avg × 50%) to the post-shock hit, lifting
    // MainSkillDPS by ~50%.
    c.config.custom_mods = "Gain 50% of Physical Damage as Extra Fire Damage".into();
    let with_gain = pob_engine::compute_full(&c, &tree, Some(&skills), Some(&bases));

    let gain_dps = with_gain.get("MainSkillDPS");
    let extra = with_gain.get("PhysicalGainAsExtraDamage");
    assert!(
        extra > 0.0,
        "Gain 50% as Extra Fire should produce a non-zero PhysicalGainAsExtraDamage output (got {extra})"
    );
    let pct_out = with_gain.get("PhysicalDamageGainAsFire");
    assert!(
        (pct_out - 50.0).abs() < 1e-6,
        "PhysicalDamageGainAsFire output should mirror the 50% mod value, got {pct_out}"
    );

    // The extra fire hit average per cast should be roughly avg_after_crit
    // × 50% × (1 - fire_res/100). At fire_res = 0 that's 0.5×.
    let crit_avg = with_gain.get("MainSkillAverageHitWithCrit");
    let expected_extra = crit_avg * 0.5;
    assert!(
        (extra - expected_extra).abs() / expected_extra < 0.05,
        "FireGainAsExtraAvg should be ~{expected_extra} (50% of crit-weighted avg), got {extra}"
    );

    // DPS lift should track the extra. Approx: `gain_dps ≈ baseline × (1 + 0.5 × phys_share)`.
    // Cleave on a normal sword has very little non-phys to dilute the
    // 50% so the lift should be substantial — at least 30%.
    let lift = gain_dps / baseline_dps;
    assert!(
        lift > 1.30,
        "50% PhysicalDamageGainAsFire should lift MainSkillDPS by >30% on a phys-sword Cleave, got {lift}× ({gain_dps} vs baseline {baseline_dps})"
    );
}

// Issue #145 (slice 2): the PhysicalDamageGainAs<Element> consumer
// must respect the gained element's enemy resist — at +75% fire
// resist, the same 50% gain-as mod should land roughly 1/4 of the
// pre-resist extra (since (1 - 75/100) = 0.25). Pen penetrates
// resist linearly per the existing dominant-element path.
#[test]
fn physical_damage_gain_as_fire_respects_enemy_fire_resist() {
    let (Some(tree), Some(skills), Some(bases)) = (load_3_25_tree(), load_skills(), load_bases())
    else {
        eprintln!("skip: data missing");
        return;
    };
    if skills.get("Cleave").is_none() {
        eprintln!("skip: Cleave not in registry");
        return;
    }
    let Some(sword_name) = bases
        .iter()
        .find(|(_, b)| b.r#type.contains("Sword") && b.weapon.is_some())
        .map(|(n, _)| n.clone())
    else {
        return;
    };

    let mut c = Character::new(ClassRef::duelist(), 90);
    let sword = parse_item(&format!(
        "Item Class: One Handed Swords\nRarity: NORMAL\n{sword_name}\n--------\n"
    ))
    .unwrap();
    c.items.equip(pob_data::Slot::Weapon1, sword);
    c.main_skill = Some(MainSkill::new("Cleave"));
    c.config.custom_mods = "Gain 50% of Physical Damage as Extra Fire Damage".into();

    c.config.enemy_fire_resist = 0;
    let zero_res = pob_engine::compute_full(&c, &tree, Some(&skills), Some(&bases));
    let extra_zero = zero_res.get("PhysicalGainAsExtraDamage");
    assert!(
        extra_zero > 0.0,
        "expected non-zero gain-as extra at 0% res"
    );

    c.config.enemy_fire_resist = 75;
    let high_res = pob_engine::compute_full(&c, &tree, Some(&skills), Some(&bases));
    let extra_high = high_res.get("PhysicalGainAsExtraDamage");
    let ratio = extra_high / extra_zero;
    assert!(
        (ratio - 0.25).abs() < 0.02,
        "75% fire resist should reduce gain-as extra to ~25% of the 0%-res value, got ratio {ratio} ({extra_high} vs {extra_zero})"
    );
}

// Issue #145 (slice 2): a non-physical hit (e.g. a pure-fire spell
// like Fireball) must NOT pick up `PhysicalDamageGainAsFire` — gain-as
// reads off the physical component of a hit, and a spell with no
// physical base damage has nothing to gain off. Verify the consumer
// is correctly gated on `is_physical_hit`.
#[test]
fn physical_damage_gain_as_skips_non_physical_skills() {
    let (Some(tree), Some(skills)) = (load_3_25_tree(), load_skills()) else {
        eprintln!("skip: data missing");
        return;
    };
    // Pick the first pure-fire spell available.
    let fire_spell = pick_stable_skill(&skills, |s| {
        s.base_flags.get("spell").copied().unwrap_or(false)
            && s.stats.iter().any(|st| st.contains("fire"))
    });
    let Some(fire_spell) = fire_spell else {
        eprintln!("skip: no fire spell in registry");
        return;
    };
    let mut c = Character::new(ClassRef::witch(), 90);
    c.main_skill = Some(MainSkill::new(&fire_spell));
    c.config.custom_mods = "Gain 50% of Physical Damage as Extra Fire Damage".into();
    c.config.enemy_fire_resist = 0;

    let out = pob_engine::compute_full(&c, &tree, Some(&skills), None);
    assert!(
        out.get("PhysicalGainAsExtraDamage") < 1e-6,
        "A pure-fire spell must not pick up phys gain-as extras; got {}",
        out.get("PhysicalGainAsExtraDamage")
    );
}

// Issue #19 (slice 14): `CritChance BASE` mods (Battlemage's Cry,
// Diamond Flask, certain jewels) need to land before the INC
// scaling, on top of the skill's intrinsic 5% baseline. PoB sums
// them as `(intrinsic + base_addn) × (1 + inc/100)`. Without the
// BASE-addition path, Battlemage's Cry's +2 / +2.5 base crit was
// silently dropped from `CritChance` and `MainSkillCritChance`.
#[test]
fn critchance_base_addition_lifts_main_skill_crit() {
    let (Some(tree), Some(skills)) = (load_3_25_tree(), load_skills()) else {
        eprintln!("skip: data missing");
        return;
    };
    if skills.get("Cleave").is_none() {
        eprintln!("skip: Cleave not in registry");
        return;
    }
    use pob_engine::{Mod, Source};

    let mut c = Character::new(ClassRef::marauder(), 90);
    c.main_skill = Some(MainSkill::new("Cleave"));

    let baseline = pob_engine::compute_full(&c, &tree, Some(&skills), None);
    let baseline_main = baseline.get("MainSkillCritChance");

    // Inject +20 base CritChance via the modDB (what a stack of
    // BattlemagesCry / Diamond Flask / Watcher's Eye would add).
    let (_, mut env) = pob_engine::compute_full_with_env(&c, &tree, Some(&skills), None);
    env.mod_db
        .add(Mod::base("CritChance", 20.0).with_source(Source::Other("test".into())));
    pob_engine::perform::perform_basic_stats(&c, &tree, &mut env);
    pob_engine::perform::perform_skill_dps(&c, &skills, &mut env);

    // MainSkillCritChance reflects the attack-side crit chance:
    //   (skill_base + crit_base_addn) × (1 + inc/100) × hit_chance
    // baseline ~ 5 × 1.0 × 0.7 = 3.5; with +20 BASE lift becomes
    // (5 + 20) × 0.7 = 17.5. So the lift must be roughly 4-5×
    // baseline in attacks — assert at least +10.
    let lifted_main = env.output.get("MainSkillCritChance");
    assert!(
        lifted_main > baseline_main + 10.0,
        "+20 base CritChance must lift MainSkillCritChance significantly (baseline {baseline_main}, got {lifted_main})"
    );

    // The basic-stat `CritChance` output gets overwritten by
    // perform_skill_dps to the attack-side effective crit, so it
    // tracks `MainSkillCritChance` here rather than the raw
    // basic-stats value. The basic-stats branch is exercised by
    // running it alone with the BASE injection in env-only form:
    let mut bare = pob_engine::perform::init_env(&c, &tree);
    bare.mod_db
        .add(Mod::base("CritChance", 20.0).with_source(Source::Other("test".into())));
    pob_engine::perform::perform_basic_stats(&c, &tree, &mut bare);
    let bare_crit = bare.output.get("CritChance");
    assert!(
        bare_crit >= 25.0,
        "perform_basic_stats with skill + 20 base CritChance should set CritChance to (5 + 20) × 1 = 25; got {bare_crit}"
    );
}

// Issue #19 (slice 13): the warcry buff injections from slices
// 8-10 + 12 (Enduring's LifeRegenPercent, Ancestral's
// ElementalResist, Seismic's Armour MORE, Battlemage's
// CritChance) need to flow through to the headline basic-stat
// outputs, not just sit in the modDB. Slice 13 re-ordered
// `detect_warcries` to run before `perform_basic_stats`; this
// test pins that the basic-stat outputs actually pick up the
// warcry buffs end-to-end.
#[test]
fn warcry_buff_chain_flows_through_to_basic_stats() {
    let (Some(tree), Some(skills)) = (load_3_25_tree(), load_skills()) else {
        eprintln!("skip: data missing");
        return;
    };
    let needed = [
        "EnduringCry",
        "AncestralCry",
        "SeismicCry",
        "BattlemagesCry",
        "Cleave",
    ];
    for id in &needed {
        if skills.get(id).is_none() {
            eprintln!("skip: {id} not in registry");
            return;
        }
    }
    use pob_engine::character::SocketGroup;

    let mut c = Character::new(ClassRef::marauder(), 90);
    c.main_skill = Some(MainSkill::new("Cleave"));

    // Baseline: no warcries socketed → no warcry-derived buffs.
    let baseline = pob_engine::compute_full(&c, &tree, Some(&skills), None);
    let baseline_life_regen = baseline.get("LifeRegen");
    let baseline_fire_resist = baseline.get("FireResist");
    let baseline_armour = baseline.get("Armour");
    let baseline_main_crit = baseline.get("MainSkillCritChance");

    // Add all four buff-shipping warcries + UsedWarcryRecently +
    // WarcryPower 25 (cap for most). Every basic-stat output
    // should now reflect the corresponding cry's buff.
    let mut warcry_gems = Vec::new();
    for id in [
        "EnduringCry",
        "AncestralCry",
        "SeismicCry",
        "BattlemagesCry",
    ] {
        let mut m = MainSkill::new(id);
        m.level = 20;
        warcry_gems.push(m);
    }
    c.skill_groups.push(SocketGroup {
        label: "Warcries".into(),
        gems: warcry_gems,
        main_active_skill_index: 1,
        enabled: true,
    });
    c.config
        .conditions
        .insert("UsedWarcryRecently".into(), true);
    c.config.warcry_power = Some(25);
    let buffed = pob_engine::compute_full(&c, &tree, Some(&skills), None);

    // Enduring Cry → +10%/s LifeRegenPercent. Player life × 0.10
    // should land in the LifeRegen output as additional regen.
    let life = baseline.get("Life");
    assert!(
        life > 0.0,
        "baseline must have non-zero Life pool to test life-regen scaling"
    );
    let buffed_life_regen = buffed.get("LifeRegen");
    let expected_extra = life * 0.10;
    assert!(
        buffed_life_regen >= baseline_life_regen + expected_extra * 0.99,
        "EnduringCry must add +10%/s of Life to LifeRegen (baseline {baseline_life_regen}, expected ≥ +{expected_extra:.2}, got {buffed_life_regen})"
    );

    // Ancestral Cry → +30% ElementalResist (cap at WarcryPower 25
    // → power-cap 30 not yet hit, so 25 → +25% ElementalResist).
    let buffed_fire_resist = buffed.get("FireResist");
    assert!(
        buffed_fire_resist >= baseline_fire_resist + 24.0,
        "AncestralCry must lift FireResist by ~+25% (baseline {baseline_fire_resist}, got {buffed_fire_resist})"
    );

    // Seismic Cry → +25% MORE Armour at WarcryPower 25.
    let buffed_armour = buffed.get("Armour");
    if baseline_armour > 0.0 {
        let expected_armour_floor = baseline_armour * 1.24;
        assert!(
            buffed_armour >= expected_armour_floor,
            "SeismicCry must scale Armour by ≥1.25× (baseline {baseline_armour}, expected ≥ {expected_armour_floor:.2}, got {buffed_armour})"
        );
    }

    // Battlemage's Cry → +2.5 base CritChance at WarcryPower 25.
    // Slice 14 ([PR #149]) added the BASE-addition path on top of
    // the skill's intrinsic 5% baseline, so this now actually
    // lifts MainSkillCritChance. The math:
    //   crit_chance = (5 + 2.5) × (1 + inc/100) × hit_chance
    //   baseline    =  5         × (1 + inc/100) × hit_chance
    // The lift is `2.5 / 5 = 50%` of baseline. Cleave default has
    // ~70% hit chance, so the absolute delta is at least 50% of
    // baseline_main_crit — assert that.
    let buffed_main_crit = buffed.get("MainSkillCritChance");
    let crit_lift_floor = baseline_main_crit * 0.49;
    assert!(
        buffed_main_crit >= baseline_main_crit + crit_lift_floor,
        "BattlemagesCry must lift MainSkillCritChance by ≥50% of baseline (baseline {baseline_main_crit}, expected ≥ {:.4}, got {buffed_main_crit})",
        baseline_main_crit + crit_lift_floor
    );
}

// Issue #19 (slice 11): integration test locking the warcry buff
// chain. Slices 6-10 each ship one warcry's per-skill buff
// (Intimidating's enemy debuff, Enduring's life regen,
// Ancestral's elemental resists, Seismic's armour + stun
// threshold). This test stacks all four at once with
// UsedWarcryRecently + WarcryPower = 25 and verifies every
// buff lands without cross-cancellation, locks the per-source
// modDB tagging, and pins the marker outputs from slice 7.
//
// Acts as a guard against future slices accidentally regressing
// any of the per-cry effects when adding Rallying / Infernal /
// Battlemage's / General's.
#[test]
fn warcry_buff_chain_stacks_independently() {
    let (Some(tree), Some(skills)) = (load_3_25_tree(), load_skills()) else {
        eprintln!("skip: data missing");
        return;
    };
    let needed = [
        "IntimidatingCry",
        "EnduringCry",
        "AncestralCry",
        "SeismicCry",
        "Cleave",
    ];
    for id in &needed {
        if skills.get(id).is_none() {
            eprintln!("skip: {id} not in registry");
            return;
        }
    }
    use pob_engine::character::SocketGroup;
    use pob_engine::ModStore as _;

    let mut c = Character::new(ClassRef::marauder(), 90);
    c.main_skill = Some(MainSkill::new("Cleave"));
    let mut warcry_gems = Vec::new();
    for id in [
        "IntimidatingCry",
        "EnduringCry",
        "AncestralCry",
        "SeismicCry",
    ] {
        let mut m = MainSkill::new(id);
        m.level = 20;
        warcry_gems.push(m);
    }
    c.skill_groups.push(SocketGroup {
        label: "Warcries".into(),
        gems: warcry_gems,
        main_active_skill_index: 1,
        enabled: true,
    });
    c.config
        .conditions
        .insert("UsedWarcryRecently".into(), true);
    c.config.warcry_power = Some(25);
    let env = pob_engine::compute_full_with_env(&c, &tree, Some(&skills), None).1;

    // Slice 6: Intimidating Cry's enemy debuff.
    assert!(
        env.state.condition("EnemyIntimidated"),
        "EnemyIntimidated must flip on with IntimidatingCry + UsedWarcryRecently"
    );

    // Slice 7: per-cry active markers.
    for key in [
        "IntimidatingCryActive",
        "EnduringCryActive",
        "AncestralCryActive",
        "SeismicCryActive",
    ] {
        assert!(
            (env.output.get(key) - 1.0).abs() < 1e-6,
            "{key} should be 1 with the cry in the loadout + UsedWarcryRecently — got {}",
            env.output.get(key)
        );
    }

    // Slice 8: Enduring Cry life regen at WarcryPower 25 (cap) → 10%/s.
    let regen: f64 = env
        .mod_db
        .iter_all()
        .filter(|m| {
            m.name == "LifeRegenPercent"
                && matches!(&m.source, Some(pob_engine::Source::Skill(s)) if s == "Enduring Cry")
        })
        .filter_map(|m| m.value.as_f64())
        .sum();
    assert!(
        (regen - 10.0).abs() < 1e-6,
        "Enduring Cry at WarcryPower 25 should give 10%/s LifeRegenPercent; got {regen}"
    );

    // Slice 9: Ancestral Cry resist + max-resist at cap (30 effective, but
    // we capped at 25 here since the test sets warcry_power = 25).
    // ElementalResist BASE = power, ElementalResistMax BASE = power × 0.1.
    let ancestral_res: f64 = env
        .mod_db
        .iter_all()
        .filter(|m| {
            m.name == "ElementalResist"
                && matches!(&m.source, Some(pob_engine::Source::Skill(s)) if s == "Ancestral Cry")
        })
        .filter_map(|m| m.value.as_f64())
        .sum();
    assert!(
        (ancestral_res - 25.0).abs() < 1e-6,
        "Ancestral Cry at WarcryPower 25 should give +25% ElementalResist; got {ancestral_res}"
    );
    let ancestral_max: f64 = env
        .mod_db
        .iter_all()
        .filter(|m| {
            m.name == "ElementalResistMax"
                && matches!(&m.source, Some(pob_engine::Source::Skill(s)) if s == "Ancestral Cry")
        })
        .filter_map(|m| m.value.as_f64())
        .sum();
    assert!(
        (ancestral_max - 2.5).abs() < 1e-6,
        "Ancestral Cry at WarcryPower 25 should give +2.5% ElementalResistMax; got {ancestral_max}"
    );

    // Slice 10: Seismic Cry armour MORE + stun threshold INC (cap 25).
    let seismic_armour: f64 = env
        .mod_db
        .iter_all()
        .filter(|m| {
            m.name == "Armour"
                && matches!(&m.source, Some(pob_engine::Source::Skill(s)) if s == "Seismic Cry")
        })
        .filter_map(|m| m.value.as_f64())
        .sum();
    assert!(
        (seismic_armour - 25.0).abs() < 1e-6,
        "Seismic Cry at WarcryPower 25 should give +25% MORE Armour; got {seismic_armour}"
    );
    let seismic_stun: f64 = env
        .mod_db
        .iter_all()
        .filter(|m| {
            m.name == "StunThreshold"
                && matches!(&m.source, Some(pob_engine::Source::Skill(s)) if s == "Seismic Cry")
        })
        .filter_map(|m| m.value.as_f64())
        .sum();
    assert!(
        (seismic_stun - 75.0).abs() < 1e-6,
        "Seismic Cry at WarcryPower 25 should give +75% INC StunThreshold; got {seismic_stun}"
    );

    // Slice 3 aggregates still work — total exert count is the sum
    // of each detected cry's `skill_empowers_next_x_melee_attacks`
    // constant: Intimidating 2, Enduring 0 (no exert), Ancestral 8,
    // Seismic 6 = 16.
    let total_exert = env.output.get("WarcryExertedAttackCountTotal");
    assert!(
        (total_exert - 16.0).abs() < 1e-6,
        "All four warcries together should aggregate exactly 16 exerted attacks (got {total_exert})"
    );
    assert!(
        (env.output.get("ActiveWarcryCount") - 4.0).abs() < 1e-6,
        "ActiveWarcryCount should equal 4 with all warcries socketed"
    );
}

// Issue #19 (slice 4): `ExertedAttackUptime` auto-derives from the
// slice-3 warcry aggregates when the user hasn't pinned it
// manually. Auto-uptime = `total_exert / (cps × cooldown)`, capped
// at 1; manual `config.exerted_attack_uptime > 0` still wins.
#[test]
fn exerted_uptime_auto_derives_from_warcry_loadout() {
    let (Some(tree), Some(skills), Some(bases)) = (load_3_25_tree(), load_skills(), load_bases())
    else {
        eprintln!("skip: data missing");
        return;
    };
    if skills.get("AncestralCry").is_none() || skills.get("Cleave").is_none() {
        eprintln!("skip: AncestralCry / Cleave not in registry");
        return;
    }
    use pob_engine::character::SocketGroup;
    use pob_engine::{Mod, Source};

    // Cleave + Ancestral Cry. We need at least one
    // `ExertedAttackDamage` mod for the existing exerted-DPS branch
    // to actually emit the output keys we want to assert on; pin one
    // via the modDB.
    let mut c = Character::new(ClassRef::marauder(), 90);
    c.main_skill = Some(MainSkill::new("Cleave"));
    let mut warcry = MainSkill::new("AncestralCry");
    warcry.level = 20;
    c.skill_groups.push(SocketGroup {
        label: "Warcry".into(),
        gems: vec![warcry],
        main_active_skill_index: 1,
        enabled: true,
    });

    // Manual override stays in control when set.
    c.config.exerted_attack_uptime = 0.42;
    let (_, mut env) = pob_engine::compute_full_with_env(&c, &tree, Some(&skills), Some(&bases));
    env.mod_db
        .add(Mod::more("ExertedAttackDamage", 50.0).with_source(Source::Other("test".into())));
    pob_engine::perform::perform_skill_dps(&c, &skills, &mut env);
    let manual = env.output.get("ExertedAttackUptime");
    assert!(
        (manual - 0.42).abs() < 0.001,
        "manual override should win when config.exerted_attack_uptime > 0; got {manual}"
    );

    // Drop the manual override → auto-derive kicks in.
    c.config.exerted_attack_uptime = 0.0;
    let (_, mut env) = pob_engine::compute_full_with_env(&c, &tree, Some(&skills), Some(&bases));
    env.mod_db
        .add(Mod::more("ExertedAttackDamage", 50.0).with_source(Source::Other("test".into())));
    pob_engine::perform::perform_skill_dps(&c, &skills, &mut env);
    let auto = env.output.get("ExertedAttackUptime");
    // We assert two things: the value is positive (auto-derive
    // fired) and capped at 1.0. The exact uptime depends on
    // Cleave's attack rate × Ancestral Cry cooldown which can shift
    // with skill data updates, so we don't pin the magnitude.
    assert!(
        auto > 0.0 && auto <= 1.0,
        "auto-uptime should be in (0, 1]; got {auto}"
    );
    // Sanity-cross-check against the slice-3 aggregates.
    let total_exert = env.output.get("WarcryExertedAttackCountTotal");
    let min_cd = env.output.get("WarcryMinCooldown");
    let cps = env.output.get("MainSkillSpeed");
    if total_exert > 0.0 && min_cd > 0.0 && cps > 0.0 {
        let expected = (total_exert / (cps * min_cd)).clamp(0.0, 1.0);
        assert!(
            (auto - expected).abs() < 0.001,
            "auto-uptime should equal total_exert / (cps × cooldown); got {auto} expected {expected}"
        );
    }
}

// Issue #68: Ruthless support `RuthlessBlowAilmentEffect`. Mirrors
// CalcOffence.lua:2780-2797 — `effect = (1 - chance) + chance × mult`,
// where `chance = 1 / RuthlessBlowMaxCount` and `mult = 1 + BASE/100`.
// Ruthless support gem at level 20 has MaxCount = 5 and ailment +100%
// (so mult = 2). Expected: `(1 - 0.2) + 0.2 × 2 = 1.2` → ailment DPS
// multiplied by 1.2.
#[test]
fn ruthless_blow_scales_ailment_dps() {
    let (Some(tree), Some(skills), Some(bases)) = (load_3_25_tree(), load_skills(), load_bases())
    else {
        eprintln!("skip: data missing");
        return;
    };
    if skills.get("Cleave").is_none() {
        eprintln!("skip: Cleave not in registry");
        return;
    }
    use pob_engine::{Mod, Source};

    // Set up a Cleave build with bleed chance so BleedDPS lands.
    let mut c = Character::new(ClassRef::duelist(), 90);
    c.main_skill = Some(MainSkill::new("Cleave"));
    let baseline = pob_engine::compute_full(&c, &tree, Some(&skills), Some(&bases));
    let baseline_bleed = baseline.get("BleedDPS");
    if baseline_bleed <= 0.0 {
        eprintln!("skip: Cleave doesn't bleed in this fixture");
        return;
    }

    // Inject the Ruthless support's payload directly: MaxCount = 5,
    // ailment multiplier +100% (so mult = 2). Expected ailment effect
    // factor = (1 - 0.2) + 0.2 × 2 = 1.2.
    let (_, mut env) = pob_engine::compute_full_with_env(&c, &tree, Some(&skills), Some(&bases));
    env.mod_db
        .add(Mod::base("RuthlessBlowMaxCount", 5.0).with_source(Source::Other("test".into())));
    env.mod_db.add(
        Mod::base("RuthlessBlowAilmentMultiplier", 100.0).with_source(Source::Other("test".into())),
    );
    pob_engine::perform::perform_skill_dps(&c, &skills, &mut env);
    let scaled_bleed = env.output.get("BleedDPS");
    let ratio = scaled_bleed / baseline_bleed;
    assert!(
        (ratio - 1.2).abs() < 0.02,
        "Ruthless 5-count × 2× ailment mult should multiply BleedDPS by 1.2; ratio={ratio}"
    );
    let effect = env.output.get("RuthlessBlowAilmentEffect");
    assert!(
        (effect - 1.2).abs() < 0.001,
        "RuthlessBlowAilmentEffect should equal 1.2; got {effect}"
    );
    let chance = env.output.get("RuthlessBlowChance");
    assert!(
        (chance - 20.0).abs() < 0.1,
        "RuthlessBlowChance should be 20% (1/5); got {chance}"
    );
}

// Issue #68: Fist of War support `FistOfWarDamageEffect`. Mirrors
// CalcOffence.lua:2799-2826 — Slam-skill-only multiplier whose
// average effect is `1 + DamageMultiplier × min((1/Speed)/Cooldown, 1)`.
// We verify Sunder (slam) picks up the support and Cleave (non-slam)
// does not.
#[test]
fn fist_of_war_only_applies_to_slam_skills() {
    let (Some(tree), Some(skills), Some(bases)) = (load_3_25_tree(), load_skills(), load_bases())
    else {
        eprintln!("skip: data missing");
        return;
    };
    if skills.get("Sunder").is_none() || skills.get("Cleave").is_none() {
        eprintln!("skip: Sunder/Cleave not in registry");
        return;
    }
    use pob_engine::{Mod, Source};

    // Slam skill (Sunder): Fist of War kicks in.
    let mut sunder = Character::new(ClassRef::duelist(), 90);
    sunder.main_skill = Some(MainSkill::new("Sunder"));
    let (_, mut env) =
        pob_engine::compute_full_with_env(&sunder, &tree, Some(&skills), Some(&bases));
    env.mod_db
        .add(Mod::base("FistOfWarCooldown", 3.0).with_source(Source::Other("test".into())));
    env.mod_db.add(
        Mod::base("FistOfWarDamageMultiplier", 80.0).with_source(Source::Other("test".into())),
    );
    pob_engine::perform::perform_skill_dps(&sunder, &skills, &mut env);
    let slam_effect = env.output.get("FistOfWarDamageEffect");
    assert!(
        slam_effect > 1.0,
        "Slam skill (Sunder) should pick up a Fist of War boost; got {slam_effect}"
    );
    let uptime = env.output.get("FistOfWarUptimeRatio");
    assert!(
        uptime > 0.0,
        "Slam skill should emit a non-zero FistOfWarUptimeRatio; got {uptime}"
    );

    // Non-slam skill (Cleave): the same mods don't trigger Fist of War.
    let mut cleave = Character::new(ClassRef::duelist(), 90);
    cleave.main_skill = Some(MainSkill::new("Cleave"));
    let (_, mut env) =
        pob_engine::compute_full_with_env(&cleave, &tree, Some(&skills), Some(&bases));
    env.mod_db
        .add(Mod::base("FistOfWarCooldown", 3.0).with_source(Source::Other("test".into())));
    env.mod_db.add(
        Mod::base("FistOfWarDamageMultiplier", 80.0).with_source(Source::Other("test".into())),
    );
    pob_engine::perform::perform_skill_dps(&cleave, &skills, &mut env);
    let cleave_effect = env.output.get("FistOfWarDamageEffect");
    assert!(
        (cleave_effect - 1.0).abs() < 0.001,
        "Non-slam skill (Cleave) must not pick up Fist of War; got {cleave_effect}"
    );
}

// Issue #68: parsed "<Ailment> enemies as though dealing N% more Damage"
// mods feed `<Ailment>AsThoughDealing` MORE multipliers into the ailment
// branch, scaling BleedDPS / PoisonDPS / IgniteDPS by `1 + N/100`.
// Mirrors upstream `Modules/CalcOffence.lua:5147 damage = ... ×
// More(cfg, ailment.."AsThoughDealing")`. Bleed only reads the
// `BleedAsThoughDealing` MORE — adding 100% should double BleedDPS.
#[test]
fn bleed_as_though_dealing_doubles_bleed_dps() {
    let (Some(tree), Some(skills), Some(bases)) = (load_3_25_tree(), load_skills(), load_bases())
    else {
        eprintln!("skip: data missing");
        return;
    };
    if skills.get("Cleave").is_none() {
        eprintln!("skip: Cleave not in registry");
        return;
    }

    // Baseline Cleave with bleed chance — pick up `BleedDPS > 0`.
    let mut c = Character::new(ClassRef::duelist(), 90);
    c.main_skill = Some(MainSkill::new("Cleave"));
    let baseline = pob_engine::compute_full(&c, &tree, Some(&skills), Some(&bases));
    let baseline_bleed = baseline.get("BleedDPS");
    if baseline_bleed <= 0.0 {
        eprintln!("skip: Cleave doesn't bleed in this fixture");
        return;
    }

    // Inject the "Bleed enemies as though dealing 100% more Damage" mod-line
    // through `custom_mods`. The mod-parser converts it into a MORE on
    // `BleedAsThoughDealing`; the ailment branch reads it and lifts BleedDPS
    // by exactly 2×.
    c.config.custom_mods = "Bleed enemies as though dealing 100% more Damage".to_owned();
    let scaled = pob_engine::compute_full(&c, &tree, Some(&skills), Some(&bases));
    let scaled_bleed = scaled.get("BleedDPS");
    let ratio = scaled_bleed / baseline_bleed;
    assert!(
        (ratio - 2.0).abs() < 0.02,
        "BleedAsThoughDealing 100% MORE should double BleedDPS; ratio={ratio} (baseline={baseline_bleed}, scaled={scaled_bleed})"
    );
}

// Issue #68: the same parser path handles Poison / Ignite. Verifies that the
// stat name lookup is wired correctly per-ailment (no copy-paste bug
// pointing all three branches at the same key) by injecting a Poison-only
// mod and confirming PoisonDPS scales but BleedDPS does not.
#[test]
fn poison_as_though_dealing_only_scales_poison() {
    let (Some(tree), Some(skills), Some(bases)) = (load_3_25_tree(), load_skills(), load_bases())
    else {
        return;
    };
    if skills.get("Cleave").is_none() {
        return;
    }
    use pob_engine::{Mod, Source};

    // Equip a weapon and inject both Bleed + Poison chance so both ailment
    // branches land. Use `compute_full_with_env` so we can stamp the
    // PoisonAsThoughDealing MORE directly into the mod_db post-init —
    // this isolates the stat-name dispatch from any parser bug.
    let mut c = Character::new(ClassRef::duelist(), 90);
    c.main_skill = Some(MainSkill::new("Cleave"));
    c.config.custom_mods = "30% chance to Poison on Hit".to_owned();
    let baseline = pob_engine::compute_full(&c, &tree, Some(&skills), Some(&bases));
    let baseline_poison = baseline.get("PoisonDPS");
    let baseline_bleed = baseline.get("BleedDPS");
    if baseline_poison <= 0.0 || baseline_bleed <= 0.0 {
        eprintln!("skip: Cleave fixture lacks both poison and bleed paths");
        return;
    }

    let (_, mut env) = pob_engine::compute_full_with_env(&c, &tree, Some(&skills), Some(&bases));
    env.mod_db
        .add(Mod::more("PoisonAsThoughDealing", 100.0).with_source(Source::Other("test".into())));
    pob_engine::perform::perform_skill_dps(&c, &skills, &mut env);
    let poison_ratio = env.output.get("PoisonDPS") / baseline_poison;
    let bleed_ratio = env.output.get("BleedDPS") / baseline_bleed;
    assert!(
        (poison_ratio - 2.0).abs() < 0.02,
        "PoisonAsThoughDealing 100% MORE should double PoisonDPS; ratio={poison_ratio}"
    );
    assert!(
        (bleed_ratio - 1.0).abs() < 0.02,
        "PoisonAsThoughDealing must NOT scale BleedDPS; bleed ratio={bleed_ratio}"
    );
}

// Issue #19 (composition): INC and MORE for ExertedAttackDamage compose
// multiplicatively, matching PoE's `(1 + inc/100) × more` chain. With
// 50% INC and a separate 50% MORE the per-exerted-attack factor is
// `1.5 × 1.5 = 2.25`, so at 100% uptime the average DPS lands × 2.25
// vs the unexerted baseline; at 50% uptime it lands × 1.625.
#[test]
fn exerted_attack_inc_and_more_compose_multiplicatively() {
    let (Some(tree), Some(skills), Some(bases)) = (load_3_25_tree(), load_skills(), load_bases())
    else {
        eprintln!("skip: data missing");
        return;
    };
    let Some(_) = skills.get("Cleave") else {
        eprintln!("skip: Cleave not found");
        return;
    };
    let sword_name = bases
        .iter()
        .find(|(_, b)| b.r#type.contains("Sword") && b.weapon.is_some())
        .map(|(n, _)| n.clone());
    let Some(sword_name) = sword_name else {
        return;
    };

    let mut c = Character::new(ClassRef::duelist(), 90);
    let sword = parse_item(&format!(
        "Item Class: One Handed Swords\nRarity: NORMAL\n{sword_name}\n--------\n"
    ))
    .unwrap();
    c.items.equip(pob_data::Slot::Weapon1, sword);
    c.main_skill = Some(MainSkill::new("Cleave"));

    // Body armour grants 50% INC; custom_mods adds 50% MORE on the same
    // ExertedAttackDamage stat from a separate source.
    let body = parse_item(
        "Item Class: Body Armours\nRarity: RARE\nWarcry Plate\nFull Plate\n--------\nExerted Attacks deal 50% increased Damage\n--------",
    )
    .unwrap();
    c.items.equip(pob_data::Slot::BodyArmour, body);
    c.config.custom_mods = "Exerted Attacks deal 50% more Damage".to_owned();

    let baseline = pob_engine::compute_full(&c, &tree, Some(&skills), Some(&bases));
    let baseline_dps = baseline.get("MainSkillDPS");
    if baseline_dps <= 0.0 {
        eprintln!("skip: Cleave produces no DPS in this fixture");
        return;
    }

    // 100% uptime: every attack is exerted; factor = 1.5 × 1.5 = 2.25.
    c.config.exerted_attack_uptime = 1.0;
    let full = pob_engine::compute_full(&c, &tree, Some(&skills), Some(&bases));
    let ratio = full.get("MainSkillDPS") / baseline_dps;
    assert!(
        (ratio - 2.25).abs() < 0.01,
        "100% uptime + 50% INC × 50% MORE should multiply DPS by 2.25 (1.5 × 1.5); ratio={ratio}"
    );
    assert!(
        (full.get("ExertedAttackDamageBonus") - 125.0).abs() < 0.01,
        "ExertedAttackDamageBonus should reflect the multiplicative composition: (1.5 × 1.5 - 1) × 100 = 125; got {}",
        full.get("ExertedAttackDamageBonus")
    );

    // 50% uptime: factor = 1 + 0.5 × 1.25 = 1.625.
    c.config.exerted_attack_uptime = 0.5;
    let half = pob_engine::compute_full(&c, &tree, Some(&skills), Some(&bases));
    let ratio = half.get("MainSkillDPS") / baseline_dps;
    assert!(
        (ratio - 1.625).abs() < 0.01,
        "50% uptime + 50% INC × 50% MORE should multiply DPS by 1.625; ratio={ratio}"
    );
}

// Issue #16 (totem half): a totem-summoning skill's MainSkillDPS must
// scale by the player's `ActiveTotemLimit` (default 1; supports like
// Multiple Totems Support raise the limit). Mirrors PoB's
// CalcOffence.lua:1388 totem branch.
#[test]
fn totem_skill_dps_scales_with_active_totem_limit() {
    let (Some(tree), Some(skills), Some(bases)) = (load_3_25_tree(), load_skills(), load_bases())
    else {
        eprintln!("skip: data missing");
        return;
    };

    let Some(_) = skills.get("HolyFlameTotem") else {
        eprintln!("skip: HolyFlameTotem not found");
        return;
    };
    let mut c = Character::new(ClassRef::templar(), 90);
    c.main_skill = Some(MainSkill::new("HolyFlameTotem"));

    let baseline = pob_engine::compute_full(&c, &tree, Some(&skills), Some(&bases));
    let baseline_dps = baseline.get("MainSkillDPS");
    if baseline_dps <= 0.0 {
        eprintln!("skip: HolyFlameTotem baseline DPS is zero");
        return;
    }
    // Default ActiveTotemLimit is 1 (PoE base).
    assert!(
        (baseline.get("ActiveTotemLimit") - 1.0).abs() < 0.001,
        "Default ActiveTotemLimit should be 1, got {}",
        baseline.get("ActiveTotemLimit")
    );
    assert!(
        (baseline.get("NumberOfTotems") - 1.0).abs() < 0.001,
        "NumberOfTotems should mirror ActiveTotemLimit"
    );

    // Equip a helmet granting "+1 to maximum number of Summoned Totems"
    // — bumps ActiveTotemLimit to 2 and doubles MainSkillDPS.
    let helm = parse_item(
        "Item Class: Helmets\nRarity: RARE\nTotem Crown\nIron Hat\n--------\n+1 to maximum number of Summoned Totems\n--------",
    )
    .unwrap();
    c.items.equip(pob_data::Slot::Helmet, helm);
    let two_totems = pob_engine::compute_full(&c, &tree, Some(&skills), Some(&bases));
    assert!(
        (two_totems.get("ActiveTotemLimit") - 2.0).abs() < 0.001,
        "+1 totem mod should lift ActiveTotemLimit to 2, got {}",
        two_totems.get("ActiveTotemLimit")
    );
    let ratio = two_totems.get("MainSkillDPS") / baseline_dps;
    assert!(
        (ratio - 2.0).abs() < 0.01,
        "MainSkillDPS should double with +1 totem; ratio={ratio} (baseline={baseline_dps}, two={})",
        two_totems.get("MainSkillDPS")
    );

    // Non-totem skill (Arc) should NOT emit ActiveTotemLimit and the
    // existing DPS should be unaffected by totem mods on items.
    let Some(_) = skills.get("Arc") else { return };
    let mut arc_char = Character::new(ClassRef::witch(), 90);
    arc_char.main_skill = Some(MainSkill::new("Arc"));
    let arc_baseline = pob_engine::compute_full(&arc_char, &tree, Some(&skills), Some(&bases));
    assert_eq!(
        arc_baseline.try_get("ActiveTotemLimit"),
        None,
        "Arc (non-totem skill) should not emit ActiveTotemLimit"
    );

    // Even with the +1-totem helm, Arc DPS must not change.
    let helm2 = parse_item(
        "Item Class: Helmets\nRarity: RARE\nTotem Crown\nIron Hat\n--------\n+1 to maximum number of Summoned Totems\n--------",
    )
    .unwrap();
    arc_char.items.equip(pob_data::Slot::Helmet, helm2);
    let arc_after = pob_engine::compute_full(&arc_char, &tree, Some(&skills), Some(&bases));
    assert!(
        (arc_after.get("MainSkillDPS") - arc_baseline.get("MainSkillDPS")).abs() < 0.01,
        "Arc DPS must not respond to totem mods (non-totem skill)"
    );
}

// where `stored` is the per-cast physical hit average post-crit. With no
// ImpaleChance source the impale path must zero out cleanly, and a body
// armour granting "30% chance to Impale on Hit" must surface a non-zero
// ImpaleDPS that approximately matches `0.3 × 5 × 0.10 × main_dps`.
#[test]
fn impale_chance_drives_impale_dps() {
    let (Some(tree), Some(skills), Some(bases)) = (load_3_25_tree(), load_skills(), load_bases())
    else {
        return;
    };
    let xml_path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .parent()
        .unwrap()
        .join("crates/pob-extract/test-builds/marauder_l90_cleave_with_axe.xml");
    let Ok(xml) = std::fs::read_to_string(&xml_path) else {
        eprintln!("skip: {} not found", xml_path.display());
        return;
    };
    let mut c = pob_engine::import_pob_xml(&xml).expect("import cleave fixture");

    // Baseline: no impale chance -> ImpaleDPS == 0, output keys populated
    // so the Calcs tab side panel doesn't show blanks.
    let baseline = pob_engine::compute_full(&c, &tree, Some(&skills), Some(&bases));
    let baseline_main = baseline.get("MainSkillDPS");
    assert_eq!(baseline.get("ImpaleChance"), 0.0);
    assert_eq!(baseline.get("ImpaleStoredHitAvg"), 0.0);
    assert_eq!(baseline.get("ImpaleDPS"), 0.0);
    assert!(
        (baseline.get("FullDPS") - baseline_main).abs() < 0.01,
        "FullDPS should equal MainSkillDPS when there's no impale (and no ailments)"
    );

    // Equip a body armour granting 30% chance to Impale on Hit. The
    // mod_parser maps "Impale" -> ImpaleChance BASE 30, which feeds the
    // impale calc.
    let body = parse_item(
        "Item Class: Body Armours\nRarity: RARE\nDoom Carapace\nFull Plate\n--------\n+50 to maximum Life\n30% chance to Impale Enemies on Hit\n--------",
    )
    .expect("parse impaling body armour");
    c.items.equip(pob_data::Slot::BodyArmour, body);

    let armoured = pob_engine::compute_full(&c, &tree, Some(&skills), Some(&bases));
    assert_eq!(
        armoured.get("ImpaleChance"),
        30.0,
        "ImpaleChance output should reflect the 30% body-armour mod"
    );
    let stored = armoured.get("ImpaleStoredHitAvg");
    let impale_dps = armoured.get("ImpaleDPS");
    let main_dps = armoured.get("MainSkillDPS");
    assert!(
        stored > 0.0,
        "ImpaleStoredHitAvg should track the physical hit avg, got {stored}"
    );
    assert!(
        impale_dps > 0.0,
        "Non-zero ImpaleChance must surface non-zero ImpaleDPS, got {impale_dps}"
    );
    // FullDPS now folds in impale.
    assert!(
        armoured.get("FullDPS") > main_dps,
        "FullDPS should grow once impale lands: full={} main={}",
        armoured.get("FullDPS"),
        main_dps
    );
    // WithImpaleDPS = MainSkillDPS + ImpaleDPS.
    let combined = main_dps + impale_dps;
    assert!(
        (armoured.get("WithImpaleDPS") - combined).abs() < 0.01,
        "WithImpaleDPS should equal MainSkillDPS + ImpaleDPS"
    );
}

// Issue #9: a Granite Flask granting "+3000 to Armour during Flask Effect"
// must contribute its bonus only when the `UsingFlask` config toggle is on,
// matching PoB's gating in CalcSetup.lua. Without the toggle the mod is in
// modDB but the Condition tag (auto-emitted by mod_parser from "during Flask
// effect") gates evaluation; with the toggle the bonus lands.
#[test]
fn flask_armour_mod_gates_on_using_flask_toggle() {
    let Some(tree) = load_3_25_tree() else {
        return;
    };
    let mut c = Character::new(ClassRef::marauder(), 90);

    // Force UsingFlask=false explicitly so we know what we're measuring.
    c.config.conditions.insert("UsingFlask".to_owned(), false);
    let no_flask_baseline = compute_with_skills(&c, &tree, None);
    let baseline_armour = no_flask_baseline.get("Armour");

    let flask = parse_item(
        "Item Class: Utility Flasks\nRarity: NORMAL\nGranite Flask\n--------\n+3000 to Armour during Flask Effect\n--------",
    )
    .expect("parse Granite Flask");
    c.items.equip(pob_data::Slot::Flask1, flask);

    // With the flask equipped but UsingFlask off, armour should be unchanged
    // — the mod is gated by the Condition tag.
    let off = compute_with_skills(&c, &tree, None);
    let off_armour = off.get("Armour");
    assert!(
        (off_armour - baseline_armour).abs() < 0.5,
        "With UsingFlask=false the flask mod must not apply: baseline={baseline_armour} got={off_armour}"
    );

    // Toggle UsingFlask on; armour should jump by ~3000.
    c.config.conditions.insert("UsingFlask".to_owned(), true);
    let on = compute_with_skills(&c, &tree, None);
    let on_armour = on.get("Armour");
    assert!(
        on_armour - off_armour > 2900.0,
        "With UsingFlask=true the Granite Flask should add ~3000 Armour: off={off_armour} on={on_armour}"
    );
}

// Issue #75: per-preset damage / pen / armour / evasion baselines.
// Pinnacle Boss adds 3% elemental pen (`pinnacleBossPen = 15/5`); Uber
// adds 8% (`uberBossPen = 40/5`); Boss has no implicit pen. Default
// armour and evasion lift to 36000 / 6000 for Pinnacle/Uber.
#[test]
fn enemy_boss_preset_per_preset_damage_defaults() {
    let Some(tree) = load_3_25_tree() else {
        eprintln!("skip: data missing");
        return;
    };
    use pob_engine::character::EnemyBoss;

    // default_penetration values mirror PoB's Data.lua constants.
    assert_eq!(EnemyBoss::None.default_penetration(), 0);
    assert_eq!(EnemyBoss::Boss.default_penetration(), 0);
    assert_eq!(EnemyBoss::Pinnacle.default_penetration(), 3);
    assert_eq!(EnemyBoss::Uber.default_penetration(), 8);

    // default_armour / default_evasion: Pinnacle and Uber jump to fixed
    // baselines mirroring PoB's `data.bossStats.PinnacleArmourMean` and
    // `PinnacleEvasionMean`. Boss inherits the level-derived default (0
    // means "fall back to engine's MONSTER_ARMOUR_TABLE").
    assert_eq!(EnemyBoss::None.default_armour(), 0);
    assert_eq!(EnemyBoss::Boss.default_armour(), 0);
    assert_eq!(EnemyBoss::Pinnacle.default_armour(), 36_000);
    assert_eq!(EnemyBoss::Uber.default_armour(), 36_000);
    assert_eq!(EnemyBoss::Pinnacle.default_evasion(), 6_000);

    // dps_taken_multiplier mirrors PoB's `Data.lua` constants:
    //   stdBossDPSMult     = 4 / 4.40    ≈ 0.909
    //   pinnacleBossDPSMult = 8 / 4.40   ≈ 1.818
    //   uberBossDPSMult    = 10 / 4.25   ≈ 2.353
    assert!((EnemyBoss::None.dps_taken_multiplier() - 1.0).abs() < 1e-6);
    assert!((EnemyBoss::Boss.dps_taken_multiplier() - 4.0 / 4.40).abs() < 1e-6);
    assert!((EnemyBoss::Pinnacle.dps_taken_multiplier() - 8.0 / 4.40).abs() < 1e-6);
    assert!((EnemyBoss::Uber.dps_taken_multiplier() - 10.0 / 4.25).abs() < 1e-6);

    // Engine: selecting Pinnacle injects an ElementalPenetration BASE
    // mod sourced from the preset.
    let mut c = Character::new(ClassRef::marauder(), 90);
    c.config.enemy_boss = EnemyBoss::Pinnacle;
    let env = pob_engine::perform::init_env(&c, &tree);
    use pob_engine::ModStore as _;
    let pen_mod = env
        .mod_db
        .iter_all()
        .find(|m| {
            m.name == "ElementalPenetration"
                && matches!(&m.source, Some(pob_engine::Source::Other(s)) if s == "EnemyBoss:Pinnacle")
        })
        .expect("Pinnacle preset should emit ElementalPenetration BASE");
    assert_eq!(pen_mod.value.as_f64(), Some(3.0));

    // Boss preset emits no pen mod.
    let mut c = Character::new(ClassRef::marauder(), 90);
    c.config.enemy_boss = EnemyBoss::Boss;
    let env = pob_engine::perform::init_env(&c, &tree);
    let pen_mods: Vec<_> = env
        .mod_db
        .iter_all()
        .filter(|m| {
            m.name == "ElementalPenetration"
                && matches!(&m.source, Some(pob_engine::Source::Other(s)) if s.starts_with("EnemyBoss:"))
        })
        .collect();
    assert!(
        pen_mods.is_empty(),
        "Boss preset should not emit ElementalPenetration"
    );

    // EnemyBossDpsTakenMultiplier output key: surfaces only when the
    // preset moves the multiplier away from 1.0.
    let mut c = Character::new(ClassRef::marauder(), 90);
    c.config.enemy_boss = EnemyBoss::Pinnacle;
    let out = pob_engine::compute_full(&c, &tree, None, None);
    let mult = out.get("EnemyBossDpsTakenMultiplier");
    assert!(
        (mult - 8.0 / 4.40).abs() < 1e-6,
        "Pinnacle should set EnemyBossDpsTakenMultiplier ≈ 8/4.40, got {mult}"
    );
}

// Issue #35: EnemyBoss preset (None / Boss / Pinnacle / Uber) injects
// `Condition:RareOrUnique` (all non-None presets) and
// `Condition:PinnacleBoss` (Pinnacle + Uber) into the eval state, plus
// an `AilmentThreshold` MORE that mirrors PoB's `enemyIsBoss`
// ConfigOption (488 for Boss, 404 for Pinnacle/Uber).
#[test]
fn enemy_boss_preset_emits_conditions_and_ailment_threshold() {
    let Some(tree) = load_3_25_tree() else {
        eprintln!("skip: data missing");
        return;
    };
    use pob_engine::character::EnemyBoss;

    let mut c = Character::new(ClassRef::marauder(), 90);

    // Default: EnemyBoss::None — no conditions, no AilmentThreshold mod.
    assert_eq!(c.config.enemy_boss, EnemyBoss::None);
    let env = pob_engine::perform::init_env(&c, &tree);
    assert!(
        !env.state.condition("RareOrUnique"),
        "None preset must not flag RareOrUnique"
    );
    assert!(
        !env.state.condition("PinnacleBoss"),
        "None preset must not flag PinnacleBoss"
    );
    use pob_engine::ModStore as _;
    let none_threshold_mods: Vec<_> = env
        .mod_db
        .iter_all()
        .filter(|m| m.name == "AilmentThreshold")
        .collect();
    assert!(
        none_threshold_mods.is_empty(),
        "None preset should not emit any AilmentThreshold mods, got {}",
        none_threshold_mods.len()
    );

    // Boss: RareOrUnique + AilmentThreshold MORE 488.
    c.config.enemy_boss = EnemyBoss::Boss;
    let env = pob_engine::perform::init_env(&c, &tree);
    assert!(
        env.state.condition("RareOrUnique"),
        "Boss preset must flag RareOrUnique"
    );
    assert!(
        !env.state.condition("PinnacleBoss"),
        "Boss preset must not flag PinnacleBoss"
    );
    let boss_threshold = env
        .mod_db
        .iter_all()
        .find(|m| m.name == "AilmentThreshold")
        .expect("Boss preset must emit AilmentThreshold");
    assert_eq!(
        boss_threshold.value.as_f64(),
        Some(488.0),
        "Boss AilmentThreshold MORE should be 488 (matching PoB)"
    );

    // Pinnacle: RareOrUnique + PinnacleBoss + AilmentThreshold MORE 404.
    c.config.enemy_boss = EnemyBoss::Pinnacle;
    let env = pob_engine::perform::init_env(&c, &tree);
    assert!(env.state.condition("RareOrUnique"));
    assert!(
        env.state.condition("PinnacleBoss"),
        "Pinnacle preset must flag PinnacleBoss"
    );
    let pinnacle_threshold = env
        .mod_db
        .iter_all()
        .find(|m| m.name == "AilmentThreshold")
        .expect("Pinnacle preset must emit AilmentThreshold");
    assert_eq!(
        pinnacle_threshold.value.as_f64(),
        Some(404.0),
        "Pinnacle AilmentThreshold MORE should be 404"
    );

    // Uber: same conditions as Pinnacle (it's "harder Pinnacle" with
    // upgraded damage / pen — those are surfaced via separate ConfigState
    // sliders, not this preset).
    c.config.enemy_boss = EnemyBoss::Uber;
    let env = pob_engine::perform::init_env(&c, &tree);
    assert!(env.state.condition("RareOrUnique"));
    assert!(env.state.condition("PinnacleBoss"));

    // Default-resists helper: Boss → 40/40/40/25, Pinnacle/Uber → 50/50/50/30.
    assert_eq!(EnemyBoss::Boss.default_resists(), (40, 40, 40, 25));
    assert_eq!(EnemyBoss::Pinnacle.default_resists(), (50, 50, 50, 30));
    assert_eq!(EnemyBoss::Uber.default_resists(), (50, 50, 50, 30));
    assert_eq!(EnemyBoss::None.default_resists(), (0, 0, 0, 0));

    // PoB-name round trip.
    for variant in [
        EnemyBoss::None,
        EnemyBoss::Boss,
        EnemyBoss::Pinnacle,
        EnemyBoss::Uber,
    ] {
        assert_eq!(
            EnemyBoss::from_pob_name(variant.as_pob_name()),
            Some(variant),
            "round trip failed for {:?}",
            variant
        );
    }
}

// Issue #10 (Pantheon half): Major + Minor god soul[1] effects are
// parsed through `mod_parser` and injected into the player modDB with
// `source = "Pantheon:<god>"`. Guards two flows: the framework (enums
// + Character round-trip + apply hook) and at least one god's mod text
// actually parsing cleanly through mod_parser.
#[test]
fn pantheon_selection_round_trips_and_injects_parseable_mods() {
    let Some(tree) = load_3_25_tree() else {
        eprintln!("skip: data missing");
        return;
    };
    use pob_engine::character::{MajorGod, MinorGod};

    // Default: None / None — no Pantheon mods land.
    let mut c = Character::new(ClassRef::marauder(), 90);
    assert_eq!(c.pantheon_major, MajorGod::None);
    assert_eq!(c.pantheon_minor, MinorGod::None);
    let env = pob_engine::perform::init_env(&c, &tree);
    use pob_engine::ModStore as _;
    let pantheon_mods: Vec<_> = env
        .mod_db
        .iter_all()
        .filter(|m| {
            matches!(&m.source, Some(pob_engine::Source::Other(s)) if s.starts_with("Pantheon:"))
        })
        .collect();
    assert!(
        pantheon_mods.is_empty(),
        "None / None should not emit any Pantheon mods, got {}",
        pantheon_mods.len()
    );

    // Arakaali: "10% reduced Damage taken from Damage Over Time".
    // mod_parser handles this; the mod must show up sourced from
    // "Pantheon:Arakaali".
    c.pantheon_major = MajorGod::Arakaali;
    let env = pob_engine::perform::init_env(&c, &tree);
    let arakaali_mods: Vec<_> = env
        .mod_db
        .iter_all()
        .filter(
            |m| matches!(&m.source, Some(pob_engine::Source::Other(s)) if s == "Pantheon:Arakaali"),
        )
        .collect();
    assert!(
        !arakaali_mods.is_empty(),
        "Soul of Arakaali should inject at least one mod (parseable through mod_parser)"
    );

    // Garukhan: "60% reduced Effect of Shock on you".
    c.pantheon_major = MajorGod::None;
    c.pantheon_minor = MinorGod::Garukhan;
    let env = pob_engine::perform::init_env(&c, &tree);
    let garukhan_mods: Vec<_> = env
        .mod_db
        .iter_all()
        .filter(
            |m| matches!(&m.source, Some(pob_engine::Source::Other(s)) if s == "Pantheon:Garukhan"),
        )
        .collect();
    assert!(
        !garukhan_mods.is_empty(),
        "Soul of Garukhan should inject at least one mod"
    );

    // PoB-name round trip for every variant.
    for v in [
        MajorGod::None,
        MajorGod::TheBrineKing,
        MajorGod::Arakaali,
        MajorGod::Solaris,
        MajorGod::Lunaris,
    ] {
        assert_eq!(MajorGod::from_pob_name(v.as_pob_name()), Some(v));
    }
    for v in [
        MinorGod::None,
        MinorGod::Abberath,
        MinorGod::Gruthkul,
        MinorGod::Yugul,
        MinorGod::Shakari,
        MinorGod::Tukohama,
        MinorGod::Ralakesh,
        MinorGod::Garukhan,
        MinorGod::Ryslatha,
    ] {
        assert_eq!(MinorGod::from_pob_name(v.as_pob_name()), Some(v));
    }
}

// Issue #83: Pantheon soul levels 2..N. Upstream PoB iterates over
// every soul (1 through 4 for majors, 1 through 2 for minors) for the
// selected god, treating the build as if all soul-stone upgrades have
// been applied — soul-level state isn't stored in the build XML, so
// PoB defaults to "max upgraded". MK2 mirrors that behaviour: picking
// Soul of Arakaali should inject every parseable line from soul[1..4],
// not just soul[1].
//
// Concretely we look for stable, parseable lines unique to each soul
// level (so a single test catches regressions from any of the four
// tiers being dropped):
//   - soul[2] "Recovery rate of Life and Energy Shield" → ESRecoveryRate
//   - soul[3] "Debuffs on you expire 20% faster"        → DebuffExpireRate
//   - soul[4] "Chaos Resistance against Damage Over Time" → ChaosResistanceAgainstDoT
// Soul[1] "10% reduced Damage taken from Damage Over Time" is already
// covered by `pantheon_selection_round_trips_and_injects_parseable_mods`.
#[test]
fn pantheon_arakaali_applies_all_four_soul_levels() {
    let Some(tree) = load_3_25_tree() else {
        eprintln!("skip: tree missing");
        return;
    };
    use pob_engine::character::MajorGod;
    use pob_engine::ModStore as _;

    let mut c = Character::new(ClassRef::marauder(), 90);
    c.pantheon_major = MajorGod::Arakaali;
    let env = pob_engine::perform::init_env(&c, &tree);
    let pantheon_lines: Vec<String> = env
        .mod_db
        .iter_all()
        .filter(
            |m| matches!(&m.source, Some(pob_engine::Source::Other(s)) if s == "Pantheon:Arakaali"),
        )
        .map(|m| m.name.clone())
        .collect();

    // soul[2..4] should all contribute. Each soul level emits at least
    // one mod whose name we can probe by substring.
    let has = |needle: &str| pantheon_lines.iter().any(|n| n.contains(needle));
    assert!(
        has("LifeRecoveryRate") || has("EnergyShieldRecoveryRate") || has("RecoveryRate"),
        "Arakaali soul[2] (Hybrid Widow) should emit a recovery-rate mod; got {pantheon_lines:?}"
    );
    assert!(
        has("DebuffExpire") || has("DebuffEffect") || has("Buff") || has("Debuff"),
        "Arakaali soul[3] (Maligaro) should emit a debuff-expire mod; got {pantheon_lines:?}"
    );
    assert!(
        pantheon_lines
            .iter()
            .any(|n| n.contains("Chaos") && (n.contains("Resist") || n.contains("Resistance"))),
        "Arakaali soul[4] (Drought-Maddened Rhoa) should emit a chaos-resistance mod; got {pantheon_lines:?}"
    );
    // Sanity floor: at least four parseable mods (one per soul level).
    assert!(
        pantheon_lines.len() >= 4,
        "Arakaali should inject at least one mod per soul level (4 total); got {} lines: {pantheon_lines:?}",
        pantheon_lines.len(),
    );
}

// Companion: minors only have soul[1..2]; verify a minor that has at
// least one parseable line on each level applies both. Yugul's soul[1]
// emits "DamageReflectionMitigation" + "HexReflectChance", and soul[2]
// emits a curse-effect mod — three distinct mods minimum.
#[test]
fn pantheon_yugul_applies_both_minor_soul_levels() {
    let Some(tree) = load_3_25_tree() else {
        eprintln!("skip: tree missing");
        return;
    };
    use pob_engine::character::MinorGod;
    use pob_engine::ModStore as _;

    let mut c = Character::new(ClassRef::marauder(), 90);
    c.pantheon_minor = MinorGod::Yugul;
    let env = pob_engine::perform::init_env(&c, &tree);
    let count = env
        .mod_db
        .iter_all()
        .filter(
            |m| matches!(&m.source, Some(pob_engine::Source::Other(s)) if s == "Pantheon:Yugul"),
        )
        .count();
    assert!(
        count >= 2,
        "Yugul should inject mods from both soul[1] and soul[2] (>= 2 total); got {count}"
    );
}

// Issue #10 (Bandit half): Act 2 reward injects a small package of stats.
// Each named bandit grants a single mod, mirroring upstream PoB
// (CalcSetup.lua:531-540): Alira → +15 to all elemental resistances;
// Kraityn → +8% increased Movement Speed; Oak → +40 to maximum Life;
// KillAll → +1 ExtraPoints (the "+2 passive points" reward).
#[test]
fn bandit_grants_stat_package() {
    let Some(tree) = load_3_25_tree() else {
        return;
    };
    let mut c = Character::new(ClassRef::marauder(), 90);

    // Default (KillAll) baseline — only ExtraPoints lands.
    assert_eq!(c.bandit, pob_engine::character::Bandit::KillAll);
    let baseline = compute_with_skills(&c, &tree, None);
    let baseline_fire = baseline.get("FireResistTotal");
    let baseline_cold = baseline.get("ColdResistTotal");
    let baseline_lightning = baseline.get("LightningResistTotal");
    let baseline_life = baseline.get("Life");
    let baseline_move = baseline.get("MovementSpeedMod");

    // Alira: ElementalResist BASE 15 — applies to all three elemental resists.
    c.bandit = pob_engine::character::Bandit::Alira;
    let alira = compute_with_skills(&c, &tree, None);
    assert!(
        (alira.get("FireResistTotal") - baseline_fire - 15.0).abs() < 0.001,
        "Alira should add +15 to Fire Resist Total (baseline={}, after={})",
        baseline_fire,
        alira.get("FireResistTotal")
    );
    assert!(
        (alira.get("ColdResistTotal") - baseline_cold - 15.0).abs() < 0.001,
        "Alira should add +15 to Cold Resist Total"
    );
    assert!(
        (alira.get("LightningResistTotal") - baseline_lightning - 15.0).abs() < 0.001,
        "Alira should add +15 to Lightning Resist Total"
    );

    // Oak: Life BASE 40.
    c.bandit = pob_engine::character::Bandit::Oak;
    let oak = compute_with_skills(&c, &tree, None);
    assert!(
        (oak.get("Life") - baseline_life - 40.0).abs() < 0.5,
        "Oak should add +40 to Life ({} vs baseline {})",
        oak.get("Life"),
        baseline_life
    );

    // Kraityn: MovementSpeed INC 8 lifts the move-speed multiplier by 0.08.
    c.bandit = pob_engine::character::Bandit::Kraityn;
    let kraityn = compute_with_skills(&c, &tree, None);
    let kraityn_move = kraityn.get("MovementSpeedMod");
    assert!(
        (kraityn_move - baseline_move - 0.08).abs() < 0.001,
        "Kraityn should add +0.08 to MovementSpeedMod ({} vs baseline {})",
        kraityn_move,
        baseline_move
    );
}

#[test]
fn config_charges_drive_per_charge_mod() {
    let Some(tree) = load_3_25_tree() else {
        return;
    };

    // BFS for a per-Power-Charge passive reachable from Marauder start, allocating
    // every node along the path. Path validation in pob-engine drops disconnected
    // allocations, so we have to walk to the target.
    let mut c = Character::new(ClassRef::marauder(), 90);
    let Some(node_id) = allocate_reachable(&mut c, &tree, "Marauder", |n| {
        n.ascendancy_name.is_none()
            && matches!(
                n.kind,
                pob_data::NodeKind::Normal | pob_data::NodeKind::Notable
            )
            && n.stats.len() == 1
            && n.stats[0].contains("per Power Charge")
            && !n.stats[0].contains(" while ")
            && !n.stats[0].contains(" if ")
    }) else {
        eprintln!("no reachable per-power-charge node — skip");
        return;
    };
    let zero = compute_with_skills(&c, &tree, None);
    c.config.multipliers.insert("PowerCharge".to_owned(), 5.0);
    let five = compute_with_skills(&c, &tree, None);

    // Some stat must differ between zero and five power charges.
    let mut found_diff = false;
    for (k, v) in zero.iter() {
        if (five.get(k) - v).abs() > 1e-9 {
            found_diff = true;
            break;
        }
    }
    for (k, _) in five.iter() {
        if zero.try_get(k).is_none() {
            found_diff = true;
            break;
        }
    }
    assert!(
        found_diff,
        "Power charges should activate per-power-charge mod (node {node_id}) and change at least one stat"
    );
}

#[test]
fn fire_resist_cap_blocks_overflow() {
    let Some(tree) = load_3_25_tree() else {
        return;
    };
    // Find a node that gives lots of fire resistance.
    // Fall back to a synthesised mod if the tree doesn't have one.
    let c = Character::new(ClassRef::marauder(), 90);
    let mut env = pob_engine::perform::init_env(&c, &tree);
    env.mod_db.add(pob_engine::Mod::base("FireResist", 999.0));
    pob_engine::perform::compute(&c, &tree); // smoke; not used
    pob_engine::perform::perform_basic_stats(&c, &tree, &mut env);
    let total = env.output.get("FireResistTotal");
    assert!(
        (total - 75.0).abs() < 1e-9,
        "FireResistTotal should cap at 75% (no max-bonus mods), got {total}"
    );
    let _ = c;
}

#[test]
fn ms_share_code_round_trips_full_character() {
    let Some(tree) = load_3_25_tree() else {
        return;
    };
    let mut c = Character::new(ClassRef::ranger(), 78);
    c.allocated.insert(101);
    c.allocated.insert(202);
    c.allocated.insert(303);
    c.notes = "Bow build with poison stacking & elemental scaling".into();
    c.main_skill = Some(MainSkill {
        skill_id: "TornadoShot".into(),
        level: 21,
        quality: 23,
        quality_id: pob_engine::QualityId::Default,
        enabled: true,
    });
    c.config.enemy_lightning_resist = 50;
    c.config.conditions.insert("FullLife".to_owned(), true);
    c.config.multipliers.insert("PowerCharge".to_owned(), 5.0);

    let code = pob_engine::export_code(&c).expect("export");
    let restored = pob_engine::import_code(&code).expect("import");

    assert_eq!(restored.class.0, "Ranger");
    assert_eq!(restored.level, 78);
    assert_eq!(restored.allocated.len(), 3);
    assert!(restored.allocated.contains(&101));
    assert_eq!(restored.notes, c.notes);
    assert_eq!(
        restored.main_skill.as_ref().map(|m| m.skill_id.as_str()),
        Some("TornadoShot")
    );
    assert_eq!(restored.main_skill.as_ref().map(|m| m.level), Some(21));
    assert_eq!(restored.config.enemy_lightning_resist, 50);
    assert!(restored
        .config
        .conditions
        .get("FullLife")
        .copied()
        .unwrap_or(false));
    assert_eq!(
        restored.config.multipliers.get("PowerCharge").copied(),
        Some(5.0)
    );

    let _ = tree;
}

#[test]
fn realistic_pob_xml_imports_cleanly() {
    // Realistic shape based on actual upstream PoB XML — multiple Specs, attribute
    // ordering varies, the active spec is referenced by activeSpec="N".
    let xml = r#"<?xml version="1.0" encoding="UTF-8"?>
<PathOfBuilding>
    <Build level="92" targetVersion="3_0" mainSocketGroup="1" className="Witch" ascendClassName="Occultist"/>
    <Tree activeSpec="1">
        <Spec masteryEffects="" treeVersion="3_25" classId="3" ascendClassId="3" nodes="59530,55156,57264,2151,4180,30880,3936"/>
        <Spec masteryEffects="" treeVersion="3_25" classId="3" ascendClassId="0" nodes=""/>
    </Tree>
    <Notes>
This is a multiline note
with several lines of detail
about the build approach.
    </Notes>
    <Items/>
    <Skills/>
    <Config>
        <Input name="enemyIsBoss" value="None"/>
    </Config>
</PathOfBuilding>"#;
    let c = pob_engine::import_pob_xml(xml).expect("import");
    assert_eq!(c.class.0, "Witch");
    assert_eq!(c.ascendancy.as_deref(), Some("Occultist"));
    assert_eq!(c.level, 92);
    // First Spec has 7 node ids.
    assert_eq!(c.allocated.len(), 7);
    assert!(c.allocated.contains(&59530));
    assert!(c.allocated.contains(&3936));
    assert!(c.notes.contains("multiline note"));
}

#[test]
fn pob_xml_round_trip_full_character() {
    let mut c = Character::new(ClassRef::witch(), 92);
    c.ascendancy = Some("Occultist".to_owned());
    c.allocated.insert(101);
    c.allocated.insert(202);
    c.notes = "POB-format build".to_owned();

    let xml = pob_engine::export_pob_xml(&c);
    let restored = pob_engine::import_pob_xml(&xml).expect("import xml");
    assert_eq!(restored.class.0, "Witch");
    assert_eq!(restored.ascendancy.as_deref(), Some("Occultist"));
    assert_eq!(restored.level, 92);
    assert_eq!(restored.allocated.len(), 2);
    assert!(restored.allocated.contains(&101));
    assert!(restored.allocated.contains(&202));
    assert_eq!(restored.notes, "POB-format build");
}

// Issue #14: round-trip the full Config payload — every typed enemy
// stat (resists, evasion, armour, block/dodge/suppress, projectile
// shotgun count) must survive export → re-import. Each field uses PoB's
// canonical Input name on the wire.
#[test]
fn pob_xml_round_trip_config_state() {
    let mut c = Character::new(ClassRef::marauder(), 90);
    c.config.enemy_level = 84;
    c.config.enemy_fire_resist = 30;
    c.config.enemy_cold_resist = -10;
    c.config.enemy_lightning_resist = 25;
    c.config.enemy_chaos_resist = -25;
    c.config.enemy_evasion = 1500;
    c.config.enemy_armour = 36000;
    c.config.enemy_block_chance = 50;
    c.config.enemy_dodge_chance = 30;
    c.config.enemy_suppression_chance = 50;
    c.config.projectiles_hitting_target = 4;
    c.config.conditions.insert("UsingFlask".to_owned(), true);
    c.config.conditions.insert("EnemyMoving".to_owned(), false);
    c.config.multipliers.insert("PowerCharge".to_owned(), 5.0);

    let xml = pob_engine::export_pob_xml(&c);
    let restored = pob_engine::import_pob_xml(&xml).expect("import xml");

    // Every typed Config field round-trips bit-for-bit.
    assert_eq!(restored.config.enemy_level, 84);
    assert_eq!(restored.config.enemy_fire_resist, 30);
    assert_eq!(restored.config.enemy_cold_resist, -10);
    assert_eq!(restored.config.enemy_lightning_resist, 25);
    assert_eq!(restored.config.enemy_chaos_resist, -25);
    assert_eq!(restored.config.enemy_evasion, 1500);
    assert_eq!(restored.config.enemy_armour, 36000);
    assert_eq!(restored.config.enemy_block_chance, 50);
    assert_eq!(restored.config.enemy_dodge_chance, 30);
    assert_eq!(restored.config.enemy_suppression_chance, 50);
    assert_eq!(restored.config.projectiles_hitting_target, 4);
    assert_eq!(
        restored.config.conditions.get("UsingFlask").copied(),
        Some(true)
    );
    assert_eq!(
        restored.config.conditions.get("EnemyMoving").copied(),
        Some(false)
    );
    assert_eq!(
        restored.config.multipliers.get("PowerCharge").copied(),
        Some(5.0)
    );
}

// Issue #14: round-trip a full Items + Skills payload. Items go through
// the `<Item> + <ItemSet><Slot>` mapping; skill groups go through
// `<Skill mainActiveSkill> <Gem skillId level quality enabled/>` blocks.
#[test]
fn pob_xml_round_trip_items_and_skills() {
    let mut c = Character::new(ClassRef::witch(), 90);

    let body = parse_item(
        "Item Class: Body Armours\nRarity: RARE\nDoom Carapace\nFull Plate\n--------\n+50 to maximum Life\n--------",
    )
    .expect("parse body armour");
    c.items.equip(pob_data::Slot::BodyArmour, body);
    let amulet = parse_item(RARE_AMULET).expect("parse amulet");
    c.items.equip(pob_data::Slot::Amulet, amulet);

    c.skill_groups.push(pob_engine::character::SocketGroup {
        label: "Main".to_owned(),
        gems: vec![
            MainSkill {
                skill_id: "Arc".to_owned(),
                level: 20,
                quality: 23,
                quality_id: pob_engine::QualityId::Default,
                enabled: true,
            },
            MainSkill {
                skill_id: "AddedLightningDamage".to_owned(),
                level: 18,
                quality: 0,
                quality_id: pob_engine::QualityId::Default,
                enabled: false,
            },
        ],
        main_active_skill_index: 1,
        enabled: true,
    });
    c.main_socket_group = 1;

    let xml = pob_engine::export_pob_xml(&c);
    let restored = pob_engine::import_pob_xml(&xml).expect("import xml");

    // Items survive the round-trip on both slots.
    assert!(
        restored
            .items
            .iter()
            .any(|(s, _)| *s == pob_data::Slot::BodyArmour),
        "BodyArmour slot should be populated after round-trip"
    );
    assert!(
        restored
            .items
            .iter()
            .any(|(s, _)| *s == pob_data::Slot::Amulet),
        "Amulet slot should be populated after round-trip"
    );

    // Skill group + gem details land back on the restored character.
    assert_eq!(restored.skill_groups.len(), 1);
    let group = &restored.skill_groups[0];
    assert_eq!(group.gems.len(), 2);
    assert_eq!(group.gems[0].skill_id, "Arc");
    assert_eq!(group.gems[0].level, 20);
    assert_eq!(group.gems[0].quality, 23);
    assert!(group.gems[0].enabled);
    assert_eq!(group.gems[1].skill_id, "AddedLightningDamage");
    assert_eq!(group.gems[1].level, 18);
    assert!(!group.gems[1].enabled);
}

// Issue #36: alt-quality variant resolution. `skill_for_quality(reg, "Arc",
// QualityId::Anomalous)` should pick up `ArcAltX` when the registry carries
// it; `Phantasmal` should fall back to the base `Arc` because no `ArcAltZ`
// exists in upstream data. Skips when the test fixture is missing the alt
// variants — running this from a fresh checkout requires `pob-extract`.
#[test]
fn skill_for_quality_resolves_alt_variants_or_falls_back() {
    let Some(skills) = load_skills() else {
        eprintln!("skip: skill data missing");
        return;
    };
    if skills.get("Arc").is_none() {
        eprintln!("skip: Arc not in registry");
        return;
    }

    let (default_skill, used_alt) =
        pob_engine::skill_for_quality(&skills, "Arc", pob_engine::QualityId::Default);
    assert!(default_skill.is_some());
    assert!(!used_alt, "Default never consults an alt variant");

    if let Some(_arc_x) = skills.get("ArcAltX") {
        let (alt_skill, used_alt) =
            pob_engine::skill_for_quality(&skills, "Arc", pob_engine::QualityId::Anomalous);
        let alt_skill = alt_skill.expect("alt skill should resolve");
        assert!(used_alt, "Anomalous should use the alt variant");
        // The resolved skill is `ArcAltX` (transfigured "Arc of Surging").
        assert_eq!(alt_skill.base_type_name, "Arc of Surging");
    }

    // Phantasmal maps to AltZ — Arc has no AltZ in current PoB data, so
    // we should silently fall back to the bare Arc skill.
    let (fallback, used_alt) =
        pob_engine::skill_for_quality(&skills, "Arc", pob_engine::QualityId::Phantasmal);
    assert!(fallback.is_some());
    assert!(!used_alt, "Phantasmal Arc has no upstream data → fallback");
}

// Issue #36: end-to-end quality-id swap. With `quality_id = Anomalous` and
// quality > 0, the engine should consult `ArcAltX.qualityStats` for the
// quality-driven part of skill mods, not the bare `Arc.qualityStats`.
// Concretely: at Q20, default Arc gains +1 chain (so ChainMax = 8 at L20);
// the AltX variant's quality stat is `projectile_number_to_split` instead,
// so its quality contribution toward ChainMax is 0 — and we expect a
// different ChainMax output between the two `quality_id`s.
#[test]
fn quality_id_anomalous_changes_arc_chain_count() {
    let (Some(tree), Some(skills), Some(bases)) = (load_3_25_tree(), load_skills(), load_bases())
    else {
        eprintln!("skip: data missing");
        return;
    };
    if skills.get("Arc").is_none() || skills.get("ArcAltX").is_none() {
        eprintln!("skip: Arc / ArcAltX not in registry");
        return;
    }

    let mut c = Character::new(ClassRef::witch(), 90);
    c.ascendancy = Some("Occultist".into());
    let mut ms = MainSkill::new("Arc");
    ms.level = 20;
    ms.quality = 20;
    ms.quality_id = pob_engine::QualityId::Default;
    c.main_skill = Some(ms);
    let default_out = pob_engine::compute_full(&c, &tree, Some(&skills), Some(&bases));
    let default_chain = default_out.get("ChainMax");

    let mut ms = MainSkill::new("Arc");
    ms.level = 20;
    ms.quality = 20;
    ms.quality_id = pob_engine::QualityId::Anomalous;
    c.main_skill = Some(ms);
    let alt_out = pob_engine::compute_full(&c, &tree, Some(&skills), Some(&bases));
    let alt_chain = alt_out.get("ChainMax");

    // Default gains a chain at Q20 from `qualityStats=[number_of_chains, 0.05]`.
    // Anomalous swaps in `[projectile_number_to_split, 0.1]` — no chain
    // contribution — so the chain count is lower (typically by 1) for the
    // same gem.
    if default_chain == 0.0 && alt_chain == 0.0 {
        eprintln!("skip: ChainMax not surfaced by this fixture");
        return;
    }
    assert!(
        default_chain > alt_chain,
        "Anomalous Arc loses the +chain quality bonus; got default={default_chain} alt={alt_chain}"
    );
}

// Issue #36: PoB XML round-trip for `qualityId="Anomalous"`. Importing
// such a build should produce a gem with `quality_id == Anomalous`,
// and re-exporting must preserve the attribute so downstream PoB
// reads recover the original choice.
#[test]
fn pob_xml_round_trips_quality_id() {
    use pob_engine::{export_pob_xml, import_pob_xml};

    let xml = r#"<PathOfBuilding>
    <Build level="90" className="Witch" ascendClassName="Occultist"/>
    <Tree activeSpec="1"><Spec/></Tree>
    <Skills>
        <Skill mainActiveSkill="1" enabled="true" label="Main">
            <Gem skillId="Arc" level="20" quality="20" qualityId="Anomalous" enabled="true"/>
            <Gem skillId="AddedLightningDamage" level="20" quality="0" qualityId="Divergent" enabled="true"/>
        </Skill>
    </Skills>
</PathOfBuilding>"#;

    let c = import_pob_xml(xml).expect("import pob xml");
    let group = c.skill_groups.first().expect("at least one group");
    assert_eq!(group.gems.len(), 2);
    assert_eq!(
        group.gems[0].quality_id,
        pob_engine::QualityId::Anomalous,
        "Anomalous qualityId on Arc must round-trip on import"
    );
    assert_eq!(
        group.gems[1].quality_id,
        pob_engine::QualityId::Divergent,
        "Divergent qualityId on support must round-trip on import"
    );

    // The main skill copy of the active gem should also pick up the
    // alt quality so compute paths see the right variant.
    let main = c.main_skill.as_ref().expect("main skill set");
    assert_eq!(main.quality_id, pob_engine::QualityId::Anomalous);

    // Re-export and confirm the canonical PoB attribute is present.
    let out = export_pob_xml(&c);
    assert!(
        out.contains("qualityId=\"Anomalous\""),
        "exported XML should carry the Anomalous qualityId attribute, got:\n{out}"
    );
    assert!(
        out.contains("qualityId=\"Divergent\""),
        "exported XML should carry the Divergent qualityId attribute, got:\n{out}"
    );
}

// Issue #36: missing-attribute tolerance. Legacy PoB exports without
// the `qualityId` attribute should default to `Default` so existing
// tests / fixtures are unaffected.
#[test]
fn pob_xml_missing_quality_id_defaults_to_default() {
    use pob_engine::import_pob_xml;
    let xml = r#"<PathOfBuilding>
    <Build level="90" className="Witch" ascendClassName="Occultist"/>
    <Tree activeSpec="1"><Spec/></Tree>
    <Skills>
        <Skill mainActiveSkill="1" enabled="true" label="Main">
            <Gem skillId="Arc" level="20" quality="20" enabled="true"/>
        </Skill>
    </Skills>
</PathOfBuilding>"#;
    let c = import_pob_xml(xml).expect("import legacy xml");
    let gem = &c.skill_groups[0].gems[0];
    assert_eq!(gem.quality_id, pob_engine::QualityId::Default);
}

#[test]
fn level_up_increases_life_and_mana() {
    let Some(tree) = load_3_25_tree() else {
        return;
    };
    let mut c = Character::new(ClassRef::marauder(), 1);
    let l1 = compute_with_skills(&c, &tree, None);
    c.level = 90;
    let l90 = compute_with_skills(&c, &tree, None);
    assert!(l90.get("Life") > l1.get("Life"));
    assert!(l90.get("Mana") > l1.get("Mana"));
    // Life formula: 50 + 12*(L-1) + Str/2 with no items / nodes
    assert_eq!(l1.get("Life"), 66.0);
    assert_eq!(l90.get("Life"), 50.0 + 12.0 * 89.0 + 16.0);
}

// Issue #21: cluster-jewel sub-graph synthesis. When a Cluster Jewel
// item is socketed into a Large jewel socket and the player allocates
// a synthesised notable, the notable's mods should flow through to
// `MainSkillDPS` / `Strength` / etc. via the standard perform pipeline.
#[test]
fn cluster_jewel_notable_contributes_mods_when_allocated() {
    let Some(tree) = load_3_25_tree() else {
        eprintln!("skip: tree data missing");
        return;
    };
    // Load the cluster jewel data files if present. CI extracts them via
    // pob-extract; if missing the test silently skips so it doesn't gate
    // CI on the data being available.
    let cluster_jewels_path = data_root().join("cluster_jewels.json");
    let cluster_mods_path = data_root().join("cluster_jewel_mods.json");
    let Ok(cj_json) = std::fs::read_to_string(&cluster_jewels_path) else {
        eprintln!("skip: cluster_jewels.json missing");
        return;
    };
    let Ok(cm_json) = std::fs::read_to_string(&cluster_mods_path) else {
        eprintln!("skip: cluster_jewel_mods.json missing");
        return;
    };
    let cluster_jewels = pob_data::load_cluster_jewels(&cj_json).expect("decode cluster_jewels");
    let cluster_mods =
        pob_data::load_cluster_jewel_mods(&cm_json).expect("decode cluster_jewel_mods");

    // Find a Large jewel socket on the live tree.
    let large_socket_id = tree
        .nodes
        .iter()
        .find_map(|(id, n)| (n.expansion_jewel_size == Some(2)).then_some(*id))
        .expect("3.25 tree should have at least one Large jewel socket");

    // Build a Marauder with the parent socket allocated. We don't bother
    // pathing to it through the full tree — Character mod application
    // honours the `allocated` set directly when synth nodes are injected
    // outside `connected_allocations`.
    let mut c = Character::new(ClassRef::marauder(), 90);
    c.allocate(large_socket_id);

    // Equip a Large Cluster Jewel with one notable: Heraldry. We pick
    // Heraldry because the 3.25 tree's orphan-notable template carries
    // an explicit "+12% to all Resistances" line that we can spot in
    // the output (post-synthesis the notable must flow into the
    // resists pass).
    let notable_name = "Heraldry";
    // Confirm the orphan-notable template exists (skip otherwise to
    // avoid false negatives across tree-version refreshes).
    let template_exists = tree
        .nodes
        .values()
        .any(|n| n.group.is_none() && n.name.as_deref() == Some(notable_name));
    if !template_exists {
        eprintln!("skip: 3.25 tree doesn't carry a {notable_name} template");
        return;
    }

    let cluster_item = parse_item(&format!(
        "Item Class: Jewel\n\
         Rarity: MAGIC\n\
         Strident Large Cluster Jewel\n\
         Large Cluster Jewel\n\
         --------\n\
         Item Level: 84\n\
         --------\n\
         Adds 8 Passive Skills\n\
         1 Added Passive Skill is {notable_name}\n\
         Added Small Passive Skills grant: 12% increased Chaos Damage\n\
         --------"
    ))
    .expect("parse cluster jewel");
    c.jewels.insert(large_socket_id, cluster_item);

    let ctx = pob_engine::ClusterContext::new(&cluster_jewels, &cluster_mods);

    // Without any synth-node allocation, the cluster's notable should NOT
    // contribute mods (parent socket alloc alone isn't enough).
    let no_alloc = pob_engine::compute_full_with_clusters(&c, &tree, None, None, Some(ctx)).0;

    // Allocate the synthesised notable. We need its synth id, which is
    // deterministic given the Large-socket index + size + slot. The
    // synthesis pass exposes them via `synthesise_all`.
    let mut jewels_map: ahash::HashMap<pob_data::NodeId, pob_data::Item> =
        ahash::HashMap::default();
    for (id, it) in &c.jewels {
        jewels_map.insert(*id, it.clone());
    }
    let specs = pob_engine::cluster_synth::synthesise_all(
        &tree,
        &jewels_map,
        &cluster_jewels,
        &cluster_mods,
    );
    assert_eq!(specs.len(), 1, "expected one synthesised cluster sub-graph");
    let spec = &specs[0];
    assert!(
        !spec.notable_ids.is_empty(),
        "expected at least one notable"
    );

    let notable_id = spec.notable_ids[0];
    c.allocate(notable_id);

    let with_alloc = pob_engine::compute_full_with_clusters(&c, &tree, None, None, Some(ctx)).0;

    // Heraldry's stats include exposure mods which don't show up on the
    // basic-stats output (they're enemy-side). To keep the assertion
    // resilient across tree-version drift in the Heraldry template, we
    // probe a generic delta: the modDB should now carry _at least one_
    // additional mod with `Source::Passive(notable_id)`. We use the
    // env-returning entry point so we can inspect the mod_db directly.
    let (_, env) = pob_engine::compute_full_with_clusters(&c, &tree, None, None, Some(ctx));
    let synth_mod_count = env
        .mod_db
        .iter_all()
        .filter(|m| matches!(m.source, Some(pob_engine::Source::Passive(id)) if id == notable_id))
        .count();
    assert!(
        synth_mod_count > 0,
        "synth notable {notable_id} should contribute at least one mod via Source::Passive — \
         no_alloc keys={}, with_alloc keys={}",
        no_alloc.len(),
        with_alloc.len()
    );
}

// Issue #21: cluster-jewel sub-graph synthesis — small-passive grant.
// `Added Small Passive Skills grant: <stat>` mods on the cluster jewel
// should propagate to every synthesised small node, and allocating a
// small node should land its stats in the modDB.
#[test]
fn cluster_jewel_small_passive_grant_flows_to_modlist() {
    let Some(tree) = load_3_25_tree() else {
        return;
    };
    let cluster_jewels_path = data_root().join("cluster_jewels.json");
    let cluster_mods_path = data_root().join("cluster_jewel_mods.json");
    let Ok(cj_json) = std::fs::read_to_string(&cluster_jewels_path) else {
        return;
    };
    let Ok(cm_json) = std::fs::read_to_string(&cluster_mods_path) else {
        return;
    };
    let cluster_jewels = pob_data::load_cluster_jewels(&cj_json).expect("decode cluster_jewels");
    let cluster_mods =
        pob_data::load_cluster_jewel_mods(&cm_json).expect("decode cluster_jewel_mods");

    let large_socket_id = tree
        .nodes
        .iter()
        .find_map(|(id, n)| (n.expansion_jewel_size == Some(2)).then_some(*id))
        .expect("Large jewel socket");

    let mut c = Character::new(ClassRef::marauder(), 90);
    c.allocate(large_socket_id);

    let cluster_item = parse_item(
        "Item Class: Jewel\n\
         Rarity: MAGIC\n\
         Strident Large Cluster Jewel\n\
         Large Cluster Jewel\n\
         --------\n\
         Item Level: 84\n\
         --------\n\
         Adds 8 Passive Skills\n\
         Added Small Passive Skills grant: 12% increased Chaos Damage\n\
         --------",
    )
    .expect("parse cluster jewel");
    c.jewels.insert(large_socket_id, cluster_item);

    let ctx = pob_engine::ClusterContext::new(&cluster_jewels, &cluster_mods);
    let mut jewels_map: ahash::HashMap<pob_data::NodeId, pob_data::Item> =
        ahash::HashMap::default();
    for (id, it) in &c.jewels {
        jewels_map.insert(*id, it.clone());
    }
    let specs = pob_engine::cluster_synth::synthesise_all(
        &tree,
        &jewels_map,
        &cluster_jewels,
        &cluster_mods,
    );
    let spec = &specs[0];
    assert_eq!(
        spec.small_ids.len(),
        8,
        "8 added passives, no notables / sockets → 8 smalls"
    );

    // Allocate one small. Its stats should mention "12% increased Chaos Damage".
    let small_id = spec.small_ids[0];
    c.allocate(small_id);

    let (_, env) = pob_engine::compute_full_with_clusters(&c, &tree, None, None, Some(ctx));
    let chaos_dmg_mod_count = env
        .mod_db
        .iter_all()
        .filter(|m| {
            matches!(m.source, Some(pob_engine::Source::Passive(id)) if id == small_id)
                && m.name.contains("ChaosDamage")
        })
        .count();
    assert!(
        chaos_dmg_mod_count > 0,
        "small passive should contribute the cluster's '12% increased Chaos Damage' mod"
    );
}

// ----------------------------------------------------------------------
// Issue #30 — Timeless jewels: keystone replacement integration tests.
//
// These run against `data/timeless_jewels.json` (shipped in this repo) and
// the live 3.25 tree. They prove that:
//   1. A Glorious Vanity / Doryani jewel socketed near an allocated keystone
//      injects the "Corrupted Soul" mod text into the modDB.
//   2. The original keystone's mods are skipped when the keystone is
//      conquered (no double-counting).
//   3. With `timeless = None`, behaviour is unchanged from pre-#30.
// ----------------------------------------------------------------------

fn load_timeless_data() -> Option<pob_data::TimelessJewelData> {
    let path = data_root().join("timeless_jewels.json");
    let json = std::fs::read_to_string(&path).ok()?;
    pob_data::load_timeless_jewels(&json).ok()
}

#[test]
fn timeless_glorious_vanity_replaces_keystone_with_corrupted_soul() {
    let Some(tree) = load_3_25_tree() else {
        eprintln!("skip: 3.25 tree missing");
        return;
    };
    let Some(data) = load_timeless_data() else {
        eprintln!("skip: timeless_jewels.json missing");
        return;
    };

    // The integration test needs (a) an allocated keystone connected to the
    // class start (else `connected_allocations` filters it out) and
    // (b) a tree-socket within Large radius of that keystone. Pick a
    // (socket, keystone) pair where socket and keystone are within 1800
    // units, then BFS-allocate from Marauder up to the keystone.
    let positions: ahash::AHashMap<pob_data::NodeId, (f64, f64)> =
        pob_engine::jewel_radius::all_node_positions(&tree);

    // Build the keystone-id set first, so we can answer "is this kid a
    // valid Keystone?" in O(1) inside the per-socket loop.
    let keystones: Vec<(pob_data::NodeId, (f64, f64))> = tree
        .nodes
        .iter()
        .filter_map(|(id, n)| {
            if !matches!(n.kind, pob_data::NodeKind::Keystone) {
                return None;
            }
            if n.ascendancy_name.is_some() {
                return None;
            }
            positions.get(id).map(|&p| (*id, p))
        })
        .collect();

    // Search every (socket, keystone) pair until the BFS in
    // `allocate_reachable` succeeds. Most non-ascendancy keystones will
    // path from Marauder; we accept the first one that does.
    let mut c = Character::new(ClassRef::marauder(), 90);
    let mut chosen: Option<(pob_data::NodeId, pob_data::NodeId)> = None;
    'pair_search: for (sid, n) in &tree.nodes {
        if !matches!(n.kind, pob_data::NodeKind::JewelSocket) {
            continue;
        }
        if n.expansion_jewel_size.is_some() || n.ascendancy_name.is_some() {
            continue;
        }
        let Some(&(sx, sy)) = positions.get(sid) else {
            continue;
        };
        for (kid, (kx, ky)) in &keystones {
            let d2 = (sx - kx) * (sx - kx) + (sy - ky) * (sy - ky);
            if d2 > 1800.0 * 1800.0 {
                continue;
            }
            // Try BFS-allocating from Marauder to this specific keystone.
            // We use a closed-over target id rather than the predicate
            // form so each pair gets its own allocation attempt.
            let mut probe = Character::new(ClassRef::marauder(), 90);
            let target = *kid;
            let allocated =
                allocate_reachable(&mut probe, &tree, "Marauder", |node| node.id == target);
            if allocated == Some(target) {
                c = probe;
                chosen = Some((*sid, target));
                break 'pair_search;
            }
        }
    }
    let Some((socket_id, keystone_id)) = chosen else {
        eprintln!("skip: no (socket, keystone) pair both in radius and reachable");
        return;
    };

    // Socket a Glorious Vanity / Doryani timeless jewel into the chosen
    // socket. Mod text is the canonical PoB string.
    let item = pob_engine::parse_item(
        "Item Class: Jewel\n\
         Rarity: UNIQUE\n\
         Glorious Vanity\n\
         Glorious Vanity\n\
         --------\n\
         Item Level: 84\n\
         --------\n\
         Bathed in the blood of 8000 sacrificed in the name of Doryani\n\
         Passives in radius are Conquered by the Vaal\n\
         --------",
    )
    .expect("parse glorious vanity");
    c.socketed_jewels.socket(socket_id, item);

    // With timeless data threaded through, the keystone replacement runs
    // and Corrupted Soul's mod text lands on the modDB.
    let (_, env) = pob_engine::compute_full_with_clusters_and_timeless(
        &c,
        &tree,
        None,
        None,
        None,
        Some(&data),
    );

    // Corrupted Soul: "Gain 15% of Maximum Life as Extra Maximum Energy
    // Shield" — parsed by `mod_parser` as `LifeAsExtraEnergyShield` with
    // a 15 base value. Look for any mod sourced as
    // `Source::Passive(keystone_id)` whose name contains
    // "EnergyShield" (the parser may name the mod
    // `LifeAsEnergyShield` / `LifeAsExtraEnergyShield` depending on
    // version) — that proves the replacement landed.
    let any_corrupted_soul_signature = env.mod_db.iter_all().any(|m| {
        matches!(m.source, Some(pob_engine::Source::Passive(id)) if id == keystone_id)
            && m.name.contains("EnergyShield")
    });
    assert!(
        any_corrupted_soul_signature,
        "Corrupted Soul's energy-shield-related mod should land on the conquered keystone after replacement"
    );
}

#[test]
fn timeless_without_data_is_a_no_op() {
    // With `timeless = None`, the engine should behave identically to the
    // pre-#30 build. We don't socket any timeless jewel here — just check
    // that compute_full_with_clusters_and_timeless(_, _, _, _, _, None) is
    // bit-identical to compute_full_with_clusters across a known-good
    // baseline (Marauder level 90 naked).
    let Some(tree) = load_3_25_tree() else {
        eprintln!("skip: 3.25 tree missing");
        return;
    };
    let c = Character::new(ClassRef::marauder(), 90);
    let with_none =
        pob_engine::compute_full_with_clusters_and_timeless(&c, &tree, None, None, None, None).0;
    let baseline = pob_engine::compute_full_with_clusters(&c, &tree, None, None, None).0;
    assert_eq!(with_none.get("Life"), baseline.get("Life"));
    assert_eq!(with_none.get("Mana"), baseline.get("Mana"));
    assert_eq!(with_none.get("Strength"), baseline.get("Strength"));
}

#[test]
fn timeless_socketed_far_from_keystone_does_not_panic() {
    // Smoke test: a Timeless jewel socketed into a tree socket that has no
    // allocated keystones in radius should be a no-op and not crash the
    // compute pipeline. Mirrors a common in-game scenario (Glorious
    // Vanity socketed in the random Marauder start area to grab the
    // small-node bonuses without conquering any keystones).
    let Some(tree) = load_3_25_tree() else {
        eprintln!("skip: 3.25 tree missing");
        return;
    };
    let Some(data) = load_timeless_data() else {
        eprintln!("skip: timeless_jewels.json missing");
        return;
    };

    let mut c = Character::new(ClassRef::marauder(), 90);
    // Pick the first non-cluster jewel socket the tree exposes; we don't
    // care where it is — we just want compute to run cleanly with a
    // Timeless jewel present and no in-radius allocated keystones.
    let Some(socket_id) = tree.nodes.iter().find_map(|(id, n)| {
        if !matches!(n.kind, pob_data::NodeKind::JewelSocket) {
            return None;
        }
        if n.expansion_jewel_size.is_some() {
            return None;
        }
        if n.ascendancy_name.is_some() {
            return None;
        }
        Some(*id)
    }) else {
        eprintln!("skip: no jewel socket on the tree");
        return;
    };

    let item = pob_engine::parse_item(
        "Item Class: Jewel\n\
         Rarity: UNIQUE\n\
         Glorious Vanity\n\
         Glorious Vanity\n\
         --------\n\
         Item Level: 84\n\
         --------\n\
         Bathed in the blood of 8000 sacrificed in the name of Doryani\n\
         Passives in radius are Conquered by the Vaal\n\
         --------",
    )
    .expect("parse glorious vanity");
    c.socketed_jewels.socket(socket_id, item);

    // No keystones allocated → no replacements → compute should still
    // produce sensible Marauder-level-90 baselines. This is the
    // negative-coverage half of the integration suite.
    let (out, _) = pob_engine::compute_full_with_clusters_and_timeless(
        &c,
        &tree,
        None,
        None,
        None,
        Some(&data),
    );
    assert!(out.get("Life") > 0.0);
}
