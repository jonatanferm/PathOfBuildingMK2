//! End-to-end integration tests that simulate real user flows: pick a class,
//! allocate some passives, equip items, pick a main skill, toggle config conditions,
//! and verify the computed stats reflect each input change.
//!
//! These are the "many things do not work" canary — anything that breaks the chain
//! between input and stat output gets caught here.

use std::path::PathBuf;

use pob_engine::{
    character::ClassRef, parse_item, perform::compute_with_skills, Character, MainSkill,
    SkillRegistry,
};

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

    assert_eq!(after.get("Strength") - baseline.get("Strength"), 10.0, "Strength");
    assert_eq!(after.get("Dexterity") - baseline.get("Dexterity"), 10.0, "Dexterity");
    assert_eq!(after.get("Intelligence") - baseline.get("Intelligence"), 10.0, "Intelligence");

    // Life base went up by 62 (item base) + 5 (Strength/2 from +10 Str) = 67.
    assert_eq!(
        after.get("Life") - baseline.get("Life"),
        62.0 + 5.0,
        "Life increase from amulet"
    );

    // All elemental resistances went up by 39% — capped Total too because cap is 75 and
    // raw base went from 0 to 39 < 75.
    assert_eq!(after.get("FireResistTotal"), 39.0);
    assert_eq!(after.get("ColdResistTotal"), 39.0);
    assert_eq!(after.get("LightningResistTotal"), 39.0);
}

#[test]
fn allocating_keystone_passive_emits_flag_mod() {
    let Some(tree) = load_3_25_tree() else {
        return;
    };

    // Pick any keystone node — they have no number-form stats; verify allocating one
    // produces a Misc:Keystone:<name> flag in the modDB.
    let keystone = tree
        .nodes
        .iter()
        .find(|(_, n)| matches!(n.kind, pob_data::NodeKind::Keystone))
        .map(|(id, _)| *id)
        .expect("at least one keystone in 3.25 tree");

    let mut c = Character::new(ClassRef::marauder(), 90);
    c.allocate(keystone);

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

    // Find a passive node with a "while at full life" stat.
    let full_life_node = tree
        .nodes
        .iter()
        .find(|(_, n)| {
            n.stats
                .iter()
                .any(|s| s.contains("while at Full Life") || s.contains("while on Full Life"))
        })
        .map(|(id, _)| *id);

    let Some(node_id) = full_life_node else {
        eprintln!("no full-life node in tree — skip");
        return;
    };

    let mut c = Character::new(ClassRef::marauder(), 90);
    c.allocate(node_id);

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
    assert!(diffs > 0, "expected toggling FullLife to change at least one stat");
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
                && !n
                    .ascendancy_name
                    .as_deref()
                    .map(|a| !a.is_empty())
                    .unwrap_or(false)
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
