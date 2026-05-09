//! Parallel minion calc env — slice 3 of [#20](https://github.com/jonatanferm/PathOfBuildingMK2/issues/20).
//!
//! When the active main skill is a minion-summoning gem (Raise Zombie, Summon Skeletons,
//! Summon Flame Golem, …), PoB constructs a `minion` sub-env: it picks the first entry
//! in the gem's `minionList`, looks it up in `Data/Minions.lua`, and runs a parallel
//! perform pass against that creature's base stats.
//!
//! This slice covers the **detection + basic life output** end of that pipeline. We:
//!   1. detect a minion-summoning skill via [`select_minion_type`],
//!   2. build a [`MinionState`] from the matched [`pob_data::MinionType`],
//!   3. emit `Minion.Life` (and the Minion-side intermediate `MinionLifeBase`) into the
//!      player's output dictionary so the Calcs tab can surface it.
//!
//! Mods that scale minion life (`MinionLife` INC / MORE, `MinionLifeRegen`, etc.) are
//! deferred — slice 4 will run a real perform pass on the minion's `ModDB`.

use pob_data::{
    monster_tables::{
        monster_ally_damage_at_level, monster_ally_life_at_level, monster_life2_at_level,
        monster_life3_at_level, monster_life_at_level,
    },
    MinionData, MinionType,
};

use crate::{mod_db::QueryCfg, Character, Env, ModStore, ModType, Output, SkillRegistry};

/// Snapshot of the minion summoned by the active main skill. Slice 3 only carries the
/// scalars the Calcs tab needs — slice 4 will add `mod_db`, an `output` map, and a
/// per-minion perform pass.
#[derive(Debug, Clone)]
pub struct MinionState<'a> {
    /// PoB's internal id (e.g. `"RaisedZombie"`, `"SummonedFlameGolem"`).
    pub id: String,
    /// Backing minion definition (lifetime tied to the loaded `MinionData`).
    pub data: &'a MinionType,
    /// Per-area level the minion is summoned at. Mirrors PoB's
    /// `actorLevel = level.levelRequirement` fallback for the gem level.
    pub level: u32,
    /// Pre-mods life value: `monster_*_life_table[level] × minion.life`. The player's
    /// `MinionLife` INC / MORE mods scale this in slice 4's perform pass.
    pub life_base: u32,
}

impl MinionState<'_> {
    /// `Player.character.level`-driven life (no minion-specific mods applied yet).
    /// Equivalent to PoB's `output.Life = base × (1 + inc/100) × more` *before* the
    /// `(1 + inc/100) × more` step.
    #[must_use]
    pub fn base_life_only(&self) -> u32 {
        self.life_base
    }
}

/// Detect the minion type the active main skill summons. Returns `None` if the active
/// skill isn't a minion-summoning skill, or its `minionList[0]` isn't present in the
/// catalogue.
///
/// Slice 3 picks `minionList[0]` (PoB's default for the gem's primary minion). General's
/// Cry alts and Animate Guardian / Animate Weapon are deferred — they pick a non-trivial
/// secondary minion.
#[must_use]
pub fn select_minion_type<'a>(
    character: &Character,
    registry: &SkillRegistry,
    minions: &'a MinionData,
) -> Option<MinionState<'a>> {
    let main = character.main_skill.as_ref()?;
    let skill = registry.get(&main.skill_id)?;
    let primary = skill.minion_list.first()?;
    let data = minions.minions.get(primary)?;

    let level = main.level.max(1).min(100);
    let life_base = (life_base_for(data, level) as f64 * data.life).round() as u32;

    Some(MinionState {
        id: primary.clone(),
        data,
        level,
        life_base,
    })
}

/// Pick the right monster-life table for a given minion. Mirrors PoB's
/// `Modules/CalcActiveSkill.lua:697-699` ladder.
fn life_base_for(_data: &MinionType, level: u32) -> u32 {
    // PoB's MinionType currently doesn't expose `lifeScaling`; use the ally table for
    // every "normal" minion. Spectres (lifeScaling unset) fall back to monsterLifeTable
    // — but MK2 doesn't yet distinguish spectre selection from regular minion summons.
    // The placeholder helpers stay wired so a future slice can switch on the field.
    let _ = monster_life_at_level;
    let _ = monster_life2_at_level;
    let _ = monster_life3_at_level;
    monster_ally_life_at_level(level)
}

