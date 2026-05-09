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
        monster_accuracy_at_level, monster_ally_damage_at_level, monster_ally_life_at_level,
        monster_life2_at_level, monster_life3_at_level, monster_life_at_level,
    },
    MinionData, MinionType,
};

use crate::{
    mod_db::QueryCfg, skill::parse_extractor_mod, Character, Env, ModDB, ModStore, ModType, Output,
    SkillRegistry,
};

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

/// Parse the minion's intrinsic `mod_list` recordings into a fresh `ModDB` keyed by
/// the minion-side stat names PoB uses (`Life` / `Armour` / `StunThreshold` / etc.).
/// Returns an empty `ModDB` when the minion has no intrinsic mods.
///
/// PoB's CalcActiveSkill.lua:697-790 builds `env.minion.modDB` from the same recordings
/// and queries it with the minion as the active actor; we mirror that locally so the
/// player-side ModDB stays untouched.
#[must_use]
pub fn parse_minion_intrinsic_mods(data: &MinionType) -> ModDB {
    let mut db = ModDB::new();
    for entry in &data.mod_list {
        let Some(value) = entry.get("value").and_then(serde_json::Value::as_f64) else {
            continue;
        };
        if let Some(m) = parse_extractor_mod(entry, value) {
            db.add(m);
        }
    }
    db
}

/// Pick the right monster-life table for a given minion. Mirrors PoB's
/// `Modules/CalcActiveSkill.lua:697-699` ladder:
///
/// - `lifeScaling = "AltLife1"` → `monster_life_table_2`
/// - `lifeScaling = "AltLife2"` → `monster_life_table_3`
/// - Spectre (any other `lifeScaling`) → base `monster_life_table`
/// - Standard summoned minion (no `lifeScaling`) → `monster_ally_life_table`
fn life_base_for(data: &MinionType, level: u32) -> u32 {
    match data.life_scaling.as_deref() {
        Some("AltLife1") => monster_life2_at_level(level),
        Some("AltLife2") => monster_life3_at_level(level),
        // PoB treats any unrecognised `lifeScaling` value as a spectre on the base
        // monsterLifeTable. Standard summoned minions (no `lifeScaling`) drop to the
        // ally life table.
        Some(_) => monster_life_at_level(level),
        None => monster_ally_life_at_level(level),
    }
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
    // Slice 8 of #20: hit-chance pass. Needs character.config.enemy_evasion which
    // isn't reachable from the pure write_minion_outputs entry point. Layered here
    // so write_minion_outputs stays env-only and unit-testable without spinning up
    // a Character. Slice 10 also threads the registry so the hit-chance pass can
    // skip the formula for spell minions (Flame Sentinel etc.).
    apply_minion_hit_chance(&state, character, registry, env, output);
    // Slice 12 of #20: mirror minion DPS into MainSkillDPS so the side-panel,
    // FullDPS aggregator, and all downstream consumers see a meaningful number
    // when the active skill is a minion-summoning gem. PoB does this in
    // CalcActiveSkill.lua by setting `output.MainSkillDPS = minion.output.TotalDPS`
    // (multiplied by `NumberOfMinions` to reflect the whole pack).
    propagate_minion_dps_to_main_skill(env, output);
    true
}

/// Replace the player's `MainSkillDPS` (and the small fan-out of DPS aliases that
/// mirror it for skills with no ailment add-ons) with the minion's pack DPS:
/// `MinionDPS × NumberOfMinions`. Called from [`apply_minion_outputs`] after
/// [`write_minion_outputs`] and [`apply_minion_hit_chance`] have settled
/// `MinionDPS`.
///
/// Mirrors PoB's `CalcActiveSkill.lua:830-844` summoner branch: when the active
/// skill is minion-flagged, the player-side `MainSkillDPS` is rewritten to the
/// minion's `TotalDPS` so the summary readouts make sense. We use
/// `NumberOfMinions` (already set by `perform_with_skills`'s minion branch) as
/// the pack size so a 4-zombie / 6-spectre build shows the full pack's DPS, not
/// a single creature.
///
/// Cheap enough to run unconditionally: when the active skill isn't a minion,
/// `apply_minion_outputs` short-circuits before reaching this helper. When it
/// IS a minion but `MinionDPS` happens to be zero (e.g. before the data
/// catalogue is loaded), we deliberately skip the overwrite so the caller's
/// existing `MainSkillDPS` value (the player's direct hit, dummy or otherwise)
/// stays put.
fn propagate_minion_dps_to_main_skill(env: &Env, output: &mut Output) {
    let _ = env; // future-proof: a richer perform pass may need env queries here
    let minion_dps = output.get("MinionDPS");
    if minion_dps <= 0.0 {
        return;
    }
    // perform.rs already wrote `NumberOfMinions` for any skill flagged
    // `baseFlags.minion = true`, defaulting to 1 when no MaxZombies / MaxSpectres
    // mods are stacked. Floor at 1 belt-and-braces in case `apply_minion_outputs`
    // is exercised against a state where the perform pass hasn't run.
    let pack_size = output.get("NumberOfMinions").max(1.0);
    let pack_dps = minion_dps * pack_size;

    // Preserve whatever the player's direct-hit DPS was so the breakdown chain
    // can still show "of which the player contributes X" if a future slice
    // wants to surface it.
    output.set("PlayerHitDPS", output.get("MainSkillDPS"));
    output.set("MainSkillDPS", pack_dps);

    // Cascade through the DPS aliases perform.rs writes immediately after
    // MainSkillDPS. With no ailment add-ons (the common summoner case) these
    // all mirror MainSkillDPS; if BleedDPS/PoisonDPS/IgniteDPS/ImpaleDPS exist
    // (rare on summon skills, but technically possible via support gems) we
    // preserve their additive contribution on top of the new pack DPS.
    let bleed = output.get("BleedDPS");
    let poison = output.get("PoisonDPS");
    let ignite = output.get("IgniteDPS");
    let impale = output.get("ImpaleDPS");
    let full_dps = pack_dps + bleed + poison + ignite + impale;
    output.set("FullDPS", full_dps);
    output.set("CombinedDPS", full_dps);
    output.set("TotalDPS", pack_dps);
    output.set("WithBleedDPS", pack_dps + bleed);
    output.set("WithPoisonDPS", pack_dps + poison);
    output.set("WithIgniteDPS", pack_dps + ignite);
    output.set("WithImpaleDPS", pack_dps + impale);
    output.set("CombinedAvg", full_dps);
}

