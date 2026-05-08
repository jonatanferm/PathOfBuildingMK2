//! Phase 2 calc pipeline. Builds an `Env` from a `Character` + `PassiveTree`, then runs
//! a basic-stats pass.
//!
//! Mirrors `Modules/CalcSetup.lua` (env construction) + a tiny slice of
//! `Modules/CalcPerform.lua` (basic life/mana/attribute computation).

use pob_data::{KeywordFlag, ModFlag, PassiveTree};

use crate::character::Character;
use crate::env::{Env, Output};
use crate::mod_db::{ModStore, QueryCfg};
use crate::mod_parser::parse_mod_line;
use crate::modifier::{Mod, ModType, Source};
use crate::skill::{skill_base_damage, skill_damage_element, SkillRegistry};

/// Top-level entry point — equivalent to PoB's `calcs.buildOutput(build, "MAIN")` for the
/// minimal scope of Phase 2/3. Returns the populated `Output`.
pub fn compute(character: &Character, tree: &PassiveTree) -> Output {
    compute_with_skills(character, tree, None)
}

/// Like `compute`, but also threads in a `SkillRegistry` so we can compute basic skill
/// hit damage for the main skill.
pub fn compute_with_skills(
    character: &Character,
    tree: &PassiveTree,
    skills: Option<&SkillRegistry>,
) -> Output {
    compute_full(character, tree, skills, None)
}

/// Most-complete entry point: bases are used for item-base implicit stats (armour /
/// evasion / ES / shield block from canonical bases instead of heuristics).
pub fn compute_full(
    character: &Character,
    tree: &PassiveTree,
    skills: Option<&SkillRegistry>,
    bases: Option<&pob_data::bases::ItemBaseSet>,
) -> Output {
    let mut env = init_env_with_bases(character, tree, bases);
    perform_basic_stats(character, tree, &mut env);
    if let Some(reg) = skills {
        perform_skill_dps(character, reg, &mut env);
    }
    perform_ehp(&mut env);
    env.output
}

/// Construct the env: gather class base attributes, parse and add tree node mods,
/// add level mods.
pub fn init_env(character: &Character, tree: &PassiveTree) -> Env {
    init_env_with_bases(character, tree, None)
}

/// Same as `init_env` but also looks up canonical item-base implicits.
pub fn init_env_with_bases(
    character: &Character,
    tree: &PassiveTree,
    bases: Option<&pob_data::bases::ItemBaseSet>,
) -> Env {
    let mut env = Env::default();

    // 1. Class base attributes (Marauder = 32 str / 14 dex / 14 int, etc.).
    if let Some(class) = character.resolve_class(tree) {
        env.mod_db.add(
            Mod::base("Strength", f64::from(class.base_str))
                .with_source(Source::Other("ClassBase".into())),
        );
        env.mod_db.add(
            Mod::base("Dexterity", f64::from(class.base_dex))
                .with_source(Source::Other("ClassBase".into())),
        );
        env.mod_db.add(
            Mod::base("Intelligence", f64::from(class.base_int))
                .with_source(Source::Other("ClassBase".into())),
        );
    }

    // 2. Level-derived bases. PoE characters get +12 max life and +6 max mana per level
    // (after the level-1 baseline), and +2 accuracy per level. Base life at level 1 is 50,
    // base mana at level 1 is 40.
    let level = character.level.max(1);
    env.mod_db.add(
        Mod::base("Life", 50.0 + 12.0 * f64::from(level - 1))
            .with_source(Source::Other("Level".into())),
    );
    env.mod_db.add(
        Mod::base("Mana", 40.0 + 6.0 * f64::from(level - 1))
            .with_source(Source::Other("Level".into())),
    );
    // Every character gets 15 base evasion rating from `characterConstants`. Items
    // and tree allocs add to this; without anything else allocated PoB still shows
    // ~15 evasion on the defence panel.
    env.mod_db.add(
        Mod::base("Evasion", 15.0).with_source(Source::Other("CharacterConstant".into())),
    );

    // 3. Tree node stats. Parse each allocated node's stat lines. PoB only credits
    // nodes that form a connected path from the character's class start, so we
    // filter the allocation set to the connected subgraph before applying mods.
    // (Disconnected node IDs come from imported XML where the user manually edited
    // the file — the in-app UI only ever allocates valid paths.)
    let effective: std::collections::HashSet<pob_data::NodeId> =
        connected_allocations(character, tree);
    for node_id in &effective {
        let Some(node) = tree.nodes.get(node_id) else {
            continue;
        };
        // Mastery nodes don't contribute their `stats` directly — the user picks one of
        // the `mastery_effects`. Look up the selection (or default to the first effect
        // if the user hasn't picked one) and use that effect's stats.
        if matches!(node.kind, pob_data::NodeKind::Mastery) {
            let selected = character
                .mastery_selections
                .get(node_id)
                .copied()
                .or_else(|| node.mastery_effects.first().map(|e| e.effect));
            if let Some(effect_id) = selected {
                if let Some(effect) =
                    node.mastery_effects.iter().find(|e| e.effect == effect_id)
                {
                    for raw in &effect.stats {
                        for line in raw.lines() {
                            let line = line.trim();
                            if line.is_empty() {
                                continue;
                            }
                            if let Some(parsed) = parse_mod_line(line) {
                                env.mod_db
                                    .add(parsed.mod_.with_source(Source::Passive(*node_id)));
                            }
                        }
                    }
                }
            }
            continue;
        }
        for raw in &node.stats {
            for line in raw.lines() {
                let line = line.trim();
                if line.is_empty() {
                    continue;
                }
                if let Some(parsed) = parse_mod_line(line) {
                    env.mod_db
                        .add(parsed.mod_.with_source(Source::Passive(*node_id)));
                }
            }
        }
    }

    // 4. Items.
    let _ = crate::item_parser::apply_item_set_with_bases(
        &character.items,
        &mut env.mod_db,
        bases,
    );
    // Set SlotName conditions for slots that have an item — supports SlotName tags on
    // mods that say "while using a shield" / "while wielding a bow", etc.
    for (slot, _) in character.items.iter() {
        let slot_name = match slot {
            pob_data::Slot::Helmet => "Helmet",
            pob_data::Slot::BodyArmour => "Body Armour",
            pob_data::Slot::Gloves => "Gloves",
            pob_data::Slot::Boots => "Boots",
            pob_data::Slot::Amulet => "Amulet",
            pob_data::Slot::Ring1 => "Ring 1",
            pob_data::Slot::Ring2 => "Ring 2",
            pob_data::Slot::Belt => "Belt",
            pob_data::Slot::Weapon1 => "Weapon 1",
            pob_data::Slot::Weapon2 => "Weapon 2",
            pob_data::Slot::Flask1 => "Flask 1",
            pob_data::Slot::Flask2 => "Flask 2",
            pob_data::Slot::Flask3 => "Flask 3",
            pob_data::Slot::Flask4 => "Flask 4",
            pob_data::Slot::Flask5 => "Flask 5",
        };
        env.state
            .set_condition(format!("SlotName:{slot_name}"), true);
    }

    // 5. Auto-detected wielding conditions from the equipped items. These activate
    // mods that have a "while using a shield" / "while wielding a two handed weapon" /
    // "while dual wielding" trailing clause via their parser-emitted Condition tag.
    detect_wielding_conditions(&character.items, &mut env.state);

    // 6. Config — push conditions and multipliers into the eval state. ConfigState
    // overrides auto-detection if the user has explicitly set the same key.
    for (k, v) in &character.config.conditions {
        env.state.set_condition(k.clone(), *v);
    }
    for (k, v) in &character.config.multipliers {
        env.state.set_multiplier(k.clone(), *v);
    }

    env
}

