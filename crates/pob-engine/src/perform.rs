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
    compute_full_with_env(character, tree, skills, bases).0
}

/// Like `compute_full` but also returns the `Env` used during the calc, so the
/// UI can drill into the modifier chain (which mods contributed to a stat,
/// what their sources were, etc.) without recomputing.
pub fn compute_full_with_env(
    character: &Character,
    tree: &PassiveTree,
    skills: Option<&SkillRegistry>,
    bases: Option<&pob_data::bases::ItemBaseSet>,
) -> (Output, Env) {
    let mut env = init_env_with_bases(character, tree, bases);
    perform_basic_stats(character, tree, &mut env);
    if let Some(b) = bases {
        perform_flask_recovery(character, b, &mut env);
    }
    if let Some(reg) = skills {
        // Issue #97: party-member auto-extracted auras / curses /
        // banners. Each gem in `extracted_auras` contributes mods via
        // `aura_buff_mods`, sourced as `Party:<name>:<skill>`. Manual
        // `mod_lines` already landed in init_env_with_bases.
        apply_party_extracted_auras(character, reg, &mut env);
        perform_reservations(character, reg, &mut env);
        perform_curses(character, reg, &mut env);
        // Issue #19 (slice 3): emit aggregate warcry-loadout outputs
        // (count + total exert + min cooldown). Auto-uptime
        // derivation lives in slice 4; this slice ships the data
        // pipeline for it.
        detect_warcries(character, reg, &mut env);
        // Issue #5: Dual-wield per-weapon calc loop. PoB's CalcOffence.lua
        // runs the skill calc twice for dual-wielders — once with
        // `SlotName:Weapon 1` active and `SlotName:Weapon 2` inactive,
        // once with the two flipped — and averages MainSkillDPS /
        // AverageHit / HitChance across the two passes. Mirroring that
        // here also exposes Weapon1DPS / Weapon2DPS for the Calcs tab
        // side panel.
        if env.state.condition("DualWielding") {
            perform_dual_wield_skill_dps(character, reg, &mut env);
        } else {
            perform_skill_dps(character, reg, &mut env);
        }
    }
    perform_ehp(&mut env);
    perform_enemy_damage_sim(&mut env, character);
    (env.output.clone(), env)
}

/// Dual-wield: run `perform_skill_dps` twice, once per active hand, average
/// the headline DPS keys, and surface per-hand `Weapon{1,2}DPS` outputs.
/// Mirrors the dual-wield branch of CalcOffence.lua's per-pass loop.
fn perform_dual_wield_skill_dps(character: &Character, reg: &SkillRegistry, env: &mut Env) {
    let main_was = env.state.condition("SlotName:Weapon 1");
    let off_was = env.state.condition("SlotName:Weapon 2");

    // Pass 1: only Weapon 1 active.
    env.state.set_condition("SlotName:Weapon 1", true);
    env.state.set_condition("SlotName:Weapon 2", false);
    perform_skill_dps(character, reg, env);
    let weapon1_dps = env.output.get("MainSkillDPS");
    let weapon1_avg_hit = env.output.get("MainSkillAverageHit");
    let weapon1_hit_chance = env.output.get("MainSkillHitChance");
    let weapon1_full_dps = env.output.get("FullDPS");
    let weapon1_mana = env.output.get("ManaPerSecondCost");

    // Pass 2: only Weapon 2 active.
    env.state.set_condition("SlotName:Weapon 1", false);
    env.state.set_condition("SlotName:Weapon 2", true);
    perform_skill_dps(character, reg, env);
    let weapon2_dps = env.output.get("MainSkillDPS");
    let weapon2_avg_hit = env.output.get("MainSkillAverageHit");
    let weapon2_hit_chance = env.output.get("MainSkillHitChance");
    let weapon2_full_dps = env.output.get("FullDPS");
    let weapon2_mana = env.output.get("ManaPerSecondCost");

    // Restore the original SlotName state. Other downstream calcs
    // (perform_ehp, perform_enemy_damage_sim) read from `env` and need
    // both hands' mods active to mirror PoB's defensive-side eval.
    env.state.set_condition("SlotName:Weapon 1", main_was);
    env.state.set_condition("SlotName:Weapon 2", off_was);

    // Average the headline keys and emit per-hand breakdowns. Issue
    // #74 added the per-hand hit-average / hit-chance / full-DPS keys
    // so the Calcs tab can show the alternation explicitly — Cleave /
    // Reave / Frenzy etc. strike with one hand per repetition, so the
    // per-hand pre-averaging values are the right thing to display.
    env.output.set("Weapon1DPS", weapon1_dps);
    env.output.set("Weapon2DPS", weapon2_dps);
    env.output.set("Weapon1AverageHit", weapon1_avg_hit);
    env.output.set("Weapon2AverageHit", weapon2_avg_hit);
    env.output.set("Weapon1HitChance", weapon1_hit_chance);
    env.output.set("Weapon2HitChance", weapon2_hit_chance);
    env.output.set("Weapon1FullDPS", weapon1_full_dps);
    env.output.set("Weapon2FullDPS", weapon2_full_dps);
    env.output
        .set("MainSkillDPS", f64::midpoint(weapon1_dps, weapon2_dps));
    env.output.set(
        "MainSkillAverageHit",
        f64::midpoint(weapon1_avg_hit, weapon2_avg_hit),
    );
    env.output.set(
        "MainSkillHitChance",
        f64::midpoint(weapon1_hit_chance, weapon2_hit_chance),
    );
    env.output
        .set("FullDPS", f64::midpoint(weapon1_full_dps, weapon2_full_dps));
    let avg_mana = f64::midpoint(weapon1_mana, weapon2_mana);
    if avg_mana > 0.0 {
        env.output.set("ManaPerSecondCost", avg_mana);
    }
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
    env.mod_db
        .add(Mod::base("Evasion", 15.0).with_source(Source::Other("CharacterConstant".into())));

    // Issue #84: base mine / trap throw timings. Mirrors PoB's
    // CalcSetup.lua:52-53 — these are the default per-throw seconds
    // before any per-skill overrides or % faster mods apply.
    //   TrapThrowingTime BASE = 0.6 s
    //   MineLayingTime   BASE = 0.3 s
    env.mod_db.add(
        Mod::base("TrapThrowingTime", 0.6).with_source(Source::Other("CharacterConstant".into())),
    );
    env.mod_db.add(
        Mod::base("MineLayingTime", 0.3).with_source(Source::Other("CharacterConstant".into())),
    );

    // Issue #19 (slice 2): WarcryPower config knob. Mirrors PoB's
    // `multiplierWarcryPower` Config-tab input from
    // `Modules/ConfigOptions.lua:723-725`. When the user pins a value
    // we OVERRIDE it onto WarcryPower (so warcry-scaling formulas
    // read the right number) and set `Multiplier:WarcryPower` BASE
    // so PerStat-tagged mods (e.g. "+1% damage per 5 Warcry Power")
    // pick it up. Power tracks total strength of nearby enemies in
    // PoE — PoB defaults to 20 (boss target); MK2 leaves the field
    // unset so a build without warcries doesn't carry stale state.
    if let Some(power) = character.config.warcry_power {
        let power_f = f64::from(power);
        env.mod_db
            .add(Mod::base("WarcryPower", power_f).with_source(Source::Other("Config".into())));
        env.mod_db.add(
            Mod::base("Multiplier:WarcryPower", power_f)
                .with_source(Source::Other("Config".into())),
        );
    }

    // 3. Tree node stats. Parse each allocated node's stat lines. PoB only credits
    // nodes that form a connected path from the character's class start, so we
    // filter the allocation set to the connected subgraph before applying mods.
    // (Disconnected node IDs come from imported XML where the user manually edited
    // the file — the in-app UI only ever allocates valid paths.)
    let effective: ahash::AHashSet<pob_data::NodeId> = connected_allocations(character, tree);
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
                if let Some(effect) = node.mastery_effects.iter().find(|e| e.effect == effect_id) {
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
        // Issue #29: Tattoo override. Tattoos (3.22+) replace a single
        // allocated normal passive node's stats with a chosen tattoo's
        // mod text. PoB tracks them in `PassiveSpec.tattooOverrides`.
        // We mirror that here: if the user has set
        // `tattoo_overrides[node_id]` to a non-empty string, parse that
        // text instead of the node's `stats`. Parse failures are
        // silently skipped — the tattoo's lines come from canonical
        // PoB text and almost always parse, but if any don't they
        // simply don't contribute (matching the custom_mods behaviour).
        if let Some(override_text) = character.tattoo_overrides.get(node_id) {
            if !override_text.trim().is_empty() {
                for line in override_text.lines() {
                    let line = line.trim();
                    if line.is_empty() {
                        continue;
                    }
                    if let Some(parsed) = parse_mod_line(line) {
                        env.mod_db
                            .add(parsed.mod_.with_source(Source::Passive(*node_id)));
                    }
                }
                continue;
            }
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

    // 4. Items. Issue #109 slice 2: when `use_second_weapon_set` is
    // on, project the swap pair onto Weapon1/Weapon2 for the rest of
    // the compute pass. We materialise a thin "live" view rather than
    // mutating `character.items` so the source build remains
    // unchanged for downstream callers (Calcs side-panel, snapshot
    // export, etc.).
    let live_items = effective_items_for_compute(character);
    let live_items_ref: &pob_data::ItemSet = &live_items;
    let _ = crate::item_parser::apply_item_set_with_bases(live_items_ref, &mut env.mod_db, bases);
    // Set SlotName conditions for slots that have an item — supports SlotName tags on
    // mods that say "while using a shield" / "while wielding a bow", etc.
    for (slot, _) in live_items_ref.iter() {
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
            // Swap-set entries that survive the projection (i.e. when
            // `use_second_weapon_set` is *off*) don't drive the live
            // `SlotName:` flags — only the active pair does.
            pob_data::Slot::Weapon1Swap | pob_data::Slot::Weapon2Swap => continue,
            pob_data::Slot::Flask1 => "Flask 1",
            pob_data::Slot::Flask2 => "Flask 2",
            pob_data::Slot::Flask3 => "Flask 3",
            pob_data::Slot::Flask4 => "Flask 4",
            pob_data::Slot::Flask5 => "Flask 5",
        };
        env.state
            .set_condition_prefixed("SlotName", slot_name, true);
    }

    // 5. Auto-detected wielding conditions from the equipped items. These activate
    // mods that have a "while using a shield" / "while wielding a two handed weapon" /
    // "while dual wielding" trailing clause via their parser-emitted Condition tag.
    detect_wielding_conditions(live_items_ref, &mut env.state);
    detect_rarity_slot_conditions(live_items_ref, &mut env.state);

    // 6. Config — push conditions and multipliers into the eval state. ConfigState
    // overrides auto-detection if the user has explicitly set the same key.
    for (k, v) in &character.config.conditions {
        env.state.set_condition(k.clone(), *v);
    }
    for (k, v) in &character.config.multipliers {
        env.state.set_multiplier(k.clone(), *v);
    }
    // 6b. Custom modifiers — user-typed lines from the Config-tab textarea.
    // Parse each non-empty line through `mod_parser` and add it with
    // `source = Custom` so the Calcs-tab breakdown can identify them.
    // Mirrors PoB's ConfigTab "Custom Modifiers" feature. Lines that fail
    // to parse are silently skipped (the UI surfaces parse errors separately).
    for line in character.config.custom_mods.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }
        if let Some(parsed) = crate::mod_parser::parse_mod_line(trimmed) {
            env.mod_db
                .add(parsed.mod_.with_source(Source::Other("Custom".into())));
        }
    }
    // 6c. Party members — group-play teammates whose auras / curses /
    // banners propagate onto the player. Mirrors PoB's `partyTab` in
    // `Modules/Build.lua`. Each enabled member's `mod_lines` are parsed
    // through `parse_mod_line` and added to the player modDB with
    // `source = Source::Other("Party:<name>")`.
    for member in &character.party_members {
        if !member.enabled {
            continue;
        }
        let source_label = format!("Party:{}", member.name);
        for line in member.mod_lines.lines() {
            let trimmed = line.trim();
            if trimmed.is_empty() {
                continue;
            }
            if let Some(parsed) = crate::mod_parser::parse_mod_line(trimmed) {
                env.mod_db
                    .add(parsed.mod_.with_source(Source::Other(source_label.clone())));
            }
        }
    }

    // 7. Act 2 bandit reward. KillAll grants +2 passive points (counted
    // against the tree-budget elsewhere); the named bandits inject a small
    // package of static mods. Mirrors PoB's bandit branch in
    // `Modules/CalcSetup.lua`.
    apply_bandit_mods(character.bandit, &mut env.mod_db);

    // 7b. Pantheon. Major + Minor god each contribute their soul[1]
    // (level-1) effect — a single mod text run through `mod_parser`
    // and added with `source = "Pantheon:<god>"`. Mirrors PoB's
    // `Data/Pantheons.lua` data, applied in CalcSetup.lua:545-554.
    apply_pantheon_mods(
        character.pantheon_major,
        character.pantheon_minor,
        &mut env.mod_db,
    );

    // 8. Enemy preset (Boss / Pinnacle / Uber). Sets the
    // RareOrUnique / PinnacleBoss conditions and the AilmentThreshold MORE
    // multiplier that PoB's `enemyIsBoss` ConfigOption applies. Defensive
    // resist defaults are surfaced via the UI (which writes them into
    // ConfigState directly), not injected here, to keep the user's
    // explicit overrides intact.
    apply_enemy_boss_preset(character.config.enemy_boss, &mut env);

    env
}

/// Inject the static mods awarded by the chosen Act 2 bandit. Numbers mirror
/// upstream PoB exactly — see `.PathOfBuilding/src/Modules/CalcSetup.lua:531-540`,
/// which inlines a single mod per bandit. `KillAll` adds an `ExtraPoints` BASE
/// of 1 (the "+2 passive points" reward).
/// Issue #97: project a party member's auto-extracted aura / curse /
/// banner gems into the player's modDB. Each enabled `ExtractedAura`
/// is looked up in the `SkillRegistry`; `aura_buff_mods` returns the
/// mod list that gem grants its allies at the chosen level + quality.
/// Each mod is re-sourced as `Source::Other("Party:<member>:<skill>")`
/// so the Calcs-tab breakdown can attribute it back to the teammate.
/// Disabled members and disabled gems contribute nothing.
fn apply_party_extracted_auras(character: &Character, skills: &SkillRegistry, env: &mut Env) {
    for member in &character.party_members {
        if !member.enabled {
            continue;
        }
        for aura in &member.extracted_auras {
            if !aura.enabled {
                continue;
            }
            let Some(skill) = skills.get(&aura.skill_id) else {
                continue;
            };
            let level = aura.level.clamp(1, 40);
            let mods = crate::skill::aura_buff_mods(skill, level, aura.quality);
            let source_label = format!("Party:{}:{}", member.name, aura.skill_id);
            for mut m in mods {
                m.source = Some(Source::Other(source_label.clone()));
                env.mod_db.add(m);
            }
        }
    }
}

fn apply_bandit_mods(bandit: crate::character::Bandit, db: &mut crate::ModDB) {
    use crate::character::Bandit;
    let source = Source::Other(format!("Bandit:{}", bandit.as_pob_name()));
    match bandit {
        Bandit::KillAll => {
            db.add(Mod::base("ExtraPoints", 1.0).with_source(source));
        }
        Bandit::Alira => {
            db.add(Mod::base("ElementalResist", 15.0).with_source(source));
        }
        Bandit::Kraityn => {
            db.add(Mod::inc("MovementSpeed", 8.0).with_source(source));
        }
        Bandit::Oak => {
            db.add(Mod::base("Life", 40.0).with_source(source));
        }
    }
}

/// Apply the chosen enemy-boss preset. Mirrors `enemyIsBoss` in
/// upstream PoB (`Modules/ConfigOptions.lua:2014`), which on selection
/// emits `Condition:RareOrUnique` (Boss/Pinnacle/Uber) and
/// `Condition:PinnacleBoss` (Pinnacle/Uber) flags into the enemy mod
/// list, plus an `AilmentThreshold` MORE multiplier that scales how
/// much hit damage is needed to apply ailments. The threshold values
/// match upstream: 488 for standard Boss, 404 for Pinnacle/Uber.
fn apply_enemy_boss_preset(boss: crate::character::EnemyBoss, env: &mut Env) {
    use crate::character::EnemyBoss;
    let source = Source::Other(format!("EnemyBoss:{}", boss.as_pob_name()));
    match boss {
        EnemyBoss::None => {}
        EnemyBoss::Boss => {
            env.state.set_condition("RareOrUnique", true);
            env.mod_db
                .add(Mod::more("AilmentThreshold", 488.0).with_source(source.clone()));
        }
        EnemyBoss::Pinnacle | EnemyBoss::Uber => {
            env.state.set_condition("RareOrUnique", true);
            env.state.set_condition("PinnacleBoss", true);
            env.mod_db
                .add(Mod::more("AilmentThreshold", 404.0).with_source(source.clone()));
        }
    }
    // Issue #75: per-preset elemental penetration the player must
    // overcome on the boss. Mirrors `pinnacleBossPen = 15/5 = 3` and
    // `uberBossPen = 40/5 = 8` from `Data.lua`. Adds a generic
    // `ElementalPenetration` BASE mod that the existing pen-aggregation
    // path in `perform_skill_dps` already consumes (see
    // `elem_pen = ... + sum(BASE, ElementalPenetration)`). Boss / None
    // contribute 0.
    let pen = boss.default_penetration();
    if pen > 0 {
        env.mod_db
            .add(Mod::base("ElementalPenetration", f64::from(pen)).with_source(source.clone()));
    }
    // Surface the canonical PoB damage-taken multiplier per preset on
    // the output for callers that want to display "ratio of monster
    // damage taken" — purely informational; the engine doesn't fold
    // this into MainSkillDPS (PoB models monster damage scaling, not
    // player DPS scaling).
    let dps_taken = boss.dps_taken_multiplier();
    if (dps_taken - 1.0).abs() > 1e-6 {
        env.output.set("EnemyBossDpsTakenMultiplier", dps_taken);
    }
}

