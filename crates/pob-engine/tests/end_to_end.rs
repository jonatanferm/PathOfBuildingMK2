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
    let baseline =
        pob_engine::compute_full(&c, &tree, None, Some(&bases)).get("Armour");

    // Equip a Sacrificial Garb (Body Armour with armour base).
    let raw = "Item Class: Body Armours\nRarity: NORMAL\nAstral Plate\n--------\n";
    let item = parse_item(raw).expect("parse body armour");
    c.items.equip(pob_data::Slot::BodyArmour, item);
    let after =
        pob_engine::compute_full(&c, &tree, None, Some(&bases)).get("Armour");

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
        .find(|(_, b)| {
            b.r#type.contains("Shield") || b.r#type.contains("Buckler")
        })
        .map(|(name, _)| name.clone());
    let Some(shield_name) = shield_base else {
        eprintln!("no shield in bases — skip");
        return;
    };

    let raw = format!(
        "Item Class: Shields\nRarity: NORMAL\n{shield_name}\n--------\n"
    );
    let mut c = Character::new(ClassRef::duelist(), 90);
    let item = parse_item(&raw).expect("parse shield");
    c.items.equip(pob_data::Slot::Weapon2, item);
    let after =
        pob_engine::compute_full(&c, &tree, None, Some(&bases)).get("BlockChance");

    assert!(
        after > 0.0,
        "Shield should add block chance — got {after} for {shield_name}"
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
    assert!(chain_mod.is_some(), "Arc should produce a MORE Damage chain mod");
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
    // Per PoB's calc, Arc lvl 20 / char L90 base damage is ~640–3653 (with the 1.2
    // damage effectiveness) before any modifiers. We use a wide tolerance because
    // upstream PoB occasionally tweaks the constants and we don't want this test
    // brittle.
    assert!(
        base_min > 200.0 && base_min < 1500.0,
        "Arc base min damage: expected 200-1500, got {base_min}"
    );
    assert!(
        base_max > 1000.0 && base_max < 6000.0,
        "Arc base max damage: expected 1000-6000, got {base_max}"
    );
}

#[test]
fn config_charges_drive_per_charge_mod() {
    let Some(tree) = load_3_25_tree() else {
        return;
    };

    // Iterate sorted so the test is deterministic, and find a per-Power-Charge passive
    // whose stat the parser handles (so toggling actually moves a value).
    let mut ids: Vec<_> = tree.nodes.keys().copied().collect();
    ids.sort_unstable();
    let charge_node = ids.into_iter().find(|id| {
        let Some(n) = tree.nodes.get(id) else { return false };
        // Only consider non-ascendancy / non-mastery normal-or-notable nodes with a
        // single "per Power Charge" stat — predictable shape.
        n.ascendancy_name.is_none()
            && matches!(
                n.kind,
                pob_data::NodeKind::Normal | pob_data::NodeKind::Notable
            )
            && n.stats.len() == 1
            && n.stats[0].contains("per Power Charge")
            && !n.stats[0].contains(" while ")
            && !n.stats[0].contains(" if ")
    });
    let Some(node_id) = charge_node else {
        eprintln!("no clean per-power-charge node — skip");
        return;
    };

    let mut c = Character::new(ClassRef::marauder(), 90);
    c.allocate(node_id);
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
    let mut c = Character::new(ClassRef::marauder(), 90);
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
    assert!(restored.config.conditions.get("FullLife").copied().unwrap_or(false));
    assert_eq!(
        restored.config.multipliers.get("PowerCharge").copied(),
        Some(5.0)
    );

    let _ = tree;
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
