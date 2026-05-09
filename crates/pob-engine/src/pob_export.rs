//! Write a `Character` as a Path of Building Community-compatible XML document.
//!
//! Inverse of [`crate::pob_import`]. Produces a document PoB can open: a
//! `<PathOfBuilding>` root with `<Build>`, `<Tree>` (with `<Spec>`), `<Notes>`,
//! `<Items>` (one `<Item>` per equipped slot + an `<ItemSet>` mapping), `<Skills>`
//! (one `<Skill>` per socket group with nested `<Gem>`s), and `<Config>` (one
//! `<Input>` per condition / multiplier / typed enemy field).

use std::fmt::Write;

use crate::character::{Character, ConfigState};
use crate::pob_import::pob_slot_to_name;

pub fn export_pob_xml(character: &Character) -> String {
    let class = xml_escape(&character.class.0);
    let ascendancy = character
        .ascendancy
        .as_deref()
        .filter(|s| !s.is_empty())
        .map_or_else(|| "None".to_owned(), xml_escape);
    let class_id = class_name_to_id(&character.class.0);

    let mut nodes_str = String::new();
    let mut sorted: Vec<_> = character.allocated.iter().copied().collect();
    sorted.sort_unstable();
    for (i, id) in sorted.iter().enumerate() {
        if i > 0 {
            nodes_str.push(',');
        }
        nodes_str.push_str(&id.to_string());
    }

    let notes = xml_escape(&character.notes);

    let mut out = String::new();
    out.push_str("<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<PathOfBuilding>\n");
    let _ = writeln!(
        out,
        "    <Build level=\"{level}\" targetVersion=\"3_0\" className=\"{class}\" ascendClassName=\"{asc}\" bandit=\"{bandit}\" mainSocketGroup=\"{msg}\"/>",
        level = character.level.max(1),
        class = class,
        asc = ascendancy,
        bandit = character.bandit.as_pob_name(),
        msg = character.main_socket_group.max(1),
    );
    out.push_str("    <Tree activeSpec=\"1\">\n");
    if character.tattoo_overrides.is_empty() {
        let _ = writeln!(
            out,
            "        <Spec masteryEffects=\"\" treeVersion=\"3_25\" classId=\"{cid}\" ascendClassId=\"0\" nodes=\"{nodes}\"/>",
            cid = class_id,
            nodes = nodes_str,
        );
    } else {
        // Issue #98: tattoos persist via PoB's `<Overrides>` block
        // inside `<Spec>`. Each override carries a node id + the mod
        // text lines. We emit a stripped-down version (no icon /
        // activeEffectImage / dn metadata yet — those are display-only
        // attributes the calc engine doesn't read; tattoo data
        // extraction will populate them in slice 2).
        let _ = writeln!(
            out,
            "        <Spec masteryEffects=\"\" treeVersion=\"3_25\" classId=\"{cid}\" ascendClassId=\"0\" nodes=\"{nodes}\">",
            cid = class_id,
            nodes = nodes_str,
        );
        out.push_str("            <Overrides>\n");
        let mut entries: Vec<(&pob_data::NodeId, &String)> =
            character.tattoo_overrides.iter().collect();
        entries.sort_by_key(|(id, _)| **id);
        for (node_id, mod_text) in entries {
            let body = xml_escape(mod_text.trim());
            let _ = writeln!(
                out,
                "                <Override nodeId=\"{node_id}\">{body}</Override>"
            );
        }
        out.push_str("            </Overrides>\n");
        out.push_str("        </Spec>\n");
    }
    out.push_str("    </Tree>\n");
    let _ = writeln!(out, "    <Notes>{notes}</Notes>");
    write_items(&mut out, character);
    write_skills(&mut out, character);
    write_config(&mut out, &character.config);
    out.push_str("</PathOfBuilding>\n");
    out
}