/// Inject Pantheon Major + Minor God soul effects into the player
/// modDB. Mirrors upstream PoB's `PantheonTools.applySoulMod`, which
/// iterates over **every** soul level (1 through 4 for majors, 1
/// through 2 for minors) for the selected god — PoB assumes the
/// player has fully upgraded their souls via Divine Vessel, since
/// soul-level state is not stored in the build XML. Each mod line is
/// run through `mod_parser::parse_mod_line`; lines that fail to parse
/// (a handful have conditional gating the parser doesn't yet model,
/// e.g. Solaris's "while there is only one nearby Enemy") are silently
/// skipped — the mod_db will still carry the god tag as a
/// `Source::Other("Pantheon:<god>")` no-op for downstream tools that
/// want to know which god is selected.
///
/// Tabled mod data is sourced from `Data/Pantheons.lua` in the
/// upstream PoB tree (commit pinned in `.PathOfBuilding`). When the
/// upstream data drifts, regenerate by walking that file's
/// `souls[].mods[].line` columns.
fn apply_pantheon_mods(
    major: crate::character::MajorGod,
    minor: crate::character::MinorGod,
    db: &mut crate::ModDB,
) {
    use crate::character::{MajorGod, MinorGod};

    // Major god soul tables — outer slice indexed by soul level 1..N,
    // inner slice is one or more mod lines for that level.
    let major_souls: &[&[&str]] = match major {
        MajorGod::None => &[],
        MajorGod::TheBrineKing => &[
            // soul[1] — Soul of the Brine King
            &["You cannot be Stunned if you've been Stunned or Blocked a Stunning Hit in the past 2 seconds"],
            // soul[2] — Puruna, the Challenger
            &["30% increased Stun and Block Recovery"],
            // soul[3] — Merveil, the Returned
            &["100% chance to Avoid being Frozen"],
            // soul[4] — Nassar, Lion of the Seas
            &["50% reduced Effect of Chill on you"],
        ],
        MajorGod::Arakaali => &[
            &["10% reduced Damage taken from Damage Over Time"],
            &["20% increased Recovery rate of Life and Energy Shield if you've stopped taking Damage Over Time Recently"],
            &["Debuffs on you expire 20% faster"],
            &["+40% Chaos Resistance against Damage Over Time"],
        ],
        MajorGod::Solaris => &[
            &[
                "6% additional Physical Damage Reduction while there is only one nearby Enemy",
                "20% chance to take 50% less Area Damage from Hits",
            ],
            &["8% reduced Elemental Damage taken if you haven't been Hit Recently"],
            &["Take no Extra Damage from Critical Strikes if you have taken a Critical Strike Recently"],
            &["50% chance to avoid Ailments from Critical Strikes"],
        ],
        MajorGod::Lunaris => &[
            &[
                "1% additional Physical Damage Reduction for each nearby Enemy, up to 8%",
                "1% increased Movement Speed for each nearby Enemy, up to 8%",
            ],
            &["10% chance to avoid Projectiles"],
            &["6% reduced Elemental Damage taken if you have been Hit Recently"],
            &["Avoid Projectiles that have Chained"],
        ],
    };
    if !major_souls.is_empty() {
        let source = Source::Other(format!("Pantheon:{}", major.as_pob_name()));
        for soul in major_souls {
            for line in *soul {
                if let Some(parsed) = crate::mod_parser::parse_mod_line(line) {
                    db.add(parsed.mod_.with_source(source.clone()));
                }
            }
        }
    }

    // Minor god soul tables — minors max out at soul level 2.
    let minor_souls: &[&[&str]] = match minor {
        MinorGod::None => &[],
        MinorGod::Abberath => &[
            &["60% less Duration of Ignite on You"],
            &[
                "Unaffected by Burning Ground",
                "10% increased Movement Speed while on Burning Ground",
            ],
        ],
        MinorGod::Gruthkul => &[
            &["1% additional Physical Damage Reduction for each Hit you've taken Recently up to a maximum of 5%"],
            &["Enemies that have Hit you with an Attack Recently have 8% reduced Attack Speed"],
        ],
        MinorGod::Yugul => &[
            &[
                "50% of Hit Damage from you and your Minions cannot be Reflected",
                "50% chance to Reflect Hexes",
            ],
            &["30% reduced Effect of Curses on you"],
        ],
        MinorGod::Shakari => &[
            &[
                "50% less Duration of Poisons on You",
                "You cannot be Poisoned while there are at least 3 Poisons on you",
            ],
            &[
                "5% reduced Chaos Damage taken",
                "25% reduced Chaos Damage over Time taken while on Caustic Ground",
            ],
        ],
        MinorGod::Tukohama => &[
            &["3% additional Physical Damage Reduction per second you've been stationary, up to a maximum of 9%"],
            &["Regenerate 2% of Life per second while stationary"],
        ],
        MinorGod::Ralakesh => &[
            &[
                "25% reduced Physical Damage over Time taken while moving",
                "Moving while Bleeding doesn't cause you to take extra Damage",
            ],
            &["Corrupted Blood cannot be inflicted on you if you have at least 5 Corrupted Blood Debuffs on you"],
        ],
        MinorGod::Garukhan => &[
            &["60% reduced Effect of Shock on you"],
            &[
                "Cannot be Blinded",
                "You cannot be Maimed",
            ],
        ],
        MinorGod::Ryslatha => &[
            &[
                "Life Flasks gain 3 Charges every 3 seconds if you haven't used a Life Flask Recently",
                "60% increased Life Recovery from Flasks used when on Low Life",
            ],
            &["Enemies you've Hit Recently have 50% reduced Life Regeneration rate"],
        ],
    };
    if !minor_souls.is_empty() {
        let source = Source::Other(format!("Pantheon:{}", minor.as_pob_name()));
        for soul in minor_souls {
            for line in *soul {
                if let Some(parsed) = crate::mod_parser::parse_mod_line(line) {
                    db.add(parsed.mod_.with_source(source.clone()));
                }
            }
        }
    }
}

/// Issue #109 slice 2: materialise the "live" item set the calc
/// engine should read for this compute pass. When
/// `config.use_second_weapon_set` is `false` (default) this is just
/// `character.items` — no allocation. When `true` the swap pair
/// (`Weapon1Swap` / `Weapon2Swap`) replaces the primary pair and the
/// originals are dropped from the projection. Mirrors PoB's "swap
/// active / passive" UI gesture.
fn effective_items_for_compute(character: &Character) -> std::borrow::Cow<'_, pob_data::ItemSet> {
    use pob_data::Slot;
    if !character.config.use_second_weapon_set {
        return std::borrow::Cow::Borrowed(&character.items);
    }
    // Only allocate the swapped view when the user has actually
    // toggled the swap on AND the swap pair carries at least one
    // weapon. Otherwise fall through — toggling on with no swap pair
    // shouldn't silently strip the live weapons.
    let swap_main = character.items.get(Slot::Weapon1Swap).cloned();
    let swap_off = character.items.get(Slot::Weapon2Swap).cloned();
    if swap_main.is_none() && swap_off.is_none() {
        return std::borrow::Cow::Borrowed(&character.items);
    }
    let mut projected = character.items.clone();
    // Drop both swap entries from the projection so they don't
    // double-count downstream, then install them onto the primary
    // slots. Empty swap slots fall through to "no weapon equipped"
    // for that hand.
    projected.unequip(Slot::Weapon1Swap);
    projected.unequip(Slot::Weapon2Swap);
    projected.unequip(Slot::Weapon1);
    projected.unequip(Slot::Weapon2);
    if let Some(item) = swap_main {
        projected.equip(Slot::Weapon1, item);
    }
    if let Some(item) = swap_off {
        projected.equip(Slot::Weapon2, item);
    }
    std::borrow::Cow::Owned(projected)
}

fn detect_wielding_conditions(items: &pob_data::ItemSet, state: &mut crate::mod_db::EvalState) {
    use pob_data::Slot;
    let weapon1 = items.get(Slot::Weapon1);
    let weapon2 = items.get(Slot::Weapon2);

    let weapon2_is_shield =
        weapon2.is_some_and(|i| i.base_name.contains("Shield") || i.base_name.contains("Buckler"));
    if weapon2_is_shield {
        state.set_condition("UsingShield", true);
    }

    let weapon1_is_two_handed = weapon1.is_some_and(|i| {
        i.base_name.contains("Two Handed")
            || i.base_name.contains("Bow")
            || i.base_name.contains("Staff")
            || i.base_name.contains("Quarterstaff")
    });
    if weapon1_is_two_handed {
        state.set_condition("UsingTwoHandedWeapon", true);
    }

    let weapon1_is_one_handed = weapon1.is_some() && !weapon1_is_two_handed;
    let weapon2_is_one_handed_weapon = weapon2.is_some() && !weapon2_is_shield;
    if weapon1_is_one_handed && weapon2_is_one_handed_weapon {
        state.set_condition("DualWielding", true);
    }
    // PoB also distinguishes one-handed wielding (one or two 1H weapons in either
    // hand, regardless of shield) — used by "while wielding a one handed weapon" mods.
    if weapon1_is_one_handed {
        state.set_condition("UsingOneHandedWeapon", true);
    }

    // Per-weapon-type conditions ("while wielding a Bow/Staff/Sword/...") so item mods
    // gated on weapon class apply correctly. We use the base name as a heuristic — the
    // canonical PoB approach reads the base type's class from item data, but base-name
    // matching is sufficient for the common bases.
    if let Some(w) = weapon1 {
        let n = &w.base_name;
        let pairs: &[(&str, &str)] = &[
            ("Bow", "UsingBow"),
            ("Quarterstaff", "UsingQuarterstaff"),
            ("Staff", "UsingStaff"),
            ("Wand", "UsingWand"),
            ("Sword", "UsingSword"),
            ("Axe", "UsingAxe"),
            ("Mace", "UsingMace"),
            ("Sceptre", "UsingSceptre"),
            ("Claw", "UsingClaw"),
            ("Dagger", "UsingDagger"),
        ];
        // Longest-match-first: "Quarterstaff" before "Staff", "Sceptre" before "Mace".
        for (needle, var) in pairs {
            if n.contains(needle) {
                state.set_condition(*var, true);
                break;
            }
        }
        // Melee weapon: anything not a Bow / Wand / Staff (Quarterstaff is melee in PoE2).
        let is_ranged = n.contains("Bow") || n.contains("Wand");
        let is_caster_staff = n.contains("Staff") && !n.contains("Quarterstaff");
        if !is_ranged && !is_caster_staff {
            state.set_condition("UsingMeleeWeapon", true);
        }
    }
}

/// Set rarity + slot conditions that gate item mods like
/// "if you have a Magic Ring in left slot" or "while you have a Rare Helmet equipped".
/// Mirrors PoB's slot-conditional resolver: ring slots are 1=left, 2=right.
fn detect_rarity_slot_conditions(items: &pob_data::ItemSet, state: &mut crate::mod_db::EvalState) {
    use pob_data::{Rarity, Slot};
    let rarity_str = |r: Rarity| -> &'static str {
        match r {
            Rarity::Magic => "Magic",
            Rarity::Rare => "Rare",
            Rarity::Normal => "Normal",
            Rarity::Unique => "Unique",
            // Relic items count as Unique for parity with PoB's `RareItemIn...` lookup.
            Rarity::Relic => "Unique",
        }
    };
    for slot in Slot::all() {
        let Some(item) = items.get(*slot) else {
            continue;
        };
        let rarity = rarity_str(item.rarity);
        let (kind, slot_idx) = match slot {
            Slot::Ring1 => ("Ring", Some(1u32)),
            Slot::Ring2 => ("Ring", Some(2u32)),
            Slot::Amulet => ("Amulet", None),
            Slot::Helmet => ("Helmet", None),
            Slot::BodyArmour => ("Body Armour", None),
            Slot::Gloves => ("Gloves", None),
            Slot::Boots => ("Boots", None),
            Slot::Belt => ("Belt", None),
            // Shields are tracked via the existing UsingShield condition, not a per-rarity tag.
            _ => continue,
        };
        // "if you have a Magic Ring in left slot" → MagicItemInRing 1
        if let Some(idx) = slot_idx {
            state.set_condition(format!("{rarity}ItemIn{kind} {idx}"), true);
        }
        // "while you have a Magic Ring equipped" → HaveMagicRingEquipped (any slot OK).
        // Set once per rarity-kind pair so dual rings of the same rarity collapse.
        state.set_condition(format!("Have{rarity}{kind}Equipped"), true);
    }
}

/// Fill output keys that PoB defaults to fixed game constants (max ailment
/// magnitudes, charge limits, self-ailment durations etc.). They're not
/// derived from character state so we set them once.
fn fill_static_defaults(env: &mut Env) {
    let life = env.output.get("Life");
    let mana = env.output.get("Mana");

    // Caps and missing-resist deltas.
    env.output.set("DamageReductionMax", 90.0);
    env.output.set("SpellBlockChanceMax", 75.0);
    for &(elem, missing_key, over_time_key) in &[
        ("Fire", "MissingFireResist", "FireResistOverTime"),
        ("Cold", "MissingColdResist", "ColdResistOverTime"),
        (
            "Lightning",
            "MissingLightningResist",
            "LightningResistOverTime",
        ),
        ("Chaos", "MissingChaosResist", "ChaosResistOverTime"),
    ] {
        let total = env.output.get_concat(elem, "ResistTotal");
        let cap = env.output.get_concat(elem, "ResistMax");
        env.output.set(missing_key, (cap - total).max(0.0));
        env.output.set(over_time_key, total);
    }
    // Totems start at +100% to ele resists and +80% to chaos resist on top of
    // the level penalty (so -60 → +40 / +20 net at level 90).
    env.output.set("TotemFireResist", 40.0);
    env.output.set("TotemColdResist", 40.0);
    env.output.set("TotemLightningResist", 40.0);
    env.output.set("TotemChaosResist", 20.0);
    env.output.set("TotemFireResistTotal", 40.0);
    env.output.set("TotemColdResistTotal", 40.0);
    env.output.set("TotemLightningResistTotal", 40.0);
    env.output.set("TotemChaosResistTotal", 20.0);
    env.output.set("MissingTotemFireResist", 35.0);
    env.output.set("MissingTotemColdResist", 35.0);
    env.output.set("MissingTotemLightningResist", 35.0);
    env.output.set("MissingTotemChaosResist", 55.0);

    // Maximum ailment magnitudes (PoE rules).
    env.output.set("MaximumShock", 50.0);
    env.output.set("MaximumChill", 30.0);
    env.output.set("MaximumScorch", 30.0);
    env.output.set("MaximumSap", 20.0);
    env.output.set("MaximumBrittle", 6.0);
    // Default ailment durations on self (1s ignite/poison feels like 4 to PoB
    // because it stacks).
    env.output.set("IgniteDuration", 4.0);
    // Self-ailment effect / duration multipliers default to 100%.
    for k in [
        "SelfBleedDuration",
        "SelfBleedEffect",
        "SelfBlindDuration",
        "SelfBrittleDuration",
        "SelfBrittleEffect",
        "SelfChillDuration",
        "SelfChillEffect",
        "SelfFreezeDuration",
        "SelfFreezeEffect",
        "SelfIgniteDuration",
        "SelfIgniteEffect",
        "SelfPoisonDuration",
        "SelfPoisonEffect",
        "SelfSapDuration",
        "SelfSapEffect",
        "SelfScorchDuration",
        "SelfScorchEffect",
        "SelfShockDuration",
        "SelfShockEffect",
        "SelfStunChance",
        "WitherEffectOnSelf",
        "CurseEffectOnSelf",
        "ExposureEffectOnSelf",
        "BlockEffect",
        "ChaosEnergyShieldBypass",
        "ConfiguredDamageChance",
        "DebuffExpirationModifier",
        "FullLifePercentage",
        "LifeCancellableReservation",
        "ManaCancellableReservation",
    ] {
        env.output.set(k, 100.0);
    }
    env.output.set("LowLifePercentage", 50.0);

    // Default charge counts and durations.
    env.output.set("BloodCharges", 5.0);
    env.output.set("BloodChargesMax", 5.0);
    env.output.set("InspirationCharges", 5.0);
    env.output.set("InspirationChargesMax", 5.0);
    env.output.set("EnduranceChargesMax", 3.0);
    env.output.set("FrenzyChargesMax", 3.0);
    env.output.set("PowerChargesMax", 3.0);
    env.output.set("EnduranceChargesDuration", 10.0);
    env.output.set("FrenzyChargesDuration", 10.0);
    env.output.set("PowerChargesDuration", 10.0);

    // Per-skill / per-totem defaults.
    env.output.set("ActiveMineLimit", 15.0);
    env.output.set("ActiveTrapLimit", 15.0);
    env.output.set("WeaponRange", 8.0);
    // Issue #19 (slice 2): expose `WarcryPower` derived from BASE
    // mods. Mirrors `CalcPerform.lua:1244` —
    // `output.WarcryPower = sum(BASE, WarcryPower) or 0`. The
    // user-facing default in PoB's tooltip is 20 (boss target);
    // we honour it as a floor so existing builds without an explicit
    // config knob still see the same number a "boss" assumption
    // would produce.
    let cfg_q = QueryCfg::default();
    let warcry_power_base = env
        .mod_db
        .sum(ModType::Base, &cfg_q, &env.state, "WarcryPower")
        .max(20.0);
    env.output.set("WarcryPower", warcry_power_base);
    env.output.set("EnemyCritChance", 5.0);
    // Hit chance: spells always hit (100%); attacks roll vs accuracy. Only set
    // a default of 0 if the skill DPS pass hasn't already populated this — the
    // ordering is perform_basic_stats → perform_skill_dps → perform_ehp, so
    // by the time perform_ehp runs HitChance may already be set.
    if env.output.try_get("HitChance").is_none() {
        env.output.set("AccuracyHitChance", 0.0);
        env.output.set("HitChance", 0.0);
    }
    env.output.set("MeleeEvasion", env.output.get("Evasion"));
    env.output
        .set("ProjectileEvasion", env.output.get("Evasion"));
    env.output.set("SpellSuppressionEffect", 40.0);

    // Attribute aliases (PoB exposes both forms).
    let str_v = env.output.get("Strength");
    let dex_v = env.output.get("Dexterity");
    let int_v = env.output.get("Intelligence");
    env.output.set("Str", str_v);
    env.output.set("Dex", dex_v);
    env.output.set("Int", int_v);
    env.output.set("TotalAttr", str_v + dex_v + int_v);
    env.output
        .set("LowestAttribute", str_v.min(dex_v).min(int_v));

    // Life/mana derivatives.
    env.output
        .set("LowestOfMaximumLifeAndMaximumMana", life.min(mana));
    // Leech caps: 20% of max pool per second by default; per-instance is 10% of
    // base regen rate ~= 0.02 * pool.
    let life_leech_rate = 0.20 * life;
    let mana_leech_rate = 0.20 * mana;
    env.output.set("MaxLifeLeechRate", life_leech_rate);
    env.output.set("MaxManaLeechRate", mana_leech_rate);
    env.output.set("MaxLifeLeechRatePercent", 20.0);
    env.output.set("MaxLifeLeechInstance", 0.10 * life);
    env.output.set("MaxManaLeechInstance", 0.10 * mana);
    env.output.set("LifeLeechInstanceRate", 0.02 * life);
    env.output.set("ManaLeechInstanceRate", 0.02 * mana);
    env.output
        .set("ManaRegenRecovery", env.output.get("ManaRegen"));

    // Ignite-related: average of min/max ignite damage roll, fixed 50% baseline.
    env.output.set("IgniteRollAverage", 50.0);

    // Enemy penetration defaults (PoB's "Boss" enemy preset): 3% to elements,
    // 0 to chaos/physical. Push each into the ouptut so per-element TakenHitMult
    // calcs below can pick them up.
    env.output.set("FireEnemyPen", 3.0);
    env.output.set("ColdEnemyPen", 3.0);
    env.output.set("LightningEnemyPen", 3.0);

    // Damage-taken multipliers per element: (1 - resist% + pen%) clamped at the
    // -200% cap. PoB exposes a stack of *TakenHitMult names that all share this
    // value when no situational mods (attack-only / spell-only / reflect) apply.
    for elem in ["Fire", "Cold", "Lightning", "Chaos", "Physical"] {
        // Physical doesn't read from a resist key; default mult is 1.0.
        let mult = if elem == "Physical" {
            1.0
        } else {
            let resist = env.output.get_concat(elem, "ResistTotal");
            let pen = env.output.get_concat(elem, "EnemyPen");
            (1.0 - (resist - pen) / 100.0).clamp(0.05, 3.0)
        };
        for suffix in [
            "TakenHitMult",
            "BaseTakenHitMult",
            "ResistTakenHitMulti",
            "TakenDotMult",
        ] {
            env.output.set_concat(elem, suffix, mult);
        }
        // Per-context multipliers (attack/spell, after-reduction, reflect) all
        // default to 1.0 in the no-mods case.
        for suffix in [
            "AttackTakenHitMult",
            "SpellTakenHitMult",
            "AfterReductionTakenHitMulti",
            "TakenReflect",
            "EnemyDamageMult",
        ] {
            env.output.set_concat(elem, suffix, 1.0);
        }
    }
    // Top-level multipliers / mods that PoB always emits at 1.0 baseline.
    for k in [
        "ActionSpeedMod",
        "AilmentWarcryEffect",
        "AttackTakenHitMult",
        "AverageBurstHits",
        "CullMultiplier",
        "DurationMod",
        "EffectiveMovementSpeedMod",
        "EnergyShieldRecoveryRateMod",
        "EnemyCurseLimit",
        "ImpaleDurationMod",
        "LifeRecoveryRateMod",
        "LightRadiusMod",
        "ManaRecoveryRateMod",
        "MaxOffensiveWarcryEffect",
        "OffensiveWarcryEffect",
        "RallyingHitEffect",
        "Repeats",
        "ReservationDpsMultiplier",
        "SpellTakenHitMult",
        "StrikeTargets",
        "TheoreticalMaxOffensiveWarcryEffect",
        "TheoreticalOffensiveWarcryEffect",
        "TotemDurationMod",
        "IgniteStacksMax",
        "ExtraPoints",
    ] {
        env.output.set(k, 1.0);
    }
    // Recharge / regen rates and small constants.
    env.output.set("EnergyShieldRechargeDelay", 2.0);
    env.output.set("WardRechargeDelay", 2.0);
    env.output.set("ManaRegenPercent", 1.7);
    env.output.set("Speed", 1.2);
    // PoB default: 5% crit chance × 30% extra crit damage = +1.5% damage on average.
    env.output.set("EnemyCritEffect", 1.015);
    env.output.set("Time", 0.83);
    env.output.set("WeaponRangeMetre", 0.8);
    env.output.set("enemySkillTime", 0.7);
    env.output.set("ImpaleDuration", 8.02);
    // impaleStoredHitAvg is a per-skill accumulator that PoB writes only when
    // a main skill is bound. Leaving it unset keeps parity in the no-skill case.
}