/// One-shot helper: detect the active minion (if any) and write its outputs into the
/// player's output dictionary. Returns `true` when a minion was found and outputs were
/// written. Safe to call when no minion data is loaded — it just returns `false`.
///
/// Designed for the UI to call after `compute_full_with_env`, since the existing
/// pipeline doesn't yet take a `MinionData` parameter and threading one through every
/// caller would churn ~30 test sites.
///
/// Slice 4 of [#20](https://github.com/jonatanferm/PathOfBuildingMK2/issues/20) takes
/// the live `Env` so it can scale `MinionLife` by the player-side `MinionLife` INC /
/// MORE mods (jewel passives, ascendancy notables, support gems). Slice 5 will route
/// the minion's intrinsic mod_list onto a parallel mod_db and add `MinionDPS`.
pub fn apply_minion_outputs(
    character: &Character,
    registry: &SkillRegistry,
    minions: &MinionData,
    env: &Env,
    output: &mut Output,
) -> bool {
    let Some(state) = select_minion_type(character, registry, minions) else {
        return false;
    };
    write_minion_outputs(&state, env, output);
    true
}

/// Emit the minion's basic stats into the player's output dictionary so the Calcs tab
/// can surface them.
///
/// Cumulative coverage as of slice 6:
/// - **Life** — `MinionLife` INC / MORE (slice 4).
/// - **Damage** — `MinionDamage` INC / MORE × `damage_spread` (slice 5).
/// - **Attack rate** — `MinionAttackSpeed` INC / MORE (slice 5).
/// - **Resists** — player-side BASE adders for `MinionFireResist` /
///   `MinionColdResist` / `MinionLightningResist` / `MinionChaosResist`, plus the
///   `MinionElementalResist` umbrella that scales fire/cold/lightning together.
///   Each resist is capped at 75% (the PoE default; minion max-resist mods land in
///   slice 7 if they ever surface).
/// - **Crit** — base 5% × `MinionCritChance` INC, with a 150% base
///   `MinionCritMultiplier` and BASE adders folded in. Crit factor multiplies
///   `MinionDPS`.
///
/// What's still **not** modelled:
/// - Hit-chance vs enemy evasion for melee minions.
/// - Minion-side armour, evasion, energy shield.
/// - Minion's intrinsic `mod_list` (slice 5 keeps it inert; the perform pass needs it).
/// - Per-minion `lifeScaling` (spectres etc.) — every minion still uses the ally
///   life table.
pub fn write_minion_outputs(state: &MinionState<'_>, env: &Env, output: &mut Output) {
    let cfg = QueryCfg::default();

    // Life — same pattern as slice 4.
    let life_inc = env.mod_db.sum(ModType::Inc, &cfg, &env.state, "MinionLife");
    let life_more = env.mod_db.more(&cfg, &env.state, "MinionLife");
    let life_scaled = (state.life_base as f64) * (1.0 + life_inc / 100.0) * life_more;
    output.set("MinionLifeBase", state.life_base as f64);
    output.set("MinionLife", life_scaled.round());

    // Resists. PoB minion resists are layered:
    //   resist = base + MinionFireResist BASE + MinionElementalResist BASE,
    //   capped at the resist max (default 75%). Chaos resist doesn't get the
    //   elemental-umbrella adder.
    let elemental_base = env
        .mod_db
        .sum(ModType::Base, &cfg, &env.state, "MinionElementalResist");
    let fire_base = env
        .mod_db
        .sum(ModType::Base, &cfg, &env.state, "MinionFireResist");
    let cold_base = env
        .mod_db
        .sum(ModType::Base, &cfg, &env.state, "MinionColdResist");
    let lightning_base = env
        .mod_db
        .sum(ModType::Base, &cfg, &env.state, "MinionLightningResist");
    let chaos_base = env
        .mod_db
        .sum(ModType::Base, &cfg, &env.state, "MinionChaosResist");
    const MINION_RESIST_CAP: f64 = 75.0;
    let fire =
        (f64::from(state.data.fire_resist) + fire_base + elemental_base).min(MINION_RESIST_CAP);
    let cold =
        (f64::from(state.data.cold_resist) + cold_base + elemental_base).min(MINION_RESIST_CAP);
    let lightning = (f64::from(state.data.lightning_resist) + lightning_base + elemental_base)
        .min(MINION_RESIST_CAP);
    let chaos = (f64::from(state.data.chaos_resist) + chaos_base).min(MINION_RESIST_CAP);
    output.set("MinionFireResistBase", state.data.fire_resist as f64);
    output.set("MinionColdResistBase", state.data.cold_resist as f64);
    output.set(
        "MinionLightningResistBase",
        state.data.lightning_resist as f64,
    );
    output.set("MinionChaosResistBase", state.data.chaos_resist as f64);
    output.set("MinionFireResist", fire);
    output.set("MinionColdResist", cold);
    output.set("MinionLightningResist", lightning);
    output.set("MinionChaosResist", chaos);

    // Damage — `monster_ally_damage[level] × minion.damage × (1 + inc/100) × more`.
    // The `damage_spread` field captures the per-hit damage variance (PoB uses ±20%
    // for most minion types); we expose Min / Max / Average so consumers can pick
    // the value that matches what they're computing.
    let damage_base = f64::from(monster_ally_damage_at_level(state.level)) * state.data.damage;
    let dmg_inc = env
        .mod_db
        .sum(ModType::Inc, &cfg, &env.state, "MinionDamage");
    let dmg_more = env.mod_db.more(&cfg, &env.state, "MinionDamage");
    let damage_scaled = damage_base * (1.0 + dmg_inc / 100.0) * dmg_more;
    let spread = state.data.damage_spread;
    let dmg_min = damage_scaled * (1.0 - spread);
    let dmg_max = damage_scaled * (1.0 + spread);
    let dmg_avg = damage_scaled;
    output.set("MinionDamageBase", damage_base);
    output.set("MinionAverageDamage", dmg_avg);
    output.set("MinionMinDamage", dmg_min);
    output.set("MinionMaxDamage", dmg_max);

    // Attack rate — `1 / attack_time × (1 + inc/100) × more`.
    let attack_time = state.data.attack_time.max(0.001);
    let speed_base = 1.0 / attack_time;
    let spd_inc = env
        .mod_db
        .sum(ModType::Inc, &cfg, &env.state, "MinionAttackSpeed");
    let spd_more = env.mod_db.more(&cfg, &env.state, "MinionAttackSpeed");
    let attacks_per_second = speed_base * (1.0 + spd_inc / 100.0) * spd_more;
    output.set("MinionAttacksPerSecondBase", speed_base);
    output.set("MinionAttacksPerSecond", attacks_per_second);

    // Crit. PoB minions start with a 5% crit chance and the player-side
    // `MinionCritChance` INC / BASE chain, with a 150% base multiplier scaled by
    // `MinionCritMultiplier` BASE. Cap chance at 100%.
    const MINION_BASE_CRIT_CHANCE: f64 = 5.0;
    const MINION_BASE_CRIT_MULT: f64 = 150.0;
    let crit_base = env
        .mod_db
        .sum(ModType::Base, &cfg, &env.state, "MinionCritChance");
    let crit_inc = env
        .mod_db
        .sum(ModType::Inc, &cfg, &env.state, "MinionCritChance");
    let crit_chance =
        ((MINION_BASE_CRIT_CHANCE + crit_base) * (1.0 + crit_inc / 100.0)).clamp(0.0, 100.0);
    let crit_mult_add = env
        .mod_db
        .sum(ModType::Base, &cfg, &env.state, "MinionCritMultiplier");
    let crit_mult = MINION_BASE_CRIT_MULT + crit_mult_add;
    let crit_factor = (1.0 - crit_chance / 100.0) + (crit_chance / 100.0) * (crit_mult / 100.0);
    output.set("MinionCritChance", crit_chance);
    output.set("MinionCritMultiplier", crit_mult);

    // Final DPS: average per-hit × rate × crit factor.
    output.set("MinionDPS", dmg_avg * attacks_per_second * crit_factor);
}