fn detect_wielding_conditions(items: &pob_data::ItemSet, state: &mut crate::mod_db::EvalState) {
    use pob_data::Slot;
    let weapon1 = items.get(Slot::Weapon1);
    let weapon2 = items.get(Slot::Weapon2);

    let weapon2_is_shield = weapon2
        .map(|i| i.base_name.contains("Shield") || i.base_name.contains("Buckler"))
        .unwrap_or(false);
    if weapon2_is_shield {
        state.set_condition("UsingShield", true);
    }

    let weapon1_is_two_handed = weapon1
        .map(|i| {
            i.base_name.contains("Two Handed")
                || i.base_name.contains("Bow")
                || i.base_name.contains("Staff")
                || i.base_name.contains("Quarterstaff")
        })
        .unwrap_or(false);
    if weapon1_is_two_handed {
        state.set_condition("UsingTwoHandedWeapon", true);
    }

    let weapon1_is_one_handed = weapon1.is_some() && !weapon1_is_two_handed;
    let weapon2_is_one_handed_weapon = weapon2.is_some() && !weapon2_is_shield;
    if weapon1_is_one_handed && weapon2_is_one_handed_weapon {
        state.set_condition("DualWielding", true);
    }
}

/// BFS from the character's class start through the allocated subgraph and
/// return the set of allocations that are actually reachable. Mirrors PoB's
/// rule that a node only contributes if it sits on a connected path from the
/// character's class root.
fn connected_allocations(
    character: &Character,
    tree: &PassiveTree,
) -> std::collections::HashSet<pob_data::NodeId> {
    let mut effective = std::collections::HashSet::new();
    if character.allocated.is_empty() {
        return effective;
    }
    // Find the class index by name match against tree.classes.
    let Some(class_idx) = tree
        .classes
        .iter()
        .position(|c| c.name.eq_ignore_ascii_case(&character.class.0))
    else {
        // Unknown class — fall back to crediting every alloc so we don't silently
        // drop stats. This path is for tests that synthesise classes that the
        // tree fixture doesn't actually carry.
        return character.allocated.iter().copied().collect();
    };
    let class_idx = class_idx as u32;
    // Find class-start node ID(s).
    let starts: Vec<pob_data::NodeId> = tree
        .nodes
        .iter()
        .filter_map(|(id, node)| {
            if node.class_start_index == Some(class_idx) {
                Some(*id)
            } else {
                None
            }
        })
        .collect();
    if starts.is_empty() {
        return character.allocated.iter().copied().collect();
    }

    let allocated: std::collections::HashSet<_> = character.allocated.iter().copied().collect();
    let mut queue: std::collections::VecDeque<pob_data::NodeId> = starts.into_iter().collect();
    while let Some(node_id) = queue.pop_front() {
        if !effective.insert(node_id) {
            continue;
        }
        let Some(node) = tree.nodes.get(&node_id) else {
            continue;
        };
        for &neighbor in node.out_edges.iter().chain(node.in_edges.iter()) {
            if !effective.contains(&neighbor) && allocated.contains(&neighbor) {
                queue.push_back(neighbor);
            }
        }
    }
    // The class-start nodes themselves are visited but should not contribute
    // mods (they're synthetic). Strip them.
    effective.retain(|id| {
        tree.nodes
            .get(id)
            .map_or(false, |n| n.class_start_index.is_none())
    });
    effective
}