/// Compute the minion's accuracy + hit-chance vs the character config's enemy
/// evasion, then fold the resulting hit-chance multiplier into `MinionDPS`.
/// Mirrors PoB's `Modules/CalcOffence.lua` minion accuracy / hit-chance branch.
///
/// Slice 10 of [#20](https://github.com/jonatanferm/PathOfBuildingMK2/issues/20)
/// reads the minion's `skill_list[0]` from the registry to decide whether the
/// minion's primary action is a spell (auto-hit, no accuracy roll needed) or an
/// attack (full PoB hit-chance formula). For minions whose first skill isn't in
/// the registry — common for spectres whose primary skill is a metadata-only
/// monster ability MK2 doesn't yet ship — we fall back to attack semantics.
pub fn apply_minion_hit_chance(
    state: &MinionState<'_>,
    character: &Character,
    registry: &SkillRegistry,
    env: &Env,
    output: &mut Output,
) {
    let cfg = QueryCfg::default();
    let intrinsic = parse_minion_intrinsic_mods(state.data);

    // Accuracy = `monster_accuracy[level] × minion.accuracy × (1 + inc/100)`. PoB
    // applies player-side `MinionAccuracy` INC (rare) plus the minion's intrinsic
    // `Accuracy` INC.
    let accuracy_base = f64::from(monster_accuracy_at_level(state.level)) * state.data.accuracy;
    let acc_inc_player = env
        .mod_db
        .sum(ModType::Inc, &cfg, &env.state, "MinionAccuracy");
    let acc_inc_intrinsic = intrinsic.sum(ModType::Inc, &cfg, &env.state, "Accuracy");
    let accuracy = accuracy_base * (1.0 + (acc_inc_player + acc_inc_intrinsic) / 100.0);
    output.set("MinionAccuracyBase", accuracy_base);
    output.set("MinionAccuracy", accuracy);

    // Spell-vs-attack discrimination. PoB checks the minion's primary skill flag;
    // we mirror that by inspecting `skill_list[0]` in the registry. Spells skip
    // the accuracy roll entirely (always 100% hit). Anything that's neither
    // identified as a spell nor missing falls into the attack branch.
    let is_attack = !minion_primary_skill_is_spell(state.data, registry);

    let enemy_evasion = f64::from(character.config.enemy_evasion);
    let hit_chance_pct = minion_hit_chance(accuracy, enemy_evasion, is_attack);
    output.set("MinionHitChance", hit_chance_pct);

    // Fold into MinionDPS. The previous DPS already factors in the crit factor
    // from slice 6; multiply through one more time to land on the
    // accuracy-adjusted number.
    let dps_pre_acc = output.get("MinionDPS");
    output.set("MinionDPSBeforeHitChance", dps_pre_acc);
    output.set("MinionDPS", dps_pre_acc * hit_chance_pct / 100.0);
}

/// Returns `true` when the minion's first listed skill resolves in the registry to
/// an entry whose `base_flags["spell"] == true`. Returns `false` for missing-from-
/// registry skills (so the caller falls back to attack semantics).
fn minion_primary_skill_is_spell(data: &MinionType, registry: &SkillRegistry) -> bool {
    let Some(primary) = data.skill_list.first() else {
        return false;
    };
    let Some(skill) = registry.get(primary) else {
        return false;
    };
    skill.base_flags.get("spell").copied().unwrap_or(false)
}