#[cfg(test)]
mod tests {
    use super::*;
    use indexmap::IndexMap;
    use pob_data::{minions::MinionType, MinionData};

    fn fake_minion() -> MinionData {
        let mut minions = IndexMap::new();
        minions.insert(
            "SummonedFlameGolem".into(),
            MinionType {
                name: "Flame Golem".into(),
                monster_tags: vec![],
                life: 6.0,
                energy_shield: None,
                armour: None,
                fire_resist: 75,
                cold_resist: 40,
                lightning_resist: 40,
                chaos_resist: 0,
                damage: 1.5,
                damage_spread: 0.2,
                attack_time: 1.0,
                attack_range: 8.0,
                accuracy: 3.4,
                limit: Some("ActiveGolemLimit".into()),
                skill_list: vec![],
                mod_list: vec![],
            },
        );
        MinionData { minions }
    }

    #[test]
    fn select_minion_type_returns_none_without_main_skill() {
        use crate::character::ClassRef;
        let c = Character::new(ClassRef::marauder(), 90);
        let reg = SkillRegistry::default();
        let minions = fake_minion();
        assert!(select_minion_type(&c, &reg, &minions).is_none());

        // apply_minion_outputs short-circuits to false in the same case.
        let env = Env::default();
        let mut output = Output::default();
        let applied = apply_minion_outputs(&c, &reg, &minions, &env, &mut output);
        assert!(!applied);
    }