pub fn perform_basic_stats(character: &Character, tree: &PassiveTree, env: &mut Env) {
    // Strength / Dexterity / Intelligence
    let cfg = QueryCfg::default();
    let str_base = env.mod_db.sum(ModType::Base, &cfg, &env.state, "Strength")
        + env.mod_db.sum(ModType::Base, &cfg, &env.state, "AllAttributes");
    let dex_base = env.mod_db.sum(ModType::Base, &cfg, &env.state, "Dexterity")
        + env.mod_db.sum(ModType::Base, &cfg, &env.state, "AllAttributes");
    let int_base = env.mod_db.sum(ModType::Base, &cfg, &env.state, "Intelligence")
        + env.mod_db.sum(ModType::Base, &cfg, &env.state, "AllAttributes");

    let str_v = str_base * env.mod_db.applied(&cfg, &env.state, "Strength");
    let dex_v = dex_base * env.mod_db.applied(&cfg, &env.state, "Dexterity");
    let int_v = int_base * env.mod_db.applied(&cfg, &env.state, "Intelligence");

    env.output.set("Strength", str_v.round());
    env.output.set("Dexterity", dex_v.round());
    env.output.set("Intelligence", int_v.round());

    // Stash attributes back into the eval state for downstream tags.
    env.state.set_stat("Strength", str_v);
    env.state.set_stat("Dexterity", dex_v);
    env.state.set_stat("Intelligence", int_v);

    // Life: base + (Strength / 2) implicit from PoE; then * (1 + inc/100) * more product.
    let life_base = env.mod_db.sum(ModType::Base, &cfg, &env.state, "Life") + str_v / 2.0;
    let life = life_base * env.mod_db.applied(&cfg, &env.state, "Life");
    env.output.set("Life", life.round());

    // Mana: base + (Intelligence / 2).
    let mana_base = env.mod_db.sum(ModType::Base, &cfg, &env.state, "Mana") + int_v / 2.0;
    let mana = mana_base * env.mod_db.applied(&cfg, &env.state, "Mana");
    env.output.set("Mana", mana.round());

    // Energy Shield: pure mods (no base). Phase 2: base 0; later integrate item ES bases.
    let es_base = env.mod_db.sum(ModType::Base, &cfg, &env.state, "EnergyShield");
    let es = es_base * env.mod_db.applied(&cfg, &env.state, "EnergyShield");
    env.output.set("EnergyShield", es.round());

    // Resistances: PoE applies a story-based penalty that drops elemental and chaos
    // resists by 30 at end of Act 5 and another 30 at end of Act 10. The character's
    // `level` is a proxy for story progression — by level 68+ players are post-Act 10.
    // Match PoB's default by applying -60 to all four resists for any character of
    // level 68 or higher unless an explicit `act` config override is in play.
    let resist_penalty: f64 = if character.level >= 68 { -60.0 } else { 0.0 };
    for elem in ["Fire", "Cold", "Lightning"] {
        let key = format!("{elem}Resist");
        let total = env.mod_db.sum(ModType::Base, &cfg, &env.state, &key)
            + env.mod_db.sum(ModType::Base, &cfg, &env.state, "ElementalResist")
            + resist_penalty;
        env.output.set(&key, total);
    }
    let chaos = env.mod_db.sum(ModType::Base, &cfg, &env.state, "ChaosResist") + resist_penalty;
    env.output.set("ChaosResist", chaos);

    // Resist caps (default 75%, mods add/subtract).
    for elem in ["Fire", "Cold", "Lightning"] {
        let key = format!("{elem}ResistMax");
        let bonus = env.mod_db.sum(ModType::Base, &cfg, &env.state, &key);
        env.output.set(&key, 75.0 + bonus);
        let cap = 75.0 + bonus;
        let raw = env.output.get(&format!("{elem}Resist"));
        let capped = raw.min(cap);
        env.output.set(&format!("{elem}ResistTotal"), capped);
    }
    {
        let bonus = env.mod_db.sum(ModType::Base, &cfg, &env.state, "ChaosResistMax");
        let cap = 75.0 + bonus;
        env.output.set("ChaosResistMax", cap);
        let raw = env.output.get("ChaosResist");
        env.output.set("ChaosResistTotal", raw.min(cap));
    }

    // Defences: armour, evasion. Treat them as INC over a base 0; items push base values.
    for stat in ["Armour", "Evasion", "Ward"] {
        let base = env.mod_db.sum(ModType::Base, &cfg, &env.state, stat);
        let total = base * env.mod_db.applied(&cfg, &env.state, stat);
        env.output.set(stat, total.round());
    }

    // Armour mitigation against a "typical" physical hit. PoE formula:
    //   reduction = armour / (armour + 12 × raw_phys_damage)
    // Capped at 90% by default.
    // We expose `PhysicalDamageReduction` as a percent against a 1000-point baseline
    // hit so the side panel can show something meaningful without an explicit enemy
    // damage knob. PoB does the same with its standard-boss configurable hit value.
    {
        let armour = env.output.get("Armour");
        let baseline_phys = 1000.0_f64;
        let raw_reduction = armour / (armour + 12.0 * baseline_phys);
        let reduction = (raw_reduction * 100.0).min(90.0);
        env.output.set("PhysicalDamageReduction", reduction);
    }

    // Block / Spell Block / Spell Suppression / Dodge — base 0, cap 75%.
    let block_inc_pct = env.mod_db.sum(ModType::Base, &cfg, &env.state, "BlockChance");
    let block_max_bonus = env.mod_db.sum(ModType::Base, &cfg, &env.state, "BlockChanceMax");
    let block_cap = 75.0 + block_max_bonus;
    env.output.set("BlockChance", block_inc_pct.min(block_cap));
    env.output.set("BlockChanceMax", block_cap);
    let spell_block = env.mod_db.sum(ModType::Base, &cfg, &env.state, "SpellBlockChance");
    env.output.set("SpellBlockChance", spell_block.min(block_cap));
    let suppress = env.mod_db.sum(ModType::Base, &cfg, &env.state, "SpellSuppressionChance");
    env.output.set("SpellSuppressionChance", suppress.min(100.0));

    // Life / mana / ES regen
    let life_regen_flat = env.mod_db.sum(ModType::Base, &cfg, &env.state, "LifeRegen");
    let life_regen_pct = env.mod_db.sum(ModType::Base, &cfg, &env.state, "LifeRegenPercent");
    let life = env.output.get("Life");
    let life_regen_total =
        life_regen_flat + life * life_regen_pct / 100.0;
    env.output.set("LifeRegen", life_regen_total);

    let mana_regen_flat = env.mod_db.sum(ModType::Base, &cfg, &env.state, "ManaRegen");
    let mana_regen_pct = env.mod_db.sum(ModType::Inc, &cfg, &env.state, "ManaRegen");
    let mana = env.output.get("Mana");
    // PoE: base mana regen = 1.75% of max mana per second; modifier is INC on rate.
    let mana_regen_total =
        (mana * 0.0175 + mana_regen_flat) * (1.0 + mana_regen_pct / 100.0);
    env.output.set("ManaRegen", mana_regen_total);

    // ES recharge — base 33% of total ES per second after delay, but we just expose the
    // increased-rate stat as a mod multiplier (delay handling lives in Phase 4).
    let es = env.output.get("EnergyShield");
    let recharge_inc =
        env.mod_db.sum(ModType::Inc, &cfg, &env.state, "EnergyShieldRecharge");
    env.output.set(
        "EnergyShieldRecharge",
        es * 0.33 * (1.0 + recharge_inc / 100.0),
    );

    // ES regen — separate from recharge. PoB exposes EnergyShieldRegen as the
    // base regen-per-second rate (zero unless mods grant it directly).
    let es_regen_flat = env.mod_db.sum(ModType::Base, &cfg, &env.state, "EnergyShieldRegen");
    let es_regen_pct = env.mod_db.sum(ModType::Inc, &cfg, &env.state, "EnergyShieldRegen");
    env.output.set("EnergyShieldRegen", es_regen_flat * (1.0 + es_regen_pct / 100.0));

    // Reservation pools. Phase 2 doesn't model auras yet, so unreserved == max.
    // PoB exposes both the absolute and the percentage; we mirror absolutes.
    env.output.set("LifeUnreserved", life);
    env.output.set("LifeUnreservedPercent", 100.0);
    env.output.set("ManaUnreserved", mana);
    env.output.set("ManaUnreservedPercent", 100.0);

    // Movement speed multiplier — `applied` returns the (1+inc)*more product.
    let move_speed = env.mod_db.applied(&cfg, &env.state, "MovementSpeed");
    env.output.set("MovementSpeedMod", move_speed);

    // Cast speed (multiplier on a base of 1.0 — PoB normalises this against skill
    // baseline cast time).
    let cast_speed_mult = env.mod_db.applied(&cfg, &env.state, "CastSpeed");
    env.output.set("CastSpeedMult", cast_speed_mult);
    let attack_speed_mult = env.mod_db.applied(&cfg, &env.state, "AttackSpeed");
    env.output.set("AttackSpeedMult", attack_speed_mult);

    // Crit chance / multiplier. PoB exposes these as flat character-level outputs
    // even with no skill selected: 0 (no chance to crit) and 1.5 (the decimal form
    // of the 150% PoE base crit damage multiplier — `1 + 50/100`).
    // With a skill we mirror PoB's full computation: crit chance scales with INC,
    // CritMultiplier picks up BASE additions on top of the 150% baseline.
    let crit_inc = env.mod_db.sum(ModType::Inc, &cfg, &env.state, "CritChance");
    let crit_chance_base = if character.main_skill.is_some() { 5.0 } else { 0.0 };
    env.output.set("CritChance", crit_chance_base * (1.0 + crit_inc / 100.0));
    // PoE base crit deals 150% damage; PoB exposes that as the decimal multiplier
    // 1.5 (= 150 / 100). BASE mods on `CritMultiplier` add extra crit damage as
    // additional percentage points.
    let crit_mult_pct = 150.0 + env.mod_db.sum(ModType::Base, &cfg, &env.state, "CritMultiplier");
    env.output.set("CritMultiplier", crit_mult_pct / 100.0);
}

