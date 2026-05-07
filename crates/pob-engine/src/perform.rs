//! Phase 2 calc pipeline. Builds an `Env` from a `Character` + `PassiveTree`, then runs
//! a basic-stats pass.
//!
//! Mirrors `Modules/CalcSetup.lua` (env construction) + a tiny slice of
//! `Modules/CalcPerform.lua` (basic life/mana/attribute computation).

use pob_data::PassiveTree;

use crate::character::Character;
use crate::env::{Env, Output};
use crate::mod_db::{ModStore, QueryCfg};
use crate::mod_parser::parse_mod_line;
use crate::modifier::{Mod, ModType, Source};

/// Top-level entry point — equivalent to PoB's `calcs.buildOutput(build, "MAIN")` for the
/// minimal scope of Phase 2. Returns the populated `Output`.
pub fn compute(character: &Character, tree: &PassiveTree) -> Output {
    let mut env = init_env(character, tree);
    perform_basic_stats(character, tree, &mut env);
    env.output
}

/// Construct the env: gather class base attributes, parse and add tree node mods,
/// add level mods.
pub fn init_env(character: &Character, tree: &PassiveTree) -> Env {
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

    // 3. Tree node stats. Parse each allocated node's stat lines.
    for node_id in &character.allocated {
        let Some(node) = tree.nodes.get(node_id) else {
            continue;
        };
        for line in &node.stats {
            if let Some(parsed) = parse_mod_line(line) {
                env.mod_db
                    .add(parsed.mod_.with_source(Source::Passive(*node_id)));
            }
        }
    }

    // 4. Items.
    let _ = crate::item_parser::apply_item_set(&character.items, &mut env.mod_db);

    env
}

fn perform_basic_stats(_character: &Character, _tree: &PassiveTree, env: &mut Env) {
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

    // Resistances: each starts at -60 unmodified for level 68+, but the base value the
    // calc engine reports is the mod sum; the cap is enforced separately. Phase 2: just
    // sum the BASE mods.
    for elem in ["Fire", "Cold", "Lightning"] {
        let key = format!("{elem}Resist");
        let total = env.mod_db.sum(ModType::Base, &cfg, &env.state, &key)
            + env.mod_db.sum(ModType::Base, &cfg, &env.state, "ElementalResist");
        env.output.set(&key, total);
    }
    let chaos = env.mod_db.sum(ModType::Base, &cfg, &env.state, "ChaosResist");
    env.output.set("ChaosResist", chaos);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::character::ClassRef;
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
        // Find any plain "+10 to Strength" notable/normal node.
        let str_node = tree
            .nodes
            .iter()
            .find(|(_, n)| {
                n.stats.iter().any(|s| s == "+10 to Strength") && n.stats.len() == 1
            })
            .map(|(id, _)| *id);
        let Some(node_id) = str_node else {
            eprintln!("no '+10 to Strength' node in tree — skip");
            return;
        };

        let mut c = Character::new(ClassRef::marauder(), 1);
        let baseline = compute(&c, &tree);
        c.allocate(node_id);
        let after = compute(&c, &tree);

        assert_eq!(after.get("Strength") - baseline.get("Strength"), 10.0);
        // +10 Str adds +5 max life.
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