    #[test]
    fn write_minion_outputs_emits_life_and_resists() {
        let minions = fake_minion();
        let data = minions.minions.get("SummonedFlameGolem").unwrap();
        let state = MinionState {
            id: "SummonedFlameGolem".into(),
            data,
            level: 20,
            life_base: 1000,
        };
        let env = Env::default();
        let mut output = Output::default();
        write_minion_outputs(&state, &env, &mut output);
        // No mods → MinionLife == MinionLifeBase.
        assert_eq!(output.get("MinionLife"), 1000.0);
        assert_eq!(output.get("MinionLifeBase"), 1000.0);
        assert_eq!(output.get("MinionFireResist"), 75.0);
        assert_eq!(output.get("MinionChaosResist"), 0.0);
    }

    #[test]
    fn write_minion_outputs_scales_life_by_inc_and_more() {
        use crate::{Mod, ModDB};
        let minions = fake_minion();
        let data = minions.minions.get("SummonedFlameGolem").unwrap();
        let state = MinionState {
            id: "SummonedFlameGolem".into(),
            data,
            level: 20,
            life_base: 1000,
        };

        // 50% inc + 20% more → 1000 × 1.5 × 1.2 = 1800.
        let mut env = Env::default();
        env.mod_db.add(Mod::inc("MinionLife", 50.0));
        env.mod_db.add(Mod::more("MinionLife", 20.0));
        let _ = ModDB::new; // suppress unused-import lint when feature gated

        let mut output = Output::default();
        write_minion_outputs(&state, &env, &mut output);
        assert_eq!(output.get("MinionLifeBase"), 1000.0);
        assert_eq!(output.get("MinionLife"), 1800.0);

        // Multiple INCs add together, multiple MOREs multiply.
        let mut env2 = Env::default();
        env2.mod_db.add(Mod::inc("MinionLife", 30.0));
        env2.mod_db.add(Mod::inc("MinionLife", 70.0));
        env2.mod_db.add(Mod::more("MinionLife", 50.0));
        env2.mod_db.add(Mod::more("MinionLife", 50.0));
        // 1000 × (1 + 1.0) × (1.5 × 1.5) = 1000 × 2 × 2.25 = 4500.
        let mut out2 = Output::default();
        write_minion_outputs(&state, &env2, &mut out2);
        assert_eq!(out2.get("MinionLife"), 4500.0);
    }