/// Compute hit damage for the main skill. Phase 3d: spell-only, single hit, single
/// target, ignores ailments / penetration / resistances of the enemy. Outputs:
/// `MainSkillId`, `MainSkillLevel`, `MainSkillBaseMin`, `MainSkillBaseMax`,
/// `MainSkillAverageHit`, `MainSkillDPS`.
fn perform_skill_dps(character: &Character, skills: &SkillRegistry, env: &mut Env) {
    let Some(main) = character.main_skill.as_ref() else {
        return;
    };
    let Some(skill) = skills.get(&main.skill_id) else {
        return;
    };
    let gem_level = main.level.clamp(1, 40);
    env.output.set("MainSkillLevel", f64::from(gem_level));

    // Apply the skill's intrinsic mods (from constantStats + qualityStats × statMap)
    // to the env. These produce things like Arc's "+15% MORE damage per chain
    // remaining" which the calc layer needs to see before the damage query runs.
    let intrinsic_mods = crate::skill::skill_mods(skill, main.quality);
    for m in intrinsic_mods {
        env.mod_db.add(m);
    }

    // Tag the EvalState with the active skill's name and types so SkillName /
    // SkillType / SkillId tags on mods can filter correctly.
    env.state
        .set_condition(format!("SkillName:{}", main.skill_id), true);
    for (st_id, on) in &skill.skill_types {
        if !*on {
            continue;
        }
        env.state
            .set_condition(format!("SkillType:{st_id}"), true);
    }

    // For each named per-level stat, push the corresponding positional value into the
    // EvalState's stats map. This lets PerStat tags (e.g. PerStat:ChainRemaining)
    // scale by the skill's own per-level numbers (Arc has 7 chains at level 20, so
    // ChainRemaining = 7). PoB does this in CalcActiveSkill via its skillData table.
    for (i, stat_id) in skill.stats.iter().enumerate() {
        // positional indices in our extractor are 1-based: positional[1] is the first.
        if let Some(v) = skill.positional(gem_level, (i + 1) as u32) {
            // Map skill stat ids onto the EvalState stat names PoB uses for tag
            // resolution. Conservative — only the well-known ones.
            let mapped = match stat_id.as_str() {
                "number_of_chains" | "number_of_chains_+" => Some("ChainRemaining"),
                "number_of_additional_projectiles" => Some("ProjectileCount"),
                "skill_repeat_count" => Some("SkillRepeatCount"),
                "active_skill_area_of_effect_radius_+%_final" => Some("AreaOfEffect"),
                _ => None,
            };
            if let Some(eval_key) = mapped {
                env.state.set_stat(eval_key, v);
            }
        }
    }
    // Move the spell/attack flag detection before damage so we can branch.
    let early_is_attack = skill.base_flags.get("attack").copied().unwrap_or(false);
    let (mut base_min, mut base_max) = if early_is_attack {
        // Attack skills: base damage is the weapon's damage. Use Weapon1 if equipped.
        let cfg_q = QueryCfg::default();
        let st = &env.state;
        let w_min =
            env.mod_db
                .sum(ModType::Base, &cfg_q, st, "Weapon1PhysicalMin");
        let w_max =
            env.mod_db
                .sum(ModType::Base, &cfg_q, st, "Weapon1PhysicalMax");
        if w_min > 0.0 || w_max > 0.0 {
            (w_min, w_max)
        } else {
            // No weapon equipped — fall back to skill positional values.
            skill_base_damage(skill, gem_level, character.level)
        }
    } else {
        skill_base_damage(skill, gem_level, character.level)
    };
    if base_min == 0.0 && base_max == 0.0 {
        // Skill has no usable damage values — abort cleanly.
        return;
    }
    base_min *= skill.damage_effectiveness(gem_level);
    base_max *= skill.damage_effectiveness(gem_level);

    // Add flat damage from "Adds N to M <element> Damage" mods. The parser emits these
    // as Mod::base("<Element>Damage", ModValue::Range{min, max}).
    let cfg_q = QueryCfg::default();
    let st = &env.state;
    let elem_for_flat = if early_is_attack {
        // Attacks pick up the most recently equipped weapon's flat damage; for now
        // we add all elements together to phys damage (rough but useful).
        ["PhysicalDamage", "FireDamage", "ColdDamage", "LightningDamage", "ChaosDamage"]
    } else {
        // Spells: only the dominant element gets flat-damage bonuses.
        let (e, _) = skill_damage_element(skill).unwrap_or(("Damage", ""));
        [e, e, e, e, e]
    };
    for elem in &elem_for_flat {
        let mut flat_min = 0.0;
        let mut flat_max = 0.0;
        for m in env.mod_db.iter_named(elem) {
            if m.kind != ModType::Base {
                continue;
            }
            if let Some((lo, hi)) = m.value.as_range() {
                flat_min += lo;
                flat_max += hi;
            }
        }
        base_min += flat_min;
        base_max += flat_max;
    }
    let _ = st;
    let _ = cfg_q;
    env.output.set("MainSkillBaseMin", base_min);
    env.output.set("MainSkillBaseMax", base_max);

    // Determine if the skill is a spell or an attack — drives which ModFlag bit we set.
    let is_spell = skill.base_flags.get("spell").copied().unwrap_or(false);
    let is_attack = skill.base_flags.get("attack").copied().unwrap_or(false);
    // SkillType 39 = DamageOverTime in PoB's enum. Skills like Caustic Arrow / Essence
    // Drain are DoT-only — the per-level positional values aren't hit damage but a
    // damage-per-minute/second base that PoB treats specially. Mark them so the hit
    // DPS report doesn't silently mislead.
    let is_dot_only = skill.skill_types.get("39").copied().unwrap_or(false)
        && !skill.skill_types.get("10").copied().unwrap_or(false); // SkillType 10 = Damage (hit)
    if is_dot_only {
        env.output.set("MainSkillIsDotOnly", 1.0);
        // Compute basic DoT DPS using a default-cfg query (no skill-name targeting).
        let dot_cfg = QueryCfg::default();
        let (dot_min, _) = skill_base_damage(skill, gem_level, character.level);
        if dot_min > 0.0 {
            let dot_eff = skill.damage_effectiveness(gem_level);
            let (elem, _) = skill_damage_element(skill).unwrap_or(("ChaosDamage", "Chaos"));
            let dot_inc = env
                .mod_db
                .sum(ModType::Inc, &dot_cfg, &env.state, "DamageOverTime")
                + env.mod_db.sum(ModType::Inc, &dot_cfg, &env.state, elem);
            let dot_more = env
                .mod_db
                .more(&dot_cfg, &env.state, "DamageOverTime")
                * env.mod_db.more(&dot_cfg, &env.state, elem);
            let dot_mult_base = env.mod_db.sum(
                ModType::Base,
                &dot_cfg,
                &env.state,
                "DamageOverTimeMultiplier",
            );
            let per_minute = dot_min
                * dot_eff
                * (1.0 + dot_inc / 100.0)
                * dot_more
                * (1.0 + dot_mult_base / 100.0);
            let per_second = per_minute / 60.0;
            let res = match elem {
                "FireDamage" => character.config.enemy_fire_resist,
                "ColdDamage" => character.config.enemy_cold_resist,
                "LightningDamage" => character.config.enemy_lightning_resist,
                "ChaosDamage" => character.config.enemy_chaos_resist,
                _ => 0,
            };
            let res_factor = (1.0 - f64::from(res) / 100.0).max(0.0);
            let dot_dps = per_second * res_factor;
            env.output.set("MainSkillDotDPS", dot_dps);
            env.output.set("MainSkillDPS", dot_dps);
            env.output.set("FullDPS", dot_dps);
            return;
        }
    }

    // Identify the element keyword for further filtering.
    let (elem_stat, _label) = skill_damage_element(skill).unwrap_or(("Damage", ""));

    let mut cfg = QueryCfg::default();
    // Skill's baseFlags map onto ModFlag bits so "Spell Damage" / "Projectile Damage"
    // mods filter correctly. PoB walks the full skillTypes table; we approximate.
    if is_spell {
        cfg.flags |= ModFlag::SPELL;
    }
    if is_attack {
        cfg.flags |= ModFlag::ATTACK;
    }
    if skill.base_flags.get("melee").copied().unwrap_or(false) {
        cfg.flags |= ModFlag::MELEE;
    }
    if skill.base_flags.get("projectile").copied().unwrap_or(false) {
        cfg.flags |= ModFlag::PROJECTILE;
    }
    if skill.base_flags.get("area").copied().unwrap_or(false) {
        cfg.flags |= ModFlag::AREA;
    }
    cfg.flags |= ModFlag::HIT;
    cfg.keyword_flags = match elem_stat {
        "FireDamage" => KeywordFlag::FIRE,
        "ColdDamage" => KeywordFlag::COLD,
        "LightningDamage" => KeywordFlag::LIGHTNING,
        "PhysicalDamage" => KeywordFlag::PHYSICAL,
        "ChaosDamage" => KeywordFlag::CHAOS,
        _ => KeywordFlag::empty(),
    };
    cfg.skill_name = Some(&main.skill_id);

    // Damage modifiers: stack the elemental, generic damage, and skill-type damage mods.
    // Order: (1+inc_total) * more_total.
    let inc_total = env.mod_db.sum(ModType::Inc, &cfg, &env.state, elem_stat)
        + env.mod_db.sum(ModType::Inc, &cfg, &env.state, "Damage")
        + env.mod_db.sum(ModType::Inc, &cfg, &env.state, "ElementalDamage")
        + if is_spell {
            env.mod_db.sum(ModType::Inc, &cfg, &env.state, "SpellDamage")
        } else {
            0.0
        };
    let more_total = env.mod_db.more(&cfg, &env.state, elem_stat)
        * env.mod_db.more(&cfg, &env.state, "Damage")
        * env.mod_db.more(&cfg, &env.state, "ElementalDamage")
        * if is_spell {
            env.mod_db.more(&cfg, &env.state, "SpellDamage")
        } else {
            1.0
        };
    let mult = (1.0 + inc_total / 100.0) * more_total;

    let hit_min = base_min * mult;
    let hit_max = base_max * mult;
    let avg = (hit_min + hit_max) * 0.5;
    env.output.set("MainSkillHitMin", hit_min);
    env.output.set("MainSkillHitMax", hit_max);
    env.output.set("MainSkillAverageHit", avg);

    // Apply crit. Spells use the skill's intrinsic critChance as base; attacks use
    // the weapon's crit chance (Weapon1CritChance from the equipped weapon's base).
    let base_crit = if is_spell {
        skill.crit_chance(gem_level)
    } else if is_attack {
        let cfg_q = QueryCfg::default();
        let st = &env.state;
        let w = env
            .mod_db
            .sum(ModType::Base, &cfg_q, st, "Weapon1CritChance");
        if w > 0.0 {
            w
        } else {
            5.0
        }
    } else {
        5.0
    };
    let crit_inc = env.mod_db.sum(ModType::Inc, &cfg, &env.state, "CritChance");
    let crit_chance = ((base_crit * (1.0 + crit_inc / 100.0)) / 100.0).clamp(0.0, 1.0);
    env.output.set("MainSkillCritChance", crit_chance * 100.0);
    let crit_mult = env.output.get("CritMultiplier") / 100.0;
    let crit_factor = (1.0 - crit_chance) + crit_chance * crit_mult;
    let avg_with_crit = avg * crit_factor;
    env.output.set("MainSkillAverageHitWithCrit", avg_with_crit);

    // Mana cost from the skill's level data — useful for sustainability checks.
    let mana_cost_base = skill.cost(gem_level, "Mana");
    if mana_cost_base > 0.0 {
        // Apply (1 + inc) and ManaCost INC mods.
        let cost_inc = env.mod_db.sum(ModType::Inc, &cfg, &env.state, "ManaCost");
        let cost = mana_cost_base * (1.0 + cost_inc / 100.0);
        env.output.set("MainSkillManaCost", cost);
    }

    // Apply enemy resistance: hit damage reduced by `(1 - effective_resist/100)`.
    // Penetration: a `<Element>Penetration` Base mod subtracts from the enemy resist
    // before clamping to PoE's -200..max range.
    let enemy_resist_raw = match elem_stat {
        "FireDamage" => character.config.enemy_fire_resist,
        "ColdDamage" => character.config.enemy_cold_resist,
        "LightningDamage" => character.config.enemy_lightning_resist,
        "ChaosDamage" => character.config.enemy_chaos_resist,
        _ => 0,
    };
    let pen_stat = match elem_stat {
        "FireDamage" => "FirePenetration",
        "ColdDamage" => "ColdPenetration",
        "LightningDamage" => "LightningPenetration",
        "ChaosDamage" => "ChaosPenetration",
        _ => "",
    };
    let elem_pen = if pen_stat.is_empty() {
        0.0
    } else {
        env.mod_db.sum(ModType::Base, &cfg, &env.state, pen_stat)
    } + env
        .mod_db
        .sum(ModType::Base, &cfg, &env.state, "ElementalPenetration");
    let effective_resist = (f64::from(enemy_resist_raw) - elem_pen).clamp(-200.0, 95.0);
    let res_factor = (1.0 - effective_resist / 100.0).max(0.0);
    let avg_after_res = avg_with_crit * res_factor;
    env.output.set("MainSkillAverageHitAfterResist", avg_after_res);
    env.output.set("MainSkillEnemyEffectiveResist", effective_resist);

    // Ailments — improved over Phase 3a-baseline. Still a rough single-skill model;
    // see docs/divergences.md for the full list of TODOs (poison-stack steady-state
    // requires cast_rate × duration with a stack cap; bleed has movement modifiers;
    // ignite is single-application, not stacking, but we currently treat it the same).
    if is_attack || is_spell {
        // Per-ailment chance (default ailment chances live in skill data; we don't yet
        // pull those, so a skill with no on-hit ailment chance + no chance-mods produces
        // a 0 ailment DPS — which is correct for spells like Arc against unmodded gear.)
        let bleed_chance = (env.mod_db.sum(ModType::Base, &cfg, &env.state, "BleedChance") / 100.0)
            .clamp(0.0, 1.0);
        let poison_chance = (env.mod_db.sum(ModType::Base, &cfg, &env.state, "PoisonChance") / 100.0)
            .clamp(0.0, 1.0);
        let ignite_chance = (env.mod_db.sum(ModType::Base, &cfg, &env.state, "IgniteChance") / 100.0)
            .clamp(0.0, 1.0);

        // Bleed: 70% of base physical hit damage as Phys DoT for 5s. One stack at a time.
        let phys_avg = if elem_stat == "PhysicalDamage" {
            avg
        } else {
            env.mod_db.sum(ModType::Base, &cfg, &env.state, "PhysicalDamage")
        };
        if bleed_chance > 0.0 && phys_avg > 0.0 {
            let dot_inc = env.mod_db.sum(ModType::Inc, &cfg, &env.state, "BleedDamage")
                + env.mod_db.sum(ModType::Inc, &cfg, &env.state, "DamageOverTime");
            let dot_more = env.mod_db.more(&cfg, &env.state, "BleedDamage")
                * env.mod_db.more(&cfg, &env.state, "DamageOverTime");
            let bleed = phys_avg * 0.70 * (1.0 + dot_inc / 100.0) * dot_more;
            // Single-stack with chance-to-apply: long-run DPS = p × per-application-DPS
            env.output.set("BleedDPS", bleed * bleed_chance);
        }

        // Poison: 30% of hit damage as Chaos DoT for 2s. Stacks; steady-state
        // DPS ≈ per-stack-DPS × cast_rate × duration × chance.
        if poison_chance > 0.0 {
            let p_inc = env.mod_db.sum(ModType::Inc, &cfg, &env.state, "PoisonDamage")
                + env.mod_db.sum(ModType::Inc, &cfg, &env.state, "ChaosDamage")
                + env.mod_db.sum(ModType::Inc, &cfg, &env.state, "DamageOverTime");
            let p_more = env.mod_db.more(&cfg, &env.state, "PoisonDamage")
                * env.mod_db.more(&cfg, &env.state, "ChaosDamage")
                * env.mod_db.more(&cfg, &env.state, "DamageOverTime");
            let p_dot_mult = env
                .mod_db
                .sum(ModType::Base, &cfg, &env.state, "PoisonDamageMultiplier")
                + env
                    .mod_db
                    .sum(ModType::Base, &cfg, &env.state, "DamageOverTimeMultiplier");
            let per_stack = avg
                * 0.30
                * (1.0 + p_inc / 100.0)
                * p_more
                * (1.0 + p_dot_mult / 100.0);
            let speed = env.output.get("MainSkillSpeed").max(0.0);
            let duration_inc = env
                .mod_db
                .sum(ModType::Inc, &cfg, &env.state, "PoisonDuration")
                / 100.0;
            let duration = 2.0 * (1.0 + duration_inc);
            // Steady-state stacks = cast_rate × duration × chance, capped at 50 (PoB
            // default visualisation cap).
            let stacks = (speed * duration * poison_chance).min(50.0);
            env.output.set("PoisonDPS", per_stack * stacks);
            env.output.set("PoisonStacks", stacks);
        }

        // Ignite: 90% of fire hit damage as Fire DoT for 4s, single-application
        // (highest-damage ignite overrides). For a skill that hits constantly, the
        // single-app DPS is the ceiling.
        if elem_stat == "FireDamage" && ignite_chance > 0.0 {
            let i_inc = env.mod_db.sum(ModType::Inc, &cfg, &env.state, "IgniteDamage")
                + env.mod_db.sum(ModType::Inc, &cfg, &env.state, "BurningDamage")
                + env.mod_db.sum(ModType::Inc, &cfg, &env.state, "DamageOverTime");
            let i_more = env.mod_db.more(&cfg, &env.state, "IgniteDamage")
                * env.mod_db.more(&cfg, &env.state, "BurningDamage")
                * env.mod_db.more(&cfg, &env.state, "DamageOverTime");
            let i_dot_mult = env
                .mod_db
                .sum(ModType::Base, &cfg, &env.state, "IgniteDamageMultiplier")
                + env
                    .mod_db
                    .sum(ModType::Base, &cfg, &env.state, "DamageOverTimeMultiplier");
            let ignite = avg
                * 0.90
                * (1.0 + i_inc / 100.0)
                * i_more
                * (1.0 + i_dot_mult / 100.0);
            // Apply chance — assumes the skill reapplies frequently enough to maintain
            // an active ignite.
            env.output.set("IgniteDPS", ignite * ignite_chance);
        }
    }

    // Hit chance — only meaningful for attack skills against an enemy with evasion.
    // PoE formula (mirrors Modules/CalcOffence.lua's accuracy block):
    //   chance = 1.15 * accuracy / (accuracy + (eva/4)^0.9) - 0.15
    // Spells always hit at 100%.
    if is_attack {
        let accuracy = env.mod_db.sum(ModType::Base, &cfg, &env.state, "Accuracy");
        let enemy_evasion = f64::from(character.config.enemy_evasion.max(1));
        let denom = accuracy + f64::powf(enemy_evasion / 4.0, 0.9);
        let raw = if denom > 0.0 {
            1.15 * accuracy / denom - 0.15
        } else {
            0.05
        };
        let chance = raw.clamp(0.05, 1.0);
        env.output.set("MainSkillHitChance", chance * 100.0);
        // Roll hit chance into the DPS at the end.
        let dps_now = env.output.get("MainSkillAverageHitAfterResist");
        env.output.set("MainSkillAverageHitAfterAccuracy", dps_now * chance);
    } else {
        env.output.set("MainSkillHitChance", 100.0);
        env.output.set(
            "MainSkillAverageHitAfterAccuracy",
            env.output.get("MainSkillAverageHitAfterResist"),
        );
    }

    // Cast/attack speed: PoB normalises against skill baseline.
    let speed_mult = if is_spell {
        env.output.get("CastSpeedMult")
    } else {
        env.output.get("AttackSpeedMult")
    };
    let baseline = if is_attack {
        // Attacks: speed is weapon-driven (attack rate from weapon base).
        let cfg_q = QueryCfg::default();
        let st = &env.state;
        let attack_rate =
            env.mod_db
                .sum(ModType::Base, &cfg_q, st, "Weapon1AttackRate");
        if attack_rate > 0.0 {
            attack_rate
        } else {
            1.0
        }
    } else if skill.cast_time > 0.0 {
        1.0 / f64::from(skill.cast_time)
    } else {
        1.0
    };
    let cps = baseline * speed_mult;
    env.output.set("MainSkillSpeed", cps);
    let final_avg = env.output.get("MainSkillAverageHitAfterAccuracy");
    let main_dps = final_avg * cps;
    env.output.set("MainSkillDPS", main_dps);

    // FullDPS aggregator: hit DPS + the highest-impact ailment that this skill can
    // sustain. Real PoB sums all simultaneously sustainable ailments; we approximate.
    let bleed = env.output.get("BleedDPS");
    let poison = env.output.get("PoisonDPS");
    let ignite = env.output.get("IgniteDPS");
    env.output.set("FullDPS", main_dps + bleed + poison + ignite);
}