fn write_items(out: &mut String, c: &Character) {
    let active_empty = c.items.iter().next().is_none();
    if active_empty && c.item_sets.is_empty() {
        out.push_str("    <Items/>\n");
        return;
    }

    // Issue #90: emit every named item-set alongside the active one.
    // Each unique item is serialised once with a stable id; each set
    // emits a `<Slot itemId>` mapping referencing those ids. The active
    // set is the live `c.items` and lands at id 1; saved sets follow at
    // id 2..N. PoB pins the active loadout via the `activeItemSet`
    // attribute on `<Items>`.
    let active_id: u32 = 1;
    let mut sets: Vec<(u32, Option<&str>, &pob_data::ItemSet)> =
        Vec::with_capacity(1 + c.item_sets.len());
    sets.push((active_id, None, &c.items));
    for (idx, named) in c.item_sets.iter().enumerate() {
        sets.push(((idx + 2) as u32, Some(named.name.as_str()), &named.items));
    }

    // Stable ordering of items: walk every set in declaration order and
    // assign ids the first time we see each (slot, raw) tuple. We key by
    // raw paste text so two slots with the *same* item only write it
    // once across sets — but rely on per-set slot uniqueness for the
    // common case where Mapping vs Bossing swap a body armour entirely.
    use std::collections::HashMap;
    let mut item_id_by_raw: HashMap<&str, u32> = HashMap::new();
    let mut next_item_id: u32 = 1;
    let mut item_blocks: Vec<(u32, &pob_data::Item)> = Vec::new();
    for (_, _, set) in &sets {
        let mut entries: Vec<(pob_data::Slot, &pob_data::Item)> =
            set.iter().map(|(slot, item)| (*slot, item)).collect();
        entries.sort_by_key(|(slot, _)| pob_slot_to_name(*slot));
        for (_, item) in entries {
            let key: &str = item.raw.as_str();
            if !item_id_by_raw.contains_key(key) {
                item_id_by_raw.insert(key, next_item_id);
                item_blocks.push((next_item_id, item));
                next_item_id += 1;
            }
        }
    }

    // Issue #109: PoB stores `useSecondWeaponSet` per-ItemSet, but
    // MK2 lifts it to a build-level toggle (single live pair). Emit
    // it on `<Items>` so a round-trip back to PoB picks the same
    // active pair when the user pastes the build there.
    let use_swap = c.config.use_second_weapon_set;
    let _ = writeln!(
        out,
        "    <Items activeItemSet=\"{active_id}\" useSecondWeaponSet=\"{use_swap}\">",
    );

    for (id, item) in &item_blocks {
        // PoB embeds the paste text directly between <Item> tags. We
        // escape for XML safety; PoB's parser unescapes on read.
        let body = xml_escape(item.raw.trim());
        let _ = writeln!(out, "        <Item id=\"{id}\" variant=\"\">{body}</Item>");
    }

    for (set_id, title, set) in &sets {
        let mut entries: Vec<(pob_data::Slot, &pob_data::Item)> =
            set.iter().map(|(slot, item)| (*slot, item)).collect();
        entries.sort_by_key(|(slot, _)| pob_slot_to_name(*slot));

        match title {
            Some(name) => {
                let _ = writeln!(
                    out,
                    "        <ItemSet id=\"{set_id}\" title=\"{title}\" useSecondWeaponSet=\"{use_swap}\">",
                    title = xml_escape(name),
                );
            }
            None => {
                let _ = writeln!(
                    out,
                    "        <ItemSet id=\"{set_id}\" useSecondWeaponSet=\"{use_swap}\">",
                );
            }
        }
        for (slot, item) in entries {
            let item_id = item_id_by_raw.get(item.raw.as_str()).copied().unwrap_or(0);
            let _ = writeln!(
                out,
                "            <Slot name=\"{name}\" itemId=\"{item_id}\"/>",
                name = pob_slot_to_name(slot),
            );
        }
        out.push_str("        </ItemSet>\n");
    }
    out.push_str("    </Items>\n");
}

fn write_skills(out: &mut String, c: &Character) {
    if c.skill_groups.is_empty() {
        out.push_str("    <Skills/>\n");
        return;
    }
    out.push_str("    <Skills>\n");
    for group in &c.skill_groups {
        let label = xml_escape(&group.label);
        let _ = writeln!(
            out,
            "        <Skill mainActiveSkill=\"{idx}\" enabled=\"{en}\" label=\"{label}\">",
            idx = group.main_active_skill_index.max(1),
            en = group.enabled,
        );
        for gem in &group.gems {
            let _ = writeln!(
                out,
                "            <Gem skillId=\"{id}\" level=\"{lvl}\" quality=\"{q}\" qualityId=\"{qid}\" enabled=\"{en}\"/>",
                id = xml_escape(&gem.skill_id),
                lvl = gem.level,
                q = gem.quality,
                qid = gem.quality_id.as_pob_name(),
                en = gem.enabled,
            );
        }
        out.push_str("        </Skill>\n");
    }
    out.push_str("    </Skills>\n");
}