/// Take a minion's accuracy and the character config's `enemy_evasion`, run them through
/// PoB's `accuracy / (accuracy + (evasion/5)^0.9) × 125` formula, and clamp the result
/// to `[5, 100]`. Spells always hit (returns 100); pass `is_attack = false` for those.
fn minion_hit_chance(accuracy: f64, enemy_evasion: f64, is_attack: bool) -> f64 {
    if !is_attack {
        return 100.0;
    }
    let evasion = enemy_evasion.max(1.0);
    let raw = if accuracy <= 0.0 {
        5.0
    } else {
        accuracy / (accuracy + f64::powf(evasion / 5.0, 0.9)) * 125.0
    };
    raw.round().clamp(5.0, 100.0)
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
/// - Armour-vs-hit mitigation (the intrinsic `Armour` INC mod is surfaced but the
///   per-hit reduction formula is deferred).
/// - Per-minion `lifeScaling` (spectres etc.) — every minion still uses the ally
///   life table.
pub fn write_minion_outputs(state: &MinionState<'_>, env: &Env, output: &mut Output) {
    let cfg = QueryCfg::default();

    // Slice 7 of #20: minion's intrinsic mod_list (e.g. Raised Zombie's
    // `mod("Armour", "INC", 40)` and `mod("StunThreshold", "INC", 30)`). PoB lays
    // these onto a per-minion modDB and queries minion-side stat names without the
    // "Minion" prefix — we mirror that locally so the player-side ModDB stays
    // untouched.
    let intrinsic = parse_minion_intrinsic_mods(state.data);

    // Life — `MinionLife` INC/MORE on the player side stack with `Life` INC/MORE
    // on the intrinsic side; the player-side scaling lifts a value the minion
    // already carries, so the two layers compose multiplicatively.
    let life_inc_player = env.mod_db.sum(ModType::Inc, &cfg, &env.state, "MinionLife");
    let life_more_player = env.mod_db.more(&cfg, &env.state, "MinionLife");
    let life_inc_intrinsic = intrinsic.sum(ModType::Inc, &cfg, &env.state, "Life");
    let life_more_intrinsic = intrinsic.more(&cfg, &env.state, "Life");
    let life_scaled = (state.life_base as f64)
        * (1.0 + (life_inc_player + life_inc_intrinsic) / 100.0)
        * life_more_player
        * life_more_intrinsic;
    output.set("MinionLifeBase", state.life_base as f64);
    output.set("MinionLife", life_scaled.round());

    // Slice 13 of #20: minion energy shield. PoB stores `data.energy_shield`
    // as a multiplier the same way `data.life` is — the base value is
    // `life_table[level] × data.life × data.energy_shield`, then scaled by
    // the minion's `EnergyShield` INC/MORE chain (intrinsic + player-side
    // `MinionEnergyShield`). Most summoned minions don't have ES (the field
    // is `None`), but Skeleton Mages, Animated Guardian, and several
    // spectre bases do. The intrinsic `mod_list` is also where Mirror Arrow
    // / Blink Arrow add a flat `EnergyShield BASE 10` — captured via the
    // intrinsic Base sum below.
    //
    // The life-table choice mirrors `life_base_for`: a "spectre" minion
    // (any non-None `life_scaling`) draws ES from the same base monster
    // table its life uses, while standard summoned minions use the ally
    // table. This keeps allies and spectres on the same scaling ladder
    // without having to rebuild the lookup here.
    let es_multiplier = state.data.energy_shield.unwrap_or(0.0);
    let es_intrinsic_base = intrinsic.sum(ModType::Base, &cfg, &env.state, "EnergyShield");
    let es_player_base = env
        .mod_db
        .sum(ModType::Base, &cfg, &env.state, "MinionEnergyShield");
    let es_table_base = if es_multiplier > 0.0 {
        f64::from(life_base_for(state.data, state.level)) * state.data.life * es_multiplier
    } else {
        0.0
    };
    let es_total_base = es_table_base.floor() + es_intrinsic_base + es_player_base;
    if es_total_base > 0.0 {
        let es_inc_player = env
            .mod_db
            .sum(ModType::Inc, &cfg, &env.state, "MinionEnergyShield");
        let es_more_player = env.mod_db.more(&cfg, &env.state, "MinionEnergyShield");
        let es_inc_intrinsic = intrinsic.sum(ModType::Inc, &cfg, &env.state, "EnergyShield");
        let es_more_intrinsic = intrinsic.more(&cfg, &env.state, "EnergyShield");
        let es_scaled = es_total_base
            * (1.0 + (es_inc_player + es_inc_intrinsic) / 100.0)
            * es_more_player
            * es_more_intrinsic;
        output.set("MinionEnergyShieldBase", es_total_base);
        output.set("MinionEnergyShield", es_scaled.round());
    } else {
        // Surface a zero so consumers can branch on "minion has ES" without
        // having to call `try_get` everywhere.
        output.set("MinionEnergyShieldBase", 0.0);
        output.set("MinionEnergyShield", 0.0);
    }

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
    // the value that matches what they're computing. Player-side `MinionDamage`
    // INC/MORE composes with the minion's intrinsic `Damage` INC/MORE.
    let damage_base = f64::from(monster_ally_damage_at_level(state.level)) * state.data.damage;
    let dmg_inc_player = env
        .mod_db
        .sum(ModType::Inc, &cfg, &env.state, "MinionDamage");
    let dmg_more_player = env.mod_db.more(&cfg, &env.state, "MinionDamage");
    let dmg_inc_intrinsic = intrinsic.sum(ModType::Inc, &cfg, &env.state, "Damage");
    let dmg_more_intrinsic = intrinsic.more(&cfg, &env.state, "Damage");
    let damage_scaled = damage_base
        * (1.0 + (dmg_inc_player + dmg_inc_intrinsic) / 100.0)
        * dmg_more_player
        * dmg_more_intrinsic;
    let spread = state.data.damage_spread;
    let dmg_min = damage_scaled * (1.0 - spread);
    let dmg_max = damage_scaled * (1.0 + spread);
    let dmg_avg = damage_scaled;
    output.set("MinionDamageBase", damage_base);
    output.set("MinionAverageDamage", dmg_avg);
    output.set("MinionMinDamage", dmg_min);
    output.set("MinionMaxDamage", dmg_max);

    // Attack rate — `1 / attack_time × (1 + inc/100) × more`. Intrinsic `Speed`
    // mods compose with the player-side `MinionAttackSpeed` chain.
    let attack_time = state.data.attack_time.max(0.001);
    let speed_base = 1.0 / attack_time;
    let spd_inc_player = env
        .mod_db
        .sum(ModType::Inc, &cfg, &env.state, "MinionAttackSpeed");
    let spd_more_player = env.mod_db.more(&cfg, &env.state, "MinionAttackSpeed");
    let spd_inc_intrinsic = intrinsic.sum(ModType::Inc, &cfg, &env.state, "Speed");
    let spd_more_intrinsic = intrinsic.more(&cfg, &env.state, "Speed");
    let attacks_per_second = speed_base
        * (1.0 + (spd_inc_player + spd_inc_intrinsic) / 100.0)
        * spd_more_player
        * spd_more_intrinsic;
    output.set("MinionAttacksPerSecondBase", speed_base);
    output.set("MinionAttacksPerSecond", attacks_per_second);

    // Surface armour / stun-threshold from the minion's intrinsic mod_list. PoB
    // computes minion armour from a base × (1 + INC/100) × MORE chain anchored on
    // monster_armour_at_level, but slice 7 stops short of the per-hit mitigation
    // formula — we just report what the intrinsic mods contribute so the user can
    // see the chain.
    let armour_inc_intrinsic = intrinsic.sum(ModType::Inc, &cfg, &env.state, "Armour");
    let armour_inc_player = env
        .mod_db
        .sum(ModType::Inc, &cfg, &env.state, "MinionArmour");
    output.set("MinionArmourInc", armour_inc_player + armour_inc_intrinsic);
    let stun_threshold_inc_intrinsic =
        intrinsic.sum(ModType::Inc, &cfg, &env.state, "StunThreshold");
    output.set("MinionStunThresholdInc", stun_threshold_inc_intrinsic);
    // Life regen: combine the intrinsic LifeRegenPercent BASE (e.g. Chaos Golem's
    // +1% baked into modList) with the player-side MinionLifeRegen BASE (rare —
    // e.g. some belt rolls). Surface both the percent and the absolute life/sec
    // value so the breakdown panel can show what scales the regen.
    let life_regen_pct_intrinsic =
        intrinsic.sum(ModType::Base, &cfg, &env.state, "LifeRegenPercent");
    let life_regen_pct_player = env
        .mod_db
        .sum(ModType::Base, &cfg, &env.state, "MinionLifeRegen");
    let life_regen_pct = life_regen_pct_intrinsic + life_regen_pct_player;
    output.set("MinionLifeRegenPercent", life_regen_pct);
    // Absolute life/sec = MinionLife × percent / 100. Reads back the just-set
    // MinionLife so any scaling layered above (player INC/MORE) flows through.
    output.set(
        "MinionLifeRegen",
        output.get("MinionLife") * life_regen_pct / 100.0,
    );

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
                life_scaling: None,
                weapon_type1: None,
                weapon_type2: None,
                base_damage_ignores_attack_speed: false,
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
    fn life_base_for_picks_right_table_per_scaling() {
        let mut data = MinionType {
            name: "T".into(),
            monster_tags: vec![],
            life: 1.0,
            energy_shield: None,
            armour: None,
            fire_resist: 0,
            cold_resist: 0,
            lightning_resist: 0,
            chaos_resist: 0,
            damage: 1.0,
            damage_spread: 0.0,
            attack_time: 1.0,
            attack_range: 0.0,
            accuracy: 1.0,
            limit: None,
            skill_list: vec![],
            mod_list: vec![],
            life_scaling: None,
            weapon_type1: None,
            weapon_type2: None,
            base_damage_ignores_attack_speed: false,
        };

        // None → ally life table.
        assert_eq!(life_base_for(&data, 90), 4178);

        // "AltLife1" → variant 2.
        data.life_scaling = Some("AltLife1".into());
        assert_eq!(life_base_for(&data, 90), 24840);

        // "AltLife2" → variant 3.
        data.life_scaling = Some("AltLife2".into());
        assert_eq!(life_base_for(&data, 90), 26380);

        // Any other lifeScaling → base monsterLifeTable (spectre default).
        data.life_scaling = Some("WhateverElse".into());
        assert_eq!(life_base_for(&data, 90), 23250);
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

    /// Build a fake minion that carries the same intrinsic mod_list a Raised Zombie
    /// has in the real catalogue plus a Life INC mod that we can verify flows into
    /// the player-side compose chain.
    fn fake_minion_with_intrinsic_mods() -> MinionData {
        use serde_json::json;
        let mut minions = IndexMap::new();
        let mod_list = vec![
            // mod("Armour", "INC", 40) — like Raised Zombie's intrinsic.
            json!({"__kind": "mod", "name": "Armour", "type": "INC", "value": 40, "flags": 0, "keywordFlags": 0}),
            // mod("StunThreshold", "INC", 30) — same source.
            json!({"__kind": "mod", "name": "StunThreshold", "type": "INC", "value": 30, "flags": 0, "keywordFlags": 0}),
            // mod("Life", "INC", 25) — fictional but exercises the player+intrinsic compose.
            json!({"__kind": "mod", "name": "Life", "type": "INC", "value": 25, "flags": 0, "keywordFlags": 0}),
            // mod("LifeRegenPercent", "BASE", 1) — like Chaos Golem's intrinsic.
            json!({"__kind": "mod", "name": "LifeRegenPercent", "type": "BASE", "value": 1, "flags": 0, "keywordFlags": 0}),
        ];
        minions.insert(
            "TestMinion".into(),
            MinionType {
                name: "Test Minion".into(),
                monster_tags: vec![],
                life: 6.0,
                energy_shield: None,
                armour: None,
                fire_resist: 40,
                cold_resist: 40,
                lightning_resist: 40,
                chaos_resist: 0,
                damage: 1.5,
                damage_spread: 0.2,
                attack_time: 1.0,
                attack_range: 8.0,
                accuracy: 3.4,
                limit: None,
                skill_list: vec![],
                mod_list,
                life_scaling: None,
                weapon_type1: None,
                weapon_type2: None,
                base_damage_ignores_attack_speed: false,
            },
        );
        MinionData { minions }
    }

    #[test]
    fn parse_minion_intrinsic_mods_decodes_recordings() {
        let minions = fake_minion_with_intrinsic_mods();
        let data = minions.minions.get("TestMinion").unwrap();
        let db = parse_minion_intrinsic_mods(data);

        let cfg = QueryCfg::default();
        let state = crate::mod_db::EvalState::default();
        // 4 recordings → 4 mods (Armour, StunThreshold, Life, LifeRegenPercent).
        assert_eq!(db.iter_all().count(), 4);
        assert_eq!(db.sum(ModType::Inc, &cfg, &state, "Armour"), 40.0);
        assert_eq!(db.sum(ModType::Inc, &cfg, &state, "StunThreshold"), 30.0);
        assert_eq!(db.sum(ModType::Inc, &cfg, &state, "Life"), 25.0);
        assert_eq!(db.sum(ModType::Base, &cfg, &state, "LifeRegenPercent"), 1.0);
    }

    #[test]
    fn minion_hit_chance_formula_matches_pob() {
        // Spells always hit.
        assert_eq!(super::minion_hit_chance(0.0, 0.0, false), 100.0);
        assert_eq!(super::minion_hit_chance(1000.0, 5000.0, false), 100.0);

        // Zero accuracy floors at 5%.
        assert_eq!(super::minion_hit_chance(0.0, 1000.0, true), 5.0);

        // 1000 acc vs 0 evasion — capped at 100 not at the raw 1000/(1000+0)*125 = 125.
        assert_eq!(super::minion_hit_chance(1000.0, 0.0, true), 100.0);

        // Spot-check the canonical formula. accuracy = 1000, evasion = 1000:
        //   raw = 1000 / (1000 + (1000/5)^0.9) * 125
        //       = 1000 / (1000 + 130.49…) * 125
        //       ≈ 110.6 → clamps to 100.
        assert_eq!(super::minion_hit_chance(1000.0, 1000.0, true), 100.0);

        // accuracy = 500, evasion = 5000:
        //   raw = 500 / (500 + (5000/5)^0.9) * 125
        //       = 500 / (500 + (1000)^0.9) * 125
        //       = 500 / (500 + 501.2) * 125
        //       ≈ 62.4 → 62.
        let chance = super::minion_hit_chance(500.0, 5000.0, true);
        assert!((58.0..=66.0).contains(&chance), "got {chance}");
    }

    #[test]
    fn apply_minion_hit_chance_folds_into_dps() {
        let minions = fake_minion();
        let data = minions.minions.get("SummonedFlameGolem").unwrap();
        let state = MinionState {
            id: "SummonedFlameGolem".into(),
            data,
            level: 90,
            life_base: 25068,
        };
        let mut character = Character::new(crate::ClassRef::marauder(), 90);
        // High enemy evasion so the formula meaningfully bites the chance below 100.
        // monster_accuracy[90] × minion.accuracy = 675 × 3.4 ≈ 2295. Pair with a
        // 25k evasion target so the formula lands well below the 100% cap.
        character.config.enemy_evasion = 25000;

        let env = Env::default();

        let mut output = Output::default();
        write_minion_outputs(&state, &env, &mut output);
        let dps_before = output.get("MinionDPS");
        let registry = SkillRegistry::default();
        apply_minion_hit_chance(&state, &character, &registry, &env, &mut output);

        let hit_chance = output.get("MinionHitChance");
        // Pre-accuracy DPS preserved as a separate output so the breakdown chain
        // stays inspectable.
        assert!((output.get("MinionDPSBeforeHitChance") - dps_before).abs() < 0.001);
        // Post-accuracy DPS = pre × chance/100.
        let dps_after = output.get("MinionDPS");
        assert!(
            (dps_after - dps_before * hit_chance / 100.0).abs() < 0.001,
            "dps_after={dps_after}, dps_before={dps_before}, chance={hit_chance}"
        );
        // Hit chance should be < 100 against a 5000-evasion target.
        assert!(
            hit_chance < 100.0,
            "MinionHitChance should be < 100 vs 5000 evasion, got {hit_chance}"
        );
    }

    #[test]
    fn minion_primary_skill_is_spell_handles_missing_and_attack_skills() {
        let mut data = MinionType {
            name: "X".into(),
            monster_tags: vec![],
            life: 1.0,
            energy_shield: None,
            armour: None,
            fire_resist: 0,
            cold_resist: 0,
            lightning_resist: 0,
            chaos_resist: 0,
            damage: 1.0,
            damage_spread: 0.0,
            attack_time: 1.0,
            attack_range: 0.0,
            accuracy: 1.0,
            limit: None,
            skill_list: vec![],
            mod_list: vec![],
            life_scaling: None,
            weapon_type1: None,
            weapon_type2: None,
            base_damage_ignores_attack_speed: false,
        };
        let registry = SkillRegistry::default();

        // No skill_list → not a spell, so attack semantics apply.
        assert!(!minion_primary_skill_is_spell(&data, &registry));

        // skill_list with an unknown id → still falls back to attack semantics.
        data.skill_list = vec!["NotInRegistry".into()];
        assert!(!minion_primary_skill_is_spell(&data, &registry));
    }

    #[test]
    fn apply_minion_hit_chance_returns_100_for_spell_minions() {
        // Use the fake minion registry trick: a real spell-detection test would
        // need a SkillRegistry populated with a minion-spell entry. Instead we
        // verify the registry-empty path lands at the attack branch (slice 8
        // behaviour) and that the formula's spells-bypass-accuracy clause is
        // exercised by the standalone helper test (`minion_hit_chance_formula_matches_pob`).
        let minions = fake_minion();
        let data = minions.minions.get("SummonedFlameGolem").unwrap();
        let state = MinionState {
            id: "SummonedFlameGolem".into(),
            data,
            level: 90,
            life_base: 25068,
        };
        let mut character = Character::new(crate::ClassRef::marauder(), 90);
        character.config.enemy_evasion = 25000;

        let env = Env::default();
        let registry = SkillRegistry::default();
        let mut output = Output::default();
        write_minion_outputs(&state, &env, &mut output);
        apply_minion_hit_chance(&state, &character, &registry, &env, &mut output);

        // Without a registry-resolvable skill, falls through to attack semantics
        // and the formula returns < 100% vs 25k evasion.
        let chance = output.get("MinionHitChance");
        assert!(
            chance < 100.0,
            "attack-fallback chance should be < 100, got {chance}"
        );
    }

    #[test]
    fn write_minion_outputs_composes_intrinsic_with_player_mods() {
        use crate::Mod;
        let minions = fake_minion_with_intrinsic_mods();
        let data = minions.minions.get("TestMinion").unwrap();
        let state = MinionState {
            id: "TestMinion".into(),
            data,
            level: 20,
            life_base: 1000,
        };

        // Player-side: 50% MinionLife INC, 0% MORE.
        let mut env = Env::default();
        env.mod_db.add(Mod::inc("MinionLife", 50.0));
        let mut output = Output::default();
        write_minion_outputs(&state, &env, &mut output);

        // Combined: 1000 × (1 + 0.50 + 0.25) = 1750. Both INCs sum additively.
        assert_eq!(output.get("MinionLife"), 1750.0);
        // Intrinsic StunThreshold and Armour bubble through.
        assert_eq!(output.get("MinionStunThresholdInc"), 30.0);
        assert_eq!(output.get("MinionArmourInc"), 40.0);
        // LifeRegenPercent BASE 1 lands on a dedicated key.
        assert_eq!(output.get("MinionLifeRegenPercent"), 1.0);
        // Life regen rate = MinionLife (1750) × 1% / 100 = 17.5 life/sec.
        assert_eq!(output.get("MinionLifeRegen"), 17.5);
    }

    #[test]
    fn write_minion_outputs_combines_intrinsic_and_player_life_regen() {
        use crate::Mod;
        let minions = fake_minion_with_intrinsic_mods();
        let data = minions.minions.get("TestMinion").unwrap();
        let state = MinionState {
            id: "TestMinion".into(),
            data,
            level: 20,
            life_base: 1000,
        };

        // Player-side MinionLifeRegen 2 BASE adds to the intrinsic LifeRegenPercent
        // 1 BASE → total 3%. The fake minion's intrinsic Life INC 25 scales
        // MinionLife to 1000 × 1.25 = 1250, so regen lands at 1250 × 3 / 100 = 37.5
        // life/sec.
        let mut env = Env::default();
        env.mod_db.add(Mod::base("MinionLifeRegen", 2.0));
        let mut output = Output::default();
        write_minion_outputs(&state, &env, &mut output);
        assert_eq!(output.get("MinionLifeRegenPercent"), 3.0);
        assert_eq!(output.get("MinionLife"), 1250.0);
        assert_eq!(output.get("MinionLifeRegen"), 37.5);
    }

    /// Slice 12 of #20: when MinionDPS is set and `NumberOfMinions` is the
    /// perform-pass default of 1, MainSkillDPS should pick up MinionDPS verbatim
    /// and the FullDPS / TotalDPS / mirror keys should follow.
    #[test]
    fn propagate_minion_dps_overwrites_main_skill_dps() {
        let env = Env::default();
        let mut output = Output::default();
        // Simulate the post-perform.rs state for a summon-skill build:
        // - MinionDPS already computed by write_minion_outputs + hit-chance
        // - NumberOfMinions = 1 (perform.rs default for any minion-flagged skill
        //   without max-pack mods)
        // - MainSkillDPS = a tiny placeholder (the gem's "you can't actually cast
        //   this on yourself" damage)
        output.set("MinionDPS", 5000.0);
        output.set("NumberOfMinions", 1.0);
        output.set("MainSkillDPS", 1.0);
        super::propagate_minion_dps_to_main_skill(&env, &mut output);
        // MainSkillDPS now reflects the minion's DPS.
        assert_eq!(output.get("MainSkillDPS"), 5000.0);
        // PlayerHitDPS preserves the pre-overwrite value for breakdown chains.
        assert_eq!(output.get("PlayerHitDPS"), 1.0);
        // FullDPS / TotalDPS / aliases mirror MainSkillDPS (no ailments).
        assert_eq!(output.get("FullDPS"), 5000.0);
        assert_eq!(output.get("TotalDPS"), 5000.0);
        assert_eq!(output.get("CombinedDPS"), 5000.0);
        assert_eq!(output.get("WithBleedDPS"), 5000.0);
        assert_eq!(output.get("WithPoisonDPS"), 5000.0);
        assert_eq!(output.get("WithIgniteDPS"), 5000.0);
        assert_eq!(output.get("WithImpaleDPS"), 5000.0);
    }

    /// Slice 12 of #20: a 6-spectre build (NumberOfMinions = 6) should multiply
    /// MainSkillDPS by the pack size — the headline number reflects the whole
    /// pack, matching PoB's summary readout.
    #[test]
    fn propagate_minion_dps_multiplies_by_number_of_minions() {
        let env = Env::default();
        let mut output = Output::default();
        output.set("MinionDPS", 1000.0);
        output.set("NumberOfMinions", 6.0);
        super::propagate_minion_dps_to_main_skill(&env, &mut output);
        assert_eq!(output.get("MainSkillDPS"), 6000.0);
        assert_eq!(output.get("FullDPS"), 6000.0);
    }

    /// Slice 12 of #20: if MinionDPS is zero (data catalogue not loaded, or the
    /// minion's primary skill metadata was absent so write_minion_outputs found
    /// nothing to compute), leave MainSkillDPS alone. The whole point is to
    /// surface a meaningful number — a zero overwrite would mask the existing
    /// player-side value without adding information.
    #[test]
    fn propagate_minion_dps_is_a_noop_when_minion_dps_zero() {
        let env = Env::default();
        let mut output = Output::default();
        output.set("MainSkillDPS", 100.0);
        output.set("MinionDPS", 0.0);
        output.set("NumberOfMinions", 1.0);
        super::propagate_minion_dps_to_main_skill(&env, &mut output);
        assert_eq!(output.get("MainSkillDPS"), 100.0);
        assert!(output.try_get("PlayerHitDPS").is_none());
        assert!(output.try_get("FullDPS").is_none());
    }

    /// Slice 12 of #20: when MinionDPS lands but NumberOfMinions hasn't been
    /// set (e.g. the test exercises apply_minion_outputs without a real perform
    /// pass), default the pack size to 1 rather than zero. A single minion's
    /// DPS is still strictly more useful than overwriting MainSkillDPS with 0.
    #[test]
    fn propagate_minion_dps_defaults_pack_size_to_one() {
        let env = Env::default();
        let mut output = Output::default();
        output.set("MinionDPS", 2500.0);
        // NumberOfMinions deliberately unset: the perform-pass minion branch
        // didn't run, so the key is missing and `output.get` returns 0.
        super::propagate_minion_dps_to_main_skill(&env, &mut output);
        assert_eq!(output.get("MainSkillDPS"), 2500.0);
    }

    /// Slice 13 of #20: minion energy shield is `None` for most summons; the
    /// output keys must still be set (to zero) so consumers can branch on
    /// "has ES" without juggling Option<f64> through every read path.
    #[test]
    fn write_minion_outputs_es_zero_when_data_missing() {
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
        assert_eq!(output.get("MinionEnergyShield"), 0.0);
        assert_eq!(output.get("MinionEnergyShieldBase"), 0.0);
    }

    /// Slice 13 of #20: when `data.energy_shield` is set (Skeleton Mage,
    /// Animated Guardian, several spectres) the base ES is
    /// `life_table[level] × data.life × data.energy_shield`. Verify the
    /// formula against pinned numbers from the ally life table at level 20:
    /// ally_life[20] = 38.94, life = 6, es_mult = 0.4 → base ≈ 38.94 × 6 ×
    /// 0.4 = 93.456, floor → 93. With no mods, MinionEnergyShield equals
    /// the floored base.
    #[test]
    fn write_minion_outputs_es_uses_life_table_formula() {
        let mut minions = fake_minion();
        let data = minions
            .minions
            .get_mut("SummonedFlameGolem")
            .expect("flame golem present");
        data.energy_shield = Some(0.4);
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

        // Pinned: monster_ally_life[level=20] × data.life (=6.0) × es (=0.4),
        // floored to match PoB's `m_floor(baseES)`.
        let expected_base =
            (f64::from(pob_data::monster_tables::monster_ally_life_at_level(20)) * 6.0 * 0.4)
                .floor();
        assert_eq!(output.get("MinionEnergyShieldBase"), expected_base);
        // No mods → scaled equals base.
        assert_eq!(output.get("MinionEnergyShield"), expected_base.round());
        // Sanity: actual life table value is non-trivial, so the test isn't
        // a tautology against zeroes.
        assert!(expected_base > 50.0 && expected_base < 200.0);
    }

    /// Slice 13 of #20: player-side `MinionEnergyShield` INC / MORE / BASE
    /// mods compose with the minion's intrinsic `EnergyShield` chain. Verify
    /// each multiplier slot independently by stacking mods that produce a
    /// known scaling.
    #[test]
    fn write_minion_outputs_es_scales_by_player_mods() {
        use crate::Mod;
        let mut minions = fake_minion();
        let data = minions
            .minions
            .get_mut("SummonedFlameGolem")
            .expect("flame golem present");
        data.energy_shield = Some(0.5);
        let data = minions.minions.get("SummonedFlameGolem").unwrap();
        let state = MinionState {
            id: "SummonedFlameGolem".into(),
            data,
            level: 1,
            life_base: 1000,
        };

        // Pin a clean base by clamping level: ally_life[1] × 6 × 0.5.
        let base_unscaled =
            f64::from(pob_data::monster_tables::monster_ally_life_at_level(1)) * 6.0 * 0.5;
        let base_floor = base_unscaled.floor();

        // 50% INC + 20% MORE → base × 1.5 × 1.2.
        let mut env = Env::default();
        env.mod_db.add(Mod::inc("MinionEnergyShield", 50.0));
        env.mod_db.add(Mod::more("MinionEnergyShield", 20.0));
        let mut output = Output::default();
        write_minion_outputs(&state, &env, &mut output);
        let expected = (base_floor * 1.5 * 1.2).round();
        assert_eq!(output.get("MinionEnergyShield"), expected);

        // BASE adders apply before INC/MORE, so a +25 BASE on a base of 12
        // becomes 37 before scaling. Use a fresh env to isolate from above.
        let mut env2 = Env::default();
        env2.mod_db.add(Mod::base("MinionEnergyShield", 25.0));
        env2.mod_db.add(Mod::inc("MinionEnergyShield", 100.0));
        let mut out2 = Output::default();
        write_minion_outputs(&state, &env2, &mut out2);
        let expected2 = ((base_floor + 25.0) * 2.0).round();
        assert_eq!(out2.get("MinionEnergyShield"), expected2);
        assert_eq!(out2.get("MinionEnergyShieldBase"), base_floor + 25.0);
    }

    /// Slice 13 of #20: Mirror Arrow / Blink Arrow add a flat `EnergyShield
    /// BASE 10` via their intrinsic `mod_list`, even when the minion data
    /// itself doesn't carry an `energyShield` multiplier. Verify the
    /// intrinsic Base feeds the same compose chain as the player-side
    /// Base.
    #[test]
    fn write_minion_outputs_es_picks_up_intrinsic_base() {
        use serde_json::json;
        let mut minions: IndexMap<String, MinionType> = IndexMap::new();
        minions.insert(
            "MirrorArrow".into(),
            MinionType {
                name: "Mirror Arrow Clone".into(),
                monster_tags: vec![],
                life: 1.0,
                energy_shield: None, // base ES comes purely from the intrinsic mod
                armour: None,
                fire_resist: 0,
                cold_resist: 0,
                lightning_resist: 0,
                chaos_resist: 0,
                damage: 1.0,
                damage_spread: 0.0,
                attack_time: 1.0,
                attack_range: 1.0,
                accuracy: 1.0,
                limit: None,
                skill_list: vec![],
                mod_list: vec![
                    json!({"__kind": "mod", "name": "EnergyShield", "type": "BASE", "value": 10, "flags": 0, "keywordFlags": 0}),
                ],
                life_scaling: None,
                weapon_type1: None,
                weapon_type2: None,
                base_damage_ignores_attack_speed: false,
            },
        );
        let data = minions.get("MirrorArrow").unwrap();
        let state = MinionState {
            id: "MirrorArrow".into(),
            data,
            level: 20,
            life_base: 100,
        };

        let env = Env::default();
        let mut output = Output::default();
        write_minion_outputs(&state, &env, &mut output);
        // Base = 0 (no es multiplier) + 10 intrinsic + 0 player = 10.
        assert_eq!(output.get("MinionEnergyShieldBase"), 10.0);
        assert_eq!(output.get("MinionEnergyShield"), 10.0);
    }
}