    #[test]
    fn write_minion_outputs_emits_damage_and_dps() {
        // Flame Golem at level 20 with no mods. Pinned values come from
        // monster_ally_damage_at_level(20) × minion.damage × (1 ± spread).
        // From monster_tables tests: monster_ally_damage[20] = 19.46. Times
        // damage = 1.5 → 29.19 average. spread = 0.2 → ±20%.
        let minions = fake_minion();
        let data = minions.minions.get("SummonedFlameGolem").unwrap();
        let state = MinionState {
            id: "SummonedFlameGolem".into(),
            data,
            level: 20,
            life_base: 1000,
        };
        let env = Env::default();
        let mut output = Output::default();
        write_minion_outputs(&state, &env, &mut output);

        let avg = output.get("MinionAverageDamage");
        assert!((avg - 29.19).abs() < 0.5, "MinionAverageDamage = {avg}");
        // ±20% spread.
        let dmg_min = output.get("MinionMinDamage");
        let dmg_max = output.get("MinionMaxDamage");
        assert!((dmg_min - avg * 0.8).abs() < 0.001);
        assert!((dmg_max - avg * 1.2).abs() < 0.001);

        // attack_time = 1.0s → 1 attack/sec base.
        assert_eq!(output.get("MinionAttacksPerSecondBase"), 1.0);
        assert_eq!(output.get("MinionAttacksPerSecond"), 1.0);

        // Crit baseline: 5% chance × 150% multiplier → factor = 0.95 + 0.05 × 1.5 = 1.025.
        assert_eq!(output.get("MinionCritChance"), 5.0);
        assert_eq!(output.get("MinionCritMultiplier"), 150.0);

        // DPS = avg × rate × crit_factor.
        let dps = output.get("MinionDPS");
        assert!((dps - avg * 1.025).abs() < 0.001, "MinionDPS = {dps}");
    }

    #[test]
    fn write_minion_outputs_scales_damage_and_speed_by_player_mods() {
        use crate::Mod;
        let minions = fake_minion();
        let data = minions.minions.get("SummonedFlameGolem").unwrap();
        let state = MinionState {
            id: "SummonedFlameGolem".into(),
            data,
            level: 20,
            life_base: 1000,
        };

        let mut env = Env::default();
        env.mod_db.add(Mod::inc("MinionDamage", 100.0)); // 2× damage
        env.mod_db.add(Mod::inc("MinionAttackSpeed", 50.0)); // 1.5× rate
        let mut output = Output::default();
        write_minion_outputs(&state, &env, &mut output);

        // Base damage avg from previous test was ~29.19. With 100% INC → ~58.38.
        let avg = output.get("MinionAverageDamage");
        assert!((avg - 58.38).abs() < 1.0, "MinionAverageDamage = {avg}");
        // Rate: 1.0 × 1.5 = 1.5.
        assert_eq!(output.get("MinionAttacksPerSecond"), 1.5);
        // DPS = 58.38 × 1.5 × 1.025 (baseline crit factor) = ~89.76.
        let dps = output.get("MinionDPS");
        assert!((dps - 89.76).abs() < 1.5, "MinionDPS = {dps}");
    }

