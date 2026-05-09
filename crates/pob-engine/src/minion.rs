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
        monster_ally_life_at_level, monster_life2_at_level, monster_life3_at_level,
        monster_life_at_level,
    },
    MinionData, MinionType,
};

use crate::{Character, Output, SkillRegistry};

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
pub fn apply_minion_outputs(
    character: &Character,
    registry: &SkillRegistry,
    minions: &MinionData,
    output: &mut Output,
) -> bool {
    let Some(state) = select_minion_type(character, registry, minions) else {
        return false;
    };
    write_minion_outputs(&state, output);
    true
}

/// Emit the minion's basic stats into the player's output dictionary so the Calcs tab
/// can surface them. Slice 3 only writes `MinionLifeBase` and a placeholder
/// `MinionLife` (= base, no mods). Slice 4 will run a real perform pass.
pub fn write_minion_outputs(state: &MinionState<'_>, output: &mut Output) {
    output.set("MinionLifeBase", state.life_base as f64);
    output.set("MinionLife", state.life_base as f64);
    output.set("MinionFireResist", state.data.fire_resist as f64);
    output.set("MinionColdResist", state.data.cold_resist as f64);
    output.set("MinionLightningResist", state.data.lightning_resist as f64);
    output.set("MinionChaosResist", state.data.chaos_resist as f64);
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
        let mut output = Output::default();
        write_minion_outputs(&state, &mut output);
        assert_eq!(output.get("MinionLife"), 1000.0);
        assert_eq!(output.get("MinionLifeBase"), 1000.0);
        assert_eq!(output.get("MinionFireResist"), 75.0);
        assert_eq!(output.get("MinionChaosResist"), 0.0);
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