fn write_config(out: &mut String, cfg: &ConfigState) {
    let mut inputs: Vec<String> = Vec::new();

    // Typed enemy fields use PoB's canonical Input names so a round-trip back
    // to PoB recovers them via apply_config_number.
    if cfg.enemy_level != 0 {
        inputs.push(format!(
            "        <Input name=\"enemyLevel\" number=\"{}\"/>",
            cfg.enemy_level
        ));
    }
    if cfg.enemy_fire_resist != 0 {
        inputs.push(format!(
            "        <Input name=\"enemyFireResist\" number=\"{}\"/>",
            cfg.enemy_fire_resist
        ));
    }
    if cfg.enemy_cold_resist != 0 {
        inputs.push(format!(
            "        <Input name=\"enemyColdResist\" number=\"{}\"/>",
            cfg.enemy_cold_resist
        ));
    }
    if cfg.enemy_lightning_resist != 0 {
        inputs.push(format!(
            "        <Input name=\"enemyLightningResist\" number=\"{}\"/>",
            cfg.enemy_lightning_resist
        ));
    }
    if cfg.enemy_chaos_resist != 0 {
        inputs.push(format!(
            "        <Input name=\"enemyChaosResist\" number=\"{}\"/>",
            cfg.enemy_chaos_resist
        ));
    }
    // Defender-side enemy stats added after the original 7d export pass.
    // Names match the canonical PoB Input names recognised by
    // `apply_config_number` so a round-trip survives.
    if cfg.enemy_evasion != 0 {
        inputs.push(format!(
            "        <Input name=\"enemyEvasion\" number=\"{}\"/>",
            cfg.enemy_evasion
        ));
    }
    if cfg.enemy_armour != 0 {
        inputs.push(format!(
            "        <Input name=\"enemyArmour\" number=\"{}\"/>",
            cfg.enemy_armour
        ));
    }
    if cfg.enemy_block_chance != 0 {
        inputs.push(format!(
            "        <Input name=\"enemyBlockChance\" number=\"{}\"/>",
            cfg.enemy_block_chance
        ));
    }
    if cfg.enemy_dodge_chance != 0 {
        inputs.push(format!(
            "        <Input name=\"enemyDodgeChance\" number=\"{}\"/>",
            cfg.enemy_dodge_chance
        ));
    }
    if cfg.enemy_suppression_chance != 0 {
        inputs.push(format!(
            "        <Input name=\"enemySuppressionChance\" number=\"{}\"/>",
            cfg.enemy_suppression_chance
        ));
    }
    // Projectile shotgun count from the Config tab. Use PoB's canonical
    // `projectileNumberHitting` name; the import side accepts that or the
    // alias `projectilesHitTarget`.
    if cfg.projectiles_hitting_target != 0 {
        inputs.push(format!(
            "        <Input name=\"projectileNumberHitting\" number=\"{}\"/>",
            cfg.projectiles_hitting_target
        ));
    }

    // Map MK2's internal multiplier keys back to the PoB Input names that
    // apply_config_number recognises so charge counts survive a round-trip.
    let mults_canon: &[(&str, &str)] = &[
        ("PowerCharge", "powerCharges"),
        ("FrenzyCharge", "frenzyCharges"),
        ("EnduranceCharge", "enduranceCharges"),
    ];

    let mut conds: Vec<(&str, bool)> = cfg
        .conditions
        .iter()
        .map(|(k, v)| (k.as_str(), *v))
        .collect();
    conds.sort_by_key(|(k, _)| *k);
    for (k, v) in conds {
        inputs.push(format!(
            "        <Input name=\"{name}\" boolean=\"{val}\"/>",
            name = xml_escape(k),
            val = v,
        ));
    }

    let mut mults: Vec<(&str, f64)> = cfg
        .multipliers
        .iter()
        .map(|(k, v)| (k.as_str(), *v))
        .collect();
    mults.sort_by_key(|(k, _)| *k);
    for (k, v) in mults {
        let pob_name = mults_canon
            .iter()
            .find_map(|(mk2, pob)| (*mk2 == k).then_some(*pob))
            .unwrap_or(k);
        inputs.push(format!(
            "        <Input name=\"{name}\" number=\"{val}\"/>",
            name = xml_escape(pob_name),
            val = format_number(v),
        ));
    }

    if inputs.is_empty() {
        out.push_str("    <Config/>\n");
        return;
    }
    out.push_str("    <Config>\n");
    for line in inputs {
        out.push_str(&line);
        out.push('\n');
    }
    out.push_str("    </Config>\n");
}

fn format_number(v: f64) -> String {
    if v.fract() == 0.0 && v.is_finite() {
        format!("{}", v as i64)
    } else {
        format!("{v}")
    }
}

fn class_name_to_id(class: &str) -> u32 {
    match class {
        "Scion" => 0,
        "Marauder" => 1,
        "Ranger" => 2,
        "Witch" => 3,
        "Duelist" => 4,
        "Templar" => 5,
        "Shadow" => 6,
        _ => 0,
    }
}

/// Encode the `xml(deflate(bytes))` PoB share-code format.
pub fn export_pob_code(character: &Character) -> Result<String, std::io::Error> {
    use base64::engine::general_purpose::URL_SAFE_NO_PAD;
    use base64::Engine as _;
    use flate2::write::ZlibEncoder;
    use flate2::Compression;
    use std::io::Write;

    let xml = export_pob_xml(character);
    let mut compressed = Vec::with_capacity(xml.len() / 2);
    let mut enc = ZlibEncoder::new(&mut compressed, Compression::default());
    enc.write_all(xml.as_bytes())?;
    enc.finish()?;
    Ok(URL_SAFE_NO_PAD.encode(&compressed))
}

