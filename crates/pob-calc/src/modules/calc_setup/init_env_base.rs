//! @pob-file Modules/PantheonTools.lua @16de4b82d
//! @pob-file Modules/CalcSetup.lua @16de4b82d
//! initEnv segment CS1 (CalcSetup.lua 488-799): env, actors, base mods, bandit/pantheon, config;
//! and Modules/PantheonTools.lua.
//!
//! Owner: CS1.
//!
//! Port notes:
//! * `env = { }` (L493) is `Env::new`: the env struct owns its `player`/`enemy` actors and the
//!   fields Lua assigns at L494-L509 are given to (or created by) the constructor; the
//!   assignments that follow it are made on those fields in Lua order.
//! * `env.player = { modDB = …, level = … }` (L514) / `env.enemy = …` (L519) fill the
//!   constructor's fresh actors (the `Actor.modDB`/`level` fields); back-pointers are `Weak`
//!   (§3.3 rule 4).
//! * `override.addNodes` / `override.removeNodes` are node-keyed sets in Lua
//!   (`pairs(override.addNodes)`, `override.removeNodes[node]`); the port holds them as node-id
//!   maps of the node (`Override.addNodes/removeNodes: LjIntMap<NodeRef>`), so
//!   `override.removeNodes[node]` is "the map holds this very node" (identity, `Rc::ptr_eq`;
//!   CCR-0082 §2).
#![allow(non_snake_case, clippy::explicit_auto_deref)]

use super::InitEnvCtx;
use crate::prelude::*;
use pob_gamedata::{mod_tools, tag};

// @pob Modules/PantheonTools.lua:4-4 h=9f7e4127
// (module-level statements: pantheon)

/// @pob Modules/PantheonTools.lua:6-19 h=7b14b491
/// `pantheon.applySoulMod(db, modParser, god)` (`modParser` is `modLib.parseMod`).
pub fn applySoulMod(db: &ModDB, god: &pob_gamedata::schema::misc::Pantheon) {
    // L7 (`pairs` over a sequence: index order)
    for soul in &god.souls {
        // L8
        for soulMod in &soul.mods {
            // L9
            let (modList, extra) = pob_modparser::parseMod(&soulMod.line, false);
            // L10
            if let (Some(mut modList), None) = (modList, extra) {
                // L11 (`parseMod` returns fresh copies: the source is set on them, §3.6)
                for r#mod in &mut modList {
                    // L12
                    let godName = match god.souls.first() {
                        Some(s) => s.name.clone(),
                        None => lua_error!("attempt to index field '?' (a nil value)"),
                    };
                    // L13
                    r#mod.source = Some(AStr::from(format!("Pantheon:{godName}").as_str()));
                }
                // L15
                let modList: Vec<ModRef> = modList.into_iter().map(Arc::new).collect();
                db.AddList(&modList);
            }
        }
    }
}

/// `data.characterConstants[key]` (every key read here is a field of P1's strict record; a
/// missing one would be nil in Lua and raises here, as in CS0's initModDB).
fn cc(key: &str) -> f64 {
    match data().characterConstants.get(key) {
        Some(v) => v,
        None => lua_error!("data.characterConstants[\"{}\"] is nil", key),
    }
}

/// A Lua scalar as a mod value (`NewMod(name, type, <value>, …)`).
fn mod_value(v: Val) -> Value {
    match v {
        Val::Nil => Value::Nil,
        Val::Bool(b) => Value::Bool(b),
        Val::Num(x) => Value::Num(x),
        Val::Str(s) => Value::Str(AStr::from(&*s)),
        Val::Data(d) => d,
        Val::Mod(m) => Value::Mod(m),
        other => lua_error!("mod value is a {} value", other.lua_type()),
    }
}

/// `modDB:NewMod(name, "BASE", value)` without a source (L777-L790: `createMod` leaves
/// `source` nil when no string follows the value).
fn new_mod_no_source(modDB: &ModDB, name: Name, value: f64) {
    modDB.AddMod(Arc::new(mod_tools::createMod(
        name,
        ModType::BASE,
        value,
        None,
        0,
        0,
        &[],
    )));
}