    #[test]
    fn write_minion_outputs_layers_resist_adders_with_cap() {
        use crate::Mod;
        let minions = fake_minion();
        let data = minions.minions.get("SummonedFlameGolem").unwrap();
        // Drop the fake minion to a more realistic baseline so we can see the cap take
        // effect (Flame Golem starts at fire 75 already).
        let mut data = data.clone();
        data.fire_resist = 30;
        data.cold_resist = 40;
        data.lightning_resist = 40;
        data.chaos_resist = 0;
        let state = MinionState {
            id: "Custom".into(),
            data: &data,
            level: 20,
            life_base: 1000,
        };

        let mut env = Env::default();
        // +20% to all elemental resists (raises each by 20).
        env.mod_db.add(Mod::base("MinionElementalResist", 20.0));
        // +10% to fire only on top of that.
        env.mod_db.add(Mod::base("MinionFireResist", 10.0));
        // +50% chaos resist — should land at 50, not capped.
        env.mod_db.add(Mod::base("MinionChaosResist", 50.0));
        let mut output = Output::default();
        write_minion_outputs(&state, &env, &mut output);

        // fire = 30 + 10 + 20 = 60.
        assert_eq!(output.get("MinionFireResist"), 60.0);
        // cold = 40 + 0 + 20 = 60.
        assert_eq!(output.get("MinionColdResist"), 60.0);
        // lightning = 40 + 0 + 20 = 60.
        assert_eq!(output.get("MinionLightningResist"), 60.0);
        // chaos = 0 + 50 = 50 (no elemental adder).
        assert_eq!(output.get("MinionChaosResist"), 50.0);
        // Base values are preserved separately.
        assert_eq!(output.get("MinionFireResistBase"), 30.0);
        assert_eq!(output.get("MinionChaosResistBase"), 0.0);
    }

    #[test]
    fn write_minion_outputs_resist_caps_at_75() {
        use crate::Mod;
        let minions = fake_minion();
        let data = minions.minions.get("SummonedFlameGolem").unwrap();
        let state = MinionState {
            id: "SummonedFlameGolem".into(),
            data,
            level: 20,
            life_base: 1000,
        };

        let mut env = Env::default();
        // Massive over-cap — sum should cap at 75.
        env.mod_db.add(Mod::base("MinionElementalResist", 100.0));
        env.mod_db.add(Mod::base("MinionFireResist", 50.0));
        let mut output = Output::default();
        write_minion_outputs(&state, &env, &mut output);
        // fire would be 75 + 100 + 50 = 225 → capped at 75.
        assert_eq!(output.get("MinionFireResist"), 75.0);
    }

    #[test]
    fn write_minion_outputs_crit_chance_and_multiplier_scale() {
        use crate::Mod;
        let minions = fake_minion();
        let data = minions.minions.get("SummonedFlameGolem").unwrap();
        let state = MinionState {
            id: "SummonedFlameGolem".into(),
            data,
            level: 20,
            life_base: 1000,
        };

        let mut env = Env::default();
        env.mod_db.add(Mod::inc("MinionCritChance", 200.0)); // 5% × 3 = 15%
        env.mod_db.add(Mod::base("MinionCritMultiplier", 100.0)); // 150 + 100 = 250%
        let mut output = Output::default();
        write_minion_outputs(&state, &env, &mut output);
        assert_eq!(output.get("MinionCritChance"), 15.0);
        assert_eq!(output.get("MinionCritMultiplier"), 250.0);
        // Crit factor = 0.85 + 0.15 × 2.5 = 0.85 + 0.375 = 1.225.
        // Average damage at level 20 ≈ 29.19; rate = 1.0; DPS = 29.19 × 1.225 ≈ 35.76.
        let dps = output.get("MinionDPS");
        assert!((dps - 35.76).abs() < 1.0, "MinionDPS = {dps}");
    }

    #[test]
    fn life_base_uses_ally_life_table_at_clamped_level() {
        // Level 90 ally life is 4178 (pinned in monster_tables tests).
        let minions = fake_minion();
        let data = minions.minions.get("SummonedFlameGolem").unwrap();
        // life multiplier is 6.0, so 4178 * 6 = 25068.
        let life = (life_base_for(data, 90) as f64 * data.life).round() as u32;
        assert_eq!(life, 25068);

        // Out-of-range gem level clamps to [1, 100].
        let life_clamped_high = (life_base_for(data, 250) as f64 * data.life).round() as u32;
        assert_eq!(life_clamped_high, (6916.0_f64 * 6.0).round() as u32);
        let life_clamped_low = (life_base_for(data, 0) as f64 * data.life).round() as u32;
        assert_eq!(life_clamped_low, (15.0_f64 * 6.0).round() as u32);
    }
}