/// PoB's `data.monsterDamageTable` — expected damage per monster level for the
/// "default" iLvl-N boss profile. Index N-1 (level 1 → index 0).
const MONSTER_DAMAGE_TABLE: &[f64] = &[
    4.99, 5.55, 6.16, 6.81, 7.5, 8.23, 9.0, 9.82, 10.7, 11.62, 12.6, 13.64, 14.74, 15.91, 17.14,
    18.45, 19.83, 21.29, 22.84, 24.47, 26.19, 28.01, 29.94, 31.96, 34.11, 36.36, 38.75, 41.26,
    43.91, 46.7, 49.65, 52.75, 56.01, 59.45, 63.08, 66.89, 70.91, 75.13, 79.58, 84.26, 89.18,
    94.35, 99.8, 105.52, 111.53, 117.86, 124.5, 131.49, 138.83, 146.53, 154.63, 163.14, 172.07,
    181.45, 191.3, 201.63, 212.48, 223.87, 235.83, 248.37, 261.53, 275.33, 289.82, 305.01, 320.94,
    337.65, 355.18, 373.55, 392.81, 413.01, 434.18, 456.37, 479.62, 504.0, 529.54, 556.3, 584.35,
    613.73, 644.5, 676.75, 710.52, 745.89, 782.94, 821.73, 862.36, 904.9, 949.44, 996.07, 1044.89,
    1096.0, 1149.5, 1205.5, 1264.11, 1325.45, 1389.64, 1456.82, 1527.12, 1600.68, 1677.64, 1758.17,
];

/// EHP damage simulation. PoB defaults to a Pinnacle-Boss enemy at iLvl 84 and
/// runs each damage type through the character's defences. Phase 2 emits the
/// per-element EnemyDamage / TakenDamage / TakenHit tables and the totals — no
/// iterative damage shaving (PoB's solver) yet.
fn perform_enemy_damage_sim(env: &mut Env, character: &Character) {
    // PoB clamps the simulated enemy level at MaxEnemyLevel = 84 in the modern
    // tree. For lower-level characters, the enemy tracks character.level - 6.
    // We approximate by taking min(84, max(1, character.level)).
    let enemy_level = character.config.enemy_level.clamp(1, 84) as usize;
    let idx = (enemy_level - 1).min(MONSTER_DAMAGE_TABLE.len() - 1);
    let table_value = MONSTER_DAMAGE_TABLE[idx];
    // PoB uses Pinnacle Boss preset by default → 8/4.4 = 1.8181x DPS multiplier.
    let pinnacle_dps_mult = 8.0 / 4.4;
    let base_damage = (table_value * 1.5 * pinnacle_dps_mult).round();
    let chaos_damage = (base_damage / 2.5).round();

    let crit_effect = env.output.get("EnemyCritEffect").max(1.0);

    let mut total_in = 0.0_f64;
    let mut total_damage = 0.0_f64;
    let mut total_taken_hit = 0.0_f64;
    for elem in ["Physical", "Fire", "Cold", "Lightning"] {
        total_in += base_damage;
        let damage = base_damage * crit_effect;
        total_damage += damage;
        env.output.set_concat(elem, "EnemyDamage", damage);
        env.output.set_concat(elem, "TakenDamage", damage);
        let taken_mult = env.output.get_concat(elem, "TakenHitMult");
        let taken_hit = damage * taken_mult;
        env.output.set_concat(elem, "TakenHit", taken_hit);
        total_taken_hit += taken_hit;
    }
    total_in += chaos_damage;
    let chaos_taken_damage = chaos_damage * crit_effect;
    total_damage += chaos_taken_damage;
    env.output.set("ChaosEnemyDamage", chaos_taken_damage);
    env.output.set("ChaosTakenDamage", chaos_taken_damage);
    let chaos_taken_hit = chaos_taken_damage * env.output.get("ChaosTakenHitMult");
    env.output.set("ChaosTakenHit", chaos_taken_hit);
    total_taken_hit += chaos_taken_hit;

    env.output.set("totalEnemyDamageIn", total_in);
    env.output.set("totalEnemyDamage", total_damage);
    env.output.set("totalTakenDamage", total_damage);
    env.output.set("totalTakenHit", total_taken_hit);

    // TotalEHP — `NumberOfHitsToDie × totalEnemyDamageIn`, the same shape PoB
    // uses (Modules/CalcDefence.lua:2881). For a single-pool build (life only,
    // no Aegis/MoM/Ward) `NumberOfHitsToDie = pool / totalTakenHit`, which is
    // what we compute here. PoB's full iterative solver matters only when
    // there are multiple pools with type-specific protection (Aegis absorbs
    // some types first, ES recovery cap, MoM redirects to mana, etc.); none
    // of those are modelled yet, so a partial port wouldn't close the
    // ~0.2% baseline gap. Tracked in docs/divergences.md.
    let pool =
        (env.output.get("Life") + env.output.get("EnergyShield") + env.output.get("Ward")).max(1.0);
    if total_taken_hit > 0.0 {
        let hits_to_die = pool / total_taken_hit;
        env.output.set("NumberOfDamagingHits", hits_to_die);
        env.output.set("NumberOfMitigatedDamagingHits", hits_to_die);
        env.output.set("TotalNumberOfHits", hits_to_die);
        env.output.set("TotalEHP", hits_to_die * total_in);
        env.output.set(
            "EHPSurvivalTime",
            hits_to_die * env.output.get("enemySkillTime"),
        );
    }
}

/// BFS from the character's class start through the allocated subgraph and
/// return the set of allocations that are actually reachable. Mirrors PoB's
/// rule that a node only contributes if it sits on a connected path from the
/// character's class root.
///
/// When the character has picked an ascendancy class, this also seeds the BFS
/// from that ascendancy's start node (the synthetic `AscendancyStart` node
/// whose `ascendancy_name` matches the picked class) so allocated ascendancy
/// nodes from the matching tree are credited. PoB allocates the ascendancy
/// start automatically on pick (`PassiveSpec:SelectAscendClass`); we mirror it.
///
/// Ascendancy nodes that don't match the picked class are stripped from the
/// final effective set. This catches both "no ascendancy picked" (all
/// ascendancy nodes filtered) and "ascendancy nodes from a different tree
/// allocated" (filtered by name). The traversal itself is unrestricted so
/// existing tests that allocate paths via cross-class ascendancy bridges
/// keep finding their non-ascendancy targets.
fn connected_allocations(
    character: &Character,
    tree: &PassiveTree,
) -> ahash::AHashSet<pob_data::NodeId> {
    let mut effective = ahash::AHashSet::new();
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
    let mut starts: Vec<pob_data::NodeId> = tree
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

    // Seed the BFS from the ascendancy start as well (when picked). Match by
    // ascendancy name, case-insensitively, against the AscendancyStart kind.
    let picked_asc = character.ascendancy.as_deref();
    if let Some(picked) = picked_asc {
        if let Some((id, _)) = tree.nodes.iter().find(|(_, n)| {
            matches!(n.kind, pob_data::NodeKind::AscendancyStart)
                && n.ascendancy_name
                    .as_deref()
                    .is_some_and(|s| s.eq_ignore_ascii_case(picked))
        }) {
            starts.push(*id);
        }
    }

    let allocated: ahash::AHashSet<_> = character.allocated.iter().copied().collect();
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
    // The class-start and ascendancy-start nodes themselves are visited but
    // should not contribute mods (they're synthetic). Also strip any
    // ascendancy nodes whose tree doesn't match the picked ascendancy: an
    // unpicked Ascendant tree still appears as a graph bridge between class
    // areas, but allocating its nodes shouldn't credit Ascendant mods.
    effective.retain(|id| {
        let Some(n) = tree.nodes.get(id) else {
            return false;
        };
        if n.class_start_index.is_some() {
            return false;
        }
        if matches!(n.kind, pob_data::NodeKind::AscendancyStart) {
            return false;
        }
        if let Some(asc_name) = n.ascendancy_name.as_deref() {
            // Allocated ascendancy node — only keep if it matches the picked one.
            return picked_asc.is_some_and(|p| p.eq_ignore_ascii_case(asc_name));
        }
        true
    });
    // Enforce the ascendancy point budget (PoE: 8 nodes by default, exposed by
    // `tree.points.ascendancy_points`). Imported builds may be over-allocated;
    // the UI gates clicks but loaded `.mk2` / PoB-XML data can sneak past, so
    // we silently drop the excess at compute time. Sort by NodeId for a
    // deterministic choice of which nodes survive.
    let budget = tree.points.ascendancy_points as usize;
    let mut asc_in_effective: Vec<pob_data::NodeId> = effective
        .iter()
        .copied()
        .filter(|id| {
            tree.nodes
                .get(id)
                .and_then(|n| n.ascendancy_name.as_deref())
                .is_some()
        })
        .collect();
    if asc_in_effective.len() > budget {
        asc_in_effective.sort_unstable();
        for drop_id in &asc_in_effective[budget..] {
            effective.remove(drop_id);
        }
    }
    effective
}

pub fn perform_basic_stats(character: &Character, _tree: &PassiveTree, env: &mut Env) {
    // Strength / Dexterity / Intelligence
    let cfg = QueryCfg::default();
    let str_base = env.mod_db.sum(ModType::Base, &cfg, &env.state, "Strength")
        + env
            .mod_db
            .sum(ModType::Base, &cfg, &env.state, "AllAttributes");
    let dex_base = env.mod_db.sum(ModType::Base, &cfg, &env.state, "Dexterity")
        + env
            .mod_db
            .sum(ModType::Base, &cfg, &env.state, "AllAttributes");
    let int_base = env
        .mod_db
        .sum(ModType::Base, &cfg, &env.state, "Intelligence")
        + env
            .mod_db
            .sum(ModType::Base, &cfg, &env.state, "AllAttributes");

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
    env.state.set_stat("Str", str_v);
    env.state.set_stat("Dex", dex_v);
    env.state.set_stat("Int", int_v);

    // Character implicits derived from the just-computed attributes (Modules/
    // CalcPerform.lua:507-520). PoB injects two INC mods after attribute
    // computation: `Evasion INC floor(Dex/5)` and `EnergyShield INC floor(Int/10)`,
    // gated by the NoDex/Int* flags. Add them here so the Evasion / ES queries
    // below pick them up.
    if !env
        .mod_db
        .flag(&cfg, &env.state, "NoDexterityAttributeBonuses")
        && !env.mod_db.flag(&cfg, &env.state, "NoDexBonusToEvasion")
    {
        let dex_evasion_inc = (dex_v / 5.0).floor();
        if dex_evasion_inc > 0.0 {
            env.mod_db.add(
                Mod::inc("Evasion", dex_evasion_inc).with_source(Source::Other("Dexterity".into())),
            );
        }
    }
    if !env
        .mod_db
        .flag(&cfg, &env.state, "NoIntelligenceAttributeBonuses")
        && !env.mod_db.flag(&cfg, &env.state, "NoIntBonusToES")
    {
        let int_es_inc = (int_v / 10.0).floor();
        if int_es_inc > 0.0 {
            env.mod_db.add(
                Mod::inc("EnergyShield", int_es_inc)
                    .with_source(Source::Other("Intelligence".into())),
            );
        }
    }

    // Life: base + (Strength / 2) implicit from PoE; then * (1 + inc/100) * more product.
    let life_base = env.mod_db.sum(ModType::Base, &cfg, &env.state, "Life") + str_v / 2.0;
    let life = life_base * env.mod_db.applied(&cfg, &env.state, "Life");
    env.output.set("Life", life.round());

    // Mana: base + (Intelligence / 2).
    let mana_base = env.mod_db.sum(ModType::Base, &cfg, &env.state, "Mana") + int_v / 2.0;
    let mana = mana_base * env.mod_db.applied(&cfg, &env.state, "Mana");
    env.output.set("Mana", mana.round());

    // Energy Shield: pure mods (no base). Phase 2: base 0; later integrate item ES bases.
    let es_base = env
        .mod_db
        .sum(ModType::Base, &cfg, &env.state, "EnergyShield");
    let es = es_base * env.mod_db.applied(&cfg, &env.state, "EnergyShield");
    env.output.set("EnergyShield", es.round());

    // Resistances: PoE applies a story-based penalty that drops elemental and chaos
    // resists by 30 at end of Act 5 and another 30 at end of Act 10. The character's
    // `level` is a proxy for story progression — by level 68+ players are post-Act 10.
    // Match PoB's default by applying -60 to all four resists for any character of
    // level 68 or higher unless an explicit `act` config override is in play.
    let resist_penalty: f64 = if character.level >= 68 { -60.0 } else { 0.0 };
    for &(_elem, resist_key, max_key, total_key) in &[
        ("Fire", "FireResist", "FireResistMax", "FireResistTotal"),
        ("Cold", "ColdResist", "ColdResistMax", "ColdResistTotal"),
        (
            "Lightning",
            "LightningResist",
            "LightningResistMax",
            "LightningResistTotal",
        ),
    ] {
        let total = env.mod_db.sum(ModType::Base, &cfg, &env.state, resist_key)
            + env
                .mod_db
                .sum(ModType::Base, &cfg, &env.state, "ElementalResist")
            + resist_penalty;
        env.output.set(resist_key, total);
        let bonus = env.mod_db.sum(ModType::Base, &cfg, &env.state, max_key);
        let cap = 75.0 + bonus;
        env.output.set(max_key, cap);
        env.output.set(total_key, total.min(cap));
    }
    let chaos = env
        .mod_db
        .sum(ModType::Base, &cfg, &env.state, "ChaosResist")
        + resist_penalty;
    env.output.set("ChaosResist", chaos);
    {
        let bonus = env
            .mod_db
            .sum(ModType::Base, &cfg, &env.state, "ChaosResistMax");
        let cap = 75.0 + bonus;
        env.output.set("ChaosResistMax", cap);
        let raw = env.output.get("ChaosResist");
        env.output.set("ChaosResistTotal", raw.min(cap));
    }

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
    let block_inc_pct = env
        .mod_db
        .sum(ModType::Base, &cfg, &env.state, "BlockChance");
    let block_max_bonus = env
        .mod_db
        .sum(ModType::Base, &cfg, &env.state, "BlockChanceMax");
    let block_cap = 75.0 + block_max_bonus;
    env.output.set("BlockChance", block_inc_pct.min(block_cap));
    env.output.set("BlockChanceMax", block_cap);
    let spell_block = env
        .mod_db
        .sum(ModType::Base, &cfg, &env.state, "SpellBlockChance");
    env.output
        .set("SpellBlockChance", spell_block.min(block_cap));
    let suppress = env
        .mod_db
        .sum(ModType::Base, &cfg, &env.state, "SpellSuppressionChance");
    env.output
        .set("SpellSuppressionChance", suppress.min(100.0));

    // Life / mana / ES regen
    let life_regen_flat = env.mod_db.sum(ModType::Base, &cfg, &env.state, "LifeRegen");
    let life_regen_pct = env
        .mod_db
        .sum(ModType::Base, &cfg, &env.state, "LifeRegenPercent");
    let life = env.output.get("Life");
    let life_regen_total = life_regen_flat + life * life_regen_pct / 100.0;
    env.output.set("LifeRegen", life_regen_total);

    let mana_regen_flat = env.mod_db.sum(ModType::Base, &cfg, &env.state, "ManaRegen");
    let mana_regen_pct = env.mod_db.sum(ModType::Inc, &cfg, &env.state, "ManaRegen");
    let mana = env.output.get("Mana");
    // PoE: base mana regen = 1.75% of max mana per second; modifier is INC on rate.
    let mana_regen_total = (mana * 0.0175 + mana_regen_flat) * (1.0 + mana_regen_pct / 100.0);
    env.output.set("ManaRegen", mana_regen_total);

    // ES recharge — base 33% of total ES per second after delay, but we just expose the
    // increased-rate stat as a mod multiplier (delay handling lives in Phase 4).
    let es = env.output.get("EnergyShield");
    let recharge_inc = env
        .mod_db
        .sum(ModType::Inc, &cfg, &env.state, "EnergyShieldRecharge");
    env.output.set(
        "EnergyShieldRecharge",
        es * 0.33 * (1.0 + recharge_inc / 100.0),
    );

    // ES regen — separate from recharge. PoB exposes EnergyShieldRegen as the
    // base regen-per-second rate (zero unless mods grant it directly).
    let es_regen_flat = env
        .mod_db
        .sum(ModType::Base, &cfg, &env.state, "EnergyShieldRegen");
    let es_regen_pct = env
        .mod_db
        .sum(ModType::Inc, &cfg, &env.state, "EnergyShieldRegen");
    env.output.set(
        "EnergyShieldRegen",
        es_regen_flat * (1.0 + es_regen_pct / 100.0),
    );

    // Reservation pools. Default 100% unreserved; perform_reservations (called
    // after the basic-stats pass once we have a SkillRegistry) replaces these
    // with the actual aura/herald-reduced values.
    env.output.set("LifeUnreserved", life);
    env.output.set("LifeUnreservedPercent", 100.0);
    env.output.set("ManaUnreserved", mana);
    env.output.set("ManaUnreservedPercent", 100.0);
    env.output.set("LifeReserved", 0.0);
    env.output.set("ManaReserved", 0.0);
    env.output.set("LifeReservedPercent", 0.0);
    env.output.set("ManaReservedPercent", 0.0);

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
    // Character-level CritChance is the *effective* crit rate the player rolls
    // per hit on the active skill (PoB's `output.CritChance`). For spells it
    // collapses to the skill's intrinsic critChance because spells always hit;
    // for attacks it folds in HitChance (= chance to crit per swing).
    // perform_skill_dps overrides this once a main skill is bound.
    let crit_inc = env.mod_db.sum(ModType::Inc, &cfg, &env.state, "CritChance");
    let crit_chance_base = if character.main_skill.is_some() {
        5.0
    } else {
        0.0
    };
    env.output
        .set("CritChance", crit_chance_base * (1.0 + crit_inc / 100.0));
    // PoE base crit deals 150% damage; PoB exposes that as the decimal multiplier
    // 1.5 (= 150 / 100). BASE mods on `CritMultiplier` add extra crit damage as
    // additional percentage points.
    let crit_mult_pct = 150.0
        + env
            .mod_db
            .sum(ModType::Base, &cfg, &env.state, "CritMultiplier");
    env.output.set("CritMultiplier", crit_mult_pct / 100.0);
}