/// Compute "effective HP" — a single-source survivability number that combines life,
/// energy shield, ward, armour mitigation, block/suppression, and resists.
///
/// EHP[type] = pool / damage_taken_multiplier[type]
/// where damage_taken_multiplier folds in:
///   - Resistance: (1 - resist/100)
///   - Block: (1 - block_chance) (block prevents the hit entirely)
///   - Suppression for spells: (1 - 0.5*suppress) (50% reduction on success)
///   - Armour mitigation for physical: (1 - phys_red)
fn perform_ehp(env: &mut Env) {
    let life = env.output.get("Life");
    let es = env.output.get("EnergyShield");
    let ward = env.output.get("Ward");
    let pool = (life + es + ward).max(1.0);

    let phys_red = (env.output.get("PhysicalDamageReduction") / 100.0).clamp(0.0, 0.9);
    let block = (env.output.get("BlockChance") / 100.0).clamp(0.0, 1.0);
    let spell_block = (env.output.get("SpellBlockChance") / 100.0).clamp(0.0, 1.0);
    let suppress = (env.output.get("SpellSuppressionChance") / 100.0).clamp(0.0, 1.0);

    let fire = (env.output.get("FireResistTotal") / 100.0).clamp(-2.0, 0.95);
    let cold = (env.output.get("ColdResistTotal") / 100.0).clamp(-2.0, 0.95);
    let lightning = (env.output.get("LightningResistTotal") / 100.0).clamp(-2.0, 0.95);
    let chaos = (env.output.get("ChaosResistTotal") / 100.0).clamp(-2.0, 0.95);

    // Damage-taken multipliers per element.
    let phys_taken = (1.0 - phys_red) * (1.0 - block);
    // Spell suppression caps at 50% damage reduction on a successful suppress; it scales
    // linearly with chance.
    let spell_mult = 1.0 - 0.5 * suppress;
    let spell_block_mult = 1.0 - spell_block;
    let fire_taken = (1.0 - fire) * spell_mult * spell_block_mult;
    let cold_taken = (1.0 - cold) * spell_mult * spell_block_mult;
    let lightning_taken = (1.0 - lightning) * spell_mult * spell_block_mult;
    let chaos_taken = (1.0 - chaos) * spell_mult * spell_block_mult;

    let phys_ehp = pool / phys_taken.max(0.05);
    let fire_ehp = pool / fire_taken.max(0.05);
    let cold_ehp = pool / cold_taken.max(0.05);
    let lightning_ehp = pool / lightning_taken.max(0.05);
    let chaos_ehp = pool / chaos_taken.max(0.05);

    env.output.set("PhysicalEHP", phys_ehp);
    env.output.set("FireEHP", fire_ehp);
    env.output.set("ColdEHP", cold_ehp);
    env.output.set("LightningEHP", lightning_ehp);
    env.output.set("ChaosEHP", chaos_ehp);
    let avg_ehp = (phys_ehp + fire_ehp + cold_ehp + lightning_ehp + chaos_ehp) / 5.0;
    env.output.set("AverageEHP", avg_ehp);
    // Worst-case (smallest of the five) — useful "what's the weakest hit type".
    let min_ehp = phys_ehp
        .min(fire_ehp)
        .min(cold_ehp)
        .min(lightning_ehp)
        .min(chaos_ehp);
    env.output.set("MinimumEHP", min_ehp);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::character::ClassRef;
    use crate::modifier::Mod;
    use std::path::PathBuf;

    fn load_3_25_tree() -> Option<PassiveTree> {
        let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .unwrap()
            .parent()
            .unwrap()
            .join("data/trees/3_25.json");
        let json = std::fs::read_to_string(&path).ok()?;
        pob_data::load_passive_tree(&json).ok()
    }

    #[test]
    fn marauder_level_1_naked() {
        let Some(tree) = load_3_25_tree() else {
            eprintln!("skip: data/trees/3_25.json missing");
            return;
        };
        let c = Character::new(ClassRef::marauder(), 1);
        let out = compute(&c, &tree);

        // Marauder base attributes: 32 / 14 / 14.
        assert_eq!(out.get("Strength"), 32.0);
        assert_eq!(out.get("Dexterity"), 14.0);
        assert_eq!(out.get("Intelligence"), 14.0);

        // Life: 50 base + Strength/2 = 50 + 16 = 66.
        assert_eq!(out.get("Life"), 66.0);

        // Mana: 40 base + Intelligence/2 = 40 + 7 = 47.
        assert_eq!(out.get("Mana"), 47.0);
    }

    #[test]
    fn marauder_level_90_naked() {
        let Some(tree) = load_3_25_tree() else {
            return;
        };
        let c = Character::new(ClassRef::marauder(), 90);
        let out = compute(&c, &tree);

        // Life: 50 + 12 * 89 = 1118 base + 32/2 = 1134.
        assert_eq!(out.get("Life"), 50.0 + 12.0 * 89.0 + 32.0 / 2.0);
        // Mana: 40 + 6 * 89 = 574 + 14/2 = 581.
        assert_eq!(out.get("Mana"), 40.0 + 6.0 * 89.0 + 14.0 / 2.0);
    }

    #[test]
    fn allocating_strength_node_increases_strength_and_life() {
        let Some(tree) = load_3_25_tree() else {
            return;
        };
        // Path validation: only credit nodes connected to the class start, so we
        // need to BFS for a `+10 to Strength` node reachable from the Marauder
        // start. We allocate every node along the path.
        let class_idx = tree
            .classes
            .iter()
            .position(|c| c.name == "Marauder")
            .unwrap() as u32;
        let start = tree
            .nodes
            .iter()
            .find_map(|(id, n)| (n.class_start_index == Some(class_idx)).then_some(*id))
            .expect("class start node");
        // BFS for a strength node within reasonable distance.
        let mut prev: std::collections::HashMap<pob_data::NodeId, pob_data::NodeId> =
            std::collections::HashMap::new();
        let mut queue: std::collections::VecDeque<_> = [start].into();
        let mut target: Option<pob_data::NodeId> = None;
        while let Some(n) = queue.pop_front() {
            if let Some(node) = tree.nodes.get(&n) {
                if n != start && node.stats == ["+10 to Strength"] {
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
        }
        let Some(target) = target else {
            eprintln!("no reachable +10 Strength node from Marauder start — skip");
            return;
        };
        let mut c = Character::new(ClassRef::marauder(), 1);
        // Walk back from target to start, allocating every node along the way.
        let mut walk = target;
        while let Some(&p) = prev.get(&walk) {
            c.allocate(walk);
            walk = p;
        }
        c.allocate(walk);
        // Re-compute baselines without the path so we know the contribution.
        let mut bare = Character::new(ClassRef::marauder(), 1);
        // Allocate every path node EXCEPT the strength target.
        let mut walk = target;
        let mut path = vec![target];
        while let Some(&p) = prev.get(&walk) {
            path.push(p);
            walk = p;
        }
        for n in path.iter().skip(1) {
            bare.allocate(*n);
        }
        let baseline = compute(&bare, &tree);
        let after = compute(&c, &tree);
        assert_eq!(
            after.get("Strength") - baseline.get("Strength"),
            10.0,
            "Strength delta from one '+10 Str' node along path"
        );
        assert_eq!(after.get("Life") - baseline.get("Life"), 5.0);
    }

    #[test]
    fn ranger_level_1_naked() {
        let Some(tree) = load_3_25_tree() else {
            return;
        };
        let c = Character::new(ClassRef::ranger(), 1);
        let out = compute(&c, &tree);
        assert_eq!(out.get("Strength"), 14.0);
        assert_eq!(out.get("Dexterity"), 32.0);
        assert_eq!(out.get("Intelligence"), 14.0);
    }

    #[test]
    fn resist_max_default_75() {
        let Some(tree) = load_3_25_tree() else {
            return;
        };
        let c = Character::new(ClassRef::marauder(), 1);
        let out = compute(&c, &tree);
        assert_eq!(out.get("FireResistMax"), 75.0);
        assert_eq!(out.get("ColdResistMax"), 75.0);
        assert_eq!(out.get("LightningResistMax"), 75.0);
        assert_eq!(out.get("ChaosResistMax"), 75.0);
    }

    #[test]
    fn fire_resist_capped_at_max() {
        let Some(tree) = load_3_25_tree() else {
            return;
        };
        let mut c = Character::new(ClassRef::marauder(), 1);
        // Synthesise: insert raw "+200% to Fire Resistance" into the env via a node we
        // know parses. Simpler: build the env manually.
        let mut env = init_env(&c, &tree);
        env.mod_db.add(Mod::base("FireResist", 200.0));
        perform_basic_stats(&c, &tree, &mut env);
        let out = env.output;
        assert_eq!(out.get("FireResist"), 200.0);
        assert_eq!(out.get("FireResistMax"), 75.0);
        assert_eq!(out.get("FireResistTotal"), 75.0);
        // Bumping the max should push the cap.
        let mut env2 = init_env(&c, &tree);
        env2.mod_db.add(Mod::base("FireResist", 200.0));
        env2.mod_db.add(Mod::base("FireResistMax", 5.0));
        perform_basic_stats(&c, &tree, &mut env2);
        assert_eq!(env2.output.get("FireResistMax"), 80.0);
        assert_eq!(env2.output.get("FireResistTotal"), 80.0);
        let _ = c.allocated; // silence clippy for unused
    }

    #[test]
    fn block_capped_at_75_by_default() {
        let Some(tree) = load_3_25_tree() else {
            return;
        };
        let c = Character::new(ClassRef::duelist(), 1);
        let mut env = init_env(&c, &tree);
        env.mod_db.add(Mod::base("BlockChance", 95.0));
        perform_basic_stats(&c, &tree, &mut env);
        assert_eq!(env.output.get("BlockChance"), 75.0);
        // With +5 to max block:
        let mut env2 = init_env(&c, &tree);
        env2.mod_db.add(Mod::base("BlockChance", 95.0));
        env2.mod_db.add(Mod::base("BlockChanceMax", 5.0));
        perform_basic_stats(&c, &tree, &mut env2);
        assert_eq!(env2.output.get("BlockChance"), 80.0);
    }

    #[test]
    fn life_regen_pct_uses_total_life() {
        let Some(tree) = load_3_25_tree() else {
            return;
        };
        let c = Character::new(ClassRef::marauder(), 1);
        let mut env = init_env(&c, &tree);
        env.mod_db.add(Mod::base("LifeRegenPercent", 5.0));
        perform_basic_stats(&c, &tree, &mut env);
        let life = env.output.get("Life");
        let regen = env.output.get("LifeRegen");
        // 5% of 66 = 3.3
        assert!((regen - life * 0.05).abs() < 1e-9);
    }

    #[test]
    fn witch_level_1_naked() {
        let Some(tree) = load_3_25_tree() else {
            return;
        };
        let c = Character::new(ClassRef::witch(), 1);
        let out = compute(&c, &tree);
        assert_eq!(out.get("Strength"), 14.0);
        assert_eq!(out.get("Dexterity"), 14.0);
        assert_eq!(out.get("Intelligence"), 32.0);
    }
}