/// `t[node.name]` as a key: a nil `name` would be a nil index (a Lua error on write, and the
/// arithmetic on the nil read errors first on the remove path).
fn name_key(node: &Node) -> Key {
    match node.extra.get(n!("name")) {
        Val::Str(s) => Key::from(&*s),
        Val::Num(x) => Key::from(x),
        Val::Nil => lua_error!("table index is nil"),
        other => lua_error!("node.name is a {} value", other.lua_type()),
    }
}

/// `node.overrideType` as a key (`Some` when truthy).
fn override_type_key(node: &Node) -> Option<Key> {
    node.overrideType.get().map(Key::S)
}

/// `t[k] + d` with Lua's arithmetic error on a nil/non-number entry.
fn add_num(t: &Table, k: Key, d: f64) -> f64 {
    match t.get(k) {
        Val::Num(x) => x + d,
        other => lua_error!(
            "attempt to perform arithmetic on a {} value",
            other.lua_type()
        ),
    }
}

/// @pob Modules/CalcSetup.lua:488-799 h=9a76817b
/// CS1: CalcSetup.lua 488-799.
#[rustfmt::skip]
#[allow(clippy::too_many_lines, clippy::cognitive_complexity, clippy::needless_late_init)]
pub(crate) fn seg(ctx: &mut InitEnvCtx) -> Flow {
    let build = ctx.build.clone();
    let mode = ctx.mode;
    let r#override = ctx.r#override.clone();
    let accelerate = ctx.accelerate;
    // L487 (`local enemyDB = nil`: declared by the orchestrator's header, used only here)
    let enemyDB: ModDB;
    // L488
    let classStats: Val;

    // L490
    let env: EnvRef = if let Some(env) = ctx.env.clone() {
        // L559
        super::wipeEnv(&env, &accelerate);
        // L560
        ctx.modDB = Some(env.modDB.get());
        // L561
        enemyDB = env.enemyDB.get();
        env
    } else {
        // L493-L499, L502 (`env = { }`, `env.build`, `env.data`, `env.configInput`,
        // `env.configPlaceholder`, `env.calcsInput`, `env.mode`, `env.override`: constructor fields)
        // L501
        let spec = match r#override.spec.get() {
            Some(s) => s,
            None => build.spec.get(),
        };
        let env = Env::new(&build, mode, r#override.clone(), spec);
        // L500
        env.buildBreakdown.set(mode == Mode::MAIN || mode == Mode::CALCS);
        // L503
        env.classId.set(env.spec.get().curClassId.get());

        // L505
        let modDB = ModStore::ModDB(None);
        ctx.modDB = Some(modDB.clone());
        // L506
        env.modDB.set(modDB);
        // L507
        enemyDB = ModStore::ModDB(None);
        // L508
        env.enemyDB.set(enemyDB.clone());
        // L509
        env.itemModDB.set(ModStore::ModDB(None));

        // L511
        let enemyLevel = match build.configTab.enemyLevel.get() {
            Some(l) => l,
            None => m_min(data().misc.MaxEnemyLevel, build.characterLevel.get()),
        };
        env.enemyLevel.set(enemyLevel);

        // Create player/enemy actors
        // L514
        env.player.modDB.set(env.modDB.get());
        env.player.level.set(build.characterLevel.get());
        // L518
        env.modDB.get().actor.set(Rc::downgrade(&env.player));
        // L519
        env.enemy.modDB.set(env.enemyDB.get());
        env.enemy.level.set(env.enemyLevel.get());
        // L523
        enemyDB.actor.set(Rc::downgrade(&env.enemy));
        // L524
        env.player.enemy.set(Rc::downgrade(&env.enemy));
        // L525
        env.enemy.enemy.set(Rc::downgrade(&env.player));
        // L526 (`enemyDB.actor` is `env.enemy`)
        env.enemy.player.set(Rc::downgrade(&env.player));
        // L527 (`env.modDB.actor` is `env.player`)
        env.player.player.set(Rc::downgrade(&env.player));

        // Set up requirements tracking
        // L530
        env.requirementsTableItems.set(Table::new());
        // L531
        env.requirementsTableGems.set(Table::new());

        // Prepare item, skill, flask tables
        // L534 (`wipeTable(nil)` is a fresh `{ }`)
        env.radiusJewelList.set(Shared::new(Vec::new()));
        // L535
        env.extraRadiusNodeList.set(Shared::new(LjIntMap::new()));
        // L536 (`itemList` is the fresh actor's own map: emptying it is the new `{ }`; CCR-0082 §4)
        env.player.itemList.with_mut(indexmap::IndexMap::clear);
        // L537
        env.grantedSkills.set(Shared::new(Vec::new()));
        // L538
        env.grantedSkillsNodes.set(Shared::new(Vec::new()));
        // L539
        env.grantedSkillsItems.set(Shared::new(Vec::new()));
        // L540
        env.explodeSources.set(Shared::new(Vec::new()));
        // L541
        env.itemWarnings.set(Table::new());
        // L542
        env.flasks.set(Shared::new(PtrMap::new()));
        // L543
        env.tinctures.set(Shared::new(PtrMap::new()));

        // tree based
        // L546
        env.grantedPassives.set(Shared::new(LjIntMap::new()));

        // skill-related
        // L549
        env.player.activeSkillList.set(Shared::new(Vec::new()));
        // L550
        env.auxSkillList.set(Shared::new(Vec::new()));
        // L551-L557 (commented-out `elseif accelerate.everything` branch)
        ctx.env = Some(env.clone());
        env
    };
    let modDB = ctx.modDB.clone().expect("modDB is set by L505/L560");

    // Set buff mode
    // L565
    let buffMode: Val;
    // L566
    if mode == Mode::CALCS {
        // L567
        buffMode = env.calcsInput.get(n!("misc_buffMode"));
    } else {
        // L569
        buffMode = Val::Str(LStr::from("EFFECTIVE"));
    }
    let buffMode = buffMode.str();
    // L571
    if buffMode.as_deref() == Some("EFFECTIVE") {
        // L572
        env.mode_buffs.set(true);
        // L573
        env.mode_combat.set(true);
        // L574
        env.mode_effective.set(true);
    } else if buffMode.as_deref() == Some("COMBAT") {
        // L576
        env.mode_buffs.set(true);
        // L577
        env.mode_combat.set(true);
        // L578
        env.mode_effective.set(false);
    } else if buffMode.as_deref() == Some("BUFFED") {
        // L580
        env.mode_buffs.set(true);
        // L581
        env.mode_combat.set(false);
        // L582
        env.mode_effective.set(false);
    } else {
        // L584
        env.mode_buffs.set(false);
        // L585
        env.mode_combat.set(false);
        // L586
        env.mode_effective.set(false);
    }
    // L588
    {
        let tree = env.spec.get().tree.get();
        let classId = env.classId.get();
        let characterData = tree.extra.get(n!("characterData"));
        classStats = lor(land(characterData.clone(), || characterData.get(classId)), || {
            tree.classes.get(classId)
        });
    }

    // L590
    if ctx.cachedPlayerDB.is_none() {
        // Initialise modifier database with base values
        // L592
        for stat in ["Str", "Dex", "Int"] {
            // L593 (indexing a nil classStats raises)
            let v = classStats.get(Name::from(format!("base_{}", pob_lua::string::lower(stat)).as_str()));
            modDB.NewMod(Name::from(stat), ModType::BASE, mod_value(v), "Base", 0, 0, &[]);
        }
        // L595
        modDB.multipliers.set(n!("Level"), m_max(1.0, m_min(100.0, build.characterLevel.get())));
        // L596
        super::initModDB(&env, &modDB);
        // L597
        modDB.NewMod(n!("Life"), ModType::BASE, cc("life_per_level"), "Base", 0, 0, &[tag!({ type = Multiplier, var = "Level", base = 38.0 })]);
        // L598
        modDB.NewMod(n!("Mana"), ModType::BASE, cc("mana_per_level"), "Base", 0, 0, &[tag!({ type = Multiplier, var = "Level", base = 34.0 })]);
        // L599
        modDB.NewMod(n!("ManaRegen"), ModType::BASE, env.data.misc.ManaRegenBase, "Base", 0, 0, &[tag!({ type = PerStat, stat = "Mana", div = 1.0 })]);
        // L600
        modDB.NewMod(n!("Devotion"), ModType::BASE, 0.0, "Base", 0, 0, &[]);
        // L601
        modDB.NewMod(n!("Evasion"), ModType::BASE, cc("base_evasion_rating"), "Base", 0, 0, &[]);
        // L602
        modDB.NewMod(n!("Accuracy"), ModType::BASE, cc("accuracy_rating_per_level"), "Base", 0, 0, &[tag!({ type = Multiplier, var = "Level", base = -cc("accuracy_rating_per_level") })]);
        // L603
        modDB.NewMod(n!("CritMultiplier"), ModType::BASE, cc("base_critical_strike_multiplier") - 100.0, "Base", 0, 0, &[]);
        // L604
        modDB.NewMod(n!("DotMultiplier"), ModType::BASE, cc("critical_ailment_dot_multiplier_+"), "Base", 0, 0, &[tag!({ type = Condition, var = "CriticalStrike" })]);
        // L605
        modDB.NewMod(n!("FireResist"), ModType::BASE, mod_value(lor(env.configInput.get(n!("resistancePenalty")), || Val::Num(-60.0))), "Base", 0, 0, &[]);
        // L606
        modDB.NewMod(n!("ColdResist"), ModType::BASE, mod_value(lor(env.configInput.get(n!("resistancePenalty")), || Val::Num(-60.0))), "Base", 0, 0, &[]);
        // L607
        modDB.NewMod(n!("LightningResist"), ModType::BASE, mod_value(lor(env.configInput.get(n!("resistancePenalty")), || Val::Num(-60.0))), "Base", 0, 0, &[]);
        // L608
        modDB.NewMod(n!("ChaosResist"), ModType::BASE, mod_value(lor(env.configInput.get(n!("resistancePenalty")), || Val::Num(-60.0))), "Base", 0, 0, &[]);
        // L609
        modDB.NewMod(n!("TotemFireResist"), ModType::BASE, 40.0, "Base", 0, 0, &[]);
        // L610
        modDB.NewMod(n!("TotemColdResist"), ModType::BASE, 40.0, "Base", 0, 0, &[]);
        // L611
        modDB.NewMod(n!("TotemLightningResist"), ModType::BASE, 40.0, "Base", 0, 0, &[]);
        // L612
        modDB.NewMod(n!("TotemChaosResist"), ModType::BASE, 20.0, "Base", 0, 0, &[]);
        // L613
        modDB.NewMod(n!("CritChance"), ModType::INC, cc("critical_strike_chance_+%_per_power_charge"), "Base", 0, 0, &[tag!({ type = Multiplier, var = "PowerCharge" })]);
        // L614
        modDB.NewMod(n!("Speed"), ModType::INC, cc("base_attack_speed_+%_per_frenzy_charge"), "Base", ModFlag::Attack, 0, &[tag!({ type = Multiplier, var = "FrenzyCharge" })]);
        // L615
        modDB.NewMod(n!("Speed"), ModType::INC, cc("base_cast_speed_+%_per_frenzy_charge"), "Base", ModFlag::Cast, 0, &[tag!({ type = Multiplier, var = "FrenzyCharge" })]);
        // L616
        modDB.NewMod(n!("Damage"), ModType::MORE, cc("object_inherent_damage_+%_final_per_frenzy_charge"), "Base", 0, 0, &[tag!({ type = Multiplier, var = "FrenzyCharge" })]);
        // L617
        modDB.NewMod(n!("PhysicalDamageReduction"), ModType::BASE, cc("physical_damage_reduction_%_per_endurance_charge"), "Base", 0, 0, &[tag!({ type = Multiplier, var = "EnduranceCharge" })]);
        // L618
        modDB.NewMod(n!("ElementalDamageReduction"), ModType::BASE, cc("elemental_damage_reduction_%_per_endurance_charge"), "Base", 0, 0, &[tag!({ type = Multiplier, var = "EnduranceCharge" })]);
        // L619
        modDB.NewMod(n!("MaximumRage"), ModType::BASE, cc("maximum_rage"), "Base", 0, 0, &[]);
        // L620
        modDB.NewMod(n!("Multiplier:GaleForce"), ModType::BASE, 0.0, "Base", 0, 0, &[]);
        // L621
        modDB.NewMod(n!("MaximumGaleForce"), ModType::BASE, 10.0, "Base", 0, 0, &[]);
        // L622
        modDB.NewMod(n!("MaximumFortification"), ModType::BASE, cc("base_max_fortification"), "Base", 0, 0, &[]);
        // L623
        modDB.NewMod(n!("MaximumValour"), ModType::BASE, 50.0, "Base", 0, 0, &[]);
        // L624
        modDB.NewMod(n!("Multiplier:IntensityLimit"), ModType::BASE, 3.0, "Base", 0, 0, &[]);
        // L625
        modDB.NewMod(n!("Damage"), ModType::INC, cc("damage_+%_per_10_rampage_stacks"), "Base", 0, 0, &[tag!({ type = Multiplier, var = "Rampage", limit = cc("max_rampage_stacks") / 20.0, div = 20.0 })]);
        // L626
        modDB.NewMod(n!("MovementSpeed"), ModType::INC, cc("movement_velocity_+%_per_10_rampage_stacks"), "Base", 0, 0, &[tag!({ type = Multiplier, var = "Rampage", limit = cc("max_rampage_stacks") / 20.0, div = 20.0 })]);
        // L627
        modDB.NewMod(n!("ActiveTrapLimit"), ModType::BASE, cc("base_number_of_traps_allowed"), "Base", 0, 0, &[]);
        // L628
        modDB.NewMod(n!("ActiveMineLimit"), ModType::BASE, cc("base_number_of_remote_mines_allowed"), "Base", 0, 0, &[]);
        // L629
        modDB.NewMod(n!("MineThrowCount"), ModType::BASE, 1.0, "Base", 0, 0, &[]);
        // L630
        modDB.NewMod(n!("TrapThrowCount"), ModType::BASE, 1.0, "Base", 0, 0, &[]);
        // L631
        modDB.NewMod(n!("ActiveBrandLimit"), ModType::BASE, 3.0, "Base", 0, 0, &[]);
        // L632
        modDB.NewMod(n!("EnemyCurseLimit"), ModType::BASE, 1.0, "Base", 0, 0, &[]);
        // L633
        modDB.NewMod(n!("SocketedCursesHexLimitValue"), ModType::BASE, 1.0, "Base", 0, 0, &[]);
        // L634
        modDB.NewMod(n!("ProjectileCount"), ModType::BASE, 1.0, "Base", 0, 0, &[]);
        // L635
        modDB.NewMod(n!("Speed"), ModType::MORE, cc("dual_wield_inherent_attack_speed_+%_final"), "Base", ModFlag::Attack, 0, &[tag!({ type = Condition, var = "DualWielding" }), tag!({ type = Condition, var = "DoubledInherentDualWieldingSpeed", neg = true })]);
        // L636
        modDB.NewMod(n!("Speed"), ModType::MORE, 2.0 * cc("dual_wield_inherent_attack_speed_+%_final"), "Base", ModFlag::Attack, 0, &[tag!({ type = Condition, var = "DualWielding" }), tag!({ type = Condition, var = "DoubledInherentDualWieldingSpeed" })]);
        // L637
        modDB.NewMod(n!("BlockChance"), ModType::BASE, cc("inherent_block_while_dual_wielding_%"), "Base", 0, 0, &[tag!({ type = Condition, var = "DualWielding" }), tag!({ type = Condition, var = "NoInherentBlock", neg = true }), tag!({ type = Condition, var = "DoubledInherentDualWieldingBlock", neg = true })]);
        // L638
        modDB.NewMod(n!("BlockChance"), ModType::BASE, 2.0 * cc("inherent_block_while_dual_wielding_%"), "Base", 0, 0, &[tag!({ type = Condition, var = "DualWielding" }), tag!({ type = Condition, var = "NoInherentBlock", neg = true }), tag!({ type = Condition, var = "DoubledInherentDualWieldingBlock" })]);
        // L639
        modDB.NewMod(n!("Damage"), ModType::MORE, 200.0, "Base", 0, KeywordFlag::Bleed, &[tag!({ type = ActorCondition, actor = "enemy", var = "Moving" }), tag!({ type = Condition, var = "NoExtraBleedDamageToMovingEnemy", neg = true })]);
        // L640
        modDB.NewMod(n!("Condition:BloodStance"), ModType::FLAG, true, "Base", 0, 0, &[tag!({ type = Condition, var = "SandStance", neg = true })]);
        // L641
        modDB.NewMod(n!("Condition:PrideMinEffect"), ModType::FLAG, true, "Base", 0, 0, &[tag!({ type = Condition, var = "PrideMaxEffect", neg = true })]);
        // L642
        modDB.NewMod(n!("PerBrutalTripleDamageChance"), ModType::BASE, cc("chance_to_deal_triple_damage_%_per_brutal_charge"), "Base", 0, 0, &[]);
        // L643
        modDB.NewMod(n!("PerAfflictionAilmentDamage"), ModType::BASE, cc("ailment_damage_+%_final_per_affliction_charge"), "Base", 0, 0, &[]);
        // L644
        modDB.NewMod(n!("PerAfflictionNonDamageEffect"), ModType::BASE, cc("non_damaging_ailment_effect_+%_final_per_affliction_charge"), "Base", 0, 0, &[]);
        // L645
        modDB.NewMod(n!("PerAbsorptionElementalEnergyShieldRecoup"), ModType::BASE, cc("elemental_damage_taken_goes_to_energy_shield_over_4_seconds_%_per_absorption_charge"), "Base", 0, 0, &[]);
        // L646
        modDB.NewMod(n!("TinctureLimit"), ModType::BASE, 1.0, "Base", 0, 0, &[]);
        // L647
        modDB.NewMod(n!("ManaDegenPercentTincture"), ModType::BASE, 1.0, "Base", 0, 0, &[tag!({ type = Multiplier, var = "EffectiveManaBurnStacks" })]);
        // L648
        modDB.NewMod(n!("LifeDegenPercentTincture"), ModType::BASE, 1.0, "Base", 0, 0, &[tag!({ type = Multiplier, var = "WeepingWoundsStacks" })]);
        // L649
        modDB.NewMod(n!("PresenceRadius"), ModType::BASE, cc("base_presence_radius"), "Base", 0, 0, &[]);

        // Add bandit mods
        let bandit = env.configInput.get(n!("bandit")).str();
        // L652
        if bandit.as_deref() == Some("Alira") {
            // L653
            modDB.NewMod(n!("ElementalResist"), ModType::BASE, 15.0, "Bandit", 0, 0, &[]);
        } else if bandit.as_deref() == Some("Kraityn") {
            // L655
            modDB.NewMod(n!("MovementSpeed"), ModType::INC, 8.0, "Bandit", 0, 0, &[]);
        } else if bandit.as_deref() == Some("Oak") {
            // L657
            modDB.NewMod(n!("Life"), ModType::BASE, 40.0, "Bandit", 0, 0, &[]);
        } else {
            // L659
            modDB.NewMod(n!("ExtraPoints"), ModType::BASE, 1.0, "Bandit", 0, 0, &[]);
        }

        // Add Pantheon mods
        // L663 (`local parser = modLib.parseMod`: applySoulMod calls pob_modparser::parseMod)
        // Major Gods
        // L665
        let majorGodName = env.configInput.get(n!("pantheonMajorGod"));
        if majorGodName.str().as_deref() != Some("None") {
            // L666
            let majorGod = pantheon_of(&env, &majorGodName);
            // L667
            applySoulMod(&modDB, &majorGod);
        }
        // Minor Gods
        // L670
        let minorGodName = env.configInput.get(n!("pantheonMinorGod"));
        if minorGodName.str().as_deref() != Some("None") {
            // L671
            let minorGod = pantheon_of(&env, &minorGodName);
            // L672
            applySoulMod(&modDB, &minorGod);
        }

        // Initialise enemy modifier database
        // L676
        super::initModDB(&env, &enemyDB);
        // L677
        let accuracy = monster_accuracy(&env, env.enemyLevel.get());
        enemyDB.NewMod(n!("Accuracy"), ModType::BASE, accuracy, "Base", 0, 0, &[]);
        // L678
        enemyDB.NewMod(n!("Condition:AgainstDamageOverTime"), ModType::FLAG, true, "Base", ModFlag::Dot, 0, &[tag!({ type = ActorCondition, actor = "player", var = "Combat" })]);

        // Add mods from the config tab
        // L681
        env.modDB.get().AddList(&build.configTab.modList.get().ipairs());
        // L682
        env.enemyDB.get().AddList(&build.configTab.enemyModList.get().ipairs());

        // Add mods from the party tab
        // L685
        env.enemyDB.get().AddList(&build.partyTab.enemyModList.get().ipairs());

        // L687
        let (p, e, m) = crate::modules::common::specCopy(&env);
        ctx.cachedPlayerDB = Some(p);
        ctx.cachedEnemyDB = Some(e);
        ctx.cachedMinionDB = m;
    } else {
        // L689
        env.modDB.get().parent.set(ctx.cachedPlayerDB.clone());
        // L690
        env.enemyDB.get().parent.set(ctx.cachedEnemyDB.clone());
        // L691
        if let (Some(cachedMinionDB), Some(minion)) = (ctx.cachedMinionDB.clone(), env.minion.get()) {
            // L692
            minion.modDB.get().parent.set(Some(cachedMinionDB));
        }
    }

    // L696
    if let Some(conditions) = r#override.conditions.get() {
        // L697
        for flag in conditions.snapshot() {
            // L698
            modDB.conditions.set(flag, true);
        }
    }

    let spec = env.spec.get();
    // L702
    let mut allocatedNotableCount = spec.allocatedNotableCount.get();
    // L703
    let mut allocatedKeystoneCount = spec.allocatedKeystoneCount.get();
    // L704
    let mut allocatedMasteryCount = spec.allocatedMasteryCount.get();
    // L705
    let mut allocatedMasteryTypeCount = spec.allocatedMasteryTypeCount.get();
    // L706
    let allocatedMasteryTypes = spec.allocatedMasteryTypes.copyTable(false);
    // L707
    let allocatedTattooTypes = spec.allocatedTattooTypes.copyTable(false);

    // L711
    if !accelerate.nodeAlloc {
        // Build list of passive nodes
        // L713
        let nodes: Shared<LjIntMap<NodeRef>>;
        let addNodes = r#override.addNodes.get();
        let removeNodes = r#override.removeNodes.get();
        // L714
        if addNodes.is_some() || removeNodes.is_some() {
            // L715
            nodes = Shared::new(LjIntMap::new());
            // L716
            if let Some(addNodes) = &addNodes {
                // L717 LUA-ORDER: Lua iterates a node-keyed set (address order); the port the
                // id map's LuaJIT order
                for (_, node) in addNodes.with(LjIntMap::pairs) {
                    // L718
                    nodes.with_mut(|m| m.set(node.id as i64, Some(node.clone())));
                    let ty = node.r#type.get();
                    // L719
                    if ty == n!("Mastery") {
                        // L720
                        allocatedMasteryCount += 1.0;

                        // L722
                        let k = name_key(&node);
                        if !allocatedMasteryTypes.truthy(k) {
                            // L723
                            allocatedMasteryTypes.set(k, 1.0);
                            // L724
                            allocatedMasteryTypeCount += 1.0;
                        } else {
                            // L726
                            let prevCount = allocatedMasteryTypes.get(k);
                            // L727
                            allocatedMasteryTypes.set(k, add_num(&allocatedMasteryTypes, k, 1.0));
                            // L728
                            if prevCount.opt_num() == Some(0.0) {
                                // L729
                                allocatedMasteryTypeCount += 1.0;
                            }
                        }
                    } else if ty == n!("Notable") {
                        // L733
                        allocatedNotableCount += 1.0;
                    } else if ty == n!("Keystone") {
                        // L735
                        allocatedKeystoneCount += 1.0;
                    }
                    // L737
                    if node.isTattoo.get() {
                        if let Some(k) = override_type_key(&node) {
                            // L738
                            if !allocatedTattooTypes.truthy(k) {
                                // L739
                                allocatedTattooTypes.set(k, 1.0);
                            } else {
                                // L741-L742
                                allocatedTattooTypes.set(k, add_num(&allocatedTattooTypes, k, 1.0));
                            }
                        }
                    }
                }
            }
            // L747 (`spec.allocNodes` is a node-id map: LuaJIT order)
            for (_, node) in spec.allocNodes.with(LjIntMap::pairs) {
                let removed = removeNodes.as_ref().is_some_and(|r| {
                    r.with(|m| m.get(node.id as i64)).is_some_and(|n| Rc::ptr_eq(&n, &node))
                });
                // L748
                if !removed {
                    // L749
                    nodes.with_mut(|m| m.set(node.id as i64, Some(node.clone())));
                } else {
                    // L750 (`elseif override.removeNodes[node]`: always true here)
                    let ty = node.r#type.get();
                    // L751
                    if ty == n!("Mastery") {
                        // L752
                        allocatedMasteryCount -= 1.0;

                        // L754
                        let k = name_key(&node);
                        allocatedMasteryTypes.set(k, add_num(&allocatedMasteryTypes, k, -1.0));
                        // L755
                        if allocatedMasteryTypes.get(k).opt_num() == Some(0.0) {
                            // L756
                            allocatedMasteryTypeCount -= 1.0;
                        }
                    } else if ty == n!("Notable") {
                        // L759
                        allocatedNotableCount -= 1.0;
                    } else if ty == n!("Keystone") {
                        // L761
                        allocatedKeystoneCount -= 1.0;
                    }
                    // L763
                    if node.isTattoo.get() {
                        if let Some(k) = override_type_key(&node) {
                            // L764
                            if allocatedTattooTypes.truthy(k) {
                                // L765
                                allocatedTattooTypes.set(k, add_num(&allocatedTattooTypes, k, -1.0));
                            }
                        }
                    }
                }
            }
        } else {
            // L771
            nodes = Shared::new(spec.allocNodes.with(LjIntMap::copyTable));
        }
        // L773
        env.allocNodes.set(nodes);
        // L774 (the second return, explodeSources, is dropped)
        let (initialNodeModDB, _) = super::buildModListForNodeList(&env, &env.allocNodes.get(), true);
        env.initialNodeModDB.set(Some(initialNodeModDB));
        // L775
        let initialNodeModDB = env.initialNodeModDB.get().expect("set at L774");
        crate::modules::mod_tools_env::mergeKeystones(&env, &initialNodeModDB);
    }

    // L778
    if allocatedNotableCount > 0.0 {
        // L779
        new_mod_no_source(&modDB, n!("Multiplier:AllocatedNotable"), allocatedNotableCount);
    }
    // L781
    if allocatedKeystoneCount > 0.0 {
        // L782
        new_mod_no_source(&modDB, n!("Multiplier:AllocatedKeystone"), allocatedKeystoneCount);
    }
    // L784
    if allocatedMasteryCount > 0.0 {
        // L785
        new_mod_no_source(&modDB, n!("Multiplier:AllocatedMastery"), allocatedMasteryCount);
    }
    // L787
    if allocatedMasteryTypeCount > 0.0 {
        // L788
        new_mod_no_source(&modDB, n!("Multiplier:AllocatedMasteryType"), allocatedMasteryTypeCount);
    }
    // L790
    let lifeMastery = allocatedMasteryTypes.get(n!("Life Mastery"));
    if lifeMastery.truthy() && gt0(&lifeMastery) {
        // L791
        modDB.AddMod(Arc::new(mod_tools::createMod(n!("Multiplier:AllocatedLifeMastery"), ModType::BASE, mod_value(lifeMastery), None, 0, 0, &[])));
    }
    // L793 (`copyTable` always returns a table)
    {
        // L794 LUA-ORDER: string keys, insertion order (PoB: hash order; only the table layout
        // of `multipliers` depends on it)
        for (r#type, count) in allocatedTattooTypes.pairs() {
            // L795
            env.modDB.get().multipliers.set(r#type, count);
        }
    }
    Flow::Normal
}

/// `x > 0` with Lua's comparison error on a non-number.
fn gt0(v: &Val) -> bool {
    match v {
        Val::Num(x) => *x > 0.0,
        other => lua_error!("attempt to compare number with {}", other.lua_type()),
    }
}

/// `env.data.pantheons[name]` (nil → the `god.souls` index error of applySoulMod).
fn pantheon_of(env: &EnvRef, name: &Val) -> pob_gamedata::schema::misc::Pantheon {
    let god = match name {
        Val::Str(s) => env.data.pantheons.get(&Name::from(&**s)).cloned(),
        _ => None,
    };
    match god {
        Some(g) => g,
        // PantheonTools.lua:7 `pairs(god.souls)` on nil
        None => lua_error!("attempt to index local 'god' (a nil value)"),
    }
}

/// `env.data.monsterAccuracyTable[level]` (a 1-based sequence; any other key is nil).
fn monster_accuracy(env: &EnvRef, level: f64) -> Value {
    let t = &env.data.monsterAccuracyTable;
    if level.fract() == 0.0 && level >= 1.0 && level <= t.len() as f64 {
        Value::Num(t[level as usize - 1])
    } else {
        Value::Nil
    }
}