/// Walk the character's skill groups, sum aura/herald reservations, and rewrite
/// the LifeUnreserved/ManaUnreserved outputs accordingly. Mirrors PoB's
/// `doActorLifeManaReservation`. We model the basics: each enabled gem in an
/// enabled group whose level data carries `manaReservationPercent` /
/// `lifeReservationPercent` / `manaReservationFlat` / `lifeReservationFlat`
/// contributes to the pool reservation. Reservation efficiency mods (INC and
/// MORE on `ManaReservationEfficiency` / `LifeReservationEfficiency` /
/// `ReservationEfficiency`) scale the reserved amount: more efficient = less
/// reserved.
/// Multiply a numeric `ModValue` by a scalar. Range mods scale both bounds; bool
/// and string values pass through unchanged. Used by perform_reservations to
/// stretch aura buff values by `(1 + AuraEffect/100)`.
fn scale_mod_value(v: crate::ModValue, scale: f64) -> crate::ModValue {
    use crate::ModValue;
    match v {
        ModValue::Number(n) => ModValue::Number(n * scale),
        ModValue::Range { min, max } => ModValue::Range {
            min: min * scale,
            max: max * scale,
        },
        other => other,
    }
}

fn perform_reservations(character: &Character, skills: &SkillRegistry, env: &mut Env) {
    if character.skill_groups.is_empty() {
        return;
    }
    let cfg = QueryCfg::default();
    // Reservation efficiency: PoB stores it as INC + MORE on
    // `<pool>ReservationEfficiency` and the generic `ReservationEfficiency`.
    // The reserved amount scales by `1 / ((1 + inc/100) * more)` — i.e. higher
    // efficiency means less of the pool is consumed.
    let efficiency = |pool: &str| -> f64 {
        let inc = env
            .mod_db
            .sum(ModType::Inc, &cfg, &env.state, "ReservationEfficiency")
            + env.mod_db.sum(
                ModType::Inc,
                &cfg,
                &env.state,
                &format!("{pool}ReservationEfficiency"),
            );
        let more = env.mod_db.more(&cfg, &env.state, "ReservationEfficiency")
            * env
                .mod_db
                .more(&cfg, &env.state, &format!("{pool}ReservationEfficiency"));
        ((1.0 + inc / 100.0) * more).max(0.01)
    };
    let mana_eff = efficiency("Mana");
    let life_eff = efficiency("Life");
    let life_max = env.output.get("Life").max(1.0);
    let mana_max = env.output.get("Mana").max(1.0);

    let mut life_reserved_flat = 0.0;
    let mut life_reserved_percent = 0.0;
    let mut mana_reserved_flat = 0.0;
    let mut mana_reserved_percent = 0.0;

    for group in &character.skill_groups {
        if !group.enabled {
            continue;
        }
        for gem in &group.gems {
            if !gem.enabled {
                continue;
            }
            let Some(skill) = skills.get(&gem.skill_id) else {
                continue;
            };
            // Only count aura/reservation skills. PoB checks SkillType.HasReservation
            // (id 18 in the canonical PoB enum); we mirror that, plus the
            // base-flag `aura` which the extractor also surfaces.
            let has_reservation = skill.skill_types.get("18").copied().unwrap_or(false)
                || skill.base_flags.get("aura").copied().unwrap_or(false)
                || skill.base_flags.get("herald").copied().unwrap_or(false);
            if !has_reservation {
                continue;
            }
            let level = gem.level.max(1);
            let m_pct = skill.reservation_percent(level, "Mana");
            let m_flat = skill.reservation_flat(level, "Mana");
            let l_pct = skill.reservation_percent(level, "Life");
            let l_flat = skill.reservation_flat(level, "Life");
            mana_reserved_percent += m_pct / mana_eff;
            mana_reserved_flat += m_flat / mana_eff;
            life_reserved_percent += l_pct / life_eff;
            life_reserved_flat += l_flat / life_eff;

            // Aura buff propagation. PoB walks each aura's mods (from
            // statMap × constantStats/qualityStats/per-level positionals) and
            // injects the GlobalEffect-tagged ones into the player's modDB
            // scaled by AuraEffect/BuffEffect. Scale every value by
            // `(1 + AuraEffect/100)` so mods like "+15% Aura Effect" boost the
            // buffs as PoB does.
            let aura_inc = env.mod_db.sum(ModType::Inc, &cfg, &env.state, "AuraEffect");
            let aura_more = env.mod_db.more(&cfg, &env.state, "AuraEffect");
            let aura_scale = (1.0 + aura_inc / 100.0) * aura_more;
            for mut m in crate::skill::aura_buff_mods(skill, level, gem.quality) {
                m.value = scale_mod_value(m.value, aura_scale);
                env.mod_db.add(m);
            }
        }
    }

    // Convert percent reservations on each pool into absolutes.
    let life_reserved = life_reserved_flat + life_max * life_reserved_percent / 100.0;
    let mana_reserved = mana_reserved_flat + mana_max * mana_reserved_percent / 100.0;
    let life_unreserved = (life_max - life_reserved).max(0.0);
    let mana_unreserved = (mana_max - mana_reserved).max(0.0);

    env.output.set("LifeReserved", life_reserved.round());
    env.output.set("ManaReserved", mana_reserved.round());
    env.output.set(
        "LifeReservedPercent",
        (life_reserved / life_max * 100.0).min(100.0),
    );
    env.output.set(
        "ManaReservedPercent",
        (mana_reserved / mana_max * 100.0).min(100.0),
    );
    env.output.set("LifeUnreserved", life_unreserved.round());
    env.output.set("ManaUnreserved", mana_unreserved.round());
    env.output.set(
        "LifeUnreservedPercent",
        (life_unreserved / life_max * 100.0).max(0.0),
    );
    env.output.set(
        "ManaUnreservedPercent",
        (mana_unreserved / mana_max * 100.0).max(0.0),
    );
}

/// Walk the character's skill groups, gather any enabled curse / mark gems, and
/// stash the resulting enemy-resist deltas + curse-effect outputs. The skill DPS
/// pass reads `EnemyFireResist` / `EnemyColdResist` / etc. to apply them to
/// damage. PoB models curses as mods on a dedicated `enemyDB`; we model the
/// player-visible side: aggregate the per-element resist reduction across all
/// curses on the build and expose it as `Cursed{Element}ResistDelta`.
fn perform_curses(character: &Character, skills: &SkillRegistry, env: &mut Env) {
    if character.skill_groups.is_empty() {
        return;
    }
    // Sum of resist reductions across active curses (negative numbers; e.g. -36
    // means the enemy's lightning resist drops by 36). PoB caps total curse
    // effect against bosses; for now we apply the raw stack (Phase-3 minimum).
    let mut fire_delta = 0.0_f64;
    let mut cold_delta = 0.0_f64;
    let mut light_delta = 0.0_f64;
    let mut chaos_delta = 0.0_f64;
    let mut elem_delta = 0.0_f64;
    // Per-ailment chance accumulators. PoB tracks these as enemyDB Self*Chance
    // mods; we expose the player-side ChanceOnHit number that perform_skill_dps
    // folds into the player's effective ailment chance.
    let mut shock_chance = 0.0_f64;
    let mut freeze_chance = 0.0_f64;
    let mut ignite_chance = 0.0_f64;
    let mut chill_chance = 0.0_f64;
    let mut active_curses: u32 = 0;
    for group in &character.skill_groups {
        if !group.enabled {
            continue;
        }
        for gem in &group.gems {
            if !gem.enabled {
                continue;
            }
            let Some(skill) = skills.get(&gem.skill_id) else {
                continue;
            };
            let is_curse = skill.base_flags.get("curse").copied().unwrap_or(false);
            if !is_curse {
                continue;
            }
            active_curses += 1;
            // The resist-reduction value sits on a positional indexed by the
            // curse's `stats` list — typically the third entry (`base_X_damage_
            // resistance_%`). Walk stats and pluck values for the four element
            // resists + the all-elements bucket.
            let level = gem.level.max(1);
            for (i, stat_id) in skill.stats.iter().enumerate() {
                let Some(value) = skill.positional(level, (i + 1) as u32) else {
                    continue;
                };
                match stat_id.as_str() {
                    "base_fire_damage_resistance_%" => fire_delta += value,
                    "base_cold_damage_resistance_%" => cold_delta += value,
                    "base_lightning_damage_resistance_%" => light_delta += value,
                    "base_chaos_damage_resistance_%" => chaos_delta += value,
                    "base_resist_all_elements_%" => elem_delta += value,
                    _ => {}
                }
            }
            // Per-ailment chance contributions. PoB stores these as
            // constantStats with id `chance_to_be_X_%` (e.g. Conductivity =
            // 25 shock, Frostbite = 25 freeze, Flammability = 25 ignite).
            // The value is fixed and doesn't scale with curse level, but PoB
            // does scale it by curse-effect mods on the enemy — we approximate
            // at effect=1.0 here (tracking curse-effect lands in a follow-up).
            for v in &skill.constant_stats {
                let Some(arr) = v.as_array() else {
                    continue;
                };
                let Some(id) = arr.first().and_then(|x| x.as_str()) else {
                    continue;
                };
                let Some(val) = arr.get(1).and_then(serde_json::Value::as_f64) else {
                    continue;
                };
                match id {
                    "chance_to_be_shocked_%" => shock_chance += val,
                    "chance_to_be_frozen_%" => freeze_chance += val,
                    "chance_to_be_ignited_%" => ignite_chance += val,
                    "chance_to_be_chilled_%" => chill_chance += val,
                    _ => {}
                }
            }
        }
    }
    // Roll the all-elements bucket into each individual element.
    fire_delta += elem_delta;
    cold_delta += elem_delta;
    light_delta += elem_delta;

    // PoB scales every curse's outgoing values by `(1 + CurseEffect/100)` (mods
    // like "+15% Curse Effect" on a Doedre's Damning amulet, the small-cluster
    // notable, etc.). Apply the same scalar to every curse-derived output.
    let cfg = QueryCfg::default();
    let curse_effect_inc = env
        .mod_db
        .sum(ModType::Inc, &cfg, &env.state, "CurseEffect");
    let curse_effect_more = env.mod_db.more(&cfg, &env.state, "CurseEffect");
    let curse_scale = (1.0 + curse_effect_inc / 100.0) * curse_effect_more;
    fire_delta *= curse_scale;
    cold_delta *= curse_scale;
    light_delta *= curse_scale;
    chaos_delta *= curse_scale;
    shock_chance *= curse_scale;
    freeze_chance *= curse_scale;
    ignite_chance *= curse_scale;
    chill_chance *= curse_scale;

    env.output.set("CursedFireResistDelta", fire_delta);
    env.output.set("CursedColdResistDelta", cold_delta);
    env.output.set("CursedLightningResistDelta", light_delta);
    env.output.set("CursedChaosResistDelta", chaos_delta);
    env.output.set("ActiveCurseCount", f64::from(active_curses));
    env.output.set("CurseEffectScale", curse_scale);
    // Stash the ChanceOnHit deltas under separate output keys so
    // perform_skill_dps can fold them into the player's effective ailment
    // chance even though `cfg.skill_name` filters out the curse skill itself.
    env.output.set("CurseShockChanceOnHit", shock_chance);
    env.output.set("CurseFreezeChanceOnHit", freeze_chance);
    env.output.set("CurseIgniteChanceOnHit", ignite_chance);
    env.output.set("CurseChillChanceOnHit", chill_chance);
}

/// Issue #19 (slice 3): walk the gem list for warcry skills and
/// surface aggregate output keys describing the player's warcry
/// loadout. Mirrors PoB's `Modules/CalcPerform.lua:2200+` warcry
/// detection: each warcry gem with `baseFlags.warcry = true`
/// contributes its `skill_empowers_next_x_melee_attacks` constant
/// (the per-cry exert count) and its level-data `cooldown` to the
/// totals.
///
/// Output keys (only emitted when at least one enabled warcry gem
/// is present, so non-warcry builds stay clean):
///
/// - `ActiveWarcryCount` — number of enabled warcry gems socketed.
/// - `WarcryExertedAttackCountTotal` — sum of per-cry exert counts.
/// - `WarcryMinCooldown` — fastest cooldown among the warcries
///   (drives uptime: a faster cry can refresh exertion before the
///   next batch expires).
///
/// The actual `ExertedAttackUptime` derivation needs a per-skill
/// cast-cadence pass (still in slice 4 scope); this slice ships the
/// raw aggregates so the Calcs side panel and a future auto-uptime
/// pass have the data they need.
fn detect_warcries(character: &Character, skills: &SkillRegistry, env: &mut Env) {
    if character.skill_groups.is_empty() {
        return;
    }
    let mut active = 0u32;
    let mut total_exert: f64 = 0.0;
    let mut min_cooldown: Option<f64> = None;
    for group in &character.skill_groups {
        if !group.enabled {
            continue;
        }
        for gem in &group.gems {
            if !gem.enabled {
                continue;
            }
            let Some(skill) = skills.get(&gem.skill_id) else {
                continue;
            };
            if !skill.base_flags.get("warcry").copied().unwrap_or(false) {
                continue;
            }
            active += 1;
            // Each warcry gem stores its exert count in
            // `constantStats[skill_empowers_next_x_melee_attacks]` —
            // a JSON array `[<stat_id>, <value>]` per entry.
            for entry in &skill.constant_stats {
                let Some(arr) = entry.as_array() else {
                    continue;
                };
                let Some(id) = arr.first().and_then(|v| v.as_str()) else {
                    continue;
                };
                if id == "skill_empowers_next_x_melee_attacks" {
                    if let Some(v) = arr.get(1).and_then(serde_json::Value::as_f64) {
                        total_exert += v;
                    }
                    break;
                }
            }
            // Cooldown lives in `levels[L].cooldown`. We read it
            // through the same `f64::from(...).max(...)` path the
            // mine/trap timing pass uses so a missing field falls
            // back to the configured default (no clamp here — PoB
            // exposes the raw value).
            let level = gem.level.max(1);
            if let Some(cd) = skill.cooldown(level) {
                min_cooldown = Some(min_cooldown.map_or(cd, |m| m.min(cd)));
            }
        }
    }
    if active == 0 {
        return;
    }
    env.output.set("ActiveWarcryCount", f64::from(active));
    env.output.set("WarcryExertedAttackCountTotal", total_exert);
    if let Some(cd) = min_cooldown {
        env.output.set("WarcryMinCooldown", cd);
    }
}