fn xml_escape(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '&' => out.push_str("&amp;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&apos;"),
            _ => out.push(c),
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::character::ClassRef;
    use crate::skill::MainSkill;

    #[test]
    fn round_trip_through_pob_xml() {
        let mut c = Character::new(ClassRef::witch(), 92);
        c.ascendancy = Some("Occultist".into());
        c.allocated.insert(101);
        c.allocated.insert(202);
        c.allocated.insert(303);
        c.notes = "Build summary <with> & special characters.".into();

        let xml = export_pob_xml(&c);
        let imported = crate::pob_import::import_pob_xml(&xml).unwrap();

        assert_eq!(imported.class.0, "Witch");
        assert_eq!(imported.ascendancy.as_deref(), Some("Occultist"));
        assert_eq!(imported.level, 92);
        assert_eq!(imported.allocated.len(), 3);
        assert!(imported.allocated.contains(&101));
        assert!(imported.allocated.contains(&303));
        assert_eq!(imported.notes, "Build summary <with> & special characters.");
    }

    #[test]
    fn round_trip_through_pob_code() {
        let mut c = Character::new(ClassRef::ranger(), 67);
        c.allocated.insert(50);
        let code = export_pob_code(&c).unwrap();
        let imported = crate::pob_import::import_pob_code(&code).unwrap();
        assert_eq!(imported.class.0, "Ranger");
        assert_eq!(imported.level, 67);
        assert!(imported.allocated.contains(&50));
    }

    #[test]
    fn round_trip_items_skills_config() {
        let mut c = Character::new(ClassRef::witch(), 90);
        c.ascendancy = Some("Occultist".into());

        // Equip an amulet via paste — the round-trip preserves Item.raw.
        let amulet_paste = "Item Class: Amulets\nRarity: RARE\nDemo Charm\nOnyx Amulet\n--------\n+10 to all Attributes\n+62 to maximum Life\n+39% to all Elemental Resistances\n--------";
        let amulet = crate::parse_item(amulet_paste).expect("parse amulet");
        c.items.equip(pob_data::Slot::Amulet, amulet);

        // One socket group with main + support.
        c.skill_groups.push(crate::character::SocketGroup {
            label: "Main".into(),
            gems: vec![
                MainSkill {
                    skill_id: "Arc".into(),
                    level: 20,
                    quality: 20,
                    quality_id: crate::QualityId::Default,
                    enabled: true,
                },
                MainSkill {
                    skill_id: "ArcaneSurge".into(),
                    level: 1,
                    quality: 0,
                    quality_id: crate::QualityId::Default,
                    enabled: false,
                },
            ],
            main_active_skill_index: 1,
            enabled: true,
        });
        c.main_socket_group = 1;
        c.sync_main_skill();

        // Config: one condition, one charge multiplier, one typed enemy field.
        c.config.conditions.insert("FullLife".to_owned(), true);
        c.config.multipliers.insert("PowerCharge".into(), 3.0);
        c.config.enemy_lightning_resist = 50;

        let xml = export_pob_xml(&c);
        let r = crate::pob_import::import_pob_xml(&xml).expect("re-import");

        // Items: amulet survives the round-trip and its mod lines are present.
        let amulet = r
            .items
            .get(pob_data::Slot::Amulet)
            .expect("amulet present after re-import");
        assert_eq!(amulet.base_name, "Onyx Amulet");
        assert!(amulet
            .mod_lines
            .iter()
            .any(|m| m.line.contains("+62 to maximum Life")));

        // Skills: two gems in one group; main is Arc level 20 quality 20.
        assert_eq!(r.skill_groups.len(), 1);
        let group = &r.skill_groups[0];
        assert_eq!(group.gems.len(), 2);
        assert_eq!(group.gems[0].skill_id, "Arc");
        assert_eq!(group.gems[0].level, 20);
        assert_eq!(group.gems[0].quality, 20);
        assert!(group.gems[0].enabled);
        assert_eq!(group.gems[1].skill_id, "ArcaneSurge");
        assert!(!group.gems[1].enabled);
        assert_eq!(
            r.main_skill.as_ref().map(|m| m.skill_id.as_str()),
            Some("Arc")
        );

        // Config: condition + charge multiplier + enemy resist.
        assert_eq!(r.config.conditions.get("FullLife"), Some(&true));
        assert_eq!(r.config.multipliers.get("PowerCharge"), Some(&3.0));
        assert_eq!(r.config.enemy_lightning_resist, 50);
    }
}
