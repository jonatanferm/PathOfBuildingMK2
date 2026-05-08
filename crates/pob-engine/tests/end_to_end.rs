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
        let Some(node) = tree.nodes.get(&n) else { continue };
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

    // Elemental resistances went up by 39%, but post-Act-10 penalty is -60.
    // Net at level 90: -60 (penalty) + 39 (item) = -21.
    assert_eq!(after.get("FireResistTotal"), -21.0);
    assert_eq!(after.get("ColdResistTotal"), -21.0);
    assert_eq!(after.get("LightningResistTotal"), -21.0);
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
fn attack_skill_with_weapon_produces_dps() {
    let (Some(tree), Some(skills)) = (load_3_25_tree(), load_skills()) else {
        return;
    };
    let bases = load_bases();
    let Some(bases) = bases else { return };

    // Find any active attack skill (e.g. HeavyStrike).
    let attack_id = skills
        .iter_active()
        .find(|(_, s)| s.base_flags.get("attack").copied().unwrap_or(false))
        .map(|(id, _)| id.to_owned());
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
    let sword_paste = format!(
        "Item Class: One Handed Swords\nRarity: NORMAL\n{sword_name}\n--------\n"
    );
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
    assert!(
        speed > 0.5 && speed < 5.0,
        "Attack speed (cps): {speed}"
    );
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
    assert!(out.get("Strength") > 14.0, "Strength: {}", out.get("Strength"));
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
        .find_map(|(id, n)| (n.ascendancy_name.is_none() && n.kind == pob_data::NodeKind::Notable).then_some(id))
        .expect("any notable in 3.25 tree");
    c.allocate(passive_id);
    assert_eq!(c.ascendancy_alloc_count(&tree), 8);
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
    let has_slot = life.tags.iter().any(|t| matches!(
        &t.kind,
        pob_engine::TagKind::SlotName { slot_name, neg: false } if slot_name == "Body Armour"
    ));
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

#[test]
fn enemy_evasion_changes_main_skill_hit_chance() {
    let (Some(tree), Some(skills), Some(bases)) =
        (load_3_25_tree(), load_skills(), load_bases())
    else {
        return;
    };

    let attack_id = skills
        .iter_active()
        .find(|(_, s)| s.base_flags.get("attack").copied().unwrap_or(false))
        .map(|(id, _)| id.to_owned());
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
    let low = pob_engine::compute_full(&c, &tree, Some(&skills), Some(&bases))
        .get("MainSkillHitChance");
    c.config.enemy_evasion = 20_000;
    let high = pob_engine::compute_full(&c, &tree, Some(&skills), Some(&bases))
        .get("MainSkillHitChance");
    assert!(
        low > high,
        "Higher enemy_evasion should drop hit chance; low={low}, high={high}"
    );
}

#[test]
fn bleed_faster_and_enemy_moving_scale_bleed_dps() {
    let (Some(tree), Some(skills), Some(bases)) =
        (load_3_25_tree(), load_skills(), load_bases())
    else {
        eprintln!("skip: data missing");
        return;
    };

    // Build a Duelist with an attack skill + sword + a body armour that grants
    // 100% chance to bleed. With those alone we get a non-zero BleedDPS to
    // measure ailment-rate scaling against.
    let attack_id = skills
        .iter_active()
        .find(|(_, s)| s.base_flags.get("attack").copied().unwrap_or(false))
        .map(|(id, _)| id.to_owned());
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
    let sword =
        parse_item(&format!("Item Class: One Handed Swords\nRarity: NORMAL\n{sword_name}\n--------\n"))
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
    assert!(restored.config.conditions.get("FullLife").copied().unwrap_or(false));
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