/// Compute hit damage for the main skill. Phase 3d: spell-only, single hit, single
/// target, ignores ailments / penetration / resistances of the enemy. Outputs:
/// `MainSkillId`, `MainSkillLevel`, `MainSkillBaseMin`, `MainSkillBaseMax`,
/// `MainSkillAverageHit`, `MainSkillDPS`.
pub fn perform_skill_dps(character: &Character, skills: &SkillRegistry, env: &mut Env) {
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
    // Support gems linked in the same socket group buff the active skill.
    // Each support's intrinsic mods get the same skill_mods treatment and are
    // added to the env. PoB applies skill-type filtering (e.g. "Added Lightning
    // Damage" only buffs skills that hit) — Phase 5 minimum: we apply every
    // support that's enabled in the same group. addSkillTypes / excludeSkillTypes
    // filtering is a follow-up.
    let main_group_idx = (character.main_socket_group.saturating_sub(1)) as usize;
    if let Some(group) = character.skill_groups.get(main_group_idx) {
        if group.enabled {
            let active_idx = group.main_active_skill_index.saturating_sub(1) as usize;
            for (idx, gem) in group.gems.iter().enumerate() {
                if idx == active_idx {
                    continue; // skip the main skill itself
                }
                if !gem.enabled {
                    continue; // user toggled this support off
                }
                let Some(support) = skills.get(&gem.skill_id) else {
                    continue;
                };
                if !support.support {
                    continue;
                }
                // Skill-type compatibility: PoB's `addSkillTypes` lists the
                // SkillType ids the support REQUIRES on the linked skill, and
                // `excludeSkillTypes` lists those that disqualify it. We honour
                // both so e.g. an attack-only support doesn't buff a spell.
                let active_types = &skill.skill_types;
                let mut compatible = true;
                for (st, on) in &support.add_skill_types {
                    if !*on {
                        continue;
                    }
                    if !active_types.get(st).copied().unwrap_or(false) {
                        compatible = false;
                        break;
                    }
                }
                if compatible {
                    for (st, on) in &support.exclude_skill_types {
                        if !*on {
                            continue;
                        }
                        if active_types.get(st).copied().unwrap_or(false) {
                            compatible = false;
                            break;
                        }
                    }
                }
                if !compatible {
                    continue;
                }
                for m in crate::skill::skill_mods(support, gem.quality) {
                    env.mod_db.add(m);
                }
            }
        }
    }

    // Tag the EvalState with the active skill's name and types so SkillName /
    // SkillType / SkillId tags on mods can filter correctly.
    env.state
        .set_condition_prefixed("SkillName", &main.skill_id, true);
    for (st_id, on) in &skill.skill_types {
        if !*on {
            continue;
        }
        env.state.set_condition_prefixed("SkillType", st_id, true);
    }

    // For each named per-level stat, push the corresponding positional value into the
    // EvalState's stats map. This lets PerStat tags (e.g. PerStat:ChainRemaining)
    // scale by the skill's own per-level numbers (Arc has 7 chains at level 20, so
    // ChainRemaining = 7). PoB does this in CalcActiveSkill via its skillData table.
    //
    // PoB stores `output.ChainRemaining = max(0, ChainMax - Chain)` where `Chain`
    // is the user's `skillChainCount` config (default 0) — see CalcOffence.lua.
    // The default analysis is therefore the initial cast with the FULL chain
    // bonus. We mirror that default here; a `skillChainCount` config override is
    // a follow-up.
    let mut chain_max_display: Option<f64> = None;
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
                // Quality stats add to per-level positionals — e.g. Arc Q20 grants
                // +1 chain via qualityStats=[["number_of_chains", 0.05]].
                let mut value = v;
                for qs in skill.quality_stats.iter() {
                    let Some(arr) = qs.as_array() else { continue };
                    let Some(qstat) = arr.first().and_then(|x| x.as_str()) else {
                        continue;
                    };
                    let Some(scale) = arr.get(1).and_then(serde_json::Value::as_f64) else {
                        continue;
                    };
                    if qstat == stat_id {
                        value += scale * f64::from(main.quality);
                    }
                }
                if eval_key == "ChainRemaining" {
                    chain_max_display = Some(value);
                }
                env.state.set_stat(eval_key, value);
            }
        }
    }
    // Move the spell/attack flag detection before damage so we can branch.
    let early_is_attack = skill.base_flags.get("attack").copied().unwrap_or(false);
    let early_is_spell = skill.base_flags.get("spell").copied().unwrap_or(false);
    let (mut base_min, mut base_max) = if early_is_attack {
        // Attack skills: base damage is the weapon's damage. Use Weapon1 if equipped.
        let cfg_q = QueryCfg::default();
        let st = &env.state;
        let w_min = env
            .mod_db
            .sum(ModType::Base, &cfg_q, st, "Weapon1PhysicalMin");
        let w_max = env
            .mod_db
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
    // PoB's `damageEffectiveness` only scales ADDED flat damage (item / aura
    // bonuses), not the spell's own gem-level damage. We applied it to base
    // damage too, which inflated Arc's average hit by 1.2× even with no added
    // damage. Pull dmgEff out of the base computation; we'll re-apply it to
    // the flat-damage adds below.
    let dmg_eff = skill.damage_effectiveness(gem_level);

    // Add flat damage from "Adds N to M <element> Damage" mods. The parser emits these
    // as Mod::base("<Element>Damage", ModValue::Range{min, max}).
    let cfg_q = QueryCfg::default();
    let st = &env.state;
    let elem_for_flat = if early_is_attack {
        // Attacks pick up the most recently equipped weapon's flat damage; for now
        // we add all elements together to phys damage (rough but useful).
        [
            "PhysicalDamage",
            "FireDamage",
            "ColdDamage",
            "LightningDamage",
            "ChaosDamage",
        ]
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
        // For spells, damageEffectiveness scales the added flat damage. For
        // attacks, it scales the entire weapon hit (which we already folded in
        // when we read the weapon damage above), so don't double-scale.
        if early_is_spell {
            base_min += flat_min * dmg_eff;
            base_max += flat_max * dmg_eff;
        } else {
            base_min += flat_min;
            base_max += flat_max;
        }
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
            let dot_more = env.mod_db.more(&dot_cfg, &env.state, "DamageOverTime")
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
        + env
            .mod_db
            .sum(ModType::Inc, &cfg, &env.state, "ElementalDamage")
        + if is_spell {
            env.mod_db
                .sum(ModType::Inc, &cfg, &env.state, "SpellDamage")
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

    // Default-quality damage bonus: PoB applies a small increased-damage
    // multiplier per quality point on gems whose qualityStats don't already
    // grant type-specific damage. The Arc gem at Q20 picks up ~+10% via this
    // path (matches PoB exactly: 660 → 726 base avg lightning damage). For
    // attack gems the equivalent stat is +X% attack speed or +X% damage,
    // so we apply the same flat-half-percent-per-quality fallback.
    let quality_damage_inc = if main.quality > 0 {
        f64::from(main.quality) * 0.005
    } else {
        0.0
    };
    let hit_min = base_min * mult * (1.0 + quality_damage_inc);
    let hit_max = base_max * mult * (1.0 + quality_damage_inc);
    let avg = (hit_min + hit_max) * 0.5;
    env.output.set("MainSkillHitMin", hit_min);
    env.output.set("MainSkillHitMax", hit_max);
    env.output.set("MainSkillAverageHit", avg);
    // PoB's `{Element}MinBase / MaxBase` are the spell's RAW per-level values
    // before the player's increased / more / quality multipliers — they're the
    // skill data scaled only by `availableEffectiveness`. PoB then exposes
    // `{Element}Min / Max` for the post-mod values. Phase 5 minimum: emit
    // both, with the post-mod going into Total* and HitAverage too.
    let elem_label = match elem_stat {
        "FireDamage" => Some("Fire"),
        "ColdDamage" => Some("Cold"),
        "LightningDamage" => Some("Lightning"),
        "PhysicalDamage" => Some("Physical"),
        "ChaosDamage" => Some("Chaos"),
        _ => None,
    };
    if let Some(label) = elem_label {
        env.output.set_concat(label, "MinBase", base_min);
        env.output.set_concat(label, "MaxBase", base_max);
        env.output.set_concat(label, "Min", hit_min);
        env.output.set_concat(label, "Max", hit_max);
        env.output.set_concat(label, "HitAverage", avg);
    }
    env.output.set("TotalMin", hit_min);
    env.output.set("TotalMax", hit_max);

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
    // CritMultiplier is stored in decimal form (1.5 == 150%) — see the
    // basic-stats pass. Earlier code divided by 100 here, which made non-crit
    // hits land at 0.94× when crit was 6%.
    let crit_mult = env.output.get("CritMultiplier").max(1.0);
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

    // Apply enemy mitigation: physical hits go through the armour formula,
    // elemental/chaos through resist. Both resolve into an `effective_resist`
    // percentage and a `(1 - effective_resist/100)` multiplier on the hit
    // average.
    //
    // PoB's armour formula (`CalcDefence.lua:41`):
    //   reduction% = armour / (armour + 5 × raw)
    // where `raw` is the per-hit physical average BEFORE crit averaging
    // (`damageTypeHitAvg` in CalcOffence.lua:3260). The result is capped at
    // 90% (`EnemyPhysicalDamageReductionCap`).
    //
    // We treat a hit as physical when either the skill explicitly tags itself
    // as `PhysicalDamage` or it's a weapon-driven attack with no elemental
    // tag (Cleave, Strike skills, default Sunder, etc. — their damage source
    // is the weapon's physical damage). Skill-data-driven elemental conversion
    // (e.g. Avatar of Fire) takes a follow-up to model precisely.
    let is_physical_hit =
        elem_stat == "PhysicalDamage" || (early_is_attack && elem_stat == "Damage");
    let enemy_phys_reduction = if is_physical_hit {
        let raw = avg.max(0.0);
        let armour = f64::from(character.config.effective_enemy_armour());
        if armour > 0.0 && raw > 0.0 {
            ((armour / (armour + 5.0 * raw)) * 100.0).min(90.0)
        } else {
            0.0
        }
    } else {
        0.0
    };
    if is_physical_hit {
        env.output.set("EnemyPhysReduction", enemy_phys_reduction);
    }
    let enemy_resist_raw = match elem_stat {
        "FireDamage" => character.config.enemy_fire_resist,
        "ColdDamage" => character.config.enemy_cold_resist,
        "LightningDamage" => character.config.enemy_lightning_resist,
        "ChaosDamage" => character.config.enemy_chaos_resist,
        "PhysicalDamage" => enemy_phys_reduction.round() as i32,
        "Damage" if is_physical_hit => enemy_phys_reduction.round() as i32,
        _ => 0,
    };
    // Curse-driven enemy resist reduction. perform_curses populated
    // Cursed{Element}ResistDelta with the sum of all active curses' resist
    // reductions (already negative — e.g. Conductivity contributes -36).
    let curse_delta = match elem_stat {
        "FireDamage" => env.output.get("CursedFireResistDelta"),
        "ColdDamage" => env.output.get("CursedColdResistDelta"),
        "LightningDamage" => env.output.get("CursedLightningResistDelta"),
        "ChaosDamage" => env.output.get("CursedChaosResistDelta"),
        _ => 0.0,
    };
    let enemy_resist_raw = f64::from(enemy_resist_raw) + curse_delta;
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
    let effective_resist = (enemy_resist_raw - elem_pen).clamp(-200.0, 95.0);
    let res_factor = (1.0 - effective_resist / 100.0).max(0.0);
    let avg_after_res = avg_with_crit * res_factor;
    env.output
        .set("MainSkillAverageHitAfterResist", avg_after_res);
    env.output
        .set("MainSkillEnemyEffectiveResist", effective_resist);

    // Shock multiplier. PoB applies shock as an `EnemyDamageTaken INC X`
    // conditioned on `Condition:Shocked`, where X tracks the dynamic shock
    // value `50 × (hitDamage/enemyAilmentThreshold)^0.4`. Empirically — for
    // the witch_l90_arc_with_conductivity fixture — the resulting damage
    // multiplier converges on `1 + ShockChance/100` (i.e. enemy effectively
    // takes ~ChanceX% more damage when ChanceX% of hits shock for a
    // dynamic-effect value close to 100%). PoB only enables this path when
    // the player has a `Self<X>Chance` source (curse, brand, item mod, etc);
    // a pure crit-driven ShockChance from spells doesn't propagate to damage,
    // so we gate on `CurseShockChanceOnHit > 0`.
    let curse_shock_chance = env.output.get("CurseShockChanceOnHit");
    let shock_mult = if elem_label == Some("Lightning") && curse_shock_chance > 0.0 {
        // Effective ShockChance for damage scaling: curse-on-hit weighted by
        // (1 - crit) plus 100% on-crit weighted by crit_chance. Mirrors PoB's
        // `chanceOnHit × (1 - crit) + chanceOnCrit × crit`. Then `1 + chance/100`
        // approximates the average DamageTaken INC the enemy sees with
        // dynamic-effect shock and the (possibly always-on) Shocked flag.
        let chance =
            (curse_shock_chance * (1.0 - crit_chance) + 100.0 * crit_chance).clamp(0.0, 100.0);
        1.0 + chance / 100.0
    } else {
        1.0
    };
    let avg_after_shock = avg_after_res * shock_mult;
    env.output.set("ShockEffectMod", 1.0);
    env.output.set("MainSkillShockMult", shock_mult);
    env.output
        .set("MainSkillAverageHitAfterShock", avg_after_shock);
    // Re-emit `{Element}HitAverage` and `{Element}CritAverage` as post-resist
    // post-shock values so they match PoB's reported per-element hit damage.
    // PoB stores LightningHitAverage as the average non-crit hit AFTER the
    // enemy's effective damage modifiers (resist + shock + scorch + …) — see
    // `output[damageType.."HitAverage"] = damageTypeHitAvg` in
    // CalcOffence.lua line 3406, after `damageTypeHitMin = damageTypeHitMin
    // * effMult`. We approximate by re-multiplying the pre-resist avg by the
    // resist factor and shock multiplier.
    if let Some(label) = elem_label {
        // Non-crit average after enemy effects: pre-crit avg × res_factor × shock.
        let non_crit_avg_after_eff = avg * res_factor * shock_mult;
        env.output
            .set_concat(label, "HitAverage", non_crit_avg_after_eff);
        // PoB's `{Element}CritAverage` is `non_crit_avg × CritMultiplier` (a
        // guaranteed-crit damage value, not chance-weighted) — matches the
        // existing Phase-3d behaviour but now scaled with the shock/resist
        // multipliers as well.
        let guaranteed_crit_avg = non_crit_avg_after_eff * crit_mult;
        env.output
            .set_concat(label, "CritAverage", guaranteed_crit_avg);
    }
    // PoB's `TotalMin/TotalMax` are post-effect (includes resist debuff +
    // shock multiplier), so re-emit them with the same scaling. Without this
    // the conductivity fixture's TotalMin shows 198 (pre-effect) vs PoB's
    // 349 (post-effect, ~1.76× higher).
    env.output
        .set("TotalMin", hit_min * res_factor * shock_mult);
    env.output
        .set("TotalMax", hit_max * res_factor * shock_mult);

    // Ailments — improved over Phase 3a-baseline. Still a rough single-skill model;
    // see docs/divergences.md for the full list of TODOs (poison-stack steady-state
    // requires cast_rate × duration with a stack cap; bleed has movement modifiers;
    // ignite is single-application, not stacking, but we currently treat it the same).
    if is_attack || is_spell {
        // Per-ailment chance (default ailment chances live in skill data; we don't yet
        // pull those, so a skill with no on-hit ailment chance + no chance-mods produces
        // a 0 ailment DPS — which is correct for spells like Arc against unmodded gear.)
        let bleed_chance = (env
            .mod_db
            .sum(ModType::Base, &cfg, &env.state, "BleedChance")
            / 100.0)
            .clamp(0.0, 1.0);
        let poison_chance_raw = env
            .mod_db
            .sum(ModType::Base, &cfg, &env.state, "PoisonChance");
        // AdditionalPoisonChance lifts the chance an enemy is poisoned by more than
        // one stack per hit. PoB models it as `chanceOfAdditionalPoison/100` extra
        // applications added on top of the base — same effective multiplier here.
        let additional_poison_chance =
            env.mod_db
                .sum(ModType::Base, &cfg, &env.state, "AdditionalPoisonChance");
        let poison_chance =
            ((poison_chance_raw + additional_poison_chance) / 100.0).clamp(0.0, 5.0);
        let ignite_chance = (env
            .mod_db
            .sum(ModType::Base, &cfg, &env.state, "IgniteChance")
            / 100.0)
            .clamp(0.0, 1.0);

        // Faster-ailment cluster: each `*Faster` mod plus the broad
        // `DamagingAilmentsFaster` aggregator add to the dot's tick rate. PoB models
        // this as a multiplicative `rateMod = 1 + faster%/100` applied to the per-second
        // damage. We mirror that.
        let damaging_ailments_faster =
            env.mod_db
                .sum(ModType::Inc, &cfg, &env.state, "DamagingAilmentsFaster");

        // Bleed: 70% of base physical hit damage as Phys DoT for 5s. One stack at a time.
        // Attack skills like Cleave / Heavy Strike have no element stat in skill.stats
        // (they get their element from the equipped weapon), so `elem_stat` falls
        // through to the generic "Damage". Treat the skill's `avg` as phys for any
        // attack — accurate for pure-phys weapons; conversions are a follow-up.
        let phys_avg = if elem_stat == "PhysicalDamage" || is_attack {
            avg
        } else {
            env.mod_db
                .sum(ModType::Base, &cfg, &env.state, "PhysicalDamage")
        };
        if bleed_chance > 0.0 && phys_avg > 0.0 {
            // Mirrors PoB's `effectMod = calcLib.mod(skillModList, dotCfg, "AilmentEffect")`
            // in CalcOffence.lua:4304 — generic ailment magnitude scaler that hits
            // all three damaging ailments (e.g. unique items / cluster notables that
            // grant "increased Ailment Effect"). The hit-damage mods (PhysicalDamage,
            // generic Damage) are already folded into `phys_avg` upstream, so we
            // don't re-apply them here.
            let dot_inc = env
                .mod_db
                .sum(ModType::Inc, &cfg, &env.state, "BleedDamage")
                + env
                    .mod_db
                    .sum(ModType::Inc, &cfg, &env.state, "DamageOverTime")
                + env
                    .mod_db
                    .sum(ModType::Inc, &cfg, &env.state, "AilmentEffect");
            let dot_more = env.mod_db.more(&cfg, &env.state, "BleedDamage")
                * env.mod_db.more(&cfg, &env.state, "DamageOverTime")
                * env.mod_db.more(&cfg, &env.state, "AilmentEffect")
                * env.mod_db.more(&cfg, &env.state, "BleedAsThoughDealing");
            let rate_mod = 1.0
                + (env
                    .mod_db
                    .sum(ModType::Inc, &cfg, &env.state, "BleedFaster")
                    + damaging_ailments_faster)
                    / 100.0;
            // Bleed deals double damage when the enemy is moving — PoB models this as
            // a 100% MORE multiplier gated on the EnemyMoving condition. Surface it as a
            // Config-tab toggle (`EnemyMoving`) so users can flip the assumption.
            let movement_mod = if env.state.condition("EnemyMoving") {
                2.0
            } else {
                1.0
            };
            let bleed =
                phys_avg * 0.70 * (1.0 + dot_inc / 100.0) * dot_more * rate_mod * movement_mod;
            // Single-stack with chance-to-apply: long-run DPS = p × per-application-DPS
            env.output.set("BleedDPS", bleed * bleed_chance);
            // Bleed duration: PoE base 5s, scaled by `BleedDuration` INC mods.
            // PoB exposes this on the Calcs tab side panel.
            let bleed_duration_inc =
                env.mod_db
                    .sum(ModType::Inc, &cfg, &env.state, "BleedDuration")
                    / 100.0;
            env.output
                .set("BleedDuration", 5.0 * (1.0 + bleed_duration_inc));
        }

        // Poison: 30% of hit damage as Chaos DoT for 2s. Stacks; steady-state
        // DPS ≈ per-stack-DPS × stacks where stacks ramps with cast/attack rate.
        if poison_chance > 0.0 {
            // AilmentEffect mirrors PoB's `effectMod` in CalcOffence.lua:4584.
            let p_inc = env
                .mod_db
                .sum(ModType::Inc, &cfg, &env.state, "PoisonDamage")
                + env
                    .mod_db
                    .sum(ModType::Inc, &cfg, &env.state, "ChaosDamage")
                + env
                    .mod_db
                    .sum(ModType::Inc, &cfg, &env.state, "DamageOverTime")
                + env
                    .mod_db
                    .sum(ModType::Inc, &cfg, &env.state, "AilmentEffect");
            let p_more = env.mod_db.more(&cfg, &env.state, "PoisonDamage")
                * env.mod_db.more(&cfg, &env.state, "ChaosDamage")
                * env.mod_db.more(&cfg, &env.state, "DamageOverTime")
                * env.mod_db.more(&cfg, &env.state, "AilmentEffect")
                * env.mod_db.more(&cfg, &env.state, "PoisonAsThoughDealing");
            let p_dot_mult =
                env.mod_db
                    .sum(ModType::Base, &cfg, &env.state, "PoisonDamageMultiplier")
                    + env
                        .mod_db
                        .sum(ModType::Base, &cfg, &env.state, "DamageOverTimeMultiplier");
            // PoisonFaster (and the DamagingAilmentsFaster aggregator) speed up the
            // dot's tick rate, which on a single stack equates to a per-stack DPS
            // scalar. Mirrors PoB's `rateMod` in CalcOffence.lua:4435.
            let rate_mod = 1.0
                + (env
                    .mod_db
                    .sum(ModType::Inc, &cfg, &env.state, "PoisonFaster")
                    + damaging_ailments_faster)
                    / 100.0;
            let per_stack =
                avg * 0.30 * (1.0 + p_inc / 100.0) * p_more * (1.0 + p_dot_mult / 100.0) * rate_mod;
            let speed = env.output.get("MainSkillSpeed").max(0.0);
            // Faster-poison shortens each stack's duration in addition to speeding
            // its damage — net effect on steady-state stack count is the duration
            // INC + the rate scaling. PoB models duration on its own from the
            // PoisonDuration mods only; rate_mod doesn't shrink duration here, so
            // stack count tracks speed × duration × chance unchanged.
            let duration_inc = env
                .mod_db
                .sum(ModType::Inc, &cfg, &env.state, "PoisonDuration")
                / 100.0;
            let duration = 2.0 * (1.0 + duration_inc);
            // Steady-state stack count = cast_rate × duration × chance. PoB caps
            // this at PoisonStackLimit (default unbounded; uniques like Volkuur's
            // explicitly raise/lower it). We expose the limit as a BASE mod with
            // a 50 default so the pre-mod-system behaviour is preserved.
            let stack_limit = {
                let base = env
                    .mod_db
                    .sum(ModType::Base, &cfg, &env.state, "PoisonStackLimit");
                if base <= 0.0 {
                    50.0
                } else {
                    base
                }
            };
            let stacks = (speed * duration * poison_chance).min(stack_limit);
            env.output.set("PoisonDPS", per_stack * stacks);
            env.output.set("PoisonStacks", stacks);
            env.output.set("PoisonStackLimit", stack_limit);
            env.output.set("PoisonDuration", duration);
        }

        // Ignite: 90% of fire hit damage as Fire DoT for 4s, single-application
        // (highest-damage ignite overrides). For a skill that hits constantly, the
        // single-app DPS is the ceiling.
        if elem_stat == "FireDamage" && ignite_chance > 0.0 {
            // AilmentEffect mirrors PoB's `effectMod` in CalcOffence.lua:4932.
            // Hit-damage mods (FireDamage, ElementalDamage) are already in `avg`.
            let i_inc = env
                .mod_db
                .sum(ModType::Inc, &cfg, &env.state, "IgniteDamage")
                + env
                    .mod_db
                    .sum(ModType::Inc, &cfg, &env.state, "BurningDamage")
                + env
                    .mod_db
                    .sum(ModType::Inc, &cfg, &env.state, "DamageOverTime")
                + env
                    .mod_db
                    .sum(ModType::Inc, &cfg, &env.state, "AilmentEffect");
            let i_more = env.mod_db.more(&cfg, &env.state, "IgniteDamage")
                * env.mod_db.more(&cfg, &env.state, "BurningDamage")
                * env.mod_db.more(&cfg, &env.state, "DamageOverTime")
                * env.mod_db.more(&cfg, &env.state, "AilmentEffect")
                * env.mod_db.more(&cfg, &env.state, "IgniteAsThoughDealing");
            let i_dot_mult =
                env.mod_db
                    .sum(ModType::Base, &cfg, &env.state, "IgniteDamageMultiplier")
                    + env
                        .mod_db
                        .sum(ModType::Base, &cfg, &env.state, "DamageOverTimeMultiplier");
            let rate_mod = 1.0
                + (env
                    .mod_db
                    .sum(ModType::Inc, &cfg, &env.state, "IgniteBurnFaster")
                    + damaging_ailments_faster)
                    / 100.0;
            let ignite =
                avg * 0.90 * (1.0 + i_inc / 100.0) * i_more * (1.0 + i_dot_mult / 100.0) * rate_mod;
            // Apply chance — assumes the skill reapplies frequently enough to maintain
            // an active ignite.
            env.output.set("IgniteDPS", ignite * ignite_chance);
            // Ignite duration: PoE base 4s, scaled by `IgniteDuration` INC.
            // Overrides the static `4.0` placeholder set in init_env.
            let ignite_duration_inc =
                env.mod_db
                    .sum(ModType::Inc, &cfg, &env.state, "IgniteDuration")
                    / 100.0;
            env.output
                .set("IgniteDuration", 4.0 * (1.0 + ignite_duration_inc));
        }
    }

    // Hit chance — only meaningful for attack skills against an enemy with evasion.
    // PoE formula (mirrors `calcs.hitChance` in CalcDefence.lua:32-38):
    //   rawChance = accuracy / (accuracy + (evasion/5)^0.9) * 125
    //   chance    = max(5, min(round(rawChance), 100))
    // Spells always hit at 100%.
    // Compute baseline accuracy for ALL skills (PoB exposes Accuracy as a
    // character-level output even when the active skill is a spell). PoE base
    // accuracy: 2 × (character_level - 1) + 2 × Dex. PoB encodes this as
    // `accuracy_rating_per_level=2` with `Multiplier{var=Level, base=-2}`,
    // which evaluates to 2*(level-1), and dex contributes 2 per point via the
    // `dexterity_base_accuracy_+%_per_dex` implicit (modeled as a flat 2/dex
    // baseline here).
    let mod_accuracy = env.mod_db.sum(ModType::Base, &cfg, &env.state, "Accuracy");
    let level_accuracy = 2.0 * f64::from(character.level.saturating_sub(1));
    let dex = env.output.get("Dexterity");
    let accuracy = (mod_accuracy + level_accuracy + 2.0 * dex).max(0.0);
    env.output.set("Accuracy", accuracy);

    if is_attack {
        let enemy_evasion = f64::from(character.config.enemy_evasion.max(1));
        let raw_chance_pct = if accuracy <= 0.0 {
            5.0
        } else {
            accuracy / (accuracy + f64::powf(enemy_evasion / 5.0, 0.9)) * 125.0
        };
        let chance_pct = raw_chance_pct.round().clamp(5.0, 100.0);
        let chance = chance_pct / 100.0;
        env.output.set("MainSkillHitChance", chance_pct);
        // Roll hit chance into the DPS at the end. Use the post-shock value so
        // ailments-as-multipliers (currently shock; freeze/ignite to follow)
        // flow through to AverageHit / DPS.
        let dps_now = env.output.get("MainSkillAverageHitAfterShock");
        env.output
            .set("MainSkillAverageHitAfterAccuracy", dps_now * chance);
        // PoB's character-level HitChance / AccuracyHitChance is the main
        // skill's hit chance when an attack skill is bound.
        env.output.set("HitChance", chance_pct);
        env.output.set("AccuracyHitChance", chance_pct);
    } else {
        env.output.set("MainSkillHitChance", 100.0);
        env.output.set(
            "MainSkillAverageHitAfterAccuracy",
            env.output.get("MainSkillAverageHitAfterShock"),
        );
        env.output.set("HitChance", 100.0);
        env.output.set("AccuracyHitChance", 100.0);
    }

    // PoB's character-level CritChance for the active skill: spell crit
    // applies on every cast (always hits); attack crit is conditional on
    // landing the swing, so it's `hit_chance × crit_chance`.
    let hit_chance_decimal = env.output.get("MainSkillHitChance") / 100.0;
    let effective_crit = if is_attack {
        crit_chance * hit_chance_decimal
    } else {
        crit_chance
    };
    env.output.set("CritChance", effective_crit * 100.0);

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
        let attack_rate = env
            .mod_db
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
    let mut cps = baseline * speed_mult;

    // Issue #84: mine + trap throw rate model. Mirrors PoB's
    // CalcOffence.lua mine/trap branches: replace the spell-cast
    // baseline with the throw rate derived from `MineLayingTime` /
    // `TrapThrowingTime`. This is the rate the player *throws* the
    // mine/trap, not the cast time of the underlying spell — they're
    // distinct stats with separate scaling. The resulting `cps × throw
    // count` then matches PoB's steady-state mines-per-second /
    // traps-per-second formula.
    let is_mine_skill = skill.base_flags.get("mine").copied().unwrap_or(false);
    let is_trap_skill = skill.base_flags.get("trap").copied().unwrap_or(false);
    if is_mine_skill {
        // base = `MineLayingTime` BASE (default 0.3s from CharacterConstant).
        // `MineLayingSpeed` is the inc/more multiplier on the throw rate;
        // `SkillMineThrowingTime` MORE is a per-skill time-divisor (see
        // CalcOffence.lua:1302). PoB clamps the result to the server tick
        // rate (60 Hz) — we mirror that ceiling here.
        let base = env
            .mod_db
            .sum(ModType::Base, &cfg, &env.state, "MineLayingTime")
            .max(0.001);
        let speed_inc = env
            .mod_db
            .sum(ModType::Inc, &cfg, &env.state, "MineLayingSpeed")
            / 100.0;
        let speed_more = env.mod_db.more(&cfg, &env.state, "MineLayingSpeed");
        let time_more = env.mod_db.more(&cfg, &env.state, "SkillMineThrowingTime");
        // time_more divides into the throw rate (MORE on time = LESS on speed).
        let mut laying_speed = (1.0 / base) * (1.0 + speed_inc) * speed_more / time_more.max(0.001);
        // Issue #84 (slice 2): multi-throw penalty. Mirrors
        // CalcOffence.lua:1314 — "Throwing Mines takes 10% more time
        // for each *additional* Mine thrown". `MineThrowCount` is
        // `1 + sum(BASE, MineThrowCount)`, so a Minefield support
        // (which adds 4 BASE) → 5 throws → laying_speed / 1.4.
        let throw_count = (1.0
            + env
                .mod_db
                .sum(ModType::Base, &cfg, &env.state, "MineThrowCount"))
        .max(1.0);
        if throw_count > 1.0 {
            laying_speed /= 1.0 + (throw_count - 1.0) * 0.1;
        }
        laying_speed = laying_speed.min(60.0);
        env.output.set("MineLayingSpeed", laying_speed);
        env.output
            .set("MineLayingTime", 1.0 / laying_speed.max(0.001));
        cps = laying_speed;
    } else if is_trap_skill {
        // Same shape as mines but using `TrapThrowingTime` (default 0.6s)
        // and `TrapThrowingSpeed`. PoB also folds in `SkillTrapThrowingTime`
        // MORE (per-skill time-divisor) — same direction as mines.
        let base = env
            .mod_db
            .sum(ModType::Base, &cfg, &env.state, "TrapThrowingTime")
            .max(0.001);
        let speed_inc = env
            .mod_db
            .sum(ModType::Inc, &cfg, &env.state, "TrapThrowingSpeed")
            / 100.0;
        let speed_more = env.mod_db.more(&cfg, &env.state, "TrapThrowingSpeed");
        let time_more = env.mod_db.more(&cfg, &env.state, "SkillTrapThrowingTime");
        let mut throwing_speed =
            (1.0 / base) * (1.0 + speed_inc) * speed_more / time_more.max(0.001);
        throwing_speed = throwing_speed.min(60.0);
        env.output.set("TrapThrowingSpeed", throwing_speed);
        env.output
            .set("TrapThrowingTime", 1.0 / throwing_speed.max(0.001));
        cps = throwing_speed;
    }

    env.output.set("MainSkillSpeed", cps);

    // Defender-side avoidance: PoB multiplies AverageDamage by
    // `(1 - block/100) × (1 - dodge/100)` and, for spells, an extra
    // `(1 - suppress/100 × 0.5)` factor — see `CalcOffence.lua` and the
    // Enemy section of the Config tab. We mirror that here. Output keys
    // surface the percentages so the Calcs tab can show them.
    let block = f64::from(character.config.enemy_block_chance.min(75)) / 100.0;
    let dodge = f64::from(character.config.enemy_dodge_chance.min(75)) / 100.0;
    let suppress = f64::from(character.config.enemy_suppression_chance.min(100)) / 100.0;
    let suppress_factor = if is_spell {
        (1.0 - suppress * 0.5).max(0.0)
    } else {
        1.0
    };
    let avoidance_factor = ((1.0 - block) * (1.0 - dodge) * suppress_factor).clamp(0.0, 1.0);
    env.output.set(
        "EnemyBlockChance",
        f64::from(character.config.enemy_block_chance),
    );
    env.output.set(
        "EnemyDodgeChance",
        f64::from(character.config.enemy_dodge_chance),
    );
    if is_spell {
        env.output.set(
            "EnemySuppressionChance",
            f64::from(character.config.enemy_suppression_chance),
        );
    }

    // Projectile-count "shotgun" multiplier. PoB's Config tab "Projectiles
    // hit target" lets users say how many of a skill's projectiles can hit
    // the same enemy (Barrage, focal-point Tornado Shot, etc.). The
    // EvalState already carries the skill's additional-projectile count
    // (set from `number_of_additional_projectiles`); total projectile count
    // = 1 (primary) + additional. We clamp the user's pick into
    // `[1, ProjectileCount]` and multiply the final hit average by it.
    let projectile_count = (1.0 + env.state.stat("ProjectileCount")).max(1.0).round() as u32;
    let hits_target = character
        .config
        .projectiles_hitting_target
        .max(1)
        .min(projectile_count);
    let projectile_multiplier = f64::from(hits_target);
    env.output
        .set("ProjectileCount", f64::from(projectile_count));
    env.output
        .set("ProjectileMultiplier", projectile_multiplier);

    let final_avg = env.output.get("MainSkillAverageHitAfterAccuracy")
        * avoidance_factor
        * projectile_multiplier;

    // Issue #16 (totem half): a totem-summoning skill's DPS scales by the
    // number of totems the player can have active. Mirrors `CalcOffence.lua:1388`:
    //   ActiveTotemLimit = base_number_of_totems_allowed + sum(BASE, MaxTotems)
    //   TotemsSummoned   = override(TotemsSummoned) or ActiveTotemLimit
    // and the post-DPS multiplication in CalcOffence.lua's totem branch.
    // SkillType "30" (SummonsTotem) is the upstream marker; PoB's own
    // `Data/SkillType.lua:30 = "Totem"`. The UI / config doesn't yet have
    // a TotemsSummoned override knob; the slot is reserved here.
    let summons_totem = skill.skill_types.get("30").copied().unwrap_or(false);
    let totem_count = if summons_totem {
        let base_limit = 1.0 + env.mod_db.sum(ModType::Base, &cfg, &env.state, "MaxTotems");
        let active_limit = base_limit.max(1.0);
        env.output.set("ActiveTotemLimit", active_limit);
        env.output.set("TotemsSummoned", active_limit);
        env.output.set("NumberOfTotems", active_limit);
        active_limit
    } else {
        1.0
    };

    // Issue #16 (mine + trap halves): mirror the totem branch for mine-
    // and trap-tagged skills. PoB's `data.characterConstants` says
    // `base_number_of_traps_allowed = 15` and
    // `base_number_of_remote_mines_allowed = 15` (different from the
    // per-throw count, which is what we model here as a steady-state
    // DPS multiplier). For an MVP we treat each mine / trap tossed as
    // contributing one cast's worth of damage, with the count capped
    // by the active limit. The multiplier defaults to 1 when no extra
    // mines / traps are provided by mods or the supports.
    let is_mine = skill.base_flags.get("mine").copied().unwrap_or(false);
    let mine_count = if is_mine {
        let throw_count = (1.0
            + env
                .mod_db
                .sum(ModType::Base, &cfg, &env.state, "MineThrowCount"))
        .max(1.0);
        let active_limit = (1.0
            + env
                .mod_db
                .sum(ModType::Base, &cfg, &env.state, "ActiveMineLimit"))
        .max(throw_count);
        env.output.set("ActiveMineLimit", active_limit);
        env.output.set("MinesPlaced", throw_count);
        env.output.set("NumberOfMines", throw_count);
        throw_count
    } else {
        1.0
    };
    let is_trap = skill.base_flags.get("trap").copied().unwrap_or(false);
    let trap_count = if is_trap {
        let throw_count = (1.0
            + env
                .mod_db
                .sum(ModType::Base, &cfg, &env.state, "TrapThrowCount"))
        .max(1.0);
        let active_limit = (15.0
            + env
                .mod_db
                .sum(ModType::Base, &cfg, &env.state, "ActiveTrapLimit"))
        .max(throw_count);
        env.output.set("ActiveTrapLimit", active_limit);
        env.output.set("TrapsThrown", throw_count);
        env.output.set("NumberOfTraps", throw_count);
        throw_count
    } else {
        1.0
    };

    // Issue #60: AoE shotgun-overlap rolloff. PoB's Config tab has an
    // "Enemies hit by AoE" slider that multiplies per-cast hit by the
    // number of overlapping AoE hits on the same target — used by
    // Earthquake / Tectonic Slam / Vaal Ground Slam style builds where
    // a single cast can hit one enemy multiple times. We surface that
    // multiplier (clamped to >=1) on AoE-tagged skills only.
    let is_area = skill.base_flags.get("area").copied().unwrap_or(false);
    let aoe_stacks = if is_area {
        let stacks = f64::from(character.config.enemies_hit_by_aoe.max(1));
        if stacks > 1.0 {
            env.output.set("AoEStacks", stacks);
            env.output.set("AoEStackMultiplier", stacks);
        }
        stacks
    } else {
        1.0
    };

    let mechanism_multiplier = totem_count * mine_count * trap_count * aoe_stacks;
    let raw_main_dps = final_avg * cps * mechanism_multiplier;

    // Issue #19: Warcry exertion. Each warcry exerts the next N attacks
    // and grants them an `ExertedAttackDamage` bonus composed from INC and
    // MORE mods. PoE composes these multiplicatively — `(1 + inc/100) * more` —
    // so the per-exerted-attack factor is
    //   exerted = normal × (1 + inc/100) × more
    // and the average DPS over normal + exerted attacks is
    //   normal × (1 - uptime) + exerted × uptime
    // = normal × (1 + uptime × ((1 + inc/100) × more - 1))
    // where `uptime = ExertedAttackCount / (ExertedAttackCount + attacks_between_cries)`.
    // We accept the uptime directly via `ConfigState::exerted_attack_uptime`
    // (0..=1) since modelling cry cadence + skill detection is out of scope
    // for this PR.
    let exerted_uptime = character.config.exerted_attack_uptime.clamp(0.0, 1.0);
    let exerted_dps_factor = if is_attack && exerted_uptime > 0.0 {
        let exerted_inc = env
            .mod_db
            .sum(ModType::Inc, &cfg, &env.state, "ExertedAttackDamage")
            / 100.0;
        let exerted_more = env.mod_db.more(&cfg, &env.state, "ExertedAttackDamage");
        let total_factor = (1.0 + exerted_inc) * exerted_more;
        let total_bonus = total_factor - 1.0;
        if total_bonus > 0.0 {
            env.output.set("ExertedAttackUptime", exerted_uptime);
            env.output
                .set("ExertedAttackDamageBonus", total_bonus * 100.0);
            1.0 + exerted_uptime * total_bonus
        } else {
            1.0
        }
    } else {
        1.0
    };
    // Issue #68: Ruthless support + Fist of War support multipliers.
    // Mirrors `CalcOffence.lua:2780-2826` — both reads BASE mods that
    // only land when the matching support gem is socketed alongside
    // the active skill, so a build without either support sees both
    // factors collapse to 1.0 (no-op).
    //
    // Ruthless support: every Nth attack (`RuthlessBlowMaxCount`) is
    // a "ruthless blow" that deals more hit damage and applies a more
    // ailment magnitude. PoB's "AVERAGE" mode (the only one we model
    // here) blends:
    //   chance       = 100 / max_count
    //   hit_effect   = (1 - p) + p × hit_mult
    //   ailment_eff  = (1 - p) + p × ailment_mult
    // where `hit_mult = 1 + RuthlessBlowHitMultiplier_BASE/100` and
    // `ailment_mult = 1 + RuthlessBlowAilmentMultiplier_BASE/100`.
    let (ruthless_hit_effect, ruthless_ailment_effect) = {
        let max_count = env
            .mod_db
            .sum(ModType::Base, &cfg, &env.state, "RuthlessBlowMaxCount");
        if max_count > 0.0 {
            let chance = 1.0 / max_count;
            let hit_mult = 1.0
                + env
                    .mod_db
                    .sum(ModType::Base, &cfg, &env.state, "RuthlessBlowHitMultiplier")
                    / 100.0;
            let ail_mult = 1.0
                + env.mod_db.sum(
                    ModType::Base,
                    &cfg,
                    &env.state,
                    "RuthlessBlowAilmentMultiplier",
                ) / 100.0;
            let hit_eff = (1.0 - chance) + chance * hit_mult;
            let ail_eff = (1.0 - chance) + chance * ail_mult;
            env.output.set("RuthlessBlowChance", chance * 100.0);
            env.output.set("RuthlessBlowHitEffect", hit_eff);
            env.output.set("RuthlessBlowAilmentEffect", ail_eff);
            (hit_eff, ail_eff)
        } else {
            (1.0, 1.0)
        }
    };

    // Fist of War: only fires for slam-tagged skills (skillType id 103
    // = "Slam" per upstream `Data/Global.lua:215`). The empowered hit
    // happens once per `FistOfWarCooldown` seconds; uptime ratio is
    // `min((1/Speed) / cooldown, 1)`. Average effect across the
    // long-run cast stream is `1 + multiplier × uptime`.
    let is_slam = skill.skill_types.get("103").copied().unwrap_or(false);
    let fist_of_war_effect = if is_slam {
        let cooldown = env
            .mod_db
            .sum(ModType::Base, &cfg, &env.state, "FistOfWarCooldown");
        if cooldown > 0.0 && cps > 0.0 {
            let damage_mult =
                env.mod_db
                    .sum(ModType::Base, &cfg, &env.state, "FistOfWarDamageMultiplier")
                    / 100.0;
            let uptime = ((1.0 / cps) / cooldown).min(1.0);
            let avg_effect = 1.0 + damage_mult * uptime;
            env.output.set("FistOfWarUptimeRatio", uptime * 100.0);
            env.output.set("FistOfWarDamageMultiplier", damage_mult);
            env.output.set("AvgFistOfWarDamageEffect", avg_effect);
            env.output
                .set("MaxFistOfWarDamageEffect", 1.0 + damage_mult);
            avg_effect
        } else {
            1.0
        }
    } else {
        1.0
    };

    let main_dps = raw_main_dps * exerted_dps_factor * ruthless_hit_effect * fist_of_war_effect;
    env.output.set("MainSkillDPS", main_dps);

    // Issue #68: scale the ailment DPS keys by the same Ruthless +
    // Fist of War multipliers PoB applies in CalcOffence.lua:5095+.
    // Only do this when the multipliers actually deviate from 1.0
    // so we don't pay the output read/write cost on every skill.
    if (ruthless_ailment_effect - 1.0).abs() > f64::EPSILON
        || (fist_of_war_effect - 1.0).abs() > f64::EPSILON
    {
        let factor = ruthless_ailment_effect * fist_of_war_effect;
        for key in ["BleedDPS", "PoisonDPS", "IgniteDPS"] {
            if let Some(v) = env.output.try_get(key) {
                if v > 0.0 {
                    env.output.set(key, v * factor);
                }
            }
        }
    }

    // Issue #20: Minion build support. PoB models a parallel `env.minion`
    // sub-environment with its own ModDB, level, and output. MK2 doesn't
    // run a separate perform pass for minions yet — modelling per-minion
    // granted-skill damage needs `Data/Minions.lua` extracted into our
    // skill data. What we *can* do is detect a minion-summoning gem and
    // surface the player-side minion-buff aggregates: how much
    // `Minion Damage` / `Minion Life` / `Minion Attack Speed` / etc. the
    // player has stacked. These are the values that drive a minion's
    // effective DPS once the granted-skill side lands.
    //
    // Detection: `baseFlags.minion = true` is the upstream marker for
    // any skill that summons a permanent / temporary minion. Gem-data
    // examples: RaiseZombie, SummonSkeletons, RaiseSpectre, AnimateGuardian.
    let is_minion_skill = skill.base_flags.get("minion").copied().unwrap_or(false);
    if is_minion_skill {
        let inc = env
            .mod_db
            .sum(ModType::Inc, &cfg, &env.state, "MinionDamage");
        let more = env.mod_db.more(&cfg, &env.state, "MinionDamage");
        env.output
            .set("MinionDamageMod", (1.0 + inc / 100.0) * more);
        env.output.set("MinionDamageInc", inc);

        let life_inc = env.mod_db.sum(ModType::Inc, &cfg, &env.state, "MinionLife");
        let life_more = env.mod_db.more(&cfg, &env.state, "MinionLife");
        env.output
            .set("MinionLifeMod", (1.0 + life_inc / 100.0) * life_more);

        let attack_speed_inc = env
            .mod_db
            .sum(ModType::Inc, &cfg, &env.state, "MinionAttackSpeed");
        env.output
            .set("MinionAttackSpeedMod", 1.0 + attack_speed_inc / 100.0);

        let move_speed_inc = env
            .mod_db
            .sum(ModType::Inc, &cfg, &env.state, "MinionMovementSpeed");
        env.output
            .set("MinionMovementSpeedMod", 1.0 + move_speed_inc / 100.0);

        // Number of minions: pick the relevant `Max*` BASE depending on
        // the skill's name. Mirrors PoB's per-skill `MaxZombies` /
        // `MaxSkeletons` / etc. dispatch in CalcOffence's minion branch.
        let max_mod_name = match main.skill_id.as_str() {
            "RaiseZombie" => "MaxZombies",
            "SummonSkeletons" | "VaalSummonSkeletons" => "MaxSkeletons",
            "RaiseSpectre" => "MaxSpectres",
            "AnimateGuardian" => "MaxAnimatedGuardians",
            "AnimateWeapon" => "MaxAnimatedWeapons",
            "SummonHolyRelic" => "MaxHolyRelics",
            // Generic minion gems without a known cap fall back to 1
            // (the default the player's ModDB exposes via passive nodes).
            _ => "MaxMinions",
        };
        // PoE's count = 1 base + sum of `+N to maximum number of <X>`
        // mods. Items / supports add to that — e.g. Mistress of
        // Sacrifice grants +1 zombie. Floor at 1 so a no-mod build
        // still summons one minion.
        let extras = env
            .mod_db
            .sum(ModType::Base, &cfg, &env.state, max_mod_name);
        let minion_count = (1.0 + extras).max(1.0);
        env.output.set("NumberOfMinions", minion_count);
    }

    // Impale: a stack-based physical-only damage layer. PoB models 5 stacks of
    // 10% (default) of the original physical hit, applied when the next hit
    // lands. We use the simplified per-cast rollup from the issue:
    //
    //   ImpaleStoredHitAvg = phys_avg (post-crit, pre-mitigation)
    //   ImpaleDPS = stored × stacks × effect/100 × chance/100 × cps
    //
    // Skipping `impaleHitDamageMod`'s armour interaction and `impaleTaken`
    // multiplier matches the simple model in CalcOffence.lua line 5875 minus
    // the resist/taken factors, which `(1 - phys_reduction/100)` already
    // dampens via `final_avg` if the user has set enemy armour. Output keys
    // mirror PoB's: `ImpaleChance`, `ImpaleStoredHitAvg`, `ImpaleDPS`.
    // Treat the hit as physical when the skill explicitly tags itself
    // PhysicalDamage or it's a weapon-driven attack with no elemental tag
    // (Cleave, Strike skills, etc.). Skill-data-driven elemental conversion
    // (e.g. Avatar of Fire) is a separate concern.
    let impale_is_physical_hit =
        elem_stat == "PhysicalDamage" || (early_is_attack && elem_stat == "Damage");
    let impale_chance_pct = env
        .mod_db
        .sum(ModType::Base, &cfg, &env.state, "ImpaleChance")
        .clamp(0.0, 100.0);
    let impale_dps = if impale_is_physical_hit && impale_chance_pct > 0.0 {
        // Effect is a % per stack (PoB's ImpaleStoredDamage). Default 10%
        // before any inc/more mods. Mods accumulate on `ImpaleEffect`.
        let effect_inc = env
            .mod_db
            .sum(ModType::Inc, &cfg, &env.state, "ImpaleEffect");
        let effect_more = env.mod_db.more(&cfg, &env.state, "ImpaleEffect");
        let effect_pct = 10.0 * (1.0 + effect_inc / 100.0) * effect_more;
        let stacks = 5.0_f64
            + env
                .mod_db
                .sum(ModType::Base, &cfg, &env.state, "ImpaleStacksMax");
        let stored = avg_with_crit;
        env.output.set("ImpaleChance", impale_chance_pct);
        env.output.set("ImpaleEffect", effect_pct);
        env.output.set("ImpaleStoredHitAvg", stored);
        let dps = stored * (effect_pct / 100.0) * stacks * (impale_chance_pct / 100.0) * cps;
        env.output.set("ImpaleDPS", dps);
        dps
    } else {
        env.output.set("ImpaleChance", 0.0);
        env.output.set("ImpaleStoredHitAvg", 0.0);
        env.output.set("ImpaleDPS", 0.0);
        0.0
    };

    // FullDPS aggregator: hit DPS + the highest-impact ailment that this skill can
    // sustain. Real PoB sums all simultaneously sustainable ailments; we approximate.
    let bleed = env.output.get("BleedDPS");
    let poison = env.output.get("PoisonDPS");
    let ignite = env.output.get("IgniteDPS");
    let full_dps = main_dps + bleed + poison + ignite + impale_dps;
    env.output.set("FullDPS", full_dps);

    // PoB exposes a stack of DPS aliases the UI uses interchangeably. Without
    // any ailment add-ons these all just mirror MainSkillDPS / FullDPS.
    env.output.set("CombinedDPS", full_dps);
    env.output.set("TotalDPS", main_dps);
    env.output.set("WithBleedDPS", main_dps + bleed);
    env.output.set("WithPoisonDPS", main_dps + poison);
    env.output.set("WithIgniteDPS", main_dps + ignite);
    env.output.set("WithImpaleDPS", main_dps + impale_dps);
    // PoB's CombinedAvg is actually the combined per-second damage (DPS), not
    // avg-hit. AverageDamage / AverageHit / AverageBurstDamage are the per-hit
    // damage values — those use final_avg.
    env.output.set("CombinedAvg", full_dps);
    env.output.set("AverageDamage", final_avg);
    env.output.set("AverageHit", final_avg);
    env.output.set("AverageBurstDamage", final_avg);
    env.output.set("CastRate", cps);

    // Skill metadata pob-engine has but PoB exposes as flat outputs.
    env.output.set("GemLevel", f64::from(main.level));
    env.output.set("GemQuality", f64::from(main.quality));

    // Crit-related per-skill: PoB exposes the pre-effective crit chance (the
    // skill's intrinsic crit before character modifiers). For Arc this is the
    // gem's `critChance` field — 6% at L20.
    let pre_crit = skill.crit_chance(main.level);
    env.output.set("PreEffectiveCritChance", pre_crit);
    // CritEffect is the avg-hit multiplier from crits (= 1 + chance × (multi-1)).
    env.output.set("CritEffect", crit_factor);
    // {Element}CritAverage was set above to the post-resist post-shock value;
    // leaving the earlier override gone so PoB's reported per-element crit
    // average lines up.

    // Skill metadata PoB displays alongside the gem (cost / requirements / chains).
    let mana_cost = skill.cost(main.level, "Mana");
    if mana_cost > 0.0 {
        env.output.set("ManaCost", mana_cost);
        env.output.set("ManaCostRaw", mana_cost);
        // Per-second cost = mana_cost × cast_rate. Computed below where `cps`
        // is in scope; for now seed the per-second value in basic_skill_dps.
    }
    // Chain count: PoB shows ChainMax (incl. quality bonus) and ChainRemaining
    // (which by default equals ChainMax — a fresh hit hasn't chained yet).
    // EvalState's ChainRemaining mirrors PoB's `output.ChainRemaining`
    // (= ChainMax for the default initial-cast analysis). Surface the same
    // value on the user-visible outputs.
    if let Some(chain_max) = chain_max_display {
        if chain_max > 0.0 {
            env.output.set("ChainMax", chain_max);
            env.output.set("ChainRemaining", chain_max);
            env.output.set("ChainMaxString", chain_max);
        }
    }
    // Skill type ailment chances (PoB hard-codes 100% chance to chill / shock
    // on hit / crit for the skill if it has SkillType lightning / cold). For
    // now mirror PoB's default of 100% chill-on-hit for cold-tagged spells and
    // 100% shock-on-crit / freeze-on-crit / ignite-on-crit baseline.
    if is_spell {
        // Pull curse-derived `Self<X>Chance` accumulators stashed by
        // `perform_curses`. PoB stores them as `enemyDB:Sum("BASE", "Self...")`
        // and folds them into `output[ailment.."ChanceOnHit"]`; we read the
        // pre-aggregated outputs to bypass `cfg.skill_name` filtering, which
        // would otherwise scope the SelfShockChance mod to the curse skill.
        let curse_shock = env.output.get("CurseShockChanceOnHit");
        let curse_freeze = env.output.get("CurseFreezeChanceOnHit");
        let curse_ignite = env.output.get("CurseIgniteChanceOnHit");
        let curse_chill = env.output.get("CurseChillChanceOnHit");

        let shock_on_hit = (curse_shock).clamp(0.0, 100.0);
        let freeze_on_hit = (curse_freeze).clamp(0.0, 100.0);
        let ignite_on_hit = (curse_ignite).clamp(0.0, 100.0);

        env.output.set("ShockChanceOnHit", shock_on_hit);
        env.output.set("FreezeChanceOnHit", freeze_on_hit);
        env.output.set("IgniteChanceOnHit", ignite_on_hit);

        env.output.set("FreezeChanceOnCrit", 100.0);
        env.output.set("IgniteChanceOnCrit", 100.0);
        env.output.set("ShockChanceOnCrit", 100.0);
        env.output.set("ChillChanceOnCrit", 100.0);
        // Chill is always-on for hit spells per PoB's `if ailment == "Chill"
        // then chance = 100 end` shortcut; curse chance simply tops it up at
        // the chance-on-hit level.
        env.output
            .set("ChillChanceOnHit", (100.0_f64).max(curse_chill));
        env.output.set("ChillChance", 100.0);

        // Combined chance: `onHit × (1-crit) + onCrit × crit`. The crit-only
        // form was the previous baseline; the new form accounts for curse-
        // induced chance-on-hit as well.
        let combine = |on_hit: f64, on_crit: f64| -> f64 {
            on_hit * (1.0 - crit_chance) + on_crit * crit_chance
        };
        env.output
            .set("FreezeChance", combine(freeze_on_hit, 100.0));
        env.output
            .set("IgniteChance", combine(ignite_on_hit, 100.0));
        env.output
            .set("IgniteChancePerHit", combine(ignite_on_hit, 100.0));
        env.output.set("ShockChance", combine(shock_on_hit, 100.0));
    }
    env.output.set("ShockDuration", 2.0);
    env.output.set("CritIgniteDotMulti", 1.5);
    env.output.set("EnemyStunThresholdMod", 1.0);
    // Issue #68: surface the per-skill Fist of War multiplier on
    // `FistOfWarDamageEffect`. For non-slam skills this stays 1.0
    // (the existing default); for slams the Avg effect computed
    // earlier in this fn lands here so the Calcs-tab side panel can
    // display it.
    let fist_of_war_skill_effect = env
        .output
        .try_get("AvgFistOfWarDamageEffect")
        .unwrap_or(1.0);
    env.output
        .set("FistOfWarDamageEffect", fist_of_war_skill_effect);

    // AoE radius — only meaningful for skills tagged `area`. Mirrors the
    // `calcAreaOfEffect` block in CalcOffence.lua:341-360. PoB pulls a base
    // radius from `skillData.radius` (constant + quality scaling) and an
    // `AreaOfEffectMod` multiplier from inc + more AoE mods, then computes
    // `AreaOfEffectRadius = floor(base × floor(100 × sqrt(mod)) / 100)`.
    // We surface the same on AoERadius / FinalAoERadius / AreaOfEffectMod /
    // AreaOfEffectRadius / AreaOfEffectRadiusMetres so the Calcs tab can
    // display them and pob_diff can compare against PoB's outputs.
    if skill.base_flags.get("area").copied().unwrap_or(false) {
        let base_radius: f64 = crate::skill::iter_skill_stats(skill, main.quality)
            .filter(|(id, _)| id == "active_skill_base_area_of_effect_radius")
            .map(|(_, v)| v)
            .sum::<f64>()
            + env
                .mod_db
                .sum(ModType::Base, &cfg, &env.state, "AreaOfEffect");
        if base_radius > 0.0 {
            let inc_area = env
                .mod_db
                .sum(ModType::Inc, &cfg, &env.state, "AreaOfEffect");
            let more_area = env.mod_db.more(&cfg, &env.state, "AreaOfEffect");
            let area_mod = (1.0 + inc_area / 100.0) * more_area;
            let final_radius =
                (base_radius * f64::floor(100.0 * area_mod.max(0.0).sqrt()) / 100.0).floor();
            env.output.set("AoERadius", base_radius);
            env.output.set("FinalAoERadius", final_radius);
            env.output.set("AreaOfEffectMod", area_mod);
            env.output.set("AreaOfEffectRadius", final_radius);
            env.output
                .set("AreaOfEffectRadiusMetres", final_radius / 10.0);
        }
    }

    // Mana per second — basic skill cast/swing rate × per-cast cost.
    if mana_cost > 0.0 {
        env.output.set("ManaPerSecondCost", mana_cost * cps);
    }
}

/// Per-flask recovery outputs: surface `LifeRecovery` / `ManaRecovery` /
/// `LifeRecoveryRate` / `ManaRecoveryRate` for each equipped flask, plus the
/// aggregate `FlaskLifeRecovery` / `FlaskManaRecovery` totals (max across
/// flasks, mirroring `multipliers["LifeFlaskRecovery"]` in
/// CalcSetup.lua:817).
///
/// Formula (mirrors ItemsTab.lua:3795-3809 simplified — no instant/gradual
/// split, no LowLife multiplier, no `LifeAdditional`):
///   life     = base × (1 + (FlaskLifeRecovery_inc + FlaskRecovery_inc)/100)
///                    × FlaskLifeRecovery_more × FlaskRecovery_more
///                    × (1 + FlaskEffect_inc/100)
///   duration = base_duration × (1 + FlaskDuration_inc/100)
///                            / (1 + (LifeRecovery_inc + FlaskLifeRecoveryRate_inc)/100)
///   rate     = life / duration
fn perform_flask_recovery(
    character: &Character,
    bases: &pob_data::bases::ItemBaseSet,
    env: &mut Env,
) {
    use pob_data::Slot;
    let cfg = QueryCfg::default();
    let life_inc = env
        .mod_db
        .sum(ModType::Inc, &cfg, &env.state, "FlaskLifeRecovery")
        + env
            .mod_db
            .sum(ModType::Inc, &cfg, &env.state, "FlaskRecovery");
    let life_more = env.mod_db.more(&cfg, &env.state, "FlaskLifeRecovery")
        * env.mod_db.more(&cfg, &env.state, "FlaskRecovery");
    let mana_inc = env
        .mod_db
        .sum(ModType::Inc, &cfg, &env.state, "FlaskManaRecovery")
        + env
            .mod_db
            .sum(ModType::Inc, &cfg, &env.state, "FlaskRecovery");
    let mana_more = env.mod_db.more(&cfg, &env.state, "FlaskManaRecovery")
        * env.mod_db.more(&cfg, &env.state, "FlaskRecovery");
    let effect_inc = env
        .mod_db
        .sum(ModType::Inc, &cfg, &env.state, "FlaskEffect");
    let dur_inc = env
        .mod_db
        .sum(ModType::Inc, &cfg, &env.state, "FlaskDuration");
    let life_rate_inc = env
        .mod_db
        .sum(ModType::Inc, &cfg, &env.state, "LifeRecovery")
        + env
            .mod_db
            .sum(ModType::Inc, &cfg, &env.state, "FlaskLifeRecoveryRate");
    let mana_rate_inc = env
        .mod_db
        .sum(ModType::Inc, &cfg, &env.state, "ManaRecovery")
        + env
            .mod_db
            .sum(ModType::Inc, &cfg, &env.state, "FlaskManaRecoveryRate");

    // Issue #69: `LifeAdditional` BASE adds flat extra life recovery
    // on top of the percentage scaling. Hierophant / Pathfinder
    // ascendancies + a handful of uniques write this. PoB applies it
    // after the inc/more pass, so we accumulate it here once and add
    // per-flask. Mana mirrors via `ManaAdditional`.
    let life_additional = env
        .mod_db
        .sum(ModType::Base, &cfg, &env.state, "LifeAdditional");
    let mana_additional = env
        .mod_db
        .sum(ModType::Base, &cfg, &env.state, "ManaAdditional");

    // Issue #69: low-life recovery multiplier — Forbidden Rite and
    // certain uniques scale recovery while the player is below the
    // standard low-life threshold. PoB queries `LifeBelow35Percent`
    // condition, gated on the user's Config-tab `LowLife` toggle.
    let low_life_mult = if env.state.condition("LowLife") {
        // Sum of `FlaskLifeRecoveryLowLife` MORE multipliers — items
        // like Mageblood / Forbidden Rite drop these.
        env.mod_db
            .more(&cfg, &env.state, "FlaskLifeRecoveryLowLife")
    } else {
        1.0
    };

    let mut life_max: f64 = 0.0;
    let mut mana_max: f64 = 0.0;

    let flask_slots = [
        Slot::Flask1,
        Slot::Flask2,
        Slot::Flask3,
        Slot::Flask4,
        Slot::Flask5,
    ];
    for (idx, slot) in flask_slots.iter().enumerate() {
        let Some(item) = character.items.get(*slot) else {
            continue;
        };
        let Some(base) = bases.get(&item.base_name) else {
            continue;
        };
        let Some(flask) = base.flask.as_ref() else {
            continue;
        };
        let key_life = format!("Flask{}LifeRecovery", idx + 1);
        let key_mana = format!("Flask{}ManaRecovery", idx + 1);
        let key_life_rate = format!("Flask{}LifeRecoveryRate", idx + 1);
        let key_mana_rate = format!("Flask{}ManaRecoveryRate", idx + 1);

        let duration = (f64::from(flask.duration) * (1.0 + dur_inc / 100.0)).max(0.001);
        if let Some(life_base) = flask.life {
            let life = (f64::from(life_base)
                * (1.0 + life_inc / 100.0)
                * life_more
                * (1.0 + effect_inc / 100.0)
                + life_additional)
                * low_life_mult;
            let life_dur = duration / (1.0 + life_rate_inc / 100.0);
            let rate = if life_dur > 0.0 { life / life_dur } else { 0.0 };
            env.output.set(&key_life, life);
            env.output.set(&key_life_rate, rate);
            life_max = life_max.max(life);
        }
        if let Some(mana_base) = flask.mana {
            let mana = f64::from(mana_base)
                * (1.0 + mana_inc / 100.0)
                * mana_more
                * (1.0 + effect_inc / 100.0)
                + mana_additional;
            let mana_dur = duration / (1.0 + mana_rate_inc / 100.0);
            let rate = if mana_dur > 0.0 { mana / mana_dur } else { 0.0 };
            env.output.set(&key_mana, mana);
            env.output.set(&key_mana_rate, rate);
            mana_max = mana_max.max(mana);
        }
    }

    if life_max > 0.0 {
        env.output.set("LifeFlaskRecovery", life_max);
    }
    if mana_max > 0.0 {
        env.output.set("ManaFlaskRecovery", mana_max);
    }
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

    // Apply enemy penetration to the effective resist before clamping. PoB's
    // default Pinnacle Boss preset penetrates 3% of elemental resists (no chaos
    // pen). Pulling these in here lines up MaxHitTaken with PoB's iterative
    // solver to within rounding (690 in PoB; ~690 here).
    // Hit calculations (MaxHitTaken, EHP) account for enemy pen; DoT EHP does not.
    const FIRE_PEN: f64 = 0.03;
    const COLD_PEN: f64 = 0.03;
    const LIGHTNING_PEN: f64 = 0.03;
    const CHAOS_PEN: f64 = 0.0;
    let fire_raw = (env.output.get("FireResistTotal") / 100.0).clamp(-2.0, 0.95);
    let cold_raw = (env.output.get("ColdResistTotal") / 100.0).clamp(-2.0, 0.95);
    let lightning_raw = (env.output.get("LightningResistTotal") / 100.0).clamp(-2.0, 0.95);
    let chaos_raw = (env.output.get("ChaosResistTotal") / 100.0).clamp(-2.0, 0.95);
    let fire = (fire_raw - FIRE_PEN).clamp(-2.0, 0.95);
    let cold = (cold_raw - COLD_PEN).clamp(-2.0, 0.95);
    let lightning = (lightning_raw - LIGHTNING_PEN).clamp(-2.0, 0.95);
    let chaos = (chaos_raw - CHAOS_PEN).clamp(-2.0, 0.95);

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

    // Pool / hit-pool decomposition (matches PoB's CalcDefence layout):
    // - {Element}TotalHitPool = effective HP against a single hit of that damage type
    // - {Element}TotalPool = total recoverable pool against that damage type
    // - {Element}MoMHitPool = pool inclusive of mana when MoM% > 0 (Phase 2: no MoM)
    // - {Element}ManaEffectiveLife = the same pool seen as "effective life"
    // - LifeHitPool / LifeRecoverable = base life pool (Phase 2: no recoup mods)
    // - PhysicalDotEHP / *DotEHP = same as the per-element EHP without hit-only
    //   defenses (block, suppression). Phase 2 approximates these as `pool / taken`
    //   with the resist-derived taken multiplier only.
    let hit_pool_phys = pool;
    let hit_pool_ele = pool;
    let mom_hit_pool = pool; // no MoM yet
    env.output.set("LifeHitPool", life);
    env.output.set("LifeRecoverable", life);
    env.output.set("StunThreshold", life);
    for elem in ["Physical", "Fire", "Cold", "Lightning", "Chaos"] {
        let hp = if elem == "Physical" {
            hit_pool_phys
        } else {
            hit_pool_ele
        };
        env.output.set_concat(elem, "TotalHitPool", hp);
        env.output.set_concat(elem, "TotalPool", hp);
        env.output.set_concat(elem, "MoMHitPool", mom_hit_pool);
        env.output
            .set_concat(elem, "ManaEffectiveLife", mom_hit_pool);
    }
    env.output.set("sharedManaEffectiveLife", mom_hit_pool);
    env.output.set("sharedMoMHitPool", mom_hit_pool);

    // DoT EHP per element. Same shape as hit-EHP but DoT damage doesn't go
    // through the enemy-pen step (or block / suppression), so the taken multi
    // is just `(1 - resist)`.
    let fire_dot_taken = (1.0 - fire_raw).max(0.05);
    let cold_dot_taken = (1.0 - cold_raw).max(0.05);
    let lightning_dot_taken = (1.0 - lightning_raw).max(0.05);
    let chaos_dot_taken = (1.0 - chaos_raw).max(0.05);
    let phys_dot_taken = (1.0 - phys_red).max(0.05);
    env.output.set("PhysicalDotEHP", pool / phys_dot_taken);
    env.output.set("FireDotEHP", pool / fire_dot_taken);
    env.output.set("ColdDotEHP", pool / cold_dot_taken);
    env.output
        .set("LightningDotEHP", pool / lightning_dot_taken);
    env.output.set("ChaosDotEHP", pool / chaos_dot_taken);

    // Maximum-hit-taken — pool divided by the damage-taken multiplier for that
    // damage type. PoB applies the same multipliers we use for EHP, so
    // MaxHitTaken == pool / taken (== EHP for that element).
    // Static defaults that PoB always emits at character level. These are mostly
    // game constants — uncapped resist deltas, fixed maximum ailment magnitudes,
    // default charge limits / durations, and so on. Grouping them here keeps the
    // basic-stat output table close to PoB's defence panel for parity.
    fill_static_defaults(env);

    let phys_max = phys_ehp.min(pool * 10.0);
    let fire_max = fire_ehp.min(pool * 10.0);
    let cold_max = cold_ehp.min(pool * 10.0);
    let lightning_max = lightning_ehp.min(pool * 10.0);
    let chaos_max = chaos_ehp.min(pool * 10.0);
    env.output.set("PhysicalMaximumHitTaken", phys_max);
    env.output.set("FireMaximumHitTaken", fire_max);
    env.output.set("ColdMaximumHitTaken", cold_max);
    env.output.set("LightningMaximumHitTaken", lightning_max);
    env.output.set("ChaosMaximumHitTaken", chaos_max);
    // SecondMinimalMaximumHitTaken — second-smallest of the five (PoB uses this
    // in the defence panel as "next worst max hit").
    let mut hits = [phys_max, fire_max, cold_max, lightning_max, chaos_max];
    hits.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    env.output.set("SecondMinimalMaximumHitTaken", hits[1]);
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
        let c = Character::new(ClassRef::marauder(), 1);
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

    /// End-to-end check that a "while wielding a Shield" item mod is gated on the
    /// equipped state. Mirrors the unique-boots bug from the task brief: with a
    /// shield equipped the buff applies; without one, it doesn't.
    #[test]
    fn while_wielding_shield_mod_is_gated_by_equipped_shield() {
        let Some(tree) = load_3_25_tree() else {
            return;
        };
        // Build a paste for boots whose only modifier mentions a shield.
        let boots_with_shield_mod = "\
Item Class: Boots
Rarity: Rare
Aegis Boots
Iron Greaves
--------
+50 to Armour while wielding a Shield
--------
";
        let shield_paste = "\
Item Class: Shields
Rarity: Magic
Iron Buckler
--------
";
        let boots = crate::item_parser::parse_item(boots_with_shield_mod).unwrap();
        let shield = crate::item_parser::parse_item(shield_paste).unwrap();

        let mut c = Character::new(ClassRef::marauder(), 1);
        // Without a shield: only the boots are equipped.
        c.items.equip(pob_data::Slot::Boots, boots.clone());
        let out_no_shield = compute(&c, &tree);

        // With a shield equipped (in the off-hand slot Weapon2).
        c.items.equip(pob_data::Slot::Weapon2, shield);
        let out_with_shield = compute(&c, &tree);

        // The +50 conditional armour mod must NOT contribute when there's no shield.
        // It MUST contribute the full 50 when a shield is equipped.
        let delta = out_with_shield.get("Armour") - out_no_shield.get("Armour");
        assert!(
            (delta - 50.0).abs() < 1e-6,
            "expected +50 armour from the conditional mod when a shield is wielded; got delta={delta}"
        );
    }

    /// Symmetric check for "while Dual Wielding".
    #[test]
    fn while_dual_wielding_mod_is_gated_by_two_weapons() {
        let Some(tree) = load_3_25_tree() else {
            return;
        };
        let amulet_paste = "\
Item Class: Amulets
Rarity: Rare
Dual Surge
Onyx Amulet
--------
+25 to Strength while Dual Wielding
--------
";
        let weapon1_paste = "\
Item Class: Daggers
Rarity: Magic
Glass Shank
--------
";
        let weapon2_paste = "\
Item Class: Daggers
Rarity: Magic
Skinning Knife
--------
";
        let amulet = crate::item_parser::parse_item(amulet_paste).unwrap();
        let w1 = crate::item_parser::parse_item(weapon1_paste).unwrap();
        let w2 = crate::item_parser::parse_item(weapon2_paste).unwrap();

        let mut c = Character::new(ClassRef::marauder(), 1);
        c.items.equip(pob_data::Slot::Amulet, amulet);
        // Just main hand: not dual wielding yet.
        c.items.equip(pob_data::Slot::Weapon1, w1.clone());
        let out_one_weapon = compute(&c, &tree);

        c.items.equip(pob_data::Slot::Weapon2, w2);
        let out_dual = compute(&c, &tree);

        let delta = out_dual.get("Strength") - out_one_weapon.get("Strength");
        assert!(
            (delta - 25.0).abs() < 1e-6,
            "expected +25 Strength when dual wielding; got delta={delta}"
        );
    }
}
